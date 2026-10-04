use std::{io, path::PathBuf, result};

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderOp {
    ReadFolder,
    SignIn,
    Search,
    Playback,
    Download,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} failed in the {provider} provider")]
    Io {
        provider: SourceId,
        op: ProviderOp,
        #[source]
        source: io::Error,
    },

    #[error("{op:?} was refused by the {provider} provider's server with status {status}")]
    Refused {
        provider: SourceId,
        op: ProviderOp,
        status: u16,
    },

    #[error("{op:?} was answered by the {provider} provider's server with something unreadable")]
    Unreadable { provider: SourceId, op: ProviderOp },

    #[error("{op:?} was turned away by the {provider} provider's server with its error {code}")]
    TurnedAway {
        provider: SourceId,
        op: ProviderOp,
        code: u16,
    },

    #[error(
        "{op:?} found the listener unwelcome at the {provider} provider's server, its error {code}"
    )]
    Unwelcome {
        provider: SourceId,
        op: ProviderOp,
        code: u16,
    },

    #[error("{op:?} was answered by the {provider} provider's server with media off its own hosts")]
    OffItsHosts { provider: SourceId, op: ProviderOp },

    #[error("{op:?} was still queued at the {provider} provider's server when it was given up")]
    StillQueued { provider: SourceId, op: ProviderOp },

    #[error(
        "{op:?} reached the {provider} provider's server, whose certificate this build does not trust"
    )]
    Untrusted { provider: SourceId, op: ProviderOp },

    #[error("{file:?} is still arriving in the {provider} provider and is asked for again later")]
    StillArriving { provider: SourceId, file: PathBuf },

    #[error("the sign-in to the {provider} provider lapsed before it was approved")]
    AuthorizationLapsed { provider: SourceId },

    #[error("the sign-in to the {provider} provider was turned down")]
    AuthorizationDenied { provider: SourceId },

    #[error("a delivery's extension is one to eight ASCII letters and digits")]
    NotAnExtension,
}

const SERVER_TROUBLE: u16 = 500;

impl Error {
    pub fn is_the_provider_away(&self) -> bool {
        match self {
            Self::Io { .. }
            | Self::Unwelcome { .. }
            | Self::StillQueued { .. }
            | Self::Untrusted { .. } => true,
            Self::Refused { status, .. } => *status >= SERVER_TROUBLE,
            Self::Unreadable { .. }
            | Self::TurnedAway { .. }
            | Self::OffItsHosts { .. }
            | Self::StillArriving { .. }
            | Self::AuthorizationLapsed { .. }
            | Self::AuthorizationDenied { .. }
            | Self::NotAnExtension => false,
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_that_cannot_be_reached_or_whose_server_fails_is_away_and_one_refusing_a_want_is_not()
     {
        let provider = SourceId::new("shop").expect("a nameable source");
        let unreachable = Error::Io {
            provider: provider.clone(),
            op: ProviderOp::Search,
            source: io::Error::from(io::ErrorKind::ConnectionRefused),
        };
        let down = Error::Refused {
            provider: provider.clone(),
            op: ProviderOp::Search,
            status: 503,
        };
        let not_found = Error::Refused {
            provider: provider.clone(),
            op: ProviderOp::Download,
            status: 404,
        };
        let turned_away = Error::TurnedAway {
            provider: provider.clone(),
            op: ProviderOp::Search,
            code: 70,
        };
        let unwelcome = Error::Unwelcome {
            provider: provider.clone(),
            op: ProviderOp::Search,
            code: 40,
        };
        let queued = Error::StillQueued {
            provider: provider.clone(),
            op: ProviderOp::Playback,
        };
        let untrusted = Error::Untrusted {
            provider,
            op: ProviderOp::Search,
        };

        assert!(unreachable.is_the_provider_away());
        assert!(down.is_the_provider_away());
        assert!(!not_found.is_the_provider_away());
        assert!(!turned_away.is_the_provider_away());
        assert!(unwelcome.is_the_provider_away());
        assert!(queued.is_the_provider_away());
        assert!(untrusted.is_the_provider_away());
    }

    #[test]
    fn a_file_still_arriving_is_about_the_want_and_never_the_provider_being_away() {
        let arriving = Error::StillArriving {
            provider: SourceId::new("inbox").expect("a nameable source"),
            file: PathBuf::from("/inbox/echoes.flac"),
        };

        assert!(!arriving.is_the_provider_away());
    }

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
