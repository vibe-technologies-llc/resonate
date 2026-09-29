use std::{
    io::{Read, Seek, SeekFrom},
    ops::Range,
};

use symphonia::core::{
    audio::{
        AsGenericAudioBufferRef, Audio, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef,
        sample::SampleFormat,
    },
    codecs::{
        CodecInfo,
        audio::{
            AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
            well_known::CODEC_ID_WAVPACK,
        },
        registry::{RegisterableAudioDecoder, SupportedAudioCodec},
    },
    common::FourCc,
    errors::{Result, decode_error, unsupported_error},
    packet::PacketRef,
};
use symphonia_codec_wavpack::WavPackDecoder;

use crate::prescan::read_exact;

pub(crate) const HYBRID_CODEC_ID: AudioCodecId = AudioCodecId::new(FourCc::new(*b"wvhy"));

const MARKER: &[u8; 4] = b"wvpk";
const HEADER_BYTES: usize = 32;
const UNCOUNTED_HEADER_BYTES: usize = 8;
const SIZE_AT: Range<usize> = 4..8;
const BLOCK_SAMPLES_AT: Range<usize> = 20..24;
const FLAGS_AT: Range<usize> = 24..28;

const MONO: u32 = 0x4;
const HYBRID: u32 = 0x8;
const FLOAT_DATA: u32 = 0x80;
const FINAL_BLOCK: u32 = 0x1000;
const FALSE_STEREO: u32 = 0x4000_0000;

const LARGE: u8 = 0x80;
const ODD_SIZE: u8 = 0x40;
const FUNCTION: u8 = 0x3f;
const FLOAT_INFO: u8 = 0x08;
const EXTENDED: u8 = 0x0c;
const EXTENDED_WITH_WIDTHS: u8 = 0x2c;
const EXTENDED_CHECKSUM_BYTES: usize = 4;
const FLOAT_INFO_BYTES: usize = 4;

const SHIFT_ONES: u8 = 0x01;
const SHIFT_SAME: u8 = 0x02;
const SHIFT_SENT: u8 = 0x04;
const ZEROS_SENT: u8 = 0x08;
const NEGATIVE_ZEROS: u8 = 0x10;

const MANTISSA_BITS: u32 = 23;
const MANTISSA: u32 = (1 << MANTISSA_BITS) - 1;
const IMPLICIT_ONE: u32 = 1 << MANTISSA_BITS;
const EXPONENT_BITS: u32 = 8;
const INFINITE_EXPONENT: u32 = 255;
const OVERFLOWED_MAGNITUDE: u32 = 0x100_0000;
const OVERFLOW_BITS: u32 = 0x0f00_0000;
const SIGN: u32 = 1 << 31;
const EXPONENT_SENT_FROM: u32 = 25;
const WIDTH_BITS: u32 = 5;
const SHIFTED_WITHIN_A_WORD: u32 = 0x1f;

const MOST_FRAMES_A_BLOCK: u64 = 1 << 20;

const MATROSKA_VERSIONS: Range<u16> = 0x402..0x411;
const MATROSKA_PREFIX_BYTES: usize = 8;
const MATROSKA_BLOCK_SIZE_BYTES: usize = 4;
const MATROSKA_FLAGS_AT: Range<usize> = 4..8;
pub(crate) const MATROSKA_HEAD_BYTES: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Coding {
    #[default]
    Lossless,
    Hybrid,
}

impl Coding {
    const fn of_flags(flags: u32) -> Self {
        if flags & HYBRID == 0 {
            Self::Lossless
        } else {
            Self::Hybrid
        }
    }

    pub(crate) fn of_matroska_head(head: [u8; MATROSKA_HEAD_BYTES]) -> Self {
        Self::of_flags(word(&head, MATROSKA_FLAGS_AT))
    }

    pub(crate) const fn or(self, other: Self) -> Self {
        match self {
            Self::Hybrid => Self::Hybrid,
            Self::Lossless => other,
        }
    }

