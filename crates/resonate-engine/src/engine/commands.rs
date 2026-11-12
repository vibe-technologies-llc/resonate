use std::{
    mem,
    time::{Duration, Instant},
};

use crossbeam_channel::{TryRecvError, TrySendError};
use resonate_core::{Frames, Span};

use super::{
    Answer, Answering, Deferred, EVENTS_OWED_AT_MOST, Engine, Following, OPENING_ANSWERED_WITHIN,
    ReadBy, Retuning, levelled,
};
use crate::{
    Command, CommandKind, Error, Event, Reply, Request, Result, Sleeping, Stepping,
    queue::{self},
};

impl Engine {
    pub(super) fn take_commands(&mut self) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(request) => self.dispatch(request),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    pub(super) fn emit(&mut self, event: Event) {
        self.hand_over_what_is_owed();
        if !self.events_owed.is_empty() {
            self.owe(event);
            return;
        }
        match self.events.try_send(event) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(event)) => self.owe(event),
        }
    }

    fn owe(&mut self, event: Event) {
        if matches!(event, Event::Underrun { .. }) {
            return;
        }
        if self.events_owed.len() == EVENTS_OWED_AT_MOST {
            tracing::warn!("an engine event was dropped; nothing is draining the event channel");
            self.events_owed.pop_front();
        }
        self.events_owed.push_back(event);
    }

    pub(super) fn hand_over_what_is_owed(&mut self) {
        while let Some(event) = self.events_owed.pop_front() {
            match self.events.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => {
                    self.events_owed.push_front(event);
                    return;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.events_owed.clear();
                    return;
                }
            }
        }
    }

    fn dispatch(&mut self, request: Request) {
        let Request {
            command,
            reply,
            queue_seen,
        } = request;
        let kind = command.kind();
        let begun = self.openings_begun;
        let now = self.queue.revision();
        let outcome = match queue_seen {
            Some(seen) if seen != now => Err(Error::QueueChanged { seen, now }),
            _ => self.apply(command),
        };
        let outcome = self.carried_on_past_a_stranded_row(outcome);

        if outcome.is_ok() && self.opening.is_some() && self.openings_begun != begun {
            self.deferred.push(Deferred {
                kind,
                reply,
                since: Instant::now(),
            });
            return;
        }
        self.reply(kind, reply, outcome);
    }

    pub(super) fn answer_the_deferred(&mut self, outcome: Result<()>) {
        let mut outcome = Some(outcome);
        for Deferred { kind, reply, .. } in mem::take(&mut self.deferred) {
            let outcome = outcome.take().unwrap_or(Ok(()));
            self.reply(kind, reply, outcome);
        }
    }

    pub(super) fn answer_what_waited_on_the_opening_too_long(&mut self) {
        if self
            .deferred
            .first()
            .is_some_and(|deferred| deferred.since.elapsed() >= OPENING_ANSWERED_WITHIN)
        {
            self.answer_the_deferred(Ok(()));
        }
    }

    pub(super) fn deferred_answer_due_in(&self) -> Option<Duration> {
        self.deferred
            .first()
            .map(|deferred| OPENING_ANSWERED_WITHIN.saturating_sub(deferred.since.elapsed()))
    }

    pub(super) fn reply(&mut self, kind: CommandKind, reply: Reply, outcome: Result<()>) {
        if let Err(error) = outcome.as_ref() {
            tracing::warn!(%error, ?kind, "command rejected");
        }
        let reply = match reply {
            Reply::Answered(reply) => Answering::Outcome(reply, outcome),
            Reply::Landed(reply) => {
                self.announce_a_refusal(kind, outcome);
                Answering::Landed(reply)
            }
            Reply::Unwaited => {
                self.announce_a_refusal(kind, outcome);
                return;
            }
        };
        self.answers.push(Answer { kind, reply });
    }

    pub(super) fn carried_on_past_a_stranded_row(&mut self, outcome: Result<()>) -> Result<()> {
        match outcome {
            Err(error) if error.track().is_some() && self.is_stranded() => {
                tracing::warn!(%error, "the row could not play on after the change and is passed over");
                self.fail(error);
                Ok(())
            }
            outcome => outcome,
        }
    }

    fn is_stranded(&self) -> bool {
        self.playing
            && self.track.is_some()
            && self.output.is_none()
            && self.opening.is_none()
            && !self.waiting_for_a_device
            && self.graph_lost.is_none()
    }

    fn announce_a_refusal(&mut self, command: CommandKind, outcome: Result<()>) {
        if let Err(error) = outcome {
            self.emit(Event::CommandFailed { command, error });
        }
    }

    pub(super) fn answer(&mut self) {
        for answer in self.answers.drain(..) {
            let delivered = match answer.reply {
                Answering::Outcome(reply, outcome) => reply.try_send(outcome).is_ok(),
                Answering::Landed(reply) => reply.try_send(()).is_ok(),
            };
            if !delivered {
                tracing::debug!(kind = ?answer.kind, "nothing was waiting on the command's outcome");
            }
        }
    }

    pub(super) fn apply(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Load {
                items,
                start_at,
                autoplay,
            } => {
                self.stop();
                let loaded = self.queue.load(items, start_at).is_some();
                self.playing = autoplay && loaded;
                if loaded && autoplay {
                    let started = self.start(Frames::ZERO);
                    return self.past_what_will_not_open(started);
                }
                Ok(())
            }
            Command::KeepInStepWith(with) => {
                if self.output.is_none() {
                    self.lockstep = Some(Following {
                        with,
                        since: Instant::now(),
                    });
                    self.stepping = Stepping::Waiting;
                }
                Ok(())
            }
            Command::Nudge(with) => {
                if self.may_nudge() {
                    if let Some(output) = self.output.as_ref() {
                        output.producer.fade_out(output.fade());
                    }
                    self.nudging = Some(Following {
                        with,
                        since: Instant::now(),
                    });
                    self.stepping = Stepping::Waiting;
                }
                Ok(())
            }
            Command::Resume(resumption) => {
                self.stop();
                let at = resumption.at;
                let resumed = self.queue.restore(resumption).is_some();
                self.playing = false;
                if resumed {
                    self.start(at)?;
                }
                Ok(())
            }
            Command::Insert {
                items,
                at,
                play: true,
            } => match self.queue.insert_to_hear(items, at) {
                Some(_) => self.start_what_was_jumped_to(),
                None => Ok(()),
            },
            Command::Insert { items, at, .. } => {
                self.queue.insert(items, at);
                Ok(())
            }
            Command::Remove(rows) => self.remove(rows),
            Command::RemoveAll(runs) => self.remove_runs(&runs),
            Command::Clear => self.clear(),
            Command::Move { rows, to } => self.queue.move_rows(rows, to).ok_or(Error::NoSuchRow {
                rows: Span::between(rows.last(), to),
                len: self.queue.len(),
            }),
            Command::Order(rows) => {
                let named = rows.len();
                self.queue.reorder(&rows).ok_or(Error::NotAnOrder {
                    named,
                    len: self.queue.len(),
                })
            }
            Command::Play => self.play(),
            Command::Pause => self.pause(),
            Command::TogglePlayPause => {
                if self.playing && !self.is_stranded() {
                    self.pause()
                } else {
                    self.play()
                }
            }
            Command::Stop => {
                self.stop();
                Ok(())
            }
            Command::Seek(to) => self.seek(to),
            Command::SeekBy(delta) => {
                let now = self.position().get();
                self.take_the_boundary_back();
                let to = Frames(if delta < 0 {
                    now.saturating_sub(delta.unsigned_abs())
                } else {
                    now.saturating_add(delta.unsigned_abs())
                });
                if self.lands_past_the_end(to) {
                    return self.next();
                }
                self.seek(to)
            }
            Command::Next => self.next(),
            Command::Previous => {
                self.take_the_boundary_back();
                self.failures = 0;
                if self.restarts_the_track() {
                    self.seek(Frames::ZERO)
                } else {
                    self.skipped_by_hand();
                    self.queue.retreat().ok_or(Error::QueueEmpty)?;
                    let started = self.start(Frames::ZERO);
                    self.past_what_will_not_open(started)
                }
            }
            Command::JumpTo(item) => self.hear(item),
            Command::SetVolume(volume) => {
                self.hear_the_volume(volume);
                self.turn_the_device();
                self.retune()
            }
            Command::SetRepeat(repeat) => {
                self.queue.set_repeat(repeat);
                Ok(())
            }
            Command::SetSkipUnderRepeat(skip) => {
                self.config.skip_under_repeat = skip;
                Ok(())
            }
            Command::SetPreviousRestarts(previous) => {
                self.config.previous_restarts = previous;
                Ok(())
            }
            Command::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                Ok(())
            }
            Command::SetSmartShuffle(smart) => {
                self.config.smart_shuffle = smart;
                self.queue.set_smart_shuffle(smart);
                Ok(())
            }
            Command::SetSink(sink) => {
                self.config.sink = sink;
                if self.stays_on_the_bound_device() {
                    return Ok(());
                }
                self.rebind_where_it_stands()
            }
            Command::FollowTheFormats => {
                self.stale_sinks = true;
                self.formats_owed = true;
                Ok(())
            }
            Command::SetQuality(quality) => {
                self.forget_the_audition();
                self.config.quality = quality;
                self.rebind_where_the_plan_moves(ReadBy::Resampler)
            }
            Command::SetFilterPhase(phase) => {
                self.forget_the_audition();
                self.config.filter_phase = phase;
                self.rebind_where_the_plan_moves(ReadBy::Resampler)
            }
            Command::SetDither(dither) => {
                self.forget_the_audition();
                self.config.dither = dither;
                self.rebind_where_the_plan_moves(ReadBy::Dither)
            }
            Command::SetRestoration(restoration) => {
                self.forget_the_audition();
                self.config.restoration = restoration;
                self.rebind_where_the_plan_moves(ReadBy::PlanAlone)
            }
            Command::SetCrossfeed(crossfeed) => {
                self.config.crossfeed = crossfeed;
                self.rebind_where_the_plan_moves(ReadBy::PlanAlone)
            }
            Command::SetTruePeak(on) => {
                self.config.true_peak = on;
                if let Some(track) = self.track.as_mut() {
                    track.replay_gain = levelled(&self.config, &track.info, track.hints);
                    track.published = None;
                    track.measure_where_wanted(&self.sources, &self.config);
                }
                self.rebind_where_it_stands()
            }
            Command::SetNoiseShaping(shaping) => {
                self.forget_the_audition();
                self.config.noise_shaping = shaping;
                self.rebind_where_the_plan_moves(ReadBy::PlanAlone)
            }
            Command::SetReplayGain(mode) => {
                self.config.replay_gain = mode;
                self.re_level()
            }
            Command::SetLevelling(levelling) => {
                self.config.levelling = levelling;
                self.re_level()
            }
            Command::SetConvolution(impulse) => {
                self.config.convolution = impulse;
                self.rebind_where_the_plan_moves(ReadBy::PlanAlone)
            }
            Command::SetEqualisation(equalisation) => {
                self.config.equaliser = equalisation;
                self.retune_heard_past_the_buffer(Retuning::Equaliser)
            }
            Command::SetBitPerfect(prefer) => {
                self.config.prefer_bit_perfect = prefer;
                self.reopen_where_the_stream_moves()
            }
            Command::SetDop(marked) => {
                self.config.dop = marked;
                self.reopen_where_the_stream_moves()
            }
            Command::SetDsdLikePcm(raised) => {
                self.config.dsd_like_pcm = raised;
                self.retune()
            }
            Command::SetDeviceMute(muted) => self.mute_the_device(muted),
            Command::SetDeviceVolume(handed) => {
                self.config.device_volume = handed;
                self.retune()
            }
            Command::SetBluetoothWake(wake) => {
                self.config.bluetooth = wake;
                self.let_the_link_rest();
                Ok(())
            }
            Command::SetForceGraphRate(force) => {
                self.config.force_graph_rate = force;
                self.rebind_where_it_stands()
            }
            Command::SetBuffer(buffer) => {
                self.config.buffer = buffer;
                self.reopen_where_the_stream_moves()
            }
            Command::SleepUntil(until) => {
                self.sleep = until.map(Sleeping::set);
                self.wake_from_the_sleep_fade();
                Ok(())
            }
            Command::Audition(asked) => self.audition(asked),
            Command::Relocate(moved) => {
                self.queue.relocate(&moved);
                if let Some(track) = self.track.as_mut()
                    && let Some(to) = queue::landed_at(&track.location, &moved)
                {
                    track.location = to;
                }
                for boundary in &mut self.boundaries {
                    if let Some(to) = queue::landed_at(&boundary.leaving.location, &moved) {
                        boundary.leaving.location = to;
                    }
                }
                Ok(())
            }
        }
    }
}
