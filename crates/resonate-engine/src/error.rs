use std::result;

use resonate_core::{Frames, MediaLocation, Span, StreamSpec, TrackId};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cause {
    Unreadable,
    Unsupported,
    Damaged,
    NoDevice,
    DeviceGone,
    SoundServer,
    DeviceRefused,
    CannotSeek,
    QueueMoved,
    NothingPlaying,
    PlayerStopped,
}

impl Error {
    pub const fn cause(&self) -> Cause {
        match self {
            Self::Decode { source, .. } => cause_of_a_read(source),
            Self::Convert { .. } | Self::Renegotiation { .. } => Cause::DeviceRefused,
            Self::Sink(sink) => cause_at_the_sink(sink),
            Self::NoSuchRow { .. } | Self::NotAnOrder { .. } => Cause::QueueMoved,
            Self::QueueEmpty | Self::InvalidTransition { .. } => Cause::NothingPlaying,
            Self::SeekOutOfRange { .. } => Cause::CannotSeek,
            Self::CommandPending { .. } | Self::EngineStopped => Cause::PlayerStopped,
        }
    }

    pub const fn location(&self) -> Option<&MediaLocation> {
        match self {
            Self::Decode { source, .. } => source.location(),
            Self::Convert { .. }
            | Self::Sink(_)
            | Self::NoSuchRow { .. }
            | Self::NotAnOrder { .. }
            | Self::QueueEmpty
            | Self::InvalidTransition { .. }
            | Self::SeekOutOfRange { .. }
            | Self::Renegotiation { .. }
            | Self::CommandPending { .. }
            | Self::EngineStopped => None,
        }
    }

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

const fn cause_of_a_read(read: &resonate_codec::Error) -> Cause {
    use resonate_codec::Error as Read;

    match read {
        Read::Io { .. }
        | Read::NoSuchSource { .. }
        | Read::LocatorNotUsable { .. }
        | Read::Unwritable { .. } => Cause::Unreadable,
        Read::UnrecognisedContainer { .. }
        | Read::NoAudioTrack { .. }
        | Read::NoDecoder { .. }
        | Read::RateNotRepresentable { .. }
        | Read::LayoutNotRepresentable { .. }
        | Read::SampleFormatNotRepresentable { .. }
        | Read::DsdCompressed { .. } => Cause::Unsupported,
        Read::SeekOutOfRange { .. }
        | Read::NotSeekable { .. }
        | Read::SeekBackwardUnsupported { .. }
        | Read::SeekInvalidTrack { .. } => Cause::CannotSeek,
        Read::TrackPropertyMissing { .. }
        | Read::UnknownDuration { .. }
        | Read::ResetRequired { .. }
        | Read::Symphonia { .. }
        | Read::DsdChunkMissing { .. }
        | Read::DsdFieldNotUsable { .. }
        | Read::SheetTooLarge { .. }
        | Read::TagsUnread { .. }
        | Read::TagsUnwritten { .. }
        | Read::Domain(_) => Cause::Damaged,
    }
}

const fn cause_at_the_sink(sink: &resonate_pipewire::Error) -> Cause {
    use resonate_pipewire::Error as Sink;

    match sink {
        Sink::NoSink => Cause::NoDevice,
        Sink::SinkGone { .. } => Cause::DeviceGone,
        Sink::StreamFailed { .. } | Sink::PodBuild { .. } => Cause::DeviceRefused,
        Sink::Daemon { .. } | Sink::LoopStopped | Sink::Disconnected => Cause::SoundServer,
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
