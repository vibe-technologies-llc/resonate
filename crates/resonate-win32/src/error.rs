use std::{io, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    JoinProAudio,
    LeaveProAudio,
    ReadCounter,
    ReadCounterFrequency,
    NameTheProcess,
    ReachEndpoints,
    FindEndpoint,
    ActivateEndpointVolume,
    ReadLevel,
    SetLevel,
    ReadMute,
    SetMute,
    WatchVolume,
    ListStreams,
    ReadSecurity,
    WriteSecurity,
    CopyStream,
    OpenPseudoConsole,
    MakePipe,
    MakeAttributes,
    StartProcess,
    WaitForProcess,
    EndProcess,
    TellTheConsole,
    ReadConsoleMode,
    ReadFileIdentity,
    LeaveTheConsole,
    HearConsoleInterrupts,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Windows refused to {op:?}")]
    Windows {
        op: Op,
        #[source]
        source: windows_core::Error,
    },
    #[error("Windows refused to {op:?} for {path}")]
    WindowsOnFile {
        op: Op,
        path: PathBuf,
        #[source]
        source: windows_core::Error,
    },
    #[error("could not {op:?} for {path}")]
    Io {
        op: Op,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} holds a NUL, which no Windows name may")]
    NulInName { path: PathBuf },
    #[error("the performance counter names no frequency")]
    CounterStandsStill,
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn refused(op: Op) -> impl FnOnce(windows_core::Error) -> Error {
    move |source| Error::Windows { op, source }
}

pub(crate) fn refused_on(
    op: Op,
    path: &std::path::Path,
) -> impl FnOnce(windows_core::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::WindowsOnFile { op, path, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_stays_small() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
