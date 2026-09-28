use std::{
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
};

const BOX_HEADER_BYTES: u64 = 8;
const LARGE_BOX_HEADER_BYTES: u64 = 16;
const TO_THE_END: u32 = 0;
const LARGE: u32 = 1;
const MOVIE: &[u8; 4] = b"moov";
const TRACK: &[u8; 4] = b"trak";
const USER_DATA: &[u8; 4] = b"udta";
const METADATA: &[u8; 4] = b"meta";
const FREE: &[u8; 4] = b"free";
const KIND_AT: u64 = 4;
const BOXES_WALKED_AT_MOST: usize = 4_096;

const EBML: u32 = 0x1a45_dfa3;
const SEGMENT: u32 = 0x1853_8067;
const TAGS: u32 = 0x1254_c367;
const ATTACHMENTS: u32 = 0x1941_a469;
const INFO: u32 = 0x1549_a966;
const TITLE: u32 = 0x7ba9;
const VOID: u8 = 0xec;
const LONGEST_ID: usize = 4;
const LONGEST_SIZE: usize = 8;
const ELEMENTS_WALKED_AT_MOST: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Blank {
    Bytes { at: u64, bytes: Vec<u8> },
    Zeros(Range<u64>),
}

impl Blank {
    fn span(&self) -> Range<u64> {
        match self {
            Self::Bytes { at, bytes } => *at..*at + bytes.len() as u64,
            Self::Zeros(range) => range.clone(),
        }
    }

