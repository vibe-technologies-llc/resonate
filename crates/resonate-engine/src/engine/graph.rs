use std::{mem, time::Instant};

use crossbeam_channel::TryRecvError;
use resonate_audio::{Downstream, StreamEvent, StreamState};
use resonate_core::{Frames, RtFault, StreamSpec};

use super::{
    Drained, Engine, GRAPH_BACK_WITHIN, MAX_RENEGOTIATIONS, SINK_TIMEOUT, Tail,
    UNDERRUNS_TOLD_EVERY, Waiting, output::Output,
};
use crate::{Error, Event, Result, TransportState, surveying::Surveying};

impl Engine {
    pub(super) fn watch_graph(&mut self) {
        if let Some(since) = self.graph_lost {
            self.wait_for_the_graph(since);
            return;
        }
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if output.stream.is_none() || !output.producer.is_abandoned() {
            return;
        }
        let again = self.graph_lost_again();
        if again {
            self.fail(Error::Sink(resonate_audio::Error::LoopStopped));
            return;
        }

        tracing::warn!("the graph let go of the stream; the row waits for it to come back");
        self.take_the_boundary_back();
        let at = self.heard_position();
        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;
        self.unbound = Some(at);
        self.transport = TransportState::Loading;
        self.graph_lost = Some(Instant::now());
    }

    fn graph_lost_again(&mut self) -> bool {
        let again = self
            .graph_last_lost
            .is_some_and(|last| last.elapsed() < GRAPH_BACK_WITHIN);
        self.graph_last_lost = Some(Instant::now());
        again
    }

    fn row_the_graph_let_go(&self) -> Option<Frames> {
        self.unbound
            .filter(|_| self.playing && self.track.is_some() && self.output.is_none())
    }

    fn wait_for_the_graph(&mut self, since: Instant) {
        if self.opening.is_some() || self.waits_on_a_stream() {
            return;
        }
        let Some(at) = self.row_the_graph_let_go() else {
            self.graph_lost = None;
            self.graph_still_away = None;
            return;
        };
        if !self.the_survey_runs() {
            self.wait_for_the_graph_in_line(since, at);
            return;
        }
        if since.elapsed() >= GRAPH_BACK_WITHIN {
            self.graph_lost = None;
            let away = self
                .graph_still_away
                .take()
                .unwrap_or(Error::Sink(resonate_audio::Error::Disconnected));
            self.fail(away);
            return;
        }
        if !self.surveying.as_ref().is_some_and(Surveying::is_asked) {
            self.stale_sinks = true;
        }
        self.transport = TransportState::Loading;
    }

    pub(super) fn bind_the_row_the_graph_let_go(&mut self) {
        if self.graph_lost.is_none() {
            return;
        }
        let Some(at) = self.row_the_graph_let_go() else {
            return;
        };
        match self.rebind(Some(at), None) {
            Ok(()) if self.playing && self.output.is_none() => {
                tracing::debug!(
                    "the sink list moved while it was surveyed; the row waits for the next survey"
                );
            }
            Ok(()) if self.waits_on_a_stream() => {
                tracing::debug!("the graph took the row; it is back once the stream opens");
                self.unbound = None;
            }
            Ok(()) => {
                tracing::info!("the graph is back; the row plays on from where it was heard");
                self.unbound = None;
                self.graph_lost = None;
                self.graph_still_away = None;
            }
            Err(error) => {
                tracing::debug!(%error, "the graph is not back yet");
                self.graph_still_away = Some(error);
                self.transport = TransportState::Loading;
            }
        }
    }

    fn wait_for_the_graph_in_line(&mut self, since: Instant, at: Frames) {
        let back = self
            .surveyor
            .enumerate_sinks(SINK_TIMEOUT)
            .map_err(Error::Sink)
            .and_then(|found| {
                *self.published.sinks.write() = found.into();
                self.stale_sinks = false;
                self.rebind(Some(at), None)
            });
        match back {
            Ok(()) if self.playing && self.output.is_none() => {
                tracing::debug!(
                    "the sink list moved while it was surveyed; the row waits for the next survey"
                );
            }
            Ok(()) => {
                tracing::info!("the graph is back; the row plays on from where it was heard");
                self.unbound = None;
                self.graph_lost = None;
            }
            Err(error) if since.elapsed() < GRAPH_BACK_WITHIN => {
                tracing::debug!(%error, "the graph is not back yet");
                self.transport = TransportState::Loading;
            }
            Err(error) => {
                self.graph_lost = None;
                self.fail(error);
            }
        }
    }

