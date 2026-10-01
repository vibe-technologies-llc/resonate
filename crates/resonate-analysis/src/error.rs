use std::result;

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnalysisOp {
    Open,
    Decode,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} failed while the stream was analysed")]
    Codec {
        op: AnalysisOp,
        #[source]
        source: Box<resonate_codec::Error>,
    },

    #[error("the analysis was stopped before it reached the end of the stream")]
    Stopped,
}

impl Error {
    pub fn is_out_of_reach(&self) -> bool {
        match self {
            Self::Codec { source, .. } => source.is_out_of_reach(),
            Self::Stopped => false,
        }
    }

    pub(crate) fn codec(op: AnalysisOp, source: resonate_codec::Error) -> Self {
        Self::Codec {
            op,
            source: Box::new(source),
        }
    }
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
