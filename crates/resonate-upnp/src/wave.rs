use resonate_core::{SampleFormat, StreamSpec};

use crate::Carriage;

pub(crate) const HEADER_BYTES: usize = 44;

const PCM: u16 = 1;

const FMT_BYTES: u32 = 16;

const LENGTH_UNKNOWN: u32 = u32::MAX;

const RING_WORD_BYTES_S24: usize = 4;

const WIRE_WORD_BYTES_S24: usize = 3;

pub(crate) const fn wire_bytes(format: SampleFormat) -> usize {
    match format {
        SampleFormat::S24 => WIRE_WORD_BYTES_S24,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => {
            format.bytes_per_sample().get() as usize
        }
    }
}

pub(crate) fn header(spec: StreamSpec) -> [u8; HEADER_BYTES] {
    let channels = u16::from(spec.channel_count().get());
    let sample_bytes = wire_bytes(spec.format) as u16;
    let block = channels.saturating_mul(sample_bytes);
    let per_second = spec.rate.hz().saturating_mul(u32::from(block));
    let bits = u16::from(spec.format.valid_bits());

    let mut header = [0; HEADER_BYTES];
    let fields: [&[u8]; 12] = [
        b"RIFF",
        &LENGTH_UNKNOWN.to_le_bytes(),
        b"WAVEfmt ",
        &FMT_BYTES.to_le_bytes(),
        &PCM.to_le_bytes(),
        &channels.to_le_bytes(),
        &spec.rate.hz().to_le_bytes(),
        &per_second.to_le_bytes(),
        &block.to_le_bytes(),
        &bits.to_le_bytes(),
        b"data",
        &(LENGTH_UNKNOWN - HEADER_BYTES as u32 + 8).to_le_bytes(),
    ];
    let mut at = 0;
    for field in fields {
        if let Some(slot) = header.get_mut(at..at + field.len()) {
            slot.copy_from_slice(field);
        }
        at += field.len();
    }
    header
}

pub(crate) fn carried(ring: &[u8], format: SampleFormat, carriage: &Carriage, wire: &mut Vec<u8>) {
    wire.clear();
    match (format, carriage) {
        (SampleFormat::S24, _) => {
            for [low, middle, high, _] in ring.as_chunks::<RING_WORD_BYTES_S24>().0 {
                wire.extend_from_slice(&[*low, *middle, *high]);
            }
        }
        (SampleFormat::S16, Carriage::L16(_)) => {
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
            &Carriage::Wave,
            &mut wire,
        );
        assert_eq!(wire, [0x01, 0x02, 0x03, 0xfd, 0xfe, 0xff]);

        carried(
            &[0x34, 0x12, 0xcd, 0xab],
            SampleFormat::S16,
            &Carriage::L16(Vec::new()),
            &mut wire,
        );
        assert_eq!(wire, [0x12, 0x34, 0xab, 0xcd]);

        carried(&[0x34, 0x12], SampleFormat::S16, &Carriage::Wave, &mut wire);
        assert_eq!(wire, [0x34, 0x12]);
    }
}
