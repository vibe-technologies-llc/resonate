#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{ffi::c_void, process, ptr::NonNull};

use objc2_audio_toolbox::AudioUnitRenderActionFlags;
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

use super::remote_io_buffers::{guarded, host_time_of, rendered};

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
}

pub(crate) unsafe extern "C-unwind" fn render(
    rendering: NonNull<c_void>,
    _: NonNull<AudioUnitRenderActionFlags>,
    time: NonNull<AudioTimeStamp>,
    _: u32,
    _: u32,
    buffers: *mut AudioBufferList,
) -> i32 {
    guarded(
        || {
            let now = unsafe { mach_absolute_time() };
            let host = host_time_of(unsafe { time.as_ref() }, now);
            unsafe { rendered(rendering, buffers, host) }
        },
        || process::abort(),
    )
}
