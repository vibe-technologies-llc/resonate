use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::{
        ffi::OsStrExt as _,
        fs::{self as unix_fs, MetadataExt as _},
    },
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use lofty::{
    TextEncoding,
    ape::{ApeItem, ApeTag},
    config::{ParseOptions, WriteOptions},
    error::FileEncodingError,
    file::{FileType, TaggedFileExt as _},
    id3::v2::{Frame, FrameId, Id3v2Tag, KeyValueFrame},
    io::FileLike,
    picture::{MimeType, Picture, PictureType},
    probe::Probe,
    tag::{ItemKey, ItemValue, Tag, TagExt as _, TagItem, TagType},
};
use resonate_core::{Decibels, MediaLocation};
use rustix::fs::XattrFlags;

use crate::{
    CoverArt, Error, Picturing, Result, TagSet,
    counted::{Counted, counts},
    overlay::{Landing, Overlay},
    padded::Head,
    probe_pictured,
    source::Sources,
    tags::{TagSource, Tagged},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    TrackNumber,
    TrackTotal,
    DiscNumber,
    DiscTotal,
    Date,
    Label,
    CatalogNumber,
    Barcode,
    Isrc,
    MusicBrainzTrackId,
    MusicBrainzReleaseTrackId,
    MusicBrainzAlbumId,
    MusicBrainzArtistId,
    MusicBrainzAlbumArtistId,
    MusicBrainzReleaseGroupId,
    Genre,
    Composer,
    Conductor,
    Lyricist,
    Performer,
    Remixer,
    Engineer,
    Producer,
    Comment,
    BeatsPerMinute,
    Compilation,
    Grouping,
    Copyright,
    ReplayGainTrackGain,
    ReplayGainTrackPeak,
    ReplayGainAlbumGain,
    ReplayGainAlbumPeak,
}

const COMPILED: &str = "1";
const MUSICIAN_CREDITS: &str = "TMCL";
const LISTED_APART: &str = "; ";
const APE_BEATS: &str = "BPM";

impl TagField {
    pub const ALL: [Self; 36] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::AlbumArtist,
        Self::TrackNumber,
        Self::TrackTotal,
        Self::DiscNumber,
        Self::DiscTotal,
        Self::Date,
        Self::Label,
        Self::CatalogNumber,
        Self::Barcode,
        Self::Isrc,
        Self::MusicBrainzTrackId,
        Self::MusicBrainzReleaseTrackId,
        Self::MusicBrainzAlbumId,
        Self::MusicBrainzArtistId,
        Self::MusicBrainzAlbumArtistId,
        Self::MusicBrainzReleaseGroupId,
        Self::Genre,
        Self::Composer,
        Self::Conductor,
        Self::Lyricist,
        Self::Performer,
        Self::Remixer,
        Self::Engineer,
        Self::Producer,
        Self::Comment,
        Self::BeatsPerMinute,
        Self::Compilation,
        Self::Grouping,
        Self::Copyright,
        Self::ReplayGainTrackGain,
        Self::ReplayGainTrackPeak,
        Self::ReplayGainAlbumGain,
        Self::ReplayGainAlbumPeak,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::AlbumArtist => "album artist",
            Self::TrackNumber => "track number",
            Self::TrackTotal => "track total",
            Self::DiscNumber => "disc number",
            Self::DiscTotal => "disc total",
            Self::Date => "date",
            Self::Label => "label",
            Self::CatalogNumber => "catalogue number",
            Self::Barcode => "barcode",
            Self::Isrc => "ISRC",
            Self::MusicBrainzTrackId => "musicbrainz track id",
            Self::MusicBrainzReleaseTrackId => "musicbrainz release track id",
            Self::MusicBrainzAlbumId => "musicbrainz album id",
            Self::MusicBrainzArtistId => "musicbrainz artist id",
            Self::MusicBrainzAlbumArtistId => "musicbrainz album artist id",
            Self::MusicBrainzReleaseGroupId => "musicbrainz release group id",
            Self::Genre => "genre",
            Self::Composer => "composer",
            Self::Conductor => "conductor",
            Self::Lyricist => "lyricist",
            Self::Performer => "performer",
            Self::Remixer => "remixer",
            Self::Engineer => "engineer",
            Self::Producer => "producer",
            Self::Comment => "comment",
            Self::BeatsPerMinute => "bpm",
            Self::Compilation => "compilation",
            Self::Grouping => "grouping",
            Self::Copyright => "copyright",
            Self::ReplayGainTrackGain => "replaygain track gain",
            Self::ReplayGainTrackPeak => "replaygain track peak",
            Self::ReplayGainAlbumGain => "replaygain album gain",
            Self::ReplayGainAlbumPeak => "replaygain album peak",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.as_str() == name)
    }

    pub fn read(self, tags: &TagSet) -> Option<String> {
        match self {
            Self::Title => tags.title.clone(),
            Self::Artist => tags.artist.clone(),
            Self::Album => tags.album.clone(),
            Self::AlbumArtist => tags.album_artist.clone(),
            Self::TrackNumber => tags.track_number.map(|number| number.to_string()),
            Self::TrackTotal => tags.track_total.map(|total| total.to_string()),
            Self::DiscNumber => tags.disc_number.map(|number| number.to_string()),
            Self::DiscTotal => tags.disc_total.map(|total| total.to_string()),
            Self::Date => tags.date.clone(),
            Self::Label => tags.label.clone(),
            Self::CatalogNumber => tags.catalog_number.clone(),
            Self::Barcode => tags.barcode.clone(),
            Self::Isrc => tags.isrc.clone(),
            Self::MusicBrainzTrackId => tags.musicbrainz_track_id.clone(),
            Self::MusicBrainzReleaseTrackId => tags.musicbrainz_release_track_id.clone(),
            Self::MusicBrainzAlbumId => tags.musicbrainz_album_id.clone(),
            Self::MusicBrainzArtistId => tags.musicbrainz_artist_id.clone(),
            Self::MusicBrainzAlbumArtistId => tags.musicbrainz_album_artist_id.clone(),
            Self::MusicBrainzReleaseGroupId => tags.musicbrainz_release_group_id.clone(),
            Self::Genre => tags.genre.clone(),
            Self::Composer => tags.credits.composer.clone(),
            Self::Conductor => tags.credits.conductor.clone(),
            Self::Lyricist => tags.credits.lyricist.clone(),
            Self::Performer => tags.credits.performer.clone(),
            Self::Remixer => tags.credits.remixer.clone(),
            Self::Engineer => tags.credits.engineer.clone(),
            Self::Producer => tags.credits.producer.clone(),
            Self::Comment => tags.comment.clone(),
            Self::BeatsPerMinute => tags.beats_per_minute.map(|beats| beats.to_string()),
            Self::Compilation => tags.compilation.then(|| COMPILED.to_owned()),
            Self::Grouping => tags.grouping.clone(),
            Self::Copyright => tags.copyright.clone(),
            Self::ReplayGainTrackGain => tags.replay_gain.track_gain.map(spelled_gain),
            Self::ReplayGainTrackPeak => tags.replay_gain.track_peak.map(spelled_peak),
            Self::ReplayGainAlbumGain => tags.replay_gain.album_gain.map(spelled_gain),
            Self::ReplayGainAlbumPeak => tags.replay_gain.album_peak.map(spelled_peak),
        }
    }

    fn key_in(self, kind: TagType) -> Option<ItemKey> {
        match (self, kind) {
            (Self::BeatsPerMinute, TagType::VorbisComments) => Some(ItemKey::Bpm),
            (Self::BeatsPerMinute, TagType::Ape) | (Self::Performer, TagType::Id3v2) => None,
            (field, _) => Some(field.key()),
        }
    }

    const fn unkeyed_in(self, kind: TagType) -> bool {
        matches!(
            (self, kind),
            (Self::BeatsPerMinute, TagType::Ape) | (Self::Performer, TagType::Id3v2)
        )
    }

    const fn key(self) -> ItemKey {
        match self {
            Self::Title => ItemKey::TrackTitle,
            Self::Artist => ItemKey::TrackArtist,
            Self::Album => ItemKey::AlbumTitle,
            Self::AlbumArtist => ItemKey::AlbumArtist,
            Self::TrackNumber => ItemKey::TrackNumber,
            Self::TrackTotal => ItemKey::TrackTotal,
            Self::DiscNumber => ItemKey::DiscNumber,
            Self::DiscTotal => ItemKey::DiscTotal,
            Self::Date => ItemKey::RecordingDate,
            Self::Label => ItemKey::Label,
            Self::CatalogNumber => ItemKey::CatalogNumber,
            Self::Barcode => ItemKey::Barcode,
            Self::Isrc => ItemKey::Isrc,
            Self::MusicBrainzTrackId => ItemKey::MusicBrainzRecordingId,
            Self::MusicBrainzReleaseTrackId => ItemKey::MusicBrainzTrackId,
            Self::MusicBrainzAlbumId => ItemKey::MusicBrainzReleaseId,
            Self::MusicBrainzArtistId => ItemKey::MusicBrainzArtistId,
            Self::MusicBrainzAlbumArtistId => ItemKey::MusicBrainzReleaseArtistId,
            Self::MusicBrainzReleaseGroupId => ItemKey::MusicBrainzReleaseGroupId,
            Self::Genre => ItemKey::Genre,
            Self::Composer => ItemKey::Composer,
            Self::Conductor => ItemKey::Conductor,
            Self::Lyricist => ItemKey::Lyricist,
            Self::Performer => ItemKey::Performer,
            Self::Remixer => ItemKey::Remixer,
            Self::Engineer => ItemKey::Engineer,
            Self::Producer => ItemKey::Producer,
            Self::Comment => ItemKey::Comment,
            Self::BeatsPerMinute => ItemKey::IntegerBpm,
            Self::Compilation => ItemKey::FlagCompilation,
            Self::Grouping => ItemKey::ContentGroup,
            Self::Copyright => ItemKey::CopyrightMessage,
            Self::ReplayGainTrackGain => ItemKey::ReplayGainTrackGain,
            Self::ReplayGainTrackPeak => ItemKey::ReplayGainTrackPeak,
            Self::ReplayGainAlbumGain => ItemKey::ReplayGainAlbumGain,
            Self::ReplayGainAlbumPeak => ItemKey::ReplayGainAlbumPeak,
        }
    }
}

