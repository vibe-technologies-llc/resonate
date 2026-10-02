use std::io::{self, ErrorKind, Read};

use crate::manifest::Timeline;

const SIZED_HEADER: u64 = 8;
const LARGE_HEADER: u64 = 16;
const LARGEST_MOVIE: u64 = 16 * 1024 * 1024;
const FLAC_MARKER: &[u8; 4] = b"fLaC";
const FULL_BOX_PREFIX: usize = 4;
const SAMPLE_DESCRIPTIONS_PREFIX: usize = 8;
const AUDIO_ENTRY_PREFIX: usize = 28;
const BLOCK_HEADER: usize = 4;
const LAST_BLOCK: u8 = 0x80;
const BLOCK_KIND: u8 = 0x7f;
const STREAMINFO: u8 = 0;
const STREAMINFO_LENGTH: usize = 34;
const PACKED_AT: usize = 10;
const PACKED_LENGTH: usize = 8;
const RATE_SHIFT: u32 = 44;
const TOTAL_BITS: u32 = 36;
const MOVIE_PATH: [&[u8; 4]; 5] = [b"trak", b"mdia", b"minf", b"stbl", b"stsd"];

#[derive(Debug)]
pub(crate) enum Unremuxable {
    Io(io::Error),
    MediaBeforeTheMovie,
    NoFlacTrack,
    MovieTooLarge,
}

impl From<io::Error> for Unremuxable {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy)]
struct Header {
    kind: [u8; 4],
    body: Option<u64>,
}

