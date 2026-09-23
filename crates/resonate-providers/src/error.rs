use std::{io, result};

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderOp {
    ReadFolder,
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

    #[error("a delivery's extension is one to eight ASCII letters and digits")]
    NotAnExtension,
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