    pub(crate) fn codec(self, declared: AudioCodecId) -> AudioCodecId {
        match self {
            Self::Hybrid if declared == CODEC_ID_WAVPACK => HYBRID_CODEC_ID,
            Self::Hybrid | Self::Lossless => declared,
        }
    }
}

pub(crate) fn read_coding<S: Read + Seek + ?Sized>(source: &mut S) -> Coding {
    let Ok(origin) = source.stream_position() else {
        return Coding::Lossless;
    };
    let found = match read_exact::<HEADER_BYTES, S>(source) {
        Some(header) if header.starts_with(MARKER) => Coding::of_flags(word(&header, FLAGS_AT)),
        Some(_) | None => Coding::Lossless,
    };
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("a WavPack header read could not restore the stream position");
    }
    found
}

#[derive(Clone, Copy)]
struct FloatInfo {
    flags: u8,
    shift: u8,
    max_exponent: u8,
}

struct Floating {
    first_channel: usize,
    stored_channels: usize,
    heard_channels: usize,
    frames: usize,
    info: FloatInfo,
    extended: Option<Extended>,
}

struct Extended {
    bits: Range<usize>,
    with_widths: bool,
}

pub(crate) struct WavPack {
    params: AudioCodecParameters,
    inner: WavPackDecoder,
    matroska_version: Option<u16>,
    headed: Vec<u8>,
    rewriting: Rewriting,
    stored: Vec<i32>,
    buffer: AudioBuffer<f32>,
}

#[derive(Default)]
struct Rewriting {
    rewritten: Vec<u8>,
    extended: Vec<u8>,
    floating: Vec<Floating>,
}

impl WavPack {
    fn try_new(params: &AudioCodecParameters, options: &AudioDecoderOptions) -> Result<Self> {
        let Some(rate) = params.sample_rate else {
            return unsupported_error("wavpack: the sample rate is not declared");
        };
        let Some(channels) = params.channels.clone() else {
            return unsupported_error("wavpack: the channels are not declared");
        };
        let capacity = params
            .max_frames_per_packet
            .unwrap_or(0)
            .min(MOST_FRAMES_A_BLOCK);
        let mut integers = params.clone();
        integers.max_frames_per_packet = Some(capacity);
        if matches!(integers.sample_format, Some(SampleFormat::F32)) {
            integers.sample_format = Some(SampleFormat::S32);
        }
        let matroska_version = params
            .extra_data
            .as_deref()
            .and_then(|extra| extra.get(..2))
            .map(|version| u16::from_le_bytes([version[0], version[1]]));
        Ok(Self {
            params: params.clone(),
            inner: WavPackDecoder::try_new(&integers, options)?,
            matroska_version,
            headed: Vec::new(),
            rewriting: Rewriting::default(),
            stored: Vec::new(),
            buffer: AudioBuffer::new(
                AudioSpec::new(rate, channels),
                usize::try_from(capacity).unwrap_or(0),
            ),
        })
    }

    fn restore_floats(&mut self) -> Result<()> {
        let GenericAudioBufferRef::S32(integers) = self.inner.last_decoded() else {
            return decode_error("wavpack: floating samples did not decode as integers");
        };
        self.buffer.clear();
        let frames = integers.frames();
        if frames > self.buffer.capacity() {
            self.buffer.grow_capacity(frames);
        }
        self.buffer.render_uninit(Some(frames));

        for floating in &self.rewriting.floating {
            let frames = floating.frames.min(frames);
            self.stored.clear();
            for frame in 0..frames {
                for lane in 0..floating.stored_channels {
                    let Some(plane) = integers.plane(floating.first_channel + lane) else {
                        return decode_error("wavpack: a block names a channel the stream lacks");
                    };
                    self.stored.push(plane[frame]);
                }
            }

            match &floating.extended {
                Some(extended) => restore_extended(
                    &mut self.stored,
                    floating.info,
                    &self.rewriting.extended[extended.bits.clone()],
                    extended.with_widths,
                )?,
                None => restore(&mut self.stored, floating.info),
            }

            for lane in 0..floating.heard_channels {
                let stored_lane = lane.min(floating.stored_channels - 1);
                let Some(plane) = self.buffer.plane_mut(floating.first_channel + lane) else {
                    return decode_error("wavpack: a block names a channel the stream lacks");
                };
                for (frame, sample) in plane.iter_mut().take(frames).enumerate() {
                    let bits = self.stored[frame * floating.stored_channels + stored_lane];
                    *sample = f32::from_bits(bits as u32);
                }
            }
        }
        Ok(())
    }
}

