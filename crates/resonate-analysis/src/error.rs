use std::result;

use resonate_core::StreamSpec;
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

    #[error(
        "a stream turning from {from:?} to {to:?} could not be brought back to the shape it opened in"
    )]
    Conformity {
        from: StreamSpec,
        to: StreamSpec,
        #[source]
        source: Box<resonate_dsp::Error>,
    },

    #[error("the equaliser could not be set up to shape the stream it was measured over")]
    Equalising {
        #[source]
        source: Box<resonate_dsp::Error>,
    },

    #[error("the room correction could not be brought to the rate the stream was measured at")]
    Correcting {
        #[source]
        source: Box<resonate_dsp::Error>,
    },

    #[error("the analysis was stopped before it reached the end of the stream")]
    Stopped,
}

impl Error {
    pub fn is_out_of_reach(&self) -> bool {
        match self {
            Self::Codec { source, .. } => source.is_out_of_reach(),
            Self::Conformity { .. }
            | Self::Equalising { .. }
            | Self::Correcting { .. }
            | Self::Stopped => false,
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
