use std::{
    io::{Read, Seek, SeekFrom},
    time::Duration,
};

use resonate_core::{Frames, SampleRate};

use crate::{flac, opus, prescan::read_exact};

const EBML_HEADER: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TITLE: u32 = 0x7BA9;
const SEGMENT_DURATION: u32 = 0x4489;
const TIMESTAMP_SCALE: u32 = 0x002A_D7B1;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0x00AE;
const TRACK_NUMBER: u32 = 0x00D7;
const CODEC_ID: u32 = 0x0086;
const TRACK_AUDIO: u32 = 0x00E1;
const BIT_DEPTH: u32 = 0x6264;
const CLUSTER: u32 = 0x1F43_B675;
const CLUSTER_TIMESTAMP: u32 = 0x00E7;
const SIMPLE_BLOCK: u32 = 0x00A3;
const BLOCK_GROUP: u32 = 0x00A0;
const BLOCK: u32 = 0x00A1;
const DISCARD_PADDING: u32 = 0x75A2;

const DEFAULT_TIMESTAMP_SCALE: u64 = 1_000_000;
const MAX_ID_BYTES: u32 = 4;
const MAX_LENGTH_BYTES: u32 = 8;
const MAX_ELEMENTS: usize = 4_096;
const MAX_TITLE_BYTES: u64 = 4_096;
const MAX_BIT_DEPTH: u64 = 64;
const MAX_CLUSTERS: usize = 65_536;
const MAX_BLOCKS: usize = 65_536;
const OPUS_CODEC_ID: &str = "A_OPUS";
const FLAC_CODEC_ID: &str = "A_FLAC";
const LACING: u8 = 0b0110;
const LACING_SHIFT: u8 = 1;
const XIPH_LACE_CONTINUES: u8 = 0xFF;
const LACED_FRAMES_AT_MOST: usize = 256;
const FRAME_HEAD_BYTES: usize = flac::FRAME_HEADER_BYTES_AT_MOST;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Segment {
    pub(crate) title: Option<String>,
    pub(crate) scale: Option<u64>,
    pub(crate) declared: Option<Duration>,
    pub(crate) counted: Option<Duration>,
    pub(crate) bit_depth: Option<u32>,
    pub(crate) discarded: Option<Duration>,
    pub(crate) opus_samples: Option<u64>,
    pub(crate) flac_samples: Option<u64>,
    pub(crate) counted_track: Option<u64>,
}

impl Segment {
    pub(crate) fn opus_music(&self, pre_skip: u32, padding: u32) -> Option<Frames> {
        self.opus_samples?
            .checked_sub(u64::from(pre_skip))?
            .checked_sub(u64::from(padding))
            .filter(|music| *music > 0)
            .map(Frames)
    }

    pub(crate) fn flac_frames_of(&self, track: u32) -> Option<Frames> {
        (self.counted_track == Some(u64::from(track)))
            .then_some(self.flac_samples)
            .flatten()
            .map(Frames)
    }

    pub(crate) fn discarded_frames(&self, rate: SampleRate) -> u32 {
        self.discarded
            .map(|held| Frames::from_duration(held, rate).get())
            .and_then(|frames| u32::try_from(frames).ok())
            .unwrap_or(0)
    }

