use std::{
    fs::File,
    io::{BufWriter, ErrorKind, Read, Write},
    path::Path,
};

use resonate_codec::{DecodeStatus, Decoder, Speakers};
use resonate_core::{AudioBuffer, ChannelCount, Frames, SampleFormat, StreamSpec};

use crate::{
    Halt,
    error::{Error, Result, VaultOp},
    key::VaultKey,
    pcm::{self, Digest},
};

const PLAIN_FORMAT_BYTES: u32 = 16;
const EXTENSIBLE_FORMAT_BYTES: u32 = 40;
const EXTENSION_BYTES: u16 = 22;
const RIFF_AND_FORMAT_HEADERS: usize = 20;
const DATA_HEADER: usize = 8;
const WIDEST_HEADER: usize =
    RIFF_AND_FORMAT_HEADERS + EXTENSIBLE_FORMAT_BYTES as usize + DATA_HEADER;
const PCM_INTEGER: u16 = 1;
const IEEE_FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xFFFE;
const SUBFORMAT_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];
const WAVE_BITS: u16 = 32;
const PACKED_A_READ_AT_A_TIME: usize = 1 << 20;
const RIFF_HEADER: u32 = 8;

pub(crate) const LARGEST_PCM: u64 = u32::MAX as u64 - WIDEST_HEADER as u64;

pub(crate) fn outgrows_a_wave(frames: Frames, channels: ChannelCount) -> bool {
    let sample_bytes = u128::from(WAVE_BITS / 8);
    u128::from(frames.get()) * u128::from(channels.get()) * sample_bytes > u128::from(LARGEST_PCM)
}
pub(crate) const ARCHIVED_AT: i32 = 19;
pub(crate) const PACKED_FRAME_BYTES: u64 = 8 << 20;
const FORETOLD_FROM_BYTES: u64 = 32 << 20;
const SLICES_HELD_AT_MOST: usize = 8;
const SLICE_BYTES: usize = 2 << 20;
const FORETOLD_HOPELESS_PAST: u64 = 8;

pub(crate) struct Tallied {
    pub(crate) key: VaultKey,
    pub(crate) frames: Frames,
    pub(crate) pcm_bytes: u64,
    samples: Samples,
}

pub(crate) fn tally(decoder: &mut Decoder, spec: StreamSpec, halt: Halt<'_>) -> Result<Tallied> {
    let mut block = AudioBuffer::empty(spec);
    let mut bytes = Vec::new();
    let mut digest = Digest::default();
    let mut samples = Samples::default();
    let mut frames = 0_u64;
    let mut pcm_bytes = 0_u64;

    loop {
        halt.heard()?;
        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(VaultOp::Read, source))?;
        if status == DecodeStatus::EndOfStream {
            break;
        }

        pcm::little_endian(&block, &mut bytes);
        digest.note(&bytes);
        samples.note(pcm_bytes, &bytes);
        frames += block.frames() as u64;
        pcm_bytes += bytes.len() as u64;

        if pcm_bytes > LARGEST_PCM {
            break;
        }
    }

    Ok(Tallied {
        key: digest.settled(),
        frames: Frames(frames),
        pcm_bytes,
        samples,
    })
}

struct Samples {
    stride: u64,
    held: Vec<(u64, Vec<u8>)>,
}

impl Default for Samples {
    fn default() -> Self {
        Self {
            stride: SLICE_BYTES as u64,
            held: Vec::new(),
        }
    }
}