impl Rewriting {
    fn rewrite(&mut self, data: &[u8]) -> Result<()> {
        self.rewritten.clear();
        self.extended.clear();
        self.floating.clear();

        let mut at = 0;
        let mut channel = 0;
        while at < data.len() {
            let Some(header) = data.get(at..at + HEADER_BYTES) else {
                return decode_error("wavpack: a block is cut short");
            };
            let size = word(header, SIZE_AT) as usize + UNCOUNTED_HEADER_BYTES;
            let Some(block) = data.get(at..at + size) else {
                return decode_error("wavpack: a block runs past its packet");
            };
            at += size;

            if u64::from(word(header, BLOCK_SAMPLES_AT)) > MOST_FRAMES_A_BLOCK {
                return decode_error(
                    "wavpack: a block claims more samples than any encoder writes",
                );
            }
            let flags = word(header, FLAGS_AT);
            let floats = flags & FLOAT_DATA != 0;
            let stored_channels = if flags & (MONO | FALSE_STEREO) != 0 {
                1
            } else {
                2
            };
            let heard_channels = if flags & MONO != 0 && flags & FALSE_STEREO == 0 {
                1
            } else {
                2
            };

            let written_from = self.rewritten.len();
            self.rewritten.extend_from_slice(header);
            let mut info = None;
            let mut extended = None;

            let mut read = HEADER_BYTES;
            while read < block.len() {
                let Some(sub) = SubBlock::at(block, read) else {
                    return decode_error("wavpack: a metadata block runs past its block");
                };
                read = sub.end;
                let function = sub.id & FUNCTION;
                let payload = &block[sub.payload.clone()];

                if floats && function == FLOAT_INFO {
                    let Some(held) = payload.get(..FLOAT_INFO_BYTES) else {
                        return decode_error("wavpack: the float information is cut short");
                    };
                    info = Some(FloatInfo {
                        flags: held[0],
                        shift: held[1],
                        max_exponent: held[2],
                    });
                    continue;
                }

                if function == EXTENDED || function == EXTENDED_WITH_WIDTHS {
                    let Some(bits) = payload.get(EXTENDED_CHECKSUM_BYTES..) else {
                        return decode_error("wavpack: the extended bits are cut short");
                    };
                    if floats {
                        let start = self.extended.len();
                        self.extended.extend_from_slice(bits);
                        extended = Some(Extended {
                            bits: start..self.extended.len(),
                            with_widths: function == EXTENDED_WITH_WIDTHS,
                        });
                    } else {
                        SubBlock::write(&mut self.rewritten, sub.id, bits);
                    }
                    continue;
                }

                self.rewritten.extend_from_slice(&block[sub.whole.clone()]);
            }

            let written = &mut self.rewritten[written_from..];
            let counted = u32::try_from(written.len() - UNCOUNTED_HEADER_BYTES).unwrap_or(u32::MAX);
            written[SIZE_AT].copy_from_slice(&counted.to_le_bytes());

            if floats {
                let Some(info) = info else {
                    return decode_error("wavpack: floating samples carry no float information");
                };
                written[FLAGS_AT].copy_from_slice(&(flags & !FLOAT_DATA).to_le_bytes());
                self.floating.push(Floating {
                    first_channel: channel,
                    stored_channels,
                    heard_channels,
                    frames: word(header, BLOCK_SAMPLES_AT) as usize,
                    info,
                    extended,
                });
            }
            channel += heard_channels;
        }
        Ok(())
    }
}

