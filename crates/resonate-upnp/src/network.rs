use std::{
    collections::{BTreeMap, BTreeSet},
    io, mem,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, HardwareVolume, Plugged, SinkChange, SinkId, SinkInfo, SinkPort, SinkResult,
    SinkStream, StreamRequest,
};
use resonate_core::{Gain, Volume};

use crate::{
    Error, Located, Neighbours, Renderer, Result, ServeOp, Vouched, carriage,
    serve::Server,
    ssdp,
    stream::{self, Holders},
};

const RENDERER_IDS_FROM: u32 = 0xF000_0000;

const SEARCHES_MISSED_BEFORE_GONE: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Searching {
    pub to: SocketAddr,
    pub within: Duration,
    pub every: Duration,
    pub serving_on: IpAddr,
}

impl Searching {
    pub const fn on_the_network() -> Self {
        Self {
            to: ssdp::SSDP,
            within: Duration::from_secs(2),
            every: Duration::from_secs(60),
            serving_on: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstSearch {
    Awaited,
    Behind,
}

pub struct Network {
    shared: Arc<Shared>,
    looking: Mutex<Option<Looking>>,
}

struct Looking {
    _stop: Sender<()>,
    multicast: bool,
}

struct Shared {
    searching: Searching,
    server: Mutex<Option<Arc<Server>>>,
    holders: Holders,
    known: Mutex<Known>,
    vouched: Mutex<Vouched>,
    located: Mutex<BTreeSet<Located>>,
    multicasting: AtomicBool,
    round: Mutex<()>,
    listeners: Mutex<Vec<Sender<SinkChange>>>,
    turns: Sender<Turn>,
}

#[derive(Default)]
struct Known {
    next: u32,
    held: BTreeMap<SinkId, Held>,
}

struct Held {
    renderer: Arc<Renderer>,
    aliases: BTreeSet<Located>,
    heard: bool,
    missed: u8,
}

enum Turn {
    Volume(Arc<Renderer>, Volume),
    Mute(Arc<Renderer>, bool),
}

impl Network {
    pub fn start(searching: Searching, first: FirstSearch) -> Result<Self> {
        let network = Self::resting(searching)?;
        network.look(first)?;
        Ok(network)
    }

    pub fn resting(searching: Searching) -> Result<Self> {
        let (turns, turned) = unbounded();
        let shared = Arc::new(Shared {
            searching,
            server: Mutex::new(None),
            holders: Holders::default(),
            known: Mutex::new(Known::default()),
            vouched: Mutex::new(Vouched::default()),
            located: Mutex::new(BTreeSet::new()),
            multicasting: AtomicBool::new(false),
            round: Mutex::new(()),
            listeners: Mutex::new(Vec::new()),
            turns,
        });

        thread::Builder::new()
            .name("resonate-upnp-turns".to_owned())
            .spawn(move || turn_when_told(&turned))
            .map_err(spawn_failed)?;

        Ok(Self {
            shared,
            looking: Mutex::new(None),
        })
    }

    pub fn vouch(&self, vouched: Vouched) {
        let _one_at_a_time = self.shared.round.lock();
        let previous = mem::replace(&mut *self.shared.vouched.lock(), vouched);
        let changes = self.shared.described_again_where_vouching_moved(&previous);
        self.shared.announce(&changes);
    }

    pub fn locate(&self, location: Located) -> Result<()> {
        self.shared.located.lock().insert(location.clone());
        if self.is_reaching() {
            self.shared.reach(&location)?;
        }
        Ok(())
    }

    pub fn unlocate(&self, location: &Located) {
        if !self.shared.located.lock().remove(location) {
            return;
        }
        let gone = self.shared.known.lock().retire_unheard_at(location);
        self.shared.announce(&gone);
    }

    pub fn located(&self) -> Vec<Located> {
        self.shared.located.lock().iter().cloned().collect()
    }

    pub fn look(&self, first: FirstSearch) -> Result<()> {
        let mut looking = self.looking.lock();
        match looking.as_mut() {
            Some(Looking {
                multicast: true, ..
            }) => return Ok(()),
            Some(held) => {
                held.multicast = true;
                self.shared.multicasting.store(true, Ordering::Release);
                drop(looking);
                return self.search_once(first);
            }
            None => {}
        }
        self.start_server()?;
        self.shared.multicasting.store(true, Ordering::Release);
        *looking = Some(self.begin_rounds(first, true)?);
        Ok(())
    }

    pub fn look_at_addresses(&self, first: FirstSearch) -> Result<()> {
        let mut looking = self.looking.lock();
        if looking.is_some() {
            return Ok(());
        }
        self.start_server()?;
        self.shared.multicasting.store(false, Ordering::Release);
        *looking = Some(self.begin_rounds(first, false)?);
        Ok(())
    }

    pub fn stop_multicasting(&self) {
        let mut looking = self.looking.lock();
        let Some(held) = looking.as_mut().filter(|held| held.multicast) else {
            return;
        };
        if self.shared.located.lock().is_empty() {
            drop(looking);
            self.rest();
            return;
        }
        held.multicast = false;
        self.shared.multicasting.store(false, Ordering::Release);
        drop(looking);
        let _one_at_a_time = self.shared.round.lock();
        let gone = self.shared.retire_unlocated();
        self.shared.announce(&gone);
    }

    pub fn rest(&self) {
        if self.looking.lock().take().is_none() {
            return;
        }
        let _one_at_a_time = self.shared.round.lock();
        let gone: Vec<SinkChange> = mem::take(&mut self.shared.known.lock().held)
            .into_keys()
            .map(SinkChange::Removed)
            .collect();
        self.shared.announce(&gone);
    }

    pub fn is_looking(&self) -> bool {
        self.looking
            .lock()
            .as_ref()
            .is_some_and(|held| held.multicast)
    }

    pub fn is_reaching(&self) -> bool {
        self.looking.lock().is_some()
    }

    fn start_server(&self) -> Result<()> {
        let mut server = self.shared.server.lock();
        if server.is_none() {
            *server = Some(Server::start(self.shared.searching.serving_on)?);
        }
        Ok(())
    }

    fn begin_rounds(&self, first: FirstSearch, multicast: bool) -> Result<Looking> {
        if first == FirstSearch::Awaited {
            self.shared.round()?;
        }
        let (stop, stopped) = unbounded();
        let searched = Arc::clone(&self.shared);
        thread::Builder::new()
            .name("resonate-upnp-search".to_owned())
            .spawn(move || searched.search_until(&stopped, first))
            .map_err(spawn_failed)?;
        Ok(Looking {
            _stop: stop,
            multicast,
        })
    }

    fn search_once(&self, first: FirstSearch) -> Result<()> {
        match first {
            FirstSearch::Awaited => self.shared.round(),
            FirstSearch::Behind => {
                let searched = Arc::clone(&self.shared);
                thread::Builder::new()
                    .name("resonate-upnp-search".to_owned())
                    .spawn(move || searched.round_or_warn())
                    .map(drop)
                    .map_err(spawn_failed)
            }
        }
    }

    pub fn renderers(&self) -> Vec<(SinkId, Arc<Renderer>)> {
        self.shared
            .known
            .lock()
            .held
            .iter()
            .map(|(id, held)| (*id, Arc::clone(&held.renderer)))
            .collect()
    }

    pub fn sinks(&self) -> Vec<SinkInfo> {
        self.renderers()
            .into_iter()
            .map(|(id, renderer)| sink_of(id, &renderer))
            .collect()
    }

    pub const fn owns(sink: SinkId) -> bool {
        sink.get() >= RENDERER_IDS_FROM
    }

    pub(crate) fn subscribe(&self) -> Receiver<SinkChange> {
        let (announce, changes) = unbounded();
        self.shared.listeners.lock().push(announce);
        changes
    }

    pub(crate) fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let sink = request.target.ok_or(resonate_audio::Error::NoSink)?;
        let renderer = self.renderer(sink)?;
        let server = self
            .shared
            .server
            .lock()
            .clone()
            .ok_or(Error::NoSuchRenderer(sink))?;
        stream::open(renderer, &server, &self.shared.holders, request, source)
    }

    pub(crate) fn turn_volume(&self, sink: SinkId, gain: Gain) -> Result<()> {
        let renderer = self.renderer(sink)?;
        let _ = self
            .shared
            .turns
            .send(Turn::Volume(renderer, Volume::heard_at(gain)));
        Ok(())
    }

    pub(crate) fn turn_mute(&self, sink: SinkId, muted: bool) -> Result<()> {
        let renderer = self.renderer(sink)?;
        let _ = self.shared.turns.send(Turn::Mute(renderer, muted));
        Ok(())
    }

    fn renderer(&self, sink: SinkId) -> Result<Arc<Renderer>> {
        self.shared
            .known
            .lock()
            .held
            .get(&sink)
            .map(|held| Arc::clone(&held.renderer))
            .ok_or(Error::NoSuchRenderer(sink))
    }
}

impl Neighbours for Network {
    fn owns(&self, sink: SinkId) -> bool {
        Self::owns(sink)
    }

    fn sinks(&self) -> Vec<SinkInfo> {
        Self::sinks(self)
    }

    fn subscribe(&self) -> Receiver<SinkChange> {
        Self::subscribe(self)
    }

    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        Self::open(self, request, source)
    }

    fn turn_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        Ok(Self::turn_volume(self, sink, gain)?)
    }

