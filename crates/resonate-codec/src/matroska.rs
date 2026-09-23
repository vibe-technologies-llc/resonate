use std::{
    io::{Read, Seek, SeekFrom},
    time::Duration,
};

use resonate_core::{Frames, SampleRate};

use crate::prescan::read_exact;

const EBML_HEADER: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TITLE: u32 = 0x7BA9;
const SEGMENT_DURATION: u32 = 0x4489;
const TIMESTAMP_SCALE: u32 = 0x002A_D7B1;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0x00AE;
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
const BLOCK_HEADER_BYTES: u64 = 4;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Segment {
    pub(crate) title: Option<String>,
    pub(crate) scale: Option<u64>,
    pub(crate) declared: Option<Duration>,
    pub(crate) counted: Option<Duration>,
    pub(crate) bit_depth: Option<u32>,
    pub(crate) discarded: Option<Duration>,
}

impl Segment {
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
    if source.seek(SeekFrom::Start(body)).is_ok() {
        let counted = count_clusters(source, within);
        let scale = found.scale.unwrap_or(DEFAULT_TIMESTAMP_SCALE);
        found.counted = span(counted.ticks.map(|ticks| ticks as f64), scale);
        found.discarded = counted.discarded;
    }

    Some(found)
}

fn count_clusters<S: Read + Seek + ?Sized>(source: &mut S, within: Option<u64>) -> Counted {
    let mut counted = Counted::default();

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
            break;
        }
    }

    counted
}

fn read_cluster<S: Read + Seek + ?Sized>(source: &mut S, limit: Option<u64>, into: &mut Counted) {
    let Ok(body) = source.stream_position() else {
        return;
    };
    let mut at = None;
    let mut offsets = Offsets::default();

    for _ in 0..MAX_BLOCKS {
        let Some(element) = element(source) else {
            break;
        };
        match element.id {
            CLUSTER_TIMESTAMP => at = uint(source, element.length),
            SIMPLE_BLOCK => {
                offsets.saw(block_offset(source, element.length));
                into.discarded = None;
            }
            BLOCK_GROUP => {
                let group = read_group(source, element.end());
                offsets.saw(group.offset);
                into.discarded = group.discarded;
            }
            _ => {}
        }

        let Some(next) = skip(source, &element) else {
            break;
        };
        if limit.is_some_and(|limit| next >= limit) {
            break;
        }
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
    offset: Option<i16>,
    discarded: Option<Duration>,
}

fn read_group<S: Read + Seek + ?Sized>(source: &mut S, limit: Option<u64>) -> Group {
    let mut group = Group::default();

    for _ in 0..MAX_ELEMENTS {
        let Some(element) = element(source) else {
            break;
        };
        match element.id {
            BLOCK => group.offset = block_offset(source, element.length),
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

fn block_offset<S: Read + Seek + ?Sized>(source: &mut S, length: Length) -> Option<i16> {
    let Length::Known(bytes) = length else {
        return None;
    };
    let Length::Known(_) = self::length(source)? else {
        return None;
    };
    if bytes < BLOCK_HEADER_BYTES {
        return None;
    }

    read_exact::<2, S>(source).map(i16::from_be_bytes)
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
    let [lead] = read_exact::<1, S>(source)?;
    let width = width(lead, MAX_LENGTH_BYTES)?;
    let marker = 0x80_u8 >> (width - 1);

    let mut length = u64::from(lead & (marker - 1));
    for _ in 1..width {
        let [byte] = read_exact::<1, S>(source)?;
        length = (length << 8) | u64::from(byte);
    }

    let unbounded = (1_u64 << (7 * u64::from(width))) - 1;
    Some(if length == unbounded {
        Length::Unknown
    } else {
        Length::Known(length)
    })
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
