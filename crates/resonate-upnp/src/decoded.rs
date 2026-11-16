use std::{
    error::Error as StdError,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
};

use resonate_core::{AudioBuffer, Frames, StreamSpec};

use crate::{Wrapping, wave};

const SILENCE_HANDED_AT_ONCE: u64 = 4_096;

pub trait Pcm: Send {
    type Failure: StdError + Send + Sync + 'static;

    fn seek(&mut self, to: Frames) -> Result<Frames, Self::Failure>;

    fn next_block(&mut self) -> Result<Option<&AudioBuffer>, Self::Failure>;
}

pub fn waves_alike(one: StreamSpec, other: StreamSpec) -> bool {
    wave::header(one) == wave::header(other)
}

pub fn wave_bytes(spec: StreamSpec, frames: Frames) -> u64 {
    wave::header_bytes(spec) as u64 + data_bytes(spec, frames)
}

fn data_bytes(spec: StreamSpec, frames: Frames) -> u64 {
    frames.get().saturating_mul(wave::block_bytes(spec) as u64)
}

pub struct AsWave<P> {
    pcm: P,
    spec: StreamSpec,
    header: Vec<u8>,
    block: u64,
    frames: u64,
    at: u64,
    held: Vec<u8>,
    held_from: u64,
    next: u64,
    exhausted: bool,
    decoded_until: Option<u64>,
}

impl<P: Pcm> AsWave<P> {
    pub fn over(pcm: P, spec: StreamSpec, frames: Frames) -> Self {
        Self {
            pcm,
            spec,
            header: wave::header_holding(spec, Some(data_bytes(spec, frames))),
            block: wave::block_bytes(spec) as u64,
            frames: frames.get(),
            at: 0,
            held: Vec::new(),
            held_from: 0,
            next: 0,
            exhausted: false,
            decoded_until: None,
        }
    }

    fn length(&self) -> u64 {
        self.header.len() as u64 + self.frames.saturating_mul(self.block)
    }

    fn held(&self) -> Range<u64> {
        self.held_from..self.held_from + self.held.len() as u64
    }

    fn sought(&mut self, frame: u64) -> io::Result<()> {
        let landed = self
            .pcm
            .seek(Frames(frame))
            .map_err(io::Error::other)?
            .get();
        if landed > frame {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.next = landed;
        self.exhausted = false;
        Ok(())
    }

    fn fill(&mut self, frame: u64) -> io::Result<()> {
        if self.decoded_until.is_some_and(|end| frame >= end) {
            self.silence_from(frame);
            return Ok(());
        }
        if frame != self.next || self.exhausted {
            self.sought(frame)?;
        }
        loop {
            let Some(decoded) = self.pcm.next_block().map_err(io::Error::other)? else {
                self.exhausted = true;
                self.decoded_until = Some(self.next);
                self.silence_from(frame);
                return Ok(());
            };
            let shape = decoded.spec();
            if shape.format != self.spec.format
                || shape.channel_count() != self.spec.channel_count()
            {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let first = self.next;
            self.next = first.saturating_add(decoded.frames() as u64);
            if self.next <= frame {
                continue;
            }
            wave::carried(
                decoded.as_bytes(),
                self.spec.format,
                Wrapping::Wave,
                &mut self.held,
            );
            let passed = usize::try_from((frame - first) * self.block).unwrap_or(usize::MAX);
            self.held.drain(..passed.min(self.held.len()));
            self.held_from = frame * self.block;
            return Ok(());
        }
    }

    fn silence_from(&mut self, frame: u64) {
        let silent = self
            .frames
            .saturating_sub(frame)
            .clamp(1, SILENCE_HANDED_AT_ONCE);
        self.held.clear();
        self.held
            .resize(usize::try_from(silent * self.block).unwrap_or(0), 0);
        self.held_from = frame * self.block;
    }
}

impl<P: Pcm> Read for AsWave<P> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let length = self.length();
        if into.is_empty() || self.at >= length {
            return Ok(0);
        }
        let left = usize::try_from(length - self.at).unwrap_or(usize::MAX);
        let header = self.header.len() as u64;

        let carried = if self.at < header {
            let from = self.at as usize;
            let carried = into.len().min(self.header.len() - from);
            into[..carried].copy_from_slice(&self.header[from..from + carried]);
            carried
        } else {
            let within = self.at - header;
            if !self.held().contains(&within) {
                self.fill(within / self.block)?;
            }
            let from = usize::try_from(within - self.held_from).unwrap_or(usize::MAX);
            let carried = into.len().min(self.held.len() - from).min(left);
            into[..carried].copy_from_slice(&self.held[from..from + carried]);
            carried
        };
        self.at += carried as u64;
        Ok(carried)
    }
}

