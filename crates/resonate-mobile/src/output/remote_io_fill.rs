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

use resonate_audio::AudioSource;

use super::handover::{Fed, Handing, fed_from};

pub(crate) struct Rendering {
    fed: Fed,
    pub(crate) empty: Arc<AtomicBool>,
}

impl Rendering {
    pub(crate) fn fed_by(source: Box<dyn AudioSource>, empty: Arc<AtomicBool>) -> (Self, Handing) {
        let (fed, handing) = fed_from(source);
        (Self { fed, empty }, handing)
    }

    #[cfg(test)]
    pub(crate) fn source(&mut self) -> &mut dyn AudioSource {
        self.fed.source()
    }

    pub(crate) fn fill(&mut self, bytes: &mut [u8]) {
        let filled = self.fed.source().fill(bytes);
        self.empty.store(filled == 0, Ordering::Release);
        if let Some(unfilled) = bytes.get_mut(filled..) {
            unfilled.fill(0);
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
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
        let (rendering, _) = Rendering::fed_by(Box::new(Supply { left }), Arc::clone(&empty));
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

    #[test]
    fn the_flag_falls_again_once_the_source_has_more() {
        let empty = Arc::new(AtomicBool::new(false));
        let (mut rendering, mut handing) =
            Rendering::fed_by(Box::new(Supply { left: 0 }), Arc::clone(&empty));
        let mut bytes = [0_u8; 4];

        rendering.fill(&mut bytes);
        assert!(empty.load(Ordering::Acquire));

        assert!(handing.hand(Box::new(Supply { left: 4 })).is_ok());
        rendering.fill(&mut bytes);
        assert!(!empty.load(Ordering::Acquire));
    }
}
