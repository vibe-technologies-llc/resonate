use std::io::{Read, Seek, SeekFrom};

use resonate_core::{Frames, SampleRate};

const CAPTURE: [u8; 4] = *b"OggS";
const FIXED_HEADER_BYTES: usize = 27;
const VERSION_AT: usize = 4;
const FLAGS_AT: usize = 5;
const SERIAL_AT: usize = 14;
const SEQUENCE_AT: usize = 18;
const CHECKSUM_AT: usize = 22;
const SEGMENT_COUNT_AT: usize = 26;
const CONTINUED: u8 = 0x01;
const FIRST_OF_STREAM: u8 = 0x02;
const NO_PACKET_ENDS: u64 = u64::MAX;
const LACING_FULL: usize = 255;
const SEGMENTS_A_PAGE: usize = 32;
pub(crate) const PAGE_BODY_AT_MOST: usize = LACING_FULL * SEGMENTS_A_PAGE;
const MOST_PAGES: usize = 4_096;
const COMMENT_AT: usize = 1;

const GRANULE_AT: usize = 6;
const NO_GRANULE: u64 = u64::MAX;
const TAILS_LOOKED_AT: [u64; 2] = [8 * 1024, 64 * 1024];
const MOST_PAGES_WALKED: usize = 1 << 20;
const IDENTIFICATION_READ: usize = 16;
const VORBIS_RATE_AT: usize = 12;
const OPUS_PRE_SKIP_AT: usize = 10;
const OPUS_DECODED_HZ: u32 = 48_000;

const VORBIS_IDENTIFICATION: &[u8] = b"\x01vorbis";
const VORBIS_COMMENT: &[u8] = b"\x03vorbis";
const OPUS_IDENTIFICATION: &[u8] = b"OpusHead";
const OPUS_COMMENT: &[u8] = b"OpusTags";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Codec {
    Vorbis,
    Opus,
}

impl Codec {
    const fn header_packets(self) -> usize {
        match self {
            Self::Vorbis => 3,
            Self::Opus => 2,
        }
    }

    const fn identification(self) -> &'static [u8] {
        match self {
            Self::Vorbis => VORBIS_IDENTIFICATION,
            Self::Opus => OPUS_IDENTIFICATION,
        }
    }

    const fn comment(self) -> &'static [u8] {
        match self {
            Self::Vorbis => VORBIS_COMMENT,
            Self::Opus => OPUS_COMMENT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) codec: Codec,
    pub(crate) pages: u32,
    pub(crate) comment: u32,
}

#[derive(Debug)]
pub(crate) struct Headers {
    pub(crate) serial: u32,
    pub(crate) packets: Vec<Vec<u8>>,
    pub(crate) pages: u32,
    pub(crate) length: u64,
}

impl Headers {
    pub(crate) fn layout(&self, codec: Codec) -> Option<Layout> {
        Some(Layout {
            codec,
            pages: self.pages,
            comment: u32::try_from(self.packets.get(COMMENT_AT)?.len()).ok()?,
        })
    }
}

