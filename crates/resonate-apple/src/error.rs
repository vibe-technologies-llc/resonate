use std::{fmt, result};

use crate::{status::OsStatus, unit::Component};

pub type Result<T> = result::Result<T, Error>;

#[cfg_attr(not(target_vendor = "apple"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    MakeUnit,
    EnableIo,
    SetDevice,
    SetFormat,
    ReadFormat,
    SetCallback,
    Initialize,
    FindDevices,
    ReadDefaultOutput,
    ReadDefaultInput,
    ReadUid,
    ReadName,
    ReadTransport,
    ReadAlive,
    ReadStreams,
    ReadRate,
    SetRate,
    ReadRates,
    ReadPhysicalFormat,
    SetPhysicalFormat,
    ReadPhysicalFormats,
    ReadLatency,
    ReadJack,
    ReadHog,
    TakeHog,
    ReadVolume,
    SetVolume,
    ReadMute,
    SetMute,
    Listen,
    NotifyOnOwnThread,
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
            Self::FindDevices => "list the audio devices",
            Self::ReadDefaultOutput => "name the default output device",
            Self::ReadDefaultInput => "name the default input device",
            Self::ReadUid => "read a device's identifier",
            Self::ReadName => "read a device's name",
            Self::ReadTransport => "read how a device is connected",
            Self::ReadAlive => "read whether a device is still there",
            Self::ReadStreams => "list a device's streams",
            Self::ReadRate => "read a device's sample rate",
            Self::SetRate => "set a device's sample rate",
            Self::ReadRates => "list a device's sample rates",
            Self::ReadPhysicalFormat => "read a stream's physical format",
            Self::SetPhysicalFormat => "set a stream's physical format",
            Self::ReadPhysicalFormats => "list a stream's physical formats",
            Self::ReadLatency => "read a device's latency",
            Self::ReadJack => "read whether a device is plugged in",
            Self::ReadHog => "read who holds a device",
            Self::TakeHog => "take or release a device",
            Self::ReadVolume => "read a device's volume",
            Self::SetVolume => "set a device's volume",
            Self::ReadMute => "read a device's mute",
            Self::SetMute => "set a device's mute",
            Self::Listen => "listen for a device's changes",
            Self::NotifyOnOwnThread => "move the device notifications off the main run loop",
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
    #[cfg(target_os = "macos")]
    #[error("the device is held by process {by}")]
    Hogged { by: crate::hog::Pid },
    #[cfg(target_os = "macos")]
    #[error("the device stayed free though it was asked for")]
    HogUntaken,
    #[cfg(target_os = "macos")]
    #[error("Core Audio answered {bytes} bytes to {op}, which is not what was asked for")]
    Missized { op: Op, bytes: u32 },
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