impl Samples {
    fn note(&mut self, mut at: u64, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let window = at / self.stride;
            let within = at % self.stride;
            let skipped = if within >= SLICE_BYTES as u64 {
                self.stride - within
            } else {
                match self.held.last() {
                    Some((last, _)) if *last == window => 0,
                    _ if self.held.len() >= SLICES_HELD_AT_MOST => {
                        self.thinned();
                        continue;
                    }
                    _ => {
                        self.held.push((window, Vec::with_capacity(SLICE_BYTES)));
                        0
                    }
                }
            };
            if skipped > 0 {
                let skipped = usize::try_from(skipped)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                at += skipped as u64;
                bytes = &bytes[skipped..];
                continue;
            }
            let Some((_, slice)) = self.held.last_mut() else {
                return;
            };
            let room = SLICE_BYTES - within as usize;
            let taken = room.min(bytes.len());
            if slice.len() as u64 == within {
                slice.extend_from_slice(&bytes[..taken]);
            }
            at += taken as u64;
            bytes = &bytes[taken..];
        }
    }

    fn thinned(&mut self) {
        self.stride *= 2;
        self.held.retain(|(window, _)| window % 2 == 0);
        for (window, _) in &mut self.held {
            *window /= 2;
        }
    }

    fn whole_slices(&self) -> impl Iterator<Item = &[u8]> {
        self.held
            .iter()
            .map(|(_, slice)| slice.as_slice())
            .filter(|slice| slice.len() == SLICE_BYTES)
    }
}

pub(crate) fn hopeless(tallied: &Tallied, smaller_than: u64) -> bool {
    let length = tallied.pcm_bytes;
    if length < FORETOLD_FROM_BYTES {
        return false;
    }

    let mut sampled = 0_u64;
    let mut packed = 0_u64;
    for slice in tallied.samples.whole_slices() {
        let Ok(squeezed) = zstd::bulk::compress(slice, ARCHIVED_AT) else {
            return false;
        };
        sampled += SLICE_BYTES as u64;
        packed += squeezed.len() as u64;
    }
    if sampled == 0 {
        return false;
    }

    let foretold = u128::from(packed) * u128::from(length) / u128::from(sampled);
    let beyond_doubt = u128::from(smaller_than) + u128::from(smaller_than / FORETOLD_HOPELESS_PAST);
    foretold > beyond_doubt
}

struct Pcm<'a> {
    decoder: &'a mut Decoder,
    block: AudioBuffer,
    pending: Vec<u8>,
    taken: usize,
    seen: u64,
    tallied: u64,
    drained: bool,
    differed: bool,
    failure: Option<Error>,
}

impl Pcm<'_> {
    fn refill(&mut self) -> std::io::Result<()> {
        self.pending.clear();
        self.taken = 0;
        let status = match self.decoder.next_block(&mut self.block) {
            Ok(status) => status,
            Err(source) => {
                self.failure = Some(Error::codec(VaultOp::Read, source));
                return Err(ErrorKind::Other.into());
            }
        };
        if status == DecodeStatus::EndOfStream {
            self.drained = true;
            if self.seen != self.tallied {
                self.differed = true;
                return Err(ErrorKind::UnexpectedEof.into());
            }
            if self.tallied % 2 == 1 {
                self.pending.push(0);
            }
            return Ok(());
        }

        pcm::little_endian(&self.block, &mut self.pending);
        self.seen += self.pending.len() as u64;
        if self.seen > self.tallied {
            self.differed = true;
            return Err(ErrorKind::InvalidData.into());
        }
        Ok(())
    }
}

impl Read for Pcm<'_> {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        while self.taken == self.pending.len() {
            if self.drained {
                return Ok(0);
            }
            self.refill()?;
        }
        let taken = into.len().min(self.pending.len() - self.taken);
        into[..taken].copy_from_slice(&self.pending[self.taken..self.taken + taken]);
        self.taken += taken;
        Ok(taken)
    }
}

pub(crate) struct Packing<'a> {
    pub(crate) spec: StreamSpec,
    pub(crate) speakers: Speakers,
    pub(crate) tallied: &'a Tallied,
    pub(crate) into: &'a Path,
    pub(crate) smaller_than: Option<u64>,
}

