#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::num::NonZeroUsize;

use resonate_audio::{AudioSource, Stamp};
use resonate_core::SampleFormat;

const WORD_BYTES: usize = 4;
const LEFT_ALIGNED_BY_BITS: u32 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filled {
    pub frames: usize,
    pub heard: usize,
}

pub fn fill_buffer(
    source: &mut dyn AudioSource,
    scratch: &mut [u8],
    frame_bytes: NonZeroUsize,
    wanted: usize,
    format: SampleFormat,
    stamp: Stamp,
) -> Filled {
    let Some(bytes) = wanted.checked_mul(frame_bytes.get()) else {
        return Filled::default();
    };
    let Some(window) = scratch.get_mut(..bytes) else {
        return Filled::default();
    };

    let filled = source.fill_stamped(window, stamp).min(bytes);
    if let Some(rest) = window.get_mut(filled..) {
        rest.fill(0);
    }
    if format == SampleFormat::S24 {
        left_aligned(window);
    }

    Filled {
        frames: wanted,
        heard: filled / frame_bytes,
    }
}

fn left_aligned(window: &mut [u8]) {
    let (words, _) = window.as_chunks_mut::<WORD_BYTES>();
    for word in words {
        let shifted = i32::from_le_bytes(*word) << LEFT_ALIGNED_BY_BITS;
        *word = shifted.to_le_bytes();
    }
}

#[cfg(test)]
mod tests {
    use resonate_audio::Moment;
    use resonate_core::Frames;

    use super::*;

    struct Ramp {
        words: Vec<i32>,
    }

    impl AudioSource for Ramp {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            let mut written = 0;
            for (slot, word) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(self.words.drain(..))
            {
                slot.copy_from_slice(&word.to_le_bytes());
                written += 4;
            }
            written
        }
    }

    fn eight_bytes() -> NonZeroUsize {
        NonZeroUsize::MIN
            .checked_add(7)
            .unwrap_or(NonZeroUsize::MIN)
    }

    fn stamp() -> Stamp {
        Stamp {
            at: Moment::from_nanos(0),
            ahead: Frames::ZERO,
        }
    }

    fn words(bytes: &[u8]) -> Vec<i32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| i32::from_le_bytes(*word))
            .collect()
    }

    #[test]
    fn a_short_fill_is_padded_with_silence_and_counts_only_what_was_heard() {
        let mut source = Ramp {
            words: vec![1, 2, 3, 4],
        };
        let mut scratch = [0xAA_u8; 32];
        let frame_bytes = eight_bytes();

        let filled = fill_buffer(
            &mut source,
            &mut scratch,
            frame_bytes,
            4,
            SampleFormat::S32,
            stamp(),
        );

        assert_eq!(
            filled,
            Filled {
                frames: 4,
                heard: 2
            }
        );
        assert_eq!(words(&scratch), [1, 2, 3, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn twenty_four_bits_are_laid_against_the_top_of_the_word() {
        let mut source = Ramp {
            words: vec![1, -1, 0x7F_FFFF, -0x80_0000],
        };
        let mut scratch = [0_u8; 16];
        let frame_bytes = eight_bytes();

        fill_buffer(
            &mut source,
            &mut scratch,
            frame_bytes,
            2,
            SampleFormat::S24,
            stamp(),
        );

        assert_eq!(
            words(&scratch),
            [1 << 8, -1 << 8, 0x7F_FFFF << 8, -0x80_0000 << 8]
        );
    }

    #[test]
    fn a_request_larger_than_the_scratch_is_refused_not_cut() {
        let mut source = Ramp { words: vec![1] };
        let mut scratch = [0_u8; 8];
        let frame_bytes = eight_bytes();

        let filled = fill_buffer(
            &mut source,
            &mut scratch,
            frame_bytes,
            2,
            SampleFormat::S32,
            stamp(),
        );

        assert_eq!(filled, Filled::default());
    }
}
