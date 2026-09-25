use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io, iter, mem,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded};
use libspa::{
    param::{
        ParamType,
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
    device::{Device, DeviceListener},
    keys,
    main_loop::MainLoopRc,
    metadata::{Metadata, MetadataListener},
    node::{Node, NodeListener},
    registry::{GlobalObject, RegistryRc},
    stream::{StreamFlags, StreamListener, StreamRc},
    types::ObjectType,
};
use resonate_core::{SampleRate, StreamSpec};

use crate::{
    AudioSink, AudioSource, CaptureRequest, CaptureStream, Capturing, Error, LatencyRequest,
    MediaRole, Microphone, NodeName, PodParam, PwOp, Result, SinkChange, SinkFormats, SinkId,
    SinkInfo, SinkPort, SinkStream, StreamCommand, StreamEvent, StreamRequest, StreamState,
    format::{
        AdvertisedFormat, AdvertisedRoute, WireWord, negotiated, packs_narrower,
        parse_allowed_rates, parse_default_sink, parse_enum_format, parse_profile, parse_rate,
        parse_route, spa_format, spa_position,
    },
    process::{Cycle, Hearing},
};

type Pending = Rc<RefCell<Vec<(i32, Sender<()>)>>>;
type Nodes = Rc<RefCell<BTreeMap<u32, (Node, NodeListener)>>>;
type Devices = Rc<RefCell<BTreeMap<u32, (Device, DeviceListener)>>>;
type Metadatas = Rc<RefCell<BTreeMap<u32, (Metadata, MetadataListener)>>>;
type ActiveStream = (StreamRc, StreamListener<Box<dyn AudioSource>>);
type HeardStream = (StreamRc, StreamListener<Box<dyn AudioSink>>);

const CORE_ID: u32 = 0;
const RECONNECT_EVERY: Duration = Duration::from_secs(1);
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
    latency: Arc<std::sync::atomic::AtomicU64>,
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
}

enum Request {
    Sync(Sender<()>),
    Open(Box<OpenRequest>),
    Capture(Box<CaptureOpen>),
    StopCapture,
    SetActive(bool),
    Drain,
    Close,
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
    fn keep(&mut self, held: Held, index: u32, route: AdvertisedRoute) {
        match held {
            Held::Current => self.current.insert(index, route),
            Held::Offered => self.offered.insert(index, route),
        };
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
}

impl SinkRecord {
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
    profiles: BTreeMap<u32, String>,
    allowed_rates: Vec<SampleRate>,
    clock_rate: Option<SampleRate>,
    forced_rate: Option<SampleRate>,
    default_sink: Option<String>,
}

impl Discovered {
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
                    current_rate: self.forced_rate.or(self.clock_rate),
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

