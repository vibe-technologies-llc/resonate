use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _, path::Path};

use crate::error::{Error, Result};

pub(crate) struct Wide(Vec<u16>);

impl Wide {
    pub(crate) fn of_path(path: &Path) -> Result<Self> {
        Self::of(path.as_os_str()).ok_or_else(|| Error::NulInName {
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn of(text: &OsStr) -> Option<Self> {
        let units: Vec<u16> = text.encode_wide().collect();
        if units.contains(&0) {
            return None;
        }
        Some(Self(units.into_iter().chain([0]).collect()))
    }

    pub(crate) fn name(&self) -> windows_core::PCWSTR {
        windows_core::PCWSTR(self.0.as_ptr())
    }
}
