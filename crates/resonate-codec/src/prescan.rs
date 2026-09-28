use std::io::{self, ErrorKind, Read, Seek, SeekFrom};

use resonate_core::{Frames, SampleRate};

use crate::{
    boxes::{self, Movie},
    caf::{self, Overflow},
    flac::{self, Flac},
    matroska::{self, Segment},
    riff::{self, Riff},
    wavpack::{self, Coding},
};

const PRESCAN_WINDOW: usize = 4 * 1024;
pub(crate) const PROBED_WITHIN: u64 = 1024 * 1024;
const SEARCHED_AT_ONCE: usize = 16 * 1024;
const LOOKED_PAST: usize = 2 * 1024;
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
    if OTHER_CONTAINERS
        .iter()
        .any(|other| marker == other.as_slice())
        || from.get(MP4_TYPE_AT..MP4_TYPE_AT + 4) == Some(MP4_TYPE.as_slice())
        || mpeg_frames_at(from)
    {
        return Some(Opened::Another);
    }
    None
}

fn mpeg_frames_at(from: &[u8]) -> bool {
    let Some(length) = mpeg_frame_length(from) else {
        return false;
    };
    from.get(length..).and_then(mpeg_frame_length).is_some()
}

fn mpeg_frame_length(header: &[u8]) -> Option<usize> {
    const BITRATES_KBPS: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const RATES_HZ: [u32; 3] = [44_100, 48_000, 32_000];
    const LAYER_THREE: u8 = 0b01;
    const MPEG_ONE: u8 = 0b11;
    const SAMPLES_A_FRAME_OVER_BITS: u32 = 144;
    const BITS_A_KILOBIT: u32 = 1_000;

    let [sync, flags, rates, ..] = *header else {
        return None;
    };
    if sync != MPEG_SYNC || flags & MPEG_SYNC_HIGH != MPEG_SYNC_HIGH {
        return None;
    }
    let version = (flags >> 3) & 0b11;
    let layer = (flags >> 1) & 0b11;
    if version != MPEG_ONE || layer != LAYER_THREE {
        return None;
    }
    let bitrate = *BITRATES_KBPS.get(usize::from(rates >> 4))?;
    let rate = *RATES_HZ.get(usize::from((rates >> 2) & 0b11))?;
    if bitrate == 0 {
        return None;
    }
    let padding = u32::from((rates >> 1) & 1);
    usize::try_from(SAMPLES_A_FRAME_OVER_BITS * bitrate * BITS_A_KILOBIT / rate + padding).ok()
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
}
