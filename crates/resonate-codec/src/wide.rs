use std::{
    io::{Seek, SeekFrom},
    num::NonZeroU32,
};

use symphonia::core::{
    audio::{Channels, Position},
    codecs::{
        CodecParameters,
        audio::{
            AudioCodecId, AudioCodecParameters,
            well_known::{
                CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F64LE, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE,
                CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U8,
            },
        },
    },
    common::FourCc,
    errors::{Error, Result, SeekErrorKind, decode_error, seek_error, unsupported_error},
    formats::{
        FormatId, FormatInfo, FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo, Track,
        prelude::{ChapterGroup, MediaInfo},
        probe::{ProbeDataMatchSpec, ProbeFormatData, ProbeableFormat, Score, Scoreable},
    },
    io::{MediaSource, MediaSourceStream, ReadBytes, ScopedStream},
    meta::{Metadata, MetadataLog},
    packet::Packet,
    units::{Duration, TimeBase, Timestamp},
};

pub(crate) const RF64_FORMAT_ID: FormatId = FormatId::new(FourCc::new(*b"RF64"));
pub(crate) const WAVE64_FORMAT_ID: FormatId = FormatId::new(FourCc::new(*b"W64 "));

const RF64_INFO: FormatInfo = FormatInfo {
    format: RF64_FORMAT_ID,
    short_name: "rf64",
    long_name: "RF64 / BW64 Waveform Audio",
};
const WAVE64_INFO: FormatInfo = FormatInfo {
    format: WAVE64_FORMAT_ID,
    short_name: "w64",
    long_name: "Sony Wave64",
};

const RF64: &[u8; 4] = b"RF64";
const BW64: &[u8; 4] = b"BW64";
const WAVE: &[u8; 4] = b"WAVE";
const WAVE_AT: usize = 8;
pub(crate) const DS64: &[u8; 4] = b"ds64";
pub(crate) const DATA: &[u8; 4] = b"data";
pub(crate) const FMT: &[u8; 4] = b"fmt ";

