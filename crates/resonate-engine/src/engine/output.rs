use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use resonate_audio::{
    Downstream, NodeName, SinkId, SinkInfo, SinkStream, StreamEvent, StreamVolume, Words,
};
use resonate_codec::Packing;
use resonate_core::{AudioBuffer, Frames, Gain, StreamSpec};
use resonate_dsp::{Chain, Recall};

use super::{
    CHAIN_BLOCK, PULLED_WITHIN, QUIET_WITHIN_AT_LEAST, QUIET_WITHIN_AT_MOST,
    RESHAPED_FORMAT_SETTLED_WITHIN, Tail, Track, arms::Arms, carried_samples, ring_capacity,
    splicing::recall_for,
};
use crate::{
    EngineConfig, Error, OutputPlan, OutputStatus, Result, Tapped,
    pipeline::{Attenuator, plan_for, plan_output},
    ring::{Entering, FADED_OVER, Level, RingConsumer, RingMonitor, RingProducer, ring},
    streaming::close_unwanted,
    tap::Tapping,
};

pub(super) struct Output {
    pub(super) sink: SinkId,
    pub(super) bound: SinkInfo,
    pub(super) attenuator: Attenuator,
    pub(super) over_bluetooth: bool,
    pub(super) awake_since: Option<Instant>,
    pub(super) plan: OutputPlan,
    pub(super) chain: Chain,
    pub(super) widened: Vec<f64>,
    pub(super) carrier: Vec<f64>,
    pub(super) staged: AudioBuffer,
    pub(super) producer: RingProducer,
    pub(super) monitor: RingMonitor,
    pub(super) consumer: Option<RingConsumer>,
    pub(super) stream: Option<SinkStream>,
    pub(super) made: u64,
    pub(super) tapping: Option<Tapping>,
    pub(super) capacity: usize,
    pub(super) status: OutputStatus,
    pub(super) draining: Option<usize>,
    pub(super) ended: bool,
    pub(super) ends_at_a_change: bool,
    pub(super) deaf: bool,
    pub(super) tail: Tail,
    pub(super) silent_until: Option<Instant>,
    pub(super) discarding_since: Option<Instant>,
    pub(super) settles_into: Option<OutputPlan>,
    pub(super) active: bool,
    pub(super) quietening_since: Option<Instant>,
    pub(super) sleep_fading: bool,
    pub(super) sleep_lifted: Option<Instant>,
    pub(super) last_pulled: Option<(u64, Instant)>,
    pub(super) overs_of_retired_chains: u64,
    pub(super) handed: Option<Handed>,
    pub(super) heard_while_kept: Vec<StreamEvent>,
    pub(super) format_owed_since: Option<Instant>,
    pub(super) recall: Recall,
    pub(super) splice_owed: bool,
    pub(super) rendered_for_another: bool,
    pub(super) arms: Option<Arms>,
    pub(super) switch_owed: bool,
    pub(super) switch_waits_for_room: bool,
    pub(super) recall_spoiled: bool,
}

pub(super) struct Retiring {
    pub(super) output: Output,
    pub(super) since: Instant,
    pub(super) hands_on_its_stream: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Retire {
    HandingOnItsStream,
    ClosingItsStream,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Handed {
    pub(super) spec: StreamSpec,
    pub(super) words: Option<Words>,
    pub(super) stream_volume: StreamVolume,
}

pub(super) struct Kept {
    pub(super) stream: SinkStream,
    pub(super) sink: SinkId,
    pub(super) handed: Handed,
}

impl Kept {
    pub(super) fn close(self) {
        close_unwanted(self.stream);
    }
}

pub(super) fn heard_amplitude(plan: &OutputPlan, muted: bool) -> f32 {
    if muted {
        Gain::SILENT.get()
    } else {
        plan.amplitude()
    }
}

pub(super) fn chosen_sink<'s>(
    sinks: &'s [SinkInfo],
    named: Option<&NodeName>,
) -> Option<&'s SinkInfo> {
    sinks
        .iter()
        .find(|sink| named.is_some_and(|named| sink.is_named(named)))
        .or_else(|| sinks.iter().find(|sink| sink.is_default))
        .or_else(|| sinks.first())
}

