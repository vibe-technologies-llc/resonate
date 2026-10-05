use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    sync::Arc,
    time::{Duration, Instant},
};

use resonate_core::{
    ChannelCount, ChannelLayout, FrameSpan, Frames, MediaLocation, SampleFormat, SampleRate,
    StreamSpec,
};
use symphonia::core::{
    audio::{Channels, Position, sample::SampleFormat as SymphoniaSampleFormat},
    codecs::{
        CodecParameters,
        audio::{
            AudioCodecId, AudioCodecParameters,
            well_known::{
                CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_OPUS, CODEC_ID_PCM_F32BE,
                CODEC_ID_PCM_F32BE_PLANAR, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F32LE_PLANAR,
                CODEC_ID_PCM_F64BE, CODEC_ID_PCM_F64BE_PLANAR, CODEC_ID_PCM_F64LE,
                CODEC_ID_PCM_F64LE_PLANAR,
            },
        },
    },
    errors,
    formats::{FormatId, FormatOptions, FormatReader, Track, TrackType, probe::Hint},
    io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions},
    meta::{MetadataOptions, Visual},
    units::{Duration as Ticks, Timestamp},
};

use crate::{
    CodecOp, Container, Error, MediaInfo, Result, Speakers, StreamTrackId, TrackProperty,
    boxes::{self, Priming},
    caf, chapters,
    cue::{self, CueFile, CueSheet},
    dsd::{self, Packing},
    matroska::Voided,
    opus,
    prescan::Prescan,
    riff::{self, Riff},
    source::{FormatHint, Media, MediaStream, Reading, Replaying, Sources},
    spool::{SPOOLED_ON_DISC_AT_MOST, Spool},
    tags::{self, Revisions, TagSet},
    timeline::Timeline,
};

const MP3_DECODER_DELAY: u32 = 529;
const READ_BY_SEEKING: [(&[u8; 4], Container); 4] = [
    (b"DSD ", Container::Dsf),
    (b"FRM8", Container::Dff),
    (b"MAC ", Container::MonkeysAudio),
    (b"MACF", Container::MonkeysAudio),
];
const ALAC_ATOM_BYTES: usize = 12;
const ALAC_ATOM_IDS: [&[u8; 4]; 2] = [b"frma", b"alac"];
const ALAC_COOKIE_BYTES: [usize; 2] = [24, 48];
const ALAC_BIT_DEPTH_AT: usize = 5;
const ALAC_MAX_BIT_DEPTH: u32 = 32;

const SPOOLED_AT_MOST: u64 = 256 << 20;
const SPOOLED_WITHIN: Duration = Duration::from_millis(750);
const SPOOLED_A_READ_AT_A_TIME: usize = 64 << 10;
const NAMEABLE_WAVE_CHANNELS: u32 = Position::all().bits().count_ones();

const STREAMINFO_BYTES: usize = 34;
const STREAMINFO_BIT_DEPTH_AT: usize = 103;
const STREAMINFO_BIT_DEPTH_BITS: u32 = 5;

pub(crate) struct Coded {
    pub(crate) reader: Box<dyn FormatReader>,
    pub(crate) seekable: bool,
    pub(crate) prescan: Prescan,
    pub(crate) revisions: Revisions,
    pub(crate) chunk_pictures: Vec<Visual>,
    pub(crate) spool: Option<Arc<Spool>>,
}

pub(crate) struct OpenedDsd {
    pub(crate) bytes: Box<dyn MediaStream>,
    pub(crate) layout: dsd::Layout,
    pub(crate) seekable: bool,
    pub(crate) tags: TagSet,
    pub(crate) visuals: Vec<Visual>,
    pub(crate) spool: Option<Arc<Spool>>,
}

pub(crate) enum Pictures<'a> {
    Read {
        reader: &'a mut dyn FormatReader,
        chunk: &'a [Visual],
    },
    Held(&'a [Visual]),
}

pub(crate) enum Opened {
    Coded(Box<Coded>),
    Dsd(Box<OpenedDsd>),
}

impl Opened {
    pub(crate) fn spool(&self) -> Option<Arc<Spool>> {
        match self {
            Self::Coded(coded) => coded.spool.clone(),
            Self::Dsd(held) => held.spool.clone(),
        }
    }

    pub(crate) fn into_coded(self) -> Option<Coded> {
        match self {
            Self::Coded(coded) => Some(*coded),
            Self::Dsd(_) => None,
        }
    }

    pub(crate) fn pictures_mut(&mut self) -> Pictures<'_> {
        match self {
            Self::Coded(coded) => Pictures::Read {
                reader: coded.reader.as_mut(),
                chunk: &coded.chunk_pictures,
            },
            Self::Dsd(held) => Pictures::Held(&held.visuals),
        }
    }

    pub(crate) fn media_info(&self, location: &MediaLocation) -> Result<MediaInfo> {
        match self {
            Self::Coded(coded) => {
                let (track, params) = audio_track(coded.reader.as_ref(), location)?;
                coded_info(coded, track, params, location)
            }
            Self::Dsd(held) => Ok(dsd::info(&held.layout, held.seekable, held.tags.clone())),
        }
    }
}

struct Probed {
    bytes: Box<dyn MediaStream>,
    at: u64,
    voided: Option<Voided>,
}

impl Probed {
    fn over(mut bytes: Box<dyn MediaStream>, voided: Option<Voided>) -> Self {
        let at = match voided {
            Some(_) => bytes.stream_position().unwrap_or(0),
            None => 0,
        };
        Self { bytes, at, voided }
    }

    fn lay_the_void_over(&self, read: &mut [u8]) {
        let Some(voided) = &self.voided else {
            return;
        };
        let read_end = self.at.saturating_add(read.len() as u64);
        let void_end = voided.at.saturating_add(voided.header.len() as u64);
        let from = self.at.max(voided.at);
        let to = read_end.min(void_end);
        for position in from..to {
            let (Ok(into), Ok(held)) = (
                usize::try_from(position - self.at),
                usize::try_from(position - voided.at),
            ) else {
                continue;
            };
            if let (Some(into), Some(held)) = (read.get_mut(into), voided.header.get(held)) {
                *into = *held;
            }
        }
    }
}

