#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use resonate_audio::{AudioSource, StreamClock};

use super::handover::{Fed, Handing, fed_from};

pub(crate) struct Rendering {
    fed: Fed,
    pub(crate) empty: Arc<AtomicBool>,
    clock: Arc<StreamClock>,
}

impl Rendering {
    pub(crate) fn fed_by(
        source: Box<dyn AudioSource>,
        empty: Arc<AtomicBool>,
        clock: Arc<StreamClock>,
    ) -> (Self, Handing) {
        let (fed, handing) = fed_from(source);
        (Self { fed, empty, clock }, handing)
    }

    #[cfg(test)]
    pub(crate) fn source(&mut self) -> &mut dyn AudioSource {
        self.fed.source()
    }

    pub(crate) fn fill(&mut self, bytes: &mut [u8]) {
        let stamp = self.clock.stamp();
        let filled = self.fed.source().fill_stamped(bytes, stamp);
        self.empty.store(filled == 0, Ordering::Release);
        if let Some(unfilled) = bytes.get_mut(filled..) {
            unfilled.fill(0);
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use parking_lot::Mutex;
    use resonate_audio::{Moment, Stamp};
    use resonate_core::Frames;

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

        rendering.fill(&mut bytes);

        assert_eq!(bytes, [0xAB; 16]);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_short_fill_zeroes_the_rest_and_leaves_the_flag_down() {
        let (mut rendering, empty) = rendering(6);
        let mut bytes = [0xFF_u8; 16];

        rendering.fill(&mut bytes);

        assert_eq!(&bytes[..6], &[0xAB; 6]);
        assert_eq!(&bytes[6..], &[0; 10]);
        assert!(!empty.load(Ordering::Acquire));
    }

    #[test]
    fn an_exhausted_source_silences_the_buffer_and_raises_the_flag() {
        let (mut rendering, empty) = rendering(0);
        let mut bytes = [0xFF_u8; 16];

        rendering.fill(&mut bytes);

        assert_eq!(bytes, [0; 16]);
        assert!(empty.load(Ordering::Acquire));
    }

    #[test]
    fn a_zero_length_buffer_reads_as_empty() {
        let (mut rendering, empty) = rendering(16);
        let mut bytes: [u8; 0] = [];

        rendering.fill(&mut bytes);

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

        rendering.fill(&mut [0_u8; 8]);

        let stamp = seen.lock().expect("the render stamped its fill");
        assert_eq!(stamp.ahead, Frames(2_048));
        assert!(stamp.at >= before);
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

        rendering.fill(&mut bytes);
        assert!(empty.load(Ordering::Acquire));

        assert!(handing.hand(Box::new(Supply { left: 4 })).is_ok());
        rendering.fill(&mut bytes);
        assert!(!empty.load(Ordering::Acquire));
    }
}
