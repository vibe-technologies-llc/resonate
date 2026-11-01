use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    io, iter, mem,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, unbounded};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, HardwareVolume, Plugged, SinkChange, SinkId, SinkInfo, SinkPort, SinkResult,
    SinkStream, StreamRequest,
};
use resonate_core::{Gain, Volume};
use resonate_upnp::{FirstSearch, Neighbours, Server};

use crate::{
    CastId, Device, Error, Found, Given, LinkOp, Result,
    bonjour::{self, CAST_SERVICE},
    device,
    link::Link,
    stream::{self, Lanes},
};

const RECEIVER_IDS_FROM: u32 = 0xE000_0000;

const RECEIVER_IDS_UNDER: u32 = 0xF000_0000;

const BROWSE_WAKES_EVERY: Duration = Duration::from_millis(250);

const ADDRESSES_TRIED_AGAIN_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    Bonjour,
    Listed(Vec<Found>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Searching {
    pub finding: Finding,
    pub within: Duration,
    pub serving_on: IpAddr,
}

impl Searching {
    pub const fn on_the_network() -> Self {
        Self {
            finding: Finding::Bonjour,
            within: Duration::from_secs(2),
            serving_on: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }
}

pub struct Casting {
    shared: Arc<Shared>,
    looking: Mutex<Option<Looking>>,
}

struct Looking {
    stop: Sender<()>,
    daemon: Option<ServiceDaemon>,
    browsing: bool,
}

struct Shared {
    searching: Searching,
    server: Mutex<Option<Arc<Server>>>,
    lanes: Lanes,
    known: Mutex<Known>,
    located: Mutex<Located>,
    listeners: Mutex<Vec<Sender<SinkChange>>>,
    turns: Sender<Turn>,
}

#[derive(Default)]
struct Known {
    next: u32,
    held: BTreeMap<SinkId, Arc<Device>>,
    named: BTreeMap<String, CastId>,
    addressed: BTreeSet<SinkId>,
}

#[derive(Default)]
struct Located {
    given: BTreeSet<Given>,
    resolved: BTreeMap<Given, SocketAddr>,
}

impl Located {
    fn addresses(&self) -> BTreeSet<SocketAddr> {
        self.given
            .iter()
            .filter_map(|given| {
                given
                    .address()
                    .or_else(|| self.resolved.get(given).copied())
            })
            .collect()
    }
}

enum Turn {
    Volume(Arc<Device>, Volume),
    Mute(Arc<Device>, bool),
}

impl Casting {
    pub fn start(searching: Searching, first: FirstSearch) -> Result<Self> {
        let casting = Self::resting(searching)?;
        casting.look(first)?;
        Ok(casting)
    }

    pub fn resting(searching: Searching) -> Result<Self> {
        let (turns, turned) = unbounded();
        let shared = Arc::new(Shared {
            searching,
            server: Mutex::new(None),
            lanes: Lanes::default(),
            known: Mutex::new(Known::default()),
            located: Mutex::new(Located::default()),
            listeners: Mutex::new(Vec::new()),
            turns,
        });

        thread::Builder::new()
            .name("resonate-cast-turns".to_owned())
            .spawn(move || turn_when_told(&turned))
            .map_err(spawn_failed)?;

        Ok(Self {
            shared,
            looking: Mutex::new(None),
        })
    }

    pub fn look(&self, first: FirstSearch) -> Result<()> {
        let mut looking = self.looking.lock();
        match looking.take() {
            Some(held) if held.browsing => {
                *looking = Some(held);
                return Ok(());
            }
            Some(held) => {
                let _ = held.stop.send(());
            }
            None => {}
        }
        self.begin(&mut looking, first, true)
    }

    pub fn look_at_addresses(&self, first: FirstSearch) -> Result<()> {
        let mut looking = self.looking.lock();
        if looking.is_some() {
            return Ok(());
        }
        self.begin(&mut looking, first, false)
    }

    pub fn stop_browsing(&self) {
        let mut looking = self.looking.lock();
        if !looking.as_ref().is_some_and(|held| held.browsing) {
            return;
        }
        if self.shared.located.lock().given.is_empty() {
            drop(looking);
            self.rest();
            return;
        }
        if let Some(held) = looking.take() {
            Self::stop(held);
        }
        let gone = self.shared.forget_unlocated();
        self.shared.announce(&gone);
        if let Err(error) = self.begin(&mut looking, FirstSearch::Behind, false) {
            tracing::warn!(%error, "the Cast receivers given by address could not be reached");
        }
    }

    pub fn rest(&self) {
        let Some(looking) = self.looking.lock().take() else {
            return;
        };
        Self::stop(looking);
        let gone: Vec<SinkChange> = {
            let mut known = self.shared.known.lock();
            known.named.clear();
            known.addressed.clear();
            mem::take(&mut known.held)
                .into_keys()
                .map(SinkChange::Removed)
                .collect()
        };
        self.shared.announce(&gone);
    }

    pub fn is_looking(&self) -> bool {
        self.looking
            .lock()
            .as_ref()
            .is_some_and(|held| held.browsing)
    }

    pub fn is_reaching(&self) -> bool {
        self.looking.lock().is_some()
    }

    pub fn locate(&self, given: impl Into<Given>) -> Result<()> {
        let given = given.into();
        self.shared.located.lock().given.insert(given.clone());
        if !self.is_reaching() {
            return Ok(());
        }
        match given {
            Given::Address(at) => self.shared.reach_address(at),
            named => {
                let shared = Arc::clone(&self.shared);
                thread::Builder::new()
                    .name("resonate-cast-resolve".to_owned())
                    .spawn(move || {
                        if let Err(error) = shared.reach_given(&named) {
                            tracing::debug!(%error, given = %named, "a Cast receiver given by name could not be reached");
                        }
                    })
                    .map_err(spawn_failed)?;
                Ok(())
            }
        }
    }

    pub fn unlocate(&self, given: impl Into<Given>) {
        let given = given.into();
        let forgotten = {
            let mut located = self.shared.located.lock();
            if !located.given.remove(&given) {
                return;
            }
            let at = given.address().or_else(|| located.resolved.remove(&given));
            at.filter(|at| !located.addresses().contains(at))
        };
        if let Some(at) = forgotten {
            let gone = self.shared.forget_at(at);
            self.shared.announce(&gone);
        }
    }

    pub fn located(&self) -> Vec<Given> {
        self.shared.located.lock().given.iter().cloned().collect()
    }

    fn stop(held: Looking) {
        let _ = held.stop.send(());
        if let Some(daemon) = held.daemon
            && let Err(error) = daemon.shutdown()
        {
            tracing::debug!(%error, "browsing for Cast receivers did not stop cleanly");
        }
    }

    fn begin(&self, looking: &mut Option<Looking>, first: FirstSearch, browse: bool) -> Result<()> {
        {
            let mut server = self.shared.server.lock();
            if server.is_none() {
                *server =
                    Some(Server::start(self.shared.searching.serving_on).map_err(Error::served)?);
            }
        }

        let (stop, stopped) = unbounded();
        let (daemon, browsing) = match (&self.shared.searching.finding, browse) {
            (Finding::Bonjour, true) => {
                let browsed = |source| Error::Browse {
                    source: Box::new(source),
                };
                let daemon = ServiceDaemon::new().map_err(browsed)?;
                let browsing = daemon.browse(CAST_SERVICE).map_err(browsed)?;
                (Some(daemon), Some(browsing))
            }
            _ => (None, None),
        };

        if first == FirstSearch::Awaited {
            self.shared.first_round(browsing.as_ref(), browse);
        }
        let searched = Arc::clone(&self.shared);
        thread::Builder::new()
            .name("resonate-cast-browse".to_owned())
            .spawn(move || {
                if first == FirstSearch::Behind {
                    searched.first_round(browsing.as_ref(), browse);
                }
                searched.browse_until(&stopped, browsing.as_ref());
            })
            .map_err(spawn_failed)?;
        *looking = Some(Looking {
            stop,
            daemon,
            browsing: browse,
        });
        Ok(())
    }

    pub fn receivers(&self) -> Vec<(SinkId, Arc<Device>)> {
        self.shared
            .known
            .lock()
            .held
            .iter()
            .map(|(id, device)| (*id, Arc::clone(device)))
            .collect()
    }

    pub fn sinks(&self) -> Vec<SinkInfo> {
        let located = self.shared.located.lock().addresses();
        self.receivers()
            .into_iter()
            .map(|(id, device)| {
                let mut sink = sink_of(id, &device);
                if located.contains(&device.at()) && device.id() != &CastId::at(device.at()) {
                    sink.aliases.push(device.address_node_name());
                }
                sink
            })
            .collect()
    }

    pub const fn owns(sink: SinkId) -> bool {
        sink.get() >= RECEIVER_IDS_FROM && sink.get() < RECEIVER_IDS_UNDER
    }

    fn device(&self, sink: SinkId) -> Result<Arc<Device>> {
        self.shared
            .known
            .lock()
            .held
            .get(&sink)
            .map(Arc::clone)
            .ok_or(Error::NoSuchReceiver(sink))
    }
}

impl Neighbours for Casting {
    fn owns(&self, sink: SinkId) -> bool {
        Self::owns(sink)
    }

    fn sinks(&self) -> Vec<SinkInfo> {
        Self::sinks(self)
    }

    fn subscribe(&self) -> Receiver<SinkChange> {
        let (announce, changes) = unbounded();
        self.shared.listeners.lock().push(announce);
        changes
    }

    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let sink = request.target.ok_or(resonate_audio::Error::NoSink)?;
        let device = self.device(sink)?;
        let server = self
            .shared
            .server
            .lock()
            .clone()
            .ok_or(Error::NoSuchReceiver(sink))?;
        stream::open(device, &server, &self.shared.lanes, request, source)
    }

    fn turn_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        let device = self.device(sink)?;
        let _ = self
            .shared
            .turns
            .send(Turn::Volume(device, Volume::heard_at(gain)));
        Ok(())
    }

    fn turn_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        let device = self.device(sink)?;
        let _ = self.shared.turns.send(Turn::Mute(device, muted));
        Ok(())
    }
}