impl Read for Probed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.bytes.read(buf)?;
        if self.voided.is_some() {
            self.lay_the_void_over(buf.get_mut(..read).unwrap_or_default());
            self.at = self.at.saturating_add(read as u64);
        }
        Ok(read)
    }
}

impl Seek for Probed {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let landed = self.bytes.seek(to)?;
        self.at = landed;
        Ok(landed)
    }
}

impl MediaSource for Probed {
    fn is_seekable(&self) -> bool {
        self.bytes.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.bytes.byte_len()
    }
}

pub(crate) fn open_media(sources: &Sources, location: &MediaLocation) -> Result<Opened> {
    open(sources.open(location)?, location)
}

pub(crate) fn open(media: Media, location: &MediaLocation) -> Result<Opened> {
    open_spooling(media, location, SPOOLED_AT_MOST)
}

pub(crate) fn open_spooling(
    media: Media,
    location: &MediaLocation,
    spooled_at_most: u64,
) -> Result<Opened> {
    open_spooling_within(
        media,
        location,
        Spooling {
            in_memory: spooled_at_most,
            on_disc: SPOOLED_ON_DISC_AT_MOST,
            within: SPOOLED_WITHIN,
        },
    )
}

#[derive(Clone, Copy)]
struct Spooling {
    in_memory: u64,
    on_disc: u64,
    within: Duration,
}

fn open_spooling_within(
    media: Media,
    location: &MediaLocation,
    at_most: Spooling,
) -> Result<Opened> {
    let Media {
        stream: mut bytes,
        hint: named,
    } = media;
    let mut seekable = bytes.is_seekable();
    let mut spool = None;
    let mut prescan = if seekable {
        Prescan::buffered(bytes.as_mut())
    } else {
        match spooled(bytes.as_mut(), at_most.in_memory, at_most.within) {
            Spooled::Whole(held) => {
                bytes = Box::new(Reading::new(Cursor::new(held)));
                seekable = true;
                Prescan::buffered(bytes.as_mut())
            }
            Spooled::Head(head) if let Some(container) = read_by_seeking(&head) => {
                bytes = spooled_whole_on_disc(&head, bytes, named.clone(), at_most.on_disc)
                    .ok_or_else(|| Error::ReadBySeeking {
                        location: location.clone(),
                        container,
                    })?;
                seekable = true;
                Prescan::buffered(bytes.as_mut())
            }
            Spooled::Head(head) => {
                let found = Prescan::read(&mut Cursor::new(head.as_slice()));
                bytes = match Spool::beginning_with(&head, bytes, named.clone()) {
                    Ok(held) => {
                        let streamed = Box::new(held.streamed());
                        spool = Some(held);
                        streamed
                    }
                    Err((source, bytes)) => {
                        tracing::debug!(%source, "a source that cannot seek could not be spooled on disc; it plays from its head");
                        Box::new(Replaying::over(bytes, head))
                    }
                };
                found
            }
        }
    };

    if let Some(container) = dsd::sniff(bytes.as_mut()) {
        let layout = dsd::layout(bytes.as_mut(), container, location)?;
        let dsd::Described { tags, visuals } = dsd::described(bytes.as_mut(), &layout);
        return Ok(Opened::Dsd(Box::new(OpenedDsd {
            bytes,
            layout,
            seekable,
            tags,
            visuals,
            spool,
        })));
    }

    refuse_what_the_wave_reader_would_overflow_on(&prescan.riff, location)?;
    refuse_what_the_caf_reader_would_overflow_on(prescan.caf, location)?;

    let mut hint = Hint::new();
    match named {
        Some(FormatHint::Extension(extension)) => {
            hint.with_extension(&extension);
        }
        Some(FormatHint::MediaType(media_type)) => {
            hint.mime_type(&media_type);
        }
        None => {}
    }

    let stream = MediaSourceStream::new(
        Box::new(Probed::over(bytes, prescan.segment.voided.clone())),
        MediaSourceStreamOptions::default(),
    );
    let mut reader = crate::registry::formats()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|source| match source {
            errors::Error::Unsupported(_) => Error::UnrecognisedContainer {
                location: location.clone(),
            },
            source => Error::from_symphonia(source, CodecOp::Probe, location),
        })?;

    let chunk = prescan.riff.id3.take().and_then(tags::read_id3_chunk);
    let revisions = Revisions::read(reader.as_mut(), chunk.as_ref());

    Ok(Opened::Coded(Box::new(Coded {
        reader,
        seekable,
        prescan,
        revisions,
        chunk_pictures: chunk.map(|held| held.media.visuals).unwrap_or_default(),
        spool,
    })))
}

fn spooled_whole_on_disc(
    head: &[u8],
    rest: Box<dyn MediaStream>,
    hint: Option<FormatHint>,
    at_most: u64,
) -> Option<Box<dyn MediaStream>> {
    let spool = Spool::beginning_with_at_most(head, rest, hint, at_most)
        .inspect_err(|(source, _)| {
            tracing::debug!(%source, "a source read by seeking could not be spooled on disc");
        })
        .ok()?;
    spool.ended_whole().map(|whole| whole.stream)
}

fn read_by_seeking(head: &[u8]) -> Option<Container> {
    READ_BY_SEEKING
        .iter()
        .find_map(|(magic, container)| head.starts_with(magic.as_slice()).then_some(*container))
}

fn refuse_what_the_wave_reader_would_overflow_on(
    riff: &Riff,
    location: &MediaLocation,
) -> Result<()> {
    let Some(channels) = riff.channels else {
        return Ok(());
    };
    if u32::from(channels) > NAMEABLE_WAVE_CHANNELS {
        return Err(Error::TooManyChannels {
            location: location.clone(),
            channels,
        });
    }
    match riff.mask {
        Some(mask) if riff::a_mask_the_decoder_cannot_widen(mask, channels) => {
            Err(Error::ChannelMaskNotRepresentable {
                location: location.clone(),
                channels,
                mask,
            })
        }
        Some(_) | None => Ok(()),
    }
}

