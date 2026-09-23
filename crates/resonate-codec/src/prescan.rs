use std::io::{self, Read, Seek, SeekFrom};

use resonate_core::{Frames, SampleRate};

use crate::{
    boxes::{self, Movie},
    flac::{self, Flac},
    matroska::{self, Segment},
    riff::{self, Riff},
};

const PRESCAN_WINDOW: usize = 4 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Prescan {
    pub(crate) riff: Riff,
    pub(crate) segment: Segment,
    pub(crate) boxes: Movie,
    pub(crate) flac: Flac,
}

impl Prescan {
    pub(crate) fn read<S: Read + Seek + ?Sized>(source: &mut S) -> Self {
        Self {
            riff: riff::read(source),
            segment: matroska::read_segment(source),
            boxes: boxes::read_movie(source),
            flac: flac::read(source),
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
