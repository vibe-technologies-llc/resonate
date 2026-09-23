use std::{
    fmt,
    io::{Read, Seek, SeekFrom},
    num::NonZeroU32,
};

use resonate_core::{FrameSpan, Frames, MediaLocation, SampleRate};

use crate::{Error, Result, source::Sources};

const HEADER: u64 = 8;
const LARGE_HEADER: u64 = 16;
const LARGE_SIZE: u32 = 1;
const TO_END: u32 = 0;
const MAX_TOP_LEVEL_BOXES: usize = 256;
const MAX_CHILD_BOXES: usize = 256;
const MAX_LEAF_BYTES: u64 = 1 << 16;
const MAX_EDIT_ENTRIES: usize = 64;
const MAX_TIME_TO_SAMPLE_ENTRIES: usize = 8_191;

const VERSION_AND_FLAGS: u64 = 4;
const ITEM_DATA_HEAD: u64 = 8;
const TIMESCALE_AT: usize = 12;
const WIDE_TIMESCALE_AT: usize = 20;
const HANDLER_TYPE_AT: usize = 8;
const ENTRY_COUNT_AT: usize = 4;
const ENTRIES_AT: usize = 8;
const EDIT_ENTRY_BYTES: usize = 12;
const WIDE_EDIT_ENTRY_BYTES: usize = 20;
const TIME_TO_SAMPLE_ENTRY_BYTES: usize = 8;
const FRAGMENT_DURATION_AT: usize = 4;
const INDEX_TIMESCALE_AT: usize = 8;
const INDEX_COUNT_AT: usize = 22;
const WIDE_INDEX_COUNT_AT: usize = 30;
const INDEX_REFERENCE_BYTES: usize = 12;

const SOUND_HANDLER: [u8; 4] = *b"soun";
const GAPLESS_ITEM: &str = "iTunSMPB";

const FTYP: BoxKind = BoxKind(*b"ftyp");
const MOOV: BoxKind = BoxKind(*b"moov");
const MDAT: BoxKind = BoxKind(*b"mdat");
const MVHD: BoxKind = BoxKind(*b"mvhd");
const TRAK: BoxKind = BoxKind(*b"trak");
const MDIA: BoxKind = BoxKind(*b"mdia");
const MDHD: BoxKind = BoxKind(*b"mdhd");
const HDLR: BoxKind = BoxKind(*b"hdlr");
const MINF: BoxKind = BoxKind(*b"minf");
const STBL: BoxKind = BoxKind(*b"stbl");
const STTS: BoxKind = BoxKind(*b"stts");
const EDTS: BoxKind = BoxKind(*b"edts");
const ELST: BoxKind = BoxKind(*b"elst");
const MVEX: BoxKind = BoxKind(*b"mvex");
const MEHD: BoxKind = BoxKind(*b"mehd");
const SIDX: BoxKind = BoxKind(*b"sidx");
const UDTA: BoxKind = BoxKind(*b"udta");
const META: BoxKind = BoxKind(*b"meta");
const ILST: BoxKind = BoxKind(*b"ilst");
const FREE_FORM: BoxKind = BoxKind(*b"----");
const ITEM_NAME: BoxKind = BoxKind(*b"name");
const ITEM_DATA: BoxKind = BoxKind(*b"data");

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxKind([u8; 4]);

impl BoxKind {
    pub const fn as_bytes(self) -> [u8; 4] {
        self.0
    }

    fn is_printable(self) -> bool {
        self.0
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    }
}

impl fmt::Display for BoxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            match byte {
                byte if byte.is_ascii_graphic() || byte == b' ' => {
                    f.write_fmt(format_args!("{}", byte as char))?;
                }
                byte => f.write_fmt(format_args!("\\x{byte:02x}"))?,
            }
        }
        Ok(())
    }
}

impl fmt::Debug for BoxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoxKind({self})")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Faststart {
    Ready,
    Trailing,
}