pub(crate) fn pack(decoder: &mut Decoder, packing: Packing<'_>, halt: Halt<'_>) -> Result<Packed> {
    let Packing {
        spec,
        speakers,
        tallied,
        into,
        smaller_than,
    } = packing;
    let declared = u32::try_from(tallied.pcm_bytes).unwrap_or(u32::MAX);
    let header = header(spec, speakers, declared);
    let length = header.len() as u64 + tallied.pcm_bytes + tallied.pcm_bytes % 2;

    let mut pcm = Pcm {
        decoder,
        block: AudioBuffer::empty(spec),
        pending: header,
        taken: 0,
        seen: 0,
        tallied: tallied.pcm_bytes,
        drained: false,
        differed: false,
        failure: None,
    };
    let packed = packed_in_frames(
        Unpacked {
            reading: &mut pcm,
            length,
            from: into,
        },
        into,
        smaller_than,
        PACKED_FRAME_BYTES,
        halt,
    );

    if pcm.differed {
        return Ok(Packed::Differed);
    }
    match pcm.failure.take() {
        Some(failure) => Err(failure),
        None => packed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Packed {
    Within,
    NoSmaller,
    Differed,
}

struct Counted<W> {
    inner: W,
    written: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let wrote = self.inner.write(bytes)?;
        self.written += wrote as u64;
        Ok(wrote)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
pub(crate) fn compressed_in_frames(
    from: &Path,
    into: &Path,
    smaller_than: Option<u64>,
    frame_bytes: u64,
    halt: Halt<'_>,
) -> Result<Packed> {
    let read = |source| Error::io(VaultOp::Read, from, source);
    let source = File::open(from).map_err(read)?;
    let length = source.metadata().map_err(read)?.len();
    packed_in_frames(
        Unpacked {
            reading: std::io::BufReader::new(source),
            length,
            from,
        },
        into,
        smaller_than,
        frame_bytes,
        halt,
    )
}

pub(crate) struct Unpacked<'a, R> {
    pub(crate) reading: R,
    pub(crate) length: u64,
    pub(crate) from: &'a Path,
}

pub(crate) fn packed_in_frames<R: Read>(
    unpacked: Unpacked<'_, R>,
    into: &Path,
    smaller_than: Option<u64>,
    frame_bytes: u64,
    halt: Halt<'_>,
) -> Result<Packed> {
    let Unpacked {
        mut reading,
        length,
        from,
    } = unpacked;
    let written = |source| Error::io(VaultOp::Write, into, source);
    let read = |source| Error::io(VaultOp::Read, from, source);
    let target = File::create(into).map_err(|source| Error::io(VaultOp::Stage, into, source))?;
    let mut counted = Counted {
        inner: BufWriter::new(target),
        written: 0,
    };
    let outgrown =
        |counted: &Counted<_>| smaller_than.is_some_and(|ceiling| counted.written >= ceiling);

    let mut buffer = vec![0_u8; PACKED_A_READ_AT_A_TIME];
    let mut left = length;
    while left > 0 {
        let framed = left.min(frame_bytes);
        let mut encoder = zstd::Encoder::new(&mut counted, ARCHIVED_AT).map_err(written)?;
        encoder
            .set_pledged_src_size(Some(framed))
            .map_err(written)?;
        let mut owed = framed;
        while owed > 0 {
            halt.heard()?;
            let taken = usize::try_from(owed.min(buffer.len() as u64)).unwrap_or(buffer.len());
            reading.read_exact(&mut buffer[..taken]).map_err(read)?;
            encoder.write_all(&buffer[..taken]).map_err(written)?;
            owed -= taken as u64;
            if outgrown(encoder.get_ref()) {
                return Ok(Packed::NoSmaller);
            }
        }
        encoder.finish().map_err(written)?;
        left -= framed;
    }

    if outgrown(&counted) {
        return Ok(Packed::NoSmaller);
    }
    let file = counted
        .inner
        .into_inner()
        .map_err(|source| written(source.into_error()))?;
    file.sync_all()
        .map_err(|source| Error::io(VaultOp::Settle, into, source))?;
    Ok(Packed::Within)
}

fn header(spec: StreamSpec, speakers: Speakers, data_bytes: u32) -> Vec<u8> {
    let count = spec.channel_count().get();
    let channels = u16::from(count);
    let rate = spec.rate.hz();
    let block_align = channels * (WAVE_BITS / 8);
    let byte_rate = rate * u32::from(block_align);
    let tag = match spec.format {
        SampleFormat::F32 => IEEE_FLOAT,
        SampleFormat::S16 | SampleFormat::S24 | SampleFormat::S32 => PCM_INTEGER,
    };
    let masked = speakers
        .wave_mask()
        .filter(|_| speakers.is_named() && Speakers::in_wave_order(count) != Some(speakers));
    let format_bytes = if masked.is_some() {
        EXTENSIBLE_FORMAT_BYTES
    } else {
        PLAIN_FORMAT_BYTES
    };

    let mut bytes = Vec::with_capacity(WIDEST_HEADER);
    bytes.extend_from_slice(b"RIFF");
    let riff = data_bytes + RIFF_AND_FORMAT_HEADERS as u32 - RIFF_HEADER
        + format_bytes
        + DATA_HEADER as u32;
    bytes.extend_from_slice(&riff.to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&format_bytes.to_le_bytes());
    bytes.extend_from_slice(&masked.map_or(tag, |_| EXTENSIBLE).to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&WAVE_BITS.to_le_bytes());
    if let Some(mask) = masked {
        bytes.extend_from_slice(&EXTENSION_BYTES.to_le_bytes());
        bytes.extend_from_slice(&WAVE_BITS.to_le_bytes());
        bytes.extend_from_slice(&mask.to_le_bytes());
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&SUBFORMAT_TAIL);
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use super::*;

    #[test]
    fn a_compression_stops_once_it_has_written_what_it_had_to_beat() {
        const HELD: usize = 4 << 20;
        const CEILING: u64 = 64 << 10;

        let folder = env::temp_dir().join(format!("resonate-wave-ceiling-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        let from = folder.join("noise.wav");
        let into = folder.join("noise.wav.zst");
        let mut state = 0x2545_f491_u32;
        let noise: Vec<u8> = (0..HELD)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        fs::write(&from, &noise).expect("a noisy file");

        let packed =
            compressed_in_frames(&from, &into, Some(CEILING), PACKED_FRAME_BYTES, Halt::NEVER)
                .expect("a compression");

        assert_eq!(packed, Packed::NoSmaller);
        let written = fs::metadata(&into).map_or(0, |held| held.len());
        assert!(
            written < HELD as u64 / 4,
            "the whole of the file was compressed before it was given up on: {written} bytes"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    fn tallied_over(bytes: &[u8]) -> Tallied {
        let mut samples = Samples::default();
        for (at, piece) in bytes.chunks(4096 * 8).enumerate() {
            samples.note((at * 4096 * 8) as u64, piece);
        }
        Tallied {
            key: VaultKey::of([0; 16]),
            frames: Frames(0),
            pcm_bytes: bytes.len() as u64,
            samples,
        }
    }

    fn noise(len: usize, mut state: u32) -> Vec<u8> {
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    #[test]
    fn a_compression_foretold_to_lose_by_its_slices_is_hopeless_and_one_that_may_win_is_not() {
        const HELD: usize = 40 << 20;

        let noisy = tallied_over(&noise(HELD, 0x9e37_79b9));
        let hushed = tallied_over(&vec![0_u8; HELD]);
        let small = tallied_over(&noise(SLICE_BYTES, 0x9e37_79b9));

        assert!(hopeless(&noisy, HELD as u64 / 2));
        assert!(!hopeless(&noisy, HELD as u64 * 2));
        assert!(!hopeless(&hushed, HELD as u64 / 2));
        assert!(!hopeless(&small, 1));
    }

    #[test]
    fn the_slices_a_stream_is_foretold_by_are_spread_over_all_of_it_whatever_its_length() {
        const WINDOWS: usize = 40;

        let mut pcm = Vec::new();
        for window in 0..WINDOWS {
            pcm.extend(std::iter::repeat_n(window as u8, SLICE_BYTES));
        }
        let tallied = tallied_over(&pcm);

        let firsts: Vec<u8> = tallied
            .samples
            .whole_slices()
            .map(|slice| slice[0])
            .collect();

        assert!(firsts.len() >= SLICES_HELD_AT_MOST / 2, "{firsts:?}");
        assert!(firsts.len() <= SLICES_HELD_AT_MOST, "{firsts:?}");
        assert_eq!(firsts[0], 0);
        assert!(
            firsts
                .last()
                .is_some_and(|last| usize::from(*last) >= WINDOWS / 2),
            "{firsts:?}"
        );
        assert!(
            tallied
                .samples
                .whole_slices()
                .all(|slice| slice.iter().all(|byte| *byte == slice[0]))
        );
    }
}
