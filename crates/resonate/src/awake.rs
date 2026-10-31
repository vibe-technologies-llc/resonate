use std::{
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use keepawake::{Builder, KeepAwake};
use resonate_engine::{PlaybackState, Player};

const LOOKED_EVERY: Duration = Duration::from_secs(1);
const REASON: &str = "Playing music";
const THREAD: &str = "resonate-awake";

pub(crate) struct Awake {
    stopping: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

pub(crate) fn start(players: impl Fn() -> Vec<Arc<Player>> + Send + 'static) -> Awake {
    let (stopping, stopped) = mpsc::channel();
    let thread = thread::Builder::new()
        .name(THREAD.to_owned())
        .spawn(move || watch(&players, &stopped))
        .map_err(|error| tracing::warn!(%error, "the machine may sleep while a track plays"))
        .ok();
    Awake { stopping, thread }
}

impl Drop for Awake {
    fn drop(&mut self) {
        let _ = self.stopping.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn sounding(playback: PlaybackState) -> bool {
    matches!(playback, PlaybackState::Playing | PlaybackState::Buffering)
}

fn watch(players: &dyn Fn() -> Vec<Arc<Player>>, stopped: &mpsc::Receiver<()>) {
    let mut held: Option<KeepAwake> = None;
    while stopped.recv_timeout(LOOKED_EVERY) == Err(mpsc::RecvTimeoutError::Timeout) {
        let wanted = players()
            .iter()
            .any(|player| sounding(player.state().playback));
        match (wanted, held.is_some()) {
            (true, false) => held = taken(),
            (false, true) => held = None,
            (true, true) | (false, false) => {}
        }
    }
}

fn taken() -> Option<KeepAwake> {
    Builder::default()
        .display(true)
        .idle(true)
        .reason(REASON)
        .create()
        .map_err(|error| tracing::warn!(%error, "the machine may sleep while a track plays"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_transport_that_means_to_sound_keeps_the_machine_awake() {
        assert!(sounding(PlaybackState::Playing));
        assert!(sounding(PlaybackState::Buffering));
        assert!(!sounding(PlaybackState::Paused));
        assert!(!sounding(PlaybackState::Stopped));
        assert!(!sounding(PlaybackState::Idle));
    }
}
