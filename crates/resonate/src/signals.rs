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

#[derive(Clone)]
struct Farewell {
    silence: Arc<dyn Fn() + Send + Sync>,
    leaving: Arc<dyn Fn(i32) + Send + Sync>,
    silenced_after: Duration,
    drains_within: Duration,
}

impl Farewell {
    fn of(player: Arc<Player>) -> Self {
        Self {
            silence: Arc::new(move || {
                if let Err(error) = player.send(Command::Stop) {
                    tracing::debug!(%error, "the engine had already stopped");
                }
            }),
            leaving: Arc::new(|signal| leave(signal)),
            silenced_after: SILENCED_AFTER,
            drains_within: DRAINS_WITHIN,
        }
    }
}

pub(crate) fn quit_when_told(asked_to_quit: Sender<()>, player: Arc<Player>) {
    let watched = match Signals::new(ASKING_TO_LEAVE) {
        Ok(watched) => watched,
        Err(error) => {
            tracing::warn!(%error, "no signal watch; an interrupt will not drain what is playing");
            return;
        }
    };
    let farewell = Farewell::of(player);

    let spawned = thread::Builder::new()
        .name("resonate-signals".to_owned())
        .spawn(move || relay(watched, &asked_to_quit, &farewell));

    if spawned.is_err() {
        tracing::warn!("no signal thread; an interrupt will not drain what is playing");
    }
}

fn relay(mut watched: Signals, asked_to_quit: &Sender<()>, farewell: &Farewell) {
    let mut told = false;
    for signal in watched.forever() {
        heard(signal);
        if told {
            tracing::warn!(
                signal,
                "a second signal; leaving without closing the stream"
            );
            (farewell.leaving)(signal);
            return;
        }
        told = true;

        match asked_to_quit.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {
                tracing::info!(signal, "asked to quit; draining what is playing");
                leave_anyway(signal, farewell.clone());
            }
            Err(TrySendError::Disconnected(())) => {
                (farewell.leaving)(signal);
                return;
            }
        }
    }
}

