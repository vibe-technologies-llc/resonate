use std::{io, result};

use thiserror::Error;

use crate::{SinkId, StreamState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    Connect,
    Open,
    Format,
    Start,
    Pause,
    Drain,
    Close,
    Volume,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("audio operation {op:?} failed")]
    Backend {
        op: Operation,
        #[source]
        source: io::Error,
    },
    #[error("the audio backend stopped")]
    LoopStopped,
    #[error("the audio backend is disconnected")]
    Disconnected,
    #[error("no audio output is available")]
    NoSink,
    #[error("audio output {node} disappeared")]
    SinkGone { node: SinkId },
    #[error("audio output {node} failed from {previous:?}")]
    StreamFailed { node: SinkId, previous: StreamState },
    #[error("the audio output refused the requested format")]
    FormatRefused,
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128);
    }
}
