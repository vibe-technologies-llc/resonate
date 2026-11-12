use resonate_core::{Frames, Span};

use super::Engine;
use crate::{Error, Event, RepeatMode, Result, SkipUnderRepeat, Sleeping, queue::Removal};

impl Engine {
    pub(super) fn clear(&mut self) -> Result<()> {
        let Some(last) = self.queue.len().checked_sub(1) else {
            return Ok(());
        };
        let Some(playing) = self.queue.cursor() else {
            return self.remove(Span::between(0, last));
        };
        if playing < last {
            self.remove(Span::between(playing + 1, last))?;
        }
        match playing.checked_sub(1) {
            Some(before) => self.remove(Span::between(0, before)),
            None => Ok(()),
        }
    }

    pub(super) fn remove(&mut self, rows: Span) -> Result<()> {
        let removal = self.queue.remove_rows(rows).ok_or(Error::NoSuchRow {
            rows,
            len: self.queue.len(),
        })?;
        self.carry_on_after(removal)
    }

    pub(super) fn remove_runs(&mut self, runs: &[Span]) -> Result<()> {
        let removal = self
            .queue
            .remove_all(runs)
            .map_err(|rows| Error::NoSuchRow {
                rows,
                len: self.queue.len(),
            })?;
        self.carry_on_after(removal)
    }

    fn carry_on_after(&mut self, removal: Removal) -> Result<()> {
        if removal == Removal::Queued || (self.track.is_none() && self.opening.is_none()) {
            return Ok(());
        }
        if self.queue.current().is_none() {
            self.finish_the_queue();
            return Ok(());
        }
        let started = self.start(Frames::ZERO);
        self.past_what_will_not_open(started)
    }

    pub(super) fn finish_the_queue(&mut self) {
        if self.sleep.is_some_and(Sleeping::ends_the_queue) {
            self.sleep = None;
        }
        self.stop();
        self.emit(Event::QueueFinished);
    }

    pub(super) fn restarts_the_track(&mut self) -> bool {
        let Some((rate, duration)) = self
            .track
            .as_ref()
            .map(|track| (track.clock(), track.info.duration))
        else {
            return false;
        };
        let heard = self.heard_position();
        self.config
            .previous_restarts
            .starts_the_track_over(heard, duration, rate)
    }

    pub(super) fn skipped_by_hand(&mut self) {
        if self.queue.repeat() == RepeatMode::Track
            && self.config.skip_under_repeat == SkipUnderRepeat::RepeatsTheQueue
        {
            self.queue.set_repeat(RepeatMode::Queue);
        }
    }

    pub(super) fn skip(&mut self, natural: bool) -> Result<()> {
        if self.queue.current().is_none() {
            return Err(Error::QueueEmpty);
        }
        if natural && !self.boundaries.is_empty() && !self.pass_every_boundary() {
            return Ok(());
        }
        if natural && let Some(id) = self.track.as_ref().map(|track| track.id) {
            self.emit(Event::TrackFinished(id));
        }
        let wraps = self.queue.wraps_next();
        let leaving = self.queue.current().map(|item| item.id);

        if natural && self.sleep.is_some_and(Sleeping::ends_the_track) {
            self.nods_off();
        }
        if self.queue.advance(natural).is_none() {
            self.finish_the_queue();
            return Ok(());
        }
        if wraps && self.sleep.is_some_and(Sleeping::ends_the_queue) {
            self.nods_off();
        }
        if self.queue.current().map(|item| item.id) == leaving {
            self.seeks = self.seeks.stepped();
        }
        self.start(Frames::ZERO)
    }

    pub(super) fn next(&mut self) -> Result<()> {
        self.failures = 0;
        self.skipped_by_hand();
        let skipped = self.skip(false);
        self.past_what_will_not_open(skipped)
    }

    pub(super) fn lands_past_the_end(&self, to: Frames) -> bool {
        self.opening.is_none()
            && self
                .track
                .as_ref()
                .and_then(|track| track.info.duration)
                .is_some_and(|duration| to >= duration)
    }
}
