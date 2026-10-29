use std::{mem, sync::Arc};

use resonate_codec::Packing;
use resonate_core::{AppliedGain, AudioBuffer, Frames, StreamSpec, Volume, abx::Arm, eq::Profile};
use resonate_dsp::{Chain, Recall};

use super::{
    CHAIN_BLOCK, Output, Track, carried_samples,
    splicing::{SPLICE_BLENDED_OVER, SPLICED_AHEAD_AT_LEAST, Splicing, recall_for},
};
use crate::{Audition, EngineConfig, Error, OutputPlan, Result, audition::plans_of};

pub(super) struct Other {
    chain: Chain,
    plan: OutputPlan,
    gain: f64,
    widened: Vec<f64>,
    carrier: Vec<f64>,
    rendered: Recall,
}

impl Other {
    fn run(&mut self, frames: usize, channels_in: usize, channels_out: usize) {
        let mut done = 0;
        while done < frames {
            let Some(input) = self.widened.get(done * channels_in..frames * channels_in) else {
                return;
            };
            let count = self.chain.process(input, &mut self.carrier);
            if let Some(made) = self.carrier.get(..count.frames_out * channels_out) {
                self.rendered.enter(made);
            }
            if count.frames_in == 0 {
                return;
            }
            done += count.frames_in;
        }
    }

    fn flush(&mut self, channels_out: usize) {
        let mut tail = vec![0.0; self.chain.max_flush_frames() * channels_out];
        if let Ok(frames) = self.chain.flush(&mut tail)
            && let Some(made) = tail.get(..frames * channels_out)
        {
            self.rendered.enter(made);
        }
    }
}

pub(super) struct Arms {
    heard: Arm,
    heard_gain: f64,
    heard_rendered: Recall,
    other: Other,
}

impl Arms {
    pub(super) const fn heard(&self) -> Arm {
        self.heard
    }

    pub(super) fn set_gain_of_the_other(&mut self, volume: Volume, replay_gain: AppliedGain) {
        self.other.chain.set_gain(volume, replay_gain);
    }

    pub(super) fn set_equalisation_of_the_other(&mut self, profile: &Arc<Profile>) {
        self.other.chain.set_equalisation(profile);
    }
}

struct Blend {
    from: u64,
    at: u64,
    over: u64,
}

impl Blend {
    fn weight_of_the_new(&self, heard: u64) -> f64 {
        (heard - self.from + 1) as f64 / (self.over + 1) as f64
    }
}

impl Output {
    pub(super) fn take_up_the_audition(
        &mut self,
        track: &Track,
        config: &EngineConfig,
        audition: &Audition,
    ) -> Result<()> {
        if !matches!(self.plan.packing, Packing::Samples) {
            return Ok(());
        }
        let source = track.source();
        let other_arm = audition.heard.other();
        let other_config = audition.contest.applied(other_arm, config);
        let (a, b) = plans_of(
            track.decoded(),
            &self.bound,
            self.plan.stream,
            config,
            track.replay_gain,
            self.attenuator,
            audition.contest,
        );
        let plan = match other_arm {
            Arm::A => a,
            Arm::B => b,
        };
        let alike = plan.stream == self.plan.stream
            && plan.packing == self.plan.packing
            && plan.remix == self.plan.remix;
        if !alike || audition.contest.renders_alike(&plan, &self.plan) {
            return Ok(());
        }
        let chain = plan
            .build_chain(source, &other_config, CHAIN_BLOCK)
            .map_err(|error| Error::Convert {
                track: track.id,
                source_spec: source,
                sink_spec: plan.stream,
                source: error,
            })?;

        let ring_frames = Frames(self.capacity as u64);
        let carrier = vec![0.0; carried_samples(&chain, plan.stream)];
        let widened = vec![0.0; self.widened.len()];
        self.arms = Some(Arms {
            heard: audition.heard,
            heard_gain: audition.lowered.gain_of(audition.heard),
            heard_rendered: recall_for(self.plan.stream, ring_frames),
            other: Other {
                gain: audition.lowered.gain_of(other_arm),
                rendered: recall_for(plan.stream, ring_frames),
                chain,
                plan,
                widened,
                carrier,
            },
        });
        Ok(())
    }

    pub(super) fn level_the_arms(&mut self, widened: usize) {
        let Some(arms) = self.arms.as_mut() else {
            return;
        };
        let (Some(heard), Some(other)) = (
            self.widened.get_mut(..widened),
            arms.other.widened.get_mut(..widened),
        ) else {
            return;
        };
        for (heard, other) in heard.iter_mut().zip(other.iter_mut()) {
            *other = *heard * arms.other.gain;
            *heard *= arms.heard_gain;
        }
    }

    pub(super) fn render_the_other_arm(&mut self, made: usize, taken: usize, source: StreamSpec) {
        let channels_out = self.plan.stream.channel_count().get() as usize;
        let channels_in = source.channel_count().get() as usize;
        let Some(arms) = self.arms.as_mut() else {
            return;
        };
        if let Some(rendered) = self.carrier.get(..made * channels_out) {
            arms.heard_rendered.enter(rendered);
        }
        arms.other.run(taken, channels_in, channels_out);
    }

