use std::result;

use resonate_core::SourceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureOp {
    Connect,
    Open,
    Play,
    Stop,
}

#[derive(Debug, Error)]
pub enum Error {
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    #[error("capturing sound failed at {op:?}")]
    Capture {
        op: CaptureOp,
        #[source]
        source: Box<resonate_audio::Error>,
    },

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    #[error("playing through the output failed")]
    Output {
        #[source]
        source: Box<resonate_audio::Error>,
    },

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    #[error("no output is named {name}")]
    NoSuchSink { name: resonate_audio::NodeName },

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    #[error("the graph has no output to play through")]
    NoOutput,

    #[cfg(any(windows, target_os = "macos"))]
    #[error("no microphone is named {name}")]
    NoSuchMicrophone { name: resonate_audio::NodeName },

    #[cfg(target_os = "macos")]
    #[error("what the desktop plays can be recorded from macOS 14.2 on")]
    DesktopUnheard,

    #[error("nothing reached the recording before it was given up on")]
    NothingHeard,

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
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
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    pub fn capture(op: CaptureOp, source: impl Into<resonate_audio::Error>) -> Self {
        Self::Capture {
            op,
            source: Box::new(source.into()),
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