fn word(bytes: &[u8], at: Range<usize>) -> u32 {
    let mut held = [0; 4];
    held.copy_from_slice(&bytes[at]);
    u32::from_le_bytes(held)
}

struct SubBlock {
    id: u8,
    whole: Range<usize>,
    payload: Range<usize>,
    end: usize,
}

impl SubBlock {
    fn at(block: &[u8], from: usize) -> Option<Self> {
        let id = *block.get(from)?;
        let mut words = usize::from(*block.get(from + 1)?);
        let mut head = 2;
        if id & LARGE != 0 {
            words |= usize::from(*block.get(from + 2)?) << 8;
            words |= usize::from(*block.get(from + 3)?) << 16;
            head = 4;
        }
        let padded = words * 2;
        let bytes = if id & ODD_SIZE != 0 {
            padded.checked_sub(1)?
        } else {
            padded
        };
        let end = from + head + padded;
        if end > block.len() {
            return None;
        }
        Some(Self {
            id,
            whole: from..end,
            payload: from + head..from + head + bytes,
            end,
        })
    }

    fn write(into: &mut Vec<u8>, id: u8, payload: &[u8]) {
        let odd = payload.len() % 2 == 1;
        let words = payload.len().div_ceil(2);
        let large = words > usize::from(u8::MAX);
        let mut id = id & !(LARGE | ODD_SIZE);
        if large {
            id |= LARGE;
        }
        if odd {
            id |= ODD_SIZE;
        }
        into.push(id);
        into.push(words as u8);
        if large {
            into.push((words >> 8) as u8);
            into.push((words >> 16) as u8);
        }
        into.extend_from_slice(payload);
        if odd {
            into.push(0);
        }
    }
}

fn expand_matroska(data: &[u8], version: u16, into: &mut Vec<u8>) -> Result<()> {
    if !MATROSKA_VERSIONS.contains(&version) {
        return unsupported_error("wavpack: the Matroska stream version is not one WavPack wrote");
    }
    into.clear();
    let Some(samples) = data.get(..4) else {
        return decode_error("wavpack: a Matroska block is cut short");
    };
    let samples: [u8; 4] = [samples[0], samples[1], samples[2], samples[3]];
    let mut at = 4;
    let mut several = false;

    while at < data.len() {
        let Some(prefix) = data.get(at..at + MATROSKA_PREFIX_BYTES) else {
            return decode_error("wavpack: a Matroska block is cut short");
        };
        let flags = word(prefix, 0..4);
        if into.is_empty() && flags & FINAL_BLOCK == 0 {
            several = true;
        }
        at += MATROSKA_PREFIX_BYTES;

        let size = if several {
            let Some(size) = data.get(at..at + MATROSKA_BLOCK_SIZE_BYTES) else {
                return decode_error("wavpack: a Matroska block names no size");
            };
            at += MATROSKA_BLOCK_SIZE_BYTES;
            word(size, 0..4) as usize
        } else {
            data.len() - at
        };
        let Some(body) = data.get(at..at + size) else {
            return decode_error("wavpack: a Matroska block runs past its packet");
        };
        at += size;

        let counted =
            u32::try_from(HEADER_BYTES - UNCOUNTED_HEADER_BYTES + size).unwrap_or(u32::MAX);
        into.extend_from_slice(MARKER);
        into.extend_from_slice(&counted.to_le_bytes());
        into.extend_from_slice(&version.to_le_bytes());
        into.extend_from_slice(&[0, 0]);
        into.extend_from_slice(&samples);
        into.extend_from_slice(&0_u32.to_le_bytes());
        into.extend_from_slice(&samples);
        into.extend_from_slice(&prefix[..MATROSKA_PREFIX_BYTES]);
        into.extend_from_slice(body);
    }
    Ok(())
}

struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
    held: u64,
    count: u32,
}