    pub(crate) fn duration(&self) -> Option<Duration> {
        match (self.declared, self.counted) {
            (Some(declared), Some(counted)) if declared < counted => Some(counted),
            (declared, counted) => declared.or(counted),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Counted {
    ticks: Option<u64>,
    blocks: usize,
    discarded: Option<Duration>,
    tally: Tally,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Counts {
    Opus,
    Flac,
}

impl Counts {
    fn of(codec: &str) -> Option<Self> {
        match codec {
            OPUS_CODEC_ID => Some(Self::Opus),
            FLAC_CODEC_ID => Some(Self::Flac),
            _ => None,
        }
    }

    fn samples_in(self, head: &[u8]) -> Option<u64> {
        match self {
            Self::Opus => opus::samples_in_a_packet(head),
            Self::Flac => flac::samples_in_a_frame(head),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counting {
    track: u64,
    counts: Counts,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tally {
    #[default]
    NotCounting,
    Counting {
        of: Counting,
        samples: u64,
    },
    Lost,
}

impl Tally {
    fn of(counting: Option<Counting>) -> Self {
        counting.map_or(Self::NotCounting, |of| Self::Counting { of, samples: 0 })
    }

    fn counting(self) -> Option<Counting> {
        match self {
            Self::Counting { of, .. } => Some(of),
            Self::NotCounting | Self::Lost => None,
        }
    }

    fn add(&mut self, block: Option<Block>) {
        let Self::Counting { of, samples } = *self else {
            return;
        };
        let Some(block) = block else {
            *self = Self::Lost;
            return;
        };
        if block.track != of.track {
            return;
        }

        *self = block
            .samples
            .and_then(|more| samples.checked_add(more))
            .map_or(Self::Lost, |samples| Self::Counting { of, samples });
    }

    fn lose(&mut self) {
        if let Self::Counting { .. } = self {
            *self = Self::Lost;
        }
    }

    fn samples_of(self, counts: Counts) -> Option<u64> {
        match self {
            Self::Counting { of, samples } if of.counts == counts && samples > 0 => Some(samples),
            Self::Counting { .. } | Self::NotCounting | Self::Lost => None,
        }
    }
}

pub(crate) fn read_segment<S: Read + Seek + ?Sized>(source: &mut S) -> Segment {
    let Ok(origin) = source.stream_position() else {
        return Segment::default();
    };
    let found = scan(source).unwrap_or_default();
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("an EBML segment scan could not restore the stream position");
    }
    found
}

fn scan<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Segment> {
    let header = element(source)?;
    if header.id != EBML_HEADER {
        return None;
    }
    skip(source, &header)?;

    let segment = find(source, SEGMENT, None)?;
    let within = segment.end();
    let body = segment.body;

    let info = find(source, INFO, within)?;
    let mut found = read_info(source, info.end());

    if source.seek(SeekFrom::Start(body)).is_ok() {
        found.bit_depth = read_bit_depth(source, within);
    }
    let counted_track = source
        .seek(SeekFrom::Start(body))
        .ok()
        .and_then(|_| read_counted_track(source, within));
    if source.seek(SeekFrom::Start(body)).is_ok() {
        let counted = count_clusters(source, within, Tally::of(counted_track));
        let scale = found.scale.unwrap_or(DEFAULT_TIMESTAMP_SCALE);
        found.counted = span(counted.ticks.map(|ticks| ticks as f64), scale);
        found.discarded = counted.discarded;
        found.opus_samples = counted.tally.samples_of(Counts::Opus);
        found.flac_samples = counted.tally.samples_of(Counts::Flac);
        found.counted_track = counted_track.map(|counting| counting.track);
    }

    Some(found)
}

fn count_clusters<S: Read + Seek + ?Sized>(
    source: &mut S,
    within: Option<u64>,
    tally: Tally,
) -> Counted {
    let mut counted = Counted {
        tally,
        ..Counted::default()
    };
    let mut reached_the_end = false;

    for _ in 0..MAX_CLUSTERS {
        let Some(element) = element(source) else {
            break;
        };
        if element.id == CLUSTER {
            if element.length == Length::Unknown {
                break;
            }
            read_cluster(source, element.end(), &mut counted);
        }

        let Some(next) = skip(source, &element) else {
            break;
        };
        if within.is_some_and(|within| next >= within) {
            reached_the_end = within == Some(next);
            break;
        }
    }

    if !reached_the_end {
        counted.tally.lose();
    }
    counted
}

fn read_cluster<S: Read + Seek + ?Sized>(source: &mut S, limit: Option<u64>, into: &mut Counted) {
    let Ok(body) = source.stream_position() else {
        return;
    };
    let mut at = None;
    let mut offsets = Offsets::default();
    let mut reached_the_end = false;

    for _ in 0..MAX_BLOCKS {
        let Some(element) = element(source) else {
            break;
        };
        match element.id {
            CLUSTER_TIMESTAMP => at = uint(source, element.length),
            SIMPLE_BLOCK => {
                let block = read_block(source, element, into.tally.counting());
                offsets.saw(block.map(|block| block.offset));
                into.tally.add(block);
                into.discarded = None;
            }
            BLOCK_GROUP => {
                let group = read_group(source, element.end(), into.tally.counting());
                offsets.saw(group.block.map(|block| block.offset));
                into.tally.add(group.block);
                into.discarded = group.discarded;
            }
            _ => {}
        }

        let Some(next) = skip(source, &element) else {
            break;
        };
        if limit.is_some_and(|limit| next >= limit) {
            reached_the_end = limit == Some(next);
            break;
        }
    }

    if !reached_the_end {
        into.tally.lose();
    }

    if let Some(at) = at {
        let reached = at.saturating_add(offsets.reach());
        into.ticks = Some(into.ticks.unwrap_or(0).max(reached));
        into.blocks = into.blocks.saturating_add(offsets.blocks);
    }
    let _ = source.seek(SeekFrom::Start(body));
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Group {
    block: Option<Block>,
    discarded: Option<Duration>,
}

fn read_group<S: Read + Seek + ?Sized>(
    source: &mut S,
    limit: Option<u64>,
    counting: Option<Counting>,
) -> Group {
    let mut group = Group::default();

    for _ in 0..MAX_ELEMENTS {
        let Some(element) = element(source) else {
            break;
        };
        match element.id {
            BLOCK => group.block = read_block(source, element, counting),
            DISCARD_PADDING => {
                group.discarded = signed(source, element.length)
                    .and_then(|nanos| u64::try_from(nanos).ok())
                    .filter(|nanos| *nanos > 0)
                    .map(Duration::from_nanos);
            }
            _ => {}
        }

        let Some(next) = skip(source, &element) else {
            break;
        };
        if limit.is_none_or(|limit| next >= limit) {
            break;
        }
    }

    group
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Offsets {
    furthest: i16,
    blocks: usize,
}

impl Offsets {
    fn saw(&mut self, offset: Option<i16>) {
        let Some(offset) = offset else {
            return;
        };
        self.blocks = self.blocks.saturating_add(1);
        self.furthest = self.furthest.max(offset);
    }

    fn reach(self) -> u64 {
        u64::from(self.furthest.max(0).unsigned_abs())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    track: u64,
    offset: i16,
    samples: Option<u64>,
}

fn read_block<S: Read + Seek + ?Sized>(
    source: &mut S,
    element: Element,
    counting: Option<Counting>,
) -> Option<Block> {
    let end = element.end()?;
    let Length::Known(track) = length(source)? else {
        return None;
    };
    let [high, low, flags] = read_exact::<3, S>(source)?;
    let samples = counting
        .filter(|counting| counting.track == track)
        .and_then(|counting| samples_in(source, end, Lacing::of(flags), counting.counts));

    Some(Block {
        track,
        offset: i16::from_be_bytes([high, low]),
        samples,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lacing {
    Unlaced,
    Xiph,
    Fixed,
    Ebml,
}

impl Lacing {
    fn of(flags: u8) -> Self {
        match (flags & LACING) >> LACING_SHIFT {
            0 => Self::Unlaced,
            1 => Self::Xiph,
            2 => Self::Fixed,
            _ => Self::Ebml,
        }
    }
}

fn samples_in<S: Read + Seek + ?Sized>(
    source: &mut S,
    end: u64,
    lacing: Lacing,
    counts: Counts,
) -> Option<u64> {
    let mut sizes = [0_u64; LACED_FRAMES_AT_MOST];
    let sizes = lace_sizes(source, end, lacing, &mut sizes)?;
    let mut at = source.stream_position().ok()?;
    let mut samples = 0_u64;

    for size in sizes {
        source.seek(SeekFrom::Start(at)).ok()?;
        samples = samples.checked_add(samples_of_a_frame(source, *size, counts)?)?;
        at = at.checked_add(*size)?;
    }
    Some(samples)
}

fn lace_sizes<'a, S: Read + Seek + ?Sized>(
    source: &mut S,
    end: u64,
    lacing: Lacing,
    sizes: &'a mut [u64; LACED_FRAMES_AT_MOST],
) -> Option<&'a [u64]> {
    let frames = match lacing {
        Lacing::Unlaced => 1,
        Lacing::Xiph | Lacing::Fixed | Lacing::Ebml => {
            let [more] = read_exact::<1, S>(source)?;
            usize::from(more) + 1
        }
    };
    let sizes = sizes.get_mut(..frames)?;
    let (last, declared) = sizes.split_last_mut()?;

    match lacing {
        Lacing::Unlaced | Lacing::Fixed => {}
        Lacing::Xiph => {
            for size in declared.iter_mut() {
                *size = xiph_lace(source)?;
            }
        }
        Lacing::Ebml => {
            let mut previous = None;
            for size in declared.iter_mut() {
                *size = ebml_lace(source, previous)?;
                previous = Some(*size);
            }
        }
    }

    let data = end.checked_sub(source.stream_position().ok()?)?;
    if lacing == Lacing::Fixed {
        let frames = u64::try_from(frames).ok()?;
        let each = data / frames;
        (each * frames == data).then_some(())?;
        declared.fill(each);
    }
    let held = declared
        .iter()
        .try_fold(0_u64, |held, size| held.checked_add(*size))?;
    *last = data.checked_sub(held)?;

    Some(sizes)
}

fn xiph_lace<S: Read + ?Sized>(source: &mut S) -> Option<u64> {
    let mut size = 0_u64;
    for _ in 0..MAX_ELEMENTS {
        let [byte] = read_exact::<1, S>(source)?;
        size = size.checked_add(u64::from(byte))?;
        if byte != XIPH_LACE_CONTINUES {
            return Some(size);
        }
    }
    None
}

fn ebml_lace<S: Read + ?Sized>(source: &mut S, previous: Option<u64>) -> Option<u64> {
    let (value, width) = vint(source)?;
    let Some(previous) = previous else {
        return Some(value);
    };
    let bias = (1_i64 << (7 * width - 1)) - 1;
    let difference = i64::try_from(value).ok()? - bias;
    previous.checked_add_signed(difference)
}

fn samples_of_a_frame<S: Read + ?Sized>(source: &mut S, size: u64, counts: Counts) -> Option<u64> {
    let mut head = [0_u8; FRAME_HEAD_BYTES];
    let head_bytes = usize::try_from(size)
        .unwrap_or(FRAME_HEAD_BYTES)
        .min(FRAME_HEAD_BYTES);
    let head = head.get_mut(..head_bytes)?;
    source.read_exact(head).ok()?;
    counts.samples_in(head)
}

fn read_counted_track<S: Read + Seek + ?Sized>(
    source: &mut S,
    within: Option<u64>,
) -> Option<Counting> {
    let tracks = find(source, TRACKS, within)?;
    let limit = tracks.end();

    for _ in 0..MAX_ELEMENTS {
        let entry = element(source)?;
        if entry.id == TRACK_ENTRY
            && let Some(track) = counted_track(source, entry.end())
        {
            return Some(track);
        }

        let next = skip(source, &entry)?;
        if limit.is_some_and(|limit| next >= limit) {
            break;
        }
    }
    None
}

fn counted_track<S: Read + Seek + ?Sized>(source: &mut S, limit: Option<u64>) -> Option<Counting> {
    let mut number = None;
    let mut counts = None;

    for _ in 0..MAX_ELEMENTS {
        let Some(element) = element(source) else {
            break;
        };
        match element.id {
            TRACK_NUMBER => number = positive(uint(source, element.length)),
            CODEC_ID => counts = text(source, element.length).as_deref().and_then(Counts::of),
            _ => {}
        }

        let Some(next) = skip(source, &element) else {
            break;
        };
        if limit.is_none_or(|limit| next >= limit) {
            break;
        }
    }

    Some(Counting {
        track: number?,
        counts: counts?,
    })
}

fn read_bit_depth<S: Read + Seek + ?Sized>(source: &mut S, within: Option<u64>) -> Option<u32> {
    let tracks = find(source, TRACKS, within)?;
    let entry = find(source, TRACK_ENTRY, tracks.end())?;
    let audio = find(source, TRACK_AUDIO, entry.end())?;

    let limit = audio.end();
    for _ in 0..MAX_ELEMENTS {
        let element = element(source)?;
        if element.id == BIT_DEPTH {
            let bits = positive(uint(source, element.length))?;
            return (bits <= MAX_BIT_DEPTH).then_some(bits as u32);
        }

        let next = skip(source, &element)?;
        if limit.is_some_and(|limit| next >= limit) {
            break;
        }
    }
    None
}

fn read_info<S: Read + Seek + ?Sized>(source: &mut S, limit: Option<u64>) -> Segment {
    let mut title = None;
    let mut ticks = None;
    let mut scale = DEFAULT_TIMESTAMP_SCALE;

    for _ in 0..MAX_ELEMENTS {
        let Some(element) = element(source) else {
            break;
        };
        match element.id {
            TITLE => title = text(source, element.length),
            SEGMENT_DURATION => ticks = float(source, element.length),
            TIMESTAMP_SCALE => scale = positive(uint(source, element.length)).unwrap_or(scale),
            _ => {}
        }

        let Some(next) = skip(source, &element) else {
            break;
        };
        if limit.is_some_and(|limit| next >= limit) {
            break;
        }
    }

    Segment {
        title,
        scale: Some(scale),
        declared: span(ticks, scale),
        counted: None,
        bit_depth: None,
        discarded: None,
        opus_samples: None,
        flac_samples: None,
        counted_track: None,
    }
}

fn span(ticks: Option<f64>, scale: u64) -> Option<Duration> {
    let nanos = ticks? * scale as f64;
    (nanos.is_finite() && nanos > 0.0 && nanos <= u64::MAX as f64)
        .then(|| Duration::from_nanos(nanos as u64))
}

fn float<S: Read + ?Sized>(source: &mut S, length: Length) -> Option<f64> {
    match length {
        Length::Known(4) => read_exact::<4, S>(source).map(|be| f64::from(f32::from_be_bytes(be))),
        Length::Known(8) => read_exact::<8, S>(source).map(f64::from_be_bytes),
        Length::Known(_) | Length::Unknown => None,
    }
}

fn uint<S: Read + ?Sized>(source: &mut S, length: Length) -> Option<u64> {
    let Length::Known(bytes) = length else {
        return None;
    };
    if bytes == 0 || bytes > u64::from(MAX_LENGTH_BYTES) {
        return None;
    }

    let mut value = 0_u64;
    for _ in 0..bytes {
        let [byte] = read_exact::<1, S>(source)?;
        value = (value << 8) | u64::from(byte);
    }
    Some(value)
}

fn signed<S: Read + ?Sized>(source: &mut S, length: Length) -> Option<i64> {
    let Length::Known(bytes) = length else {
        return None;
    };
    let unused = u64::from(MAX_LENGTH_BYTES).checked_sub(bytes)? * 8;
    let value = uint(source, length)?;
    Some(((value << unused) as i64) >> unused)
}

fn positive(value: Option<u64>) -> Option<u64> {
    value.filter(|value| *value > 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Length {
    Known(u64),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Element {
    id: u32,
    length: Length,
    body: u64,
}

impl Element {
    fn end(self) -> Option<u64> {
        match self.length {
            Length::Known(bytes) => self.body.checked_add(bytes),
            Length::Unknown => None,
        }
    }
}

fn find<S: Read + Seek + ?Sized>(source: &mut S, id: u32, limit: Option<u64>) -> Option<Element> {
    for _ in 0..MAX_ELEMENTS {
        let element = element(source)?;
        if element.id == id {
            return Some(element);
        }

        let next = skip(source, &element)?;
        if limit.is_some_and(|limit| next >= limit) {
            return None;
        }
    }
    None
}

fn skip<S: Seek + ?Sized>(source: &mut S, element: &Element) -> Option<u64> {
    let end = element.end()?;
    source.seek(SeekFrom::Start(end)).ok()
}

fn element<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Element> {
    let id = id(source)?;
    let length = length(source)?;
    let body = source.stream_position().ok()?;

    Some(Element { id, length, body })
}

fn id<S: Read + ?Sized>(source: &mut S) -> Option<u32> {
    let [lead] = read_exact::<1, S>(source)?;
    let width = width(lead, MAX_ID_BYTES)?;

    let mut id = u32::from(lead);
    for _ in 1..width {
        let [byte] = read_exact::<1, S>(source)?;
        id = (id << 8) | u32::from(byte);
    }
    Some(id)
}

fn length<S: Read + ?Sized>(source: &mut S) -> Option<Length> {
    let (length, width) = vint(source)?;
    let unbounded = (1_u64 << (7 * u64::from(width))) - 1;
    Some(if length == unbounded {
        Length::Unknown
    } else {
        Length::Known(length)
    })
}

fn vint<S: Read + ?Sized>(source: &mut S) -> Option<(u64, u32)> {
    let [lead] = read_exact::<1, S>(source)?;
    let width = width(lead, MAX_LENGTH_BYTES)?;
    let marker = 0x80_u8 >> (width - 1);

    let mut value = u64::from(lead & (marker - 1));
    for _ in 1..width {
        let [byte] = read_exact::<1, S>(source)?;
        value = (value << 8) | u64::from(byte);
    }
    Some((value, width))
}

fn width(lead: u8, max: u32) -> Option<u32> {
    let width = lead.leading_zeros() + 1;
    (width <= max).then_some(width)
}

fn text<S: Read + ?Sized>(source: &mut S, length: Length) -> Option<String> {
    let Length::Known(bytes) = length else {
        return None;
    };

    let mut value = vec![0_u8; usize::try_from(bytes.min(MAX_TITLE_BYTES)).ok()?];
    source.read_exact(&mut value).ok()?;

    let end = value
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(value.len());
    let title = String::from_utf8_lossy(value.get(..end)?).trim().to_owned();

    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const SEEK_HEAD: u32 = 0x114D_9B74;
    const CLUSTER: u32 = 0x1F43_B675;
    const MUXING_APP: u32 = 0x4D80;
    const NANOSECONDS_PER_TICK: u32 = 1_000_000;
    const UNBOUNDED: u8 = 0xFF;

    fn vint(value: u64, width: u32) -> Vec<u8> {
        let marked = value | (1 << (7 * width));
        marked.to_be_bytes()[8 - width as usize..].to_vec()
    }

    fn element(id: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = id.to_be_bytes().to_vec();
        while bytes.first() == Some(&0) {
            bytes.remove(0);
        }
        bytes.extend_from_slice(&vint(body.len() as u64, 8));
        bytes.extend_from_slice(body);
        bytes
    }

    fn file(title: &str) -> Vec<u8> {
        let mut segment = element(SEEK_HEAD, b"a seek head");
        segment.extend_from_slice(&element(INFO, &titled(title)));
        segment.extend_from_slice(&element(CLUSTER, b"a cluster"));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &segment));
        bytes
    }

    fn titled(title: &str) -> Vec<u8> {
        let mut info = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        info.extend_from_slice(&element(SEGMENT_DURATION, &2_003.0_f64.to_be_bytes()));
        info.extend_from_slice(&element(TITLE, title.as_bytes()));
        info.extend_from_slice(&element(MUXING_APP, b"Lavf63.1.101"));
        info
    }

    #[test]
    fn the_segment_duration_is_its_ticks_scaled_by_the_timestamp_scale() {
        assert_eq!(
            read_segment(&mut Cursor::new(file("Echoes"))).duration(),
            Some(Duration::from_millis(2_003))
        );
    }

    #[test]
    fn a_duration_the_segment_does_not_declare_is_left_unknown() {
        let mut info = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        info.extend_from_slice(&element(TITLE, b"Echoes"));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &element(INFO, &info)));

        assert_eq!(read_segment(&mut Cursor::new(bytes)).duration(), None);
    }

    #[test]
    fn a_timestamp_scale_of_its_own_is_what_the_ticks_are_counted_in() {
        let mut info = element(TIMESTAMP_SCALE, &100_u32.to_be_bytes());
        info.extend_from_slice(&element(SEGMENT_DURATION, &10_000_000.0_f64.to_be_bytes()));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &element(INFO, &info)));

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).duration(),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn the_segment_title_is_found_past_the_elements_before_it() {
        assert_eq!(
            read_segment(&mut Cursor::new(file("Echoes")))
                .title
                .as_deref(),
            Some("Echoes")
        );
    }

    #[test]
    fn the_scan_leaves_the_stream_where_it_found_it() {
        let mut source = Cursor::new(file("Echoes"));

        let _ = read_segment(&mut source);

        assert_eq!(source.position(), 0);
    }

    #[test]
    fn a_segment_of_unbounded_length_is_still_walked() {
        let mut segment = SEGMENT.to_be_bytes().to_vec();
        segment.push(UNBOUNDED);
        segment.extend_from_slice(&element(INFO, &titled("Echoes")));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&segment);

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).title.as_deref(),
            Some("Echoes")
        );
    }

    #[test]
    fn an_untitled_segment_yields_nothing() {
        let mut segment = element(
            INFO,
            &element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes()),
        );
        segment.extend_from_slice(&element(CLUSTER, b"a cluster"));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &segment));

        assert!(read_segment(&mut Cursor::new(bytes)).title.is_none());
        assert!(read_segment(&mut Cursor::new(file("   "))).title.is_none());
    }

    fn simple_block(offset: i16) -> Vec<u8> {
        let mut block = vec![0x81];
        block.extend_from_slice(&offset.to_be_bytes());
        block.push(0);
        element(SIMPLE_BLOCK, &block)
    }

    fn cluster(at: u64, offsets: &[i16]) -> Vec<u8> {
        let mut body = element(CLUSTER_TIMESTAMP, &at.to_be_bytes());
        for offset in offsets {
            body.extend_from_slice(&simple_block(*offset));
        }
        element(CLUSTER, &body)
    }

    fn clustered(info: &[u8], clusters: &[Vec<u8>]) -> Vec<u8> {
        let mut segment = element(INFO, info);
        for cluster in clusters {
            segment.extend_from_slice(cluster);
        }

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &segment));
        bytes
    }

