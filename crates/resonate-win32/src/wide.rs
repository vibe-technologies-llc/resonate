#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

use crate::error::{Error, Result};

pub(crate) struct Wide(Vec<u16>);

impl Wide {
    pub(crate) fn of_path(path: &Path) -> Result<Self> {
        Self::of(path.as_os_str()).ok_or_else(|| Error::NulInName {
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn of(text: &OsStr) -> Option<Self> {
        let units = units_of(text);
        if units.contains(&0) {
            return None;
        }
        Some(Self(units.into_iter().chain([0]).collect()))
    }

    pub(crate) fn units(&self) -> &[u16] {
        &self.0
    }
}

#[cfg(windows)]
pub(crate) fn units_of(text: &OsStr) -> Vec<u16> {
    text.encode_wide().collect()
}

#[cfg(not(windows))]
pub(crate) fn units_of(text: &OsStr) -> Vec<u16> {
    text.to_string_lossy().encode_utf16().collect()
}

#[cfg(windows)]
pub(crate) fn text_of(units: &[u16]) -> OsString {
    OsString::from_wide(units)
}

#[cfg(not(windows))]
pub(crate) fn text_of(units: &[u16]) -> OsString {
    OsString::from(String::from_utf16_lossy(units))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_name_ends_in_its_nul_and_a_name_holding_one_is_refused() {
        let named = Wide::of(OsStr::new("a.flac")).expect("a name");
        let holding = Wide::of_path(&PathBuf::from("a\0b"));

        assert_eq!(
            named.units(),
            [u16::from(b'a'), u16::from(b'.'), 102, 108, 97, 99, 0]
        );
        assert!(matches!(holding, Err(Error::NulInName { .. })));
    }

    #[test]
    fn units_come_back_as_the_text_they_were() {
        let units = units_of(OsStr::new("caf\u{e9} \u{1d11e}"));

        assert_eq!(text_of(&units), OsString::from("caf\u{e9} \u{1d11e}"));
    }
}
