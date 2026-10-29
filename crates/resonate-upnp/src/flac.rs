use flacenc::{
    bitsink::MemSink,
    component::{BitRepr, Stream},
    config,
    error::{Verified, Verify},
    source::{Context, Fill, FrameBuf},
};
use resonate_core::{SampleFormat, StreamSpec};

use crate::{Error, FlacOp, Result};

const BLOCK_FRAMES_UP_TO_48K: usize = 4096;

const BLOCK_FRAMES_ABOVE_48K: usize = 8192;

const RATE_OF_THE_SMALL_BLOCK: u32 = 48_000;

const S16_WORD_BYTES: usize = 2;

const WIDE_WORD_BYTES: usize = 4;

pub(crate) struct Live {
    config: Verified<config::Encoder>,
    stream: Stream,
    framebuf: FrameBuf,
    context: Context,
    block: usize,
    channels: usize,
    format: SampleFormat,
    pending: Vec<i32>,
}

impl Live {
    pub(crate) fn begin(spec: StreamSpec) -> Result<(Self, Vec<u8>)> {
        let channels = usize::from(spec.channel_count().get());
        let depth = usize::from(spec.format.valid_bits());
        let block = if spec.rate.hz() > RATE_OF_THE_SMALL_BLOCK {
            BLOCK_FRAMES_ABOVE_48K
        } else {
            BLOCK_FRAMES_UP_TO_48K
        };

        let mut encoder = config::Encoder::default();
        encoder.block_size = block;
        encoder.multithread = false;
        let config = encoder.into_verified().map_err(|(_, source)| {
            tracing::warn!(%source, "the encoder settings a renderer's stream asks for were refused");
            Error::Encoding {
                op: FlacOp::Configure,
            }
        })?;

        let mut stream =
            Stream::new(spec.rate.hz() as usize, channels, depth).map_err(|source| {
                tracing::warn!(%source, "a stream of this shape cannot be FLAC");
                Error::Encoding { op: FlacOp::Shape }
            })?;
        stream
            .stream_info_mut()
            .set_block_sizes(block, block)
            .map_err(|source| {
                tracing::warn!(%source, "the block size of a FLAC stream was refused");
                Error::Encoding { op: FlacOp::Shape }
            })?;
        let framebuf = FrameBuf::with_size(channels, block).map_err(|source| {
            tracing::warn!(%source, "a block of FLAC samples could not be held");
            Error::Encoding { op: FlacOp::Shape }
        })?;
        let head = written(&stream, FlacOp::Header)?;

        Ok((
            Self {
                config,
                stream,
                framebuf,
                context: Context::new(depth, channels),
                block,
                channels,
                format: spec.format,
                pending: Vec::with_capacity(block * channels * 2),
            },
            head,
        ))
    }

    pub(crate) fn carry(&mut self, ring: &[u8], wire: &mut Vec<u8>) -> Result<u64> {
        wire.clear();
        self.widen(ring);
        let whole_block = self.block * self.channels;
        let mut at = 0;
        let mut frames = 0;
        while self.pending.len() - at >= whole_block {
            frames += self.emit(at, at + whole_block, wire)?;
            at += whole_block;
        }
        self.pending.drain(..at);
        Ok(frames)
    }

    pub(crate) fn finish(&mut self, wire: &mut Vec<u8>) -> Result<u64> {
        wire.clear();
        if self.pending.is_empty() {
            return Ok(0);
        }
        let to = self.pending.len();
        let frames = self.emit(0, to, wire)?;
        self.pending.clear();
        Ok(frames)
    }

    fn widen(&mut self, ring: &[u8]) {
        match self.format {
            SampleFormat::S16 => self.pending.extend(
                ring.as_chunks::<S16_WORD_BYTES>()
                    .0
                    .iter()
                    .map(|word| i32::from(i16::from_le_bytes(*word))),
            ),
            SampleFormat::S24 | SampleFormat::S32 | SampleFormat::F32 => self.pending.extend(
                ring.as_chunks::<WIDE_WORD_BYTES>()
                    .0
                    .iter()
                    .map(|word| i32::from_le_bytes(*word)),
            ),
        }
    }

    fn emit(&mut self, from: usize, to: usize, wire: &mut Vec<u8>) -> Result<u64> {
        let samples = self.pending.get(from..to).unwrap_or_default();
        let mut filling = (&mut self.framebuf, &mut self.context);
        filling.fill_interleaved(samples).map_err(|source| {
            tracing::warn!(%source, "a block of samples could not be laid into a frame");
            Error::Encoding { op: FlacOp::Fill }
        })?;

        let number = self.context.current_frame_number().unwrap_or_default();
        let frame = flacenc::encode_fixed_size_frame(
            &self.config,
            &self.framebuf,
            number,
            self.stream.stream_info(),
        )
        .map_err(|source| {
            tracing::warn!(%source, "a frame could not be encoded");
            Error::Encoding { op: FlacOp::Frame }
        })?;
        wire.extend_from_slice(&written(&frame, FlacOp::Frame)?);
        Ok((samples.len() / self.channels) as u64)
    }
}

fn written<T: BitRepr>(component: &T, op: FlacOp) -> Result<Vec<u8>> {
    let mut sink = MemSink::<u8>::new();
    component.write(&mut sink).map_err(|source| {
        tracing::warn!(%source, "a FLAC component could not be written");
        Error::Encoding { op }
    })?;
    Ok(sink.into_inner())
}
