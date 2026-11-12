use resonate_core::Frames;

use super::{Engine, Opening, Track, Unwrapped};
use crate::{Error, Event, Result, TransportState, queue::QueueItem};

impl Engine {
    pub(super) fn start(&mut self, at: Frames) -> Result<()> {
        let Some(item) = self.queue.current().cloned() else {
            return Err(Error::QueueEmpty);
        };

        self.retire_the_output();
        self.track = None;
        self.boundaries.clear();
        self.failing_past_the_boundary = None;
        if self.opening.take().is_some() {
            self.answer_the_deferred(Ok(()));
        }
        self.heard_at_least = None;
        self.rebind_owed = false;

        let opening = match self.ahead.take() {
            Some(ahead) if ahead.opening.item == item && !ahead.lost => ahead.opening_at(at),
            Some(_) | None => Opening::begin(item, at, &self.sources),
        };
        self.opening = Some(opening);
        self.openings_begun = self.openings_begun.wrapping_add(1);
        self.transport = TransportState::Loading;
        Ok(())
    }

    pub(super) fn land_the_opening(&mut self) {
        let Some(landed) = self.opening.as_ref().and_then(Opening::landed) else {
            return;
        };
        let Some(Opening { item, at, .. }) = self.opening.take() else {
            return;
        };
        let started = landed.and_then(|unwrapped| self.started(&item, unwrapped, at));
        if !self.deferred.is_empty() {
            self.answer_for_the_opening(started);
            return;
        }
        let Err(error) = started else {
            return;
        };
        if self.playing {
            self.fail(error);
            return;
        }
        self.transport = TransportState::Stopped;
        self.emit(Event::Failed {
            track: item.id,
            error,
        });
    }

    fn answer_for_the_opening(&mut self, started: Result<()>) {
        let outcome = self.past_what_will_not_open(started);
        let outcome = self.carried_on_past_a_stranded_row(outcome);
        if outcome.is_ok() && self.opening.is_some() {
            return;
        }
        if outcome.is_err() && self.track.is_none() && self.transport == TransportState::Loading {
            self.transport = TransportState::Stopped;
        }
        self.answer_the_deferred(outcome);
    }

    pub(super) fn started(
        &mut self,
        item: &QueueItem,
        unwrapped: Unwrapped,
        at: Frames,
    ) -> Result<()> {
        let track = Track::of(item, unwrapped, &self.sources, &self.config);
        let at = track
            .info
            .duration
            .filter(|duration| at > *duration)
            .map_or(at, |_| Frames::ZERO);
        self.track = Some(track);
        self.unbound = (!self.playing).then_some(at);

        let bound = self.rebind(Some(at), None);
        self.renegotiations = 0;
        self.emit(Event::TrackStarted(item.id));
        bound
    }
}
