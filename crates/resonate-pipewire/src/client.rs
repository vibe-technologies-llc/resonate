use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    io, iter, mem,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded};
use libspa::{
    param::{
        ParamInfoFlags, ParamType,
        audio::{AudioInfoRaw, MAX_CHANNELS},
    },
    pod::{Object, Pod, Value, deserialize::PodDeserializer, serialize::PodSerializer},
    sys,
    utils::Direction,
};
use parking_lot::Mutex;
use pipewire::{
    channel::Sender as LoopSender,
    context::ContextRc,
    core::CoreRc,
    device::{Device, DeviceChangeMask, DeviceListener},
    keys,
    main_loop::MainLoopRc,
    metadata::{Metadata, MetadataListener},
    node::{Node, NodeChangeMask, NodeListener},
    registry::{GlobalObject, RegistryRc},
    stream::{StreamFlags, StreamListener, StreamRc},
    types::ObjectType,
};
use resonate_core::{Gain, SampleRate, StreamSpec};

use crate::{
    AudioSink, AudioSource, CaptureRequest, CaptureStream, Capturing, CardProfile, Error,
    LatencyRequest, MediaRole, Microphone, NodeName, PodParam, PwOp, Reshaped, Result, SinkChange,
    SinkFormats, SinkId, SinkInfo, SinkPort, SinkStream, StreamClock, StreamCommand, StreamEvent,
    StreamRequest, StreamState, Unreshapable,
    format::{
        AdvertisedFormat, AdvertisedRoute, Negotiated, RouteChange, RouteSetting, WireWord,
        negotiated, packs_narrower, parse_allowed_rates, parse_default_sink, parse_enum_format,
        parse_profile, parse_rate, parse_route, route_change, running_rate, spa_format,
        spa_position,
    },
    process::{Cycle, Fed, Feed, Handing, Hearing, Taking, fed_from},
    props::StreamLevels,
};

type Pending = Rc<RefCell<Vec<(i32, Sender<()>)>>>;
type Nodes = Rc<RefCell<BTreeMap<u32, (Node, NodeListener)>>>;
type Devices = Rc<RefCell<BTreeMap<u32, (Device, DeviceListener)>>>;
type Metadatas = Rc<RefCell<BTreeMap<u32, (Metadata, MetadataListener, HeldIn)>>>;
type HeardStream = Opened<Box<dyn AudioSink>>;

struct ActiveStream {
    opened: Opened<Fed>,
    handing: Handing,
    asked: Rc<Cell<StreamSpec>>,
    settled: Rc<Cell<Option<Negotiated>>>,
    taking: Arc<Taking>,
    clock: Arc<StreamClock>,
    request: StreamRequest,
    drain_asked: bool,
    drained: Rc<Cell<bool>>,
}

impl ActiveStream {
    fn stream(&self) -> &StreamRc {
        &self.opened.stream
    }

    fn refuses(&self, request: &StreamRequest) -> Option<Unreshapable> {
        let open = &self.request;
        let pins_the_rate = open.force_graph_rate || request.force_graph_rate;
        if request.target != open.target {
            Some(Unreshapable::AnotherTarget)
        } else if request.realtime != open.realtime || request.latency != open.latency {
            Some(Unreshapable::AnotherPriority)
        } else if request.force_graph_rate != open.force_graph_rate
            || (pins_the_rate && request.spec.rate != open.spec.rate)
        {
            Some(Unreshapable::PinnedRateMoved)
        } else if !self.handing.has_room() {
            Some(Unreshapable::StillHandingOver)
        } else {
            None
        }
    }

    fn reshape(&mut self, request: &StreamRequest, source: Box<dyn AudioSource>) -> Reshaped {
        self.handing.let_go_of_the_spent();
        if let Some(because) = self.refuses(request) {
            return Reshaped::Refused { source, because };
        }
        if self.drain_asked {
            if !self.drained.get() {
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::StillDraining,
                };
            }
            if let Err(error) = self.opened.stream.set_active(true) {
                tracing::debug!(%error, "the graph would not stop the stream's drain");
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::DrainUnstopped,
                };
            }
            self.drain_asked = false;
            self.drained.set(false);
        }
        let asked = request.spec;
        let was = self.asked.get();
        if asked != was {
            let Ok(formats) = enum_formats(asked) else {
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::FormatUnwritten,
                };
            };
            let Some(mut params) = formats
                .iter()
                .map(|bytes| Pod::from_bytes(bytes))
                .collect::<Option<Vec<_>>>()
            else {
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::FormatUnwritten,
                };
            };
            let settled = self.settled.get().unwrap_or(Negotiated {
                spec: was,
                word: WireWord::Padded,
            });
            self.taking.store(settled.taken_against(asked));
            self.asked.set(asked);
            if let Err(error) = self.opened.stream.update_params(&mut params) {
                tracing::debug!(%error, "the graph would not take the stream's new formats");
                self.taking.store(settled.taken_against(was));
                self.asked.set(was);
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::FormatUnwritten,
                };
            }
        }

        let cycle = Cycle::new(asked, Arc::clone(&self.taking), Arc::clone(&self.clock));
        if let Err(feed) = self.handing.hand(Feed::new(cycle, source)) {
            return Reshaped::Refused {
                source: feed.into_source(),
                because: Unreshapable::StillHandingOver,
            };
        }
        self.request = request.clone();
        Reshaped::Kept
    }
}

struct ReshapeRequest {
    request: StreamRequest,
    source: Box<dyn AudioSource>,
    reply: Sender<Reshaped>,
}

struct Opened<D> {
    stream: StreamRc,
    listener: Option<StreamListener<D>>,
}

impl<D> Opened<D> {
    fn new(stream: StreamRc, listener: StreamListener<D>) -> Self {
        Self {
            stream,
            listener: Some(listener),
        }
    }
}

impl<D> Drop for Opened<D> {
    fn drop(&mut self) {
        drop(self.listener.take());
    }
}

const CORE_ID: u32 = 0;
const RECONNECT_EVERY: Duration = Duration::from_secs(1);
const STREAM_OPENED_WITHIN: Duration = Duration::from_secs(5);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);
const DAEMON_ANSWERS_WITHIN: Duration = Duration::from_secs(8);
const METADATA_NAME: &str = "metadata.name";
const ALLOWED_RATES: &str = "clock.allowed-rates";
const CLOCK_RATE: &str = "clock.rate";
const FORCED_RATE: &str = "clock.force-rate";
const DEFAULT_SINK: &str = "default.audio.sink";
const DEFAULT_SOURCE: &str = "default.audio.source";
const MICROPHONE_CLASS: &str = "Audio/Source";
const CARD_PROFILE_DEVICE: &str = "card.profile.device";
const NODE_DONT_MOVE: &str = "node.dont-move";

struct StreamSlots {
    clock: Arc<StreamClock>,
    events: Sender<StreamEvent>,
}

struct CaptureOpen {
    request: CaptureRequest,
    sink: Box<dyn AudioSink>,
    events: Sender<StreamEvent>,
    reply: Sender<Result<()>>,
}

struct OpenRequest {
    request: StreamRequest,
    target: Option<NodeName>,
    source: Box<dyn AudioSource>,
    slots: StreamSlots,
    reply: Sender<Result<()>>,
    awaiting: Awaiting,
}

#[derive(Clone, Default)]
struct Awaiting(Arc<AtomicU8>);

impl Awaiting {
    const WAITED_ON: u8 = 0;
    const KEPT: u8 = 1;
    const GIVEN_UP: u8 = 2;

    fn kept(&self) -> bool {
        self.settle(Self::KEPT)
    }

    fn given_up(&self) -> bool {
        self.settle(Self::GIVEN_UP)
    }

    fn is_given_up(&self) -> bool {
        self.0.load(Ordering::Acquire) == Self::GIVEN_UP
    }