    fn turn_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        Ok(Self::turn_mute(self, sink, muted)?)
    }
}

impl Shared {
    fn search_until(&self, stopped: &Receiver<()>, first: FirstSearch) {
        if first == FirstSearch::Behind {
            self.round_or_warn();
        }
        while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(self.searching.every) {
            self.round_or_warn();
        }
    }

    fn round_or_warn(&self) {
        if let Err(error) = self.round() {
            tracing::warn!(%error, "the network could not be searched for renderers");
        }
    }

    fn round(&self) -> Result<()> {
        let _one_at_a_time = self.round.lock();
        let addresses = self.located();
        let found = match self
            .multicasting
            .load(Ordering::Acquire)
            .then(|| ssdp::search(self.searching.to, self.searching.within))
        {
            None => None,
            Some(Ok(found)) => Some(found),
            Some(Err(error)) if !addresses.is_empty() => {
                tracing::warn!(%error, "the network could not be searched, the renderers given by address are still reached");
                None
            }
            Some(Err(error)) => return Err(error),
        };
        let heard: &[Located] = found.as_deref().unwrap_or_default();

        let mut seen: Vec<(SinkId, bool)> = Vec::new();
        let mut reached = Vec::new();
        let vouched = self.vouched.lock().clone();
        let by_address = addresses.iter().filter(|at| !heard.contains(at));
        for location in heard.iter().chain(by_address) {
            let sighted = heard.contains(location);
            if let Some(id) = self.known.lock().holding_at(location) {
                seen.push((id, sighted));
                continue;
            }
            match Renderer::reached(location.clone(), &vouched) {
                Ok(renderer) => reached.push((renderer, sighted)),
                Err(error) => {
                    tracing::debug!(%error, %location, "a device at an address is not a renderer this player can stream to");
                }
            }
        }

        let mut known = self.known.lock();
        if found.is_none() {
            seen.extend(known.held.keys().map(|id| (*id, false)));
        }
        let changes = known.settle(reached, &seen, found.is_some());
        drop(known);
        self.announce(&changes);
        Ok(())
    }

