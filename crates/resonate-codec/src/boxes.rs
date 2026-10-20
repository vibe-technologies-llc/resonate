use std::{
    fmt,
    io::{Read, Seek, SeekFrom},
    num::NonZeroU32,
};

use resonate_core::{FrameSpan, Frames, SampleRate};

use crate::{
    chapters::{ChapterStart, MOST_CHAPTERS, Marked, named},
    splices::{Edit, Splices},
};

const HEADER: u64 = 8;
const LARGE_HEADER: u64 = 16;
const LARGE_SIZE: u32 = 1;
const TO_END: u32 = 0;
const MAX_TOP_LEVEL_BOXES: usize = 256;
const MAX_CHILD_BOXES: usize = 256;
const MAX_LEAF_BYTES: u64 = 1 << 16;
const MAX_EDIT_LIST_BYTES: u64 = 1 << 20;
const EDIT_ROUNDING_TICKS: u64 = 1;
const MAX_INDEX_ROUNDS: usize = 64;
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
const EDIT_RATE_AT: usize = 8;
const WIDE_EDIT_RATE_AT: usize = 16;
const UNITY_EDIT_RATE: u32 = 0x0001_0000;
const DWELL_EDIT_RATE: u32 = 0;
const TIME_TO_SAMPLE_ENTRY_BYTES: usize = 8;
const FRAGMENT_DURATION_AT: usize = 4;
const INDEX_REFERENCE_AT: usize = 4;
const INDEX_TIMESCALE_AT: usize = 8;
const INDEX_COUNT_AT: usize = 22;
const WIDE_INDEX_COUNT_AT: usize = 30;
const INDEX_REFERENCE_BYTES: usize = 12;

const SOUND_HANDLER: [u8; 4] = *b"soun";
const GAPLESS_ITEM: &str = "iTunSMPB";
const SOUND_CHECK_ITEM: &str = crate::tags::ITUNES_SOUND_CHECK;

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
const TREF: BoxKind = BoxKind(*b"tref");
const CHAP: BoxKind = BoxKind(*b"chap");
const TKHD: BoxKind = BoxKind(*b"tkhd");
const CHPL: BoxKind = BoxKind(*b"chpl");
const STSZ: BoxKind = BoxKind(*b"stsz");
const STSC: BoxKind = BoxKind(*b"stsc");
const STCO: BoxKind = BoxKind(*b"stco");
const CO64: BoxKind = BoxKind(*b"co64");

const TRACK_ID_AT: usize = 12;
const WIDE_TRACK_ID_AT: usize = 20;
const SAMPLE_SIZE_AT: usize = 4;
const SAMPLE_COUNT_AT: usize = 8;
const SAMPLE_SIZES_AT: usize = 12;
const SAMPLE_TO_CHUNK_ENTRY_BYTES: usize = 12;
const NERO_TICKS_A_SECOND: NonZeroU32 = NonZeroU32::new(10_000_000).expect("not zero");
const NERO_RESERVED_BYTES: usize = 4;
const TEXT_LENGTH_BYTES: u64 = 2;
const LONGEST_CHAPTER_TITLE: u64 = 1 << 10;
const UTF16_BIG_ENDIAN_MARK: [u8; 2] = [0xFE, 0xFF];

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

pub(crate) fn read_layout<S: Read + Seek + ?Sized>(source: &mut S) -> Option<BoxLayout> {
    let origin = source.stream_position().ok()?;
    let found = layout_of(source);
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("an ISO-BMFF layout scan could not restore the stream position");
    }
    found
}