impl Shared {
    fn first_round(&self, browsing: Option<&mdns_sd::Receiver<ServiceEvent>>, browse: bool) {
        if browse {
            self.browse_first(browsing);
        }
        self.reach_located();
    }

    fn browse_first(&self, browsing: Option<&mdns_sd::Receiver<ServiceEvent>>) {
        match (&self.searching.finding, browsing) {
            (Finding::Listed(listed), _) => {
                for found in listed.clone() {
                    let heard_as = found.id.to_string();
                    self.reach(found, Some(heard_as));
                }
            }
            (Finding::Bonjour, Some(browsing)) => {
                let since = Instant::now();
                while let Some(left) = self.searching.within.checked_sub(since.elapsed()) {
                    match browsing.recv_timeout(left) {
                        Ok(event) => self.take(event),
                        Err(_) => return,
                    }
                }
            }
            (Finding::Bonjour, None) => {}
        }
    }

    fn browse_until(
        &self,
        stopped: &Receiver<()>,
        browsing: Option<&mdns_sd::Receiver<ServiceEvent>>,
    ) {
        let mut tried = Instant::now();
        let Some(browsing) = browsing else {
            while let Err(RecvTimeoutError::Timeout) =
                stopped.recv_timeout(ADDRESSES_TRIED_AGAIN_EVERY)
            {
                self.reach_located();
            }
            return;
        };
        while let Err(TryRecvError::Empty) = stopped.try_recv() {
            match browsing.recv_timeout(BROWSE_WAKES_EVERY) {
                Ok(event) => self.take(event),
                Err(mdns_sd::RecvTimeoutError::Timeout) => {}
                Err(mdns_sd::RecvTimeoutError::Disconnected) => return,
            }
            if tried.elapsed() >= ADDRESSES_TRIED_AGAIN_EVERY {
                tried = Instant::now();
                self.reach_located();
            }
        }
    }