    fn lay_over(&self, read: &mut [u8], from: u64) {
        let over = from..from + read.len() as u64;
        let span = self.span();
        let start = span.start.max(over.start);
        let end = span.end.min(over.end);
        if start >= end {
            return;
        }
        let (Ok(into), Ok(to)) = (usize::try_from(start - from), usize::try_from(end - from))
        else {
            return;
        };
        match self {
            Self::Bytes { at, bytes } => {
                let (Ok(first), Ok(last)) =
                    (usize::try_from(start - at), usize::try_from(end - at))
                else {
                    return;
                };
                read[into..to].copy_from_slice(&bytes[first..last]);
            }
            Self::Zeros(_) => read[into..to].fill(0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Within {
    TheFile,
    TheMovie,
    ATrack,
}

pub(crate) fn blanked_movie<S: Read + Seek + ?Sized>(stream: &mut S) -> Option<Vec<Blank>> {
    let end = stream.seek(SeekFrom::End(0)).ok()?;
    let mut blanks = Vec::new();
    let mut walked = 0;
    boxes(stream, 0..end, Within::TheFile, &mut blanks, &mut walked)?;
    (!blanks.is_empty()).then_some(blanks)
}

fn boxes<S: Read + Seek + ?Sized>(
    stream: &mut S,
    within: Range<u64>,
    inside: Within,
    blanks: &mut Vec<Blank>,
    walked: &mut usize,
) -> Option<()> {
    let mut at = within.start;
    while at + BOX_HEADER_BYTES <= within.end {
        *walked += 1;
        if *walked > BOXES_WALKED_AT_MOST {
            return None;
        }
        stream.seek(SeekFrom::Start(at)).ok()?;
        let mut header = [0_u8; BOX_HEADER_BYTES as usize];
        stream.read_exact(&mut header).ok()?;
        let declared = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let kind: [u8; 4] = [header[4], header[5], header[6], header[7]];
        let (size, header_bytes) = match declared {
            TO_THE_END => (within.end - at, BOX_HEADER_BYTES),
            LARGE => {
                let mut large = [0_u8; 8];
                stream.read_exact(&mut large).ok()?;
                (u64::from_be_bytes(large), LARGE_BOX_HEADER_BYTES)
            }
            declared => (u64::from(declared), BOX_HEADER_BYTES),
        };
        if size < header_bytes || at.checked_add(size)? > within.end {
            return None;
        }
        let body = at + header_bytes..at + size;
        match (inside, &kind) {
            (Within::TheFile, MOVIE) => boxes(stream, body, Within::TheMovie, blanks, walked)?,
            (Within::TheMovie, TRACK) => boxes(stream, body, Within::ATrack, blanks, walked)?,
            (_, USER_DATA) | (Within::TheFile | Within::TheMovie, METADATA) => {
                blanks.push(Blank::Bytes {
                    at: at + KIND_AT,
                    bytes: FREE.to_vec(),
                });
                if !body.is_empty() {
                    blanks.push(Blank::Zeros(body));
                }
            }
            _ => {}
        }
        at += size;
    }
    Some(())
}

pub(crate) fn blanked_segment<S: Read + Seek + ?Sized>(stream: &mut S) -> Option<Vec<Blank>> {
    let end = stream.seek(SeekFrom::End(0)).ok()?;
    stream.seek(SeekFrom::Start(0)).ok()?;
    let (id, size) = element(stream)?;
    if id != EBML {
        return None;
    }
    let mut at = stream.stream_position().ok()?.checked_add(size?)?;
    stream.seek(SeekFrom::Start(at)).ok()?;
    let (id, size) = element(stream)?;
    if id != SEGMENT {
        return None;
    }
    let body_starts = stream.stream_position().ok()?;
    let body_ends = size.map_or(end, |size| body_starts.saturating_add(size).min(end));

    let mut blanks = Vec::new();
    at = body_starts;
    let mut walked = 0;
    while at < body_ends {
        walked += 1;
        if walked > ELEMENTS_WALKED_AT_MOST {
            break;
        }
        stream.seek(SeekFrom::Start(at)).ok()?;
        let Some((id, size)) = element(stream) else {
            break;
        };
        let Some(size) = size else {
            break;
        };
        let body = stream.stream_position().ok()?;
        let Some(past) = body.checked_add(size).filter(|past| *past <= body_ends) else {
            break;
        };
        if id == TAGS || id == ATTACHMENTS {
            blanks.extend(void(at..past)?);
        }
        if id == INFO
            && let Some(titles) = titles_within(stream, body..past)
        {
            blanks.extend(titles);
        }
        at = past;
    }
    (!blanks.is_empty()).then_some(blanks)
}

fn titles_within<S: Read + Seek + ?Sized>(
    stream: &mut S,
    within: Range<u64>,
) -> Option<Vec<Blank>> {
    let mut blanks = Vec::new();
    let mut at = within.start;
    while at < within.end {
        stream.seek(SeekFrom::Start(at)).ok()?;
        let (id, size) = element(stream)?;
        let body = stream.stream_position().ok()?;
        let past = body.checked_add(size?).filter(|past| *past <= within.end)?;
        if id == TITLE {
            blanks.extend(void(at..past)?);
        }
        at = past;
    }
    Some(blanks)
}

fn void(whole: Range<u64>) -> Option<[Blank; 2]> {
    let length = whole.end - whole.start;
    let (width, payload) = (1..=LONGEST_SIZE).rev().find_map(|width| {
        let payload = length.checked_sub(1 + width as u64)?;
        (payload < (1_u64 << (7 * width)) - 1).then_some((width, payload))
    })?;
    let mut header = vec![VOID];
    let marked = payload | (1_u64 << (7 * width));
    header.extend_from_slice(&marked.to_be_bytes()[LONGEST_SIZE - width..]);
    let written = whole.start + header.len() as u64;
    Some([
        Blank::Bytes {
            at: whole.start,
            bytes: header,
        },
        Blank::Zeros(written..whole.end),
    ])
}

fn element<S: Read + ?Sized>(stream: &mut S) -> Option<(u32, Option<u64>)> {
    let mut first = [0_u8; 1];
    stream.read_exact(&mut first).ok()?;
    let id_width = first[0].leading_zeros() as usize + 1;
    if id_width > LONGEST_ID {
        return None;
    }
    let mut id = u32::from(first[0]);
    for _ in 1..id_width {
        stream.read_exact(&mut first).ok()?;
        id = (id << 8) | u32::from(first[0]);
    }

    stream.read_exact(&mut first).ok()?;
    let size_width = first[0].leading_zeros() as usize + 1;
    if size_width > LONGEST_SIZE {
        return None;
    }
    let bits_in_the_first = (0xff_u16 >> size_width) as u8;
    let mut size = u64::from(first[0] & bits_in_the_first);
    let mut unknown = first[0] & bits_in_the_first == bits_in_the_first;
    for _ in 1..size_width {
        stream.read_exact(&mut first).ok()?;
        size = (size << 8) | u64::from(first[0]);
        unknown &= first[0] == 0xff;
    }
    Some((id, (!unknown).then_some(size)))
}

pub(crate) struct Blanked<R> {
    inner: R,
    at: u64,
    blanks: Vec<Blank>,
}

impl<R: Read> Blanked<R> {
    pub(crate) const fn over(inner: R, at: u64, blanks: Vec<Blank>) -> Self {
        Self { inner, at, blanks }
    }
}

impl<R: Read> Read for Blanked<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        let Some(held) = buffer.get_mut(..read) else {
            return Ok(0);
        };
        for blank in &self.blanks {
            blank.lay_over(held, self.at);
        }
        self.at += read as u64;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut held = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        held.extend_from_slice(kind);
        held.extend_from_slice(body);
        held
    }

    fn laid(file: &[u8], blanks: Vec<Blank>) -> Vec<u8> {
        let mut copied = Vec::new();
        Blanked::over(Cursor::new(file.to_vec()), 0, blanks)
            .read_to_end(&mut copied)
            .expect("a copy");
        copied
    }

    #[test]
    fn a_movies_user_data_and_metadata_become_free_space_of_the_same_size() {
        let tags = atom(USER_DATA, &atom(METADATA, b"a cover and a title"));
        let track = atom(
            TRACK,
            &[atom(b"tkhd", &[1; 12]), atom(USER_DATA, b"name")].concat(),
        );
        let movie = atom(MOVIE, &[atom(b"mvhd", &[2; 20]), track, tags].concat());
        let file = [atom(b"ftyp", b"M4A mp42"), movie, atom(b"mdat", &[7; 64])].concat();

        let blanks = blanked_movie(&mut Cursor::new(file.clone())).expect("tags to blank");
        let copied = laid(&file, blanks);

        assert_eq!(copied.len(), file.len());
        assert!(!copied.windows(4).any(|window| window == USER_DATA));
        assert!(!copied.windows(5).any(|window| window == b"cover"));
        assert!(copied.windows(4).any(|window| window == b"tkhd"));
        assert!(copied.ends_with(&atom(b"mdat", &[7; 64])));
    }

    #[test]
    fn a_movie_with_nothing_to_blank_is_copied_as_it_stands() {
        let file = [
            atom(b"ftyp", b"M4A mp42"),
            atom(MOVIE, &atom(b"mvhd", &[2; 20])),
            atom(b"mdat", &[7; 64]),
        ]
        .concat();
        assert_eq!(blanked_movie(&mut Cursor::new(file)), None);
    }

    #[test]
    fn a_box_running_past_the_file_is_not_walked() {
        let mut file = atom(b"ftyp", b"M4A mp42");
        file.extend_from_slice(&1_000_u32.to_be_bytes());
        file.extend_from_slice(MOVIE);
        assert_eq!(blanked_movie(&mut Cursor::new(file)), None);
    }

    fn ebml(id: u32, body: &[u8]) -> Vec<u8> {
        let mut held: Vec<u8> = id
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        held.push(0x01);
        held.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        held.extend_from_slice(body);
        held
    }

    #[test]
    fn a_segments_tags_and_attachments_become_void_of_the_same_length() {
        let info = [
            ebml(TITLE, b"Echoes"),
            ebml(0x002a_d7b1, &[0x0f, 0x42, 0x40]),
        ]
        .concat();
        let segment = [
            ebml(INFO, &info),
            ebml(TAGS, b"a title and an artist"),
            ebml(ATTACHMENTS, b"a cover"),
            ebml(0x1f43_b675, &[9; 32]),
        ]
        .concat();
        let file = [ebml(EBML, &[0x42, 0x82, 0x88]), ebml(SEGMENT, &segment)].concat();

        let blanks = blanked_segment(&mut Cursor::new(file.clone())).expect("tags to blank");
        let copied = laid(&file, blanks);

        assert_eq!(copied.len(), file.len());
        assert!(!copied.windows(5).any(|window| window == b"title"));
        assert!(!copied.windows(5).any(|window| window == b"cover"));
        assert!(!copied.windows(6).any(|window| window == b"Echoes"));
        assert!(copied.windows(3).any(|window| window == [0x0f, 0x42, 0x40]));
        assert!(copied.ends_with(&ebml(0x1f43_b675, &[9; 32])));

        let mut read = Cursor::new(copied);
        read.set_position((file.len() - segment.len()) as u64 + ebml(INFO, &info).len() as u64);
        let (id, size) = element(&mut read).expect("a void where the tags stood");
        assert_eq!(id, u32::from(VOID));
        assert_eq!(
            read.position() + size.expect("a known size"),
            ((file.len() - segment.len())
                + ebml(INFO, &info).len()
                + ebml(TAGS, b"a title and an artist").len()) as u64
        );
    }

    #[test]
    fn a_void_is_written_however_short_the_element_it_stands_in_for() {
        for length in 2..24_u64 {
            let [Blank::Bytes { bytes, .. }, Blank::Zeros(rest)] = void(0..length).expect("a void")
            else {
                panic!("a void is a header and its zeros");
            };
            let (id, size) = element(&mut Cursor::new(bytes.clone())).expect("a readable void");
            assert_eq!(id, u32::from(VOID));
            assert_eq!(bytes.len() as u64 + size.expect("a known size"), length);
            assert_eq!(rest.end, length);
        }
    }
}
