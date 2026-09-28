use std::{
    io::{Read, Seek, SeekFrom},
    num::NonZeroU32,
};

use ape_decoder::{ApeFileInfo, FrameDecoder, format::APE_FORMAT_FLAG_FLOATING_POINT};
use symphonia::core::{
    audio::{
        AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels, GenericAudioBufferRef,
        Position,
        sample::{Sample, SampleFormat, i24},
    },
    codecs::{
        CodecInfo, CodecParameters,
        audio::{
            AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
            well_known::CODEC_ID_MONKEYS_AUDIO,
        },
        registry::{RegisterableAudioDecoder, SupportedAudioCodec},
    },
    common::FourCc,
    errors::{Error, Result, SeekErrorKind, decode_error, seek_error, unsupported_error},
    formats::{
        FormatId, FormatInfo, FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo, Track,
        prelude::{ChapterGroup, MediaInfo},
        probe::{ProbeDataMatchSpec, ProbeFormatData, ProbeableFormat, Score, Scoreable},
    },
    io::{MediaSource, MediaSourceStream, ScopedStream},
    meta::{Metadata, MetadataLog},
    packet::{Packet, PacketRef},
    units::{Duration, TimeBase, Timestamp},
};

pub(crate) const APE_FORMAT_ID: FormatId = FormatId::new(FourCc::new(*b"MAC "));

const FORMAT_INFO: FormatInfo = FormatInfo {
    format: APE_FORMAT_ID,
    short_name: "ape",
    long_name: "Monkey's Audio",
};

const TRACK: u32 = 0;
const LARGEST_FRAME_BYTES: u64 = 64 << 20;
const READ_PAST_THE_FRAME: u64 = 4;
const PARAMETER_BYTES: usize = 6;

const RIFF_MAGIC: &[u8; 4] = b"RIFF";
const WAVE_MAGIC: &[u8; 4] = b"WAVE";
const FORMAT_CHUNK: &[u8; 4] = b"fmt ";
const RIFF_PREAMBLE_BYTES: usize = 12;
const CHUNK_HEADER_BYTES: usize = 8;
const EXTENSIBLE: u16 = 0xFFFE;
const EXTENSIBLE_MASK_AT: usize = 20;

const WIDEST_BITS: u32 = 32;
const MOST_BLOCKS_HELD_AHEAD: u64 = 1 << 20;
const STEREO: usize = 2;

const FLOAT_KEPT: u32 = 0xC3FF_FFFF;
const FLOAT_FLIPPED: u32 = 0x3C00_0000;
const SIGN: u32 = 1 << 31;

pub(crate) struct ApeReader<'s> {
    reader: MediaSourceStream<'s>,
    file: ApeFileInfo,
    media_info: MediaInfo,
    tracks: Vec<Track>,
    metadata: MetadataLog,
    chapters: Option<ChapterGroup>,
    time_base: TimeBase,
    next: u32,
}

impl<'s> ApeReader<'s> {
    fn try_new(mut reader: MediaSourceStream<'s>, options: FormatOptions) -> Result<Self> {
        if !reader.is_seekable() {
            return unsupported_error("ape: a Monkey's Audio stream is read by its seek table");
        }
        let file = match ape_decoder::format::parse(&mut reader) {
            Ok(file) => file,
            Err(error) => {
                tracing::debug!(%error, "a Monkey's Audio header would not parse");
                return decode_error("ape: the header would not parse");
            }
        };
        if file.header.total_frames == 0 {
            return decode_error("ape: the stream holds no frames");
        }
        if !seek_table_holds(&file) {
            return decode_error("ape: the seek table does not run forward through the file");
        }

        let header = &file.header;
        let Some(rate) = NonZeroU32::new(header.sample_rate) else {
            return decode_error("ape: the stream names no sample rate");
        };
        let time_base = TimeBase::new(NonZeroU32::MIN, rate);
        let floats = header.format_flags & APE_FORMAT_FLAG_FLOATING_POINT != 0;
        let format = match (header.bits_per_sample, floats) {
            (8, false) => SampleFormat::U8,
            (16, false) => SampleFormat::S16,
            (24, false) => SampleFormat::S24,
            (32, false) => SampleFormat::S32,
            (32, true) => SampleFormat::F32,
            _ => {
                return unsupported_error("ape: the sample width is not one Monkey's Audio writes");
            }
        };

        let mut extra = Vec::with_capacity(PARAMETER_BYTES);
        extra.extend_from_slice(&file.descriptor.version.to_le_bytes());
        extra.extend_from_slice(&header.compression_level.to_le_bytes());
        extra.extend_from_slice(&header.format_flags.to_le_bytes());

        let mut params = AudioCodecParameters::new();
        params
            .for_codec(CODEC_ID_MONKEYS_AUDIO)
            .with_sample_rate(header.sample_rate)
            .with_channels(channels_of(header.channels, &file.wav_header_data))
            .with_sample_format(format)
            .with_bits_per_sample(u32::from(header.bits_per_sample))
            .with_bits_per_coded_sample(u32::from(header.bits_per_sample))
            .with_max_frames_per_packet(u64::from(header.blocks_per_frame))
            .with_extra_data(extra.into_boxed_slice());

        let frames = u64::try_from(file.total_blocks).unwrap_or(0);
        let mut track = Track::new(TRACK);
        track
            .with_codec_params(CodecParameters::Audio(params))
            .with_time_base(time_base)
            .with_num_frames(frames)
            .with_duration(Duration::from(frames));

        Ok(Self {
            reader,
            media_info: MediaInfo::from_track(&track),
            tracks: vec![track],
            metadata: options.external_data.metadata.unwrap_or_default(),
            chapters: options.external_data.chapters,
            time_base,
            file,
            next: 0,
        })
    }

