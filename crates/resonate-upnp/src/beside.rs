use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Select, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Backend, Opener, SinkChange, SinkId, SinkInfo, SinkResult, SinkStream,
    StreamRequest, Surveyor,
};
use resonate_core::Gain;

pub trait Neighbours: Send + Sync {
    fn owns(&self, sink: SinkId) -> bool;

    fn sinks(&self) -> Vec<SinkInfo>;

    fn subscribe(&self) -> Receiver<SinkChange>;

    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>)
    -> SinkResult<SinkStream>;

    fn turn_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()>;

    fn turn_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()>;
}

type Neighbourhood = Vec<Arc<dyn Neighbours>>;

pub struct Beside {
    inner: Box<dyn Backend>,
    neighbours: Arc<Neighbourhood>,
    forwarding: Forwarding,
}

struct Forwarding {
    stop: Option<Sender<()>>,
    stopped: Receiver<()>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Forwarding {
    fn new() -> Self {
        let (stop, stopped) = bounded(0);

        Self {
            stop: Some(stop),
            stopped,
            threads: Mutex::new(Vec::new()),
        }
    }

    fn spawn(&self, sources: Vec<Receiver<SinkChange>>, announce: Sender<SinkChange>) {
        let stopped = self.stopped.clone();
        let spawned = thread::Builder::new()
            .name("resonate-upnp-sinks".to_owned())
            .spawn(move || forward(&sources, &stopped, &announce));

        match spawned {
            Ok(handle) => {
                let mut threads = self.threads.lock();

                threads.retain(|thread| !thread.is_finished());
                threads.push(handle);
            }
            Err(error) => tracing::warn!(%error, "renderers coming and going will not be told"),
        }
    }
}

impl Drop for Forwarding {
    fn drop(&mut self) {
        drop(self.stop.take());

        for thread in self.threads.get_mut().drain(..) {
            let _ = thread.join();
        }
    }
}

impl Beside {
    pub fn over(inner: Box<dyn Backend>, neighbours: Arc<dyn Neighbours>) -> Self {
        Self {
            inner,
            neighbours: Arc::new(vec![neighbours]),
            forwarding: Forwarding::new(),
        }
    }

    pub fn and(self, more: Arc<dyn Neighbours>) -> Self {
        let mut neighbours = self.neighbours.as_ref().clone();

        neighbours.push(more);
        Self {
            inner: self.inner,
            neighbours: Arc::new(neighbours),
            forwarding: self.forwarding,
        }
    }

    fn owner_of(&self, sink: SinkId) -> Option<&Arc<dyn Neighbours>> {
        owner_among(&self.neighbours, sink)
    }
}

fn owner_among(neighbours: &[Arc<dyn Neighbours>], sink: SinkId) -> Option<&Arc<dyn Neighbours>> {
    neighbours.iter().find(|neighbours| neighbours.owns(sink))
}

impl Backend for Beside {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        let mut sources = vec![self.inner.subscribe_sinks()];

        sources.extend(
            self.neighbours
                .iter()
                .map(|neighbours| neighbours.subscribe()),
        );

        let (announce, changes) = unbounded();

        self.forwarding.spawn(sources, announce);
        changes
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Surveyed {
            inner: self.inner.surveyor(),
            neighbours: Arc::clone(&self.neighbours),
        })
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Opening {
            inner: self.inner.opener(),
            neighbours: Arc::clone(&self.neighbours),
        })
    }

    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        if let Some(owner) = self.owner_of(sink) {
            return owner.turn_volume(sink, gain);
        }
        self.inner.set_device_volume(sink, gain)
    }

    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        if let Some(owner) = self.owner_of(sink) {
            return owner.turn_mute(sink, muted);
        }
        self.inner.set_device_mute(sink, muted)
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        self.inner.shutdown()
    }
}

fn forward(
    sources: &[Receiver<SinkChange>],
    stopped: &Receiver<()>,
    announce: &Sender<SinkChange>,
) {
    let mut open: Vec<&Receiver<SinkChange>> = sources.iter().collect();
    while !open.is_empty() {
        let mut select = Select::new();

        select.recv(stopped);
        for source in &open {
            select.recv(source);
        }

        let ready = select.select();
        let Some(at) = ready.index().checked_sub(1) else {
            let _ = ready.recv(stopped);

            return;
        };
        let Some(source) = open.get(at) else {
            return;
        };
        match ready.recv(source) {
            Ok(change) => {
                if announce.send(change).is_err() {
                    return;
                }
            }
            Err(_) => {
                open.remove(at);
            }
        }
    }
}

struct Surveyed {
    inner: Arc<dyn Surveyor>,
    neighbours: Arc<Neighbourhood>,
}

impl Surveyor for Surveyed {
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        let mut sinks = self.inner.enumerate_sinks(timeout)?;

        for neighbours in self.neighbours.iter() {
            sinks.extend(neighbours.sinks());
        }
        Ok(sinks)
    }
}

struct Opening {
    inner: Arc<dyn Opener>,
    neighbours: Arc<Neighbourhood>,
}

impl Opener for Opening {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        match request
            .target
            .and_then(|sink| owner_among(&self.neighbours, sink))
        {
            Some(owner) => owner.open(request, source),
            None => self.inner.open(request, source),
        }
    }
}
