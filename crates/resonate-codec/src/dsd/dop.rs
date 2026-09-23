use resonate_core::{SampleFormat, Silence};

use crate::dsd::{BitOrder, DSD_SILENCE};

const DOP_MARK_FIRST: u8 = 0x05;
const DOP_MARK_SECOND: u8 = 0xFA;
const DOP_MARK_SHIFT: u32 = 16;
const SIGN_EXTEND_FROM_24: u32 = 8;
const DOP_SILENT_PAIR: u32 = ((DSD_SILENCE as u32) << 8) | DSD_SILENCE as u32;

pub(crate) const BIT_REVERSE: [u8; 256] = reversed();

const fn reversed() -> [u8; 256] {
    let mut table = [0_u8; 256];
    let mut value = 0_usize;
    while value < 256 {
        let mut held = value as u8;
        let mut turned = 0_u8;
        let mut bit = 0;
        while bit < 8 {
            turned = (turned << 1) | (held & 1);
            held >>= 1;
            bit += 1;
        }
        table[value] = turned;
        value += 1;
    }
    table
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dop {
    bits: BitOrder,
}

impl Dop {
    pub(crate) const fn reading(bits: BitOrder) -> Self {
        Self { bits }
    }

    pub(crate) const fn marker(frame: u64) -> u8 {
        if frame.is_multiple_of(2) {
            DOP_MARK_FIRST
        } else {
            DOP_MARK_SECOND
        }
    }

    pub(crate) fn ordered(self, byte: u8) -> u8 {
        match self.bits {
            BitOrder::LeastSignificantFirst => BIT_REVERSE[usize::from(byte)],
            BitOrder::MostSignificantFirst => byte,
        }
    }

    pub(crate) fn frame(self, frame: u64, earlier: u8, later: u8) -> i32 {
        let held = (u32::from(self.ordered(earlier)) << 8) | u32::from(self.ordered(later));
        packed(Self::marker(frame), held)
    }

    pub(crate) fn silence(format: SampleFormat) -> Silence {
        Silence::Marked {
            word: packed(DOP_MARK_FIRST, DOP_SILENT_PAIR).to_le_bytes(),
            alternate: packed(DOP_MARK_SECOND, DOP_SILENT_PAIR).to_le_bytes(),
            format,
        }
    }
}

pub(crate) const fn packed(marker: u8, held: u32) -> i32 {
    let word = ((marker as u32) << DOP_MARK_SHIFT) | (held & 0xFFFF);
    ((word << SIGN_EXTEND_FROM_24) as i32) >> SIGN_EXTEND_FROM_24
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_marker_reaches_the_sink_as_a_negative_sample() {
        assert_eq!(packed(DOP_MARK_FIRST, 0x1234), 0x0005_1234);
        assert_eq!(packed(DOP_MARK_SECOND, 0x1234), -388_556);
    }

    #[test]
    fn a_marked_sample_keeps_its_marker_in_the_bytes_a_sink_reads() {
        for (marker, held) in [(DOP_MARK_FIRST, 0xBEEF_u32), (DOP_MARK_SECOND, 0x0000)] {
            let bytes = packed(marker, held).to_le_bytes();
            assert_eq!(
                bytes[2], marker,
                "the marker left the byte a DAC reads it in"
            );
            assert_eq!(u32::from(bytes[1]) << 8 | u32::from(bytes[0]), held);
        }
    }

    #[test]
    fn the_marker_alternates_per_frame_and_starts_at_the_first_one() {
        assert_eq!(Dop::marker(0), DOP_MARK_FIRST);
        assert_eq!(Dop::marker(1), DOP_MARK_SECOND);
        assert_eq!(Dop::marker(2), DOP_MARK_FIRST);
        assert_eq!(Dop::marker(1_001), DOP_MARK_SECOND);
    }

    #[test]
    fn a_dsf_plane_is_bit_reversed_where_a_dff_plane_is_not() {
        let least = Dop::reading(BitOrder::LeastSignificantFirst);
        let most = Dop::reading(BitOrder::MostSignificantFirst);

        assert_eq!(least.ordered(0b1000_0000), 0b0000_0001);
        assert_eq!(most.ordered(0b1000_0000), 0b1000_0000);
        assert_eq!(least.frame(0, 0x01, 0x80) & 0xFFFF, 0x8001);
        assert_eq!(most.frame(0, 0x01, 0x80) & 0xFFFF, 0x0180);
    }

    #[test]
    fn dop_silence_carries_the_alternating_marker_over_a_silent_dsd_pair() {
        let Silence::Marked {
            word,
            alternate,
            format,
        } = Dop::silence(SampleFormat::S24)
        else {
            panic!("a DoP stream went silent without its markers");
        };

        assert_eq!(format, SampleFormat::S24);
        assert_eq!(word, [DSD_SILENCE, DSD_SILENCE, DOP_MARK_FIRST, 0x00]);
        assert_eq!(
            alternate,
            [DSD_SILENCE, DSD_SILENCE, DOP_MARK_SECOND, 0xFF],
            "the second marker left the sign the sample carries"
        );
    }

    #[test]
    fn reversing_a_byte_twice_is_the_byte_again() {
        for value in 0..=u8::MAX {
            let turned = BIT_REVERSE[usize::from(value)];
            assert_eq!(BIT_REVERSE[usize::from(turned)], value);
        }
    }
}
