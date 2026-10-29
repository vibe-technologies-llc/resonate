use std::{io, result};

use resonate_audio::{Error as SinkError, Operation, SinkId};
use resonate_core::StreamSpec;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    SetAvTransportUri,
    Play,
    Pause,
    Stop,
    GetPositionInfo,
    GetProtocolInfo,
    GetVolume,
    Volume,
    Characteristics,
    SetVolume,
    SetMute,
}

impl Action {
    pub const fn name(self) -> &'static str {
        match self {
            Self::SetAvTransportUri => "SetAVTransportURI",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::GetPositionInfo => "GetPositionInfo",
            Self::GetProtocolInfo => "GetProtocolInfo",
            Self::GetVolume => "GetVolume",
            Self::Volume => "Volume",
            Self::Characteristics => "Characteristics",
            Self::SetVolume => "SetVolume",
            Self::SetMute => "SetMute",
        }
    }

    const fn operation(self) -> Operation {
        match self {
            Self::SetAvTransportUri | Self::GetProtocolInfo => Operation::Open,
            Self::Play => Operation::Start,
            Self::Pause => Operation::Pause,
            Self::Stop => Operation::Close,
            Self::GetPositionInfo => Operation::Connect,
            Self::GetVolume
            | Self::Volume
            | Self::Characteristics
            | Self::SetVolume
            | Self::SetMute => Operation::Volume,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Asked {
    Description,
    Action(Action),
}

impl Asked {
    const fn operation(self) -> Operation {
        match self {
            Self::Description => Operation::Connect,
            Self::Action(action) => action.operation(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchOp {
    Bind,
    Send,
    Receive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServeOp {
    Bind,
    Listen,
    Route,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlacOp {
    Configure,
    Shape,
    Header,
    Fill,
    Frame,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{asked:?} could not reach the renderer")]
    Network {
        asked: Asked,
        #[source]
        source: io::Error,
    },
    #[error("the renderer answered {asked:?} with HTTP status {status}")]
    Status { asked: Asked, status: u16 },
    #[error("the renderer's answer to {asked:?} is not HTTP")]
    NotHttp { asked: Asked },
    #[error("the renderer's answer to {asked:?} is longer than {at_most} bytes")]
    TooLong { asked: Asked, at_most: usize },
    #[error("the renderer's answer to {asked:?} is not UTF-8 text")]
    NotText { asked: Asked },
    #[error("the renderer's answer to {asked:?} is not an XML document")]
    NotADocument {
        asked: Asked,
        #[source]
        source: Box<roxmltree::Error>,
    },
    #[error("the renderer's answer to {action:?} does not say what was asked")]
    Unanswered { action: Action },
    #[error("the renderer refused {action:?} (UPnP error {code:?})")]
    Fault { action: Action, code: Option<u32> },
    #[error("the device description names no media renderer with an AVTransport service")]
    NotARenderer,
    #[error("the renderer takes none of FLAC, WAVE or L16, so it cannot be streamed to")]
    TakesNoCarriage,
    #[error("the stream could not be encoded as FLAC at {op:?}")]
    Encoding { op: FlacOp },
    #[error("the address is not an http address a renderer can be reached at")]
    UnreadableAddress,
    #[error("searching the network failed at {op:?}")]
    Search {
        op: SearchOp,
        #[source]
        source: io::Error,
    },
    #[error("serving the stream failed at {op:?}")]
    Serve {
        op: ServeOp,
        #[source]
        source: io::Error,
    },
    #[error("no renderer is known as {0}")]
    NoSuchRenderer(SinkId),
    #[error("the renderer takes no stream of {0}")]
    FormatRefused(StreamSpec),
}

impl Error {
    const fn operation(&self) -> Operation {
        match self {
            Self::Network { asked, .. }
            | Self::Status { asked, .. }
            | Self::NotHttp { asked }
            | Self::TooLong { asked, .. }
            | Self::NotText { asked }
            | Self::NotADocument { asked, .. } => asked.operation(),
            Self::Unanswered { action } | Self::Fault { action, .. } => action.operation(),
            Self::NotARenderer
            | Self::TakesNoCarriage
            | Self::UnreadableAddress
            | Self::Search { .. }
            | Self::Serve { .. }
            | Self::NoSuchRenderer(_) => Operation::Connect,
            Self::FormatRefused(_) | Self::Encoding { .. } => Operation::Format,
        }
    }
}

impl From<Error> for SinkError {
    fn from(error: Error) -> Self {
        match error {
            Error::NoSuchRenderer(node) => Self::SinkGone { node },
            Error::FormatRefused(_) => Self::FormatRefused,
            error => Self::Backend {
                op: error.operation(),
                source: io::Error::other(error),
            },
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128);
    }

    #[test]
    fn a_renderer_gone_is_a_sink_gone_and_a_refused_play_names_its_operation() {
        let gone = SinkError::from(Error::NoSuchRenderer(SinkId::new(7)));
        let refused = SinkError::from(Error::Fault {
            action: Action::Play,
            code: Some(701),
        });

        assert!(matches!(gone, SinkError::SinkGone { node } if node == SinkId::new(7)));
        assert!(matches!(
            refused,
            SinkError::Backend {
                op: Operation::Start,
                ..
            }
        ));
    }
}
