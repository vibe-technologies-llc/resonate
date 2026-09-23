use std::io::{Read, Seek, SeekFrom};

use crate::{TagName, prescan::read_exact};

const RIFF: &[u8; 4] = b"RIFF";
const WAVE: &[u8; 4] = b"WAVE";
const LIST: &[u8; 4] = b"LIST";
const FMT: &[u8; 4] = b"fmt ";
const INFO: &[u8; 4] = b"INFO";
const ID3: &[u8; 3] = b"ID3";
const ID3_CHUNKS: [&[u8; 4]; 2] = [b"id3 ", b"ID3 "];

const ID3_HEADER: u64 = 10;
const ID3_FOOTER_FLAG: u8 = 0x10;
const FORM_BYTES: u64 = 4;
const MAX_CHUNKS: usize = 4_096;
const MAX_INFO_BYTES: u64 = 1 << 20;
const MAX_VALUE_BYTES: u64 = 4_096;
const MAX_ID3_CHUNK_BYTES: u64 = 32 * 1024 * 1024;

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const FMT_FORMAT_TAG_AT: usize = 0;
const FMT_BITS_PER_SAMPLE_AT: usize = 14;
const FMT_VALID_BITS_AT: usize = 18;
const FMT_EXTENSIBLE_BYTES: u64 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InfoTag {
    pub(crate) name: TagName,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Riff {
    pub(crate) info: Vec<InfoTag>,
    pub(crate) valid_bits: Option<u32>,
    pub(crate) id3: Option<Vec<u8>>,
}

pub(crate) fn read<S: Read + Seek + ?Sized>(source: &mut S) -> Riff {
    let Ok(origin) = source.stream_position() else {
        return Riff::default();
    };
    let found = scan(source).unwrap_or_default();
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("a RIFF INFO scan could not restore the stream position");
    }
    found
}

fn scan<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Riff> {
    skip_id3(source)?;

    let header = read_exact::<12, S>(source)?;
    if !header.starts_with(RIFF) || header.get(8..12) != Some(WAVE.as_slice()) {
        return None;
    }

    let mut found = Riff::default();
    for _ in 0..MAX_CHUNKS {
        let Some(header) = read_exact::<8, S>(source) else {
            break;
        };
        let Some(size) = payload_size(&header) else {
            break;
        };
        let Ok(body) = source.stream_position() else {
            break;
        };

        if header.starts_with(LIST) && size >= FORM_BYTES {
            read_info_list(source, size - FORM_BYTES, &mut found.info);
        }
        if header.starts_with(FMT) && size >= FMT_EXTENSIBLE_BYTES {
            found.valid_bits = read_valid_bits(source);
        }
        if ID3_CHUNKS.iter().any(|id| header.starts_with(*id)) {
            found.id3 = read_id3_chunk(source, size).or(found.id3);
        }
        if source.seek(SeekFrom::Start(body + padded(size))).is_err() {
            break;
        }
    }

    Some(found)
}

fn read_id3_chunk<S: Read + ?Sized>(source: &mut S, size: u64) -> Option<Vec<u8>> {
    if size > MAX_ID3_CHUNK_BYTES {
        tracing::debug!(
            bytes = size,
            limit = MAX_ID3_CHUNK_BYTES,
            "passing over an ID3 chunk larger than one is read at"
        );
        return None;
    }

    let mut bytes = Vec::new();
    source.take(size).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 == size && bytes.starts_with(ID3)).then_some(bytes)
}

fn read_valid_bits<S: Read + ?Sized>(source: &mut S) -> Option<u32> {
    let fields = read_exact::<{ FMT_EXTENSIBLE_BYTES as usize }, S>(source)?;
    if field(&fields, FMT_FORMAT_TAG_AT)? != WAVE_FORMAT_EXTENSIBLE {
        return None;
    }

    let container = u32::from(field(&fields, FMT_BITS_PER_SAMPLE_AT)?);
    let valid = u32::from(field(&fields, FMT_VALID_BITS_AT)?);
    (valid > 0 && valid <= container).then_some(valid)
}

fn field(fields: &[u8], at: usize) -> Option<u16> {
    let pair: [u8; 2] = fields.get(at..at + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(pair))
}