fn spelled_gain(gain: Decibels) -> String {
    format!("{:+.2} dB", gain.get())
}

fn spelled_peak(peak: f32) -> String {
    format!("{peak:.6}")
}

impl fmt::Display for TagField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagEdit {
    pub field: TagField,
    pub value: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Writing<'a> {
    pub edits: &'a [TagEdit],
    pub taken: &'a [TagField],
    pub picture: Option<&'a CoverArt>,
    pub unpictured: bool,
    pub popularity: Option<Popularity>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Popularity {
    pub favourite: bool,
    pub plays: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rated {
    #[default]
    Unrateable,
    Unrated {
        plays: Option<u64>,
    },
    Favourite {
        plays: Option<u64>,
    },
}

impl Rated {
    pub fn differs_from(self, wanted: Popularity) -> bool {
        let counted_otherwise =
            |plays: Option<u64>| plays.is_some_and(|plays| plays != wanted.plays);
        match self {
            Self::Unrateable => false,
            Self::Unrated { plays } => wanted.favourite || counted_otherwise(plays),
            Self::Favourite { plays } => !wanted.favourite || counted_otherwise(plays),
        }
    }
}

pub const RATED_BY: &str = "resonate";

const FAVOURITE_STARS: u8 = 5;

pub trait TagSink: TagSource {
    fn writes(&self, location: &MediaLocation) -> bool;
    fn write(&self, location: &MediaLocation, writing: Writing<'_>) -> Result<()>;
    fn rated(&self, location: &MediaLocation) -> Result<Rated>;
}

pub struct FileTags {
    sources: Sources,
}

impl FileTags {
    pub const fn over(sources: Sources) -> Self {
        Self { sources }
    }

    fn writable<'a>(&self, location: &'a MediaLocation) -> Result<&'a Path> {
        let path = location.as_path().ok_or_else(|| Error::LocatorNotUsable {
            location: location.clone(),
        })?;
        if !self.writes(location) {
            return Err(Error::Unwritable {
                location: location.clone(),
            });
        }
        Ok(path)
    }
}

impl Default for FileTags {
    fn default() -> Self {
        Self::over(Sources::local())
    }
}

impl TagSource for FileTags {
    fn read(&self, location: &MediaLocation, picturing: Picturing) -> Result<Tagged> {
        let (info, picture) = probe_pictured(&self.sources, location, picturing)?;
        Ok(Tagged {
            tags: info.tags,
            picture,
        })
    }
}

impl TagSink for FileTags {
    fn writes(&self, location: &MediaLocation) -> bool {
        location
            .as_path()
            .and_then(FileType::from_path)
            .is_some_and(read_back_here)
    }

    fn write(&self, location: &MediaLocation, writing: Writing<'_>) -> Result<()> {
        let named = self.writable(location)?;
        let linked = fs::symlink_metadata(named).is_ok_and(|held| held.is_symlink());
        let resolved = match linked {
            true => fs::canonicalize(named).map_err(|source| Error::Io {
                location: location.clone(),
                source,
            })?,
            false => named.to_path_buf(),
        };
        let path = resolved.as_path();
        let taken = Taken::of(path).map_err(|source| Error::Io {
            location: location.clone(),
            source,
        })?;
        let rechunked = match tag_in_front_of_a_riff(path) {
            Ok(Some(riff_at)) => {
                let staged = staged_beside(path);
                if let Err(source) = chunked(path, &staged, riff_at) {
                    let _ = fs::remove_file(&staged);
                    return Err(Error::Io {
                        location: location.clone(),
                        source,
                    });
                }
                Some(staged)
            }
            Ok(None) => None,
            Err(source) => {
                return Err(Error::Io {
                    location: location.clone(),
                    source,
                });
            }
        };
        let written = self.written(path, rechunked.as_deref(), location, writing, taken);
        if written.is_err()
            && let Some(staged) = rechunked
        {
            let _ = fs::remove_file(staged);
        }
        written
    }

    fn rated(&self, location: &MediaLocation) -> Result<Rated> {
        self.rating_of(location)
    }
}

const RIFF: &[u8; 4] = b"RIFF";
const WAVE: &[u8; 4] = b"WAVE";
const ID3_CHUNK: &[u8; 4] = b"id3 ";
const RIFF_HEADER_BYTES: u64 = 12;
const CHUNK_HEADER_BYTES: u32 = 8;

fn tag_in_front_of_a_riff(path: &Path) -> io::Result<Option<u64>> {
    let mut file = File::open(path)?;
    let Some(riff_at) = crate::prescan::past_id3(&mut file).filter(|at| *at > 0) else {
        return Ok(None);
    };
    file.seek(SeekFrom::Start(riff_at))?;
    let mut header = [0_u8; RIFF_HEADER_BYTES as usize];
    if file.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    Ok((header.starts_with(RIFF) && header[8..] == *WAVE).then_some(riff_at))
}

fn chunked(path: &Path, staged: &Path, riff_at: u64) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut tag = vec![0_u8; usize::try_from(riff_at).unwrap_or(usize::MAX)];
    file.read_exact(&mut tag)?;
    let mut header = [0_u8; RIFF_HEADER_BYTES as usize];
    file.read_exact(&mut header)?;
    let declared = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let tag_bytes =
        u32::try_from(tag.len()).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    let padded = tag_bytes + tag_bytes % 2;
    let grown = declared
        .checked_add(CHUNK_HEADER_BYTES + padded)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;

    let mut written = io::BufWriter::new(File::create(staged)?);
    written.write_all(RIFF)?;
    written.write_all(&grown.to_le_bytes())?;
    written.write_all(WAVE)?;
    let within = u64::from(declared).saturating_sub(WAVE.len() as u64);
    io::copy(&mut (&mut file).take(within), &mut written)?;
    written.write_all(ID3_CHUNK)?;
    written.write_all(&tag_bytes.to_le_bytes())?;
    written.write_all(&tag)?;
    if padded > tag_bytes {
        written.write_all(&[0])?;
    }
    io::copy(&mut file, &mut written)?;
    written
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()
}

impl FileTags {
    fn written(
        &self,
        path: &Path,
        rechunked: Option<&Path>,
        location: &MediaLocation,
        writing: Writing<'_>,
        taken: Taken,
    ) -> Result<()> {
        let mut tagged = opened(rechunked.unwrap_or(path), location)?;
        let kind = tagged.file_type();
        let options = WriteOptions::default()
            .use_id3v23(Counted::holds_id3v2_3(rechunked.unwrap_or(path), kind));
        let others = cleared_elsewhere(&tagged, writing.taken);
        if tagged.primary_tag().is_none() {
            let kind = tagged.primary_tag_type();
            tagged.insert_tag(Tag::new(kind));
        }

        let tag = tagged
            .primary_tag_mut()
            .expect("a primary tag this call has just put there");
        for edit in writing.edits {
            if let Some(key) = edit.field.key_in(tag.tag_type()) {
                tag.insert_text(key, edit.value.clone());
            }
        }
        for field in writing.taken {
            if let Some(key) = field.key_in(tag.tag_type()) {
                tag.remove_key(key);
            }
        }
        if writing.unpictured || writing.picture.is_some() {
            uncovered(tag);
        }
        if let Some(picture) = writing.picture {
            tag.push_picture(front_cover(picture));
        }
        let counting = writing.popularity.filter(|_| counts(tag.tag_type()));
        if let Some(popularity) = writing.popularity
            && keeps_popularimeters(tag.tag_type())
        {
            rate(tag, popularity);
        }

        let unkeyed: Vec<(TagField, Option<&str>)> = writing
            .edits
            .iter()
            .map(|edit| (edit.field, Some(edit.value.as_str())))
            .chain(writing.taken.iter().map(|field| (*field, None)))
            .filter(|(field, _)| field.unkeyed_in(tag.tag_type()))
            .collect();
        let saving = Saving {
            kind,
            tag,
            counting,
            unkeyed: &unkeyed,
            others: &others,
            taken,
            options,
        };
        match rechunked {
            Some(staged) => landed_through(staged, path, location, &saving),
            None => landed(path, location, &saving),
        }
    }

