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

pub(crate) struct Encoded {
    pub(crate) key: VaultKey,
    pub(crate) frames: Frames,
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
) -> Result<Encoded> {
    let channels = usize::from(spec.channel_count().get());
    let depth = usize::from(bits);
    let rate = spec.rate.hz() as usize;
    let whole_block = BLOCK_FRAMES * channels;
    let config = at_the_most_compression()?;

    let mut stream = unencodable(Stream::new(rate, channels, depth), spec, bits)?;
    unencodable(
        stream
            .stream_info_mut()
            .set_block_sizes(BLOCK_FRAMES, BLOCK_FRAMES),
        spec,
        bits,
    )?;

    let mut file = File::create(into).map_err(|source| Error::io(VaultOp::Stage, into, source))?;
    let placeholder = written(&stream)?;
    file.write_all(&placeholder)
        .map_err(|source| Error::io(VaultOp::Write, into, source))?;

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
                &mut file,
                into,
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
            &mut file,
            into,
            &pending,
        )?;
    }

    let digest = context.md5_digest();
    let total = context.total_samples();
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
    file.seek(SeekFrom::Start(0))
        .map_err(|source| Error::io(VaultOp::Write, into, source))?;
    file.write_all(&header)
        .map_err(|source| Error::io(VaultOp::Write, into, source))?;
    file.sync_all()
        .map_err(|source| Error::io(VaultOp::Settle, into, source))?;

    Ok(Encoded {
        key: VaultKey::of(digest),
        frames: Frames(total as u64),
    })
}

fn emit(
    config: &Verified<config::Encoder>,
    stream: &mut Stream,
    framebuf: &mut FrameBuf,
    context: &mut Context,
    file: &mut File,
    path: &Path,
    samples: &[i32],
) -> Result<()> {
    let mut filling = (&mut *framebuf, &mut *context);
    filling.fill_interleaved(samples).map_err(|source| {
        tracing::warn!(%source, "a block of samples could not be laid into a frame");
        Error::Encoding { op: FlacOp::Fill }
    })?;

    let number = context.current_frame_number().unwrap_or_default();
    let frame = encode_fixed_size_frame(config, framebuf, number, stream.stream_info()).map_err(
        |source| {
            tracing::warn!(%source, "a frame could not be encoded");
            Error::Encoding { op: FlacOp::Frame }
        },
    )?;

    let bytes = written(&frame)?;
    file.write_all(&bytes)
        .map_err(|source| Error::io(VaultOp::Write, path, source))?;
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
