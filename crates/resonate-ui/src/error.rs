use std::result;

use thiserror::Error;

use crate::SettingKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowKind {
    Main,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("the {kind:?} window could not be opened")]
    WindowOpen { kind: WindowKind },

    #[error("the {key:?} setting could not be saved")]
    SettingNotStored { key: SettingKey },

    #[error(transparent)]
    Engine(#[from] resonate_engine::Error),

    #[error(transparent)]
    Library(#[from] resonate_library::Error),
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
