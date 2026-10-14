use std::{sync::Arc, time::Duration};

use crossbeam_channel::Receiver;
use resonate_audio::{Backend, Opener, SinkResult, Surveyor};
use resonate_core::Gain;

use crate::{
    AudioSource, PipeWire, SinkChange, SinkId, SinkInfo, SinkStream, StreamRequest, Survey,
};

impl Surveyor for Survey {
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        match Self::enumerate_sinks(self, timeout) {
            Err(crate::Error::NoSink) => Ok(Vec::new()),
            found => found.map_err(Into::into),
        }
    }
}

impl Opener for Survey {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        Self::open(self, request, source).map_err(Into::into)
    }
}

impl Backend for PipeWire {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        Self::subscribe_sinks(self)
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(self.survey())
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(self.survey())
    }

    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        Self::set_device_volume(self, sink, gain).map_err(Into::into)
    }

    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        Self::set_device_mute(self, sink, muted).map_err(Into::into)
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Self::shutdown(*self).map_err(Into::into)
    }
}
