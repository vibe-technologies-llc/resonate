use std::{fmt, io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no task named; `cargo xtask help` lists them")]
    NoTask,
    #[error("`{word}` is not a task; `cargo xtask help` lists them")]
    UnknownTask { word: String },
    #[error("`{flag}` is not a flag xtask reads")]
    UnknownFlag { flag: String },
    #[error("`{flag}` needs a value after it")]
    MissingValue { flag: &'static str },
    #[error("`{flag}` does not apply to `{task}`")]
    NotForTask {
        flag: &'static str,
        task: &'static str,
    },
    #[error("ANDROID_HOME is not set; point it at the Android SDK")]
    NoAndroidSdk,
    #[error("could not start `{program}`")]
    Spawn {
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("asking for {op} failed (exit code {code:?})")]
    Refused { op: Query, code: Option<i32> },
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} names no package")]
    Unnamed { path: PathBuf },
    #[error("could not keep a step's output at {path}")]
    Log {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{count} of {of} steps failed")]
    Failed { count: usize, of: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    WorkspaceRoot,
    MergeBase,
    Differing,
    Untracked,
    Dependents,
}

impl fmt::Display for Query {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WorkspaceRoot => "where the workspace is",
            Self::MergeBase => "the commit to compare against",
            Self::Differing => "the files changed",
            Self::Untracked => "the files not yet tracked",
            Self::Dependents => "the packages depending on those changed",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
