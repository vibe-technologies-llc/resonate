use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::pending,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use ahash::AHashSet;
use parking_lot::{Mutex, RwLock};
use quinn::{ClientConfig, Connection, Endpoint, EndpointConfig, SendStream, TokioRuntime};
use resonate_core::Recording;
use ring::digest::{SHA256, SHA256_OUTPUT_LEN, digest};
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, sleep_until, timeout},
};

use crate::{
    Catalog, Change, DeviceId, Error, Handoff, Ledger, LocalPlaylistId, Platform, Result, SyncKey,
    key::{Derived, hex, random_bytes},
    ledger::{Follow, Me},
    nearby::{BEACON_EVERY, LARGEST_BEACON, Nearby, beacon_bytes, beacon_of, bound},
    peer::{Answering, Heard, blocking, dialled, fetch, holdings_of, meet, serve, tell},
    quic::{PeerIdentity, read_frame, send_frame, to_the_server, trusted_roots},
    seal::{opened, sealed},
    wire::{
        Beacon, FromServer, LARGEST_FRAME, PROTOCOL_VERSION, Refusal, ToServer, decoded_bundle,
        decoded_candidates, encoded_bundle, encoded_candidates, encoded_change,
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
const DIAL_A_NEIGHBOUR_EVERY: Duration = Duration::from_secs(30);
const HEARD_FOR: Duration = Duration::from_secs(60);

pub struct Settings {
    pub server: Option<String>,
    pub trusted: Option<PathBuf>,
    pub key: SyncKey,
    pub me: Me,
    pub follow: Follow,
    pub ledger: PathBuf,
    pub staging: PathBuf,
    pub transfers_allowed: bool,
    pub nearby: Option<Nearby>,
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
    pub nearby: bool,
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
    nearby_port: Option<u16>,
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
    beacons: Option<tokio::net::UdpSocket>,
    told: Vec<SocketAddr>,
    events: UnboundedSender<Event>,
    answering: Arc<Answering>,
}

enum Work {
    Fetch(Vec<Recording>),
    Follow,
}

enum Event {
    Linked(Result<Link>),
    Punched(DeviceId, Option<Connection>),
    Met(DeviceId, Connection),
    Unmet(DeviceId),
    Gone(DeviceId, usize),
}

struct Link {
    connection: Connection,
    send: SendStream,
    frames: UnboundedReceiver<Result<FromServer>>,
    reading: JoinHandle<()>,
    address: SocketAddr,
    online: Vec<DeviceId>,
    head: u64,
    caught_up: bool,
    in_flight: VecDeque<Vec<Change>>,
    compacting: bool,
}

type ToldDigest = [u8; SHA256_OUTPUT_LEN];

struct Neighbour {
    connection: Connection,
    told: AHashSet<ToldDigest>,
}

struct Syncing {
    running: Arc<Running>,
    server: Option<Link>,
    retry_at: Option<Instant>,
    wait: Duration,
    server_online: BTreeSet<DeviceId>,
    caught_up_once: bool,
    neighbour_caught_up: bool,
    neighbours: BTreeMap<DeviceId, Neighbour>,
    heard: BTreeMap<DeviceId, (SocketAddr, Instant)>,
    dialled_at: BTreeMap<DeviceId, Instant>,
    peers: BTreeMap<DeviceId, Connection>,
    waiting: BTreeMap<DeviceId, Vec<Work>>,
    followed: BTreeMap<DeviceId, Instant>,
    look_at: Option<Instant>,
    looked_at: Instant,
    looked_once: bool,
    seen_revision: u64,
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
        let beacon_socket = settings.nearby.as_ref().map(bound).transpose()?;
        let nearby_port = beacon_socket
            .as_ref()
            .and_then(|socket| socket.local_addr().ok())
            .map(|local| local.port());
        let (endpoint, beacons) = {
            let _entered = runtime.enter();
            let endpoint = Endpoint::new(
                EndpointConfig::default(),
                Some(serving),
                socket,
                Arc::new(TokioRuntime),
            )
            .map_err(Error::Bind)?;
            let beacons = beacon_socket
                .map(tokio::net::UdpSocket::from_std)
                .transpose()
                .map_err(Error::BeaconBind)?;
            (endpoint, beacons)
        };

        let (events, events_received) = mpsc::unbounded_channel();
        let (heard, heard_received) = mpsc::unbounded_channel();
        let answering = Arc::new(Answering {
            catalog: Arc::clone(&catalog),
            ledger: Arc::clone(&ledger),
            device,
            heard,
        });
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
            beacons,
            told: settings
                .nearby
                .map(|nearby| nearby.told)
                .unwrap_or_default(),
            events,
            answering,
        });

        let (commands, received) = mpsc::unbounded_channel();
        let thread = thread::Builder::new()
            .name("resonate-sync".into())
            .spawn(move || {
                ran(
                    &runtime,
                    running,
                    Inbound {
                        commands: received,
                        events: events_received,
                        heard: heard_received,
                    },
                );
            })
            .map_err(Error::Thread)?;

        Ok(Self {
            commands,
            shown,
            ledger,
            code: settings.key.code(),
            device,
            nearby_port,
            thread: Some(thread),
        })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub const fn device(&self) -> DeviceId {
        self.device
    }

    pub const fn nearby_port(&self) -> Option<u16> {
        self.nearby_port
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

struct Inbound {
    commands: UnboundedReceiver<Command>,
    events: UnboundedReceiver<Event>,
    heard: UnboundedReceiver<Heard>,
}

fn ran(runtime: &Runtime, running: Arc<Running>, inbound: Inbound) {
    runtime.block_on(run(running, inbound));
}

async fn run(running: Arc<Running>, mut inbound: Inbound) {
    let accepting = tokio::spawn(accept_peers(Arc::clone(&running)));
    let mut syncing = Syncing::new(Arc::clone(&running));
    let mut watch = interval(WATCH_EVERY);
    watch.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut beacon_at = Instant::now();
    let mut beacon = vec![0; LARGEST_BEACON];
    loop {
        let outcome = tokio::select! {
            frame = next_frame(&mut syncing.server) => match frame {
                None => {
                    syncing.unlinked(None);
                    Ok(())
                }
                Some(Err(error)) => {
                    syncing.unlinked(Some(error));
                    Ok(())
                }
                Some(Ok(frame)) => {
                    if let Err(error) = syncing.heard_from_server(frame).await {
                        syncing.unlinked(Some(error));
                    }
                    Ok(())
                }
            },
            () = until(syncing.retry_at) => {
                syncing.connect();
                Ok(())
            }
            command = inbound.commands.recv() => match command {
                None | Some(Command::Stop) => break,
                Some(Command::Transfers(allowed)) => syncing.transfers(allowed).await,
                Some(Command::Fetch { recordings, from }) => syncing.want(from, Work::Fetch(recordings)).await,
            },
            Some(event) = inbound.events.recv() => syncing.event(event).await,
            Some(heard) = inbound.heard.recv() => syncing.heard_from_peer(heard).await,
            Some((length, from)) = beacon_heard(running.beacons.as_ref(), &mut beacon) => {
                syncing.beacon_heard(&beacon[..length], from).await;
                Ok(())
            }
            () = sleep_until(beacon_at) => {
                syncing.beacon().await;
                beacon_at = Instant::now() + jittered(BEACON_EVERY);
                Ok(())
            }
            _ = watch.tick() => syncing.watched().await,
        };
        if let Err(error) = outcome {
            tracing::warn!(%error, "sync could not carry on with something");
        }
    }
    syncing.bye().await;
    accepting.abort();
    leave(&running).await;
}

async fn next_frame(server: &mut Option<Link>) -> Option<Result<FromServer>> {
    match server {
        Some(link) => link.frames.recv().await,
        None => pending().await,
    }
}

async fn until(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => pending().await,
    }
}

async fn beacon_heard(
    socket: Option<&tokio::net::UdpSocket>,
    buffer: &mut [u8],
) -> Option<(usize, SocketAddr)> {
    let Some(socket) = socket else {
        return pending().await;
    };
    match socket.recv_from(buffer).await {
        Ok(heard) => Some(heard),
        Err(error) => {
            tracing::debug!(%error, "a beacon could not be heard");
            None
        }
    }
}

async fn leave(running: &Running) {
    running.endpoint.close(0_u32.into(), b"leaving");
    let _ = timeout(LEAVE_WITHIN, running.endpoint.wait_idle()).await;
}

fn jittered(wait: Duration) -> Duration {
    let spread = random_bytes::<1>().map(|[byte]| byte).unwrap_or_default();
    wait + wait * u32::from(spread) / 1024
}

fn told_digest(change: &Change) -> ToldDigest {
    let mut told = [0; SHA256_OUTPUT_LEN];
    told.copy_from_slice(digest(&SHA256, &encoded_change(change)).as_ref());
    told
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

async fn linked(running: Arc<Running>) -> Result<Link> {
    let (address, host) = resolve(&running.server).await?;
    let connection = running
        .endpoint
        .connect_with(running.to_server.clone(), address, &host)
        .map_err(|source| Error::Connect(Box::new(source)))?
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    let (mut send, recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;

    let (frames_in, mut frames) = mpsc::unbounded_channel();
    let reading = tokio::spawn(read_frames(recv, frames_in));
    let welcomed = welcomed(&running, &mut send, &mut frames).await;
    let (online, head, cursor) = match welcomed {
        Ok(welcomed) => welcomed,
        Err(error) => {
            reading.abort();
            connection.close(0_u32.into(), b"leaving");
            return Err(error);
        }
    };
    tracing::info!(server = %running.server, "syncing with the server");
    Ok(Link {
        connection,
        send,
        frames,
        reading,
        address,
        online,
        head,
        caught_up: cursor >= head,
        in_flight: VecDeque::new(),
        compacting: false,
    })
}

async fn welcomed(
    running: &Running,
    send: &mut SendStream,
    frames: &mut UnboundedReceiver<Result<FromServer>>,
) -> Result<(Vec<DeviceId>, u64, u64)> {
    let cursor = running.ledger.lock().cursor()?;
    let hello = ToServer::Hello {
        version: PROTOCOL_VERSION,
        space: running.derived.space,
        token: running.derived.token.clone(),
        device: running.device,
        after: cursor,
    };
    send_frame(send, &hello.encoded()).await?;
    match frames.recv().await {
        Some(Ok(FromServer::Welcome { online, head, .. })) => Ok((online, head, cursor)),
        Some(Ok(FromServer::Refused(refusal))) => Err(Error::Refused(refusal)),
        Some(Err(error)) => Err(error),
        Some(Ok(_)) | None => Err(Error::Unwelcome),
    }
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

impl Syncing {
    fn new(running: Arc<Running>) -> Self {
        let now = Instant::now();
        Self {
            running,
            server: None,
            retry_at: Some(now),
            wait: FIRST_RETRY,
            server_online: BTreeSet::new(),
            caught_up_once: false,
            neighbour_caught_up: false,
            neighbours: BTreeMap::new(),
            heard: BTreeMap::new(),
            dialled_at: BTreeMap::new(),
            peers: BTreeMap::new(),
            waiting: BTreeMap::new(),
            followed: BTreeMap::new(),
            look_at: None,
            looked_at: now,
            looked_once: false,
            seen_revision: 0,
        }
    }

    fn connect(&mut self) {
        self.retry_at = None;
        self.running.stand(Standing::Connecting);
        self.publish_settled();
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            let link = linked(Arc::clone(&running)).await;
            let _ = running.events.send(Event::Linked(link));
        });
    }

    async fn link(&mut self, link: Link) -> Result<()> {
        self.server_online = link.online.iter().copied().collect();
        if link.caught_up {
            self.caught_up_once = true;
        }
        self.server = Some(link);
        self.look_at = Some(Instant::now());
        self.running.stand(Standing::Connected);
        self.publish_devices();
        self.start_waiting().await?;
        for device in self.server_online.clone() {
            self.follow(device).await?;
        }
        Ok(())
    }

    fn unlinked(&mut self, error: Option<Error>) {
        if let Some(link) = self.server.take() {
            link.reading.abort();
            link.connection.close(0_u32.into(), b"leaving");
        }
        self.server_online.clear();
        match error {
            None => {
                self.running.stand(Standing::Unreachable);
                self.wait = FIRST_RETRY;
            }
            Some(Error::Refused(refusal)) => {
                tracing::warn!(?refusal, "the sync server refused this device");
                self.running.stand(Standing::Refused(refusal));
                self.wait = LAST_RETRY;
            }
            Some(error) => {
                tracing::debug!(%error, server = %self.running.server, "the sync server could not be reached");
                self.running.stand(Standing::Unreachable);
            }
        }
        self.retry_at = Some(Instant::now() + jittered(self.wait));
        self.wait = (self.wait * 2).min(LAST_RETRY);
        self.publish_devices();
        self.publish_settled();
    }

    async fn bye(&mut self) {
        if let Some(mut link) = self.server.take() {
            let _ = send_frame(&mut link.send, &ToServer::Bye.encoded()).await;
            let _ = link.send.finish();
            link.reading.abort();
            link.connection.close(0_u32.into(), b"leaving");
        }
    }

    async fn transfers(&mut self, allowed: bool) -> Result<()> {
        self.running
            .transfers_allowed
            .store(allowed, Ordering::Relaxed);
        if allowed {
            self.start_waiting().await?;
        }
        Ok(())
    }

    async fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Linked(Ok(link)) => self.link(link).await?,
            Event::Linked(Err(error)) => self.unlinked(Some(error)),
            Event::Punched(device, Some(connection)) => {
                for work in self.waiting.remove(&device).unwrap_or_default() {
                    spawn_work(Arc::clone(&self.running), device, connection.clone(), work);
                }
                self.peers.insert(device, connection);
            }
            Event::Punched(device, None) | Event::Unmet(device) => self.unreached(device),
            Event::Met(device, connection) => self.met(device, connection, true).await?,
            Event::Gone(device, id) => {
                if self
                    .peers
                    .get(&device)
                    .is_some_and(|connection| connection.stable_id() == id)
                {
                    self.peers.remove(&device);
                }
                if self
                    .neighbours
                    .get(&device)
                    .is_some_and(|neighbour| neighbour.connection.stable_id() == id)
                {
                    self.neighbours.remove(&device);
                    self.publish_devices();
                }
            }
        }
        Ok(())
    }

    async fn heard_from_peer(&mut self, heard: Heard) -> Result<()> {
        match heard {
            Heard::Met {
                device,
                connection,
                records,
            } => self.met(device, connection, records).await,
            Heard::Told { whole } => {
                if whole && !self.neighbour_caught_up {
                    self.neighbour_caught_up = true;
                    self.look_at = Some(Instant::now());
                }
                self.publish_devices();
                Ok(())
            }
        }
    }

    async fn heard_from_server(&mut self, frame: FromServer) -> Result<()> {
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
                let sent = self
                    .server
                    .as_mut()
                    .and_then(|link| link.in_flight.pop_front());
                if let Some(changes) = sent {
                    let ledger = Arc::clone(&self.running.ledger);
                    blocking(move || ledger.lock().sent(&changes)).await?;
                }
                self.push().await?;
                self.publish_settled();
            }
            FromServer::Arrived { device } => {
                self.server_online.insert(device);
                self.publish_devices();
                if self.waiting.contains_key(&device) {
                    self.punch(device).await?;
                }
                self.follow(device).await?;
            }
            FromServer::Left { device } => {
                self.server_online.remove(&device);
                self.publish_devices();
            }
            FromServer::Punched {
                device,
                address,
                sealed,
            } => self.punched(device, address, &sealed).await?,
            FromServer::Compacted { through } => {
                if let Some(link) = self.server.as_mut() {
                    link.compacting = false;
                }
                let ledger = Arc::clone(&self.running.ledger);
                blocking(move || ledger.lock().compacted(through)).await?;
            }
            FromServer::Refused(Refusal::Stale) => {
                if let Some(link) = self.server.as_mut() {
                    link.compacting = false;
                }
            }
            FromServer::Refused(Refusal::TooLarge) => {
                tracing::warn!("the sync server refused a record as too large");
                if let Some(link) = self.server.as_mut() {
                    link.in_flight.pop_front();
                }
            }
            FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
            FromServer::Welcome { .. } => {}
        }
        Ok(())
    }

    fn caught_up_through(&mut self, through: u64) {
        let Some(link) = self.server.as_mut() else {
            return;
        };
        if !link.caught_up && through >= link.head {
            link.caught_up = true;
            self.caught_up_once = true;
            self.look_at = Some(Instant::now());
        }
    }

    fn may_look(&self) -> bool {
        match &self.server {
            Some(link) => link.caught_up || self.neighbour_caught_up,
            None => self.caught_up_once || self.neighbour_caught_up,
        }
    }

    fn publish_devices(&self) {
        let notes = match self.running.ledger.lock().devices() {
            Ok(notes) => notes,
            Err(error) => {
                tracing::warn!(%error, "the synced devices could not be read");
                return;
            }
        };
        let this = self.running.device;
        let devices = notes
            .into_iter()
            .map(|note| {
                let nearby = self.neighbours.contains_key(&note.device);
                SyncDevice {
                    online: nearby
                        || self.server_online.contains(&note.device)
                        || note.device == this,
                    nearby,
                    this: note.device == this,
                    id: note.device,
                    name: note.name,
                    platform: note.platform,
                }
            })
            .collect();
        self.running.shown.write().devices = devices;
    }

    async fn apply(&mut self, blobs: Vec<Option<Vec<u8>>>, seqs: Option<Vec<u64>>) -> Result<()> {
        let running = Arc::clone(&self.running);
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
        self.publish_devices();
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
        if self.may_look() && self.look_at.is_some_and(|at| at <= now) {
            self.look_at = None;
            self.looked_at = now;
            self.seen_revision = revision;
            self.look().await?;
        }
        Ok(())
    }

    async fn look(&mut self) -> Result<()> {
        let running = Arc::clone(&self.running);
        let looked = blocking(move || {
            let mut ledger = running.ledger.lock();
            ledger.settle(running.catalog.as_ref())?;
            ledger.look(running.catalog.as_ref(), &running.me)
        })
        .await?;
        self.running.shown.write().owed = looked.owed;
        self.publish_devices();
        self.looked_once = true;
        self.tell_neighbours().await?;
        self.push().await?;
        self.publish_settled();
        Ok(())
    }

    fn publish_settled(&self) {
        let flown = self
            .server
            .as_ref()
            .is_none_or(|link| link.in_flight.is_empty());
        self.running.shown.write().settled = self.may_look() && self.looked_once && flown;
    }

    async fn push(&mut self) -> Result<()> {
        if self
            .server
            .as_ref()
            .is_none_or(|link| !link.in_flight.is_empty())
        {
            return Ok(());
        }
        let running = Arc::clone(&self.running);
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
        let Some(link) = self.server.as_mut() else {
            return Ok(());
        };
        if bundles.is_empty() {
            return Ok(());
        }
        let (changes, blobs): (Vec<_>, Vec<_>) = bundles.into_iter().unzip();
        link.in_flight
            .push_back(changes.into_iter().flatten().collect());
        send_frame(&mut link.send, &ToServer::Append { blobs }.encoded()).await
    }

    async fn tell_neighbours(&mut self) -> Result<()> {
        if self.neighbours.is_empty() {
            return Ok(());
        }
        let ledger = Arc::clone(&self.running.ledger);
        let owed = blocking(move || ledger.lock().owed()).await?;
        let owed = owed.into_iter().flatten().collect::<Vec<_>>();
        for (device, neighbour) in &mut self.neighbours {
            let fresh = owed
                .iter()
                .filter(|change| neighbour.told.insert(told_digest(change)))
                .cloned()
                .collect::<Vec<_>>();
            if !fresh.is_empty() {
                spawn_tell(*device, neighbour.connection.clone(), false, fresh);
            }
        }
        Ok(())
    }

    async fn compact(&mut self) -> Result<()> {
        if self
            .server
            .as_ref()
            .is_none_or(|link| !link.caught_up || link.compacting)
        {
            return Ok(());
        }
        let running = Arc::clone(&self.running);
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
        if let Some((through, parts)) = snapshot
            && let Some(link) = self.server.as_mut()
        {
            link.compacting = true;
            send_frame(
                &mut link.send,
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
            spawn_work(Arc::clone(&self.running), device, connection.clone(), work);
            return Ok(());
        }
        let first = !self.waiting.contains_key(&device);
        self.waiting.entry(device).or_default().push(work);
        if first && self.running.transfers_allowed() {
            self.reach(device).await?;
        }
        Ok(())
    }

    async fn start_waiting(&mut self) -> Result<()> {
        if !self.running.transfers_allowed() {
            return Ok(());
        }
        let devices = self.waiting.keys().copied().collect::<Vec<_>>();
        for device in devices {
            self.reach(device).await?;
        }
        Ok(())
    }

    async fn reach(&mut self, device: DeviceId) -> Result<()> {
        if self.server_online.contains(&device) {
            self.punch(device).await
        } else {
            self.dial_neighbour(device);
            Ok(())
        }
    }

    fn unreached(&mut self, device: DeviceId) {
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

    async fn punch(&mut self, device: DeviceId) -> Result<()> {
        let candidates = self.candidates();
        let Some(link) = self.server.as_mut() else {
            return Ok(());
        };
        let sealed = sealed(
            &self.running.derived.sealer,
            self.running.derived.space,
            &encoded_candidates(&candidates),
        )?;
        send_frame(
            &mut link.send,
            &ToServer::Punch { to: device, sealed }.encoded(),
        )
        .await
    }

    fn candidates(&self) -> Vec<SocketAddr> {
        let Some(server_address) = self.server.as_ref().map(|link| link.address) else {
            return Vec::new();
        };
        let Ok(port) = self.running.endpoint.local_addr().map(|local| local.port()) else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        if server_address.ip().is_loopback() {
            candidates.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
        }
        let routed = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
            .and_then(|probe| {
                probe.connect(server_address)?;
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
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            let connection =
                match dialled(&running.endpoint, &running.to_peers, &candidates, device).await {
                    Ok(connection) => Some(connection),
                    Err(error) => {
                        tracing::debug!(%error, "a device could not be reached directly");
                        None
                    }
                };
            let _ = running.events.send(Event::Punched(device, connection));
        });
        Ok(())
    }

    async fn beacon(&mut self) {
        let Some(socket) = self.running.beacons.as_ref() else {
            return;
        };
        if !self.running.transfers_allowed() || self.running.told.is_empty() {
            return;
        }
        let Some(bytes) = self.beacon_bytes(false) else {
            return;
        };
        for told in &self.running.told {
            if let Err(error) = socket.send_to(&bytes, told).await {
                tracing::debug!(%error, %told, "a beacon could not be sent");
            }
        }
    }

    fn beacon_bytes(&self, answering: bool) -> Option<Vec<u8>> {
        let port = self.running.endpoint.local_addr().ok()?.port();
        let beacon = Beacon {
            device: self.running.device,
            port,
            answering,
        };
        beacon_bytes(&self.running.derived, beacon)
            .inspect_err(|error| tracing::debug!(%error, "a beacon could not be sealed"))
            .ok()
    }

    async fn beacon_heard(&mut self, bytes: &[u8], from: SocketAddr) {
        if !self.running.transfers_allowed() {
            return;
        }
        let Some(beacon) = beacon_of(&self.running.derived, bytes) else {
            return;
        };
        if beacon.device == self.running.device {
            return;
        }
        let at = SocketAddr::new(from.ip(), beacon.port);
        self.heard.insert(beacon.device, (at, Instant::now()));
        if self.neighbours.contains_key(&beacon.device) {
            return;
        }
        if !beacon.answering
            && let Some(socket) = self.running.beacons.as_ref()
            && let Some(answer) = self.beacon_bytes(true)
            && let Err(error) = socket.send_to(&answer, from).await
        {
            tracing::debug!(%error, %from, "a beacon could not be answered");
        }
        if self.running.device < beacon.device {
            self.dial_neighbour(beacon.device);
        }
    }

    fn dial_neighbour(&mut self, device: DeviceId) {
        let now = Instant::now();
        let Some(at) = self
            .heard
            .get(&device)
            .filter(|(_, heard)| now.duration_since(*heard) < HEARD_FOR)
            .map(|(at, _)| *at)
        else {
            return;
        };
        if self
            .dialled_at
            .get(&device)
            .is_some_and(|dialled| now.duration_since(*dialled) < DIAL_A_NEIGHBOUR_EVERY)
        {
            return;
        }
        self.dialled_at.insert(device, now);
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            let event = match neighbour_dialled(&running, at, device).await {
                Ok((met, connection)) => Event::Met(met, connection),
                Err(error) => {
                    tracing::debug!(%error, %at, "a device on this network could not be reached");
                    Event::Unmet(device)
                }
            };
            let _ = running.events.send(event);
        });
    }

    async fn met(&mut self, device: DeviceId, connection: Connection, records: bool) -> Result<()> {
        if device == self.running.device {
            return Ok(());
        }
        let events = self.running.events.clone();
        let closing = connection.clone();
        tokio::spawn(async move {
            closing.closed().await;
            let _ = events.send(Event::Gone(device, closing.stable_id()));
        });
        for work in self.waiting.remove(&device).unwrap_or_default() {
            spawn_work(Arc::clone(&self.running), device, connection.clone(), work);
        }
        self.peers.insert(device, connection.clone());
        if records {
            let ledger = Arc::clone(&self.running.ledger);
            let snapshot = blocking(move || ledger.lock().snapshot()).await?;
            let changes = snapshot.into_iter().flatten().collect::<Vec<_>>();
            let told = changes.iter().map(told_digest).collect();
            self.neighbours.insert(
                device,
                Neighbour {
                    connection: connection.clone(),
                    told,
                },
            );
            spawn_tell(device, connection, true, changes);
            tracing::info!("syncing with a device on this network");
        }
        self.publish_devices();
        self.follow(device).await
    }
}

async fn neighbour_dialled(
    running: &Arc<Running>,
    at: SocketAddr,
    device: DeviceId,
) -> Result<(DeviceId, Connection)> {
    let connection = dialled(&running.endpoint, &running.to_peers, &[at], device).await?;
    let met = meet(&connection, running.device, true).await?;
    tokio::spawn(serve(
        connection.clone(),
        Arc::clone(&running.answering),
        Some(met),
    ));
    Ok((met, connection))
}

fn spawn_tell(device: DeviceId, connection: Connection, whole: bool, changes: Vec<Change>) {
    tokio::spawn(async move {
        if let Err(error) = tell(&connection, device, whole, changes).await {
            tracing::debug!(%error, "changes could not be told to a device on this network");
        }
    });
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
        let answering = Arc::clone(&running.answering);
        tokio::spawn(async move {
            match incoming.await {
                Ok(connection) => serve(connection, answering, None).await,
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
