#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{ffi::c_void, process, ptr::NonNull};

use objc2_audio_toolbox::{AudioUnit, AudioUnitRender, AudioUnitRenderActionFlags};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

use crate::{
    buffers::{guarded, heard, rendered},
    clock::host_ticks,
};

pub(crate) unsafe extern "C-unwind" fn render(
    handed: NonNull<c_void>,
    _: NonNull<AudioUnitRenderActionFlags>,
    time: NonNull<AudioTimeStamp>,
    _: u32,
    _: u32,
    buffers: *mut AudioBufferList,
) -> i32 {
    guarded(
        || {
            let now = host_ticks();
            let time = unsafe { time.as_ref() };
            unsafe { rendered(handed, buffers, time, now) }
        },
        || process::abort(),
    )
}

pub(crate) unsafe extern "C-unwind" fn hear(
    handed: NonNull<c_void>,
    flags: NonNull<AudioUnitRenderActionFlags>,
    time: NonNull<AudioTimeStamp>,
    bus: u32,
    frames: u32,
    _: *mut AudioBufferList,
) -> i32 {
    guarded(
        || {
            let pull = |unit: AudioUnit, list: NonNull<AudioBufferList>| unsafe {
                AudioUnitRender(unit, flags.as_ptr(), time, bus, frames, list)
            };
            unsafe { heard(handed, frames, pull) }
        },
        || process::abort(),
    )
}
