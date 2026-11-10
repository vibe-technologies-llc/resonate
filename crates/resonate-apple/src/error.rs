use std::{fmt, result};

use crate::{status::OsStatus, unit::Component};

pub type Result<T> = result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    MakeUnit,
    EnableIo,
    SetDevice,
    SetFormat,
    ReadFormat,
    SetCallback,
    Initialize,
}

impl Op {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MakeUnit => "make an audio unit",
            Self::EnableIo => "turn a unit's input or output on or off",
            Self::SetDevice => "give a unit its device",
            Self::SetFormat => "set a stream format",
            Self::ReadFormat => "read a stream format",
            Self::SetCallback => "set a callback",
            Self::Initialize => "initialise an audio unit",
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Core Audio refused to {op}")]
    Refused {
        op: Op,
        #[source]
        source: OsStatus,
    },
    #[error("this system has no {component} audio unit")]
    NoComponent { component: Component },
}

impl Error {
    pub(crate) fn refused(op: Op) -> impl FnOnce(OsStatus) -> Self {
        move |source| Self::Refused { op, source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128);
    }
}
