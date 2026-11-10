use std::{io, result};

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JarOp {
    MakeFolder,
    Stage,
    Narrow,
    Write,
    Sync,
    PutInPlace,
}

#[cfg(feature = "window")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineOp {
    Build,
    Address,
    Cookies,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} of the cookie jar failed")]
    Jar {
        op: JarOp,
        #[source]
        source: io::Error,
    },

    #[cfg(feature = "window")]
    #[error("the sign-in window could not be made")]
    Window {
        #[source]
        source: tao::error::OsError,
    },

    #[cfg(feature = "window")]
    #[error("the sign-in window has nowhere to hold its page")]
    NoContainer,

    #[cfg(feature = "window")]
    #[error("{op:?} of the browser engine failed")]
    Engine {
        op: EngineOp,
        #[source]
        source: Box<wry::Error>,
    },
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