impl fmt::Display for Faststart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ready => "moov before mdat",
            Self::Trailing => "mdat before moov",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopLevelBox {
    pub kind: BoxKind,
    pub at: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxLayout {
    pub boxes: Vec<TopLevelBox>,
    pub faststart: Option<Faststart>,
}

pub fn read(sources: &Sources, location: &MediaLocation) -> Result<Option<BoxLayout>> {
    let mut media = sources.open(location)?;
    let Some(len) = media.stream.byte_len() else {
        return Ok(None);
    };
    media.stream.rewind().map_err(|source| Error::Io {
        location: location.clone(),
        source,
    })?;

    Ok(walk(media.stream.as_mut(), len).filter(starts_with_file_type))
}

fn starts_with_file_type(layout: &BoxLayout) -> bool {
    layout.boxes.first().is_some_and(|first| first.kind == FTYP)
}

fn walk<S: Read + Seek + ?Sized>(file: &mut S, len: u64) -> Option<BoxLayout> {
    let mut boxes = Vec::new();
    let mut at = 0;

    while at + HEADER <= len && boxes.len() < MAX_TOP_LEVEL_BOXES {
        let header: [u8; 8] = read_at(file, at)?;
        let kind = BoxKind([header[4], header[5], header[6], header[7]]);
        if !kind.is_printable() {
            break;
        }

        let declared = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let bytes = match declared {
            TO_END => len.checked_sub(at)?,
            LARGE_SIZE => large_size(file, at)?,
            declared => u64::from(declared),
        };
        if bytes < HEADER || at.checked_add(bytes)? > len {
            break;
        }

        boxes.push(TopLevelBox { kind, at, bytes });
        at += bytes;
    }

    (!boxes.is_empty()).then(|| BoxLayout {
        faststart: faststart(&boxes),
        boxes,
    })
}

fn large_size<S: Read + Seek + ?Sized>(file: &mut S, at: u64) -> Option<u64> {
    let extended = read_at::<8, S>(file, at + HEADER)?;
    let bytes = u64::from_be_bytes(extended);
    (bytes >= LARGE_HEADER).then_some(bytes)
}

fn read_at<const N: usize, S: Read + Seek + ?Sized>(file: &mut S, at: u64) -> Option<[u8; N]> {
    let mut buffer = [0; N];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Movie {
    timescale: Option<NonZeroU32>,
    priming: Option<Priming>,
    fragmented: Option<Ticks>,
}

impl Movie {
    pub(crate) fn priming_at(self, rate: SampleRate) -> Option<Priming> {
        (self.timescale?.get() == rate.hz())
            .then_some(self.priming)
            .flatten()
    }

    pub(crate) fn fragmented_length(self, rate: SampleRate) -> Option<Frames> {
        let held = self.fragmented?;
        rescaled(held.ticks, held.timescale, NonZeroU32::new(rate.hz())?).map(Frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticks {
    ticks: u64,
    timescale: NonZeroU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Priming {
    pub(crate) delay: u32,
    pub(crate) padding: u32,
    playable: Option<Frames>,
}

impl Priming {
    pub(crate) const fn new(delay: u32, padding: u32, playable: Option<Frames>) -> Self {
        Self {
            delay,
            padding,
            playable,
        }
    }

    pub(crate) fn window(self) -> Option<FrameSpan> {
        if self.delay == 0 && self.padding == 0 {
            return None;
        }

        let start = Frames(u64::from(self.delay));
        Some(match self.playable {
            Some(playable) => FrameSpan::between(start, start.saturating_add(playable)),
            None => FrameSpan::starting(start),
        })
    }

    fn fits_within(self, decoded: Option<u64>) -> bool {
        let Some(playable) = self.playable.filter(|held| *held != Frames::ZERO) else {
            return false;
        };
        let Some(decoded) = decoded else {
            return true;
        };

        u64::from(self.delay)
            .checked_add(playable.get())
            .is_some_and(|held| held <= decoded)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Extent {
    at: u64,
    end: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Child {
    kind: BoxKind,
    body: Extent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Header {
    timescale: NonZeroU32,
    frames: u64,
}

pub(crate) fn read_movie<S: Read + Seek + ?Sized>(source: &mut S) -> Movie {
    let Ok(origin) = source.stream_position() else {
        return Movie::default();
    };
    let found = scan_movie(source).unwrap_or_default();
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("an ISO-BMFF gapless scan could not restore the stream position");
    }
    found
}

fn scan_movie<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Movie> {
    let end = source.seek(SeekFrom::End(0)).ok()?;
    let top = children(source, Extent { at: 0, end });
    if top.first()?.kind != FTYP {
        return None;
    }

    let moov = top.iter().find(|held| held.kind == MOOV)?.body;
    let trak = sound_track(source, moov)?;
    let mdia = child(source, trak, MDIA)?;
    let mdhd = child(source, mdia, MDHD)?;
    let media = header_of(source, mdhd)?;
    let decoded =
        descend(source, mdia, &[MINF, STBL, STTS]).and_then(|table| decoded_frames(source, table));

    Some(Movie {
        timescale: Some(media.timescale),
        priming: itunes_priming(source, moov)
            .or_else(|| edit_priming(source, moov, trak, media, decoded))
            .filter(|held| held.fits_within(decoded)),
        fragmented: extended_length(source, moov).or_else(|| indexed_length(source, &top)),
    })
}

fn extended_length<S: Read + Seek + ?Sized>(source: &mut S, moov: Extent) -> Option<Ticks> {
    let mvhd = child(source, moov, MVHD)?;
    let movie = header_of(source, mvhd)?;
    let mehd = descend(source, moov, &[MVEX, MEHD])?;
    let bytes = leaf(source, mehd)?;
    let ticks = match *bytes.first()? {
        0 => u64::from(be32(&bytes, FRAGMENT_DURATION_AT)?),
        _ => be64(&bytes, FRAGMENT_DURATION_AT)?,
    };

    (ticks > 0).then_some(Ticks {
        ticks,
        timescale: movie.timescale,
    })
}

fn indexed_length<S: Read + Seek + ?Sized>(source: &mut S, top: &[Child]) -> Option<Ticks> {
    let sidx = top.iter().find(|held| held.kind == SIDX)?.body;
    let bytes = leaf(source, sidx)?;
    let timescale = NonZeroU32::new(be32(&bytes, INDEX_TIMESCALE_AT)?)?;
    let count_at = match *bytes.first()? {
        0 => INDEX_COUNT_AT,
        _ => WIDE_INDEX_COUNT_AT,
    };
    let references = usize::from(u16::from_be_bytes(
        bytes.get(count_at..count_at + 2)?.try_into().ok()?,
    ));

    let mut ticks = 0_u64;
    for reference in 0..references {
        let at = count_at + 2 + reference * INDEX_REFERENCE_BYTES;
        ticks = ticks.checked_add(u64::from(be32(&bytes, at + 4)?))?;
    }

    (ticks > 0).then_some(Ticks { ticks, timescale })
}

fn sound_track<S: Read + Seek + ?Sized>(source: &mut S, moov: Extent) -> Option<Extent> {
    let traks = children(source, moov)
        .into_iter()
        .filter(|held| held.kind == TRAK)
        .map(|held| held.body)
        .collect::<Vec<_>>();

    traks.into_iter().find(|trak| carries_sound(source, *trak))
}

fn carries_sound<S: Read + Seek + ?Sized>(source: &mut S, trak: Extent) -> bool {
    descend(source, trak, &[MDIA, HDLR])
        .and_then(|hdlr| leaf(source, hdlr))
        .is_some_and(|bytes| {
            bytes.get(HANDLER_TYPE_AT..HANDLER_TYPE_AT + SOUND_HANDLER.len())
                == Some(SOUND_HANDLER.as_slice())
        })
}

fn itunes_priming<S: Read + Seek + ?Sized>(source: &mut S, moov: Extent) -> Option<Priming> {
    let meta = descend(source, moov, &[UDTA, META])?;
    let ilst = item_list(source, meta)?;
    let items = children(source, ilst)
        .into_iter()
        .filter(|held| held.kind == FREE_FORM)
        .map(|held| held.body)
        .collect::<Vec<_>>();

    for item in items {
        let named =
            child(source, item, ITEM_NAME).and_then(|name| text(source, name, VERSION_AND_FLAGS));
        if named.as_deref() != Some(GAPLESS_ITEM) {
            continue;
        }

        let data = child(source, item, ITEM_DATA)?;
        return gapless_fields(&text(source, data, ITEM_DATA_HEAD)?);
    }

    None
}

fn item_list<S: Read + Seek + ?Sized>(source: &mut S, meta: Extent) -> Option<Extent> {
    let past_flags = Extent {
        at: meta.at.checked_add(VERSION_AND_FLAGS)?,
        end: meta.end,
    };

    child(source, past_flags, ILST).or_else(|| child(source, meta, ILST))
}

fn gapless_fields(value: &str) -> Option<Priming> {
    let mut fields = value.split_ascii_whitespace().skip(1);

    Some(Priming {
        delay: u32::from_str_radix(fields.next()?, 16).ok()?,
        padding: u32::from_str_radix(fields.next()?, 16).ok()?,
        playable: Some(Frames(u64::from_str_radix(fields.next()?, 16).ok()?)),
    })
}

fn edit_priming<S: Read + Seek + ?Sized>(
    source: &mut S,
    moov: Extent,
    trak: Extent,
    media: Header,
    decoded: Option<u64>,
) -> Option<Priming> {
    let mvhd = child(source, moov, MVHD)?;
    let movie = header_of(source, mvhd)?;
    let elst = descend(source, trak, &[EDTS, ELST])?;
    let bytes = leaf(source, elst)?;
    let version = *bytes.first()?;
    let entries = (be32(&bytes, ENTRY_COUNT_AT)? as usize).min(MAX_EDIT_ENTRIES);

    for entry in 0..entries {
        let (segment, media_time) = edit_entry(&bytes, version, entry)?;
        let Ok(delay) = u32::try_from(media_time) else {
            continue;
        };

        let playable = rescaled(segment, movie.timescale, media.timescale)?;
        let whole = decoded.unwrap_or(media.frames);
        return Some(Priming {
            delay,
            padding: u32::try_from(
                whole
                    .saturating_sub(u64::from(delay))
                    .saturating_sub(playable),
            )
            .unwrap_or(u32::MAX),
            playable: Some(Frames(playable)),
        });
    }

    None
}

fn edit_entry(bytes: &[u8], version: u8, entry: usize) -> Option<(u64, i64)> {
    if version == 0 {
        let at = ENTRIES_AT + entry.checked_mul(EDIT_ENTRY_BYTES)?;
        return Some((
            u64::from(be32(bytes, at)?),
            i64::from(be32(bytes, at + 4)? as i32),
        ));
    }

    let at = ENTRIES_AT + entry.checked_mul(WIDE_EDIT_ENTRY_BYTES)?;
    Some((be64(bytes, at)?, be64(bytes, at + 8)? as i64))
}

fn rescaled(ticks: u64, from: NonZeroU32, to: NonZeroU32) -> Option<u64> {
    u64::try_from(u128::from(ticks) * u128::from(to.get()) / u128::from(from.get())).ok()
}

fn decoded_frames<S: Read + Seek + ?Sized>(source: &mut S, table: Extent) -> Option<u64> {
    let bytes = leaf(source, table)?;
    let entries = be32(&bytes, ENTRY_COUNT_AT)? as usize;
    if entries > MAX_TIME_TO_SAMPLE_ENTRIES {
        return None;
    }

    let mut samples = 0_u64;
    let mut longest = 0_u64;
    for entry in 0..entries {
        let at = ENTRIES_AT + entry * TIME_TO_SAMPLE_ENTRY_BYTES;
        samples = samples.checked_add(u64::from(be32(&bytes, at)?))?;
        longest = longest.max(u64::from(be32(&bytes, at + 4)?));
    }

    samples.checked_mul(longest)
}

fn header_of<S: Read + Seek + ?Sized>(source: &mut S, header: Extent) -> Option<Header> {
    let bytes = leaf(source, header)?;
    let (timescale, frames) = match *bytes.first()? {
        0 => (
            be32(&bytes, TIMESCALE_AT)?,
            u64::from(be32(&bytes, TIMESCALE_AT + 4)?),
        ),
        _ => (
            be32(&bytes, WIDE_TIMESCALE_AT)?,
            be64(&bytes, WIDE_TIMESCALE_AT + 4)?,
        ),
    };

    Some(Header {
        timescale: NonZeroU32::new(timescale)?,
        frames,
    })
}

fn descend<S: Read + Seek + ?Sized>(
    source: &mut S,
    within: Extent,
    path: &[BoxKind],
) -> Option<Extent> {
    let mut held = within;
    for kind in path {
        held = child(source, held, *kind)?;
    }
    Some(held)
}

fn child<S: Read + Seek + ?Sized>(
    source: &mut S,
    within: Extent,
    wanted: BoxKind,
) -> Option<Extent> {
    children(source, within)
        .into_iter()
        .find(|held| held.kind == wanted)
        .map(|held| held.body)
}

fn children<S: Read + Seek + ?Sized>(source: &mut S, within: Extent) -> Vec<Child> {
    let mut found = Vec::new();
    let mut at = within.at;

    while at.saturating_add(HEADER) <= within.end && found.len() < MAX_CHILD_BOXES {
        let Some(header) = read_at::<8, S>(source, at) else {
            break;
        };
        let kind = BoxKind([header[4], header[5], header[6], header[7]]);
        if !kind.is_printable() {
            break;
        }

        let declared = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let (head, bytes) = match declared {
            TO_END => (HEADER, within.end - at),
            LARGE_SIZE => match large_size(source, at) {
                Some(bytes) => (LARGE_HEADER, bytes),
                None => break,
            },
            declared => (HEADER, u64::from(declared)),
        };
        let Some(ends) = at.checked_add(bytes) else {
            break;
        };
        if bytes < head || ends > within.end {
            break;
        }

        found.push(Child {
            kind,
            body: Extent {
                at: at + head,
                end: ends,
            },
        });
        at = ends;
    }

    found
}

fn leaf<S: Read + Seek + ?Sized>(source: &mut S, held: Extent) -> Option<Vec<u8>> {
    let bytes = held.end.checked_sub(held.at)?;
    if bytes > MAX_LEAF_BYTES {
        return None;
    }

    let mut read = vec![0; bytes as usize];
    source.seek(SeekFrom::Start(held.at)).ok()?;
    source.read_exact(&mut read).ok()?;
    Some(read)
}

fn text<S: Read + Seek + ?Sized>(source: &mut S, held: Extent, past: u64) -> Option<String> {
    let bytes = leaf(
        source,
        Extent {
            at: held.at.checked_add(past)?,
            end: held.end,
        },
    )?;

    Some(String::from_utf8_lossy(&bytes).trim().to_owned())
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn be64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn faststart(boxes: &[TopLevelBox]) -> Option<Faststart> {
    let position = |wanted: BoxKind| boxes.iter().position(|entry| entry.kind == wanted);
    let moov = position(MOOV)?;
    let mdat = position(MDAT)?;

    Some(if moov < mdat {
        Faststart::Ready
    } else {
        Faststart::Trailing
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        io::Cursor,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    struct Scratch {
        path: std::path::PathBuf,
    }

    impl Scratch {
        fn holding(bytes: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = env::temp_dir().join(format!(
                "resonate-boxes-{}-{}.mp4",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).expect("a writable temporary file");
            Self { path }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn large_atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = LARGE_SIZE.to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(&((payload.len() + 16) as u64).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn file(atoms: &[Vec<u8>]) -> Scratch {
        Scratch::holding(&atoms.concat())
    }

    fn layout(scratch: &Scratch) -> Option<BoxLayout> {
        read(&Sources::local(), &MediaLocation::local(&scratch.path)).expect("a readable file")
    }

    #[test]
    fn moov_ahead_of_mdat_reads_as_ready_to_stream() {
        let scratch = file(&[
            atom(b"ftyp", b"M4A isom"),
            atom(b"moov", &[0; 32]),
            atom(b"mdat", &[0; 64]),
        ]);
        let found = layout(&scratch).expect("an ISO-BMFF layout");

        assert_eq!(found.faststart, Some(Faststart::Ready));
        assert_eq!(found.boxes.len(), 3);
        assert_eq!(found.boxes[1].kind, MOOV);
        assert_eq!(found.boxes[1].at, 16);
        assert_eq!(found.boxes[1].bytes, 40);
    }

    #[test]
    fn moov_behind_mdat_reads_as_trailing() {
        let scratch = file(&[
            atom(b"ftyp", b"M4A isom"),
            atom(b"mdat", &[0; 64]),
            atom(b"moov", &[0; 32]),
        ]);

        assert_eq!(
            layout(&scratch).expect("an ISO-BMFF layout").faststart,
            Some(Faststart::Trailing)
        );
    }

    #[test]
    fn a_sixty_four_bit_size_is_followed_to_the_next_box() {
        let scratch = file(&[
            atom(b"ftyp", b"M4A isom"),
            large_atom(b"mdat", &[0; 64]),
            atom(b"moov", &[0; 32]),
        ]);
        let found = layout(&scratch).expect("an ISO-BMFF layout");

        assert_eq!(found.boxes.len(), 3);
        assert_eq!(found.boxes[1].bytes, 80);
        assert_eq!(found.faststart, Some(Faststart::Trailing));
    }

    #[test]
    fn a_final_box_declaring_zero_runs_to_the_end_of_the_file() {
        let mut bytes = atom(b"ftyp", b"M4A isom");
        bytes.extend_from_slice(&atom(b"moov", &[0; 16]));
        bytes.extend_from_slice(&TO_END.to_be_bytes());
        bytes.extend_from_slice(b"mdat");
        bytes.extend_from_slice(&[0; 100]);
        let scratch = Scratch::holding(&bytes);

        let found = layout(&scratch).expect("an ISO-BMFF layout");
        assert_eq!(found.boxes.len(), 3);
        assert_eq!(found.boxes[2].bytes, 108);
        assert_eq!(found.faststart, Some(Faststart::Ready));
    }

    #[test]
    fn a_container_that_is_not_iso_bmff_has_no_layout() {
        let scratch = Scratch::holding(b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00");

        assert_eq!(layout(&scratch), None);
    }

    #[test]
    fn a_box_declaring_more_than_the_file_holds_ends_the_walk() {
        let mut bytes = atom(b"ftyp", b"M4A isom");
        bytes.extend_from_slice(&1_000_000_u32.to_be_bytes());
        bytes.extend_from_slice(b"moov");
        let scratch = Scratch::holding(&bytes);

        let found = layout(&scratch).expect("the file type box was readable");
        assert_eq!(found.boxes.len(), 1);
        assert_eq!(found.faststart, None);
    }

    #[test]
    fn an_iso_bmff_file_with_no_media_reports_no_faststart_verdict() {
        let scratch = file(&[atom(b"ftyp", b"M4A isom"), atom(b"free", &[0; 8])]);

        assert_eq!(layout(&scratch).expect("a layout").faststart, None);
    }

    const PRIMING: u32 = 1_024;
    const MUSIC: u32 = 88_200;
    const PACKET: u32 = 1_024;
    const PACKETS: u32 = 88;
    const REMAINDER: u32 = PACKETS * PACKET - PRIMING - MUSIC;

    fn full_atom(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
        let mut body = vec![version, 0, 0, 0];
        body.extend_from_slice(payload);
        atom(kind, &body)
    }

    fn header_atom(kind: &[u8; 4], timescale: u32, frames: u32) -> Vec<u8> {
        let mut payload = vec![0; 8];
        payload.extend_from_slice(&timescale.to_be_bytes());
        payload.extend_from_slice(&frames.to_be_bytes());
        full_atom(kind, 0, &payload)
    }

    fn edit_list(entries: &[(u32, i32)]) -> Vec<u8> {
        let mut payload = (entries.len() as u32).to_be_bytes().to_vec();
        for (segment, media_time) in entries {
            payload.extend_from_slice(&segment.to_be_bytes());
            payload.extend_from_slice(&media_time.to_be_bytes());
            payload.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
        }
        atom(b"edts", &full_atom(b"elst", 0, &payload))
    }

    fn time_to_sample(entries: &[(u32, u32)]) -> Vec<u8> {
        let mut payload = (entries.len() as u32).to_be_bytes().to_vec();
        for (samples, ticks) in entries {
            payload.extend_from_slice(&samples.to_be_bytes());
            payload.extend_from_slice(&ticks.to_be_bytes());
        }
        full_atom(b"stts", 0, &payload)
    }

    fn handler(kind: &[u8; 4]) -> Vec<u8> {
        let mut payload = vec![0; 4];
        payload.extend_from_slice(kind);
        full_atom(b"hdlr", 0, &payload)
    }

    fn track(handled: &[u8; 4], edits: &[Vec<u8>], samples: &[(u32, u32)]) -> Vec<u8> {
        let stbl = atom(b"stbl", &time_to_sample(samples));
        let minf = atom(b"minf", &stbl);
        let mdia = atom(
            b"mdia",
            &[
                header_atom(b"mdhd", RATE, PRIMING + MUSIC),
                handler(handled),
                minf,
            ]
            .concat(),
        );

        atom(b"trak", &[edits.concat(), mdia].concat())
    }

    fn gapless_item(value: &str) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
        data.extend_from_slice(value.as_bytes());

        atom(
            b"----",
            &[
                full_atom(b"mean", 0, b"com.apple.iTunes"),
                full_atom(b"name", 0, GAPLESS_ITEM.as_bytes()),
                atom(b"data", &data),
            ]
            .concat(),
        )
    }

    fn user_data(items: &[Vec<u8>]) -> Vec<u8> {
        let ilst = atom(b"ilst", &items.concat());
        atom(
            b"udta",
            &atom(b"meta", &[vec![0; 4], handler(b"mdir"), ilst].concat()),
        )
    }

    fn movie(traks: &[Vec<u8>], udta: &[Vec<u8>]) -> Vec<u8> {
        let mut inside = header_atom(b"mvhd", RATE, PRIMING + MUSIC);
        inside.extend_from_slice(&traks.concat());
        inside.extend_from_slice(&udta.concat());

        [
            atom(b"ftyp", b"M4A isom"),
            atom(b"mdat", &[0; 16]),
            atom(b"moov", &inside),
        ]
        .concat()
    }

    const RATE: u32 = 44_100;

    fn priming_of(bytes: &[u8]) -> Option<Priming> {
        read_movie(&mut Cursor::new(bytes.to_vec())).priming_at(SampleRate::HZ_44100)
    }

    fn sound(edits: &[Vec<u8>]) -> Vec<u8> {
        track(
            b"soun",
            edits,
            &[(PACKETS - 1, PACKET), (1, PACKET - REMAINDER)],
        )
    }

    #[test]
    fn an_edit_list_names_the_priming_and_the_music_that_follows_it() {
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[(MUSIC, PRIMING as i32)])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.delay, PRIMING);
        assert_eq!(held.padding, REMAINDER);
        assert_eq!(
            held.window(),
            Some(FrameSpan::between(
                Frames(u64::from(PRIMING)),
                Frames(u64::from(PRIMING + MUSIC))
            ))
        );
    }

    #[test]
    fn an_empty_edit_is_stepped_past_to_the_one_that_names_the_priming() {
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[(512, -1), (MUSIC, PRIMING as i32)])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.delay, PRIMING);
    }

    #[test]
    fn an_itunes_gapless_tag_is_read_in_place_of_the_edit_list() {
        let declared = format!(
            " 00000000 {:08X} {:08X} {:016X} 00000000 00000000",
            PRIMING + 1,
            REMAINDER + 1,
            MUSIC - 2
        );
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[(MUSIC, PRIMING as i32)])])],
            &[user_data(&[gapless_item(&declared)])],
        ))
        .expect("an iTunSMPB tag the reader can act on");

        assert_eq!(held.delay, PRIMING + 1);
        assert_eq!(held.padding, REMAINDER + 1);
        assert_eq!(
            held.window().and_then(FrameSpan::frames),
            Some(Frames(u64::from(MUSIC - 2)))
        );
    }

    #[test]
    fn the_sound_track_is_the_one_read_past_a_track_carrying_something_else() {
        let held = priming_of(&movie(
            &[
                track(b"vide", &[edit_list(&[(MUSIC, 4_096)])], &[(1, 1)]),
                sound(&[edit_list(&[(MUSIC, PRIMING as i32)])]),
            ],
            &[],
        ))
        .expect("the sound track's edit list");

        assert_eq!(held.delay, PRIMING);
    }

    #[test]
    fn a_file_declaring_neither_leaves_the_priming_unknown() {
        assert_eq!(priming_of(&movie(&[sound(&[])], &[])), None);
    }

    #[test]
    fn a_priming_longer_than_the_music_the_samples_hold_is_refused() {
        let declared = format!(" 00000000 {:08X} 00000000 {:016X}", u32::MAX, MUSIC);

        assert_eq!(
            priming_of(&movie(
                &[sound(&[])],
                &[user_data(&[gapless_item(&declared)])]
            )),
            None
        );
    }

    #[test]
    fn a_rate_the_media_timescale_disagrees_with_declines_the_whole_declaration() {
        let held = read_movie(&mut Cursor::new(movie(
            &[sound(&[edit_list(&[(MUSIC, PRIMING as i32)])])],
            &[],
        )));

        assert!(held.priming_at(SampleRate::HZ_44100).is_some());
        assert_eq!(held.priming_at(SampleRate::HZ_48000), None);
    }

    fn fragmented(extends: &[Vec<u8>], index: &[Vec<u8>]) -> Movie {
        let mut inside = header_atom(b"mvhd", RATE, 0);
        inside.extend_from_slice(&track(b"soun", &[], &[]));
        inside.extend_from_slice(&extends.concat());

        read_movie(&mut Cursor::new(
            [
                atom(b"ftyp", b"iso8mp41dash"),
                atom(b"moov", &inside),
                index.concat(),
                atom(b"moof", &[0; 8]),
                atom(b"mdat", &[0; 16]),
            ]
            .concat(),
        ))
    }

    fn index(timescale: u32, durations: &[u32]) -> Vec<u8> {
        let mut payload = 1_u32.to_be_bytes().to_vec();
        payload.extend_from_slice(&timescale.to_be_bytes());
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&(durations.len() as u16).to_be_bytes());
        for duration in durations {
            payload.extend_from_slice(&[0; 4]);
            payload.extend_from_slice(&duration.to_be_bytes());
            payload.extend_from_slice(&[0; 4]);
        }
        full_atom(b"sidx", 0, &payload)
    }

    #[test]
    fn a_fragmented_movie_is_as_long_as_its_extends_header_says() {
        let extends = atom(b"mvex", &full_atom(b"mehd", 0, &(MUSIC * 3).to_be_bytes()));

        assert_eq!(
            fragmented(&[extends], &[]).fragmented_length(SampleRate::HZ_44100),
            Some(Frames(u64::from(MUSIC) * 3))
        );
    }

    #[test]
    fn a_fragmented_movie_with_no_extends_header_is_as_long_as_its_index() {
        let held = fragmented(&[], &[index(1_000, &[2_000, 1_500])]);

        assert_eq!(
            held.fragmented_length(SampleRate::HZ_48000),
            Some(Frames(168_000))
        );
    }

    #[test]
    fn a_movie_that_is_not_fragmented_declares_no_fragmented_length() {
        let held = read_movie(&mut Cursor::new(movie(&[sound(&[])], &[])));

        assert_eq!(held.fragmented_length(SampleRate::HZ_44100), None);
    }

    #[test]
    fn a_container_that_is_not_iso_bmff_declares_no_priming() {
        assert_eq!(
            priming_of(b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00"),
            None
        );
    }
}