fn refuse_what_the_caf_reader_would_overflow_on(
    overflow: Option<caf::Overflow>,
    location: &MediaLocation,
) -> Result<()> {
    let location = location.clone();
    match overflow {
        None => Ok(()),
        Some(caf::Overflow::PacketBits { bytes_per_packet }) => Err(Error::PacketTooLarge {
            location,
            bytes_per_packet,
        }),
        Some(caf::Overflow::FrameCount {
            packets,
            frames_per_packet,
        }) => Err(Error::FrameCountNotRepresentable {
            location,
            packets,
            frames_per_packet,
        }),
        Some(caf::Overflow::PacketOffset { packets }) => {
            Err(Error::PacketOffsetNotRepresentable { location, packets })
        }
        Some(caf::Overflow::PacketFrames { frames_per_packet }) => Err(Error::PacketTooLong {
            location,
            frames_per_packet,
        }),
        Some(caf::Overflow::TableCut { declared, held }) => Err(Error::PacketTableCut {
            location,
            declared,
            held,
        }),
    }
}

enum Spooled {
    Whole(Vec<u8>),
    Head(Vec<u8>),
}

fn spooled(bytes: &mut dyn MediaStream, at_most: u64, within: Duration) -> Spooled {
    let began = Instant::now();
    let mut held = Vec::new();
    let mut reading = vec![0_u8; SPOOLED_A_READ_AT_A_TIME];
    loop {
        let room = at_most.saturating_sub(held.len() as u64);
        if room == 0 || began.elapsed() >= within {
            return Spooled::Head(held);
        }
        let wanted = usize::try_from(room)
            .unwrap_or(usize::MAX)
            .min(reading.len());
        match bytes.read(&mut reading[..wanted]) {
            Ok(0) => return Spooled::Whole(held),
            Ok(read) => held.extend_from_slice(&reading[..read]),
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(source) => {
                tracing::debug!(%source, "a source that cannot seek stopped before it was spooled");
                return Spooled::Head(held);
            }
        }
    }
}

pub(crate) fn audio_track<'a>(
    reader: &'a dyn FormatReader,
    location: &MediaLocation,
) -> Result<(&'a Track, &'a AudioCodecParameters)> {
    reader
        .default_track(TrackType::Audio)
        .and_then(|track| Some((track, audio_params(track)?)))
        .ok_or_else(|| Error::NoAudioTrack {
            location: location.clone(),
        })
}

fn audio_params(track: &Track) -> Option<&AudioCodecParameters> {
    match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(params)) => Some(params),
        _ => None,
    }
}

pub(crate) fn coded_info(
    opened: &Coded,
    track: &Track,
    params: &AudioCodecParameters,
    location: &MediaLocation,
) -> Result<MediaInfo> {
    let id = StreamTrackId(track.id);
    let spec = stream_spec(params, location, id)?;
    let prescan = &opened.prescan;
    let carrying = Carrying::of(opened.reader.format_info().format, params.codec);
    let noted = itunes_priming(opened, track, params);
    let primed = priming(track, params, carrying, prescan, spec.rate).or(noted);
    let playable = primed.and_then(Priming::window);
    let tags = tags::read(
        &opened.revisions,
        track.id,
        prescan,
        segment_title_names_the_track(opened.reader.as_ref()),
    );

    Ok(MediaInfo {
        container: opened.reader.format_info().format,
        codec: prescan.wavpack.codec(params.codec),
        spec,
        speakers: Speakers::of(opus::channels_of(params).as_ref()),
        duration: playable.and_then(FrameSpan::frames).or_else(|| {
            prescan
                .segment
                .counted_frames_of(track.id)
                .or(prescan.mpeg_frames)
                .or_else(|| duration(track, spec.rate))
                .filter(|declared| *declared != Frames::ZERO)
                .or_else(|| prescan.boxes.fragmented_length(spec.rate))
                .or_else(|| prescan.segment_duration(spec.rate))
                .map(|declared| declared.saturating_sub(carrying.declared_before_the_music(primed)))
        }),
        encoder_delay: track.delay.or(primed.map(|held| held.delay)).unwrap_or(0),
        encoder_padding: track
            .padding
            .or(primed.map(|held| held.padding))
            .unwrap_or(0),
        playable,
        bits_per_coded_sample: declared_bits(params, prescan)
            .and_then(|bits| u8::try_from(bits).ok()),
        is_seekable: opened.seekable,
        packing: Packing::Samples,
        cue: embedded_cue(opened, track, &tags).or_else(|| chaptered(opened, spec.rate, &tags)),
        tags,
    })
}

fn embedded_cue(opened: &Coded, track: &Track, tags: &TagSet) -> Option<CueFile> {
    tags::read_cue_sheet(&opened.revisions, track.id)
        .map(|text| cue::read(text.as_bytes()))
        .and_then(the_first_cut)
        .or_else(|| {
            opened
                .prescan
                .flac
                .cue
                .clone()
                .map(|cut| cut.billed_by(tags))
        })
}

fn chaptered(opened: &Coded, rate: SampleRate, tags: &TagSet) -> Option<CueFile> {
    let mut marked = opened
        .reader
        .chapters()
        .map(chapters::of_group)
        .unwrap_or_default();
    if marked.is_empty() {
        marked = opened.prescan.chapters();
    }
    if marked.is_empty() {
        marked = chapters::of_comments(&opened.prescan.flac.chapters);
    }
    chapters::cut(&marked, rate, tags)
}

