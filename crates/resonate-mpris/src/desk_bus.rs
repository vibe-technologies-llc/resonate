use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::Receiver;

use crate::{
    BusOp, Error, Result,
    blocking::{Connection, connection},
    desktop::Errand,
    idle::{Awake, Grip, Gripping},
    interfaces::Shared,
    notifier::Notifier,
};

const DEADLINE: Duration = Duration::from_secs(2);

pub(crate) struct Desk {
    connection: Connection,
}

impl Desk {
    pub(crate) fn open() -> Result<Self> {
        let connection = connection::Builder::session()
            .map_err(|source| Error::bus(BusOp::Connect, source))?
            .method_timeout(DEADLINE)
            .build()
            .map_err(|source| Error::bus(BusOp::Connect, source))?;

        Ok(Self { connection })
    }

    pub(crate) fn run(self, shared: &Arc<Shared>, errands: &Receiver<Errand>, wanted: &AtomicBool) {
        let connection = &self.connection;
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
        let _ = connection.close();
    }
}
