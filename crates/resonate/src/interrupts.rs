use std::{
    process,
    sync::{Arc, OnceLock},
    thread,
    time::Duration,
};

use crossbeam_channel::{Sender, TrySendError};
use parking_lot::Mutex;
use resonate_engine::{Command, Player};

const TERMINATED_BY: i32 = 128;

const INTERRUPT: i32 = 2;

const DRAINS_WITHIN: Duration = Duration::from_secs(5);

const SILENCED_AFTER: Duration = Duration::from_secs(1);

type Reaction = Box<dyn FnMut() + Send>;

static REACTION: Mutex<Option<Reaction>> = Mutex::new(None);

static WATCHING: OnceLock<bool> = OnceLock::new();

#[derive(Clone)]
struct Farewell {
    silence: Arc<dyn Fn() + Send + Sync>,
    leaving: Arc<dyn Fn(i32) + Send + Sync>,
    silenced_after: Duration,
    drains_within: Duration,
}

pub(crate) type Silencing = Arc<dyn Fn() + Send + Sync>;

pub(crate) fn silencing(
    players: impl Fn() -> Vec<Arc<Player>> + Send + Sync + 'static,
) -> Silencing {
    Arc::new(move || {
        for player in players() {
            if let Err(error) = player.send(Command::Stop) {
                tracing::debug!(%error, "the engine had already stopped");
            }
        }
    })
}

impl Farewell {
    fn of(silence: Silencing) -> Self {
        Self {
            silence,
            leaving: Arc::new(|signal| leave(signal)),
            silenced_after: SILENCED_AFTER,
            drains_within: DRAINS_WITHIN,
        }
    }
}

fn watch(reaction: Reaction) -> bool {
    let watching = *WATCHING.get_or_init(start_watching);
    if watching {
        *REACTION.lock() = Some(reaction);
    }
    watching
}

fn start_watching() -> bool {
    if ctrlc::set_handler(interrupted).is_err() {
        return false;
    }
    if let Err(error) = resonate_win32::hear_console_interrupts() {
        tracing::warn!(%error, "the console's inherited interrupt suppression could not be cleared");
    }
    true
}

pub(crate) fn quit_when_told(asked_to_quit: Sender<()>, silence: Silencing) {
    let farewell = Farewell::of(silence);
    let mut told = false;
    let watching = watch(Box::new(move || {
        if told {
            tracing::warn!("a second interrupt; leaving without closing the stream");
            (farewell.leaving)(INTERRUPT);
            return;
        }
        told = true;
        match asked_to_quit.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {
                tracing::info!("asked to quit; draining what is playing");
                leave_anyway(farewell.clone());
            }
            Err(TrySendError::Disconnected(())) => (farewell.leaving)(INTERRUPT),
        }
    }));
    if !watching {
        tracing::warn!("no interrupt watch; an interrupt will not drain what is playing");
    }
}

fn leave_anyway(farewell: Farewell) {
    let spawned = thread::Builder::new()
        .name("resonate-farewell".to_owned())
        .spawn(move || {
            thread::sleep(farewell.silenced_after);
            tracing::warn!("the drain is taking too long; silencing the graph");
            (farewell.silence)();

            thread::sleep(
                farewell
                    .drains_within
                    .saturating_sub(farewell.silenced_after),
            );
            tracing::warn!("the drain did not finish; leaving without it");
            (farewell.leaving)(INTERRUPT);
        });

    if spawned.is_err() {
        tracing::warn!("no farewell thread; a front end that does not answer will play on");
    }
}

pub(crate) fn interrupted() {
    match REACTION.lock().as_mut() {
        Some(react) => react(),
        None => leave(INTERRUPT),
    }
}

pub(crate) struct Interrupting;

impl Drop for Interrupting {
    fn drop(&mut self) {
        *REACTION.lock() = None;
    }
}

pub(crate) fn cancel_when_told(cancel: impl Fn() + Send + 'static) -> Option<Interrupting> {
    let mut told = false;
    let watching = watch(Box::new(move || {
        if told {
            tracing::warn!("a second interrupt; leaving without finishing the file in hand");
            leave(INTERRUPT);
        }
        told = true;
        told!("{}", crate::words::signals::stopping_once());
        cancel();
    }));
    if watching {
        Some(Interrupting)
    } else {
        tracing::warn!("no interrupt watch; an interrupt will stop the pass mid-file");
        None
    }
}

#[cfg(feature = "online")]
pub(crate) fn leave_when_told() -> Option<Interrupting> {
    watch(Box::new(|| leave(INTERRUPT))).then_some(Interrupting)
}

const fn exit_code_after(signal: i32) -> i32 {
    TERMINATED_BY.saturating_add(signal)
}

fn leave(signal: i32) -> ! {
    crate::providers::stop_the_tools();
    crate::input::restore_the_terminal();
    process::exit(exit_code_after(signal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interrupt_exits_with_128_and_its_number() {
        assert_eq!(exit_code_after(INTERRUPT), 130);
    }
}
