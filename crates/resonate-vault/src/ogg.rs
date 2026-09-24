use std::io::{self, Read};

const CAPTURE: &[u8; 4] = b"OggS";
const HEADER_BYTES: usize = 27;
const VERSION_AT: usize = 4;
const FLAGS_AT: usize = 5;
const SERIAL_AT: usize = 14;
const SEQUENCE_AT: usize = 18;
const CHECKSUM_AT: usize = 22;
const SEGMENTS_AT: usize = 26;
const CONTINUED: u8 = 0x01;
const FIRST: u8 = 0x02;
const SEGMENT_BYTES: usize = 255;
const SEGMENTS_AT_MOST: usize = 255;
const NO_PACKET_ENDS: u64 = u64::MAX;
const HEADER_BYTES_AT_MOST: usize = 64 << 20;
const CHECKSUM_POLYNOMIAL: u32 = 0x04c1_1db7;
const CHECKSUM_TABLE: [u32; 256] = checksum_table();

const VORBIS_IDENTIFICATION: &[u8] = b"\x01vorbis";
const VORBIS_COMMENT: &[u8] = b"\x03vorbis";
const VORBIS_FRAMED: u8 = 0x01;
const OPUS_HEAD: &[u8] = b"OpusHead";
const OPUS_TAGS: &[u8] = b"OpusTags";
const LENGTH_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Renumbering {
    serial: u32,
    shift: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bared {
    pub(crate) head: Vec<u8>,
    pub(crate) renumbering: Option<Renumbering>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Vorbis,
    Opus,
}

impl Codec {
    fn of(identification: &[u8]) -> Option<Self> {
        if identification.starts_with(VORBIS_IDENTIFICATION) {
            Some(Self::Vorbis)
        } else if identification.starts_with(OPUS_HEAD) {
            Some(Self::Opus)
        } else {
            None
        }
    }

    const fn headers_after_the_first(self) -> usize {
        match self {
            Self::Vorbis => 2,
            Self::Opus => 1,
        }
    }

    fn untagged(self, comment: &[u8]) -> Option<Vec<u8>> {
        let (magic, framed) = match self {
            Self::Vorbis => (VORBIS_COMMENT, true),
            Self::Opus => (OPUS_TAGS, false),
        };
        let rest = comment.strip_prefix(magic)?;
        let vendor_bytes = u32::from_le_bytes(rest.get(..LENGTH_BYTES)?.try_into().ok()?);
        let vendor = rest.get(LENGTH_BYTES..LENGTH_BYTES.checked_add(vendor_bytes as usize)?)?;

        let mut untagged = Vec::with_capacity(magic.len() + 2 * LENGTH_BYTES + vendor.len() + 1);
        untagged.extend_from_slice(magic);
        untagged.extend_from_slice(&vendor_bytes.to_le_bytes());
        untagged.extend_from_slice(vendor);
        untagged.extend_from_slice(&0_u32.to_le_bytes());
        if framed {
            untagged.push(VORBIS_FRAMED);
        }
        Some(untagged)
    }
}

struct Page {
    raw: Vec<u8>,
    flags: u8,
    serial: u32,
    sequence: u32,
    lacing: Vec<u8>,
    body_at: usize,
}

impl Page {
    fn read(from: &mut impl Read) -> Option<Self> {
        let mut raw = vec![0_u8; HEADER_BYTES];
        from.read_exact(&mut raw).ok()?;
        if !raw.starts_with(CAPTURE) || raw[VERSION_AT] != 0 {
            return None;
        }
        let segments = usize::from(raw[SEGMENTS_AT]);
        let mut lacing = vec![0_u8; segments];
        from.read_exact(&mut lacing).ok()?;
        raw.extend_from_slice(&lacing);
        let body_at = raw.len();
        let body: usize = lacing.iter().map(|lace| usize::from(*lace)).sum();
        raw.resize(body_at + body, 0);
        from.read_exact(&mut raw[body_at..]).ok()?;

        Some(Self {
            flags: raw[FLAGS_AT],
            serial: u32::from_le_bytes(raw[SERIAL_AT..SEQUENCE_AT].try_into().ok()?),
            sequence: u32::from_le_bytes(raw[SEQUENCE_AT..CHECKSUM_AT].try_into().ok()?),
            raw,
            lacing,
            body_at,
        })
    }

    fn body(&self) -> &[u8] {
        &self.raw[self.body_at..]
    }
}

pub(crate) fn bare(stream: &mut impl Read) -> Option<Bared> {
    let first = Page::read(stream)?;
    let ends_one_packet = first
        .lacing
        .iter()
        .position(|lace| usize::from(*lace) < SEGMENT_BYTES)
        .is_some_and(|end| end + 1 == first.lacing.len());
    if first.flags & FIRST == 0 || first.flags & CONTINUED != 0 || !ends_one_packet {
        return None;
    }
    let codec = Codec::of(first.body())?;

    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut open = Vec::new();
    let mut last_sequence = first.sequence;
    let mut read = first.raw.len();
    while packets.len() < codec.headers_after_the_first() {
        let page = Page::read(stream)?;
        read += page.raw.len();
        if read > HEADER_BYTES_AT_MOST
            || page.serial != first.serial
            || page.sequence != last_sequence.wrapping_add(1)
            || (page.flags & CONTINUED != 0) != !open.is_empty()
        {
            return None;
        }
        last_sequence = page.sequence;

        let mut at = 0;
        for (segment, lace) in page.lacing.iter().enumerate() {
            let lace = usize::from(*lace);
            open.extend_from_slice(page.body().get(at..at + lace)?);
            at += lace;
            if lace < SEGMENT_BYTES {
                packets.push(std::mem::take(&mut open));
                let headers_done = packets.len() == codec.headers_after_the_first();
                if headers_done && segment + 1 != page.lacing.len() {
                    return None;
                }
            }
        }
    }

    let mut kept = packets.into_iter();
    let comment = kept.next()?;
    let untagged = codec.untagged(&comment)?;
    if untagged == comment {
        return None;
    }

    let rest: Vec<Vec<u8>> = std::iter::once(untagged).chain(kept).collect();
    let mut head = first.raw;
    let written = paginated(
        &rest,
        first.serial,
        first.sequence.wrapping_add(1),
        &mut head,
    );
    let shift = first
        .sequence
        .wrapping_add(written)
        .wrapping_sub(last_sequence);

    Some(Bared {
        head,
        renumbering: (shift != 0).then_some(Renumbering {
            serial: first.serial,
            shift,
        }),
    })
}

fn paginated(packets: &[Vec<u8>], serial: u32, first_sequence: u32, into: &mut Vec<u8>) -> u32 {
    let mut pieces: Vec<(u8, &[u8])> = Vec::new();
    for packet in packets {
        let mut chunks = packet.chunks(SEGMENT_BYTES).peekable();
        let mut ended = false;
        while let Some(chunk) = chunks.next() {
            pieces.push((chunk.len() as u8, chunk));
            ended = chunk.len() < SEGMENT_BYTES && chunks.peek().is_none();
        }
        if !ended {
            pieces.push((0, &[]));
        }
    }

    let mut sequence = first_sequence;
    let mut written = 0;
    let mut continued = false;
    for page in pieces.chunks(SEGMENTS_AT_MOST) {
        let ends_a_packet = page
            .iter()
            .any(|(lace, _)| usize::from(*lace) < SEGMENT_BYTES);
        let granule = if ends_a_packet { 0 } else { NO_PACKET_ENDS };

        let mut raw = Vec::with_capacity(HEADER_BYTES + page.len() * (SEGMENT_BYTES + 1));
        raw.extend_from_slice(CAPTURE);
        raw.push(0);
        raw.push(if continued { CONTINUED } else { 0 });
        raw.extend_from_slice(&granule.to_le_bytes());
        raw.extend_from_slice(&serial.to_le_bytes());
        raw.extend_from_slice(&sequence.to_le_bytes());
        raw.extend_from_slice(&0_u32.to_le_bytes());
        raw.push(page.len() as u8);
        raw.extend(page.iter().map(|(lace, _)| *lace));
        for (_, chunk) in page {
            raw.extend_from_slice(chunk);
        }
        stamped(&mut raw);
        into.extend_from_slice(&raw);

        continued = page
            .last()
            .is_some_and(|(lace, _)| usize::from(*lace) == SEGMENT_BYTES);
        sequence = sequence.wrapping_add(1);
        written += 1;
    }
    written
}

const fn checksum_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut at = 0;
    while at < table.len() {
        let mut remainder = (at as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            remainder = if remainder & 0x8000_0000 == 0 {
                remainder << 1
            } else {
                (remainder << 1) ^ CHECKSUM_POLYNOMIAL
            };
            bit += 1;
        }
        table[at] = remainder;
        at += 1;
    }
    table
}

