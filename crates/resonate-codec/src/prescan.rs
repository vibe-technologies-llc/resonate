use std::io::{self, ErrorKind, Read, Seek, SeekFrom};

use resonate_core::{Frames, SampleRate};

use crate::{
    boxes::{self, Movie},
    caf::{self, Overflow},
    chapters::Marked,
    flac::{self, Flac},
    matroska::{self, Segment},
    riff::{self, Riff},
    wavpack::{self, Coding},
    wide::Wide,
};

const PRESCAN_WINDOW: usize = 4 * 1024;
pub(crate) const PROBED_WITHIN: u64 = 1024 * 1024;
const SEARCHED_AT_ONCE: usize = 16 * 1024;
const LOOKED_PAST: usize = 9 * 1024;
const OTHER_CONTAINERS: [&[u8; 4]; 9] = [
    b"fLaC",
    b"OggS",
    b"\x1a\x45\xdf\xa3",
    b"FORM",
    b"wvpk",
    b"MAC ",
    b"MACF",
    b"DSD ",
    b"FRM8",
];
const MP4_TYPE_AT: usize = 4;
const MP4_TYPE: &[u8; 4] = b"ftyp";
const RIFF: &[u8; 4] = b"RIFF";
const WAVE: &[u8; 4] = b"WAVE";
const WAVE_AT: usize = 8;
const CAFF: &[u8; 4] = b"caff";
const MPEG_SYNC: u8 = 0xff;
const MPEG_SYNC_HIGH: u8 = 0xe0;
const ID3: &[u8; 3] = b"ID3";
const ID3_HEADER: u64 = 10;
const ID3_FOOTER_FLAG: u8 = 0x10;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Prescan {
    pub(crate) riff: Riff,
    pub(crate) segment: Segment,
    pub(crate) boxes: Movie,
    pub(crate) flac: Flac,
    pub(crate) caf: Option<Overflow>,
    pub(crate) wavpack: Coding,
}

impl Prescan {
    pub(crate) fn read<S: Read + Seek + ?Sized>(source: &mut S) -> Self {
        let riff = riff::read(source);
        let segment = matroska::read_segment(source);
        Self {
            riff,
            boxes: boxes::read_movie(source),
            flac: flac::read(source),
            caf: caf::read(source),
            wavpack: wavpack::read_coding(source).or(segment.wavpack),
            segment,
        }
    }

    pub(crate) fn buffered<S: Read + Seek + ?Sized>(source: &mut S) -> Self {
        let Ok(origin) = source.stream_position() else {
            return Self::read(source);
        };

        let mut window = Window::over(source, origin);
        let found = Self::read(&mut window);
        window.rewind_the_source();

        found
    }

    pub(crate) fn chapters(&self) -> Vec<Marked> {
        match self.segment.chapters.is_empty() {
            true => self.boxes.chapters.clone(),
            false => self.segment.chapters.clone(),
        }
    }

    pub(crate) fn segment_duration(&self, rate: SampleRate) -> Option<Frames> {
        self.segment
            .duration()
            .map(|held| Frames::from_duration(held, rate))
    }
}

struct Window<'a, S: ?Sized> {
    source: &'a mut S,
    held: [u8; PRESCAN_WINDOW],
    filled: usize,
    at: u64,
    origin: u64,
    position: u64,
}

impl<'a, S: Read + Seek + ?Sized> Window<'a, S> {
    fn over(source: &'a mut S, origin: u64) -> Self {
        Self {
            source,
            held: [0; PRESCAN_WINDOW],
            filled: 0,
            at: origin,
            origin,
            position: origin,
        }
    }

    fn rewind_the_source(&mut self) {
        if self.source.seek(SeekFrom::Start(self.origin)).is_err() {
            tracing::debug!("a buffered prescan could not restore the stream position");
        }
    }

    fn unread(&self) -> &[u8] {
        let Some(past) = self.position.checked_sub(self.at) else {
            return &[];
        };
        let Ok(past) = usize::try_from(past) else {
            return &[];
        };
        self.held.get(past..self.filled).unwrap_or_default()
    }

