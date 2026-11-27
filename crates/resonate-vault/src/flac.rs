use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use flacenc::{
    bitsink::MemSink,
    component::{BitRepr, MetadataBlockData, Stream},
    config,
    constant::{fixed, qlpc, rice},
    encode_fixed_size_frame,
    error::{Verified, Verify},
    source::{Context, Fill, FrameBuf},
};
use resonate_codec::{DecodeStatus, Decoder};
use resonate_core::{AudioBuffer, Frames, StreamSpec};

use crate::{
    Halt,
    error::{Error, FlacOp, Result, VaultOp},
    head::HeadBlock,
    key::VaultKey,
    pcm::{self, BLOCK_FRAMES, Narrowing},
};

const TUKEY_ALPHA: f32 = 0.4;

#[cfg(test)]
thread_local! {
    pub(crate) static ENCODES_BEGUN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) struct Encoded {
    pub(crate) key: VaultKey,
    pub(crate) frames: Frames,
    pub(crate) outgrew: bool,
}

pub(crate) enum Encode {
    Encoded(Encoded),
    Wider,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlacWritten {
    pub digest: VaultKey,
    pub frames: Frames,
    pub bytes: u64,
    pub outgrew: bool,
}

struct Writing {
    file: File,
    path: PathBuf,
    written: u64,
    ceiling: Option<u64>,
}

impl Writing {
    fn outgrew(&self) -> bool {
        self.ceiling.is_some_and(|ceiling| self.written >= ceiling)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.file
            .write_all(bytes)
            .map_err(|source| Error::io(VaultOp::Write, &self.path, source))?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

pub struct FlacWriter {
    config: Verified<config::Encoder>,
    stream: Stream,
    framebuf: FrameBuf,
    context: Context,
    writing: Writing,
    pending: Vec<i32>,
    whole_block: usize,
    spec: StreamSpec,
    bits: u8,
}

impl FlacWriter {
    pub fn over(
        file: File,
        path: &Path,
        spec: StreamSpec,
        bits: u8,
        carried: &[HeadBlock],
        ceiling: Option<u64>,
    ) -> Result<Self> {
        let channels = usize::from(spec.channel_count().get());
        let depth = usize::from(bits);
        let rate = spec.rate.hz() as usize;
        let config = at_the_most_compression()?;

        let mut stream = unencodable(Stream::new(rate, channels, depth), spec, bits)?;

        unencodable(
            stream
                .stream_info_mut()
                .set_block_sizes(BLOCK_FRAMES, BLOCK_FRAMES),
            spec,
            bits,
        )?;
        for block in carried {
            stream.add_metadata_block(unencodable(
                MetadataBlockData::new_unknown(block.kind.tag(), &block.body),
                spec,
                bits,
            )?);
        }

        let framebuf = unencodable(FrameBuf::with_size(channels, BLOCK_FRAMES), spec, bits)?;
        let mut writing = Writing {
            file,
            path: path.to_path_buf(),
            written: 0,
            ceiling,
        };

        writing.write(&written(&stream)?)?;
        Ok(Self {
            config,
            stream,
            framebuf,
            context: Context::new(depth, channels),
            writing,
            pending: Vec::with_capacity(BLOCK_FRAMES * channels * 2),
            whole_block: BLOCK_FRAMES * channels,
            spec,
            bits,
        })
    }

    pub fn outgrew(&self) -> bool {
        self.writing.outgrew()
    }

    pub fn push(&mut self, interleaved: &[i32]) -> Result<()> {
        self.pending.extend_from_slice(interleaved);

        let mut at = 0;

        while self.pending.len() - at >= self.whole_block {
            emit(
                &self.config,
                &mut self.stream,
                &mut self.framebuf,
                &mut self.context,
                &mut self.writing,
                &self.pending[at..at + self.whole_block],
            )?;
            at += self.whole_block;
        }
        self.pending.drain(..at);
        Ok(())
    }

    pub fn finish(mut self) -> Result<FlacWritten> {
        if !self.pending.is_empty() {
            emit(
                &self.config,
                &mut self.stream,
                &mut self.framebuf,
                &mut self.context,
                &mut self.writing,
                &self.pending,
            )?;
        }

        let digest = self.context.md5_digest();
        let total = self.context.total_samples();

        let mut finished = FlacWritten {
            digest: VaultKey::of(digest),
            frames: Frames(total as u64),
            bytes: self.writing.written,
            outgrew: true,
        };

        if self.writing.outgrew() {
            return Ok(finished);
        }
        self.stream.stream_info_mut().set_md5_digest(&digest);
        self.stream.stream_info_mut().set_total_samples(total);
        unencodable(
            self.stream
                .stream_info_mut()
                .set_block_sizes(BLOCK_FRAMES, BLOCK_FRAMES),
            self.spec,
            self.bits,
        )?;

        let header = written(&self.stream)?;
        let Writing { mut file, path, .. } = self.writing;

        file.seek(SeekFrom::Start(0))
            .map_err(|source| Error::io(VaultOp::Write, &path, source))?;
        file.write_all(&header)
            .map_err(|source| Error::io(VaultOp::Write, &path, source))?;
        file.sync_all()
            .map_err(|source| Error::io(VaultOp::Settle, &path, source))?;

        finished.outgrew = false;
        Ok(finished)
    }
}

pub(crate) fn at_the_most_compression() -> Result<Verified<config::Encoder>> {
    let mut encoder = config::Encoder::default();

    encoder.block_size = BLOCK_FRAMES;
    encoder.multithread = false;
    encoder.stereo_coding.use_leftside = true;
    encoder.stereo_coding.use_rightside = true;
    encoder.stereo_coding.use_midside = true;
    encoder.subframe_coding.use_constant = true;
    encoder.subframe_coding.use_fixed = true;
    encoder.subframe_coding.use_lpc = true;
    encoder.subframe_coding.fixed.max_order = fixed::MAX_LPC_ORDER;
    encoder.subframe_coding.fixed.order_sel = config::OrderSel::BitCount;
    encoder.subframe_coding.qlpc.lpc_order = qlpc::MAX_ORDER;
    encoder.subframe_coding.qlpc.quant_precision = qlpc::MAX_PRECISION;
    encoder.subframe_coding.qlpc.window = config::Window::Tukey { alpha: TUKEY_ALPHA };
    encoder.subframe_coding.prc.max_parameter = rice::MAX_RICE_PARAMETER;

    encoder.into_verified().map_err(|(_, source)| {
        tracing::warn!(%source, "the encoder settings this build asks for were refused");
        Error::Encoding {
            op: FlacOp::Configure,
        }
    })
}

pub(crate) fn encode(
    decoder: &mut Decoder,
    spec: StreamSpec,
    bits: u8,
    narrowing: Narrowing,
    into: &Path,
    ceiling: Option<u64>,
    halt: Halt<'_>,
) -> Result<Encode> {
    #[cfg(test)]
    ENCODES_BEGUN.with(|begun| begun.set(begun.get() + 1));

    at_the_most_compression()?;
    unencodable(
        Stream::new(
            spec.rate.hz() as usize,
            usize::from(spec.channel_count().get()),
            usize::from(bits),
        ),
        spec,
        bits,
    )?;

    let file = File::create(into).map_err(|source| Error::io(VaultOp::Stage, into, source))?;
    let mut writer = FlacWriter::over(file, into, spec, bits, &[], ceiling)?;
    let mut block = AudioBuffer::empty(spec);
    let mut widened = Vec::new();

    loop {
        halt.heard()?;

        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(VaultOp::Read, source))?;

        if status == DecodeStatus::EndOfStream {
            break;
        }

        pcm::widened(&block, &mut widened);
        if !narrowing.narrowed(&mut widened) {
            return Ok(Encode::Wider);
        }
        writer.push(&widened)?;
    }

    let written = writer.finish()?;

    Ok(Encode::Encoded(Encoded {
        key: written.digest,
        frames: written.frames,
        outgrew: written.outgrew,
    }))
}

fn emit(
    config: &Verified<config::Encoder>,
    stream: &mut Stream,
    framebuf: &mut FrameBuf,
    context: &mut Context,
    writing: &mut Writing,
    samples: &[i32],
) -> Result<()> {
    let mut filling = (&mut *framebuf, &mut *context);

    filling.fill_interleaved(samples).map_err(|source| {
        tracing::warn!(%source, "a block of samples could not be laid into a frame");
        Error::Encoding { op: FlacOp::Fill }
    })?;
    if writing.outgrew() {
        return Ok(());
    }

    let number = context.current_frame_number().unwrap_or_default();
    let frame = encode_fixed_size_frame(config, framebuf, number, stream.stream_info()).map_err(
        |source| {
            tracing::warn!(%source, "a frame could not be encoded");
            Error::Encoding { op: FlacOp::Frame }
        },
    )?;

    writing.write(&written(&frame)?)?;
    stream.stream_info_mut().update_frame_info(&frame);
    Ok(())
}

fn written<T: BitRepr>(component: &T) -> Result<Vec<u8>> {
    let mut sink = MemSink::<u8>::new();

    component
        .write(&mut sink)
        .map_err(|source| Error::Written {
            source: Box::new(source),
        })?;
    Ok(sink.into_inner())
}

fn unencodable<T, E>(answered: std::result::Result<T, E>, spec: StreamSpec, bits: u8) -> Result<T> {
    answered.map_err(|_| Error::Unencodable {
        rate: spec.rate.hz(),
        channels: spec.channel_count().get(),
        bits,
    })
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use resonate_codec::Sources;
    use resonate_core::{MediaLocation, SampleFormat};

    use super::*;

    const RATE: u32 = 44_100;
    const FRAMES: u32 = 20_000;

    fn noise_wave() -> Vec<u8> {
        let mut state = 0x9e37_79b9_u32;
        let mut data = Vec::with_capacity(FRAMES as usize * 4);

        for _ in 0..FRAMES * 2 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            data.extend_from_slice(&((state >> 16) as i16).to_le_bytes());
        }

        let mut wave = b"RIFF".to_vec();

        wave.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wave.extend_from_slice(b"WAVEfmt ");
        wave.extend_from_slice(&16_u32.to_le_bytes());
        wave.extend_from_slice(&1_u16.to_le_bytes());
        wave.extend_from_slice(&2_u16.to_le_bytes());
        wave.extend_from_slice(&RATE.to_le_bytes());
        wave.extend_from_slice(&(RATE * 4).to_le_bytes());
        wave.extend_from_slice(&4_u16.to_le_bytes());
        wave.extend_from_slice(&16_u16.to_le_bytes());
        wave.extend_from_slice(b"data");
        wave.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wave.extend_from_slice(&data);
        wave
    }

