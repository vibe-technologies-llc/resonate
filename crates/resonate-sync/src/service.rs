use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::{Future, pending},
    mem,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use parking_lot::{Mutex, RwLock};
use quinn::{
    AsyncUdpSocket, ClientConfig, Connection, Endpoint, EndpointConfig, Runtime as _, TokioRuntime,
};
use resonate_core::{Recording, RecordingKey};
use ring::digest::{SHA256, SHA256_OUTPUT_LEN, digest};
use rustls::ClientConfig as WebConfig;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, sleep_until, timeout},
};

use crate::{
    Catalog, Change, DeviceId, Error, Handoff, Ledger, LocalPlaylistId, Platform, Result, SyncKey,
    key::{Derived, PAIRING_LINK, hex, random_bytes},
    ledger::{Follow, Me},
    nearby::{
        BEACON_EVERY, Bonjour, Found, LARGEST_BEACON, Nearby, addresses_here,
        beacon_addresses_here, beacon_bytes, beacon_of, beacon_recorded, bound, bound_v6,
        global_addresses_here, record_of, told_over_ipv6,
    },
    peer::{
        Answering, Heard, blocking, covers_from, dialled, dialled_within, fetch, holdings_of,
        lyricsfiles_from, meet, prints_from, serve, tell,
    },
    quic::{KEEP_THE_SERVER_EVERY, PeerIdentity, to_the_server, trusted_roots},
    seal::{opened, sealed},
    stun::{Reflector, STUN_PORT},
    web::{Incoming, Outgoing, connect, to_the_web, web_address},
    wire::{
        Beacon, COVERS_ASKED_AT_MOST, FromServer, LARGEST_FRAME, LARGEST_RECORD,
        LYRICSFILES_ASKED_AT_MOST, PRINTS_ASKED_AT_MOST, PROTOCOL_VERSION, Refusal, ToServer,
        decoded_bundle, decoded_candidates, encoded_bundle, encoded_candidates, encoded_change,
    },
};

pub const HOSTED_SERVER: &str = resonate_core::RESONATE_SOCKET_ADDRESS;
pub const SERVER_PORT: u16 = 443;
const TRANSFERS_SHOWN_AT_MOST: usize = 200;
const FIRST_RETRY: Duration = Duration::from_secs(5);
const LAST_RETRY: Duration = Duration::from_secs(30 * 60);
const LOOK_AFTER: Duration = Duration::from_millis(300);
const FOLLOW_AGAIN_AFTER: Duration = Duration::from_millis(250);
const EVERYTHING_FOLLOWED_AGAIN_AT_MOST_EVERY: Duration = Duration::from_secs(10);
const FOLLOWED_AGAIN_AFTER_A_LOOK_EVERY: Duration = Duration::from_secs(60);
const LOOK_ANYWAY_EVERY: Duration = Duration::from_secs(5 * 60);
const WATCH_EVERY: Duration = Duration::from_millis(500);
const COMPACT_PAST_RECORDS: u64 = 512;
const COMPACT_PAST_BYTES: u64 = 4 * 1024 * 1024;
const BLOBS_PER_APPEND: usize = 32;
const FOLLOW_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
const COVERS_ASKED_OF_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
const LYRICSFILES_ASKED_OF_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
const PRINTS_ASKED_OF_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
pub const A_FAILED_SONG_WAITS: Duration = Duration::from_secs(30 * 60);
const LEAVE_WITHIN: Duration = Duration::from_secs(2);
const SNAPSHOT_FRAME_SHARE: usize = LARGEST_FRAME / 2;
const FIRST_REDIAL: Duration = Duration::from_secs(1);
const LAST_REDIAL: Duration = Duration::from_secs(30);
const DIAL_BACK_AFTER: Duration = Duration::from_millis(500);
const HEARD_FOR: Duration = Duration::from_secs(60);
const SERVER_ANSWERS_WITHIN: Duration = Duration::from_secs(15);
const PUNCH_ANSWERED_AT_MOST_EVERY: Duration = Duration::from_secs(10);
const RELAY_DIAL_WITHIN: Duration = Duration::from_secs(20);
const RELAYED_DATAGRAMS_QUEUED_AT_MOST: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Server {
    #[default]
    Hosted,
    At(String),
    Nowhere,
}

impl Server {
    fn address(self) -> Option<String> {
        match self {
            Self::At(server) if !server.trim().is_empty() => Some(server.trim().to_owned()),
            Self::Hosted | Self::At(_) => Some(HOSTED_SERVER.to_owned()),
            Self::Nowhere => None,
        }
    }
}

