use std::{io, path::PathBuf, result};

use resonate_core::SourceId;
use thiserror::Error;

use crate::SidecarOp;

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

    #[error("the {provider} lyric provider refused {op:?} with status {status}")]
    Refused {
        provider: SourceId,
        op: LyricOp,
        status: u16,
    },

    #[error("{op:?} against the {provider} lyric provider produced nothing this build can read")]
    Unreadable { provider: SourceId, op: LyricOp },

    #[error("no lyric provider is named {provider}")]
    NoSuchProvider { provider: SourceId },

    #[error("a synced lyric set carries at least one line with the moment it is sung at")]
    NoLineTimed,

    #[error("a lyric sheet already stands beside the track at {}", path.display())]
    SidecarStands { path: PathBuf },

    #[error("{} is read ahead of any lyric sheet written beside the track", path.display())]
    SidecarOutranked { path: PathBuf },

    #[error("{} has no name a lyric sheet beside it could be named after", track.display())]
    SidecarUnnamed { track: PathBuf },

    #[error("a lyric sheet named after {} would be a longer name than a file may have", track.display())]
    SidecarNameTooLong { track: PathBuf },

    #[error("the lyric sheet at {} could not {op:?}", path.display())]
    SidecarUnwritten {
        path: PathBuf,
        op: SidecarOp,
        #[source]
        cause: io::Error,
    },

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