    fn profile_of(&self, record: &SinkRecord) -> Option<String> {
        self.profiles.get(&record.device?).cloned()
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

    fn unanswered(&self) -> Error {
        if self.connected.load(Ordering::Acquire) {
            Error::LoopStopped
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

    fn node_name(&self, node: SinkId) -> Result<NodeName> {
        self.survey
            .shared
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
        let latency = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (reply, outcome) = bounded(1);

        self.survey
            .commands
            .send(Request::Open(Box::new(OpenRequest {
                request: request.clone(),
                target,
                source,
                slots: StreamSlots {
                    latency: Arc::clone(&latency),
                    events,
                },
                reply,
            })))
            .map_err(|_| Error::LoopStopped)?;

        outcome
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::LoopStopped)??;

        let control = self.survey.commands.clone();
        Ok(SinkStream::new(
            incoming,
            latency,
            Box::new(move |command| {
                control
                    .send(match command {
                        StreamCommand::SetActive(active) => Request::SetActive(active),
                        StreamCommand::Drain => Request::Drain,
                        StreamCommand::Close => Request::Close,
                    })
                    .map_err(|_| Error::LoopStopped)
            }),
        ))
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
        Ok(graph) => Rc::new(RefCell::new(Some(graph))),
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    connected.store(true, Ordering::Release);

    let active: Rc<RefCell<Option<ActiveStream>>> = Rc::new(RefCell::new(None));
    let heard: Rc<RefCell<Option<HeardStream>>> = Rc::new(RefCell::new(None));
    let sequence = Rc::new(RefCell::new(0_i32));
    let receiver = requests.attach(mainloop.loop_(), {
        let mainloop = mainloop.downgrade();
        let graph = Rc::clone(&graph);
        let pending = Rc::clone(&pending);
        let sequence = Rc::clone(&sequence);
        let active = Rc::clone(&active);
        let heard = Rc::clone(&heard);
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
                if let Some((stream, _)) = heard.borrow().as_ref() {
                    let _ = stream.disconnect();
                }
                heard.borrow_mut().take();
            }
            Request::SetActive(wanted) => {
                if let Some((stream, _)) = active.borrow().as_ref() {
                    let _ = stream.set_active(wanted);
                }
            }
            Request::Drain => {
                if let Some((stream, _)) = active.borrow().as_ref() {
                    let _ = stream.flush(true);
                }
            }
            Request::Close => {
                if let Some((stream, _)) = active.borrow().as_ref() {
                    let _ = stream.disconnect();
                }
                active.borrow_mut().take();
            }
            Request::Lost => {
                let Some(gone) = graph.borrow_mut().take() else {
                    return;
                };
                tracing::warn!("the PipeWire daemon went away; reconnecting when it is back");
                connected.store(false, Ordering::Release);
                active.borrow_mut().take();
                heard.borrow_mut().take();
                pending.borrow_mut().clear();
                drop(gone);
                reaching.forget_the_graph();
                reaching.ask_again_later();
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

    let _ = ready.send(Ok(()));
    mainloop.run();

    drop(receiver);
    active.borrow_mut().take();
    heard.borrow_mut().take();
    graph.borrow_mut().take();
    connected.store(false, Ordering::Release);
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
        io::Error::from_raw_os_error(errno).kind() == io::ErrorKind::BrokenPipe
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
                    };
                    shared.lock().sinks.insert(global.id, record);

                    let Ok(node) = registry.bind::<Node, _>(global) else {
                        return;
                    };
                    let listener = node
                        .add_listener_local()
                        .info({
                            let shared = Arc::clone(&shared);
                            let id = global.id;
                            move |info| {
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
                                if param_type != ParamType::EnumFormat {
                                    return;
                                }
                                let Some(param) = param else { return };
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
                        .param({
                            let shared = Arc::clone(&shared);
                            let id = global.id;
                            move |_seq, param_type, index, _next, param| {
                                let Some(param) = param else { return };
                                let Ok((_, value)) =
                                    PodDeserializer::deserialize_any_from(param.as_bytes())
                                else {
                                    return;
                                };
                                if param_type == ParamType::Profile {
                                    if let Some(profile) = parse_profile(&value) {
                                        shared.lock().profiles.insert(id, profile);
                                    }
                                    return;
                                }
                                let Some(held) = Held::of(param_type) else {
                                    return;
                                };
                                let Some(route) = parse_route(&value) else {
                                    return;
                                };
                                shared
                                    .lock()
                                    .ports
                                    .entry(id)
                                    .or_default()
                                    .keep(held, index, route);
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
                    let which = node_property(global, METADATA_NAME).unwrap_or_default();
                    if which != "settings" && which != "default" {
                        return;
                    }
                    let Ok(metadata) = registry.bind::<Metadata, _>(global) else {
                        return;
                    };
                    let listener = metadata
                        .add_listener_local()
                        .property({
                            let shared = Arc::clone(&shared);
                            let announce = announce.clone();
                            move |_subject, key, _type, value| {
                                match (key, value) {
                                    (Some(ALLOWED_RATES), Some(value)) => {
                                        shared.lock().allowed_rates = parse_allowed_rates(value);
                                    }
                                    (Some(CLOCK_RATE), Some(value)) => {
                                        shared.lock().clock_rate = parse_rate(value);
                                    }
                                    (Some(FORCED_RATE), Some(value)) => {
                                        shared.lock().forced_rate = parse_rate(value);
                                    }
                                    (Some(DEFAULT_SINK), Some(value)) => {
                                        shared.lock().default_sink = parse_default_sink(value);
                                        let _ = announce.try_send(SinkChange::DefaultChanged);
                                    }
                                    (Some(DEFAULT_SOURCE), Some(value)) => {
                                        shared.lock().default_source = parse_default_sink(value);
                                    }
                                    _ => {}
                                }
                                0
                            }
                        })
                        .register();
                    metadatas
                        .borrow_mut()
                        .insert(global.id, (metadata, listener));
                }
                _ => {}
            }
        })
        .global_remove({
            let shared = Arc::clone(shared);
            let nodes = Rc::clone(nodes);
            let devices = Rc::clone(devices);
            let announce = announce.clone();
            move |id| {
                let mut state = shared.lock();
                state.driven.remove(&id);
                state.microphones.remove(&id);
                if state.ports.remove(&id).is_some() {
                    devices.borrow_mut().remove(&id);
                }
                if state.sinks.remove(&id).is_some() {
                    drop(state);
                    nodes.borrow_mut().remove(&id);
                    let _ = announce.try_send(SinkChange::Removed(SinkId::new(id)));
                }
            }
        })
        .register()
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
    } = open;

    let outcome = build_stream(core, &request, target.as_ref(), source, &slots);
    match outcome {
        Ok(stream) => {
            active.borrow_mut().replace(stream);
            let _ = reply.send(Ok(()));
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
    let StreamSlots { latency, events } = slots;
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
        LatencyRequest::Frames(frames) => {
            properties.insert(*keys::NODE_LATENCY, format!("{frames}/{}", spec.rate.hz()));
        }
        LatencyRequest::Duration(duration) => {
            let frames = (duration.as_secs_f64() * f64::from(spec.rate.hz())).round() as u64;
            properties.insert(*keys::NODE_LATENCY, format!("{frames}/{}", spec.rate.hz()));
        }
    }

    let stream = StreamRc::new(core.clone(), &request.media_name, properties)
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;

    let packed = Arc::new(AtomicBool::new(false));
    let cycle = Cycle::new(spec, Arc::clone(&packed), Arc::clone(latency));
    let listener = stream
        .add_local_listener_with_user_data(source)
        .process(move |stream, source| cycle.run(stream, source.as_mut()))
        .drained({
            let events = events.clone();
            move |_stream, _source| {
                let _ = events.try_send(StreamEvent::Drained);
            }
        })
        .state_changed({
            let events = events.clone();
            move |_stream, _source, from, to| {
                let from = StreamState::from(&from);
                let to = StreamState::from(&to);
                let _ = events.try_send(StreamEvent::StateChanged { from, to });
            }
        })
        .param_changed({
            let events = events.clone();
            let packed = Arc::clone(&packed);
            move |_stream, _source, id, param| {
                if id != sys::SPA_PARAM_Format {
                    return;
                }
                let Some(taken) = param.and_then(negotiated) else {
                    return;
                };
                packed.store(taken.word.is_packed(), Ordering::Relaxed);
                let _ = events.try_send(taken.changed());
            }
        })
        .register()
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;

    let narrower = packs_narrower(spec.format)
        .then(|| format_pod(spec, WireWord::Packed, sys::SPA_PARAM_EnumFormat))
        .transpose()?;
    let padded = format_pod(spec, WireWord::Padded, sys::SPA_PARAM_EnumFormat)?;

    let mut params = Vec::with_capacity(2);
    for bytes in narrower.as_ref().into_iter().chain(iter::once(&padded)) {
        params.push(Pod::from_bytes(bytes).ok_or(Error::PodBuild {
            param: PodParam::EnumFormat,
            source: libspa::pod::serialize::GenError::NotYetImplemented,
        })?);
    }

    let mut flags = StreamFlags::MAP_BUFFERS | StreamFlags::AUTOCONNECT;
    if request.realtime {
        flags |= StreamFlags::RT_PROCESS;
    }
    if request.no_convert {
        flags |= StreamFlags::NO_CONVERT;
    }
    if request.exclusive {
        flags |= StreamFlags::EXCLUSIVE;
    }

    stream
        .connect(Direction::Output, None, flags, &mut params)
        .map_err(|source| Error::daemon(PwOp::StreamConnect, source))?;

    Ok((stream, listener))
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
                let from = StreamState::from(&from);
                let to = StreamState::from(&to);
                let _ = events.try_send(StreamEvent::StateChanged { from, to });
            }
        })
        .param_changed({
            let events = events.clone();
            move |_stream, _sink, id, param| {
                if id != sys::SPA_PARAM_Format {
                    return;
                }
                if let Some(taken) = param.and_then(negotiated) {
                    let _ = events.try_send(taken.changed());
                }
            }
        })
        .register()
        .map_err(|source| Error::daemon(PwOp::StreamCreate, source))?;

    let format = format_pod(spec, WireWord::Padded, sys::SPA_PARAM_EnumFormat)?;
    let mut params = [Pod::from_bytes(&format).ok_or(Error::PodBuild {
        param: PodParam::EnumFormat,
        source: libspa::pod::serialize::GenError::NotYetImplemented,
    })?];
    stream
        .connect(
            Direction::Input,
            None,
            StreamFlags::MAP_BUFFERS | StreamFlags::AUTOCONNECT | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|source| Error::daemon(PwOp::StreamConnect, source))?;

    Ok((stream, listener))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HardwareVolume, Plugged};

    fn sink(names_an_api: bool, device: Option<u32>) -> SinkRecord {
        SinkRecord {
            name: "alsa_output.test".to_owned(),
            description: "Test DAC".to_owned(),
            names_an_api,
            device,
            seat: Some(1),
            formats: BTreeMap::new(),
        }
    }

    fn headphones(plugged: Plugged) -> SinkPort {
        SinkPort {
            description: "Headphones".to_owned(),
            plugged,
            hardware_volume: HardwareVolume::Unsaid,
        }
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
            seats: seats.to_vec(),
            port,
        }
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
}