impl Output {
    pub(super) fn open(
        track: &Track,
        sink: &SinkInfo,
        config: &EngineConfig,
        target: Option<StreamSpec>,
        listening: &Arc<AtomicBool>,
        entering: Entering,
        muted: bool,
    ) -> Result<Self> {
        let source = track.source();
        let decoded = track.decoded();
        let attenuator = Attenuator::of(config, sink);
        let plan = match target {
            Some(target) => plan_for(decoded, sink, target, config, track.replay_gain, attenuator),
            None => plan_output(decoded, sink, config, track.replay_gain),
        };

        let chain = plan
            .build_chain(source, config, CHAIN_BLOCK)
            .map_err(|error| Error::Convert {
                track: track.id,
                source_spec: source,
                sink_spec: plan.stream,
                source: error,
            })?;

        let capacity = ring_capacity(config.buffer, plan.stream, chain.max_process_frames());
        let fade = Frames::from_duration(FADED_OVER, plan.stream.rate);
        let level = Level {
            rendered: plan.amplitude(),
            heard: heard_amplitude(&plan, muted),
        };
        let (producer, consumer, monitor) =
            ring(plan.stream, capacity, plan.silence(), fade, entering, level);

        let widened = vec![0.0; CHAIN_BLOCK * source.channel_count().get() as usize];
        let recall = recall_for(plan.stream, capacity);
        let carrier = vec![0.0; carried_samples(&chain, plan.stream)];
        let staged = AudioBuffer::silence(plan.stream, chain.max_output_frames());
        let tapping = match plan.packing {
            Packing::Samples => Some(Tapping::new(
                plan.stream.rate,
                capacity.get() as usize,
                listening,
                Some(producer.heard()),
            )),
            Packing::DopMarked(_) => None,
        };

        Ok(Self {
            sink: sink.id,
            bound: sink.clone(),
            attenuator,
            over_bluetooth: sink.is_bluetooth(),
            awake_since: None,
            status: OutputStatus {
                sink: sink.id,
                negotiated: plan.stream,
                words: None,
                mode: plan.mode,
                graph_rate: None,
                latency: Frames::ZERO,
                underruns: 0,
                went_without: Frames::ZERO,
                overs: 0,
                device_turned: attenuator == Attenuator::Device,
                device_muted: attenuator.hears_the_mute_of(sink),
                stream_volume: StreamVolume::Unity,
                holds_seconds_downstream: false,
            },
            plan,
            chain,
            widened,
            carrier,
            staged,
            producer,
            monitor,
            consumer: Some(consumer),
            stream: None,
            made: 0,
            tapping,
            capacity: capacity.get() as usize,
            draining: None,
            ended: false,
            ends_at_a_change: false,
            deaf: false,
            tail: Tail::Unasked,
            silent_until: None,
            discarding_since: None,
            settles_into: None,
            active: false,
            quietening_since: None,
            sleep_fading: false,
            sleep_lifted: None,
            last_pulled: None,
            overs_of_retired_chains: 0,
            handed: None,
            heard_while_kept: Vec::new(),
            format_owed_since: None,
            recall,
            splice_owed: false,
            rendered_for_another: false,
            arms: None,
            switch_owed: false,
            switch_waits_for_room: false,
            recall_spoiled: false,
        })
    }

    pub(super) fn fade(&self) -> Frames {
        Frames::from_duration(FADED_OVER, self.plan.stream.rate)
    }

    pub(super) const fn can_fade(&self) -> bool {
        matches!(self.plan.packing, Packing::Samples)
    }

    pub(super) fn quiet_within(&self) -> Duration {
        let behind = self.latency().to_duration(self.plan.stream.rate);
        (FADED_OVER + behind * 2).clamp(QUIET_WITHIN_AT_LEAST, QUIET_WITHIN_AT_MOST)
    }

