use std::io::{Read, Seek, SeekFrom};

use crate::prescan::{Opened, opened_first, past_id3, read_exact};

const CAFF: &[u8; 4] = b"caff";
const DESC: &[u8; 4] = b"desc";
const DATA: &[u8; 4] = b"data";
const PAKT: &[u8; 4] = b"pakt";

const FILE_VERSION: u16 = 1;
const FILE_HEADER_BYTES: u64 = 8;
const DESC_BYTES: i64 = 32;
const EDIT_COUNT_BYTES: i64 = 4;
const PAKT_HEADER_BYTES: i64 = 24;
const UNKNOWN_SIZE: i64 = -1;
const MAX_CHUNKS: usize = 4_096;
const BITS_PER_BYTE: u32 = 8;
const VARIABLE_LENGTH_BYTES_AT_MOST: usize = 9;
const VARIABLE_LENGTH_MORE: u8 = 0x80;
const VARIABLE_LENGTH_BITS: u8 = 0x7f;
const VARIABLE_LENGTH_SHIFT: u32 = 7;
const MOST_FRAMES_A_PACKET: u32 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overflow {
    PacketBits {
        bytes_per_packet: u32,
    },
    FrameCount {
        packets: u64,
        frames_per_packet: u32,
    },
    PacketOffset {
        packets: u64,
    },
    PacketFrames {
        frames_per_packet: u32,
    },
}

#[derive(Clone, Copy, Debug)]
struct Description {
    bytes_per_packet: u32,
    frames_per_packet: u32,
}

pub(crate) fn read<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Overflow> {
    let origin = source.stream_position().ok()?;
    let found = scan(source);
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("a CAF scan could not restore the stream position");
    }
    found
}

fn scan<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Overflow> {
    let start = past_id3(source)?;
    let (Opened::Caf, header) = opened_first(source, start)? else {
        return None;
    };
    source
        .seek(SeekFrom::Start(header + CAFF.len() as u64))
        .ok()?;
    let version = read_exact::<2, S>(source)?;
    if u16::from_be_bytes(version) != FILE_VERSION {
        return None;
    }
    source
        .seek(SeekFrom::Start(header + FILE_HEADER_BYTES))
        .ok()?;

    let (id, size) = chunk_header(source)?;
    if id != *DESC || size != DESC_BYTES {
        return None;
    }
    let desc = description(source)?;
    if desc.bytes_per_packet.checked_mul(BITS_PER_BYTE).is_none() {
        return Some(Overflow::PacketBits {
            bytes_per_packet: desc.bytes_per_packet,
        });
    }
    if desc.frames_per_packet > MOST_FRAMES_A_PACKET {
        return Some(Overflow::PacketFrames {
            frames_per_packet: desc.frames_per_packet,
        });
    }

    for _ in 0..MAX_CHUNKS {
        let (id, size) = chunk_header(source)?;
        let body = source.stream_position().ok()?;
        if id == *DATA {
            if size == UNKNOWN_SIZE {
                return None;
            }
            if let Some(overflow) = frame_count(desc, size) {
                return Some(overflow);
            }
        }
        if id == *PAKT
            && let Some(overflow) = packet_table(source, desc, size)
        {
            return Some(overflow);
        }
        let next = body.checked_add(u64::try_from(size).ok()?)?;
        source.seek(SeekFrom::Start(next)).ok()?;
    }

    None
}

fn chunk_header<S: Read + ?Sized>(source: &mut S) -> Option<([u8; 4], i64)> {
    let header = read_exact::<12, S>(source)?;
    let id: [u8; 4] = header.get(..4)?.try_into().ok()?;
    let size: [u8; 8] = header.get(4..)?.try_into().ok()?;
    Some((id, i64::from_be_bytes(size)))
}

fn description<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Description> {
    let fields = read_exact::<{ DESC_BYTES as usize }, S>(source)?;
    let word = |at: usize| -> Option<u32> {
        let bytes: [u8; 4] = fields.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    };
    Some(Description {
        bytes_per_packet: word(16)?,
        frames_per_packet: word(20)?,
    })
}

fn frame_count(desc: Description, size: i64) -> Option<Overflow> {
    if desc.bytes_per_packet == 0 || desc.frames_per_packet == 0 {
        return None;
    }
    let audio = u64::try_from(size.checked_sub(EDIT_COUNT_BYTES)?).ok()?;
    let packets = audio / u64::from(desc.bytes_per_packet);
    packets
        .checked_mul(u64::from(desc.frames_per_packet))
        .is_none()
        .then_some(Overflow::FrameCount {
            packets,
            frames_per_packet: desc.frames_per_packet,
        })
}

fn packet_table<S: Read + ?Sized>(
    source: &mut S,
    desc: Description,
    size: i64,
) -> Option<Overflow> {
    if size < PAKT_HEADER_BYTES {
        return None;
    }
    let header = read_exact::<{ PAKT_HEADER_BYTES as usize }, S>(source)?;
    let total: [u8; 8] = header.get(..8)?.try_into().ok()?;
    let total = u64::try_from(i64::from_be_bytes(total)).ok()?;
    if desc.bytes_per_packet != 0 && desc.frames_per_packet != 0 {
        return None;
    }

    let mut offset = 0_u64;
    for packets in 1..=total {
        let bytes = match desc.bytes_per_packet {
            0 => variable_length(source)?,
            fixed => u64::from(fixed),
        };
        if desc.frames_per_packet == 0 {
            variable_length(source)?;
        }
        let Some(next) = offset.checked_add(bytes) else {
            return Some(Overflow::PacketOffset { packets });
        };
        offset = next;
    }

    None
}

