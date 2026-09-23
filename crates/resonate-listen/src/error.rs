use std::result;

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureOp {
    Connect,
    Open,
    Stop,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("capturing sound failed at {op:?}")]
    Capture {
        op: CaptureOp,
        #[source]
        source: Box<resonate_pipewire::Error>,
    },

    #[error("nothing reached the recording before it was given up on")]
    NothingHeard,

    #[error("the listening was stopped")]
    Stopped,

    #[error("{service} could not be reached")]
    Unreachable { service: SourceId },

    #[error("{service} refused the question with status {status}")]
    Refused { service: SourceId, status: u16 },

    #[error("{service} answered with something that could not be read")]
    Unreadable { service: SourceId },

    #[error("the clip is larger than {service} takes")]
    TooLarge { service: SourceId },
}

impl Error {
    pub fn capture(op: CaptureOp, source: resonate_pipewire::Error) -> Self {
        Self::Capture {
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
