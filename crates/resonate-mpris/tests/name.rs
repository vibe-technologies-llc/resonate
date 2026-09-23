use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, unbounded};
use resonate_engine::{
    AudioSource, Backend, EngineConfig, Player, SinkChange, SinkError, SinkInfo, SinkResult,
    SinkStream, StreamRequest,
};
use resonate_mpris::{Host, Mpris};
use zbus::{
    blocking::{Connection, Proxy, connection, fdo::DBusProxy},
    fdo::{RequestNameFlags, RequestNameReply},
    names::{BusName, WellKnownName},
};

const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const IDENTITY: &str = "Resonate";
const INSTANCE_PREFIX: &str = "org.mpris.MediaPlayer2.resonate.instance";
const PATIENCE: Duration = Duration::from_secs(15);

struct NoSinks {
    changes: Receiver<SinkChange>,
}

impl NoSinks {
    fn new() -> Self {
        let (announce, changes) = unbounded();
        drop(announce);
        Self { changes }
    }
}

impl Backend for NoSinks {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(Vec::new())
    }

    fn open(
        &self,
        _request: &StreamRequest,
        _source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        Err(SinkError::NoSink)
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Ok(())
    }
}

struct Desktop;

impl Host for Desktop {
    fn identity(&self) -> String {
        IDENTITY.to_owned()
    }
}

struct Served {
    connection: Connection,
    mpris: Option<Mpris>,
}

impl Served {
    fn start() -> Option<Self> {
        let connection = match Connection::session() {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("skipped: no session bus to talk to ({error})");
                return None;
            }
        };
        let player = Arc::new(
            Player::with_backend(EngineConfig::default(), |_| Ok(Box::new(NoSinks::new())))
                .expect("the engine starts behind a silent sink"),
        );
        let mpris = Mpris::start(player, Arc::new(Desktop), None)
            .expect("the MPRIS service reaches the session bus");

        Some(Self {
            connection,
            mpris: Some(mpris),
        })
    }

    fn name(&self) -> String {
        self.mpris
            .as_ref()
            .expect("the service is running")
            .name()
            .to_string()
    }

    fn wait_for(&self, mut ready: impl FnMut(&Self) -> bool, what: &str) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if ready(self) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "timed out waiting for {what}; the service answers on {}",
            self.name()
        );
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        if let Some(mpris) = self.mpris.take() {
            mpris.shutdown();
        }
    }
}

fn owner(connection: &Connection, name: &str) -> Option<String> {
    DBusProxy::new(connection)
        .expect("the bus answers for itself")
        .get_name_owner(BusName::try_from(name).expect("a bus name"))
        .ok()
        .map(|unique| unique.to_string())
}

#[test]
fn the_bus_name_is_handed_back_when_the_service_stops() {
    let Some(mut served) = Served::start() else {
        return;
    };
    let held = served.name();
    let answering =
        owner(&served.connection, &held).expect("the service owns the name it answered on");

    served
        .mpris
        .take()
        .expect("the service is running")
        .shutdown();

    assert_ne!(
        owner(&served.connection, &held),
        Some(answering),
        "the name was still owned by the service that stopped"
    );
}

#[test]
fn a_name_taken_away_is_answered_by_serving_under_an_instance_name() {
    let Some(served) = Served::start() else {
        return;
    };
    let held = served.name();
    let taking = connection::Builder::session()
        .expect("a session bus to talk to")
        .build()
        .expect("a second connection of our own");

    let reply = taking
        .request_name_with_flags(
            WellKnownName::try_from(held.as_str()).expect("a well-known name"),
            RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting,
        )
        .expect("the bus answers a name request");
    assert_eq!(
        reply,
        RequestNameReply::PrimaryOwner,
        "the service would not let the name go"
    );

    served.wait_for(
        |served| served.name() != held,
        "the service to take up a name of its own",
    );

    let moved = served.name();
    assert!(
        moved.starts_with(INSTANCE_PREFIX),
        "the service moved to {moved} rather than to an instance name"
    );
    let root = Proxy::new(&served.connection, moved, OBJECT_PATH, ROOT)
        .expect("the served object answers under the name it moved to");
    assert_eq!(
        root.get_property::<String>("Identity").expect("Identity"),
        IDENTITY
    );
}
