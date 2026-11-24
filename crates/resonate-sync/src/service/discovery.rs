use std::{
    collections::BTreeSet,
    io, mem,
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use tokio::{
    sync::{mpsc::UnboundedSender, oneshot},
    time::timeout,
};

use super::Event;
use crate::{
    DeviceId, Error, Result,
    stun::{Reflector, STUN_PORT},
};

pub(super) const PUBLIC_ADDRESS_KEPT_FOR: Duration = Duration::from_secs(30);
pub(super) const STUN_NAME_RESOLVED_WITHIN: Duration = Duration::from_secs(2);

pub(super) type Resolver = Arc<dyn Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync>;

pub(super) fn resolved_by_the_system() -> Resolver {
    Arc::new(|host, port| (host, port).to_socket_addrs().map(Iterator::collect))
}

enum Learnt {
    Never,
    Asking,
    At {
        learnt_at: Instant,
        public: Option<SocketAddr>,
    },
}

pub(super) enum Public {
    Known(Option<SocketAddr>),
    Awaited,
}

pub(super) struct Discovery {
    stun: Option<Arc<str>>,
    resolver: Resolver,
    resolving: Arc<AtomicBool>,
    learnt: Learnt,
    awaiting: BTreeSet<DeviceId>,
}

impl Discovery {
    pub(super) fn new(stun: Option<&str>, resolver: Resolver) -> Self {
        Self {
            stun: stun.map(Arc::from),
            resolver,
            resolving: Arc::new(AtomicBool::new(false)),
            learnt: Learnt::Never,
            awaiting: BTreeSet::new(),
        }
    }

    pub(super) fn public_for(
        &mut self,
        device: DeviceId,
        now: Instant,
        reflector: &Arc<Reflector>,
        events: &UnboundedSender<Event>,
    ) -> Public {
        let Some(stun) = self.stun.clone() else {
            return Public::Known(None);
        };

        match self.learnt {
            Learnt::At { learnt_at, public }
                if now.saturating_duration_since(learnt_at) < PUBLIC_ADDRESS_KEPT_FOR =>
            {
                return Public::Known(public);
            }
            Learnt::Asking => {}
            Learnt::Never | Learnt::At { .. } => {
                self.learnt = Learnt::Asking;
                tokio::spawn(learn(
                    stun,
                    Arc::clone(&self.resolver),
                    Arc::clone(&self.resolving),
                    Arc::clone(reflector),
                    events.clone(),
                ));
            }
        }
        self.awaiting.insert(device);
        Public::Awaited
    }

    pub(super) fn learnt(
        &mut self,
        public: Option<SocketAddr>,
        now: Instant,
    ) -> BTreeSet<DeviceId> {
        self.learnt = Learnt::At {
            learnt_at: now,
            public,
        };
        mem::take(&mut self.awaiting)
    }
}

async fn learn(
    stun: Arc<str>,
    resolver: Resolver,
    resolving: Arc<AtomicBool>,
    reflector: Arc<Reflector>,
    events: UnboundedSender<Event>,
) {
    let asked = async {
        let server = resolved(&stun, resolver, &resolving).await?;
        reflector.ask(server).await
    };
    let public = match asked.await {
        Ok(public) => Some(public),
        Err(error) => {
            tracing::debug!(%error, %stun, "the public address could not be learned");
            None
        }
    };
    let _ = events.send(Event::Discovered(public));
}

async fn resolved(
    stun: &str,
    resolver: Resolver,
    resolving: &Arc<AtomicBool>,
) -> Result<SocketAddr> {
    if resolving.swap(true, Ordering::AcqRel) {
        return Err(Error::StunNameStalled {
            server: stun.into(),
        });
    }

    let (host, port) = host_and_port(stun);
    let host = host.to_owned();
    let (answer, answered) = oneshot::channel();
    let still_resolving = Arc::clone(resolving);

    let started = thread::Builder::new()
        .name("resonate-stun-name".into())
        .spawn(move || {
            let addresses = resolver(&host, port);

            still_resolving.store(false, Ordering::Release);

            let _ = answer.send(addresses);
        });

    if let Err(source) = started {
        resolving.store(false, Ordering::Release);
        return Err(Error::StunResolver(source));
    }

    let addresses = match timeout(STUN_NAME_RESOLVED_WITHIN, answered).await {
        Ok(Ok(Ok(addresses))) => addresses,
        Ok(Ok(Err(source))) => {
            return Err(Error::Resolve {
                server: stun.into(),
                source,
            });
        }
        Ok(Err(_)) => Vec::new(),
        Err(_) => {
            return Err(Error::StunNameStalled {
                server: stun.into(),
            });
        }
    };
    addresses
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .ok_or_else(|| Error::Unresolved {
            server: stun.into(),
        })
}

fn host_and_port(stun: &str) -> (&str, u16) {
    match stun.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host, port),
            Err(_) => (stun, STUN_PORT),
        },
        _ => (stun, STUN_PORT),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        net::{Ipv4Addr, UdpSocket},
        path::{Path, PathBuf},
        process,
        sync::{atomic::AtomicUsize, mpsc as std_mpsc},
    };

    use parking_lot::Mutex;
    use quinn::{Runtime as _, TokioRuntime};
    use resonate_core::Recording;
    use tokio::{net::TcpListener, sync::mpsc};

    use super::*;
    use crate::{
        Catalog, LocalListen, LocalPlaylist, LocalPlaylistId, Opened, Platform, Refused,
        RemoteListen, Resumed, SongTags, SyncKey,
        catalog::Held,
        ledger::{Follow, Me},
        service::{Server, Settings, Standing, Sync},
        web::accepted,
        wire::{FromServer, ToServer},
    };

    const PEER: DeviceId = DeviceId([9; 16]);
    const A_COMMAND_IS_ANSWERED_WITHIN: Duration = Duration::from_secs(1);
    const STOPPED_WITHIN: Duration = Duration::from_millis(1500);
    const SETTLES_WITHIN: Duration = Duration::from_secs(10);
    const LOOKED_AT_EVERY: Duration = Duration::from_millis(10);
    const A_PUNCH_GOES_AHEAD_WITHIN: Duration =
        STUN_NAME_RESOLVED_WITHIN.saturating_add(Duration::from_secs(2));

    struct Bare;

    impl Catalog for Bare {
        fn playlists(&self) -> std::result::Result<Vec<LocalPlaylist>, Refused> {
            Ok(Vec::new())
        }

        fn write_playlist(
            &self,
            _: Option<LocalPlaylistId>,
            _: &str,
            _: &[Recording],
        ) -> std::result::Result<LocalPlaylist, Refused> {
            Err(Refused::Unwritable)
        }

        fn remove_playlist(&self, _: LocalPlaylistId) -> std::result::Result<(), Refused> {
            Ok(())
        }

        fn favourites(&self) -> std::result::Result<Vec<Recording>, Refused> {
            Ok(Vec::new())
        }

        fn favour(&self, _: &Recording, _: bool) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn listens_after(
            &self,
            _: u64,
            _: usize,
        ) -> std::result::Result<Vec<LocalListen>, Refused> {
            Ok(Vec::new())
        }

        fn credit(&self, _: &RemoteListen) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn resumption(&self) -> std::result::Result<Option<Resumed>, Refused> {
            Ok(None)
        }

        fn holds(&self, _: &Recording) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn holdings(&self) -> std::result::Result<Vec<Held>, Refused> {
            Ok(Vec::new())
        }

        fn open(&self, _: &Recording) -> std::result::Result<Option<Opened>, Refused> {
            Ok(None)
        }

        fn land(
            &self,
            _: &Recording,
            _: &SongTags,
            _: &str,
            _: &Path,
        ) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn revision(&self) -> u64 {
            0
        }
    }

    fn song(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: Some(Duration::from_secs(400)),
        }
    }

    fn device(number: u8) -> DeviceId {
        DeviceId([number; 16])
    }

    fn counted(lookups: &Arc<AtomicUsize>) -> Resolver {
        let lookups = Arc::clone(lookups);

        Arc::new(move |_, _| {
            lookups.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        })
    }

    fn reflector() -> Arc<Reflector> {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a socket");

        Reflector::over(TokioRuntime.wrap_udp_socket(socket).expect("wrapped"))
    }

    #[tokio::test]
    async fn punches_within_the_window_share_one_lookup_and_one_past_it_asks_again() {
        let lookups = Arc::new(AtomicUsize::new(0));
        let reflector = reflector();
        let (events, mut heard) = mpsc::unbounded_channel();
        let mut discovery = Discovery::new(Some("stun.example.org"), counted(&lookups));
        let asked_at = Instant::now();

        let first = discovery.public_for(device(1), asked_at, &reflector, &events);
        let second = discovery.public_for(device(2), asked_at, &reflector, &events);
        let answer = heard.recv().await;

        assert!(matches!(first, Public::Awaited));
        assert!(matches!(second, Public::Awaited));
        assert!(matches!(answer, Some(Event::Discovered(None))));

        let awaiting = discovery.learnt(None, asked_at);
        let within = discovery.public_for(
            device(3),
            asked_at + PUBLIC_ADDRESS_KEPT_FOR / 2,
            &reflector,
            &events,
        );

        assert_eq!(awaiting, BTreeSet::from([device(1), device(2)]));
        assert!(matches!(within, Public::Known(None)));
        assert_eq!(lookups.load(Ordering::Relaxed), 1);

        let past = discovery.public_for(
            device(4),
            asked_at + PUBLIC_ADDRESS_KEPT_FOR,
            &reflector,
            &events,
        );
        let answer = heard.recv().await;

        assert!(matches!(past, Public::Awaited));
        assert!(matches!(answer, Some(Event::Discovered(None))));
        assert_eq!(lookups.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn no_stun_server_offers_no_public_address_and_looks_nothing_up() {
        let lookups = Arc::new(AtomicUsize::new(0));
        let (events, _heard) = mpsc::unbounded_channel();
        let mut discovery = Discovery::new(None, counted(&lookups));

        let asked = discovery.public_for(device(1), Instant::now(), &reflector(), &events);

        assert!(matches!(asked, Public::Known(None)));
        assert_eq!(lookups.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_name_still_stalled_from_the_last_lookup_is_not_looked_up_twice() {
        let lookups = Arc::new(AtomicUsize::new(0));
        let (release, released) = std_mpsc::channel::<()>();
        let released = Mutex::new(released);
        let counting = Arc::clone(&lookups);

        let stalled: Resolver = Arc::new(move |_, _| {
            counting.fetch_add(1, Ordering::Relaxed);

            let _ = released.lock().recv();
            Ok(Vec::new())
        });
        let resolving = Arc::new(AtomicBool::new(false));

        let first = resolved("stun.example.org", Arc::clone(&stalled), &resolving).await;
        let second = resolved("stun.example.org", stalled, &resolving).await;

        assert!(matches!(first, Err(Error::StunNameStalled { .. })));
        assert!(matches!(second, Err(Error::StunNameStalled { .. })));
        assert_eq!(lookups.load(Ordering::Relaxed), 1);

        drop(release);
    }

    fn folder(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-discovery-{name}-{}", process::id()));

        let _ = fs::remove_dir_all(&folder);

        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    fn settings(folder: &Path, server: String, stun: String) -> Settings {
        Settings {
            server: Server::At(server),
            stun: Some(stun),
            trusted: None,
            key: SyncKey::minted().expect("a key"),
            me: Me {
                name: "Desk".into(),
                platform: Platform::Linux,
            },
            follow: Follow::default(),
            ledger: folder.join("ledger.sqlite"),
            staging: folder.join("staging"),
            transfers_allowed: true,
            nearby: None,
            relay_only: false,
        }
    }

    struct Introducing {
        address: String,
        heard: std_mpsc::Receiver<ToServer>,
    }

    fn introducing_a_peer() -> Introducing {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let listener = runtime
            .block_on(TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
            .expect("bound");
        let port = listener.local_addr().expect("an address").port();
        let (told, heard) = std_mpsc::channel();

        std::thread::spawn(move || runtime.block_on(introduced(listener, told)));
        Introducing {
            address: format!("ws://127.0.0.1:{port}/"),
            heard,
        }
    }

    async fn introduced(listener: TcpListener, told: std_mpsc::Sender<ToServer>) {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(mut accepted) = accepted(stream, None).await else {
            return;
        };

        while let Ok(Some(bytes)) = accepted.incoming.read().await {
            let Ok(frame) = ToServer::decoded(&bytes) else {
                continue;
            };

            if matches!(frame, ToServer::Hello { .. }) {
                let welcome = FromServer::Welcome {
                    online: vec![PEER],
                    seen_at: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
                    head: 0,
                };

                if accepted.outgoing.send(&welcome.encoded()).await.is_err() {
                    return;
                }
            }

            let _ = told.send(frame);
        }
    }

    fn eventually(what: &str, within: Duration, done: impl Fn() -> bool) {
        let deadline = Instant::now() + within;

        while !done() {
            assert!(
                Instant::now() < deadline,
                "{what} took longer than {within:?}"
            );

            thread::sleep(LOOKED_AT_EVERY);
        }
    }

    fn waiting_on(sync: &Sync, recording: &Recording) -> bool {
        sync.shown()
            .transfers
            .iter()
            .any(|transfer| transfer.recording == *recording)
    }

    fn punched(heard: &std_mpsc::Receiver<ToServer>, within: Duration) -> bool {
        let deadline = Instant::now() + within;

        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match heard.recv_timeout(left) {
                Ok(ToServer::Punch { to, .. }) if to == PEER => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    fn asking_while_the_lookup_waits(sync: &Sync) {
        eventually("linking", SETTLES_WITHIN, || {
            sync.shown().standing == Standing::Connected
        });

        sync.fetch(vec![song("Echoes")], PEER);
        sync.fetch(vec![song("One of These Days")], PEER);

        eventually(
            "a command behind a punch",
            A_COMMAND_IS_ANSWERED_WITHIN,
            || waiting_on(sync, &song("One of These Days")),
        );

        assert_eq!(sync.shown().standing, Standing::Connected);
    }

    fn stopped_at_once(sync: &Sync) {
        let asked = Instant::now();

        sync.stop();

        assert!(asked.elapsed() < STOPPED_WITHIN, "{:?}", asked.elapsed());
    }

    #[test]
    fn a_stun_name_that_never_resolves_holds_up_no_command_nor_stop_and_the_punch_goes_ahead() {
        let folder = folder("stalled");
        let introducing = introducing_a_peer();
        let lookups = Arc::new(AtomicUsize::new(0));
        let (release, released) = std_mpsc::channel::<()>();
        let released = Mutex::new(released);
        let counting = Arc::clone(&lookups);

        let stalled: Resolver = Arc::new(move |_, _| {
            counting.fetch_add(1, Ordering::Relaxed);

            let _ = released.lock().recv();
            Ok(Vec::new())
        });

        let sync = Sync::started(
            settings(&folder, introducing.address, "stun.example.org".into()),
            Arc::new(Bare),
            stalled,
        )
        .expect("started");

        asking_while_the_lookup_waits(&sync);

        assert!(punched(&introducing.heard, A_PUNCH_GOES_AHEAD_WITHIN));
        assert_eq!(lookups.load(Ordering::Relaxed), 1);

        stopped_at_once(&sync);
        drop(release);

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_stun_server_that_never_answers_holds_up_no_command_nor_stop() {
        let folder = folder("silent");
        let introducing = introducing_a_peer();
        let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a socket");
        let stun = silent.local_addr().expect("an address").to_string();

        let sync = Sync::start(settings(&folder, introducing.address, stun), Arc::new(Bare))
            .expect("started");

        asking_while_the_lookup_waits(&sync);
        stopped_at_once(&sync);

        drop(silent);

        let _ = fs::remove_dir_all(&folder);
    }
}
