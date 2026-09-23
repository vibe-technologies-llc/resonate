use std::{io, result};

use thiserror::Error;

use crate::frame::LARGEST_FRAME;

const INVALID_CLIENT_ID: i64 = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpcOp {
    Connect,
    Configure,
    Write,
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JsonOp {
    Encode,
    Decode,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("no Discord client is listening on this machine")]
    NoDiscord,

    #[error("{op:?} failed on the Discord socket")]
    Socket {
        op: IpcOp,
        #[source]
        source: io::Error,
    },

    #[error("{op:?} failed on a Discord frame")]
    Json {
        op: JsonOp,
        #[source]
        source: serde_json::Error,
    },

    #[error("Discord closed the connection with code {code}")]
    Closed { code: i64 },

    #[error("Discord refused what it was sent with code {code}")]
    Refused { code: i64 },

    #[error("a Discord frame of {length} bytes is past the {LARGEST_FRAME} one may hold")]
    Oversized { length: u32 },

    #[error("Discord sent a frame under opcode {opcode}, which this client does not read")]
    Unexpected { opcode: u32 },
}

impl Error {
    pub const fn names_no_application(&self) -> bool {
        matches!(
            self,
            Self::Closed {
                code: INVALID_CLIENT_ID
            }
        )
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

    #[test]
    fn only_the_invalid_client_close_names_no_application() {
        assert!(Error::Closed { code: 4000 }.names_no_application());
        assert!(!Error::Closed { code: 4002 }.names_no_application());
        assert!(!Error::Refused { code: 4000 }.names_no_application());
    }
}