    fn refill(&mut self) -> io::Result<()> {
        self.source.seek(SeekFrom::Start(self.position))?;

        let mut filled = 0;
        while filled < PRESCAN_WINDOW {
            let Some(room) = self.held.get_mut(filled..) else {
                break;
            };
            match self.source.read(room) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
                Err(source) => {
                    self.filled = 0;
                    self.at = self.position;
                    return Err(source);
                }
            }
        }

        self.filled = filled;
        self.at = self.position;
        Ok(())
    }

    fn straight_through(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.source.seek(SeekFrom::Start(self.position))?;
        let read = self.source.read(buf)?;
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }
}

impl<S: Read + Seek + ?Sized> Read for Window<'_, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.len() >= PRESCAN_WINDOW {
            return self.straight_through(buf);
        }
        if self.unread().is_empty() {
            self.refill()?;
        }

        let unread = self.unread();
        let taking = unread.len().min(buf.len());
        buf[..taking].copy_from_slice(&unread[..taking]);
        self.position = self.position.saturating_add(taking as u64);

        Ok(taking)
    }
}

impl<S: Read + Seek + ?Sized> Seek for Window<'_, S> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let landed = match to {
            SeekFrom::Start(at) => at,
            SeekFrom::Current(by) => self
                .position
                .checked_add_signed(by)
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?,
            SeekFrom::End(by) => self.source.seek(SeekFrom::End(by))?,
        };
        self.position = landed;

        Ok(landed)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.position)
    }
}