    fn encoded(source: &Path, into: &Path, ceiling: Option<u64>) -> Encoded {
        let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(source))
            .expect("a readable source");

        decoder.set_output_format(SampleFormat::S16);

        let spec = StreamSpec::new(info.spec.rate, info.spec.channels, SampleFormat::S16);
        let narrowing = Narrowing::between(SampleFormat::S16, SampleFormat::S16);
        match encode(
            &mut decoder,
            spec,
            16,
            narrowing,
            into,
            ceiling,
            Halt::NEVER,
        ) {
            Ok(Encode::Encoded(encoded)) => encoded,
            Ok(Encode::Wider) => panic!("a sixteen-bit source asked for a wider depth"),
            Err(error) => panic!("an encode: {error}"),
        }
    }

    #[test]
    fn an_encode_that_outgrows_its_ceiling_stops_writing_but_still_names_the_samples() {
        let folder = env::temp_dir().join(format!("resonate-vault-flac-{}", process::id()));

        fs::create_dir_all(&folder).expect("a scratch folder");

        let source = folder.join("noise.wav");

        fs::write(&source, noise_wave()).expect("a written source");

        let whole_at = folder.join("whole.flac");
        let cut_at = folder.join("cut.flac");

        let whole = encoded(&source, &whole_at, None);
        let ceiling = 4_096;
        let cut = encoded(&source, &cut_at, Some(ceiling));
        let whole_bytes = fs::metadata(&whole_at).expect("the whole encode").len();
        let cut_bytes = fs::metadata(&cut_at).expect("the cut encode").len();
        let _ = fs::remove_dir_all(&folder);

        assert!(!whole.outgrew);
        assert!(cut.outgrew);
        assert_eq!(
            cut.key, whole.key,
            "an encode cut short named other samples"
        );
        assert_eq!(cut.frames, whole.frames);
        assert!(
            cut_bytes < whole_bytes / 2,
            "an encode past its ceiling went on writing: {cut_bytes} of {whole_bytes}"
        );
    }

