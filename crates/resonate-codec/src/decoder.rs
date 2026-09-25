use std::{
    fmt,
    io::{self, Read, Seek},
};

use resonate_core::{
    AudioBuffer, FrameSpan, Frames, MediaLocation, SampleData, SampleFormat, StreamSpec,
};
use symphonia::core::{
    audio::GenericAudioSlice,
    codecs::audio::{AudioDecoder, AudioDecoderOptions},
    errors,
    formats::{FormatReader, SeekMode, SeekTo},
    packet::Packet,
    units::Timestamp,
};

use crate::{
    AudioCodecId, CodecOp, CueFile, Error, FormatId, Result, Speakers, StreamTrackId, TagSet,
    container::{self, Opened},
    cue,
    dsd::{self, DSD_BLOCK_FRAMES, Packing},
    source::{FormatHint, Media, Reading as SourceReading, Sources},
    stream::PacketSpan,
    timeline::Timeline,
};

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecodeStatus {
    Decoded,
    EndOfStream,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Container {
    Wave,
    Aiff,
    Caf,
    Mpeg,
    Adts,
    Ogg,
    Flac,
    Dsf,
    Dff,
    IsoMp4,
    Matroska,
    #[default]
    Unknown,
}

impl Container {
    pub fn from_id(id: FormatId) -> Self {
        use symphonia::core::formats::well_known::{
            FORMAT_ID_ADTS, FORMAT_ID_AIFF, FORMAT_ID_CAF, FORMAT_ID_FLAC, FORMAT_ID_ISOMP4,
            FORMAT_ID_MKV, FORMAT_ID_MP1, FORMAT_ID_MP2, FORMAT_ID_MP3, FORMAT_ID_OGG,
            FORMAT_ID_WAVE,
        };

        if id == crate::dsd::DSF_FORMAT_ID {
            return Self::Dsf;
        }
        if id == crate::dsd::DFF_FORMAT_ID {
            return Self::Dff;
        }

        match id {
            FORMAT_ID_WAVE => Self::Wave,
            FORMAT_ID_AIFF => Self::Aiff,
            FORMAT_ID_CAF => Self::Caf,
            FORMAT_ID_MP1 | FORMAT_ID_MP2 | FORMAT_ID_MP3 => Self::Mpeg,
            FORMAT_ID_ADTS => Self::Adts,
            FORMAT_ID_OGG => Self::Ogg,
            FORMAT_ID_FLAC => Self::Flac,
            FORMAT_ID_ISOMP4 => Self::IsoMp4,
            FORMAT_ID_MKV => Self::Matroska,
            _ => Self::Unknown,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wave => "WAVE",
            Self::Aiff => "AIFF",
            Self::Caf => "CAF",
            Self::Mpeg => "MPEG audio",
            Self::Adts => "ADTS",
            Self::Ogg => "Ogg",
            Self::Flac => "FLAC",
            Self::Dsf => "DSF",
            Self::Dff => "DSDIFF",
            Self::IsoMp4 => "ISO-BMFF",
            Self::Matroska => "Matroska",
            Self::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for Container {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Codec {
    Flac,
    Alac,
    Dsd,
    Pcm,
    Aac,
    Mp3,
    Vorbis,
    Opus,
    #[default]
    Unknown,
}

impl Codec {
    pub const ALL: [Self; 9] = [
        Self::Flac,
        Self::Alac,
        Self::Dsd,
        Self::Pcm,
        Self::Aac,
        Self::Mp3,
        Self::Vorbis,
        Self::Opus,
        Self::Unknown,
    ];

    pub const fn is_lossless(self) -> bool {
        match self {
            Self::Flac | Self::Alac | Self::Dsd | Self::Pcm => true,
            Self::Aac | Self::Mp3 | Self::Vorbis | Self::Opus | Self::Unknown => false,
        }
    }

    pub fn from_id(id: AudioCodecId) -> Self {
        use symphonia::core::codecs::audio::well_known::{
            CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP1, CODEC_ID_MP2, CODEC_ID_MP3,
            CODEC_ID_OPUS, CODEC_ID_PCM_ALAW, CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE,
            CODEC_ID_PCM_F64BE, CODEC_ID_PCM_F64LE, CODEC_ID_PCM_MULAW, CODEC_ID_PCM_S8,
            CODEC_ID_PCM_S16BE, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24BE, CODEC_ID_PCM_S24LE,
            CODEC_ID_PCM_S32BE, CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U8, CODEC_ID_PCM_U16LE,
            CODEC_ID_PCM_U24LE, CODEC_ID_PCM_U32LE, CODEC_ID_VORBIS,
        };

        if id == crate::dsd::DSD_CODEC_ID {
            return Self::Dsd;
        }

        match id {
            CODEC_ID_FLAC => Self::Flac,
            CODEC_ID_ALAC => Self::Alac,
            CODEC_ID_AAC => Self::Aac,
            CODEC_ID_MP1 | CODEC_ID_MP2 | CODEC_ID_MP3 => Self::Mp3,
            CODEC_ID_VORBIS => Self::Vorbis,
            CODEC_ID_OPUS => Self::Opus,
            CODEC_ID_PCM_S32LE | CODEC_ID_PCM_S32BE | CODEC_ID_PCM_S24LE | CODEC_ID_PCM_S24BE
            | CODEC_ID_PCM_S16LE | CODEC_ID_PCM_S16BE | CODEC_ID_PCM_S8 | CODEC_ID_PCM_U32LE
            | CODEC_ID_PCM_U24LE | CODEC_ID_PCM_U16LE | CODEC_ID_PCM_U8 | CODEC_ID_PCM_F32LE
            | CODEC_ID_PCM_F32BE | CODEC_ID_PCM_F64LE | CODEC_ID_PCM_F64BE | CODEC_ID_PCM_ALAW
            | CODEC_ID_PCM_MULAW => Self::Pcm,
            _ => Self::Unknown,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flac => "FLAC",
            Self::Alac => "ALAC",
            Self::Dsd => "DSD",
            Self::Pcm => "PCM",
            Self::Aac => "AAC",
            Self::Mp3 => "MP3",
            Self::Vorbis => "Vorbis",
            Self::Opus => "Opus",
            Self::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub container: FormatId,
    pub codec: AudioCodecId,
    pub spec: StreamSpec,
    pub speakers: Speakers,
    pub duration: Option<Frames>,
    pub encoder_delay: u32,
    pub encoder_padding: u32,
    pub playable: Option<FrameSpan>,
    pub bits_per_coded_sample: Option<u8>,
    pub is_seekable: bool,
    pub packing: Packing,
    pub tags: TagSet,
    pub cue: Option<CueFile>,
}

impl MediaInfo {
    pub fn priming(&self) -> Frames {
        self.playable.map_or(Frames::ZERO, FrameSpan::start)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Delivery {
    pub format: SampleFormat,
    pub packing: Packing,
}

impl Delivery {
    pub const fn samples(format: SampleFormat) -> Self {
        Self {
            format,
            packing: Packing::Samples,
        }
    }
}

struct Coded {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: StreamTrackId,
    pending: usize,
    consumed: usize,
    ahead: Option<Packet>,
    trailing: usize,
    first_ts: Option<Timestamp>,
}

impl Coded {
    fn next_packet(&mut self, location: &MediaLocation) -> Result<Option<Packet>> {
        if let Some(packet) = self.ahead.take() {
            return Ok(Some(packet));
        }
        loop {
            match self.reader.next_packet() {
                Ok(Some(packet)) if packet.track_id == self.track.0 => {
                    self.first_ts.get_or_insert(packet.pts);
                    return Ok(Some(packet));
                }
                Ok(Some(_)) => {}
                Ok(None) => return Ok(None),
                Err(errors::Error::IoError(source))
                    if source.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    tracing::debug!("stream ends mid-packet; treating it as the end of the track");
                    return Ok(None);
                }
                Err(source) => {
                    return Err(Error::from_symphonia(source, CodecOp::ReadPacket, location));
                }
            }
        }
    }

    fn is_last(&mut self, location: &MediaLocation) -> bool {
        match self.next_packet(location) {
            Ok(ahead) => {
                self.ahead = ahead;
                self.ahead.is_none()
            }
            Err(source) => {
                tracing::debug!(%source, "a packet read ahead failed; it is read again in turn");
                false
            }
        }
    }
}

enum Held {
    Coded(Box<Coded>),
    Dsd(Box<dsd::Stream>),
}

pub struct Decoder {
    location: MediaLocation,
    reading: Held,
    info: MediaInfo,
    timeline: Timeline,
    delivery: Delivery,
    position: Frames,
    origin: Frames,
    limit: Option<Frames>,
    last_packet: Option<PacketSpan>,
}

impl Decoder {
    pub fn open(sources: &Sources, location: &MediaLocation) -> Result<(Self, MediaInfo)> {
        if let Some(stood) = Self::stood_in(sources, location, None) {
            return Ok(stood);
        }
        Self::build(container::open_media(sources, location)?, location)
    }

    fn stood_in(
        sources: &Sources,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<(Self, MediaInfo)> {
        let stood = sources.stood_in(location, span)?;
        let opened = container::open_media(sources, &stood.location)
            .and_then(|opened| Self::build(opened, &stood.location));
        match opened {
            Ok((mut decoder, _)) => {
                decoder.info.tags = stood.tags;
                decoder.info.cue = None;
                let info = decoder.info.clone();
                Some((decoder, info))
            }
            Err(error) => {
                tracing::debug!(%error, %location, "what stands in for a row would not open; decoding the row itself");
                None
            }
        }
    }

    pub fn open_span(
        sources: &Sources,
        location: &MediaLocation,
        span: FrameSpan,
    ) -> Result<(Self, MediaInfo)> {
        if let Some(stood) = Self::stood_in(sources, location, Some(span)) {
            return Ok(stood);
        }
        let (mut decoder, _) = Self::build(container::open_media(sources, location)?, location)?;
        let cut = cue::cut_for(sources, location, &decoder.info);
        let info = decoder.confine(span, cut.as_ref())?;
        Ok((decoder, info))
    }

    pub fn open_reader<R>(reader: R, location: &MediaLocation) -> Result<(Self, MediaInfo)>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let media = Media {
            stream: Box::new(SourceReading::new(reader)),
            hint: FormatHint::of(location),
        };
        Self::build(container::open(media, location)?, location)
    }

    #[cfg(test)]
    pub fn open_reader_span<R>(
        reader: R,
        location: &MediaLocation,
        span: FrameSpan,
    ) -> Result<(Self, MediaInfo)>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let (mut decoder, _) = Self::open_reader(reader, location)?;
        let cut = decoder.info.cue.clone();
        let info = decoder.confine(span, cut.as_ref())?;
        Ok((decoder, info))
    }

    fn confine(&mut self, span: FrameSpan, cut: Option<&CueFile>) -> Result<MediaInfo> {
        let held = span.within(self.info.duration.unwrap_or(Frames(u64::MAX)));
        let start = held.start();

        if self.info.is_seekable {
            let landed = self.seek_reader(start)?;
            self.restart(landed)?;
        }
        if self.position < start {
            self.discard_to(start)?;
        }

        self.origin = start;
        self.limit = held.end();
        self.info.duration = held.frames();
        if let Some(track) = cut.and_then(|cut| cut.cut_at(start, self.info.spec.rate)) {
            self.info.tags = track.titled();
        }
        Ok(self.info.clone())
    }

    fn build(opened: Opened, location: &MediaLocation) -> Result<(Self, MediaInfo)> {
        let info = opened.media_info(location)?;

        let (reading, timeline) = match opened {
            Opened::Dsd(held) => {
                let timeline = Timeline::at(info.spec.rate);
                let stream = dsd::Stream::over(held.bytes, held.layout, info.packing);
                (Held::Dsd(Box::new(stream)), timeline)
            }
            Opened::Coded(coded) => {
                let (track, params) = container::audio_track(coded.reader.as_ref(), location)?;
                let id = StreamTrackId(track.id);

                let decoder = crate::opus::codecs()
                    .make_audio_decoder(params, &untrimmed())
                    .map_err(|source| match source {
                        errors::Error::Unsupported(_) => Error::NoDecoder {
                            location: location.clone(),
                            track: id,
                            codec: params.codec,
                        },
                        source => Error::from_symphonia(source, CodecOp::Decode, location),
                    })?;

                let timeline = container::timeline(track, &info);
                (
                    Held::Coded(Box::new(Coded {
                        reader: coded.reader,
                        decoder,
                        track: id,
                        pending: 0,
                        consumed: 0,
                        ahead: None,
                        trailing: padding_past_an_open_window(&info),
                        first_ts: None,
                    })),
                    timeline,
                )
            }
        };

        let mut decoder = Self {
            location: location.clone(),
            reading,
            delivery: Delivery {
                format: info.spec.format,
                packing: info.packing,
            },
            info: info.clone(),
            timeline,
            position: Frames::ZERO,
            origin: Frames::ZERO,
            limit: None,
            last_packet: None,
        };
        decoder.open_on_the_music()?;

        Ok((decoder, info))
    }

    fn open_on_the_music(&mut self) -> Result<()> {
        let Some(playable) = self.info.playable else {
            if matches!(self.reading, Held::Dsd(_)) {
                self.limit = self.info.duration;
            }
            return Ok(());
        };

        self.skip(playable.start().get())?;
        self.limit = playable.frames();
        Ok(())
    }

    pub fn info(&self) -> &MediaInfo {
        &self.info
    }

    pub fn position(&self) -> Frames {
        self.position.saturating_sub(self.origin)
    }

    fn remaining(&self) -> Option<u64> {
        self.limit
            .map(|limit| limit.get().saturating_sub(self.position.get()))
    }

    pub const fn timeline(&self) -> Timeline {
        self.timeline
    }

    pub const fn last_packet(&self) -> Option<PacketSpan> {
        self.last_packet
    }

    pub const fn delivery(&self) -> Delivery {
        self.delivery
    }

    pub fn set_output_format(&mut self, format: SampleFormat) {
        self.deliver(Delivery::samples(format));
    }

    pub fn deliver(&mut self, delivery: Delivery) {
        let wanted = match &self.reading {
            Held::Coded(_) => Delivery::samples(delivery.format),
            Held::Dsd(_) => match delivery.packing {
                Packing::DopMarked(rate) => Delivery {
                    format: SampleFormat::S24,
                    packing: Packing::DopMarked(rate),
                },
                Packing::Samples => Delivery::samples(delivery.format),
            },
        };
        if wanted == self.delivery {
            return;
        }

        self.delivery = wanted;
        if let Held::Dsd(held) = &mut self.reading {
            held.deliver(wanted.packing);
        }
    }

    pub fn next_block(&mut self, out: &mut AudioBuffer) -> Result<DecodeStatus> {
        out.set_spec(self.output_spec());

        if self.remaining() == Some(0) {
            out.set_frames(0);
            return Ok(DecodeStatus::EndOfStream);
        }

        let wanted = self.remaining();
        let frames = match &self.reading {
            Held::Coded(_) => self.coded_block(out, wanted)?,
            Held::Dsd(_) => self.dsd_block(out, wanted)?,
        };

        if frames == 0 {
            out.set_frames(0);
            return Ok(DecodeStatus::EndOfStream);
        }
        self.position = self.position.saturating_add(Frames(frames as u64));
        Ok(DecodeStatus::Decoded)
    }

    fn coded_block(&mut self, out: &mut AudioBuffer, wanted: Option<u64>) -> Result<usize> {
        if self.pending() == 0 && self.fill()? == DecodeStatus::EndOfStream {
            return Ok(0);
        }

        let Held::Coded(coded) = &mut self.reading else {
            return Ok(0);
        };
        let taking = match wanted {
            Some(remaining) => (remaining as usize).min(coded.pending),
            None => coded.pending,
        };

        let decoded = coded.decoder.last_decoded();
        let block = decoded.slice(coded.consumed..coded.consumed + taking);
        let frames = block.frames();

        out.set_frames(frames);
        copy_out(&block, out.data_mut());

        coded.consumed = coded.consumed.saturating_add(frames);
        coded.pending = coded.pending.saturating_sub(frames);
        Ok(frames)
    }

    fn dsd_block(&mut self, out: &mut AudioBuffer, wanted: Option<u64>) -> Result<usize> {
        let Held::Dsd(held) = &mut self.reading else {
            return Ok(0);
        };
        held.next_block(out, wanted, DSD_BLOCK_FRAMES)
    }

    fn pending(&self) -> usize {
        match &self.reading {
            Held::Coded(coded) => coded.pending,
            Held::Dsd(_) => 0,
        }
    }

    pub fn seek(&mut self, to: Frames) -> Result<Frames> {
        if !self.info.is_seekable {
            return Err(Error::NotSeekable {
                location: self.location.clone(),
            });
        }
        let Some(duration) = self.info.duration else {
            return Err(Error::UnknownDuration {
                location: self.location.clone(),
                track: self.track(),
            });
        };
        if to > duration {
            return Err(Error::SeekOutOfRange {
                requested: to,
                duration,
            });
        }

        let wanted = self.origin.saturating_add(to);
        let landed = self.seek_reader(wanted)?;
        self.restart(landed)?;
        self.discard_to(wanted)?;

        Ok(self.position())
    }

    pub fn reset(&mut self) {
        match &mut self.reading {
            Held::Coded(coded) => {
                coded.decoder.reset();
                coded.pending = 0;
                coded.consumed = 0;
                coded.ahead = None;
            }
            Held::Dsd(held) => held.reset(),
        }
        self.last_packet = None;
    }

    fn restart(&mut self, landed: Landing) -> Result<()> {
        self.reset();
        self.position = landed.at;
        self.skip(landed.short_of_the_music.get())?;
        Ok(())
    }

    fn track(&self) -> StreamTrackId {
        match &self.reading {
            Held::Coded(coded) => coded.track,
            Held::Dsd(_) => StreamTrackId(0),
        }
    }

    fn output_spec(&self) -> StreamSpec {
        StreamSpec::new(
            self.info.spec.rate,
            self.info.spec.channels,
            self.delivery.format,
        )
    }

    fn fill(&mut self) -> Result<DecodeStatus> {
        let Self {
            location,
            reading,
            info,
            timeline,
            last_packet,
            ..
        } = self;
        let rate = info.spec.rate;
        let channels = usize::from(info.spec.channel_count().get());
        let timeline = *timeline;

        let Held::Coded(coded) = reading else {
            return Ok(DecodeStatus::EndOfStream);
        };

        loop {
            let Some(packet) = coded.next_packet(location)? else {
                return Ok(DecodeStatus::EndOfStream);
            };
            let span = PacketSpan::of(&packet, &timeline);

            let decoded = match coded.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(errors::Error::DecodeError(reason)) => {
                    tracing::debug!(reason, "discarding an undecodable packet");
                    continue;
                }
                Err(source) => {
                    return Err(Error::from_symphonia(source, CodecOp::Decode, location));
                }
            };

            let mut frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            if decoded.spec().rate() != rate.hz() || decoded.spec().channels().count() != channels {
                return Err(Error::ResetRequired {
                    location: location.clone(),
                });
            }
            if coded.trailing > 0 && coded.is_last(location) {
                frames = frames.saturating_sub(coded.trailing);
                if frames == 0 {
                    return Ok(DecodeStatus::EndOfStream);
                }
            }

            coded.pending = frames;
            coded.consumed = 0;
            *last_packet = Some(span);
            return Ok(DecodeStatus::Decoded);
        }
    }

    fn discard_to(&mut self, to: Frames) -> Result<()> {
        let skipped = self.skip(to.get().saturating_sub(self.position.get()))?;
        self.position = self.position.saturating_add(Frames(skipped));
        Ok(())
    }

    fn skip(&mut self, frames: u64) -> Result<u64> {
        let mut left = frames;

        while left > 0 {
            let skipped = match &mut self.reading {
                Held::Dsd(held) => held.skip(left),
                Held::Coded(_) => {
                    if self.pending() == 0 && self.fill()? == DecodeStatus::EndOfStream {
                        break;
                    }
                    let Held::Coded(coded) = &mut self.reading else {
                        break;
                    };
                    let taking = left.min(coded.pending as u64) as usize;
                    coded.consumed += taking;
                    coded.pending -= taking;
                    taking as u64
                }
            };

            if skipped == 0 {
                break;
            }
            left = left.saturating_sub(skipped);
        }

        Ok(frames - left)
    }

    fn seek_reader(&mut self, to: Frames) -> Result<Landing> {
        let Self {
            location,
            reading,
            timeline,
            info,
            ..
        } = self;
        let timeline = *timeline;

        let coded = match reading {
            Held::Dsd(held) => {
                return held.seek(to).map(|at| Landing {
                    at,
                    short_of_the_music: Frames::ZERO,
                });
            }
            Held::Coded(coded) => coded,
        };
        let to = to.saturating_sub(crate::opus::pre_roll(info.codec));

        let seek_to = match timeline.timestamp(to) {
            Some(ts) => SeekTo::Timestamp {
                ts,
                track_id: coded.track.0,
            },
            None => SeekTo::Time {
                time: timeline.elapsed(to),
                track_id: Some(coded.track.0),
            },
        };

        let landed = coded
            .reader
            .seek(SeekMode::Accurate, seek_to)
            .map_err(|source| Error::from_symphonia(source, CodecOp::Seek, location))?;

        let on_the_first_packet = coded.first_ts == Some(landed.actual_ts);
        let short_of_the_music = if on_the_first_packet && !timeline.is_sample_accurate() {
            info.priming()
        } else {
            timeline.short_of_the_music(landed.actual_ts)
        };
        Ok(Landing {
            at: timeline.frames(landed.actual_ts),
            short_of_the_music,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Landing {
    at: Frames,
    short_of_the_music: Frames,
}

fn padding_past_an_open_window(info: &MediaInfo) -> usize {
    match info.playable {
        Some(window) if window.frames().is_none() => info.encoder_padding as usize,
        Some(_) | None => 0,
    }
}

fn untrimmed() -> AudioDecoderOptions {
    AudioDecoderOptions::default().gapless(false)
}

fn copy_out(block: &GenericAudioSlice<'_>, data: &mut SampleData) {
    match data {
        SampleData::S16(samples) => {
            block.copy_to_slice_interleaved::<i16, _>(samples.as_mut_slice())
        }
        SampleData::S32(samples) => {
            block.copy_to_slice_interleaved::<i32, _>(samples.as_mut_slice())
        }
        SampleData::F32(samples) => {
            block.copy_to_slice_interleaved::<f32, _>(samples.as_mut_slice())
        }
        SampleData::S24(samples) => {
            block.copy_to_slice_interleaved::<i32, _>(samples.as_mut_slice());
            for sample in samples {
                *sample >>= 8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::Cursor,
        path::PathBuf,
        process,
        sync::atomic::{self, AtomicU32},
    };

    use resonate_core::{ChannelLayout, SampleRate};

    use super::*;

    const RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const EXTENSIBLE: u16 = 0xFFFE;
    const EXTENSIBLE_EXTRA_BYTES: u16 = 22;
    const STEREO_MASK: u32 = 0x3;
    const PCM_SUBFORMAT: [u8; 16] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
        0x71,
    ];
    const FRAMES: usize = 40_000;

    struct Wav {
        bits: u16,
        valid_bits: Option<u16>,
        float: bool,
        id3: Vec<u8>,
        info: Vec<u8>,
    }

    impl Wav {
        fn pcm(bits: u16) -> Self {
            Self {
                bits,
                valid_bits: None,
                float: false,
                id3: Vec::new(),
                info: Vec::new(),
            }
        }

        fn using(mut self, valid_bits: u16) -> Self {
            self.valid_bits = Some(valid_bits);
            self
        }

        fn with_info(mut self, id: &[u8; 4], value: &str) -> Self {
            if self.info.is_empty() {
                self.info.extend_from_slice(b"INFO");
            }
            let mut terminated = value.as_bytes().to_vec();
            terminated.push(0);
            chunk(&mut self.info, id, &terminated);
            self
        }

        fn with_text(mut self, id: &[u8; 4], value: &str) -> Self {
            let mut data = vec![UTF8];
            data.extend_from_slice(value.as_bytes());
            frame(&mut self.id3, id, &data);
            self
        }

        fn with_user_text(mut self, description: &str, value: &str) -> Self {
            let mut data = vec![UTF8];
            data.extend_from_slice(description.as_bytes());
            data.push(0);
            data.extend_from_slice(value.as_bytes());
            frame(&mut self.id3, b"TXXX", &data);
            self
        }

        fn build(&self, samples: &[i32]) -> Cursor<Vec<u8>> {
            let bytes_per_sample = usize::from(self.bits / 8);
            let mut data = Vec::with_capacity(samples.len() * bytes_per_sample);
            for sample in samples {
                if self.float {
                    data.extend_from_slice(&(*sample as f32).to_le_bytes());
                } else {
                    let encoded = sample.to_le_bytes();
                    data.extend_from_slice(&encoded[..bytes_per_sample]);
                }
            }

            let tag = match self.valid_bits {
                Some(_) => EXTENSIBLE,
                None => u16::from(self.float as u8 * 2 + 1),
            };

            let mut fmt = Vec::new();
            fmt.extend_from_slice(&tag.to_le_bytes());
            fmt.extend_from_slice(&CHANNELS.to_le_bytes());
            fmt.extend_from_slice(&RATE.to_le_bytes());
            let block_align = CHANNELS * self.bits / 8;
            fmt.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
            fmt.extend_from_slice(&block_align.to_le_bytes());
            fmt.extend_from_slice(&self.bits.to_le_bytes());
            if let Some(valid_bits) = self.valid_bits {
                fmt.extend_from_slice(&EXTENSIBLE_EXTRA_BYTES.to_le_bytes());
                fmt.extend_from_slice(&valid_bits.to_le_bytes());
                fmt.extend_from_slice(&STEREO_MASK.to_le_bytes());
                fmt.extend_from_slice(&PCM_SUBFORMAT);
            }

            let mut body = Vec::new();
            body.extend_from_slice(b"WAVE");
            chunk(&mut body, b"fmt ", &fmt);
            if !self.info.is_empty() {
                chunk(&mut body, b"LIST", &self.info);
            }
            chunk(&mut body, b"data", &data);

            let mut file = Vec::new();
            if !self.id3.is_empty() {
                file.extend_from_slice(b"ID3\x04\x00\x00");
                file.extend_from_slice(&synchsafe(self.id3.len() as u32));
                file.extend_from_slice(&self.id3);
            }
            file.extend_from_slice(b"RIFF");
            file.extend_from_slice(&(body.len() as u32).to_le_bytes());
            file.extend_from_slice(&body);
            Cursor::new(file)
        }
    }

    const UTF8: u8 = 3;

    fn frame(into: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(&synchsafe(data.len() as u32));
        into.extend_from_slice(&[0, 0]);
        into.extend_from_slice(data);
    }

    fn synchsafe(value: u32) -> [u8; 4] {
        [
            ((value >> 21) & 0x7f) as u8,
            ((value >> 14) & 0x7f) as u8,
            ((value >> 7) & 0x7f) as u8,
            (value & 0x7f) as u8,
        ]
    }

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn ramp(bits: u16) -> Vec<i32> {
        let span = 1_i32 << (bits - 1);
        (0..FRAMES * usize::from(CHANNELS))
            .map(|n| (n as i32 % (2 * span - 1)) - span + 1)
            .collect()
    }

    struct Piped(Cursor<Vec<u8>>);

    impl Read for Piped {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Seek for Piped {
        fn seek(&mut self, _: io::SeekFrom) -> io::Result<u64> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "a pipe"))
        }
    }

    fn piped(wav: Cursor<Vec<u8>>) -> (Decoder, MediaInfo) {
        Decoder::open_reader(Piped(wav), &MediaLocation::local("held.wav"))
            .expect("a well-formed wav opens over a pipe")
    }

    fn open(wav: Cursor<Vec<u8>>) -> (Decoder, MediaInfo) {
        Decoder::open_reader(wav, &MediaLocation::local("held.wav"))
            .expect("a well-formed wav opens")
    }

    fn drain(decoder: &mut Decoder, out: &mut AudioBuffer) -> Vec<i32> {
        let mut collected = Vec::new();
        while decoder.next_block(out).expect("a well-formed wav decodes") == DecodeStatus::Decoded {
            collected.extend(integers(out));
        }
        collected
    }

    fn integers(buffer: &AudioBuffer) -> Vec<i32> {
        match buffer.data() {
            SampleData::S16(samples) => samples.iter().map(|sample| i32::from(*sample)).collect(),
            SampleData::S24(samples) | SampleData::S32(samples) => samples.clone(),
            other => panic!("expected an integer format, got {other:?}"),
        }
    }

    #[test]
    fn sixteen_bit_pcm_survives_the_decode_path_sample_for_sample() {
        let samples = ramp(16);
        let (mut decoder, info) = open(Wav::pcm(16).build(&samples));

        assert_eq!(info.spec.rate, SampleRate::HZ_44100);
        assert_eq!(info.spec.channels, ChannelLayout::Stereo);
        assert_eq!(info.spec.format, SampleFormat::S16);
        assert_eq!(info.duration, Some(Frames(FRAMES as u64)));
        assert!(info.is_seekable);

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(drain(&mut decoder, &mut out), samples);
        assert_eq!(decoder.position(), Frames(FRAMES as u64));
    }

    #[test]
    fn twenty_four_bit_pcm_arrives_sign_extended_in_the_low_bits_of_an_i32() {
        let samples = ramp(24);
        let (mut decoder, info) = open(Wav::pcm(24).build(&samples));

        assert_eq!(info.spec.format, SampleFormat::S24);
        assert_eq!(info.bits_per_coded_sample, Some(24));

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(drain(&mut decoder, &mut out), samples);
    }

    #[test]
    fn a_wav_reports_the_bits_it_uses_rather_than_the_width_it_pads_them_to() {
        let samples = ramp(24);
        let (_, info) = open(Wav::pcm(24).using(20).build(&samples));

        assert_eq!(info.bits_per_coded_sample, Some(20));
        assert_eq!(info.spec.format, SampleFormat::S24);
    }

    #[test]
    fn the_destination_buffer_stops_reallocating_once_warm() {
        let samples = ramp(16);
        let (mut decoder, info) = open(Wav::pcm(16).build(&samples));

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(
            decoder.next_block(&mut out).expect("a first block"),
            DecodeStatus::Decoded
        );
        assert_eq!(
            decoder.next_block(&mut out).expect("a second block"),
            DecodeStatus::Decoded
        );
        let store = out.as_bytes().as_ptr();

        for _ in 0..8 {
            assert_eq!(
                decoder.next_block(&mut out).expect("a further block"),
                DecodeStatus::Decoded
            );
            assert_eq!(out.as_bytes().as_ptr(), store, "the block buffer moved");
        }
    }

    #[test]
    fn seeking_lands_on_the_exact_requested_frame() {
        let samples = ramp(16);
        let (mut decoder, info) = open(Wav::pcm(16).build(&samples));
        let mut out = AudioBuffer::empty(info.spec);

        for target in [1_u64, 999, 4_097, 31_337] {
            let landed = decoder.seek(Frames(target)).expect("a seekable wav seeks");
            assert_eq!(landed, Frames(target));
            assert_eq!(decoder.position(), Frames(target));

            assert_eq!(
                decoder
                    .next_block(&mut out)
                    .expect("a block after the seek"),
                DecodeStatus::Decoded
            );
            let first = integers(&out);
            let expected = target as usize * usize::from(CHANNELS);
            assert_eq!(
                first.first().copied(),
                samples.get(expected).copied(),
                "seek to {target} landed on the wrong frame"
            );
        }
    }

    #[test]
    fn a_seek_past_the_end_is_refused_rather_than_clamped() {
        let (mut decoder, _) = open(Wav::pcm(16).build(&ramp(16)));

        let refused = decoder.seek(Frames(FRAMES as u64 + 1));
        assert!(matches!(refused, Err(Error::SeekOutOfRange { .. })));
    }

    #[test]
    fn the_output_format_can_be_forced_to_f32_for_the_dsp_chain() {
        let samples = ramp(16);
        let (mut decoder, info) = open(Wav::pcm(16).build(&samples));
        decoder.set_output_format(SampleFormat::F32);

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(
            decoder.next_block(&mut out).expect("a block"),
            DecodeStatus::Decoded
        );

        assert_eq!(out.spec().format, SampleFormat::F32);
        let decoded = out.as_f32().expect("an f32 buffer");
        let expected = samples[0] as f32 / SampleFormat::S16.full_scale();
        assert!((decoded[0] - expected).abs() < 1e-6, "got {}", decoded[0]);
    }

    #[test]
    fn tags_found_ahead_of_the_container_reach_the_tag_set() {
        let wav = Wav::pcm(16)
            .with_text(b"TIT2", "Several Species")
            .with_text(b"TPE1", "Pink Floyd")
            .with_text(b"TALB", "Ummagumma")
            .with_text(b"TRCK", "5")
            .with_text(b"TDRC", "1969-10-25")
            .with_user_text("REPLAYGAIN_TRACK_GAIN", "-7.06 dB")
            .with_user_text("REPLAYGAIN_TRACK_PEAK", "0.98765")
            .build(&ramp(16));

        let (_, info) = open(wav);

        assert_eq!(info.tags.title.as_deref(), Some("Several Species"));
        assert_eq!(info.tags.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(info.tags.album.as_deref(), Some("Ummagumma"));
        assert_eq!(info.tags.track_number, Some(5));
        assert_eq!(info.tags.date.as_deref(), Some("1969-10-25"));
        assert_eq!(
            info.tags.replay_gain.track_gain,
            Some(resonate_core::Decibels::new(-7.06).expect("finite"))
        );
        assert_eq!(info.tags.replay_gain.track_peak, Some(0.98765));
    }

    fn spanned(wav: Cursor<Vec<u8>>, span: FrameSpan) -> (Decoder, MediaInfo) {
        Decoder::open_reader_span(wav, &MediaLocation::local("held.wav"), span)
            .expect("a well-formed wav opens to a span")
    }

    fn slice_of(samples: &[i32], span: FrameSpan) -> Vec<i32> {
        let channels = usize::from(CHANNELS);
        let start = span.start().get() as usize * channels;
        let end = span
            .end()
            .map_or(samples.len(), |end| end.get() as usize * channels);
        samples[start..end.min(samples.len())].to_vec()
    }

    #[test]
    fn a_decoder_confined_to_a_span_reports_the_span_as_its_whole_length() {
        let span = FrameSpan::between(Frames(1_000), Frames(4_000));
        let (decoder, info) = spanned(Wav::pcm(16).build(&ramp(16)), span);

        assert_eq!(info.duration, Some(Frames(3_000)));
        assert_eq!(decoder.position(), Frames::ZERO);
    }

    #[test]
    fn a_decoder_confined_to_a_span_hands_back_exactly_those_frames() {
        let samples = ramp(16);
        let span = FrameSpan::between(Frames(1_000), Frames(4_000));
        let (mut decoder, info) = spanned(Wav::pcm(16).build(&samples), span);

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(drain(&mut decoder, &mut out), slice_of(&samples, span));
        assert_eq!(decoder.position(), Frames(3_000));
    }

    #[test]
    fn a_span_with_no_end_runs_to_the_end_of_the_file() {
        let samples = ramp(16);
        let span = FrameSpan::starting(Frames(FRAMES as u64 - 500));
        let (mut decoder, info) = spanned(Wav::pcm(16).build(&samples), span);

        assert_eq!(info.duration, Some(Frames(500)));

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(drain(&mut decoder, &mut out), slice_of(&samples, span));
    }

    #[test]
    fn a_span_reaching_past_the_file_is_cut_back_to_what_it_holds() {
        let samples = ramp(16);
        let span = FrameSpan::between(Frames(FRAMES as u64 - 200), Frames(FRAMES as u64 + 9_000));
        let (mut decoder, info) = spanned(Wav::pcm(16).build(&samples), span);

        assert_eq!(info.duration, Some(Frames(200)));

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(
            drain(&mut decoder, &mut out).len(),
            200 * usize::from(CHANNELS)
        );
    }

    const SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.wav" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Echoes"
    REM REPLAYGAIN_TRACK_GAIN -7.06 dB
    INDEX 01 00:00:30
"#;

    struct Rip {
        folder: PathBuf,
    }

    impl Rip {
        fn cut_by(sheet: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let folder = env::temp_dir().join(format!(
                "resonate-rip-{}-{}",
                process::id(),
                NEXT.fetch_add(1, atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&folder).expect("a writable temporary folder");
            fs::write(
                folder.join("Meddle.wav"),
                Wav::pcm(16).build(&ramp(16)).into_inner(),
            )
            .expect("a writable temporary file");
            fs::write(folder.join("Meddle.cue"), sheet).expect("a writable temporary file");

            Self { folder }
        }

        fn file(&self) -> MediaLocation {
            MediaLocation::local(self.folder.join("Meddle.wav"))
        }

        fn second_row(&self) -> FrameSpan {
            cue::read(SHEET.as_bytes()).files[0]
                .span_of(1, SampleRate::HZ_44100, Some(Frames(FRAMES as u64)))
                .expect("the sheet cuts a second row")
        }
    }

    impl Drop for Rip {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    #[test]
    fn a_row_cut_by_the_sheet_beside_the_file_is_opened_as_the_track_that_sheet_names() {
        let rip = Rip::cut_by(SHEET);
        let span = rip.second_row();

        let (_, info) = Decoder::open_span(&Sources::local(), &rip.file(), span)
            .expect("a wav a sheet beside it cuts");

        assert_eq!(info.tags.title.as_deref(), Some("Echoes"));
        assert_eq!(info.tags.album.as_deref(), Some("Meddle"));
        assert_eq!(info.tags.track_number, Some(2));
        assert_eq!(
            info.tags.replay_gain.track_gain,
            resonate_core::Decibels::new(-7.06).ok(),
            "the row plays at the gain the file declares rather than its own"
        );
        assert_eq!(info.duration, span.frames());
    }

    #[test]
    fn a_row_probed_for_its_tags_reads_the_same_as_the_one_opened_to_play() {
        let rip = Rip::cut_by(SHEET);
        let span = rip.second_row();

        let read = crate::probe_span(&Sources::local(), &rip.file(), span)
            .expect("a wav a sheet beside it cuts");
        let (_, opened) = Decoder::open_span(&Sources::local(), &rip.file(), span)
            .expect("a wav a sheet beside it cuts");

        assert_eq!(read.tags, opened.tags);
        assert_eq!(read.duration, opened.duration);
    }

    #[test]
    fn a_file_no_sheet_beside_it_names_keeps_the_tags_it_carries() {
        let rip =
            Rip::cut_by("FILE \"Elsewhere.wav\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n");
        let span = rip.second_row();

        let read = crate::probe_span(&Sources::local(), &rip.file(), span).expect("a plain wav");

        assert_eq!(read.tags.title, None);
        assert_eq!(read.duration, span.frames());
    }

    #[test]
    fn a_seek_inside_a_span_is_measured_from_where_the_span_starts() {
        let samples = ramp(16);
        let span = FrameSpan::between(Frames(1_000), Frames(4_000));
        let (mut decoder, info) = spanned(Wav::pcm(16).build(&samples), span);

        assert_eq!(
            decoder.seek(Frames(500)).expect("a seek inside"),
            Frames(500)
        );
        assert_eq!(decoder.position(), Frames(500));

        let mut out = AudioBuffer::empty(info.spec);
        let rest = drain(&mut decoder, &mut out);
        let wanted = slice_of(&samples, FrameSpan::between(Frames(1_500), Frames(4_000)));
        assert_eq!(rest, wanted);
    }

    #[test]
    fn a_seek_past_the_span_is_refused_though_the_file_runs_on() {
        let span = FrameSpan::between(Frames(1_000), Frames(4_000));
        let (mut decoder, _) = spanned(Wav::pcm(16).build(&ramp(16)), span);

        assert!(matches!(
            decoder.seek(Frames(3_500)),
            Ok(Frames(3_500)) | Err(Error::SeekOutOfRange { .. })
        ));
        assert!(matches!(
            decoder.seek(Frames(9_000)),
            Err(Error::SeekOutOfRange { .. })
        ));
    }

    #[test]
    fn a_source_that_cannot_seek_keeps_the_tags_a_seekable_one_would_have() {
        let wav = Wav::pcm(16)
            .with_info(b"INAM", "Speak to Me")
            .with_info(b"IART", "Pink Floyd")
            .build(&ramp(16));

        let (_, piped) = piped(wav.clone());
        let (_, seekable) = open(wav);

        assert!(!piped.is_seekable);
        assert_eq!(piped.tags.title.as_deref(), Some("Speak to Me"));
        assert_eq!(piped.tags.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(piped.tags, seekable.tags);
    }

    #[test]
    fn a_source_that_cannot_seek_still_decodes_every_sample_it_holds() {
        let samples = ramp(16);
        let (mut decoder, info) = piped(Wav::pcm(16).build(&samples));

        let mut out = AudioBuffer::empty(info.spec);
        assert_eq!(drain(&mut decoder, &mut out), samples);
    }

    #[test]
    fn a_riff_info_list_reaches_the_tag_set_symphonia_never_exposes() {
        let wav = Wav::pcm(16)
            .with_info(b"INAM", "Speak to Me")
            .with_info(b"IART", "Pink Floyd")
            .with_info(b"IPRD", "The Dark Side of the Moon")
            .with_info(b"IGNR", "Progressive Rock")
            .with_info(b"ICRD", "1973-03-01")
            .with_info(b"ITRK", "1/10")
            .build(&ramp(16));

        let (_, info) = open(wav);

        assert_eq!(info.tags.title.as_deref(), Some("Speak to Me"));
        assert_eq!(info.tags.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(
            info.tags.album.as_deref(),
            Some("The Dark Side of the Moon")
        );
        assert_eq!(info.tags.genre.as_deref(), Some("Progressive Rock"));
        assert_eq!(info.tags.date.as_deref(), Some("1973-03-01"));
        assert_eq!(info.tags.track_number, Some(1));
    }

    #[test]
    fn the_info_ids_that_credit_a_recording_reach_the_tag_set_the_same_way() {
        let wav = Wav::pcm(16)
            .with_info(b"INAM", "Us and Them")
            .with_info(b"IMUS", "Richard Wright")
            .with_info(b"IWRI", "Roger Waters")
            .with_info(b"IENG", "Alan Parsons")
            .with_info(b"IPRO", "Pink Floyd")
            .with_info(b"ICOP", "(c) 1973 Harvest")
            .with_info(b"ISFT", "Exact Audio Copy")
            .with_info(b"ICMT", "ripped from the 1994 remaster")
            .build(&ramp(16));

        let (_, info) = open(wav);
        let tags = &info.tags;

        assert_eq!(tags.title.as_deref(), Some("Us and Them"));
        assert_eq!(tags.credits.composer.as_deref(), Some("Richard Wright"));
        assert_eq!(tags.credits.lyricist.as_deref(), Some("Roger Waters"));
        assert_eq!(tags.credits.engineer.as_deref(), Some("Alan Parsons"));
        assert_eq!(tags.credits.producer.as_deref(), Some("Pink Floyd"));
        assert_eq!(tags.copyright.as_deref(), Some("(c) 1973 Harvest"));
        assert_eq!(tags.encoder.as_deref(), Some("Exact Audio Copy"));
        assert_eq!(
            tags.comment.as_deref(),
            Some("ripped from the 1994 remaster")
        );
    }

    #[test]
    fn an_id3_tag_outranks_the_info_list_it_shares_a_file_with() {
        let wav = Wav::pcm(16)
            .with_info(b"INAM", "Untitled")
            .with_info(b"IART", "Unknown Artist")
            .with_text(b"TIT2", "Echoes")
            .build(&ramp(16));

        let (_, info) = open(wav);

        assert_eq!(info.tags.title.as_deref(), Some("Echoes"));
        assert_eq!(info.tags.artist.as_deref(), Some("Unknown Artist"));
    }

    #[test]
    fn probing_a_file_reports_the_same_stream_the_decoder_will_produce() {
        let path = std::env::temp_dir().join("resonate-probe-agreement.wav");
        std::fs::write(
            &path,
            Wav::pcm(24)
                .with_text(b"TIT2", "Echoes")
                .build(&ramp(24))
                .into_inner(),
        )
        .expect("a writable temp dir");

        let sources = Sources::local();
        let location = MediaLocation::local(&path);
        let probed = crate::probe(&sources, &location).expect("a well-formed wav probes");
        let (_, opened) = Decoder::open(&sources, &location).expect("a well-formed wav opens");
        let _ = std::fs::remove_file(&path);

        assert_eq!(probed, opened);
        assert_eq!(probed.spec.format, SampleFormat::S24);
        assert_eq!(probed.tags.title.as_deref(), Some("Echoes"));
    }

    struct Standing {
        row: MediaLocation,
        span: Option<FrameSpan>,
        object: MediaLocation,
    }

    impl crate::StandIn for Standing {
        fn stands_in(
            &self,
            location: &MediaLocation,
            span: Option<FrameSpan>,
        ) -> Option<crate::StoodIn> {
            (*location == self.row && span == self.span).then(|| crate::StoodIn {
                location: self.object.clone(),
                tags: TagSet {
                    title: Some("Echoes".to_owned()),
                    lyrics: Some("Overhead the albatross".to_owned()),
                    ..TagSet::default()
                },
            })
        }
    }

    #[test]
    fn a_row_something_stands_in_for_is_decoded_from_it_under_the_tags_it_was_given() {
        let object = std::env::temp_dir().join(format!("resonate-stood-in-{}.wav", process::id()));
        let samples = ramp(16);
        fs::write(&object, Wav::pcm(16).build(&samples).into_inner()).expect("a writable temp dir");

        let row = MediaLocation::local("/music/gone/Meddle.flac");
        let span = FrameSpan::between(Frames(44_100), Frames(88_200));
        let sources = Sources::local().standing_in(std::sync::Arc::new(Standing {
            row: row.clone(),
            span: Some(span),
            object: MediaLocation::local(&object),
        }));

        let (mut decoder, info) =
            Decoder::open_span(&sources, &row, span).expect("the object stands in");
        let probed = crate::probe_span(&sources, &row, span).expect("the object stands in");
        let mut out = AudioBuffer::empty(info.spec);
        let decoded = drain(&mut decoder, &mut out);
        let _ = fs::remove_file(&object);

        assert_eq!(
            decoded, samples,
            "the span was applied to an object that is the span"
        );
        assert_eq!(info.tags.title.as_deref(), Some("Echoes"));
        assert_eq!(info.tags.lyrics.as_deref(), Some("Overhead the albatross"));
        assert_eq!(probed, info);
        assert!(
            Decoder::open(&sources, &row).is_err(),
            "a row read whole was stood in for by a cut's object"
        );
    }

    #[test]
    fn a_stream_that_ends_mid_packet_ends_the_track_rather_than_failing_it() {
        let samples = ramp(16);
        let mut truncated = Wav::pcm(16).build(&samples).into_inner();
        truncated.truncate(truncated.len() / 2);

        let (mut decoder, info) = open(Cursor::new(truncated));
        let mut out = AudioBuffer::empty(info.spec);

        let decoded = drain(&mut decoder, &mut out);
        assert!(!decoded.is_empty(), "nothing decoded before the truncation");
        assert!(decoded.len() < samples.len());
        assert_eq!(decoded.as_slice(), &samples[..decoded.len()]);
    }

    #[test]
    fn a_stream_that_is_not_a_container_is_named_as_such() {
        let opened = Decoder::open_reader(
            Cursor::new(vec![0_u8; 8192]),
            &MediaLocation::local("held.bin"),
        );

        assert!(matches!(opened, Err(Error::UnrecognisedContainer { .. })));
    }
}
