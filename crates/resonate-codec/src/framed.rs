use std::{
    collections::{BTreeMap, VecDeque},
    io::{Cursor, Seek, SeekFrom},
    marker::PhantomData,
};

use symphonia::{
    core::{
        errors::{Error, Result, SeekErrorKind, decode_error, seek_error},
        formats::{
            FormatInfo, FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo, Track,
            prelude::{ChapterGroup, MediaInfo},
            probe::{ProbeFormatData, ProbeableFormat, Score, Scoreable},
        },
        io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions, ReadBytes, ScopedStream},
        meta::{Metadata, MetadataLog},
        packet::Packet,
        units::{Duration, Timestamp},
    },
    default::formats::{AdtsReader, MpaReader},
};

use crate::mpa::FrameHeader;

const FRAMES_HELD_BEHIND: usize = 16;
const MARKED_EVERY_FRAMES: u64 = 32;
const LOST_TO_A_FAILED_SEEK: &str = "the stream was lost to a seek that could not reopen it";

const MPEG_SYNCED: u32 = 0xffe0_0000;
const MPEG_HEADER_BYTES: u64 = 4;
const MPEG_CRC_BYTES: usize = 2;
const MPEG_WEIGHED_BYTES: usize = 64;
const MPEG_RESERVOIR_LEAD_FRAMES: i64 = 8;
const MPEG_LONGEST_FRAME_SAMPLES: i64 = 1_152;
const XING: &[u8; 4] = b"Xing";
const INFO: &[u8; 4] = b"Info";
const VBRI: &[u8; 4] = b"VBRI";
const VBRI_AT: usize = 36;
const VBRI_BYTES_AT_LEAST: usize = 26;
const VBRI_VERSION: u16 = 1;
const XING_NOTE_BYTES: usize = 8;
const XING_FIELDS: [(u32, usize); 4] = [(0b0001, 4), (0b0010, 4), (0b0100, 100), (0b1000, 4)];

const ADTS_SYNC_MASK: u16 = 0xfff6;
const ADTS_SYNC: u16 = 0xfff0;
const ADTS_HEADER_BYTES: u16 = 7;
const ADTS_CRC_BYTES: u16 = 2;
const ADTS_SAMPLES: u64 = 1_024;
const ADTS_SAMPLE_RATES: u8 = 13;

pub(crate) trait Framing: 'static {
    type Key: Copy + PartialEq;

    const RESTARTS_ONLY_BEFORE_A_LIKE_FRAME: bool;
    const LEAD: i64;

    fn open<'s>(
        stream: MediaSourceStream<'s>,
        options: FormatOptions,
    ) -> Result<Box<dyn FormatReader + 's>>;

    fn score(stream: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score>;

    fn probe_data() -> &'static [ProbeFormatData];

    fn walk(stream: &mut MediaSourceStream<'_>) -> Result<Option<Walked<Self::Key>>>;

    fn lasts(packet: &Packet) -> u64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Walked<K> {
    at: u64,
    length: u64,
    samples: u64,
    reservoir: u64,
    beside: u64,
    key: K,
}

pub(crate) enum Mpeg {}

impl Framing for Mpeg {
    type Key = (crate::mpa::Stream, bool);

    const RESTARTS_ONLY_BEFORE_A_LIKE_FRAME: bool = true;
    const LEAD: i64 = MPEG_RESERVOIR_LEAD_FRAMES * MPEG_LONGEST_FRAME_SAMPLES;

    fn open<'s>(
        stream: MediaSourceStream<'s>,
        options: FormatOptions,
    ) -> Result<Box<dyn FormatReader + 's>> {
        Ok(Box::new(MpaReader::try_new(stream, options)?))
    }

    fn score(stream: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        MpaReader::score(stream)
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        <MpaReader<'_> as ProbeableFormat<'_>>::probe_data()
    }

    fn walk(stream: &mut MediaSourceStream<'_>) -> Result<Option<Walked<Self::Key>>> {
        loop {
            let Some(word) = until_eof(mpeg_sync(stream))? else {
                return Ok(None);
            };
            let Some(header) = FrameHeader::read(&word.to_be_bytes()) else {
                continue;
            };
            let at = stream.pos() - MPEG_HEADER_BYTES;
            let length = header.length as u64;

            let mut opening = [0_u8; MPEG_WEIGHED_BYTES];
            let weighed = header.length.min(MPEG_WEIGHED_BYTES);
            opening[..4].copy_from_slice(&word.to_be_bytes());
            let body = &mut opening[MPEG_HEADER_BYTES as usize..weighed];
            if until_eof(stream.read_buf_exact(body).map_err(Error::from))?.is_none() {
                return Ok(None);
            }
            let rest = length.saturating_sub(weighed as u64);
            if until_eof(stream.ignore_bytes(rest).map_err(Error::from))?.is_none() {
                return Ok(None);
            }

            let opening = &opening[..weighed];
            if is_an_info_note(opening, header) || is_a_vbri_note(opening, header) {
                continue;
            }
            return Ok(Some(Walked {
                at,
                length,
                samples: header.samples(),
                reservoir: reservoir(opening, header),
                beside: 0,
                key: (header.stream, header.mono),
            }));
        }
    }

    fn lasts(packet: &Packet) -> u64 {
        FrameHeader::read(&packet.data).map_or(packet.dur.get(), FrameHeader::samples)
    }
}