fn layout_of<S: Read + Seek + ?Sized>(source: &mut S) -> Option<BoxLayout> {
    let len = source.seek(SeekFrom::End(0)).ok()?;
    let head = read_at::<8, S>(source, 0)?;
    if head[4..] != FTYP.as_bytes() {
        return None;
    }

    walk(source, len).filter(starts_with_file_type)
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Movie {
    timescale: Option<NonZeroU32>,
    priming: Option<Priming>,
    edited: Option<Edited>,
    fragmented: Option<Ticks>,
    pub(crate) chapters: Vec<Marked>,
    pub(crate) sound_check: Option<String>,
}

impl Movie {
    pub(crate) fn priming_at(&self, rate: SampleRate) -> Option<Priming> {
        if self.priming.is_some() {
            return (self.timescale?.get() == rate.hz())
                .then(|| self.priming.clone())
                .flatten();
        }

        self.edited.as_ref()?.priming_at(rate)
    }

    pub(crate) fn fragmented_length(&self, rate: SampleRate) -> Option<Frames> {
        let held = self.fragmented?;
        rescaled(held.ticks, held.timescale, NonZeroU32::new(rate.hz())?).map(Frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticks {
    ticks: u64,
    timescale: NonZeroU32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Priming {
    pub(crate) delay: u32,
    pub(crate) padding: u32,
    playable: Option<Frames>,
    pub(crate) splices: Option<Splices>,
}

impl Priming {
    pub(crate) const fn new(delay: u32, padding: u32, playable: Option<Frames>) -> Self {
        Self {
            delay,
            padding,
            playable,
            splices: None,
        }
    }

    pub(crate) fn behind_a_decoder_delay(self, frames: u32) -> Self {
        Self {
            delay: self.delay.saturating_add(frames),
            padding: self.padding.saturating_sub(frames),
            ..self
        }
    }

    pub(crate) fn window(&self) -> Option<FrameSpan> {
        if self.delay == 0 && self.padding == 0 && self.splices.is_none() {
            return None;
        }

        let start = Frames(u64::from(self.delay));
        Some(match self.playable {
            Some(playable) => FrameSpan::between(start, start.saturating_add(playable)),
            None => FrameSpan::starting(start),
        })
    }

    fn fits_within(&self, decoded: Option<u64>) -> bool {
        let Some(playable) = self.playable.filter(|held| *held != Frames::ZERO) else {
            return false;
        };
        let Some(decoded) = decoded else {
            return true;
        };
        let reach = self.splices.as_ref().map_or(playable, Splices::reach);

        u64::from(self.delay)
            .checked_add(reach.get())
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

    let priming = free_form(source, moov, GAPLESS_ITEM)
        .as_deref()
        .and_then(gapless_fields)
        .filter(|held| held.fits_within(decoded));
    let edited = match priming {
        Some(_) => None,
        None => edit_list(source, moov, trak, media, decoded),
    };

    Some(Movie {
        timescale: Some(media.timescale),
        priming,
        edited,
        sound_check: free_form(source, moov, SOUND_CHECK_ITEM),
        fragmented: extended_length(source, moov)
            .or_else(|| indexed_length(source, &top, end, trak)),
        chapters: chapter_track(source, moov, trak)
            .filter(|marked| !marked.is_empty())
            .or_else(|| nero_chapters(source, moov))
            .unwrap_or_default(),
    })
}

fn nero_chapters<S: Read + Seek + ?Sized>(source: &mut S, moov: Extent) -> Option<Vec<Marked>> {
    let chpl = descend(source, moov, &[UDTA, CHPL])?;
    let bytes = leaf(source, chpl)?;
    let mut at = VERSION_AND_FLAGS as usize;
    if *bytes.first()? > 0 {
        at += NERO_RESERVED_BYTES;
    }
    let count = usize::from(*bytes.get(at)?);
    at += 1;

    let mut marked = Vec::with_capacity(count);
    for _ in 0..count {
        let ticks = be64(&bytes, at)?;
        let length = usize::from(*bytes.get(at + 8)?);
        let title = bytes.get(at + 9..at + 9 + length)?;
        at += 9 + length;
        marked.push(Marked {
            start: ChapterStart::new(ticks, NERO_TICKS_A_SECOND),
            title: chapter_title(title),
        });
    }
    Some(marked)
}

fn chapter_track<S: Read + Seek + ?Sized>(
    source: &mut S,
    moov: Extent,
    sound: Extent,
) -> Option<Vec<Marked>> {
    let named = descend(source, sound, &[TREF, CHAP]).and_then(|chap| leaf(source, chap))?;
    let wanted = be32(&named, 0)?;
    let traks = children(source, moov)
        .into_iter()
        .filter(|held| held.kind == TRAK)
        .map(|held| held.body)
        .collect::<Vec<_>>();
    let trak = traks
        .into_iter()
        .find(|trak| track_id(source, *trak) == Some(wanted))?;

    let mdia = child(source, trak, MDIA)?;
    let mdhd = child(source, mdia, MDHD)?;
    let media = header_of(source, mdhd)?;
    let stbl = descend(source, mdia, &[MINF, STBL])?;
    let stts = child(source, stbl, STTS)?;
    let lasting = sample_durations(source, stts)?;
    let stsz = child(source, stbl, STSZ)?;
    let sizes = sample_sizes(source, stsz)?;
    let offsets = sample_offsets(source, stbl, &sizes)?;

    let mut marked = Vec::new();
    let mut ticks = 0_u64;
    for (offset, lasted) in offsets.into_iter().zip(lasting).take(MOST_CHAPTERS) {
        marked.push(Marked {
            start: ChapterStart::new(ticks, media.timescale),
            title: sample_text(source, offset),
        });
        ticks = ticks.checked_add(lasted)?;
    }
    Some(marked)
}

fn track_id<S: Read + Seek + ?Sized>(source: &mut S, trak: Extent) -> Option<u32> {
    let tkhd = child(source, trak, TKHD)?;
    let bytes = leaf(source, tkhd)?;
    match *bytes.first()? {
        0 => be32(&bytes, TRACK_ID_AT),
        _ => be32(&bytes, WIDE_TRACK_ID_AT),
    }
}

fn sample_durations<S: Read + Seek + ?Sized>(source: &mut S, table: Extent) -> Option<Vec<u64>> {
    let bytes = leaf(source, table)?;
    let entries = (be32(&bytes, ENTRY_COUNT_AT)? as usize).min(MAX_TIME_TO_SAMPLE_ENTRIES);
    let mut lasting = Vec::new();
    for entry in 0..entries {
        let at = ENTRIES_AT + entry * TIME_TO_SAMPLE_ENTRY_BYTES;
        let count = be32(&bytes, at)? as usize;
        let delta = u64::from(be32(&bytes, at + 4)?);
        let room = MOST_CHAPTERS.saturating_sub(lasting.len());
        lasting.extend(std::iter::repeat_n(delta, count.min(room)));
    }
    Some(lasting)
}

fn sample_sizes<S: Read + Seek + ?Sized>(source: &mut S, table: Extent) -> Option<Vec<u64>> {
    let bytes = leaf(source, table)?;
    let fixed = be32(&bytes, SAMPLE_SIZE_AT)?;
    let count = (be32(&bytes, SAMPLE_COUNT_AT)? as usize).min(MOST_CHAPTERS);
    (0..count)
        .map(|sample| match fixed {
            0 => be32(&bytes, SAMPLE_SIZES_AT + sample * 4).map(u64::from),
            fixed => Some(u64::from(fixed)),
        })
        .collect()
}

fn sample_offsets<S: Read + Seek + ?Sized>(
    source: &mut S,
    stbl: Extent,
    sizes: &[u64],
) -> Option<Vec<u64>> {
    let chunks = chunk_offsets(source, stbl)?;
    let stsc = child(source, stbl, STSC)?;
    let runs = leaf(source, stsc)?;
    let entries = be32(&runs, ENTRY_COUNT_AT)? as usize;
    let run = |entry: usize| -> Option<(usize, usize)> {
        let at = ENTRIES_AT + entry.checked_mul(SAMPLE_TO_CHUNK_ENTRY_BYTES)?;
        Some((be32(&runs, at)? as usize, be32(&runs, at + 4)? as usize))
    };

    let mut offsets = Vec::with_capacity(sizes.len());
    let mut sizes = sizes.iter();
    let mut entry = 0;
    for (chunk, start) in chunks.into_iter().enumerate() {
        let number = chunk + 1;
        while entry + 1 < entries && run(entry + 1)?.0 <= number {
            entry += 1;
        }
        let mut at = start;
        for _ in 0..run(entry)?.1 {
            let Some(size) = sizes.next() else {
                return Some(offsets);
            };
            offsets.push(at);
            at = at.checked_add(*size)?;
        }
    }
    Some(offsets)
}

fn chunk_offsets<S: Read + Seek + ?Sized>(source: &mut S, stbl: Extent) -> Option<Vec<u64>> {
    let (table, wide) = match child(source, stbl, STCO) {
        Some(table) => (table, false),
        None => (child(source, stbl, CO64)?, true),
    };
    let bytes = leaf(source, table)?;
    let count = (be32(&bytes, ENTRY_COUNT_AT)? as usize).min(MOST_CHAPTERS);
    (0..count)
        .map(|chunk| match wide {
            false => be32(&bytes, ENTRIES_AT + chunk * 4).map(u64::from),
            true => be64(&bytes, ENTRIES_AT + chunk * 8),
        })
        .collect()
}

fn sample_text<S: Read + Seek + ?Sized>(source: &mut S, at: u64) -> Option<String> {
    let length = u16::from_be_bytes(read_at::<2, S>(source, at)?);
    let body = at.checked_add(TEXT_LENGTH_BYTES)?;
    let text = leaf(
        source,
        Extent {
            at: body,
            end: body.checked_add(u64::from(length).min(LONGEST_CHAPTER_TITLE))?,
        },
    )?;
    chapter_title(&text)
}

fn chapter_title(bytes: &[u8]) -> Option<String> {
    let text = match bytes.strip_prefix(&UTF16_BIG_ENDIAN_MARK) {
        Some(wide) => String::from_utf16_lossy(
            &wide
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair))
                .collect::<Vec<_>>(),
        ),
        None => String::from_utf8_lossy(bytes).into_owned(),
    };
    named(&text)
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

fn indexed_length<S: Read + Seek + ?Sized>(
    source: &mut S,
    top: &[Child],
    end: u64,
    sound: Extent,
) -> Option<Ticks> {
    let sound = track_id(source, sound);
    let mut indexes = index_boxes(source, top, end);
    let followed = sound
        .filter(|wanted| indexes.iter().any(|held| held.reference == *wanted))
        .or_else(|| indexes.first().map(|held| held.reference))?;
    indexes.retain(|held| held.reference == followed);

    let timescale = indexes.first()?.timescale;
    let mut ticks = 0_u64;
    for held in &indexes {
        ticks = ticks.checked_add(rescaled(held.ticks, held.timescale, timescale)?)?;
    }

    (ticks > 0).then_some(Ticks { ticks, timescale })
}

struct Index {
    reference: u32,
    timescale: NonZeroU32,
    ticks: u64,
}

fn index_boxes<S: Read + Seek + ?Sized>(source: &mut S, top: &[Child], end: u64) -> Vec<Index> {
    let mut seen = top.to_vec();
    if seen.iter().any(|held| held.kind == SIDX) {
        for _ in 0..MAX_INDEX_ROUNDS {
            let Some(last) = seen
                .last()
                .filter(|_| seen.len().is_multiple_of(MAX_CHILD_BOXES))
            else {
                break;
            };
            let more = children(
                source,
                Extent {
                    at: last.body.end,
                    end,
                },
            );
            if more.is_empty() {
                break;
            }
            seen.extend(more);
        }
    }

    seen.iter()
        .filter(|held| held.kind == SIDX)
        .filter_map(|held| index_of(source, held.body))
        .collect()
}

fn index_of<S: Read + Seek + ?Sized>(source: &mut S, sidx: Extent) -> Option<Index> {
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

    Some(Index {
        reference: be32(&bytes, INDEX_REFERENCE_AT)?,
        timescale,
        ticks,
    })
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

fn free_form<S: Read + Seek + ?Sized>(source: &mut S, moov: Extent, name: &str) -> Option<String> {
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
        if named.as_deref() != Some(name) {
            continue;
        }

        let data = child(source, item, ITEM_DATA)?;
        return text(source, data, ITEM_DATA_HEAD);
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

pub(crate) fn gapless_fields(value: &str) -> Option<Priming> {
    let mut fields = value.split_ascii_whitespace().skip(1);

    Some(Priming {
        delay: u32::from_str_radix(fields.next()?, 16).ok()?,
        padding: u32::from_str_radix(fields.next()?, 16).ok()?,
        playable: Some(Frames(u64::from_str_radix(fields.next()?, 16).ok()?)),
        splices: None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Listed {
    Media { from: u64, lasting: u64 },
    Empty { lasting: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Edited {
    timescale: NonZeroU32,
    listed: Vec<Listed>,
    whole: u64,
    decoded: Option<u64>,
}

impl Edited {
    fn priming_at(&self, rate: SampleRate) -> Option<Priming> {
        let to = NonZeroU32::new(rate.hz())?;
        let rescale = |ticks: u64| rescaled(ticks, self.timescale, to);

        let mut listed = Vec::with_capacity(self.listed.len());
        for edit in &self.listed {
            listed.push(match *edit {
                Listed::Media { from, lasting } => Listed::Media {
                    from: rescale(from)?,
                    lasting: rescale(lasting)?,
                },
                Listed::Empty { lasting } => Listed::Empty {
                    lasting: rescale(lasting)?,
                },
            });
        }
        let decoded = match self.decoded {
            Some(ticks) => Some(rescale(ticks)?),
            None => None,
        };
        let whole = match decoded {
            Some(frames) => frames,
            None => rescale(self.whole)?,
        };

        let &Listed::Media { from, lasting } = listed.first()? else {
            return None;
        };
        let first = Priming {
            delay: u32::try_from(from).ok()?,
            padding: frames_past(whole, from.saturating_add(lasting)),
            playable: Some(Frames(lasting)),
            splices: None,
        };
        let held = match listed.len() {
            1 => Some(first),
            _ => spliced_priming(&listed, whole)
                .filter(|spliced| spliced.fits_within(decoded))
                .or(Some(first)),
        };

        held.filter(|held| held.fits_within(decoded))
    }
}

fn edit_list<S: Read + Seek + ?Sized>(
    source: &mut S,
    moov: Extent,
    trak: Extent,
    media: Header,
    decoded: Option<u64>,
) -> Option<Edited> {
    let mvhd = child(source, moov, MVHD)?;
    let movie = header_of(source, mvhd)?;
    let elst = descend(source, trak, &[EDTS, ELST])?;
    let bytes = leaf_up_to(source, elst, MAX_EDIT_LIST_BYTES)?;
    let version = *bytes.first()?;
    let entry_bytes = match version {
        0 => EDIT_ENTRY_BYTES,
        _ => WIDE_EDIT_ENTRY_BYTES,
    };
    let entries = be32(&bytes, ENTRY_COUNT_AT)? as usize;
    if entries > (bytes.len().saturating_sub(ENTRIES_AT)) / entry_bytes {
        return None;
    }

    let mut listed = Vec::with_capacity(entries);
    for entry in 0..entries {
        let (segment, media_time, rate) = edit_entry(&bytes, version, entry)?;
        let lasting = rescaled(segment, movie.timescale, media.timescale)?;
        if lasting == 0 {
            continue;
        }

        let edit = match (u64::try_from(media_time), rate) {
            (Ok(from), UNITY_EDIT_RATE) => Listed::Media { from, lasting },
            (Ok(_), DWELL_EDIT_RATE) | (Err(_), _) => Listed::Empty { lasting },
            (Ok(_), _) => return None,
        };
        joined(&mut listed, edit);
    }
    while matches!(listed.last(), Some(Listed::Empty { .. })) {
        listed.pop();
    }

    Some(Edited {
        timescale: media.timescale,
        listed,
        whole: media.frames,
        decoded,
    })
}

fn joined(listed: &mut Vec<Listed>, edit: Listed) {
    let merged = match (listed.last_mut(), edit) {
        (None, Listed::Empty { .. }) => true,
        (
            Some(Listed::Media { from, lasting }),
            Listed::Media {
                from: next,
                lasting: more,
            },
        ) if from.saturating_add(*lasting).abs_diff(next) <= EDIT_ROUNDING_TICKS => {
            *lasting = lasting.saturating_add(more);
            true
        }
        (Some(Listed::Empty { lasting }), Listed::Empty { lasting: more }) => {
            *lasting = lasting.saturating_add(more);
            true
        }
        _ => false,
    };
    if !merged {
        listed.push(edit);
    }
}

fn spliced_priming(listed: &[Listed], whole: u64) -> Option<Priming> {
    let media = listed.iter().filter_map(|edit| match *edit {
        Listed::Media { from, lasting } => Some((from, from.saturating_add(lasting))),
        Listed::Empty { .. } => None,
    });
    let earliest = media.clone().map(|(from, _)| from).min()?;
    let reach = media.map(|(_, end)| end).max()?;

    let edits = listed
        .iter()
        .map(|edit| match *edit {
            Listed::Media { from, lasting } => Edit::Music {
                from: Frames(from - earliest),
                lasting: Frames(lasting),
            },
            Listed::Empty { lasting } => Edit::Silence {
                lasting: Frames(lasting),
            },
        })
        .collect();
    let splices = Splices::of(edits);

    Some(Priming {
        delay: u32::try_from(earliest).ok()?,
        padding: frames_past(whole, reach),
        playable: Some(splices.lasting()),
        splices: Some(splices),
    })
}

fn frames_past(whole: u64, reach: u64) -> u32 {
    u32::try_from(whole.saturating_sub(reach)).unwrap_or(u32::MAX)
}

fn edit_entry(bytes: &[u8], version: u8, entry: usize) -> Option<(u64, i64, u32)> {
    if version == 0 {
        let at = ENTRIES_AT + entry.checked_mul(EDIT_ENTRY_BYTES)?;
        return Some((
            u64::from(be32(bytes, at)?),
            i64::from(be32(bytes, at + 4)? as i32),
            be32(bytes, at + EDIT_RATE_AT)?,
        ));
    }

    let at = ENTRIES_AT + entry.checked_mul(WIDE_EDIT_ENTRY_BYTES)?;
    Some((
        be64(bytes, at)?,
        be64(bytes, at + 8)? as i64,
        be32(bytes, at + WIDE_EDIT_RATE_AT)?,
    ))
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
    leaf_up_to(source, held, MAX_LEAF_BYTES)
}

fn leaf_up_to<S: Read + Seek + ?Sized>(source: &mut S, held: Extent, most: u64) -> Option<Vec<u8>> {
    let bytes = held.end.checked_sub(held.at)?;
    if bytes > most {
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
        read_layout(&mut std::fs::File::open(&scratch.path).expect("a readable file"))
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
        let paced = entries
            .iter()
            .map(|&(segment, media_time)| (segment, media_time, UNITY_EDIT_RATE))
            .collect::<Vec<_>>();

        edit_list_paced(&paced)
    }

    fn edit_list_paced(entries: &[(u32, i32, u32)]) -> Vec<u8> {
        let mut payload = (entries.len() as u32).to_be_bytes().to_vec();
        for (segment, media_time, rate) in entries {
            payload.extend_from_slice(&segment.to_be_bytes());
            payload.extend_from_slice(&media_time.to_be_bytes());
            payload.extend_from_slice(&rate.to_be_bytes());
        }
        atom(b"edts", &full_atom(b"elst", 0, &payload))
    }

    fn wide_edit_list_paced(entries: &[(u64, i64, u32)]) -> Vec<u8> {
        let mut payload = (entries.len() as u32).to_be_bytes().to_vec();
        for (segment, media_time, rate) in entries {
            payload.extend_from_slice(&segment.to_be_bytes());
            payload.extend_from_slice(&media_time.to_be_bytes());
            payload.extend_from_slice(&rate.to_be_bytes());
        }
        atom(b"edts", &full_atom(b"elst", 1, &payload))
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

    fn track_header(id: u32) -> Vec<u8> {
        let mut payload = vec![0; 8];
        payload.extend_from_slice(&id.to_be_bytes());
        full_atom(b"tkhd", 0, &payload)
    }

    fn sound_track_numbered(id: u32) -> Vec<u8> {
        let stbl = atom(b"stbl", &time_to_sample(&[]));
        let mdia = atom(
            b"mdia",
            &[
                header_atom(b"mdhd", RATE, 0),
                handler(b"soun"),
                atom(b"minf", &stbl),
            ]
            .concat(),
        );

        atom(b"trak", &[track_header(id), mdia].concat())
    }

    fn track(handled: &[u8; 4], edits: &[Vec<u8>], samples: &[(u32, u32)]) -> Vec<u8> {
        track_scaled(RATE, handled, edits, samples)
    }

    fn track_scaled(
        timescale: u32,
        handled: &[u8; 4],
        edits: &[Vec<u8>],
        samples: &[(u32, u32)],
    ) -> Vec<u8> {
        let stbl = atom(b"stbl", &time_to_sample(samples));
        let minf = atom(b"minf", &stbl);
        let mdia = atom(
            b"mdia",
            &[
                header_atom(b"mdhd", timescale, PRIMING + MUSIC),
                handler(handled),
                minf,
            ]
            .concat(),
        );

        atom(b"trak", &[edits.concat(), mdia].concat())
    }

    fn gapless_item(value: &str) -> Vec<u8> {
        free_form_item(GAPLESS_ITEM, value)
    }

    fn free_form_item(name: &str, value: &str) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
        data.extend_from_slice(value.as_bytes());

        atom(
            b"----",
            &[
                full_atom(b"mean", 0, b"com.apple.iTunes"),
                full_atom(b"name", 0, name.as_bytes()),
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
    fn edits_that_continue_one_another_are_one_stretch_of_music() {
        let half = MUSIC / 2;
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[
                (half, PRIMING as i32),
                (MUSIC - half, (PRIMING + half) as i32),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.delay, PRIMING);
        assert_eq!(held.padding, REMAINDER);
        assert_eq!(
            held.window().and_then(FrameSpan::frames),
            Some(Frames(u64::from(MUSIC)))
        );
    }

    fn music(from: u32, lasting: u32) -> Edit {
        Edit::Music {
            from: Frames(u64::from(from)),
            lasting: Frames(u64::from(lasting)),
        }
    }

    fn silence(lasting: u32) -> Edit {
        Edit::Silence {
            lasting: Frames(u64::from(lasting)),
        }
    }

    #[test]
    fn edits_that_jump_in_media_time_are_spliced_in_the_order_they_are_listed() {
        let quarter = MUSIC / 4;
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[
                (quarter, PRIMING as i32),
                (quarter, (PRIMING + 3 * quarter) as i32),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.delay, PRIMING);
        assert_eq!(held.padding, REMAINDER);
        assert_eq!(
            held.window(),
            Some(FrameSpan::between(
                Frames(u64::from(PRIMING)),
                Frames(u64::from(PRIMING + 2 * quarter))
            ))
        );
        assert_eq!(
            held.splices.as_ref().map(Splices::edits),
            Some([music(0, quarter), music(3 * quarter, quarter)].as_slice())
        );
    }

    #[test]
    fn a_reordered_list_is_primed_by_the_earliest_music_any_edit_reads() {
        let half = MUSIC / 2;
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[
                (half, (PRIMING + half) as i32),
                (half, PRIMING as i32),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.delay, PRIMING);
        assert_eq!(
            held.window().and_then(FrameSpan::frames),
            Some(Frames(u64::from(MUSIC)))
        );
        assert_eq!(
            held.splices.as_ref().map(Splices::edits),
            Some([music(half, half), music(0, half)].as_slice())
        );
    }

    #[test]
    fn an_empty_edit_between_two_stretches_is_heard_as_silence() {
        let quarter = MUSIC / 4;
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[
                (quarter, PRIMING as i32),
                (512, -1),
                (quarter, (PRIMING + quarter) as i32),
                (256, -1),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(
            held.splices.as_ref().map(Splices::edits),
            Some([music(0, quarter), silence(512), music(quarter, quarter)].as_slice())
        );
        assert_eq!(
            held.window().and_then(FrameSpan::frames),
            Some(Frames(u64::from(2 * quarter + 512)))
        );
    }

    #[test]
    fn a_spliced_list_reaching_past_the_music_the_samples_hold_plays_its_first_stretch_alone() {
        let half = MUSIC / 2;
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[
                (half, PRIMING as i32),
                (half, (PRIMING + MUSIC) as i32),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.splices, None);
        assert_eq!(held.delay, PRIMING);
        assert_eq!(
            held.window().and_then(FrameSpan::frames),
            Some(Frames(u64::from(half)))
        );
    }

    #[test]
    fn a_single_edit_spliced_from_nothing_names_no_splices() {
        let held = priming_of(&movie(
            &[sound(&[edit_list(&[
                (512, -1),
                (MUSIC, PRIMING as i32),
                (256, -1),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(held.splices, None);
        assert_eq!(held.padding, REMAINDER);
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
    fn an_itunes_sound_check_item_is_read_beside_the_gapless_one() {
        const CHECKED: &str = " 000009C4 000009C4 00002710 00002710 00024CA8 00024CA8 00004000 \
                               00002000 00024CA8 00024CA8";
        let bytes = movie(
            &[sound(&[])],
            &[user_data(&[
                gapless_item(" 00000000 00000840 00000000 0000000000000000"),
                free_form_item(SOUND_CHECK_ITEM, CHECKED),
            ])],
        );

        assert_eq!(
            read_movie(&mut Cursor::new(bytes)).sound_check.as_deref(),
            Some(CHECKED.trim())
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
    fn a_declared_gapless_tag_is_declined_at_a_rate_the_media_timescale_disagrees_with() {
        let declared = format!(" 00000000 {:08X} {:08X} {:016X}", PRIMING, REMAINDER, MUSIC);
        let held = read_movie(&mut Cursor::new(movie(
            &[sound(&[])],
            &[user_data(&[gapless_item(&declared)])],
        )));

        assert!(held.priming_at(SampleRate::HZ_44100).is_some());
        assert_eq!(held.priming_at(SampleRate::HZ_48000), None);
    }

    #[test]
    fn an_edit_list_is_rescaled_to_the_rate_the_media_timescale_differs_from() {
        let core = RATE / 2;
        let bytes = movie(
            &[track_scaled(
                core,
                b"soun",
                &[edit_list(&[(MUSIC, (PRIMING / 2) as i32)])],
                &[(PACKETS, PACKET / 2)],
            )],
            &[],
        );
        let held = read_movie(&mut Cursor::new(bytes))
            .priming_at(SampleRate::HZ_44100)
            .expect("an edit list in the media's own timescale");

        assert_eq!(held.delay, PRIMING);
        assert_eq!(held.padding, REMAINDER);
        assert_eq!(
            held.window().and_then(FrameSpan::frames),
            Some(Frames(u64::from(MUSIC)))
        );
    }

    #[test]
    fn an_edit_that_dwells_is_heard_as_silence_the_segment_lasts() {
        let quarter = MUSIC / 4;
        let held = priming_of(&movie(
            &[sound(&[edit_list_paced(&[
                (quarter, PRIMING as i32, UNITY_EDIT_RATE),
                (512, (PRIMING + quarter) as i32, DWELL_EDIT_RATE),
                (quarter, (PRIMING + quarter) as i32, UNITY_EDIT_RATE),
            ])])],
            &[],
        ))
        .expect("an edit list the reader can act on");

        assert_eq!(
            held.splices.as_ref().map(Splices::edits),
            Some([music(0, quarter), silence(512), music(quarter, quarter)].as_slice())
        );
    }

    #[test]
    fn a_dwell_in_a_wide_edit_list_is_read_from_its_own_offset() {
        let quarter = u64::from(MUSIC / 4);
        let held = priming_of(&movie(
            &[sound(&[wide_edit_list_paced(&[
                (quarter, i64::from(PRIMING), UNITY_EDIT_RATE),
                (512, i64::from(PRIMING), DWELL_EDIT_RATE),
                (quarter, i64::from(PRIMING), UNITY_EDIT_RATE),
            ])])],
            &[],
        ))
        .expect("a wide edit list the reader can act on");

        assert_eq!(
            held.splices.as_ref().map(Splices::edits),
            Some([music(0, MUSIC / 4), silence(512), music(0, MUSIC / 4)].as_slice())
        );
    }

    #[test]
    fn an_edit_playing_at_another_speed_declines_the_whole_list() {
        let double = 0x0002_0000;
        let slow = 0x0000_8000;

        for rate in [double, slow, 0x8000_0000] {
            let held = priming_of(&movie(
                &[sound(&[edit_list_paced(&[
                    (MUSIC / 2, PRIMING as i32, UNITY_EDIT_RATE),
                    (MUSIC / 2, (PRIMING + MUSIC / 2) as i32, rate),
                ])])],
                &[],
            ));

            assert_eq!(held, None, "rate {rate:#x}");
        }
    }

    #[test]
    fn an_edit_list_longer_than_sixty_four_entries_is_read_whole() {
        let stretch = 16;
        let entries = (0..200)
            .map(|each| (stretch, (PRIMING + 2 * each * stretch) as i32))
            .collect::<Vec<_>>();
        let held = priming_of(&movie(&[sound(&[edit_list(&entries)])], &[]))
            .expect("an edit list the reader can act on");

        let edits = held
            .splices
            .as_ref()
            .map(Splices::edits)
            .unwrap_or_default();
        assert_eq!(edits.len(), 200);
        assert_eq!(edits.last().copied(), Some(music(398 * stretch, stretch)));
    }

    #[test]
    fn an_edit_list_declaring_more_entries_than_its_box_holds_is_declined() {
        let mut payload = u32::MAX.to_be_bytes().to_vec();
        payload.extend_from_slice(&[0; 12]);
        let hostile = atom(b"edts", &full_atom(b"elst", 0, &payload));

        assert_eq!(priming_of(&movie(&[sound(&[hostile])], &[])), None);
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
        index_of_track(1, timescale, durations)
    }

    fn index_of_track(reference: u32, timescale: u32, durations: &[u32]) -> Vec<u8> {
        let mut payload = reference.to_be_bytes().to_vec();
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
    fn every_segment_of_a_dash_file_adds_its_own_index() {
        let held = fragmented(
            &[],
            &[
                index(1_000, &[2_000]),
                atom(b"moof", &[0; 8]),
                atom(b"mdat", &[0; 8]),
                index(1_000, &[1_500]),
                atom(b"moof", &[0; 8]),
                atom(b"mdat", &[0; 8]),
                index(1_000, &[500, 1_000]),
            ],
        );

        assert_eq!(
            held.fragmented_length(SampleRate::HZ_48000),
            Some(Frames(240_000))
        );
    }

    #[test]
    fn an_index_of_another_track_is_not_added_to_the_sound_tracks() {
        let mut inside = header_atom(b"mvhd", RATE, 0);
        inside.extend_from_slice(&sound_track_numbered(2));
        inside.extend_from_slice(&sound_track_numbered(7));
        let held = read_movie(&mut Cursor::new(
            [
                atom(b"ftyp", b"iso8mp41dash"),
                atom(b"moov", &inside),
                index_of_track(7, 1_000, &[9_000]),
                index_of_track(2, 1_000, &[2_000]),
                index_of_track(2, 1_000, &[1_000]),
            ]
            .concat(),
        ));

        assert_eq!(
            held.fragmented_length(SampleRate::HZ_48000),
            Some(Frames(144_000))
        );
    }

    #[test]
    fn a_dash_file_with_more_segments_than_one_walk_reads_is_still_added_up() {
        let segments = MAX_CHILD_BOXES;
        let held = fragmented(
            &[],
            &(0..segments)
                .flat_map(|_| [index(1_000, &[1_000]), atom(b"mdat", &[0; 8])])
                .collect::<Vec<_>>(),
        );

        assert_eq!(
            held.fragmented_length(SampleRate::HZ_48000),
            Some(Frames(48_000 * segments as u64))
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
