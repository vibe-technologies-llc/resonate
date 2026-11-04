use std::{ffi::OsString, io, path::PathBuf};

use crate::contender::{Contender, Pair};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SoxrOp {
    #[error("create a resampler")]
    Create,
    #[error("resample")]
    Process,
    #[error("drain the tail")]
    Drain,
    #[error("take more input, taking none and giving none")]
    Stall,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Resonate's resampler refused {contender} for {pair}")]
    Ours {
        contender: Contender,
        pair: Pair,
        #[source]
        source: resonate_dsp::Error,
    },
    #[error("libsoxr could not {op} for {contender} at {pair}")]
    Soxr {
        op: SoxrOp,
        contender: Contender,
        pair: Pair,
    },
    #[error("could not write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{flag} needs a value")]
    MissingValue { flag: &'static str },
    #[error("unknown argument {0:?}")]
    UnknownArgument(OsString),
    #[error("could not read candidates from {file}")]
    Candidates {
        file: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("a candidate is level, half taps, beta, clear taps, hop taps, edge terms: {0}")]
    UnreadableCandidate(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_stays_small() {
        assert!(size_of::<Error>() <= 128);
    }
}