fn mpeg_sync(stream: &mut MediaSourceStream<'_>) -> Result<u32> {
    let mut word = 0_u32;
    loop {
        while word & MPEG_SYNCED != MPEG_SYNCED {
            word = (word << 8) | u32::from(stream.read_u8()?);
        }
        if could_head_a_frame(word) {
            return Ok(word);
        }
        word = (word << 8) | u32::from(stream.read_u8()?);
    }
}

const fn could_head_a_frame(word: u32) -> bool {
    let version = (word >> 19) & 0b11;
    let layer = (word >> 17) & 0b11;
    let bitrate = (word >> 12) & 0b1111;
    let rate = (word >> 10) & 0b11;
    version != 0b01 && layer != 0b00 && bitrate != 0b1111 && rate != 0b11
}

fn header_bytes(header: FrameHeader) -> usize {
    MPEG_HEADER_BYTES as usize + if header.has_crc { MPEG_CRC_BYTES } else { 0 }
}

fn is_an_info_note(opening: &[u8], header: FrameHeader) -> bool {
    let Some(at) = header.xing_at().map(|at| at as usize) else {
        return false;
    };
    let Some(note) = opening.get(at..at + XING_NOTE_BYTES) else {
        return false;
    };
    let (named, flags) = note.split_at(4);
    if named != XING && named != INFO {
        return false;
    }
    if opening
        .get(header_bytes(header)..at)
        .is_none_or(|side| side.iter().any(|&byte| byte != 0))
    {
        return false;
    }
    let flags = u32::from_be_bytes([flags[0], flags[1], flags[2], flags[3]]);
    let fields: usize = XING_FIELDS
        .iter()
        .filter(|(flag, _)| flags & flag != 0)
        .map(|(_, bytes)| bytes)
        .sum();
    at + XING_NOTE_BYTES + fields <= header.length
}

fn is_a_vbri_note(opening: &[u8], header: FrameHeader) -> bool {
    if header.xing_at().is_none() || header.length < VBRI_AT + VBRI_BYTES_AT_LEAST {
        return false;
    }
    if opening.get(VBRI_AT..VBRI_AT + 4) != Some(VBRI.as_slice()) {
        return false;
    }
    if opening
        .get(header_bytes(header)..VBRI_AT)
        .is_none_or(|side| side.iter().any(|&byte| byte != 0))
    {
        return false;
    }
    opening
        .get(VBRI_AT + 4..VBRI_AT + 6)
        .is_some_and(|version| u16::from_be_bytes([version[0], version[1]]) == VBRI_VERSION)
}

fn reservoir(opening: &[u8], header: FrameHeader) -> u64 {
    if header.xing_at().is_none() {
        return 0;
    }
    let at = header_bytes(header);
    match (header.is_mpeg_one(), opening.get(at..at + 2)) {
        (true, Some(&[high, low])) => u64::from(u16::from_be_bytes([high, low]) >> 7),
        (false, Some(&[high, _])) => u64::from(high),
        (_, _) => 0,
    }
}

pub(crate) enum Adts {}

