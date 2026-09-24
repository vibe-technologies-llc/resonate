use std::result;

use libspa::pod::serialize::GenError;
use thiserror::Error;

use crate::{SinkId, StreamState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PwOp {
    MainLoopCreate,
    ContextCreate,
    CoreConnect,
    RegistryBind,
    StreamCreate,
    StreamConnect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PodParam {
    EnumFormat,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("pipewire operation {op:?} failed")]
    Daemon {
        op: PwOp,
        #[source]
        source: pipewire::Error,
    },

    #[error("the pipewire loop thread stopped")]
    LoopStopped,

    #[error("the pipewire daemon is not connected")]
    Disconnected,

    #[error("no audio sink is present in the graph")]
    NoSink,

    #[error("sink {node} left the graph")]
    SinkGone { node: SinkId },

    #[error("stream on sink {node} entered the failed state from {previous:?}")]
    StreamFailed { node: SinkId, previous: StreamState },

    #[error("building the {param:?} POD failed")]
    PodBuild {
        param: PodParam,
        #[source]
        source: GenError,
    },
}

impl Error {
    pub fn daemon(op: PwOp, source: pipewire::Error) -> Self {
        Self::Daemon { op, source }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