    fn located(&self) -> Vec<Located> {
        self.located.lock().iter().cloned().collect()
    }

    fn reach(&self, location: &Located) -> Result<()> {
        if self.known.lock().holding_at(location).is_some() {
            return Ok(());
        }
        let vouched = self.vouched.lock().clone();
        let renderer = Renderer::reached(location.clone(), &vouched)?;
        let change = self.known.lock().admit(renderer, false).1;
        self.announce(&change.into_iter().collect::<Vec<_>>());
        Ok(())
    }

    fn described_again_where_vouching_moved(&self, previous: &Vouched) -> Vec<SinkChange> {
        let vouched = self.vouched.lock().clone();
        let moved: Vec<(SinkId, Located, bool)> = self
            .known
            .lock()
            .held
            .iter()
            .filter(|(_, held)| {
                let udn = held.renderer.udn();
                previous.flac_unsaid_for(udn) != vouched.flac_unsaid_for(udn)
            })
            .map(|(id, held)| (*id, held.renderer.location().clone(), held.heard))
            .collect();
        let mut changes = Vec::new();
        for (id, location, heard) in moved {
            match Renderer::reached(location.clone(), &vouched) {
                Ok(renderer) => {
                    let (admitted, _) = self.known.lock().admit(renderer, heard);
                    changes.push(SinkChange::Reformatted(admitted));
                }
                Err(error) => {
                    tracing::warn!(%error, %location, sink = id.get(), "a renderer could not be described again after the vouching moved");
                }
            }
        }
        changes
    }