    fn remainder(&self, frame: u32) -> u64 {
        (self.file.seek_byte(frame) - self.file.seek_byte(0)) % 4
    }

    fn read_frame(&mut self, frame: u32) -> Result<Vec<u8>> {
        let bytes = self.file.frame_byte_count(frame);
        if bytes > LARGEST_FRAME_BYTES {
            return decode_error("ape: a frame claims more bytes than any encoder writes");
        }
        let remainder = self.remainder(frame);
        let Some(from) = self.file.seek_byte(frame).checked_sub(remainder) else {
            return decode_error("ape: a frame starts before the stream");
        };
        self.reader.seek(SeekFrom::Start(from))?;

        let wanted = bytes + remainder + READ_PAST_THE_FRAME;
        let mut data = Vec::with_capacity(wanted as usize + 1);
        data.push(remainder as u8);
        (&mut self.reader).take(wanted).read_to_end(&mut data)?;
        if (data.len() as u64) < 1 + bytes + remainder {
            return decode_error("ape: a frame is cut short");
        }
        Ok(data)
    }
}

fn seek_table_holds(file: &ApeFileInfo) -> bool {
    let frames = file.header.total_frames as usize;
    let Some(offsets) = file.seek_table.get(..frames) else {
        return false;
    };
    let Some(last) = offsets.last() else {
        return false;
    };
    offsets.is_sorted()
        && last
            .checked_add(u64::from(file.junk_header_bytes))
            .is_some_and(|end| end <= file.file_bytes)
}

fn channels_of(count: u16, wav_header: &[u8]) -> Channels {
    let placed = wave_mask(wav_header)
        .filter(|mask| mask.count_ones() == u32::from(count))
        .and_then(Position::from_wave_channel_mask);
    match (placed, count) {
        (Some(positions), _) => Channels::Positioned(positions),
        (None, 1) => Channels::Positioned(Position::FRONT_LEFT),
        (None, 2) => Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT),
        (None, count) => Channels::Discrete(count),
    }
}

fn wave_mask(header: &[u8]) -> Option<u32> {
    if header.get(..4)? != RIFF_MAGIC || header.get(8..12)? != WAVE_MAGIC {
        return None;
    }
    let mut at = RIFF_PREAMBLE_BYTES;
    loop {
        let id = header.get(at..at + 4)?;
        let size = u32::from_le_bytes(header.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        let body = at + CHUNK_HEADER_BYTES;
        if id == FORMAT_CHUNK {
            let chunk = header.get(body..body + size)?;
            let tag = u16::from_le_bytes(chunk.get(..2)?.try_into().ok()?);
            if tag != EXTENSIBLE {
                return None;
            }
            let mask = chunk.get(EXTENSIBLE_MASK_AT..EXTENSIBLE_MASK_AT + 4)?;
            return Some(u32::from_le_bytes(mask.try_into().ok()?));
        }
        at = body.checked_add(size)?.checked_add(size % 2)?;
    }
}

impl Scoreable for ApeReader<'_> {
    fn score(_stream: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        Ok(Score::Supported(255))
    }
}

impl ProbeableFormat<'_> for ApeReader<'_> {
    fn try_probe_new(
        stream: MediaSourceStream<'_>,
        options: FormatOptions,
    ) -> Result<Box<dyn FormatReader + '_>> {
        Ok(Box::new(ApeReader::try_new(stream, options)?))
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        &[ProbeFormatData {
            spec: ProbeDataMatchSpec {
                extensions: &["ape"],
                mime_types: &["audio/ape", "audio/x-ape"],
                markers: &[b"MAC ", b"MACF"],
            },
            info: FORMAT_INFO,
        }]
    }
}

