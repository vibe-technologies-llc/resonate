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

const RATE_THE_ENCODER_VERIFIES_AT_MOST: u32 = 96_000;

const STREAM_MARKER_BYTES: usize = 4;

const METADATA_HEADER_BYTES: usize = 4;

const STREAMINFO_RATE_AT: usize = STREAM_MARKER_BYTES + METADATA_HEADER_BYTES + 10;

const FRAME_BLOCK_AND_RATE_AT: usize = 2;

const FRAME_NUMBER_AT: usize = 4;

const LOW_NIBBLE: u8 = 0x0F;

const RATE_CODE_FROM_STREAMINFO: u8 = 0x0;

const RATE_CODE_176_4_KHZ: u8 = 0x2;

const RATE_CODE_192_KHZ: u8 = 0x3;

const BLOCK_SIZE_CODE_EIGHT_BITS: u8 = 0x6;

const BLOCK_SIZE_CODE_SIXTEEN_BITS: u8 = 0x7;

const CRC8_POLYNOMIAL: u8 = 0x07;

const CRC16_POLYNOMIAL: u16 = 0x8005;

const CRC16_BYTES: usize = 2;

pub(crate) struct Live {
    config: Verified<config::Encoder>,
    stream: Stream,
    framebuf: FrameBuf,
    context: Context,
    block: usize,
    channels: usize,
    format: SampleFormat,
    pending: Vec<i32>,
    rate: u32,
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

        let encoded_at = spec.rate.hz().min(RATE_THE_ENCODER_VERIFIES_AT_MOST);
        let mut stream = Stream::new(encoded_at as usize, channels, depth).map_err(|source| {
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
        let mut head = written(&stream, FlacOp::Header)?;
        if spec.rate.hz() != encoded_at {
            name_the_rate_in_the_streaminfo(&mut head, spec.rate.hz())?;
        }

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
                rate: spec.rate.hz(),
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
        let mut bytes = written(&frame, FlacOp::Frame)?;
        if self.rate > RATE_THE_ENCODER_VERIFIES_AT_MOST {
            name_the_rate_in_the_frame(&mut bytes, self.rate)?;
        }
        wire.extend_from_slice(&bytes);
        Ok((samples.len() / self.channels) as u64)
    }
}

fn rate_unnamed() -> Error {
    Error::Encoding { op: FlacOp::Rate }
}

fn name_the_rate_in_the_streaminfo(head: &mut [u8], rate: u32) -> Result<()> {
    let [high, middle, low] = head
        .get_mut(STREAMINFO_RATE_AT..STREAMINFO_RATE_AT + 3)
        .and_then(|bytes| <&mut [u8; 3]>::try_from(bytes).ok())
        .ok_or_else(rate_unnamed)?;
    *high = (rate >> 12) as u8;
    *middle = (rate >> 4) as u8;
    *low = (*low & LOW_NIBBLE) | (((rate & u32::from(LOW_NIBBLE)) as u8) << 4);
    Ok(())
}

fn name_the_rate_in_the_frame(frame: &mut [u8], rate: u32) -> Result<()> {
    let block_and_rate = *frame
        .get(FRAME_BLOCK_AND_RATE_AT)
        .ok_or_else(rate_unnamed)?;
    let first_of_number = *frame.get(FRAME_NUMBER_AT).ok_or_else(rate_unnamed)?;
    let code = match rate {
        176_400 => RATE_CODE_176_4_KHZ,
        192_000 => RATE_CODE_192_KHZ,
        _ => RATE_CODE_FROM_STREAMINFO,
    };
    let block_size_bytes = match block_and_rate >> 4 {
        BLOCK_SIZE_CODE_EIGHT_BITS => 1,
        BLOCK_SIZE_CODE_SIXTEEN_BITS => 2,
        _ => 0,
    };
    let header_ends = FRAME_NUMBER_AT + number_bytes(first_of_number) + block_size_bytes;
    let frame_ends = frame
        .len()
        .checked_sub(CRC16_BYTES)
        .filter(|at| *at > header_ends)
        .ok_or_else(rate_unnamed)?;

    *frame
        .get_mut(FRAME_BLOCK_AND_RATE_AT)
        .ok_or_else(rate_unnamed)? = (block_and_rate & !LOW_NIBBLE) | code;
    let header_crc = crc8(frame.get(..header_ends).ok_or_else(rate_unnamed)?);
    *frame.get_mut(header_ends).ok_or_else(rate_unnamed)? = header_crc;
    let frame_crc = crc16(frame.get(..frame_ends).ok_or_else(rate_unnamed)?);
    frame
        .get_mut(frame_ends..)
        .ok_or_else(rate_unnamed)?
        .copy_from_slice(&frame_crc.to_be_bytes());
    Ok(())
}

fn number_bytes(first: u8) -> usize {
    match first.leading_ones() {
        0 => 1,
        ones => ones as usize,
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |crc, byte| {
        (0..8).fold(crc ^ byte, |crc, _| {
            if crc & 0x80 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ CRC8_POLYNOMIAL
            }
        })
    })
}

fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |crc, byte| {
        (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
            if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ CRC16_POLYNOMIAL
            }
        })
    })
}

fn written<T: BitRepr>(component: &T, op: FlacOp) -> Result<Vec<u8>> {
    let mut sink = MemSink::<u8>::new();
    component.write(&mut sink).map_err(|source| {
        tracing::warn!(%source, "a FLAC component could not be written");
        Error::Encoding { op }
    })?;
    Ok(sink.into_inner())
}