    pub(super) fn has_gone_quiet(&self, since: Instant) -> bool {
        self.producer.is_quiet() || since.elapsed() >= self.quiet_within()
    }

    pub(super) fn note_the_pulls(&mut self) {
        let pulls = self.producer.pulls();
        if self.last_pulled.is_none_or(|(seen, _)| seen != pulls) {
            self.last_pulled = Some((pulls, Instant::now()));
        }
    }

    pub(super) fn is_sounding(&self) -> bool {
        self.active
            && self.stream.is_some()
            && !self.producer.is_quiet()
            && self
                .last_pulled
                .is_some_and(|(pulls, at)| pulls > 0 && at.elapsed() < PULLED_WITHIN)
    }

    pub(super) fn activate(&mut self, active: bool) -> Result<()> {
        let Some(stream) = self.stream.as_ref() else {
            return Ok(());
        };
        stream.set_active(active)?;
        self.active = active;
        Ok(())
    }

    pub(super) fn take_chain(&mut self, chain: Chain, plan: OutputPlan) -> OutputStatus {
        if self.draining.is_none() {
            self.carrier
                .resize(carried_samples(&chain, plan.stream), 0.0);
            self.staged.set_frames(chain.max_output_frames());
        }
        self.overs_of_retired_chains = self
            .overs_of_retired_chains
            .saturating_add(self.chain.overs());
        self.chain = chain;
        self.arms = None;
        self.note_the_overs();
        self.status.hears(plan.mode);
        self.plan = plan;
        self.settles_into = None;
        self.status
    }

    pub(super) fn has_room_for_what_the_chain_holds(&self, carrying_the_front: bool) -> bool {
        let held = if carrying_the_front {
            self.chain.max_flush_frames_behind_the_front()
        } else {
            self.chain.max_flush_frames()
        };
        self.producer.free_frames() >= held
    }

    pub(super) fn frames_rendered_ahead_of_the_ring(&self) -> usize {
        let staged_unwritten = self
            .draining
            .map_or(0, |written| self.staged.frames().saturating_sub(written));
        self.chain
            .frames_held_after_the_gain()
            .saturating_add(staged_unwritten)
    }

    pub(super) fn note_the_rendered_level(&mut self) {
        let rendered = self.chain.gain_amplitude().unwrap_or(Gain::UNITY.get());
        let ahead = self.frames_rendered_ahead_of_the_ring();
        self.producer.render_at(rendered, ahead);
    }

    pub(super) fn note_the_overs(&mut self) {
        let overs = self
            .overs_of_retired_chains
            .saturating_add(self.chain.overs());
        if self.status.overs == 0 && overs > 0 {
            tracing::warn!(
                sink = %self.bound.name,
                "samples past full scale reach the sink's word with nothing weighing them; \
                 lower the preamp or turn true-peak on"
            );
        }
        self.status.overs = overs;
    }

    pub(super) fn waits_for_room_to_reshape(&self) -> bool {
        self.settles_into.is_some() && !self.chain.is_ramping()
    }

    pub(super) fn hand_on_what_the_chain_holds(&mut self) {
        if self.chain.max_flush_frames() == 0 {
            return;
        }
        let frames = match self
            .chain
            .flush_recalled(&mut self.carrier, &mut self.recall)
        {
            Ok(frames) => frames,
            Err(error) => {
                tracing::error!(%error, "the chain's held frames did not fit the carrier; they were dropped");
                return;
            }
        };
        self.stage(frames);
        let written = self.producer.write(&self.staged);
        if let Some(tapping) = self.tapping.as_mut() {
            tapping.rendered_at(self.producer.rendering());
            tapping.record(&self.staged, 0, written);
        }
    }

    pub(super) fn stage(&mut self, frames: usize) {
        let channels = self.plan.stream.channel_count().get() as usize;
        self.staged.set_frames(frames);
        let carried = self
            .carrier
            .get(..frames.saturating_mul(channels))
            .unwrap_or_default();
        self.staged.data_mut().write_f64(carried);
    }

