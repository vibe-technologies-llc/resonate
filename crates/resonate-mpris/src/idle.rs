use std::{
    collections::HashMap,
    io,
    time::{Duration, Instant},
};

use zbus::{OwnedObjectPath, Variant};

use crate::{
    BusOp, Error, Host, Result,
    blocking::{Connection, Proxy},
    words,
};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const INHIBIT: &str = "org.freedesktop.portal.Inhibit";
const REQUEST: &str = "org.freedesktop.portal.Request";
const REQUEST_ROOT: &str = "/org/freedesktop/portal/desktop/request";
const TOKEN_PREFIX: &str = "resonate_";
const SETTLED_WITHIN: Duration = Duration::from_secs(60);
const NO_WINDOW: &str = "";
const SUSPEND: u32 = 4;
const IDLE: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grip {
    Take,
    LetGo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Gripping {
    held: bool,
}

impl Gripping {
    pub(crate) const fn toward(&mut self, wanted: bool) -> Option<Grip> {
        match (self.held, wanted) {
            (false, true) => {
                self.held = true;
                Some(Grip::Take)
            }
            (true, false) => {
                self.held = false;
                Some(Grip::LetGo)
            }
            (false, false) | (true, true) => None,
        }
    }
}

struct Closing {
    request: OwnedObjectPath,
    until: Instant,
}

pub(crate) struct Awake {
    connection: Connection,
    proxy: Proxy<'static>,
    reason: String,
    request: Option<OwnedObjectPath>,
    closing: Vec<Closing>,
    asked: u32,
}

fn timed_out(source: &zbus::Error) -> bool {
    matches!(source, zbus::Error::InputOutput(error) if error.kind() == io::ErrorKind::TimedOut)
}

fn handle_of(sender: &str, token: &str) -> Option<OwnedObjectPath> {
    let sender = sender.trim_start_matches(':').replace('.', "_");

    OwnedObjectPath::try_from(format!("{REQUEST_ROOT}/{sender}/{token}")).ok()
}

impl Awake {
    pub(crate) fn start(connection: &Connection, host: &dyn Host) -> Option<Self> {
        match Self::new(connection, host) {
            Ok(awake) => Some(awake),
            Err(error) => {
                tracing::warn!(%error, "the session may blank or suspend while a track plays");
                None
            }
        }
    }

    fn new(connection: &Connection, host: &dyn Host) -> Result<Self> {
        let proxy = Proxy::new(connection, PORTAL, OBJECT_PATH, INHIBIT)
            .map_err(|source| Error::bus(BusOp::Connect, source))?;

        Ok(Self {
            connection: connection.clone(),
            proxy,
            reason: words::idle::playing(host.identity()),
            request: None,
            closing: Vec::new(),
            asked: 0,
        })
    }

    pub(crate) fn is_settling(&self) -> bool {
        !self.closing.is_empty()
    }

    fn token(&mut self) -> String {
        self.asked += 1;

        format!("{TOKEN_PREFIX}{}", self.asked)
    }

    fn handle_for(&self, token: &str) -> Option<OwnedObjectPath> {
        self.connection
            .unique_name()
            .and_then(|sender| handle_of(sender.as_str(), token))
    }

    pub(crate) fn hold(&mut self) {
        if self.request.is_some() {
            return;
        }

        let token = self.token();
        let promised = self.handle_for(&token);
        let options: HashMap<&str, Variant<'_>> = HashMap::from([
            ("reason", Variant::from(self.reason.as_str())),
            ("handle_token", Variant::from(token.as_str())),
        ]);
        let asked = self
            .proxy
            .call::<_, _, OwnedObjectPath>("Inhibit", &(NO_WINDOW, IDLE | SUSPEND, options));

        match asked {
            Ok(request) => self.request = Some(request),
            Err(source) if timed_out(&source) => {
                let error = Error::bus(BusOp::Inhibit, source);

                tracing::debug!(%error, "the session may grant the request to stay awake late");
                self.request = promised;
            }
            Err(source) => {
                let error = Error::bus(BusOp::Inhibit, source);

                tracing::debug!(%error, "the session was not asked to stay awake");
            }
        }
    }

    fn close(&self, request: &OwnedObjectPath) -> Result<()> {
        Proxy::new(
            &self.connection,
            PORTAL,
            request.clone().into_inner(),
            REQUEST,
        )
        .map_err(|source| Error::bus(BusOp::Connect, source))
        .and_then(|request| {
            request
                .call::<_, _, ()>("Close", &())
                .map_err(|source| Error::bus(BusOp::Release, source))
        })
    }

    pub(crate) fn release(&mut self) {
        let Some(request) = self.request.take() else {
            return;
        };

        if let Err(error) = self.close(&request) {
            tracing::debug!(%error, "the session may still be held awake, closing it again");
            self.closing.push(Closing {
                request,
                until: Instant::now() + SETTLED_WITHIN,
            });
        }
    }

    pub(crate) fn reconcile(&mut self) {
        let now = Instant::now();
        let closing = std::mem::take(&mut self.closing);

        self.closing = closing
            .into_iter()
            .filter(|pending| match self.close(&pending.request) {
                Ok(()) => false,
                Err(error) if now >= pending.until => {
                    tracing::debug!(%error, "the session was left held awake");
                    false
                }
                Err(_) => true,
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        future::poll_fn,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        task::{Poll, Waker},
        thread,
    };

    use parking_lot::Mutex;
    use zbus::{OwnedVariant, Str, message::Header};

    use super::*;
    use crate::blocking::{connection, fdo::DBusProxy};

    const LATE_DELAY: Duration = Duration::from_millis(800);
    const DEADLINE: Duration = Duration::from_millis(300);
    const GIVEN_UP_AFTER: Duration = Duration::from_secs(10);
    const LOOKED_EVERY: Duration = Duration::from_millis(100);

    type Held = Arc<Mutex<BTreeSet<String>>>;

    struct Portal {
        delay_ms: Arc<AtomicU64>,
        held: Held,
    }

    struct Handle {
        path: String,
        held: Held,
    }

    async fn slept(duration: Duration) {
        let state = Arc::new(Mutex::new((false, None::<Waker>)));
        let sleeper = Arc::clone(&state);

        thread::spawn(move || {
            thread::sleep(duration);

            let mut state = sleeper.lock();

            state.0 = true;
            if let Some(waker) = state.1.take() {
                waker.wake();
            }
        });

        poll_fn(|context| {
            let mut state = state.lock();
            if state.0 {
                Poll::Ready(())
            } else {
                state.1 = Some(context.waker().clone());
                Poll::Pending
            }
        })
        .await;
    }

    #[zbus::interface(name = "org.freedesktop.portal.Request")]
    impl Handle {
        fn close(&self) {
            self.held.lock().remove(&self.path);
        }
    }

    #[zbus::interface(name = "org.freedesktop.portal.Inhibit")]
    impl Portal {
        async fn inhibit(
            &self,
            _window: &str,
            _flags: u32,
            options: HashMap<String, OwnedVariant>,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            slept(Duration::from_millis(self.delay_ms.load(Ordering::Relaxed))).await;

            let sender = header.sender().map(|sender| sender.as_str().to_owned());
            let token = options
                .get("handle_token")
                .and_then(|token| token.downcast_ref::<Str>().ok())
                .map(|token| token.as_str().to_owned());
            let path = sender
                .zip(token)
                .and_then(|(sender, token)| handle_of(&sender, &token))
                .ok_or_else(|| zbus::fdo::Error::InvalidArgs("no handle token".to_owned()))?;

            self.held.lock().insert(path.as_str().to_owned());
            connection
                .object_server()
                .at(
                    path.clone(),
                    Handle {
                        path: path.as_str().to_owned(),
                        held: Arc::clone(&self.held),
                    },
                )
                .await?;

            Ok(path)
        }
    }

    fn awake_over(connection: &Connection) -> Awake {
        Awake {
            connection: connection.clone(),
            proxy: Proxy::new(connection, PORTAL, OBJECT_PATH, INHIBIT)
                .expect("the portal's proxy is made"),
            reason: "a test".to_owned(),
            request: None,
            closing: Vec::new(),
            asked: 0,
        }
    }

    fn settled(awake: &mut Awake) {
        let begun = Instant::now();

        while awake.is_settling() && begun.elapsed() < GIVEN_UP_AFTER {
            thread::sleep(LOOKED_EVERY);
            awake.reconcile();
        }

        assert!(!awake.is_settling(), "the request was never closed");
    }

    #[test]
    fn the_session_is_taken_hold_of_once_and_let_go_of_once() {
        let mut gripping = Gripping::default();

        assert_eq!(gripping.toward(true), Some(Grip::Take));
        assert_eq!(gripping.toward(true), None);
        assert_eq!(gripping.toward(false), Some(Grip::LetGo));
        assert_eq!(gripping.toward(false), None);
    }

    #[test]
    fn a_request_handle_is_the_one_the_portal_names_for_a_sender_and_a_token() {
        assert_eq!(
            handle_of(":1.42", "resonate_3")
                .as_ref()
                .map(|handle| handle.as_str()),
            Some("/org/freedesktop/portal/desktop/request/1_42/resonate_3")
        );
        assert!(handle_of(":1.42", "no token").is_none());
    }

    #[test]
    fn an_inhibit_granted_after_its_deadline_is_closed_once_the_session_is_let_go() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            return;
        }

        let client = connection::Builder::session()
            .and_then(|builder| builder.method_timeout(DEADLINE).build())
            .expect("the session bus connects");
        let bus = DBusProxy::new(&client).expect("the bus answers");

        if bus
            .name_has_owner(PORTAL.try_into().expect("a service name"))
            .expect("the owner reads")
        {
            return;
        }

        let held = Held::default();
        let delay_ms = Arc::new(AtomicU64::new(LATE_DELAY.as_millis() as u64));

        let stand_in = connection::Builder::session()
            .and_then(|builder| builder.name(PORTAL))
            .and_then(|builder| {
                builder.serve_at(
                    OBJECT_PATH,
                    Portal {
                        delay_ms: Arc::clone(&delay_ms),
                        held: Arc::clone(&held),
                    },
                )
            })
            .and_then(|builder| builder.build())
            .expect("the stand-in portal starts");
        let mut awake = awake_over(&client);

        awake.hold();

        assert!(awake.request.is_some(), "a late grant is waited for");
        assert!(
            held.lock().is_empty(),
            "the portal answered within the deadline"
        );

        awake.release();

        assert!(awake.is_settling(), "the request has nothing to close yet");

        settled(&mut awake);

        assert!(held.lock().is_empty(), "a late grant was left held");

        delay_ms.store(0, Ordering::Relaxed);
        awake.hold();

        assert_eq!(held.lock().len(), 1);

        awake.release();

        assert!(held.lock().is_empty());
        assert!(!awake.is_settling());

        stand_in.close().expect("the stand-in stops");
    }
}