    fn retire_unlocated(&self) -> Vec<SinkChange> {
        let located = self.located();
        let mut known = self.known.lock();
        let gone: Vec<SinkId> = known
            .held
            .iter()
            .filter(|(_, held)| {
                !located
                    .iter()
                    .any(|at| held.renderer.location() == at || held.aliases.contains(at))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            known.held.remove(id);
        }
        gone.into_iter().map(SinkChange::Removed).collect()
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

impl Known {
    fn holding_at(&self, location: &Located) -> Option<SinkId> {
        self.held
            .iter()
            .find(|(_, held)| {
                held.renderer.location() == location || held.aliases.contains(location)
            })
            .map(|(id, _)| *id)
    }

    fn admit(&mut self, renderer: Renderer, heard: bool) -> (SinkId, Option<SinkChange>) {
        let same = self
            .held
            .iter()
            .find(|(_, held)| held.renderer.udn() == renderer.udn())
            .map(|(id, _)| *id);
        let (id, change, mut aliases) =
            match same.and_then(|id| self.held.remove(&id).map(|held| (id, held))) {
                Some((id, held)) => {
                    let mut aliases = held.aliases;
                    if held.renderer.location() != renderer.location() {
                        aliases.insert(held.renderer.location().clone());
                    }
                    (id, None, aliases)
                }
                None => {
                    let id = SinkId::new(RENDERER_IDS_FROM.saturating_add(self.next));
                    self.next = self.next.saturating_add(1);
                    (id, Some(SinkChange::Added(id)), BTreeSet::new())
                }
            };
        aliases.remove(renderer.location());
        self.held.insert(
            id,
            Held {
                renderer: Arc::new(renderer),
                aliases,
                heard,
                missed: 0,
            },
        );
        (id, change)
    }

    fn settle(
        &mut self,
        reached: Vec<(Renderer, bool)>,
        seen: &[(SinkId, bool)],
        searched: bool,
    ) -> Vec<SinkChange> {
        let mut changes = Vec::new();
        let mut kept: Vec<SinkId> = seen.iter().map(|(id, _)| *id).collect();
        if searched {
            for (id, heard) in seen {
                if let Some(held) = self.held.get_mut(id) {
                    held.heard = *heard;
                }
            }
        }
        for (renderer, heard) in reached {
            let (id, change) = self.admit(renderer, heard);
            changes.extend(change);
            kept.push(id);
        }
        self.held.retain(|id, held| {
            if kept.contains(id) {
                held.missed = 0;
                return true;
            }
            held.missed = held.missed.saturating_add(1);
            if held.missed >= SEARCHES_MISSED_BEFORE_GONE {
                changes.push(SinkChange::Removed(*id));
                return false;
            }
            true
        });
        changes
    }

    fn retire_unheard_at(&mut self, location: &Located) -> Vec<SinkChange> {
        let Some(id) = self.holding_at(location) else {
            return Vec::new();
        };
        if self.held.get(&id).is_some_and(|held| held.heard) {
            return Vec::new();
        }
        self.held.remove(&id);
        vec![SinkChange::Removed(id)]
    }
}

fn spawn_failed(source: io::Error) -> Error {
    Error::Serve {
        op: ServeOp::Listen,
        source,
    }
}

fn sink_of(id: SinkId, renderer: &Renderer) -> SinkInfo {
    let (formats, allowed_rates) = carriage::unite(renderer.carriages());
    SinkInfo {
        id,
        name: renderer.node_name(),
        description: renderer.name().to_owned(),
        is_default: false,
        is_hardware: true,
        port: Some(SinkPort {
            description: format!("UPnP renderer at {}", renderer.location().host()),
            plugged: Plugged::Unsaid,
            hardware_volume: if renderer.turns_its_own_volume() {
                HardwareVolume::Always
            } else {
                HardwareVolume::No
            },
            volume: renderer.volume().map(Volume::to_gain),
            muted: false,
        }),
        profile: None,
        formats,
        allowed_rates,
        current_rate: None,
    }
}

fn turn_when_told(turned: &Receiver<Turn>) {
    for turn in turned {
        let (renderer, turned) = match &turn {
            Turn::Volume(renderer, volume) => (renderer, renderer.set_volume(*volume)),
            Turn::Mute(renderer, muted) => (renderer, renderer.set_mute(*muted)),
        };
        if let Err(error) = turned {
            tracing::warn!(%error, renderer = %renderer.udn(), "a renderer's volume could not be turned");
        }
    }
}