fn leave_anyway(signal: i32, farewell: Farewell) {
    let spawned = thread::Builder::new()
        .name("resonate-farewell".to_owned())
        .spawn(move || {
            thread::sleep(farewell.silenced_after);
            tracing::warn!(signal, "the drain is taking too long; silencing the graph");
            (farewell.silence)();

            thread::sleep(
                farewell
                    .drains_within
                    .saturating_sub(farewell.silenced_after),
            );
            tracing::warn!(signal, "the drain did not finish; leaving without it");
            (farewell.leaving)(signal);
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
    leaving_on(ASKING_TO_LEAVE, |signal| leave(signal))
}

#[cfg(feature = "online")]
fn leaving_on(
    signals: impl IntoIterator<Item = i32>,
    leaving: impl Fn(i32) + Send + 'static,
) -> Option<Interrupting> {
    let watched = match Signals::new(signals) {
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
                leaving(signal);
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

    use crossbeam_channel::bounded;
    use parking_lot::Mutex;
    use signal_hook::{
        consts::{SIGALRM, SIGIO, SIGPROF, SIGUSR1, SIGUSR2, SIGVTALRM, SIGWINCH},
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

    fn farewell_recording(
        silenced: &Arc<AtomicUsize>,
        left_with: &Arc<Mutex<Vec<i32>>>,
        silenced_after: Duration,
        drains_within: Duration,
    ) -> Farewell {
        let silencing = Arc::clone(silenced);
        let leaving = Arc::clone(left_with);
        Farewell {
            silence: Arc::new(move || {
                silencing.fetch_add(1, Ordering::Relaxed);
            }),
            leaving: Arc::new(move |signal| leaving.lock().push(exit_code_after(signal))),
            silenced_after,
            drains_within,
        }
    }

    fn slow_farewell(silenced: &Arc<AtomicUsize>, left_with: &Arc<Mutex<Vec<i32>>>) -> Farewell {
        farewell_recording(
            silenced,
            left_with,
            Duration::from_secs(600),
            Duration::from_secs(600),
        )
    }

    #[test]
    fn the_first_signal_asks_to_quit_and_a_second_leaves_with_its_number() {
        let silenced = Arc::new(AtomicUsize::new(0));
        let left_with = Arc::new(Mutex::new(Vec::new()));
        let farewell = slow_farewell(&silenced, &left_with);
        let (asked_to_quit, quit_asked) = bounded(1);
        let watched = Signals::new([SIGWINCH]).expect("a watch over the signal");
        let relaying = thread::spawn(move || relay(watched, &asked_to_quit, &farewell));

        raise(SIGWINCH).expect("a signal raised");
        waited_for(
            || quit_asked.try_recv().is_ok(),
            "the first signal never asked to quit",
        );
        assert!(
            left_with.lock().is_empty(),
            "the first signal left the process"
        );

        raise(SIGWINCH).expect("a signal raised");
        relaying.join().expect("the relay returns after leaving");

        assert_eq!(*left_with.lock(), [128 + SIGWINCH]);
        assert_eq!(silenced.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_front_end_that_never_answers_is_silenced_and_then_left() {
        let silenced = Arc::new(AtomicUsize::new(0));
        let left_with = Arc::new(Mutex::new(Vec::new()));
        let farewell = farewell_recording(
            &silenced,
            &left_with,
            Duration::from_millis(20),
            Duration::from_millis(80),
        );
        let (asked_to_quit, quit_asked) = bounded(1);
        let watched = Signals::new([SIGPROF]).expect("a watch over the signal");
        thread::spawn(move || relay(watched, &asked_to_quit, &farewell));

        raise(SIGPROF).expect("a signal raised");
        waited_for(
            || silenced.load(Ordering::Relaxed) == 1,
            "the graph was never silenced",
        );
        assert!(
            left_with.lock().is_empty(),
            "the process left before its drain time was up"
        );

        waited_for(
            || !left_with.lock().is_empty(),
            "the farewell never left the process",
        );

        assert_eq!(*left_with.lock(), [128 + SIGPROF]);
        assert_eq!(silenced.load(Ordering::Relaxed), 1);
        assert!(quit_asked.try_recv().is_ok());
    }

    #[test]
    fn a_signal_with_no_front_end_listening_leaves_at_once() {
        let silenced = Arc::new(AtomicUsize::new(0));
        let left_with = Arc::new(Mutex::new(Vec::new()));
        let farewell = slow_farewell(&silenced, &left_with);
        let (asked_to_quit, quit_asked) = bounded(1);
        drop(quit_asked);
        let watched = Signals::new([SIGIO]).expect("a watch over the signal");
        let relaying = thread::spawn(move || relay(watched, &asked_to_quit, &farewell));

        raise(SIGIO).expect("a signal raised");
        relaying.join().expect("the relay returns after leaving");

        assert_eq!(*left_with.lock(), [128 + SIGIO]);
        assert_eq!(silenced.load(Ordering::Relaxed), 0);
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_signal_during_a_prompt_leaves_with_its_number() {
        let left_with = Arc::new(Mutex::new(Vec::new()));
        let leaving = Arc::clone(&left_with);

        let interrupting = leaving_on([SIGALRM], move |signal| {
            leaving.lock().push(exit_code_after(signal));
        })
        .expect("a watch over the signal");
        raise(SIGALRM).expect("a signal raised");
        waited_for(
            || !left_with.lock().is_empty(),
            "the signal never left the prompt",
        );
        drop(interrupting);

        assert_eq!(*left_with.lock(), [128 + SIGALRM]);
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_prompt_answered_without_a_signal_lets_go_of_its_thread_and_leaves_nothing() {
        let left_with = Arc::new(Mutex::new(Vec::new()));
        let leaving = Arc::clone(&left_with);

        let interrupting = leaving_on([SIGVTALRM], move |signal| {
            leaving.lock().push(exit_code_after(signal));
        })
        .expect("a watch over the signal");
        drop(interrupting);

        assert!(left_with.lock().is_empty());
    }
}