impl Framing for Adts {
    type Key = ();

    const RESTARTS_ONLY_BEFORE_A_LIKE_FRAME: bool = false;
    const LEAD: i64 = 0;

    fn open<'s>(
        stream: MediaSourceStream<'s>,
        options: FormatOptions,
    ) -> Result<Box<dyn FormatReader + 's>> {
        Ok(Box::new(AdtsReader::try_new(stream, options)?))
    }

    fn score(stream: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        AdtsReader::score(stream)
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        <AdtsReader<'_> as ProbeableFormat<'_>>::probe_data()
    }

    fn walk(stream: &mut MediaSourceStream<'_>) -> Result<Option<Walked<Self::Key>>> {
        let mut word = 0_u16;
        while word & ADTS_SYNC_MASK != ADTS_SYNC {
            let Some(byte) = until_eof(stream.read_u8().map_err(Error::from))? else {
                return Ok(None);
            };
            word = (word << 8) | u16::from(byte);
        }
        let at = stream.pos() - 2;
        let checked = word & 1 == 0;
        let header = if checked {
            ADTS_HEADER_BYTES + ADTS_CRC_BYTES
        } else {
            ADTS_HEADER_BYTES
        };

        let mut body = [0_u8; 5];
        if until_eof(stream.read_buf_exact(&mut body).map_err(Error::from))?.is_none() {
            return Ok(None);
        }
        if (body[0] >> 2) & 0b1111 >= ADTS_SAMPLE_RATES {
            return decode_error("adts: invalid sample rate");
        }
        let length = (u16::from(body[1] & 0b11) << 11)
            | (u16::from(body[2]) << 3)
            | (u16::from(body[3]) >> 5);
        if length < header {
            return decode_error("adts: invalid adts frame length");
        }
        if body[4] & 0b11 != 0 {
            return decode_error("adts: only 1 aac frame per adts packet is supported");
        }

        let rest = u64::from(length) - 5 - 2;
        if until_eof(stream.ignore_bytes(rest).map_err(Error::from))?.is_none() {
            return Ok(None);
        }
        Ok(Some(Walked {
            at,
            length: u64::from(length),
            samples: ADTS_SAMPLES,
            reservoir: 0,
            beside: u64::from(header),
            key: (),
        }))
    }

    fn lasts(_: &Packet) -> u64 {
        ADTS_SAMPLES
    }
}

