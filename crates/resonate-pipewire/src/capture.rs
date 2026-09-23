use crossbeam_channel::Receiver;
use resonate_core::StreamSpec;

use crate::{NodeName, Result, StreamEvent};

pub trait AudioSink: Send {
    fn take(&mut self, bytes: &[u8]);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    pub name: NodeName,
    pub description: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capturing {
    Desktop { sink: Option<NodeName> },
    Microphone { source: Option<NodeName> },
}

#[derive(Clone, Debug)]
pub struct CaptureRequest {
    pub from: Capturing,
    pub spec: StreamSpec,
    pub media_name: String,
}

pub struct CaptureStream {
    events: Receiver<StreamEvent>,
    stop: Box<dyn Fn() -> Result<()> + Send>,
    stopped: bool,
}

impl CaptureStream {
    pub(crate) fn new(
        events: Receiver<StreamEvent>,
        stop: Box<dyn Fn() -> Result<()> + Send>,
    ) -> Self {
        Self {
            events,
            stop,
            stopped: false,
        }
    }

    pub fn events(&self) -> &Receiver<StreamEvent> {
        &self.events
    }

    pub fn stop(mut self) -> Result<()> {
        self.stopped = true;
        (self.stop)()
    }
}

impl Drop for CaptureStream {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = (self.stop)();
        }
    }
}
