use std::{
    sync::{Arc, Weak},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;
use resonate_engine::{Event, PlaybackState, Player, QueueItem};

use crate::{Error, MobilePlayer, Result};

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
    fn tell(&self) {
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
    pub(crate) fn take_in(&self, events: Vec<Event>, rows: &[Arc<QueueItem>]) {
        for event in events {
            if let Some(told) = crate::failure::waiting_told(&event) {
                *self.waiting.lock() = Some(told);
            } else if let Some(told) = self.failure_told(rows, event) {
                *self.failure.lock() = Some(told);
            }
        }
    }

    pub(crate) fn absorbed(&self, first: Event, rest: &Receiver<Event>) {
        let mut events = vec![first];
        events.extend(rest.try_iter());

        let moved = events.iter().any(changes_what_is_shown);
        let rows = self
            .held_player(|player| player.queued().rows)
            .unwrap_or_default();

        self.take_in(events, &rows);
        if moved {
            self.services.watching.tell();
        }
    }

    fn held_player<T>(&self, read: impl FnOnce(&Player) -> T) -> Result<T> {
        let held = self.player.lock();
        Ok(read(held.as_ref().ok_or(Error::Closed)?))
    }

    pub(crate) fn counted_now(&self) -> Result<PlaybackState> {
        let (state, rows) = self.held_player(|player| {
            let state = player.state();
            (state, player.queued().rows)
        })?;

        self.count_listening(&state, &rows)?;
        Ok(state.playback)
    }
}

pub(crate) fn keep(player: &Arc<MobilePlayer>) {
    let Ok(events) = player.held_player(|held| held.events().clone()) else {
        return;
    };
    let player = Arc::downgrade(player);
    let started = thread::Builder::new()
        .name("resonate-watch".into())
        .spawn(move || watch(&player, &events));
    if let Err(error) = started {
        tracing::warn!(%error, "listening could not be counted in the background");
    }
}

pub(crate) fn watch(player: &Weak<MobilePlayer>, events: &Receiver<Event>) {
    let mut wait = COUNTED_EVERY_OTHERWISE;
    let mut failing = false;
    loop {
        let arrived = events.recv_timeout(wait);
        let Some(player) = player.upgrade() else {
            return;
        };
        match arrived {
            Ok(first) => player.absorbed(first, events),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        match player.counted_now() {
            Ok(playback) => {
                failing = false;
                wait = if playback == PlaybackState::Playing {
                    COUNTED_EVERY_WHILE_PLAYING
                } else {
                    COUNTED_EVERY_OTHERWISE
                };
            }
            Err(Error::Closed) => return,
            Err(error) => {
                if !failing {
                    tracing::warn!(%error, "a listen could not be counted");
                }
                failing = true;
            }
        }
    }
}