    pub(super) fn collect_faults(&mut self) {
        let playing = self.transport == TransportState::Playing;
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.note_the_overs();

        let mut seen = 0;
        while let Some(fault) = output.monitor.next_fault() {
            match fault {
                RtFault::Underrun => seen += 1,
                RtFault::Dropped { count } => seen += u64::from(count),
            }
        }
        let missing = output.monitor.went_without();

        if !playing || output.ended {
            return;
        }
        output.status.went_without = output.status.went_without.saturating_add(missing);
        if seen == 0 {
            return;
        }
        output.status.underruns = output.status.underruns.saturating_add(seen);
        self.missing_untold = self.missing_untold.saturating_add(missing);
        if self
            .underruns_told_at
            .is_some_and(|told| told.elapsed() < UNDERRUNS_TOLD_EVERY)
        {
            return;
        }
        self.underruns_told_at = Some(Instant::now());
        let missing = mem::replace(&mut self.missing_untold, Frames::ZERO);
        self.emit(Event::Underrun { missing });
    }

    pub(super) fn poll_stream(&mut self) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let node = output.sink;

        let Some(stream) = output.stream.as_ref() else {
            return;
        };
        output.status.latency = stream.latency();
        output.status.holds_seconds_downstream = stream.downstream() == Downstream::Seconds;
        output.status.graph_rate = stream.graph_rate();
        output.status.hears(output.plan.mode);
        let events = Self::drain_events(output);

        let mut renegotiated = None;
        let mut failed = None;
        let mut refused = None;

        for event in events {
            match event {
                StreamEvent::FormatChanged { spec, words } => {
                    output.format_owed_since = None;
                    output.status.words = Some(words);
                    if spec != output.plan.stream {
                        renegotiated = Some(spec);
                    }
                }
                StreamEvent::VolumeChanged(volume) => {
                    output.status.stream_volume = volume;
                    output.status.hears(output.plan.mode);
                }
                StreamEvent::Drained if output.tail == Tail::Asked => output.tail = Tail::Played,
                StreamEvent::Drained => {}
                StreamEvent::StateChanged {
                    from,
                    to: StreamState::Failed,
                } => failed = Some(from),
                StreamEvent::Refused { from, op, status } => refused = Some((from, op, status)),
                StreamEvent::StateChanged { .. } => {}
            }
        }

        let format_went_unanswered = output.reshaped_format_went_unanswered();

