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
    ptr::NonNull,
    slice,
};

use objc2_core_audio_types::AudioBufferList;

use super::remote_io_fill::Rendering;

pub(crate) const RENDERED: i32 = 0;

pub(crate) unsafe fn rendered(rendering: NonNull<c_void>, buffers: *mut AudioBufferList) -> i32 {
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

pub(crate) fn guarded(rendered: impl FnOnce() -> i32, aborting: impl FnOnce() -> i32) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(rendered)).unwrap_or_else(|_| aborting())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use objc2_core_audio_types::AudioBuffer;
    use resonate_audio::AudioSource;

    use super::*;

    struct Supply {
        left: usize,
    }

    impl AudioSource for Supply {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            let given = self.left.min(dst.len());
            dst[..given].fill(0xAB);
            self.left -= given;
            given
        }
    }

    fn rendering(left: usize) -> (Box<Rendering>, Arc<AtomicBool>) {
        let empty = Arc::new(AtomicBool::new(false));
        let (rendering, _) = Rendering::fed_by(Box::new(Supply { left }), Arc::clone(&empty));
        (Box::new(rendering), empty)
    }

    fn list_over(data: *mut c_void, bytes: u32) -> AudioBufferList {
        AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: 2,
                mDataByteSize: bytes,
                mData: data,
            }],
        }
    }

    fn handed(rendering: &mut Box<Rendering>) -> NonNull<c_void> {
        NonNull::from(&mut **rendering).cast()
    }

    #[test]
    fn the_systems_buffer_is_filled_up_to_the_bytes_it_declares_and_no_further() {
        let (mut rendering, _) = rendering(64);
        let mut memory = [0x11_u8; 24];
        let mut list = list_over(memory.as_mut_ptr().cast(), 16);

        let answer = unsafe { rendered(handed(&mut rendering), &mut list) };

        assert_eq!(answer, RENDERED);
        assert_eq!(&memory[..16], &[0xAB; 16]);
        assert_eq!(&memory[16..], &[0x11; 8]);
    }

    #[test]
    fn a_short_source_leaves_the_rest_of_the_systems_buffer_silent_and_raises_nothing() {
        let (mut rendering, empty) = rendering(6);
        let mut memory = [0x11_u8; 16];
        let mut list = list_over(memory.as_mut_ptr().cast(), 16);

        unsafe { rendered(handed(&mut rendering), &mut list) };

        assert_eq!(&memory[..6], &[0xAB; 6]);
        assert_eq!(&memory[6..], &[0; 10]);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_source_with_nothing_gives_a_silent_buffer_and_raises_the_flag() {
        let (mut rendering, empty) = rendering(0);
        let mut memory = [0x11_u8; 16];
        let mut list = list_over(memory.as_mut_ptr().cast(), 16);

        unsafe { rendered(handed(&mut rendering), &mut list) };

        assert_eq!(memory, [0; 16]);
        assert!(empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_list_the_system_did_not_hand_over_is_rendered_as_nothing_and_asks_the_source_for_nothing()
    {
        let (mut rendering, empty) = rendering(16);

        let answer = unsafe { rendered(handed(&mut rendering), std::ptr::null_mut()) };

        assert_eq!(answer, RENDERED);
        assert_eq!(rendering.source().fill(&mut [0; 4]), 4);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_buffer_with_no_memory_is_rendered_as_nothing_and_asks_the_source_for_nothing() {
        let (mut rendering, empty) = rendering(16);
        let mut list = list_over(std::ptr::null_mut(), 16);

        let answer = unsafe { rendered(handed(&mut rendering), &mut list) };

        assert_eq!(answer, RENDERED);
        assert_eq!(rendering.source().fill(&mut [0; 16]), 16);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_buffer_declaring_no_bytes_is_never_written_and_is_not_an_empty_source() {
        let (mut rendering, empty) = rendering(16);
        let mut memory = [0x11_u8; 8];
        let mut list = list_over(memory.as_mut_ptr().cast(), 0);

        unsafe { rendered(handed(&mut rendering), &mut list) };

        assert_eq!(memory, [0x11; 8]);
        assert!(empty.load(Ordering::Acquire));
    }

    #[test]
    fn the_list_is_laid_out_as_coreaudio_reads_it() {
        assert_eq!(size_of::<AudioBufferList>(), 24);
        assert_eq!(align_of::<AudioBufferList>(), 8);
        assert_eq!(size_of::<AudioBuffer>(), 16);
    }

    #[test]
    fn a_render_that_returns_hands_its_status_back_and_never_aborts() {
        let status = guarded(|| 7, || -1);

        assert_eq!(status, 7);
    }

    #[test]
    #[allow(clippy::panic)]
    fn a_render_that_panics_is_caught_and_answered_by_the_abort_alone() {
        let status = guarded(|| panic!("a source that panicked in the callback"), || -1);

        assert_eq!(status, -1);
    }

    #[test]
    fn the_abort_is_not_taken_unless_the_render_panicked() {
        let aborted = std::cell::Cell::new(false);

        guarded(
            || RENDERED,
            || {
                aborted.set(true);
                -1
            },
        );

        assert!(!aborted.get());
    }
}