    const STAMP_THE_PINS_WERE_TAKEN_AT: u16 = 9;
    const FLAC_OF_THE_NOISE: &str = "5a50edbaf266bb9d43f93957cfa2894c";
    const ZSTD_OF_THE_NOISE: &str = "a5df14a15f07cc00fae2bf2e3ac3ac26";

    fn digest_of(bytes: &[u8]) -> String {
        let mut digest = crate::pcm::Digest::default();

        digest.note(bytes);
        digest.settled().to_string()
    }

    #[test]
    fn the_encoders_an_object_is_stamped_for_are_the_ones_this_build_runs() {
        let folder = env::temp_dir().join(format!("resonate-vault-pinned-{}", process::id()));

        fs::create_dir_all(&folder).expect("a scratch folder");

        let source = folder.join("noise.wav");
        let flac_at = folder.join("noise.flac");

        fs::write(&source, noise_wave()).expect("a written source");

        encoded(&source, &flac_at, None);

        let flac = fs::read(&flac_at).expect("an encoded file");
        let _ = fs::remove_dir_all(&folder);
        let zstd = zstd::bulk::compress(&noise_wave(), crate::wave::ARCHIVED_AT)
            .expect("a compressed wave");

        assert_eq!(
            crate::Encoding::OF_THIS_BUILD.get(),
            STAMP_THE_PINS_WERE_TAKEN_AT,
            "the stamp moved without the pins below being retaken"
        );
        assert_eq!(
            digest_of(&flac),
            FLAC_OF_THE_NOISE,
            "the FLAC encoder's output changed: bump Encoding::OF_THIS_BUILD and retake the pins"
        );
        assert_eq!(
            digest_of(&zstd),
            ZSTD_OF_THE_NOISE,
            "the zstd output changed: bump Encoding::OF_THIS_BUILD and retake the pins"
        );
    }
}
