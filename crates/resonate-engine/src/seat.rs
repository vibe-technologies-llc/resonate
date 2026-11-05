use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_audio::Moment;
use resonate_core::{Frames, QueueStamp, Resumable, Resumption, Span};

use crate::{
    Command, HeardAt, HeardClock, Placement, PlaybackState, Player, QueueItem, RepeatMode, Result,
    Unclaimed, resumption_of,
};

pub const TOO_NEAR_THE_END_TO_CHANGE_WHAT_FOLLOWS: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sounding {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatHeard {
    pub sounding: Sounding,
    pub row: Option<QueueStamp>,
    pub position: Option<Duration>,
    pub at: Moment,
    pub holds_seconds_downstream: bool,
}

#[derive(Clone)]
struct Before {
    repeat: RepeatMode,
    resumption: Resumption,
}

pub struct Seat {
    player: Arc<Player>,
    clock: HeardClock,
    before: Mutex<Option<Before>>,
}

fn row_stamp(row: &Resumable) -> QueueStamp {
    QueueStamp::of([(&row.location, row.span)])
}

fn seconds_of(frames: Frames, rate: u32) -> Duration {
    Duration::from_secs_f64(frames.get() as f64 / f64::from(rate.max(1)))
}

impl Seat {
    pub fn on(player: Arc<Player>) -> Self {
        Self {
            player,
            clock: HeardClock::default(),
            before: Mutex::new(None),
        }
    }

    pub const fn player(&self) -> &Arc<Player> {
        &self.player
    }

    pub fn stamp_of(row: &Resumable) -> QueueStamp {
        row_stamp(row)
    }

    pub fn heard(&self) -> SeatHeard {
        let state = self.player.state();
        let sounding = match state.playback {
            PlaybackState::Playing | PlaybackState::Buffering => Sounding::Playing,
            PlaybackState::Paused => Sounding::Paused,
            PlaybackState::Idle | PlaybackState::Stopped => Sounding::Stopped,
        };
        let holds_seconds_downstream = state
            .output
            .as_ref()
            .is_some_and(|output| output.holds_seconds_downstream);

        if sounding == Sounding::Playing
            && let Some(heard) = self.player.heard_at()
        {
            return SeatHeard {
                sounding,
                row: Some(heard.row),
                position: Some(seconds_of(heard.frame, heard.rate.hz())),
                at: heard.at,
                holds_seconds_downstream,
            };
        }

        let queued = self.player.queued();
        let row = state
            .queue_position
            .and_then(|row| queued.rows.get(row))
            .map(|item| QueueStamp::of([(&item.location, item.span)]));
        let position = state
            .current
            .as_ref()
            .map(|track| seconds_of(track.position, track.source.rate.hz()));

        SeatHeard {
            sounding,
            row,
            position,
            at: Moment::now(),
            holds_seconds_downstream,
        }
    }

    fn remember_what_was_here(&self) {
        let mut before = self.before.lock();
        if before.is_none() {
            let state = self.player.state();
            *before = Some(Before {
                repeat: state.repeat,
                resumption: resumption_of(&state, &self.player.queued()),
            });
        }
    }

    fn resumed_at(rows: &[Resumable], at: Frames) -> Resumption {
        Resumption {
            rows: rows.to_vec(),
            order: (0..rows.len()).collect(),
            row: 0,
            at,
            shuffle: false,
            next: None,
        }
    }

    pub fn take_up(
        &self,
        rows: &[Resumable],
        position: Duration,
        at: Moment,
        sounding: bool,
    ) -> Result<bool> {
        let Some(first) = rows.first() else {
            return Ok(false);
        };

        self.remember_what_was_here();
        self.player.send(Command::SetRepeat(RepeatMode::Off))?;
        self.player
            .request(Command::Resume(Self::resumed_at(rows, Frames::ZERO)))?
            .wait()?;
        let Some(rate) = self
            .player
            .state()
            .current
            .as_ref()
            .map(|track| track.source.rate)
        else {
            return Ok(false);
        };

        let now = Moment::now();
        let elapsed = if sounding {
            Duration::from_secs_f64(now.frames_since(at, rate).max(0.0) / f64::from(rate.hz()))
        } else {
            Duration::ZERO
        };
        let here = position.saturating_add(elapsed);
        self.player
            .request(Command::Resume(Self::resumed_at(
                rows,
                Frames::from_duration(here, rate),
            )))?
            .wait()?;

        if sounding {
            self.clock.tell(Some(HeardAt {
                row: row_stamp(first),
                frame: Frames::from_duration(here, rate),
                at: now,
                rate,
            }));
            self.player
                .send(Command::KeepInStepWith(self.clock.clone()))?;
            self.player.request(Command::Play)?.wait()?;
        }

        Ok(true)
    }

    pub fn keep_ahead(&self, ahead: &[Resumable]) -> Result<bool> {
        let state = self.player.state();
        let Some(playing) = state.queue_position else {
            return Ok(false);
        };
        if let Some(track) = state.current.as_ref()
            && let Some(length) = track.duration
        {
            let rate = track.source.rate.hz();
            let left = seconds_of(
                Frames(length.get().saturating_sub(track.position.get())),
                rate,
            );
            if left < TOO_NEAR_THE_END_TO_CHANGE_WHAT_FOLLOWS {
                return Ok(false);
            }
        }

        let queued = self.player.queued();
        let standing: Vec<QueueStamp> = queued
            .rows
            .iter()
            .skip(playing + 1)
            .map(|item| QueueStamp::of([(&item.location, item.span)]))
            .collect();
        let alike = standing
            .iter()
            .zip(ahead)
            .take_while(|(standing, wanted)| **standing == row_stamp(wanted))
            .count();
        let first_unlike = playing + 1 + alike;

        if standing.len() > alike {
            self.player
                .request(Command::Remove(Span::between(
                    first_unlike,
                    queued.rows.len() - 1,
                )))?
                .wait()?;
        }

        let rest = ahead.get(alike..).unwrap_or_default();
        if rest.is_empty() {
            return Ok(true);
        }
        let mut minting = Unclaimed::beside(queued.rows.iter().map(AsRef::as_ref));
        let items = rest
            .iter()
            .map(|row| QueueItem {
                id: minting.mint(),
                location: row.location.clone(),
                span: row.span,
            })
            .collect();
        self.player
            .request(Command::Insert {
                items,
                at: Placement::At(first_unlike),
                play: false,
            })?
            .wait()?;

        Ok(true)
    }

    pub fn nudge(&self, position: Duration, at: Moment) -> Result<()> {
        let Some(heard) = self.player.heard_at() else {
            return Ok(());
        };

        self.clock.tell(Some(HeardAt {
            row: heard.row,
            frame: Frames::from_duration(position, heard.rate),
            at,
            rate: heard.rate,
        }));

        self.player.send(Command::Nudge(self.clock.clone()))
    }

    pub fn seek(&self, to: Duration) -> Result<()> {
        let Some(rate) = self
            .player
            .state()
            .current
            .as_ref()
            .map(|track| track.source.rate)
        else {
            return Ok(());
        };

        self.player
            .send(Command::Seek(Frames::from_duration(to, rate)))
    }

    pub fn pause(&self) -> Result<()> {
        self.player.send(Command::Pause)
    }

    pub fn play(&self) -> Result<()> {
        self.player.send(Command::Play)
    }

    pub fn put_back(&self) -> Result<()> {
        let Some(before) = self.before.lock().take() else {
            return Ok(());
        };

        self.player.send(Command::Stop)?;
        self.player.send(Command::SetRepeat(before.repeat))?;
        if before.resumption.is_empty() {
            return self.player.send(Command::Clear);
        }

        self.player.send(Command::Resume(before.resumption))
    }

    pub fn is_taking_part(&self) -> bool {
        self.before.lock().is_some()
    }

    pub fn nanos_of(position: Duration) -> u64 {
        u64::try_from(position.as_nanos()).unwrap_or(u64::MAX)
    }
}