    fn grouped(offset: i16, discarded: &[u8]) -> Vec<u8> {
        let mut block = vec![0x81];
        block.extend_from_slice(&offset.to_be_bytes());
        block.push(0);
        let mut group = element(BLOCK, &block);
        group.extend_from_slice(&element(DISCARD_PADDING, discarded));
        element(BLOCK_GROUP, &group)
    }

    #[test]
    fn the_padding_the_last_block_discards_is_what_the_segment_discards() {
        let scale = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        let mut last = element(CLUSTER_TIMESTAMP, &1_000_u64.to_be_bytes());
        last.extend_from_slice(&simple_block(0));
        last.extend_from_slice(&grouped(20, &[0x35, 0x67, 0xE0]));
        let bytes = clustered(&scale, &[cluster(0, &[0, 20]), element(CLUSTER, &last)]);

        let found = read_segment(&mut Cursor::new(bytes));

        assert_eq!(found.discarded, Some(Duration::from_micros(3_500)));
        assert_eq!(
            found.discarded_frames(SampleRate::new(48_000).expect("a rate")),
            168
        );
        assert_eq!(found.counted, Some(Duration::from_millis(1_020)));
    }

    #[test]
    fn a_padding_an_earlier_block_or_a_negative_one_discards_is_no_padding_at_the_end() {
        let scale = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        let mut earlier = element(CLUSTER_TIMESTAMP, &0_u64.to_be_bytes());
        earlier.extend_from_slice(&grouped(0, &[0x35, 0x67, 0xE0]));
        earlier.extend_from_slice(&simple_block(20));
        let bytes = clustered(&scale, &[element(CLUSTER, &earlier)]);
        assert_eq!(read_segment(&mut Cursor::new(bytes)).discarded, None);

        let mut negative = element(CLUSTER_TIMESTAMP, &0_u64.to_be_bytes());
        negative.extend_from_slice(&grouped(0, &[0xCA, 0x98, 0x20]));
        let bytes = clustered(&scale, &[element(CLUSTER, &negative)]);
        assert_eq!(read_segment(&mut Cursor::new(bytes)).discarded, None);
    }

