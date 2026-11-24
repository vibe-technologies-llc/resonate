use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::{Future, pending},
    mem,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::{Duration, SystemTime},
};

use ahash::AHashSet;
use parking_lot::{Mutex, RwLock};
use quinn::{
    AsyncUdpSocket, ClientConfig, Connection, Endpoint, EndpointConfig, Runtime as _, TokioRuntime,
};
use resonate_core::{Recording, RecordingKey, RemoteTrust, ZoneId};
use ring::digest::{SHA256, SHA256_OUTPUT_LEN, digest};
use rustls::ClientConfig as WebConfig;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, sleep_until, timeout},
};

use self::{
    discovery::{Discovery, Public, Resolver, resolved_by_the_system},
    unsent::{Awaited, Awaiting, Unsent, awaited},
};
use crate::{
    Catalog, Change, DeviceId, Error, Handoff, LayoutNote, Ledger, LocalPlaylistId, Platform,
    Refused, Result, SyncKey,
    consulted::{
        AskedForExport, AskedToConsult, Consultable, Consulting, ExportFetched, ExportHeld,
        ExportPart, ExportReceiving,
    },
    fresh::{HeardOnce, Unfresh, wall_seconds},
    key::{Derived, PAIRING_LINK, hex, random_bytes},
    ledger::{Follow, Me, lacked},
    merge::one_recording_a_song,
    nearby::{
        BEACON_EVERY, BEACON_SENT_WITHIN, BEACONS_HEARD_AT_MOST, Bonjour, Found, LARGEST_BEACON,
        Nearby, RECORD_RESEALED_EVERY, addresses_here, beacon_addresses_here, beacon_recorded,
        bound, bound_v6, datagram_of, global_addresses_here, heard_in, record_of, record_resolved,
        told_over_ipv6,
    },
    peer::{
        Answering, ExportStaged, Heard, blocking, consult, control, covers_from, dialled,
        dialled_within, export_from, fetch, holdings_of, kept_from, lyricsfiles_from, meet,
        prints_from, serve, tell, watch,
    },
    quic::{KEEP_THE_SERVER_EVERY, PeerIdentity, to_the_server, trusted_roots},
    remote::{Controlled, Controlling, RemoteCommand, Watched},
    routes::{each_once, first_answering},
    seal::{LARGEST_OPENED, opened, sealed},
    stun::Reflector,
    web::{Incoming, Outgoing, connect, to_the_web, web_address},
    wire::{
        BLOBS_A_FRAME_AT_MOST, Beacon, COVERS_ASKED_AT_MOST, Candidates, FromServer, LARGEST_FRAME,
        LARGEST_RECORD, LYRICSFILES_ASKED_AT_MOST, PRINTS_ASKED_AT_MOST, PROTOCOL_VERSION, Refusal,
        ToServer, Unread, encoded_bundle, encoded_change, read_bundle,
    },
};

mod discovery;
mod unsent;

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
const KEPT_ASKED_OF_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
const LYRICSFILES_ASKED_OF_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
const PRINTS_ASKED_OF_A_DEVICE_EVERY: Duration = Duration::from_secs(30 * 60);
pub const A_FAILED_SONG_WAITS: Duration = Duration::from_secs(30 * 60);
pub const A_PARTIAL_IS_SWEPT_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const LEAVE_WITHIN: Duration = Duration::from_secs(2);
const SNAPSHOT_FRAME_SHARE: usize = LARGEST_FRAME / 2;
const FIRST_REDIAL: Duration = Duration::from_secs(1);
const LAST_REDIAL: Duration = Duration::from_secs(30);
const DIAL_BACK_AFTER: Duration = Duration::from_millis(500);
const DIALS_CROSS_WITHIN: Duration = Duration::from_secs(5);
const HEARD_FOR: Duration = Duration::from_secs(60);
const WATCH_LAPSES_AFTER: Duration = Duration::from_secs(30);
const SERVER_ANSWERS_WITHIN: Duration = Duration::from_secs(15);
const PUNCH_ANSWERED_AT_MOST_EVERY: Duration = Duration::from_secs(10);
const LARGEST_CANDIDATES_OPENED: usize = 64 * 1024;
const PUNCH_SEALED_WITHIN: Duration = Duration::from_secs(2 * 60);
const PUNCHES_HEARD_AT_MOST: usize = 4096;
const KNOWN_DEVICES_ANSWERED_AT_MOST: usize = 1024;
const STRANGERS_ANSWERED_AT_MOST: usize = 16;
const RELAY_DIAL_WITHIN: Duration = Duration::from_secs(20);
const RELAYED_DATAGRAMS_QUEUED_AT_MOST: usize = 256;
const FRAMES_FROM_THE_SERVER_QUEUED_AT_MOST: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Server {
    #[default]
    Hosted,
    At(String),
    Nowhere,
}

