use std::result;

use thiserror::Error;
use zbus::names::OwnedWellKnownName;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BusOp {
    Connect,
    Find,
    Read,
    Write,
    Call,
    Queue,
    ClaimName,
    ReleaseName,
    Serve,
    Emit,
    Listen,
    Notify,
    Inhibit,
    Release,
    Shutdown,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} failed on the session bus")]
    Bus {
        op: BusOp,
        #[source]
        source: Box<zbus::Error>,
    },

    #[error("{name} is already owned by another player")]
    NameTaken { name: OwnedWellKnownName },

    #[error("{reading} is not a volume between zero and one")]
    NotAVolume { reading: f64 },

    #[error("the service thread could not be started")]
    ThreadStopped,
}

impl Error {
    pub(crate) fn bus(op: BusOp, source: zbus::Error) -> Self {
        Self::Bus {
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
    fn an_error_stays_small_enough_for_every_result_in_the_crate() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
