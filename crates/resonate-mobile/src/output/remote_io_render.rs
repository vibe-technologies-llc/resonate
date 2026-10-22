#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
    process,
    ptr::NonNull,
};

use objc2_audio_toolbox::AudioUnitRenderActionFlags;
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

use super::remote_io_buffers::rendered;

pub(crate) unsafe extern "C-unwind" fn render(
    rendering: NonNull<c_void>,
    _: NonNull<AudioUnitRenderActionFlags>,
    _: NonNull<AudioTimeStamp>,
    _: u32,
    _: u32,
    buffers: *mut AudioBufferList,
) -> i32 {
    let rendered =
        panic::catch_unwind(AssertUnwindSafe(|| unsafe { rendered(rendering, buffers) }));
    rendered.unwrap_or_else(|_| process::abort())
}