    pub(super) fn buffered(&self) -> usize {
        self.producer.held_frames()
    }

    pub(super) fn tapped(&self) -> Tapped {
        self.tapping
            .as_ref()
            .map_or(Tapped::Markers, Tapping::tapped)
    }

    pub(super) fn hear_the_tap(&self, moving: bool) {
        let Some(tapping) = self.tapping.as_ref() else {
            return;
        };
        let downstream = (self.producer.written_ahead_of_the_graph() as u64)
            .saturating_add(self.latency().get());
        tapping.hear(downstream, moving && self.stream.is_some());
    }

    pub(super) fn primed(&self) -> bool {
        self.ended || self.buffered().saturating_mul(2) >= self.capacity
    }

    pub(super) fn wants_more(&self) -> bool {
        if self.draining.is_some() {
            return true;
        }
        if self.ended
            || self.producer.is_discarding()
            || self.waits_for_room_to_reshape()
            || self.switch_waits_for_room
        {
            return false;
        }
        self.producer.free_frames() >= self.chain.max_process_frames()
    }

    pub(super) fn holds_a_live_stream(&self) -> bool {
        self.stream.is_some()
            && !self.ended
            && self.draining.is_none()
            && !self.producer.is_discarding()
    }

    pub(super) fn holds_seconds_downstream(&self) -> bool {
        self.stream
            .as_ref()
            .is_some_and(|stream| stream.downstream() == Downstream::Seconds)
    }

    pub(super) fn latency(&self) -> Frames {
        self.stream
            .as_ref()
            .map(SinkStream::latency)
            .unwrap_or(Frames::ZERO)
    }

    pub(super) fn end(&mut self) {
        self.ended = true;
        self.producer.finish();
    }

    pub(super) fn ask_to_drain(&mut self) {
        if self.tail != Tail::Unasked {
            return;
        }
        let Some(stream) = self.stream.as_ref() else {
            return;
        };
        if let Err(error) = stream.drain() {
            tracing::debug!(%error, "the graph would not take a drain request");
        }
        self.tail = Tail::Asked;
    }

    pub(super) fn tail_is_out(&mut self) -> bool {
        if self.tail == Tail::Played {
            return true;
        }
        let played_by = self.latency().to_duration(self.plan.stream.rate);
        let until = *self.silent_until.get_or_insert(Instant::now() + played_by);
        Instant::now() >= until
    }

    pub(super) fn can_hand_on_its_stream(&self) -> bool {
        self.stream.is_some() && self.active && !self.deaf
    }

    pub(super) fn kept(mut self) -> Option<Kept> {
        let stream = self.stream.take()?;
        Some(Kept {
            stream,
            sink: self.sink,
            handed: Handed {
                spec: self.plan.stream,
                words: self.status.words,
                stream_volume: self.status.stream_volume,
            },
        })
    }

    pub(super) fn take_what_the_stream_was_handed(&mut self, heard_while_kept: Vec<StreamEvent>) {
        let Some(handed) = self.handed.take() else {
            return;
        };
        self.status.stream_volume = handed.stream_volume;
        let asked_alike = handed.spec == self.plan.stream;
        self.heard_while_kept = heard_while_kept
            .into_iter()
            .filter(|event| asked_alike || !matches!(event, StreamEvent::FormatChanged { .. }))
            .collect();
        if asked_alike {
            self.status.words = handed.words;
            return;
        }
        self.format_owed_since = Some(Instant::now());
    }

    pub(super) fn reshaped_format_went_unanswered(&self) -> bool {
        self.format_owed_since
            .is_some_and(|since| since.elapsed() >= RESHAPED_FORMAT_SETTLED_WITHIN)
    }

    pub(super) fn close(&mut self) {
        let Some(stream) = self.stream.take() else {
            return;
        };
        let _ = stream.set_active(false);
        if let Err(error) = stream.close() {
            tracing::warn!(%error, "the sink stream did not close cleanly");
        }
    }
}
