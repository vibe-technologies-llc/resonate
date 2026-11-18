use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{
    MediaLocation, SampleRate, TrackId,
    titles::{self, Rating},
};
use resonate_engine::QueueItem;
use resonate_library::{Quality, SongHeard, TheSong, VersionSought};
use resonate_providers::{Named, PlaysInPlace, Song};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::{Catalog, read},
    collections::wanted_key,
    documents::location,
    downloads::{Job, Searched, Tried},
    features::SearchSong,
    search::codec_named,
    served::ServedSong,
    words,
};

pub(crate) const HELD_WEIGHED_AT_MOST: u32 = 64;

pub(crate) const NAMES_APART: &str = ", ";

const BITS_WHERE_NONE_ARE_KNOWN: u8 = 16;

const KEPT_AS_WHAT_IT_IS_NAMED: &str = "flac";

const EXTENSIONS_OF_MEDIA_TYPES: [(&str, &str); 9] = [
    ("audio/flac", "flac"),
    ("audio/x-flac", "flac"),
    ("audio/mpeg", "mp3"),
    ("audio/mp4", "m4a"),
    ("audio/x-m4a", "m4a"),
    ("audio/ogg", "ogg"),
    ("audio/opus", "opus"),
    ("audio/wav", "wav"),
    ("audio/x-wav", "wav"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum VersionSearch {
    Better,
    Clean,
    Explicit,
}

impl VersionSearch {
    const fn of(sought: VersionSought) -> Self {
        match sought {
            VersionSought::Better => Self::Better,
            VersionSought::Rated(Rating::Explicit) => Self::Explicit,
            VersionSought::Rated(Rating::Clean | Rating::Unmarked) => Self::Clean,
        }
    }

    pub(crate) const fn sought(self) -> VersionSought {
        match self {
            Self::Better => VersionSought::Better,
            Self::Clean => VersionSought::Rated(Rating::Clean),
            Self::Explicit => VersionSought::Rated(Rating::Explicit),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Better => words::versions::find_a_better_version(),
            Self::Clean => words::versions::look_for_clean(),
            Self::Explicit => words::versions::look_for_explicit(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct VersionSearchOffered {
    pub search: VersionSearch,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SongVersion {
    Held { id: u64 },
    Served { uri: String },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FoundVersion {
    pub version: SongVersion,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct FoundVersions {
    pub versions: Vec<FoundVersion>,
    pub said: Option<String>,
    pub trouble: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct VersionChosen {
    pub said: String,
    pub download: Option<SearchSong>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SongVariant {
    pub id: u64,
    pub label: String,
    pub original: bool,
    pub default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nothing {
    AlreadyLossless,
    NowhereToAsk,
    NoneOn,
}

fn nothing_said(search: VersionSearch, why: Nothing, title: &str, services: &[String]) -> String {
    let services = services.join(NAMES_APART);
    match (search, why) {
        (VersionSearch::Better, Nothing::AlreadyLossless) => {
            words::versions::already_lossless(title)
        }
        (VersionSearch::Better, Nothing::NowhereToAsk) => {
            words::versions::none_held_and_nowhere_to_ask(title)
        }
        (VersionSearch::Better, Nothing::NoneOn) => words::versions::none_found(title, &services),
        (VersionSearch::Explicit, Nothing::NoneOn) => {
            words::versions::no_explicit_found(title, &services)
        }
        (VersionSearch::Explicit, _) => words::versions::no_explicit_and_nowhere_to_ask(title),
        (VersionSearch::Clean, Nothing::NoneOn) => {
            words::versions::no_clean_found(title, &services)
        }
        (VersionSearch::Clean, _) => words::versions::no_clean_and_nowhere_to_ask(title),
    }
}

fn rating_of(track: &Track) -> Rating {
    Rating::of(Rating::flagged(None), &track.title, None)
}

pub(crate) fn artist_of(track: &Track) -> Option<&str> {
    Some(track.artist.as_str()).filter(|artist| !artist.trim().is_empty())
}

pub(crate) fn heard_of(track: &Track) -> SongHeard<'_> {
    SongHeard {
        title: &track.title,
        artist: artist_of(track),
        length: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
        lossless: codec_named(&track.codec).is_lossless(),
        rating: rating_of(track),
    }
}

pub(crate) fn quality_of(track: &Track) -> Quality {
    let bits = track.bits.map_or(BITS_WHERE_NONE_ARE_KNOWN, |bits| {
        u8::try_from(bits).unwrap_or(u8::MAX)
    });
    Quality::new(codec_named(&track.codec).is_lossless(), bits, track.rate)
}

fn heard_served(song: &Song) -> SongHeard<'_> {
    SongHeard {
        title: &song.title,
        artist: song.artist.as_deref(),
        length: song.length,
        lossless: song
            .suffix
            .as_deref()
            .and_then(resonate_library::codec_named)
            .is_some_and(|codec| codec.is_lossless()),
        rating: song.rating,
    }
}

pub(crate) fn better_held(track: &Track, found: impl IntoIterator<Item = Track>) -> Vec<Track> {
    let song = TheSong::new(heard_of(track));
    let held = quality_of(track);
    let mut better: Vec<Track> = Vec::new();
    for copy in found {
        let fits = copy.id != track.id
            && !better.iter().any(|kept| kept.id == copy.id)
            && song.names(heard_of(&copy))
            && quality_of(&copy).is_above(held);
        if fits {
            better.push(copy);
        }
    }
    better.sort_by_key(|copy| quality_of(copy).best_first());
    better
}

fn held_versions(track: &Track, search: VersionSearch, found: Vec<Track>) -> Vec<Track> {
    match search.sought() {
        VersionSought::Better => better_held(track, found),
        VersionSought::Rated(wanted) => {
            let song = TheSong::new(heard_of(track));
            found
                .into_iter()
                .filter(|copy| copy.id != track.id && song.held_as(wanted, heard_of(copy)))
                .collect()
        }
    }
}

fn quality_said(track: &Track) -> String {
    let bits = track.bits.unwrap_or(u32::from(BITS_WHERE_NONE_ARE_KNOWN));
    let rate = SampleRate::new(track.rate)
        .map_or_else(|_| track.rate.to_string(), |rate| rate.to_string());
    words::versions::quality(bits, rate)
}

fn album_of(track: &Track) -> Option<&str> {
    Some(track.album.as_str()).filter(|album| !album.trim().is_empty())
}

pub(crate) fn held_label(search: VersionSearch, copy: &Track) -> String {
    match (search, album_of(copy)) {
        (VersionSearch::Better, _) => {
            words::versions::play_held(copy.codec.as_str(), quality_said(copy))
        }
        (_, Some(album)) => words::versions::play_your_copy_from(album),
        (_, None) => words::versions::play_your_copy(copy.codec.as_str()),
    }
}

fn served_label(search: VersionSearch, service: &str, song: &Song) -> String {
    let album = song.album.as_deref().filter(|album| !album.is_empty());
    match search {
        VersionSearch::Better => {
            let codec = song
                .suffix
                .as_deref()
                .and_then(resonate_library::codec_named)
                .map_or(words::versions::lossless(), |codec| codec.as_str());
            match album {
                Some(album) => words::versions::download_served_on_album(album, codec, service),
                None => words::versions::download_served(codec, service),
            }
        }
        VersionSearch::Clean | VersionSearch::Explicit => match album {
            Some(album) => words::versions::download_it_from_album(album, service),
            None => words::versions::download_it_from(service),
        },
    }
}

fn variant_label(original: &Track, variant: &Track) -> String {
    let codec = variant.codec.as_str();
    let quality = quality_said(variant);
    if variant.id == original.id {
        return words::versions::variant_original(codec, quality);
    }
    let named = match album_of(variant) {
        Some(album) if album_of(original) != Some(album) => {
            words::versions::variant_on_album(album, codec, quality)
        }
        _ => words::versions::variant(codec, quality),
    };
    match rating_of(variant) {
        rating if rating == rating_of(original) => named,
        Rating::Clean => words::versions::rated_clean(named),
        Rating::Explicit => words::versions::rated_explicit(named),
        Rating::Unmarked => named,
    }
}

fn served_song_of(location: &MediaLocation, song: &Song) -> ServedSong {
    ServedSong {
        uri: location.to_uri(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        track: song.track,
        duration_ms: song
            .length
            .map(|length| u64::try_from(length.as_millis()).unwrap_or(u64::MAX)),
        format: song.suffix.clone(),
        cover: None,
        favourite: None,
    }
}

#[derive(Default)]
pub(crate) struct Asked {
    pub(crate) found: Vec<FoundVersion>,
    heard: Vec<ServedSong>,
    pub(crate) services: Vec<String>,
    pub(crate) refused: Vec<String>,
}

pub(crate) fn asked_of(
    services: &[Arc<dyn PlaysInPlace>],
    words: &str,
    song: &TheSong,
    search: VersionSearch,
) -> Asked {
    let mut asked = Asked::default();
    for plays in services.iter().filter(|plays| plays.plays_songs()) {
        let service = plays.title().to_owned();
        match plays.search(words) {
            Ok(found) => {
                for found in found
                    .songs
                    .into_iter()
                    .filter(|found| song.served_as(search.sought(), heard_served(found)))
                {
                    let location = plays.location_of(&Named::Song(found.id.clone()));
                    asked.heard.push(served_song_of(&location, &found));
                    asked.found.push(FoundVersion {
                        label: served_label(search, &service, &found),
                        version: SongVersion::Served {
                            uri: location.to_uri(),
                        },
                    });
                }
            }
            Err(error) => {
                tracing::warn!(%error, service = %service, "a service could not be asked for another version");
                asked.refused.push(service.clone());
            }
        }
        asked.services.push(service);
    }
    asked
}

const THE_VERSION_CHOSEN: &str = "SELECT d.id, d.uri FROM versions_chosen v
     JOIN documents d ON d.id = v.held AND d.missing = 0
     WHERE v.track = ?1";

const CHOSEN: &str = "INSERT INTO versions_chosen (track, held, served, chosen)
     VALUES (?1, ?2, NULL, ?3)
     ON CONFLICT (track) DO UPDATE
        SET held = excluded.held, served = NULL, chosen = excluded.chosen";

const HELD_BESIDE: &str = "INSERT INTO versions_held (track, held, kept) VALUES (?1, ?2, ?3)
     ON CONFLICT (track, held) DO NOTHING";

const FORGOTTEN: &str = "DELETE FROM versions_chosen WHERE track = ?1";

const KEPT: &str = "SELECT held FROM versions_chosen WHERE track = ?1 AND held IS NOT NULL";

const THE_SONG_A_VARIANT_IS_OF: &str = "SELECT v.track FROM versions_held v
     JOIN documents d ON d.id = v.track AND d.missing = 0
     WHERE v.held = ?1 ORDER BY v.kept, v.track LIMIT 1";

const THE_VARIANTS_OF_A_SONG: &str = "SELECT d.* FROM versions_held v
     JOIN documents d ON d.id = v.held AND d.missing = 0
     WHERE v.track = ?1 ORDER BY v.kept, v.held";

const SONGS_HOLDING_VARIANTS: &str = "SELECT v.track FROM versions_held v
     JOIN documents d ON d.id = v.held WHERE d.missing = 0
     UNION SELECT v.held FROM versions_held v
     JOIN documents d ON d.id = v.held WHERE d.missing = 0";

const THE_WANT_OF_A_VERSION: &str = "SELECT id FROM wanted WHERE identity = ?1";

const THE_WANT_ASKED_AGAIN: &str =
    "UPDATE wanted SET tried = NULL, misses = 0, offered_at = NULL WHERE id = ?1";

const A_VERSION_WANTED: &str =
    "INSERT INTO versions_wanted (want, track, asked) VALUES (?1, ?2, ?3)
     ON CONFLICT (want, track) DO UPDATE SET asked = excluded.asked";

const A_WANT_FOR_A_VERSION: &str = "SELECT EXISTS(SELECT 1 FROM versions_wanted WHERE want = ?1)";

const A_VERSION_WAITS: &str = "SELECT EXISTS(SELECT 1 FROM versions_wanted WHERE track = ?1)";

const THE_SONGS_A_WANT_IS_A_VERSION_OF: &str = "SELECT track FROM versions_wanted WHERE want = ?1";

const THE_VERSIONS_WANTED_GO: &str = "DELETE FROM versions_wanted WHERE want = ?1";

const THE_WANT_GOES: &str = "DELETE FROM wanted WHERE id = ?1";

fn seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

pub(crate) fn wanted_as_a_version(connection: &Connection, want: u64) -> Result<bool> {
    connection
        .query_row(A_WANT_FOR_A_VERSION, [want as i64], |row| row.get(0))
        .map_err(read_error)
}

pub(crate) fn kept_as_a_variant(
    connection: &Connection,
    song: u64,
    variant: u64,
    at: i64,
) -> rusqlite::Result<()> {
    if song == variant {
        connection.execute(FORGOTTEN, params![song as i64])?;
        return Ok(());
    }
    connection.execute(HELD_BESIDE, params![song as i64, variant as i64, at])?;
    connection.execute(CHOSEN, params![song as i64, variant as i64, at])?;
    Ok(())
}

fn extension_of(known: Option<&str>, media_type: Option<&str>) -> String {
    let named = known
        .and_then(|format| resonate_providers::Extension::new(format).ok())
        .map(|extension| extension.as_str().to_owned());
    let typed = || {
        let media_type = media_type?.split(';').next()?.trim().to_ascii_lowercase();
        EXTENSIONS_OF_MEDIA_TYPES
            .iter()
            .find(|(typed, _)| *typed == media_type)
            .map(|(_, extension)| (*extension).to_owned())
    };
    named
        .or_else(typed)
        .unwrap_or_else(|| KEPT_AS_WHAT_IT_IS_NAMED.to_owned())
}

#[uniffi::export]
impl MobilePlayer {
    pub fn version_searches(&self, id: u64) -> Result<Vec<VersionSearchOffered>> {
        let track = self.catalog.track(id)?;
        Ok(VersionSought::offered_for(rating_of(&track))
            .into_iter()
            .map(VersionSearch::of)
            .map(|search| VersionSearchOffered {
                search,
                label: search.label().to_owned(),
            })
            .collect())
    }

    pub fn find_versions(&self, id: u64, search: VersionSearch) -> Result<FoundVersions> {
        let track = self.catalog.track(id)?;
        let title = titles::bare(&track.title).to_owned();
        let words = TheSong::words(&track.title, artist_of(&track));
        let named_alike = self.services.searching.ranked_tracks(
            &self.catalog,
            &words,
            0,
            HELD_WEIGHED_AT_MOST,
        )?;
        let held: Vec<FoundVersion> = held_versions(&track, search, named_alike)
            .into_iter()
            .map(|copy| FoundVersion {
                label: held_label(search, &copy),
                version: SongVersion::Held { id: copy.id },
            })
            .collect();
        let song = TheSong::new(heard_of(&track));
        let services = self.services_to_ask();

        let nothing_to_ask = match search {
            VersionSearch::Better if song.is_lossless() => Some(Nothing::AlreadyLossless),
            _ if services.iter().all(|plays| !plays.plays_songs()) => Some(Nothing::NowhereToAsk),
            _ => None,
        };
        if let Some(why) = nothing_to_ask {
            let said = match held.is_empty() {
                true => nothing_said(search, why, &title, &[]),
                false => words::versions::found(held.len(), &title),
            };
            return Ok(FoundVersions {
                versions: held,
                said: Some(said),
                trouble: None,
            });
        }

        let asked = self.asked_for_versions(&services, &words, &song, search);
        let trouble = (!asked.refused.is_empty())
            .then(|| words::versions::could_not_ask(asked.refused.join(NAMES_APART)));
        let mut versions = held;
        versions.extend(asked.found);
        let said = match versions.len() {
            0 => {
                let answered: Vec<String> = asked
                    .services
                    .into_iter()
                    .filter(|service| !asked.refused.contains(service))
                    .collect();
                (!answered.is_empty())
                    .then(|| nothing_said(search, Nothing::NoneOn, &title, &answered))
            }
            found => Some(words::versions::found(found, &title)),
        };
        Ok(FoundVersions {
            versions,
            said,
            trouble,
        })
    }

    pub fn choose_version(&self, id: u64, version: SongVersion) -> Result<VersionChosen> {
        let track = self.catalog.track(id)?;
        let title = titles::bare(&track.title);
        match version {
            SongVersion::Held { id: copy } => {
                self.catalog.track(copy)?;
                self.catalog.keep_a_variant(id, copy)?;
                Ok(VersionChosen {
                    said: match copy == id {
                        true => words::versions::own_copy_again(title),
                        false => words::versions::chosen(title),
                    },
                    download: None,
                })
            }
            SongVersion::Served { uri } => {
                let wanted = self.served_version_wanted(&track, &uri)?;
                self.catalog.want_a_version(id, &wanted)?;
                Ok(VersionChosen {
                    said: words::versions::downloading(title),
                    download: Some(wanted),
                })
            }
        }
    }

    pub fn version_chosen(&self, id: u64) -> Result<Option<SongVersion>> {
        let kept = self
            .catalog
            .0
            .lock()
            .query_row(KEPT, params![id as i64], |row| row.get::<_, i64>(0))
            .optional()
            .map_err(read_error)?;
        Ok(kept
            .and_then(|copy| u64::try_from(copy).ok())
            .map(|id| SongVersion::Held { id }))
    }

    pub fn forget_version_chosen(&self, id: u64) -> Result<String> {
        let track = self.catalog.track(id)?;
        self.catalog
            .0
            .lock()
            .execute(FORGOTTEN, params![id as i64])
            .map_err(write_error)?;
        Ok(words::versions::own_copy_again(titles::bare(&track.title)))
    }

    pub fn variants_of(&self, id: u64) -> Result<Vec<SongVariant>> {
        let original = self.catalog.the_song_a_variant_is_of(id)?.unwrap_or(id);
        let song = self.catalog.track(original)?;
        let variants = self.catalog.variants_held_by(original)?;
        if variants.is_empty() {
            return Ok(Vec::new());
        }

        let chosen = match self.version_chosen(original)? {
            Some(SongVersion::Held { id }) if variants.iter().any(|held| held.id == id) => id,
            Some(_) | None => original,
        };
        Ok(std::iter::once(&song)
            .chain(&variants)
            .map(|variant| SongVariant {
                id: variant.id,
                label: variant_label(&song, variant),
                original: variant.id == original,
                default: variant.id == chosen,
            })
            .collect())
    }

    pub fn songs_with_variants(&self) -> Result<Vec<u64>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare_cached(SONGS_HOLDING_VARIANTS)
            .map_err(read_error)?;
        let ids = query
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok(ids
            .into_iter()
            .filter_map(|id| u64::try_from(id).ok())
            .collect())
    }

    pub fn choose_variant(&self, song: u64, variant: u64) -> Result<String> {
        let original = self.catalog.the_song_a_variant_is_of(song)?.unwrap_or(song);
        let track = self.catalog.track(original)?;
        let title = titles::bare(&track.title);
        if variant == original {
            self.catalog.keep_a_variant(original, original)?;
            return Ok(words::versions::own_copy_again(title));
        }
        let held = self.catalog.variants_held_by(original)?;
        if !held.iter().any(|kept| kept.id == variant) {
            return Err(Error::NotAVariant);
        }
        self.catalog.keep_a_variant(original, variant)?;
        Ok(words::versions::chosen(title))
    }
}

impl MobilePlayer {
    pub(crate) fn services_to_ask(&self) -> Vec<Arc<dyn PlaysInPlace>> {
        match self.require_online() {
            Ok(()) => self.served.playing_services(),
            Err(_) => Vec::new(),
        }
    }

    pub(crate) fn asked_for_versions(
        &self,
        services: &[Arc<dyn PlaysInPlace>],
        words: &str,
        song: &TheSong,
        search: VersionSearch,
    ) -> Asked {
        let asked = asked_of(services, words, song, search);
        self.served.kept(&asked.heard);
        asked
    }

    pub(crate) fn served_version_wanted(&self, track: &Track, uri: &str) -> Result<SearchSong> {
        let location = MediaLocation::from_uri(uri).ok_or(Error::InvalidUri)?;
        if !self.served.serves(&location) {
            return Err(Error::NoServedServer);
        }
        let served = self.served.song_at(&location);
        Ok(SearchSong {
            title: Some(served.title)
                .filter(|named| named != uri && !named.trim().is_empty())
                .unwrap_or_else(|| track.title.clone()),
            artist: served.artist.unwrap_or_else(|| track.artist.clone()),
            album: served.album.unwrap_or_else(|| track.album.clone()),
            duration_ms: served
                .duration_ms
                .or((track.duration_ms > 0).then_some(track.duration_ms)),
            served: Some(uri.to_owned()),
            ..SearchSong::default()
        })
    }

    pub(crate) fn found_served(&self, job: &Arc<Job>, uri: &str) -> Searched {
        let offer = MediaLocation::from_uri(uri)
            .ok_or(Error::InvalidUri)
            .and_then(|location| {
                let (streamed, provider) = self.served.streamed_to_keep(&location)?;
                let known = self.served.song_at(&location).format;
                let extension = extension_of(known.as_deref(), streamed.media_type.as_deref());
                self.downloads
                    .served(job, streamed, extension, provider, uri.to_owned())
            });
        let tried = match &offer {
            Ok(_) => Tried::Offered,
            Err(_) => Tried::Unheard,
        };
        Searched {
            offer: job.settled(offer),
            tried,
        }
    }

    fn chosen_for(&self, id: TrackId) -> Result<Option<(u64, String)>> {
        let row = self
            .catalog
            .0
            .lock()
            .query_row(THE_VERSION_CHOSEN, params![id.get() as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .optional()
            .map_err(read_error)?;
        Ok(row.and_then(|(copy, uri)| u64::try_from(copy).ok().map(|copy| (copy, uri))))
    }

    pub(crate) fn as_chosen(&self, items: Vec<QueueItem>) -> Result<Vec<QueueItem>> {
        let mut chosen = Vec::with_capacity(items.len());
        for item in &items {
            chosen.push(self.chosen_for(item.id)?);
        }
        if chosen.iter().all(Option::is_none) {
            return Ok(items);
        }

        items
            .into_iter()
            .zip(chosen)
            .map(|(item, chosen)| match chosen {
                Some((id, uri)) => Ok(QueueItem::whole(TrackId::new(id)?, location(&uri)?)),
                None => Ok(item),
            })
            .collect()
    }
}

impl Catalog {
    pub(crate) fn keep_a_variant(&self, song: u64, variant: u64) -> Result<()> {
        let connection = self.0.lock();
        kept_as_a_variant(&connection, song, variant, seconds_now()).map_err(write_error)
    }

    pub(crate) fn a_version_waits_for(&self, song: u64) -> Result<bool> {
        self.0
            .lock()
            .query_row(A_VERSION_WAITS, [song as i64], |row| row.get(0))
            .map_err(read_error)
    }

    fn the_song_a_variant_is_of(&self, id: u64) -> Result<Option<u64>> {
        let song = self
            .0
            .lock()
            .query_row(THE_SONG_A_VARIANT_IS_OF, [id as i64], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .map_err(read_error)?;
        Ok(song.and_then(|song| u64::try_from(song).ok()))
    }

    fn variants_held_by(&self, song: u64) -> Result<Vec<Track>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare_cached(THE_VARIANTS_OF_A_SONG)
            .map_err(read_error)?;
        query
            .query_map([song as i64], read)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(read_error)
    }

    pub(crate) fn want_a_version(&self, song: u64, wanted: &SearchSong) -> Result<()> {
        self.save_wanted(std::slice::from_ref(wanted))?;
        let connection = self.0.lock();
        let want: i64 = connection
            .query_row(THE_WANT_OF_A_VERSION, [wanted_key(wanted)], |row| {
                row.get(0)
            })
            .map_err(read_error)?;
        connection
            .execute(THE_WANT_ASKED_AGAIN, [want])
            .and_then(|_| {
                connection.execute(A_VERSION_WANTED, params![want, song as i64, seconds_now()])
            })
            .map(drop)
            .map_err(write_error)
    }

    pub(crate) fn variant_landed(&self, song: &SearchSong, landed: u64) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let want: Option<i64> = transaction
            .query_row(THE_WANT_OF_A_VERSION, [wanted_key(song)], |row| row.get(0))
            .optional()
            .map_err(read_error)?;
        let Some(want) = want else {
            return Ok(());
        };
        let songs = {
            let mut query = transaction
                .prepare(THE_SONGS_A_WANT_IS_A_VERSION_OF)
                .map_err(read_error)?;
            query
                .query_map([want], |row| row.get::<_, i64>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(read_error)?
        };
        if songs.is_empty() {
            return Ok(());
        }

        let now = seconds_now();
        songs
            .iter()
            .filter_map(|song| u64::try_from(*song).ok())
            .try_for_each(|song| kept_as_a_variant(&transaction, song, landed, now))
            .and_then(|()| transaction.execute(THE_VERSIONS_WANTED_GO, [want]))
            .and_then(|_| transaction.execute(THE_WANT_GOES, [want]))
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;

    const SERVED: &str = "tidal:track/7";

    fn wanted_from_a_service() -> SearchSong {
        SearchSong {
            title: "Echoes".into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            served: Some(SERVED.into()),
            ..SearchSong::default()
        }
    }

    fn count(harness: &Harness, table: &str) -> i64 {
        harness
            .player
            .catalog
            .0
            .lock()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("the table")
    }

    #[test]
    fn a_version_chosen_is_kept_beside_its_song_and_queued_in_its_place() {
        let harness = Harness::new();
        let (song, copy) = (harness.song('a').id, harness.song('b').id);

        let told = harness
            .player
            .choose_version(song, SongVersion::Held { id: copy })
            .expect("a version chosen");
        harness.player.load(vec![song], 0, false).expect("a load");

        assert!(told.said.starts_with(&harness.song('a').title));
        assert_eq!(told.download, None);
        assert_eq!(
            harness.player.version_chosen(song).expect("the choice"),
            Some(SongVersion::Held { id: copy })
        );
        assert_eq!(harness.queued(), "b");

        harness
            .player
            .add_to_queue(vec![song, harness.song('c').id])
            .expect("an append");

        assert_eq!(harness.queued(), "bbc");
        assert_eq!(count(&harness, "versions_held"), 1);
    }

    #[test]
    fn the_variants_name_the_original_and_each_copy_kept_with_the_default_checked() {
        let harness = Harness::new();
        let (song, b, c) = (
            harness.song('a').id,
            harness.song('b').id,
            harness.song('c').id,
        );

        assert!(harness.player.variants_of(song).expect("none").is_empty());

        for copy in [b, c] {
            harness
                .player
                .choose_version(song, SongVersion::Held { id: copy })
                .expect("a version kept");
        }
        let listed = |id: u64| {
            harness
                .player
                .variants_of(id)
                .expect("the variants")
                .into_iter()
                .map(|variant| (variant.id, variant.original, variant.default))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            listed(song),
            vec![(song, true, false), (b, false, false), (c, false, true)]
        );
        assert_eq!(listed(b), listed(song));

        let mut holding = harness.player.songs_with_variants().expect("the songs");
        holding.sort_unstable();

        assert_eq!(holding, vec![song, b, c]);

        harness
            .player
            .choose_variant(b, b)
            .expect("another variant chosen");

        assert_eq!(
            listed(song),
            vec![(song, true, false), (b, false, true), (c, false, false)]
        );

        harness
            .player
            .choose_variant(c, song)
            .expect("the original chosen");

        assert_eq!(harness.player.version_chosen(song).expect("none"), None);
        assert_eq!(
            listed(song),
            vec![(song, true, true), (b, false, false), (c, false, false)]
        );
        assert!(matches!(
            harness.player.choose_variant(song, harness.song('d').id),
            Err(Error::NotAVariant)
        ));
    }

    #[test]
    fn a_variant_forgotten_takes_its_choice_and_leaves_the_song_playing_itself() {
        let harness = Harness::new();
        let (song, copy) = (harness.song('a').id, harness.song('b').id);

        harness
            .player
            .choose_version(song, SongVersion::Held { id: copy })
            .expect("a version chosen");
        harness
            .player
            .catalog
            .0
            .lock()
            .execute("DELETE FROM documents WHERE id = ?1", [copy as i64])
            .expect("the copy forgotten");
        harness.player.load(vec![song], 0, false).expect("a load");

        assert_eq!(harness.player.version_chosen(song).expect("none"), None);
        assert!(harness.player.variants_of(song).expect("none").is_empty());
        assert_eq!(harness.queued(), "a");
    }

    #[test]
    fn a_served_version_is_never_queued_and_one_of_no_service_is_refused() {
        let harness = Harness::new();
        let song = harness.song('a').id;

        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "INSERT INTO versions_chosen (track, held, served, chosen) VALUES (?1, NULL, ?2, 0)",
                params![song as i64, SERVED],
            )
            .expect("a served choice an older build kept");
        harness.player.load(vec![song], 0, false).expect("a load");

        assert_eq!(harness.queued(), "a");
        assert_eq!(harness.player.version_chosen(song).expect("none"), None);
        assert!(matches!(
            harness
                .player
                .choose_version(song, SongVersion::Served { uri: SERVED.into() }),
            Err(Error::NoServedServer)
        ));
        assert_eq!(count(&harness, "wanted"), 0);
    }

    #[test]
    fn a_version_downloaded_lands_beside_its_song_and_plays_from_then_on() {
        let harness = Harness::new();
        let (song, landed) = (harness.song('a').id, harness.song('b').id);
        let catalog = &harness.player.catalog;

        catalog
            .want_a_version(song, &wanted_from_a_service())
            .expect("a version wanted");
        let due = harness.player.poll_wanted_due().expect("the wants due");

        assert_eq!(due.len(), 1);
        assert_eq!(due[0].song.served.as_deref(), Some(SERVED));
        assert!(catalog.a_version_waits_for(song).expect("a wait"));

        catalog
            .variant_landed(&due[0].song, landed)
            .expect("the version kept");
        harness.player.load(vec![song], 0, false).expect("a load");

        assert_eq!(harness.queued(), "b");
        assert!(!catalog.a_version_waits_for(song).expect("no wait"));
        assert_eq!(count(&harness, "wanted"), 0);
        assert_eq!(
            harness
                .player
                .variants_of(song)
                .expect("the variants")
                .iter()
                .map(|variant| (variant.id, variant.default))
                .collect::<Vec<_>>(),
            vec![(song, false), (landed, true)]
        );
    }

    #[test]
    fn a_want_whose_song_is_gone_is_held_and_never_downloaded() {
        let harness = Harness::new();
        let song = harness.song('a').id;

        harness
            .player
            .catalog
            .want_a_version(song, &wanted_from_a_service())
            .expect("a version wanted");
        harness
            .player
            .catalog
            .0
            .lock()
            .execute("DELETE FROM documents WHERE id = ?1", [song as i64])
            .expect("the song forgotten");

        assert!(harness.player.poll_wanted_due().expect("none").is_empty());
    }

    #[test]
    fn a_file_is_named_by_its_known_format_else_by_its_media_type() {
        assert_eq!(extension_of(Some("FLAC"), Some("audio/mpeg")), "flac");
        assert_eq!(extension_of(None, Some("audio/mp4; codecs=alac")), "m4a");
        assert_eq!(extension_of(Some("not a format"), None), "flac");
    }
}
