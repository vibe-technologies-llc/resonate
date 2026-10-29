use resonate_core::{ChannelLayout, ChannelPosition, SampleFormat, StreamSpec};

use crate::Wrapping;

pub(crate) const HEADER_BYTES: usize = 44;

pub(crate) const EXTENSIBLE_HEADER_BYTES: usize = 68;

const PCM: u16 = 1;

const EXTENSIBLE: u16 = 0xFFFE;

const FMT_BYTES: u32 = 16;

const FMT_EXTENSIBLE_BYTES: u32 = 40;

const EXTENSION_BYTES: u16 = 22;

const PCM_SUBFORMAT: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];

const LENGTH_UNKNOWN: u32 = u32::MAX;

const RING_WORD_BYTES_S24: usize = 4;

const WIRE_WORD_BYTES_S24: usize = 3;

const PLAIN_CHANNELS_AT_MOST: u16 = 2;

pub(crate) const fn wire_bytes(format: SampleFormat) -> usize {
    match format {
        SampleFormat::S24 => WIRE_WORD_BYTES_S24,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => {
            format.bytes_per_sample().get() as usize
        }
    }
}

const fn speaker_bit(position: ChannelPosition) -> u32 {
    match position {
        ChannelPosition::FrontLeft => 0x1,
        ChannelPosition::FrontRight => 0x2,
        ChannelPosition::FrontCenter => 0x4,
        ChannelPosition::Lfe => 0x8,
        ChannelPosition::RearLeft => 0x10,
        ChannelPosition::RearRight => 0x20,
        ChannelPosition::RearCenter => 0x100,
        ChannelPosition::SideLeft => 0x200,
        ChannelPosition::SideRight => 0x400,
    }
}

pub(crate) fn speaker_mask(layout: ChannelLayout) -> u32 {
    layout
        .positions()
        .iter()
        .fold(0, |mask, position| mask | speaker_bit(*position))
}

pub(crate) fn header(spec: StreamSpec) -> Vec<u8> {
    let channels = u16::from(spec.channel_count().get());
    let sample_bytes = wire_bytes(spec.format) as u16;
    let block = channels.saturating_mul(sample_bytes);
    let per_second = spec.rate.hz().saturating_mul(u32::from(block));
    let valid_bits = u16::from(spec.format.valid_bits());
    let container_bits = sample_bytes * 8;
    let extensible = channels > PLAIN_CHANNELS_AT_MOST;
    let data_bytes = LENGTH_UNKNOWN
        - if extensible {
            EXTENSIBLE_HEADER_BYTES
        } else {
            HEADER_BYTES
        } as u32
        + 8;

    let mut header = Vec::with_capacity(EXTENSIBLE_HEADER_BYTES);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&LENGTH_UNKNOWN.to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    if extensible {
        header.extend_from_slice(&FMT_EXTENSIBLE_BYTES.to_le_bytes());
        header.extend_from_slice(&EXTENSIBLE.to_le_bytes());
    } else {
        header.extend_from_slice(&FMT_BYTES.to_le_bytes());
        header.extend_from_slice(&PCM.to_le_bytes());
    }
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&spec.rate.hz().to_le_bytes());
    header.extend_from_slice(&per_second.to_le_bytes());
    header.extend_from_slice(&block.to_le_bytes());
    if extensible {
        header.extend_from_slice(&container_bits.to_le_bytes());
        header.extend_from_slice(&EXTENSION_BYTES.to_le_bytes());
        header.extend_from_slice(&valid_bits.to_le_bytes());
        header.extend_from_slice(&speaker_mask(spec.channels).to_le_bytes());
        header.extend_from_slice(&PCM_SUBFORMAT);
    } else {
        header.extend_from_slice(&valid_bits.to_le_bytes());
    }
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    header
}

pub(crate) fn carried(ring: &[u8], format: SampleFormat, wrapping: Wrapping, wire: &mut Vec<u8>) {
    wire.clear();
    match (format, wrapping) {
        (SampleFormat::S24, _) => {
            for [low, middle, high, _] in ring.as_chunks::<RING_WORD_BYTES_S24>().0 {
                wire.extend_from_slice(&[*low, *middle, *high]);
            }
        }
        (SampleFormat::S16, Wrapping::L16) => {
            for [low, high] in ring.as_chunks::<2>().0 {
                wire.extend_from_slice(&[*high, *low]);
            }
        }
        (_, _) => wire.extend_from_slice(ring),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, SampleRate};

    use super::*;

    #[test]
    fn the_header_names_the_streams_shape_and_no_length() {
        let header = header(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ));

        assert_eq!(&header[..4], b"RIFF");
        assert_eq!(&header[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([header[22], header[23]]), 2);
        assert_eq!(
            u32::from_le_bytes([header[24], header[25], header[26], header[27]]),
            96_000
        );
        assert_eq!(
            u32::from_le_bytes([header[28], header[29], header[30], header[31]]),
            96_000 * 6
        );
        assert_eq!(u16::from_le_bytes([header[32], header[33]]), 6);
        assert_eq!(u16::from_le_bytes([header[34], header[35]]), 24);
        assert_eq!(&header[36..40], b"data");
    }

    #[test]
    fn twenty_four_bits_go_out_packed_and_l16_goes_out_big_endian() {
        let mut wire = Vec::new();

        carried(
            &[0x01, 0x02, 0x03, 0x00, 0xfd, 0xfe, 0xff, 0xff],
            SampleFormat::S24,
            Wrapping::Wave,
            &mut wire,
        );
        assert_eq!(wire, [0x01, 0x02, 0x03, 0xfd, 0xfe, 0xff]);

        carried(
            &[0x34, 0x12, 0xcd, 0xab],
            SampleFormat::S16,
            Wrapping::L16,
            &mut wire,
        );
        assert_eq!(wire, [0x12, 0x34, 0xab, 0xcd]);

        carried(&[0x34, 0x12], SampleFormat::S16, Wrapping::Wave, &mut wire);
        assert_eq!(wire, [0x34, 0x12]);
    }
}
