use std::{fmt, io, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    WatchGameMode,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HResult(pub i32);

impl HResult {
    pub(crate) const HANDLE_EOF: Self = Self(0x8007_0026_u32.cast_signed());

    #[cfg(windows)]
    fn explanation(self) -> String {
        windows_core::HRESULT(self.0).message()
    }

    #[cfg(not(windows))]
    fn explanation(self) -> String {
        String::new()
    }
}

impl fmt::Display for HResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:#010X}", self.0.cast_unsigned())?;
        let explanation = self.explanation();
        match explanation.trim() {
            "" => Ok(()),
            explained => write!(formatter, ": {explained}"),
        }
    }
}

impl std::error::Error for HResult {}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Windows refused to {op:?}")]
    Windows {
        op: Op,
        #[source]
        source: HResult,
    },
    #[error("Windows refused to {op:?} for {path}")]
    WindowsOnFile {
        op: Op,
        path: PathBuf,
        #[source]
        source: HResult,
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

pub(crate) type Raw<T> = std::result::Result<T, HResult>;

pub(crate) fn refused(op: Op) -> impl FnOnce(HResult) -> Error {
    move |source| Error::Windows { op, source }
}

pub(crate) fn refused_on(op: Op, path: &std::path::Path) -> impl FnOnce(HResult) -> Error {
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

    #[test]
    fn a_failure_code_prints_as_the_eight_digits_windows_documents() {
        assert!(
            HResult(0x8000_4005_u32.cast_signed())
                .to_string()
                .starts_with("0x80004005")
        );
        assert!(HResult(0).to_string().starts_with("0x00000000"));
    }

    #[test]
    fn handle_eof_is_the_win32_code_made_a_failure() {
        assert_eq!(HResult::HANDLE_EOF, HResult(0x8007_0026_u32.cast_signed()));
    }

    #[test]
    fn a_refusal_names_the_call_and_keeps_the_code() {
        let error = refused(Op::ReadLevel)(HResult(-5));

        assert!(matches!(
            error,
            Error::Windows {
                op: Op::ReadLevel,
                source: HResult(-5)
            }
        ));
    }
}
