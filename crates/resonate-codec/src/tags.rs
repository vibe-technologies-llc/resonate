use std::{fmt, io::Cursor, str, sync::Arc};

use resonate_core::{Decibels, MediaLocation};
use symphonia::{
    core::{
        formats::FormatReader,
        io::{MediaSourceStream, MediaSourceStreamOptions},
        meta::{
            Metadata, MetadataOptions, MetadataReader as _, MetadataRevision, RawValue,
            StandardTag, Tag,
        },
    },
    default::meta::Id3v2Reader,
};

use crate::{Pictured, Picturing, Result, prescan::Prescan, riff::InfoTag, sylt};

const SEGMENT_TITLE: &str = "SEGMENT@TITLE";
const CUE_SHEET: &str = "CUESHEET";
const UNIQUE_FILE_IDENTIFIER: &str = "UFID";
const IDENTIFIER_OWNER: &str = "OWNER";
const MUSICBRAINZ_OWNER: &str = "http://musicbrainz.org";
const R128_TRACK_GAIN: &str = "R128_TRACK_GAIN";
const R128_ALBUM_GAIN: &str = "R128_ALBUM_GAIN";
const R128_STEPS_PER_DB: f32 = 256.0;
const R128_BELOW_REPLAY_GAIN_DB: f32 = 5.0;

const VORBIS_COMMENT_ONLY: [&str; 28] = [
    "ALBUM",
    "ALBUMARTIST",
    "ALBUM_ARTIST",
    "ALBUM ARTIST",
    "TRACKNUMBER",
    "TRACKTOTAL",
    "TOTALTRACKS",
    "DISCNUMBER",
    "DISCTOTAL",
    "TOTALDISCS",
    "ORIGINALDATE",
    "ORIGINAL_DATE",
    "COMPILATION",
    "ORGANIZATION",
    "CONTENTGROUP",
    "MIXARTIST",
    "UNSYNCEDLYRICS",
    "UNSYNCED LYRICS",
    "MUSICBRAINZ_TRACKID",
    "MUSICBRAINZ_ALBUMID",
    "MUSICBRAINZ_ARTISTID",
    "MUSICBRAINZ_ALBUMARTISTID",
    "MUSICBRAINZ_RELEASEGROUPID",
    "MUSICBRAINZ_RELEASETRACKID",
    "REPLAYGAIN_TRACK_GAIN",
    "REPLAYGAIN_TRACK_PEAK",
    "REPLAYGAIN_ALBUM_GAIN",
    "REPLAYGAIN_ALBUM_PEAK",
];

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagName(Box<str>);

impl TagName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TagValue {
    Text(String),
    List(Vec<String>),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Boolean(bool),
    Flag,
    Binary { bytes: usize },
    Unrepresentable,
}

