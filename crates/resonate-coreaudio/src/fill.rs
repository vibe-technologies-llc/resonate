#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use resonate_audio::{
    AudioSource, DeviceTime, StreamClock,
    handover::{Fed, Handing, fed_from},
    left_aligned,
};
use resonate_core::Frames;

const STEREO_F32_FRAME: NonZeroUsize = match NonZeroUsize::new(8) {
    Some(frame) => frame,
    None => NonZeroUsize::MIN,
};
const A_HOST_TIME_IS_BELIEVED_WITHIN_NANOS: u64 = 500_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostNanos {
    pub now: u64,
    pub of_the_buffer: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Laid {
    AsHeld,
    TwentyFourAgainstTheTop,
}

#[derive(Clone, Debug)]
pub enum Presented {
    BehindTheRouteLessTheBuffer,
    PastTheHostTime(Arc<AtomicU64>),
}

#[derive(Clone, Debug)]
pub struct Shaped {
    pub frame: NonZeroUsize,
    pub laid: Laid,
    pub presented: Presented,
}

impl Shaped {
    pub const fn float_stereo_behind_a_route() -> Self {
        Self {
            frame: STEREO_F32_FRAME,
            laid: Laid::AsHeld,
            presented: Presented::BehindTheRouteLessTheBuffer,
        }
    }
}

pub struct Rendering {
    fed: Fed,
    pub empty: Arc<AtomicBool>,
    clock: Arc<StreamClock>,
    shaped: Shaped,
}

impl Rendering {
    pub fn fed_by(
        source: Box<dyn AudioSource>,
        empty: Arc<AtomicBool>,
        clock: Arc<StreamClock>,
        shaped: Shaped,
    ) -> (Self, Handing) {
        let (fed, handing) = fed_from(source);
        (
            Self {
                fed,
                empty,
                clock,
                shaped,
            },
            handing,
        )
    }

    pub fn fill(&mut self, bytes: &mut [u8], host: Option<HostNanos>) {
        let device = host.and_then(|host| self.read_by_the_host(host, bytes));
        let stamp = self.clock.stamp_by(device);
        let filled = self.fed.source().fill_stamped(bytes, stamp);
        self.empty.store(filled == 0, Ordering::Release);
        if let Some(unfilled) = bytes.get_mut(filled..) {
            unfilled.fill(0);
        }
        if self.shaped.laid == Laid::TwentyFourAgainstTheTop {
            left_aligned(bytes);
        }
    }

    fn read_by_the_host(&self, host: HostNanos, bytes: &[u8]) -> Option<DeviceTime> {
        let ahead = match &self.shaped.presented {
            Presented::BehindTheRouteLessTheBuffer => {
                let frames = (bytes.len() / self.shaped.frame) as u64;
                self.clock.latency().get().saturating_sub(frames)
            }
            Presented::PastTheHostTime(frames) => {
                if host.now.abs_diff(host.of_the_buffer) > A_HOST_TIME_IS_BELIEVED_WITHIN_NANOS {
                    return None;
                }
                frames.load(Ordering::Relaxed)
            }
        };
        Some(DeviceTime {
            clock_now: host.now,
            read_at: host.of_the_buffer,
            ahead: Frames(ahead),
        })
    }
}

#[cfg(target_vendor = "apple")]
impl resonate_apple::Render for Rendering {
    fn render(&mut self, bytes: &mut [u8], host: Option<resonate_apple::HostTime>) {
        let host = host.map(|host| HostNanos {
            now: host.now,
            of_the_buffer: host.of_the_buffer,
        });
        self.fill(bytes, host);
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
            Shaped::float_stereo_behind_a_route(),
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

    fn stamped(clock: Arc<StreamClock>, shaped: Shaped) -> (Rendering, Arc<Mutex<Option<Stamp>>>) {
        let seen = Arc::new(Mutex::new(None));
        let (rendering, _) = Rendering::fed_by(
            Box::new(Stamped(Arc::clone(&seen))),
            Arc::new(AtomicBool::new(false)),
            clock,
            shaped,
        );
        (rendering, seen)
    }

    #[test]
    fn the_source_is_told_the_route_latency_it_is_heard_behind() {
        let clock = Arc::new(StreamClock::behind_by(Frames(2_048)));
        let (mut rendering, seen) = stamped(clock, Shaped::float_stereo_behind_a_route());
        let before = Moment::now();

        rendering.fill(&mut [0_u8; 8], None);

        let stamp = seen.lock().expect("the render stamped its fill");
        assert_eq!(stamp.ahead, Frames(2_048));
        assert!(stamp.at >= before);
    }

    #[test]
    fn a_buffer_the_host_timed_is_heard_at_its_host_time_plus_the_routes_latency_past_the_buffer() {
        const BUFFER_FRAMES: u64 = 256;
        let clock = Arc::new(StreamClock::behind_by(Frames(2_048)));
        let (mut rendering, seen) = stamped(clock, Shaped::float_stereo_behind_a_route());
        let host = HostNanos {
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

    fn past_the_host_time(frames: u64) -> Shaped {
        Shaped {
            frame: STEREO_F32_FRAME,
            laid: Laid::AsHeld,
            presented: Presented::PastTheHostTime(Arc::new(AtomicU64::new(frames))),
        }
    }

    #[test]
    fn a_device_that_times_its_own_output_is_heard_what_lies_past_that_time_after_it() {
        let clock = Arc::new(StreamClock::behind_by(Frames(2_048)));
        let (mut rendering, seen) = stamped(clock, past_the_host_time(300));
        let host = HostNanos {
            now: 9_000_000_000,
            of_the_buffer: 9_010_000_000,
        };
        let before = Moment::now();

        rendering.fill(&mut [0_u8; 4_096], Some(host));

        let stamp = seen.lock().expect("the render stamped its fill");
        assert_eq!(stamp.ahead, Frames(300));
        assert!(stamp.at >= before.after(Duration::from_millis(10)));
        assert!(stamp.at <= Moment::now().after(Duration::from_millis(10)));
    }

    #[test]
    fn a_host_time_far_from_now_is_not_believed_and_the_latency_stands_in() {
        let clock = Arc::new(StreamClock::behind_by(Frames(2_048)));
        let (mut rendering, seen) = stamped(clock, past_the_host_time(300));
        let host = HostNanos {
            now: 9_000_000_000,
            of_the_buffer: 19_000_000_000,
        };
        let before = Moment::now();

        rendering.fill(&mut [0_u8; 8], Some(host));

        let stamp = seen.lock().expect("the render stamped its fill");
        assert_eq!(stamp.ahead, Frames(2_048));
        assert!(stamp.at >= before);
        assert!(stamp.at <= Moment::now());
    }

    struct Words(Vec<i32>);

    impl AudioSource for Words {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            let mut written = 0;
            for (slot, word) in dst.as_chunks_mut::<4>().0.iter_mut().zip(self.0.drain(..)) {
                slot.copy_from_slice(&word.to_le_bytes());
                written += 4;
            }
            written
        }
    }

    #[test]
    fn twenty_four_bits_are_laid_against_the_top_of_the_word_and_a_short_fill_stays_silent() {
        let shaped = Shaped {
            laid: Laid::TwentyFourAgainstTheTop,
            ..past_the_host_time(0)
        };
        let (mut rendering, _) = Rendering::fed_by(
            Box::new(Words(vec![1, -1, 0x7F_FFFF])),
            Arc::new(AtomicBool::new(false)),
            Arc::default(),
            shaped,
        );
        let mut bytes = [0xFF_u8; 16];

        rendering.fill(&mut bytes, None);

        let words: Vec<i32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| i32::from_le_bytes(*word))
            .collect();
        assert_eq!(words, [1 << 8, -1 << 8, 0x7F_FFFF << 8, 0]);
    }

    #[test]
    fn the_flag_falls_again_once_the_source_has_more() {
        let empty = Arc::new(AtomicBool::new(false));
        let (mut rendering, mut handing) = Rendering::fed_by(
            Box::new(Supply { left: 0 }),
            Arc::clone(&empty),
            Arc::default(),
            Shaped::float_stereo_behind_a_route(),
        );
        let mut bytes = [0_u8; 4];

        rendering.fill(&mut bytes, None);
        assert!(empty.load(Ordering::Acquire));

        assert!(handing.hand(Box::new(Supply { left: 4 })).is_ok());
        rendering.fill(&mut bytes, None);
        assert!(!empty.load(Ordering::Acquire));
    }
}