    #[test]
    fn a_segment_declaring_no_duration_is_counted_off_its_clusters() {
        let scale = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        let bytes = clustered(&scale, &[cluster(0, &[0, 500]), cluster(1_000, &[0, 500])]);

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).duration(),
            Some(Duration::from_millis(1_500))
        );
    }

    #[test]
    fn a_duration_the_clusters_run_past_is_the_writers_mistake_and_the_count_wins() {
        let mut info = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        info.extend_from_slice(&element(SEGMENT_DURATION, &400.0_f64.to_be_bytes()));

        let bytes = clustered(&info, &[cluster(0, &[0]), cluster(1_000, &[0, 500])]);
        let segment = read_segment(&mut Cursor::new(bytes));

        assert_eq!(segment.declared, Some(Duration::from_millis(400)));
        assert_eq!(segment.duration(), Some(Duration::from_millis(1_500)));
    }

    #[test]
    fn a_duration_the_clusters_stay_within_is_kept_as_the_writer_wrote_it() {
        let mut info = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        info.extend_from_slice(&element(SEGMENT_DURATION, &2_003.0_f64.to_be_bytes()));

        let bytes = clustered(&info, &[cluster(0, &[0, 500]), cluster(1_000, &[0, 500])]);

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).duration(),
            Some(Duration::from_millis(2_003))
        );
    }

    #[test]
    fn a_cluster_of_unknown_length_ends_the_count_rather_than_being_guessed_at() {
        let scale = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        let mut segment = element(INFO, &scale);
        segment.extend_from_slice(&cluster(0, &[0, 500]));
        segment.extend_from_slice(&CLUSTER.to_be_bytes());
        segment.push(UNBOUNDED);
        segment.extend_from_slice(&element(CLUSTER_TIMESTAMP, &9_000_u64.to_be_bytes()));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &segment));

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).duration(),
            Some(Duration::from_millis(500))
        );
    }

    const CELT_20_MS: u8 = 31 << 3;
    const TWO_FRAMES: u8 = 1;
    const XIPH_LACED: u8 = 0b0010;

    fn track_entry(number: u8, codec: &str) -> Vec<u8> {
        let mut entry = element(TRACK_NUMBER, &[number]);
        entry.extend_from_slice(&element(CODEC_ID, codec.as_bytes()));
        element(TRACK_ENTRY, &entry)
    }

    fn opus_block(track: u8, flags: u8, toc: u8) -> Vec<u8> {
        element(SIMPLE_BLOCK, &[0x80 | track, 0, 0, flags, toc, 0xFC])
    }

    fn with_tracks(entries: &[Vec<u8>], blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut segment = element(
            INFO,
            &element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes()),
        );
        segment.extend_from_slice(&element(TRACKS, &entries.concat()));
        let mut cluster = element(CLUSTER_TIMESTAMP, &0_u64.to_be_bytes());
        for block in blocks {
            cluster.extend_from_slice(block);
        }
        segment.extend_from_slice(&element(CLUSTER, &cluster));

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &segment));
        bytes
    }

    #[test]
    fn an_opus_track_is_counted_packet_by_packet_out_of_its_toc_bytes() {
        let bytes = with_tracks(
            &[track_entry(1, OPUS_CODEC_ID)],
            &[
                opus_block(1, 0, CELT_20_MS),
                opus_block(1, 0, CELT_20_MS | TWO_FRAMES),
                opus_block(1, 0, CELT_20_MS),
            ],
        );

        let found = read_segment(&mut Cursor::new(bytes));

        assert_eq!(found.opus_samples, Some(3_840));
        assert_eq!(found.opus_music(312, 100), Some(Frames(3_428)));
    }

    #[test]
    fn only_the_opus_track_s_blocks_are_counted() {
        let bytes = with_tracks(
            &[track_entry(1, "A_VORBIS"), track_entry(2, OPUS_CODEC_ID)],
            &[
                opus_block(1, 0, CELT_20_MS | TWO_FRAMES),
                opus_block(2, 0, CELT_20_MS),
            ],
        );

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).opus_samples,
            Some(960)
        );
    }

    #[test]
    fn a_file_with_no_opus_track_counts_nothing() {
        let bytes = with_tracks(&[track_entry(1, "A_FLAC")], &[opus_block(1, 0, CELT_20_MS)]);

        assert_eq!(read_segment(&mut Cursor::new(bytes)).opus_samples, None);
    }

    const FIXED_LACED: u8 = 0b0100;
    const EBML_LACED: u8 = 0b0110;

    fn frame(toc: u8, bytes: usize) -> Vec<u8> {
        let mut frame = vec![0xFC; bytes];
        frame[0] = toc;
        frame
    }

    fn laced_block(flags: u8, sizes: &[u8], frames: &[Vec<u8>]) -> Vec<u8> {
        let mut block = vec![0x81, 0, 0, flags, (frames.len() - 1) as u8];
        block.extend_from_slice(sizes);
        block.extend_from_slice(&frames.concat());
        element(SIMPLE_BLOCK, &block)
    }

    fn counted(blocks: &[Vec<u8>]) -> Option<u64> {
        read_segment(&mut Cursor::new(with_tracks(
            &[track_entry(1, OPUS_CODEC_ID)],
            blocks,
        )))
        .opus_samples
    }

    fn three_frames() -> Vec<Vec<u8>> {
        vec![
            frame(CELT_20_MS | TWO_FRAMES, 300),
            frame(CELT_20_MS, 3),
            frame(CELT_20_MS, 2),
        ]
    }

    #[test]
    fn a_xiph_laced_block_is_counted_frame_by_frame() {
        let block = laced_block(XIPH_LACED, &[255, 45, 3], &three_frames());

        assert_eq!(counted(&[opus_block(1, 0, CELT_20_MS), block]), Some(4_800));
    }

    #[test]
    fn an_ebml_laced_block_reads_its_sizes_as_a_first_and_signed_differences() {
        let first = [0x41, 0x2C];
        let three_less_than_three_hundred = [0x5E, 0xD6];
        let sizes = [first, three_less_than_three_hundred].concat();

        assert_eq!(
            counted(&[laced_block(EBML_LACED, &sizes, &three_frames())]),
            Some(3_840)
        );
    }

    #[test]
    fn a_fixed_laced_block_shares_what_it_holds_equally() {
        let frames = vec![frame(CELT_20_MS, 4); 3];

        assert_eq!(
            counted(&[laced_block(FIXED_LACED, &[], &frames)]),
            Some(2_880)
        );
    }

    #[test]
    fn a_lace_that_does_not_add_up_leaves_the_count_unknown_rather_than_wrong() {
        let uneven = vec![frame(CELT_20_MS, 4), frame(CELT_20_MS, 5)];
        let overlong = laced_block(XIPH_LACED, &[255, 255, 3, 3], &three_frames());

        assert_eq!(counted(&[laced_block(FIXED_LACED, &[], &uneven)]), None);
        assert_eq!(counted(&[overlong]), None);
    }

    #[test]
    fn a_laced_block_of_another_track_is_passed_over_uncounted() {
        let bytes = with_tracks(
            &[track_entry(1, "A_VORBIS"), track_entry(2, OPUS_CODEC_ID)],
            &[
                laced_block(XIPH_LACED, &[9], &[vec![0; 9], vec![0; 4]]),
                opus_block(2, 0, CELT_20_MS),
            ],
        );

        assert_eq!(
            read_segment(&mut Cursor::new(bytes)).opus_samples,
            Some(960)
        );
    }

    fn flac_block(track: u8, sizing: u8) -> Vec<u8> {
        element(
            SIMPLE_BLOCK,
            &[
                0x80 | track,
                0,
                0,
                0,
                0xFF,
                0xF8,
                sizing << 4 | 0x9,
                0x18,
                0,
                0xAB,
            ],
        )
    }

    #[test]
    fn a_flac_track_is_counted_frame_by_frame_out_of_its_frame_headers() {
        let four_thousand_and_ninety_six = 12;
        let one_hundred_and_ninety_two = 1;
        let bytes = with_tracks(
            &[track_entry(1, FLAC_CODEC_ID)],
            &[
                flac_block(1, four_thousand_and_ninety_six),
                flac_block(1, four_thousand_and_ninety_six),
                flac_block(1, one_hundred_and_ninety_two),
            ],
        );

        let found = read_segment(&mut Cursor::new(bytes));

        assert_eq!(found.flac_samples, Some(8_384));
        assert_eq!(found.opus_samples, None);
        assert_eq!(found.flac_frames_of(1), Some(Frames(8_384)));
        assert_eq!(found.flac_frames_of(2), None);
    }

    #[test]
    fn a_walk_that_stops_short_of_the_segment_s_end_leaves_the_count_unknown() {
        let whole = with_tracks(
            &[track_entry(1, OPUS_CODEC_ID)],
            &[opus_block(1, 0, CELT_20_MS), opus_block(1, 0, CELT_20_MS)],
        );
        let head = whole[..whole.len() - 3].to_vec();

        assert_eq!(
            read_segment(&mut Cursor::new(whole)).opus_samples,
            Some(1_920)
        );
        assert_eq!(read_segment(&mut Cursor::new(head)).opus_samples, None);
    }

    #[test]
    fn a_track_audio_bit_depth_is_read_out_of_the_tracks_element() {
        let audio = element(TRACK_AUDIO, &element(BIT_DEPTH, &[24_u8]));
        let tracks = element(TRACKS, &element(TRACK_ENTRY, &audio));

        let mut segment = element(INFO, &titled("Echoes"));
        segment.extend_from_slice(&tracks);

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &segment));

        assert_eq!(read_segment(&mut Cursor::new(bytes)).bit_depth, Some(24));
    }

    #[test]
    fn a_file_declaring_no_bit_depth_leaves_it_unknown() {
        assert_eq!(
            read_segment(&mut Cursor::new(file("Echoes"))).bit_depth,
            None
        );
    }

    #[test]
    fn a_container_that_is_not_matroska_yields_nothing() {
        assert_eq!(
            read_segment(&mut Cursor::new(b"fLaC\0\0\0\x22".to_vec())),
            Segment::default()
        );
        assert_eq!(
            read_segment(&mut Cursor::new(b"RIFF\0\0\0\0WAVE".to_vec())),
            Segment::default()
        );
        assert_eq!(
            read_segment(&mut Cursor::new(Vec::new())),
            Segment::default()
        );
    }

    #[test]
    fn a_title_that_claims_more_than_the_file_holds_yields_nothing() {
        let mut info = element(TIMESTAMP_SCALE, &NANOSECONDS_PER_TICK.to_be_bytes());
        info.extend_from_slice(&TITLE.to_be_bytes()[2..]);
        info.extend_from_slice(&vint(64, 8));
        info.extend_from_slice(b"Echo");

        let mut bytes = element(EBML_HEADER, b"an ebml header");
        bytes.extend_from_slice(&element(SEGMENT, &element(INFO, &info)));

        assert!(read_segment(&mut Cursor::new(bytes)).title.is_none());
    }
}