fn read_info_list<S: Read + Seek + ?Sized>(source: &mut S, size: u64, into: &mut Vec<InfoTag>) {
    let Some(form) = read_exact::<4, S>(source) else {
        return;
    };
    if !form.starts_with(INFO) {
        return;
    }

    let mut entries = Vec::new();
    let mut read = 0;
    while read < size.min(MAX_INFO_BYTES) {
        let Some(header) = read_exact::<8, S>(source) else {
            break;
        };
        let Some(value_size) = payload_size(&header) else {
            break;
        };
        let taken = padded(value_size);
        if read + 8 + taken > size {
            break;
        }
        read += 8 + taken;

        let Ok(body) = source.stream_position() else {
            break;
        };
        let Some(value) = read_text(source, value_size) else {
            break;
        };
        let Some(next) = body.checked_add(taken) else {
            break;
        };
        if source.seek(SeekFrom::Start(next)).is_err() {
            break;
        }
        let Some(name) = four_cc(&header) else {
            break;
        };
        if !value.is_empty() {
            entries.push(InfoTag { name, value });
        }
    }

    into.append(&mut entries);
}

fn read_text<S: Read + ?Sized>(source: &mut S, size: u64) -> Option<String> {
    let mut bytes = vec![0_u8; usize::try_from(size.min(MAX_VALUE_BYTES)).ok()?];
    source.read_exact(&mut bytes).ok()?;

    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    Some(String::from_utf8_lossy(bytes.get(..end)?).trim().to_owned())
}

fn four_cc(header: &[u8; 8]) -> Option<TagName> {
    let id = header.get(..4)?;
    if !id.iter().all(u8::is_ascii_graphic) {
        return None;
    }
    Some(TagName::new(String::from_utf8_lossy(id).into_owned()))
}

fn payload_size(header: &[u8; 8]) -> Option<u64> {
    let size: [u8; 4] = header.get(4..8)?.try_into().ok()?;
    Some(u64::from(u32::from_le_bytes(size)))
}

const fn padded(size: u64) -> u64 {
    size + (size & 1)
}

fn skip_id3<S: Read + Seek + ?Sized>(source: &mut S) -> Option<()> {
    let origin = source.stream_position().ok()?;
    let Some(header) = read_exact::<10, S>(source) else {
        source.seek(SeekFrom::Start(origin)).ok()?;
        return Some(());
    };

    if !header.starts_with(ID3) {
        source.seek(SeekFrom::Start(origin)).ok()?;
        return Some(());
    }

    let size = synchsafe(header.get(6..10)?)?;
    let footer = u64::from(header.get(5)? & ID3_FOOTER_FLAG != 0) * ID3_HEADER;
    source
        .seek(SeekFrom::Start(origin + ID3_HEADER + size + footer))
        .ok()?;
    Some(())
}

