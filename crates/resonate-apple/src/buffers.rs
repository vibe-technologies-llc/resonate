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

use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp, AudioTimeStampFlags};

use crate::{
    clock::HostClock,
    unit::{Hearing, HostTime, Rendered},
};

pub(crate) const ANSWERED: i32 = 0;

pub(crate) fn host_time_of(time: &AudioTimeStamp, now: u64, clock: HostClock) -> Option<HostTime> {
    (time.mFlags.contains(AudioTimeStampFlags::HostTimeValid) && time.mHostTime != 0).then(|| {
        HostTime {
            now: clock.nanos(now),
            of_the_buffer: clock.nanos(time.mHostTime),
        }
    })
}

pub(crate) unsafe fn rendered(
    rendered: NonNull<c_void>,
    buffers: *mut AudioBufferList,
    time: &AudioTimeStamp,
    now: u64,
) -> i32 {
    let rendered = unsafe { rendered.cast::<Rendered>().as_mut() };
    let Some(buffers) = (unsafe { buffers.as_mut() }) else {
        return ANSWERED;
    };
    let [buffer] = &mut buffers.mBuffers;
    let Some(data) = NonNull::new(buffer.mData.cast::<u8>()) else {
        return ANSWERED;
    };
    let bytes = unsafe { slice::from_raw_parts_mut(data.as_ptr(), buffer.mDataByteSize as usize) };
    let host = host_time_of(time, now, rendered.clock);
    rendered.render.render(bytes, host);
    ANSWERED
}

pub(crate) unsafe fn heard<I: Copy>(
    hearing: NonNull<c_void>,
    frames: u32,
    pull: impl FnOnce(I, NonNull<AudioBufferList>) -> i32,
) -> i32 {
    let hearing = unsafe { hearing.cast::<Hearing<I>>().as_mut() };
    let Some(wanted) = (frames as usize).checked_mul(hearing.frame.get()) else {
        return ANSWERED;
    };
    let Some(window) = hearing.scratch.get_mut(..wanted) else {
        return ANSWERED;
    };
    let Ok(declared) = u32::try_from(wanted) else {
        return ANSWERED;
    };
    let mut list = AudioBufferList {
        mNumberBuffers: 1,
        mBuffers: [AudioBuffer {
            mNumberChannels: hearing.channels,
            mDataByteSize: declared,
            mData: window.as_mut_ptr().cast::<c_void>(),
        }],
    };
    if pull(hearing.unit, NonNull::from(&mut list)) != ANSWERED {
        return ANSWERED;
    }
    let [buffer] = &list.mBuffers;
    let given = (buffer.mDataByteSize as usize).min(wanted);
    if let Some(taken) = hearing.scratch.get(..given) {
        hearing.hear.hear(taken);
    }
    ANSWERED
}

