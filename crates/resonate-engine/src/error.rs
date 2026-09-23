use std::result;

use resonate_core::{Frames, Span, StreamSpec, TrackId};
use thiserror::Error;

use crate::{CommandKind, TransportState};

#[derive(Debug, Error)]
pub enum Error {
    #[error("decoding track {track} failed")]
    Decode {
        track: TrackId,
        #[source]
        source: resonate_codec::Error,
    },

    #[error("converting track {track} from {source_spec} to {sink_spec} failed")]
    Convert {
        track: TrackId,
        source_spec: StreamSpec,
        sink_spec: StreamSpec,
        #[source]
        source: resonate_dsp::Error,
    },

    #[error(transparent)]
    Sink(#[from] resonate_pipewire::Error),

    #[error("the queue has no row {rows}; it holds {len}")]
    NoSuchRow { rows: Span, len: usize },

    #[error("{named} rows are not a reordering of the {len} the queue holds")]
    NotAnOrder { named: usize, len: usize },

    #[error("the queue is empty")]
    QueueEmpty,

    #[error("{attempted:?} is not valid while the transport is {state:?}")]
    InvalidTransition {
        state: TransportState,
        attempted: CommandKind,
    },

    #[error("seek to {requested} is past the {duration}-frame duration of track {track}")]
    SeekOutOfRange {
        track: TrackId,
        requested: Frames,
        duration: Frames,
    },

    #[error(
        "the graph would not settle on a format for track {track}; it answered {negotiated} to a request for {requested} after {attempts} rebuilds"
    )]
    Renegotiation {
        track: TrackId,
        requested: StreamSpec,
        negotiated: StreamSpec,
        attempts: u8,
    },

    #[error("the engine has not answered {command:?} yet")]
    CommandPending { command: CommandKind },

    #[error("the engine thread stopped")]
    EngineStopped,
}

impl Error {
    pub const fn track(&self) -> Option<TrackId> {
        match self {
            Self::Decode { track, .. }
            | Self::Convert { track, .. }
            | Self::SeekOutOfRange { track, .. }
            | Self::Renegotiation { track, .. } => Some(*track),
            Self::Sink(_)
            | Self::NoSuchRow { .. }
            | Self::NotAnOrder { .. }
            | Self::QueueEmpty
            | Self::InvalidTransition { .. }
            | Self::CommandPending { .. }
            | Self::EngineStopped => None,
        }
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
