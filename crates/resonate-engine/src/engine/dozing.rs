use std::time::{Duration, Instant};

use resonate_core::Frames;

use super::{Engine, SLEEP_FADES_OVER};
use crate::Sleeping;

impl Engine {
    pub(super) fn doze(&mut self) {
        self.fade_toward_sleep();
        if !self.sleep.is_some_and(Sleeping::is_out) {
            return;
        }
        self.sleep = None;
        if self.track.is_none() && self.opening.is_none() {
            return;
        }
        if let Err(error) = self.pause() {
            tracing::warn!(%error, "the sleep timer could not pause the transport");
        }
    }

    fn sleep_is_due_in(&self) -> Option<Duration> {
        let sleeping = self.sleep?;
        if let Some(left) = sleeping.left() {
            return Some(left);
        }
        let ends_here = sleeping.ends_the_track()
            || (sleeping.ends_the_queue()
                && (self.queue.ends_with_this_row() || self.queue.wraps_next()));
        if !ends_here {
            return None;
        }
        let track = self.heard_track()?;
        let left = track.info.duration?.saturating_sub(self.position());
        Some(left.to_duration(track.info.spec.rate))
    }

    fn fade_toward_sleep(&mut self) {
        let Some(left) = self
            .sleep_is_due_in()
            .filter(|left| *left <= SLEEP_FADES_OVER)
        else {
            return;
        };
        let playing = self.playing;
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.note_the_pulls();
        let lifting = output
            .sleep_lifted
            .is_some_and(|lifted| lifted.elapsed() < output.quiet_within());
        if !playing
            || output.sleep_fading
            || lifting
            || !output.is_sounding()
            || !output.producer.is_audible()
            || !output.can_fade()
        {
            return;
        }
        output.sleep_fading = true;
        output
            .producer
            .fade_out(Frames::from_duration(left, output.plan.stream.rate));
    }

    pub(super) fn wake_from_the_sleep_fade(&mut self) {
        if self
            .sleep_is_due_in()
            .is_some_and(|left| left <= SLEEP_FADES_OVER)
        {
            return;
        }
        self.lift_the_sleep_fade();
    }

    pub(super) fn lift_the_sleep_fade(&mut self) {
        let playing = self.playing;
        let Some(output) = self.output.as_mut().filter(|output| output.sleep_fading) else {
            return;
        };
        output.sleep_fading = false;
        output.sleep_lifted = Some(Instant::now());
        if playing {
            output.producer.fade_in(output.fade());
        }
    }

    pub(super) fn nods_off(&mut self) {
        self.sleep = None;
        self.playing = false;
    }
}