    fn settle(&self, into: u8) -> bool {
        self.0
            .compare_exchange(Self::WAITED_ON, into, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

enum Request {
    Sync(Sender<()>),
    Open(Box<OpenRequest>),
    Reshape(Box<ReshapeRequest>),
    Capture(Box<CaptureOpen>),
    StopCapture,
    SetActive(bool),
    Turn { sink: SinkId, setting: RouteSetting },
    Drain,
    Close,
    Heartbeat,
    Lost,
    Reconnect,
    Shutdown,
}

#[derive(Default)]
struct DevicePorts {
    current: BTreeMap<u32, AdvertisedRoute>,
    offered: BTreeMap<u32, AdvertisedRoute>,
}

impl DevicePorts {
    fn keep(&mut self, held: Held, index: u32, route: AdvertisedRoute) -> Option<Vec<i32>> {
        let kept = match held {
            Held::Current => &mut self.current,
            Held::Offered => &mut self.offered,
        };
        let seats = route.seats.clone();
        let heard = (route.port.volume, route.port.muted);
        let was = kept.insert(index, route);
        let turned = matches!(held, Held::Current)
            && was.is_some_and(|was| (was.port.volume, was.port.muted) != heard);
        turned.then_some(seats)
    }

    fn turning(&self, seat: i32) -> Option<&AdvertisedRoute> {
        self.current
            .values()
            .find(|route| route.seats.contains(&seat))
    }

    fn serving(&self, seat: i32) -> Option<SinkPort> {
        Self::found(&self.current, seat).or_else(|| Self::found(&self.offered, seat))
    }

    fn found(routes: &BTreeMap<u32, AdvertisedRoute>, seat: i32) -> Option<SinkPort> {
        routes
            .values()
            .find(|route| route.seats.contains(&seat))
            .map(|route| route.port.clone())
    }
}

#[derive(Clone, Copy)]
enum Held {
    Current,
    Offered,
}

impl Held {
    const fn of(param: ParamType) -> Option<Self> {
        match param {
            ParamType::Route => Some(Self::Current),
            ParamType::EnumRoute => Some(Self::Offered),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct SinkRecord {
    name: String,
    description: String,
    names_an_api: bool,
    device: Option<u32>,
    seat: Option<i32>,
    formats: BTreeMap<u32, Vec<SinkFormats>>,
    format_serial: Option<bool>,
    running_at: Option<SampleRate>,
    running_serial: Option<bool>,
}

impl SinkRecord {
    fn formats_moved(&mut self, serial: bool) -> bool {
        let was = self.format_serial.replace(serial);
        if was.is_none_or(|was| was == serial) {
            return false;
        }
        self.formats.clear();
        true
    }

    fn running_moved(&mut self, serial: bool) -> bool {
        let was = self.running_serial.replace(serial);
        if was.is_none_or(|was| was == serial) {
            return false;
        }
        self.running_at = None;
        true
    }

    fn advertised(&self) -> Vec<SinkFormats> {
        self.formats.values().flatten().cloned().collect()
    }
}

#[derive(Clone, Debug, Default)]
struct MicrophoneRecord {
    name: String,
    description: String,
}

#[derive(Default)]
struct Discovered {
    sinks: BTreeMap<u32, SinkRecord>,
    microphones: BTreeMap<u32, MicrophoneRecord>,
    default_source: Option<String>,
    driven: BTreeSet<u32>,
    ports: BTreeMap<u32, DevicePorts>,
    profiles: BTreeMap<u32, CardProfile>,
    allowed_rates: Vec<SampleRate>,
    clock_rate: Option<SampleRate>,
    forced_rate: Option<SampleRate>,
    default_sink: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeldIn {
    Settings,
    Defaults,
}

impl HeldIn {
    const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Settings => &[ALLOWED_RATES, CLOCK_RATE, FORCED_RATE],
            Self::Defaults => &[DEFAULT_SINK, DEFAULT_SOURCE],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefaultSink {
    Stayed,
    Moved,
}

impl Discovered {
    fn heard(
        &mut self,
        held: HeldIn,
        subject: u32,
        key: Option<&str>,
        value: Option<&str>,
    ) -> DefaultSink {
        if subject != CORE_ID {
            return DefaultSink::Stayed;
        }
        let Some(key) = key else {
            let mut moved = DefaultSink::Stayed;
            for key in held.keys() {
                if self.heard_key(key, None) == DefaultSink::Moved {
                    moved = DefaultSink::Moved;
                }
            }
            return moved;
        };
        self.heard_key(key, value)
    }

    fn heard_key(&mut self, key: &str, value: Option<&str>) -> DefaultSink {
        match key {
            ALLOWED_RATES => {
                self.allowed_rates = value.map(parse_allowed_rates).unwrap_or_default()
            }
            CLOCK_RATE => self.clock_rate = value.and_then(parse_rate),
            FORCED_RATE => self.forced_rate = value.and_then(parse_rate),
            DEFAULT_SOURCE => self.default_source = value.and_then(parse_default_sink),
            DEFAULT_SINK => {
                let named = value.and_then(parse_default_sink);
                if named == self.default_sink {
                    return DefaultSink::Stayed;
                }
                self.default_sink = named;
                return DefaultSink::Moved;
            }
            _ => {}
        }
        DefaultSink::Stayed
    }

    fn snapshot(&self) -> Vec<SinkInfo> {
        self.sinks
            .iter()
            .map(|(id, record)| {
                let formats = record.advertised();
                let mut advertised: Vec<SampleRate> = formats
                    .iter()
                    .flat_map(|entry| entry.rates.iter().copied())
                    .collect();
                advertised.sort_unstable();
                advertised.dedup();
                let allowed = if self.allowed_rates.is_empty() {
                    advertised
                } else {
                    self.allowed_rates.clone()
                };
                SinkInfo {
                    id: SinkId::new(*id),
                    name: NodeName::new(record.name.clone()),
                    description: record.description.clone(),
                    is_default: self.default_sink.as_deref() == Some(record.name.as_str()),
                    is_hardware: self.is_hardware(record),
                    port: self.port_of(record),
                    profile: self.profile_of(record),
                    formats,
                    allowed_rates: allowed,
                    current_rate: record.running_at.or(self.forced_rate).or(self.clock_rate),
                }
            })
            .collect()
    }

    fn microphones(&self) -> Vec<Microphone> {
        self.microphones
            .values()
            .map(|record| Microphone {
                name: NodeName::new(record.name.clone()),
                description: record.description.clone(),
                is_default: self.default_source.as_deref() == Some(record.name.as_str()),
            })
            .collect()
    }

    fn is_hardware(&self, record: &SinkRecord) -> bool {
        record.names_an_api
            || record
                .device
                .is_some_and(|above| self.driven.contains(&above))
    }

    fn port_of(&self, record: &SinkRecord) -> Option<SinkPort> {
        let above = record.device?;
        let seat = record.seat?;
        self.ports.get(&above)?.serving(seat)
    }

    fn profile_of(&self, record: &SinkRecord) -> Option<CardProfile> {
        self.profiles.get(&record.device?).cloned()
    }

    fn route_turning(&self, sink: SinkId, setting: RouteSetting) -> Option<(u32, RouteChange)> {
        let record = self.sinks.get(&sink.get())?;
        let above = record.device?;
        let seat = record.seat?;
        let route = self.ports.get(&above)?.turning(seat)?;
        let turned = RouteChange {
            index: route.index?,
            seat,
            channel_volumes: route.channel_volumes.clone(),
            setting,
        };
        turned.can_be_made().then_some((above, turned))
    }

    fn sinks_on(&self, device: u32) -> Vec<SinkId> {
        self.sinks
            .iter()
            .filter(|(_, record)| record.device == Some(device))
            .map(|(id, _)| SinkId::new(*id))
            .collect()
    }

    fn sinks_seated_on(&self, device: u32, seats: &[i32]) -> Vec<SinkId> {
        self.sinks
            .iter()
            .filter(|(_, record)| {
                record.device == Some(device)
                    && record.seat.is_some_and(|seat| seats.contains(&seat))
            })
            .map(|(id, _)| SinkId::new(*id))
            .collect()
    }
}

pub struct PipeWire {
    survey: Survey,
    thread: Option<JoinHandle<()>>,
    changes: Receiver<SinkChange>,
}

#[derive(Clone)]
pub struct Survey {
    commands: LoopSender<Request>,
    shared: Arc<Mutex<Discovered>>,
    connected: Arc<AtomicBool>,
}

impl Survey {
    fn roundtrip(&self, timeout: Duration) -> Result<()> {
        let (reply, done) = bounded(1);
        self.commands
            .send(Request::Sync(reply))
            .map_err(|_| Error::LoopStopped)?;
        done.recv_timeout(timeout).map_err(|_| self.unanswered())
    }

    fn node_name(&self, node: SinkId) -> Result<NodeName> {
        self.shared
            .lock()
            .sinks
            .get(&node.get())
            .map(|record| NodeName::new(record.name.clone()))
            .ok_or(Error::SinkGone { node })
    }

    pub fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> Result<SinkStream> {
        let target = request
            .target
            .map(|node| self.node_name(node))
            .transpose()?;
        let (events, incoming) = bounded(256);
        let clock = Arc::new(StreamClock::default());
        let (reply, outcome) = bounded(1);
        let awaiting = Awaiting::default();

        self.commands
            .send(Request::Open(Box::new(OpenRequest {
                request: request.clone(),
                target,
                source,
                slots: StreamSlots {
                    clock: Arc::clone(&clock),
                    events,
                },
                reply,
                awaiting: awaiting.clone(),
            })))
            .map_err(|_| Error::LoopStopped)?;

        match outcome.recv_timeout(STREAM_OPENED_WITHIN) {
            Ok(opened) => opened?,
            Err(RecvTimeoutError::Disconnected) => return Err(Error::LoopStopped),
            Err(RecvTimeoutError::Timeout) if awaiting.given_up() => {
                return Err(self.unanswered_open());
            }
            Err(RecvTimeoutError::Timeout) => outcome.recv().map_err(|_| Error::LoopStopped)??,
        }

        let control = self.commands.clone();
        let reshaping = self.commands.clone();
        let stream = SinkStream::new(
            incoming,
            clock,
            Box::new(move |command| {
                control
                    .send(match command {
                        StreamCommand::SetActive(active) => Request::SetActive(active),
                        StreamCommand::Drain => Request::Drain,
                        StreamCommand::Close => Request::Close,
                    })
                    .map_err(|_| resonate_audio::Error::LoopStopped)
            }),
        );
        Ok(stream.reshaped_by(Box::new(move |request, source| {
            reshape_on_the_loop(&reshaping, request, source)
        })))
    }

    fn unanswered(&self) -> Error {
        if self.connected.load(Ordering::Acquire) {
            Error::LoopStopped
        } else {
            Error::Disconnected
        }
    }

    fn unanswered_open(&self) -> Error {
        if self.connected.load(Ordering::Acquire) {
            Error::OpenUnanswered
        } else {
            Error::Disconnected
        }
    }

    fn settled(&self, timeout: Duration) -> Result<()> {
        let half = timeout / 2;
        self.roundtrip(half)?;
        self.roundtrip(half)
    }

    pub fn enumerate_sinks(&self, timeout: Duration) -> Result<Vec<SinkInfo>> {
        self.settled(timeout)?;
        let sinks = self.shared.lock().snapshot();
        if sinks.is_empty() {
            return Err(Error::NoSink);
        }
        Ok(sinks)
    }
}

impl PipeWire {
    pub fn start(app_name: &str) -> Result<Self> {
        let (commands, requests) = pipewire::channel::channel();
        let (ready, started) = bounded(1);
        let (announce, changes) = bounded(64);
        let shared = Arc::new(Mutex::new(Discovered::default()));
        let connected = Arc::new(AtomicBool::new(false));

        let thread = {
            let app_name = app_name.to_owned();
            let shared = Arc::clone(&shared);
            let connected = Arc::clone(&connected);
            let commands = commands.clone();
            thread::Builder::new()
                .name("resonate-pipewire".to_owned())
                .spawn(move || {
                    run(
                        &app_name, &shared, &connected, requests, &commands, &ready, &announce,
                    );
                })
                .map_err(|_| Error::LoopStopped)?
        };

        match started.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                survey: Survey {
                    commands,
                    shared,
                    connected,
                },
                thread: Some(thread),
                changes,
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(Error::LoopStopped),
        }
    }

    pub fn survey(&self) -> Survey {
        self.survey.clone()
    }

    pub fn enumerate_sinks(&self, timeout: Duration) -> Result<Vec<SinkInfo>> {
        self.survey.enumerate_sinks(timeout)
    }

    pub fn default_sink(&self, timeout: Duration) -> Result<Option<SinkInfo>> {
        let sinks = self.enumerate_sinks(timeout)?;
        Ok(sinks
            .iter()
            .find(|sink| sink.is_default)
            .or_else(|| sinks.first())
            .cloned())
    }

    pub fn microphones(&self, timeout: Duration) -> Result<Vec<Microphone>> {
        self.survey.settled(timeout)?;
        Ok(self.survey.shared.lock().microphones())
    }

    pub fn capture(
        &self,
        request: &CaptureRequest,
        sink: Box<dyn AudioSink>,
    ) -> Result<CaptureStream> {
        let (events, incoming) = bounded(64);
        let (reply, outcome) = bounded(1);
        self.survey
            .commands
            .send(Request::Capture(Box::new(CaptureOpen {
                request: request.clone(),
                sink,
                events,
                reply,
            })))
            .map_err(|_| Error::LoopStopped)?;
        outcome
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::LoopStopped)??;

        let control = self.survey.commands.clone();
        Ok(CaptureStream::new(
            incoming,
            Box::new(move || {
                control
                    .send(Request::StopCapture)
                    .map_err(|_| Error::LoopStopped)
            }),
        ))
    }

    pub fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    pub fn set_device_volume(&self, sink: SinkId, gain: Gain) -> Result<()> {
        self.turn(sink, RouteSetting::Volume(gain))
    }

    pub fn set_device_mute(&self, sink: SinkId, muted: bool) -> Result<()> {
        self.turn(sink, RouteSetting::Mute(muted))
    }

    fn turn(&self, sink: SinkId, setting: RouteSetting) -> Result<()> {
        self.survey
            .commands
            .send(Request::Turn { sink, setting })
            .map_err(|_| Error::LoopStopped)
    }

    pub fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> Result<SinkStream> {
        self.survey.open(request, source)
    }

    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.survey.commands.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}

impl Drop for PipeWire {
    fn drop(&mut self) {
        let _ = self.survey.commands.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn reshape_on_the_loop(
    commands: &LoopSender<Request>,
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
) -> Reshaped {
    let (reply, answer) = bounded(1);
    let asked = Request::Reshape(Box::new(ReshapeRequest {
        request: request.clone(),
        source,
        reply,
    }));
    if let Err(unsent) = commands.send(asked) {
        let source = match unsent {
            Request::Reshape(unsent) => unsent.source,
            _ => return Reshaped::Lost(resonate_audio::Error::LoopStopped),
        };
        return Reshaped::Refused {
            source,
            because: Unreshapable::NoStream,
        };
    }
    answer
        .recv_timeout(STREAM_OPENED_WITHIN)
        .unwrap_or(Reshaped::Lost(resonate_audio::Error::LoopStopped))
}

fn node_property(
    global: &GlobalObject<&libspa::utils::dict::DictRef>,
    key: &str,
) -> Option<String> {
    global
        .props
        .and_then(|props| props.get(key))
        .map(ToOwned::to_owned)
}

fn run(
    app_name: &str,
    shared: &Arc<Mutex<Discovered>>,
    connected: &Arc<AtomicBool>,
    requests: pipewire::channel::Receiver<Request>,
    commands: &LoopSender<Request>,
    ready: &Sender<Result<()>>,
    announce: &Sender<SinkChange>,
) {
    pipewire::init();

    macro_rules! stage {
        ($expression:expr, $op:expr) => {
            match $expression {
                Ok(value) => value,
                Err(error) => {
                    let _ = ready.send(Err(Error::daemon($op, error)));
                    return;
                }
            }
        };
    }

    let mainloop = stage!(MainLoopRc::new(None), PwOp::MainLoopCreate);
    let context = stage!(ContextRc::new(&mainloop, None), PwOp::ContextCreate);
    let pending: Pending = Rc::new(RefCell::new(Vec::new()));
    let reaching = Reaching {
        app_name: app_name.to_owned(),
        context,
        shared: Arc::clone(shared),
        pending: Rc::clone(&pending),
        commands: commands.clone(),
        announce: announce.clone(),
    };
    let graph: Rc<RefCell<Option<Graph>>> = match reaching.connect() {
        Ok(graph) => {
            connected.store(true, Ordering::Release);
            Rc::new(RefCell::new(Some(graph)))
        }
        Err(error) => {
            tracing::warn!(%error, "no PipeWire daemon to reach yet; connecting once it is there");
            reaching.ask_again_later();
            Rc::new(RefCell::new(None))
        }
    };

    let active: Rc<RefCell<Option<ActiveStream>>> = Rc::new(RefCell::new(None));
    let heard: Rc<RefCell<Option<HeardStream>>> = Rc::new(RefCell::new(None));
    let sequence = Rc::new(RefCell::new(0_i32));
    let beat: Rc<RefCell<Option<Beat>>> = Rc::new(RefCell::new(None));
    let receiver = requests.attach(mainloop.loop_(), {
        let mainloop = mainloop.downgrade();
        let graph = Rc::clone(&graph);
        let pending = Rc::clone(&pending);
        let sequence = Rc::clone(&sequence);
        let active = Rc::clone(&active);
        let heard = Rc::clone(&heard);
        let beat = Rc::clone(&beat);
        let connected = Arc::clone(connected);
        move |request| match request {
            Request::Sync(reply) => {
                let Some(core) = graph.borrow().as_ref().map(|held| held.core.clone()) else {
                    return;
                };
                let mut sequence = sequence.borrow_mut();
                *sequence = sequence.wrapping_add(1);
                match core.sync(*sequence) {
                    Ok(seq) => pending.borrow_mut().push((seq.seq(), reply)),
                    Err(_) => drop(reply),
                }
            }
            Request::Open(open) => {
                let Some(core) = graph.borrow().as_ref().map(|held| held.core.clone()) else {
                    let _ = open.reply.send(Err(Error::Disconnected));
                    return;
                };
                let outcome = open_stream(&core, *open, &active);
                if outcome.is_err() {
                    active.borrow_mut().take();
                }
            }
            Request::Reshape(reshape) => {
                let ReshapeRequest {
                    request,
                    source,
                    reply,
                } = *reshape;
                let reshaped = match active.borrow_mut().as_mut() {
                    Some(playing) => playing.reshape(&request, source),
                    None => Reshaped::Refused {
                        source,
                        because: Unreshapable::NoStream,
                    },
                };
                let _ = reply.send(reshaped);
            }
            Request::Capture(open) => {
                let Some(core) = graph.borrow().as_ref().map(|held| held.core.clone()) else {
                    let _ = open.reply.send(Err(Error::Disconnected));
                    return;
                };
                let CaptureOpen {
                    request,
                    sink,
                    events,
                    reply,
                } = *open;
                match build_capture_stream(&core, &request, sink, &events) {
                    Ok(stream) => {
                        heard.borrow_mut().replace(stream);
                        let _ = reply.send(Ok(()));
                    }
                    Err(error) => {
                        heard.borrow_mut().take();
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Request::StopCapture => {
                if let Some(Opened { stream, .. }) = heard.borrow().as_ref() {
                    let _ = stream.disconnect();
                }
                heard.borrow_mut().take();
            }
            Request::SetActive(wanted) => {
                if let Some(playing) = active.borrow().as_ref() {
                    let _ = playing.stream().set_active(wanted);
                }
            }
            Request::Turn { sink, setting } => {
                let turning = reaching.shared.lock().route_turning(sink, setting);
                let Some((device, turned)) = turning else {
                    tracing::warn!(%sink, "the device under this sink names no route whose volume could be turned");
                    return;
                };
                let held = graph.borrow();
                let Some(devices) = held.as_ref().map(|held| held.devices.borrow()) else {
                    return;
                };
                match devices.get(&device) {
                    Some((proxy, _)) => turn_the_route(proxy, &turned),
                    None => tracing::warn!(%sink, device, "the device under this sink has left the graph"),
                }
            }
            Request::Drain => {
                if let Some(playing) = active.borrow_mut().as_mut() {
                    let _ = playing.stream().flush(true);
                    playing.drain_asked = true;
                    playing.drained.set(false);
                }
            }
            Request::Close => {
                if let Some(playing) = active.borrow().as_ref() {
                    let _ = playing.stream().disconnect();
                }
                active.borrow_mut().take();
            }
            Request::Heartbeat => {
                if let Some(playing) = active.borrow_mut().as_mut() {
                    playing.handing.let_go_of_the_spent();
                }
                let Some(core) = graph.borrow().as_ref().map(|held| held.core.clone()) else {
                    beat.borrow_mut().take();
                    return;
                };
                let mut beating = beat.borrow_mut();
                match beating.as_ref().map(Beat::heard) {
                    Some(Heard::Waiting) => return,
                    Some(Heard::Never) => {
                        beating.take();
                        tracing::warn!(
                            within = ?DAEMON_ANSWERS_WITHIN,
                            "the PipeWire daemon stopped answering without closing the connection"
                        );
                        drop(beating);
                        lose_the_graph(&graph, &connected, &active, &heard, &reaching);
                        return;
                    }
                    Some(Heard::Answered) | None => {}
                }
                let (reply, answer) = bounded(1);
                let mut sequence = sequence.borrow_mut();
                *sequence = sequence.wrapping_add(1);
                if let Ok(seq) = core.sync(*sequence) {
                    pending.borrow_mut().push((seq.seq(), reply));
                    *beating = Some(Beat {
                        sent: Instant::now(),
                        answer,
                    });
                }
            }
            Request::Lost => {
                beat.borrow_mut().take();
                lose_the_graph(&graph, &connected, &active, &heard, &reaching);
            }
            Request::Reconnect => {
                if graph.borrow().is_some() {
                    return;
                }
                match reaching.connect() {
                    Ok(reached) => {
                        graph.borrow_mut().replace(reached);
                        connected.store(true, Ordering::Release);
                        tracing::info!("reconnected to the PipeWire daemon");
                    }
                    Err(error) => {
                        tracing::debug!(%error, "the PipeWire daemon is not back yet");
                        reaching.ask_again_later();
                    }
                }
            }
            Request::Shutdown => {
                active.borrow_mut().take();
                heard.borrow_mut().take();
                if let Some(mainloop) = mainloop.upgrade() {
                    mainloop.quit();
                }
            }
        }
    });

    let heartbeat = mainloop.loop_().add_timer({
        let commands = commands.clone();
        move |_| {
            let _ = commands.send(Request::Heartbeat);
        }
    });
    if let Err(error) = heartbeat
        .update_timer(Some(HEARTBEAT_EVERY), Some(HEARTBEAT_EVERY))
        .into_result()
    {
        tracing::warn!(%error, "no timer could be set to hear whether the PipeWire daemon still answers");
    }

    let _ = ready.send(Ok(()));
    mainloop.run();

    drop(receiver);
    active.borrow_mut().take();
    heard.borrow_mut().take();
    graph.borrow_mut().take();
    connected.store(false, Ordering::Release);
}

fn lose_the_graph(
    graph: &RefCell<Option<Graph>>,
    connected: &AtomicBool,
    active: &RefCell<Option<ActiveStream>>,
    heard: &RefCell<Option<HeardStream>>,
    reaching: &Reaching,
) {
    let Some(gone) = graph.borrow_mut().take() else {
        return;
    };
    tracing::warn!("the PipeWire daemon went away; reconnecting when it is back");
    connected.store(false, Ordering::Release);
    active.borrow_mut().take();
    heard.borrow_mut().take();
    reaching.pending.borrow_mut().clear();
    drop(gone);
    reaching.forget_the_graph();
    reaching.ask_again_later();
}

struct Beat {
    sent: Instant,
    answer: Receiver<()>,
}

enum Heard {
    Answered,
    Waiting,
    Never,
}

impl Beat {
    fn heard(&self) -> Heard {
        match self.answer.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => Heard::Answered,
            Err(TryRecvError::Empty) if self.sent.elapsed() < DAEMON_ANSWERS_WITHIN => {
                Heard::Waiting
            }
            Err(TryRecvError::Empty) => Heard::Never,
        }
    }
}

struct Graph {
    _core_listener: pipewire::core::Listener,
    _registry_listener: pipewire::registry::Listener,
    nodes: Nodes,
    devices: Devices,
    metadatas: Metadatas,
    _registry: RegistryRc,
    core: CoreRc,
}

impl Drop for Graph {
    fn drop(&mut self) {
        self.nodes.borrow_mut().clear();
        self.devices.borrow_mut().clear();
        self.metadatas.borrow_mut().clear();
    }
}

struct Reaching {
    app_name: String,
    context: ContextRc,
    shared: Arc<Mutex<Discovered>>,
    pending: Pending,
    commands: LoopSender<Request>,
    announce: Sender<SinkChange>,
}

impl Reaching {
    fn connect(&self) -> Result<Graph> {
        let properties = pipewire::properties::properties! {
            *keys::APP_NAME => self.app_name.as_str(),
            *keys::MEDIA_CATEGORY => "Playback",
        };
        let core = self
            .context
            .connect_rc(Some(properties))
            .map_err(|source| Error::daemon(PwOp::CoreConnect, source))?;
        let registry = core
            .get_registry_rc()
            .map_err(|source| Error::daemon(PwOp::RegistryBind, source))?;

        let nodes: Nodes = Rc::new(RefCell::new(BTreeMap::new()));
        let devices: Devices = Rc::new(RefCell::new(BTreeMap::new()));
        let metadatas: Metadatas = Rc::new(RefCell::new(BTreeMap::new()));

        let core_listener = core
            .add_listener_local()
            .done({
                let pending = Rc::clone(&self.pending);
                move |id, seq| {
                    if id != CORE_ID {
                        return;
                    }
                    pending.borrow_mut().retain(|(wanted, reply)| {
                        if *wanted == seq.seq() {
                            let _ = reply.send(());
                            false
                        } else {
                            true
                        }
                    });
                }
            })
            .error({
                let commands = self.commands.clone();
                move |id, _seq, res, message| {
                    tracing::debug!(id, res, message, "the PipeWire core reported an error");
                    if id == CORE_ID && is_a_broken_connection(res) {
                        let _ = commands.send(Request::Lost);
                    }
                }
            })
            .register();
        let registry_listener = watch_the_registry(
            &registry,
            &self.shared,
            &nodes,
            &devices,
            &metadatas,
            &self.announce,
        );

        Ok(Graph {
            _core_listener: core_listener,
            _registry_listener: registry_listener,
            nodes,
            devices,
            metadatas,
            _registry: registry,
            core,
        })
    }

    fn forget_the_graph(&self) {
        let forgotten = mem::take(&mut *self.shared.lock());
        for id in forgotten.sinks.keys() {
            let _ = self
                .announce
                .try_send(SinkChange::Removed(SinkId::new(*id)));
        }
    }

    fn ask_again_later(&self) {
        let commands = self.commands.clone();
        let spawned = thread::Builder::new()
            .name("resonate-pipewire-reconnect".to_owned())
            .spawn(move || {
                thread::sleep(RECONNECT_EVERY);
                let _ = commands.send(Request::Reconnect);
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "no thread could be started to reconnect to PipeWire from");
        }
    }
}

fn is_a_broken_connection(res: i32) -> bool {
    res.checked_neg().is_some_and(|errno| {
        matches!(
            io::Error::from_raw_os_error(errno).kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::NotConnected
        )
    })
}

fn watch_the_registry(
    registry: &RegistryRc,
    shared: &Arc<Mutex<Discovered>>,
    nodes: &Nodes,
    devices: &Devices,
    metadatas: &Metadatas,
    announce: &Sender<SinkChange>,
) -> pipewire::registry::Listener {
    registry
        .add_listener_local()
        .global({
            let shared = Arc::clone(shared);
            let nodes = Rc::clone(nodes);
            let devices = Rc::clone(devices);
            let metadatas = Rc::clone(metadatas);
            let registry = registry.clone();
            let announce = announce.clone();
            move |global| match global.type_ {
                ObjectType::Node => {
                    let class = node_property(global, *keys::MEDIA_CLASS);
                    if class.as_deref() == Some(MICROPHONE_CLASS) {
                        shared.lock().microphones.insert(
                            global.id,
                            MicrophoneRecord {
                                name: node_property(global, *keys::NODE_NAME).unwrap_or_default(),
                                description: node_property(global, *keys::NODE_DESCRIPTION)
                                    .unwrap_or_default(),
                            },
                        );
                        return;
                    }
                    if class.as_deref() != Some("Audio/Sink") {
                        return;
                    }
                    let record = SinkRecord {
                        name: node_property(global, *keys::NODE_NAME).unwrap_or_default(),
                        description: node_property(global, *keys::NODE_DESCRIPTION)
                            .unwrap_or_default(),
                        names_an_api: node_property(global, *keys::DEVICE_API).is_some(),
                        device: node_property(global, *keys::DEVICE_ID)
                            .and_then(|above| above.parse().ok()),
                        seat: None,
                        formats: BTreeMap::new(),
                        format_serial: None,
                        running_at: None,
                        running_serial: None,
                    };
                    shared.lock().sinks.insert(global.id, record);

                    let Ok(node) = registry.bind::<Node, _>(global) else {
                        return;
                    };
                    let listener = node
                        .add_listener_local()
                        .info({
                            let shared = Arc::clone(&shared);
                            let nodes = Rc::clone(&nodes);
                            let announce = announce.clone();
                            let id = global.id;
                            move |info| {
                                if info.change_mask().contains(NodeChangeMask::PARAMS) {
                                    let serial_of = |wanted: ParamType| {
                                        info.params().iter().find(|param| param.id() == wanted).map(
                                            |param| param.flags().contains(ParamInfoFlags::SERIAL),
                                        )
                                    };
                                    let (offered, running) = (
                                        serial_of(ParamType::EnumFormat),
                                        serial_of(ParamType::Format),
                                    );
                                    let (offered_moved, running_moved) = {
                                        let mut state = shared.lock();
                                        let record = state.sinks.get_mut(&id);
                                        record.map_or((false, false), |record| {
                                            (
                                                offered.is_some_and(|serial| {
                                                    record.formats_moved(serial)
                                                }),
                                                running.is_some_and(|serial| {
                                                    record.running_moved(serial)
                                                }),
                                            )
                                        })
                                    };
                                    let asked_again = [
                                        (offered_moved, ParamType::EnumFormat),
                                        (running_moved, ParamType::Format),
                                    ];
                                    for (moved, param) in asked_again {
                                        if !moved {
                                            continue;
                                        }
                                        if let Some((node, _)) = nodes.borrow().get(&id) {
                                            node.enum_params(0, Some(param), 0, u32::MAX);
                                        }
                                    }
                                    if offered_moved || running_moved {
                                        let _ = announce
                                            .try_send(SinkChange::Reformatted(SinkId::new(id)));
                                    }
                                }
                                if !info.change_mask().contains(NodeChangeMask::PROPS) {
                                    return;
                                }
                                let seat = info
                                    .props()
                                    .and_then(|props| props.get(CARD_PROFILE_DEVICE))
                                    .and_then(|seat| seat.parse().ok());
                                if let Some(record) = shared.lock().sinks.get_mut(&id) {
                                    record.seat = seat;
                                }
                            }
                        })
                        .param({
                            let shared = Arc::clone(&shared);
                            let id = global.id;
                            move |_seq, param_type, index, _next, param| {
                                let Some(param) = param else { return };
                                if param_type == ParamType::Format {
                                    if let Some(record) = shared.lock().sinks.get_mut(&id) {
                                        record.running_at = running_rate(param);
                                    }
                                    return;
                                }
                                if param_type != ParamType::EnumFormat {
                                    return;
                                }
                                let Ok((_, value)) =
                                    PodDeserializer::deserialize_any_from(param.as_bytes())
                                else {
                                    return;
                                };
                                let Some(advertised) = parse_enum_format(&value) else {
                                    return;
                                };
                                if advertised.formats.is_empty() || advertised.rates.is_empty() {
                                    return;
                                }
                                let AdvertisedFormat {
                                    formats,
                                    rates,
                                    layouts,
                                } = advertised;
                                let mut state = shared.lock();
                                if let Some(record) = state.sinks.get_mut(&id) {
                                    record.formats.insert(
                                        index,
                                        formats
                                            .into_iter()
                                            .map(|(format, words)| SinkFormats {
                                                format,
                                                words,
                                                rates: rates.clone(),
                                                channels: layouts.clone(),
                                            })
                                            .collect(),
                                    );
                                }
                            }
                        })
                        .register();
                    node.enum_params(0, Some(ParamType::EnumFormat), 0, u32::MAX);
                    node.enum_params(0, Some(ParamType::Format), 0, u32::MAX);
                    nodes.borrow_mut().insert(global.id, (node, listener));

                    let _ = announce.try_send(SinkChange::Added(SinkId::new(global.id)));
                }
                ObjectType::Device => {
                    if node_property(global, *keys::DEVICE_API).is_none() {
                        return;
                    }
                    shared.lock().driven.insert(global.id);

                    let Ok(device) = registry.bind::<Device, _>(global) else {
                        return;
                    };
                    let listener = device
                        .add_listener_local()
                        .info({
                            let devices = Rc::clone(&devices);
                            let id = global.id;
                            move |info| {
                                if !info.change_mask().contains(DeviceChangeMask::PARAMS) {
                                    return;
                                }
                                if let Some((device, _)) = devices.borrow().get(&id) {
                                    device.enum_params(0, Some(ParamType::Route), 0, u32::MAX);
                                }
                            }
                        })
                        .param({
                            let shared = Arc::clone(&shared);
                            let announce = announce.clone();
                            let id = global.id;
                            move |_seq, param_type, index, _next, param| {
                                let Some(param) = param else { return };
                                let Ok((_, value)) =
                                    PodDeserializer::deserialize_any_from(param.as_bytes())
                                else {
                                    return;
                                };
                                if param_type == ParamType::Profile {
                                    let Some(profile) = parse_profile(&value) else {
                                        return;
                                    };
                                    let mut state = shared.lock();
                                    let was = state.profiles.insert(id, profile);
                                    let moved = was.is_some_and(|was| {
                                        state.profiles.get(&id).map(|now| now.index)
                                            != Some(was.index)
                                    });
                                    if moved {
                                        for sink in state.sinks_on(id) {
                                            let _ = announce.try_send(SinkChange::Switched(sink));
                                        }
                                    }
                                    return;
                                }
                                let Some(held) = Held::of(param_type) else {
                                    return;
                                };
                                let Some(route) = parse_route(&value) else {
                                    return;
                                };
                                let mut state = shared.lock();
                                let turned =
                                    state.ports.entry(id).or_default().keep(held, index, route);
                                let Some(seats) = turned else {
                                    return;
                                };
                                for sink in state.sinks_seated_on(id, &seats) {
                                    let _ = announce.try_send(SinkChange::Turned(sink));
                                }
                            }
                        })
                        .register();
                    device.subscribe_params(&[
                        ParamType::Route,
                        ParamType::EnumRoute,
                        ParamType::Profile,
                    ]);
                    device.enum_params(0, Some(ParamType::Route), 0, u32::MAX);
                    device.enum_params(0, Some(ParamType::EnumRoute), 0, u32::MAX);
                    device.enum_params(0, Some(ParamType::Profile), 0, u32::MAX);
                    devices.borrow_mut().insert(global.id, (device, listener));
                }
                ObjectType::Metadata => {
                    let held = match node_property(global, METADATA_NAME).as_deref() {
                        Some("settings") => HeldIn::Settings,
                        Some("default") => HeldIn::Defaults,
                        _ => return,
                    };
                    let Ok(metadata) = registry.bind::<Metadata, _>(global) else {
                        return;
                    };
                    let listener = metadata
                        .add_listener_local()
                        .property({
                            let shared = Arc::clone(&shared);
                            let announce = announce.clone();
                            move |subject, key, _type, value| {
                                let moved = shared.lock().heard(held, subject, key, value);
                                if moved == DefaultSink::Moved {
                                    let _ = announce.try_send(SinkChange::DefaultChanged);
                                }
                                0
                            }
                        })
                        .register();
                    metadatas
                        .borrow_mut()
                        .insert(global.id, (metadata, listener, held));
                }
                _ => {}
            }
        })
        .global_remove({
            let shared = Arc::clone(shared);
            let nodes = Rc::clone(nodes);
            let devices = Rc::clone(devices);
            let metadatas = Rc::clone(metadatas);
            let announce = announce.clone();
            move |id| {
                let gone = metadatas.borrow_mut().remove(&id);
                if let Some((_, _, held)) = gone
                    && shared.lock().heard(held, CORE_ID, None, None) == DefaultSink::Moved
                {
                    let _ = announce.try_send(SinkChange::DefaultChanged);
                }
                let mut state = shared.lock();
                state.driven.remove(&id);
                state.microphones.remove(&id);
                state.profiles.remove(&id);
                state.ports.remove(&id);
                devices.borrow_mut().remove(&id);
                if state.sinks.remove(&id).is_some() {
                    drop(state);
                    nodes.borrow_mut().remove(&id);
                    let _ = announce.try_send(SinkChange::Removed(SinkId::new(id)));
                }
            }
        })
        .register()
}

fn turn_the_route(device: &Device, turned: &RouteChange) {
    let serialized =
        PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &route_change(turned));
    let bytes = match serialized {
        Ok((cursor, _)) => cursor.into_inner(),
        Err(error) => {
            tracing::warn!(%error, "the route's volume could not be written as a POD");
            return;
        }
    };
    let Some(pod) = Pod::from_bytes(&bytes) else {
        return;
    };
    device.set_param(ParamType::Route, 0, pod);
}

fn format_pod(spec: StreamSpec, word: WireWord, id: u32) -> Result<Vec<u8>> {
    let mut info = AudioInfoRaw::new();
    info.set_format(spa_format(spec.format, word));
    info.set_rate(spec.rate.hz());
    info.set_channels(u32::from(spec.channel_count().get()));

    let mut position = [0; MAX_CHANNELS];
    for (slot, channel) in position.iter_mut().zip(spa_position(spec.channels)) {
        *slot = channel;
    }
    info.set_position(position);

    pod_of(info, id)
}

fn pod_of(info: AudioInfoRaw, id: u32) -> Result<Vec<u8>> {
    let object = Value::Object(Object {
        type_: sys::SPA_TYPE_OBJECT_Format,
        id,
        properties: info.into(),
    });

    let bytes = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &object)
        .map_err(|source| Error::PodBuild {
            param: PodParam::EnumFormat,
            source,
        })?
        .0
        .into_inner();
    Ok(bytes)
}

fn open_stream(
    core: &pipewire::core::CoreRc,
    open: OpenRequest,
    active: &Rc<RefCell<Option<ActiveStream>>>,
) -> Result<()> {
    let OpenRequest {
        request,
        target,
        source,
        slots,
        reply,
        awaiting,
    } = open;
    if awaiting.is_given_up() {
        return Ok(());
    }

    let outcome = build_stream(core, &request, target.as_ref(), source, &slots);
    match outcome {
        Ok(stream) if awaiting.kept() => {
            active.borrow_mut().replace(stream);
            let _ = reply.send(Ok(()));
            Ok(())
        }
        Ok(abandoned) => {
            tracing::debug!("a stream opened after its asker gave up waiting is closed");
            let _ = abandoned.stream().disconnect();
            Ok(())
        }
        Err(error) => {
            let _ = reply.send(Err(error));
            Err(Error::LoopStopped)
        }
    }
}

fn build_stream(
    core: &pipewire::core::CoreRc,
    request: &StreamRequest,
    target: Option<&NodeName>,
    source: Box<dyn AudioSource>,
    slots: &StreamSlots,
) -> Result<ActiveStream> {
    let StreamSlots { clock, events } = slots;
    let spec = request.spec;
    let role = match request.role {
        MediaRole::Music => "Music",
        MediaRole::Notification => "Notification",
    };

    let mut properties = pipewire::properties::properties! {
        *keys::MEDIA_TYPE => "Audio",
        *keys::MEDIA_CATEGORY => "Playback",
        *keys::MEDIA_ROLE => role,
        *keys::MEDIA_NAME => request.media_name.as_str(),
        *keys::AUDIO_CHANNELS => spec.channel_count().get().to_string(),
        *keys::AUDIO_RATE => spec.rate.hz().to_string(),
        NODE_DONT_MOVE => "true",
    };
    if let Some(target) = target {
        properties.insert(*keys::TARGET_OBJECT, target.as_str());
    }
    if request.force_graph_rate {
        properties.insert(*keys::NODE_RATE, format!("1/{}", spec.rate.hz()));
    }
    match request.latency {
        LatencyRequest::Auto => {}
        LatencyRequest::Duration(duration) => {
            let frames = (duration.as_secs_f64() * f64::from(spec.rate.hz())).round() as u64;
            properties.insert(*keys::NODE_LATENCY, format!("{frames}/{}", spec.rate.hz()));
        }
    }

    let stream = StreamRc::new(core.clone(), &request.media_name, properties)
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;

    let taking = Arc::new(Taking::default());
    let asked = Rc::new(Cell::new(spec));
    let settled = Rc::new(Cell::new(None));
    let drained = Rc::new(Cell::new(false));
    let cycle = Cycle::new(spec, Arc::clone(&taking), Arc::clone(clock));
    let (fed, handing) = fed_from(Feed::new(cycle, source));
    let listener = stream
        .add_local_listener_with_user_data(fed)
        .process(|stream, fed| fed.run(stream))
        .drained({
            let events = events.clone();
            let drained = Rc::clone(&drained);
            move |_stream, _fed| {
                drained.set(true);
                let _ = events.try_send(StreamEvent::Drained);
            }
        })
        .state_changed({
            let events = events.clone();
            move |_stream, _fed, from, to| {
                let from = stream_state(&from);
                let to = stream_state(&to);
                let _ = events.try_send(StreamEvent::StateChanged { from, to });
            }
        })
        .param_changed({
            let events = events.clone();
            let taking = Arc::clone(&taking);
            let asked = Rc::clone(&asked);
            let settled = Rc::clone(&settled);
            let mut levels = StreamLevels::default();
            move |_stream, _fed, id, param| match id {
                sys::SPA_PARAM_Format => {
                    let spec = asked.get();
                    let Some(taken) = param.and_then(|param| negotiated(param, spec)) else {
                        return;
                    };
                    settled.set(Some(taken));
                    taking.store(taken.taken_against(spec));
                    let _ = events.try_send(taken.changed());
                }
                sys::SPA_PARAM_Props => {
                    if let Some(heard) = param.and_then(|param| levels.absorb(param)) {
                        let _ = events.try_send(StreamEvent::VolumeChanged(heard));
                    }
                }
                _ => {}
            }
        })
        .register()
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;

    let formats = enum_formats(spec)?;
    let mut params = Vec::with_capacity(formats.len());
    for bytes in &formats {
        params.push(Pod::from_bytes(bytes).ok_or(Error::PodUnread {
            param: PodParam::EnumFormat,
        })?);
    }

    let mut flags = StreamFlags::MAP_BUFFERS | StreamFlags::AUTOCONNECT;
    if request.realtime {
        flags |= StreamFlags::RT_PROCESS;
    }
    if request.no_convert {
        flags |= StreamFlags::NO_CONVERT;
    }

    stream
        .connect(Direction::Output, None, flags, &mut params)
        .map_err(|source| Error::daemon(PwOp::StreamConnect, source))?;

    Ok(ActiveStream {
        opened: Opened::new(stream, listener),
        handing,
        asked,
        settled,
        taking,
        clock: Arc::clone(clock),
        request: request.clone(),
        drain_asked: false,
        drained,
    })
}

fn enum_formats(spec: StreamSpec) -> Result<Vec<Vec<u8>>> {
    let narrower = packs_narrower(spec.format)
        .then(|| format_pod(spec, WireWord::Packed, sys::SPA_PARAM_EnumFormat))
        .transpose()?;
    let padded = format_pod(spec, WireWord::Padded, sys::SPA_PARAM_EnumFormat)?;
    Ok(narrower.into_iter().chain(iter::once(padded)).collect())
}

fn build_capture_stream(
    core: &pipewire::core::CoreRc,
    request: &CaptureRequest,
    sink: Box<dyn AudioSink>,
    events: &Sender<StreamEvent>,
) -> Result<HeardStream> {
    let spec = request.spec;
    let mut properties = pipewire::properties::properties! {
        *keys::MEDIA_TYPE => "Audio",
        *keys::MEDIA_CATEGORY => "Capture",
        *keys::MEDIA_ROLE => "Music",
        *keys::MEDIA_NAME => request.media_name.as_str(),
        *keys::AUDIO_CHANNELS => spec.channel_count().get().to_string(),
        *keys::AUDIO_RATE => spec.rate.hz().to_string(),
        *keys::NODE_DONT_RECONNECT => "true",
    };
    match &request.from {
        Capturing::Desktop { sink } => {
            properties.insert(*keys::STREAM_CAPTURE_SINK, "true");
            if let Some(sink) = sink {
                properties.insert(*keys::TARGET_OBJECT, sink.as_str());
            }
        }
        Capturing::Microphone { source } => {
            if let Some(source) = source {
                properties.insert(*keys::TARGET_OBJECT, source.as_str());
            }
        }
    }

    let stream = StreamRc::new(core.clone(), &request.media_name, properties)
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;
    let listener = stream
        .add_local_listener_with_user_data(sink)
        .process(|stream, sink| Hearing::run(stream, sink.as_mut()))
        .state_changed({
            let events = events.clone();
            move |_stream, _sink, from, to| {
                let from = stream_state(&from);
                let to = stream_state(&to);
                let _ = events.try_send(StreamEvent::StateChanged { from, to });
            }
        })
        .param_changed({
            let events = events.clone();
            move |_stream, _sink, id, param| {
                if id != sys::SPA_PARAM_Format {
                    return;
                }
                if let Some(taken) = param.and_then(|param| negotiated(param, spec)) {
                    let _ = events.try_send(taken.changed());
                }
            }
        })
        .register()
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;

    let format = format_pod(spec, WireWord::Padded, sys::SPA_PARAM_EnumFormat)?;
    let mut params = [Pod::from_bytes(&format).ok_or(Error::PodUnread {
        param: PodParam::EnumFormat,
    })?];
    stream
        .connect(
            Direction::Input,
            None,
            StreamFlags::MAP_BUFFERS | StreamFlags::AUTOCONNECT | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|source| Error::daemon(PwOp::StreamConnect, source))?;

    Ok(Opened::new(stream, listener))
}

fn stream_state(state: &pipewire::stream::StreamState) -> StreamState {
    match state {
        pipewire::stream::StreamState::Error(_) => StreamState::Failed,
        pipewire::stream::StreamState::Unconnected => StreamState::Unconnected,
        pipewire::stream::StreamState::Connecting => StreamState::Connecting,
        pipewire::stream::StreamState::Paused => StreamState::Paused,
        pipewire::stream::StreamState::Streaming => StreamState::Streaming,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use resonate_core::{ChannelCount, ChannelLayout, SampleFormat};

    use super::*;
    use crate::{
        HardwareVolume, Plugged,
        format::{Negotiated, WireWord},
        process::Taken,
    };

    fn asked(channels: ChannelLayout, format: SampleFormat) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_48000, channels, format)
    }

    fn read_back(bytes: &[u8], asked: StreamSpec) -> Negotiated {
        let pod = Pod::from_bytes(bytes).expect("a format pod");
        negotiated(pod, asked).expect("a raw audio format")
    }

    fn channels(count: u16) -> ChannelCount {
        ChannelCount::new(count).expect("a channel count in range")
    }

    #[test]
    fn every_layout_asked_for_reads_back_as_itself_once_the_graph_settles_on_it() {
        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround50,
            ChannelLayout::Surround51,
            ChannelLayout::Surround61,
            ChannelLayout::Surround71,
            ChannelLayout::Discrete(channels(3)),
            ChannelLayout::Discrete(channels(5)),
            ChannelLayout::Discrete(channels(6)),
        ] {
            for word in [WireWord::Padded, WireWord::Packed] {
                let spec = asked(layout, SampleFormat::S24);
                let settled = format_pod(spec, word, sys::SPA_PARAM_Format).expect("a pod");
                let taken = read_back(&settled, spec);

                assert_eq!(taken.spec, spec, "{layout} read back as another layout");
                assert_eq!(taken.word, word);
                assert_eq!(
                    taken.taken_against(spec),
                    Taken::AsAsked(word),
                    "{layout} was taken as a format the ring is not laid out in"
                );
            }
        }
    }

    #[test]
    fn a_layout_other_than_the_one_asked_for_is_taken_as_another_frame() {
        let stereo = asked(ChannelLayout::Stereo, SampleFormat::S16);
        let five = asked(ChannelLayout::Surround50, SampleFormat::S16);
        let settled = format_pod(five, WireWord::Padded, sys::SPA_PARAM_Format).expect("a pod");
        let taken = read_back(&settled, stereo);

        assert_eq!(taken.spec, five);
        assert_eq!(
            taken.taken_against(stereo),
            Taken::Otherwise {
                frame: NonZeroU16::new(5 * 2).expect("a five-channel frame")
            }
        );

        let six_one = asked(ChannelLayout::Surround61, SampleFormat::S24);
        let settled = format_pod(six_one, WireWord::Packed, sys::SPA_PARAM_Format).expect("a pod");

        assert_eq!(
            read_back(&settled, stereo).taken_against(stereo),
            Taken::Otherwise {
                frame: NonZeroU16::new(7 * 3).expect("a packed 6.1 frame")
            }
        );
    }

    #[test]
    fn a_graph_naming_no_positions_leaves_the_layout_asked_for_where_the_count_agrees() {
        let mut info = AudioInfoRaw::new();
        info.set_format(spa_format(SampleFormat::F32, WireWord::Padded));
        info.set_rate(SampleRate::HZ_48000.hz());
        info.set_channels(5);
        let unplaced = pod_of(info, sys::SPA_PARAM_Format).expect("a pod");

        assert_eq!(
            read_back(
                &unplaced,
                asked(ChannelLayout::Surround50, SampleFormat::F32)
            )
            .spec
            .channels,
            ChannelLayout::Surround50
        );
        assert_eq!(
            read_back(&unplaced, asked(ChannelLayout::Stereo, SampleFormat::F32))
                .spec
                .channels,
            ChannelLayout::Discrete(channels(5)),
            "a count the ask did not name was given a layout nobody said"
        );
    }

    #[test]
    fn positions_naming_no_layout_read_as_unnamed_channels() {
        let mut info = AudioInfoRaw::new();
        info.set_format(spa_format(SampleFormat::F32, WireWord::Padded));
        info.set_rate(SampleRate::HZ_48000.hz());
        info.set_channels(6);
        let mut position = [0; MAX_CHANNELS];
        for (slot, channel) in position.iter_mut().zip([
            sys::SPA_AUDIO_CHANNEL_FL,
            sys::SPA_AUDIO_CHANNEL_FR,
            sys::SPA_AUDIO_CHANNEL_FC,
            sys::SPA_AUDIO_CHANNEL_RC,
            sys::SPA_AUDIO_CHANNEL_SL,
            sys::SPA_AUDIO_CHANNEL_SR,
        ]) {
            *slot = channel;
        }
        info.set_position(position);
        let six_oh = pod_of(info, sys::SPA_PARAM_Format).expect("a pod");

        assert_eq!(
            read_back(&six_oh, asked(ChannelLayout::Surround51, SampleFormat::F32))
                .spec
                .channels,
            ChannelLayout::Discrete(channels(6)),
            "a 6.0 answer was read as the 5.1 its count suggests"
        );
    }

    #[test]
    fn a_stream_its_asker_gave_up_on_is_not_kept_and_one_kept_is_still_waited_for() {
        let abandoned = Awaiting::default();
        let answered = Awaiting::default();

        assert!(abandoned.given_up());
        assert!(abandoned.is_given_up());
        assert!(!abandoned.kept());

        assert!(answered.kept());
        assert!(!answered.given_up());
        assert!(!answered.is_given_up());
    }

    #[test]
    fn a_daemon_gone_by_reset_or_abort_is_as_lost_as_one_gone_by_a_broken_pipe() {
        const EPIPE: i32 = 32;
        const ECONNRESET: i32 = 104;
        const ECONNABORTED: i32 = 103;
        const ENOTCONN: i32 = 107;
        const ENOENT: i32 = 2;
        const EBUSY: i32 = 16;
        const EINVAL: i32 = 22;

        for errno in [EPIPE, ECONNRESET, ECONNABORTED, ENOTCONN] {
            assert!(
                is_a_broken_connection(-errno),
                "errno {errno} was not taken as lost"
            );
        }
        for errno in [EINVAL, ENOENT, EBUSY] {
            assert!(
                !is_a_broken_connection(-errno),
                "errno {errno} was taken as lost"
            );
        }
    }

    fn sink(names_an_api: bool, device: Option<u32>) -> SinkRecord {
        SinkRecord {
            name: "alsa_output.test".to_owned(),
            description: "Test DAC".to_owned(),
            names_an_api,
            device,
            seat: Some(1),
            formats: BTreeMap::new(),
            format_serial: None,
            running_at: None,
            running_serial: None,
        }
    }

    #[test]
    fn a_sinks_formats_are_read_again_only_when_the_node_says_they_moved() {
        let mut record = sink(true, Some(49));
        record.formats.insert(3, Vec::new());

        assert!(
            !record.formats_moved(false),
            "the first sighting enumerated again"
        );
        assert!(!record.formats_moved(false));
        assert_eq!(record.formats.len(), 1);

        assert!(record.formats_moved(true));
        assert!(
            record.formats.is_empty(),
            "a stale index outlived the enumeration"
        );
        assert!(!record.formats_moved(true));
        assert!(record.formats_moved(false));
    }

    #[test]
    fn a_running_device_reports_its_own_rate_and_the_graphs_once_it_stops() {
        let mut graph = Discovered::default();
        graph.heard(HeldIn::Settings, CORE_ID, Some(CLOCK_RATE), Some("48000"));
        let mut record = sink(true, Some(49));
        record.running_at = Some(SampleRate::HZ_44100);
        graph.sinks.insert(50, record);

        assert_eq!(graph.snapshot()[0].current_rate, Some(SampleRate::HZ_44100));

        let record = graph.sinks.get_mut(&50).expect("the sink held");
        assert!(!record.running_moved(false), "the first sighting moved");
        assert!(record.running_moved(true));
        assert_eq!(
            graph.snapshot()[0].current_rate,
            Some(SampleRate::HZ_48000),
            "a stopped device kept the rate it last ran at"
        );
    }

    fn headphones(plugged: Plugged) -> SinkPort {
        SinkPort {
            description: "Headphones".to_owned(),
            plugged,
            hardware_volume: HardwareVolume::Unsaid,
            volume: None,
            muted: false,
        }
    }

    #[test]
    fn a_rate_key_cleared_or_zeroed_leaves_the_rate_unforced() {
        let mut graph = Discovered::default();

        graph.heard(HeldIn::Settings, CORE_ID, Some(FORCED_RATE), Some("96000"));
        graph.heard(HeldIn::Settings, CORE_ID, Some(CLOCK_RATE), Some("48000"));

        assert_eq!(graph.forced_rate, Some(SampleRate::HZ_96000));
        assert_eq!(graph.clock_rate, Some(SampleRate::HZ_48000));

        graph.heard(HeldIn::Settings, CORE_ID, Some(FORCED_RATE), None);

        assert_eq!(graph.forced_rate, None);

        graph.heard(HeldIn::Settings, CORE_ID, Some(FORCED_RATE), Some("96000"));
        graph.heard(HeldIn::Settings, CORE_ID, Some(FORCED_RATE), Some("0"));

        assert_eq!(graph.forced_rate, None);
        assert_eq!(graph.clock_rate, Some(SampleRate::HZ_48000));
    }

    #[test]
    fn a_default_sink_cleared_is_forgotten_and_announced_once() {
        const NAMED: &str = r#"{ "name": "alsa_output.test" }"#;

        let mut graph = Discovered::default();

        assert_eq!(
            graph.heard(HeldIn::Defaults, CORE_ID, Some(DEFAULT_SINK), Some(NAMED)),
            DefaultSink::Moved
        );
        assert_eq!(
            graph.heard(HeldIn::Defaults, CORE_ID, Some(DEFAULT_SINK), Some(NAMED)),
            DefaultSink::Stayed
        );
        assert_eq!(
            graph.heard(HeldIn::Defaults, CORE_ID, Some(DEFAULT_SINK), None),
            DefaultSink::Moved
        );
        assert_eq!(graph.default_sink, None);
    }

    #[test]
    fn every_key_cleared_at_once_clears_only_what_that_metadata_holds() {
        const NAMED: &str = r#"{ "name": "alsa_output.test" }"#;

        let mut graph = Discovered::default();
        graph.heard(HeldIn::Defaults, CORE_ID, Some(DEFAULT_SINK), Some(NAMED));
        graph.heard(HeldIn::Settings, CORE_ID, Some(FORCED_RATE), Some("96000"));
        graph.heard(
            HeldIn::Settings,
            CORE_ID,
            Some(ALLOWED_RATES),
            Some("[ 44100 48000 ]"),
        );

        assert_eq!(
            graph.heard(HeldIn::Settings, CORE_ID, None, None),
            DefaultSink::Stayed
        );
        assert_eq!(graph.forced_rate, None);
        assert!(graph.allowed_rates.is_empty());
        assert_eq!(graph.default_sink.as_deref(), Some("alsa_output.test"));

        assert_eq!(
            graph.heard(HeldIn::Defaults, CORE_ID, None, None),
            DefaultSink::Moved
        );
        assert_eq!(graph.default_sink, None);
    }

    #[test]
    fn a_node_leaving_the_metadata_leaves_the_default_sink_where_it_was() {
        const NAMED: &str = r#"{ "name": "alsa_output.test" }"#;
        const A_STREAM: u32 = 87;

        let mut graph = Discovered::default();
        graph.heard(HeldIn::Defaults, CORE_ID, Some(DEFAULT_SINK), Some(NAMED));

        assert_eq!(
            graph.heard(
                HeldIn::Defaults,
                A_STREAM,
                Some("target.object"),
                Some("alsa_output.other")
            ),
            DefaultSink::Stayed
        );
        assert_eq!(
            graph.heard(HeldIn::Defaults, A_STREAM, None, None),
            DefaultSink::Stayed
        );
        assert_eq!(
            graph.heard(HeldIn::Defaults, A_STREAM, Some(DEFAULT_SINK), None),
            DefaultSink::Stayed
        );
        assert_eq!(graph.default_sink.as_deref(), Some("alsa_output.test"));
    }

    #[test]
    fn a_sink_is_hardware_where_it_or_the_device_above_it_names_a_driver() {
        let mut graph = Discovered::default();
        graph.driven.insert(49);

        assert!(
            graph.is_hardware(&sink(false, Some(49))),
            "a node whose device names a driver was read as something the graph made up"
        );
        assert!(graph.is_hardware(&sink(true, None)));
        assert!(!graph.is_hardware(&sink(false, Some(50))));
        assert!(!graph.is_hardware(&sink(false, None)));
    }

    fn serving(seats: &[i32], port: SinkPort) -> AdvertisedRoute {
        AdvertisedRoute {
            index: Some(0),
            seats: seats.to_vec(),
            channel_volumes: vec![0.125, 0.25],
            port,
        }
    }

    fn heard_at(volume: f32) -> SinkPort {
        SinkPort {
            hardware_volume: HardwareVolume::Yes,
            volume: Gain::new(volume).ok(),
            ..headphones(Plugged::Yes)
        }
    }

    #[test]
    fn a_metadata_object_that_leaves_takes_the_values_it_held_with_it() {
        let mut graph = Discovered::default();
        graph.heard(
            HeldIn::Defaults,
            CORE_ID,
            Some(DEFAULT_SINK),
            Some(r#"{ "name": "alsa_output.usb" }"#),
        );
        graph.heard(HeldIn::Settings, CORE_ID, Some(CLOCK_RATE), Some("44100"));
        assert_eq!(graph.default_sink.as_deref(), Some("alsa_output.usb"));
        assert!(graph.clock_rate.is_some());

        let moved = graph.heard(HeldIn::Defaults, CORE_ID, None, None);

        assert_eq!(moved, DefaultSink::Moved);
        assert_eq!(graph.default_sink, None);
        assert!(
            graph.clock_rate.is_some(),
            "the settings object's values were cleared with the defaults'"
        );
        graph.heard(HeldIn::Settings, CORE_ID, None, None);
        assert_eq!(graph.clock_rate, None);
    }

    #[test]
    fn a_sink_comes_out_of_the_route_its_card_seats_it_on() {
        let mut graph = Discovered::default();
        let mut ports = DevicePorts::default();
        ports.keep(Held::Offered, 0, serving(&[1], headphones(Plugged::Yes)));
        ports.keep(
            Held::Offered,
            1,
            serving(
                &[2],
                SinkPort {
                    description: "Digital Output (S/PDIF)".to_owned(),
                    plugged: Plugged::Unsaid,
                    hardware_volume: HardwareVolume::Unsaid,
                    volume: None,
                    muted: false,
                },
            ),
        );
        graph.ports.insert(49, ports);

        assert_eq!(
            graph.port_of(&sink(false, Some(49))),
            Some(headphones(Plugged::Yes)),
            "a sink was drawn against a port its card does not seat it on"
        );
        assert_eq!(
            graph.port_of(&SinkRecord {
                seat: None,
                ..sink(false, Some(49))
            }),
            None,
            "a node the graph made up was given a physical port"
        );
        assert_eq!(graph.port_of(&sink(false, Some(50))), None);
        assert_eq!(graph.port_of(&sink(true, None)), None);
    }

    #[test]
    fn the_route_a_card_is_switched_to_outranks_the_one_it_merely_offers() {
        let mut graph = Discovered::default();
        let mut ports = DevicePorts::default();
        ports.keep(Held::Offered, 0, serving(&[1], headphones(Plugged::Unsaid)));
        ports.keep(Held::Current, 0, serving(&[1], headphones(Plugged::Yes)));
        graph.ports.insert(49, ports);

        assert_eq!(
            graph.port_of(&sink(false, Some(49))),
            Some(headphones(Plugged::Yes)),
            "a card switched to a port was drawn as the port it only offers"
        );
    }

    #[test]
    fn a_port_a_card_offers_and_is_not_switched_to_is_what_an_unplugged_sink_reads_as() {
        let mut graph = Discovered::default();
        let mut ports = DevicePorts::default();
        ports.keep(
            Held::Offered,
            0,
            serving(
                &[7, 8, 9],
                SinkPort {
                    description: "HDMI / DisplayPort 2".to_owned(),
                    plugged: Plugged::No,
                    hardware_volume: HardwareVolume::Unsaid,
                    volume: None,
                    muted: false,
                },
            ),
        );
        graph.ports.insert(48, ports);

        assert_eq!(
            graph
                .port_of(&SinkRecord {
                    seat: Some(7),
                    ..sink(false, Some(48))
                })
                .map(|port| port.plugged),
            Some(Plugged::No),
            "a card publishes no current route for a port nothing is plugged into"
        );
    }

    #[test]
    fn a_device_that_goes_away_leaves_the_nodes_under_it_reading_as_the_graphs_own() {
        let mut graph = Discovered::default();
        graph.driven.insert(49);
        graph.sinks.insert(59, sink(false, Some(49)));

        assert!(graph.snapshot()[0].is_hardware);

        graph.driven.remove(&49);
        assert!(!graph.snapshot()[0].is_hardware);
    }

    #[test]
    fn a_route_the_card_is_switched_to_says_which_seats_it_turned_when_its_volume_moves() {
        let mut ports = DevicePorts::default();
        assert_eq!(
            ports.keep(Held::Current, 0, serving(&[1], heard_at(0.125))),
            None,
            "a route read for the first time was announced as turned"
        );
        assert_eq!(
            ports.keep(Held::Current, 0, serving(&[1], heard_at(0.125))),
            None
        );
        assert_eq!(
            ports.keep(Held::Current, 0, serving(&[1], heard_at(0.5))),
            Some(vec![1])
        );
        assert_eq!(
            ports.keep(
                Held::Current,
                0,
                serving(
                    &[1],
                    SinkPort {
                        muted: true,
                        ..heard_at(0.5)
                    }
                )
            ),
            Some(vec![1]),
            "a route muted at the level it was at was not announced as turned"
        );
        assert_eq!(
            ports.keep(Held::Offered, 0, serving(&[1], heard_at(0.125))),
            None,
            "a port the card only offers was announced as turned"
        );
    }

    #[test]
    fn a_sink_turns_the_route_its_card_is_switched_to_and_no_other() {
        let mut graph = Discovered::default();
        graph.sinks.insert(59, sink(false, Some(49)));
        let mut ports = DevicePorts::default();
        ports.keep(Held::Offered, 3, serving(&[1], heard_at(0.125)));
        graph.ports.insert(49, ports);
        let half = RouteSetting::Volume(Gain::new(0.5).expect("in range"));

        assert_eq!(
            graph.route_turning(SinkId::new(59), half),
            None,
            "a route the card is not switched to was turned"
        );

        graph
            .ports
            .entry(49)
            .or_default()
            .keep(Held::Current, 3, serving(&[1], heard_at(0.125)));
        assert_eq!(
            graph.route_turning(SinkId::new(59), half),
            Some((
                49,
                RouteChange {
                    index: 0,
                    seat: 1,
                    channel_volumes: vec![0.125, 0.25],
                    setting: half,
                }
            ))
        );
        assert_eq!(graph.route_turning(SinkId::new(60), half), None);
        assert_eq!(graph.sinks_seated_on(49, &[1]), vec![SinkId::new(59)]);
        assert!(graph.sinks_seated_on(49, &[2]).is_empty());
    }
}