impl<P: Pcm> Seek for AsWave<P> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.length().checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        self.at = at.ok_or(io::ErrorKind::InvalidInput)?;
        Ok(self.at)
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, fmt};

    use resonate_core::{ChannelLayout, SampleData, SampleFormat, SampleRate};

    use super::*;

    const BLOCK_FRAMES: usize = 300;

    const SPEC: StreamSpec = StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S24,
    );

    struct Counting {
        lasts: u64,
        next: u64,
        block: AudioBuffer,
        seeks: Vec<u64>,
    }

    #[derive(Debug)]
    struct Unseekable;

    impl fmt::Display for Unseekable {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("past the end")
        }
    }

    impl StdError for Unseekable {}

    fn sample(frame: u64, channel: u64) -> i32 {
        let value = (frame * 2 + channel) as i32 * 97;
        if channel == 1 { -value } else { value }
    }

    impl Counting {
        fn lasting(frames: u64) -> Self {
            Self {
                lasts: frames,
                next: 0,
                block: AudioBuffer::empty(SPEC),
                seeks: Vec::new(),
            }
        }
    }

    impl Pcm for Counting {
        type Failure = Unseekable;

        fn seek(&mut self, to: Frames) -> Result<Frames, Unseekable> {
            if to.get() > self.lasts {
                return Err(Unseekable);
            }
            self.seeks.push(to.get());
            self.next = to.get();
            Ok(to)
        }

        fn next_block(&mut self) -> Result<Option<&AudioBuffer>, Unseekable> {
            let frames = self
                .lasts
                .saturating_sub(self.next)
                .min(BLOCK_FRAMES as u64);
            if frames == 0 {
                return Ok(None);
            }
            self.block.set_frames(frames as usize);
            let SampleData::S24(samples) = self.block.data_mut() else {
                panic!("not twenty-four bits");
            };
            for (at, sample_at) in samples.iter_mut().enumerate() {
                let at = at as u64;
                *sample_at = sample(self.next + at / 2, at % 2);
            }
            self.next += frames;
            Ok(Some(&self.block))
        }
    }

    struct Never;

    impl Pcm for Never {
        type Failure = Infallible;

        fn seek(&mut self, to: Frames) -> Result<Frames, Infallible> {
            Ok(to)
        }

        fn next_block(&mut self) -> Result<Option<&AudioBuffer>, Infallible> {
            Ok(None)
        }
    }

    fn packed(frames: Range<u64>) -> Vec<u8> {
        frames
            .flat_map(|frame| [sample(frame, 0), sample(frame, 1)])
            .flat_map(|sample| {
                let [low, middle, high, _] = sample.to_le_bytes();
                [low, middle, high]
            })
            .collect()
    }

    fn whole<R: Read>(mut read: R) -> Vec<u8> {
        let mut bytes = Vec::new();
        read.read_to_end(&mut bytes).expect("the body");
        bytes
    }

    #[test]
    fn a_body_is_a_wave_of_the_frames_decoded_and_of_the_length_it_names() {
        let frames = Frames(1_000);
        let mut body = AsWave::over(Counting::lasting(1_000), SPEC, frames);

        let length = body.seek(SeekFrom::End(0)).expect("the length");
        body.seek(SeekFrom::Start(0)).expect("the start");
        let bytes = whole(&mut body);

        assert_eq!(length, wave_bytes(SPEC, frames));
        assert_eq!(length, 44 + 1_000 * 6);
        assert_eq!(bytes.len() as u64, length);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            (length - 8) as u32
        );
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
            6_000
        );
        assert_eq!(bytes[44..], packed(0..1_000));
    }

    #[test]
    fn a_range_into_the_middle_of_a_frame_seeks_to_that_frame_and_reads_on_in_order() {
        let mut body = AsWave::over(Counting::lasting(1_000), SPEC, Frames(1_000));
        let from = 44 + 437 * 6 + 4;

        body.seek(SeekFrom::Start(from)).expect("the range");
        let mut part = vec![0; 6 * 400];
        body.read_exact(&mut part).expect("the range read");
        let rest = whole(&mut body);
        body.seek(SeekFrom::Start(10))
            .expect("back into the header");
        let mut header_and_more = vec![0; 40];
        body.read_exact(&mut header_and_more)
            .expect("across the header");

        assert_eq!(part, packed(437..1_000)[4..4 + 6 * 400]);
        assert_eq!(rest, packed(437..1_000)[4 + 6 * 400..]);
        assert_eq!(body.pcm.seeks, [437, 0]);
        assert_eq!(&header_and_more[26..30], b"data");
        assert_eq!(header_and_more[34..], packed(0..1)[..]);
    }

    #[test]
    fn a_decode_ending_early_is_carried_on_as_silence_to_the_length_named() {
        let mut body = AsWave::over(Counting::lasting(500), SPEC, Frames(800));
        let bytes = whole(&mut body);

        body.seek(SeekFrom::Start(44 + 700 * 6))
            .expect("past the decode");
        let tail = whole(&mut body);

        assert_eq!(bytes.len(), 44 + 800 * 6);
        assert_eq!(bytes[44..44 + 500 * 6], packed(0..500));
        assert!(bytes[44 + 500 * 6..].iter().all(|byte| *byte == 0));
        assert_eq!(tail, vec![0; 100 * 6]);
    }

    #[test]
    fn a_decode_running_past_the_length_named_is_cut_at_it() {
        let bytes = whole(AsWave::over(Counting::lasting(900), SPEC, Frames(250)));

        assert_eq!(bytes.len(), 44 + 250 * 6);
        assert_eq!(bytes[44..], packed(0..250));
    }

    #[test]
    fn float_and_surround_are_named_in_the_header_as_what_they_are() {
        let float = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::F32,
        );
        let surround = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S16,
        );

        let floating = whole(AsWave::over(Never, float, Frames(3)));
        let six = whole(AsWave::over(Never, surround, Frames(3)));

        assert_eq!(u16::from_le_bytes([floating[20], floating[21]]), 3);
        assert_eq!(u16::from_le_bytes([floating[34], floating[35]]), 32);
        assert_eq!(floating.len(), 44 + 3 * 8);
        assert_eq!(u16::from_le_bytes([six[20], six[21]]), 0xFFFE);
        assert_eq!(six[44], 0x01);
        assert_eq!(six.len() as u64, wave_bytes(surround, Frames(3)));
        assert_eq!(six.len(), 68 + 3 * 12);
    }

    #[test]
    fn specs_carried_alike_are_those_whose_wave_reads_the_same() {
        let stereo = SPEC;
        let unnamed = StreamSpec {
            channels: ChannelLayout::Discrete(ChannelLayout::Stereo.count()),
            ..SPEC
        };

        assert!(waves_alike(stereo, unnamed));
        assert!(!waves_alike(
            stereo,
            StreamSpec {
                rate: SampleRate::HZ_48000,
                ..SPEC
            }
        ));
        assert!(!waves_alike(
            stereo,
            StreamSpec {
                format: SampleFormat::S16,
                ..SPEC
            }
        ));
        assert!(!waves_alike(
            stereo,
            StreamSpec {
                channels: ChannelLayout::Mono,
                ..SPEC
            }
        ));
    }
}
