use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use parking_lot::{Mutex, RwLock};
use quinn::{ClientConfig, Connection, Endpoint, EndpointConfig, SendStream, TokioRuntime};
use resonate_core::Recording;
use ring::digest::{SHA256, digest};
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    time::{Instant, MissedTickBehavior, interval, sleep_until, timeout},
};

use crate::{
    Catalog, Change, DeviceId, Error, Handoff, Ledger, LocalPlaylistId, Platform, Result, SyncKey,
    key::{Derived, hex, random_bytes},
    ledger::{Follow, Me},
    peer::{blocking, dialled, fetch, holdings_of, serve},
    quic::{PeerIdentity, read_frame, send_frame, to_the_server, trusted_roots},
    seal::{opened, sealed},
    wire::{
        FromServer, LARGEST_FRAME, PROTOCOL_VERSION, Refusal, ToServer, decoded_bundle,
        decoded_candidates, encoded_bundle, encoded_candidates,
    },
};

pub const HOSTED_SERVER: &str = "resonate.tecnio.dev";
pub const SERVER_PORT: u16 = 443;
const FIRST_RETRY: Duration = Duration::from_secs(5);
const LAST_RETRY: Duration = Duration::from_secs(30 * 60);
const LOOK_AFTER: Duration = Duration::from_secs(3);
const LOOK_ANYWAY_EVERY: Duration = Duration::from_secs(5 * 60);
const WATCH_EVERY: Duration = Duration::from_secs(1);
const COMPACT_PAST_RECORDS: u64 = 512;
const COMPACT_PAST_BYTES: u64 = 4 * 1024 * 1024;
const BLOBS_PER_APPEND: usize = 32;
const FOLLOW_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
const LEAVE_WITHIN: Duration = Duration::from_secs(2);
const SNAPSHOT_FRAME_SHARE: usize = LARGEST_FRAME / 2;

