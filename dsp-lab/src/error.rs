use std::{io, path::PathBuf};

use crate::lab::{pair::Pair, subject::SubjectName};

#[cfg(feature = "soxr")]
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
    #[error("Resonate refused {subject} at {pair}")]
    Ours {
        subject: SubjectName,
        pair: Pair,
        #[source]
        source: resonate_dsp::Error,
    },
    #[cfg(feature = "soxr")]
    #[error("libsoxr could not {op} for {subject} at {pair}")]
    Soxr {
        op: SoxrOp,
        subject: SubjectName,
        pair: Pair,
    },
    #[cfg(feature = "rubato")]
    #[error("rubato could not build {subject} at {pair}")]
    RubatoBuilt {
        subject: SubjectName,
        pair: Pair,
        #[source]
        source: rubato::ResamplerConstructionError,
    },
    #[cfg(feature = "rubato")]
    #[error("rubato could not resample for {subject} at {pair}")]
    RubatoRan {
        subject: SubjectName,
        pair: Pair,
        #[source]
        source: rubato::ResampleError,
    },
    #[cfg(feature = "rubato")]
    #[error("rubato refused a buffer laid out for {subject} at {pair}")]
    RubatoBuffer { subject: SubjectName, pair: Pair },
    #[cfg(feature = "rubato")]
    #[error("rubato gave nothing more while {subject} at {pair} was still owed its tail")]
    RubatoStalled { subject: SubjectName, pair: Pair },
    #[cfg(feature = "soxr")]
    #[error("{subject} takes no {layout:?} stream")]
    LayoutUnsupported {
        subject: SubjectName,
        layout: resonate_core::ChannelLayout,
    },
    #[error("{subject} was given a response no convolver takes")]
    ResponseRefused { subject: SubjectName },
    #[error("a value the lab measures with was refused")]
    Vocabulary(#[source] resonate_core::Error),
    #[error("could not write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "line {line} of {file} is no candidate: a level, half taps, beta, clear taps, hop taps, steep hop taps and a window file or -, or a SoXR recipe's name"
    )]
    UnreadableCandidate { file: PathBuf, line: usize },
    #[error(
        "line {line} of {file} is no reading: suite, rates, subject, metric and value apart by tabs"
    )]
    UnreadableBaseline { file: PathBuf, line: usize },
    #[error("there is no suite called {named}; `dsp-lab list` names them")]
    UnknownSuite { named: String },
    #[error("{count} readings are worse than the baseline holds")]
    Regressed { count: usize },
    #[error("{count} readings the baseline holds are no longer measured")]
    Unmeasured { count: usize },
    #[error("nothing measured was held to {file}")]
    NothingHeld { file: PathBuf },
    #[error("a worker measuring {suite} panicked")]
    WorkerPanicked { suite: &'static str },
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
