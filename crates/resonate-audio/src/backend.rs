use std::{result, sync::Arc, time::Duration};

use crossbeam_channel::Receiver;
use resonate_core::Gain;

use crate::{
    AudioSource, Error as SinkError, SinkChange, SinkId, SinkInfo, SinkStream, StreamRequest,
};

pub type SinkResult<T> = result::Result<T, SinkError>;

pub trait Surveyor: Send + Sync + 'static {
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>>;
}

pub trait Opener: Send + Sync + 'static {
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>)
    -> SinkResult<SinkStream>;
}

pub trait Backend: Send + 'static {
    fn subscribe_sinks(&self) -> Receiver<SinkChange>;
    fn surveyor(&self) -> Arc<dyn Surveyor>;
    fn opener(&self) -> Arc<dyn Opener>;
    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()>;
    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()>;
    fn shutdown(self: Box<Self>) -> SinkResult<()>;
}
