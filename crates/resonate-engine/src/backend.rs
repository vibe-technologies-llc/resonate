use std::{result, sync::Arc, time::Duration};

use crossbeam_channel::Receiver;
use resonate_core::Gain;
use resonate_pipewire::{
    AudioSource, Error as SinkError, PipeWire, ProfileIndex, SinkChange, SinkId, SinkInfo,
    SinkStream, StreamRequest, Survey,
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
    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()>;
    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()>;
    fn set_card_profile(&self, sink: SinkId, profile: ProfileIndex) -> SinkResult<()>;
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

    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        Self::set_device_volume(self, sink, gain)
    }

    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        Self::set_device_mute(self, sink, muted)
    }

    fn set_card_profile(&self, sink: SinkId, profile: ProfileIndex) -> SinkResult<()> {
        Self::set_card_profile(self, sink, profile)
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Self::shutdown(*self)
    }
}
