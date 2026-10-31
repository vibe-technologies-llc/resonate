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
    CastId, Device, Error, Found, LinkOp, Result,
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
    located: Mutex<BTreeSet<SocketAddr>>,
    listeners: Mutex<Vec<Sender<SinkChange>>>,
    turns: Sender<Turn>,
}

#[derive(Default)]
struct Known {
    next: u32,
    held: BTreeMap<SinkId, Arc<Device>>,
    named: BTreeMap<String, CastId>,
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
            located: Mutex::new(BTreeSet::new()),
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
        if self.shared.located.lock().is_empty() {
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

    pub fn locate(&self, at: SocketAddr) -> Result<()> {
        self.shared.located.lock().insert(at);
        if self.is_reaching() {
            self.shared.reach_address(at)?;
        }
        Ok(())
    }

    pub fn unlocate(&self, at: SocketAddr) {
        if !self.shared.located.lock().remove(&at) {
            return;
        }
        let gone = self.shared.forget_at(at);
        self.shared.announce(&gone);
    }

    pub fn located(&self) -> Vec<SocketAddr> {
        self.shared.located.lock().iter().copied().collect()
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
        self.receivers()
            .into_iter()
            .map(|(id, device)| sink_of(id, &device))
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
                    self.reach(found, None);
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
        let addresses: Vec<SocketAddr> = self.located.lock().iter().copied().collect();
        for at in addresses {
            if let Err(error) = self.reach_address(at) {
                tracing::debug!(%error, %at, "a Cast receiver given by address could not be reached");
            }
        }
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
        self.admit(device, None);
        Ok(())
    }

    fn forget_unlocated(&self) -> Vec<SinkChange> {
        let located = self.located.lock().clone();
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
        }
        gone.into_iter().map(SinkChange::Removed).collect()
    }

    fn forget_at(&self, at: SocketAddr) -> Vec<SinkChange> {
        let mut known = self.known.lock();
        let gone: Vec<SinkId> = known
            .held
            .iter()
            .filter(|(_, device)| device.at() == at && device.id() == &CastId::at(at))
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            known.held.remove(id);
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
        if fullname.is_some() && self.located.lock().contains(&found.at) {
            return;
        }
        let known_at = self
            .known
            .lock()
            .held
            .values()
            .any(|device| device.id() == &found.id && device.at() == found.at);
        if known_at {
            return;
        }
        let device = match Device::reached(found.clone()) {
            Ok(device) => device,
            Err(error) => {
                tracing::debug!(%error, receiver = %found.id, at = %found.at, "a Cast receiver answering the browse could not be reached");
                return;
            }
        };

        self.admit(device, fullname);
    }

    fn admit(&self, device: Device, fullname: Option<String>) {
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
            known.held.insert(id, Arc::new(device));
            change
        };
        self.announce(change.as_slice());
    }

    fn forget(&self, fullname: &str) {
        let change = {
            let mut known = self.known.lock();
            let Some(id) = known.named.remove(fullname) else {
                return;
            };
            let gone = known
                .held
                .iter()
                .find(|(_, device)| device.id() == &id)
                .map(|(sink, _)| *sink);
            gone.map(|sink| {
                known.held.remove(&sink);
                SinkChange::Removed(sink)
            })
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
    let mut links: BTreeMap<CastId, Link> = BTreeMap::new();
    while let Ok(first) = turned.recv() {
        let mut volumes: BTreeMap<CastId, (Arc<Device>, Volume)> = BTreeMap::new();
        let mut mutes: BTreeMap<CastId, (Arc<Device>, bool)> = BTreeMap::new();
        for turn in iter::once(first).chain(turned.try_iter()) {
            match turn {
                Turn::Volume(device, volume) => {
                    volumes.insert(device.id().clone(), (device, volume));
                }
                Turn::Mute(device, muted) => {
                    mutes.insert(device.id().clone(), (device, muted));
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
    links: &mut BTreeMap<CastId, Link>,
    device: &Device,
    turn: impl FnOnce(&Link) -> Result<()>,
) {
    let stale = links
        .get(device.id())
        .is_some_and(|link| !link.is_alive() || link.at() != device.at());
    if stale {
        links.remove(device.id());
    }
    let link = match links.entry(device.id().clone()) {
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
        links.remove(device.id());
    }
}
