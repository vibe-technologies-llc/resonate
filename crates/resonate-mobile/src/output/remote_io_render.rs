#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    ffi::c_void,
    ptr::NonNull,
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use objc2_audio_toolbox::AudioUnitRenderActionFlags;
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};
use resonate_audio::AudioSource;

const RENDERED: i32 = 0;

pub(crate) struct Rendering {
    pub(crate) source: Box<dyn AudioSource>,
    pub(crate) empty: Arc<AtomicBool>,
}

pub(crate) unsafe extern "C-unwind" fn render(
    rendering: NonNull<c_void>,
    _: NonNull<AudioUnitRenderActionFlags>,
    _: NonNull<AudioTimeStamp>,
    _: u32,
    _: u32,
    buffers: *mut AudioBufferList,
) -> i32 {
    let rendering = unsafe { rendering.cast::<Rendering>().as_mut() };
    let Some(buffers) = (unsafe { buffers.as_mut() }) else {
        return RENDERED;
    };
    let [buffer] = &mut buffers.mBuffers;
    let Some(data) = NonNull::new(buffer.mData.cast::<u8>()) else {
        return RENDERED;
    };
    let bytes = unsafe { slice::from_raw_parts_mut(data.as_ptr(), buffer.mDataByteSize as usize) };
    let filled = rendering.source.fill(bytes);
    rendering.empty.store(filled == 0, Ordering::Release);
    if let Some(unfilled) = bytes.get_mut(filled..) {
        unfilled.fill(0);
    }
    RENDERED
}
