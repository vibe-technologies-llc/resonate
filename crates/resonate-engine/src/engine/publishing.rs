use std::{borrow::Cow, sync::Arc};

use resonate_core::Frames;

use super::{Engine, digest_is_due, output::Output};
use crate::{
    OutputSettings, PlaybackState, PlayerState, Sleeping, Tapped, TrackState, TransportState,
};

impl Engine {
    pub(super) fn heard_position(&mut self) -> Frames {
        let position = self
            .heard_at_least
            .map_or(self.position(), |floor| self.position().max(floor));
        self.heard_at_least = self.track.is_some().then_some(position);
        position
    }

    pub(super) fn position(&self) -> Frames {
        if let (Some(boundary), Some(output)) = (self.boundaries.front(), self.output.as_ref()) {
            let leaving = &boundary.leaving;
            let behind = boundary
                .at
                .saturating_sub(output.producer.read())
                .saturating_add(output.latency().get());
            let on_the_clock = Frames(behind).rescaled(output.plan.stream.rate, leaving.clock());
            return leaving.decoding.position().saturating_sub(on_the_clock);
        }
        let Some(track) = self.track.as_ref() else {
            return Frames::ZERO;
        };
        let decoded = track
            .decoding
            .position()
            .saturating_sub(track.undecoded_on_the_clock());

        let Some(output) = self.output.as_ref() else {
            return self.unbound.unwrap_or(decoded);
        };
        if output.producer.is_discarding() {
            return decoded;
        }
        let downstream = (output.buffered() as u64)
            .saturating_add(output.latency().get())
            .saturating_add(output.chain.latency_frames().round() as u64);
        let on_the_clock = Frames(downstream).rescaled(output.plan.stream.rate, track.clock());

        decoded.saturating_sub(on_the_clock)
    }

    pub(super) fn publish(&mut self) {
        let playback = match self.transport {
            TransportState::Idle => PlaybackState::Idle,
            TransportState::Loading if self.playing => PlaybackState::Buffering,
            TransportState::Loading => PlaybackState::Paused,
            TransportState::Playing | TransportState::Draining => PlaybackState::Playing,
            TransportState::Paused => PlaybackState::Paused,
            TransportState::Stopped => PlaybackState::Stopped,
        };
        let position = self.heard_position();
        let current = self.heard_track().map(|track| TrackState {
            id: track.id,
            source: track.info.spec,
            position,
            duration: track.info.duration,
        });

        self.published.heard.tell(self.heard_at());
        *self.published.stepping.write() = self.stepping;
        self.publish_queue();
        self.publish_digest();
        self.publish_settings();
        self.publish_tap();

        *self.published.state.write() = PlayerState {
            playback,
            current,
            volume: self.heard_volume(),
            repeat: self.queue.repeat(),
            skip_under_repeat: self.config.skip_under_repeat,
            previous_restarts: self.config.previous_restarts,
            shuffle: self.queue.shuffle(),
            smart_shuffle: self.queue.smart_shuffle(),
            queue_position: self.queue.cursor(),
            loaded_position: self.queue.position(),
            queue_len: self.queue.len(),
            queue_stamp: self.queue.playing_from(),
            seeks: self.seeks,
            sleeping: self.sleep.map(Sleeping::published),
            output: self.output.as_ref().map(|output| output.status),
        };
    }

    fn publish_tap(&self) {
        let tapped = self.output.as_ref().map_or(Tapped::Nothing, Output::tapped);
        if !self.published.tap.read().is(&tapped) {
            *self.published.tap.write() = tapped;
        }

        let moving = matches!(
            self.transport,
            TransportState::Playing | TransportState::Draining
        );
        if let Some(output) = self.output.as_ref() {
            output.hear_the_tap(moving);
        }
    }

    fn publish_settings(&self) {
        let set = match self.auditioning {
            Some(_) => Cow::Owned(self.settings_as_set()),
            None => Cow::Borrowed(&self.config),
        };
        let unchanged = self.published.settings.read().already_says(&set);
        if unchanged {
            return;
        }
        *self.published.settings.write() = Arc::new(OutputSettings::of(&set));
    }

    fn publish_queue(&mut self) {
        let revision = self.queue.revision();
        if self.announced == Some(revision) {
            return;
        }
        self.announced = Some(revision);
        *self.published.queue.write() = self.queue.published();
    }

    fn publish_digest(&mut self) {
        let mode = self.config.replay_gain;
        let heard = match self.boundaries.front_mut() {
            Some(boundary) => Some(&mut boundary.leaving),
            None => self.track.as_mut(),
        };
        let Some(track) = heard else {
            if self.published.digest.read().is_some() {
                *self.published.digest.write() = None;
            }
            return;
        };

        let points = track.profile.points();
        if !digest_is_due(track.published, points) {
            return;
        }
        track.published = Some(points);
        *self.published.digest.write() = Some(Arc::new(track.digest(mode)));
    }
}
