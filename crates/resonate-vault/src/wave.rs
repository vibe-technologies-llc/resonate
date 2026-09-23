use std::{
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

use resonate_codec::{DecodeStatus, Decoder, Speakers};
use resonate_core::{AudioBuffer, Frames, SampleFormat, StreamSpec};

use crate::{
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
const RIFF_SIZE_AT: u64 = 4;
const RIFF_HEADER: u32 = 8;

pub(crate) const LARGEST_PCM: u64 = u32::MAX as u64 - WIDEST_HEADER as u64;
pub(crate) const ARCHIVED_AT: i32 = 19;

pub(crate) struct Written {
    pub(crate) key: VaultKey,
    pub(crate) frames: Frames,
    pub(crate) pcm_bytes: u64,
}

pub(crate) fn write(
    decoder: &mut Decoder,
    spec: StreamSpec,
    speakers: Speakers,
    into: &Path,
) -> Result<Written> {
    let file = File::create(into).map_err(|source| Error::io(VaultOp::Stage, into, source))?;
    let mut writer = BufWriter::new(file);

    let header = header(spec, speakers, 0);
    writer
        .write_all(&header)
        .map_err(|source| Error::io(VaultOp::Write, into, source))?;

    let mut block = AudioBuffer::empty(spec);
    let mut bytes = Vec::new();
    let mut digest = Digest::default();
    let mut frames = 0_u64;
    let mut pcm_bytes = 0_u64;

    loop {
        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(VaultOp::Read, source))?;
        if status == DecodeStatus::EndOfStream {
            break;
        }

        pcm::little_endian(&block, &mut bytes);
        digest.note(&bytes);
        writer
            .write_all(&bytes)
            .map_err(|source| Error::io(VaultOp::Write, into, source))?;
        frames += block.frames() as u64;
        pcm_bytes += bytes.len() as u64;

        if pcm_bytes > LARGEST_PCM {
            return Ok(Written {
                key: digest.settled(),
                frames: Frames(frames),
                pcm_bytes,
            });
        }
    }

    if pcm_bytes % 2 == 1 {
        writer
            .write_all(&[0])
            .map_err(|source| Error::io(VaultOp::Write, into, source))?;
    }

    let mut file = writer
        .into_inner()
        .map_err(|source| Error::io(VaultOp::Write, into, source.into_error()))?;
    let declared = u32::try_from(pcm_bytes).unwrap_or(u32::MAX);
    let riff = declared.saturating_add(header.len() as u32 - RIFF_HEADER);
    let data_size_at = (header.len() - size_of::<u32>()) as u64;

    patched(&mut file, into, RIFF_SIZE_AT, riff)?;
    patched(&mut file, into, data_size_at, declared)?;
    file.sync_all()
        .map_err(|source| Error::io(VaultOp::Settle, into, source))?;

    Ok(Written {
        key: digest.settled(),
        frames: Frames(frames),
        pcm_bytes,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Packed {
    Within,
    NoSmaller,
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

pub(crate) fn compressed(from: &Path, into: &Path, smaller_than: Option<u64>) -> Result<Packed> {
    let written = |source| Error::io(VaultOp::Write, into, source);
    let source = File::open(from).map_err(|source| Error::io(VaultOp::Read, from, source))?;
    let target = File::create(into).map_err(|source| Error::io(VaultOp::Stage, into, source))?;
    let counted = Counted {
        inner: BufWriter::new(target),
        written: 0,
    };
    let mut encoder = zstd::Encoder::new(counted, ARCHIVED_AT).map_err(written)?;

    let mut reading = std::io::BufReader::new(source);
    let mut buffer = vec![0_u8; PACKED_A_READ_AT_A_TIME];
    loop {
        let read = reading
            .read(&mut buffer)
            .map_err(|source| Error::io(VaultOp::Read, from, source))?;
        if read == 0 {
            break;
        }
        encoder.write_all(&buffer[..read]).map_err(written)?;
        if smaller_than.is_some_and(|ceiling| encoder.get_ref().written >= ceiling) {
            return Ok(Packed::NoSmaller);
        }
    }

    let counted = encoder.finish().map_err(written)?;
    if smaller_than.is_some_and(|ceiling| counted.written >= ceiling) {
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

pub(crate) fn unpacked(from: &Path) -> std::io::Result<Vec<u8>> {
    let file = File::open(from)?;
    zstd::decode_all(std::io::BufReader::new(file))
}

pub(crate) fn decompressed(from: &Path) -> Result<Vec<u8>> {
    unpacked(from).map_err(|source| Error::io(VaultOp::Read, from, source))
}

fn patched(file: &mut File, path: &Path, at: u64, value: u32) -> Result<()> {
    file.seek(SeekFrom::Start(at))
        .map_err(|source| Error::io(VaultOp::Write, path, source))?;
    file.write_all(&value.to_le_bytes())
        .map_err(|source| Error::io(VaultOp::Write, path, source))
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

        let packed = compressed(&from, &into, Some(CEILING)).expect("a compression");

        assert_eq!(packed, Packed::NoSmaller);
        let written = fs::metadata(&into).map_or(0, |held| held.len());
        assert!(
            written < HELD as u64 / 4,
            "the whole of the file was compressed before it was given up on: {written} bytes"
        );
        let _ = fs::remove_dir_all(&folder);
    }
}
