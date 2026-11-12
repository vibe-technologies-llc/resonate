use std::time::Instant;

use resonate_audio::Moment;
use resonate_core::Frames;

use super::{
    Engine, Following, IN_STEP_WITHIN, PULLS_RELEASED_AHEAD_BY, RELEASED_AHEAD_BY, Track,
    output::Output,
};
use crate::{HeardAt, Stepping, TransportState};

impl Engine {
    pub(super) fn may_nudge(&self) -> bool {
        let Some(output) = self.output.as_ref() else {
            return false;
        };

        self.playing
            && self.lockstep.is_none()
            && self.nudging.is_none()
            && self.boundaries.is_empty()
            && !output.splice_owed
            && !output.producer.is_splicing()
            && output.can_fade()
            && !output.holds_seconds_downstream()
    }

    pub(super) fn begin_what_was_nudged(&mut self) {
        let Some(nudging) = self.nudging.as_ref() else {
            return;
        };
        let moved_on = !self.playing || !self.boundaries.is_empty() || self.output.is_none();
        if moved_on || nudging.since.elapsed() >= IN_STEP_WITHIN {
            self.nudging = None;
            if let Some(output) = self.output.as_ref() {
                output.producer.fade_in(output.fade());
            }
            self.stepping = Stepping::Alone;
            return;
        }
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if !output.producer.is_quiet() {
            return;
        }

        output.producer.hold_in_step();
        self.faded_for_a_nudge = true;
        self.lockstep = self.nudging.take().map(|nudging| Following {
            with: nudging.with,
            since: Instant::now(),
        });
    }

    fn sound_again_after_a_nudge(&mut self) {
        if !self.faded_for_a_nudge {
            return;
        }
        self.faded_for_a_nudge = false;
        if let Some(output) = self.output.as_ref() {
            output.producer.fade_in(output.fade());
        }
    }

    pub(super) fn keep_in_step(&mut self) {
        let Some(following) = self.lockstep.as_ref() else {
            return;
        };
        if following.since.elapsed() >= IN_STEP_WITHIN {
            self.step_in_now();
            return;
        }
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if !output.producer.is_held_in_step() {
            self.step_in_now();
            return;
        }
        if output.stream.is_none() || output.producer.drops_pending() {
            return;
        }
        let stamped_under_the_hold = output.producer.stamp().filter(|stamp| !stamp.flowing);
        let (Some(stamp), Some(aim)) = (stamped_under_the_hold, following.with.read()) else {
            return;
        };
        let Some((track, next)) = self.frame_of_the_ring(output, stamp.read) else {
            return;
        };
        let clock = track.clock();
        if track.heard_as != aim.row || aim.rate != clock {
            self.step_in_now();
            return;
        }

        let sink = output.plan.stream.rate;
        let kept = stamp.pulled.saturating_mul(PULLS_RELEASED_AHEAD_BY);
        let lead = stamp.next.nanos().saturating_sub(stamp.taken.nanos());
        let soonest = stamp
            .next
            .max(Moment::from_nanos(
                Moment::now().nanos().saturating_add(lead),
            ))
            .after(RELEASED_AHEAD_BY)
            .after_frames(kept as f64, sink);
        let behind = aim.frame_at(soonest) - next.get() as f64;
        let (release, behind) = if behind < 0.0 {
            (soonest.after_frames(-behind, clock), 0.0)
        } else {
            (soonest, behind)
        };
        let dropped = (behind * f64::from(sink.hz()) / f64::from(clock.hz())).round() as u64;
        let held = output.buffered() as u64;

        if dropped.saturating_add(kept) > held {
            let partly = held.saturating_sub(kept);
            if partly > 0 {
                output.producer.drop_ahead(partly);
            }
            return;
        }
        output.producer.drop_ahead(dropped);
        output.producer.release_at(release);
        self.lockstep = None;
        self.stepping = Stepping::Joined(release);
        self.sound_again_after_a_nudge();
    }

    fn step_in_now(&mut self) {
        let now = Moment::now();
        if let Some(output) = self.output.as_ref() {
            output.producer.release_at(now);
        }
        self.lockstep = None;
        self.stepping = Stepping::Joined(now);
        self.sound_again_after_a_nudge();
    }

    pub(super) fn heard_at(&self) -> Option<HeardAt> {
        if self.transport != TransportState::Playing {
            return None;
        }
        let output = self.output.as_ref()?;
        output.stream.as_ref()?;
        let stamp = output.producer.stamp().filter(|stamp| stamp.flowing)?;
        let (track, frame) = self.frame_of_the_ring(output, stamp.read)?;
        Some(HeardAt {
            row: track.heard_as,
            frame,
            at: stamp.heard,
            rate: track.clock(),
        })
    }

    fn frame_of_the_ring<'a>(
        &'a self,
        output: &Output,
        ring_frame: u64,
    ) -> Option<(&'a Track, Frames)> {
        let rate = output.plan.stream.rate;
        if let Some(boundary) = self.boundaries.front() {
            let leaving = &boundary.leaving;
            let behind = boundary.at.checked_sub(ring_frame)?;
            let on_the_clock = Frames(behind).rescaled(rate, leaving.clock());
            return Some((
                leaving,
                leaving.decoding.position().saturating_sub(on_the_clock),
            ));
        }
        let track = self.track.as_ref()?;
        let decoded = track
            .decoding
            .position()
            .saturating_sub(track.undecoded_on_the_clock());
        let written = output
            .producer
            .read()
            .saturating_add(output.buffered() as u64);
        let behind = written
            .saturating_sub(ring_frame)
            .saturating_add(output.chain.latency_frames().round() as u64);
        let on_the_clock = Frames(behind).rescaled(rate, track.clock());
        Some((track, decoded.saturating_sub(on_the_clock)))
    }
}
