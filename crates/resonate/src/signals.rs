use std::{
    process,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Sender, TrySendError};
use resonate_engine::{Command, Player};
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGTERM},
    iterator::{Handle, Signals},
};

const TERMINATED_BY: i32 = 128;

const ASKING_TO_LEAVE: [i32; 3] = [SIGINT, SIGTERM, SIGHUP];

const DRAINS_WITHIN: Duration = Duration::from_secs(5);

const SILENCED_AFTER: Duration = Duration::from_secs(1);

pub(crate) fn quit_when_told(asked_to_quit: Sender<()>, player: Arc<Player>) {
    let watched = match Signals::new(ASKING_TO_LEAVE) {
        Ok(watched) => watched,
        Err(error) => {
            tracing::warn!(%error, "no signal watch; an interrupt will not drain what is playing");
            return;
        }
    };

    let spawned = thread::Builder::new()
        .name("resonate-signals".to_owned())
        .spawn(move || relay(watched, &asked_to_quit, &player));

    if spawned.is_err() {
        tracing::warn!("no signal thread; an interrupt will not drain what is playing");
    }
}

fn relay(mut watched: Signals, asked_to_quit: &Sender<()>, player: &Arc<Player>) {
    let mut told = false;
    for signal in watched.forever() {
        heard(signal);
        if told {
            tracing::warn!(
                signal,
                "a second signal; leaving without closing the stream"
            );
            leave(signal);
        }
        told = true;

        match asked_to_quit.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {
                tracing::info!(signal, "asked to quit; draining what is playing");
                leave_anyway(signal, Arc::clone(player));
            }
            Err(TrySendError::Disconnected(())) => {
                leave(signal);
            }
        }
    }
}

fn leave_anyway(signal: i32, player: Arc<Player>) {
    let spawned = thread::Builder::new()
        .name("resonate-farewell".to_owned())
        .spawn(move || {
            thread::sleep(SILENCED_AFTER);
            tracing::warn!(signal, "the drain is taking too long; silencing the graph");
            if let Err(error) = player.send(Command::Stop) {
                tracing::debug!(%error, "the engine had already stopped");
            }

            thread::sleep(DRAINS_WITHIN.saturating_sub(SILENCED_AFTER));
            tracing::warn!(signal, "the drain did not finish; leaving without it");
            leave(signal);
        });

    if spawned.is_err() {
        tracing::warn!("no farewell thread; a front end that does not answer will play on");
    }
}

pub(crate) struct Interrupting {
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Interrupting {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::debug!("the signal watch over a pass ended in a panic");
        }
    }
}

pub(crate) fn cancel_when_told(cancel: impl Fn() + Send + 'static) -> Option<Interrupting> {
    cancelling_on(ASKING_TO_LEAVE, cancel, |signal| leave(signal))
}

fn cancelling_on(
    signals: impl IntoIterator<Item = i32>,
    cancel: impl Fn() + Send + 'static,
    leaving: impl Fn(i32) + Send + 'static,
) -> Option<Interrupting> {
    let watched = match Signals::new(signals) {
        Ok(watched) => watched,
        Err(error) => {
            tracing::warn!(%error, "no signal watch; an interrupt will stop the pass mid-file");
            return None;
        }
    };
    let handle = watched.handle();

    let spawned = thread::Builder::new()
        .name("resonate-interrupt".to_owned())
        .spawn(move || stop_the_pass(watched, &cancel, &leaving));
    match spawned {
        Ok(thread) => Some(Interrupting {
            handle,
            thread: Some(thread),
        }),
        Err(error) => {
            tracing::warn!(%error, "no signal thread; an interrupt will stop the pass mid-file");
            None
        }
    }
}

