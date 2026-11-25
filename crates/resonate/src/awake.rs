use std::{
    fmt::Display,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use keepawake::{Builder, KeepAwake};
use resonate_engine::{PlaybackState, Player};

const LOOKED_EVERY: Duration = Duration::from_secs(1);
const REASON: &str = "Playing music";
const THREAD: &str = "resonate-awake";
const HOLDS_THE_DISPLAY_TOO: bool = cfg!(windows);

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

struct Keeping<H> {
    held: Option<H>,
    refused: bool,
}

impl<H> Default for Keeping<H> {
    fn default() -> Self {
        Self {
            held: None,
            refused: false,
        }
    }
}

impl<H> Keeping<H> {
    fn toward<E: Display>(
        &mut self,
        wanted: bool,
        take: impl FnOnce() -> Result<H, E>,
    ) -> Option<E> {
        match (wanted, self.held.is_some()) {
            (true, false) => match take() {
                Ok(held) => {
                    self.held = Some(held);
                    self.refused = false;
                    None
                }
                Err(error) if self.refused => {
                    tracing::debug!(%error, "the machine still may not be kept awake");
                    None
                }
                Err(error) => {
                    self.refused = true;
                    Some(error)
                }
            },
            (false, true) => {
                self.held = None;
                None
            }
            (false, false) => {
                self.refused = false;
                None
            }
            (true, true) => None,
        }
    }
}

fn watch(players: &dyn Fn() -> Vec<Arc<Player>>, stopped: &mpsc::Receiver<()>) {
    let mut keeping = Keeping::default();
    while stopped.recv_timeout(LOOKED_EVERY) == Err(mpsc::RecvTimeoutError::Timeout) {
        let wanted = players()
            .iter()
            .any(|player| sounding(player.state().playback));
        if let Some(error) = keeping.toward(wanted, taken) {
            tracing::warn!(%error, "the machine may sleep while a track plays");
        }
    }
}

fn taken() -> keepawake::Result<KeepAwake> {
    Builder::default()
        .display(HOLDS_THE_DISPLAY_TOO)
        .idle(true)
        .reason(REASON)
        .create()
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

    #[test]
    fn a_refusal_is_warned_of_once_however_often_it_is_retried() {
        let mut keeping = Keeping::<()>::default();
        let mut attempts = 0;
        let mut warnings = 0;

        for _ in 0..10 {
            let refused = keeping.toward(true, || {
                attempts += 1;
                Err::<(), _>("refused")
            });

            warnings += usize::from(refused.is_some());
        }

        assert_eq!(attempts, 10);
        assert_eq!(warnings, 1);
    }

    #[test]
    fn a_refusal_is_warned_of_again_once_the_machine_was_kept_or_no_longer_wanted() {
        let mut keeping = Keeping::<()>::default();

        assert!(keeping.toward(true, || Err::<(), _>("refused")).is_some());
        assert!(keeping.toward(true, || Err::<(), _>("refused")).is_none());
        assert!(keeping.toward(false, || Err::<(), _>("refused")).is_none());
        assert!(keeping.toward(true, || Err::<(), _>("refused")).is_some());
        assert!(keeping.toward(true, || Ok::<_, &str>(())).is_none());
        assert!(keeping.toward(false, || Ok::<_, &str>(())).is_none());
        assert!(keeping.toward(true, || Err::<(), _>("refused")).is_some());
    }

    #[test]
    fn a_hold_is_made_once_and_dropped_when_no_longer_wanted() {
        let mut keeping = Keeping::<u8>::default();
        let mut takes = 0;

        for wanted in [true, true, true, false, false, true] {
            keeping.toward(wanted, || {
                takes += 1;
                Ok::<_, &str>(0)
            });
        }

        assert_eq!(takes, 2);
        assert!(keeping.held.is_some());
    }
}
