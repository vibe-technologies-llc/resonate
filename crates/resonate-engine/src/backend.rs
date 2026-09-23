use std::{result, time::Duration};

use crossbeam_channel::Receiver;
use resonate_pipewire::{
    AudioSource, Error as SinkError, PipeWire, SinkChange, SinkInfo, SinkStream, StreamRequest,
};

pub type SinkResult<T> = result::Result<T, SinkError>;

pub trait Backend: Send + 'static {
    fn subscribe_sinks(&self) -> Receiver<SinkChange>;
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>>;
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>)
    -> SinkResult<SinkStream>;
    fn shutdown(self: Box<Self>) -> SinkResult<()>;
}

impl Backend for PipeWire {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        Self::subscribe_sinks(self)
    }

    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Self::enumerate_sinks(self, timeout)
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