        if let Some((previous, op, status)) = refused {
            self.fail(Error::Sink(resonate_audio::Error::StreamRefused {
                node,
                previous,
                op,
                status,
            }));
            return;
        }
        if let Some(previous) = failed {
            self.fail(Error::Sink(resonate_audio::Error::StreamFailed {
                node,
                previous,
            }));
            return;
        }
        if let Some(spec) = renegotiated {
            self.downgrade(spec);
            return;
        }
        if format_went_unanswered {
            tracing::warn!(
                "the graph never settled the format a reshaped stream asked for; it is reopened"
            );
            let at = self.position();
            if let Err(error) = self.rebind_on_a_new_stream(Some(at), None) {
                self.fail(error);
            }
        }
    }

    fn drain_events(output: &mut Output) -> Drained {
        let mut events: Drained = output.heard_while_kept.drain(..).collect();
        loop {
            let Some(stream) = output.stream.as_ref() else {
                return events;
            };
            match stream.events().try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => return events,
                Err(TryRecvError::Disconnected) => {
                    if !output.deaf {
                        tracing::warn!("the sink stream stopped reporting; nothing is on its loop");
                    }
                    output.deaf = true;
                    return events;
                }
            }
        }
    }

    fn downgrade(&mut self, negotiated: StreamSpec) {
        let Some(requested) = self.output.as_ref().map(|output| output.plan.stream) else {
            return;
        };

        self.renegotiations = self.renegotiations.saturating_add(1);
        if self.renegotiations > MAX_RENEGOTIATIONS {
            let Some(track) = self.track.as_ref().map(|track| track.id) else {
                return;
            };
            self.fail(Error::Renegotiation {
                track,
                requested,
                negotiated,
                attempts: self.renegotiations,
            });
            return;
        }

        tracing::info!(
            %requested,
            %negotiated,
            "the graph negotiated a different format; rebuilding around it"
        );
        let at = self.position();
        if let Err(error) = self.rebind(Some(at), Some(negotiated)) {
            self.fail(error);
        }
    }

    pub(super) fn settle(&mut self) {
        let at_a_change;
        let finished = {
            let Some(output) = self.output.as_mut() else {
                return;
            };
            at_a_change = output.ends_at_a_change;
            if !output.ended {
                return;
            }
            if !self.playing {
                output.silent_until = None;
                return;
            }

            if output.buffered() > 0 {
                output.silent_until = None;
                false
            } else {
                output.ask_to_drain();
                output.tail_is_out()
            }
        };

        if self.transport == TransportState::Playing {
            self.transport = TransportState::Draining;
        }
        if !finished {
            return;
        }
        if at_a_change {
            if let Err(error) = self.reopen_at_the_change() {
                self.fail(error);
            }
            return;
        }
        self.failures = 0;
        if let Err(error) = self.skip(true) {
            tracing::warn!(%error, "could not advance past the finished track");
            self.fail(error);
        }
    }

    fn parked_for_a_device(&mut self, error: Error) -> Result<()> {
        use resonate_audio::Error as Sink;

        if matches!(
            error,
            Error::Sink(Sink::Disconnected | Sink::OpenUnanswered)
        ) && self.graph_lost.is_none()
            && self.track.is_some()
            && !self.graph_lost_again()
        {
            self.graph_lost = Some(Instant::now());
        }

        let the_graph_is_away = self.graph_lost.is_some()
            && matches!(
                error,
                Error::Sink(
                    Sink::Disconnected
                        | Sink::OpenUnanswered
                        | Sink::LoopStopped
                        | Sink::Backend {
                            op: resonate_audio::Operation::Connect,
                            ..
                        }
                )
            );
        if the_graph_is_away && self.track.is_some() {
            tracing::warn!(%error, "the graph is away; the row waits for it to come back");
            let at = self.unbound.unwrap_or_else(|| self.heard_position());
            if let Some(output) = self.output.as_mut() {
                output.close();
            }
            self.output = None;
            self.unbound = Some(at);
            self.transport = if self.playing {
                TransportState::Loading
            } else {
                TransportState::Paused
            };
            return Ok(());
        }

        let had_a_stream = match &error {
            Error::Sink(Sink::NoSink) => false,
            Error::Sink(
                Sink::SinkGone { .. } | Sink::StreamFailed { .. } | Sink::StreamRefused { .. },
            ) => true,
            _ => return Err(error),
        };
        let Some(track) = self.track.as_ref().map(|track| track.id) else {
            return Err(error);
        };
        if had_a_stream {
            let again = self
                .device_last_lost
                .is_some_and(|last| last.elapsed() < GRAPH_BACK_WITHIN);
            if again {
                return Err(error);
            }
            self.device_last_lost = Some(Instant::now());
        }

        tracing::warn!(%error, "the device went; the row waits for one to play on");
        let at = self.unbound.unwrap_or_else(|| self.heard_position());
        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;
        self.unbound = Some(at);
        self.transport = if self.playing {
            TransportState::Loading
        } else {
            TransportState::Paused
        };
        self.waiting_for_a_device = true;
        self.stale_sinks = true;
        self.emit(Event::Waiting { track, error });
        Ok(())
    }

    pub(super) fn bind_the_row_waiting_for_a_device(&mut self) {
        if !self.waiting_for_a_device {
            return;
        }
        let at = self
            .unbound
            .filter(|_| self.track.is_some() && self.output.is_none());
        let Some(at) = at else {
            self.waiting_for_a_device = false;
            return;
        };
        if !self.playing {
            return;
        }

        match self.rebind(Some(at), None) {
            Ok(()) if self.output.is_none() => {
                tracing::debug!(
                    "the sink list moved while it was surveyed; the row waits for the next survey"
                );
                self.waiting_for_a_device = true;
            }
            Ok(()) => {
                tracing::info!("a device is there again; the row plays on from where it was heard");
                self.unbound = None;
                self.waiting_for_a_device = false;
            }
            Err(Error::Sink(resonate_audio::Error::NoSink)) => {
                tracing::debug!("still no device to play through");
                self.transport = TransportState::Loading;
            }
            Err(error) => {
                self.waiting_for_a_device = false;
                self.fail(error);
            }
        }
    }

    pub(super) fn fail(&mut self, error: Error) {
        self.failing(error, Waiting::ForTheBoundary);
    }

    pub(super) fn failing(&mut self, error: Error, waiting: Waiting) {
        if !self.boundaries.is_empty() {
            let entering = self.track.as_ref().map(|track| track.id);
            if entering.is_some() && error.track() == entering {
                if waiting == Waiting::ForTheBoundary
                    && (self.drop_what_lies_past_the_boundary()
                        || self.the_leaving_tail_waits_in_the_chain())
                {
                    self.failing_past_the_boundary = Some(error);
                    return;
                }
                if !self.pass_every_boundary() {
                    tracing::warn!(%error, "a row failed after the queue had moved past it");
                    return;
                }
            } else {
                self.take_the_boundary_back();
            }
        }
        let mut error = error;
        loop {
            error = match self.parked_for_a_device(error) {
                Ok(()) => return,
                Err(error) => error,
            };
            tracing::error!(%error, "playback failed");
            match self.track.as_ref().map(|track| track.id).or(error.track()) {
                Some(track) => self.emit(Event::Failed { track, error }),
                None => drop(error),
            }

            self.failures = self.failures.saturating_add(1);
            if self.failures > self.queue.len() {
                self.finish_the_queue();
                return;
            }
            match self.skip(false) {
                Ok(()) => return,
                Err(next) => error = next,
            }
        }
    }
}