pub struct Settings {
    pub server: Option<String>,
    pub trusted: Option<PathBuf>,
    pub key: SyncKey,
    pub me: Me,
    pub follow: Follow,
    pub ledger: PathBuf,
    pub staging: PathBuf,
    pub transfers_allowed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standing {
    #[default]
    Starting,
    Connecting,
    Connected,
    Unreachable,
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncDevice {
    pub id: DeviceId,
    pub name: String,
    pub platform: Platform,
    pub online: bool,
    pub this: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Waiting,
    Moving,
    Landed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub recording: Recording,
    pub from: DeviceId,
    pub received: u64,
    pub size: u64,
    pub state: TransferState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub standing: Standing,
    pub devices: Vec<SyncDevice>,
    pub transfers: Vec<Transfer>,
    pub owed: usize,
    pub settled: bool,
}

enum Command {
    Fetch {
        recordings: Vec<Recording>,
        from: DeviceId,
    },
    Transfers(bool),
    Stop,
}

pub struct Sync {
    commands: UnboundedSender<Command>,
    shown: Arc<RwLock<Shown>>,
    ledger: Arc<Mutex<Ledger>>,
    code: String,
    device: DeviceId,
    thread: Option<thread::JoinHandle<()>>,
}

struct Running {
    catalog: Arc<dyn Catalog>,
    ledger: Arc<Mutex<Ledger>>,
    shown: Arc<RwLock<Shown>>,
    derived: Derived,
    device: DeviceId,
    me: Me,
    follow: Follow,
    server: String,
    staging: PathBuf,
    endpoint: Endpoint,
    to_server: ClientConfig,
    to_peers: ClientConfig,
    transfers_allowed: AtomicBool,
}

enum Ended {
    Stopped,
    Lost,
}

enum Work {
    Fetch(Vec<Recording>),
    Follow,
}

enum PeerEvent {
    Connected(DeviceId, Connection),
    Unreached(DeviceId),
}

struct Session<'a> {
    running: &'a Arc<Running>,
    send: SendStream,
    server_address: SocketAddr,
    online: BTreeSet<DeviceId>,
    head: u64,
    caught_up: bool,
    in_flight: VecDeque<Vec<Change>>,
    compacting: bool,
    peers: BTreeMap<DeviceId, Connection>,
    waiting: BTreeMap<DeviceId, Vec<Work>>,
    followed: BTreeMap<DeviceId, Instant>,
    look_at: Option<Instant>,
    looked_at: Instant,
    looked_once: bool,
    seen_revision: u64,
    events: UnboundedSender<PeerEvent>,
}

impl Sync {
    pub fn start(settings: Settings, catalog: Arc<dyn Catalog>) -> Result<Self> {
        let derived = settings.key.derived()?;
        let identity = PeerIdentity::of(&derived)?;
        let roots = trusted_roots(settings.trusted.as_deref())?;
        let to_server = to_the_server(roots)?;
        let to_peers = identity.dialling()?;
        let serving = identity.serving()?;

        let mut ledger = Ledger::open(&settings.ledger)?;
        ledger.enter_space(derived.space)?;
        let device = ledger.device();
        let ledger = Arc::new(Mutex::new(ledger));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;
        let socket = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
            .map_err(Error::Bind)?;
        let endpoint = {
            let _entered = runtime.enter();
            Endpoint::new(
                EndpointConfig::default(),
                Some(serving),
                socket,
                Arc::new(TokioRuntime),
            )
            .map_err(Error::Bind)?
        };

        let shown = Arc::new(RwLock::new(Shown::default()));
        let running = Arc::new(Running {
            catalog,
            ledger: Arc::clone(&ledger),
            shown: Arc::clone(&shown),
            derived,
            device,
            me: settings.me,
            follow: settings.follow,
            server: settings
                .server
                .filter(|server| !server.trim().is_empty())
                .unwrap_or_else(|| HOSTED_SERVER.to_owned()),
            staging: settings.staging,
            endpoint,
            to_server,
            to_peers,
            transfers_allowed: AtomicBool::new(settings.transfers_allowed),
        });

        let (commands, received) = mpsc::unbounded_channel();
        let thread = thread::Builder::new()
            .name("resonate-sync".into())
            .spawn(move || ran(&runtime, running, received))
            .map_err(Error::Thread)?;

        Ok(Self {
            commands,
            shown,
            ledger,
            code: settings.key.code(),
            device,
            thread: Some(thread),
        })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub const fn device(&self) -> DeviceId {
        self.device
    }

    pub fn shown(&self) -> Shown {
        self.shown.read().clone()
    }

    pub fn fetch(&self, recordings: Vec<Recording>, from: DeviceId) {
        let _ = self.commands.send(Command::Fetch { recordings, from });
    }

    pub fn allow_transfers(&self, allowed: bool) {
        let _ = self.commands.send(Command::Transfers(allowed));
    }

    pub fn keep_songs_of(&self, at: LocalPlaylistId, keep: bool) -> Result<bool> {
        self.ledger.lock().keep_songs_of(at, keep)
    }

    pub fn kept_songs_of(&self, at: LocalPlaylistId) -> Result<bool> {
        self.ledger.lock().kept_songs_of(at)
    }

    pub fn missing_from(&self, at: LocalPlaylistId) -> Result<Vec<Recording>> {
        self.ledger.lock().missing_from(at)
    }

    pub fn handoffs(&self) -> Result<Vec<Handoff>> {
        let mut handoffs = self.ledger.lock().handoffs()?;
        handoffs.retain(|handoff| handoff.device != self.device && !handoff.rows.is_empty());
        handoffs.sort_by_key(|handoff| Reverse(handoff.stamp));
        Ok(handoffs)
    }

    pub fn leave(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Sync {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Running {
    fn stand(&self, standing: Standing) {
        self.shown.write().standing = standing;
    }

    fn transfers_allowed(&self) -> bool {
        self.transfers_allowed.load(Ordering::Relaxed)
    }

    fn publish_devices(&self, online: &BTreeSet<DeviceId>) {
        let notes = match self.ledger.lock().devices() {
            Ok(notes) => notes,
            Err(error) => {
                tracing::warn!(%error, "the synced devices could not be read");
                return;
            }
        };
        let devices = notes
            .into_iter()
            .map(|note| SyncDevice {
                online: online.contains(&note.device) || note.device == self.device,
                this: note.device == self.device,
                id: note.device,
                name: note.name,
                platform: note.platform,
            })
            .collect();
        self.shown.write().devices = devices;
    }

    fn transfer(&self, recording: &Recording, from: DeviceId, change: impl FnOnce(&mut Transfer)) {
        let mut shown = self.shown.write();
        let at = shown
            .transfers
            .iter()
            .position(|transfer| transfer.from == from && transfer.recording == *recording);
        let at = at.unwrap_or_else(|| {
            shown.transfers.push(Transfer {
                recording: recording.clone(),
                from,
                received: 0,
                size: 0,
                state: TransferState::Waiting,
            });
            shown.transfers.len() - 1
        });
        change(&mut shown.transfers[at]);
    }
}

fn ran(runtime: &Runtime, running: Arc<Running>, commands: UnboundedReceiver<Command>) {
    runtime.block_on(run(running, commands));
}

async fn run(running: Arc<Running>, mut commands: UnboundedReceiver<Command>) {
    let accepting = tokio::spawn(accept_peers(Arc::clone(&running)));
    let mut queued = Vec::new();
    let mut wait = FIRST_RETRY;
    loop {
        running.stand(Standing::Connecting);
        running.shown.write().settled = false;
        match session(&running, &mut commands, &mut queued).await {
            Ok(Ended::Stopped) => break,
            Ok(Ended::Lost) => {
                running.stand(Standing::Unreachable);
                wait = FIRST_RETRY;
            }
            Err(Error::Refused(refusal)) => {
                tracing::warn!(?refusal, "the sync server refused this device");
                running.stand(Standing::Refused(refusal));
                wait = LAST_RETRY;
            }
            Err(error) => {
                tracing::debug!(%error, server = %running.server, "the sync server could not be reached");
                running.stand(Standing::Unreachable);
            }
        }

        let again = Instant::now() + jittered(wait);
        loop {
            tokio::select! {
                () = sleep_until(again) => break,
                command = commands.recv() => match command {
                    None | Some(Command::Stop) => {
                        accepting.abort();
                        leave(&running).await;
                        return;
                    }
                    Some(Command::Transfers(allowed)) => {
                        running.transfers_allowed.store(allowed, Ordering::Relaxed);
                    }
                    Some(Command::Fetch { recordings, from }) => queued.push((from, Work::Fetch(recordings))),
                },
            }
        }
        wait = (wait * 2).min(LAST_RETRY);
    }
    accepting.abort();
    leave(&running).await;
}

async fn leave(running: &Running) {
    running.endpoint.close(0_u32.into(), b"leaving");
    let _ = timeout(LEAVE_WITHIN, running.endpoint.wait_idle()).await;
}

fn jittered(wait: Duration) -> Duration {
    let spread = random_bytes::<1>().map(|[byte]| byte).unwrap_or_default();
    wait + wait * u32::from(spread) / 1024
}

async fn resolve(server: &str) -> Result<(SocketAddr, String)> {
    let (host, port) = host_and_port(server.trim());
    let addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|source| Error::Resolve {
            server: server.into(),
            source,
        })?
        .collect::<Vec<_>>();
    let address = addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or_else(|| addresses.first())
        .copied()
        .ok_or_else(|| Error::Unresolved {
            server: server.into(),
        })?;
    Ok((address, host))
}

fn host_and_port(server: &str) -> (String, u16) {
    if let Ok(address) = server.parse::<SocketAddr>() {
        return (address.ip().to_string(), address.port());
    }
    match server.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host.to_owned(), port),
            Err(_) => (server.to_owned(), SERVER_PORT),
        },
        _ => (server.trim_matches(['[', ']']).to_owned(), SERVER_PORT),
    }
}

async fn session(
    running: &Arc<Running>,
    commands: &mut UnboundedReceiver<Command>,
    queued: &mut Vec<(DeviceId, Work)>,
) -> Result<Ended> {
    let (server_address, host) = resolve(&running.server).await?;
    let connection = running
        .endpoint
        .connect_with(running.to_server.clone(), server_address, &host)
        .map_err(|source| Error::Connect(Box::new(source)))?
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    let (mut send, recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;

    let (frames_in, mut frames) = mpsc::unbounded_channel();
    let reading = tokio::spawn(read_frames(recv, frames_in));
    let cursor = running.ledger.lock().cursor()?;
    let hello = ToServer::Hello {
        version: PROTOCOL_VERSION,
        space: running.derived.space,
        token: running.derived.token.clone(),
        device: running.device,
        after: cursor,
    };
    send_frame(&mut send, &hello.encoded()).await?;

    let (online, head) = match frames.recv().await {
        Some(Ok(FromServer::Welcome { online, head, .. })) => (online, head),
        Some(Ok(FromServer::Refused(refusal))) => return Err(Error::Refused(refusal)),
        Some(Err(error)) => return Err(error),
        Some(Ok(_)) | None => return Err(Error::Unwelcome),
    };
    tracing::info!(server = %running.server, "syncing with the server");

    let (events, mut peer_events) = mpsc::unbounded_channel();
    let mut session = Session {
        running,
        send,
        server_address,
        online: online.into_iter().collect(),
        head,
        caught_up: cursor >= head,
        in_flight: VecDeque::new(),
        compacting: false,
        peers: BTreeMap::new(),
        waiting: BTreeMap::new(),
        followed: BTreeMap::new(),
        look_at: Some(Instant::now()),
        looked_at: Instant::now(),
        looked_once: false,
        seen_revision: 0,
        events,
    };
    running.stand(Standing::Connected);
    running.publish_devices(&session.online);
    for (device, work) in queued.drain(..) {
        session.want(device, work).await?;
    }
    for device in session.online.clone() {
        session.follow(device).await?;
    }

    let mut watch = interval(WATCH_EVERY);
    watch.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let ended = loop {
        tokio::select! {
            frame = frames.recv() => match frame {
                None => break Ok(Ended::Lost),
                Some(Err(error)) => break Err(error),
                Some(Ok(frame)) => session.heard(frame).await?,
            },
            command = commands.recv() => match command {
                None | Some(Command::Stop) => {
                    let _ = send_frame(&mut session.send, &ToServer::Bye.encoded()).await;
                    let _ = session.send.finish();
                    break Ok(Ended::Stopped);
                }
                Some(Command::Transfers(allowed)) => {
                    running.transfers_allowed.store(allowed, Ordering::Relaxed);
                    if allowed {
                        session.start_waiting().await?;
                    }
                }
                Some(Command::Fetch { recordings, from }) => session.want(from, Work::Fetch(recordings)).await?,
            },
            Some(event) = peer_events.recv() => session.peer_event(event).await,
            _ = watch.tick() => session.watched().await?,
        }
    };
    reading.abort();
    connection.close(0_u32.into(), b"leaving");
    ended
}

async fn read_frames(mut recv: quinn::RecvStream, frames: UnboundedSender<Result<FromServer>>) {
    loop {
        let frame = match read_frame(&mut recv).await {
            Ok(Some(bytes)) => FromServer::decoded(&bytes),
            Ok(None) => return,
            Err(error) => Err(error),
        };
        let failed = frame.is_err();
        if frames.send(frame).is_err() || failed {
            return;
        }
    }
}

impl Session<'_> {
    async fn heard(&mut self, frame: FromServer) -> Result<()> {
        match frame {
            FromServer::Snapshot { through, parts } => {
                self.apply(parts.into_iter().map(Some).collect(), None)
                    .await?;
                let ledger = Arc::clone(&self.running.ledger);
                blocking(move || ledger.lock().compacted(through)).await?;
                self.caught_up_through(through);
            }
            FromServer::Records { entries } => {
                let last = entries.iter().map(|entry| entry.seq).max();
                let seqs = entries.iter().map(|entry| entry.seq).collect::<Vec<_>>();
                self.apply(
                    entries.into_iter().map(|entry| entry.blob).collect(),
                    Some(seqs),
                )
                .await?;
                if let Some(last) = last {
                    self.caught_up_through(last);
                }
                self.compact().await?;
            }
            FromServer::Appended { .. } => {
                if let Some(changes) = self.in_flight.pop_front() {
                    let ledger = Arc::clone(&self.running.ledger);
                    blocking(move || ledger.lock().sent(&changes)).await?;
                }
                if self.in_flight.is_empty() {
                    self.push().await?;
                }
                self.publish_settled();
            }
            FromServer::Arrived { device } => {
                self.online.insert(device);
                self.running.publish_devices(&self.online);
                if self.waiting.contains_key(&device) {
                    self.punch(device).await?;
                }
                self.follow(device).await?;
            }
            FromServer::Left { device } => {
                self.online.remove(&device);
                self.running.publish_devices(&self.online);
            }
            FromServer::Punched {
                device,
                address,
                sealed,
            } => self.punched(device, address, &sealed).await?,
            FromServer::Compacted { through } => {
                self.compacting = false;
                let ledger = Arc::clone(&self.running.ledger);
                blocking(move || ledger.lock().compacted(through)).await?;
            }
            FromServer::Refused(Refusal::Stale) => self.compacting = false,
            FromServer::Refused(Refusal::TooLarge) => {
                tracing::warn!("the sync server refused a record as too large");
                self.in_flight.pop_front();
            }
            FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
            FromServer::Welcome { .. } => {}
        }
        Ok(())
    }

    fn caught_up_through(&mut self, through: u64) {
        if !self.caught_up && through >= self.head {
            self.caught_up = true;
            self.look_at = Some(Instant::now());
        }
    }

    async fn apply(&mut self, blobs: Vec<Option<Vec<u8>>>, seqs: Option<Vec<u64>>) -> Result<()> {
        let running = Arc::clone(self.running);
        blocking(move || {
            let mut ledger = running.ledger.lock();
            for (at, blob) in blobs.into_iter().enumerate() {
                let length = blob.as_ref().map_or(0, Vec::len);
                if let Some(blob) = blob {
                    match opened(&running.derived.sealer, running.derived.space, &blob)
                        .and_then(|plain| decoded_bundle(&plain))
                    {
                        Ok(changes) => {
                            ledger.arrived(running.catalog.as_ref(), &changes)?;
                        }
                        Err(error) => {
                            tracing::warn!(%error, "a synced record could not be read");
                        }
                    }
                }
                if let Some(seq) = seqs.as_ref().and_then(|seqs| seqs.get(at)) {
                    ledger.passed(*seq, length)?;
                }
            }
            Ok(())
        })
        .await?;
        self.running.publish_devices(&self.online);
        Ok(())
    }

    async fn watched(&mut self) -> Result<()> {
        let revision = self.running.catalog.revision();
        let now = Instant::now();
        if revision != self.seen_revision && self.look_at.is_none() {
            self.look_at = Some(now + LOOK_AFTER);
        }
        if now.duration_since(self.looked_at) >= LOOK_ANYWAY_EVERY && self.look_at.is_none() {
            self.look_at = Some(now);
        }
        if self.caught_up && self.look_at.is_some_and(|at| at <= now) {
            self.look_at = None;
            self.looked_at = now;
            self.seen_revision = revision;
            self.look().await?;
        }
        Ok(())
    }

    async fn look(&mut self) -> Result<()> {
        let running = Arc::clone(self.running);
        let looked = blocking(move || {
            let mut ledger = running.ledger.lock();
            ledger.settle(running.catalog.as_ref())?;
            ledger.look(running.catalog.as_ref(), &running.me)
        })
        .await?;
        self.running.shown.write().owed = looked.owed;
        self.running.publish_devices(&self.online);
        self.looked_once = true;
        if self.in_flight.is_empty() {
            self.push().await?;
        }
        self.publish_settled();
        Ok(())
    }

    fn publish_settled(&self) {
        self.running.shown.write().settled =
            self.caught_up && self.looked_once && self.in_flight.is_empty();
    }

    async fn push(&mut self) -> Result<()> {
        let running = Arc::clone(self.running);
        let bundles = blocking(move || {
            let owed = running.ledger.lock().owed()?;
            let mut sealed_bundles = Vec::new();
            for changes in owed.into_iter().take(BLOBS_PER_APPEND) {
                let blob = sealed(
                    &running.derived.sealer,
                    running.derived.space,
                    &encoded_bundle(&changes),
                )?;
                sealed_bundles.push((changes, blob));
            }
            Ok(sealed_bundles)
        })
        .await?;
        if bundles.is_empty() {
            return Ok(());
        }
        let (changes, blobs): (Vec<_>, Vec<_>) = bundles.into_iter().unzip();
        self.in_flight
            .push_back(changes.into_iter().flatten().collect());
        send_frame(&mut self.send, &ToServer::Append { blobs }.encoded()).await
    }

    async fn compact(&mut self) -> Result<()> {
        if !self.caught_up || self.compacting {
            return Ok(());
        }
        let running = Arc::clone(self.running);
        let snapshot = blocking(move || {
            let ledger = running.ledger.lock();
            let (records, bytes) = ledger.since_snapshot()?;
            if records < COMPACT_PAST_RECORDS && bytes < COMPACT_PAST_BYTES {
                return Ok(None);
            }
            let through = ledger.cursor()?;
            let mut parts = Vec::new();
            let mut total = 0;
            for changes in ledger.snapshot()? {
                let blob = sealed(
                    &running.derived.sealer,
                    running.derived.space,
                    &encoded_bundle(&changes),
                )?;
                total += blob.len();
                parts.push(blob);
            }
            if total > SNAPSHOT_FRAME_SHARE {
                tracing::warn!(
                    total,
                    "the synced state is too large to compact in one frame"
                );
                return Ok(None);
            }
            Ok(Some((through, parts)))
        })
        .await?;
        if let Some((through, parts)) = snapshot {
            self.compacting = true;
            send_frame(
                &mut self.send,
                &ToServer::Compact { through, parts }.encoded(),
            )
            .await?;
        }
        Ok(())
    }

    async fn follow(&mut self, device: DeviceId) -> Result<()> {
        let follow = self.running.follow;
        if device == self.running.device
            || !(follow.everything || follow.favourites || follow.playlists)
        {
            return Ok(());
        }
        let now = Instant::now();
        if self
            .followed
            .get(&device)
            .is_some_and(|at| now.duration_since(*at) < FOLLOW_A_DEVICE_EVERY)
        {
            return Ok(());
        }
        self.followed.insert(device, now);
        self.want(device, Work::Follow).await
    }

    async fn want(&mut self, device: DeviceId, work: Work) -> Result<()> {
        if let Work::Fetch(recordings) = &work {
            for recording in recordings {
                self.running.transfer(recording, device, |_| {});
            }
        }
        if let Some(connection) = self
            .peers
            .get(&device)
            .filter(|connection| connection.close_reason().is_none())
            && self.running.transfers_allowed()
        {
            spawn_work(Arc::clone(self.running), device, connection.clone(), work);
            return Ok(());
        }
        let first = !self.waiting.contains_key(&device);
        self.waiting.entry(device).or_default().push(work);
        if first && self.online.contains(&device) && self.running.transfers_allowed() {
            self.punch(device).await?;
        }
        Ok(())
    }

    async fn start_waiting(&mut self) -> Result<()> {
        let devices = self.waiting.keys().copied().collect::<Vec<_>>();
        for device in devices {
            if self.online.contains(&device) {
                self.punch(device).await?;
            }
        }
        Ok(())
    }

    async fn punch(&mut self, device: DeviceId) -> Result<()> {
        let candidates = self.candidates();
        let sealed = sealed(
            &self.running.derived.sealer,
            self.running.derived.space,
            &encoded_candidates(&candidates),
        )?;
        send_frame(
            &mut self.send,
            &ToServer::Punch { to: device, sealed }.encoded(),
        )
        .await
    }

    fn candidates(&self) -> Vec<SocketAddr> {
        let Ok(port) = self.running.endpoint.local_addr().map(|local| local.port()) else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        if self.server_address.ip().is_loopback() {
            candidates.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
        }
        let routed = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
            .and_then(|probe| {
                probe.connect(self.server_address)?;
                probe.local_addr()
            });
        if let Ok(routed) = routed
            && !routed.ip().is_unspecified()
            && !routed.ip().is_loopback()
        {
            candidates.push(SocketAddr::new(routed.ip(), port));
        }
        candidates
    }

    async fn punched(
        &mut self,
        device: DeviceId,
        address: SocketAddr,
        sealed: &[u8],
    ) -> Result<()> {
        if !self.running.transfers_allowed() {
            return Ok(());
        }
        if !self.waiting.contains_key(&device) {
            self.punch(device).await?;
        }
        let mut candidates = vec![address];
        match opened(
            &self.running.derived.sealer,
            self.running.derived.space,
            sealed,
        )
        .and_then(|plain| decoded_candidates(&plain))
        {
            Ok(theirs) => candidates.extend(theirs),
            Err(error) => tracing::debug!(%error, "a peer's candidates could not be read"),
        }
        candidates.dedup();
        let running = Arc::clone(self.running);
        let events = self.events.clone();
        tokio::spawn(async move {
            let event =
                match dialled(&running.endpoint, &running.to_peers, &candidates, device).await {
                    Ok(connection) => PeerEvent::Connected(device, connection),
                    Err(error) => {
                        tracing::debug!(%error, "a device could not be reached directly");
                        PeerEvent::Unreached(device)
                    }
                };
            let _ = events.send(event);
        });
        Ok(())
    }

    async fn peer_event(&mut self, event: PeerEvent) {
        match event {
            PeerEvent::Connected(device, connection) => {
                for work in self.waiting.remove(&device).unwrap_or_default() {
                    spawn_work(Arc::clone(self.running), device, connection.clone(), work);
                }
                self.peers.insert(device, connection);
            }
            PeerEvent::Unreached(device) => {
                if self.peers.contains_key(&device) {
                    return;
                }
                for work in self.waiting.remove(&device).unwrap_or_default() {
                    if let Work::Fetch(recordings) = work {
                        for recording in &recordings {
                            self.running.transfer(recording, device, |transfer| {
                                transfer.state = TransferState::Failed;
                            });
                        }
                    }
                }
            }
        }
    }
}

fn spawn_work(running: Arc<Running>, device: DeviceId, connection: Connection, work: Work) {
    tokio::spawn(async move {
        let recordings = match work {
            Work::Fetch(recordings) => recordings,
            Work::Follow => match followed_songs(&running, &connection).await {
                Ok(recordings) => recordings,
                Err(error) => {
                    tracing::debug!(%error, "a device's songs could not be listed");
                    return;
                }
            },
        };
        for recording in recordings {
            fetch_one(&running, device, &connection, recording).await;
        }
    });
}

async fn followed_songs(running: &Arc<Running>, connection: &Connection) -> Result<Vec<Recording>> {
    let holdings = holdings_of(connection).await?;
    let held_running = Arc::clone(running);
    let wanted = blocking(move || {
        let follow = held_running.follow;
        let catalog = held_running.catalog.as_ref();
        if follow.everything {
            let mut missing = Vec::new();
            for held in holdings {
                if !catalog.holds(&held.recording).map_err(Error::Catalog)? {
                    missing.push(held.recording);
                }
            }
            return Ok(missing);
        }
        let wanted = held_running.ledger.lock().wanted(follow, catalog)?;
        Ok(wanted
            .into_iter()
            .filter(|recording| {
                holdings
                    .iter()
                    .any(|held| held.recording.names_the_same_song_as(recording))
            })
            .collect())
    })
    .await?;
    Ok(wanted)
}

async fn fetch_one(
    running: &Arc<Running>,
    device: DeviceId,
    connection: &Connection,
    recording: Recording,
) {
    running.transfer(&recording, device, |transfer| {
        transfer.state = TransferState::Moving;
    });
    let staged = running.staging.join(format!(
        "{}.part",
        hex(&digest(&SHA256, recording.key().as_str().as_bytes()).as_ref()[..12])
    ));
    if let Err(error) = tokio::fs::create_dir_all(&running.staging).await {
        tracing::warn!(%error, "the sync staging folder could not be made");
    }
    let progress = Arc::clone(running);
    let progress_recording = recording.clone();
    let fetched = fetch(
        connection,
        device,
        &recording,
        &staged,
        move |received, size| {
            progress.transfer(&progress_recording, device, |transfer| {
                transfer.received = received;
                transfer.size = size;
            });
        },
    )
    .await;
    let landed = match fetched {
        Ok(fetched) => {
            let catalog = Arc::clone(&running.catalog);
            let landing = recording.clone();
            let staged = fetched.staged.clone();
            let landed = blocking(move || {
                catalog
                    .land(&landing, &fetched.extension, &fetched.staged)
                    .map_err(Error::Catalog)
            })
            .await;
            let _ = tokio::fs::remove_file(&staged).await;
            landed
        }
        Err(error) => Err(error),
    };
    let state = match landed {
        Ok(true) => TransferState::Landed,
        Ok(false) => TransferState::Failed,
        Err(error) => {
            tracing::warn!(%error, title = %recording.title, "a song could not be fetched from another device");
            TransferState::Failed
        }
    };
    running.transfer(&recording, device, |transfer| transfer.state = state);
}

async fn accept_peers(running: Arc<Running>) {
    while let Some(incoming) = running.endpoint.accept().await {
        if !running.transfers_allowed() {
            incoming.refuse();
            continue;
        }
        let catalog = Arc::clone(&running.catalog);
        tokio::spawn(async move {
            match incoming.await {
                Ok(connection) => serve(connection, catalog).await,
                Err(error) => tracing::debug!(%error, "a device's connection failed"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_names_its_port_or_takes_the_hosted_one() {
        assert_eq!(
            host_and_port("resonate.tecnio.dev"),
            ("resonate.tecnio.dev".into(), SERVER_PORT)
        );
        assert_eq!(
            host_and_port("sync.example:7443"),
            ("sync.example".into(), 7443)
        );
        assert_eq!(host_and_port("127.0.0.1:9000"), ("127.0.0.1".into(), 9000));
        assert_eq!(host_and_port("[::1]:9000"), ("::1".into(), 9000));
        assert_eq!(
            host_and_port("2001:db8::1"),
            ("2001:db8::1".into(), SERVER_PORT)
        );
    }
}