    fn rating_of(&self, location: &MediaLocation) -> Result<Rated> {
        let path = self.writable(location)?;
        let tagged = opened_for_its_tags(path, location)?;
        let kind = tagged.primary_tag_type();
        if !rates(kind) {
            return Ok(Rated::Unrateable);
        }
        let counted = if counts(kind) {
            Counted::read(path, tagged.file_type()).map_err(|source| Error::TagsUnread {
                location: location.clone(),
                source,
            })?
        } else {
            None
        };
        let ours = tagged.primary_tag().and_then(|tag| {
            tag.get_strings(ItemKey::Popularimeter)
                .filter_map(Popularimeter::read)
                .find(|popularimeter| popularimeter.is_ours(kind))
                .map(|popularimeter| (popularimeter.stars, popularimeter.plays))
        });
        let favourite = match counted.as_ref().and_then(Counted::favoured_in_ape) {
            Some(favoured) => favoured,
            None => ours.is_some_and(|(stars, _)| stars == FAVOURITE_STARS),
        };
        let popularimeter_plays = ours
            .filter(|_| kind == TagType::Id3v2 && favourite)
            .map(|(_, plays)| plays);
        let plays = counts(kind).then(|| {
            counted
                .as_ref()
                .and_then(Counted::plays)
                .or(popularimeter_plays)
                .unwrap_or(0)
        });
        Ok(if favourite {
            Rated::Favourite { plays }
        } else {
            Rated::Unrated { plays }
        })
    }
}

struct Popularimeter<'a> {
    by: &'a str,
    stars: u8,
    plays: u64,
}

impl<'a> Popularimeter<'a> {
    fn read(text: &'a str) -> Option<Self> {
        let mut parts = text.splitn(3, '|');
        let by = parts.next()?;
        let stars = parts.next()?.parse().ok()?;
        let plays = parts.next()?.parse().ok()?;
        Some(Self { by, stars, plays })
    }

    fn is_ours(&self, kind: TagType) -> bool {
        !names_who_rated(kind) || self.by == RATED_BY
    }
}

fn uncovered(tag: &mut Tag) {
    let mut at = tag.pictures().len();
    while let Some(before) = at.checked_sub(1) {
        at = before;
        let kind = tag.pictures().get(at).map(Picture::pic_type);
        if kind.is_some_and(read_as_the_cover) {
            tag.remove_picture(at);
        }
    }
}

const fn read_as_the_cover(kind: PictureType) -> bool {
    matches!(
        kind,
        PictureType::CoverFront | PictureType::Other | PictureType::Undefined(_)
    )
}

fn cleared_elsewhere(tagged: &lofty::file::TaggedFile, taken: &[TagField]) -> Vec<Tag> {
    let primary = tagged.primary_tag_type();
    tagged
        .tags()
        .iter()
        .filter(|held| held.tag_type() != primary)
        .filter(|held| {
            taken
                .iter()
                .filter_map(|field| field.key_in(held.tag_type()))
                .any(|key| held.get(key).is_some())
        })
        .map(|held| {
            let mut cleared = held.clone();
            for key in taken
                .iter()
                .filter_map(|field| field.key_in(held.tag_type()))
            {
                cleared.remove_key(key);
            }
            cleared
        })
        .collect()
}

const fn rates(kind: TagType) -> bool {
    keeps_popularimeters(kind) || counts(kind)
}

const fn keeps_popularimeters(kind: TagType) -> bool {
    matches!(
        kind,
        TagType::Id3v2 | TagType::VorbisComments | TagType::Mp4Ilst | TagType::RiffInfo
    )
}

fn saved<F: FileLike>(
    tag: &Tag,
    counting: Option<Popularity>,
    unkeyed: &[(TagField, Option<&str>)],
    file: &mut F,
    options: WriteOptions,
) -> std::result::Result<(), FileEncodingError> {
    let concrete = (counting.is_some() || !unkeyed.is_empty())
        .then(|| Counted::of(tag.clone()))
        .flatten();
    let Some(mut counted) = concrete else {
        return tag.save_to(file, options);
    };

    if let Some(popularity) = counting {
        counted.count(popularity.plays);
        counted.favour_in_ape(popularity.favourite);
    }
    for (field, value) in unkeyed {
        match (field, &mut counted) {
            (TagField::Performer, Counted::Framed(frames)) => credit_performers(frames, *value),
            (TagField::BeatsPerMinute, Counted::Ape(items)) => beat(items, *value),
            _ => {}
        }
    }
    counted.save(file, options)
}

fn credit_performers(frames: &mut Id3v2Tag, performers: Option<&str>) {
    let id = FrameId::Valid(MUSICIAN_CREDITS.into());
    let named: Vec<&str> = performers
        .into_iter()
        .flat_map(|held| held.split(LISTED_APART))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let standing: Vec<(String, String)> = frames
        .remove(&id)
        .filter_map(|frame| match frame {
            Frame::KeyValue(credits) => Some(credits.key_value_pairs.into_owned()),
            _ => None,
        })
        .flatten()
        .map(|(role, name)| (role.into_owned(), name.into_owned()))
        .collect();

    let mut credited: Vec<(Cow<'static, str>, Cow<'static, str>)> = Vec::new();
    for name in named {
        if credited.iter().any(|(_, held)| held == name) {
            continue;
        }
        let role = standing
            .iter()
            .find(|(_, held)| held == name)
            .map_or_else(String::new, |(role, _)| role.clone());
        credited.push((role.into(), name.to_owned().into()));
    }
    if credited.is_empty() {
        return;
    }
    frames.insert(Frame::KeyValue(KeyValueFrame::new(
        id,
        TextEncoding::UTF8,
        credited,
    )));
}

fn beat(items: &mut ApeTag, beats: Option<&str>) {
    items.remove(APE_BEATS);
    let Some(beats) = beats else {
        return;
    };
    match ApeItem::new(APE_BEATS.to_owned(), ItemValue::Text(beats.to_owned())) {
        Ok(item) => items.insert(item),
        Err(error) => tracing::debug!(%error, "a tempo could not be an APE item"),
    }
}

const fn names_who_rated(kind: TagType) -> bool {
    matches!(kind, TagType::Id3v2 | TagType::VorbisComments)
}

fn rate(tag: &mut Tag, popularity: Popularity) {
    let kind = tag.tag_type();
    tag.retain(|item| {
        item.key() != ItemKey::Popularimeter
            || !item
                .value()
                .text()
                .and_then(Popularimeter::read)
                .is_some_and(|popularimeter| popularimeter.is_ours(kind))
    });
    if popularity.favourite {
        tag.push(TagItem::new(
            ItemKey::Popularimeter,
            ItemValue::Text(format!("{RATED_BY}|{FAVOURITE_STARS}|{}", popularity.plays)),
        ));
    }
}

const LEEWAY_PAST_THE_HEAD: u64 = 64 << 10;

struct Saving<'a> {
    kind: FileType,
    tag: &'a Tag,
    counting: Option<Popularity>,
    unkeyed: &'a [(TagField, Option<&'a str>)],
    others: &'a [Tag],
    taken: Taken,
    options: WriteOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Taken {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
}

impl Taken {
    fn of(path: &Path) -> io::Result<Self> {
        rustix::fs::access(path, rustix::fs::Access::WRITE_OK)?;
        let held = fs::metadata(path)?;
        Ok(Self {
            device: held.dev(),
            inode: held.ino(),
            length: held.len(),
            modified: (held.mtime(), held.mtime_nsec()),
        })
    }

    fn still_stands(self, path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|held| {
            Self {
                device: held.dev(),
                inode: held.ino(),
                length: held.len(),
                modified: (held.mtime(), held.mtime_nsec()),
            } == self
        })
    }
}

impl Saving<'_> {
    fn written_into<F: FileLike>(
        &self,
        file: &mut F,
    ) -> std::result::Result<(), FileEncodingError> {
        file.rewind()?;
        saved(self.tag, self.counting, self.unkeyed, file, self.options)?;
        self.others_written_into(file)
    }

    fn others_written_into<F: FileLike>(
        &self,
        file: &mut F,
    ) -> std::result::Result<(), FileEncodingError> {
        for other in self.others {
            file.rewind()?;
            other.save_to(file, self.options)?;
        }
        Ok(())
    }

    fn written_into_the_head<F: FileLike>(
        &self,
        head: Head,
        overlay: &mut F,
    ) -> std::result::Result<bool, FileEncodingError> {
        let stood = overlay.len()?;
        let tail = stood.saturating_sub(head.length).min(LEEWAY_PAST_THE_HEAD);
        if tail == 0 {
            return Ok(false);
        }

        let length = usize::try_from(head.length + tail)
            .map_err(|_| io::Error::from(io::ErrorKind::FileTooLarge))?;
        let mut held = vec![0_u8; length];
        overlay.rewind()?;
        overlay.read_exact(&mut held)?;
        let past = held[usize::try_from(head.length).unwrap_or(length)..].to_vec();

        let mut copy = io::Cursor::new(held);
        saved(
            self.tag,
            self.counting,
            self.unkeyed,
            &mut copy,
            self.options,
        )?;
        let written = copy.into_inner();
        let Some(written) = written.strip_suffix(past.as_slice()) else {
            return Ok(false);
        };
        let Some(absorbed) = head.absorbed(written) else {
            return Ok(false);
        };

        overlay.rewind()?;
        overlay.write_all(&absorbed)?;
        Ok(true)
    }
}