pub(crate) fn read_exact<const N: usize, S: Read + ?Sized>(source: &mut S) -> Option<[u8; N]> {
    let mut bytes = [0_u8; N];
    source.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

pub(crate) fn past_id3<S: Read + Seek + ?Sized>(source: &mut S) -> Option<u64> {
    let origin = source.stream_position().ok()?;
    let Some(header) = read_exact::<10, S>(source) else {
        return Some(origin);
    };
    if !header.starts_with(ID3) {
        return Some(origin);
    }

    let Some(size) = header.get(6..10).and_then(synchsafe) else {
        return Some(origin);
    };
    let footer = u64::from(header.get(5)? & ID3_FOOTER_FLAG != 0) * ID3_HEADER;
    Some(origin + ID3_HEADER + size + footer)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    Wave,
    Wide(Wide),
    Caf,
    Another,
}

pub(crate) fn opened_first<S: Read + Seek + ?Sized>(
    source: &mut S,
    start: u64,
) -> Option<(Opened, u64)> {
    source.seek(SeekFrom::Start(start)).ok()?;
    let mut held: Vec<u8> = Vec::with_capacity(SEARCHED_AT_ONCE + LOOKED_PAST);
    let mut searched: u64 = 0;
    let mut chunk = [0_u8; SEARCHED_AT_ONCE];
    loop {
        let read = filled(source, &mut chunk);
        held.extend_from_slice(chunk.get(..read)?);
        let ended = read < SEARCHED_AT_ONCE;
        let settled = if ended {
            held.len()
        } else {
            held.len().saturating_sub(LOOKED_PAST)
        };
        let within = usize::try_from(PROBED_WITHIN.saturating_sub(searched)).unwrap_or(usize::MAX);
        let found = (0..settled.min(within)).find_map(|at| {
            let opened = container_at(held.get(at..)?)?;
            Some((opened, at))
        });
        if let Some((opened, at)) = found {
            let at = searched + at as u64;
            if at > 0 {
                tracing::debug!(
                    junk = at,
                    ?opened,
                    "a container header was found behind bytes that are not one"
                );
            }
            return Some((opened, start + at));
        }
        searched += settled as u64;
        if ended || searched >= PROBED_WITHIN {
            return None;
        }
        held.drain(..settled);
    }
}

fn container_at(from: &[u8]) -> Option<Opened> {
    let marker = from.get(..4)?;
    if marker == RIFF {
        return (from.get(WAVE_AT..WAVE_AT + 4) == Some(WAVE.as_slice())).then_some(Opened::Wave);
    }
    if marker == CAFF {
        return Some(Opened::Caf);
    }
    if let Some(wide) = Wide::at(from) {
        return Some(Opened::Wide(wide));
    }
    if OTHER_CONTAINERS
        .iter()
        .any(|other| marker == other.as_slice())
        || from.get(MP4_TYPE_AT..MP4_TYPE_AT + 4) == Some(MP4_TYPE.as_slice())
        || mpeg_audio_at(from)
    {
        return Some(Opened::Another);
    }
    None
}

fn mpeg_audio_at(from: &[u8]) -> bool {
    if let Some(length) = mpeg_frame_length(from) {
        return from.get(length..).and_then(mpeg_frame_length).is_some();
    }
    if let Some(length) = adts_frame_length(from) {
        return from.get(length..).and_then(adts_frame_length).is_some();
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MpegVersion {
    One,
    Two,
    TwoAndAHalf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MpegLayer {
    One,
    Two,
    Three,
}

const BITRATES_MPEG_ONE_LAYER_ONE: [u32; 15] = [
    0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
];
const BITRATES_MPEG_ONE_LAYER_TWO: [u32; 15] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
const BITRATES_MPEG_ONE_LAYER_THREE: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const BITRATES_MPEG_TWO_LAYER_ONE: [u32; 15] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
];
const BITRATES_MPEG_TWO_LAYERS_TWO_AND_THREE: [u32; 15] =
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
const LAYER_TWO_REFUSED_IN_MONO_KBPS: [u32; 4] = [224, 256, 320, 384];
const LAYER_TWO_REFUSED_BESIDE_MONO_KBPS: [u32; 4] = [32, 48, 56, 80];
const MONO: u8 = 0b11;
const FREE_BITRATE: u8 = 0b0000;
const BAD_BITRATE: u8 = 0b1111;
const BITS_A_KILOBIT: u32 = 1_000;

fn mpeg_frame_length(header: &[u8]) -> Option<usize> {
    let [sync, flags, rates, mode, ..] = *header else {
        return None;
    };
    if sync != MPEG_SYNC || flags & MPEG_SYNC_HIGH != MPEG_SYNC_HIGH {
        return None;
    }
    let version = match (flags >> 3) & 0b11 {
        0b00 => MpegVersion::TwoAndAHalf,
        0b10 => MpegVersion::Two,
        0b11 => MpegVersion::One,
        _ => return None,
    };
    let layer = match (flags >> 1) & 0b11 {
        0b01 => MpegLayer::Three,
        0b10 => MpegLayer::Two,
        0b11 => MpegLayer::One,
        _ => return None,
    };
    let bitrate_index = rates >> 4;
    if bitrate_index == FREE_BITRATE || bitrate_index == BAD_BITRATE {
        return None;
    }
    let bitrates = match (version, layer) {
        (MpegVersion::One, MpegLayer::One) => &BITRATES_MPEG_ONE_LAYER_ONE,
        (MpegVersion::One, MpegLayer::Two) => &BITRATES_MPEG_ONE_LAYER_TWO,
        (MpegVersion::One, MpegLayer::Three) => &BITRATES_MPEG_ONE_LAYER_THREE,
        (_, MpegLayer::One) => &BITRATES_MPEG_TWO_LAYER_ONE,
        (_, MpegLayer::Two | MpegLayer::Three) => &BITRATES_MPEG_TWO_LAYERS_TWO_AND_THREE,
    };
    let kbps = *bitrates.get(usize::from(bitrate_index))?;
    let rate = match ((rates >> 2) & 0b11, version) {
        (0b00, MpegVersion::One) => 44_100,
        (0b01, MpegVersion::One) => 48_000,
        (0b10, MpegVersion::One) => 32_000,
        (0b00, MpegVersion::Two) => 22_050,
        (0b01, MpegVersion::Two) => 24_000,
        (0b10, MpegVersion::Two) => 16_000,
        (0b00, MpegVersion::TwoAndAHalf) => 11_025,
        (0b01, MpegVersion::TwoAndAHalf) => 12_000,
        (0b10, MpegVersion::TwoAndAHalf) => 8_000,
        _ => return None,
    };
    if layer == MpegLayer::Two {
        let refused = if mode >> 6 == MONO {
            &LAYER_TWO_REFUSED_IN_MONO_KBPS
        } else {
            &LAYER_TWO_REFUSED_BESIDE_MONO_KBPS
        };
        if refused.contains(&kbps) {
            return None;
        }
    }
    let (slots_a_frame, bytes_a_slot) = match (layer, version) {
        (MpegLayer::One, _) => (12, 4),
        (MpegLayer::Two, _) | (MpegLayer::Three, MpegVersion::One) => (144, 1),
        (MpegLayer::Three, _) => (72, 1),
    };
    let padding = u32::from((rates >> 1) & 1);
    let slots = slots_a_frame * kbps * BITS_A_KILOBIT / rate + padding;
    usize::try_from(slots * bytes_a_slot).ok()
}

const ADTS_SYNC_MASK: u8 = 0xf6;
const ADTS_SYNC: u8 = 0xf0;
const ADTS_SAMPLE_RATES: u8 = 13;
const ADTS_HEADER_BYTES: usize = 7;
const ADTS_CHECKED_HEADER_BYTES: usize = 9;

fn adts_frame_length(header: &[u8]) -> Option<usize> {
    let [
        sync,
        flags,
        profile,
        channels,
        length_high,
        length_low,
        blocks,
        ..,
    ] = *header
    else {
        return None;
    };
    if sync != MPEG_SYNC || flags & ADTS_SYNC_MASK != ADTS_SYNC {
        return None;
    }
    if (profile >> 2) & 0b1111 >= ADTS_SAMPLE_RATES {
        return None;
    }
    if blocks & 0b11 != 0 {
        return None;
    }
    let length = (usize::from(channels & 0b11) << 11)
        | (usize::from(length_high) << 3)
        | usize::from(length_low >> 5);
    let checked = flags & 1 == 0;
    let header_bytes = if checked {
        ADTS_CHECKED_HEADER_BYTES
    } else {
        ADTS_HEADER_BYTES
    };
    (length >= header_bytes).then_some(length)
}

fn filled<S: Read + ?Sized>(source: &mut S, buf: &mut [u8]) -> usize {
    let mut filled = 0;
    while let Some(room) = buf.get_mut(filled..).filter(|room| !room.is_empty()) {
        match source.read(room) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(source) if source.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    filled
}

fn synchsafe(bytes: &[u8]) -> Option<u64> {
    bytes.iter().try_fold(0_u64, |value, byte| {
        (byte & 0x80 == 0).then(|| (value << 7) | u64::from(*byte))
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct Counting {
        held: Cursor<Vec<u8>>,
        reads: usize,
        seeks: usize,
    }

    impl Counting {
        fn over(bytes: Vec<u8>) -> Self {
            Self {
                held: Cursor::new(bytes),
                reads: 0,
                seeks: 0,
            }
        }
    }

    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            self.held.read(buf)
        }
    }

    impl Seek for Counting {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.seeks += 1;
            self.held.seek(to)
        }
    }

    fn sawtooth(bytes: usize) -> Vec<u8> {
        (0..bytes).map(|at| (at % 251) as u8).collect()
    }

    fn walked<S: Read + Seek + ?Sized>(source: &mut S, over: &[(u64, usize)]) -> Vec<Vec<u8>> {
        over.iter()
            .map(|(at, bytes)| {
                source.seek(SeekFrom::Start(*at)).expect("a seek");
                let mut held = vec![0_u8; *bytes];
                let read = source.read(&mut held).expect("a read");
                held.truncate(read);
                held
            })
            .collect()
    }

    const A_WALK: [(u64, usize); 7] = [
        (0, 4),
        (4, 12),
        (9_000, 8),
        (9_016, 4),
        (12, 8),
        (20_000, 64),
        (19_000, PRESCAN_WINDOW + 16),
    ];

    #[test]
    fn a_window_reads_back_what_the_source_holds() {
        let bytes = sawtooth(24 * 1024);
        let mut plain = Cursor::new(bytes.clone());
        let mut counting = Counting::over(bytes);
        let mut window = Window::over(&mut counting, 0);

        assert_eq!(walked(&mut window, &A_WALK), walked(&mut plain, &A_WALK));
    }

    #[test]
    fn a_window_costs_fewer_reads_than_the_source_it_is_over() {
        let bytes = sawtooth(24 * 1024);
        let mut counting = Counting::over(bytes.clone());
        let mut window = Window::over(&mut counting, 0);
        let _ = walked(&mut window, &A_WALK);

        let mut bare = Counting::over(bytes);
        let _ = walked(&mut bare, &A_WALK);

        assert!(
            counting.reads < bare.reads,
            "a window took {} reads where the bare source took {}",
            counting.reads,
            bare.reads
        );
        assert!(counting.seeks < bare.seeks);
    }

    #[test]
    fn a_window_leaves_the_source_where_it_found_it() {
        let mut counting = Counting::over(sawtooth(4_096));
        counting.held.set_position(64);

        let mut window = Window::over(&mut counting, 64);
        let _ = walked(&mut window, &[(0, 16), (2_000, 16)]);
        window.rewind_the_source();

        assert_eq!(counting.held.position(), 64);
    }

    #[test]
    fn a_window_reads_the_same_prescan_the_source_does() {
        let mut flac = b"fLaC".to_vec();
        flac.extend_from_slice(&[0x80, 0, 0, 4]);
        flac.extend_from_slice(&[1, 2, 3, 4]);
        flac.extend_from_slice(&sawtooth(64 * 1024));

        let mut counting = Counting::over(flac.clone());
        assert_eq!(
            Prescan::buffered(&mut counting),
            Prescan::read(&mut Cursor::new(flac))
        );
    }

    fn frames(header: [u8; 4], length: usize, count: usize) -> Vec<u8> {
        let mut held = Vec::new();
        for _ in 0..count {
            held.extend_from_slice(&header);
            held.resize(held.len() + length - header.len(), 0);
        }
        held
    }

    fn adts(length: usize, count: usize) -> Vec<u8> {
        let header = [
            MPEG_SYNC,
            0xf1,
            0x50,
            0x80 | ((length >> 11) as u8 & 0b11),
            (length >> 3) as u8,
            ((length & 0b111) as u8) << 5 | 0x1f,
            0xfc,
        ];
        let mut held = Vec::new();
        for _ in 0..count {
            held.extend_from_slice(&header);
            held.resize(held.len() + length - header.len(), 0);
        }
        held
    }

    fn behind(stream: &[u8]) -> Vec<u8> {
        let mut file = stream.to_vec();
        file.extend_from_slice(b"RIFF\x24\x00\x00\x00WAVE");
        file
    }

    fn opened(file: Vec<u8>) -> Option<(Opened, u64)> {
        opened_first(&mut Cursor::new(file), 0)
    }

    #[test]
    fn two_frames_of_every_mpeg_layer_and_version_symphonia_reads_are_a_stream() {
        let streams = [
            ("MPEG-1 layer III", frames([0xff, 0xfb, 0x90, 0x00], 417, 2)),
            ("MPEG-2 layer III", frames([0xff, 0xf3, 0x80, 0xc0], 208, 2)),
            (
                "MPEG-2.5 layer III",
                frames([0xff, 0xe3, 0x88, 0x00], 576, 2),
            ),
            ("MPEG-1 layer II", frames([0xff, 0xfd, 0xc0, 0x00], 835, 2)),
            ("MPEG-1 layer I", frames([0xff, 0xff, 0x90, 0x00], 312, 2)),
            ("ADTS", adts(371, 2)),
        ];
        for (named, stream) in streams {
            assert_eq!(
                opened(behind(&stream)),
                Some((Opened::Another, 0)),
                "{named} was not read as the stream symphonia opens"
            );
        }
    }

    #[test]
    fn a_frame_symphonia_would_refuse_leaves_the_wave_behind_it_to_be_weighed() {
        let refused = [
            ("a lone frame", frames([0xff, 0xfb, 0x90, 0x00], 417, 1)),
            ("a free bitrate", frames([0xff, 0xfb, 0x00, 0x00], 417, 2)),
            (
                "layer II mono at 320",
                frames([0xff, 0xfd, 0xd0, 0xc0], 1_044, 2),
            ),
            (
                "layer II stereo at 32",
                frames([0xff, 0xfd, 0x10, 0x00], 104, 2),
            ),
            ("a reserved rate", frames([0xff, 0xfb, 0x9c, 0x00], 417, 2)),
        ];
        for (named, junk) in refused {
            let at = junk.len() as u64;
            assert_eq!(
                opened(behind(&junk)),
                Some((Opened::Wave, at)),
                "{named} was taken for a stream"
            );
        }
    }
}
