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
    slice,
};

use objc2_audio_toolbox::AudioUnitRenderActionFlags;
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

use super::remote_io_fill::Rendering;

const RENDERED: i32 = 0;

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

unsafe fn rendered(rendering: NonNull<c_void>, buffers: *mut AudioBufferList) -> i32 {
    let rendering = unsafe { rendering.cast::<Rendering>().as_mut() };
    let Some(buffers) = (unsafe { buffers.as_mut() }) else {
        return RENDERED;
    };
    let [buffer] = &mut buffers.mBuffers;
    let Some(data) = NonNull::new(buffer.mData.cast::<u8>()) else {
        return RENDERED;
    };
    let bytes = unsafe { slice::from_raw_parts_mut(data.as_ptr(), buffer.mDataByteSize as usize) };
    rendering.fill(bytes);
    RENDERED
}
