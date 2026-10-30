use std::{
    collections::BTreeMap,
    io, mem,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
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
    Error, Located, Renderer, Result, ServeOp, Vouched, carriage,
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
    looking: Mutex<Option<Sender<()>>>,
}

struct Shared {
    searching: Searching,
    server: Mutex<Option<Arc<Server>>>,
    holders: Holders,
    known: Mutex<Known>,
    vouched: Mutex<Vouched>,
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
        *self.shared.vouched.lock() = vouched;
    }

    pub fn look(&self, first: FirstSearch) -> Result<()> {
        let mut looking = self.looking.lock();
        if looking.is_some() {
            return Ok(());
        }
        {
            let mut server = self.shared.server.lock();
            if server.is_none() {
                *server = Some(Server::start(self.shared.searching.serving_on)?);
            }
        }

        if first == FirstSearch::Awaited {
            self.shared.round()?;
        }
        let (stop, stopped) = unbounded();
        let searched = Arc::clone(&self.shared);
        thread::Builder::new()
            .name("resonate-upnp-search".to_owned())
            .spawn(move || searched.search_until(&stopped, first))
            .map_err(spawn_failed)?;
        *looking = Some(stop);
        Ok(())
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
        self.looking.lock().is_some()
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
        let found = ssdp::search(self.searching.to, self.searching.within)?;

        let held_at: Vec<(SinkId, Located)> = self
            .known
            .lock()
            .held
            .iter()
            .map(|(id, held)| (*id, held.renderer.location().clone()))
            .collect();
        let mut seen: Vec<SinkId> = Vec::new();
        let mut reached = Vec::new();
        let vouched = self.vouched.lock().clone();
        for location in found {
            if let Some((id, _)) = held_at.iter().find(|(_, at)| *at == location) {
                seen.push(*id);
                continue;
            }
            match Renderer::reached(location.clone(), &vouched) {
                Ok(renderer) => reached.push(renderer),
                Err(error) => {
                    tracing::debug!(%error, %location, "a device answering the search is not a renderer this player can stream to");
                }
            }
        }

        let changes = self.known.lock().settle(reached, &seen);
        self.announce(&changes);
        Ok(())
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
    fn settle(&mut self, reached: Vec<Renderer>, seen: &[SinkId]) -> Vec<SinkChange> {
        let mut changes = Vec::new();
        let mut kept: Vec<SinkId> = seen.to_vec();
        for renderer in reached {
            let same = self
                .held
                .iter()
                .find(|(_, held)| held.renderer.udn() == renderer.udn())
                .map(|(id, _)| *id);
            let id = if let Some(id) = same {
                id
            } else {
                let id = SinkId::new(RENDERER_IDS_FROM.saturating_add(self.next));
                self.next = self.next.saturating_add(1);
                changes.push(SinkChange::Added(id));
                id
            };
            self.held.insert(
                id,
                Held {
                    renderer: Arc::new(renderer),
                    missed: 0,
                },
            );
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
