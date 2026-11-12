use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{
    MediaLocation, SampleRate, TrackId,
    titles::{self, Rating},
};
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::{Quality, SongHeard, TheSong, VersionSought};
use resonate_providers::{Named, PlaysInPlace, Song};
use rusqlite::{OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track, documents::location, search::codec_named,
    served::ServedSong, words,
};

const HELD_WEIGHED_AT_MOST: u32 = 64;

const NAMES_APART: &str = ", ";

const BITS_WHERE_NONE_ARE_KNOWN: u8 = 16;

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

    const fn sought(self) -> VersionSought {
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

fn artist_of(track: &Track) -> Option<&str> {
    Some(track.artist.as_str()).filter(|artist| !artist.trim().is_empty())
}

fn heard_of(track: &Track) -> SongHeard<'_> {
    SongHeard {
        title: &track.title,
        artist: artist_of(track),
        length: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
        lossless: codec_named(&track.codec).is_lossless(),
        rating: rating_of(track),
    }
}

fn quality_of(track: &Track) -> Quality {
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

fn held_versions(track: &Track, search: VersionSearch, found: Vec<Track>) -> Vec<Track> {
    let song = TheSong::new(heard_of(track));
    let others = found.into_iter().filter(|copy| copy.id != track.id);
    match search.sought() {
        VersionSought::Better => {
            let held = quality_of(track);
            let mut better: Vec<Track> = others
                .filter(|copy| song.names(heard_of(copy)) && quality_of(copy).is_above(held))
                .collect();
            better.sort_by_key(|copy| quality_of(copy).best_first());
            better
        }
        VersionSought::Rated(wanted) => others
            .filter(|copy| song.held_as(wanted, heard_of(copy)))
            .collect(),
    }
}

fn quality_said(track: &Track) -> String {
    let bits = track.bits.unwrap_or(u32::from(BITS_WHERE_NONE_ARE_KNOWN));
    let rate = SampleRate::new(track.rate)
        .map_or_else(|_| track.rate.to_string(), |rate| rate.to_string());
    words::versions::quality(bits, rate)
}

fn held_label(search: VersionSearch, copy: &Track) -> String {
    let album = Some(copy.album.as_str()).filter(|album| !album.trim().is_empty());
    match (search, album) {
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
                Some(album) => words::versions::play_served_on_album(album, codec, service),
                None => words::versions::play_served(codec, service),
            }
        }
        VersionSearch::Clean | VersionSearch::Explicit => match album {
            Some(album) => words::versions::play_it_on_album(album, service),
            None => words::versions::play_it_on(service),
        },
    }
}

#[derive(Default)]
struct Asked {
    found: Vec<FoundVersion>,
    services: Vec<String>,
    refused: Vec<String>,
}

fn asked_of(
    services: &[Arc<dyn PlaysInPlace>],
    words: &str,
    song: &TheSong,
    search: VersionSearch,
) -> Asked {
    let mut asked = Asked::default();
    for plays in services.iter().filter(|plays| plays.plays_songs()) {
        let service = plays.title().to_owned();
        match plays.search(words) {
            Ok(found) => asked.found.extend(
                found
                    .songs
                    .into_iter()
                    .filter(|found| song.served_as(search.sought(), heard_served(found)))
                    .map(|found| FoundVersion {
                        label: served_label(search, &service, &found),
                        version: SongVersion::Served {
                            uri: plays.location_of(&Named::Song(found.id)).to_uri(),
                        },
                    }),
            ),
            Err(error) => {
                tracing::warn!(%error, service = %service, "a service could not be asked for another version");
                asked.refused.push(service.clone());
            }
        }
        asked.services.push(service);
    }
    asked
}

pub(crate) enum Chosen {
    Held { id: u64, uri: String },
    Served(MediaLocation),
}

const THE_VERSION_CHOSEN: &str = "SELECT d.id, d.uri, v.served FROM versions_chosen v
     LEFT JOIN documents d ON d.id = v.held AND d.missing = 0
     WHERE v.track = ?1";

const CHOSEN: &str = "INSERT INTO versions_chosen (track, held, served, chosen)
     VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT (track) DO UPDATE
        SET held = excluded.held, served = excluded.served, chosen = excluded.chosen";

const FORGOTTEN: &str = "DELETE FROM versions_chosen WHERE track = ?1";

const KEPT: &str = "SELECT held, served FROM versions_chosen WHERE track = ?1";

