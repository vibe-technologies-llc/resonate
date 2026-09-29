use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
};

use flacenc::{
    bitsink::MemSink,
    component::{BitRepr, Stream},
    config,
    constant::{fixed, qlpc, rice},
    encode_fixed_size_frame,
    error::{Verified, Verify},
    source::{Context, Fill, FrameBuf},
};
use resonate_codec::{DecodeStatus, Decoder};
use resonate_core::{AudioBuffer, Frames, StreamSpec};

use crate::{
    error::{Error, FlacOp, Result, VaultOp},
    key::VaultKey,
    pcm::{self, BLOCK_FRAMES},
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

struct Writing<'a> {
    file: File,
    path: &'a Path,
    written: u64,
    ceiling: Option<u64>,
}

impl Writing<'_> {
    fn outgrew(&self) -> bool {
        self.ceiling.is_some_and(|ceiling| self.written >= ceiling)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.file
            .write_all(bytes)
            .map_err(|source| Error::io(VaultOp::Write, self.path, source))?;
        self.written += bytes.len() as u64;
        Ok(())
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
    into: &Path,
    ceiling: Option<u64>,
) -> Result<Encoded> {
    let channels = usize::from(spec.channel_count().get());
    let depth = usize::from(bits);
    let rate = spec.rate.hz() as usize;
    let whole_block = BLOCK_FRAMES * channels;
    let config = at_the_most_compression()?;
    #[cfg(test)]
    ENCODES_BEGUN.with(|begun| begun.set(begun.get() + 1));

    let mut stream = unencodable(Stream::new(rate, channels, depth), spec, bits)?;
    unencodable(
        stream
            .stream_info_mut()
            .set_block_sizes(BLOCK_FRAMES, BLOCK_FRAMES),
        spec,
        bits,
    )?;

    let file = File::create(into).map_err(|source| Error::io(VaultOp::Stage, into, source))?;
    let mut writing = Writing {
        file,
        path: into,
        written: 0,
        ceiling,
    };
    writing.write(&written(&stream)?)?;

    let mut framebuf = unencodable(FrameBuf::with_size(channels, BLOCK_FRAMES), spec, bits)?;
    let mut context = Context::new(depth, channels);
    let mut block = AudioBuffer::empty(spec);
    let mut widened = Vec::new();
    let mut pending: Vec<i32> = Vec::with_capacity(whole_block * 2);

    loop {
        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(VaultOp::Read, source))?;
        if status == DecodeStatus::EndOfStream {
            break;
        }

        pcm::widened(&block, &mut widened);
        pending.extend_from_slice(&widened);

        let mut at = 0;
        while pending.len() - at >= whole_block {
            emit(
                &config,
                &mut stream,
                &mut framebuf,
                &mut context,
                &mut writing,
                &pending[at..at + whole_block],
            )?;
            at += whole_block;
        }
        pending.drain(..at);
    }

    if !pending.is_empty() {
        emit(
            &config,
            &mut stream,
            &mut framebuf,
            &mut context,
            &mut writing,
            &pending,
        )?;
    }

    let digest = context.md5_digest();
    let total = context.total_samples();
    if writing.outgrew() {
        return Ok(Encoded {
            key: VaultKey::of(digest),
            frames: Frames(total as u64),
            outgrew: true,
        });
    }
    stream.stream_info_mut().set_md5_digest(&digest);
    stream.stream_info_mut().set_total_samples(total);
    unencodable(
        stream
            .stream_info_mut()
            .set_block_sizes(BLOCK_FRAMES, BLOCK_FRAMES),
        spec,
        bits,
    )?;

    let header = written(&stream)?;
    let mut file = writing.file;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| Error::io(VaultOp::Write, into, source))?;
    file.write_all(&header)
        .map_err(|source| Error::io(VaultOp::Write, into, source))?;
    file.sync_all()
        .map_err(|source| Error::io(VaultOp::Settle, into, source))?;

    Ok(Encoded {
        key: VaultKey::of(digest),
        frames: Frames(total as u64),
        outgrew: false,
    })
}

fn emit(
    config: &Verified<config::Encoder>,
    stream: &mut Stream,
    framebuf: &mut FrameBuf,
    context: &mut Context,
    writing: &mut Writing<'_>,
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
        encode(&mut decoder, spec, 16, into, ceiling).expect("an encode")
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
}
