use std::{result, sync::Arc, time::Duration};

use crossbeam_channel::Receiver;
use resonate_pipewire::{
    AudioSource, Error as SinkError, PipeWire, SinkChange, SinkInfo, SinkStream, StreamRequest,
    Survey,
};

pub type SinkResult<T> = result::Result<T, SinkError>;

pub trait Surveyor: Send + Sync + 'static {
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>>;
}

pub trait Backend: Send + 'static {
    fn subscribe_sinks(&self) -> Receiver<SinkChange>;
    fn surveyor(&self) -> Arc<dyn Surveyor>;
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>)
    -> SinkResult<SinkStream>;
    fn shutdown(self: Box<Self>) -> SinkResult<()>;
}

impl Surveyor for Survey {
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Self::enumerate_sinks(self, timeout)
    }
}

impl Backend for PipeWire {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        Self::subscribe_sinks(self)
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(self.survey())
    }

    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        Self::open(self, request, source)
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Self::shutdown(*self)
    }
}