fn landed(path: &Path, location: &MediaLocation, saving: &Saving<'_>) -> Result<()> {
    let unread = |source| Error::Io {
        location: location.clone(),
        source,
    };
    sweep_what_a_dead_writer_staged(path);
    match cloned_beside(path) {
        Ok(Some(staged)) => return landed_through(&staged, path, location, saving),
        Ok(None) => {}
        Err(source) if source.kind() == io::ErrorKind::PermissionDenied => {}
        Err(source) => return Err(unread(source)),
    }
    if landed_in_place(path, location, saving)? {
        return Ok(());
    }

    let staged = staged_beside(path);
    match fs::copy(path, &staged) {
        Ok(_) => landed_through(&staged, path, location, saving),
        Err(source) if source.kind() == io::ErrorKind::PermissionDenied => {
            let _ = fs::remove_file(&staged);
            landed_from_elsewhere(path, location, saving)
        }
        Err(source) => {
            let _ = fs::remove_file(&staged);
            Err(unread(source))
        }
    }
}

fn landed_from_elsewhere(path: &Path, location: &MediaLocation, saving: &Saving<'_>) -> Result<()> {
    let mut refused = io::Error::from(io::ErrorKind::PermissionDenied);
    for folder in crate::spool::spooled_under() {
        let staged = staged_in(&folder, path);
        match fs::copy(path, &staged) {
            Ok(_) => return landed_by(&staged, path, location, saving, written_back),
            Err(source) => {
                let _ = fs::remove_file(&staged);
                refused = source;
            }
        }
    }
    Err(Error::Io {
        location: location.clone(),
        source: refused,
    })
}

fn cloned_beside(path: &Path) -> io::Result<Option<PathBuf>> {
    let file = File::open(path)?;
    let staged = staged_beside(path);
    let into = File::create_new(&staged)?;
    if let Err(error) = rustix::fs::ioctl_ficlone(&into, &file) {
        tracing::trace!(path = %path.display(), %error, "the file cannot be cloned");
        drop(into);
        fs::remove_file(&staged)?;
        return Ok(None);
    }
    Ok(Some(staged))
}

fn landed_in_place(path: &Path, location: &MediaLocation, saving: &Saving<'_>) -> Result<bool> {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return Ok(false);
    };
    let unread = |source| Error::Io {
        location: location.clone(),
        source,
    };
    let mut overlay = Overlay::over(&file).map_err(unread)?;
    let fitted =
        match Head::of(saving.kind, &mut &file) {
            Some(head) => saving
                .written_into_the_head(head, &mut overlay)
                .and_then(|fits| match fits {
                    true => saving.others_written_into(&mut overlay).map(|()| true),
                    false => Ok(false),
                }),
            None => saving.written_into(&mut overlay).map(|()| true),
        };
    match fitted {
        Ok(true) => {}
        Ok(false) => return Ok(false),
        Err(_) if overlay.outgrown() => return Ok(false),
        Err(source) => {
            return Err(Error::TagsUnwritten {
                location: location.clone(),
                source,
            });
        }
    }

    Ok(match overlay.land().map_err(unread)? {
        Landing::InPlace | Landing::Unchanged => true,
        Landing::TooWide => false,
    })
}

fn landed_through(
    staged: &Path,
    path: &Path,
    location: &MediaLocation,
    saving: &Saving<'_>,
) -> Result<()> {
    landed_by(staged, path, location, saving, settled_over)
}

fn landed_by(
    staged: &Path,
    path: &Path,
    location: &MediaLocation,
    saving: &Saving<'_>,
    settle: fn(&Path, &Path) -> io::Result<()>,
) -> Result<()> {
    let written = OpenOptions::new()
        .read(true)
        .write(true)
        .open(staged)
        .map_err(|source| Error::Io {
            location: location.clone(),
            source,
        })
        .and_then(|mut file| {
            saving
                .written_into(&mut file)
                .map_err(|source| Error::TagsUnwritten {
                    location: location.clone(),
                    source,
                })
        })
        .and_then(|()| {
            if saving.taken.still_stands(path) {
                return Ok(());
            }
            Err(Error::ChangedWhileWritten {
                location: location.clone(),
            })
        })
        .and_then(|()| {
            settle(staged, path).map_err(|source| Error::Io {
                location: location.clone(),
                source,
            })
        });
    if written.is_err() {
        let _ = fs::remove_file(staged);
    }
    written
}

static STAGED: AtomicU64 = AtomicU64::new(0);
const ATTRIBUTE_READS_AT_MOST: usize = 4;
const RUNNING_PROCESSES: &str = "/proc";
const STAGED_BY_AND_COUNTED: char = '-';

fn staged_beside(path: &Path) -> PathBuf {
    staged_in(path.parent().unwrap_or(Path::new("")), path)
}

fn staged_in(folder: &Path, path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    folder.join(format!(
        ".{stem}.{}{STAGED_BY_AND_COUNTED}{}.{extension}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    ))
}

fn staged_by(path: &Path, staged: &OsStr) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or_default();
    let (writer, counted) = staged
        .to_str()?
        .strip_prefix('.')?
        .strip_prefix(stem)?
        .strip_prefix('.')?
        .strip_suffix(extension)?
        .strip_suffix('.')?
        .split_once(STAGED_BY_AND_COUNTED)?;
    counted.parse::<u64>().ok()?;
    writer.parse().ok()
}

fn sweep_what_a_dead_writer_staged(path: &Path) {
    let Some(folder) = path.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(writer) = staged_by(path, &entry.file_name()) else {
            continue;
        };
        let alive = Path::new(RUNNING_PROCESSES)
            .join(writer.to_string())
            .exists();
        if writer == process::id() || alive {
            continue;
        }
        let left = entry.path();
        match fs::remove_file(&left) {
            Ok(()) => {
                tracing::info!(path = %left.display(), "swept a copy a tag write cut short left behind")
            }
            Err(error) => {
                tracing::debug!(%error, path = %left.display(), "a copy a tag write cut short left behind could not be swept")
            }
        }
    }
}

fn settled_over(staged: &Path, path: &Path) -> io::Result<()> {
    let standing = fs::metadata(path)?;
    File::open(staged)?.sync_all()?;
    if standing.nlink() > 1 || !carries_what_the_file_did(&standing, path, staged) {
        return written_back(staged, path);
    }

    fs::rename(staged, path)?;
    if let Some(folder) = path.parent() {
        File::open(folder)?.sync_all()?;
    }
    Ok(())
}

fn carries_what_the_file_did(standing: &Metadata, path: &Path, staged: &Path) -> bool {
    let owned = unix_fs::chown(staged, Some(standing.uid()), Some(standing.gid()));
    let moded = fs::set_permissions(staged, standing.permissions());
    let attributed = attributes_of(path).and_then(|attributes| {
        attributes.iter().try_for_each(|(name, value)| {
            rustix::fs::setxattr(staged, name.as_os_str(), value, XattrFlags::empty())
                .map_err(io::Error::from)
        })
    });

    match (owned, moded, attributed) {
        (Ok(()), Ok(()), Ok(())) => true,
        (owned, moded, attributed) => {
            tracing::debug!(
                path = %path.display(),
                owned = ?owned.err(),
                moded = ?moded.err(),
                attributed = ?attributed.err(),
                "a staged copy cannot carry what the file did, so it is written back in place"
            );
            false
        }
    }
}

fn attributes_of(path: &Path) -> io::Result<Vec<(OsString, Vec<u8>)>> {
    let listed = match read_sized(|buffer| rustix::fs::listxattr(path, buffer)) {
        Ok(listed) => listed,
        Err(error) if unsupported(&error) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    listed
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| {
            let name = OsStr::from_bytes(name);
            let value = read_sized(|buffer| rustix::fs::getxattr(path, name, buffer))?;
            Ok((name.to_os_string(), value))
        })
        .collect()
}

