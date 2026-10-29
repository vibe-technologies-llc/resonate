#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    num::{NonZeroU64, NonZeroU128, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use resonate_audio::{AudioSource, DeviceTime, StreamClock};
use resonate_core::Frames;

use super::handover::{Fed, Handing, fed_from};

const STEREO_F32_FRAME: NonZeroUsize = match NonZeroUsize::new(8) {
    Some(frame) => frame,
    None => NonZeroUsize::MIN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HostTime {
    pub(crate) now: u64,
    pub(crate) of_the_buffer: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timebase {
    numer: u64,
    denom: NonZeroU64,
}

impl Timebase {
    pub(crate) const NANOS: Self = Self {
        numer: 1,
        denom: NonZeroU64::MIN,
    };

    pub(crate) fn of(numer: u32, denom: u32) -> Self {
        NonZeroU64::new(u64::from(denom)).map_or(Self::NANOS, |denom| Self {
            numer: u64::from(numer),
            denom,
        })
    }

    #[cfg(target_os = "ios")]
    pub(crate) fn of_the_host() -> Self {
        let mut info = MachTimebaseInfo::default();
        if unsafe { mach_timebase_info(&mut info) } != KERN_SUCCESS {
            return Self::NANOS;
        }
        Self::of(info.numer, info.denom)
    }

    #[cfg(not(target_os = "ios"))]
    pub(crate) fn of_the_host() -> Self {
        Self::NANOS
    }

    fn nanos(self, ticks: u64) -> u64 {
        let nanos = u128::from(ticks).saturating_mul(u128::from(self.numer))
            / NonZeroU128::from(self.denom);
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }
}

#[cfg(target_os = "ios")]
const KERN_SUCCESS: i32 = 0;

#[cfg(target_os = "ios")]
#[derive(Default)]
#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

#[cfg(target_os = "ios")]
unsafe extern "C" {
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

pub(crate) struct Rendering {
    fed: Fed,
    pub(crate) empty: Arc<AtomicBool>,
    clock: Arc<StreamClock>,
    timebase: Timebase,
}

impl Rendering {
    pub(crate) fn fed_by(
        source: Box<dyn AudioSource>,
        empty: Arc<AtomicBool>,
        clock: Arc<StreamClock>,
    ) -> (Self, Handing) {
        let (fed, handing) = fed_from(source);
        (
            Self {
                fed,
                empty,
                clock,
                timebase: Timebase::of_the_host(),
            },
            handing,
        )
    }

    #[cfg(test)]
    pub(crate) fn source(&mut self) -> &mut dyn AudioSource {
        self.fed.source()
    }

    pub(crate) fn fill(&mut self, bytes: &mut [u8], host: Option<HostTime>) {
        let stamp = self
            .clock
            .stamp_by(host.map(|host| self.read_by_the_host(host, bytes)));
        let filled = self.fed.source().fill_stamped(bytes, stamp);
        self.empty.store(filled == 0, Ordering::Release);
        if let Some(unfilled) = bytes.get_mut(filled..) {
            unfilled.fill(0);
        }
    }

    fn read_by_the_host(&self, host: HostTime, bytes: &[u8]) -> DeviceTime {
        let frames = (bytes.len() / STEREO_F32_FRAME) as u64;
        DeviceTime {
            clock_now: self.timebase.nanos(host.now),
            read_at: self.timebase.nanos(host.of_the_buffer),
            ahead: Frames(self.clock.latency().get().saturating_sub(frames)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::time::Duration;

    use parking_lot::Mutex;
    use resonate_audio::{Moment, Stamp};

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

    fn rendering(left: usize) -> (Rendering, Arc<AtomicBool>) {
        let empty = Arc::new(AtomicBool::new(false));
        let (rendering, _) = Rendering::fed_by(
            Box::new(Supply { left }),
            Arc::clone(&empty),
            Arc::default(),
        );
        (rendering, empty)
    }

    #[test]
    fn a_full_fill_keeps_every_byte_and_leaves_the_flag_down() {
        let (mut rendering, empty) = rendering(16);
        let mut bytes = [0_u8; 16];

        rendering.fill(&mut bytes, None);

        assert_eq!(bytes, [0xAB; 16]);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_short_fill_zeroes_the_rest_and_leaves_the_flag_down() {
        let (mut rendering, empty) = rendering(6);
        let mut bytes = [0xFF_u8; 16];

        rendering.fill(&mut bytes, None);

        assert_eq!(&bytes[..6], &[0xAB; 6]);
        assert_eq!(&bytes[6..], &[0; 10]);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn an_exhausted_source_silences_the_buffer_and_raises_the_flag() {
        let (mut rendering, empty) = rendering(0);
        let mut bytes = [0xFF_u8; 16];

        rendering.fill(&mut bytes, None);

        assert_eq!(bytes, [0; 16]);
        assert!(empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_zero_length_buffer_reads_as_empty() {
        let (mut rendering, empty) = rendering(16);
        let mut bytes: [u8; 0] = [];

        rendering.fill(&mut bytes, None);

        assert!(empty.load(Ordering::Acquire));
    }

    struct Stamped(Arc<Mutex<Option<Stamp>>>);

    impl AudioSource for Stamped {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            dst.len()
        }

        fn fill_stamped(&mut self, dst: &mut [u8], stamp: Stamp) -> usize {
            *self.0.lock() = Some(stamp);
            self.fill(dst)
        }
    }

    #[test]
    fn the_source_is_told_the_route_latency_it_is_heard_behind() {
        let seen = Arc::new(Mutex::new(None));
        let clock = Arc::new(StreamClock::behind_by(Frames(2_048)));
        let (mut rendering, _) = Rendering::fed_by(
            Box::new(Stamped(Arc::clone(&seen))),
            Arc::new(AtomicBool::new(false)),
            clock,
        );
        let before = Moment::now();

        rendering.fill(&mut [0_u8; 8], None);

        let stamp = seen.lock().expect("the render stamped its fill");
        assert_eq!(stamp.ahead, Frames(2_048));
        assert!(stamp.at >= before);
    }

    #[test]
    fn a_buffer_the_host_timed_is_heard_at_its_host_time_plus_the_routes_latency_past_the_buffer() {
        const BUFFER_FRAMES: u64 = 256;
        let seen = Arc::new(Mutex::new(None));
        let clock = Arc::new(StreamClock::behind_by(Frames(2_048)));
        let (mut rendering, _) = Rendering::fed_by(
            Box::new(Stamped(Arc::clone(&seen))),
            Arc::new(AtomicBool::new(false)),
            clock,
        );
        let host = HostTime {
            now: 9_000_000_000,
            of_the_buffer: 9_004_000_000,
        };
        let before = Moment::now();

        rendering.fill(&mut [0_u8; 8 * BUFFER_FRAMES as usize], Some(host));

        let stamp = seen.lock().expect("the render stamped its fill");
        assert_eq!(stamp.ahead, Frames(2_048 - BUFFER_FRAMES));
        assert!(stamp.at >= before.after(Duration::from_millis(4)));
        assert!(stamp.at <= Moment::now().after(Duration::from_millis(4)));
    }

    #[test]
    fn host_ticks_are_read_in_nanoseconds_through_the_timebase() {
        assert_eq!(Timebase::of(125, 3).nanos(24_000_000), 1_000_000_000);
        assert_eq!(Timebase::NANOS.nanos(42), 42);
        assert_eq!(Timebase::of(1, 0), Timebase::NANOS);
    }

    #[test]
    fn the_flag_falls_again_once_the_source_has_more() {
        let empty = Arc::new(AtomicBool::new(false));
        let (mut rendering, mut handing) = Rendering::fed_by(
            Box::new(Supply { left: 0 }),
            Arc::clone(&empty),
            Arc::default(),
        );
        let mut bytes = [0_u8; 4];

        rendering.fill(&mut bytes, None);
        assert!(empty.load(Ordering::Acquire));

        assert!(handing.hand(Box::new(Supply { left: 4 })).is_ok());
        rendering.fill(&mut bytes, None);
        assert!(!empty.load(Ordering::Acquire));
    }
}
