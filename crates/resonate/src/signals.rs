use std::{
    process,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Sender, TrySendError};
use resonate_engine::{Command, Player};
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    iterator::{Handle, Signals},
};

const TERMINATED_BY: i32 = 128;

const DRAINS_WITHIN: Duration = Duration::from_secs(5);

const SILENCED_AFTER: Duration = Duration::from_secs(1);

pub(crate) fn quit_when_told(asked_to_quit: Sender<()>, player: Arc<Player>) {
    let watched = match Signals::new([SIGINT, SIGTERM]) {
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
        if told {
            tracing::warn!(
                signal,
                "a second signal; leaving without closing the stream"
            );
            process::exit(TERMINATED_BY.saturating_add(signal));
        }
        told = true;

        match asked_to_quit.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {
                tracing::info!(signal, "asked to quit; draining what is playing");
                leave_anyway(signal, Arc::clone(player));
            }
            Err(TrySendError::Disconnected(())) => {
                process::exit(TERMINATED_BY.saturating_add(signal));
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
            process::exit(TERMINATED_BY.saturating_add(signal));
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
    let watched = match Signals::new([SIGINT, SIGTERM]) {
        Ok(watched) => watched,
        Err(error) => {
            tracing::warn!(%error, "no signal watch; an interrupt will stop the pass mid-file");
            return None;
        }
    };
    let handle = watched.handle();

    let spawned = thread::Builder::new()
        .name("resonate-interrupt".to_owned())
        .spawn(move || stop_the_pass(watched, &cancel));
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

fn stop_the_pass(mut watched: Signals, cancel: &impl Fn()) {
    let mut told = false;
    for signal in watched.forever() {
        if told {
            tracing::warn!(
                signal,
                "a second signal; leaving without finishing the file in hand"
            );
            process::exit(TERMINATED_BY.saturating_add(signal));
        }
        told = true;
        eprintln!("stopping once the file in hand is finished; a second interrupt leaves at once");
        cancel();
    }
}
