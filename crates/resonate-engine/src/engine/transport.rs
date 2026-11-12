use std::time::Instant;

use resonate_core::Frames;

use super::{
    Engine,
    output::{Retire, Retiring},
};
use crate::{CommandKind, Error, Result, Stepping, TransportState};

impl Engine {
    pub(super) fn hear(&mut self, row: usize) -> Result<()> {
        self.queue.jump_to(row).ok_or(Error::QueueEmpty)?;
        self.start_what_was_jumped_to()
    }

    pub(super) fn start_what_was_jumped_to(&mut self) -> Result<()> {
        self.failures = 0;
        self.playing = true;
        let started = self.start(Frames::ZERO);
        self.past_what_will_not_open(started)
    }

    pub(super) fn past_what_will_not_open(&mut self, started: Result<()>) -> Result<()> {
        match started {
            Err(error @ Error::Decode { .. }) if self.playing => {
                self.fail(error);
                Ok(())
            }
            started => started,
        }
    }

    pub(super) fn play(&mut self) -> Result<()> {
        let nothing_to_start =
            self.track.is_none() && self.opening.is_none() && self.queue.current().is_none();
        if nothing_to_start {
            return Err(Error::QueueEmpty);
        }
        self.playing = true;
        if self.opening.is_some() {
            return Ok(());
        }
        if self.track.is_none() {
            let started = self.start(Frames::ZERO);
            return self.past_what_will_not_open(started);
        }
        if let Some(at) = self.unbound {
            self.rebind(Some(at), None)?;
            if self.output.is_some() {
                self.unbound = None;
            }
            return Ok(());
        }
        if self.transport == TransportState::Paused {
            self.transport = TransportState::Playing;
        }
        if let Some(output) = self.output.as_mut() {
            output.producer.fade_in(output.fade());
            output.quietening_since = None;
            if output.sleep_fading {
                output.sleep_lifted = Some(Instant::now());
            }
            output.sleep_fading = false;
            if output.awake_since.take().is_some() {
                return Ok(());
            }
        }
        self.wake_the_link();
        self.set_active(true)
    }

    pub(super) fn pause(&mut self) -> Result<()> {
        if self.opening.is_some() {
            self.playing = false;
            return Ok(());
        }
        if self.track.is_none() {
            return Err(Error::InvalidTransition {
                state: self.transport,
                attempted: CommandKind::Pause,
            });
        }
        self.playing = false;
        if matches!(
            self.transport,
            TransportState::Playing | TransportState::Draining
        ) || (self.transport == TransportState::Loading
            && self.output.is_none()
            && self.unbound.is_some())
        {
            self.transport = TransportState::Paused;
        }
        let keeps_the_link_awake = self.keeps_the_link_awake();
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        output.note_the_pulls();
        if output.holds_seconds_downstream() {
            return output.activate(false);
        }
        output.producer.fade_out(output.fade());
        if keeps_the_link_awake {
            output.awake_since = Some(Instant::now());
            return Ok(());
        }
        if output.is_sounding() {
            output.quietening_since = Some(Instant::now());
            return Ok(());
        }
        output.activate(false)
    }

    pub(super) fn set_active(&mut self, active: bool) -> Result<()> {
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        output.activate(active)
    }

    pub(super) fn retire_the_output(&mut self) {
        self.retire(Retire::ClosingItsStream);
    }

    pub(super) fn retire(&mut self, how: Retire) {
        let Some(mut output) = self.output.take() else {
            return;
        };
        output.note_the_pulls();
        self.close_what_was_retiring();
        self.close_what_was_kept();
        let hands_on_its_stream =
            how == Retire::HandingOnItsStream && output.can_hand_on_its_stream();
        if !output.is_sounding() {
            if hands_on_its_stream {
                self.kept = output.kept();
            } else {
                output.close();
            }
            return;
        }
        output.producer.fade_out(output.fade());
        self.retiring = Some(Retiring {
            output,
            since: Instant::now(),
            hands_on_its_stream,
        });
    }

    pub(super) fn close_what_was_retiring(&mut self) {
        if let Some(mut retiring) = self.retiring.take() {
            retiring.output.close();
        }
    }

    pub(super) fn close_what_was_kept(&mut self) {
        if let Some(kept) = self.kept.take() {
            kept.close();
        }
    }

    fn hand_on_what_has_retired(&mut self) {
        let Some(retiring) = self.retiring.take() else {
            return;
        };
        if retiring.hands_on_its_stream {
            self.close_what_was_kept();
            self.kept = retiring.output.kept();
            return;
        }
        let mut output = retiring.output;
        output.close();
    }

    pub(super) fn finish_fading(&mut self) {
        let retired = self
            .retiring
            .as_ref()
            .is_some_and(|retiring| retiring.output.has_gone_quiet(retiring.since));
        if retired {
            self.hand_on_what_has_retired();
        }
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.note_the_pulls();
        let quiet = output
            .quietening_since
            .is_some_and(|since| output.has_gone_quiet(since));
        if !quiet {
            return;
        }
        output.quietening_since = None;
        if let Err(error) = output.activate(false) {
            tracing::warn!(%error, "the stream would not stand down once it had faded out");
        }
    }

    pub(super) fn stop(&mut self) {
        self.playing = false;
        self.failures = 0;
        self.retire_the_output();
        self.track = None;
        self.boundaries.clear();
        self.failing_past_the_boundary = None;
        self.ahead = None;
        self.opening = None;
        self.answer_the_deferred(Ok(()));
        self.unbound = None;
        self.heard_at_least = None;
        self.lockstep = None;
        self.nudging = None;
        self.faded_for_a_nudge = false;
        self.stepping = Stepping::Alone;
        self.transport = TransportState::Stopped;
    }
}
