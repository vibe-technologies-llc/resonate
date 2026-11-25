use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError};

use crate::{
    BusOp, Error, Result,
    blocking::{Connection, connection},
    desktop::Errand,
    idle::{Awake, Grip, Gripping},
    interfaces::Shared,
    notifier::Notifier,
};

const DEADLINE: Duration = Duration::from_secs(2);
const SETTLING_EVERY: Duration = Duration::from_secs(1);
const WATCHED_EVERY: Duration = Duration::from_secs(5);
const REOPENED_AT_MOST_EVERY: Duration = Duration::from_secs(10);

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
        let mut reached = Some(Reached::over(self.connection, shared));
        let mut reopening = Reopening::default();

        loop {
            let waiting = reached.as_ref().map_or(WATCHED_EVERY, Reached::looks_every);
            let errand = match errands.recv_timeout(waiting) {
                Ok(errand) => Some(errand),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };

            if reached.as_ref().is_some_and(Reached::is_closed) {
                tracing::warn!(
                    "the session bus closed the connection notifications and the idle inhibit were made on"
                );
                reached = None;
            }
            if reached.is_none() && reopening.due(Instant::now()) {
                reached = Self::open()
                    .inspect_err(|error| {
                        tracing::warn!(%error, "the desktop could not be reached again");
                    })
                    .ok()
                    .map(|desk| Reached::over(desk.connection, shared));
            }
            if let Some(reached) = reached.as_mut() {
                reached.attend(errand, wanted.load(Ordering::Acquire));
            }
        }

        if let Some(reached) = reached {
            reached.leave();
        }
    }
}

struct Reached {
    connection: Connection,
    notifier: Option<Notifier>,
    awake: Option<Awake>,
    gripping: Gripping,
}

impl Reached {
    fn over(connection: Connection, shared: &Arc<Shared>) -> Self {
        let notifier = Notifier::start(&connection, shared.host.as_ref());
        let awake = Awake::start(&connection, shared.host.as_ref());

        if let Some(notifier) = notifier.as_ref() {
            notifier.listen(shared);
        }

        Self {
            connection,
            notifier,
            awake,
            gripping: Gripping::default(),
        }
    }

    fn is_closed(&self) -> bool {
        self.connection.is_closed()
    }

    fn looks_every(&self) -> Duration {
        if self.awake.as_ref().is_some_and(Awake::is_settling) {
            SETTLING_EVERY
        } else {
            WATCHED_EVERY
        }
    }

    fn attend(&mut self, errand: Option<Errand>, wanted: bool) {
        match (errand, self.notifier.as_ref()) {
            (Some(Errand::Show(shown)), Some(notifier)) => notifier.show(&shown),
            (Some(Errand::Tell(shown)), Some(notifier)) => notifier.tell(&shown),
            _ => {}
        }
        match (self.gripping.toward(wanted), self.awake.as_mut()) {
            (Some(Grip::Take), Some(awake)) => awake.hold(),
            (Some(Grip::LetGo), Some(awake)) => awake.release(),
            _ => {}
        }
        if let Some(awake) = self.awake.as_mut() {
            awake.reconcile();
        }
    }

    fn leave(mut self) {
        if let Some(awake) = self.awake.as_mut() {
            awake.release();
        }
        if let Some(notifier) = self.notifier.as_ref() {
            notifier.hush();
        }

        let _ = self.connection.close();
    }
}

#[derive(Default)]
struct Reopening {
    last: Option<Instant>,
}

impl Reopening {
    fn due(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < REOPENED_AT_MOST_EVERY)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_connection_is_opened_again_at_once_and_then_no_more_often_than_its_pace() {
        let start = Instant::now();
        let mut reopening = Reopening::default();

        assert!(reopening.due(start));
        assert!(!reopening.due(start + REOPENED_AT_MOST_EVERY / 2));
        assert!(reopening.due(start + REOPENED_AT_MOST_EVERY));
        assert!(!reopening.due(start + REOPENED_AT_MOST_EVERY));
    }
}
