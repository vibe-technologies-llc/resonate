use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::{Receiver, Select, Sender, unbounded};
use resonate_audio::{
    AudioSource, Backend, Opener, SinkChange, SinkId, SinkInfo, SinkResult, SinkStream,
    StreamRequest, Surveyor,
};
use resonate_core::Gain;

use crate::Network;

pub struct Beside {
    inner: Box<dyn Backend>,
    network: Arc<Network>,
}

impl Beside {
    pub fn over(inner: Box<dyn Backend>, network: Arc<Network>) -> Self {
        Self { inner, network }
    }
}

impl Backend for Beside {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        let graph = self.inner.subscribe_sinks();
        let renderers = self.network.subscribe();
        let (announce, changes) = unbounded();
        let spawned = thread::Builder::new()
            .name("resonate-upnp-sinks".to_owned())
            .spawn(move || forward(&[graph, renderers], &announce));
        if let Err(error) = spawned {
            tracing::warn!(%error, "renderers coming and going will not be told");
        }
        changes
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Surveyed {
            inner: self.inner.surveyor(),
            network: Arc::clone(&self.network),
        })
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Opening {
            inner: self.inner.opener(),
            network: Arc::clone(&self.network),
        })
    }

    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        if Network::owns(sink) {
            return Ok(self.network.turn_volume(sink, gain)?);
        }
        self.inner.set_device_volume(sink, gain)
    }

    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        if Network::owns(sink) {
            return Ok(self.network.turn_mute(sink, muted)?);
        }
        self.inner.set_device_mute(sink, muted)
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        self.inner.shutdown()
    }
}

fn forward(sources: &[Receiver<SinkChange>], announce: &Sender<SinkChange>) {
    let mut open: Vec<&Receiver<SinkChange>> = sources.iter().collect();
    while !open.is_empty() {
        let mut select = Select::new();
        for source in &open {
            select.recv(source);
        }
        let ready = select.select();
        let at = ready.index();
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
    network: Arc<Network>,
}

impl Surveyor for Surveyed {
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        let mut sinks = self.inner.enumerate_sinks(timeout)?;
        sinks.extend(self.network.sinks());
        Ok(sinks)
    }
}

struct Opening {
    inner: Arc<dyn Opener>,
    network: Arc<Network>,
}

impl Opener for Opening {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        match request.target {
            Some(sink) if Network::owns(sink) => self.network.open(request, source),
            Some(_) | None => self.inner.open(request, source),
        }
    }
}