impl<'a> Bits<'a> {
    const fn over(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            held: 0,
            count: 0,
        }
    }

    fn take(&mut self, wanted: u32) -> Result<u32> {
        if wanted == 0 {
            return Ok(0);
        }
        while self.count < wanted {
            let Some(byte) = self.bytes.get(self.at) else {
                return decode_error("wavpack: the extended bits run out");
            };
            self.held |= u64::from(*byte) << self.count;
            self.at += 1;
            self.count += 8;
        }
        let value = (self.held & ((1_u64 << wanted) - 1)) as u32;
        self.held >>= wanted;
        self.count -= wanted;
        Ok(value)
    }

    fn one(&mut self) -> Result<bool> {
        Ok(self.take(1)? == 1)
    }
}

fn restore(stored: &mut [i32], info: FloatInfo) {
    for sample in stored {
        if *sample == 0 {
            continue;
        }
        let mut exponent = u32::from(info.max_exponent);
        let mut out = 0;
        let mut value = sample.wrapping_shl(u32::from(info.shift));
        if value < 0 {
            value = value.wrapping_neg();
            out |= SIGN;
        }
        let mut magnitude = value as u32;

        if magnitude >= OVERFLOWED_MAGNITUDE {
            while magnitude & OVERFLOW_BITS != 0 {
                magnitude >>= 1;
                exponent += 1;
            }
        } else if exponent != 0 {
            let mut shifted = 0;
            while magnitude & IMPLICIT_ONE == 0 {
                exponent -= 1;
                if exponent == 0 {
                    break;
                }
                shifted += 1;
                magnitude <<= 1;
            }
            shifted &= SHIFTED_WITHIN_A_WORD;
            if shifted != 0 && info.flags & SHIFT_ONES != 0 {
                magnitude |= ones(shifted);
            }
        }

        *sample = (out | (magnitude & MANTISSA) | ((exponent & 0xff) << MANTISSA_BITS)) as i32;
    }
}

fn restore_extended(
    stored: &mut [i32],
    info: FloatInfo,
    bits: &[u8],
    with_widths: bool,
) -> Result<()> {
    let mut bits = Bits::over(bits);
    let (fewest_zeros, most_ones) = if with_widths {
        (bits.take(WIDTH_BITS)?, bits.take(WIDTH_BITS)?)
    } else {
        (0, 0)
    };

    for sample in stored {
        let mut exponent = u32::from(info.max_exponent);
        let mut out = 0;

        if *sample == 0 {
            if info.flags & ZEROS_SENT != 0 {
                if bits.one()? {
                    out |= bits.take(MANTISSA_BITS)? & MANTISSA;
                    if exponent >= EXPONENT_SENT_FROM {
                        out |= (bits.take(EXPONENT_BITS)? & 0xff) << MANTISSA_BITS;
                    }
                    if bits.one()? {
                        out |= SIGN;
                    }
                } else if info.flags & NEGATIVE_ZEROS != 0 && bits.one()? {
                    out |= SIGN;
                }
            }
            *sample = out as i32;
            continue;
        }

        let mut value = sample.wrapping_shl(u32::from(info.shift));
        if value < 0 {
            value = value.wrapping_neg();
            out |= SIGN;
        }
        let mut magnitude = value as u32;

        if magnitude == OVERFLOWED_MAGNITUDE {
            if bits.one()? {
                out |= bits.take(MANTISSA_BITS)? & MANTISSA;
            }
            exponent = INFINITE_EXPONENT;
        } else {
            let mut shifted = 0;
            if exponent != 0 {
                while magnitude & IMPLICIT_ONE == 0 {
                    exponent -= 1;
                    if exponent == 0 {
                        break;
                    }
                    shifted += 1;
                    magnitude <<= 1;
                }
            }
            shifted &= SHIFTED_WITHIN_A_WORD;

            if shifted != 0 {
                if info.flags & SHIFT_ONES != 0 || (info.flags & SHIFT_SAME != 0 && bits.one()?) {
                    magnitude |= ones(shifted);
                } else if info.flags & SHIFT_SENT != 0 {
                    let mut zeros = 0;
                    if most_ones != 0 && shifted > most_ones {
                        zeros = shifted - most_ones;
                    }
                    if fewest_zeros > zeros {
                        zeros = fewest_zeros.min(shifted);
                    }
                    let sent = shifted - zeros;
                    if sent > 0 {
                        magnitude |= (bits.take(sent)? << zeros) & ones(shifted);
                    }
                }
            }
            out |= magnitude & MANTISSA;
        }

        *sample = (out | ((exponent & 0xff) << MANTISSA_BITS)) as i32;
    }
    Ok(())
}