fn until_eof<T>(read: Result<T>) -> Result<Option<T>> {
    match read {
        Ok(read) => Ok(Some(read)),
        Err(Error::IoError(source)) if source.kind() == std::io::ErrorKind::UnexpectedEof => {
            Ok(None)
        }
        Err(source) => Err(source),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mark {
    at: u64,
    ts: Timestamp,
}

#[derive(Clone, Copy, Debug)]
struct Frame<K> {
    walked: Walked<K>,
    ts: Timestamp,
    restarts: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Walked,
    Heard,
}

impl<K: Copy + PartialEq> Frame<K> {
    const fn mark(&self) -> Mark {
        Mark {
            at: self.walked.at,
            ts: self.ts,
        }
    }

    fn is_followed_by(&self, next: &Walked<K>) -> bool {
        self.walked.at + self.walked.length == next.at && self.walked.key == next.key
    }
}

pub(crate) struct Framed<'s, F> {
    reading: Option<Box<dyn FormatReader + 's>>,
    format: FormatInfo,
    media: MediaInfo,
    tracks: Vec<Track>,
    metadata: MetadataLog,
    chapters: Option<ChapterGroup>,
    opened_at: u64,
    music: Mark,
    marks: BTreeMap<i64, (u64, Found)>,
    shift: i64,
    next: Timestamp,
    playing: Option<u64>,
    synced: Timestamp,
    beside: u64,
    seekable: bool,
    framing: PhantomData<fn() -> F>,
}

impl<'s, F: Framing> Framed<'s, F> {
    pub(crate) fn try_new(stream: MediaSourceStream<'s>, options: FormatOptions) -> Result<Self> {
        let mut options = options;
        let metadata = options.external_data.metadata.take().unwrap_or_default();
        let chapters = options.external_data.chapters.take();
        let seekable = stream.is_seekable();
        let opened_at = stream.pos();

        let reading = F::open(stream, options)?;
        let format = *reading.format_info();
        let media = *reading.media_info();
        let tracks = reading.tracks().to_vec();
        let first = first_ts(&tracks);

        let (reading, music_at, beside) = if seekable {
            let mut stream = reading.into_inner();
            let music_at = stream.pos();
            let beside = F::walk(&mut stream)
                .ok()
                .flatten()
                .map_or(0, |walked| walked.beside);
            stream.seek(SeekFrom::Start(opened_at))?;
            (F::open(stream, FormatOptions::default())?, music_at, beside)
        } else {
            (reading, opened_at, 0)
        };

        Ok(Self {
            reading: Some(reading),
            format,
            media,
            tracks,
            metadata,
            chapters,
            opened_at,
            music: Mark {
                at: music_at,
                ts: first,
            },
            marks: BTreeMap::new(),
            shift: 0,
            next: first,
            playing: seekable.then_some(music_at),
            synced: first,
            beside,
            seekable,
            framing: PhantomData,
        })
    }

    fn reading(&mut self) -> Result<&mut Box<dyn FormatReader + 's>> {
        match self.reading.as_mut() {
            Some(reading) => Ok(reading),
            None => decode_error(LOST_TO_A_FAILED_SEEK),
        }
    }

    fn required(&self, to: SeekTo) -> Result<Timestamp> {
        match to {
            SeekTo::Timestamp { ts, .. } => Ok(ts),
            SeekTo::Time { time, .. } => {
                let base = self
                    .tracks
                    .first()
                    .and_then(|track| track.time_base)
                    .ok_or(Error::SeekError(SeekErrorKind::Unseekable))?;
                base.calc_timestamp(time)
                    .ok_or(Error::SeekError(SeekErrorKind::OutOfRange))
            }
        }
    }

    fn walk_from(&self, current: Mark, required: Timestamp) -> Mark {
        let lead = required.get().saturating_sub(F::LEAD);
        let marked = self
            .marks
            .range(..=lead)
            .next_back()
            .map(|(&ts, &(at, _))| Mark {
                at,
                ts: Timestamp::new(ts),
            });
        [marked, (current.ts.get() <= lead).then_some(current)]
            .into_iter()
            .flatten()
            .fold(
                self.music,
                |best, mark| if mark.ts > best.ts { mark } else { best },
            )
    }

    fn keep_a_mark(&mut self, mark: Mark, samples: u64, found: Found) {
        let ts = mark.ts.get();
        let apart = i64::try_from(samples.saturating_mul(MARKED_EVERY_FRAMES)).unwrap_or(i64::MAX);
        let near = ts.saturating_sub(apart)..=ts.saturating_add(apart);
        if self.marks.range(near).next().is_none() {
            self.marks.insert(ts, (mark.at, found));
        }
    }

    fn forget_what_was_heard_since_the_stream_drifted(&mut self, stood_at: u64) {
        if self.playing.is_none_or(|playing| playing == stood_at) {
            return;
        }
        let synced = self.synced.get();
        self.marks
            .retain(|&ts, &mut (_, found)| ts <= synced || found == Found::Walked);
    }

    fn restart_for(
        &mut self,
        stream: &mut MediaSourceStream<'_>,
        from: Mark,
        required: Timestamp,
    ) -> Result<Option<Mark>> {
        stream.seek(SeekFrom::Start(from.at))?;
        let mut behind: VecDeque<Frame<F::Key>> = VecDeque::with_capacity(FRAMES_HELD_BEHIND);
        let mut ts = from.ts;

        let target = loop {
            let Some(walked) = F::walk(stream)? else {
                return seek_error(SeekErrorKind::OutOfRange);
            };
            if let Some(before) = behind.back_mut() {
                before.restarts =
                    !F::RESTARTS_ONLY_BEFORE_A_LIKE_FRAME || before.is_followed_by(&walked);
                let before = *before;
                self.keep_a_mark(before.mark(), before.walked.samples, Found::Walked);
            }
            if behind.len() == FRAMES_HELD_BEHIND {
                behind.pop_front();
            }
            behind.push_back(Frame {
                walked,
                ts,
                restarts: false,
            });

            let ends = ts
                .checked_add(Duration::new(walked.samples))
                .ok_or(Error::SeekError(SeekErrorKind::OutOfRange))?;
            if ends > required {
                break walked;
            }
            ts = ends;
        };

        let after = F::walk(stream);
        if let Some(last) = behind.back_mut() {
            last.restarts = !F::RESTARTS_ONLY_BEFORE_A_LIKE_FRAME
                || match after {
                    Ok(Some(next)) => last.is_followed_by(&next),
                    Ok(None) => stream.byte_len().is_some_and(|length| {
                        length.saturating_sub(last.walked.at + last.walked.length)
                            < MPEG_HEADER_BYTES
                    }),
                    Err(_) => false,
                };
        }

        Ok(behind
            .iter()
            .rev()
            .find(|frame| {
                frame.restarts && target.at.saturating_sub(frame.walked.at) >= target.reservoir
            })
            .map(Frame::mark))
    }

    fn reopen_at(&mut self, mut stream: MediaSourceStream<'s>, restart: Mark) -> Result<()> {
        stream.seek(SeekFrom::Start(restart.at))?;
        let reading = F::open(stream, FormatOptions::default())?;
        self.shift = restart.ts.get() - first_ts(reading.tracks()).get();
        self.next = restart.ts;
        self.playing = Some(restart.at);
        self.synced = restart.ts;
        self.reading = Some(reading);
        Ok(())
    }

    fn reopen_at_the_opening(&mut self, mut stream: MediaSourceStream<'s>) -> Result<()> {
        stream.seek(SeekFrom::Start(self.opened_at))?;
        self.reading = Some(F::open(stream, FormatOptions::default())?);
        self.shift = 0;
        self.next = self.music.ts;
        self.playing = Some(self.music.at);
        self.synced = self.music.ts;
        Ok(())
    }

    fn walk_from_the_opening(
        &mut self,
        stream: MediaSourceStream<'s>,
        mode: SeekMode,
        required: Timestamp,
    ) -> Result<SeekedTo> {
        self.reopen_at_the_opening(stream)?;
        let seeked = self.reading()?.seek(
            mode,
            SeekTo::Timestamp {
                ts: required,
                track_id: 0,
            },
        )?;
        self.next = seeked.actual_ts;
        self.playing = None;
        Ok(seeked)
    }

    fn hear(&mut self, packet: &Packet) {
        let Some(at) = self.playing else {
            return;
        };
        let samples = F::lasts(packet);
        self.keep_a_mark(Mark { at, ts: packet.pts }, samples, Found::Heard);
        self.playing = Some(at + packet.data.len() as u64 + self.beside);
    }
}