fn variable_length<S: Read + ?Sized>(source: &mut S) -> Option<u64> {
    let mut value = 0_u64;
    for _ in 0..VARIABLE_LENGTH_BYTES_AT_MOST {
        let [byte] = read_exact::<1, S>(source)?;
        value |= u64::from(byte & VARIABLE_LENGTH_BITS);
        if byte & VARIABLE_LENGTH_MORE == 0 {
            return Some(value);
        }
        value <<= VARIABLE_LENGTH_SHIFT;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], size: i64, body: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(&size.to_be_bytes());
        into.extend_from_slice(body);
    }

    fn desc(bytes_per_packet: u32, frames_per_packet: u32) -> Vec<u8> {
        let mut body = 44_100.0_f64.to_be_bytes().to_vec();
        body.extend_from_slice(b"lpcm");
        body.extend_from_slice(&0_u32.to_be_bytes());
        body.extend_from_slice(&bytes_per_packet.to_be_bytes());
        body.extend_from_slice(&frames_per_packet.to_be_bytes());
        body.extend_from_slice(&2_u32.to_be_bytes());
        body.extend_from_slice(&16_u32.to_be_bytes());
        body
    }

    fn caf(bytes_per_packet: u32, frames_per_packet: u32, after: &[u8]) -> Vec<u8> {
        let mut file = CAFF.to_vec();
        file.extend_from_slice(&FILE_VERSION.to_be_bytes());
        file.extend_from_slice(&0_u16.to_be_bytes());
        chunk(
            &mut file,
            DESC,
            DESC_BYTES,
            &desc(bytes_per_packet, frames_per_packet),
        );
        file.extend_from_slice(after);
        file
    }

    fn data(declared: i64) -> Vec<u8> {
        let mut after = Vec::new();
        chunk(&mut after, DATA, declared, &[0; 8]);
        after
    }

    fn pakt(total: i64, entries: &[u8]) -> Vec<u8> {
        let mut body = total.to_be_bytes().to_vec();
        body.extend_from_slice(&[0; 16]);
        body.extend_from_slice(entries);
        let mut after = Vec::new();
        chunk(&mut after, PAKT, body.len() as i64, &body);
        after
    }

    #[test]
    fn a_plain_caf_overflows_nothing_and_leaves_the_stream_where_it_found_it() {
        let mut source = Cursor::new(caf(4, 1, &data(12)));

        assert_eq!(read(&mut source), None);
        assert_eq!(source.position(), 0);
    }

    #[test]
    fn packets_whose_bits_a_u32_cannot_count_are_named() {
        let file = caf(0xd400_0000, 0, &[]);

        assert_eq!(
            read(&mut Cursor::new(file)),
            Some(Overflow::PacketBits {
                bytes_per_packet: 0xd400_0000
            })
        );
    }

    #[test]
    fn a_data_chunk_holding_more_frames_than_a_u64_counts_is_named() {
        let file = caf(1, MOST_FRAMES_A_PACKET, &data(i64::MAX));

        assert_eq!(
            read(&mut Cursor::new(file)),
            Some(Overflow::FrameCount {
                packets: i64::MAX as u64 - 4,
                frames_per_packet: MOST_FRAMES_A_PACKET,
            })
        );
    }

    #[test]
    fn packets_longer_than_a_decoder_is_sized_for_are_named() {
        let file = caf(1, 0xfffb_0001, &data(12));

        assert_eq!(
            read(&mut Cursor::new(file)),
            Some(Overflow::PacketFrames {
                frames_per_packet: 0xfffb_0001
            })
        );
    }

    #[test]
    fn a_data_chunk_of_unknown_size_counts_no_frames() {
        assert_eq!(
            read(&mut Cursor::new(caf(
                1,
                MOST_FRAMES_A_PACKET,
                &data(UNKNOWN_SIZE)
            ))),
            None
        );
    }

    #[test]
    fn a_packet_table_whose_offsets_run_past_a_u64_is_named() {
        let huge = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
        let entries = [huge.as_slice(), &[1], &huge, &[1], &huge, &[1]].concat();
        let file = caf(0, 0, &pakt(3, &entries));

        assert_eq!(
            read(&mut Cursor::new(file)),
            Some(Overflow::PacketOffset { packets: 3 })
        );
    }

    #[test]
    fn a_packet_table_of_ordinary_sizes_overflows_nothing() {
        let file = caf(0, 1_024, &pakt(3, &[0x81, 0x00, 0x7f, 0x05]));

        assert_eq!(read(&mut Cursor::new(file)), None);
    }

    #[test]
    fn a_caf_behind_bytes_that_are_not_one_is_still_read() {
        let mut file = b"junk".to_vec();
        file.extend_from_slice(&caf(0xd400_0000, 0, &[]));

        assert!(matches!(
            read(&mut Cursor::new(file)),
            Some(Overflow::PacketBits { .. })
        ));
    }

    #[test]
    fn a_file_that_is_not_a_caf_is_passed_over() {
        assert_eq!(read(&mut Cursor::new(b"fLaC\0\0\0\x22".to_vec())), None);
    }
}