    fn reach_located(&self) {
        let given: Vec<Given> = self.located.lock().given.iter().cloned().collect();
        for given in given {
            if let Err(error) = self.reach_given(&given) {
                tracing::debug!(%error, %given, "a Cast receiver given by address could not be reached");
            }
        }
    }

    fn reach_given(&self, given: &Given) -> Result<()> {
        let at = given.resolve(matches!(self.searching.finding, Finding::Bonjour))?;
        let moved_from = {
            let mut located = self.located.lock();
            if !located.given.contains(given) {
                return Ok(());
            }
            match given {
                Given::Address(_) => None,
                named => located
                    .resolved
                    .insert(named.clone(), at)
                    .filter(|before| *before != at)
                    .filter(|before| !located.addresses().contains(before)),
            }
        };
        if let Some(before) = moved_from {
            let gone = self.forget_at(before);
            self.announce(&gone);
        }
        self.reach_address(at)
    }

    fn reach_address(&self, at: SocketAddr) -> Result<()> {
        if self
            .known
            .lock()
            .held
            .values()
            .any(|device| device.at() == at)
        {
            return Ok(());
        }
        let device = Device::reached(Found::at(at))?;
        self.admit(device, None, true);
        Ok(())
    }

    fn forget_unlocated(&self) -> Vec<SinkChange> {
        let located = self.located.lock().addresses();
        let mut known = self.known.lock();
        known.named.clear();
        let gone: Vec<SinkId> = known
            .held
            .iter()
            .filter(|(_, device)| !located.contains(&device.at()))
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            known.held.remove(id);
            known.addressed.remove(id);
        }
        gone.into_iter().map(SinkChange::Removed).collect()
    }

    fn forget_at(&self, at: SocketAddr) -> Vec<SinkChange> {
        let mut known = self.known.lock();
        let browsed: BTreeSet<CastId> = known.named.values().cloned().collect();
        let gone: Vec<SinkId> = known
            .held
            .iter()
            .filter(|(id, device)| {
                device.at() == at && known.addressed.contains(id) && !browsed.contains(device.id())
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            known.held.remove(id);
            known.addressed.remove(id);
        }
        gone.into_iter().map(SinkChange::Removed).collect()
    }

    fn take(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::ServiceResolved(resolved) => {
                if let Some(found) = bonjour::found_in(&resolved) {
                    self.reach(found, Some(resolved.get_fullname().to_owned()));
                }
            }
            ServiceEvent::ServiceRemoved(_, fullname) => self.forget(&fullname),
            _ => {}
        }
    }

    fn reach(&self, found: Found, fullname: Option<String>) {
        let held_here = self
            .known
            .lock()
            .held
            .iter()
            .find(|(_, device)| device.at() == found.at)
            .map(|(sink, device)| (*sink, device.id().clone()));
        if let Some((sink, id)) = held_here {
            if id == found.id {
                self.remember(fullname, &found.id);
                return;
            }
            if self.known.lock().addressed.contains(&sink) {
                self.relabel(sink, found, fullname);
                return;
            }
        }
        let device = match Device::reached(found.clone()) {
            Ok(device) => device,
            Err(error) => {
                tracing::debug!(%error, receiver = %found.id, at = %found.at, "a Cast receiver answering the browse could not be reached");
                return;
            }
        };

        self.admit(device, fullname, false);
    }

    fn remember(&self, fullname: Option<String>, id: &CastId) {
        if let Some(fullname) = fullname {
            self.known.lock().named.insert(fullname, id.clone());
        }
    }

    fn relabel(&self, sink: SinkId, found: Found, fullname: Option<String>) {
        let changed = {
            let mut known = self.known.lock();
            if let Some(fullname) = fullname {
                known.named.insert(fullname, found.id.clone());
            }
            let renamed = known
                .held
                .get(&sink)
                .map(|device| Arc::new(device.renamed(found)));
            renamed.map(|device| {
                known.held.insert(sink, device);
                SinkChange::Reformatted(sink)
            })
        };
        self.announce(changed.as_slice());
    }

    fn admit(&self, device: Device, fullname: Option<String>, by_address: bool) {
        let change = {
            let mut known = self.known.lock();
            if let Some(fullname) = fullname {
                known.named.insert(fullname, device.id().clone());
            }
            let same = known
                .held
                .iter()
                .find(|(_, held)| held.id() == device.id())
                .map(|(id, _)| *id);
            let (id, change) = match same {
                Some(id) => (id, None),
                None => {
                    let id = SinkId::new(RECEIVER_IDS_FROM.saturating_add(known.next));
                    known.next = known.next.saturating_add(1);
                    (id, Some(SinkChange::Added(id)))
                }
            };
            if by_address {
                known.addressed.insert(id);
            }
            known.held.insert(id, Arc::new(device));
            change
        };
        self.announce(change.as_slice());
    }

    fn forget(&self, fullname: &str) {
        let located = self.located.lock().addresses();
        let change = {
            let mut known = self.known.lock();
            let Some(id) = known.named.remove(fullname) else {
                return;
            };
            let held = known
                .held
                .iter()
                .find(|(_, device)| device.id() == &id)
                .map(|(sink, device)| (*sink, device.at()));
            let Some((sink, at)) = held else {
                return;
            };
            if known.addressed.contains(&sink) && located.contains(&at) {
                let reverted = known
                    .held
                    .get(&sink)
                    .map(|device| Arc::new(device.renamed(Found::at(at))));
                reverted.map(|device| {
                    known.held.insert(sink, device);
                    SinkChange::Reformatted(sink)
                })
            } else {
                known.held.remove(&sink);
                known.addressed.remove(&sink);
                Some(SinkChange::Removed(sink))
            }
        };
        self.announce(change.as_slice());
    }

    fn announce(&self, changes: &[SinkChange]) {
        if changes.is_empty() {
            return;
        }
        self.listeners.lock().retain(|listener| {
            changes
                .iter()
                .all(|change| listener.send(change.clone()).is_ok())
        });
    }
}