fn first_ts(tracks: &[Track]) -> Timestamp {
    let delay = tracks.first().and_then(|track| track.delay).unwrap_or(0);
    Timestamp::new(-i64::from(delay))
}

fn shifted(ts: Timestamp, by: i64) -> Result<Timestamp> {
    match ts.get().checked_add(by) {
        Some(ts) => Ok(Timestamp::new(ts)),
        None => decode_error("a packet's timestamp moved past what a timestamp holds"),
    }
}

impl<F: Framing> Scoreable for Framed<'_, F> {
    fn score(stream: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        F::score(stream)
    }
}

impl<'s, F: Framing> ProbeableFormat<'s> for Framed<'_, F> {
    fn try_probe_new(
        stream: MediaSourceStream<'s>,
        options: FormatOptions,
    ) -> Result<Box<dyn FormatReader + 's>> {
        Ok(Box::new(Framed::<'s, F>::try_new(stream, options)?))
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        F::probe_data()
    }
}

impl<'s, F: Framing> FormatReader for Framed<'s, F> {
    fn format_info(&self) -> &FormatInfo {
        &self.format
    }

    fn media_info(&self) -> &MediaInfo {
        &self.media
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.metadata.metadata()
    }

    fn chapters(&self) -> Option<&ChapterGroup> {
        self.chapters.as_ref()
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self) -> Result<Option<Packet>> {
        let shift = self.shift;
        let Some(mut packet) = self.reading()?.next_packet()? else {
            return Ok(None);
        };
        packet.pts = shifted(packet.pts, shift)?;
        packet.dts = shifted(packet.dts, shift)?;
        self.hear(&packet);
        self.next = packet
            .pts
            .checked_add(Duration::new(F::lasts(&packet)))
            .unwrap_or(Timestamp::MAX);
        Ok(Some(packet))
    }