pub(crate) fn guarded(answered: impl FnOnce() -> i32, aborting: impl FnOnce() -> i32) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(answered)).unwrap_or_else(|_| aborting())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use parking_lot::Mutex;

    use super::*;
    use crate::unit::{Hear, Render};

    #[derive(Default)]
    struct Seen {
        renders: AtomicUsize,
        host: Mutex<Option<HostTime>>,
        heard: Mutex<Vec<u8>>,
    }

    struct Marking(Arc<Seen>);

    impl Render for Marking {
        fn render(&mut self, bytes: &mut [u8], host: Option<HostTime>) {
            self.0.renders.fetch_add(1, Ordering::AcqRel);
            *self.0.host.lock() = host;
            bytes.fill(0xAB);
        }
    }

    impl Hear for Marking {
        fn hear(&mut self, bytes: &[u8]) {
            self.0.heard.lock().extend_from_slice(bytes);
        }
    }

    fn marking() -> (Box<Rendered>, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let rendered = Rendered {
            render: Box::new(Marking(Arc::clone(&seen))),
            clock: HostClock::of(125, 3),
        };
        (Box::new(rendered), seen)
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

    fn handed<V>(value: &mut Box<V>) -> NonNull<c_void> {
        NonNull::from(&mut **value).cast()
    }

    fn timed(host: u64, flags: AudioTimeStampFlags) -> AudioTimeStamp {
        let mut time: AudioTimeStamp = unsafe { std::mem::zeroed() };
        time.mHostTime = host;
        time.mFlags = flags;
        time
    }

    fn untimed() -> AudioTimeStamp {
        timed(0, AudioTimeStampFlags::empty())
    }

    #[test]
    fn the_systems_buffer_is_handed_over_up_to_the_bytes_it_declares_and_no_further() {
        let (mut rendering, _) = marking();
        let mut memory = [0x11_u8; 24];
        let mut list = list_over(memory.as_mut_ptr().cast(), 16);

        let answer = unsafe { rendered(handed(&mut rendering), &mut list, &untimed(), 0) };

        assert_eq!(answer, ANSWERED);
        assert_eq!(&memory[..16], &[0xAB; 16]);
        assert_eq!(&memory[16..], &[0x11; 8]);
    }

    #[test]
    fn a_list_the_system_did_not_hand_over_is_answered_and_asks_the_render_for_nothing() {
        let (mut rendering, seen) = marking();

        let answer =
            unsafe { rendered(handed(&mut rendering), std::ptr::null_mut(), &untimed(), 0) };

        assert_eq!(answer, ANSWERED);
        assert_eq!(seen.renders.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_buffer_with_no_memory_is_answered_and_asks_the_render_for_nothing() {
        let (mut rendering, seen) = marking();
        let mut list = list_over(std::ptr::null_mut(), 16);

        let answer = unsafe { rendered(handed(&mut rendering), &mut list, &untimed(), 0) };

        assert_eq!(answer, ANSWERED);
        assert_eq!(seen.renders.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_buffer_declaring_no_bytes_is_never_written() {
        let (mut rendering, seen) = marking();
        let mut memory = [0x11_u8; 8];
        let mut list = list_over(memory.as_mut_ptr().cast(), 0);

        unsafe { rendered(handed(&mut rendering), &mut list, &untimed(), 0) };

        assert_eq!(memory, [0x11; 8]);
        assert_eq!(seen.renders.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_render_is_told_the_host_time_in_nanoseconds_only_where_the_system_says_it_is_valid() {
        let clock = HostClock::of(125, 3);
        let valid = timed(
            48_000,
            AudioTimeStampFlags::HostTimeValid | AudioTimeStampFlags::SampleTimeValid,
        );
        let unsaid = timed(48_000, AudioTimeStampFlags::SampleTimeValid);
        let unset = timed(0, AudioTimeStampFlags::HostTimeValid);

        assert_eq!(
            host_time_of(&valid, 24_000, clock),
            Some(HostTime {
                now: 1_000_000,
                of_the_buffer: 2_000_000
            })
        );
        assert_eq!(host_time_of(&unsaid, 24_000, clock), None);
        assert_eq!(host_time_of(&unset, 24_000, clock), None);
    }

    #[test]
    fn the_render_reads_the_host_time_through_the_clock_its_unit_opened_with() {
        let (mut rendering, seen) = marking();
        let mut memory = [0_u8; 8];
        let mut list = list_over(memory.as_mut_ptr().cast(), 8);
        let time = timed(48_000, AudioTimeStampFlags::HostTimeValid);

        unsafe { rendered(handed(&mut rendering), &mut list, &time, 24_000) };

        assert_eq!(
            *seen.host.lock(),
            Some(HostTime {
                now: 1_000_000,
                of_the_buffer: 2_000_000
            })
        );
    }

    #[test]
    fn the_list_is_laid_out_as_coreaudio_reads_it() {
        assert_eq!(size_of::<AudioBufferList>(), 24);
        assert_eq!(align_of::<AudioBufferList>(), 8);
        assert_eq!(size_of::<AudioBuffer>(), 16);
    }

    fn hearing(scratch: usize) -> (Box<Hearing<u8>>, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let hearing = Hearing {
            unit: 7,
            scratch: vec![0; scratch].into_boxed_slice(),
            frame: NonZeroUsize::MIN.saturating_add(7),
            channels: 2,
            hear: Box::new(Marking(Arc::clone(&seen))),
        };
        (Box::new(hearing), seen)
    }

    fn filling(with: u8) -> impl FnOnce(u8, NonNull<AudioBufferList>) -> i32 {
        move |unit, mut list| {
            assert_eq!(unit, 7);
            let [buffer] = &mut unsafe { list.as_mut() }.mBuffers;
            let bytes = unsafe {
                slice::from_raw_parts_mut(buffer.mData.cast::<u8>(), buffer.mDataByteSize as usize)
            };
            bytes.fill(with);
            ANSWERED
        }
    }

    #[test]
    fn what_the_unit_pulled_is_handed_on_frame_for_frame() {
        let (mut hearing, seen) = hearing(64);

        let answer = unsafe { heard(handed(&mut hearing), 4, filling(0x5A)) };

        assert_eq!(answer, ANSWERED);
        assert_eq!(*seen.heard.lock(), [0x5A; 32]);
    }

    #[test]
    fn a_pull_the_unit_refused_hands_on_nothing() {
        let (mut hearing, seen) = hearing(64);

        let answer = unsafe { heard(handed(&mut hearing), 4, |_: u8, _| -10_863) };

        assert_eq!(answer, ANSWERED);
        assert!(seen.heard.lock().is_empty());
    }

    #[test]
    fn a_slice_longer_than_the_scratch_is_never_pulled() {
        let (mut hearing, seen) = hearing(16);
        let pulled = AtomicUsize::new(0);

        unsafe {
            heard(handed(&mut hearing), 4, |_: u8, _| {
                pulled.fetch_add(1, Ordering::AcqRel);
                ANSWERED
            })
        };

        assert_eq!(pulled.load(Ordering::Acquire), 0);
        assert!(seen.heard.lock().is_empty());
    }

    #[test]
    fn a_unit_that_gave_fewer_bytes_than_asked_hands_on_only_those() {
        let (mut hearing, seen) = hearing(64);

        unsafe {
            heard(handed(&mut hearing), 4, |_: u8, mut list| {
                let [buffer] = &mut list.as_mut().mBuffers;
                buffer.mDataByteSize = 8;
                ANSWERED
            })
        };

        assert_eq!(seen.heard.lock().len(), 8);
    }

    #[test]
    fn a_callback_that_returns_hands_its_status_back_and_never_aborts() {
        let status = guarded(|| 7, || -1);

        assert_eq!(status, 7);
    }

    #[test]
    #[allow(clippy::panic)]
    fn a_callback_that_panics_is_caught_and_answered_by_the_abort_alone() {
        let status = guarded(|| panic!("a source that panicked in the callback"), || -1);

        assert_eq!(status, -1);
    }

    #[test]
    fn the_abort_is_not_taken_unless_the_callback_panicked() {
        let aborted = std::cell::Cell::new(false);

        guarded(
            || ANSWERED,
            || {
                aborted.set(true);
                -1
            },
        );

        assert!(!aborted.get());
    }
}