fn header_of<R: Read>(inner: &mut R) -> io::Result<Option<Header>> {
    let mut sized = [0_u8; SIZED_HEADER as usize];
    let mut filled = 0;
    while filled < sized.len() {
        match inner.read(&mut sized[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(io::Error::from(ErrorKind::UnexpectedEof)),
            Ok(read) => filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    let size = u64::from(u32::from_be_bytes([sized[0], sized[1], sized[2], sized[3]]));
    let kind = [sized[4], sized[5], sized[6], sized[7]];
    let body = match size {
        0 => None,
        1 => {
            let mut large = [0_u8; 8];
            inner.read_exact(&mut large)?;
            Some(
                u64::from_be_bytes(large)
                    .checked_sub(LARGE_HEADER)
                    .ok_or_else(|| io::Error::from(ErrorKind::InvalidData))?,
            )
        }
        sized => Some(
            sized
                .checked_sub(SIZED_HEADER)
                .ok_or_else(|| io::Error::from(ErrorKind::InvalidData))?,
        ),
    };
    Ok(Some(Header { kind, body }))
}

fn skipped<R: Read>(inner: &mut R, body: Option<u64>) -> io::Result<()> {
    match body {
        Some(length) => {
            let passed = io::copy(&mut inner.take(length), &mut io::sink())?;
            if passed < length {
                return Err(io::Error::from(ErrorKind::UnexpectedEof));
            }
        }
        None => {
            io::copy(inner, &mut io::sink())?;
        }
    }
    Ok(())
}

fn boxes(within: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut rest = within;
    std::iter::from_fn(move || {
        if rest.len() < SIZED_HEADER as usize {
            return None;
        }
        let size = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let (header, size) = match size {
            0 => (SIZED_HEADER as usize, rest.len()),
            1 => {
                let large = rest.get(8..16)?;
                let size = usize::try_from(u64::from_be_bytes(large.try_into().ok()?)).ok()?;
                (LARGE_HEADER as usize, size)
            }
            size => (SIZED_HEADER as usize, size),
        };
        if size < header || size > rest.len() {
            return None;
        }
        let kind = &rest[4..8];
        let body = &rest[header..size];
        rest = &rest[size..];
        Some((kind, body))
    })
}

fn child<'a>(within: &'a [u8], kind: &[u8; 4]) -> impl Iterator<Item = &'a [u8]> {
    boxes(within).filter_map(move |(held, body)| (held == kind).then_some(body))
}

fn flac_blocks(movie: &[u8]) -> Option<&[u8]> {
    child(movie, MOVIE_PATH[0]).find_map(|track| {
        let mut within = track;
        for kind in &MOVIE_PATH[1..] {
            within = child(within, kind).next()?;
        }
        let entries = within.get(SAMPLE_DESCRIPTIONS_PREFIX..)?;
        let entry = child(entries, FLAC_MARKER).next()?;
        let described = entry.get(AUDIO_ENTRY_PREFIX..)?;
        child(described, b"dfLa").next()?.get(FULL_BOX_PREFIX..)
    })
}

fn stream_header(blocks: &[u8], timeline: Option<Timeline>) -> Option<Vec<u8>> {
    let mut header = FLAC_MARKER.to_vec();
    let mut rest = blocks;
    let mut last_at = None;
    let mut first = true;
    while rest.len() >= BLOCK_HEADER {
        let length = usize::from(rest[1]) << 16 | usize::from(rest[2]) << 8 | usize::from(rest[3]);
        let block = rest.get(..BLOCK_HEADER + length)?;
        let kind = block[0] & BLOCK_KIND;
        if first && (kind != STREAMINFO || length != STREAMINFO_LENGTH) {
            return None;
        }
        last_at = Some(header.len());
        header.extend_from_slice(block);
        if first {
            counted(&mut header[last_at? + BLOCK_HEADER..], timeline);
        }
        first = false;
        rest = &rest[block.len()..];
    }
    let last_at = last_at?;

    let mut at = FLAC_MARKER.len();
    while at < header.len() {
        let length = usize::from(header[at + 1]) << 16
            | usize::from(header[at + 2]) << 8
            | usize::from(header[at + 3]);
        if at == last_at {
            header[at] |= LAST_BLOCK;
        } else {
            header[at] &= BLOCK_KIND;
        }
        at += BLOCK_HEADER + length;
    }
    Some(header)
}

fn counted(streaminfo: &mut [u8], timeline: Option<Timeline>) {
    let Some(timeline) = timeline else {
        return;
    };
    let Some(packed) = streaminfo.get_mut(PACKED_AT..PACKED_AT + PACKED_LENGTH) else {
        return;
    };
    let Ok(bytes) = <[u8; PACKED_LENGTH]>::try_from(&*packed) else {
        return;
    };
    let word = u64::from_be_bytes(bytes);
    let mask = (1_u64 << TOTAL_BITS) - 1;
    let rate = word >> RATE_SHIFT;
    if word & mask != 0 || rate != timeline.timescale || timeline.ticks > mask {
        return;
    }
    packed.copy_from_slice(&(word | timeline.ticks).to_be_bytes());
}

pub(crate) struct Remuxed<R> {
    inner: R,
    pending: Vec<u8>,
    sent: usize,
    media_left: Option<Option<u64>>,
    ended: bool,
}

impl<R: Read> Remuxed<R> {
    pub(crate) fn opened(mut inner: R, timeline: Option<Timeline>) -> Result<Self, Unremuxable> {
        loop {
            let Some(header) = header_of(&mut inner)? else {
                return Err(Unremuxable::NoFlacTrack);
            };
            match &header.kind {
                b"mdat" => return Err(Unremuxable::MediaBeforeTheMovie),
                b"moov" => {
                    let length = header
                        .body
                        .filter(|length| *length <= LARGEST_MOVIE)
                        .ok_or(Unremuxable::MovieTooLarge)?;
                    let mut movie = Vec::with_capacity(usize::try_from(length).unwrap_or_default());
                    (&mut inner).take(length).read_to_end(&mut movie)?;
                    if (movie.len() as u64) < length {
                        return Err(Unremuxable::Io(io::Error::from(ErrorKind::UnexpectedEof)));
                    }
                    let pending = flac_blocks(&movie)
                        .and_then(|blocks| stream_header(blocks, timeline))
                        .ok_or(Unremuxable::NoFlacTrack)?;
                    return Ok(Self {
                        inner,
                        pending,
                        sent: 0,
                        media_left: None,
                        ended: false,
                    });
                }
                _ => skipped(&mut inner, header.body)?,
            }
        }
    }
}

impl<R: Read> Read for Remuxed<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.sent < self.pending.len() {
                let given = (self.pending.len() - self.sent).min(buf.len());
                buf[..given].copy_from_slice(&self.pending[self.sent..self.sent + given]);
                self.sent += given;
                if self.sent == self.pending.len() {
                    self.pending = Vec::new();
                    self.sent = 0;
                }
                return Ok(given);
            }
            if self.ended {
                return Ok(0);
            }
            match self.media_left {
                Some(Some(0)) => self.media_left = None,
                Some(Some(left)) => {
                    let asked = usize::try_from(left).map_or(buf.len(), |left| left.min(buf.len()));
                    let read = self.inner.read(&mut buf[..asked])?;
                    if read == 0 {
                        return Err(io::Error::from(ErrorKind::UnexpectedEof));
                    }
                    self.media_left = Some(Some(left - read as u64));
                    return Ok(read);
                }
                Some(None) => {
                    let read = self.inner.read(buf)?;
                    if read == 0 {
                        self.ended = true;
                    }
                    return Ok(read);
                }
                None => {
                    let Some(header) = header_of(&mut self.inner)? else {
                        self.ended = true;
                        return Ok(0);
                    };
                    match &header.kind {
                        b"mdat" => self.media_left = Some(header.body),
                        _ => skipped(&mut self.inner, header.body)?,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut written = u32::try_from(body.len() + 8)
            .expect("a small box")
            .to_be_bytes()
            .to_vec();
        written.extend_from_slice(kind);
        written.extend_from_slice(body);
        written
    }

    fn streaminfo(rate: u64, total: u64) -> Vec<u8> {
        let mut info = vec![0_u8; STREAMINFO_LENGTH];
        info[0..2].copy_from_slice(&4096_u16.to_be_bytes());
        info[2..4].copy_from_slice(&4096_u16.to_be_bytes());
        let word = rate << RATE_SHIFT | 1 << 41 | 15 << 36 | total;
        info[PACKED_AT..PACKED_AT + PACKED_LENGTH].copy_from_slice(&word.to_be_bytes());
        info
    }

    fn block(kind: u8, last: bool, data: &[u8]) -> Vec<u8> {
        let length = u32::try_from(data.len())
            .expect("a small block")
            .to_be_bytes();
        let mut written = vec![if last { kind | LAST_BLOCK } else { kind }];
        written.extend_from_slice(&length[1..]);
        written.extend_from_slice(data);
        written
    }

    fn movie(blocks: &[u8]) -> Vec<u8> {
        let mut described = vec![0_u8; FULL_BOX_PREFIX];
        described.extend_from_slice(blocks);
        let mut entry = vec![0_u8; AUDIO_ENTRY_PREFIX];
        entry.extend(boxed(b"dfLa", &described));
        let mut descriptions = vec![0, 0, 0, 0, 0, 0, 0, 1];
        descriptions.extend(boxed(FLAC_MARKER, &entry));
        let mut within = boxed(b"stsd", &descriptions);
        for kind in [b"stbl", b"minf", b"mdia", b"trak"] {
            within = boxed(kind, &within);
        }
        boxed(b"moov", &within)
    }

    fn fragmented(blocks: &[u8], frames: &[&[u8]]) -> Vec<u8> {
        let mut file = boxed(b"ftyp", b"iso6dash");
        file.extend(movie(blocks));
        for frame in frames {
            file.extend(boxed(b"styp", b"msdh"));
            file.extend(boxed(b"moof", &[0_u8; 24]));
            file.extend(boxed(b"mdat", frame));
        }
        file
    }

    fn remuxed(file: &[u8], timeline: Option<Timeline>) -> Vec<u8> {
        let mut remuxed = Remuxed::opened(file, timeline).expect("a FLAC track");
        let mut bytes = Vec::new();
        remuxed.read_to_end(&mut bytes).expect("the remux reads");
        bytes
    }

    #[test]
    fn a_fragmented_flac_track_becomes_a_native_stream_of_the_same_frames() {
        let info = streaminfo(44_100, 0);
        let blocks = block(STREAMINFO, true, &info);
        let file = fragmented(&blocks, &[b"\xff\xf8frame-one", b"\xff\xf8frame-two"]);

        let bytes = remuxed(
            &file,
            Some(Timeline {
                ticks: 12_345,
                timescale: 44_100,
            }),
        );

        let mut expected = FLAC_MARKER.to_vec();
        expected.extend(block(STREAMINFO, true, &streaminfo(44_100, 12_345)));
        expected.extend_from_slice(b"\xff\xf8frame-one\xff\xf8frame-two");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_count_the_track_declares_or_a_timeline_in_another_scale_is_left_as_it_stands() {
        let declared = block(STREAMINFO, true, &streaminfo(44_100, 777));
        let undeclared = block(STREAMINFO, true, &streaminfo(44_100, 0));
        let timeline = Some(Timeline {
            ticks: 12_345,
            timescale: 44_100,
        });
        let other_scale = Some(Timeline {
            ticks: 12_345,
            timescale: 1_000,
        });

        let kept = remuxed(&fragmented(&declared, &[]), timeline);
        let unscaled = remuxed(&fragmented(&undeclared, &[]), other_scale);

        assert_eq!(&kept[4..], &declared[..]);
        assert_eq!(&unscaled[4..], &undeclared[..]);
    }

    #[test]
    fn only_the_final_block_is_marked_last() {
        let mut blocks = block(STREAMINFO, true, &streaminfo(48_000, 0));
        blocks.extend(block(4, false, b"vorbis"));
        let file = fragmented(&blocks, &[]);

        let bytes = remuxed(&file, None);

        assert_eq!(bytes[4] & LAST_BLOCK, 0);
        assert_eq!(
            bytes[4 + BLOCK_HEADER + STREAMINFO_LENGTH] & LAST_BLOCK,
            LAST_BLOCK
        );
    }

    #[test]
    fn a_file_without_a_flac_track_or_with_its_media_first_is_refused() {
        let mut media_first = boxed(b"mdat", b"frames");
        media_first.extend(movie(&block(STREAMINFO, true, &streaminfo(44_100, 0))));
        let no_track = boxed(b"moov", &boxed(b"trak", b""));

        assert!(matches!(
            Remuxed::opened(&media_first[..], None),
            Err(Unremuxable::MediaBeforeTheMovie)
        ));
        assert!(matches!(
            Remuxed::opened(&no_track[..], None),
            Err(Unremuxable::NoFlacTrack)
        ));
    }
}