    fn seek(&mut self, mode: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let required = self.required(to)?;
        if !self.seekable {
            let seeked = self.reading()?.seek(
                mode,
                SeekTo::Timestamp {
                    ts: required,
                    track_id: 0,
                },
            )?;
            self.next = seeked.actual_ts;
            return Ok(seeked);
        }
        if required < self.music.ts {
            return seek_error(SeekErrorKind::OutOfRange);
        }

        let Some(reading) = self.reading.take() else {
            return decode_error(LOST_TO_A_FAILED_SEEK);
        };
        let mut stream = reading.into_inner();
        let current = Mark {
            at: stream.pos(),
            ts: self.next,
        };
        self.forget_what_was_heard_since_the_stream_drifted(current.at);
        let from = self.walk_from(current, required);

        match self.restart_for(&mut stream, from, required) {
            Ok(Some(restart)) => {
                self.reopen_at(stream, restart)?;
                Ok(SeekedTo {
                    track_id: 0,
                    required_ts: required,
                    actual_ts: restart.ts,
                })
            }
            Ok(None) => self.walk_from_the_opening(stream, mode, required),
            Err(source) => {
                self.reopen_at_the_opening(stream)?;
                Err(source)
            }
        }
    }

    fn into_inner<'a>(self: Box<Self>) -> MediaSourceStream<'a>
    where
        Self: 'a,
    {
        match self.reading {
            Some(reading) => reading.into_inner(),
            None => MediaSourceStream::new(
                Box::new(Cursor::new(Vec::<u8>::new())),
                MediaSourceStreamOptions::default(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Read},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use super::*;

    const AT_128: [u8; 4] = [0xff, 0xfb, 0x90, 0x00];
    const AT_128_LENGTH: usize = 417;
    const AT_320: [u8; 4] = [0xff, 0xfb, 0xe0, 0x00];
    const AT_320_LENGTH: usize = 1_044;
    const SAMPLES: i64 = 1_152;
    const FRAMES: usize = 600;

    struct Counted {
        held: Cursor<Vec<u8>>,
        read: Arc<AtomicU64>,
    }

    impl Read for Counted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let read = self.held.read(buf)?;
            self.read.fetch_add(read as u64, Ordering::Relaxed);
            Ok(read)
        }
    }

    impl Seek for Counted {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.held.seek(to)
        }
    }