fn spawn_failed(source: io::Error) -> Error {
    Error::Link {
        at: (Ipv4Addr::UNSPECIFIED, 0).into(),
        op: LinkOp::Spawn,
        source,
    }
}

fn sink_of(id: SinkId, device: &Device) -> SinkInfo {
    let carriage = Device::carriage();
    SinkInfo {
        id,
        name: device.node_name(),
        aliases: Vec::new(),
        description: device.name().to_owned(),
        is_default: false,
        is_hardware: true,
        port: Some(SinkPort {
            description: format!("Cast receiver at {}", device.at().ip()),
            plugged: Plugged::Unsaid,
            hardware_volume: HardwareVolume::Always,
            volume: device.volume().map(Volume::to_gain),
            muted: false,
        }),
        profile: None,
        formats: carriage.formats(),
        allowed_rates: carriage.rates().to_vec(),
        current_rate: None,
    }
}

fn turn_when_told(turned: &Receiver<Turn>) {
    let mut links: BTreeMap<SocketAddr, Link> = BTreeMap::new();
    while let Ok(first) = turned.recv() {
        let mut volumes: BTreeMap<SocketAddr, (Arc<Device>, Volume)> = BTreeMap::new();
        let mut mutes: BTreeMap<SocketAddr, (Arc<Device>, bool)> = BTreeMap::new();
        for turn in iter::once(first).chain(turned.try_iter()) {
            match turn {
                Turn::Volume(device, volume) => {
                    volumes.insert(device.at(), (device, volume));
                }
                Turn::Mute(device, muted) => {
                    mutes.insert(device.at(), (device, muted));
                }
            }
        }

        for (device, volume) in volumes.into_values() {
            turned_on(&mut links, &device, |link| device::set_volume(link, volume));
        }
        for (device, muted) in mutes.into_values() {
            turned_on(&mut links, &device, |link| device::set_mute(link, muted));
        }
    }
}

fn turned_on(
    links: &mut BTreeMap<SocketAddr, Link>,
    device: &Device,
    turn: impl FnOnce(&Link) -> Result<()>,
) {
    let stale = links.get(&device.at()).is_some_and(|link| !link.is_alive());
    if stale {
        links.remove(&device.at());
    }
    let link = match links.entry(device.at()) {
        Entry::Occupied(held) => held.into_mut(),
        Entry::Vacant(free) => match Link::connect(device.at()) {
            Ok(link) => free.insert(link),
            Err(error) => {
                tracing::warn!(%error, receiver = %device.id(), "a Cast receiver's volume could not be turned");
                return;
            }
        },
    };
    if let Err(error) = turn(link) {
        tracing::warn!(%error, receiver = %device.id(), "a Cast receiver's volume could not be turned");
        links.remove(&device.at());
    }
}