pub(crate) fn headers<S: Read + ?Sized>(codec: Codec, source: &mut S) -> Option<Headers> {
    let wanted = codec.header_packets();
    let mut packets: Vec<Vec<u8>> = Vec::with_capacity(wanted);
    let mut open: Vec<u8> = Vec::new();
    let mut serial = 0;
    let mut length = 0_u64;

    for sequence in 0..MOST_PAGES {
        let mut fixed = [0_u8; FIXED_HEADER_BYTES];
        source.read_exact(&mut fixed).ok()?;
        if fixed[..CAPTURE.len()] != CAPTURE || fixed[VERSION_AT] != 0 {
            return None;
        }

        let flags = fixed[FLAGS_AT];
        let page_serial = u32::from_le_bytes(fixed[SERIAL_AT..SERIAL_AT + 4].try_into().ok()?);
        let page_sequence =
            u32::from_le_bytes(fixed[SEQUENCE_AT..SEQUENCE_AT + 4].try_into().ok()?);
        let first = sequence == 0;
        if first {
            serial = page_serial;
        }
        let begins_stream = flags & FIRST_OF_STREAM != 0;
        let continues = flags & CONTINUED != 0;
        if begins_stream != first
            || page_serial != serial
            || usize::try_from(page_sequence).ok()? != sequence
            || continues != !open.is_empty()
        {
            return None;
        }

        let mut lacing = vec![0_u8; usize::from(fixed[SEGMENT_COUNT_AT])];
        source.read_exact(&mut lacing).ok()?;
        let body_bytes: usize = lacing.iter().map(|lace| usize::from(*lace)).sum();
        let mut body = vec![0_u8; body_bytes];
        source.read_exact(&mut body).ok()?;
        length += (FIXED_HEADER_BYTES + lacing.len() + body_bytes) as u64;

        let mut rest = body.as_slice();
        for (index, lace) in lacing.iter().enumerate() {
            let (taken, after) = rest.split_at(usize::from(*lace));
            open.extend_from_slice(taken);
            rest = after;
            if usize::from(*lace) < LACING_FULL {
                packets.push(std::mem::take(&mut open));
                if packets.len() == wanted {
                    let ends_the_page = index + 1 == lacing.len();
                    return ends_the_page
                        .then(|| signed(codec, &packets))
                        .flatten()
                        .map(|()| Headers {
                            serial,
                            packets,
                            pages: u32::try_from(sequence + 1).unwrap_or(u32::MAX),
                            length,
                        });
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) rate: SampleRate,
    pub(crate) frames: Frames,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chain {
    links: Vec<Link>,
}

impl Chain {
    pub(crate) fn length_at(&self, clock: SampleRate) -> Frames {
        self.links.iter().fold(Frames::ZERO, |length, link| {
            length.saturating_add(link.frames.rescaled(link.rate, clock))
        })
    }

    pub(crate) fn origins_at(&self, clock: SampleRate) -> Vec<Frames> {
        let mut origin = Frames::ZERO;
        self.links
            .iter()
            .map(|link| {
                let this = origin;
                origin = origin.saturating_add(link.frames.rescaled(link.rate, clock));
                this
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageHead {
    flags: u8,
    granule: u64,
    serial: u32,
    head: u64,
    body: u64,
}

impl PageHead {
    const fn begins_a_stream(self) -> bool {
        self.flags & FIRST_OF_STREAM != 0
    }

    const fn length(self) -> u64 {
        self.head.saturating_add(self.body)
    }

    fn read(fixed: &[u8; FIXED_HEADER_BYTES]) -> Option<Self> {
        if fixed[..CAPTURE.len()] != CAPTURE || fixed[VERSION_AT] != 0 {
            return None;
        }
        Some(Self {
            flags: fixed[FLAGS_AT],
            granule: u64::from_le_bytes(fixed[GRANULE_AT..GRANULE_AT + 8].try_into().ok()?),
            serial: u32::from_le_bytes(fixed[SERIAL_AT..SERIAL_AT + 4].try_into().ok()?),
            head: (FIXED_HEADER_BYTES + usize::from(fixed[SEGMENT_COUNT_AT])) as u64,
            body: 0,
        })
    }
}

fn page_at<S: Read + Seek + ?Sized>(source: &mut S, at: u64) -> Option<PageHead> {
    source.seek(SeekFrom::Start(at)).ok()?;
    let mut fixed = [0_u8; FIXED_HEADER_BYTES];
    source.read_exact(&mut fixed).ok()?;
    let page = PageHead::read(&fixed)?;
    let mut lacing = vec![0_u8; usize::from(fixed[SEGMENT_COUNT_AT])];
    source.read_exact(&mut lacing).ok()?;
    let body = lacing.iter().map(|lace| u64::from(*lace)).sum();
    Some(PageHead { body, ..page })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sounding {
    serial: u32,
    rate: SampleRate,
    pre_skip: u64,
}

impl Sounding {
    fn named_by(serial: u32, identification: &[u8]) -> Option<Self> {
        if identification.starts_with(VORBIS_IDENTIFICATION) {
            let hz = identification.get(VORBIS_RATE_AT..VORBIS_RATE_AT + 4)?;
            return Some(Self {
                serial,
                rate: SampleRate::new(u32::from_le_bytes(hz.try_into().ok()?)).ok()?,
                pre_skip: 0,
            });
        }
        if identification.starts_with(OPUS_IDENTIFICATION) {
            let skip = identification.get(OPUS_PRE_SKIP_AT..OPUS_PRE_SKIP_AT + 2)?;
            return Some(Self {
                serial,
                rate: SampleRate::new(OPUS_DECODED_HZ).ok()?,
                pre_skip: u64::from(u16::from_le_bytes(skip.try_into().ok()?)),
            });
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Walking {
    sounding: Option<Sounding>,
    last_granule: Option<u64>,
    past_its_heads: bool,
}

impl Walking {
    fn link(self) -> Option<Link> {
        let sounding = self.sounding?;
        Some(Link {
            rate: sounding.rate,
            frames: Frames(self.last_granule?.saturating_sub(sounding.pre_skip)),
        })
    }
}

pub(crate) fn chain<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Chain> {
    let origin = source.stream_position().ok()?;
    let first = page_at(source, origin)?;
    if !first.begins_a_stream() || !ends_in_another_stream(source, first.serial) {
        return None;
    }

    let mut links = Vec::new();
    let mut walking: Option<Walking> = None;
    let mut at = origin;
    for _ in 0..MOST_PAGES_WALKED {
        let Some(page) = page_at(source, at) else {
            break;
        };
        if page.begins_a_stream() {
            if walking.is_none_or(|link| link.past_its_heads) {
                if let Some(finished) = walking.take() {
                    links.push(finished.link()?);
                }
                walking = Some(Walking::default());
            }
            let mut identification = [0_u8; IDENTIFICATION_READ];
            let read = usize::try_from(page.body)
                .map_or(IDENTIFICATION_READ, |body| body.min(IDENTIFICATION_READ));
            source.read_exact(identification.get_mut(..read)?).ok()?;
            if let Some(link) = walking.as_mut()
                && link.sounding.is_none()
            {
                link.sounding = Sounding::named_by(page.serial, &identification[..read]);
            }
        } else if let Some(link) = walking.as_mut() {
            link.past_its_heads = true;
            let sounds = link
                .sounding
                .is_some_and(|sounding| sounding.serial == page.serial);
            if sounds && page.granule != NO_GRANULE {
                link.last_granule = Some(page.granule);
            }
        }
        at = at.saturating_add(page.length());
    }
    links.push(walking?.link()?);

    (links.len() > 1).then_some(Chain { links })
}

fn ends_in_another_stream<S: Read + Seek + ?Sized>(source: &mut S, serial: u32) -> bool {
    TAILS_LOOKED_AT
        .iter()
        .find_map(|looked_at| last_page_within(source, *looked_at))
        .is_some_and(|page| page.serial != serial)
}

fn last_page_within<S: Read + Seek + ?Sized>(source: &mut S, looked_at: u64) -> Option<PageHead> {
    let end = source.seek(SeekFrom::End(0)).ok()?;
    source
        .seek(SeekFrom::Start(end.saturating_sub(looked_at)))
        .ok()?;
    let mut tail = Vec::new();
    source.take(looked_at).read_to_end(&mut tail).ok()?;
    tail.windows(CAPTURE.len())
        .enumerate()
        .rev()
        .filter(|(_, marker)| *marker == CAPTURE)
        .find_map(|(at, _)| {
            let fixed: &[u8; FIXED_HEADER_BYTES] =
                tail.get(at..at + FIXED_HEADER_BYTES)?.try_into().ok()?;
            PageHead::read(fixed)
        })
}

fn signed(codec: Codec, packets: &[Vec<u8>]) -> Option<()> {
    let identification = packets.first()?;
    let comment = packets.get(COMMENT_AT)?;
    (identification.starts_with(codec.identification()) && comment.starts_with(codec.comment()))
        .then_some(())
}

pub(crate) fn paginated(serial: u32, packets: &[&[u8]]) -> Vec<u8> {
    let mut pages = Vec::new();
    let mut sequence = 0_u32;
    for packet in packets {
        let chunks: Vec<&[u8]> = packet.chunks(PAGE_BODY_AT_MOST).collect();
        let last = chunks.len().saturating_sub(1);
        for (index, body) in chunks.iter().enumerate() {
            let flags = match (sequence, index) {
                (0, _) => FIRST_OF_STREAM,
                (_, 0) => 0,
                _ => CONTINUED,
            };
            let granule = if index == last { 0 } else { NO_PACKET_ENDS };
            let mut lacing = vec![LACING_FULL as u8; body.len() / LACING_FULL];
            if lacing.len() != SEGMENTS_A_PAGE {
                lacing.push((packet.len() % LACING_FULL) as u8);
            }
            pages.extend_from_slice(&page(flags, granule, serial, sequence, &lacing, body));
            sequence += 1;
        }
        if !packet.is_empty() && packet.len().is_multiple_of(PAGE_BODY_AT_MOST) {
            pages.extend_from_slice(&page(CONTINUED, 0, serial, sequence, &[0], &[]));
            sequence += 1;
        }
    }
    pages
}

pub(crate) fn page(
    flags: u8,
    granule: u64,
    serial: u32,
    sequence: u32,
    lacing: &[u8],
    body: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FIXED_HEADER_BYTES + lacing.len() + body.len());
    bytes.extend_from_slice(&CAPTURE);
    bytes.push(0);
    bytes.push(flags);
    bytes.extend_from_slice(&granule.to_le_bytes());
    bytes.extend_from_slice(&serial.to_le_bytes());
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.push(lacing.len() as u8);
    bytes.extend_from_slice(lacing);
    bytes.extend_from_slice(body);
    seal(&mut bytes);
    bytes
}

fn seal(page: &mut [u8]) {
    page[CHECKSUM_AT..CHECKSUM_AT + 4].fill(0);
    let checksum = checksum_of(page);
    page[CHECKSUM_AT..CHECKSUM_AT + 4].copy_from_slice(&checksum.to_le_bytes());
}

const POLYNOMIAL: u32 = 0x04C1_1DB7;

const CHECKSUMS: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut value = (byte as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ POLYNOMIAL
            } else {
                value << 1
            };
            bit += 1;
        }
        table[byte] = value;
        byte += 1;
    }
    table
};

fn checksum_of(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0_u32, |checksum, byte| {
        (checksum << 8) ^ CHECKSUMS[usize::from((checksum >> 24) as u8 ^ byte)]
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Paged {
    pub(crate) pages: usize,
    pub(crate) bytes: usize,
}

impl Paged {
    pub(crate) const NOTHING: Self = Self { pages: 0, bytes: 0 };

    pub(crate) const fn of_packet(length: usize) -> Self {
        if length == 0 {
            return Self::NOTHING;
        }
        let chunks = length.div_ceil(PAGE_BODY_AT_MOST);
        let last = length - (chunks - 1) * PAGE_BODY_AT_MOST;
        let last_segments = if last == PAGE_BODY_AT_MOST {
            SEGMENTS_A_PAGE
        } else {
            last / LACING_FULL + 1
        };
        let segments = (chunks - 1) * SEGMENTS_A_PAGE + last_segments;
        let paged = Self {
            pages: chunks,
            bytes: length + FIXED_HEADER_BYTES * chunks + segments,
        };
        if length.is_multiple_of(PAGE_BODY_AT_MOST) {
            return Self {
                pages: paged.pages + 1,
                bytes: paged.bytes + FIXED_HEADER_BYTES + 1,
            };
        }
        paged
    }

    pub(crate) const fn with(self, other: Self) -> Self {
        Self {
            pages: self.pages + other.pages,
            bytes: self.bytes + other.bytes,
        }
    }
}

pub(crate) fn renumbered(tail: &mut [u8], first: u32) {
    let mut at = 0;
    let mut sequence = first;
    while let Some(length) = page_length_at(tail, at) {
        let page = &mut tail[at..at + length];
        page[SEQUENCE_AT..SEQUENCE_AT + 4].copy_from_slice(&sequence.to_le_bytes());
        seal(page);
        at += length;
        sequence = sequence.wrapping_add(1);
    }
}

fn page_length_at(bytes: &[u8], at: usize) -> Option<usize> {
    let header = bytes.get(at..at + FIXED_HEADER_BYTES)?;
    if header[..CAPTURE.len()] != CAPTURE || header[VERSION_AT] != 0 {
        return None;
    }
    let segments = usize::from(header[SEGMENT_COUNT_AT]);
    let lacing = bytes.get(at + FIXED_HEADER_BYTES..at + FIXED_HEADER_BYTES + segments)?;
    let body: usize = lacing.iter().map(|lace| usize::from(*lace)).sum();
    let length = FIXED_HEADER_BYTES + segments + body;
    (at + length <= bytes.len()).then_some(length)
}

#[cfg(test)]
pub(crate) fn pages_are_whole(bytes: &[u8]) -> bool {
    let mut at = 0;
    let mut sequence = 0_u32;
    while at < bytes.len() {
        let Some(length) = page_length_at(bytes, at) else {
            return false;
        };
        let mut page = bytes[at..at + length].to_vec();
        let stored = page[CHECKSUM_AT..CHECKSUM_AT + 4].to_vec();
        seal(&mut page);
        let numbered = page[SEQUENCE_AT..SEQUENCE_AT + 4] == sequence.to_le_bytes();
        if page[CHECKSUM_AT..CHECKSUM_AT + 4] != stored[..] || !numbered {
            return false;
        }
        at += length;
        sequence += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use lofty::{
        config::WriteOptions,
        tag::{ItemKey, Tag, TagExt as _, TagType},
    };

    use super::*;

    fn opus_packets(comment: &[u8]) -> Vec<Vec<u8>> {
        let mut identification = b"OpusHead".to_vec();
        identification.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0]);
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(comment);
        vec![identification, tags]
    }

    fn comment_of(length: usize) -> Vec<u8> {
        let vendor = b"test";
        let mut comment = (vendor.len() as u32).to_le_bytes().to_vec();
        comment.extend_from_slice(vendor);
        comment.extend_from_slice(&0_u32.to_le_bytes());
        comment.resize(length.max(comment.len()), 0);
        comment
    }

    fn head_with(comment: usize) -> Vec<u8> {
        let packets = opus_packets(&comment_of(comment));
        let borrowed: Vec<&[u8]> = packets.iter().map(Vec::as_slice).collect();
        paginated(0xABCD, &borrowed)
    }

    fn lofty_saved(head: &[u8], title: &str) -> Vec<u8> {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.insert_text(ItemKey::TrackTitle, title.to_owned());
        let mut cursor = Cursor::new(head.to_vec());
        tag.save_to(&mut cursor, WriteOptions::default().preferred_padding(0))
            .expect("lofty writes the head");
        cursor.into_inner()
    }

    #[test]
    fn pagination_is_what_lofty_writes_whatever_the_size_of_the_comment() {
        for title in [1, 100, 8_100, 8_160, 8_300, 16_300, 16_330, 70_000] {
            let head = head_with(40);
            let written = lofty_saved(&head, &"t".repeat(title));
            let parsed = headers(Codec::Opus, &mut Cursor::new(&written)).expect("lofty's head");
            let packets: Vec<&[u8]> = parsed.packets.iter().map(Vec::as_slice).collect();

            assert_eq!(
                paginated(parsed.serial, &packets),
                written,
                "a {title}-byte title was paged apart from lofty"
            );
        }
    }

    #[test]
    fn a_comment_of_every_length_is_counted_in_pages_and_bytes_as_it_is_written() {
        for length in (1..40_000)
            .step_by(37)
            .chain([8_160, 16_320, 24_480, 24_479, 24_481])
        {
            let packet = vec![9_u8; length];
            let written = paginated(1, &[&packet]);
            let counted = Paged::of_packet(length);

            assert_eq!(counted.bytes, written.len(), "{length} bytes counted apart");
            let mut pages = 0;
            let mut at = 0;
            while let Some(page) = page_length_at(&written, at) {
                at += page;
                pages += 1;
            }
            assert_eq!(counted.pages, pages, "{length} bytes paged apart");
        }
    }

    #[test]
    fn headers_read_back_what_was_paged_and_know_where_the_audio_begins() {
        let head = head_with(500);
        let mut stream = head.clone();
        stream.extend_from_slice(&page(0, 960, 0xABCD, 2, &[40], &[7; 40]));

        let parsed = headers(Codec::Opus, &mut Cursor::new(&stream)).expect("the headers");

        assert_eq!(parsed.length, head.len() as u64);
        assert_eq!(parsed.pages, 2);
        assert_eq!(parsed.serial, 0xABCD);
        assert_eq!(parsed.packets[1].len(), 8 + 500);
    }

    #[test]
    fn a_stream_whose_headers_share_a_page_with_audio_is_not_taken_for_a_head() {
        let packets = opus_packets(&comment_of(40));
        let mut shared = Vec::new();
        shared.extend_from_slice(&packets[0]);
        shared.extend_from_slice(&packets[1]);
        shared.extend_from_slice(&[1; 30]);
        let lacing = [19, 48, 30];
        let stream = page(FIRST_OF_STREAM, 0, 1, 0, &lacing, &shared);

        assert!(headers(Codec::Opus, &mut Cursor::new(&stream)).is_none());
    }

    #[test]
    fn a_stream_that_is_not_the_codec_asked_for_is_not_taken_for_a_head() {
        let head = head_with(40);

        assert!(headers(Codec::Vorbis, &mut Cursor::new(&head)).is_none());
        assert!(headers(Codec::Opus, &mut Cursor::new(&head[..head.len() - 1])).is_none());
    }

    #[test]
    fn renumbering_pages_makes_them_a_run_from_the_number_asked_with_their_sums_mended() {
        let mut tail = Vec::new();
        for sequence in 0..3_u32 {
            tail.extend_from_slice(&page(
                0,
                960 * u64::from(sequence),
                7,
                sequence,
                &[10],
                &[3; 10],
            ));
        }
        tail.extend_from_slice(b"not a page");
        let trailing = b"not a page".len();

        renumbered(&mut tail, 9);

        let mut expected = Vec::new();
        for sequence in 0..3_u32 {
            expected.extend_from_slice(&page(
                0,
                960 * u64::from(sequence),
                7,
                9 + sequence,
                &[10],
                &[3; 10],
            ));
        }
        expected.extend_from_slice(b"not a page");
        assert_eq!(tail, expected);
        assert_eq!(tail.len() - trailing, 3 * (FIXED_HEADER_BYTES + 1 + 10));
    }
}
