use std::{io, net::SocketAddr, result};

use resonate_audio::{Error as SinkError, Operation, SinkId};
use resonate_core::StreamSpec;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Asked {
    Status,
    Launch,
    Load,
    Play,
    Pause,
    Stop,
    MediaStatus,
    SetVolume,
    SetMute,
}

impl Asked {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Status | Self::MediaStatus => "GET_STATUS",
            Self::Launch => "LAUNCH",
            Self::Load => "LOAD",
            Self::Play => "PLAY",
            Self::Pause => "PAUSE",
            Self::Stop => "STOP",
            Self::SetVolume | Self::SetMute => "SET_VOLUME",
        }
    }

    const fn operation(self) -> Operation {
        match self {
            Self::Status | Self::MediaStatus => Operation::Connect,
            Self::Launch | Self::Load => Operation::Open,
            Self::Play => Operation::Start,
            Self::Pause => Operation::Pause,
            Self::Stop => Operation::Close,
            Self::SetVolume | Self::SetMute => Operation::Volume,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    LaunchError,
    LoadFailed,
    LoadCancelled,
    InvalidRequest,
    InvalidPlayerState,
}

impl Refusal {
    pub(crate) fn named(kind: &str) -> Option<Self> {
        match kind {
            "LAUNCH_ERROR" => Some(Self::LaunchError),
            "LOAD_FAILED" => Some(Self::LoadFailed),
            "LOAD_CANCELLED" => Some(Self::LoadCancelled),
            "INVALID_REQUEST" => Some(Self::InvalidRequest),
            "INVALID_PLAYER_STATE" => Some(Self::InvalidPlayerState),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkOp {
    Connect,
    Handshake,
    Read,
    Write,
    Spawn,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("the Cast receiver at {at} could not be reached at {op:?}")]
    Link {
        at: SocketAddr,
        op: LinkOp,
        #[source]
        source: io::Error,
    },
    #[error("TLS with the Cast receiver failed")]
    Tls {
        #[source]
        source: Box<rustls::Error>,
    },
    #[error("the connection to the Cast receiver ended before it answered {asked:?}")]
    Closed { asked: Asked },
    #[error("the Cast receiver did not answer {asked:?} in time")]
    Unanswered { asked: Asked },
    #[error("the Cast receiver answered {asked:?} with something this player cannot read")]
    Unreadable { asked: Asked },
    #[error("the Cast receiver refused {asked:?} ({refusal:?})")]
    Refused { asked: Asked, refusal: Refusal },
    #[error("the Cast receiver sent a message of {bytes} bytes, longer than any it may send")]
    TooLong { bytes: usize },
    #[error("the Cast receiver sent a message that is not a CastMessage")]
    NotACastMessage,
    #[error("the network could not be browsed for Cast receivers")]
    Browse {
        #[source]
        source: Box<mdns_sd::Error>,
    },
    #[error("the stream could not be served to the Cast receiver")]
    Serve {
        #[source]
        source: Box<resonate_upnp::Error>,
    },
    #[error("no Cast receiver is known as {0}")]
    NoSuchReceiver(SinkId),
    #[error("the Cast receiver takes no stream of {0}")]
    FormatRefused(StreamSpec),
}

impl Error {
    const fn operation(&self) -> Operation {
        match self {
            Self::Closed { asked }
            | Self::Unanswered { asked }
            | Self::Unreadable { asked }
            | Self::Refused { asked, .. } => asked.operation(),
            Self::Link { .. }
            | Self::Tls { .. }
            | Self::TooLong { .. }
            | Self::NotACastMessage
            | Self::Browse { .. }
            | Self::Serve { .. }
            | Self::NoSuchReceiver(_) => Operation::Connect,
            Self::FormatRefused(_) => Operation::Format,
        }
    }

    pub(crate) fn served(source: resonate_upnp::Error) -> Self {
        Self::Serve {
            source: Box::new(source),
        }
    }
}

impl From<Error> for SinkError {
    fn from(error: Error) -> Self {
        match error {
            Error::NoSuchReceiver(node) => Self::SinkGone { node },
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
    fn a_receiver_gone_is_a_sink_gone_and_a_refused_load_names_its_operation() {
        let gone = SinkError::from(Error::NoSuchReceiver(SinkId::new(7)));
        let refused = SinkError::from(Error::Refused {
            asked: Asked::Load,
            refusal: Refusal::LoadFailed,
        });

        assert!(matches!(gone, SinkError::SinkGone { node } if node == SinkId::new(7)));
        assert!(matches!(
            refused,
            SinkError::Backend {
                op: Operation::Open,
                ..
            }
        ));
        assert_eq!(
            Refusal::named("LOAD_CANCELLED"),
            Some(Refusal::LoadCancelled)
        );
        assert_eq!(Refusal::named("MEDIA_STATUS"), None);
    }
}