fn checksum(page: &[u8]) -> u32 {
    page.iter().enumerate().fold(0_u32, |held, (at, byte)| {
        let byte = if (CHECKSUM_AT..SEGMENTS_AT).contains(&at) {
            0
        } else {
            *byte
        };
        (held << 8) ^ CHECKSUM_TABLE[((held >> 24) as u8 ^ byte) as usize]
    })
}

fn stamped(page: &mut [u8]) {
    let sum = checksum(page);
    page[CHECKSUM_AT..SEGMENTS_AT].copy_from_slice(&sum.to_le_bytes());
}

pub(crate) struct Renumbered<R> {
    inner: R,
    renumbering: Renumbering,
    pending: Vec<u8>,
    at: usize,
    passing: bool,
}

impl<R: Read> Renumbered<R> {
    pub(crate) const fn over(inner: R, renumbering: Renumbering) -> Self {
        Self {
            inner,
            renumbering,
            pending: Vec::new(),
            at: 0,
            passing: false,
        }
    }

    fn next_page(&mut self) -> io::Result<()> {
        self.pending.clear();
        self.at = 0;

        let mut header = [0_u8; HEADER_BYTES];
        let filled = filled_into(&mut self.inner, &mut header)?;
        self.pending.extend_from_slice(&header[..filled]);
        if filled < HEADER_BYTES || !header.starts_with(CAPTURE) {
            self.passing = true;
            return Ok(());
        }

        let segments = usize::from(header[SEGMENTS_AT]);
        let mut lacing = vec![0_u8; segments];
        let laced = filled_into(&mut self.inner, &mut lacing)?;
        self.pending.extend_from_slice(&lacing[..laced]);
        if laced < segments {
            self.passing = true;
            return Ok(());
        }

        let body: usize = lacing.iter().map(|lace| usize::from(*lace)).sum();
        let start = self.pending.len();
        self.pending.resize(start + body, 0);
        let bodied = filled_into(&mut self.inner, &mut self.pending[start..])?;
        self.pending.truncate(start + bodied);
        if bodied < body {
            self.passing = true;
            return Ok(());
        }

        let serial = u32::from_le_bytes([
            header[SERIAL_AT],
            header[SERIAL_AT + 1],
            header[SERIAL_AT + 2],
            header[SERIAL_AT + 3],
        ]);
        if serial == self.renumbering.serial {
            let sequence = u32::from_le_bytes([
                header[SEQUENCE_AT],
                header[SEQUENCE_AT + 1],
                header[SEQUENCE_AT + 2],
                header[SEQUENCE_AT + 3],
            ]);
            let moved = sequence.wrapping_add(self.renumbering.shift);
            self.pending[SEQUENCE_AT..CHECKSUM_AT].copy_from_slice(&moved.to_le_bytes());
            stamped(&mut self.pending);
        }
        Ok(())
    }
}

