use std::{
    sync::{Arc, Weak},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;
use resonate_core::ZoneId;
use resonate_engine::{Event, PlaybackState, QueueRows};

use crate::{Error, MobilePlayer, Result, zoning::Zoned};

const COUNTED_EVERY_WHILE_PLAYING: Duration = Duration::from_secs(1);
const COUNTED_EVERY_OTHERWISE: Duration = Duration::from_secs(2);

#[uniffi::export(callback_interface)]
pub trait PlayerWatch: Send + Sync {
    fn moved(&self);
}

#[derive(Default)]
pub(crate) struct Watching {
    watch: Mutex<Option<Arc<dyn PlayerWatch>>>,
}

impl Watching {
    pub(crate) fn tell(&self) {
        let watch = self.watch.lock().clone();
        if let Some(watch) = watch {
            watch.moved();
        }
    }
}

const fn changes_what_is_shown(event: &Event) -> bool {
    matches!(
        event,
        Event::TrackStarted(_)
            | Event::TrackFinished(_)
            | Event::QueueFinished
            | Event::OutputChanged(_)
            | Event::Failed { .. }
            | Event::Waiting { .. }
            | Event::CommandFailed { .. }
    )
}

#[uniffi::export]
impl MobilePlayer {
    pub fn watch_player(&self, watch: Box<dyn PlayerWatch>) {
        *self.services.watching.watch.lock() = Some(Arc::from(watch));
    }
}

impl MobilePlayer {
    pub(crate) fn take_in(&self, zoned: &Zoned, events: Vec<Event>, rows: &QueueRows) {
        for event in events {
            if let Some(told) = crate::failure::waiting_told(&event) {
                *zoned.waiting.lock() = Some(told);
            } else if let Some(told) = self.failure_told(rows, event) {
                *zoned.failure.lock() = Some(told);
            }
        }
    }

    pub(crate) fn absorbed(
        &self,
        zone: ZoneId,
        first: Event,
        rest: &Receiver<Event>,
    ) -> Result<()> {
        let mut events = vec![first];
        events.extend(rest.try_iter());

        let moved = events.iter().any(changes_what_is_shown);
        let zoned = self.playing.zoned(zone)?;
        let rows = self.playing.player_of(zone)?.queued().rows;

        self.take_in(&zoned, events, &rows);
        if moved {
            self.services.watching.tell();
        }
        Ok(())
    }

    pub(crate) fn counted_now(&self, zone: ZoneId) -> Result<PlaybackState> {
        let player = self.playing.player_of(zone)?;
        let (state, rows) = (player.state(), player.queued().rows);

        self.count_listening(zone, &state, &rows)?;
        Ok(state.playback)
    }
}

pub(crate) fn keep(player: &Arc<MobilePlayer>, zone: ZoneId) {
    let Ok(events) = player
        .playing
        .player_of(zone)
        .map(|held| held.events().clone())
    else {
        return;
    };
    let player = Arc::downgrade(player);
    let started = thread::Builder::new()
        .name("resonate-watch".into())
        .spawn(move || watch(&player, zone, &events));
    if let Err(error) = started {
        tracing::warn!(%error, "listening could not be counted in the background");
    }
}

pub(crate) fn watch(player: &Weak<MobilePlayer>, zone: ZoneId, events: &Receiver<Event>) {
    let mut wait = COUNTED_EVERY_OTHERWISE;
    let mut failing = false;
    loop {
        let arrived = events.recv_timeout(wait);
        let Some(player) = player.upgrade() else {
            return;
        };
        let absorbed = match arrived {
            Ok(first) => player.absorbed(zone, first, events),
            Err(RecvTimeoutError::Timeout) => Ok(()),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match absorbed.and_then(|()| player.counted_now(zone)) {
            Ok(playback) => {
                failing = false;
                wait = if playback == PlaybackState::Playing {
                    COUNTED_EVERY_WHILE_PLAYING
                } else {
                    COUNTED_EVERY_OTHERWISE
                };
            }
            Err(Error::Closed | Error::NoZone { .. }) => return,
            Err(error) => {
                if !failing {
                    tracing::warn!(%error, "a listen could not be counted");
                }
                failing = true;
            }
        }
    }
}
