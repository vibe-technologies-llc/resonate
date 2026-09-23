use std::{io, path::PathBuf, result};

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EqOp {
    Index,
    Fetch,
    Parse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StoreOp {
    Read,
    Write,
    Walk,
    Remove,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("the {provider} correction source could not be reached")]
    Unreachable {
        provider: SourceId,
        #[source]
        cause: io::Error,
    },

    #[error("{op:?} against the {provider} correction source produced nothing this build can read")]
    Unreadable { provider: SourceId, op: EqOp },

    #[error("no correction source is named {provider}")]
    NoSuchProvider { provider: SourceId },

    #[error("{op:?} on {} could not be carried out", path.display())]
    Store {
        path: PathBuf,
        op: StoreOp,
        #[source]
        source: io::Error,
    },

    #[error("{} holds neither a parametric nor a graphic equaliser", path.display())]
    NotAProfile { path: PathBuf },

    #[error("no profile is kept under that name")]
    NoSuchProfile,

    #[error("a profile is named in printable characters, without a path separator")]
    NameNotUsable,

    #[error("no file can be named after that device, so its own curve cannot be kept")]
    DeviceNotNameable,

    #[error("{op:?} answered more than the {limit} bytes a profile may hold")]
    TooLarge { op: EqOp, limit: usize },

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
