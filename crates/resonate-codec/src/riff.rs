use std::io::{Read, Seek, SeekFrom};

use resonate_core::text::{decoded_as, detected};

use crate::{
    TagName,
    prescan::{Opened, opened_first, past_id3, read_exact},
    wide::{ChunkHeader, DS64, FMT, MOST_DS64_BYTES, Sizes, WIDEST_CHUNK_HEADER, Wide},
};

const LIST: &[u8; 4] = b"LIST";
const INFO: &[u8; 4] = b"INFO";
const ID3: &[u8; 3] = b"ID3";
const ID3_CHUNKS: [&[u8; 4]; 2] = [b"id3 ", b"ID3 "];

const FORM_BYTES: u64 = 4;
const MAX_CHUNKS: usize = 4_096;
const MAX_INFO_BYTES: u64 = 1 << 20;
const MAX_VALUE_BYTES: u64 = 4_096;
const MAX_ID3_CHUNK_BYTES: u64 = 32 * 1024 * 1024;
const RIFF_HEADER_BYTES: usize = 12;

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const FMT_FORMAT_TAG_AT: usize = 0;
const FMT_CHANNELS_AT: usize = 2;
const FMT_BITS_PER_SAMPLE_AT: usize = 14;
const FMT_PLAIN_BYTES: u64 = 16;
const FMT_VALID_BITS_AT: usize = 18;
const FMT_CHANNEL_MASK_AT: usize = 20;
const FMT_EXTENSIBLE_BYTES: u64 = 20;
const FMT_MASKED_BYTES: u64 = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InfoTag {
    pub(crate) name: TagName,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Riff {
    pub(crate) info: Vec<InfoTag>,
    pub(crate) valid_bits: Option<u32>,
    pub(crate) channels: Option<u16>,
    pub(crate) mask: Option<u32>,
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
    let start = past_id3(source)?;
    let (layout, header) = riff_header_at(source, start)?;
    source
        .seek(SeekFrom::Start(header + layout.header_bytes()))
        .ok()?;

    let mut found = Riff::default();
    let mut sizes = Sizes::default();
    for walked in 0..MAX_CHUNKS {
        let Some(chunk) = layout.next_chunk(source, &sizes) else {
            break;
        };
        let Ok(body) = source.stream_position() else {
            break;
        };
        let size = chunk.size;

        if walked == 0 && chunk.is(DS64) {
            sizes = Sizes::read(&read_bounded(source, size, MOST_DS64_BYTES));
        }
        if chunk.is(LIST) && size >= FORM_BYTES {
            read_info_list(source, size - FORM_BYTES, &mut found.info);
        }
        if chunk.is(FMT)
            && layout == Layout::Riff
            && size >= FMT_PLAIN_BYTES
            && found.channels.is_none()
        {
            let fmt = read_fmt(source, size);
            found.channels = fmt.channels;
            found.mask = fmt.mask;
            found.valid_bits = fmt.valid_bits;
        }
        if ID3_CHUNKS.iter().any(|id| chunk.is(id)) {
            found.id3 = read_id3_chunk(source, size).or(found.id3);
        }
        let Some(next) = body.checked_add(layout.padded(size)) else {
            break;
        };
        if source.seek(SeekFrom::Start(next)).is_err() {
            break;
        }
    }

    Some(found)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Riff,
    Wide(Wide),
}

impl Layout {
    fn header_bytes(self) -> u64 {
        match self {
            Self::Riff => RIFF_HEADER_BYTES as u64,
            Self::Wide(wide) => wide.header_bytes(),
        }
    }

    fn padded(self, size: u64) -> u64 {
        match self {
            Self::Riff => padded(size),
            Self::Wide(wide) => wide.padded(size),
        }
    }

    fn next_chunk<S: Read + ?Sized>(self, source: &mut S, sizes: &Sizes) -> Option<ChunkHeader> {
        match self {
            Self::Riff => {
                let header = read_exact::<8, S>(source)?;
                Some(ChunkHeader {
                    id: header.first_chunk::<4>().copied(),
                    size: payload_size(&header)?,
                })
            }
            Self::Wide(wide) => {
                let mut header = [0_u8; WIDEST_CHUNK_HEADER];
                let header = header.get_mut(..wide.chunk_header_bytes())?;
                source.read_exact(header).ok()?;
                wide.chunk(header, sizes)
            }
        }
    }
}

fn read_bounded<S: Read + ?Sized>(source: &mut S, size: u64, most: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    if source.take(size.min(most)).read_to_end(&mut bytes).is_err() {
        bytes.clear();
    }
    bytes
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fmt {
    channels: Option<u16>,
    mask: Option<u32>,
    valid_bits: Option<u32>,
}

fn read_fmt<S: Read + ?Sized>(source: &mut S, size: u64) -> Fmt {
    let wanted = if size >= FMT_MASKED_BYTES {
        FMT_MASKED_BYTES
    } else if size >= FMT_EXTENSIBLE_BYTES {
        FMT_EXTENSIBLE_BYTES
    } else {
        FMT_PLAIN_BYTES
    };
    let mut fields = [0_u8; FMT_MASKED_BYTES as usize];
    let Some(fields) = fields.get_mut(..wanted as usize) else {
        return Fmt::default();
    };
    if source.read_exact(fields).is_err() {
        return Fmt::default();
    }

    Fmt {
        channels: field(fields, FMT_CHANNELS_AT),
        mask: channel_mask(fields),
        valid_bits: valid_bits(fields),
    }
}

fn channel_mask(fields: &[u8]) -> Option<u32> {
    if field(fields, FMT_FORMAT_TAG_AT)? != WAVE_FORMAT_EXTENSIBLE {
        return None;
    }
    let bytes: [u8; 4] = fields
        .get(FMT_CHANNEL_MASK_AT..FMT_CHANNEL_MASK_AT + 4)?
        .try_into()
        .ok()?;

    Some(u32::from_le_bytes(bytes))
}

pub(crate) fn a_mask_the_decoder_cannot_widen(mask: u32, channels: u16) -> bool {
    let short = u32::from(channels).saturating_sub(mask.count_ones());

    short > 0 && (short >= u32::BITS || mask.leading_zeros() == 0)
}

fn valid_bits(fields: &[u8]) -> Option<u32> {
    if field(fields, FMT_FORMAT_TAG_AT)? != WAVE_FORMAT_EXTENSIBLE {
        return None;
    }

    let container = u32::from(field(fields, FMT_BITS_PER_SAMPLE_AT)?);
    let valid = u32::from(field(fields, FMT_VALID_BITS_AT)?);
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
        let Some(value) = read_value(source, value_size) else {
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
        entries.push((name, value));
    }

    let encoding = detected(&entries.iter().fold(Vec::new(), |mut all, (_, value)| {
        all.extend_from_slice(value);
        all.push(b'\n');
        all
    }));
    into.extend(entries.into_iter().filter_map(|(name, value)| {
        let value = decoded_as(&value, encoding).trim().to_owned();
        (!value.is_empty()).then_some(InfoTag { name, value })
    }));
}

fn read_value<S: Read + ?Sized>(source: &mut S, size: u64) -> Option<Vec<u8>> {
    let mut bytes = vec![0_u8; usize::try_from(size.min(MAX_VALUE_BYTES)).ok()?];
    source.read_exact(&mut bytes).ok()?;

    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    bytes.truncate(end);
    Some(bytes)
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

fn riff_header_at<S: Read + Seek + ?Sized>(source: &mut S, start: u64) -> Option<(Layout, u64)> {
    match opened_first(source, start)? {
        (Opened::Wave, at) => Some((Layout::Riff, at)),
        (Opened::Wide(wide), at) => Some((Layout::Wide(wide), at)),
        (Opened::Caf | Opened::Another, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_core::{LegacyEncoding, TextEncoding, text::encoded};

    use super::*;
    use crate::prescan::PROBED_WITHIN;

    const RIFF: &[u8; 4] = b"RIFF";
    const WAVE: &[u8; 4] = b"WAVE";

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

    fn fmt(channels: u16) -> Vec<u8> {
        let mut fields = vec![0_u8; FMT_PLAIN_BYTES as usize];
        fields[FMT_FORMAT_TAG_AT] = 1;
        fields[FMT_CHANNELS_AT..FMT_CHANNELS_AT + 2].copy_from_slice(&channels.to_le_bytes());
        fields[FMT_BITS_PER_SAMPLE_AT] = 16;
        fields
    }

    #[test]
    fn an_extensible_fmt_chunk_names_its_channel_mask() {
        let mut fields = vec![0_u8; FMT_MASKED_BYTES as usize];
        fields[..2].copy_from_slice(&WAVE_FORMAT_EXTENSIBLE.to_le_bytes());
        fields[FMT_CHANNELS_AT] = 6;
        fields[FMT_CHANNEL_MASK_AT..FMT_CHANNEL_MASK_AT + 4]
            .copy_from_slice(&0x3F_u32.to_le_bytes());
        let found = read(&mut Cursor::new(wave(&[(b"fmt ", fields)])));

        assert_eq!(found.channels, Some(6));
        assert_eq!(found.mask, Some(0x3F));
    }

    #[test]
    fn a_mask_short_of_positions_is_refused_only_where_widening_it_would_run_off_the_top() {
        assert!(!a_mask_the_decoder_cannot_widen(0x3F, 6));
        assert!(!a_mask_the_decoder_cannot_widen(0, 9));
        assert!(!a_mask_the_decoder_cannot_widen(0b10_1000, 5));
        assert!(!a_mask_the_decoder_cannot_widen(0x8000_0003, 2));
        assert!(a_mask_the_decoder_cannot_widen(0x8000_0000, 3));
        assert!(a_mask_the_decoder_cannot_widen(0, 40));
    }

    #[test]
    fn a_second_fmt_chunk_is_passed_over_as_the_decoder_passes_it_over() {
        let found = read(&mut Cursor::new(wave(&[
            (b"fmt ", fmt(20_000)),
            (b"fmt ", fmt(2)),
        ])));

        assert_eq!(found.channels, Some(20_000));
    }

    #[test]
    fn the_channels_a_plain_fmt_chunk_declares_are_read_whatever_they_number() {
        for channels in [2_u16, 6, 20_000] {
            let found = read(&mut Cursor::new(wave(&[(b"fmt ", fmt(channels))])));

            assert_eq!(found.channels, Some(channels));
            assert_eq!(found.valid_bits, None);
        }
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
    fn an_info_list_in_a_legacy_code_page_is_read_in_it() {
        let cyrillic = TextEncoding::Legacy(LegacyEncoding::WINDOWS_1251);
        let mut body = INFO.to_vec();
        for (id, value) in [(b"INAM", "Группа крови"), (b"IART", "Кино")] {
            let mut terminated = encoded(value, cyrillic).expect("Cyrillic letters");
            terminated.push(0);
            chunk(&mut body, id, &terminated);
        }

        let found = read(&mut Cursor::new(wave(&[(b"LIST", body)]))).info;

        assert_eq!(named(&found), [("INAM", "Группа крови"), ("IART", "Кино")]);
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
    fn a_riff_header_behind_bytes_that_are_not_one_is_still_found() {
        let mut file = b"ft".to_vec();
        file.extend_from_slice(&wave(&[(b"fmt ", fmt(3))]));

        assert_eq!(read(&mut Cursor::new(file)).channels, Some(3));
    }

    #[test]
    fn a_leading_id3_tag_whose_size_cannot_be_read_does_not_hide_the_riff_header() {
        let mut file = b"ID3\x04\x00\x00\x00\x00\x00\xff".to_vec();
        file.extend_from_slice(&[0xff; 64]);
        file.extend_from_slice(&wave(&[(b"fmt ", fmt(3))]));

        assert_eq!(read(&mut Cursor::new(file)).channels, Some(3));
    }

    #[test]
    fn a_riff_header_further_in_than_the_search_reaches_is_not_found() {
        let mut file = vec![0_u8; PROBED_WITHIN as usize];
        file.extend_from_slice(&wave(&[(b"fmt ", fmt(3))]));

        assert_eq!(read(&mut Cursor::new(file)), Riff::default());
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