pub struct Settings {
    pub server: Server,
    pub stun: Option<String>,
    pub trusted: Option<PathBuf>,
    pub key: SyncKey,
    pub me: Me,
    pub follow: Follow,
    pub ledger: PathBuf,
    pub staging: PathBuf,
    pub transfers_allowed: bool,
    pub nearby: Option<Nearby>,
    pub relay_only: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standing {
    #[default]
    Starting,
    Connecting,
    Connected,
    Unreachable,
    Refused(Refusal),
    Nearby,
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

impl Shown {
    pub fn downloading(&self) -> usize {
        self.in_state(TransferState::Moving)
    }

    pub fn waiting(&self) -> usize {
        self.in_state(TransferState::Waiting)
    }

    fn in_state(&self, state: TransferState) -> usize {
        self.transfers
            .iter()
            .filter(|transfer| transfer.state == state)
            .count()
    }
}

enum Command {
    Fetch {
        recordings: Vec<Recording>,
        from: DeviceId,
    },
    Transfers(bool),
    Heard(Beacon),
    Force,
    Forgot,
    Stop,
}

pub struct Sync {
    commands: UnboundedSender<Command>,
    shown: Arc<RwLock<Shown>>,
    ledger: Arc<Mutex<Ledger>>,
    code: String,
    device: DeviceId,
    nearby_port: Option<u16>,
    running: Arc<Running>,
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
    server: Option<String>,
    stun: Option<String>,
    staging: PathBuf,
    endpoint: Endpoint,
    reflector: Arc<Reflector>,
    to_server: ClientConfig,
    to_web: Arc<WebConfig>,
    to_peers: ClientConfig,
    transfers_allowed: Arc<AtomicBool>,
    nearby: bool,
    bonjour: bool,
    relay_only: bool,
    beacons: Option<tokio::net::UdpSocket>,
    beacons_v6: Option<tokio::net::UdpSocket>,
    told: Vec<SocketAddr>,
    record: Mutex<Option<(Beacon, String)>>,
    events: UnboundedSender<Event>,
    answering: Arc<Answering>,
    fetching: Mutex<AHashSet<RecordingKey>>,
    failed_lately: Mutex<AHashMap<RecordingKey, Instant>>,
    lyricsfiles_skipped: Mutex<BTreeMap<DeviceId, usize>>,
    prints_skipped: Mutex<BTreeMap<DeviceId, usize>>,
}

enum Work {
    Fetch(Vec<Recording>),
    Follow,
}

enum Event {
    Linked(Result<Link>),
    Punched(DeviceId, Option<Connection>),
    Relayed(DeviceId, Option<Connection>),
    Met(DeviceId, Connection),
    Unmet(DeviceId),
    Redial(DeviceId),
    Gone(DeviceId, usize),
    CutShort(DeviceId),
    Dropped(usize, Error),
}

struct Pushed {
    changes: Vec<Change>,
    bytes: usize,
}

enum Carrier {
    Quic(Connection),
    Web(usize),
}

static WEB_LINKS: AtomicUsize = AtomicUsize::new(0);

impl Carrier {
    fn web() -> Self {
        Self::Web(usize::MAX - WEB_LINKS.fetch_add(1, Ordering::Relaxed))
    }

    fn id(&self) -> usize {
        match self {
            Self::Quic(connection) => connection.stable_id(),
            Self::Web(id) => *id,
        }
    }

    fn close(&self) {
        if let Self::Quic(connection) = self {
            connection.close(0_u32.into(), b"leaving");
        }
    }
}

struct Link {
    carrier: Carrier,
    reflector: Arc<Reflector>,
    relay_out: mpsc::Sender<Vec<u8>>,
    outgoing: UnboundedSender<Vec<u8>>,
    writing: JoinHandle<()>,
    frames: UnboundedReceiver<Result<FromServer>>,
    reading: JoinHandle<()>,
    address: Option<SocketAddr>,
    online: Vec<DeviceId>,
    head: u64,
    caught_up: bool,
    in_flight: VecDeque<Vec<Pushed>>,
    compacting: bool,
}

impl Link {
    fn say(&self, message: &ToServer) -> Result<()> {
        let payload = message.encoded();
        if payload.len() > LARGEST_FRAME {
            return Err(Error::Oversized {
                length: payload.len(),
            });
        }
        self.outgoing.send(payload).map_err(|_| Error::Stopped)
    }

    fn hang_up(self) {
        self.reflector.relay().detach();
        self.reading.abort();
        self.writing.abort();
        self.carrier.close();
    }
}

type ToldDigest = [u8; SHA256_OUTPUT_LEN];

struct Neighbour {
    connection: Connection,
    told: AHashSet<ToldDigest>,
}

#[derive(Clone, Copy)]
struct Redial {
    at: Instant,
    wait: Duration,
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
    heard: BTreeMap<DeviceId, (Vec<SocketAddr>, Instant)>,
    dialled_at: BTreeMap<DeviceId, Redial>,
    dialling_back: BTreeSet<DeviceId>,
    peers: BTreeMap<DeviceId, Connection>,
    punched_back: BTreeMap<DeviceId, Instant>,
    waiting: BTreeMap<DeviceId, Vec<Work>>,
    followed: BTreeMap<DeviceId, Instant>,
    covers_asked: BTreeMap<DeviceId, Instant>,
    lyricsfiles_asked: BTreeMap<DeviceId, Instant>,
    prints_asked: BTreeMap<DeviceId, Instant>,
    follow_again_at: Option<Instant>,
    followed_again_at: Option<Instant>,
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
        let to_web = to_the_web(roots.clone())?;
        let to_server = to_the_server(roots)?;
        let to_peers = identity.dialling()?;
        let serving = identity.serving()?;

        let server = settings.server.address();
        let mut ledger = Ledger::open(&settings.ledger)?;
        ledger.enter_space(derived.space)?;
        ledger.through_a_server(server.is_some())?;
        let device = ledger.device();
        let ledger = Arc::new(Mutex::new(ledger));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;
        let socket = peer_socket()?;
        let broadcast = settings
            .nearby
            .as_ref()
            .and_then(|nearby| nearby.broadcast.as_ref());
        let beacon_socket = broadcast.map(bound).transpose()?;
        let nearby_port = beacon_socket
            .as_ref()
            .and_then(|socket| socket.local_addr().ok())
            .map(|local| local.port());
        let beacon_socket_v6 = broadcast
            .zip(nearby_port)
            .and_then(|(broadcast, port)| bound_v6(broadcast, port));
        let (endpoint, reflector, beacons, beacons_v6) = {
            let _entered = runtime.enter();
            let reflector =
                Reflector::over(TokioRuntime.wrap_udp_socket(socket).map_err(Error::Bind)?);
            let endpoint = Endpoint::new_with_abstract_socket(
                EndpointConfig::default(),
                Some(serving),
                Arc::clone(&reflector) as Arc<dyn AsyncUdpSocket>,
                Arc::new(TokioRuntime),
            )
            .map_err(Error::Bind)?;
            let beacons = beacon_socket
                .map(tokio::net::UdpSocket::from_std)
                .transpose()
                .map_err(Error::BeaconBind)?;
            let beacons_v6 = beacon_socket_v6
                .map(tokio::net::UdpSocket::from_std)
                .transpose()
                .map_err(Error::BeaconBind)?;
            (endpoint, reflector, beacons, beacons_v6)
        };

        let (events, events_received) = mpsc::unbounded_channel();
        let (heard, heard_received) = mpsc::unbounded_channel();
        let transfers_allowed = Arc::new(AtomicBool::new(settings.transfers_allowed));
        let answering = Arc::new(Answering {
            catalog: Arc::clone(&catalog),
            ledger: Arc::clone(&ledger),
            device,
            heard,
            transfers_allowed: Arc::clone(&transfers_allowed),
        });
        let shown = Arc::new(RwLock::new(Shown {
            standing: if server.is_some() {
                Standing::Starting
            } else {
                Standing::Nearby
            },
            ..Shown::default()
        }));
        let running = Arc::new(Running {
            catalog,
            ledger: Arc::clone(&ledger),
            shown: Arc::clone(&shown),
            derived,
            device,
            me: settings.me,
            follow: settings.follow,
            server,
            stun: settings
                .stun
                .map(|stun| stun.trim().to_owned())
                .filter(|stun| !stun.is_empty()),
            staging: settings.staging,
            endpoint,
            reflector,
            to_server,
            to_web,
            to_peers,
            transfers_allowed,
            nearby: settings.nearby.is_some(),
            relay_only: settings.relay_only,
            bonjour: settings
                .nearby
                .as_ref()
                .is_some_and(|nearby| nearby.bonjour),
            beacons,
            beacons_v6,
            told: broadcast
                .map(|broadcast| broadcast.told.clone())
                .unwrap_or_default(),
            record: Mutex::new(None),
            events,
            answering,
            fetching: Mutex::new(AHashSet::new()),
            failed_lately: Mutex::new(AHashMap::new()),
            lyricsfiles_skipped: Mutex::new(BTreeMap::new()),
            prints_skipped: Mutex::new(BTreeMap::new()),
        });

        let (commands, received) = mpsc::unbounded_channel();
        let ran_by = Arc::clone(&running);
        let thread = thread::Builder::new()
            .name("resonate-sync".into())
            .spawn(move || {
                ran(
                    &runtime,
                    ran_by,
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
            running,
            thread: Some(thread),
        })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn link(&self) -> String {
        format!("{PAIRING_LINK}{}", self.code)
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

    pub fn nearby_record(&self) -> Option<String> {
        self.running.record()
    }

    pub fn nearby_heard(&self, record: &str) {
        if !self.running.nearby {
            return;
        }
        if let Some(beacon) = beacon_recorded(&self.running.derived, record) {
            let _ = self.commands.send(Command::Heard(beacon));
        }
    }

    pub fn fetch(&self, recordings: Vec<Recording>, from: DeviceId) {
        let _ = self.commands.send(Command::Fetch { recordings, from });
    }

    pub fn refused(&self, recording: &Recording) {
        self.running.failed(recording);
    }

    pub fn allow_transfers(&self, allowed: bool) {
        let _ = self.commands.send(Command::Transfers(allowed));
    }

    pub fn force(&self) {
        let _ = self.commands.send(Command::Force);
    }

    pub fn forget(&self, device: DeviceId) -> Result<bool> {
        let forgotten = self.ledger.lock().forget_device(device)?;
        if forgotten {
            let _ = self.commands.send(Command::Forgot);
        }
        Ok(forgotten)
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
    fn failed(&self, recording: &Recording) {
        self.failed_lately
            .lock()
            .insert(recording.key(), Instant::now());
    }

    fn asked_again(&self, recordings: &[Recording]) {
        let mut failed = self.failed_lately.lock();
        for recording in recordings {
            failed.remove(&recording.key());
        }
    }

    fn not_failed_lately(&self, recordings: Vec<Recording>) -> Vec<Recording> {
        let mut failed = self.failed_lately.lock();
        failed.retain(|_, at| at.elapsed() < A_FAILED_SONG_WAITS);
        recordings
            .into_iter()
            .filter(|recording| !failed.contains_key(&recording.key()))
            .collect()
    }

    fn stand(&self, standing: Standing) {
        self.shown.write().standing = standing;
    }

    fn transfers_allowed(&self) -> bool {
        self.transfers_allowed.load(Ordering::Relaxed)
    }

    fn endpoint_takes_ipv6(&self) -> bool {
        self.endpoint
            .local_addr()
            .is_ok_and(|local| local.is_ipv6())
    }

    fn beacon(&self, answering: bool) -> Option<Beacon> {
        let port = self.endpoint.local_addr().ok()?.port();
        Some(Beacon {
            device: self.device,
            port,
            answering,
            addresses: beacon_addresses_here(),
        })
    }

    fn beacon_socket_for(&self, address: &SocketAddr) -> Option<&tokio::net::UdpSocket> {
        if address.is_ipv6() {
            self.beacons_v6.as_ref()
        } else {
            self.beacons.as_ref()
        }
    }

    fn record(&self) -> Option<String> {
        if !self.nearby || !self.transfers_allowed() {
            return None;
        }
        let beacon = self.beacon(false)?;
        let mut held = self.record.lock();
        if let Some((was, record)) = held.as_ref()
            && *was == beacon
        {
            return Some(record.clone());
        }
        let record = record_of(&self.derived, &beacon)
            .inspect_err(
                |error| tracing::debug!(%error, "this device's record could not be sealed"),
            )
            .ok()?;
        *held = Some((beacon, record.clone()));
        Some(record)
    }

    fn listed_waiting_as(&self, recording: &Recording, from: DeviceId, state: TransferState) {
        let mut shown = self.shown.write();
        if let Some(listed) = shown.transfers.iter_mut().find(|transfer| {
            transfer.from == from
                && transfer.recording == *recording
                && transfer.state == TransferState::Waiting
        }) {
            listed.state = state;
        }
    }

    fn unlisted_waiting(&self, recording: &Recording, from: DeviceId) {
        self.shown.write().transfers.retain(|transfer| {
            !(transfer.from == from
                && transfer.recording == *recording
                && transfer.state == TransferState::Waiting)
        });
    }

    fn queued(&self, recording: &Recording, from: DeviceId) {
        self.transfer(recording, from, |transfer| {
            if matches!(
                transfer.state,
                TransferState::Landed | TransferState::Failed
            ) {
                transfer.state = TransferState::Waiting;
                transfer.received = 0;
                transfer.size = 0;
            }
        });
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
        while shown.transfers.len() > TRANSFERS_SHOWN_AT_MOST {
            let Some(oldest) = shown.transfers.iter().position(|transfer| {
                matches!(
                    transfer.state,
                    TransferState::Landed | TransferState::Failed
                )
            }) else {
                break;
            };
            shown.transfers.remove(oldest);
        }
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
    let mut bonjour = running.bonjour.then(Bonjour::started).flatten();
    let mut watch = interval(WATCH_EVERY);
    watch.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut beacon_at = Instant::now();
    let mut beacon = vec![0; LARGEST_BEACON];
    let mut beacon_v6 = vec![0; LARGEST_BEACON];
    loop {
        let due = syncing.next_due();
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
                Some(Command::Transfers(allowed)) => {
                    let started = syncing.transfers(allowed).await;
                    if let Some(bonjour) = bonjour.as_mut() {
                        syncing.advertise(bonjour);
                    }
                    started
                }
                Some(Command::Heard(beacon)) => {
                    syncing.heard_nearby(beacon, &[], None).await;
                    Ok(())
                }
                Some(Command::Force) => syncing.forced().await,
                Some(Command::Forgot) => {
                    syncing.forgot();
                    Ok(())
                }
                Some(Command::Fetch { recordings, from }) => syncing.want(from, Work::Fetch(recordings)).await,
            },
            Some(event) = inbound.events.recv() => syncing.event(event).await,
            Some(heard) = inbound.heard.recv() => syncing.heard_from_peer(heard).await,
            Some((length, from)) = beacon_heard(running.beacons.as_ref(), &mut beacon) => {
                syncing.beacon_heard(&beacon[..length], from).await;
                Ok(())
            }
            Some((length, from)) = beacon_heard(running.beacons_v6.as_ref(), &mut beacon_v6) => {
                syncing.beacon_heard(&beacon_v6[..length], from).await;
                Ok(())
            }
            found = found_nearby(bonjour.as_ref()) => {
                match found {
                    Found::Record { record, at } => {
                        if let Some(beacon) = beacon_recorded(&running.derived, &record) {
                            syncing.heard_nearby(beacon, &at, None).await;
                        }
                    }
                    Found::Nothing => {}
                    Found::Ended => bonjour = None,
                }
                Ok(())
            }
            () = sleep_until(beacon_at) => {
                syncing.beacon().await;
                if let Some(bonjour) = bonjour.as_mut() {
                    syncing.advertise(bonjour);
                }
                beacon_at = Instant::now() + jittered(BEACON_EVERY);
                Ok(())
            }
            _ = watch.tick() => syncing.watched().await,
            () = until(due) => Ok(()),
        };
        if let Err(error) = outcome {
            tracing::warn!(%error, "sync could not carry on with something");
        }
        if syncing.look_is_due()
            && let Err(error) = syncing.watched().await
        {
            tracing::warn!(%error, "sync could not look at the catalog");
        }
        if syncing.follow_again_is_due()
            && let Err(error) = syncing.follow_again().await
        {
            tracing::warn!(%error, "sync could not follow its devices again");
        }
    }
    syncing.bye().await;
    if let Some(bonjour) = bonjour {
        bonjour.end();
    }
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

async fn found_nearby(bonjour: Option<&Bonjour>) -> Found {
    match bonjour {
        Some(bonjour) => bonjour.found().await,
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

pub(crate) fn peer_socket() -> Result<UdpSocket> {
    let dual_stack =
        Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP)).and_then(|socket| {
            socket.set_only_v6(false)?;
            socket.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0).into())?;
            Ok(socket)
        });
    match dual_stack {
        Ok(socket) => Ok(socket.into()),
        Err(error) => {
            tracing::debug!(%error, "no dual stack socket for peers; IPv4 alone");
            UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
                .map_err(Error::Bind)
        }
    }
}

fn routed_candidates(server_address: SocketAddr, port: u16) -> Vec<SocketAddr> {
    let mut candidates = Vec::new();
    if server_address.ip().is_loopback() {
        candidates.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    let unspecified = match server_address.ip().to_canonical() {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    let routed = UdpSocket::bind(SocketAddr::new(unspecified, 0)).and_then(|probe| {
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

async fn resolve_stun(stun: &str) -> Result<SocketAddr> {
    let (host, port) = match stun.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host, port),
            Err(_) => (stun, STUN_PORT),
        },
        _ => (stun, STUN_PORT),
    };
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|source| Error::Resolve {
            server: stun.into(),
            source,
        })?
        .collect::<Vec<_>>();
    addresses
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .ok_or_else(|| Error::Unresolved {
            server: stun.into(),
        })
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

async fn within<T>(waiting: impl Future<Output = Result<T>>) -> Result<T> {
    timeout(SERVER_ANSWERS_WITHIN, waiting)
        .await
        .unwrap_or(Err(Error::ServerSilent))
}

async fn reached(
    running: &Running,
    server: &str,
) -> Result<(Carrier, Outgoing, Incoming, Option<SocketAddr>)> {
    if let Some(web) = web_address(server) {
        let (send, recv) = within(connect(&web, &running.to_web)).await?;
        return Ok((Carrier::web(), send, recv, None));
    }
    let (address, host) = within(resolve(server)).await?;
    let connecting = running
        .endpoint
        .connect_with(running.to_server.clone(), address, &host)
        .map_err(|source| Error::Connect(Box::new(source)))?;
    let connection = within(async {
        connecting
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    let (send, recv) = within(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    Ok((
        Carrier::Quic(connection),
        Outgoing::Quic(send),
        Incoming::Quic(recv),
        Some(address),
    ))
}

async fn linked(running: Arc<Running>, server: String) -> Result<Link> {
    let (carrier, mut send, recv, address) = reached(&running, &server).await?;

    let (frames_in, mut frames) = mpsc::unbounded_channel();
    let reading = tokio::spawn(read_frames(recv, frames_in, Arc::clone(&running.reflector)));
    let welcomed = within(welcomed(&running, &mut send, &mut frames)).await;
    let (online, head, cursor) = match welcomed {
        Ok(welcomed) => welcomed,
        Err(error) => {
            reading.abort();
            carrier.close();
            return Err(error);
        }
    };
    if head < cursor {
        reading.abort();
        carrier.close();
        let ledger = Arc::clone(&running.ledger);
        blocking(move || ledger.lock().space_made_anew()).await?;
        return Err(Error::SpaceMadeAnew);
    }
    tracing::info!(%server, "syncing with the server");
    let (outgoing, queued) = mpsc::unbounded_channel();
    let (relay_out, relayed) = mpsc::channel(RELAYED_DATAGRAMS_QUEUED_AT_MOST);
    let writing = tokio::spawn(write_frames(
        send,
        queued,
        relayed,
        running.events.clone(),
        carrier.id(),
    ));
    Ok(Link {
        carrier,
        reflector: Arc::clone(&running.reflector),
        relay_out,
        outgoing,
        writing,
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
    send: &mut Outgoing,
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
    send.send(&hello.encoded()).await?;
    match frames.recv().await {
        Some(Ok(FromServer::Welcome { online, head, .. })) => Ok((online, head, cursor)),
        Some(Ok(FromServer::Refused(refusal))) => Err(Error::Refused(refusal)),
        Some(Err(error)) => Err(error),
        Some(Ok(_)) | None => Err(Error::Unwelcome),
    }
}

async fn write_frames(
    mut send: Outgoing,
    mut queued: UnboundedReceiver<Vec<u8>>,
    mut relayed: mpsc::Receiver<Vec<u8>>,
    events: UnboundedSender<Event>,
    connection: usize,
) {
    let mut keeping = interval(KEEP_THE_SERVER_EVERY);
    keeping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut relaying = true;
    loop {
        let written = tokio::select! {
            biased;
            payload = queued.recv() => match payload {
                Some(payload) => send.send(&payload).await,
                None => break,
            },
            datagram = relayed.recv(), if relaying => match datagram {
                Some(datagram) => send.send(&datagram).await,
                None => {
                    relaying = false;
                    Ok(())
                }
            },
            _ = keeping.tick() => send.keep_alive().await,
        };
        if let Err(error) = written {
            let _ = events.send(Event::Dropped(connection, error));
            return;
        }
    }
    send.finish().await;
}

async fn read_frames(
    mut recv: Incoming,
    frames: UnboundedSender<Result<FromServer>>,
    reflector: Arc<Reflector>,
) {
    loop {
        let frame = match recv.read().await {
            Ok(Some(bytes)) => FromServer::decoded(&bytes),
            Ok(None) => return,
            Err(error) => Err(error),
        };
        if let Ok(FromServer::Relayed { from, datagram }) = frame {
            reflector.relay().arrived(from, datagram);
            continue;
        }
        let failed = frame.is_err();
        if frames.send(frame).is_err() || failed {
            return;
        }
    }
}

impl Syncing {
    fn new(running: Arc<Running>) -> Self {
        let now = Instant::now();
        let retry_at = running.server.as_ref().map(|_| now);
        Self {
            running,
            server: None,
            retry_at,
            wait: FIRST_RETRY,
            server_online: BTreeSet::new(),
            caught_up_once: false,
            neighbour_caught_up: false,
            neighbours: BTreeMap::new(),
            heard: BTreeMap::new(),
            dialled_at: BTreeMap::new(),
            dialling_back: BTreeSet::new(),
            peers: BTreeMap::new(),
            punched_back: BTreeMap::new(),
            waiting: BTreeMap::new(),
            followed: BTreeMap::new(),
            covers_asked: BTreeMap::new(),
            lyricsfiles_asked: BTreeMap::new(),
            prints_asked: BTreeMap::new(),
            follow_again_at: None,
            followed_again_at: None,
            look_at: None,
            looked_at: now,
            looked_once: false,
            seen_revision: 0,
        }
    }

    fn connect(&mut self) {
        self.retry_at = None;
        let Some(server) = self.running.server.clone() else {
            return;
        };
        self.running.stand(Standing::Connecting);
        self.publish_settled();
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            let link = linked(Arc::clone(&running), server).await;
            let _ = running.events.send(Event::Linked(link));
        });
    }

    async fn link(&mut self, link: Link) -> Result<()> {
        self.server_online = link.online.iter().copied().collect();
        if link.caught_up {
            self.caught_up_once = true;
        }
        self.running
            .reflector
            .relay()
            .attach(link.relay_out.clone());
        let _ = link.say(&ToServer::Relaying);
        self.server = Some(link);
        self.wait = FIRST_RETRY;
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
            link.hang_up();
        }
        self.server_online.clear();
        match error {
            None => {
                self.running.stand(Standing::Unreachable);
                self.wait = FIRST_RETRY;
            }
            Some(Error::SpaceMadeAnew) => {
                tracing::info!("the sync server holds this space afresh; everything is sent again");
                self.running.stand(Standing::Connecting);
                self.wait = FIRST_RETRY;
            }
            Some(Error::Refused(refusal)) => {
                tracing::warn!(?refusal, "the sync server refused this device");
                self.running.stand(Standing::Refused(refusal));
                self.wait = LAST_RETRY;
            }
            Some(error) => {
                tracing::debug!(%error, server = ?self.running.server, "the sync server could not be reached");
                self.running.stand(Standing::Unreachable);
            }
        }
        self.retry_at = Some(Instant::now() + jittered(self.wait));
        self.wait = (self.wait * 2).min(LAST_RETRY);
        self.publish_devices();
        self.publish_settled();
    }

    async fn bye(&mut self) {
        if let Some(link) = self.server.take() {
            let _ = link.say(&ToServer::Bye);
            let Link {
                outgoing,
                writing,
                reading,
                carrier,
                reflector,
                ..
            } = link;
            drop(outgoing);
            reflector.relay().detach();
            let _ = timeout(LEAVE_WITHIN, writing).await;
            reading.abort();
            carrier.close();
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
            Event::Punched(device, Some(connection)) | Event::Relayed(device, Some(connection)) => {
                self.watch_closing(device, &connection);
                for work in self.waiting.remove(&device).unwrap_or_default() {
                    spawn_work(Arc::clone(&self.running), device, connection.clone(), work);
                }
                self.ask_for_covers(device, &connection);
                self.ask_for_lyricsfiles(device, &connection);
                self.ask_for_prints(device, &connection);
                self.peers.insert(device, connection);
            }
            Event::Punched(device, None) => self.relayed_or_unreached(device),
            Event::Relayed(device, None) => self.unreached(device),
            Event::CutShort(device) => {
                self.followed.remove(&device);
            }
            Event::Unmet(device) => {
                self.unreached(device);
                self.redial_later(device);
            }
            Event::Redial(device) => {
                self.dialling_back.remove(&device);
                if !self.neighbours.contains_key(&device) {
                    self.dial_neighbour(device);
                }
            }
            Event::Met(device, connection) => self.met(device, connection, true).await?,
            Event::Dropped(id, error) => {
                if self
                    .server
                    .as_ref()
                    .is_some_and(|link| link.carrier.id() == id)
                {
                    self.unlinked(Some(error));
                }
            }
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
                    self.dialled_at.remove(&device);
                    self.publish_devices();
                    self.meet_soon(device);
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
                self.follow_again_soon();
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
                if let Some(pushed) = sent {
                    let changes = pushed
                        .into_iter()
                        .flat_map(|pushed| pushed.changes)
                        .collect::<Vec<_>>();
                    self.sent(changes).await?;
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
                let by_another = self
                    .server
                    .as_mut()
                    .is_some_and(|link| !mem::replace(&mut link.compacting, false));
                let ledger = Arc::clone(&self.running.ledger);
                blocking(move || {
                    let mut ledger = ledger.lock();
                    ledger.compacted(through)?;
                    if by_another {
                        ledger.owe_queries()?;
                    }
                    Ok(())
                })
                .await?;
                if by_another {
                    self.push().await?;
                }
            }
            FromServer::Refused(Refusal::Stale) => {
                if let Some(link) = self.server.as_mut() {
                    link.compacting = false;
                }
            }
            FromServer::Refused(Refusal::TooLarge) => {
                let refused = self
                    .server
                    .as_mut()
                    .and_then(|link| link.in_flight.pop_front());
                let largest = refused
                    .into_iter()
                    .flatten()
                    .max_by_key(|pushed| pushed.bytes);
                if let Some(largest) = largest {
                    tracing::warn!(
                        bytes = largest.bytes,
                        "the sync server refused a record as too large; it is kept for devices on this network"
                    );
                    self.sent(largest.changes).await?;
                }
                self.push().await?;
            }
            FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
            FromServer::Welcome { .. } | FromServer::Relayed { .. } => {}
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
        self.follow_again_soon();
        self.publish_devices();
        Ok(())
    }

    fn follow_again_soon(&mut self) {
        if self.follow_again_at.is_none() {
            let soon = Instant::now() + FOLLOW_AGAIN_AFTER;
            let soonest = self
                .followed_again_at
                .filter(|_| self.running.follow.everything)
                .map(|at| at + EVERYTHING_FOLLOWED_AGAIN_AT_MOST_EVERY);
            self.follow_again_at = Some(soonest.map_or(soon, |soonest| soon.max(soonest)));
        }
    }

    fn next_due(&self) -> Option<Instant> {
        let look = self.look_at.filter(|_| self.may_look());
        match (look, self.follow_again_at) {
            (Some(look), Some(follow)) => Some(look.min(follow)),
            (look, follow) => look.or(follow),
        }
    }

    fn follow_again_is_due(&self) -> bool {
        self.follow_again_at.is_some_and(|at| at <= Instant::now())
    }

    async fn follow_again(&mut self) -> Result<()> {
        self.follow_again_at = None;
        self.followed_again_at = Some(Instant::now());
        let connected = self
            .neighbours
            .keys()
            .chain(self.peers.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        for device in connected {
            self.followed.remove(&device);
            self.follow(device).await?;
        }
        Ok(())
    }

    fn look_is_due(&self) -> bool {
        self.may_look() && self.look_at.is_some_and(|at| at <= Instant::now())
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
        if !self.running.follow.everything
            && self
                .followed_again_at
                .is_none_or(|at| at.elapsed() >= FOLLOWED_AGAIN_AFTER_A_LOOK_EVERY)
        {
            self.follow_again_soon();
        }
        let owed = self.tell_neighbours().await?;
        self.push().await?;
        if self.running.server.is_none() {
            self.sent(owed).await?;
        }
        self.publish_settled();
        Ok(())
    }

    async fn sent(&mut self, changes: Vec<Change>) -> Result<()> {
        let ledger = Arc::clone(&self.running.ledger);
        let owed = blocking(move || {
            let mut ledger = ledger.lock();
            ledger.sent(&changes)?;
            ledger.owed_count()
        })
        .await?;
        self.running.shown.write().owed = owed;
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
            let mut ledger = running.ledger.lock();
            let owed = ledger.owed()?;
            let mut sealed_bundles = Vec::new();
            for changes in owed {
                if sealed_bundles.len() == BLOBS_PER_APPEND {
                    break;
                }
                let blob = sealed(
                    &running.derived.sealer,
                    running.derived.space,
                    &encoded_bundle(&changes),
                )?;
                if blob.len() > LARGEST_RECORD {
                    tracing::warn!(
                        bytes = blob.len(),
                        "a change too large for the sync server is kept for devices on this network"
                    );
                    ledger.sent(&changes)?;
                    continue;
                }
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
        let (pushed, blobs): (Vec<_>, Vec<_>) = bundles
            .into_iter()
            .map(|(changes, blob)| {
                (
                    Pushed {
                        changes,
                        bytes: blob.len(),
                    },
                    blob,
                )
            })
            .unzip();
        link.in_flight.push_back(pushed);
        link.say(&ToServer::Append { blobs })
    }

    async fn tell_neighbours(&mut self) -> Result<Vec<Change>> {
        if self.neighbours.is_empty() && self.running.server.is_some() {
            return Ok(Vec::new());
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
        Ok(owed)
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
            link.say(&ToServer::Compact { through, parts })?;
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

    fn ask_for_covers(&mut self, device: DeviceId, connection: &Connection) {
        if device == self.running.device || !self.running.transfers_allowed() {
            return;
        }
        let now = Instant::now();
        if self
            .covers_asked
            .get(&device)
            .is_some_and(|at| now.duration_since(*at) < COVERS_ASKED_OF_A_DEVICE_EVERY)
        {
            return;
        }
        self.covers_asked.insert(device, now);
        spawn_covers(Arc::clone(&self.running), connection.clone());
    }

    fn ask_for_lyricsfiles(&mut self, device: DeviceId, connection: &Connection) {
        if device == self.running.device || !self.running.transfers_allowed() {
            return;
        }
        let now = Instant::now();
        if self
            .lyricsfiles_asked
            .get(&device)
            .is_some_and(|at| now.duration_since(*at) < LYRICSFILES_ASKED_OF_A_DEVICE_EVERY)
        {
            return;
        }
        self.lyricsfiles_asked.insert(device, now);
        spawn_lyricsfiles(Arc::clone(&self.running), device, connection.clone());
    }

    fn ask_for_prints(&mut self, device: DeviceId, connection: &Connection) {
        if device == self.running.device || !self.running.transfers_allowed() {
            return;
        }
        let now = Instant::now();
        if self
            .prints_asked
            .get(&device)
            .is_some_and(|at| now.duration_since(*at) < PRINTS_ASKED_OF_A_DEVICE_EVERY)
        {
            return;
        }
        self.prints_asked.insert(device, now);
        spawn_prints(Arc::clone(&self.running), device, connection.clone());
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

    fn relayed_or_unreached(&mut self, device: DeviceId) {
        let wanted = self.waiting.contains_key(&device) && !self.peers.contains_key(&device);
        if !wanted || self.server.is_none() || !self.running.reflector.relay().is_attached() {
            self.unreached(device);
            return;
        }
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            let address = running.reflector.relay().address_of(device);
            let connection = match dialled_within(
                &running.endpoint,
                &running.to_peers,
                &[address],
                device,
                RELAY_DIAL_WITHIN,
            )
            .await
            {
                Ok(connection) => Some(connection),
                Err(error) => {
                    tracing::debug!(%error, "a device could not be reached through the server either");
                    None
                }
            };
            let _ = running.events.send(Event::Relayed(device, connection));
        });
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
        let candidates = self.candidates().await;
        let Some(link) = self.server.as_ref() else {
            return Ok(());
        };
        let sealed = sealed(
            &self.running.derived.sealer,
            self.running.derived.space,
            &encoded_candidates(&candidates),
        )?;
        link.say(&ToServer::Punch { to: device, sealed })
    }

    async fn candidates(&self) -> Vec<SocketAddr> {
        let Ok(port) = self.running.endpoint.local_addr().map(|local| local.port()) else {
            return Vec::new();
        };
        let mut candidates = match self.server.as_ref().map(|link| link.address) {
            None => return Vec::new(),
            Some(Some(server_address)) => routed_candidates(server_address, port),
            Some(None) => addresses_here()
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect(),
        };
        if self.running.endpoint_takes_ipv6() {
            candidates.extend(
                global_addresses_here()
                    .into_iter()
                    .map(|ip| SocketAddr::new(ip, port)),
            );
        }
        if let Some(public) = self.public_address().await {
            candidates.push(public);
        }
        candidates
    }

    async fn public_address(&self) -> Option<SocketAddr> {
        let stun = self.running.stun.as_deref()?;
        let asked = async {
            let server = resolve_stun(stun).await?;
            self.running.reflector.ask(server).await
        };
        match asked.await {
            Ok(public) => Some(public),
            Err(error) => {
                tracing::debug!(%error, stun, "the public address could not be learned");
                None
            }
        }
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
        let connected = self
            .peers
            .get(&device)
            .is_some_and(|connection| connection.close_reason().is_none());
        let answered_lately = self
            .punched_back
            .get(&device)
            .is_some_and(|at| at.elapsed() < PUNCH_ANSWERED_AT_MOST_EVERY);
        if connected {
            return Ok(());
        }
        if !self.waiting.contains_key(&device) {
            if answered_lately {
                return Ok(());
            }
            self.punched_back.insert(device, Instant::now());
            self.punch(device).await?;
        }
        let mut candidates = Vec::new();
        if !address.ip().is_unspecified() {
            candidates.push(address);
        }
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
        if self.running.relay_only {
            candidates.clear();
        }
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
        if !self.running.transfers_allowed() || self.running.told.is_empty() {
            return;
        }
        let Some(bytes) = self.beacon_bytes(false) else {
            return;
        };
        for told in &self.running.told {
            let Some(socket) = self.running.beacon_socket_for(told) else {
                continue;
            };
            let sent_to = if told.is_ipv6() {
                told_over_ipv6(*told)
            } else {
                vec![*told]
            };
            for told in sent_to {
                if let Err(error) = socket.send_to(&bytes, told).await {
                    tracing::debug!(%error, %told, "a beacon could not be sent");
                }
            }
        }
    }

    fn beacon_bytes(&self, answering: bool) -> Option<Vec<u8>> {
        let beacon = self.running.beacon(answering)?;
        beacon_bytes(&self.running.derived, &beacon)
            .inspect_err(|error| tracing::debug!(%error, "a beacon could not be sealed"))
            .ok()
    }

    fn advertise(&self, bonjour: &mut Bonjour) {
        let port = self
            .running
            .endpoint
            .local_addr()
            .ok()
            .map(|local| local.port());
        match (self.running.record(), port) {
            (Some(record), Some(port)) => bonjour.advertise(&record, port),
            (None, _) | (_, None) => bonjour.withdraw(),
        }
    }

    async fn beacon_heard(&mut self, bytes: &[u8], from: SocketAddr) {
        if let Some(beacon) = beacon_of(&self.running.derived, bytes) {
            self.heard_nearby(beacon, &[from], Some(from)).await;
        }
    }

    async fn heard_nearby(
        &mut self,
        beacon: Beacon,
        seen_at: &[SocketAddr],
        answer_to: Option<SocketAddr>,
    ) {
        if !self.running.transfers_allowed() || beacon.device == self.running.device {
            return;
        }
        let named = beacon
            .addresses
            .iter()
            .map(|ip| SocketAddr::new(*ip, beacon.port));
        let seen = seen_at.iter().map(|seen| {
            let mut candidate = *seen;
            candidate.set_port(beacon.port);
            candidate
        });
        let mut candidates = Vec::new();
        for candidate in seen.chain(named) {
            let dialled_here = candidate.is_ipv4() || self.running.endpoint_takes_ipv6();
            if dialled_here && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
        self.heard
            .insert(beacon.device, (candidates, Instant::now()));
        if self.neighbours.contains_key(&beacon.device) {
            return;
        }
        if !beacon.answering
            && let Some(from) = answer_to
            && let Some(socket) = self.running.beacon_socket_for(&from)
            && let Some(answer) = self.beacon_bytes(true)
            && let Err(error) = socket.send_to(&answer, from).await
        {
            tracing::debug!(%error, %from, "a beacon could not be answered");
        }
        self.meet_soon(beacon.device);
    }

    fn meet_soon(&mut self, device: DeviceId) {
        if self.running.device < device {
            self.dial_neighbour(device);
        } else if self.dialling_back.insert(device) {
            self.send_later(DIAL_BACK_AFTER, Event::Redial(device));
        }
    }

    fn redial_later(&mut self, device: DeviceId) {
        let Some(redial) = self.dialled_at.get(&device).copied() else {
            return;
        };
        if self.dialling_back.insert(device) {
            self.send_later(redial.wait, Event::Redial(device));
        }
    }

    fn send_later(&self, after: Duration, event: Event) {
        let events = self.running.events.clone();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            let _ = events.send(event);
        });
    }

    fn dial_neighbour(&mut self, device: DeviceId) {
        let now = Instant::now();
        let Some(at) = self
            .heard
            .get(&device)
            .filter(|(at, heard)| !at.is_empty() && now.duration_since(*heard) < HEARD_FOR)
            .map(|(at, _)| at.clone())
        else {
            return;
        };
        let wait = match self.dialled_at.get(&device) {
            Some(redial) if now.duration_since(redial.at) < redial.wait => return,
            Some(redial) => (redial.wait * 2).min(LAST_REDIAL),
            None => FIRST_REDIAL,
        };
        self.dialled_at.insert(device, Redial { at: now, wait });
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            let event = match neighbour_dialled(&running, &at, device).await {
                Ok((met, connection)) => Event::Met(met, connection),
                Err(error) => {
                    tracing::debug!(%error, ?at, "a device on this network could not be reached");
                    Event::Unmet(device)
                }
            };
            let _ = running.events.send(event);
        });
    }

    fn watch_closing(&self, device: DeviceId, connection: &Connection) {
        let events = self.running.events.clone();
        let closing = connection.clone();
        tokio::spawn(async move {
            closing.closed().await;
            let _ = events.send(Event::Gone(device, closing.stable_id()));
        });
    }

    async fn whole(&self) -> Result<(Vec<Change>, AHashSet<ToldDigest>)> {
        let ledger = Arc::clone(&self.running.ledger);
        let snapshot = blocking(move || ledger.lock().snapshot()).await?;
        let changes = snapshot.into_iter().flatten().collect::<Vec<_>>();
        let told = changes.iter().map(told_digest).collect();
        Ok((changes, told))
    }

    fn forgot(&mut self) {
        self.publish_devices();
        self.look_at = Some(Instant::now());
    }

    async fn forced(&mut self) -> Result<()> {
        self.running.failed_lately.lock().clear();
        self.wait = FIRST_RETRY;
        self.followed.clear();
        self.covers_asked.clear();
        self.lyricsfiles_asked.clear();
        self.prints_asked.clear();
        self.dialled_at.clear();
        match self.server.take() {
            Some(link) => {
                link.hang_up();
                self.server_online.clear();
                self.connect();
            }
            None if self.retry_at.is_some() => self.retry_at = Some(Instant::now()),
            None => {}
        }
        self.look_at = Some(Instant::now());
        self.beacon().await;

        let unmet = self
            .heard
            .keys()
            .copied()
            .filter(|device| !self.neighbours.contains_key(device))
            .collect::<Vec<_>>();
        for device in unmet {
            self.dial_neighbour(device);
        }

        if !self.neighbours.is_empty() {
            let (changes, told) = self.whole().await?;
            for (device, neighbour) in &mut self.neighbours {
                neighbour.told.clone_from(&told);
                spawn_tell(*device, neighbour.connection.clone(), true, changes.clone());
            }
        }
        for device in self.neighbours.keys().copied().collect::<Vec<_>>() {
            self.follow(device).await?;
        }
        self.publish_devices();
        self.publish_settled();
        Ok(())
    }

    async fn met(&mut self, device: DeviceId, connection: Connection, records: bool) -> Result<()> {
        if device == self.running.device {
            return Ok(());
        }
        self.watch_closing(device, &connection);
        for work in self.waiting.remove(&device).unwrap_or_default() {
            spawn_work(Arc::clone(&self.running), device, connection.clone(), work);
        }
        self.ask_for_covers(device, &connection);
        self.ask_for_lyricsfiles(device, &connection);
        self.ask_for_prints(device, &connection);
        self.peers.insert(device, connection.clone());
        if records {
            let (changes, told) = self.whole().await?;
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
    at: &[SocketAddr],
    device: DeviceId,
) -> Result<(DeviceId, Connection)> {
    let connection = dialled(&running.endpoint, &running.to_peers, at, device).await?;
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

fn spawn_covers(running: Arc<Running>, connection: Connection) {
    tokio::spawn(async move {
        let catalog = Arc::clone(&running.catalog);
        let wanted = blocking(move || {
            catalog
                .covers_wanted(COVERS_ASKED_AT_MOST)
                .map_err(Error::Catalog)
        })
        .await;
        let wanted = match wanted {
            Ok(wanted) if wanted.is_empty() => return,
            Ok(wanted) => wanted,
            Err(error) => {
                tracing::debug!(%error, "the covers this catalog lacks could not be read");
                return;
            }
        };
        match covers_from(&connection, wanted, Arc::clone(&running.catalog)).await {
            Ok(0) => {}
            Ok(landed) => tracing::info!(landed, "covers arrived from another device"),
            Err(error) => tracing::debug!(%error, "a device was not asked for its covers"),
        }
    });
}

fn spawn_lyricsfiles(running: Arc<Running>, device: DeviceId, connection: Connection) {
    tokio::spawn(async move {
        let skip = running
            .lyricsfiles_skipped
            .lock()
            .get(&device)
            .copied()
            .unwrap_or(0);
        let catalog = Arc::clone(&running.catalog);
        let wanted = blocking(move || {
            catalog
                .lyricsfiles_wanted(skip, LYRICSFILES_ASKED_AT_MOST)
                .map_err(Error::Catalog)
        })
        .await;
        let wanted = match wanted {
            Ok(wanted) => wanted,
            Err(error) => {
                tracing::debug!(%error, "the lyricsfiles this catalog lacks could not be read");
                return;
            }
        };
        let next = if wanted.len() < LYRICSFILES_ASKED_AT_MOST {
            0
        } else {
            skip + wanted.len()
        };
        running.lyricsfiles_skipped.lock().insert(device, next);
        if wanted.is_empty() {
            return;
        }
        match lyricsfiles_from(&connection, wanted, Arc::clone(&running.catalog)).await {
            Ok(0) => {}
            Ok(landed) => tracing::info!(landed, "lyricsfiles arrived from another device"),
            Err(error) => tracing::debug!(%error, "a device was not asked for its lyricsfiles"),
        }
    });
}

fn spawn_prints(running: Arc<Running>, device: DeviceId, connection: Connection) {
    tokio::spawn(async move {
        let skip = running
            .prints_skipped
            .lock()
            .get(&device)
            .copied()
            .unwrap_or(0);
        let catalog = Arc::clone(&running.catalog);
        let wanted = blocking(move || {
            catalog
                .prints_wanted(skip, PRINTS_ASKED_AT_MOST)
                .map_err(Error::Catalog)
        })
        .await;
        let wanted = match wanted {
            Ok(wanted) => wanted,
            Err(error) => {
                tracing::debug!(%error, "the prints this catalog lacks could not be read");
                return;
            }
        };
        let next = if wanted.len() < PRINTS_ASKED_AT_MOST {
            0
        } else {
            skip + wanted.len()
        };
        running.prints_skipped.lock().insert(device, next);
        if wanted.is_empty() {
            return;
        }
        match prints_from(&connection, wanted, Arc::clone(&running.catalog)).await {
            Ok(0) => {}
            Ok(landed) => tracing::info!(landed, "prints arrived from another device"),
            Err(error) => tracing::debug!(%error, "a device was not asked for its prints"),
        }
    });
}

fn spawn_work(running: Arc<Running>, device: DeviceId, connection: Connection, work: Work) {
    tokio::spawn(async move {
        let recordings = match work {
            Work::Fetch(recordings) => {
                running.asked_again(&recordings);
                recordings
            }
            Work::Follow => match followed_songs(&running, &connection).await {
                Ok(recordings) => {
                    let recordings = running.not_failed_lately(recordings);
                    for recording in &recordings {
                        running.queued(recording, device);
                    }
                    recordings
                }
                Err(error) => {
                    tracing::debug!(%error, "a device's songs could not be listed");
                    return;
                }
            },
        };
        let mut left = recordings.into_iter();
        while let Some(recording) = left.next() {
            if connection.close_reason().is_some() || !running.transfers_allowed() {
                running.listed_waiting_as(&recording, device, TransferState::Failed);
                for unasked in left.by_ref() {
                    running.listed_waiting_as(&unasked, device, TransferState::Failed);
                }
                let _ = running.events.send(Event::CutShort(device));
                return;
            }
            fetch_one(&running, device, &connection, recording).await;
        }
    });
}

async fn followed_songs(running: &Arc<Running>, connection: &Connection) -> Result<Vec<Recording>> {
    let wanted = if running.follow.everything {
        Vec::new()
    } else {
        let asking = Arc::clone(running);
        let wanted = blocking(move || {
            asking
                .ledger
                .lock()
                .wanted(asking.follow, asking.catalog.as_ref())
        })
        .await?;
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        wanted
    };
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
    let key = recording.key();
    if !running.fetching.lock().insert(key.clone()) {
        running.unlisted_waiting(&recording, device);
        return;
    }
    if held_already(running, &recording).await {
        running.listed_waiting_as(&recording, device, TransferState::Landed);
    } else {
        fetch_claimed(running, device, connection, recording).await;
    }
    running.fetching.lock().remove(&key);
}

async fn held_already(running: &Arc<Running>, recording: &Recording) -> bool {
    let catalog = Arc::clone(&running.catalog);
    let asked = recording.clone();
    blocking(move || catalog.holds(&asked).map_err(Error::Catalog))
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, "whether a song is held here could not be read");
            false
        })
}

async fn fetch_claimed(
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
    let mut refused_here = false;
    let landed = match fetched {
        Ok(fetched) => {
            let catalog = Arc::clone(&running.catalog);
            let landing = recording.clone();
            let staged = fetched.staged.clone();
            let landed = blocking(move || {
                catalog
                    .land(&landing, &fetched.tags, &fetched.extension, &fetched.staged)
                    .map_err(Error::Catalog)
            })
            .await;
            let _ = tokio::fs::remove_file(&staged).await;
            refused_here = !matches!(landed, Ok(true));
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
    if refused_here {
        running.failed(&recording);
    }
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

    fn transfer_in(state: TransferState, title: &str) -> Transfer {
        Transfer {
            recording: Recording {
                mbid: None,
                title: title.into(),
                artist: String::new(),
                album: String::new(),
                length: None,
            },
            from: DeviceId([1; 16]),
            received: 0,
            size: 0,
            state,
        }
    }

    #[test]
    fn what_is_downloading_and_what_waits_are_counted_apart_and_a_finished_song_is_neither() {
        let shown = Shown {
            transfers: vec![
                transfer_in(TransferState::Moving, "a"),
                transfer_in(TransferState::Waiting, "b"),
                transfer_in(TransferState::Waiting, "c"),
                transfer_in(TransferState::Landed, "d"),
                transfer_in(TransferState::Failed, "e"),
            ],
            ..Shown::default()
        };

        assert_eq!(shown.downloading(), 1);
        assert_eq!(shown.waiting(), 2);
    }

    #[test]
    fn a_server_names_its_port_or_takes_the_hosted_one() {
        assert_eq!(
            host_and_port(resonate_core::RESONATE_HOST),
            (resonate_core::RESONATE_HOST.into(), SERVER_PORT)
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
