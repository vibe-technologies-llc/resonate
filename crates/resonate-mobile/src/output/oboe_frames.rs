#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::atomic::{AtomicBool, Ordering};

use resonate_audio::AudioSource;

pub(crate) const FRAME_CAPACITY: usize = 4096;
const WIDEST_FRAME_BYTES: usize = 8;
pub(crate) const RENDER_BYTES: usize = FRAME_CAPACITY * WIDEST_FRAME_BYTES;

pub(crate) trait Word: Copy {
    const BYTES: usize;

    fn read(bytes: &[u8]) -> Option<Self>;
}

impl Word for f32 {
    const BYTES: usize = 4;

    fn read(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self::from_le_bytes)
    }
}

impl Word for i16 {
    const BYTES: usize = 2;

    fn read(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self::from_le_bytes)
    }
}

impl Word for i32 {
    const BYTES: usize = 4;

    fn read(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self::from_le_bytes)
    }
}

pub(crate) trait Frame: Copy {
    const BYTES: usize;

    fn read(bytes: &[u8]) -> Option<Self>;
}

impl<W: Word> Frame for W {
    const BYTES: usize = W::BYTES;

    fn read(bytes: &[u8]) -> Option<Self> {
        <W as Word>::read(bytes)
    }
}

impl<W: Word> Frame for (W, W) {
    const BYTES: usize = W::BYTES * 2;

    fn read(bytes: &[u8]) -> Option<Self> {
        let (left, right) = bytes.split_at_checked(W::BYTES)?;
        Some((<W as Word>::read(left)?, <W as Word>::read(right)?))
    }
}

pub(crate) fn rendered<F: Frame>(
    source: &mut dyn AudioSource,
    bytes: &mut [u8],
    empty: &AtomicBool,
    frames: &mut [F],
) -> bool {
    for chunk in frames.chunks_mut(FRAME_CAPACITY) {
        let Some(bytes) = bytes.get_mut(..chunk.len().saturating_mul(F::BYTES)) else {
            return false;
        };
        let filled = source.fill(bytes);
        empty.store(filled == 0, Ordering::Release);
        if let Some(unfilled) = bytes.get_mut(filled..) {
            unfilled.fill(0);
        }
        for (frame, encoded) in chunk.iter_mut().zip(bytes.chunks_exact(F::BYTES)) {
            let Some(read) = F::read(encoded) else {
                return false;
            };
            *frame = read;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bytes(Vec<u8>);

    impl AudioSource for Bytes {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            let taken = dst.len().min(self.0.len());
            for (into, from) in dst.iter_mut().zip(self.0.drain(..taken)) {
                *into = from;
            }
            taken
        }
    }

    fn rendered_from<F: Frame + Default>(bytes: Vec<u8>, frames: usize) -> (Vec<F>, bool) {
        let mut source = Bytes(bytes);
        let mut held = vec![0; RENDER_BYTES];
        let empty = AtomicBool::new(true);
        let mut out = vec![F::default(); frames];

        assert!(rendered(&mut source, &mut held, &empty, &mut out));
        (out, empty.load(Ordering::Acquire))
    }

    #[test]
    fn every_word_and_layout_reads_the_bytes_the_ring_wrote_in_order() {
        let words: Vec<u8> = [1_i16, -2, 3, -4]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        assert_eq!(
            rendered_from::<(i16, i16)>(words.clone(), 2).0,
            vec![(1, -2), (3, -4)]
        );
        assert_eq!(rendered_from::<i16>(words, 4).0, vec![1, -2, 3, -4]);

        let wide: Vec<u8> = [i32::MAX, i32::MIN]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        assert_eq!(
            rendered_from::<(i32, i32)>(wide, 1).0,
            vec![(i32::MAX, i32::MIN)]
        );

        let floats: Vec<u8> = [0.5_f32, -0.25]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        assert_eq!(rendered_from::<f32>(floats, 2).0, vec![0.5, -0.25]);
    }

    #[test]
    fn a_short_fill_ends_in_silence_and_an_empty_one_raises_the_flag() {
        let one_frame = [7_i16, 8]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let (frames, empty) = rendered_from::<(i16, i16)>(one_frame, 3);
        assert_eq!(frames, vec![(7, 8), (0, 0), (0, 0)]);
        assert!(!empty);

        let (frames, empty) = rendered_from::<(f32, f32)>(Vec::new(), 2);
        assert_eq!(frames, vec![(0.0, 0.0), (0.0, 0.0)]);
        assert!(empty);
    }

    #[test]
    fn a_callback_longer_than_the_bytes_held_is_filled_a_chunk_at_a_time() {
        let frames = FRAME_CAPACITY * 2 + 3;
        let words: Vec<u8> = (0..frames * 2)
            .flat_map(|at| (at as i32).to_le_bytes())
            .collect();

        let (out, _) = rendered_from::<(i32, i32)>(words, frames);

        assert_eq!(out.len(), frames);
        assert!(
            out.iter()
                .enumerate()
                .all(|(at, frame)| *frame == ((at * 2) as i32, (at * 2 + 1) as i32))
        );
    }
}