const fn ones(count: u32) -> u32 {
    (1 << count) - 1
}

impl AudioDecoder for WavPack {
    fn reset(&mut self) {
        self.inner.reset();
        self.buffer.clear();
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs()[0].info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        let data = if packet.data.starts_with(MARKER) {
            packet.data
        } else if let Some(version) = self.matroska_version {
            expand_matroska(packet.data, version, &mut self.headed)?;
            &self.headed
        } else {
            return decode_error("wavpack: a packet carries no header and the stream no version");
        };
        self.rewriting.rewrite(data)?;

        let rewritten = PacketRef::new(
            packet.track_id,
            packet.pts,
            packet.dur,
            &self.rewriting.rewritten,
        );
        self.inner.decode_ref(&rewritten)?;

        if self.rewriting.floating.is_empty() {
            return Ok(self.inner.last_decoded());
        }
        self.restore_floats()?;
        Ok(self.buffer.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        self.inner.finalize()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        if self.rewriting.floating.is_empty() {
            self.inner.last_decoded()
        } else {
            self.buffer.as_generic_audio_buffer_ref()
        }
    }
}

impl RegisterableAudioDecoder for WavPack {
    fn try_registry_new(
        params: &AudioCodecParameters,
        options: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>> {
        Ok(Box::new(Self::try_new(params, options)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[SupportedAudioCodec {
            id: CODEC_ID_WAVPACK,
            info: CodecInfo {
                short_name: "wavpack",
                long_name: "WavPack",
                profiles: &[],
            },
        }]
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use symphonia::core::codecs::audio::well_known::CODEC_ID_FLAC;

    use super::*;

    #[test]
    fn a_sample_shifted_to_nothing_under_the_greatest_exponent_is_restored_without_overflowing() {
        let info = FloatInfo {
            flags: SHIFT_ONES,
            shift: 24,
            max_exponent: u8::MAX,
        };
        let mut stored = [0x100, 1, -1];

        restore(&mut stored, info);

        assert_eq!(stored[0] as u32, MANTISSA, "{:#x}", stored[0]);
    }

    fn header(flags: u32) -> Vec<u8> {
        let mut header = vec![0; HEADER_BYTES + 3];
        header[..MARKER.len()].copy_from_slice(MARKER);
        header[FLAGS_AT].copy_from_slice(&flags.to_le_bytes());
        header
    }

    #[test]
    fn a_stream_whose_first_block_is_coded_hybrid_is_read_as_hybrid() {
        let mut source = Cursor::new(header(HYBRID | FINAL_BLOCK));

        assert_eq!(read_coding(&mut source), Coding::Hybrid);
        assert_eq!(source.position(), 0);
        assert_eq!(Coding::Hybrid.codec(CODEC_ID_WAVPACK), HYBRID_CODEC_ID);
        assert_eq!(Coding::Hybrid.codec(CODEC_ID_FLAC), CODEC_ID_FLAC);
    }

    #[test]
    fn a_lossless_block_or_a_stream_that_is_not_wavpack_is_read_as_lossless() {
        assert_eq!(
            read_coding(&mut Cursor::new(header(FINAL_BLOCK))),
            Coding::Lossless
        );

        let mut elsewhere = header(HYBRID);
        elsewhere[..MARKER.len()].copy_from_slice(b"fLaC");
        assert_eq!(read_coding(&mut Cursor::new(elsewhere)), Coding::Lossless);

        assert_eq!(
            read_coding(&mut Cursor::new(MARKER.to_vec())),
            Coding::Lossless
        );
        assert_eq!(Coding::Lossless.codec(CODEC_ID_WAVPACK), CODEC_ID_WAVPACK);
    }
}