#[cfg(feature = "online")]
pub(crate) fn leave_when_told() -> Option<Interrupting> {
    let watched = match Signals::new(ASKING_TO_LEAVE) {
        Ok(watched) => watched,
        Err(error) => {
            tracing::warn!(%error, "no signal watch; an interrupt may leave the terminal unechoed");
            return None;
        }
    };
    let handle = watched.handle();

    let spawned = thread::Builder::new()
        .name("resonate-interrupt".to_owned())
        .spawn(move || {
            let mut watched = watched;
            if let Some(signal) = watched.forever().next() {
                heard(signal);
                leave(signal);
            }
        });
    match spawned {
        Ok(thread) => Some(Interrupting {
            handle,
            thread: Some(thread),
        }),
        Err(error) => {
            tracing::warn!(%error, "no signal thread; an interrupt may leave the terminal unechoed");
            None
        }
    }
}

fn stop_the_pass(mut watched: Signals, cancel: &impl Fn(), leaving: &impl Fn(i32)) {
    let mut told = false;
    for signal in watched.forever() {
        heard(signal);
        if told {
            tracing::warn!(
                signal,
                "a second signal; leaving without finishing the file in hand"
            );
            leaving(signal);
            return;
        }
        told = true;
        told!("stopping once the file in hand is finished; a second interrupt leaves at once");
        cancel();
    }
}

fn heard(signal: i32) {
    if signal == SIGHUP {
        crate::said::the_terminal_hung_up();
    }
}

const fn exit_code_after(signal: i32) -> i32 {
    TERMINATED_BY.saturating_add(signal)
}

fn leave(signal: i32) -> ! {
    crate::input::restore_the_terminal();
    process::exit(exit_code_after(signal))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };

    use parking_lot::Mutex;
    use signal_hook::{
        consts::{SIGUSR1, SIGUSR2},
        low_level::raise,
    };

    use super::*;

    const WAITS_AT_MOST: Duration = Duration::from_secs(5);

    fn waited_for(mut ready: impl FnMut() -> bool, what: &str) {
        let deadline = Instant::now() + WAITS_AT_MOST;
        while !ready() {
            assert!(Instant::now() < deadline, "{what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_process_ended_by_a_signal_exits_with_128_and_the_signal() {
        assert_eq!(exit_code_after(SIGINT), 130);
        assert_eq!(exit_code_after(SIGHUP), 129);
        assert_eq!(exit_code_after(SIGTERM), 143);
        assert_eq!(exit_code_after(i32::MAX), i32::MAX);
    }

    #[test]
    fn the_first_signal_cancels_the_pass_and_a_second_leaves_with_its_number() {
        let cancelled = Arc::new(AtomicUsize::new(0));
        let left_with = Arc::new(Mutex::new(Vec::new()));
        let counting = Arc::clone(&cancelled);
        let leaving = Arc::clone(&left_with);

        let interrupting = cancelling_on(
            [SIGUSR1],
            move || {
                counting.fetch_add(1, Ordering::Relaxed);
            },
            move |signal| leaving.lock().push(exit_code_after(signal)),
        )
        .expect("a watch over the signal");

        raise(SIGUSR1).expect("a signal raised");
        waited_for(
            || cancelled.load(Ordering::Relaxed) == 1,
            "the first signal never cancelled the pass",
        );
        assert!(
            left_with.lock().is_empty(),
            "the first signal left the process"
        );

        raise(SIGUSR1).expect("a signal raised");
        waited_for(
            || !left_with.lock().is_empty(),
            "the second signal never left",
        );
        drop(interrupting);

        assert_eq!(*left_with.lock(), [128 + SIGUSR1]);
        assert_eq!(cancelled.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_watch_over_a_pass_that_ends_without_a_signal_lets_go_of_its_thread() {
        let cancelled = Arc::new(AtomicUsize::new(0));
        let counting = Arc::clone(&cancelled);

        let interrupting = cancelling_on(
            [SIGUSR2],
            move || {
                counting.fetch_add(1, Ordering::Relaxed);
            },
            |_| {},
        )
        .expect("a watch over the signal");
        drop(interrupting);

        assert_eq!(cancelled.load(Ordering::Relaxed), 0);
    }
}