impl fmt::Display for TagValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => f.write_str(value),
            Self::List(values) => f.write_str(&values.join("; ")),
            Self::Signed(value) => write!(f, "{value}"),
            Self::Unsigned(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::Boolean(value) => write!(f, "{value}"),
            Self::Flag => f.write_str("set"),
            Self::Binary { bytes } => write!(f, "<{bytes} bytes>"),
            Self::Unrepresentable => f.write_str("<unrepresentable>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawTag {
    pub name: TagName,
    pub value: TagValue,
    pub fields: Vec<(TagName, TagValue)>,
}

impl RawTag {
    pub fn qualified_name(&self) -> String {
        let Some((_, discriminator)) = self.fields.first() else {
            return self.name.to_string();
        };
        format!("{}:{discriminator}", self.name)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain: Option<Decibels>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<Decibels>,
    pub album_peak: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Credits {
    pub composer: Option<String>,
    pub conductor: Option<String>,
    pub lyricist: Option<String>,
    pub performer: Option<String>,
    pub remixer: Option<String>,
    pub engineer: Option<String>,
    pub producer: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagSet {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub grouping: Option<String>,
    pub collection: Option<String>,
    pub edition: Option<String>,
    pub label: Option<String>,
    pub isrc: Option<String>,
    pub beats_per_minute: Option<u32>,
    pub copyright: Option<String>,
    pub encoder: Option<String>,
    pub comment: Option<String>,
    pub musicbrainz_track_id: Option<String>,
    pub musicbrainz_album_id: Option<String>,
    pub musicbrainz_artist_id: Option<String>,
    pub musicbrainz_album_artist_id: Option<String>,
    pub musicbrainz_release_group_id: Option<String>,
    pub musicbrainz_release_track_id: Option<String>,
    pub barcode: Option<String>,
    pub catalog_number: Option<String>,
    pub compilation: bool,
    pub lyrics: Option<String>,
    pub credits: Credits,
    pub replay_gain: ReplayGain,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tagged {
    pub tags: TagSet,
    pub picture: Pictured,
}

pub trait TagSource: Send + Sync {
    fn read(&self, location: &MediaLocation, picturing: Picturing) -> Result<Tagged>;
}

pub(crate) struct Revisions(Vec<Revision>);

struct Revision {
    media: Vec<Tag>,
    per_track: Vec<TrackTags>,
}

struct TrackTags {
    track: u64,
    tags: Vec<Tag>,
}

impl Revisions {
    pub(crate) fn read(reader: &mut dyn FormatReader, chunk: Option<&MetadataRevision>) -> Self {
        let mut held = Self::of(reader.metadata());
        held.0.extend(chunk.map(Revision::of));
        held
    }

    fn of(mut metadata: Metadata<'_>) -> Self {
        let mut held = Vec::new();
        while let Some(earlier) = metadata.pop() {
            held.push(Revision::taken(earlier));
        }
        held.extend(metadata.current().map(Revision::of));
        Self(held)
    }

    fn sets(&self, track: u32) -> impl Iterator<Item = Vec<&Tag>> {
        self.0.iter().map(move |revision| revision.tags(track))
    }
}

impl Revision {
    fn taken(revision: MetadataRevision) -> Self {
        Self {
            media: revision.media.tags,
            per_track: revision
                .per_track
                .into_iter()
                .map(|per_track| TrackTags {
                    track: per_track.track_id,
                    tags: per_track.metadata.tags,
                })
                .collect(),
        }
    }

    fn of(revision: &MetadataRevision) -> Self {
        Self {
            media: revision.media.tags.clone(),
            per_track: revision
                .per_track
                .iter()
                .map(|per_track| TrackTags {
                    track: per_track.track_id,
                    tags: per_track.metadata.tags.clone(),
                })
                .collect(),
        }
    }

    fn tags(&self, track: u32) -> Vec<&Tag> {
        self.media
            .iter()
            .chain(
                self.per_track
                    .iter()
                    .filter(|per_track| per_track.track == u64::from(track))
                    .flat_map(|per_track| per_track.tags.iter()),
            )
            .collect()
    }
}

pub(crate) fn read_id3_chunk(bytes: Vec<u8>) -> Option<MetadataRevision> {
    let stream = MediaSourceStream::new(
        Box::new(Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    Id3v2Reader::try_new(stream, MetadataOptions::default())
        .and_then(|mut reader| reader.read_all())
        .map(|read| read.revision)
        .inspect_err(|source| tracing::debug!(%source, "an ID3 chunk could not be read"))
        .ok()
}

pub(crate) fn read_raw(revisions: &Revisions, track: u32, prescan: &Prescan) -> Vec<RawTag> {
    let mut raw: Vec<RawTag> = prescan
        .riff
        .info
        .iter()
        .map(|entry| RawTag {
            name: entry.name.clone(),
            value: TagValue::Text(entry.value.clone()),
            fields: Vec::new(),
        })
        .chain(prescan.segment.title.iter().map(|title| RawTag {
            name: TagName::new(SEGMENT_TITLE),
            value: TagValue::Text(title.clone()),
            fields: Vec::new(),
        }))
        .collect();

    for set in revisions.sets(track) {
        collect(&set, &mut raw);
    }
    raw
}

pub(crate) fn read_cue_sheet(revisions: &Revisions, track: u32) -> Option<String> {
    let mut newest = None;
    for set in revisions.sets(track) {
        for tag in set {
            if !tag.raw.key.as_str().eq_ignore_ascii_case(CUE_SHEET) {
                continue;
            }
            if let RawValue::String(text) = &tag.raw.value {
                newest = Some(text.to_string());
            }
        }
    }
    newest
}

fn collect(tags: &[&Tag], into: &mut Vec<RawTag>) {
    into.extend(tags.iter().map(|tag| {
        RawTag {
            name: TagName::new(tag.raw.key.as_str()),
            value: value_of(&tag.raw.value),
            fields: tag
                .raw
                .sub_fields
                .iter()
                .flat_map(|fields| fields.iter())
                .map(|field| (TagName::new(field.field.as_str()), value_of(&field.value)))
                .collect(),
        }
    }));
}

fn value_of(value: &RawValue) -> TagValue {
    match value {
        RawValue::Binary(bytes) => TagValue::Binary { bytes: bytes.len() },
        RawValue::Boolean(value) => TagValue::Boolean(*value),
        RawValue::Flag => TagValue::Flag,
        RawValue::Float(value) => TagValue::Float(*value),
        RawValue::SignedInt(value) => TagValue::Signed(*value),
        RawValue::String(value) => TagValue::Text(value.to_string()),
        RawValue::StringList(values) => TagValue::List(values.to_vec()),
        RawValue::UnsignedInt(value) => TagValue::Unsigned(*value),
        _ => TagValue::Unrepresentable,
    }
}

pub(crate) fn from_revision(revision: &MetadataRevision) -> TagSet {
    let held = Revision::of(revision);
    let mut builder = Builder::default();
    builder.absorb(&held.media.iter().collect::<Vec<_>>());
    builder.finish()
}

pub(crate) fn read(
    revisions: &Revisions,
    track: u32,
    prescan: &Prescan,
    segment_title_names_the_track: bool,
) -> TagSet {
    let mut builder = Builder::default();
    builder.absorb_info(&prescan.riff.info);
    for set in revisions.sets(track) {
        builder.absorb(&set);
    }

    let mut tags = builder.finish();
    if segment_title_names_the_track {
        tags.title = tags.title.or_else(|| prescan.segment.title.clone());
    }
    tags
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum DateRank {
    RecordingDate,
    RecordingTime,
    RecordingYear,
    ReleaseDate,
    ReleaseTime,
    ReleaseYear,
    OriginalReleaseDate,
}

#[derive(Default)]
struct Builder {
    tags: TagSet,
    date: Option<(DateRank, String)>,
    synced_lyrics: Option<String>,
}

impl Builder {
    fn absorb_info(&mut self, info: &[InfoTag]) {
        for entry in info {
            if let Some(std) = standard_info(entry.name.as_str(), &entry.value) {
                self.absorb_one(&std);
            }
        }
    }

    fn absorb(&mut self, tags: &[&Tag]) {
        let naming = Naming::of(tags);
        for tag in tags {
            if let Some(std) = naming
                .standard(tag)
                .or_else(|| musicbrainz_identifier(tag))
                .or_else(|| loudness_gain(tag))
            {
                self.absorb_one(&std);
            }
            if let Some(sheet) = synchronised_lyrics(tag) {
                self.synced_lyrics = Some(sheet);
            }
        }
    }

    fn absorb_one(&mut self, tag: &StandardTag) {
        use StandardTag as T;

        match tag {
            T::TrackTitle(value) => given(&mut self.tags.title, value),
            T::Artist(value) => given(&mut self.tags.artist, value),
            T::Album(value) => given(&mut self.tags.album, value),
            T::AlbumArtist(value) => given(&mut self.tags.album_artist, value),
            T::Genre(value) => given(&mut self.tags.genre, value),
            T::Grouping(value) => given(&mut self.tags.grouping, value),
            T::CollectionTitle(value) => given(&mut self.tags.collection, value),
            T::EditionTitle(value) => given(&mut self.tags.edition, value),
            T::Label(value) => given(&mut self.tags.label, value),
            T::IdentIsrc(value) => given(&mut self.tags.isrc, value),
            T::Copyright(value) => given(&mut self.tags.copyright, value),
            T::Encoder(value) => given(&mut self.tags.encoder, value),
            T::Comment(value) => given(&mut self.tags.comment, value),

            T::Composer(value) => given(&mut self.tags.credits.composer, value),
            T::Conductor(value) => given(&mut self.tags.credits.conductor, value),
            T::Lyricist(value) | T::Writer(value) => {
                given(&mut self.tags.credits.lyricist, value);
            }
            T::Performer(value) => given(&mut self.tags.credits.performer, value),
            T::Remixer(value) => given(&mut self.tags.credits.remixer, value),
            T::Engineer(value) => given(&mut self.tags.credits.engineer, value),
            T::Producer(value) => given(&mut self.tags.credits.producer, value),

            T::Bpm(value) => self.tags.beats_per_minute = count(*value),
            T::TrackNumber(value) => self.tags.track_number = count(*value),
            T::TrackTotal(value) => self.tags.track_total = count(*value),
            T::DiscNumber(value) => self.tags.disc_number = count(*value),
            T::DiscTotal(value) => self.tags.disc_total = count(*value),

            T::MusicBrainzTrackId(value) | T::MusicBrainzRecordingId(value) => {
                given(&mut self.tags.musicbrainz_track_id, value);
            }
            T::MusicBrainzAlbumId(value) => {
                given(&mut self.tags.musicbrainz_album_id, value);
            }
            T::MusicBrainzArtistId(value) => {
                given(&mut self.tags.musicbrainz_artist_id, value);
            }
            T::MusicBrainzAlbumArtistId(value) => {
                given(&mut self.tags.musicbrainz_album_artist_id, value);
            }
            T::MusicBrainzReleaseGroupId(value) => {
                given(&mut self.tags.musicbrainz_release_group_id, value);
            }
            T::MusicBrainzReleaseTrackId(value) => {
                given(&mut self.tags.musicbrainz_release_track_id, value);
            }
            T::IdentBarcode(value) => given(&mut self.tags.barcode, value),
            T::IdentCatalogNumber(value) => given(&mut self.tags.catalog_number, value),
            T::CompilationFlag(value) => self.tags.compilation = *value,
            T::Lyrics(value) => given(&mut self.tags.lyrics, value),

            T::RecordingDate(value) => self.date(DateRank::RecordingDate, value),
            T::RecordingTime(value) => self.date(DateRank::RecordingTime, value),
            T::RecordingYear(value) => self.date(DateRank::RecordingYear, &value.to_string()),
            T::ReleaseDate(value) => self.date(DateRank::ReleaseDate, value),
            T::ReleaseTime(value) => self.date(DateRank::ReleaseTime, value),
            T::ReleaseYear(value) => self.date(DateRank::ReleaseYear, &value.to_string()),
            T::OriginalReleaseDate(value) => self.date(DateRank::OriginalReleaseDate, value),

            T::ReplayGainTrackGain(value) => {
                parsed(&mut self.tags.replay_gain.track_gain, decibels(value));
            }
            T::ReplayGainAlbumGain(value) => {
                parsed(&mut self.tags.replay_gain.album_gain, decibels(value));
            }
            T::ReplayGainTrackPeak(value) => {
                parsed(&mut self.tags.replay_gain.track_peak, peak(value));
            }
            T::ReplayGainAlbumPeak(value) => {
                parsed(&mut self.tags.replay_gain.album_peak, peak(value));
            }

            _ => {}
        }
    }

    fn date(&mut self, rank: DateRank, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        match &self.date {
            Some((held, _)) if *held <= rank => {}
            _ => self.date = Some((rank, value.to_owned())),
        }
    }

    fn finish(mut self) -> TagSet {
        self.tags.date = self.date.map(|(_, value)| value);
        if let Some(sheet) = self.synced_lyrics {
            self.tags.lyrics = Some(sheet);
        }
        self.tags
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Naming {
    Container,
    VorbisComment,
}

impl Naming {
    fn of(tags: &[&Tag]) -> Self {
        let vorbis = tags
            .iter()
            .any(|tag| is_vorbis_comment_only(scoped_name(tag.raw.key.as_str())));

        if vorbis {
            Self::VorbisComment
        } else {
            Self::Container
        }
    }

    fn standard(self, tag: &Tag) -> Option<StandardTag> {
        let name = scoped_name(tag.raw.key.as_str());

        match (self, tag.std.as_ref()) {
            (Self::VorbisComment, Some(std)) => Some(as_vorbis_comment(std, name)),
            (Self::Container, Some(std)) => Some(std.clone()),
            (Self::VorbisComment, None) => vorbis_comment(name, &tag.raw.value),
            (Self::Container, None) => None,
        }
    }
}

fn musicbrainz_identifier(tag: &Tag) -> Option<StandardTag> {
    if !tag
        .raw
        .key
        .as_str()
        .eq_ignore_ascii_case(UNIQUE_FILE_IDENTIFIER)
        || !owned_by_musicbrainz(tag)
    {
        return None;
    }

    let RawValue::Binary(identifier) = &tag.raw.value else {
        return None;
    };
    let recording = str::from_utf8(identifier).ok()?.trim();
    if recording.is_empty() {
        return None;
    }

    Some(StandardTag::MusicBrainzRecordingId(Arc::new(
        recording.to_owned(),
    )))
}

fn synchronised_lyrics(tag: &Tag) -> Option<String> {
    let name = scoped_name(tag.raw.key.as_str());
    if !sylt::FRAME_IDS
        .iter()
        .any(|id| id.eq_ignore_ascii_case(name))
    {
        return None;
    }
    let RawValue::Binary(frame) = &tag.raw.value else {
        return None;
    };
    sylt::as_lrc(frame)
}

fn loudness_gain(tag: &Tag) -> Option<StandardTag> {
    let name = scoped_name(tag.raw.key.as_str());
    let album = name.eq_ignore_ascii_case(R128_ALBUM_GAIN);
    if !album && !name.eq_ignore_ascii_case(R128_TRACK_GAIN) {
        return None;
    }
    let RawValue::String(value) = &tag.raw.value else {
        return None;
    };
    let Ok(steps) = value.trim().parse::<i16>() else {
        tracing::debug!(
            value = value.as_str(),
            "discarding an unparseable R128 gain"
        );
        return None;
    };

    let db = f32::from(steps) / R128_STEPS_PER_DB + R128_BELOW_REPLAY_GAIN_DB;
    let written = Arc::new(format!("{db:+.2} dB"));
    Some(if album {
        StandardTag::ReplayGainAlbumGain(written)
    } else {
        StandardTag::ReplayGainTrackGain(written)
    })
}

fn owned_by_musicbrainz(tag: &Tag) -> bool {
    tag.raw
        .sub_fields
        .iter()
        .flat_map(|fields| fields.iter())
        .filter(|field| field.field.eq_ignore_ascii_case(IDENTIFIER_OWNER))
        .any(|field| match &field.value {
            RawValue::String(owner) => owner.trim().eq_ignore_ascii_case(MUSICBRAINZ_OWNER),
            _ => false,
        })
}

fn scoped_name(key: &str) -> &str {
    key.split_once('@').map_or(key, |(_, name)| name)
}

fn is_vorbis_comment_only(name: &str) -> bool {
    VORBIS_COMMENT_ONLY
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
}

fn as_vorbis_comment(std: &StandardTag, name: &str) -> StandardTag {
    match std {
        StandardTag::AlbumArtist(value) if name.eq_ignore_ascii_case("ARTIST") => {
            StandardTag::Artist(value.clone())
        }
        StandardTag::Album(value) if name.eq_ignore_ascii_case("TITLE") => {
            StandardTag::TrackTitle(value.clone())
        }
        held => held.clone(),
    }
}

pub(crate) const LONGEST_NAME_MATCHED: usize = 32;

pub(crate) struct Uppercased {
    held: [u8; LONGEST_NAME_MATCHED],
    len: usize,
}

impl Uppercased {
    pub(crate) fn of(name: &str) -> Option<Self> {
        let bytes = name.as_bytes();
        let mut held = [0_u8; LONGEST_NAME_MATCHED];
        held.get_mut(..bytes.len())?.copy_from_slice(bytes);
        held.make_ascii_uppercase();

        Some(Self {
            held,
            len: bytes.len(),
        })
    }

    pub(crate) fn as_str(&self) -> &str {
        str::from_utf8(&self.held[..self.len]).unwrap_or_default()
    }
}

fn vorbis_comment(name: &str, value: &RawValue) -> Option<StandardTag> {
    let RawValue::String(value) = value else {
        return None;
    };
    let text = || value.clone();

    let std = match Uppercased::of(name)?.as_str() {
        "TITLE" => StandardTag::TrackTitle(text()),
        "ARTIST" => StandardTag::Artist(text()),
        "ALBUM" => StandardTag::Album(text()),
        "ALBUMARTIST" | "ALBUM_ARTIST" | "ALBUM ARTIST" => StandardTag::AlbumArtist(text()),
        "GENRE" => StandardTag::Genre(text()),
        "GROUPING" | "CONTENTGROUP" => StandardTag::Grouping(text()),
        "LABEL" | "ORGANIZATION" | "PUBLISHER" => StandardTag::Label(text()),
        "ISRC" => StandardTag::IdentIsrc(text()),
        "BPM" => StandardTag::Bpm(beats_per_minute(value)?),
        "COPYRIGHT" => StandardTag::Copyright(text()),
        "ENCODER" => StandardTag::Encoder(text()),
        "COMMENT" | "DESCRIPTION" => StandardTag::Comment(text()),
        "COMPOSER" => StandardTag::Composer(text()),
        "CONDUCTOR" => StandardTag::Conductor(text()),
        "LYRICIST" | "WRITER" => StandardTag::Lyricist(text()),
        "PERFORMER" => StandardTag::Performer(text()),
        "REMIXER" | "MIXARTIST" => StandardTag::Remixer(text()),
        "ENGINEER" => StandardTag::Engineer(text()),
        "PRODUCER" => StandardTag::Producer(text()),
        "DATE" => StandardTag::ReleaseDate(text()),
        "ORIGINALDATE" | "ORIGINAL_DATE" => StandardTag::OriginalReleaseDate(text()),
        "TRACK" | "TRACKNUMBER" | "PART_NUMBER" => StandardTag::TrackNumber(leading_number(value)?),
        "TRACKTOTAL" | "TOTALTRACKS" => StandardTag::TrackTotal(leading_number(value)?),
        "DISC" | "DISCNUMBER" => StandardTag::DiscNumber(leading_number(value)?),
        "DISCTOTAL" | "TOTALDISCS" => StandardTag::DiscTotal(leading_number(value)?),
        "COMPILATION" => StandardTag::CompilationFlag(is_set(value)),
        "LYRICS" | "UNSYNCEDLYRICS" | "UNSYNCED LYRICS" => StandardTag::Lyrics(text()),
        "MUSICBRAINZ_TRACKID" => StandardTag::MusicBrainzTrackId(text()),
        "MUSICBRAINZ_ALBUMID" => StandardTag::MusicBrainzAlbumId(text()),
        "MUSICBRAINZ_ARTISTID" => StandardTag::MusicBrainzArtistId(text()),
        "MUSICBRAINZ_ALBUMARTISTID" => StandardTag::MusicBrainzAlbumArtistId(text()),
        "MUSICBRAINZ_RELEASEGROUPID" => StandardTag::MusicBrainzReleaseGroupId(text()),
        "MUSICBRAINZ_RELEASETRACKID" => StandardTag::MusicBrainzReleaseTrackId(text()),
        "BARCODE" => StandardTag::IdentBarcode(text()),
        "CATALOGNUMBER" => StandardTag::IdentCatalogNumber(text()),
        "REPLAYGAIN_TRACK_GAIN" => StandardTag::ReplayGainTrackGain(text()),
        "REPLAYGAIN_TRACK_PEAK" => StandardTag::ReplayGainTrackPeak(text()),
        "REPLAYGAIN_ALBUM_GAIN" => StandardTag::ReplayGainAlbumGain(text()),
        "REPLAYGAIN_ALBUM_PEAK" => StandardTag::ReplayGainAlbumPeak(text()),
        _ => return None,
    };
    Some(std)
}

fn is_set(value: &str) -> bool {
    let value = value.trim();
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

fn standard_info(name: &str, value: &str) -> Option<StandardTag> {
    let text = || Arc::new(value.to_owned());

    let std = match Uppercased::of(name)?.as_str() {
        "INAM" | "TITL" => StandardTag::TrackTitle(text()),
        "IART" => StandardTag::Artist(text()),
        "IPRD" => StandardTag::Album(text()),
        "IGNR" | "GENR" | "ISGN" => StandardTag::Genre(text()),
        "ICRD" => StandardTag::ReleaseDate(text()),
        "YEAR" => StandardTag::ReleaseYear(u16::try_from(leading_number(value)?).ok()?),
        "ITRK" | "IPRT" | "TRCK" => StandardTag::TrackNumber(leading_number(value)?),
        "IFRM" => StandardTag::TrackTotal(leading_number(value)?),
        "IMUS" => StandardTag::Composer(text()),
        "IWRI" => StandardTag::Lyricist(text()),
        "IENG" => StandardTag::Engineer(text()),
        "IPRO" => StandardTag::Producer(text()),
        "ICOP" => StandardTag::Copyright(text()),
        "ISFT" => StandardTag::Encoder(text()),
        "ICMT" => StandardTag::Comment(text()),
        _ => return None,
    };
    Some(std)
}

fn beats_per_minute(value: &str) -> Option<u64> {
    let beats = value.trim().parse::<f64>().ok()?;
    (beats.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&beats)).then_some(beats as u64)
}

fn leading_number(value: &str) -> Option<u64> {
    let head = value.split('/').next()?.trim();
    head.parse().ok()
}

fn given(slot: &mut Option<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        *slot = Some(value.to_owned());
    }
}

fn parsed<T>(slot: &mut Option<T>, value: Option<T>) {
    if value.is_some() {
        *slot = value;
    }
}

fn count(value: u64) -> Option<u32> {
    u32::try_from(value).ok().filter(|value| *value > 0)
}

fn decibels(value: &str) -> Option<Decibels> {
    let text = strip_db(value);
    let Ok(db) = text.parse::<f32>() else {
        tracing::debug!(value, "discarding an unparseable ReplayGain gain");
        return None;
    };
    Decibels::new(db).ok()
}

fn peak(value: &str) -> Option<f32> {
    let Ok(peak) = value.trim().parse::<f32>() else {
        tracing::debug!(value, "discarding an unparseable ReplayGain peak");
        return None;
    };
    (peak.is_finite() && peak >= 0.0).then_some(peak)
}

fn strip_db(value: &str) -> &str {
    let text = value.trim();
    let Some(cut) = text.len().checked_sub(2) else {
        return text;
    };
    match (text.get(..cut), text.get(cut..)) {
        (Some(head), Some(suffix)) if suffix.eq_ignore_ascii_case("db") => head.trim_end(),
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use symphonia::core::meta::{
        MetadataBuilder, MetadataInfo, MetadataLog, MetadataRevision, PerTrackMetadataBuilder,
        RawTag, RawTagSubField, well_known::METADATA_ID_ID3V2,
    };

    use super::*;

    const TEST_METADATA: MetadataInfo = MetadataInfo {
        metadata: METADATA_ID_ID3V2,
        short_name: "test",
        long_name: "test",
    };

    fn tag(std: StandardTag) -> Tag {
        Tag::new_std(RawTag::new("TEST", "test"), std)
    }

    fn keyed(key: &str, value: &str) -> Tag {
        Tag::new(RawTag::new(key, value))
    }

    fn mapped(key: &str, value: &str, std: StandardTag) -> Tag {
        Tag::new_std(RawTag::new(key, value), std)
    }

    fn identified(owner: &str, identifier: &[u8]) -> Tag {
        Tag::new(RawTag::new_with_sub_fields(
            UNIQUE_FILE_IDENTIFIER,
            identifier,
            vec![RawTagSubField::new(IDENTIFIER_OWNER, owner)].into_boxed_slice(),
        ))
    }

    fn text(value: &str) -> Arc<String> {
        Arc::new(value.to_owned())
    }

    fn absorb(tags: &[Tag]) -> TagSet {
        let mut builder = Builder::default();
        builder.absorb(&tags.iter().collect::<Vec<_>>());
        builder.finish()
    }

    fn info(name: &str, value: &str) -> InfoTag {
        InfoTag {
            name: TagName::new(name),
            value: value.to_owned(),
        }
    }

    fn absorb_info(entries: &[InfoTag]) -> TagSet {
        let mut builder = Builder::default();
        builder.absorb_info(entries);
        builder.finish()
    }

    #[test]
    fn an_info_id_written_under_another_of_its_names_is_read_the_same_way() {
        let set = absorb_info(&[
            info("titl", "Echoes"),
            info("TRCK", "2/12"),
            info("GENR", "Progressive Rock"),
            info("YEAR", "1971"),
        ]);

        assert_eq!(set.title.as_deref(), Some("Echoes"));
        assert_eq!(set.track_number, Some(2));
        assert_eq!(set.genre.as_deref(), Some("Progressive Rock"));
        assert_eq!(set.date.as_deref(), Some("1971"));
    }

    #[test]
    fn a_creation_date_outranks_the_bare_year_beside_it_whichever_came_first() {
        let dated = absorb_info(&[info("YEAR", "1971"), info("ICRD", "1971-10-30")]);
        let reversed = absorb_info(&[info("ICRD", "1971-10-30"), info("YEAR", "1971")]);

        assert_eq!(dated.date.as_deref(), Some("1971-10-30"));
        assert_eq!(reversed.date.as_deref(), Some("1971-10-30"));
    }

    fn revision(tags: Vec<Tag>) -> MetadataRevision {
        let mut builder = MetadataBuilder::new(TEST_METADATA);
        for tag in tags {
            builder.add_tag(tag);
        }
        builder.build()
    }

    fn for_track(track: u64, tags: Vec<Tag>) -> MetadataRevision {
        let mut per_track = PerTrackMetadataBuilder::new(track);
        for tag in tags {
            per_track.add_tag(tag);
        }
        let mut builder = MetadataBuilder::new(TEST_METADATA);
        builder.add_track(per_track.build());
        builder.build()
    }

    fn logged(revisions: Vec<MetadataRevision>) -> Revisions {
        let mut log = MetadataLog::default();
        for revision in revisions {
            log.push(revision);
        }
        Revisions::of(log.metadata())
    }

    fn set_of(revisions: &Revisions) -> TagSet {
        read(revisions, 0, &Prescan::default(), true)
    }

    #[test]
    fn a_revision_the_newest_one_used_to_hide_is_read_beside_it() {
        let held = logged(vec![
            revision(vec![mapped(
                "TPE1",
                "Pink Floyd",
                StandardTag::Artist(text("Pink Floyd")),
            )]),
            revision(vec![mapped(
                "TIT2",
                "Echoes",
                StandardTag::TrackTitle(text("Echoes")),
            )]),
        ]);

        let set = set_of(&held);

        assert_eq!(set.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(set.title.as_deref(), Some("Echoes"));
    }

    #[test]
    fn the_newest_revision_still_wins_a_tag_an_earlier_one_also_names() {
        let held = logged(vec![
            revision(vec![mapped(
                "TIT2",
                "Untitled",
                StandardTag::TrackTitle(text("Untitled")),
            )]),
            revision(vec![mapped(
                "TIT2",
                "Echoes",
                StandardTag::TrackTitle(text("Echoes")),
            )]),
        ]);

        assert_eq!(set_of(&held).title.as_deref(), Some("Echoes"));
    }

    #[test]
    fn each_revision_is_read_under_the_naming_its_own_keys_declare() {
        let held = logged(vec![
            revision(vec![keyed("IENG", "Alan Parsons"), keyed("DATE", "1969")]),
            revision(vec![keyed("TRACKNUMBER", "2/12"), keyed("DATE", "1971")]),
        ]);

        let set = set_of(&held);

        assert_eq!(set.track_number, Some(2));
        assert_eq!(set.date.as_deref(), Some("1971"));
    }

    #[test]
    fn a_revisions_per_track_tags_reach_the_track_they_name_and_no_other() {
        let held = logged(vec![
            for_track(
                0,
                vec![mapped(
                    "TIT2",
                    "Echoes",
                    StandardTag::TrackTitle(text("Echoes")),
                )],
            ),
            for_track(
                1,
                vec![mapped(
                    "TIT2",
                    "Commentary",
                    StandardTag::TrackTitle(text("Commentary")),
                )],
            ),
        ]);

        assert_eq!(set_of(&held).title.as_deref(), Some("Echoes"));
        assert_eq!(
            read(&held, 1, &Prescan::default(), true).title.as_deref(),
            Some("Commentary")
        );
    }

    #[test]
    fn every_revision_names_its_tags_in_the_raw_listing() {
        let held = logged(vec![
            revision(vec![keyed("IENG", "Alan Parsons")]),
            revision(vec![keyed("ISFT", "Sound Forge")]),
        ]);

        let raw = read_raw(&held, 0, &Prescan::default());

        assert_eq!(
            raw.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>(),
            ["IENG", "ISFT"]
        );
    }

    #[test]
    fn a_writer_that_names_its_tags_as_vorbis_comments_is_read_that_way() {
        let set = absorb(&[
            mapped(
                "ALBUM@ARTIST",
                "Pink Floyd",
                StandardTag::AlbumArtist(text("Pink Floyd")),
            ),
            keyed("ALBUM@ALBUM", "Meddle"),
            keyed("ALBUM@ALBUM_ARTIST", "Various Artists"),
            keyed("ALBUM@DATE", "1971"),
            keyed("ALBUM@PART_NUMBER", "2"),
        ]);

        assert_eq!(set.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(set.album.as_deref(), Some("Meddle"));
        assert_eq!(set.album_artist.as_deref(), Some("Various Artists"));
        assert_eq!(set.date.as_deref(), Some("1971"));
        assert_eq!(set.track_number, Some(2));
    }

    #[test]
    fn a_target_the_matroska_spec_names_keeps_the_meaning_that_spec_gives_it() {
        let set = absorb(&[
            mapped("ALBUM@TITLE", "Meddle", StandardTag::Album(text("Meddle"))),
            mapped(
                "ALBUM@ARTIST",
                "Pink Floyd",
                StandardTag::AlbumArtist(text("Pink Floyd")),
            ),
            mapped(
                "TRACK@TITLE",
                "Echoes",
                StandardTag::TrackTitle(text("Echoes")),
            ),
        ]);

        assert_eq!(set.album.as_deref(), Some("Meddle"));
        assert_eq!(set.album_artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(set.title.as_deref(), Some("Echoes"));
        assert_eq!(set.artist, None);
    }

    #[test]
    fn a_key_the_container_could_not_map_is_left_alone_where_nothing_says_vorbis() {
        let set = absorb(&[keyed("IENG", "Alan Parsons"), keyed("DATE", "1971")]);

        assert_eq!(set.date, None);
    }

    #[test]
    fn one_vorbis_only_name_is_enough_to_read_the_rest_as_vorbis_comments() {
        let alone = absorb(&[keyed("PERFORMER", "Pink Floyd"), keyed("DATE", "1971")]);

        assert_eq!(alone.credits.performer, None);
        assert_eq!(alone.date, None);

        let beside = absorb(&[
            keyed("ORIGINALDATE", "1971-10-30"),
            keyed("PERFORMER", "Pink Floyd"),
            keyed("DATE", "1971"),
        ]);

        assert_eq!(beside.credits.performer.as_deref(), Some("Pink Floyd"));
        assert_eq!(beside.date.as_deref(), Some("1971"));
    }

    #[test]
    fn a_name_two_vocabularies_define_is_not_on_its_own_a_vorbis_comment() {
        for ambiguous in [
            "DATE",
            "BPM",
            "ISRC",
            "LABEL",
            "PUBLISHER",
            "LYRICS",
            "DESCRIPTION",
        ] {
            assert!(
                !is_vorbis_comment_only(ambiguous),
                "{ambiguous} is a name a container spec defines too"
            );
        }
    }

    #[test]
    fn a_vorbis_comment_name_the_container_left_unmapped_still_reaches_the_set() {
        let set = absorb(&[
            keyed("TRACKNUMBER", "2/12"),
            keyed("DISCNUMBER", "1"),
            keyed("TOTALDISCS", "2"),
            keyed("COMPILATION", "1"),
            keyed("REPLAYGAIN_TRACK_GAIN", "-7.06 dB"),
        ]);

        assert_eq!(set.track_number, Some(2));
        assert_eq!(set.disc_number, Some(1));
        assert_eq!(set.disc_total, Some(2));
        assert!(set.compilation);
        assert_eq!(
            set.replay_gain.track_gain,
            Some(Decibels::new(-7.06).expect("finite"))
        );
    }

    #[test]
    fn the_rest_of_what_a_tagger_writes_about_a_release_reaches_its_own_field() {
        let set = absorb(&[
            keyed(
                "MUSICBRAINZ_ARTISTID",
                "83d91898-7763-47d7-b03b-b92132375c47",
            ),
            keyed(
                "MUSICBRAINZ_ALBUMARTISTID",
                "83d91898-7763-47d7-b03b-b92132375c47",
            ),
            keyed(
                "MUSICBRAINZ_RELEASEGROUPID",
                "f5093c06-23e3-404f-aeaa-40f72885ee3a",
            ),
            keyed(
                "MUSICBRAINZ_RELEASETRACKID",
                "3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11",
            ),
            keyed("BARCODE", "724382920021"),
            keyed("CATALOGNUMBER", "SHVL 795"),
        ]);

        assert_eq!(
            set.musicbrainz_artist_id.as_deref(),
            Some("83d91898-7763-47d7-b03b-b92132375c47")
        );
        assert_eq!(
            set.musicbrainz_album_artist_id.as_deref(),
            Some("83d91898-7763-47d7-b03b-b92132375c47")
        );
        assert_eq!(
            set.musicbrainz_release_group_id.as_deref(),
            Some("f5093c06-23e3-404f-aeaa-40f72885ee3a")
        );
        assert_eq!(
            set.musicbrainz_release_track_id.as_deref(),
            Some("3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11")
        );
        assert_eq!(set.barcode.as_deref(), Some("724382920021"));
        assert_eq!(set.catalog_number.as_deref(), Some("SHVL 795"));
    }

    #[test]
    fn a_recording_id_reaches_the_set_from_the_identifier_frame_musicbrainz_owns() {
        const RECORDING: &str = "83d91898-7763-47d7-b03b-b92132375c47";

        let tagged = absorb(&[identified(MUSICBRAINZ_OWNER, RECORDING.as_bytes())]);
        assert_eq!(tagged.musicbrainz_track_id.as_deref(), Some(RECORDING));

        let elsewhere = absorb(&[identified("http://www.cddb.com", RECORDING.as_bytes())]);
        assert_eq!(elsewhere.musicbrainz_track_id, None);

        let empty = absorb(&[identified(MUSICBRAINZ_OWNER, b"  ")]);
        assert_eq!(empty.musicbrainz_track_id, None);
    }

    #[test]
    fn replay_gain_survives_the_units_encoders_write() {
        let set = absorb(&[
            tag(StandardTag::ReplayGainTrackGain(text("-7.06 dB"))),
            tag(StandardTag::ReplayGainAlbumGain(text("+3.5DB"))),
            tag(StandardTag::ReplayGainTrackPeak(text("0.987654"))),
        ]);

        assert_eq!(
            set.replay_gain.track_gain,
            Some(Decibels::new(-7.06).expect("finite"))
        );
        assert_eq!(
            set.replay_gain.album_gain,
            Some(Decibels::new(3.5).expect("finite"))
        );
        assert_eq!(set.replay_gain.track_peak, Some(0.987_654));
    }

    #[test]
    fn an_unparseable_replay_gain_value_is_dropped_rather_than_failing_the_read() {
        let set = absorb(&[
            tag(StandardTag::ReplayGainTrackGain(text("loud"))),
            tag(StandardTag::ReplayGainTrackPeak(text("-1.0"))),
        ]);

        assert_eq!(set.replay_gain.track_gain, None);
        assert_eq!(set.replay_gain.track_peak, None);
    }

    #[test]
    fn a_blank_replay_gain_after_a_real_one_leaves_the_real_one_standing() {
        let set = absorb(&[
            tag(StandardTag::ReplayGainTrackGain(text("-7.06 dB"))),
            tag(StandardTag::ReplayGainTrackPeak(text("0.987654"))),
            tag(StandardTag::ReplayGainTrackGain(text(""))),
            tag(StandardTag::ReplayGainTrackPeak(text("  "))),
        ]);

        assert_eq!(
            set.replay_gain.track_gain,
            Some(Decibels::new(-7.06).expect("finite"))
        );
        assert_eq!(set.replay_gain.track_peak, Some(0.987_654));
    }

    #[test]
    fn the_most_specific_date_tag_wins_whatever_order_it_arrives_in() {
        let set = absorb(&[
            tag(StandardTag::ReleaseYear(1969)),
            tag(StandardTag::RecordingDate(text("1969-04-11"))),
            tag(StandardTag::OriginalReleaseDate(text("1990-01-01"))),
        ]);

        assert_eq!(set.date.as_deref(), Some("1969-04-11"));
    }

    #[test]
    fn a_year_only_tag_is_kept_when_nothing_narrower_is_present() {
        let set = absorb(&[tag(StandardTag::ReleaseYear(1969))]);

        assert_eq!(set.date.as_deref(), Some("1969"));
    }

    #[test]
    fn a_blank_text_tag_is_no_tag_and_a_padded_one_is_stored_trimmed() {
        let blank = absorb(&[
            tag(StandardTag::TrackTitle(text(""))),
            tag(StandardTag::Artist(text("   "))),
            tag(StandardTag::Composer(text("\t\n"))),
        ]);
        assert_eq!(blank.title, None);
        assert_eq!(blank.artist, None);
        assert_eq!(blank.credits.composer, None);

        let padded = absorb(&[tag(StandardTag::TrackTitle(text("  Echoes \n")))]);
        assert_eq!(padded.title.as_deref(), Some("Echoes"));

        let blanked_after = absorb(&[
            tag(StandardTag::TrackTitle(text("Echoes"))),
            tag(StandardTag::TrackTitle(text(" "))),
        ]);
        assert_eq!(blanked_after.title.as_deref(), Some("Echoes"));

        let named_after = absorb(&[
            tag(StandardTag::TrackTitle(text(" "))),
            tag(StandardTag::TrackTitle(text("Echoes"))),
            tag(StandardTag::TrackTitle(text("Fearless"))),
        ]);
        assert_eq!(named_after.title.as_deref(), Some("Fearless"));
    }

    #[test]
    fn the_compilation_flag_is_absent_until_a_tag_sets_it() {
        assert!(!absorb(&[tag(StandardTag::Album(text("Anything")))]).compilation);
        assert!(absorb(&[tag(StandardTag::CompilationFlag(true))]).compilation);
        assert!(!absorb(&[tag(StandardTag::CompilationFlag(false))]).compilation);
    }

    #[test]
    fn the_words_a_file_carries_reach_the_set_whichever_name_its_writer_used() {
        let carried = absorb(&[tag(StandardTag::Lyrics(text("all that you touch")))]);
        let vorbis = absorb(&[
            keyed("TRACKNUMBER", "2"),
            keyed("UNSYNCEDLYRICS", "all that you see"),
        ]);

        assert_eq!(carried.lyrics.as_deref(), Some("all that you touch"));
        assert_eq!(vorbis.lyrics.as_deref(), Some("all that you see"));
        assert_eq!(
            absorb(&[keyed("LYRICS", "nothing says vorbis")]).lyrics,
            None
        );
    }

    #[test]
    fn an_opus_loudness_gain_is_read_as_replay_gain_against_its_own_reference() {
        let set = absorb(&[
            keyed("R128_TRACK_GAIN", "-1792"),
            keyed("r128_album_gain", "256"),
        ]);

        assert_eq!(set.replay_gain.track_gain, Decibels::new(-2.0).ok());
        assert_eq!(set.replay_gain.album_gain, Decibels::new(6.0).ok());
        assert_eq!(
            absorb(&[keyed("R128_TRACK_GAIN", "-7 dB")])
                .replay_gain
                .track_gain,
            None
        );
    }

    #[test]
    fn a_synchronised_frame_outranks_the_plain_words_beside_it_as_a_timed_sheet() {
        let mut frame = vec![3, b'e', b'n', b'g', 2, 1, 0];
        frame.extend_from_slice(b"all that you touch\0");
        frame.extend_from_slice(&1_500_u32.to_be_bytes());
        let synced = Tag::new(RawTag::new("SYLT", frame.as_slice()));

        let set = absorb(&[synced, tag(StandardTag::Lyrics(text("all that you touch")))]);

        assert_eq!(
            set.lyrics.as_deref(),
            Some("[00:01.500]all that you touch\n")
        );
    }

    #[test]
    fn an_info_id_the_set_now_has_a_field_for_no_longer_stops_at_the_raw_listing() {
        let set = absorb_info(&[
            info("IENG", "Alan Parsons"),
            info("IMUS", "Richard Wright"),
            info("IWRI", "Roger Waters"),
            info("IPRO", "Pink Floyd"),
            info("ICOP", "(c) 1971 Harvest"),
            info("ISFT", "Exact Audio Copy"),
            info("ICMT", "ripped from the 1994 remaster"),
        ]);

        assert_eq!(set.credits.engineer.as_deref(), Some("Alan Parsons"));
        assert_eq!(set.credits.composer.as_deref(), Some("Richard Wright"));
        assert_eq!(set.credits.lyricist.as_deref(), Some("Roger Waters"));
        assert_eq!(set.credits.producer.as_deref(), Some("Pink Floyd"));
        assert_eq!(set.copyright.as_deref(), Some("(c) 1971 Harvest"));
        assert_eq!(set.encoder.as_deref(), Some("Exact Audio Copy"));
        assert_eq!(
            set.comment.as_deref(),
            Some("ripped from the 1994 remaster")
        );
    }

    #[test]
    fn a_matroska_target_above_the_album_names_the_set_it_belongs_to() {
        let set = absorb(&[
            mapped(
                "COLLECTION@TITLE",
                "The Early Years",
                StandardTag::CollectionTitle(text("The Early Years")),
            ),
            mapped(
                "EDITION@TITLE",
                "2016 Remaster",
                StandardTag::EditionTitle(text("2016 Remaster")),
            ),
            mapped("ALBUM@TITLE", "Meddle", StandardTag::Album(text("Meddle"))),
        ]);

        assert_eq!(set.collection.as_deref(), Some("The Early Years"));
        assert_eq!(set.edition.as_deref(), Some("2016 Remaster"));
        assert_eq!(set.album.as_deref(), Some("Meddle"));
    }

    #[test]
    fn a_credit_the_container_could_not_map_still_reaches_the_set_under_vorbis_naming() {
        let set = absorb(&[
            keyed("TRACKNUMBER", "2/12"),
            keyed("COMPOSER", "Richard Wright"),
            keyed("CONDUCTOR", "Ron Goodwin"),
            keyed("LYRICIST", "Roger Waters"),
            keyed("PERFORMER", "David Gilmour"),
            keyed("REMIXER", "James Guthrie"),
            keyed("ENGINEER", "Alan Parsons"),
            keyed("PRODUCER", "Pink Floyd"),
        ]);

        assert_eq!(set.credits.composer.as_deref(), Some("Richard Wright"));
        assert_eq!(set.credits.conductor.as_deref(), Some("Ron Goodwin"));
        assert_eq!(set.credits.lyricist.as_deref(), Some("Roger Waters"));
        assert_eq!(set.credits.performer.as_deref(), Some("David Gilmour"));
        assert_eq!(set.credits.remixer.as_deref(), Some("James Guthrie"));
        assert_eq!(set.credits.engineer.as_deref(), Some("Alan Parsons"));
        assert_eq!(set.credits.producer.as_deref(), Some("Pink Floyd"));
    }

    #[test]
    fn a_publication_name_the_container_left_unmapped_reaches_the_set_the_same_way() {
        let set = absorb(&[
            keyed("TRACKNUMBER", "2/12"),
            keyed("ORGANIZATION", "Harvest"),
            keyed("ISRC", "GBAYE7100195"),
            keyed("BPM", "68.5"),
            keyed("COPYRIGHT", "(c) 1971 Harvest"),
            keyed("ENCODER", "libFLAC 1.4.3"),
            keyed("GROUPING", "Side One"),
            keyed("COMMENT", "ripped from the 1994 remaster"),
        ]);

        assert_eq!(set.label.as_deref(), Some("Harvest"));
        assert_eq!(set.isrc.as_deref(), Some("GBAYE7100195"));
        assert_eq!(set.beats_per_minute, Some(68));
        assert_eq!(set.copyright.as_deref(), Some("(c) 1971 Harvest"));
        assert_eq!(set.encoder.as_deref(), Some("libFLAC 1.4.3"));
        assert_eq!(set.grouping.as_deref(), Some("Side One"));
        assert_eq!(
            set.comment.as_deref(),
            Some("ripped from the 1994 remaster")
        );
    }

    #[test]
    fn a_tempo_that_is_not_a_count_of_beats_is_dropped_rather_than_read_as_one() {
        for written in ["allegro", "-1", "NaN", "inf"] {
            let set = absorb(&[keyed("TRACKNUMBER", "2"), keyed("BPM", written)]);

            assert_eq!(set.beats_per_minute, None, "{written} was read as a tempo");
        }
    }

    #[test]
    fn a_zero_track_number_is_treated_as_absent() {
        let set = absorb(&[
            tag(StandardTag::TrackNumber(0)),
            tag(StandardTag::DiscNumber(1)),
        ]);

        assert_eq!(set.track_number, None);
        assert_eq!(set.disc_number, Some(1));
    }
}
