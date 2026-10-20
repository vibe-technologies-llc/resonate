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
const BOXES_WALKED_AT_MOST: usize = 1 << 20;

const EBML: u32 = 0x1a45_dfa3;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114d_9b74;
const INFO: u32 = 0x1549_a966;
const TRACKS: u32 = 0x1654_ae6b;
const CHAPTERS: u32 = 0x1043_a770;
const CLUSTER: u32 = 0x1f43_b675;
const CUES: u32 = 0x1c53_bb6b;
const ATTACHMENTS: u32 = 0x1941_a469;
const TAGS: u32 = 0x1254_c367;
const TITLE: u32 = 0x7ba9;
const TRACK_ENTRY: u32 = 0xae;
const TRACK_NAME: u32 = 0x536e;
const VOID: u8 = 0xec;
const VOID_ELEMENT: u32 = VOID as u32;
const CHECKSUM: u32 = 0xbf;
const SEGMENT_CHILDREN: [u32; 8] = [
    SEEK_HEAD,
    INFO,
    TRACKS,
    CHAPTERS,
    CLUSTER,
    CUES,
    ATTACHMENTS,
    TAGS,
];
const VOIDED_WHOLE: [u32; 3] = [TAGS, ATTACHMENTS, CHAPTERS];
const BLANKED_WITHIN: [u32; 5] = [TAGS, ATTACHMENTS, CHAPTERS, INFO, TRACKS];
const LONGEST_ID: usize = 4;
const LONGEST_SIZE: usize = 8;
const ELEMENTS_WALKED_AT_MOST: usize = 1 << 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unwalked {
    NotThisContainer,
    Unreadable,
    Misformed,
    RunsPast,
    UnsizedWithinUnsized,
    TooManyElements,
    Unvoidable,
}

pub(crate) type Walked = Result<Vec<Blank>, Unwalked>;

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

pub(crate) fn blanked_movie<S: Read + Seek + ?Sized>(stream: &mut S) -> Walked {
    let end = stream
        .seek(SeekFrom::End(0))
        .map_err(|_| Unwalked::Unreadable)?;
    let mut blanks = Vec::new();
    let mut walked = 0;
    boxes(stream, 0..end, Within::TheFile, &mut blanks, &mut walked)?;
    Ok(blanks)
}