impl FormatReader for ApeReader<'_> {
    fn format_info(&self) -> &FormatInfo {
        &FORMAT_INFO
    }

    fn media_info(&self) -> &MediaInfo {
        &self.media_info
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.metadata.metadata()
    }

    fn chapters(&self) -> Option<&ChapterGroup> {
        self.chapters.as_ref()
    }

    fn seek(&mut self, _mode: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let required = match to {
            SeekTo::Timestamp { ts, .. } => ts,
            SeekTo::Time { time, .. } => self
                .time_base
                .calc_timestamp(time)
                .ok_or(Error::SeekError(SeekErrorKind::OutOfRange))?,
        };
        let Ok(sample) = u64::try_from(required.get()) else {
            return seek_error(SeekErrorKind::OutOfRange);
        };
        let per_frame = u64::from(self.file.header.blocks_per_frame.max(1));
        let frame = sample / per_frame;
        if sample > u64::try_from(self.file.total_blocks).unwrap_or(0)
            || frame > u64::from(self.file.header.total_frames)
        {
            return seek_error(SeekErrorKind::OutOfRange);
        }
        self.next = frame as u32;
        Ok(SeekedTo {
            track_id: TRACK,
            actual_ts: Timestamp::new((frame * per_frame) as i64),
            required_ts: required,
        })
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self) -> Result<Option<Packet>> {
        let frame = self.next;
        if frame >= self.file.header.total_frames {
            return Ok(None);
        }
        let data = self.read_frame(frame)?;
        self.next += 1;
        let at = u64::from(frame) * u64::from(self.file.header.blocks_per_frame);
        let blocks = u64::from(self.file.frame_block_count(frame));
        Ok(Some(Packet::new(
            TRACK,
            Timestamp::new(at as i64),
            Duration::from(blocks),
            data,
        )))
    }

    fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s>
    where
        Self: 's,
    {
        self.reader
    }
}

enum Planes {
    Bytes(AudioBuffer<u8>),
    Short(AudioBuffer<i16>),
    Wide(AudioBuffer<i24>),
    Whole(AudioBuffer<i32>),
    Floats(AudioBuffer<f32>),
}

impl Planes {
    fn of(format: SampleFormat, spec: AudioSpec, capacity: usize) -> Result<Self> {
        Ok(match format {
            SampleFormat::U8 => Self::Bytes(AudioBuffer::new(spec, capacity)),
            SampleFormat::S16 => Self::Short(AudioBuffer::new(spec, capacity)),
            SampleFormat::S24 => Self::Wide(AudioBuffer::new(spec, capacity)),
            SampleFormat::S32 => Self::Whole(AudioBuffer::new(spec, capacity)),
            SampleFormat::F32 => Self::Floats(AudioBuffer::new(spec, capacity)),
            _ => {
                return unsupported_error(
                    "ape: the sample format is not one Monkey's Audio writes",
                );
            }
        })
    }