fn seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
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
        let services = match self.require_online() {
            Ok(()) => self.served.playing_services(),
            Err(_) => Vec::new(),
        };

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

        let asked = asked_of(&services, &words, &song, search);
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

    pub fn choose_version(&self, id: u64, version: SongVersion) -> Result<String> {
        let track = self.catalog.track(id)?;
        let (held, served) = match &version {
            SongVersion::Held { id: copy } => {
                self.catalog.track(*copy)?;
                (Some(*copy), None)
            }
            SongVersion::Served { uri } => {
                MediaLocation::from_uri(uri).ok_or(Error::InvalidUri)?;
                (None, Some(uri.clone()))
            }
        };
        {
            let connection = self.catalog.0.lock();
            let written = match held {
                Some(copy) if copy == id => connection.execute(FORGOTTEN, params![id as i64]),
                _ => connection.execute(
                    CHOSEN,
                    params![
                        id as i64,
                        held.map(|copy| copy as i64),
                        served,
                        seconds_now()
                    ],
                ),
            };
            written.map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(words::versions::chosen(titles::bare(&track.title)))
    }

    pub fn version_chosen(&self, id: u64) -> Result<Option<SongVersion>> {
        let kept = self
            .catalog
            .0
            .lock()
            .query_row(KEPT, params![id as i64], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(kept.and_then(|kept| match kept {
            (Some(copy), None) => u64::try_from(copy).ok().map(|id| SongVersion::Held { id }),
            (None, Some(uri)) => Some(SongVersion::Served { uri }),
            _ => None,
        }))
    }

    pub fn forget_version_chosen(&self, id: u64) -> Result<String> {
        let track = self.catalog.track(id)?;
        self.catalog
            .0
            .lock()
            .execute(FORGOTTEN, params![id as i64])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(words::versions::own_copy_again(titles::bare(&track.title)))
    }
}

impl MobilePlayer {
    fn chosen_for(&self, id: TrackId) -> Result<Option<Chosen>> {
        let row = self
            .catalog
            .0
            .lock()
            .query_row(THE_VERSION_CHOSEN, params![id.get() as i64], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(row.and_then(|row| match row {
            (Some(copy), Some(uri), None) => {
                u64::try_from(copy).ok().map(|id| Chosen::Held { id, uri })
            }
            (None, None, Some(served)) => MediaLocation::from_uri(&served).map(Chosen::Served),
            _ => None,
        }))
    }

    pub(crate) fn as_chosen<'a>(
        &self,
        items: Vec<QueueItem>,
        beside: impl IntoIterator<Item = &'a QueueItem>,
    ) -> Result<Vec<QueueItem>> {
        let mut chosen = Vec::with_capacity(items.len());
        for item in &items {
            chosen.push(self.chosen_for(item.id)?);
        }
        if chosen.iter().all(Option::is_none) {
            return Ok(items);
        }

        let standing: Vec<QueueItem> = beside.into_iter().cloned().collect();
        let mut minting = Unclaimed::beside(standing.iter().chain(&items));
        items
            .into_iter()
            .zip(chosen)
            .map(|(item, chosen)| match chosen {
                Some(Chosen::Held { id, uri }) => {
                    Ok(QueueItem::whole(TrackId::new(id)?, location(&uri)?))
                }
                Some(Chosen::Served(served)) if self.served.serves(&served) => {
                    self.name_as_its_song(&item, &served)?;
                    Ok(QueueItem::whole(minting.mint(), served))
                }
                Some(Chosen::Served(_)) | None => Ok(item),
            })
            .collect()
    }

    fn name_as_its_song(&self, item: &QueueItem, served: &MediaLocation) -> Result<()> {
        if self.served.knows(served) {
            return Ok(());
        }
        let song = self.catalog.track(item.id.get())?;
        self.served.kept(&[ServedSong {
            uri: served.to_uri(),
            title: song.title,
            artist: Some(song.artist).filter(|artist| !artist.is_empty()),
            album: Some(song.album).filter(|album| !album.is_empty()),
            track: song.track_number,
            duration_ms: (song.duration_ms > 0).then_some(song.duration_ms),
            format: None,
            cover: None,
            favourite: None,
        }]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;

    #[test]
    fn a_version_chosen_is_queued_in_place_of_its_song_and_forgotten_with_it() {
        let harness = Harness::new();
        let (song, copy) = (harness.song('a').id, harness.song('b').id);

        let told = harness
            .player
            .choose_version(song, SongVersion::Held { id: copy })
            .expect("a version chosen");
        harness.player.load(vec![song], 0, false).expect("a load");

        assert!(told.starts_with(&harness.song('a').title));
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

        harness
            .player
            .choose_version(
                song,
                SongVersion::Served {
                    uri: "tidal:track/7".to_owned(),
                },
            )
            .expect("a served version chosen");
        harness.player.load(vec![song], 0, false).expect("a load");

        assert_eq!(harness.queued(), "a");

        harness
            .player
            .choose_version(song, SongVersion::Held { id: copy })
            .expect("a version chosen again");
        harness
            .player
            .catalog
            .0
            .lock()
            .execute("DELETE FROM documents WHERE id = ?1", [copy as i64])
            .expect("the copy forgotten");

        assert_eq!(
            harness.player.version_chosen(song).expect("the choice"),
            None
        );

        harness
            .player
            .choose_version(song, SongVersion::Held { id: song })
            .expect("the song itself chosen");

        assert_eq!(
            harness.player.version_chosen(song).expect("the choice"),
            None
        );
    }
}
