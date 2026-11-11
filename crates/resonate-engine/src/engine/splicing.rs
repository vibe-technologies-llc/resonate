use std::{num::NonZeroUsize, ops::Range, time::Duration};

use resonate_codec::Packing;
use resonate_core::{AudioBuffer, Frames, Gain, StreamSpec};
use resonate_dsp::{Chain, Recall};

use super::{CHAIN_BLOCK, LARGEST_RING, Output, Track};
use crate::{EngineConfig, Error, OutputPlan, OutputStatus, Result, ring::RingProducer};

pub const SPLICED_AHEAD_AT_LEAST: Duration = Duration::from_millis(10);
pub const SPLICE_BLENDED_OVER: Duration = Duration::from_millis(20);
pub const ROLLED_IN_BEFORE_A_SPLICE: Duration = Duration::from_millis(100);
const WRITTEN_BEFORE_A_SPLICE_IS_PUBLISHED: Duration = Duration::from_millis(50);
const RECALL_SAMPLE_BYTES: u64 = size_of::<f64>() as u64;

pub enum Splicing {
    Spliced(OutputStatus),
    Owed,
    Unspliceable,
}

pub fn recall_for(stream: StreamSpec, ring_frames: Frames) -> Recall {
    let channels =
        NonZeroUsize::new(stream.channel_count().get() as usize).unwrap_or(NonZeroUsize::MIN);
    let rolled_in = Frames::from_duration(ROLLED_IN_BEFORE_A_SPLICE, stream.rate).get();
    let wanted = ring_frames
        .get()
        .saturating_mul(2)
        .saturating_add(rolled_in)
        .saturating_add(CHAIN_BLOCK as u64);
    let most = LARGEST_RING / RECALL_SAMPLE_BYTES / channels.get() as u64;
    Recall::new(channels, wanted.min(most) as usize)
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

fn splice_from_here(
    producer: &RingProducer,
    recall: &Recall,
    ahead: Frames,
    over: Frames,
    no_sooner_than: u64,
) -> Option<Blend> {
    let at = producer.splice_point(ahead, over)?.max(no_sooner_than);
    let from = producer.heard_as(at);
    let entered = recall.entered();
    let rendered_again = entered.saturating_sub(from);
    (from >= recall.oldest()
        && from <= entered
        && rendered_again <= producer.room_to_splice() as u64)
        .then_some(Blend {
            from,
            at,
            over: over.get(),
        })
}

fn splice_after_rendering<Made>(
    producer: &RingProducer,
    recall: &Recall,
    ahead: Frames,
    over: Frames,
    render: impl FnOnce(&Blend) -> Made,
) -> Option<(Blend, Made)> {
    let first = splice_from_here(producer, recall, ahead, over, 0)?;
    let rendered = render(&first);
    let blend = splice_from_here(producer, recall, ahead, over, first.at)?;
    Some((blend, rendered))
}

impl Output {
    pub(super) fn recall_written(&mut self, buffer: &AudioBuffer, from: usize, frames: usize) {
        let channels = self.plan.stream.channel_count().get() as usize;
        let block = (self.widened.len() / channels).max(1);
        let mut done = 0;
        while done < frames {
            let run = (frames - done).min(block);
            let Some(room) = self.widened.get_mut(..run * channels) else {
                return;
            };
            let widened = buffer.data().widen_into((from + done) * channels, room);
            if let Some(entered) = self.widened.get(..widened) {
                self.recall.enter(entered);
            }
            done += run;
        }
    }

    pub(super) fn splice_the_back(
        &mut self,
        track: &mut Track,
        wanted: OutputPlan,
        config: &EngineConfig,
    ) -> Result<Splicing> {
        let renders_in_place = matches!(self.plan.packing, Packing::Samples)
            && self.draining.is_none()
            && !self.ended
            && !self.ends_at_a_change
            && !self.producer.is_discarding()
            && !self.recall_spoiled
            && self.arms.is_none()
            && self.plan.fronts_alike(&wanted);
        if !renders_in_place {
            return Ok(Splicing::Unspliceable);
        }
        if self.producer.is_splicing() {
            self.splice_owed = true;
            return Ok(Splicing::Owed);
        }

        let rate = self.plan.stream.rate;
        let over = Frames::from_duration(SPLICE_BLENDED_OVER, rate);
        let ahead = Frames::from_duration(SPLICED_AHEAD_AT_LEAST, rate);
        let edge = self.chain.back();
        let source = track.source();
        let mut back = wanted
            .build_the_back(edge, config)
            .map_err(|error| Error::Convert {
                track: track.id,
                source_spec: source,
                sink_spec: wanted.stream,
                source: error,
            })?;

        let rolled_in = Frames::from_duration(ROLLED_IN_BEFORE_A_SPLICE, rate).get();
        let Some((blend, rendered)) =
            splice_after_rendering(&self.producer, &self.recall, ahead, over, |first| {
                let rolled = first
                    .from
                    .saturating_sub(rolled_in)
                    .max(self.recall.oldest());
                self.rendered_again(&mut back, rolled, first.from)
            })
        else {
            return Ok(Splicing::Unspliceable);
        };
        let stale_to = self.producer.written();
        let published_after = blend.from.saturating_add(
            Frames::from_duration(WRITTEN_BEFORE_A_SPLICE_IS_PUBLISHED, rate)
                .get()
                .max(blend.over),
        );
        self.write_blended(&back, &rendered, &blend, blend.from..published_after);
        self.producer.splice(blend.at, stale_to);
        self.write_blended(&back, &rendered, &blend, published_after..u64::MAX);

        let front = self.chain.take_what_comes_before_the_back();
        let chain = Chain::rejoined(front, back, CHAIN_BLOCK);
        let delivery = wanted.delivery();
        track.decoding.deliver(delivery);
        track.decoded.retype(delivery.format);
        self.widened
            .resize(CHAIN_BLOCK * source.channel_count().get() as usize, 0.0);
        let status = self.take_chain(chain, wanted);
        self.note_the_rendered_level();
        Ok(Splicing::Spliced(status))
    }

    fn rendered_again(&self, back: &mut Chain, rolled: u64, kept_from: u64) -> Rendered {
        let channels = self.plan.stream.channel_count().get() as usize;
        let edge = back.back();
        let block = edge.frames_in.max(1);
        let mut entering = vec![0.0; block * channels];
        let mut made_now = vec![0.0; back.max_output_frames().max(block) * channels];
        let wanted = self.recall.entered().saturating_sub(kept_from) as usize;
        let mut rendered = Rendered {
            from: kept_from,
            samples: Vec::with_capacity(wanted * channels),
        };

        let mut next = rolled;
        let mut made_from = rolled;
        while next < self.recall.entered() {
            let frames = self.recall.recalled(next, &mut entering);
            if frames == 0 {
                break;
            }
            let Some(taken) = entering.get(..frames * channels) else {
                break;
            };
            let count = back.process(taken, &mut made_now);
            next = next.saturating_add(count.frames_in.max(1) as u64);
            let made = count.frames_out;
            let passed_over = kept_from.saturating_sub(made_from).min(made as u64) as usize;
            if let Some(kept) = made_now.get(passed_over * channels..made * channels) {
                rendered.samples.extend_from_slice(kept);
            }
            made_from += made as u64;
        }
        rendered
    }

    fn write_blended(
        &mut self,
        back: &Chain,
        rendered: &Rendered,
        blend: &Blend,
        heard: Range<u64>,
    ) {
        let channels = self.plan.stream.channel_count().get() as usize;
        let block = back.max_output_frames().max(back.back().frames_in).max(1);
        let mut before = vec![0.0; blend.over as usize * channels];
        let mut laid = vec![0.0; block * channels];
        let mut staged = AudioBuffer::silence(self.plan.stream, block);

        self.producer
            .render_at(back.gain_amplitude().unwrap_or(Gain::UNITY.get()), 0);
        let held = (rendered.samples.len() / channels) as u64;
        let first = heard.start.saturating_sub(rendered.from).min(held) as usize;
        let last = heard.end.saturating_sub(rendered.from).min(held) as usize;
        let Some(kept) = rendered.samples.get(first * channels..last * channels) else {
            return;
        };
        let mut made_from = heard.start;
        for run in kept.chunks(block * channels) {
            let made = run.len() / channels;
            let Some(laying) = laid.get_mut(..run.len()) else {
                return;
            };
            laying.copy_from_slice(run);

            let blended_to = (made_from + made as u64).min(blend.from + blend.over);
            if made_from < blended_to {
                let frames = (blended_to - made_from) as usize;
                let old = before.get_mut(..frames * channels).unwrap_or_default();
                let widened = self
                    .producer
                    .widen_what_was_written(blend.at + (made_from - blend.from), old);
                for (offset, (new, old)) in laying
                    .get_mut(..widened * channels)
                    .unwrap_or_default()
                    .chunks_exact_mut(channels)
                    .zip(old.chunks_exact(channels))
                    .enumerate()
                {
                    let weight = blend.weight_of_the_new(made_from + offset as u64);
                    for (new, old) in new.iter_mut().zip(old) {
                        *new = *old + (*new - *old) * weight;
                    }
                }
            }

            staged.set_frames(made);
            staged.data_mut().write_f64(laying);
            let written = self.producer.write_spliced(&staged);
            if let Some(tapping) = self.tapping.as_mut() {
                tapping.rendered_at(self.producer.rendering());
                tapping.record(&staged, 0, written);
            }
            made_from += made as u64;
        }
    }
}

struct Rendered {
    from: u64,
    samples: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use resonate_audio::AudioSource;
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, Silence};

    use super::*;
    use crate::ring::{Entering, Level, ring};

    const RING_FRAMES: usize = 4_096;
    const PULLED_FRAMES: usize = 256;
    const PULLED_WHILE_THE_BACK_RENDERS: usize = 4;

    #[test]
    fn a_point_taken_again_once_the_back_has_rendered_lies_ahead_of_what_the_graph_pulled_meanwhile()
     {
        let spec = StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Stereo,
            SampleFormat::S16,
        );
        let (mut producer, mut consumer, _) = ring(
            spec,
            Frames(RING_FRAMES as u64),
            Silence::Unmarked,
            Frames::ZERO,
            Entering::Whole,
            Level::WHOLE,
        );
        let mut recall = Recall::new(NonZeroUsize::MIN.saturating_add(1), 2 * RING_FRAMES);
        let ahead = Frames::from_duration(SPLICED_AHEAD_AT_LEAST, spec.rate);
        let over = Frames::from_duration(SPLICE_BLENDED_OVER, spec.rate);
        let mut pulled = vec![0_u8; PULLED_FRAMES * spec.bytes_per_frame().get() as usize];

        producer.write(&AudioBuffer::silence(spec, RING_FRAMES));
        recall.enter(&vec![0.0; RING_FRAMES * 2]);
        consumer.fill(&mut pulled);
        let mut first_at = None;
        let (taken, ()) = splice_after_rendering(&producer, &recall, ahead, over, |first| {
            first_at = Some(first.at);
            for _ in 0..PULLED_WHILE_THE_BACK_RENDERS {
                consumer.fill(&mut pulled);
            }
        })
        .expect("the ring still has a point to splice at");

        assert!(
            first_at.is_some_and(|first| first < producer.read()),
            "the graph did not pull past the first point"
        );
        assert!(
            taken.at >= producer.read() + 2 * PULLED_FRAMES as u64,
            "the point taken again at {} is not two pulls ahead of the graph at {}",
            taken.at,
            producer.read()
        );
        assert_eq!(taken.from, producer.heard_as(taken.at));
    }
}
