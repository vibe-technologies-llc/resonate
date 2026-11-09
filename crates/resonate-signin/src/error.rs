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

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} of the cookie jar failed")]
    Jar {
        op: JarOp,
        #[source]
        source: io::Error,
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