    impl MediaSource for Counted {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            Some(self.held.get_ref().len() as u64)
        }
    }

    fn mpeg_frame(index: usize) -> Vec<u8> {
        let (header, length) = if index.is_multiple_of(3) {
            (AT_320, AT_320_LENGTH)
        } else {
            (AT_128, AT_128_LENGTH)
        };
        let mut frame = header.to_vec();
        frame.resize(length, 0);
        frame[48..56].copy_from_slice(&(index as u64).to_be_bytes());
        frame
    }

    fn mpeg(frames: usize) -> Vec<u8> {
        (0..frames).flat_map(mpeg_frame).collect()
    }

    fn adts_frame(index: usize) -> Vec<u8> {
        let length = 200 + index % 7 * 31;
        let mut frame = vec![0xff, 0xf1, 0x50, 0x80, 0, 0, 0xfc];
        frame[3] |= ((length >> 11) & 0b11) as u8;
        frame[4] = ((length >> 3) & 0xff) as u8;
        frame[5] = (((length & 0b111) << 5) as u8) | 0x1f;
        frame.resize(length, 0);
        frame[8..16].copy_from_slice(&(index as u64).to_be_bytes());
        frame
    }

    fn adts(frames: usize) -> Vec<u8> {
        (0..frames).flat_map(adts_frame).collect()
    }

    fn opened<F: Framing>(file: Vec<u8>) -> (Framed<'static, F>, Arc<AtomicU64>) {
        let read = Arc::new(AtomicU64::new(0));
        let source = Counted {
            held: Cursor::new(file),
            read: Arc::clone(&read),
        };
        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
        (
            Framed::try_new(stream, FormatOptions::default()).unwrap(),
            read,
        )
    }

    fn every_packet<F: Framing>(reader: &mut Framed<'_, F>) -> Vec<(i64, Box<[u8]>)> {
        std::iter::from_fn(|| reader.next_packet().unwrap())
            .map(|packet| (packet.pts.get(), packet.data))
            .collect()
    }

    fn seek_to<F: Framing>(reader: &mut Framed<'_, F>, ts: i64) -> i64 {
        reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: Timestamp::new(ts),
                    track_id: 0,
                },
            )
            .unwrap()
            .actual_ts
            .get()
    }

    fn lands_where_a_reading_from_the_start_would<F: Framing>(
        file: Vec<u8>,
        samples: i64,
        wanted: &[i64],
    ) {
        let whole = every_packet(&mut opened::<F>(file.clone()).0);
        let (mut reader, _) = opened::<F>(file);
        every_packet(&mut reader);

        for &ts in wanted {
            let landed = seek_to(&mut reader, ts);
            assert!(landed <= ts, "a seek to {ts} landed past it, at {landed}");
            assert!(
                ts - landed < samples * FRAMES_HELD_BEHIND as i64,
                "a seek to {ts} landed far short of it, at {landed}"
            );
            let next = reader.next_packet().unwrap().unwrap();
            let heard = whole.iter().find(|(pts, _)| *pts == landed).unwrap();
            assert_eq!(next.pts.get(), landed);
            assert_eq!(
                next.data, heard.1,
                "the packet at {landed} is not the one there"
            );
        }
    }

    #[test]
    fn a_variable_rate_mpeg_stream_lands_every_seek_where_a_walk_from_its_start_would() {
        let wanted = [
            500 * SAMPLES + 17,
            10 * SAMPLES,
            599 * SAMPLES,
            0,
            300 * SAMPLES + 1_151,
        ];
        lands_where_a_reading_from_the_start_would::<Mpeg>(mpeg(FRAMES), SAMPLES, &wanted);
    }

    #[test]
    fn an_adts_stream_lands_every_seek_where_a_walk_from_its_start_would() {
        let samples = ADTS_SAMPLES as i64;
        let wanted = [500 * samples + 3, 0, 250 * samples, 599 * samples + 1_000];
        lands_where_a_reading_from_the_start_would::<Adts>(adts(FRAMES), samples, &wanted);
    }

    #[test]
    fn a_backward_seek_after_playing_reads_near_where_it_lands_not_from_the_first_frame() {
        let file = mpeg(FRAMES);
        let (mut reader, read) = opened::<Mpeg>(file.clone());
        every_packet(&mut reader);

        let before = read.load(Ordering::Relaxed);
        let landed = seek_to(&mut reader, 550 * SAMPLES);
        reader.next_packet().unwrap().unwrap();
        let spent = read.load(Ordering::Relaxed) - before;

        assert!(landed > 500 * SAMPLES);
        let prefix = (0..550).map(|index| mpeg_frame(index).len()).sum::<usize>() as u64;
        assert!(
            spent * 10 < prefix,
            "a seek near the end read {spent} bytes of a {prefix} byte prefix"
        );
    }

    #[test]
    fn junk_between_frames_moves_no_seek_off_its_frame() {
        let mut file = mpeg(200);
        file.extend_from_slice(&[0x12; 333]);
        file.extend((200..FRAMES).flat_map(mpeg_frame));
        let wanted = [
            100 * SAMPLES,
            450 * SAMPLES + 5,
            199 * SAMPLES,
            201 * SAMPLES,
            50,
        ];
        lands_where_a_reading_from_the_start_would::<Mpeg>(file, SAMPLES, &wanted);
    }

    #[test]
    fn a_seek_past_the_last_frame_is_out_of_range_and_the_stream_still_reads() {
        let (mut reader, _) = opened::<Mpeg>(mpeg(40));
        let past = reader.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts: Timestamp::new(41 * SAMPLES),
                track_id: 0,
            },
        );
        assert!(matches!(
            past,
            Err(Error::SeekError(SeekErrorKind::OutOfRange))
        ));
        assert_eq!(every_packet(&mut reader).len(), 40);
    }
}