    fn lay(&mut self, pcm: &[u8], channels: usize) {
        match self {
            Self::Bytes(buffer) => spread(buffer, pcm, channels, 1, |bytes| bytes[0]),
            Self::Short(buffer) => spread(buffer, pcm, channels, 2, |bytes| {
                i16::from_le_bytes([bytes[0], bytes[1]])
            }),
            Self::Wide(buffer) => spread(buffer, pcm, channels, 3, |bytes| {
                i24::from(i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8)
            }),
            Self::Whole(buffer) => spread(buffer, pcm, channels, 4, |bytes| {
                i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            }),
            Self::Floats(buffer) => spread(buffer, pcm, channels, 4, |bytes| {
                f32::from_bits(float_of(u32::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
            }),
        }
    }

    fn clear(&mut self) {
        match self {
            Self::Bytes(buffer) => buffer.clear(),
            Self::Short(buffer) => buffer.clear(),
            Self::Wide(buffer) => buffer.clear(),
            Self::Whole(buffer) => buffer.clear(),
            Self::Floats(buffer) => buffer.clear(),
        }
    }

    fn as_ref(&self) -> GenericAudioBufferRef<'_> {
        match self {
            Self::Bytes(buffer) => buffer.as_generic_audio_buffer_ref(),
            Self::Short(buffer) => buffer.as_generic_audio_buffer_ref(),
            Self::Wide(buffer) => buffer.as_generic_audio_buffer_ref(),
            Self::Whole(buffer) => buffer.as_generic_audio_buffer_ref(),
            Self::Floats(buffer) => buffer.as_generic_audio_buffer_ref(),
        }
    }
}

fn spread<S: Sample>(
    buffer: &mut AudioBuffer<S>,
    pcm: &[u8],
    channels: usize,
    width: usize,
    read: impl Fn(&[u8]) -> S,
) {
    let frames = pcm.len() / (width * channels);
    buffer.clear();
    if frames > buffer.capacity() {
        buffer.grow_capacity(frames);
    }
    buffer.render_uninit(Some(frames));
    for channel in 0..channels {
        let Some(plane) = buffer.plane_mut(channel) else {
            continue;
        };
        for (frame, sample) in plane.iter_mut().enumerate() {
            let at = (frame * channels + channel) * width;
            *sample = read(&pcm[at..at + width]);
        }
    }
}

const fn float_of(stored: u32) -> u32 {
    let mut out = stored & FLOAT_KEPT;
    out |= !(stored & FLOAT_FLIPPED) ^ FLOAT_KEPT;
    if out & SIGN != 0 {
        out = !out | SIGN;
    }
    out
}

pub(crate) struct Ape {
    params: AudioCodecParameters,
    frames: FrameDecoder,
    channels: usize,
    planes: Planes,
}

impl Ape {
    fn try_new(params: &AudioCodecParameters) -> Result<Self> {
        let Some(extra) = params
            .extra_data
            .as_deref()
            .filter(|extra| extra.len() >= PARAMETER_BYTES)
        else {
            return unsupported_error("ape: the stream carries no version or level");
        };
        let version = u16::from_le_bytes([extra[0], extra[1]]);
        let level = u16::from_le_bytes([extra[2], extra[3]]);
        let (Some(rate), Some(channels), Some(bits), Some(format)) = (
            params.sample_rate,
            params.channels.clone(),
            params.bits_per_sample,
            params.sample_format,
        ) else {
            return unsupported_error("ape: the stream does not say what it holds");
        };
        let count = channels.count();
        if bits == WIDEST_BITS && count == STEREO {
            return unsupported_error(
                "ape: a 32-bit stereo stream is undone through too narrow a side",
            );
        }
        let frames = match FrameDecoder::new(
            version,
            u16::try_from(count).unwrap_or(0),
            u16::try_from(bits).unwrap_or(0),
            level,
        ) {
            Ok(frames) => frames,
            Err(error) => {
                tracing::debug!(%error, "a Monkey's Audio decoder would not be built");
                return unsupported_error("ape: the stream is not one this decoder reads");
            }
        };
        let capacity = params
            .max_frames_per_packet
            .unwrap_or(0)
            .min(MOST_BLOCKS_HELD_AHEAD) as usize;

        Ok(Self {
            params: params.clone(),
            frames,
            channels: count,
            planes: Planes::of(format, AudioSpec::new(rate, channels), capacity)?,
        })
    }
}

impl AudioDecoder for Ape {
    fn reset(&mut self) {
        self.planes.clear();
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs()[0].info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        let Some((&remainder, frame)) = packet.data.split_first() else {
            return decode_error("ape: an empty packet");
        };
        let Ok(blocks) = usize::try_from(packet.dur.get()) else {
            return decode_error("ape: a frame too long to hold");
        };
        let pcm = match self
            .frames
            .decode_frame(frame, u32::from(remainder), blocks)
        {
            Ok(pcm) => pcm,
            Err(error) => {
                self.planes.clear();
                tracing::debug!(%error, "a Monkey's Audio frame would not decode");
                return decode_error("ape: a frame would not decode");
            }
        };
        self.planes.lay(&pcm, self.channels);
        Ok(self.planes.as_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.planes.as_ref()
    }
}

impl RegisterableAudioDecoder for Ape {
    fn try_registry_new(
        params: &AudioCodecParameters,
        _options: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>> {
        Ok(Box::new(Self::try_new(params)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[SupportedAudioCodec {
            id: CODEC_ID_MONKEYS_AUDIO,
            info: CodecInfo {
                short_name: "ape",
                long_name: "Monkey's Audio",
                profiles: &[],
            },
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_float_is_turned_back_into_the_bits_it_was_made_from() {
        for value in [0.25_f32, -0.75, 1.0e-30, -3.0e-38, 0.0, 1.5] {
            let bits = value.to_bits();
            assert_eq!(float_of(float_of(bits)), bits, "{value} did not survive");
        }
    }

    #[test]
    fn a_wave_header_places_the_channels_its_mask_names() {
        let mut fmt = vec![0_u8; 40];
        fmt[..2].copy_from_slice(&EXTENSIBLE.to_le_bytes());
        fmt[EXTENSIBLE_MASK_AT..EXTENSIBLE_MASK_AT + 4].copy_from_slice(&0x3F_u32.to_le_bytes());
        let mut header = b"RIFF\0\0\0\0WAVE".to_vec();
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        header.extend_from_slice(&fmt);

        assert_eq!(wave_mask(&header), Some(0x3F));
        assert!(matches!(channels_of(6, &header), Channels::Positioned(_)));
        assert!(matches!(channels_of(6, b"FORM"), Channels::Discrete(6)));
    }
}
