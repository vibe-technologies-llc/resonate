use std::{io, result};

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LyricOp {
    Search,
    Fetch,
    Parse,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("the {provider} lyric provider could not be reached")]
    Unreachable {
        provider: SourceId,
        #[source]
        cause: io::Error,
    },

    #[error("{op:?} against the {provider} lyric provider produced nothing this build can read")]
    Unreadable { provider: SourceId, op: LyricOp },

    #[error("no lyric provider is named {provider}")]
    NoSuchProvider { provider: SourceId },

    #[error("a synced lyric line carries the moment it is sung at")]
    LineNotTimed,

    #[error(transparent)]
    Domain(#[from] resonate_core::Error),
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
