#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::atomic::{AtomicBool, Ordering};

use resonate_audio::{AudioSource, DeviceTime, Stamp};
use resonate_core::{Frames, SampleRate};

pub(crate) const FRAME_CAPACITY: usize = 4096;
const WIDEST_FRAME_BYTES: usize = 8;
pub(crate) const RENDER_BYTES: usize = FRAME_CAPACITY * WIDEST_FRAME_BYTES;
const NANOS_A_SECOND: i128 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Presented {
    pub(crate) frame: i64,
    pub(crate) at: i64,
}

pub(crate) fn heard_by_the_device(
    written: i64,
    presented: Presented,
    now: u64,
    rate: SampleRate,
) -> Option<DeviceTime> {
    if presented.at <= 0 || presented.frame < 0 {
        return None;
    }
    let after = i128::from(written)
        .saturating_sub(i128::from(presented.frame))
        .saturating_mul(NANOS_A_SECOND)
        .checked_div(i128::from(rate.hz()))?;
    let read_at = u64::try_from(i128::from(presented.at).saturating_add(after)).ok()?;
    Some(DeviceTime {
        clock_now: now,
        read_at,
        ahead: Frames::ZERO,
    })
}

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
    stamp: Stamp,
) -> bool {
    let mut ahead = stamp.ahead;
    for chunk in frames.chunks_mut(FRAME_CAPACITY) {
        let Some(bytes) = bytes.get_mut(..chunk.len().saturating_mul(F::BYTES)) else {
            return false;
        };
        let filled = source.fill_stamped(bytes, Stamp { ahead, ..stamp });
        ahead = Frames(ahead.get().saturating_add(chunk.len() as u64));
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
    use resonate_audio::Moment;

    use super::*;

    struct Bytes {
        left: Vec<u8>,
        stamped: Vec<Stamp>,
    }

    impl AudioSource for Bytes {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            let taken = dst.len().min(self.left.len());
            for (into, from) in dst.iter_mut().zip(self.left.drain(..taken)) {
                *into = from;
            }
            taken
        }

        fn fill_stamped(&mut self, dst: &mut [u8], stamp: Stamp) -> usize {
            self.stamped.push(stamp);
            self.fill(dst)
        }
    }

    fn stamp() -> Stamp {
        Stamp {
            at: Moment::from_nanos(1_000),
            ahead: Frames(100),
        }
    }

    fn rendered_stamped<F: Frame + Default>(
        bytes: Vec<u8>,
        frames: usize,
    ) -> (Vec<F>, bool, Vec<Stamp>) {
        let mut source = Bytes {
            left: bytes,
            stamped: Vec::new(),
        };
        let mut held = vec![0; RENDER_BYTES];
        let empty = AtomicBool::new(true);
        let mut out = vec![F::default(); frames];

        assert!(rendered(&mut source, &mut held, &empty, &mut out, stamp()));
        (out, empty.load(Ordering::Acquire), source.stamped)
    }

    fn rendered_from<F: Frame + Default>(bytes: Vec<u8>, frames: usize) -> (Vec<F>, bool) {
        let (out, empty, _) = rendered_stamped(bytes, frames);
        (out, empty)
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
    fn the_first_frame_written_is_heard_as_far_past_the_presented_frame_as_it_lies_behind_it() {
        let presented = Presented {
            frame: 48_000,
            at: 5_000_000_000,
        };

        let reading = heard_by_the_device(48_480, presented, 5_002_000_000, SampleRate::HZ_48000);
        let ahead_of_it =
            heard_by_the_device(47_520, presented, 5_002_000_000, SampleRate::HZ_48000);

        assert_eq!(
            reading,
            Some(DeviceTime {
                clock_now: 5_002_000_000,
                read_at: 5_010_000_000,
                ahead: Frames::ZERO,
            })
        );
        assert_eq!(
            ahead_of_it.map(|reading| reading.read_at),
            Some(4_990_000_000)
        );
    }

    #[test]
    fn a_device_that_has_presented_nothing_yet_leaves_the_stamp_to_the_clock() {
        let unread = Presented { frame: 0, at: 0 };

        assert_eq!(
            heard_by_the_device(256, unread, 1_000, SampleRate::HZ_48000),
            None
        );
    }

    #[test]
    fn a_callback_longer_than_the_bytes_held_is_filled_a_chunk_at_a_time() {
        let frames = FRAME_CAPACITY * 2 + 3;
        let words: Vec<u8> = (0..frames * 2)
            .flat_map(|at| (at as i32).to_le_bytes())
            .collect();

        let (out, _, stamped) = rendered_stamped::<(i32, i32)>(words, frames);

        assert_eq!(
            stamped.iter().map(|stamp| stamp.ahead).collect::<Vec<_>>(),
            vec![
                Frames(100),
                Frames(100 + FRAME_CAPACITY as u64),
                Frames(100 + 2 * FRAME_CAPACITY as u64)
            ],
            "a later chunk was not told it is heard after the chunks before it"
        );
        assert!(stamped.iter().all(|each| each.at == stamp().at));
        assert_eq!(out.len(), frames);
        assert!(
            out.iter()
                .enumerate()
                .all(|(at, frame)| *frame == ((at * 2) as i32, (at * 2 + 1) as i32))
        );
    }
}