impl<R: Read> Read for Renumbered<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if self.at == self.pending.len() {
            if self.passing {
                return self.inner.read(into);
            }
            self.next_page()?;
        }
        let taken = (self.pending.len() - self.at).min(into.len());
        into[..taken].copy_from_slice(&self.pending[self.at..self.at + taken]);
        self.at += taken;
        Ok(taken)
    }
}

fn filled_into(from: &mut impl Read, into: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < into.len() {
        match from.read(&mut into[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const OPUS_FIRST_PAGE: &str = "4f6767530002000000000000000037d32bd30000000048601b6c011\
                                   34f707573486561640101380180bb0000000000";

    fn bytes_of(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect()
    }

    fn page(flags: u8, serial: u32, sequence: u32, lacing: &[u8], body: &[u8]) -> Vec<u8> {
        let mut raw = CAPTURE.to_vec();
        raw.push(0);
        raw.push(flags);
        raw.extend_from_slice(&0_u64.to_le_bytes());
        raw.extend_from_slice(&serial.to_le_bytes());
        raw.extend_from_slice(&sequence.to_le_bytes());
        raw.extend_from_slice(&0_u32.to_le_bytes());
        raw.push(lacing.len() as u8);
        raw.extend_from_slice(lacing);
        raw.extend_from_slice(body);
        stamped(&mut raw);
        raw
    }

    fn laced(length: usize) -> Vec<u8> {
        let mut lacing = vec![255_u8; length / SEGMENT_BYTES];
        lacing.push((length % SEGMENT_BYTES) as u8);
        lacing
    }

    fn opus_tags(comments: &[&str]) -> Vec<u8> {
        let mut packet = OPUS_TAGS.to_vec();
        packet.extend_from_slice(&7_u32.to_le_bytes());
        packet.extend_from_slice(b"encoder");
        packet.extend_from_slice(&(comments.len() as u32).to_le_bytes());
        for comment in comments {
            packet.extend_from_slice(&(comment.len() as u32).to_le_bytes());
            packet.extend_from_slice(comment.as_bytes());
        }
        packet
    }

    fn pages_of(mut bytes: &[u8]) -> Vec<Page> {
        let mut pages = Vec::new();
        while !bytes.is_empty() {
            let page = Page::read(&mut bytes).expect("a whole page");
            pages.push(page);
        }
        pages
    }

    #[test]
    fn a_page_is_stamped_with_the_checksum_libogg_gives_it() {
        let written = bytes_of(OPUS_FIRST_PAGE);
        let mut restamped = written.clone();
        restamped[CHECKSUM_AT..SEGMENTS_AT].copy_from_slice(&[0; 4]);

        stamped(&mut restamped);

        assert_eq!(restamped, written);
    }

    #[test]
    fn an_opus_stream_is_given_empty_tags_and_every_page_after_them_is_numbered_again() {
        let serial = 0xd32b_d337;
        let first = bytes_of(OPUS_FIRST_PAGE);
        let tags = opus_tags(&[
            "TITLE=Echoes",
            &format!("METADATA_BLOCK_PICTURE={}", "x".repeat(70_000)),
        ]);
        let (early, late) = tags.split_at(255 * 255);
        let audio = [
            page(0, serial, 3, &[40], &[7; 40]),
            page(0, serial, 4, &[9], &[8; 9]),
        ]
        .concat();
        let whole = [
            first.clone(),
            page(0, serial, 1, &[255; 255], early),
            page(CONTINUED, serial, 2, &laced(late.len()), late),
            audio.clone(),
        ]
        .concat();

        let mut stream = Cursor::new(whole);
        let bared = bare(&mut stream).expect("the tags are shed");
        let mut rest = Vec::new();
        Renumbered::over(&mut stream, bared.renumbering.expect("the pages move"))
            .read_to_end(&mut rest)
            .expect("the rest");

        assert_eq!(&bared.head[..first.len()], first.as_slice());
        let written = [bared.head.clone(), rest].concat();
        let pages = pages_of(&written);
        let sequences: Vec<u32> = pages.iter().map(|page| page.sequence).collect();
        assert_eq!(sequences, vec![0, 1, 2, 3]);
        for page in &pages {
            assert_eq!(
                checksum(&page.raw).to_le_bytes(),
                page.raw[CHECKSUM_AT..SEGMENTS_AT]
            );
        }
        assert_eq!(
            pages[1].body(),
            Codec::Opus.untagged(&tags).expect("tags").as_slice()
        );
        assert_eq!(pages[2].body(), &[7; 40]);
        assert_eq!(pages[3].body(), &[8; 9]);
    }

    #[test]
    fn a_vorbis_comment_keeps_its_vendor_and_its_framing_bit_and_loses_every_comment() {
        let mut comment = VORBIS_COMMENT.to_vec();
        comment.extend_from_slice(&4_u32.to_le_bytes());
        comment.extend_from_slice(b"Xiph");
        comment.extend_from_slice(&1_u32.to_le_bytes());
        comment.extend_from_slice(&13_u32.to_le_bytes());
        comment.extend_from_slice(b"ARTIST=Floyd!");
        comment.push(VORBIS_FRAMED);

        let untagged = Codec::Vorbis.untagged(&comment).expect("a comment packet");

        let mut wanted = VORBIS_COMMENT.to_vec();
        wanted.extend_from_slice(&4_u32.to_le_bytes());
        wanted.extend_from_slice(b"Xiph");
        wanted.extend_from_slice(&0_u32.to_le_bytes());
        wanted.push(VORBIS_FRAMED);
        assert_eq!(untagged, wanted);
    }

    #[test]
    fn a_stream_whose_tags_are_already_empty_is_left_as_it_stands() {
        let serial = 9;
        let first = bytes_of(OPUS_FIRST_PAGE);
        let tags = opus_tags(&[]);
        let whole = [first, page(0, serial, 1, &laced(tags.len()), &tags)].concat();

        assert_eq!(bare(&mut Cursor::new(whole)), None);
    }

    #[test]
    fn a_header_page_that_carries_audio_as_well_is_left_as_it_stands() {
        let serial = 0xd32b_d337;
        let first = bytes_of(OPUS_FIRST_PAGE);
        let tags = opus_tags(&["TITLE=Echoes"]);
        let mut lacing = laced(tags.len());
        lacing.push(3);
        let body = [tags.as_slice(), &[1, 2, 3]].concat();
        let whole = [first, page(0, serial, 1, &lacing, &body)].concat();

        assert_eq!(bare(&mut Cursor::new(whole)), None);
    }

    #[test]
    fn a_stream_that_is_not_vorbis_or_opus_is_left_as_it_stands() {
        let flac = page(FIRST, 1, 0, &[9], b"\x7fFLAC\x01\x00\x00\x01");

        assert_eq!(bare(&mut Cursor::new(flac)), None);
    }
}