fn boxes<S: Read + Seek + ?Sized>(
    stream: &mut S,
    within: Range<u64>,
    inside: Within,
    blanks: &mut Vec<Blank>,
    walked: &mut usize,
) -> Result<(), Unwalked> {
    let mut at = within.start;
    while at + BOX_HEADER_BYTES <= within.end {
        *walked += 1;
        if *walked > BOXES_WALKED_AT_MOST {
            return Err(Unwalked::TooManyElements);
        }
        stream
            .seek(SeekFrom::Start(at))
            .map_err(|_| Unwalked::Unreadable)?;
        let mut header = [0_u8; BOX_HEADER_BYTES as usize];
        stream
            .read_exact(&mut header)
            .map_err(|_| Unwalked::Unreadable)?;
        let declared = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let kind: [u8; 4] = [header[4], header[5], header[6], header[7]];
        let (size, header_bytes) = match declared {
            TO_THE_END => (within.end - at, BOX_HEADER_BYTES),
            LARGE => {
                let mut large = [0_u8; 8];
                stream
                    .read_exact(&mut large)
                    .map_err(|_| Unwalked::Unreadable)?;
                (u64::from_be_bytes(large), LARGE_BOX_HEADER_BYTES)
            }
            declared => (u64::from(declared), BOX_HEADER_BYTES),
        };
        if size < header_bytes {
            return Err(Unwalked::Misformed);
        }
        if at.checked_add(size).is_none_or(|past| past > within.end) {
            return Err(Unwalked::RunsPast);
        }
        let body = at + header_bytes..at + size;
        match (inside, &kind) {
            (Within::TheFile, MOVIE) => boxes(stream, body, Within::TheMovie, blanks, walked)?,
            (Within::TheMovie, TRACK) => boxes(stream, body, Within::ATrack, blanks, walked)?,
            (_, USER_DATA | METADATA) => {
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
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Element {
    id: u32,
    at: u64,
    body: u64,
    size: Option<u64>,
}

struct Segments<'a, S: ?Sized> {
    stream: &'a mut S,
    end: u64,
    walked: usize,
    blanks: Vec<Blank>,
}

pub(crate) fn blanked_segment<S: Read + Seek + ?Sized>(stream: &mut S) -> Walked {
    let end = stream
        .seek(SeekFrom::End(0))
        .map_err(|_| Unwalked::Unreadable)?;
    let mut walking = Segments {
        stream,
        end,
        walked: 0,
        blanks: Vec::new(),
    };
    let first = walking.element_at(0)?.ok_or(Unwalked::NotThisContainer)?;
    if first.id != EBML {
        return Err(Unwalked::NotThisContainer);
    }

    let mut at = 0;
    while at < end {
        let Some(element) = walking.element_at(at)? else {
            break;
        };
        at = match element.id {
            SEGMENT => walking.segment(element)?,
            EBML | VOID_ELEMENT | CHECKSUM => sized_past(element, end)?,
            _ => return Err(Unwalked::Misformed),
        };
    }
    Ok(walking.blanks)
}

impl<S: Read + Seek + ?Sized> Segments<'_, S> {
    fn element_at(&mut self, at: u64) -> Result<Option<Element>, Unwalked> {
        self.walked += 1;
        if self.walked > ELEMENTS_WALKED_AT_MOST {
            return Err(Unwalked::TooManyElements);
        }
        self.stream
            .seek(SeekFrom::Start(at))
            .map_err(|_| Unwalked::Unreadable)?;
        let Some((id, size)) = element(self.stream)? else {
            return Ok(None);
        };
        let body = self
            .stream
            .stream_position()
            .map_err(|_| Unwalked::Unreadable)?;
        Ok(Some(Element { id, at, body, size }))
    }

    fn segment(&mut self, segment: Element) -> Result<u64, Unwalked> {
        let ends = segment.size.map_or(self.end, |size| {
            segment.body.saturating_add(size).min(self.end)
        });
        let mut at = segment.body;
        while at < ends {
            let Some(element) = self.element_at(at)? else {
                return Ok(self.end);
            };
            if segment.size.is_none() && matches!(element.id, EBML | SEGMENT) {
                return Ok(at);
            }
            let past = match element.size {
                Some(size) => match element.body.checked_add(size) {
                    Some(past) if past <= ends => past,
                    Some(_) if ends == self.end && !BLANKED_WITHIN.contains(&element.id) => {
                        return Ok(self.end);
                    }
                    _ => return Err(Unwalked::RunsPast),
                },
                None => self.unsized_ends(element.body, ends)?,
            };
            self.blank(element, past)?;
            at = past;
        }
        Ok(ends)
    }

    fn unsized_ends(&mut self, from: u64, within: u64) -> Result<u64, Unwalked> {
        let mut at = from;
        while at < within {
            let Some(child) = self.element_at(at)? else {
                return Ok(self.end);
            };
            if SEGMENT_CHILDREN.contains(&child.id) || matches!(child.id, EBML | SEGMENT) {
                return Ok(at);
            }
            let size = child.size.ok_or(Unwalked::UnsizedWithinUnsized)?;
            match child.body.checked_add(size) {
                Some(past) if past <= within => at = past,
                Some(_) if within == self.end => return Ok(self.end),
                _ => return Err(Unwalked::RunsPast),
            }
        }
        Ok(within)
    }

    fn blank(&mut self, element: Element, past: u64) -> Result<(), Unwalked> {
        if VOIDED_WHOLE.contains(&element.id) {
            let voided = void(element.at..past).ok_or(Unwalked::Unvoidable)?;
            self.blanks.extend(voided);
        } else if element.id == INFO {
            self.voided_within(element.body..past, TITLE)?;
        } else if element.id == TRACKS {
            let mut at = element.body;
            while at < past {
                let entry = self.element_at(at)?.ok_or(Unwalked::RunsPast)?;
                let entry_past = sized_past(entry, past)?;
                if entry.id == TRACK_ENTRY {
                    self.voided_within(entry.body..entry_past, TRACK_NAME)?;
                }
                at = entry_past;
            }
        }
        Ok(())
    }

    fn voided_within(&mut self, within: Range<u64>, named: u32) -> Result<(), Unwalked> {
        let mut at = within.start;
        while at < within.end {
            let child = self.element_at(at)?.ok_or(Unwalked::RunsPast)?;
            let past = sized_past(child, within.end)?;
            if child.id == named {
                let voided = void(at..past).ok_or(Unwalked::Unvoidable)?;
                self.blanks.extend(voided);
            }
            at = past;
        }
        Ok(())
    }
}

fn sized_past(element: Element, within: u64) -> Result<u64, Unwalked> {
    let size = element.size.ok_or(Unwalked::UnsizedWithinUnsized)?;
    element
        .body
        .checked_add(size)
        .filter(|past| *past <= within)
        .ok_or(Unwalked::RunsPast)
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

fn element<S: Read + ?Sized>(stream: &mut S) -> Result<Option<(u32, Option<u64>)>, Unwalked> {
    let mut first = [0_u8; 1];
    if !read_one(stream, &mut first)? {
        return Ok(None);
    }
    let id_width = first[0].leading_zeros() as usize + 1;
    if id_width > LONGEST_ID {
        return Err(Unwalked::Misformed);
    }
    let mut id = u32::from(first[0]);
    for _ in 1..id_width {
        if !read_one(stream, &mut first)? {
            return Ok(None);
        }
        id = (id << 8) | u32::from(first[0]);
    }

    if !read_one(stream, &mut first)? {
        return Ok(None);
    }
    let size_width = first[0].leading_zeros() as usize + 1;
    if size_width > LONGEST_SIZE {
        return Err(Unwalked::Misformed);
    }
    let bits_in_the_first = (0xff_u16 >> size_width) as u8;
    let mut size = u64::from(first[0] & bits_in_the_first);
    let mut unknown = first[0] & bits_in_the_first == bits_in_the_first;
    for _ in 1..size_width {
        if !read_one(stream, &mut first)? {
            return Ok(None);
        }
        size = (size << 8) | u64::from(first[0]);
        unknown &= first[0] == 0xff;
    }
    Ok(Some((id, (!unknown).then_some(size))))
}

fn read_one<S: Read + ?Sized>(stream: &mut S, into: &mut [u8; 1]) -> Result<bool, Unwalked> {
    match stream.read_exact(into) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(_) => Err(Unwalked::Unreadable),
    }
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
            &[
                atom(b"tkhd", &[1; 12]),
                atom(USER_DATA, b"name"),
                atom(METADATA, b"a track's own credits"),
            ]
            .concat(),
        );
        let movie = atom(MOVIE, &[atom(b"mvhd", &[2; 20]), track, tags].concat());
        let file = [atom(b"ftyp", b"M4A mp42"), movie, atom(b"mdat", &[7; 64])].concat();

        let blanks = blanked_movie(&mut Cursor::new(file.clone())).expect("tags to blank");
        let copied = laid(&file, blanks);

        assert_eq!(copied.len(), file.len());
        assert!(!copied.windows(4).any(|window| window == USER_DATA));
        assert!(!copied.windows(5).any(|window| window == b"cover"));
        assert!(!copied.windows(7).any(|window| window == b"credits"));
        assert!(!copied.windows(4).any(|window| window == METADATA));
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
        assert_eq!(blanked_movie(&mut Cursor::new(file)), Ok(Vec::new()));
    }

    #[test]
    fn a_box_running_past_the_file_is_not_walked() {
        let mut file = atom(b"ftyp", b"M4A mp42");
        file.extend_from_slice(&1_000_u32.to_be_bytes());
        file.extend_from_slice(MOVIE);
        assert_eq!(
            blanked_movie(&mut Cursor::new(file)),
            Err(Unwalked::RunsPast)
        );
    }

    #[test]
    fn a_movie_of_many_more_boxes_than_an_album_holds_is_still_walked_to_its_tags() {
        let fragments: Vec<u8> = (0..6_000)
            .flat_map(|_| [atom(b"moof", &[3; 8]), atom(b"mdat", &[7; 8])].concat())
            .collect();
        let file = [
            atom(b"ftyp", b"M4A mp42"),
            fragments,
            atom(MOVIE, &atom(USER_DATA, b"a title")),
        ]
        .concat();

        let blanks = blanked_movie(&mut Cursor::new(file.clone())).expect("a walk to the end");
        let copied = laid(&file, blanks);

        assert!(!copied.windows(7).any(|window| window == b"a title"));
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

    fn of_unknown_size(id: u32, body: &[u8]) -> Vec<u8> {
        let mut held: Vec<u8> = id
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        held.extend_from_slice(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        held.extend_from_slice(body);
        held
    }

    fn matroska(segment: &[u8]) -> Vec<u8> {
        [ebml(EBML, &[0x42, 0x82, 0x88]), ebml(SEGMENT, segment)].concat()
    }

    #[test]
    fn a_cluster_of_unknown_size_is_walked_through_to_the_tags_after_it() {
        const SIMPLE_BLOCK: u32 = 0xa3;
        const TIMESTAMP: u32 = 0xe7;
        let cluster = of_unknown_size(
            CLUSTER,
            &[
                ebml(TIMESTAMP, &[0]),
                ebml(SIMPLE_BLOCK, &[5; 40]),
                ebml(SIMPLE_BLOCK, &[6; 40]),
            ]
            .concat(),
        );
        let file = matroska(&[cluster, ebml(TAGS, b"an artist")].concat());

        let blanks = blanked_segment(&mut Cursor::new(file.clone())).expect("a walk to the end");
        let copied = laid(&file, blanks);

        assert!(!copied.windows(9).any(|window| window == b"an artist"));
        assert!(copied.windows(40).any(|window| window == [6; 40]));
    }

    #[test]
    fn a_track_name_and_the_chapters_become_void_and_the_rest_of_the_entry_stays() {
        const CODEC_ID: u32 = 0x86;
        let entry = ebml(
            TRACK_ENTRY,
            &[ebml(TRACK_NAME, b"Commentary"), ebml(CODEC_ID, b"A_OPUS")].concat(),
        );
        let file = matroska(
            &[
                ebml(TRACKS, &entry),
                ebml(CHAPTERS, b"Overture"),
                ebml(CLUSTER, &[9; 16]),
            ]
            .concat(),
        );

        let blanks = blanked_segment(&mut Cursor::new(file.clone())).expect("a walk to the end");
        let copied = laid(&file, blanks);

        assert_eq!(copied.len(), file.len());
        assert!(!copied.windows(10).any(|window| window == b"Commentary"));
        assert!(!copied.windows(8).any(|window| window == b"Overture"));
        assert!(copied.windows(6).any(|window| window == b"A_OPUS"));
    }

    #[test]
    fn a_segment_whose_tags_run_past_it_or_whose_bytes_name_nothing_is_not_called_blanked() {
        let mut cut = matroska(&[ebml(CLUSTER, &[9; 16]), ebml(TAGS, b"an artist")].concat());
        cut.truncate(cut.len() - 3);
        let nested = matroska(&of_unknown_size(CLUSTER, &of_unknown_size(0xa0, &[1; 8])));
        let mut misformed = matroska(&ebml(TAGS, b"an artist"));
        misformed.extend_from_slice(&[0x00, 0x00]);

        assert_eq!(
            blanked_segment(&mut Cursor::new(cut)),
            Err(Unwalked::RunsPast)
        );
        assert_eq!(
            blanked_segment(&mut Cursor::new(nested)),
            Err(Unwalked::UnsizedWithinUnsized)
        );
        assert_eq!(
            blanked_segment(&mut Cursor::new(misformed)),
            Err(Unwalked::Misformed)
        );
    }

    #[test]
    fn a_last_cluster_cut_short_ends_the_walk_with_everything_before_it_blanked() {
        let mut file = matroska(&[ebml(TAGS, b"an artist"), ebml(CLUSTER, &[9; 64])].concat());
        file.truncate(file.len() - 10);

        let blanks = blanked_segment(&mut Cursor::new(file.clone())).expect("a walk to the end");

        assert!(
            !laid(&file, blanks)
                .windows(9)
                .any(|window| window == b"an artist")
        );
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
        let (id, size) = element(&mut read)
            .expect("a readable element")
            .expect("a void where the tags stood");
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
            let (id, size) = element(&mut Cursor::new(bytes.clone()))
                .expect("a readable element")
                .expect("a readable void");
            assert_eq!(id, u32::from(VOID));
            assert_eq!(bytes.len() as u64 + size.expect("a known size"), length);
            assert_eq!(rest.end, length);
        }
    }
}
