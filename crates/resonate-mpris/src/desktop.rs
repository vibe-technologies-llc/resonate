use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded};
use resonate_engine::PlaybackState;
use zbus::blocking::{Connection, connection};

use crate::{
    BusOp, Error, Result,
    idle::{Awake, Grip, Gripping},
    interfaces::Shared,
    notify::{Notifier, Shown},
    track::sounding,
};

const DEADLINE: Duration = Duration::from_secs(2);
const FAREWELL: Duration = Duration::from_millis(500);
const WAITING_ERRANDS: usize = 8;

enum Errand {
    Show(Shown),
    Tell(Shown),
    Grip,
}

pub(crate) struct Errands {
    asks: Sender<Errand>,
    done: Receiver<()>,
    awake: Arc<AtomicBool>,
    noted: Cell<bool>,
}

impl Errands {
    pub(crate) fn start(shared: &Arc<Shared>) -> Option<Self> {
        match Self::new(shared) {
            Ok(errands) => Some(errands),
            Err(error) => {
                tracing::warn!(%error, "the desktop will not be told what is playing");
                None
            }
        }
    }

    fn new(shared: &Arc<Shared>) -> Result<Self> {
        let connection = connection::Builder::session()
            .map_err(|source| Error::bus(BusOp::Connect, source))?
            .method_timeout(DEADLINE)
            .build()
            .map_err(|source| Error::bus(BusOp::Connect, source))?;

        let (asks, errands) = bounded(WAITING_ERRANDS);
        let (finished, done) = bounded(0);
        let shared = Arc::clone(shared);
        let awake = Arc::new(AtomicBool::new(false));
        let wanted = Arc::clone(&awake);

        thread::Builder::new()
            .name("resonate-desktop".to_owned())
            .spawn(move || {
                run(&connection, &shared, &errands, &wanted);
                let _ = connection.close();
                drop(finished);
            })
            .map_err(|_| Error::ThreadStopped)?;

        Ok(Self {
            asks,
            done,
            awake,
            noted: Cell::new(false),
        })
    }

    pub(crate) fn show(&self, shown: &Shown) {
        self.ask(Errand::Show(shown.clone()));
    }

    pub(crate) fn tell(&self, shown: Shown) {
        self.ask(Errand::Tell(shown));
    }

    pub(crate) fn follow(&self, playback: PlaybackState) {
        let wanted = sounding(playback);
        if self.noted.replace(wanted) == wanted {
            return;
        }
        self.awake.store(wanted, Ordering::Release);
        self.ask(Errand::Grip);
    }

    fn ask(&self, errand: Errand) {
        if let Err(TrySendError::Full(_)) = self.asks.try_send(errand) {
            tracing::debug!("the desktop is still answering the last thing it was told");
        }
    }

    pub(crate) fn rest(self) {
        drop(self.asks);
        if self.done.recv_timeout(FAREWELL) == Err(RecvTimeoutError::Timeout) {
            tracing::debug!("the desktop was left mid-call rather than waited on");
        }
    }
}

fn run(
    connection: &Connection,
    shared: &Arc<Shared>,
    errands: &Receiver<Errand>,
    wanted: &AtomicBool,
) {
    let notifier = Notifier::start(connection, shared.host.as_ref());
    let mut awake = Awake::start(connection, shared.host.as_ref());
    if let Some(notifier) = notifier.as_ref() {
        notifier.listen(shared);
    }

    let mut gripping = Gripping::default();
    for errand in errands {
        match (errand, notifier.as_ref()) {
            (Errand::Show(shown), Some(notifier)) => notifier.show(&shown),
            (Errand::Tell(shown), Some(notifier)) => notifier.tell(&shown),
            _ => {}
        }
        match (
            gripping.toward(wanted.load(Ordering::Acquire)),
            awake.as_mut(),
        ) {
            (Some(Grip::Take), Some(awake)) => awake.hold(),
            (Some(Grip::LetGo), Some(awake)) => awake.release(),
            _ => {}
        }
    }

    if let Some(awake) = awake.as_mut() {
        awake.release();
    }
    if let Some(notifier) = notifier.as_ref() {
        notifier.hush();
    }
}