impl Server {
    pub fn address(self) -> Option<String> {
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

    fn listed_waiting_as(&mut self, recording: &Recording, from: DeviceId, state: TransferState) {
        if let Some(listed) = self.transfers.iter_mut().find(|transfer| {
            transfer.names(recording, from) && transfer.state == TransferState::Waiting
        }) {
            listed.state = state;
        }
    }

    fn unreached(&mut self, recordings: &[Recording], from: DeviceId) {
        for recording in recordings {
            self.listed_waiting_as(recording, from, TransferState::Failed);
        }
    }

    fn unlisted_waiting(&mut self, recording: &Recording, from: DeviceId) {
        self.transfers.retain(|transfer| {
            !(transfer.names(recording, from) && transfer.state == TransferState::Waiting)
        });
    }

    fn queued(&mut self, recording: &Recording, from: DeviceId) {
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

    fn transfer(
        &mut self,
        recording: &Recording,
        from: DeviceId,
        change: impl FnOnce(&mut Transfer),
    ) {
        let at = self
            .transfers
            .iter()
            .position(|transfer| transfer.names(recording, from));
        let at = at.unwrap_or_else(|| {
            self.transfers.push(Transfer {
                recording: recording.clone(),
                from,
                received: 0,
                size: 0,
                state: TransferState::Waiting,
            });
            self.transfers.len() - 1
        });

        change(&mut self.transfers[at]);
        while self.transfers.len() > TRANSFERS_SHOWN_AT_MOST {
            let Some(oldest) = self.transfers.iter().position(|transfer| {
                matches!(
                    transfer.state,
                    TransferState::Landed | TransferState::Failed
                )
            }) else {
                break;
            };

            self.transfers.remove(oldest);
        }
    }
}

impl Transfer {
    fn names(&self, recording: &Recording, from: DeviceId) -> bool {
        self.from == from && self.recording.names_the_same_song_as(recording)
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
    Control {
        device: DeviceId,
        zone: ZoneId,
        command: RemoteCommand,
        queue_seen: Option<u64>,
        answer: std_mpsc::Sender<Controlling>,
        awaited: Awaited,
    },
    Watch(DeviceId),
    Consult {
        device: DeviceId,
        asked: Vec<u8>,
        answer: std_mpsc::Sender<Consulting>,
    },
    Export {
        device: DeviceId,
        asked: ExportAsked,
    },
    KeptOwed,
    Stop,
}

struct ExportAsked {
    part: ExportPart,
    held: Option<ExportHeld>,
    staged: PathBuf,
    noted: PathBuf,
    receiving: Arc<ExportReceiving>,
    answer: std_mpsc::Sender<ExportFetched>,
}

pub struct ExportWanted {
    pub part: ExportPart,
    pub held: Option<ExportHeld>,
    pub staged: PathBuf,
    pub noted: PathBuf,
}

pub struct Sync {
    commands: UnboundedSender<Command>,
    shown: Arc<RwLock<Shown>>,
    ledger: Arc<Mutex<Ledger>>,
    code: String,
    device: DeviceId,
    nearby_port: Option<u16>,
    running: Arc<Running>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
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
    resolver: Resolver,
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
    record: Mutex<Option<Advertised>>,
    events: UnboundedSender<Event>,
    answering: Arc<Answering>,
    fetching: Claims,
    passed_over: Mutex<Vec<PassedOver>>,
    lyricsfiles_skipped: Mutex<BTreeMap<DeviceId, usize>>,
    prints_skipped: Mutex<BTreeMap<DeviceId, usize>>,
    mirrors: RwLock<BTreeMap<DeviceId, Watched>>,
    watched: Mutex<BTreeMap<DeviceId, std::time::Instant>>,
    watching: Mutex<BTreeSet<DeviceId>>,
}

pub struct AskedToControl {
    answered: std_mpsc::Receiver<Controlling>,
    awaiting: Awaiting,
}

impl AskedToControl {
    pub fn answer(self) -> Controlling {
        self.answered
            .recv_timeout(self.awaiting.left())
            .unwrap_or(Controlling::Unanswered)
    }
}

enum Work {
    Fetch {
        recordings: Vec<Recording>,
        asked: bool,
    },
    Follow,
    Control {
        zone: ZoneId,
        command: RemoteCommand,
        queue_seen: Option<u64>,
        answer: std_mpsc::Sender<Controlling>,
        awaited: Awaited,
    },
    Watch,
    Consult {
        asked: Vec<u8>,
        answer: std_mpsc::Sender<Consulting>,
    },
    Export(ExportAsked),
}

#[derive(Default)]
struct Claims(Mutex<AHashSet<RecordingKey>>);

impl Claims {
    fn claim(&self, recording: &Recording) -> bool {
        let keys = [recording.key(), recording.named_key()];
        let mut claimed = self.0.lock();

        if keys.iter().any(|key| claimed.contains(key)) {
            return false;
        }
        claimed.extend(keys);
        true
    }

    fn release(&self, recording: &Recording) {
        let mut claimed = self.0.lock();

        claimed.remove(&recording.key());
        claimed.remove(&recording.named_key());
    }
}

struct PassedOver {
    device: DeviceId,
    recording: Recording,
    asked: bool,
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
    Followed(DeviceId),
    Unclaimed(Vec<PassedOver>),
    Dropped(usize, Error),
    Discovered(Option<SocketAddr>),
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
    frames: mpsc::Receiver<Result<FromServer>>,
    reading: JoinHandle<()>,
    address: Option<SocketAddr>,
    online: Vec<DeviceId>,
    head: u64,
    read_from: u64,
    caught_up: bool,
    in_flight: VecDeque<Vec<Pushed>>,
    compacting: Option<u64>,
    in_parts: InParts,
}

impl Link {
    fn answer_awaited(&self) -> bool {
        !self.in_flight.is_empty() || self.compacting.is_some()
    }

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
    met_at: Instant,
}

#[derive(Default)]
struct Unanswered(Vec<(ToldDigest, Vec<u8>)>);

impl Unanswered {
    fn sealed_as(&self, plain: &[u8]) -> Option<&Vec<u8>> {
        let asked = plain_digest(plain);
        self.0
            .iter()
            .find(|(digest, _)| *digest == asked)
            .map(|(_, blob)| blob)
    }
}

fn plain_digest(plain: &[u8]) -> ToldDigest {
    let mut told = [0; SHA256_OUTPUT_LEN];

    told.copy_from_slice(digest(&SHA256, plain).as_ref());
    told
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Oversized {
    records_since_at: Option<u64>,
}

impl Oversized {
    pub(crate) const fn found_at(records_since: u64) -> Self {
        Self {
            records_since_at: Some(records_since),
        }
    }

    pub(crate) fn waits(self, records_since: u64) -> bool {
        self.records_since_at
            .is_some_and(|at| (at..at.saturating_mul(2)).contains(&records_since))
    }
}

enum Snapshotted {
    Unneeded,
    Oversized {
        records_since: u64,
    },
    Sealed {
        through: u64,
        records_since: u64,
        pieces: Pieces,
    },
}

#[derive(Debug, Default)]
pub(crate) struct Pieces {
    frames: Vec<Vec<Vec<u8>>>,
    last_bytes: usize,
}

impl Pieces {
    pub(crate) fn push(&mut self, part: Vec<u8>) {
        let fits = self.frames.last().is_some_and(|last| {
            last.len() < BLOBS_A_FRAME_AT_MOST
                && self.last_bytes + part.len() <= SNAPSHOT_FRAME_SHARE
        });

        if !fits {
            self.frames.push(Vec::new());
            self.last_bytes = 0;
        }
        self.last_bytes += part.len();
        if let Some(last) = self.frames.last_mut() {
            last.push(part);
        }
    }

    const fn more_than_a_frame(&self) -> bool {
        self.frames.len() > 1
    }

    #[cfg(test)]
    pub(crate) fn frame_lengths(&self) -> Vec<usize> {
        self.frames.iter().map(Vec::len).collect()
    }
}

#[derive(Debug)]
struct Staging {
    through: u64,
    records_since: u64,
    frames: Vec<Vec<Vec<u8>>>,
}

#[derive(Debug, Default)]
pub(crate) struct InParts {
    waiting: Option<Staging>,
    refused: bool,
}

impl InParts {
    pub(crate) fn asked(&mut self, through: u64, records_since: u64, pieces: Pieces) -> ToServer {
        let mut frames = pieces.frames;

        if frames.len() <= 1 {
            return ToServer::Compact {
                through,
                parts: frames.pop().unwrap_or_default(),
            };
        }
        self.waiting = Some(Staging {
            through,
            records_since,
            frames,
        });
        ToServer::Stage
    }

    pub(crate) fn taken(&mut self) -> Vec<ToServer> {
        let Some(Staging {
            through,
            mut frames,
            ..
        }) = self.waiting.take()
        else {
            return Vec::new();
        };
        let last = frames.pop().unwrap_or_default();
        frames
            .into_iter()
            .map(|parts| ToServer::Staged { parts })
            .chain([ToServer::CompactStaged {
                through,
                parts: last,
            }])
            .collect()
    }

    pub(crate) fn refused_as_stale(&mut self) -> Option<Oversized> {
        let staging = self.waiting.take()?;

        self.refused = true;
        Some(Oversized::found_at(staging.records_since))
    }
}

fn compacted_by_another(asked: &mut Option<u64>, through: u64) -> bool {
    if *asked == Some(through) {
        *asked = None;
        return false;
    }
    true
}

fn the_connection_kept(this: DeviceId, other: DeviceId, dialled_here: bool) -> bool {
    dialled_here == (this < other)
}

fn the_new_connection_kept(
    this: DeviceId,
    other: DeviceId,
    dialled_here: bool,
    standing_for: Duration,
) -> bool {
    standing_for >= DIALS_CROSS_WITHIN || the_connection_kept(this, other, dialled_here)
}

struct Advertised {
    beacon: Beacon,
    sealed_at: u64,
    record: String,
}

impl Advertised {
    fn still_says(&self, beacon: &Beacon, now: u64) -> bool {
        self.beacon == *beacon
            && now.saturating_sub(self.sealed_at) < RECORD_RESEALED_EVERY.as_secs()
    }
}

enum Parting {
    Ended,
    Failed(Error),
    CatalogRefused(Refused),
}

impl Parting {
    fn of(error: Error) -> Self {
        match error {
            Error::Catalog(refused) => Self::CatalogRefused(refused),
            error => Self::Failed(error),
        }
    }
}

fn moved_on(caught_up: bool, read_from: u64, cursor: Option<u64>, parting: &Parting) -> bool {
    let still_caught_up = caught_up && !matches!(parting, Parting::CatalogRefused(_));
    still_caught_up || cursor.is_some_and(|cursor| cursor > read_from)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Backoff {
    wait: Duration,
}

impl Backoff {
    const fn first() -> Self {
        Self { wait: FIRST_RETRY }
    }

    const fn reset(&mut self) {
        self.wait = FIRST_RETRY;
    }

    const fn longest(&mut self) {
        self.wait = LAST_RETRY;
    }

    fn next(&mut self) -> Duration {
        let wait = self.wait;

        self.wait = (wait * 2).min(LAST_RETRY);
        wait
    }
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
    backoff: Backoff,
    server_online: BTreeSet<DeviceId>,
    caught_up_once: bool,
    neighbour_caught_up: bool,
    neighbours: BTreeMap<DeviceId, Neighbour>,
    heard: BTreeMap<DeviceId, (Vec<SocketAddr>, Instant)>,
    dialled_at: BTreeMap<DeviceId, Redial>,
    dialling_back: BTreeSet<DeviceId>,
    peers: BTreeMap<DeviceId, Connection>,
    punched_back: BTreeMap<DeviceId, Instant>,
    strangers_answered: BTreeMap<DeviceId, Instant>,
    punches_heard: HeardOnce,
    beacons_heard: HeardOnce,
    records_resolved: HeardOnce,
    unsent: Unsent,
    followed: BTreeMap<DeviceId, Instant>,
    following: BTreeSet<DeviceId>,
    follow_owed: BTreeSet<DeviceId>,
    unanswered: Unanswered,
    covers_asked: BTreeMap<DeviceId, Instant>,
    lyricsfiles_asked: BTreeMap<DeviceId, Instant>,
    kept_asked: BTreeMap<DeviceId, Instant>,
    prints_asked: BTreeMap<DeviceId, Instant>,
    follow_again_at: Option<Instant>,
    followed_again_at: Option<Instant>,
    look_at: Option<Instant>,
    looked_at: Instant,
    looked_once: bool,
    seen_revision: u64,
    oversized: Oversized,
    discovery: Discovery,
}

impl Sync {
    pub fn start(settings: Settings, catalog: Arc<dyn Catalog>) -> Result<Self> {
        Self::started(settings, catalog, resolved_by_the_system())
    }

    fn started(settings: Settings, catalog: Arc<dyn Catalog>, resolver: Resolver) -> Result<Self> {
        let derived = settings.key.derived()?;
        let identity = PeerIdentity::of(&derived)?;
        let roots = trusted_roots(settings.trusted.as_deref())?;
        let to_web = to_the_web(roots.clone())?;
        let to_server = to_the_server(roots)?;
        let to_peers = identity.dialling()?;
        let serving = identity.serving()?;

        swept(&settings.staging, A_PARTIAL_IS_SWEPT_AFTER);

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
            controlled: RwLock::new(None),
            consulted: RwLock::new(None),
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
            resolver,
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
            fetching: Claims::default(),
            passed_over: Mutex::new(Vec::new()),
            lyricsfiles_skipped: Mutex::new(BTreeMap::new()),
            prints_skipped: Mutex::new(BTreeMap::new()),
            mirrors: RwLock::new(BTreeMap::new()),
            watched: Mutex::new(BTreeMap::new()),
            watching: Mutex::new(BTreeSet::new()),
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
            thread: Mutex::new(Some(thread)),
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
        match beacon_recorded(&self.running.derived, record, wall_seconds()) {
            Ok(beacon) => {
                let _ = self.commands.send(Command::Heard(beacon));
            }
            Err(unheard) => tracing::debug!(%unheard, "a Bonjour record was passed over"),
        }
    }

    pub fn fetch(&self, recordings: Vec<Recording>, from: DeviceId) {
        let _ = self.commands.send(Command::Fetch { recordings, from });
    }

    pub fn refused(&self, recording: &Recording) {
        if let Err(error) = self.ledger.lock().refused(recording) {
            tracing::warn!(%error, "a song refused here could not be remembered");
        }
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

    pub fn layouts(&self) -> Result<Vec<LayoutNote>> {
        let mut layouts = self.ledger.lock().layouts()?;

        layouts.retain(|note| note.device != self.device);
        layouts.sort_by_key(|note| Reverse(note.stamp));
        Ok(layouts)
    }

    pub fn control_with(&self, controlled: Arc<dyn Controlled>, trust: RemoteTrust) {
        *self.running.answering.controlled.write() = Some((controlled, trust));
    }

    pub fn trust(&self, trust: RemoteTrust) {
        if let Some((_, held)) = self.running.answering.controlled.write().as_mut() {
            *held = trust;
        }
    }

    pub fn uncontrolled(&self) {
        *self.running.answering.controlled.write() = None;
    }

    pub fn consult_with(&self, consulted: Arc<dyn Consultable>) {
        *self.running.answering.consulted.write() = Some(consulted);

        let _ = self.commands.send(Command::KeptOwed);
    }

    pub fn ask_for_export(
        &self,
        device: DeviceId,
        wanted: ExportWanted,
        receiving: Arc<ExportReceiving>,
    ) -> AskedForExport {
        let (answer, answered) = std_mpsc::channel();
        let _ = self.commands.send(Command::Export {
            device,
            asked: ExportAsked {
                part: wanted.part,
                held: wanted.held,
                staged: wanted.staged,
                noted: wanted.noted,
                receiving,
                answer,
            },
        });
        AskedForExport(answered)
    }

    pub fn ask_to_consult(&self, device: DeviceId, asked: Vec<u8>) -> AskedToConsult {
        let (answer, answered) = std_mpsc::channel();
        let _ = self.commands.send(Command::Consult {
            device,
            asked,
            answer,
        });
        AskedToConsult(answered)
    }

    pub fn control(&self, device: DeviceId, zone: ZoneId, command: RemoteCommand) -> Controlling {
        let drawn = self.queue_revision_mirrored(device, zone);

        self.ask_to_control(device, zone, command, drawn).answer()
    }

    pub fn ask_to_control(
        &self,
        device: DeviceId,
        zone: ZoneId,
        command: RemoteCommand,
        drawn: Option<u64>,
    ) -> AskedToControl {
        let (answer, answered) = std_mpsc::channel();
        let (awaited, awaiting) = awaited(std::time::Instant::now());
        let queue_seen = command.names_a_row().then_some(drawn).flatten();

        let asked = Command::Control {
            device,
            zone,
            command,
            queue_seen,
            answer,
            awaited,
        };
        let _ = self.commands.send(asked);
        AskedToControl { answered, awaiting }
    }

    fn queue_revision_mirrored(&self, device: DeviceId, zone: ZoneId) -> Option<u64> {
        self.running
            .mirrors
            .read()
            .get(&device)
            .and_then(Watched::mirror)
            .and_then(|mirror| mirror.zone(zone))
            .and_then(|zone| zone.queue_revision)
    }

    pub fn watch(&self, device: DeviceId) {
        self.running
            .watched
            .lock()
            .insert(device, std::time::Instant::now());
        if !self.running.watching.lock().contains(&device) {
            let _ = self.commands.send(Command::Watch(device));
        }
    }

    pub fn unwatch(&self, device: DeviceId) {
        self.running.watched.lock().remove(&device);
        self.running.mirrors.write().remove(&device);
    }

    pub fn watched(&self, device: DeviceId) -> Watched {
        self.running
            .mirrors
            .read()
            .get(&device)
            .cloned()
            .unwrap_or_default()
    }

    pub fn leave(self) {
        self.stop();
    }

    pub fn stop(&self) {
        let mut thread = self.thread.lock();
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = thread.take() {
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
    async fn failed(&self, recording: &Recording) {
        let ledger = Arc::clone(&self.ledger);
        let refused = recording.clone();
        if let Err(error) = blocking(move || ledger.lock().refused(&refused)).await {
            tracing::warn!(%error, "a song refused here could not be remembered");
        }
    }

    async fn asked_again(&self, recordings: &[Recording]) {
        let ledger = Arc::clone(&self.ledger);
        let asked = recordings.to_vec();
        if let Err(error) = blocking(move || ledger.lock().asked_again(&asked)).await {
            tracing::warn!(%error, "the songs asked for again could not be forgiven");
        }
    }

    async fn not_failed_lately(&self, recordings: Vec<Recording>) -> Vec<Recording> {
        let ledger = Arc::clone(&self.ledger);
        blocking(move || {
            ledger
                .lock()
                .not_refused_within(recordings, A_FAILED_SONG_WAITS)
        })
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the songs refused lately could not be read");
            Vec::new()
        })
    }

    fn passed_over(&self, device: DeviceId, recording: Recording, asked: bool) {
        self.passed_over.lock().push(PassedOver {
            device,
            recording,
            asked,
        });
    }

    fn unpassed(&self, recording: &Recording) -> Vec<PassedOver> {
        let mut passed = self.passed_over.lock();
        let (again, kept) = mem::take(&mut *passed)
            .into_iter()
            .partition(|passed| passed.recording.names_the_same_song_as(recording));

        *passed = kept;
        again
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
        let now = wall_seconds();
        let mut held = self.record.lock();

        if let Some(advertised) = held.as_ref()
            && advertised.still_says(&beacon, now)
        {
            return Some(advertised.record.clone());
        }

        let record = record_of(&self.derived, &beacon, now)
            .inspect_err(
                |error| tracing::debug!(%error, "this device's record could not be sealed"),
            )
            .ok()?;

        *held = Some(Advertised {
            beacon,
            sealed_at: now,
            record: record.clone(),
        });
        Some(record)
    }

    fn listed_waiting_as(&self, recording: &Recording, from: DeviceId, state: TransferState) {
        self.shown.write().listed_waiting_as(recording, from, state);
    }

    fn unlisted_waiting(&self, recording: &Recording, from: DeviceId) {
        self.shown.write().unlisted_waiting(recording, from);
    }

    fn queued(&self, recording: &Recording, from: DeviceId) {
        self.shown.write().queued(recording, from);
    }

    fn still_watched(&self, device: DeviceId) -> bool {
        self.watched
            .lock()
            .get(&device)
            .is_some_and(|asked| asked.elapsed() < WATCH_LAPSES_AFTER)
    }

    fn transfer(&self, recording: &Recording, from: DeviceId, change: impl FnOnce(&mut Transfer)) {
        self.shown.write().transfer(recording, from, change);
    }
}

fn swept(staging: &Path, untouched_for: Duration) {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let partial = path
            .extension()
            .is_some_and(|extension| extension == "part" || extension == "from");
        let untouched = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|since| since >= untouched_for);
        if partial
            && untouched
            && let Err(error) = std::fs::remove_file(&path)
        {
            tracing::debug!(%error, path = %path.display(), "a stale partial could not be swept");
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
                    syncing.unlinked(Parting::Ended);
                    Ok(())
                }
                Some(Err(error)) => {
                    syncing.unlinked(Parting::Failed(error));
                    Ok(())
                }
                Some(Ok(frame)) => {
                    if let Err(error) = syncing.heard_from_server(frame).await {
                        syncing.unlinked(Parting::of(error));
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
                Some(Command::Fetch { recordings, from }) => syncing.want(from, Work::Fetch { recordings, asked: true }).await,
                Some(Command::Control { device, zone, command, queue_seen, answer, awaited }) => syncing.want(device, Work::Control { zone, command, queue_seen, answer, awaited }).await,
                Some(Command::Consult { device, asked, answer }) => syncing.want(device, Work::Consult { asked, answer }).await,
                Some(Command::Export { device, asked }) => syncing.want(device, Work::Export(asked)).await,
                Some(Command::KeptOwed) => {
                    syncing.ask_every_peer_for_kept();
                    Ok(())
                }
                Some(Command::Watch(device)) => {
                    if running.watching.lock().insert(device) {
                        syncing.want(device, Work::Watch).await
                    } else {
                        Ok(())
                    }
                }
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
                    Found::Record { record, at } => syncing.record_resolved(&record, &at).await,
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
            _ = watch.tick() => {
                syncing.expire_unsent();
                syncing.watched().await
            }
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

pub(crate) async fn resolve(server: &str) -> Result<(Vec<SocketAddr>, String)> {
    let (host, port) = host_and_port(server.trim());

    let resolved = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|source| Error::Resolve {
            server: server.into(),
            source,
        })?;
    let routes = ipv4_first(each_once(resolved));

    if routes.is_empty() {
        return Err(Error::Unresolved {
            server: server.into(),
        });
    }
    Ok((routes, host))
}

fn ipv4_first(routes: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let (ipv4, ipv6): (Vec<_>, Vec<_>) = routes.into_iter().partition(SocketAddr::is_ipv4);
    [ipv4, ipv6].concat()
}

pub(crate) async fn dialled_server(
    endpoint: &Endpoint,
    config: &ClientConfig,
    server: &str,
) -> Result<(Connection, SocketAddr)> {
    let (routes, host) = within(resolve(server)).await?;
    within(dialled_through(endpoint, config, server, &routes, &host)).await
}

async fn dialled_through(
    endpoint: &Endpoint,
    config: &ClientConfig,
    server: &str,
    routes: &[SocketAddr],
    host: &str,
) -> Result<(Connection, SocketAddr)> {
    first_answering(server, routes, |route| async move {
        endpoint
            .connect_with(config.clone(), route, host)
            .map_err(|source| Error::Connect(Box::new(source)))?
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await
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

fn may_answer(table: &mut BTreeMap<DeviceId, Instant>, device: DeviceId, at_most: usize) -> bool {
    table.retain(|_, at| at.elapsed() < PUNCH_ANSWERED_AT_MOST_EVERY);
    if table.contains_key(&device) || table.len() >= at_most {
        return false;
    }
    table.insert(device, Instant::now());
    true
}

#[derive(Debug, thiserror::Error)]
enum Unpunched {
    #[error("its candidates do not read")]
    Unread(#[source] Error),

    #[error("its candidates were sealed by {asker:?} for {asked:?}")]
    ForAnotherPair { asker: DeviceId, asked: DeviceId },

    #[error("its candidates are not fresh")]
    Unfresh(#[source] Unfresh),
}

struct PunchHeard<'a> {
    from: DeviceId,
    to: DeviceId,
    address: SocketAddr,
    sealed: &'a [u8],
}

fn punched_candidates(
    derived: &Derived,
    heard: PunchHeard<'_>,
    punches: &mut HeardOnce,
    now: u64,
) -> std::result::Result<Vec<SocketAddr>, Unpunched> {
    let theirs = opened(
        &derived.sealer,
        derived.space,
        heard.sealed,
        LARGEST_CANDIDATES_OPENED,
    )
    .and_then(|plain| Candidates::decoded(&plain))
    .map_err(Unpunched::Unread)?;

    if theirs.asker != heard.from || theirs.asked != heard.to {
        return Err(Unpunched::ForAnotherPair {
            asker: theirs.asker,
            asked: theirs.asked,
        });
    }
    punches
        .admit(heard.sealed, theirs.sealed_at, now)
        .map_err(Unpunched::Unfresh)?;

    let mut candidates = Vec::with_capacity(theirs.addresses.len() + 1);

    if !heard.address.ip().is_unspecified() {
        candidates.push(heard.address);
    }
    candidates.extend(theirs.addresses);
    candidates.dedup();

    Ok(candidates)
}

pub(crate) async fn within<T>(waiting: impl Future<Output = Result<T>>) -> Result<T> {
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

    let (connection, address) =
        dialled_server(&running.endpoint, &running.to_server, server).await?;
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

    let (reading, mut frames) = reading_from_the_server(recv, Arc::clone(&running.reflector));
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
        read_from: cursor,
        caught_up: cursor >= head,
        in_flight: VecDeque::new(),
        compacting: None,
        in_parts: InParts::default(),
    })
}

async fn welcomed(
    running: &Running,
    send: &mut Outgoing,
    frames: &mut mpsc::Receiver<Result<FromServer>>,
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

fn reading_from_the_server(
    recv: Incoming,
    reflector: Arc<Reflector>,
) -> (JoinHandle<()>, mpsc::Receiver<Result<FromServer>>) {
    let (frames_in, frames) = mpsc::channel(FRAMES_FROM_THE_SERVER_QUEUED_AT_MOST);
    (
        tokio::spawn(read_frames(recv, frames_in, reflector)),
        frames,
    )
}

async fn read_frames(
    mut recv: Incoming,
    frames: mpsc::Sender<Result<FromServer>>,
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
        if frames.send(frame).await.is_err() || failed {
            return;
        }
    }
}

impl Syncing {
    fn new(running: Arc<Running>) -> Self {
        let now = Instant::now();
        let retry_at = running.server.as_ref().map(|_| now);
        let discovery = Discovery::new(running.stun.as_deref(), Arc::clone(&running.resolver));

        Self {
            running,
            server: None,
            retry_at,
            backoff: Backoff::first(),
            server_online: BTreeSet::new(),
            caught_up_once: false,
            neighbour_caught_up: false,
            neighbours: BTreeMap::new(),
            heard: BTreeMap::new(),
            dialled_at: BTreeMap::new(),
            dialling_back: BTreeSet::new(),
            peers: BTreeMap::new(),
            punched_back: BTreeMap::new(),
            strangers_answered: BTreeMap::new(),
            punches_heard: HeardOnce::new(PUNCH_SEALED_WITHIN, PUNCHES_HEARD_AT_MOST),
            beacons_heard: HeardOnce::new(BEACON_SENT_WITHIN, BEACONS_HEARD_AT_MOST),
            records_resolved: HeardOnce::new(BEACON_SENT_WITHIN, BEACONS_HEARD_AT_MOST),
            unsent: Unsent::default(),
            followed: BTreeMap::new(),
            following: BTreeSet::new(),
            follow_owed: BTreeSet::new(),
            unanswered: Unanswered::default(),
            covers_asked: BTreeMap::new(),
            lyricsfiles_asked: BTreeMap::new(),
            kept_asked: BTreeMap::new(),
            prints_asked: BTreeMap::new(),
            follow_again_at: None,
            followed_again_at: None,
            look_at: None,
            looked_at: now,
            looked_once: false,
            seen_revision: 0,
            oversized: Oversized::default(),
            discovery,
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
        self.look_at = Some(Instant::now());
        self.running.stand(Standing::Connected);
        self.publish_devices();
        self.start_waiting().await?;
        for device in self.server_online.clone() {
            self.follow(device).await?;
        }
        Ok(())
    }

    fn unlinked(&mut self, parting: Parting) {
        if let Some(link) = self.server.take() {
            let cursor = self.running.ledger.lock().cursor().ok();

            if moved_on(link.caught_up, link.read_from, cursor, &parting) {
                self.backoff.reset();
            }
            link.hang_up();
        }
        self.server_online.clear();
        match parting {
            Parting::Ended => self.running.stand(Standing::Unreachable),
            Parting::CatalogRefused(refused) => {
                tracing::warn!(
                    ?refused,
                    "this device's catalog refused a record from the sync server, so catch-up stops before it and is tried again later"
                );
                self.running.stand(Standing::Connecting);
            }
            Parting::Failed(Error::SpaceMadeAnew) => {
                tracing::info!("the sync server holds this space afresh; everything is sent again");
                self.running.stand(Standing::Connecting);
                self.backoff.reset();
            }
            Parting::Failed(Error::Refused(refusal)) => {
                tracing::warn!(?refusal, "the sync server refused this device");
                self.running.stand(Standing::Refused(refusal));
                self.backoff.longest();
            }
            Parting::Failed(error) => {
                tracing::debug!(%error, server = ?self.running.server, "the sync server could not be reached");
                self.running.stand(Standing::Unreachable);
            }
        }
        self.retry_at = Some(Instant::now() + jittered(self.backoff.next()));
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
            Event::Linked(Err(error)) => self.unlinked(Parting::Failed(error)),
            Event::Relayed(device, Some(connection)) if self.reached(device) => {
                connection.close(0_u32.into(), b"reached directly");
            }
            Event::Punched(device, Some(connection)) | Event::Relayed(device, Some(connection)) => {
                self.watch_closing(device, &connection);
                self.send_unsent(device, &connection);
                self.ask_for_covers(device, &connection);
                self.ask_for_kept(device, &connection);
                self.ask_for_lyricsfiles(device, &connection);
                self.ask_for_prints(device, &connection);
                self.peers.insert(device, connection);
            }
            Event::Punched(device, None) => self.relayed_or_unreached(device),
            Event::Relayed(device, None) => self.unreached(device),
            Event::CutShort(device) => {
                self.followed.remove(&device);
            }
            Event::Followed(device) => {
                self.following.remove(&device);
                if self.follow_owed.remove(&device) {
                    self.followed.remove(&device);
                    self.follow(device).await?;
                }
            }
            Event::Unclaimed(passed) => {
                let mut again: BTreeMap<(DeviceId, bool), Vec<Recording>> = BTreeMap::new();

                for passed in passed {
                    again
                        .entry((passed.device, passed.asked))
                        .or_default()
                        .push(passed.recording);
                }
                for ((device, asked), recordings) in again {
                    self.want(device, Work::Fetch { recordings, asked }).await?;
                }
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
            Event::Met(device, connection) => self.met(device, connection, true, true).await?,
            Event::Discovered(public) => self.discovered(public)?,
            Event::Dropped(id, error) => {
                if self
                    .server
                    .as_ref()
                    .is_some_and(|link| link.carrier.id() == id)
                {
                    self.unlinked(Parting::Failed(error));
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
            } => self.met(device, connection, records, false).await,
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
                self.unanswered = Unanswered::default();

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
                self.compact().await?;
                self.publish_settled();
            }
            FromServer::Arrived { device } => {
                self.server_online.insert(device);
                self.publish_devices();
                if self.unsent.holds(device) {
                    self.punch(device)?;
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
                    .is_some_and(|link| compacted_by_another(&mut link.compacting, through));
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
                self.push().await?;
            }
            FromServer::Staging => {
                if let Some(link) = self.server.as_mut() {
                    for frame in link.in_parts.taken() {
                        link.say(&frame)?;
                    }
                }
            }
            FromServer::Refused(Refusal::Stale) => {
                if let Some(link) = self.server.as_mut() {
                    link.compacting = None;
                    if let Some(oversized) = link.in_parts.refused_as_stale() {
                        tracing::warn!(
                            "the sync server takes no compaction past one frame, so it is tried again once the records since the last snapshot double"
                        );
                        self.oversized = oversized;
                    }
                }
                self.push().await?;
            }
            FromServer::Refused(Refusal::SpaceFull)
                if self
                    .server
                    .as_ref()
                    .is_some_and(|link| link.compacting.is_some()) =>
            {
                self.compaction_refused_as_full().await?;
            }
            FromServer::Refused(Refusal::TooLarge) => {
                self.unanswered = Unanswered::default();

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
            FromServer::Welcome { .. }
            | FromServer::Relayed { .. }
            | FromServer::Closed { .. }
            | FromServer::Heads { .. }
            | FromServer::RoomWelcome { .. }
            | FromServer::RoomHeard { .. }
            | FromServer::RoomArrived { .. }
            | FromServer::RoomLeft { .. }
            | FromServer::RoomPong { .. }
            | FromServer::RoomEnded => {}
        }
        Ok(())
    }

    async fn compaction_refused_as_full(&mut self) -> Result<()> {
        if let Some(link) = self.server.as_mut() {
            link.compacting = None;
            link.in_parts.waiting = None;
        }

        let ledger = Arc::clone(&self.running.ledger);
        let (records_since, _) = blocking(move || ledger.lock().since_snapshot()).await?;

        tracing::warn!(
            records_since,
            "the sync server's space is too full to take a compaction, so it is tried again once the records since the last snapshot double"
        );
        self.oversized = Oversized::found_at(records_since);
        self.push().await
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

    fn knows_device(&self, device: DeviceId) -> bool {
        self.running
            .ledger
            .lock()
            .knows_device(device)
            .unwrap_or(false)
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

        self.running.reflector.relay().admit(
            notes
                .iter()
                .map(|note| note.device)
                .chain(self.server_online.iter().copied()),
        );

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
        let from_elsewhere = blobs.iter().any(Option::is_some);
        let running = Arc::clone(&self.running);

        blocking(move || {
            let mut ledger = running.ledger.lock();

            for (at, blob) in blobs.into_iter().enumerate() {
                let length = blob.as_ref().map_or(0, Vec::len);

                if let Some(blob) = blob {
                    match opened(
                        &running.derived.sealer,
                        running.derived.space,
                        &blob,
                        LARGEST_OPENED,
                    )
                    .and_then(|plain| read_bundle(&plain))
                    {
                        Ok(read) => {
                            let refused = read
                                .unread
                                .iter()
                                .filter(|unread| matches!(unread, Unread::Refused(_)))
                                .count();

                            if refused > 0 {
                                tracing::warn!(
                                    refused,
                                    "synced changes that do not read were kept to carry forward beside the rest of their record"
                                );
                            }
                            ledger.arrived(running.catalog.as_ref(), &read.changes)?;
                            ledger.unread_arrived(&read.unread)?;
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
        if from_elsewhere {
            self.follow_again_soon();
        }
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
        if self.server.as_ref().is_none_or(Link::answer_awaited) {
            return Ok(());
        }

        let running = Arc::clone(&self.running);
        let unanswered = mem::take(&mut self.unanswered);

        let bundles = blocking(move || {
            let mut ledger = running.ledger.lock();

            let owed = ledger.owed()?;
            let mut sealed_bundles = Vec::new();

            for changes in owed {
                if sealed_bundles.len() == BLOBS_PER_APPEND {
                    break;
                }

                let plain = encoded_bundle(&changes);
                let blob = match unanswered.sealed_as(&plain) {
                    Some(blob) => blob.clone(),
                    None => sealed(&running.derived.sealer, running.derived.space, &plain)?,
                };

                if blob.len() > LARGEST_RECORD {
                    tracing::warn!(
                        bytes = blob.len(),
                        "a change too large for the sync server is kept for devices on this network"
                    );
                    ledger.sent(&changes)?;
                    continue;
                }
                sealed_bundles.push((changes, plain_digest(&plain), blob));
            }
            Ok(sealed_bundles)
        })
        .await?;

        self.unanswered = Unanswered(
            bundles
                .iter()
                .map(|(_, plain, blob)| (*plain, blob.clone()))
                .collect(),
        );

        let Some(link) = self.server.as_mut() else {
            return Ok(());
        };

        if bundles.is_empty() {
            return Ok(());
        }

        let (pushed, blobs): (Vec<_>, Vec<_>) = bundles
            .into_iter()
            .map(|(changes, _, blob)| {
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
            .is_none_or(|link| !link.caught_up || link.answer_awaited())
        {
            return Ok(());
        }

        let running = Arc::clone(&self.running);
        let oversized = self.oversized;
        let parts_refused = self
            .server
            .as_ref()
            .is_some_and(|link| link.in_parts.refused);

        let snapshot = blocking(move || {
            let ledger = running.ledger.lock();
            let (records_since, bytes) = ledger.since_snapshot()?;

            if (records_since < COMPACT_PAST_RECORDS && bytes < COMPACT_PAST_BYTES)
                || oversized.waits(records_since)
            {
                return Ok(Snapshotted::Unneeded);
            }

            let through = ledger.cursor()?;
            let mut pieces = Pieces::default();

            for plain in ledger.snapshot_bundles()? {
                pieces.push(sealed(
                    &running.derived.sealer,
                    running.derived.space,
                    &plain,
                )?);
                if parts_refused && pieces.more_than_a_frame() {
                    tracing::warn!(
                        records_since,
                        "the synced state is too large to compact in one frame on this server, so it is tried again once the records since the last snapshot double"
                    );
                    return Ok(Snapshotted::Oversized { records_since });
                }
            }
            Ok(Snapshotted::Sealed {
                through,
                records_since,
                pieces,
            })
        })
        .await?;

        match snapshot {
            Snapshotted::Unneeded => {}
            Snapshotted::Oversized { records_since } => {
                self.oversized = Oversized::found_at(records_since);
            }
            Snapshotted::Sealed {
                through,
                records_since,
                pieces,
            } => {
                self.oversized = Oversized::default();
                if let Some(link) = self.server.as_mut() {
                    link.compacting = Some(through);

                    let asked = link.in_parts.asked(through, records_since, pieces);

                    link.say(&asked)?;
                }
            }
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

    fn ask_every_peer_for_kept(&mut self) {
        let standing: Vec<(DeviceId, Connection)> = self
            .peers
            .iter()
            .filter(|(_, connection)| connection.close_reason().is_none())
            .map(|(device, connection)| (*device, connection.clone()))
            .collect();
        for (device, connection) in standing {
            self.ask_for_kept(device, &connection);
        }
    }

    fn ask_for_kept(&mut self, device: DeviceId, connection: &Connection) {
        if device == self.running.device || !self.running.transfers_allowed() {
            return;
        }

        let Some(consulted) = self.running.answering.consulted.read().clone() else {
            return;
        };
        let now = Instant::now();

        if self
            .kept_asked
            .get(&device)
            .is_some_and(|at| now.duration_since(*at) < KEPT_ASKED_OF_A_DEVICE_EVERY)
        {
            return;
        }
        self.kept_asked.insert(device, now);

        let asker = self.running.device;
        let connection = connection.clone();

        tokio::spawn(async move {
            match kept_from(&connection, asker, device, consulted).await {
                Ok(0) => {}
                Ok(taken) => tracing::info!(taken, "lookup answers arrived from another device"),
                Err(error) => {
                    tracing::debug!(%error, "a device was not asked for the answers it kept");
                }
            }
        });
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
        if work.unawaited(
            std::time::Instant::now(),
            self.running.still_watched(device),
        ) {
            self.unreached_work(device, work);
            return Ok(());
        }
        match &work {
            Work::Fetch { recordings, .. } => {
                for recording in recordings {
                    self.running.queued(recording, device);
                }
            }
            Work::Follow if self.following.contains(&device) => {
                self.follow_owed.insert(device);
                return Ok(());
            }
            Work::Follow
            | Work::Control { .. }
            | Work::Watch
            | Work::Consult { .. }
            | Work::Export(_) => {}
        }
        if let Some(connection) = self
            .peers
            .get(&device)
            .filter(|connection| connection.close_reason().is_none())
            .cloned()
            && self.running.transfers_allowed()
        {
            self.spawn(device, connection, work);
            return Ok(());
        }

        let first = self.unsent.queue(device, work, std::time::Instant::now());

        if first && self.running.transfers_allowed() {
            self.reach(device).await?;
        }
        Ok(())
    }

    fn spawn(&mut self, device: DeviceId, connection: Connection, work: Work) {
        if matches!(work, Work::Follow) {
            self.following.insert(device);
        }
        spawn_work(Arc::clone(&self.running), device, connection, work);
    }

    fn send_unsent(&mut self, device: DeviceId, connection: &Connection) {
        let now = std::time::Instant::now();
        let watched = self.running.still_watched(device);

        for work in self.unsent.taken(device) {
            if work.unawaited(now, watched) {
                self.unreached_work(device, work);
            } else {
                self.spawn(device, connection.clone(), work);
            }
        }
    }

    fn expire_unsent(&mut self) {
        let running = &self.running;
        let lapsed = self.unsent.lapsed(std::time::Instant::now(), |device| {
            running.still_watched(device)
        });

        for (device, work) in lapsed {
            self.unreached_work(device, work);
        }
    }

    fn reached(&self, device: DeviceId) -> bool {
        self.peers
            .get(&device)
            .is_some_and(|connection| connection.close_reason().is_none())
    }

    async fn start_waiting(&mut self) -> Result<()> {
        if !self.running.transfers_allowed() {
            return Ok(());
        }

        let devices = self.unsent.devices();

        for device in devices {
            self.reach(device).await?;
        }
        Ok(())
    }

    async fn reach(&mut self, device: DeviceId) -> Result<()> {
        if self.server_online.contains(&device) {
            self.punch(device)
        } else {
            self.dial_neighbour(device);
            Ok(())
        }
    }

    fn relayed_or_unreached(&mut self, device: DeviceId) {
        let wanted = self.unsent.holds(device) && !self.peers.contains_key(&device);

        if !wanted || self.server.is_none() || !self.running.reflector.relay().is_attached() {
            self.unreached(device);
            return;
        }

        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            let Some(address) = running.reflector.relay().address_of(device) else {
                let _ = running.events.send(Event::Relayed(device, None));

                return;
            };
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
        for work in self.unsent.taken(device) {
            self.unreached_work(device, work);
        }
    }

    fn unreached_work(&self, device: DeviceId, work: Work) {
        match work {
            Work::Fetch { recordings, .. } => {
                self.running.shown.write().unreached(&recordings, device);
            }
            Work::Watch => {
                self.running.watching.lock().remove(&device);
            }
            Work::Follow | Work::Control { .. } | Work::Consult { .. } | Work::Export(_) => {}
        }
    }

    fn punch(&mut self, device: DeviceId) -> Result<()> {
        if self.server.is_none() {
            return Ok(());
        }
        match self.discovery.public_for(
            device,
            std::time::Instant::now(),
            &self.running.reflector,
            &self.running.events,
        ) {
            Public::Known(public) => self.punch_offering(device, public),
            Public::Awaited => Ok(()),
        }
    }

    fn discovered(&mut self, public: Option<SocketAddr>) -> Result<()> {
        let awaiting = self.discovery.learnt(public, std::time::Instant::now());

        for device in awaiting {
            self.punch_offering(device, public)?;
        }
        Ok(())
    }

    fn punch_offering(&mut self, device: DeviceId, public: Option<SocketAddr>) -> Result<()> {
        let candidates = self.candidates(public);
        let Some(link) = self.server.as_ref() else {
            return Ok(());
        };
        let candidates = Candidates {
            asker: self.running.device,
            asked: device,
            sealed_at: wall_seconds(),
            addresses: candidates,
        };
        let sealed = sealed(
            &self.running.derived.sealer,
            self.running.derived.space,
            &candidates.encoded(),
        )?;

        link.say(&ToServer::Punch { to: device, sealed })?;
        self.unsent.tried(device, std::time::Instant::now());
        Ok(())
    }

    fn candidates(&self, public: Option<SocketAddr>) -> Vec<SocketAddr> {
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
        candidates.extend(public);
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

        let connected = self
            .peers
            .get(&device)
            .is_some_and(|connection| connection.close_reason().is_none());

        if connected {
            return Ok(());
        }

        let heard = PunchHeard {
            from: device,
            to: self.running.device,
            address,
            sealed,
        };
        let mut candidates = match punched_candidates(
            &self.running.derived,
            heard,
            &mut self.punches_heard,
            wall_seconds(),
        ) {
            Ok(candidates) => candidates,
            Err(unpunched) => {
                tracing::debug!(%unpunched, "a punch was not acted on");
                return Ok(());
            }
        };

        if !self.unsent.holds(device) {
            let known = self.knows_device(device);
            let (table, at_most) = if known {
                (&mut self.punched_back, KNOWN_DEVICES_ANSWERED_AT_MOST)
            } else {
                (&mut self.strangers_answered, STRANGERS_ANSWERED_AT_MOST)
            };

            if !may_answer(table, device, at_most) {
                return Ok(());
            }
            self.punch(device)?;
        }
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

        let Some(beacon) = self.running.beacon(false) else {
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
                let Some(bytes) = self.sealed_beacon(&beacon) else {
                    return;
                };
                if let Err(error) = socket.send_to(&bytes, told).await {
                    tracing::debug!(%error, %told, "a beacon could not be sent");
                }
            }
        }
    }

    fn sealed_beacon(&self, beacon: &Beacon) -> Option<Vec<u8>> {
        datagram_of(&self.running.derived, beacon, wall_seconds())
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
        match heard_in(
            &self.running.derived,
            bytes,
            &mut self.beacons_heard,
            wall_seconds(),
        ) {
            Ok(beacon) => self.heard_nearby(beacon, &[from], Some(from)).await,
            Err(unheard) => tracing::debug!(%unheard, %from, "a beacon was passed over"),
        }
    }

    async fn record_resolved(&mut self, record: &str, at: &[SocketAddr]) {
        match record_resolved(
            &self.running.derived,
            record,
            &mut self.records_resolved,
            wall_seconds(),
        ) {
            Ok(resolved) => {
                let seen_at = if resolved.first { at } else { &[] };

                self.heard_nearby(resolved.beacon, seen_at, None).await;
            }
            Err(unheard) => tracing::debug!(%unheard, "a Bonjour record was passed over"),
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
            && let Some(answer) = self
                .running
                .beacon(true)
                .and_then(|answering| self.sealed_beacon(&answering))
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
        self.unsent.tried(device, std::time::Instant::now());

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
        let ledger = Arc::clone(&self.running.ledger);

        blocking(move || ledger.lock().forget_refusals()).await?;

        let catalog = Arc::clone(&self.running.catalog);

        blocking(move || catalog.forgive_deletions().map_err(Error::Catalog)).await?;
        self.backoff.reset();
        self.followed.clear();
        self.covers_asked.clear();
        self.lyricsfiles_asked.clear();
        self.kept_asked.clear();
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

    async fn met(
        &mut self,
        device: DeviceId,
        connection: Connection,
        records: bool,
        dialled_here: bool,
    ) -> Result<()> {
        if device == self.running.device {
            return Ok(());
        }
        if records
            && let Some(standing) = self.neighbours.get(&device)
            && standing.connection.stable_id() != connection.stable_id()
            && standing.connection.close_reason().is_none()
        {
            if !the_new_connection_kept(
                self.running.device,
                device,
                dialled_here,
                standing.met_at.elapsed(),
            ) {
                connection.close(0_u32.into(), b"met already");
                return Ok(());
            }
            standing.connection.close(0_u32.into(), b"met already");
        }
        self.watch_closing(device, &connection);
        self.send_unsent(device, &connection);
        self.ask_for_covers(device, &connection);
        self.ask_for_kept(device, &connection);
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
                    met_at: Instant::now(),
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
        let following = matches!(work, Work::Follow);

        worked(&running, device, &connection, work).await;
        if following {
            let _ = running.events.send(Event::Followed(device));
        }
    });
}

async fn worked(running: &Arc<Running>, device: DeviceId, connection: &Connection, work: Work) {
    let (recordings, asked) = match work {
        Work::Control {
            zone,
            command,
            queue_seen,
            answer,
            ..
        } => {
            let _ = answer.send(control(connection, zone, command, queue_seen).await);

            return;
        }
        Work::Consult { asked, answer } => {
            let _ = answer.send(consult(connection, asked).await);

            return;
        }
        Work::Export(asked) => {
            let staged = ExportStaged {
                part: asked.part,
                held: asked.held.as_ref(),
                staged: &asked.staged,
                noted: &asked.noted,
            };
            let allowed = || running.transfers_allowed();

            let fetched = export_from(connection, device, &staged, &asked.receiving, &allowed)
                .await
                .unwrap_or_else(|error| {
                    tracing::debug!(%error, "MusicBrainz's copy could not be fetched from a device");
                    ExportFetched::Failed
                });
            let _ = asked.answer.send(fetched);

            return;
        }
        Work::Watch => {
            let still_watched = || running.still_watched(device);

            if let Err(error) = watch(connection, device, &running.mirrors, still_watched).await {
                tracing::debug!(%error, "another device's zones could not be watched");
            }
            running.watching.lock().remove(&device);
            if !still_watched() {
                running.watched.lock().remove(&device);
                running.mirrors.write().remove(&device);
            }
            return;
        }
        Work::Fetch {
            recordings,
            asked: true,
        } => {
            running.asked_again(&recordings).await;
            (recordings, true)
        }
        Work::Fetch {
            recordings,
            asked: false,
        } => {
            let wanted = running.not_failed_lately(recordings.clone()).await;

            for recording in &recordings {
                if !wanted.contains(recording) {
                    running.unlisted_waiting(recording, device);
                }
            }
            (wanted, false)
        }
        Work::Follow => match followed_songs(running, connection).await {
            Ok(recordings) => {
                let recordings = running.not_failed_lately(recordings).await;

                for recording in &recordings {
                    running.queued(recording, device);
                }
                (recordings, false)
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
        fetch_one(running, device, connection, recording, asked).await;
    }
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
            let ledger = held_running.ledger.lock();
            let mut missing = Vec::new();

            for recording in one_recording_a_song(holdings.into_iter().map(|held| held.recording)) {
                if !ledger.deleted_everywhere(&recording)? && lacked(catalog, &recording)? {
                    missing.push(recording);
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
    asked: bool,
) {
    if !running.fetching.claim(&recording) {
        running.unlisted_waiting(&recording, device);
        running.passed_over(device, recording, asked);
        return;
    }

    let held = if held_already(running, &recording).await {
        running.unlisted_waiting(&recording, device);
        true
    } else if !asked && deleted_here(running, &recording).await {
        running.unlisted_waiting(&recording, device);
        false
    } else {
        let landed = fetch_claimed(running, device, connection, &recording).await;

        if landed && asked {
            asked_back(running, &recording).await;
        }
        landed
    };

    running.fetching.release(&recording);

    let again = running.unpassed(&recording);
    if !held && !again.is_empty() {
        let _ = running.events.send(Event::Unclaimed(again));
    }
}

async fn asked_back(running: &Arc<Running>, recording: &Recording) {
    let ledger = Arc::clone(&running.ledger);
    let catalog = Arc::clone(&running.catalog);
    let asked = recording.clone();
    if let Err(error) = blocking(move || ledger.lock().asked_back(catalog.as_ref(), &asked)).await {
        tracing::debug!(%error, "a song asked for could not answer its deletion everywhere");
    }
}

async fn deleted_here(running: &Arc<Running>, recording: &Recording) -> bool {
    let catalog = Arc::clone(&running.catalog);
    let ledger = Arc::clone(&running.ledger);
    let asked = recording.clone();
    blocking(move || {
        if ledger.lock().deleted_everywhere(&asked)? {
            return Ok(true);
        }
        catalog.deleted_here(&asked).map_err(Error::Catalog)
    })
    .await
    .unwrap_or_else(|error| {
        tracing::debug!(%error, "whether a song was deleted here could not be read");
        false
    })
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
    recording: &Recording,
) -> bool {
    running.transfer(recording, device, |transfer| {
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
        recording,
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
        running.failed(recording).await;
    }
    running.transfer(recording, device, |transfer| transfer.state = state);
    state == TransferState::Landed
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
    use std::{env, fs::File, process};

    use quinn::{ServerConfig, crypto::rustls::QuicServerConfig};
    use resonate_core::Mbid;
    use rustls::{RootCertStore, crypto::ring::default_provider, version::TLS13};
    use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

    use super::*;
    use crate::{routes::NEXT_ROUTE_AFTER, wire::SERVER_ALPN};

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const A_ROUTE_SETTLES_WITHIN: Duration = Duration::from_secs(5);
    const FRAMES_A_HOSTILE_SERVER_SENDS: u32 = 64;
    const THE_READER_SETTLES_WITHIN: Duration = Duration::from_millis(300);

    #[tokio::test]
    async fn a_device_reads_no_further_from_the_server_than_the_frames_it_has_room_for() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bound");
        let port = listener.local_addr().expect("an address").port();
        let serving = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accepted");
            let mut accepted = crate::web::accepted(stream, None).await.expect("upgraded");

            for count in 0..FRAMES_A_HOSTILE_SERVER_SENDS {
                let frame = FromServer::Appended { count }.encoded();

                accepted.outgoing.send(&frame).await.expect("sent");
            }
            accepted
        });
        let address = web_address(&format!("ws://127.0.0.1:{port}/")).expect("an address");
        let tls = to_the_web(rustls::RootCertStore::empty()).expect("a configuration");
        let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("a socket");
        let reflector = Reflector::over(TokioRuntime.wrap_udp_socket(socket).expect("wrapped"));

        let (_outgoing, incoming) = connect(&address, &tls).await.expect("connected");
        let (reading, mut frames) = reading_from_the_server(incoming, reflector);
        let _server_side = serving.await.expect("the server task");

        tokio::time::sleep(THE_READER_SETTLES_WITHIN).await;

        let queued = frames.len();
        let first = frames.recv().await;

        assert_eq!(queued, FRAMES_FROM_THE_SERVER_QUEUED_AT_MOST);
        assert!(matches!(first, Some(Ok(FromServer::Appended { count: 0 }))));
        assert!(!reading.is_finished());

        reading.abort();
    }

    #[test]
    fn a_device_is_answered_once_in_a_while_and_a_crowd_of_strangers_only_up_to_a_few() {
        let mut table = BTreeMap::new();
        let device = |number: u8| DeviceId([number; 16]);

        assert!(may_answer(&mut table, device(1), 2));
        assert!(!may_answer(&mut table, device(1), 2));
        assert!(may_answer(&mut table, device(2), 2));
        assert!(!may_answer(&mut table, device(3), 2));
        assert_eq!(table.len(), 2);
    }

    fn observed() -> SocketAddr {
        SocketAddr::from(([203, 0, 113, 9], 4000))
    }

    const NOW: u64 = 1_790_000_000;

    fn sealed_punch(derived: &Derived, candidates: &Candidates) -> Vec<u8> {
        sealed(&derived.sealer, derived.space, &candidates.encoded()).expect("sealed")
    }

    fn heard_punch(sealed: &[u8]) -> PunchHeard<'_> {
        PunchHeard {
            from: DeviceId([1; 16]),
            to: DeviceId([2; 16]),
            address: observed(),
            sealed,
        }
    }

    fn fresh_punches() -> HeardOnce {
        HeardOnce::new(PUNCH_SEALED_WITHIN, PUNCHES_HEARD_AT_MOST)
    }

    #[test]
    fn the_address_a_server_observed_joins_the_candidates_only_when_the_sealed_ones_open() {
        let derived = SyncKey::minted()
            .expect("randomness")
            .derived()
            .expect("derived");
        let stranger = SyncKey::minted()
            .expect("randomness")
            .derived()
            .expect("derived");
        let theirs = SocketAddr::from(([192, 168, 1, 7], 5000));
        let candidates = Candidates {
            asker: DeviceId([1; 16]),
            asked: DeviceId([2; 16]),
            sealed_at: NOW,
            addresses: vec![theirs],
        };

        let sealed_candidates = sealed_punch(&derived, &candidates);
        let foreign = sealed_punch(&stranger, &candidates);
        let mut punches = fresh_punches();

        assert_eq!(
            punched_candidates(&derived, heard_punch(&sealed_candidates), &mut punches, NOW).ok(),
            Some(vec![observed(), theirs])
        );
        assert!(matches!(
            punched_candidates(&derived, heard_punch(&foreign), &mut punches, NOW),
            Err(Unpunched::Unread(_))
        ));
        assert!(matches!(
            punched_candidates(&derived, heard_punch(&[0; 8]), &mut punches, NOW),
            Err(Unpunched::Unread(_))
        ));
    }

    #[test]
    fn a_punch_replayed_under_another_pair_late_or_twice_dials_nothing() {
        let derived = SyncKey::minted()
            .expect("randomness")
            .derived()
            .expect("derived");
        let candidates = Candidates {
            asker: DeviceId([1; 16]),
            asked: DeviceId([2; 16]),
            sealed_at: NOW,
            addresses: vec![SocketAddr::from(([192, 168, 1, 7], 5000))],
        };

        let genuine = sealed_punch(&derived, &candidates);
        let to_another = sealed_punch(
            &derived,
            &Candidates {
                asked: DeviceId([3; 16]),
                ..candidates.clone()
            },
        );
        let from_another = sealed_punch(
            &derived,
            &Candidates {
                asker: DeviceId([4; 16]),
                ..candidates.clone()
            },
        );
        let stale = sealed_punch(
            &derived,
            &Candidates {
                sealed_at: NOW - PUNCH_SEALED_WITHIN.as_secs() - 1,
                ..candidates.clone()
            },
        );
        let mut punches = fresh_punches();

        assert!(matches!(
            punched_candidates(&derived, heard_punch(&to_another), &mut punches, NOW),
            Err(Unpunched::ForAnotherPair { .. })
        ));
        assert!(matches!(
            punched_candidates(&derived, heard_punch(&from_another), &mut punches, NOW),
            Err(Unpunched::ForAnotherPair { .. })
        ));
        assert!(matches!(
            punched_candidates(&derived, heard_punch(&stale), &mut punches, NOW),
            Err(Unpunched::Unfresh(Unfresh::OutsideTheWindow { .. }))
        ));
        assert!(punched_candidates(&derived, heard_punch(&genuine), &mut punches, NOW).is_ok());
        assert!(matches!(
            punched_candidates(&derived, heard_punch(&genuine), &mut punches, NOW + 1),
            Err(Unpunched::Unfresh(Unfresh::HeardAlready))
        ));
    }

    fn named(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: None,
        }
    }

    fn identified(title: &str) -> Recording {
        Recording {
            mbid: Some(Mbid::new(ECHOES).expect("an mbid")),
            ..named(title)
        }
    }

    #[test]
    fn a_song_claimed_by_its_mbid_is_claimed_by_its_tags_too_until_released() {
        let claims = Claims::default();

        assert!(claims.claim(&identified("Echoes")));
        assert!(!claims.claim(&named("Echoes")));
        assert!(!claims.claim(&identified("Echoes")));
        assert!(claims.claim(&named("Time")));

        claims.release(&identified("Echoes"));

        assert!(claims.claim(&named("Echoes")));
    }

    #[test]
    fn a_row_is_one_song_however_keyed_and_a_failed_song_asked_again_waits_again() {
        let device = DeviceId([1; 16]);
        let mut shown = Shown::default();

        shown.transfer(&identified("Echoes"), device, |transfer| {
            transfer.state = TransferState::Failed;
        });
        shown.queued(&named("Echoes"), device);

        assert_eq!(shown.transfers.len(), 1);
        assert_eq!(shown.transfers[0].state, TransferState::Waiting);

        shown.unlisted_waiting(&named("Echoes"), device);

        assert!(shown.transfers.is_empty());
    }

    #[test]
    fn a_bundle_still_unanswered_is_sent_again_as_the_same_bytes_and_a_changed_one_is_not() {
        let unanswered = Unanswered(vec![(plain_digest(b"bundle"), vec![7, 7, 7])]);

        assert_eq!(unanswered.sealed_as(b"bundle"), Some(&vec![7, 7, 7]));
        assert_eq!(unanswered.sealed_as(b"another bundle"), None);
    }

    #[test]
    fn a_state_too_large_to_compact_is_sealed_again_only_once_the_records_since_double_or_a_compaction_resets_them()
     {
        let found = Oversized::found_at(600);

        let unfound = Oversized::default().waits(600);
        let next_batch = found.waits(601);
        let short_of_double = found.waits(1_199);
        let doubled = found.waits(1_200);
        let compacted_since = found.waits(512);

        assert!(!unfound);
        assert!(next_batch);
        assert!(short_of_double);
        assert!(!doubled);
        assert!(!compacted_since);
    }

    fn pieces_of(parts: impl IntoIterator<Item = Vec<u8>>) -> Pieces {
        let mut pieces = Pieces::default();

        for part in parts {
            pieces.push(part);
        }
        pieces
    }

    #[test]
    fn a_snapshot_within_a_frame_is_one_compaction_as_an_older_server_takes_it() {
        let mut in_parts = InParts::default();
        let pieces = pieces_of([vec![1; 40], vec![2; 40]]);

        let empty = InParts::default().asked(9, 600, Pieces::default());
        let asked = in_parts.asked(9, 600, pieces);

        assert_eq!(
            empty,
            ToServer::Compact {
                through: 9,
                parts: Vec::new()
            }
        );
        assert_eq!(
            asked,
            ToServer::Compact {
                through: 9,
                parts: vec![vec![1; 40], vec![2; 40]]
            }
        );
        assert!(in_parts.taken().is_empty());
    }

    #[test]
    fn a_snapshot_past_a_frames_share_or_its_blobs_is_cut_into_frames_within_both() {
        let half = SNAPSHOT_FRAME_SHARE / 2;

        let by_bytes = pieces_of([vec![1; half], vec![2; half], vec![3; 1]]);
        let by_blobs = pieces_of((0..=BLOBS_A_FRAME_AT_MOST).map(|_| vec![4; 40]));

        assert_eq!(by_bytes.frame_lengths(), [2, 1]);
        assert_eq!(by_blobs.frame_lengths(), [BLOBS_A_FRAME_AT_MOST, 1]);
    }

    #[test]
    fn a_snapshot_of_several_frames_is_staged_once_the_server_takes_it_and_named_through_its_last()
    {
        let mut in_parts = InParts::default();
        let pieces = pieces_of((0..=BLOBS_A_FRAME_AT_MOST).map(|_| vec![4; 40]));

        let asked = in_parts.asked(700, 600, pieces);
        let sent = in_parts.taken();
        let again = in_parts.taken();
        let stale_after = in_parts.refused_as_stale();

        assert_eq!(asked, ToServer::Stage);
        assert_eq!(
            sent,
            [
                ToServer::Staged {
                    parts: vec![vec![4; 40]; BLOBS_A_FRAME_AT_MOST]
                },
                ToServer::CompactStaged {
                    through: 700,
                    parts: vec![vec![4; 40]]
                },
            ]
        );
        assert!(again.is_empty());
        assert_eq!(stale_after, None);
        assert!(!in_parts.refused);
    }

    #[test]
    fn a_server_refusing_to_stage_as_stale_takes_no_parts_and_the_state_waits_until_the_records_double()
     {
        let mut in_parts = InParts::default();
        let pieces = pieces_of((0..=BLOBS_A_FRAME_AT_MOST).map(|_| vec![4; 40]));

        in_parts.asked(700, 600, pieces);

        let refused = in_parts.refused_as_stale();
        let sent = in_parts.taken();

        assert_eq!(refused, Some(Oversized::found_at(600)));
        assert!(in_parts.refused);
        assert!(sent.is_empty());
    }

    #[test]
    fn a_compaction_told_while_this_device_waits_on_its_own_is_another_devices_unless_it_is_the_one_asked()
     {
        let mut asked = Some(40);

        assert!(compacted_by_another(&mut asked, 35));
        assert_eq!(asked, Some(40));
        assert!(!compacted_by_another(&mut asked, 40));
        assert_eq!(asked, None);
        assert!(compacted_by_another(&mut asked, 50));
    }

    #[test]
    fn of_two_connections_between_two_devices_both_keep_the_one_the_lower_dialled() {
        let lower = DeviceId([1; 16]);
        let higher = DeviceId([2; 16]);

        assert!(the_connection_kept(lower, higher, true));
        assert!(!the_connection_kept(lower, higher, false));
        assert!(the_connection_kept(higher, lower, false));
        assert!(!the_connection_kept(higher, lower, true));
    }

    #[test]
    fn a_device_meeting_again_long_after_its_last_connection_is_kept_whoever_dialled() {
        let lower = DeviceId([1; 16]);
        let higher = DeviceId([2; 16]);
        let crossing = DIALS_CROSS_WITHIN / 2;

        assert!(!the_new_connection_kept(lower, higher, false, crossing));
        assert!(!the_new_connection_kept(higher, lower, true, crossing));
        assert!(the_new_connection_kept(
            lower,
            higher,
            false,
            DIALS_CROSS_WITHIN
        ));
        assert!(the_new_connection_kept(
            higher,
            lower,
            true,
            DIALS_CROSS_WITHIN
        ));
        assert!(the_new_connection_kept(lower, higher, true, Duration::ZERO));
    }

    #[test]
    fn a_partial_untouched_for_long_is_swept_and_a_fresh_one_or_another_file_stays() {
        let staging = env::temp_dir().join(format!("resonate-staging-{}", process::id()));

        std::fs::create_dir_all(&staging).expect("a staging folder");

        let old = SystemTime::now() - A_PARTIAL_IS_SWEPT_AFTER - Duration::from_secs(60);

        for name in ["old.part", "old.from", "old.flac"] {
            let file = File::create(staging.join(name)).expect("a file");

            file.set_modified(old).expect("aged");
        }
        File::create(staging.join("fresh.part")).expect("a file");

        swept(&staging, A_PARTIAL_IS_SWEPT_AFTER);

        assert!(!staging.join("old.part").exists());
        assert!(!staging.join("old.from").exists());
        assert!(staging.join("old.flac").exists());
        assert!(staging.join("fresh.part").exists());

        let _ = std::fs::remove_dir_all(&staging);
    }

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

    fn quic_server_certified_as(name: &str) -> (Endpoint, RootCertStore) {
        let signing = rcgen::KeyPair::generate().expect("a key");
        let certificate = rcgen::CertificateParams::new(vec![name.to_owned()])
            .and_then(|params| params.self_signed(&signing))
            .expect("a certificate");
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing.serialize_der()));
        let mut roots = RootCertStore::empty();

        roots.add(certificate.der().clone()).expect("trusted");

        let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_protocol_versions(&[&TLS13])
            .expect("TLS 1.3")
            .with_no_client_auth()
            .with_single_cert(vec![certificate.der().clone()], key)
            .expect("a server configuration");

        tls.alpn_protocols = vec![SERVER_ALPN.to_vec()];

        let crypto = QuicServerConfig::try_from(tls).expect("QUIC crypto");
        let endpoint = Endpoint::server(
            ServerConfig::with_crypto(Arc::new(crypto)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        )
        .expect("listening");
        (endpoint, roots)
    }

    fn accepting(server: Endpoint) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Some(incoming) = server.accept().await {
                if let Ok(connection) = incoming.await {
                    held.push(connection);
                }
            }
        })
    }

    #[tokio::test]
    async fn quic_goes_on_to_the_next_route_when_the_first_never_answers_and_still_checks_the_name()
    {
        let (server, roots) = quic_server_certified_as("localhost");
        let working = server.local_addr().expect("an address");
        let accepted = accepting(server);
        let never_answering = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bound");
        let stalled = never_answering.local_addr().expect("an address");
        let client =
            Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("a client");
        let to_server = to_the_server(roots).expect("a configuration");

        let reached = timeout(
            NEXT_ROUTE_AFTER + A_ROUTE_SETTLES_WITHIN,
            dialled_through(
                &client,
                &to_server,
                "localhost",
                &[stalled, working],
                "localhost",
            ),
        )
        .await;
        let (connection, route) = reached.expect("in time").expect("connected");

        assert_eq!(route, working);
        assert_eq!(connection.remote_address(), working);

        let named_otherwise = timeout(
            A_ROUTE_SETTLES_WITHIN,
            dialled_through(
                &client,
                &to_server,
                "elsewhere.example",
                &[working],
                "elsewhere.example",
            ),
        )
        .await;

        assert!(matches!(
            named_otherwise.expect("in time"),
            Err(Error::Connection(_))
        ));

        accepted.abort();
    }

    #[test]
    fn a_server_is_dialled_over_ipv4_first_and_ipv6_after_each_in_the_order_resolved() {
        let v6 =
            |last: u16| SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, last), 443));
        let v4 = |last: u8| SocketAddr::from(([192, 0, 2, last], 443));

        assert_eq!(
            ipv4_first(vec![v6(1), v4(1), v6(2), v4(2)]),
            [v4(1), v4(2), v6(1), v6(2)]
        );
    }

    #[test]
    fn a_link_whose_catalog_refused_before_the_cursor_moved_waits_longer_each_time_and_progress_waits_the_first_again()
     {
        let mut backoff = Backoff::first();
        let refused = || Parting::CatalogRefused(Refused::Unavailable);
        let mut waits = Vec::new();

        for _ in 0..3 {
            if moved_on(false, 7, Some(7), &refused()) {
                backoff.reset();
            }
            waits.push(backoff.next());
        }

        let caught_up_then_refused = moved_on(true, 7, Some(7), &refused());
        let caught_up_then_dropped = moved_on(true, 7, Some(7), &Parting::Ended);
        let moved_then_refused = moved_on(false, 7, Some(9), &refused());
        let unread = moved_on(false, 7, None, &Parting::Failed(Error::ServerSilent));

        assert_eq!(waits, [FIRST_RETRY, FIRST_RETRY * 2, FIRST_RETRY * 4]);
        assert!(!caught_up_then_refused);
        assert!(caught_up_then_dropped);
        assert!(moved_then_refused);
        assert!(!unread);

        backoff.reset();

        assert_eq!(backoff.next(), FIRST_RETRY);

        backoff.longest();

        assert_eq!(backoff.next(), LAST_RETRY);
        assert_eq!(backoff.next(), LAST_RETRY);
    }

    #[test]
    fn a_catalog_refusal_is_told_apart_from_a_connection_failure() {
        assert!(matches!(
            Parting::of(Error::Catalog(Refused::Unwritable)),
            Parting::CatalogRefused(Refused::Unwritable)
        ));
        assert!(matches!(
            Parting::of(Error::ServerSilent),
            Parting::Failed(Error::ServerSilent)
        ));
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
