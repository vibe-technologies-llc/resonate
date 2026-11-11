#[cfg(feature = "console")]
use std::ptr;

#[cfg(feature = "console")]
use windows::Win32::Foundation::HANDLE;
use windows_core::PCWSTR;

use crate::{error::HResult, wide::Wide};

#[derive(Clone, Copy)]
pub(crate) struct Windows;

pub(crate) fn hresult(error: windows_core::Error) -> HResult {
    HResult(error.code().0)
}

pub(crate) fn name_of(wide: &Wide) -> PCWSTR {
    PCWSTR(wide.units().as_ptr())
}

#[cfg(feature = "console")]
pub(crate) fn handle_of(raw: usize) -> HANDLE {
    HANDLE(ptr::without_provenance_mut(raw))
}

#[cfg(feature = "console")]
pub(crate) fn raw_of(handle: HANDLE) -> usize {
    handle.0.addr()
}
