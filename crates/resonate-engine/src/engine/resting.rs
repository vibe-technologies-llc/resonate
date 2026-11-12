use std::time::{Duration, Instant};

use resonate_audio::SinkId;
use resonate_core::Frames;

use super::{Engine, LINK_NAPS_AFTER};

impl Engine {
    pub(super) fn is_fading(&self) -> bool {
        self.retiring.is_some()
            || self
                .output
                .as_ref()
                .is_some_and(|output| output.quietening_since.is_some())
    }

    pub(super) fn kept_awake_for(&self) -> Option<Duration> {
        let since = self.output.as_ref()?.awake_since?;
        Some(
            self.config
                .bluetooth
                .awake_for
                .saturating_sub(since.elapsed()),
        )
    }

    pub(super) fn keeps_the_link_awake(&self) -> bool {
        self.config.bluetooth.on
            && self
                .output
                .as_ref()
                .is_some_and(|output| output.over_bluetooth && output.stream.is_some())
    }

    fn link_is_asleep(&self, sink: SinkId) -> bool {
        self.sounded
            .is_none_or(|(held, at)| held != sink || at.elapsed() >= LINK_NAPS_AFTER)
    }

    fn note_what_is_sounding(&mut self) {
        let Some(output) = self.output.as_ref() else {
            return;
        };
        let sounding = output.stream.is_some() && (self.playing || output.awake_since.is_some());
        if sounding {
            self.sounded = Some((output.sink, Instant::now()));
        }
    }

    pub(super) fn wake_the_link(&self) {
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if !self.config.bluetooth.on || !output.over_bluetooth || !self.link_is_asleep(output.sink)
        {
            return;
        }
        let lead = Frames::from_duration(self.config.bluetooth.lead, output.plan.stream.rate);
        output.producer.lead_in(lead);
    }

    pub(super) fn let_the_link_rest(&mut self) {
        self.note_what_is_sounding();
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let Some(since) = output.awake_since else {
            return;
        };
        let wake = self.config.bluetooth;
        if wake.on && since.elapsed() < wake.awake_for {
            return;
        }
        output.awake_since = None;
        if let Err(error) = output.activate(false) {
            tracing::warn!(%error, "the stream kept awake would not stand down");
        }
    }

    pub(super) fn at_rest(&self) -> bool {
        if self.playing
            || self.is_fading()
            || self.sleep.is_some()
            || self.graph_lost.is_some()
            || self.stale_sinks
            || self.opening.is_some()
            || self.is_opening_a_stream()
        {
            return false;
        }
        self.output.as_ref().is_none_or(|output| {
            !output.wants_more()
                && output.discarding_since.is_none()
                && output.settles_into.is_none()
                && !output.chain.is_ramping()
        })
    }
}
