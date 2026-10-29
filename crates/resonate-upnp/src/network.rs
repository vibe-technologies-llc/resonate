use std::{
    collections::BTreeMap,
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

use crate::{Error, Located, Renderer, Result, ServeOp, serve::Server, ssdp, stream};

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
    _stop: Sender<()>,
}

struct Shared {
    searching: Searching,
    server: Arc<Server>,
    known: Mutex<Known>,
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
        let server = Server::start(searching.serving_on)?;
        let (turns, turned) = unbounded();
        let shared = Arc::new(Shared {
            searching,
            server,
            known: Mutex::new(Known::default()),
            round: Mutex::new(()),
            listeners: Mutex::new(Vec::new()),
            turns,
        });
        let spawn_failed = |source| Error::Serve {
            op: ServeOp::Listen,
            source,
        };

        thread::Builder::new()
            .name("resonate-upnp-turns".to_owned())
            .spawn(move || turn_when_told(&turned))
            .map_err(spawn_failed)?;

        if first == FirstSearch::Awaited {
            shared.round()?;
        }
        let (stop, stopped) = unbounded();
        let searched = Arc::clone(&shared);
        thread::Builder::new()
            .name("resonate-upnp-search".to_owned())
            .spawn(move || searched.search_until(&stopped, first))
            .map_err(spawn_failed)?;

        Ok(Self {
            shared,
            _stop: stop,
        })
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
        stream::open(renderer, &self.shared.server, request, source)
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
        for location in found {
            if let Some((id, _)) = held_at.iter().find(|(_, at)| *at == location) {
                seen.push(*id);
                continue;
            }
            match Renderer::reached(location.clone()) {
                Ok(renderer) => reached.push(renderer),
                Err(error) => {
                    tracing::debug!(%error, %location, "a device answering the search is not a renderer this player can stream to");
                }
            }
        }

        let changes = self.known.lock().settle(reached, &seen);
        if !changes.is_empty() {
            self.listeners.lock().retain(|listener| {
                changes
                    .iter()
                    .all(|change| listener.send(change.clone()).is_ok())
            });
        }
        Ok(())
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

fn sink_of(id: SinkId, renderer: &Renderer) -> SinkInfo {
    let carriage = renderer.carriage();
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
                HardwareVolume::Yes
            } else {
                HardwareVolume::No
            },
            volume: renderer.volume().map(Volume::to_gain),
            muted: false,
        }),
        profile: None,
        formats: carriage.formats(),
        allowed_rates: carriage.rates(),
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