fn the_first_cut(sheet: CueSheet) -> Option<CueFile> {
    sheet.files.into_iter().find(|file| !file.tracks.is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Carrying {
    OpusInOgg,
    OpusInMatroska,
    Anything,
}

impl Carrying {
    fn declared_before_the_music(self, primed: Option<Priming>) -> Frames {
        match (self, primed) {
            (Self::OpusInMatroska, Some(primed)) => Frames(u64::from(primed.delay)),
            _ => Frames::ZERO,
        }
    }

    fn of(format: FormatId, codec: AudioCodecId) -> Self {
        match (Container::from_id(format), codec) {
            (Container::Ogg, CODEC_ID_OPUS) => Self::OpusInOgg,
            (Container::Matroska, CODEC_ID_OPUS) => Self::OpusInMatroska,
            _ => Self::Anything,
        }
    }
}

fn priming(
    track: &Track,
    params: &AudioCodecParameters,
    carrying: Carrying,
    prescan: &Prescan,
    rate: SampleRate,
) -> Option<Priming> {
    match carrying {
        Carrying::OpusInOgg => priming_counted_inside_the_granules(track),
        Carrying::OpusInMatroska => {
            params
                .extra_data
                .as_deref()
                .and_then(opus::Head::read)
                .map(|head| {
                    let pre_skip = u32::from(head.pre_skip);
                    let padding = prescan.segment.discarded_frames(rate);
                    Priming::new(
                        pre_skip,
                        padding,
                        prescan.segment.opus_music(pre_skip, padding),
                    )
                })
        }
        Carrying::Anything => None,
    }
    .or_else(|| priming_the_reader_read(track))
    .or_else(|| prescan.boxes.priming_at(rate))
}

fn itunes_priming(opened: &Coded, track: &Track, params: &AudioCodecParameters) -> Option<Priming> {
    if params.codec != CODEC_ID_MP3 {
        return None;
    }
    tags::itunes_gapless_note(&opened.revisions, track.id)
        .as_deref()
        .and_then(boxes::gapless_fields)
        .map(|noted| noted.behind_a_decoder_delay(MP3_DECODER_DELAY))
}

fn priming_counted_inside_the_granules(track: &Track) -> Option<Priming> {
    let delay = track.delay?;
    let playable = track
        .num_frames
        .map(|frames| Frames(frames.saturating_sub(u64::from(delay))));
    Some(Priming::new(delay, track.padding.unwrap_or(0), playable))
}

fn priming_the_reader_read(track: &Track) -> Option<Priming> {
    let delay = track.delay.unwrap_or(0);
    let padding = track.padding.unwrap_or(0);
    if delay == 0 && padding == 0 {
        return None;
    }
    Some(Priming::new(delay, padding, track.num_frames.map(Frames)))
}

pub(crate) fn timeline(track: &Track, info: &MediaInfo) -> Timeline {
    Timeline::new(track.time_base, music_at(track, info), info.spec.rate)
}

fn music_at(track: &Track, info: &MediaInfo) -> Timestamp {
    match Carrying::of(info.container, info.codec) {
        Carrying::OpusInMatroska => return Timestamp::ZERO,
        Carrying::OpusInOgg => return after_the_priming(track, info),
        Carrying::Anything if track.delay.is_some() => return Timestamp::ZERO,
        Carrying::Anything => {}
    }
    after_the_priming(track, info)
}

fn after_the_priming(track: &Track, info: &MediaInfo) -> Timestamp {
    track
        .start_ts
        .checked_add(Ticks::new(info.priming().get()))
        .unwrap_or(track.start_ts)
}

fn segment_title_names_the_track(reader: &dyn FormatReader) -> bool {
    reader
        .tracks()
        .iter()
        .filter(|track| track.track_type() == Some(TrackType::Audio))
        .count()
        == 1
}

fn stream_spec(
    params: &AudioCodecParameters,
    location: &MediaLocation,
    track: StreamTrackId,
) -> Result<StreamSpec> {
    Ok(StreamSpec::new(
        sample_rate(params, location, track)?,
        channel_layout(params, location, track)?,
        sample_format(params),
    ))
}

fn sample_rate(
    params: &AudioCodecParameters,
    location: &MediaLocation,
    track: StreamTrackId,
) -> Result<SampleRate> {
    let hz = params
        .sample_rate
        .ok_or_else(|| Error::TrackPropertyMissing {
            location: location.clone(),
            track,
            property: TrackProperty::SampleRate,
        })?;

    SampleRate::new(hz).map_err(|_| Error::RateNotRepresentable {
        location: location.clone(),
        track,
        rate: hz,
    })
}

fn channel_layout(
    params: &AudioCodecParameters,
    location: &MediaLocation,
    track: StreamTrackId,
) -> Result<ChannelLayout> {
    let placed = opus::channels_of(params);
    let channels = placed.as_ref().ok_or_else(|| Error::TrackPropertyMissing {
        location: location.clone(),
        track,
        property: TrackProperty::ChannelLayout,
    })?;

    let count = ChannelCount::new(u16::try_from(channels.count()).unwrap_or(u16::MAX))?;
    match channels {
        Channels::Positioned(positions) => Ok(positioned_layout(*positions, count)),
        Channels::Discrete(_) => Ok(discrete_layout(count)),
        _ => Err(Error::LayoutNotRepresentable {
            location: location.clone(),
            track,
            channels: count,
        }),
    }
}

const FRONT_PAIR: Position = Position::FRONT_LEFT.union(Position::FRONT_RIGHT);
const REAR_PAIR: Position = Position::REAR_LEFT.union(Position::REAR_RIGHT);
const SIDE_PAIR: Position = Position::SIDE_LEFT.union(Position::SIDE_RIGHT);
const FRONT_THREE: Position = FRONT_PAIR.union(Position::FRONT_CENTER);
const FRONT_THREE_AND_LFE: Position = FRONT_THREE.union(Position::LFE1);

pub(crate) fn positioned_layout(positions: Position, count: ChannelCount) -> ChannelLayout {
    if matches!(count, ChannelCount::MONO | ChannelCount::STEREO) {
        return discrete_layout(count);
    }
    let named = [
        (FRONT_PAIR.union(REAR_PAIR), ChannelLayout::Quad),
        (FRONT_PAIR.union(SIDE_PAIR), ChannelLayout::Quad),
        (FRONT_THREE.union(REAR_PAIR), ChannelLayout::Surround50),
        (FRONT_THREE.union(SIDE_PAIR), ChannelLayout::Surround50),
        (
            FRONT_THREE_AND_LFE
                .union(Position::REAR_CENTER)
                .union(SIDE_PAIR),
            ChannelLayout::Surround61,
        ),
        (
            FRONT_THREE_AND_LFE.union(REAR_PAIR),
            ChannelLayout::Surround51,
        ),
        (
            FRONT_THREE_AND_LFE.union(SIDE_PAIR),
            ChannelLayout::Surround51,
        ),
        (
            FRONT_THREE_AND_LFE.union(REAR_PAIR).union(SIDE_PAIR),
            ChannelLayout::Surround71,
        ),
    ];
    named
        .into_iter()
        .find_map(|(held, layout)| (held == positions).then_some(layout))
        .unwrap_or(ChannelLayout::Discrete(count))
}

fn discrete_layout(count: ChannelCount) -> ChannelLayout {
    match count {
        ChannelCount::MONO => ChannelLayout::Mono,
        ChannelCount::STEREO => ChannelLayout::Stereo,
        _ => ChannelLayout::Discrete(count),
    }
}

fn sample_format(params: &AudioCodecParameters) -> SampleFormat {
    if let Some(format) = params
        .sample_format
        .or_else(|| float_pcm_format(params.codec))
    {
        return representable(format);
    }
    match params.bits_per_sample.or_else(|| alac_bit_depth(params)) {
        Some(bits) => integer_format(bits),
        None => undeclared_format(params.codec),
    }
}

fn declared_bits(params: &AudioCodecParameters, prescan: &Prescan) -> Option<u32> {
    params
        .bits_per_coded_sample
        .or(prescan.riff.valid_bits)
        .or_else(|| flac_bit_depth(params))
        .or(prescan.segment.bit_depth)
        .or(params.bits_per_sample)
        .or_else(|| alac_bit_depth(params))
}

fn flac_bit_depth(params: &AudioCodecParameters) -> Option<u32> {
    if params.codec != CODEC_ID_FLAC {
        return None;
    }

    let block = params.extra_data.as_deref()?;
    if block.len() < STREAMINFO_BYTES {
        return None;
    }

    let depth = bits_at(block, STREAMINFO_BIT_DEPTH_AT, STREAMINFO_BIT_DEPTH_BITS)? + 1;
    (depth > 1).then_some(depth)
}

fn bits_at(block: &[u8], at: usize, width: u32) -> Option<u32> {
    let mut value = 0_u32;
    for offset in 0..width {
        let bit = at + offset as usize;
        let byte = *block.get(bit / 8)?;
        value = (value << 1) | u32::from((byte >> (7 - bit % 8)) & 1);
    }
    Some(value)
}

fn alac_bit_depth(params: &AudioCodecParameters) -> Option<u32> {
    if params.codec != CODEC_ID_ALAC {
        return None;
    }

    let mut cookie = params.extra_data.as_deref()?;
    for atom in ALAC_ATOM_IDS {
        if cookie.get(4..8) == Some(atom.as_slice()) {
            cookie = cookie.get(ALAC_ATOM_BYTES..)?;
        }
    }
    if !ALAC_COOKIE_BYTES.contains(&cookie.len()) {
        return None;
    }

    let depth = u32::from(*cookie.get(ALAC_BIT_DEPTH_AT)?);
    (1..=ALAC_MAX_BIT_DEPTH).contains(&depth).then_some(depth)
}

const fn float_pcm_format(codec: AudioCodecId) -> Option<SymphoniaSampleFormat> {
    match codec {
        CODEC_ID_PCM_F32LE
        | CODEC_ID_PCM_F32BE
        | CODEC_ID_PCM_F32LE_PLANAR
        | CODEC_ID_PCM_F32BE_PLANAR => Some(SymphoniaSampleFormat::F32),
        CODEC_ID_PCM_F64LE
        | CODEC_ID_PCM_F64BE
        | CODEC_ID_PCM_F64LE_PLANAR
        | CODEC_ID_PCM_F64BE_PLANAR => Some(SymphoniaSampleFormat::F64),
        _ => None,
    }
}

const fn integer_format(bits: u32) -> SampleFormat {
    match bits {
        ..=16 => SampleFormat::S16,
        17..=24 => SampleFormat::S24,
        _ => SampleFormat::S32,
    }
}

const fn undeclared_format(codec: AudioCodecId) -> SampleFormat {
    match codec {
        CODEC_ID_ALAC => SampleFormat::S32,
        _ => SampleFormat::F32,
    }
}

const fn representable(format: SymphoniaSampleFormat) -> SampleFormat {
    use SymphoniaSampleFormat as S;

    match format {
        S::U8 | S::S8 | S::U16 | S::S16 => SampleFormat::S16,
        S::U24 | S::S24 => SampleFormat::S24,
        S::U32 | S::S32 => SampleFormat::S32,
        S::F32 | S::F64 => SampleFormat::F32,
    }
}

fn duration(track: &Track, rate: SampleRate) -> Option<Frames> {
    if let Some(frames) = track.num_frames {
        return Some(Frames(frames));
    }

    let time = track.time_base?.calc_duration(track.duration?)?;
    let nanos = u64::try_from(time.as_nanos()).ok()?;
    Some(Frames::from_duration(Duration::from_nanos(nanos), rate))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::source::Reading;

    fn wave_of(channels: u16) -> Vec<u8> {
        let mut fmt = vec![0_u8; 16];
        fmt[0] = 1;
        fmt[2..4].copy_from_slice(&channels.to_le_bytes());
        fmt[14] = 32;
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend_from_slice(&fmt);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&0_u32.to_le_bytes());
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    struct Piped(Cursor<Vec<u8>>);

    impl Read for Piped {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Seek for Piped {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "a pipe"))
        }
    }

    fn piped(bytes: Vec<u8>) -> Media {
        Media {
            stream: Box::new(Reading::new(Piped(Cursor::new(bytes)))),
            hint: None,
        }
    }

    #[test]
    fn a_pipe_longer_than_the_spool_is_replayed_from_its_head_and_cannot_seek() {
        let mut wave = wave_of(2);
        wave.extend(std::iter::repeat_n(0, 4_096));
        let location = MediaLocation::local("long.wav");

        let long = open_spooling(piped(wave.clone()), &location, 1_024)
            .expect("a pipe opens")
            .into_coded()
            .expect("a coded stream");
        assert!(!long.seekable);

        let short = open_spooling(piped(wave), &location, SPOOLED_AT_MOST)
            .expect("a pipe opens")
            .into_coded()
            .expect("a coded stream");
        assert!(short.seekable, "a pipe under the spool was not read whole");
    }

    fn silent_dsf(blocks: usize) -> Vec<u8> {
        const BLOCK: usize = 4_096;
        const CHANNELS: u32 = 2;

        let data = vec![0x69_u8; blocks * BLOCK * CHANNELS as usize];
        let mut fmt = b"fmt ".to_vec();
        fmt.extend_from_slice(&52_u64.to_le_bytes());
        fmt.extend_from_slice(&1_u32.to_le_bytes());
        fmt.extend_from_slice(&0_u32.to_le_bytes());
        fmt.extend_from_slice(&CHANNELS.to_le_bytes());
        fmt.extend_from_slice(&CHANNELS.to_le_bytes());
        fmt.extend_from_slice(&2_822_400_u32.to_le_bytes());
        fmt.extend_from_slice(&1_u32.to_le_bytes());
        fmt.extend_from_slice(&((blocks * BLOCK * 8) as u64).to_le_bytes());
        fmt.extend_from_slice(&(BLOCK as u32).to_le_bytes());
        fmt.extend_from_slice(&0_u32.to_le_bytes());

        let mut chunk = b"data".to_vec();
        chunk.extend_from_slice(&((data.len() + 12) as u64).to_le_bytes());
        chunk.extend_from_slice(&data);

        let total = (28 + fmt.len() + chunk.len()) as u64;
        let mut file = b"DSD ".to_vec();
        file.extend_from_slice(&28_u64.to_le_bytes());
        file.extend_from_slice(&total.to_le_bytes());
        file.extend_from_slice(&0_u64.to_le_bytes());
        file.extend_from_slice(&fmt);
        file.extend_from_slice(&chunk);
        file
    }

    fn spooled_at_most(on_disc: u64) -> Spooling {
        Spooling {
            in_memory: 1_024,
            on_disc,
            within: SPOOLED_WITHIN,
        }
    }

    #[test]
    fn a_dsd_pipe_too_long_to_hold_in_memory_is_spooled_whole_on_disc_and_seeks() {
        let location = MediaLocation::local("long.dsf");

        let opened = open_spooling_within(
            piped(silent_dsf(4)),
            &location,
            spooled_at_most(SPOOLED_ON_DISC_AT_MOST),
        )
        .expect("a DSF over a pipe opens");

        let Opened::Dsd(dsd) = opened else {
            panic!("a DSF was not opened as DSD");
        };
        assert!(dsd.seekable, "a DSF spooled whole cannot seek");
        assert!(dsd.spool.is_none(), "a spool held whole was left to settle");
    }

    #[test]
    fn a_dsd_or_monkeys_audio_pipe_too_long_even_for_the_disc_is_named_as_wanting_a_seek() {
        let location = MediaLocation::local("long.dsf");
        for (magic, container) in READ_BY_SEEKING {
            let mut bytes = magic.to_vec();
            bytes.extend(std::iter::repeat_n(0, 4_096));

            let opened = open_spooling_within(piped(bytes), &location, spooled_at_most(2_048));

            assert!(
                matches!(
                    opened,
                    Err(Error::ReadBySeeking { container: named, .. }) if named == container
                ),
                "{container:?} over a pipe was not named as read by seeking"
            );
        }
    }

    struct Trickling {
        bytes: Cursor<Vec<u8>>,
        prompt_for: u64,
        then_every_read_waits: Duration,
    }

    impl Read for Trickling {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.bytes.position() >= self.prompt_for {
                std::thread::sleep(self.then_every_read_waits);
                let short = buf.len().min(64);
                return self.bytes.read(&mut buf[..short]);
            }
            let prompt = usize::try_from(self.prompt_for - self.bytes.position())
                .unwrap_or(usize::MAX)
                .min(buf.len());
            self.bytes.read(&mut buf[..prompt])
        }
    }

    impl Seek for Trickling {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "a slow stream"))
        }
    }

    #[test]
    fn a_stream_that_arrives_slowly_opens_from_what_came_within_the_wait_rather_than_its_whole() {
        const WAITED_AT_MOST: Duration = Duration::from_millis(40);
        let mut wave = wave_of(2);
        wave.extend(std::iter::repeat_n(0, 4_096));
        let head = wave_of(2).len() as u64 + 256;
        let location = MediaLocation::local("remote.wav");
        let trickling = Media {
            stream: Box::new(Reading::new(Trickling {
                bytes: Cursor::new(wave),
                prompt_for: head,
                then_every_read_waits: Duration::from_millis(10),
            })),
            hint: None,
        };

        let began = Instant::now();
        let opened = open_spooling_within(
            trickling,
            &location,
            Spooling {
                in_memory: SPOOLED_AT_MOST,
                on_disc: SPOOLED_ON_DISC_AT_MOST,
                within: WAITED_AT_MOST,
            },
        )
        .expect("a slow stream opens")
        .into_coded()
        .expect("a coded stream");

        assert!(
            began.elapsed() < Duration::from_millis(400),
            "the open waited for the whole stream"
        );
        assert!(!opened.seekable);
    }

    #[test]
    fn a_wave_naming_more_channels_than_a_layout_holds_is_refused_before_symphonia_reads_it() {
        let location = MediaLocation::local("wide.wav");
        let opened = open(
            Media {
                stream: Box::new(Reading::new(Cursor::new(wave_of(20_000)))),
                hint: None,
            },
            &location,
        );

        assert!(matches!(
            opened,
            Err(Error::TooManyChannels {
                channels: 20_000,
                ..
            })
        ));
    }

    #[test]
    fn a_wave_behind_bytes_that_are_not_one_is_refused_as_one_at_the_start_is() {
        let location = MediaLocation::local("junk.wav");
        let mut file = b"ft".to_vec();
        file.extend_from_slice(&wave_of(20_000));
        let opened = open(
            Media {
                stream: Box::new(Reading::new(Cursor::new(file))),
                hint: None,
            },
            &location,
        );

        assert!(matches!(
            opened,
            Err(Error::TooManyChannels {
                channels: 20_000,
                ..
            })
        ));
    }

    #[test]
    fn a_wave_behind_more_junk_than_a_tag_would_hold_is_still_refused_as_symphonia_would_open_it() {
        let location = MediaLocation::local("junk.wav");
        let mut file = vec![0x55_u8; 100 * 1024];
        file.extend_from_slice(&wave_of(20_000));
        let opened = open(
            Media {
                stream: Box::new(Reading::new(Cursor::new(file))),
                hint: None,
            },
            &location,
        );

        assert!(matches!(
            opened,
            Err(Error::TooManyChannels {
                channels: 20_000,
                ..
            })
        ));
    }

    #[test]
    fn a_wave_behind_another_containers_marker_is_left_to_the_container_symphonia_opens_first() {
        let location = MediaLocation::local("ogg-first.wav");
        let mut file = b"OggS".to_vec();
        file.extend_from_slice(&[0_u8; 60]);
        file.extend_from_slice(&wave_of(20_000));
        let opened = open(
            Media {
                stream: Box::new(Reading::new(Cursor::new(file))),
                hint: None,
            },
            &location,
        );

        assert!(
            !matches!(opened, Err(Error::TooManyChannels { .. })),
            "a WAVE symphonia never reaches was refused for what it declares"
        );
    }

    #[test]
    fn a_caf_whose_packets_overflow_the_reader_is_refused_before_symphonia_reads_it() {
        let location = MediaLocation::local("wide.caf");
        let mut file = b"caff\x00\x01\x00\x00desc".to_vec();
        file.extend_from_slice(&32_i64.to_be_bytes());
        file.extend_from_slice(&8_000.0_f64.to_be_bytes());
        file.extend_from_slice(b"lpcm");
        for word in [2_u32, 0xd400_0000, 0, 6, 16] {
            file.extend_from_slice(&word.to_be_bytes());
        }
        let opened = open(
            Media {
                stream: Box::new(Reading::new(Cursor::new(file))),
                hint: None,
            },
            &location,
        );

        assert!(matches!(
            opened,
            Err(Error::PacketTooLarge {
                bytes_per_packet: 0xd400_0000,
                ..
            })
        ));
    }

    fn params(codec: AudioCodecId) -> AudioCodecParameters {
        let mut params = AudioCodecParameters::new();
        params.for_codec(codec);
        params
    }

    fn format(params: &AudioCodecParameters) -> Result<SampleFormat> {
        Ok(sample_format(params))
    }

    #[test]
    fn a_declared_bit_depth_picks_the_narrowest_format_that_holds_it() {
        for (bits, expected) in [
            (8, SampleFormat::S16),
            (16, SampleFormat::S16),
            (20, SampleFormat::S24),
            (24, SampleFormat::S24),
            (32, SampleFormat::S32),
        ] {
            let mut params = params(CODEC_ID_ALAC);
            params.with_bits_per_sample(bits);
            assert_eq!(format(&params).expect("representable"), expected, "{bits}");
        }
    }

    fn cookie(depth: u8, prefix: &[u8]) -> Vec<u8> {
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(&4_096_u32.to_be_bytes());
        bytes.push(0);
        bytes.push(depth);
        bytes.extend_from_slice(&[40, 10, 14, 2]);
        bytes.extend_from_slice(&255_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&96_000_u32.to_be_bytes());
        bytes
    }

    fn alac(depth: u8, prefix: &[u8]) -> AudioCodecParameters {
        let mut params = params(CODEC_ID_ALAC);
        params.with_extra_data(cookie(depth, prefix).into_boxed_slice());
        params
    }

    #[test]
    fn an_alac_depth_is_read_from_the_magic_cookie_symphonia_leaves_undeclared() {
        for prefix in [b"".as_slice(), b"\0\0\0\x24alac\0\0\0\0".as_slice()] {
            assert_eq!(
                format(&alac(24, prefix)).expect("representable"),
                SampleFormat::S24,
                "a twenty-four bit cookie was read as something else"
            );
            assert_eq!(
                format(&alac(16, prefix)).expect("representable"),
                SampleFormat::S16
            );
            assert_eq!(
                declared_bits(&alac(24, prefix), &Prescan::default()),
                Some(24)
            );
        }
    }

    fn streaminfo(depth: u32) -> Vec<u8> {
        let mut bits = vec![0_u8; STREAMINFO_BYTES];
        let at = STREAMINFO_BIT_DEPTH_AT;
        let value = depth - 1;
        for offset in 0..STREAMINFO_BIT_DEPTH_BITS {
            let set = (value >> (STREAMINFO_BIT_DEPTH_BITS - 1 - offset)) & 1;
            let bit = at + offset as usize;
            bits[bit / 8] |= (set as u8) << (7 - bit % 8);
        }
        bits
    }

    fn flac(depth: u32) -> AudioCodecParameters {
        let mut params = params(CODEC_ID_FLAC);
        params.with_extra_data(streaminfo(depth).into_boxed_slice());
        params
    }

    #[test]
    fn a_flac_depth_is_read_from_streaminfo_where_the_container_left_it_unset() {
        for depth in [16, 20, 24, 32] {
            assert_eq!(
                declared_bits(&flac(depth), &Prescan::default()),
                Some(depth),
                "a {depth}-bit STREAMINFO was read as something else"
            );
        }
    }

    #[test]
    fn a_streaminfo_shorter_than_the_block_leaves_the_depth_undeclared() {
        let mut short = params(CODEC_ID_FLAC);
        short.with_extra_data(vec![0; STREAMINFO_BYTES - 1].into_boxed_slice());
        assert_eq!(flac_bit_depth(&short), None);

        let mut other = params(CODEC_ID_ALAC);
        other.with_extra_data(streaminfo(24).into_boxed_slice());
        assert_eq!(flac_bit_depth(&other), None);
    }

    #[test]
    fn a_wav_declares_the_bits_it_uses_rather_than_the_ones_it_pads_to() {
        let mut params = params(AudioCodecId::default());
        params.with_bits_per_sample(24);

        let mut prescan = Prescan::default();
        prescan.riff.valid_bits = Some(20);

        assert_eq!(declared_bits(&params, &prescan), Some(20));
        assert_eq!(declared_bits(&params, &Prescan::default()), Some(24));
    }

    #[test]
    fn a_matroska_bit_depth_answers_where_nothing_nearer_the_codec_does() {
        let mut prescan = Prescan::default();
        prescan.segment.bit_depth = Some(24);

        assert_eq!(
            declared_bits(&params(AudioCodecId::default()), &prescan),
            Some(24)
        );
    }

    #[test]
    fn a_magic_cookie_that_is_not_one_leaves_the_depth_undeclared() {
        let mut short = params(CODEC_ID_ALAC);
        short.with_extra_data(vec![0; 12].into_boxed_slice());
        assert_eq!(alac_bit_depth(&short), None);

        let mut zero = params(CODEC_ID_ALAC);
        zero.with_extra_data(cookie(0, b"").into_boxed_slice());
        assert_eq!(alac_bit_depth(&zero), None);

        let mut flac = params(AudioCodecId::default());
        flac.with_extra_data(cookie(24, b"").into_boxed_slice());
        assert_eq!(alac_bit_depth(&flac), None);
    }

    #[test]
    fn float_pcm_is_not_read_as_an_integer_format_despite_declaring_a_depth() {
        let mut params = params(CODEC_ID_PCM_F32LE);
        params.with_bits_per_sample(32);

        assert_eq!(format(&params).expect("representable"), SampleFormat::F32);
    }

    #[test]
    fn a_codec_that_declares_no_depth_falls_back_to_what_its_decoder_emits() {
        assert_eq!(
            format(&params(CODEC_ID_ALAC)).expect("representable"),
            SampleFormat::S32
        );
        assert_eq!(
            format(&params(AudioCodecId::default())).expect("representable"),
            SampleFormat::F32
        );
    }

    #[test]
    fn sixty_four_bit_float_samples_are_decoded_as_thirty_two_bit_float() {
        let mut params = params(CODEC_ID_PCM_F64LE);
        params.with_bits_per_sample(64);

        assert_eq!(format(&params).expect("representable"), SampleFormat::F32);
    }

    #[test]
    fn an_unmappable_channel_set_is_rejected_rather_than_guessed_at() {
        let mut params = params(CODEC_ID_ALAC);
        params.with_channels(Channels::Ambisonic(1));

        let layout = channel_layout(
            &params,
            &MediaLocation::local("/music/a.m4a"),
            StreamTrackId(0),
        );
        assert!(matches!(layout, Err(Error::LayoutNotRepresentable { .. })));
    }

    fn layout_of(channels: Channels) -> ChannelLayout {
        let mut params = params(CODEC_ID_ALAC);
        params.with_channels(channels);
        channel_layout(
            &params,
            &MediaLocation::local("/music/a.wav"),
            StreamTrackId(0),
        )
        .expect("a positioned or discrete set is representable")
    }

    #[test]
    fn a_five_and_a_seven_channel_file_are_named_by_where_their_channels_sit() {
        assert_eq!(
            layout_of(Channels::Positioned(FRONT_THREE | REAR_PAIR)),
            ChannelLayout::Surround50
        );
        assert_eq!(
            layout_of(Channels::Positioned(FRONT_THREE | SIDE_PAIR)),
            ChannelLayout::Surround50
        );
        assert_eq!(
            layout_of(Channels::Positioned(
                FRONT_THREE_AND_LFE | Position::REAR_CENTER | SIDE_PAIR
            )),
            ChannelLayout::Surround61
        );
    }

    #[test]
    fn a_layout_is_named_only_where_the_container_places_every_channel_where_that_layout_does() {
        let six = ChannelCount::new(6).expect("six channels");
        let four = ChannelCount::new(4).expect("four channels");

        assert_eq!(
            layout_of(Channels::Positioned(FRONT_THREE_AND_LFE | SIDE_PAIR)),
            ChannelLayout::Surround51
        );
        assert_eq!(
            layout_of(Channels::Positioned(FRONT_THREE_AND_LFE | REAR_PAIR)),
            ChannelLayout::Surround51
        );
        assert_eq!(
            layout_of(Channels::Positioned(
                FRONT_PAIR | Position::FRONT_CENTER | REAR_PAIR | Position::REAR_CENTER
            )),
            ChannelLayout::Discrete(six),
            "a 6.0 file was read as 5.1, so a downmix would drop its fourth channel as LFE"
        );
        assert_eq!(
            layout_of(Channels::Positioned(
                FRONT_PAIR | Position::FRONT_CENTER | Position::REAR_CENTER
            )),
            ChannelLayout::Discrete(four),
            "an LCRS file was read as quad"
        );
        assert_eq!(
            layout_of(Channels::Discrete(6)),
            ChannelLayout::Discrete(six)
        );
        assert_eq!(layout_of(Channels::Discrete(1)), ChannelLayout::Mono);
        assert_eq!(
            layout_of(Channels::Positioned(Position::FRONT_LEFT)),
            ChannelLayout::Mono,
            "a mono stream placed on the left was read as one discrete channel"
        );
    }

    #[test]
    fn discrete_channel_counts_map_to_the_matching_named_layout() {
        let mut params = params(CODEC_ID_ALAC);
        params.with_channels(Channels::Discrete(2));

        let layout = channel_layout(
            &params,
            &MediaLocation::local("/music/a.m4a"),
            StreamTrackId(0),
        );
        assert_eq!(
            layout.expect("stereo is representable"),
            ChannelLayout::Stereo
        );
    }
}