const WAVE64_RIFF: [u8; 16] = [
    0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00,
];
const WAVE64_WAVE: [u8; 16] = [
    0x77, 0x61, 0x76, 0x65, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
const WAVE64_CHUNK_TAIL: [u8; 12] = [
    0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
const WAVE64_WAVE_AT: usize = 24;
const GUID_BYTES: usize = 16;
const ID_BYTES: usize = 4;

const RF64_HEADER_BYTES: u64 = 12;
const WAVE64_HEADER_BYTES: u64 = 40;
const RF64_CHUNK_HEADER_BYTES: usize = 8;
const WAVE64_CHUNK_HEADER_BYTES: usize = 24;
pub(crate) const WIDEST_CHUNK_HEADER: usize = WAVE64_CHUNK_HEADER_BYTES;
const WAVE64_ALIGNMENT: u64 = 8;
const RF64_ALIGNMENT: u64 = 2;
const UNSTATED: u32 = u32::MAX;

const DS64_DATA_AT: usize = 8;
const DS64_TABLE_LENGTH_AT: usize = 24;
const DS64_TABLE_AT: usize = 28;
const DS64_ENTRY_BYTES: usize = 12;
pub(crate) const MOST_DS64_BYTES: u64 = 1 << 12;
const MOST_FMT_BYTES: u64 = 1 << 10;
const MOST_CHUNKS: usize = 4_096;

const PCM: u16 = 1;
const IEEE_FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xFFFE;
const FMT_PLAIN_BYTES: usize = 16;
const FMT_EXTENSIBLE_BYTES: usize = 40;
const FMT_CHANNELS_AT: usize = 2;
const FMT_RATE_AT: usize = 4;
const FMT_BITS_AT: usize = 14;
const FMT_VALID_BITS_AT: usize = 18;
const FMT_MASK_AT: usize = 20;
const FMT_SUBFORMAT_AT: usize = 24;
const SUBFORMAT_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

const TRACK: u32 = 0;
const FRAMES_A_PACKET: u64 = 4_096;
const MOST_PACKET_BYTES: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wide {
    Rf64,
    Wave64,
}

impl Wide {
    pub(crate) fn at(from: &[u8]) -> Option<Self> {
        let marker = from.get(..ID_BYTES)?;
        if marker == RF64 || marker == BW64 {
            return (from.get(WAVE_AT..WAVE_AT + ID_BYTES) == Some(WAVE.as_slice()))
                .then_some(Self::Rf64);
        }
        let wave = from.get(WAVE64_WAVE_AT..WAVE64_WAVE_AT + GUID_BYTES);
        (from.get(..GUID_BYTES) == Some(WAVE64_RIFF.as_slice())
            && wave == Some(WAVE64_WAVE.as_slice()))
        .then_some(Self::Wave64)
    }

    pub(crate) const fn header_bytes(self) -> u64 {
        match self {
            Self::Rf64 => RF64_HEADER_BYTES,
            Self::Wave64 => WAVE64_HEADER_BYTES,
        }
    }

    pub(crate) const fn chunk_header_bytes(self) -> usize {
        match self {
            Self::Rf64 => RF64_CHUNK_HEADER_BYTES,
            Self::Wave64 => WAVE64_CHUNK_HEADER_BYTES,
        }
    }

    pub(crate) const fn padded(self, size: u64) -> u64 {
        let alignment = match self {
            Self::Rf64 => RF64_ALIGNMENT,
            Self::Wave64 => WAVE64_ALIGNMENT,
        };
        size.saturating_add((alignment - size % alignment) % alignment)
    }

    pub(crate) fn chunk(self, header: &[u8], sizes: &Sizes) -> Option<ChunkHeader> {
        match self {
            Self::Rf64 => {
                let id: [u8; ID_BYTES] = header.get(..ID_BYTES)?.try_into().ok()?;
                let stated = u32::from_le_bytes(
                    header
                        .get(ID_BYTES..RF64_CHUNK_HEADER_BYTES)?
                        .try_into()
                        .ok()?,
                );
                let size = match stated {
                    UNSTATED => sizes.of(&id)?,
                    stated => u64::from(stated),
                };
                Some(ChunkHeader { id: Some(id), size })
            }
            Self::Wave64 => {
                let guid = header.get(..GUID_BYTES)?;
                let whole = u64::from_le_bytes(
                    header
                        .get(GUID_BYTES..WAVE64_CHUNK_HEADER_BYTES)?
                        .try_into()
                        .ok()?,
                );
                let size = whole.checked_sub(WAVE64_CHUNK_HEADER_BYTES as u64)?;
                let id = (guid.get(ID_BYTES..) == Some(WAVE64_CHUNK_TAIL.as_slice()))
                    .then(|| guid.get(..ID_BYTES)?.try_into().ok())
                    .flatten();
                Some(ChunkHeader { id, size })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChunkHeader {
    pub(crate) id: Option<[u8; ID_BYTES]>,
    pub(crate) size: u64,
}

impl ChunkHeader {
    pub(crate) fn is(&self, id: &[u8; ID_BYTES]) -> bool {
        self.id.as_ref() == Some(id)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sizes {
    data: Option<u64>,
    table: Vec<([u8; ID_BYTES], u64)>,
}

impl Sizes {
    pub(crate) fn read(body: &[u8]) -> Self {
        let data = body
            .get(DS64_DATA_AT..DS64_DATA_AT + 8)
            .and_then(|held| held.try_into().ok())
            .map(u64::from_le_bytes);
        let entries = body
            .get(DS64_TABLE_LENGTH_AT..DS64_TABLE_AT)
            .and_then(|held| held.try_into().ok())
            .map_or(0, u32::from_le_bytes);
        let table = body
            .get(DS64_TABLE_AT..)
            .unwrap_or_default()
            .as_chunks::<DS64_ENTRY_BYTES>()
            .0
            .iter()
            .take(entries as usize)
            .map(|entry| {
                let (id, size) = entry
                    .split_first_chunk::<ID_BYTES>()
                    .unwrap_or((&[0; ID_BYTES], &[0; 8]));
                (
                    *id,
                    size.first_chunk::<8>()
                        .map_or(0, |held| u64::from_le_bytes(*held)),
                )
            })
            .collect();
        Self { data, table }
    }

    fn of(&self, id: &[u8; ID_BYTES]) -> Option<u64> {
        if id == DATA {
            return self.data;
        }
        self.table
            .iter()
            .find(|(held, _)| held == id)
            .map(|(_, size)| *size)
    }
}

struct Format {
    codec: AudioCodecId,
    channels: Channels,
    rate: NonZeroU32,
    frame_bytes: u64,
    bits: Option<u32>,
}

impl Format {
    fn read(fields: &[u8]) -> Result<Self> {
        let field = |at: usize| -> Option<u16> {
            Some(u16::from_le_bytes(fields.get(at..at + 2)?.try_into().ok()?))
        };
        if fields.len() < FMT_PLAIN_BYTES {
            return decode_error("wide: the format chunk is cut short");
        }
        let (Some(tag), Some(count), Some(bits)) =
            (field(0), field(FMT_CHANNELS_AT), field(FMT_BITS_AT))
        else {
            return decode_error("wide: the format chunk is cut short");
        };
        let rate = fields
            .get(FMT_RATE_AT..FMT_RATE_AT + 4)
            .and_then(|held| held.try_into().ok())
            .map(u32::from_le_bytes)
            .and_then(NonZeroU32::new);
        let Some(rate) = rate else {
            return decode_error("wide: the format names no sample rate");
        };
        if count == 0 {
            return decode_error("wide: the format names no channels");
        }

        let (floats, valid, mask) = match tag {
            PCM => (false, bits, None),
            IEEE_FLOAT => (true, bits, None),
            EXTENSIBLE => {
                if fields.len() < FMT_EXTENSIBLE_BYTES {
                    return decode_error("wide: an extensible format chunk is cut short");
                }
                let subformat = fields.get(FMT_SUBFORMAT_AT..FMT_EXTENSIBLE_BYTES);
                let Some((kind, tail)) = subformat.and_then(|held| held.split_first_chunk::<2>())
                else {
                    return decode_error("wide: an extensible format chunk is cut short");
                };
                if tail != SUBFORMAT_TAIL {
                    return unsupported_error("wide: the samples are in a form not read here");
                }
                let floats = match u16::from_le_bytes(*kind) {
                    PCM => false,
                    IEEE_FLOAT => true,
                    _ => return unsupported_error("wide: the samples are in a form not read here"),
                };
                let valid = field(FMT_VALID_BITS_AT).filter(|valid| (1..=bits).contains(valid));
                let mask = fields
                    .get(FMT_MASK_AT..FMT_MASK_AT + 4)
                    .and_then(|held| held.try_into().ok())
                    .map(u32::from_le_bytes);
                (floats, valid.unwrap_or(bits), mask)
            }
            _ => return unsupported_error("wide: the samples are in a form not read here"),
        };

        let codec = match (floats, bits) {
            (false, 8) => CODEC_ID_PCM_U8,
            (false, 16) => CODEC_ID_PCM_S16LE,
            (false, 24) => CODEC_ID_PCM_S24LE,
            (false, 32) => CODEC_ID_PCM_S32LE,
            (true, 32) => CODEC_ID_PCM_F32LE,
            (true, 64) => CODEC_ID_PCM_F64LE,
            _ => return unsupported_error("wide: the sample width is not one read here"),
        };

        Ok(Self {
            codec,
            channels: channels_of(count, mask),
            rate,
            frame_bytes: u64::from(count) * u64::from(bits / 8),
            bits: (!floats).then_some(u32::from(valid)),
        })
    }
}

fn channels_of(count: u16, mask: Option<u32>) -> Channels {
    mask.filter(|mask| mask.count_ones() == u32::from(count))
        .and_then(Position::from_wave_channel_mask)
        .or_else(|| Position::from_count(u32::from(count)))
        .map_or(Channels::Discrete(count), Channels::Positioned)
}

pub(crate) struct WideReader<'s> {
    reader: MediaSourceStream<'s>,
    wide: Wide,
    media_info: MediaInfo,
    tracks: Vec<Track>,
    metadata: MetadataLog,
    chapters: Option<ChapterGroup>,
    frame_bytes: u64,
    frames: u64,
    data_start: u64,
    data_end: u64,
}

impl<'s> WideReader<'s> {
    fn try_new(mut reader: MediaSourceStream<'s>, options: FormatOptions) -> Result<Self> {
        let mut header = [0_u8; WAVE64_HEADER_BYTES as usize];
        reader.read_buf_exact(&mut header[..RF64_HEADER_BYTES as usize])?;
        let wide = match Wide::at(&header) {
            Some(Wide::Rf64) => Wide::Rf64,
            _ => {
                reader.read_buf_exact(&mut header[RF64_HEADER_BYTES as usize..])?;
                match Wide::at(&header) {
                    Some(wide) => wide,
                    None => return unsupported_error("wide: neither an RF64 nor a Wave64 header"),
                }
            }
        };

        let mut sizes = Sizes::default();
        let mut format = None;
        let mut chunk = [0_u8; WAVE64_CHUNK_HEADER_BYTES];
        for walked in 0..MOST_CHUNKS {
            let header = &mut chunk[..wide.chunk_header_bytes()];
            reader.read_buf_exact(header)?;
            let Some(held) = wide.chunk(header, &sizes) else {
                return decode_error("wide: a chunk states a size the file cannot mean");
            };

            let sizing = walked == 0 && held.is(DS64);
            let read_up_to = match held.id.as_ref() {
                Some(DS64) if sizing => Some(MOST_DS64_BYTES),
                Some(FMT) => Some(MOST_FMT_BYTES),
                _ => None,
            };
            let body = match read_up_to {
                Some(most) => {
                    let wanted = held.size.min(most);
                    let mut body = vec![0_u8; wanted as usize];
                    reader.read_buf_exact(&mut body)?;
                    reader.ignore_bytes(wide.padded(held.size) - wanted)?;
                    Some(body)
                }
                None => None,
            };

            match (held.id.as_ref(), body) {
                (Some(DS64), Some(body)) => sizes = Sizes::read(&body),
                (Some(FMT), Some(body)) => format = Some(Format::read(&body)?),
                (Some(DATA), _) => {
                    let Some(format) = format else {
                        return decode_error("wide: the data chunk comes before its format");
                    };
                    return Ok(Self::over(reader, wide, format, held.size, options));
                }
                _ => reader.ignore_bytes(wide.padded(held.size))?,
            }
        }

        decode_error("wide: no data chunk within the chunks walked")
    }

    fn over(
        reader: MediaSourceStream<'s>,
        wide: Wide,
        format: Format,
        data_bytes: u64,
        options: FormatOptions,
    ) -> Self {
        let frames = data_bytes / format.frame_bytes.max(1);
        let data_start = reader.pos();

        let mut params = AudioCodecParameters::new();
        params
            .for_codec(format.codec)
            .with_sample_rate(format.rate.get())
            .with_channels(format.channels)
            .with_max_frames_per_packet(FRAMES_A_PACKET)
            .with_frames_per_block(1);
        if let Some(bits) = format.bits {
            params
                .with_bits_per_sample(bits)
                .with_bits_per_coded_sample(bits);
        }

        let mut track = Track::new(TRACK);
        track
            .with_codec_params(CodecParameters::Audio(params))
            .with_time_base(TimeBase::new(NonZeroU32::MIN, format.rate))
            .with_num_frames(frames)
            .with_duration(Duration::from(frames));

        Self {
            reader,
            wide,
            media_info: MediaInfo::from_track(&track),
            tracks: vec![track],
            metadata: options.external_data.metadata.unwrap_or_default(),
            chapters: options.external_data.chapters,
            frame_bytes: format.frame_bytes,
            frames,
            data_start,
            data_end: data_start.saturating_add(frames * format.frame_bytes),
        }
    }
}

impl Scoreable for WideReader<'_> {
    fn score(mut stream: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        let mut header = [0_u8; WAVE64_HEADER_BYTES as usize];
        stream.read_buf_exact(&mut header)?;
        Ok(match Wide::at(&header) {
            Some(_) => Score::Supported(255),
            None => Score::Unsupported,
        })
    }
}

impl ProbeableFormat<'_> for WideReader<'_> {
    fn try_probe_new(
        stream: MediaSourceStream<'_>,
        options: FormatOptions,
    ) -> Result<Box<dyn FormatReader + '_>> {
        Ok(Box::new(WideReader::try_new(stream, options)?))
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        &[
            ProbeFormatData {
                spec: ProbeDataMatchSpec {
                    extensions: &["rf64"],
                    mime_types: &[],
                    markers: &[RF64, BW64],
                },
                info: RF64_INFO,
            },
            ProbeFormatData {
                spec: ProbeDataMatchSpec {
                    extensions: &["w64"],
                    mime_types: &[],
                    markers: &[b"riff"],
                },
                info: WAVE64_INFO,
            },
        ]
    }
}

impl FormatReader for WideReader<'_> {
    fn format_info(&self) -> &FormatInfo {
        match self.wide {
            Wide::Rf64 => &RF64_INFO,
            Wide::Wave64 => &WAVE64_INFO,
        }
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
                .tracks
                .first()
                .and_then(|track| track.time_base)
                .and_then(|base| base.calc_timestamp(time))
                .ok_or(Error::SeekError(SeekErrorKind::OutOfRange))?,
        };
        let Ok(frame) = u64::try_from(required.get()) else {
            return seek_error(SeekErrorKind::OutOfRange);
        };
        if frame > self.frames {
            return seek_error(SeekErrorKind::OutOfRange);
        }

        let at = self.data_start + frame * self.frame_bytes;
        if self.reader.is_seekable() {
            self.reader.seek(SeekFrom::Start(at))?;
        } else {
            let Some(ahead) = at.checked_sub(self.reader.pos()) else {
                return seek_error(SeekErrorKind::ForwardOnly);
            };
            self.reader.ignore_bytes(ahead)?;
        }
        Ok(SeekedTo {
            track_id: TRACK,
            actual_ts: required,
            required_ts: required,
        })
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self) -> Result<Option<Packet>> {
        let at = self.reader.pos();
        let left = self.data_end.saturating_sub(at) / self.frame_bytes;
        if left == 0 {
            return Ok(None);
        }
        let frames = left
            .min(FRAMES_A_PACKET)
            .min((MOST_PACKET_BYTES / self.frame_bytes).max(1));
        let Ok(stamp) = i64::try_from((at - self.data_start) / self.frame_bytes) else {
            return Ok(None);
        };

        let data = self
            .reader
            .read_boxed_slice_exact((frames * self.frame_bytes) as usize)?;
        Ok(Some(Packet::new(
            TRACK,
            Timestamp::new(stamp),
            Duration::from(frames),
            data,
        )))
    }

    fn into_inner<'r>(self: Box<Self>) -> MediaSourceStream<'r>
    where
        Self: 'r,
    {
        self.reader
    }
}