    pub(super) fn flush_the_other_arm(&mut self, made: usize) {
        let channels_out = self.plan.stream.channel_count().get() as usize;
        let Some(arms) = self.arms.as_mut() else {
            return;
        };
        if let Some(rendered) = self.carrier.get(..made * channels_out) {
            arms.heard_rendered.enter(rendered);
        }
        arms.other.flush(channels_out);
    }

    pub(super) fn start_the_arms_over(&mut self) {
        let Some(arms) = self.arms.as_mut() else {
            return;
        };
        arms.other.chain.reset();
        arms.other.rendered.forget();
        arms.heard_rendered.forget();
    }

    pub(super) fn hear_the_other_arm(&mut self) -> Splicing {
        let renders_in_place = matches!(self.plan.packing, Packing::Samples)
            && self.draining.is_none()
            && !self.ended
            && !self.ends_at_a_change
            && !self.producer.is_discarding()
            && self.arms.is_some();
        if !renders_in_place {
            return Splicing::Unspliceable;
        }
        if self.producer.is_splicing() {
            self.switch_owed = true;
            return Splicing::Owed;
        }

        let rate = self.plan.stream.rate;
        let over = Frames::from_duration(SPLICE_BLENDED_OVER, rate);
        let ahead = Frames::from_duration(SPLICED_AHEAD_AT_LEAST, rate);
        let Some(at) = self.producer.splice_point(ahead, over) else {
            return Splicing::Unspliceable;
        };
        let from = self.producer.heard_as(at);
        let Some(arms) = self.arms.as_ref() else {
            return Splicing::Unspliceable;
        };
        let rendered = &arms.other.rendered;
        let entered = rendered.entered();
        if from < rendered.oldest()
            || from >= entered
            || entered - from > self.producer.room_to_splice() as u64
        {
            return Splicing::Unspliceable;
        }

        let blend = Blend {
            from,
            at,
            over: over.get(),
        };
        let stale_to = self.producer.written();
        self.lay_the_other_arm(&blend);
        self.producer.splice(at, stale_to);
        self.change_arms();
        self.note_the_rendered_level();
        Splicing::Spliced(self.status)
    }

    fn lay_the_other_arm(&mut self, blend: &Blend) {
        let channels = self.plan.stream.channel_count().get() as usize;
        let Some(arms) = self.arms.as_ref() else {
            return;
        };
        let rendered = &arms.other.rendered;
        let entered = rendered.entered();
        let mut taken = vec![0.0; CHAIN_BLOCK * channels];
        let mut before = vec![0.0; CHAIN_BLOCK * channels];
        let mut staged = AudioBuffer::silence(self.plan.stream, CHAIN_BLOCK);
        let mut next = blend.from;
        while next < entered {
            let frames = rendered.recalled(next, &mut taken);
            if frames == 0 {
                return;
            }
            let blended_to = (next + frames as u64).min(blend.from + blend.over);
            if next < blended_to {
                let blended = (blended_to - next) as usize;
                let old = before.get_mut(..blended * channels).unwrap_or_default();
                let widened = self
                    .producer
                    .widen_what_was_written(blend.at + (next - blend.from), old);
                for (offset, (new, old)) in taken
                    .get_mut(..widened * channels)
                    .unwrap_or_default()
                    .chunks_exact_mut(channels)
                    .zip(old.chunks_exact(channels))
                    .enumerate()
                {
                    let weight = blend.weight_of_the_new(next + offset as u64);
                    for (new, old) in new.iter_mut().zip(old) {
                        *new = *old + (*new - *old) * weight;
                    }
                }
            }
            staged.set_frames(frames);
            if let Some(laid) = taken.get(..frames * channels) {
                staged.data_mut().write_f64(laid);
            }
            let written = self.producer.write_spliced(&staged);
            if let Some(tapping) = self.tapping.as_mut() {
                tapping.rendered_at(self.producer.rendering());
                tapping.record(&staged, 0, written);
            }
            if written < frames {
                return;
            }
            next += frames as u64;
        }
    }

    fn change_arms(&mut self) {
        let Some(arms) = self.arms.as_mut() else {
            return;
        };
        let fronts_alike = self.plan.fronts_alike(&arms.other.plan);
        mem::swap(&mut self.chain, &mut arms.other.chain);
        mem::swap(&mut self.plan, &mut arms.other.plan);
        mem::swap(&mut arms.heard_gain, &mut arms.other.gain);
        mem::swap(&mut arms.heard_rendered, &mut arms.other.rendered);
        mem::swap(&mut self.carrier, &mut arms.other.carrier);
        arms.heard = arms.heard.other();
        if !fronts_alike {
            self.recall_spoiled = true;
        }
        self.staged = AudioBuffer::silence(self.plan.stream, self.chain.max_output_frames());
    }

    pub(super) fn let_go_of_the_other_arm(&mut self) {
        self.arms = None;
    }
}
