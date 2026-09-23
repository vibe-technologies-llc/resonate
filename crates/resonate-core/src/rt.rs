use std::num::NonZeroU32;

use thiserror::Error;

use crate::SampleFormat;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Error)]
pub enum RtFault {
    #[error("underrun: the graph asked for more than the ring held")]
    Underrun,

    #[error("{count} faults were dropped because the escalation queue was full")]
    Dropped { count: u32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Silence {
    #[default]
    Unmarked,
    Marked {
        word: [u8; SampleFormat::MAX_BYTES],
        alternate: [u8; SampleFormat::MAX_BYTES],
        format: SampleFormat,
    },
}

const SIGN_BIT: u8 = 0x80;

impl Silence {
    pub fn would_write(&self, frame: &[u8]) -> bool {
        let Self::Marked { word, format, .. } = self else {
            return false;
        };
        let width = usize::from(format.bytes_per_sample().get());
        match (marks_negative(frame, width), marks_negative(word, width)) {
            (Some(carried), Some(due)) => carried == due,
            _ => false,
        }
    }

    pub fn follows(&mut self, frame: &[u8]) {
        if !self.would_write(frame) {
            return;
        }
        if let Self::Marked {
            word, alternate, ..
        } = self
        {
            std::mem::swap(word, alternate);
        }
    }

    pub fn write(&mut self, dst: &mut [u8], bytes_per_frame: NonZeroU32) -> usize {
        let Self::Marked {
            word,
            alternate,
            format,
        } = self
        else {
            return 0;
        };

        let stride = bytes_per_frame.get() as usize;
        let width = usize::from(format.bytes_per_sample().get());
        let mut written = 0_usize;

        for frame in dst.chunks_exact_mut(stride) {
            let Some(marked) = word.get(..width) else {
                break;
            };
            for (slot, byte) in frame.iter_mut().zip(marked.iter().cycle()) {
                *slot = *byte;
            }
            std::mem::swap(word, alternate);
            written = written.saturating_add(stride);
        }

        written
    }
}

fn marks_negative(sample: &[u8], width: usize) -> Option<bool> {
    let top = width.checked_sub(1)?;
    sample.get(top).map(|byte| byte & SIGN_BIT != 0)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::{ChannelLayout, SampleRate, StreamSpec};

    fn stereo(format: SampleFormat) -> NonZeroU32 {
        StreamSpec::new(SampleRate::HZ_176400, ChannelLayout::Stereo, format).bytes_per_frame()
    }

    fn dop_silence() -> Silence {
        Silence::Marked {
            word: [0x69, 0x69, 0x05, 0x00],
            alternate: [0x69, 0x69, 0xFA, 0xFF],
            format: SampleFormat::S24,
        }
    }

    const POSITIVE: [u8; 4] = [0x34, 0x12, 0x05, 0x00];
    const NEGATIVE: [u8; 4] = [0x34, 0x12, 0xFA, 0xFF];

    #[test]
    fn a_marker_is_read_off_the_sign_the_word_reaches_the_sink_as() {
        let silence = dop_silence();

        assert!(silence.would_write(&POSITIVE), "0x05 is the word due first");
        assert!(!silence.would_write(&NEGATIVE));
    }

    #[test]
    fn silence_takes_the_marker_that_follows_the_frame_it_is_given() {
        let mut silence = dop_silence();
        silence.follows(&POSITIVE);

        let mut dst = [0_u8; 8];
        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S24)), 8);
        assert_eq!(dst[2], 0xFA, "silence repeated the marker it followed");

        let mut silence = dop_silence();
        silence.follows(&NEGATIVE);
        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S24)), 8);
        assert_eq!(dst[2], 0x05);
    }

    #[test]
    fn unmarked_silence_follows_nothing_and_is_due_nothing() {
        let mut silence = Silence::default();
        silence.follows(&POSITIVE);

        assert_eq!(silence, Silence::Unmarked);
        assert!(!silence.would_write(&POSITIVE));
    }

    #[test]
    fn unmarked_silence_writes_nothing_and_leaves_the_buffer_to_the_graph() {
        let mut silence = Silence::default();
        let mut dst = [0xAA_u8; 16];

        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S24)), 0);
        assert_eq!(dst, [0xAA; 16], "unmarked silence touched the buffer");
    }

    #[test]
    fn a_marked_word_repeats_across_the_channels_of_one_frame() {
        let mut silence = Silence::Marked {
            word: [1, 2, 3, 4],
            alternate: [5, 6, 7, 8],
            format: SampleFormat::S24,
        };
        let mut dst = [0_u8; 8];

        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S24)), 8);
        assert_eq!(dst, [1, 2, 3, 4, 1, 2, 3, 4]);
    }

    #[test]
    fn a_marked_word_and_its_alternate_trade_places_every_frame() {
        let mut silence = Silence::Marked {
            word: [1, 2, 3, 4],
            alternate: [5, 6, 7, 8],
            format: SampleFormat::S24,
        };
        let mut dst = [0_u8; 24];

        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S24)), 24);
        assert_eq!(
            dst,
            [
                1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8, 1, 2, 3, 4, 1, 2, 3, 4
            ]
        );

        let mut next = [0_u8; 8];
        silence.write(&mut next, stereo(SampleFormat::S24));
        assert_eq!(next, [5, 6, 7, 8, 5, 6, 7, 8], "the parity did not carry");
    }

    #[test]
    fn a_word_narrower_than_the_widest_writes_only_the_bytes_the_format_holds() {
        let mut silence = Silence::Marked {
            word: [1, 2, 3, 4],
            alternate: [5, 6, 7, 8],
            format: SampleFormat::S16,
        };
        let mut dst = [0_u8; 8];

        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S16)), 8);
        assert_eq!(dst, [1, 2, 1, 2, 5, 6, 5, 6]);
    }

    #[test]
    fn a_buffer_short_of_a_whole_frame_is_left_alone() {
        let mut silence = Silence::Marked {
            word: [1, 2, 3, 4],
            alternate: [5, 6, 7, 8],
            format: SampleFormat::S24,
        };
        let mut dst = [0_u8; 5];

        assert_eq!(silence.write(&mut dst, stereo(SampleFormat::S24)), 0);
        assert_eq!(dst, [0; 5]);
    }

    #[test]
    fn silence_never_allocates_and_stays_small() {
        assert!(!std::mem::needs_drop::<Silence>());
        assert!(size_of::<Silence>() <= 16, "{}", size_of::<Silence>());
    }

    #[test]
    fn rt_fault_never_allocates_and_stays_small() {
        assert!(!std::mem::needs_drop::<RtFault>());
        assert!(size_of::<RtFault>() <= 32, "{}", size_of::<RtFault>());
    }
}