fn read_sized(read: impl Fn(&mut [u8]) -> rustix::io::Result<usize>) -> io::Result<Vec<u8>> {
    for _ in 0..ATTRIBUTE_READS_AT_MOST {
        let wanted = read(&mut [])?;
        let mut held = vec![0_u8; wanted];
        match read(&mut held) {
            Ok(read) => {
                held.truncate(read);
                return Ok(held);
            }
            Err(rustix::io::Errno::RANGE) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(rustix::io::Errno::RANGE.into())
}

fn unsupported(error: &io::Error) -> bool {
    error.raw_os_error() == Some(rustix::io::Errno::NOTSUP.raw_os_error())
}

fn written_back(staged: &Path, path: &Path) -> io::Result<()> {
    let mut whole = File::open(staged)?;
    let mut file = OpenOptions::new().write(true).open(path)?;
    let length = io::copy(&mut whole, &mut file)?;
    file.set_len(length)?;
    file.sync_all()?;
    fs::remove_file(staged)
}

fn front_cover(picture: &CoverArt) -> Picture {
    Picture::unchecked(picture.bytes.clone())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::from_str(picture.format.media_type()))
        .build()
}

const fn read_back_here(kind: FileType) -> bool {
    matches!(
        kind,
        FileType::Aac
            | FileType::Aiff
            | FileType::Ape
            | FileType::Flac
            | FileType::Mp4
            | FileType::Mpeg
            | FileType::Opus
            | FileType::Vorbis
            | FileType::Wav
            | FileType::WavPack
    )
}

fn opened(path: &Path, location: &MediaLocation) -> Result<lofty::file::TaggedFile> {
    opened_as(path, location, ParseOptions::new())
}

fn opened_for_its_tags(path: &Path, location: &MediaLocation) -> Result<lofty::file::TaggedFile> {
    opened_as(
        path,
        location,
        ParseOptions::new()
            .read_properties(false)
            .read_cover_art(false),
    )
}

fn opened_as(
    path: &Path,
    location: &MediaLocation,
    options: ParseOptions,
) -> Result<lofty::file::TaggedFile> {
    let opened = Probe::open(path)
        .map(|probe| probe.options(options))
        .map_err(|source| Error::TagsUnread {
            location: location.clone(),
            source,
        })?;
    let probed = opened.guess_file_type().map_err(|source| Error::Io {
        location: location.clone(),
        source,
    })?;

    if !probed.file_type().is_some_and(read_back_here) {
        return Err(Error::Unwritable {
            location: location.clone(),
        });
    }

    probed.read().map_err(|source| Error::TagsUnread {
        location: location.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::Cursor,
        path::PathBuf,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use lofty::ogg::tag::VorbisComments;

    use super::*;
    use crate::{ImageFormat, Pictured};

    const RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    const FRAMES: u32 = 4_410;
    const FLAC_BLOCK: u32 = 192;
    const STREAMINFO: u8 = 0x80;
    const CRC8_POLYNOMIAL: u8 = 0x07;
    const CRC16_POLYNOMIAL: u16 = 0x8005;
    const FLAC_44100: u8 = 0b1001;
    const FLAC_192_SAMPLES: u8 = 0b0001;
    const FLAC_16_BIT: u8 = 0b100;
    const FLAC_CONSTANT_SUBFRAME: u8 = 0x00;

    struct Folder {
        root: PathBuf,
    }

    impl Folder {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "resonate-writing-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("a writable temporary directory");
            Self { root }
        }

        fn holding(&self, name: &str, bytes: &[u8]) -> MediaLocation {
            let path = self.root.join(name);
            fs::write(&path, bytes).expect("a writable temporary file");
            MediaLocation::local(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_copy_a_dead_writer_staged_beside_the_track_is_swept_and_no_other() {
        const NEVER_A_PROCESS: u32 = 999_999_999;

        let folder = Folder::new();
        let track = folder.root.join("Echoes.flac");
        let left = folder
            .root
            .join(format!(".Echoes.{NEVER_A_PROCESS}-3.flac"));
        let ours = folder
            .root
            .join(format!(".Echoes.{}-0.flac", process::id()));
        let another = folder.root.join(format!(".Time.{NEVER_A_PROCESS}-0.flac"));
        let hidden = folder.root.join(".Echoes.live.flac");
        for path in [&track, &left, &ours, &another, &hidden] {
            fs::write(path, b"fLaC").expect("a writable temporary file");
        }

        sweep_what_a_dead_writer_staged(&track);

        assert!(!left.exists(), "a dead writer's copy was left");
        for kept in [&track, &ours, &another, &hidden] {
            assert!(kept.exists(), "{} was swept", kept.display());
        }
    }

    fn aiff() -> Vec<u8> {
        let mut common = Vec::new();
        common.extend_from_slice(&CHANNELS.to_be_bytes());
        common.extend_from_slice(&FRAMES.to_be_bytes());
        common.extend_from_slice(&BITS.to_be_bytes());
        let leading = RATE.leading_zeros();
        let exponent = u16::try_from(16_383 + 31 - leading).expect("a rate inside the exponent");
        common.extend_from_slice(&exponent.to_be_bytes());
        common.extend_from_slice(&(u64::from(RATE) << (leading + 32)).to_be_bytes());

        let stride = usize::from(CHANNELS) * usize::from(BITS / 8);
        let mut sound = vec![0_u8; 8];
        sound.resize(sound.len() + FRAMES as usize * stride, 0);

        let mut body = b"AIFF".to_vec();
        chunk(&mut body, b"COMM", &common);
        chunk(&mut body, b"SSND", &sound);

        let mut file = b"FORM".to_vec();
        file.extend_from_slice(
            &u32::try_from(body.len())
                .expect("a small file")
                .to_be_bytes(),
        );
        file.extend_from_slice(&body);
        file
    }

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("a small chunk")
                .to_be_bytes(),
        );
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn flac() -> Vec<u8> {
        let mut info = Vec::new();
        info.extend_from_slice(
            &u16::try_from(FLAC_BLOCK)
                .expect("a small block")
                .to_be_bytes(),
        );
        info.extend_from_slice(
            &u16::try_from(FLAC_BLOCK)
                .expect("a small block")
                .to_be_bytes(),
        );
        info.extend_from_slice(&[0; 6]);
        let packed = (u64::from(RATE) << 44)
            | (u64::from(CHANNELS - 1) << 41)
            | (u64::from(BITS - 1) << 36)
            | u64::from(FLAC_BLOCK);
        info.extend_from_slice(&packed.to_be_bytes());
        info.extend_from_slice(&[0; 16]);

        let mut file = b"fLaC".to_vec();
        file.push(STREAMINFO);
        file.extend_from_slice(
            &u32::try_from(info.len())
                .expect("a small block")
                .to_be_bytes()[1..],
        );
        file.extend_from_slice(&info);
        file.extend_from_slice(&flac_frame());
        file
    }

    fn flac_frame() -> Vec<u8> {
        let channels = u8::try_from(CHANNELS).expect("a small channel count");
        let mut frame = vec![
            0xFF,
            0xF8,
            FLAC_192_SAMPLES << 4 | FLAC_44100,
            (channels - 1) << 4 | FLAC_16_BIT << 1,
            0x00,
        ];
        frame.push(crc8(&frame));
        for _ in 0..channels {
            frame.push(FLAC_CONSTANT_SUBFRAME);
            frame.extend_from_slice(&[0; 2]);
        }
        frame.extend_from_slice(&crc16(&frame).to_be_bytes());
        frame
    }

    fn crc8(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0, |crc, byte| {
            (0..8).fold(crc ^ byte, |crc, _| {
                (crc << 1) ^ (if crc & 0x80 == 0 { 0 } else { CRC8_POLYNOMIAL })
            })
        })
    }

    fn crc16(bytes: &[u8]) -> u16 {
        bytes.iter().fold(0, |crc, byte| {
            (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
                (crc << 1)
                    ^ (if crc & 0x8000 == 0 {
                        0
                    } else {
                        CRC16_POLYNOMIAL
                    })
            })
        })
    }

    fn riff_chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("a small chunk")
                .to_le_bytes(),
        );
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn wave() -> Vec<u8> {
        wave_with(&[])
    }

    fn wave_with(info: &[(&[u8; 4], &str)]) -> Vec<u8> {
        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&CHANNELS.to_le_bytes());
        format.extend_from_slice(&RATE.to_le_bytes());
        let align = CHANNELS * BITS / 8;
        format.extend_from_slice(&(RATE * u32::from(align)).to_le_bytes());
        format.extend_from_slice(&align.to_le_bytes());
        format.extend_from_slice(&BITS.to_le_bytes());

        let mut body = b"WAVE".to_vec();
        riff_chunk(&mut body, b"fmt ", &format);
        if !info.is_empty() {
            let mut list = b"INFO".to_vec();
            for (id, value) in info {
                let mut terminated = value.as_bytes().to_vec();
                terminated.push(0);
                riff_chunk(&mut list, id, &terminated);
            }
            riff_chunk(&mut body, b"LIST", &list);
        }
        riff_chunk(
            &mut body,
            b"data",
            &vec![0; FRAMES as usize * usize::from(align)],
        );

        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(
            &u32::try_from(body.len())
                .expect("a small file")
                .to_le_bytes(),
        );
        file.extend_from_slice(&body);
        file
    }

    fn edited(field: TagField, value: &str) -> TagEdit {
        TagEdit {
            field,
            value: value.to_owned(),
        }
    }

    fn named_and_identified() -> Vec<TagEdit> {
        vec![
            edited(TagField::Title, "Echoes"),
            edited(TagField::Artist, "Pink Floyd"),
            edited(TagField::Album, "Meddle"),
            edited(TagField::AlbumArtist, "Pink Floyd"),
            edited(TagField::TrackNumber, "6"),
            edited(TagField::TrackTotal, "6"),
            edited(TagField::DiscNumber, "1"),
            edited(TagField::DiscTotal, "2"),
            edited(TagField::Date, "1971-10-30"),
            edited(TagField::Label, "Harvest"),
            edited(TagField::CatalogNumber, "SHVL 795"),
            edited(TagField::Barcode, "5099902893525"),
            edited(TagField::Isrc, "GBAYE7100195"),
            edited(
                TagField::MusicBrainzTrackId,
                "b1a9c0de-1111-4222-8333-444455556666",
            ),
            edited(
                TagField::MusicBrainzReleaseTrackId,
                "3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11",
            ),
            edited(
                TagField::MusicBrainzAlbumId,
                "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f",
            ),
            edited(
                TagField::MusicBrainzArtistId,
                "83d91898-7763-47d7-b03b-b92132375c47",
            ),
            edited(
                TagField::MusicBrainzAlbumArtistId,
                "83d91898-7763-47d7-b03b-b92132375c47",
            ),
            edited(
                TagField::MusicBrainzReleaseGroupId,
                "f5093c06-23e3-404f-aeaa-40f72885ee3a",
            ),
            edited(TagField::Genre, "Progressive Rock"),
            edited(TagField::Composer, "Roger Waters"),
            edited(TagField::Conductor, "Ron Geesin"),
            edited(TagField::Lyricist, "Roger Waters"),
            edited(TagField::Performer, "David Gilmour"),
            edited(TagField::Remixer, "James Guthrie"),
            edited(TagField::Engineer, "John Leckie"),
            edited(TagField::Producer, "Pink Floyd"),
            edited(TagField::Comment, "Side two, whole"),
            edited(TagField::BeatsPerMinute, "68"),
            edited(TagField::Compilation, "1"),
            edited(TagField::Grouping, "Meddle sessions"),
            edited(TagField::Copyright, "1971 Pink Floyd Music Ltd"),
            edited(TagField::ReplayGainTrackGain, "-6.50 dB"),
            edited(TagField::ReplayGainTrackPeak, "0.988547"),
            edited(TagField::ReplayGainAlbumGain, "-7.25 dB"),
            edited(TagField::ReplayGainAlbumPeak, "0.999969"),
        ]
    }

    fn assert_read_back(tags: &TagSet, edits: &[TagEdit], unheld: &[TagField]) {
        assert_eq!(
            edits.len(),
            TagField::ALL.len(),
            "a field was added to the vocabulary without a round trip to hold it"
        );
        for edit in edits {
            let wanted = match unheld.contains(&edit.field) {
                true => None,
                false => Some(edit.value.as_str()),
            };
            assert_eq!(
                edit.field.read(tags).as_deref(),
                wanted,
                "{} did not read back as it was written",
                edit.field
            );
        }
    }

    #[test]
    fn a_write_lands_through_a_staged_copy_and_a_failed_one_leaves_the_file_as_it_was() {
        use std::os::unix::fs::MetadataExt as _;

        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let before = fs::metadata(&path).expect("a file").ino();
        let tags = FileTags::default();

        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");
        assert_ne!(
            fs::metadata(&path).expect("a file").ino(),
            before,
            "the tags were spliced into the file where it stood"
        );

        let broken = folder.holding("broken.flac", b"fLaC but nothing after it");
        assert!(tags.write(&broken, just(&named_and_identified())).is_err());
        assert_eq!(
            fs::read(broken.as_path().expect("a local file")).expect("the file"),
            b"fLaC but nothing after it"
        );

        let left: Vec<_> = fs::read_dir(&folder.root)
            .expect("the folder")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().starts_with('.'))
            .collect();
        assert!(left.is_empty(), "a staged copy was left behind: {left:?}");
    }

    #[test]
    fn every_field_written_into_a_vorbis_comment_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let edits = named_and_identified();

        tags.write(&location, just(&edits)).expect("a written FLAC");
        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .tags,
            &edits,
            &[],
        );
    }

    fn retitled(path: &Path, location: &MediaLocation, title: &str) -> Result<bool> {
        let mut tagged = opened(path, location)?;
        let kind = tagged.file_type();
        if tagged.primary_tag().is_none() {
            tagged.insert_tag(Tag::new(tagged.primary_tag_type()));
        }
        let tag = tagged.primary_tag_mut().expect("a primary tag");
        tag.insert_text(ItemKey::TrackTitle, title.to_owned());
        landed_in_place(
            path,
            location,
            &Saving {
                kind,
                tag,
                counting: None,
                unkeyed: &[],
                others: &[],
                taken: Taken::of(path).expect("a writable file"),
                options: WriteOptions::default(),
            },
        )
    }

    #[test]
    fn a_file_nobody_may_write_is_refused_rather_than_replaced() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let mut permissions = fs::metadata(&path).expect("a file").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).expect("a file made read-only");
        if OpenOptions::new().write(true).open(&path).is_ok() {
            eprintln!("skipped: this user writes a read-only file regardless");
            return;
        }
        let before = fs::read(&path).expect("the file");

        let written = FileTags::default().write(&location, just(&named_and_identified()));

        assert!(
            matches!(&written, Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied),
            "a read-only file was written: {written:?}"
        );
        assert_eq!(fs::read(&path).expect("the file"), before);
    }

    #[test]
    fn a_file_in_a_folder_nothing_may_be_created_in_is_written_all_the_same() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let before = fs::metadata(&path).expect("a file").ino();
        fs::set_permissions(&folder.root, fs::Permissions::from_mode(0o555))
            .expect("a folder made unwritable");
        let probe = folder.root.join("probe");
        if File::create_new(&probe).is_ok() {
            let _ = fs::remove_file(&probe);
            let _ = fs::set_permissions(&folder.root, fs::Permissions::from_mode(0o755));
            eprintln!("skipped: this user creates files in an unwritable folder regardless");
            return;
        }
        let tags = FileTags::default();
        let edits = named_and_identified();

        let written = tags.write(&location, just(&edits));
        let _ = fs::set_permissions(&folder.root, fs::Permissions::from_mode(0o755));

        written.expect("a file in an unwritable folder written");
        assert_eq!(fs::metadata(&path).expect("a file").ino(), before);
        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .tags,
            &edits,
            &[],
        );
    }

    #[test]
    fn a_file_another_program_changed_meanwhile_no_longer_stands_as_taken() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let taken = Taken::of(&path).expect("a writable file");

        assert!(taken.still_stands(&path));

        let mut grown = fs::read(&path).expect("the file");
        grown.push(0);
        fs::write(&path, &grown).expect("a writable file");

        assert!(
            !taken.still_stands(&path),
            "an edit made meanwhile went unseen"
        );
    }

    #[test]
    fn an_edit_the_padding_holds_lands_in_the_file_itself_rather_than_a_copy() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let tags = FileTags::default();
        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");
        let standing = fs::metadata(&path).expect("the file");

        assert!(
            retitled(&path, &location, "Echoes (Live)").expect("an edit in place"),
            "an edit the padding holds was not landed in place"
        );

        let landed = fs::metadata(&path).expect("the file");
        assert_eq!(landed.ino(), standing.ino());
        assert_eq!(landed.len(), standing.len());
        assert_eq!(
            tags.read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .tags
                .title
                .as_deref(),
            Some("Echoes (Live)")
        );
        assert_eq!(
            fs::read_dir(&folder.root).expect("the folder").count(),
            1,
            "an edit in place left a copy beside the file"
        );
    }

    fn mp3() -> Vec<u8> {
        const FRAME_BYTES: usize = 417;
        const FRAMES_HELD: usize = 8;
        let mut file = Vec::with_capacity(FRAME_BYTES * FRAMES_HELD);
        for _ in 0..FRAMES_HELD {
            let mut frame = vec![0xFF, 0xFB, 0x90, 0x64];
            frame.resize(FRAME_BYTES, 0);
            file.extend_from_slice(&frame);
        }
        file
    }

    #[test]
    fn an_edit_a_leading_id3_tag_has_room_for_lands_in_the_file_itself() {
        let folder = Folder::new();
        let location = folder.holding("echoes.mp3", &mp3());
        let path = location.as_path().expect("a local file").to_path_buf();
        let tags = FileTags::default();
        tags.write(&location, just(&named_and_identified()))
            .expect("a written MP3");
        let standing = fs::metadata(&path).expect("the file");

        assert!(
            retitled(&path, &location, "Echoes (Live)").expect("an edit in place"),
            "an edit the tag's padding holds was not landed in place"
        );

        let landed = fs::metadata(&path).expect("the file");
        assert_eq!(landed.ino(), standing.ino());
        assert_eq!(landed.len(), standing.len());
        let read = tags
            .read(&location, Picturing::Whether)
            .expect("a readable MP3")
            .tags;
        assert_eq!(read.title.as_deref(), Some("Echoes (Live)"));
        assert_eq!(read.album.as_deref(), Some("Meddle"));
    }

    #[test]
    fn an_edit_that_moves_the_audio_is_left_to_a_whole_copy() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let before = fs::read(&path).expect("the file");

        assert!(
            !retitled(&path, &location, &"Echoes ".repeat(4096)).expect("a refusal"),
            "an edit that moved the audio was landed in place"
        );
        assert_eq!(fs::read(&path).expect("the file"), before);
    }

    #[test]
    fn every_field_written_into_an_id3_tag_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.aiff", &aiff());
        let tags = FileTags::default();
        let edits = named_and_identified();

        tags.write(&location, just(&edits)).expect("a written AIFF");
        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable AIFF")
                .tags,
            &edits,
            &[],
        );
    }

    #[test]
    fn writing_leaves_the_tags_it_was_not_asked_about_where_they_stood() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();

        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");
        tags.write(
            &location,
            just(&[edited(TagField::Title, "Echoes (2011 Remaster)")]),
        )
        .expect("a rewritten FLAC");

        let read = tags
            .read(&location, Picturing::Whether)
            .expect("a readable FLAC")
            .tags;
        assert_eq!(read.title.as_deref(), Some("Echoes (2011 Remaster)"));
        assert_eq!(read.album.as_deref(), Some("Meddle"));
        assert_eq!(
            read.musicbrainz_album_id.as_deref(),
            Some("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f")
        );
    }

    fn painted(shade: u8) -> CoverArt {
        const SIDE: u32 = 8;
        let drawn =
            image::RgbaImage::from_pixel(SIDE, SIDE, image::Rgba([shade, shade, shade, 255]));
        let mut bytes = Vec::new();
        drawn
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("a flat image is written as PNG");

        CoverArt {
            format: ImageFormat::Png,
            bytes,
        }
    }

    fn just(edits: &[TagEdit]) -> Writing<'_> {
        Writing {
            edits,
            taken: &[],
            picture: None,
            unpictured: false,
            popularity: None,
        }
    }

    fn only(picture: &CoverArt) -> Writing<'_> {
        Writing {
            edits: &[],
            taken: &[],
            picture: Some(picture),
            unpictured: false,
            popularity: None,
        }
    }

    fn rating(popularity: Popularity) -> Writing<'static> {
        Writing {
            edits: &[],
            taken: &[],
            picture: None,
            unpictured: false,
            popularity: Some(popularity),
        }
    }

    #[test]
    fn a_favourite_is_written_as_our_rating_and_taken_away_again() {
        let folder = Folder::new();
        let tags = FileTags::default();
        for (name, bytes) in [
            ("echoes.flac", flac()),
            ("echoes.aiff", aiff()),
            ("echoes.wav", wave()),
        ] {
            let location = folder.holding(name, &bytes);
            assert_eq!(
                tags.rated(&location).expect("a readable file"),
                Rated::Unrated { plays: Some(0) },
                "{name}"
            );

            let favoured = Popularity {
                favourite: true,
                plays: 12,
            };
            tags.write(&location, rating(favoured))
                .expect("a rated file");
            let held = tags.rated(&location).expect("a readable file");
            assert_eq!(held, Rated::Favourite { plays: Some(12) }, "{name}");
            assert!(!held.differs_from(favoured), "{name} did not read back");
            assert!(
                held.differs_from(Popularity {
                    favourite: false,
                    plays: 12
                }),
                "{name}"
            );

            tags.write(
                &location,
                rating(Popularity {
                    favourite: false,
                    plays: 12,
                }),
            )
            .expect("a file unrated");
            assert_eq!(
                tags.rated(&location).expect("a readable file"),
                Rated::Unrated { plays: Some(12) },
                "{name} kept the rating it was unfavoured of"
            );
        }
    }

    #[test]
    fn a_play_count_reaches_a_file_nobody_marked_a_favourite() {
        let folder = Folder::new();
        let tags = FileTags::default();
        for (name, bytes) in [
            ("echoes.flac", flac()),
            ("echoes.aiff", aiff()),
            ("echoes.wav", wave()),
        ] {
            let location = folder.holding(name, &bytes);
            let heard = Popularity {
                favourite: false,
                plays: 3,
            };
            assert!(
                tags.rated(&location)
                    .expect("a readable file")
                    .differs_from(heard),
                "{name} read as already counted"
            );

            tags.write(&location, rating(heard))
                .expect("a counted file");

            let held = tags.rated(&location).expect("a readable file");
            assert_eq!(held, Rated::Unrated { plays: Some(3) }, "{name}");
            assert!(!held.differs_from(heard), "{name} did not read back");

            tags.write(&location, rating(Popularity::default()))
                .expect("a count taken away");
            assert_eq!(
                tags.rated(&location).expect("a readable file"),
                Rated::Unrated { plays: Some(0) },
                "{name} kept a count of nothing"
            );
        }
    }

    #[test]
    fn a_count_another_player_wrote_is_read_as_the_files_own() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let mut comments = VorbisComments::default();
        comments.insert("FMPS_PLAYCOUNT".to_owned(), "9.0".to_owned());
        comments
            .save_to_path(&path, WriteOptions::default())
            .expect("a counted FLAC");

        assert_eq!(
            FileTags::default()
                .rated(&location)
                .expect("a readable FLAC"),
            Rated::Unrated { plays: Some(9) }
        );
    }

    #[test]
    fn a_write_keeps_the_comments_it_has_no_name_for() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let mut comments = VorbisComments::default();
        comments.insert("MOOD".to_owned(), "calm".to_owned());
        comments.insert("ACOUSTID_ID".to_owned(), "abc".to_owned());
        comments
            .save_to_path(&path, WriteOptions::default())
            .expect("a commented FLAC");

        FileTags::default()
            .write(&location, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a written FLAC");

        let kept = Counted::read(&path, FileType::Flac)
            .expect("a readable FLAC")
            .expect("comments");
        let Counted::Commented(kept) = kept else {
            panic!("a FLAC read as something other than comments");
        };
        assert_eq!(kept.get("MOOD"), Some("calm"));
        assert_eq!(kept.get("ACOUSTID_ID"), Some("abc"));
    }

    #[test]
    fn an_id3v2_3_tag_is_written_back_as_the_version_it_was() {
        const MAJOR_VERSION_AT: usize = 3;

        let folder = Folder::new();
        for (name, three) in [("three.mp3", true), ("four.mp3", false)] {
            let location = folder.holding(name, &mp3());
            let path = location.as_path().expect("a local file").to_path_buf();
            let mut frames = Id3v2Tag::new();
            frames.insert_user_text("MOOD".to_owned(), "calm".to_owned());
            frames
                .save_to_path(&path, WriteOptions::default().use_id3v23(three))
                .expect("a framed MP3");

            FileTags::default()
                .write(
                    &location,
                    just(&[
                        edited(TagField::Title, "Echoes"),
                        edited(TagField::Date, "1971-10-30"),
                    ]),
                )
                .expect("a written MP3");

            let written = fs::read(&path).expect("the file");
            assert!(written.starts_with(b"ID3"), "{name}");
            assert_eq!(
                written[MAJOR_VERSION_AT],
                if three { 3 } else { 4 },
                "{name} changed its tag's version"
            );
            assert_eq!(
                titled(&FileTags::default(), &location).as_deref(),
                Some("Echoes")
            );
        }
    }

    #[test]
    fn a_write_keeps_the_frames_it_has_no_name_for() {
        use lofty::id3::v2::PrivateFrame;

        let folder = Folder::new();
        let location = folder.holding("echoes.mp3", &mp3());
        let path = location.as_path().expect("a local file").to_path_buf();
        let mut frames = Id3v2Tag::new();
        frames.insert_user_text("MOOD".to_owned(), "calm".to_owned());
        frames.insert(Frame::Private(PrivateFrame::new("resonate", vec![1, 2, 3])));
        frames
            .save_to_path(&path, WriteOptions::default())
            .expect("a framed MP3");

        FileTags::default()
            .write(&location, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a written MP3");
        FileTags::default()
            .write(
                &location,
                rating(Popularity {
                    favourite: true,
                    plays: 3,
                }),
            )
            .expect("a counted MP3");

        let Some(Counted::Framed(kept)) =
            Counted::read(&path, FileType::Mpeg).expect("a readable MP3")
        else {
            panic!("an MP3 read with no ID3v2 tag");
        };
        assert_eq!(kept.get_user_text("MOOD"), Some("calm"));
        assert!(
            kept.get(&FrameId::Valid("PRIV".into())).is_some(),
            "a private frame was lost"
        );
    }

    #[test]
    fn a_performer_written_into_an_id3_tag_keeps_the_instrument_it_was_credited_with() {
        let folder = Folder::new();
        let location = folder.holding("echoes.mp3", &mp3());
        let path = location.as_path().expect("a local file").to_path_buf();
        let mut frames = Id3v2Tag::new();
        frames.insert(Frame::KeyValue(KeyValueFrame::new(
            FrameId::Valid(MUSICIAN_CREDITS.into()),
            TextEncoding::UTF8,
            vec![
                (Cow::from("guitar"), Cow::from("David Gilmour")),
                (Cow::from("bass"), Cow::from("Roger Waters")),
            ],
        )));
        frames
            .save_to_path(&path, WriteOptions::default())
            .expect("a credited MP3");
        let tags = FileTags::default();

        tags.write(
            &location,
            just(&[edited(TagField::Performer, "David Gilmour; Nick Mason")]),
        )
        .expect("a written MP3");

        let Some(Counted::Framed(kept)) =
            Counted::read(&path, FileType::Mpeg).expect("a readable MP3")
        else {
            panic!("an MP3 read with no ID3v2 tag");
        };
        let Some(Frame::KeyValue(credits)) = kept.get(&FrameId::Valid(MUSICIAN_CREDITS.into()))
        else {
            panic!("no musician credits were written");
        };
        assert_eq!(
            credits.key_value_pairs.as_ref(),
            [
                (Cow::from("guitar"), Cow::from("David Gilmour")),
                (Cow::from(""), Cow::from("Nick Mason")),
            ]
        );
        assert_eq!(
            tags.read(&location, Picturing::Whether)
                .expect("a readable MP3")
                .tags
                .credits
                .performer
                .as_deref(),
            Some("David Gilmour; Nick Mason")
        );

        tags.write(
            &location,
            Writing {
                taken: &[TagField::Performer],
                ..just(&[])
            },
        )
        .expect("a cleared MP3");
        assert_eq!(
            tags.read(&location, Picturing::Whether)
                .expect("a readable MP3")
                .tags
                .credits
                .performer,
            None
        );
    }

    #[test]
    fn a_rating_another_player_wrote_is_left_where_it_stands() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let mut tagged = lofty::read_from_path(&path).expect("a readable FLAC");
        if tagged.primary_tag().is_none() {
            let kind = tagged.primary_tag_type();
            tagged.insert_tag(Tag::new(kind));
        }
        let tag = tagged.primary_tag_mut().expect("a primary tag");
        tag.push(TagItem::new(
            ItemKey::Popularimeter,
            ItemValue::Text("MusicBee|3|0".to_owned()),
        ));
        tag.save_to_path(&path, WriteOptions::default())
            .expect("a rated FLAC");

        let tags = FileTags::default();
        tags.write(
            &location,
            rating(Popularity {
                favourite: true,
                plays: 1,
            }),
        )
        .expect("a rated FLAC");

        let tagged = lofty::read_from_path(&path).expect("a readable FLAC");
        let ratings: Vec<String> = tagged
            .primary_tag()
            .expect("a primary tag")
            .get_strings(ItemKey::Popularimeter)
            .map(str::to_owned)
            .collect();
        assert!(
            ratings
                .iter()
                .any(|rating| rating.starts_with("MusicBee|3")),
            "another player's rating was taken away: {ratings:?}"
        );
        assert!(
            ratings
                .iter()
                .any(|rating| rating.starts_with("resonate|5")),
            "the favourite was not written: {ratings:?}"
        );
    }

    #[test]
    fn a_picture_written_into_a_vorbis_comment_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let cover = painted(200);

        tags.write(&location, only(&cover)).expect("a written FLAC");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            Some(cover)
        );
    }

    #[test]
    fn a_picture_written_into_an_id3_tag_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.aiff", &aiff());
        let tags = FileTags::default();
        let cover = painted(137);

        tags.write(&location, only(&cover)).expect("a written AIFF");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable AIFF")
                .picture
                .into_copied(),
            Some(cover)
        );
    }

    #[test]
    fn a_picture_takes_the_place_of_the_front_cover_rather_than_standing_beside_it() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let first = painted(40);
        let second = painted(210);

        tags.write(&location, only(&first)).expect("a written FLAC");
        tags.write(&location, only(&second))
            .expect("a rewritten FLAC");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            Some(second)
        );
    }

    #[test]
    fn a_file_with_no_picture_in_it_answers_with_none() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();

        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            None
        );
        assert_eq!(
            tags.read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .picture,
            Pictured::Bare
        );
    }

    #[test]
    fn one_read_answers_the_fields_and_whether_a_picture_is_there_without_copying_it() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let picture = painted(0x40);
        let edits = named_and_identified();

        tags.write(
            &location,
            Writing {
                edits: &edits,
                taken: &[],
                picture: Some(&picture),
                unpictured: false,
                popularity: None,
            },
        )
        .expect("a written FLAC");

        let weighed = tags
            .read(&location, Picturing::Whether)
            .expect("a readable FLAC");
        assert_read_back(&weighed.tags, &edits, &[]);
        assert_eq!(weighed.picture, Pictured::Carried);

        let copied = tags
            .read(&location, Picturing::Copied)
            .expect("a readable FLAC");
        assert_read_back(&copied.tags, &edits, &[]);
        assert_eq!(copied.picture, Pictured::Copied(picture));
    }

    #[test]
    fn a_picture_rides_beside_the_fields_in_one_write() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let edits = named_and_identified();
        let cover = painted(90);

        tags.write(
            &location,
            Writing {
                edits: &edits,
                taken: &[],
                picture: Some(&cover),
                unpictured: false,
                popularity: None,
            },
        )
        .expect("a written FLAC");

        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .tags,
            &edits,
            &[],
        );
        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            Some(cover)
        );
    }

    #[test]
    fn every_field_and_the_picture_written_into_a_wave_file_read_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.wav", &wave());
        let tags = FileTags::default();
        let edits = named_and_identified();
        let cover = painted(77);

        assert!(tags.writes(&location), "a WAV was refused for writing");
        tags.write(
            &location,
            Writing {
                edits: &edits,
                taken: &[],
                picture: Some(&cover),
                unpictured: false,
                popularity: None,
            },
        )
        .expect("a written WAV");

        let read = tags
            .read(&location, Picturing::Copied)
            .expect("a readable WAV");
        assert_read_back(&read.tags, &edits, &[]);
        assert_eq!(read.picture, Pictured::Copied(cover));
    }

    fn titled(tags: &FileTags, location: &MediaLocation) -> Option<String> {
        tags.read(location, Picturing::Whether)
            .expect("a readable file")
            .tags
            .title
    }

    #[test]
    fn a_track_reached_through_a_link_is_written_where_the_link_points_and_stays_a_link() {
        let folder = Folder::new();
        let target = folder.holding("echoes.flac", &flac());
        let link = folder.root.join("linked.flac");
        unix_fs::symlink(target.as_path().expect("a local file"), &link).expect("a link");
        let linked = MediaLocation::local(&link);
        let tags = FileTags::default();

        tags.write(&linked, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a file written through its link");

        assert!(
            fs::symlink_metadata(&link).is_ok_and(|held| held.is_symlink()),
            "the link became a file of its own"
        );
        assert_eq!(titled(&tags, &target).as_deref(), Some("Echoes"));
    }

    #[test]
    fn a_track_with_two_names_keeps_both_and_both_read_the_write() {
        let folder = Folder::new();
        let first = folder.holding("echoes.flac", &flac());
        let second = folder.root.join("also.flac");
        fs::hard_link(first.as_path().expect("a local file"), &second).expect("a hard link");
        let tags = FileTags::default();

        tags.write(&first, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a hard-linked file written");

        let also = MediaLocation::local(&second);
        assert_eq!(titled(&tags, &also).as_deref(), Some("Echoes"));
        assert_eq!(
            fs::metadata(&second).expect("the second name").nlink(),
            2,
            "the write split the two names apart"
        );
        assert!(
            fs::read_dir(&folder.root)
                .expect("the folder")
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().starts_with('.')),
            "a staged copy was left behind"
        );
    }

    #[test]
    fn a_tracks_extended_attributes_survive_a_write() {
        const NAME: &str = "user.resonate.kept";

        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        if rustix::fs::setxattr(&path, NAME, b"yes", XattrFlags::empty()).is_err() {
            eprintln!("skipped: the temporary folder holds no extended attributes");
            return;
        }
        let tags = FileTags::default();

        tags.write(&location, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a written file");

        let mut held = [0_u8; 8];
        let read =
            rustix::fs::getxattr(&path, NAME, &mut held[..]).expect("the attribute is there");
        assert_eq!(held.get(..read), Some(b"yes".as_slice()));
        assert_eq!(titled(&tags, &location).as_deref(), Some("Echoes"));
    }

    #[test]
    fn a_field_cleared_from_a_wave_file_is_gone_from_its_info_list_too() {
        let folder = Folder::new();
        let listed = wave_with(&[(b"INAM", "Untitled"), (b"IART", "Pink Floyd")]);
        let location = folder.holding("echoes.wav", &listed);
        let tags = FileTags::default();

        tags.write(&location, just(&[edited(TagField::Album, "Meddle")]))
            .expect("a written WAV");
        tags.write(
            &location,
            Writing {
                edits: &[],
                taken: &[TagField::Title],
                picture: None,
                unpictured: false,
                popularity: None,
            },
        )
        .expect("a WAV with a field cleared");

        let read = tags
            .read(&location, Picturing::Whether)
            .expect("a readable WAV")
            .tags;
        assert_eq!(read.title, None, "the INFO list's title came back");
        assert_eq!(read.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(read.album.as_deref(), Some("Meddle"));
    }

    #[test]
    fn a_title_written_into_a_wave_file_outranks_the_info_list_beside_it() {
        let folder = Folder::new();
        let listed = wave_with(&[(b"INAM", "Untitled"), (b"IART", "Pink Floyd")]);
        let location = folder.holding("echoes.wav", &listed);
        let tags = FileTags::default();

        tags.write(&location, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a written WAV");

        let read = tags
            .read(&location, Picturing::Whether)
            .expect("a readable WAV")
            .tags;
        assert_eq!(read.title.as_deref(), Some("Echoes"));
        assert_eq!(read.artist.as_deref(), Some("Pink Floyd"));
    }

    #[test]
    fn a_format_whose_tags_this_build_would_not_read_back_is_refused() {
        let folder = Folder::new();
        let location = folder.holding("echoes.mpc", b"MPCK");
        let tags = FileTags::default();

        assert!(
            !tags.writes(&location),
            "a Musepack was offered for writing"
        );
        assert!(
            matches!(
                tags.write(&location, just(&[edited(TagField::Title, "Echoes")])),
                Err(Error::Unwritable { .. })
            ),
            "a Musepack was written rather than refused"
        );
    }

    #[test]
    fn a_location_with_no_path_behind_it_is_refused() {
        let location = MediaLocation::new(
            resonate_core::SourceId::new("elsewhere").expect("a nameable source"),
            "echoes.flac",
        );
        let tags = FileTags::default();

        assert!(!tags.writes(&location));
        assert!(matches!(
            tags.write(&location, just(&[edited(TagField::Title, "Echoes")])),
            Err(Error::LocatorNotUsable { .. })
        ));
    }

    #[test]
    fn a_file_whose_bytes_disagree_with_its_extension_is_refused() {
        let folder = Folder::new();
        let mut musepack = b"MPCK".to_vec();
        musepack.resize(64, 0);
        let location = folder.holding("echoes.flac", &musepack);
        let tags = FileTags::default();

        assert!(
            tags.writes(&location),
            "the extension alone reads as a FLAC"
        );
        assert!(
            matches!(
                tags.write(&location, just(&[edited(TagField::Title, "Echoes")])),
                Err(Error::Unwritable { .. })
            ),
            "a Musepack named .flac was written rather than refused"
        );
    }

    #[test]
    fn a_field_names_itself_once() {
        let mut named: Vec<&str> = TagField::ALL.iter().map(|field| field.as_str()).collect();
        named.sort_unstable();
        let held = named.len();
        named.dedup();
        assert_eq!(named.len(), held, "two fields answer to one name");
    }
}