fn synchsafe(bytes: &[u8]) -> Option<u64> {
    bytes.iter().try_fold(0_u64, |value, byte| {
        (byte & 0x80 == 0).then(|| (value << 7) | u64::from(*byte))
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn info_list(entries: &[(&[u8; 4], &str)]) -> Vec<u8> {
        let mut body = INFO.to_vec();
        for (id, value) in entries {
            let mut terminated = value.as_bytes().to_vec();
            terminated.push(0);
            chunk(&mut body, id, &terminated);
        }
        body
    }

    fn wave(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = WAVE.to_vec();
        for (id, payload) in chunks {
            chunk(&mut body, id, payload);
        }

        let mut file = RIFF.to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    fn named(found: &[InfoTag]) -> Vec<(&str, &str)> {
        found
            .iter()
            .map(|tag| (tag.name.as_str(), tag.value.as_str()))
            .collect()
    }

    #[test]
    fn an_info_list_is_read_in_the_order_the_writer_laid_it_down() {
        let file = wave(&[
            (b"fmt ", vec![0; 16]),
            (
                b"LIST",
                info_list(&[(b"INAM", "Echoes"), (b"IART", "Pink Floyd")]),
            ),
            (b"data", vec![0; 8]),
        ]);

        let found = read(&mut Cursor::new(file)).info;

        assert_eq!(named(&found), [("INAM", "Echoes"), ("IART", "Pink Floyd")]);
    }

    #[test]
    fn an_info_list_after_the_data_chunk_is_still_found() {
        let file = wave(&[
            (b"fmt ", vec![0; 16]),
            (b"data", vec![0; 64]),
            (b"LIST", info_list(&[(b"IPRD", "Meddle")])),
        ]);

        assert_eq!(
            named(&read(&mut Cursor::new(file)).info),
            [("IPRD", "Meddle")]
        );
    }

    #[test]
    fn an_odd_length_value_is_followed_by_its_pad_byte_rather_than_the_next_id() {
        let file = wave(&[(b"LIST", info_list(&[(b"INAM", "odd"), (b"IGNR", "Rock")]))]);

        assert_eq!(
            named(&read(&mut Cursor::new(file)).info),
            [("INAM", "odd"), ("IGNR", "Rock")]
        );
    }

    #[test]
    fn a_list_that_is_not_an_info_list_contributes_nothing() {
        let mut adtl = b"adtl".to_vec();
        chunk(&mut adtl, b"labl", b"a cue point\0");
        let file = wave(&[(b"LIST", adtl), (b"data", vec![0; 8])]);

        assert!(read(&mut Cursor::new(file)).info.is_empty());
    }

    #[test]
    fn the_scan_leaves_the_stream_where_it_found_it() {
        let file = wave(&[(b"LIST", info_list(&[(b"INAM", "Echoes")]))]);
        let mut source = Cursor::new(file);

        let _ = read(&mut source);

        assert_eq!(source.position(), 0);
    }

    #[test]
    fn a_leading_id3_tag_does_not_hide_the_riff_header() {
        let mut file = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
        file.extend_from_slice(&[0; 10]);
        file.extend_from_slice(&wave(&[(b"LIST", info_list(&[(b"INAM", "Echoes")]))]));

        assert_eq!(
            named(&read(&mut Cursor::new(file)).info),
            [("INAM", "Echoes")]
        );
    }

    #[test]
    fn a_container_that_is_not_a_wave_file_yields_nothing() {
        assert!(
            read(&mut Cursor::new(b"fLaC\0\0\0\x22".to_vec()))
                .info
                .is_empty()
        );
        assert!(read(&mut Cursor::new(Vec::new())).info.is_empty());
    }

    #[test]
    fn a_value_longer_than_the_bound_is_cut_there_and_the_walk_goes_on() {
        let long = "e".repeat(MAX_VALUE_BYTES as usize + 64);
        let file = wave(&[(b"LIST", info_list(&[(b"INAM", &long), (b"IGNR", "Rock")]))]);

        let found = read(&mut Cursor::new(file)).info;

        assert_eq!(found[0].value.len(), MAX_VALUE_BYTES as usize);
        assert_eq!(named(&found)[1], ("IGNR", "Rock"));
    }

    #[test]
    fn a_value_claiming_a_gigabyte_the_file_does_not_hold_allocates_none_of_it() {
        let mut body = INFO.to_vec();
        body.extend_from_slice(b"INAM");
        body.extend_from_slice(&(1_u32 << 30).to_le_bytes());
        body.extend_from_slice(b"Echoes\0");

        let mut file = RIFF.to_vec();
        file.extend_from_slice(&u32::MAX.to_le_bytes());
        file.extend_from_slice(WAVE);
        file.extend_from_slice(LIST);
        file.extend_from_slice(&u32::MAX.to_le_bytes());
        file.extend_from_slice(&body);

        assert!(read(&mut Cursor::new(file)).info.is_empty());
    }

    fn id3_tag() -> Vec<u8> {
        let mut tag = b"ID3\x04\x00\x00\x00\x00\x00\x0b".to_vec();
        tag.extend_from_slice(b"TIT2\x00\x00\x00\x01\x00\x00\x03");
        tag
    }

    #[test]
    fn an_id3_chunk_is_held_whole_wherever_it_sits_and_whichever_case_names_it() {
        for id in ID3_CHUNKS {
            let file = wave(&[
                (b"fmt ", vec![0; 16]),
                (b"data", vec![0; 64]),
                (id, id3_tag()),
            ]);

            assert_eq!(read(&mut Cursor::new(file)).id3, Some(id3_tag()));
        }
    }

    #[test]
    fn an_id3_chunk_that_holds_no_tag_or_claims_more_than_the_file_holds_is_passed_over() {
        let empty = wave(&[(b"id3 ", b"not a tag".to_vec())]);
        assert_eq!(read(&mut Cursor::new(empty)).id3, None);

        let mut file = wave(&[(b"fmt ", vec![0; 16])]);
        file.extend_from_slice(b"id3 ");
        file.extend_from_slice(&(1_u32 << 24).to_le_bytes());
        file.extend_from_slice(&id3_tag());
        assert_eq!(read(&mut Cursor::new(file)).id3, None);
    }

    #[test]
    fn a_chunk_that_claims_more_than_the_list_holds_stops_the_walk() {
        let mut body = INFO.to_vec();
        body.extend_from_slice(b"INAM");
        body.extend_from_slice(&u32::MAX.to_le_bytes());
        body.extend_from_slice(b"Echoes\0");
        let file = wave(&[(b"LIST", body), (b"data", vec![0; 8])]);

        assert!(read(&mut Cursor::new(file)).info.is_empty());
    }
}
