use std::{io, result};

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderOp {
    ReadFolder,
    Search,
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

    #[error("a delivery's extension is one to eight ASCII letters and digits")]
    NotAnExtension,
}

const SERVER_TROUBLE: u16 = 500;

impl Error {
    pub fn is_the_provider_away(&self) -> bool {
        match self {
            Self::Io { .. } | Self::Unwelcome { .. } => true,
            Self::Refused { status, .. } => *status >= SERVER_TROUBLE,
            Self::Unreadable { .. } | Self::TurnedAway { .. } | Self::NotAnExtension => false,
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
            provider,
            op: ProviderOp::Search,
            code: 40,
        };

        assert!(unreachable.is_the_provider_away());
        assert!(down.is_the_provider_away());
        assert!(!not_found.is_the_provider_away());
        assert!(!turned_away.is_the_provider_away());
        assert!(unwelcome.is_the_provider_away());
    }

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
