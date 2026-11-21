use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::title_words;
use resonate_engine::{Command, Placement, QueueItem, Unclaimed};
use resonate_library::{
    ExploreOffer, ExplorePlaylistKind as LibraryExploreKind, ExploreProfile, ExploreTrack,
    HALF_A_YEAR, Kinship, MixEntry, OF_LATE, artists_as_heard, artists_for_explore,
    artists_heard_lately, explore_blueprints, explore_seeds, genres_ranked_by, weave_mix,
};
use resonate_providers::{Id, Song, SongCatalog};

use crate::{
    Error, MobilePlayer, Result, Track,
    catalog_extras::the_listeners_calendar,
    features::{SearchSong, TrackFilter},
    mixes::MixAccents,
    saved_searches::SavedSearch,
    search::Index,
    served::{QueueSong, ServedSong},
};

const QUEUE_SUGGESTIONS_AT_MOST: usize = 4;
const QUEUE_WEAVE_EVERY: usize = 3;
const LEFT_BEHIND_PLAYS_AT_LEAST: i64 = 10;

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ExploreScope {
    Library,
    Saved { search: SavedSearch },
    Filtered { filter: TrackFilter },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ExplorePlaylistKind {
    DiscoverMix,
    BeyondArtist,
    GenreDiscovery,
    FavouritesExpansion,
    RediscoverBeyond,
}

impl From<LibraryExploreKind> for ExplorePlaylistKind {
    fn from(kind: LibraryExploreKind) -> Self {
        match kind {
            LibraryExploreKind::DiscoverMix => Self::DiscoverMix,
            LibraryExploreKind::BeyondArtist => Self::BeyondArtist,
            LibraryExploreKind::GenreDiscovery => Self::GenreDiscovery,
            LibraryExploreKind::FavouritesExpansion => Self::FavouritesExpansion,
            LibraryExploreKind::RediscoverBeyond => Self::RediscoverBeyond,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExploreSong {
    pub song: ServedSong,
    pub service: String,
    pub download: SearchSong,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExploreFeed {
    pub songs: Vec<ExploreSong>,
    pub services: u32,
    pub failed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExplorePlaylist {
    pub id: String,
    pub kind: ExplorePlaylistKind,
    pub name: String,
    pub detail: String,
    pub accents: MixAccents,
    pub songs: Vec<ExploreSong>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExplorePlaylistsFeed {
    pub playlists: Vec<ExplorePlaylist>,
    pub services: u32,
    pub failed: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn explore_playlists(
        &self,
        time_zone: String,
        utc_offset_seconds: i32,
        refresh: bool,
    ) -> Result<ExplorePlaylistsFeed> {
        self.require_online()?;
        let sources = self.served.song_catalogs();
        if sources.is_empty() {
            return Ok(ExplorePlaylistsFeed {
                playlists: Vec::new(),
                services: 0,
                failed: 0,
            });
        }
        let searching = &self.services.searching;
        let index = searching.index(&self.catalog)?;
        let now = SystemTime::now();
        let day = the_listeners_calendar(&time_zone, utc_offset_seconds).day_of(now);
        let kinship = Kinship::of(&index.genres_of_artists());
        let profile = self.explore_profile(&index, now)?;
        let blueprints = explore_blueprints(&profile, &kinship, day);
        let hydrated = self.served.explore.hydrate_blueprints_with(
            day,
            blueprints,
            &sources,
            refresh,
            self.catalog.as_ref(),
        );
        self.require_online()?;
        let keys: BTreeSet<_> = hydrated.artists().into_iter().map(title_words).collect();
        let current = searching.index(&self.catalog)?;
        let held = held_for_keys(&current, &keys);
        let curated = hydrated.curate(&held);

        let mut playlists = Vec::with_capacity(curated.playlists.len());
        for playlist in curated.playlists {
            let songs = self.explore_songs_of(&sources, playlist.songs)?;
            if songs.is_empty() {
                continue;
            }
            self.served.remember_explore_songs(&songs);
            let (from, to) = playlist.kind.accents(&playlist.name);
            playlists.push(ExplorePlaylist {
                id: playlist.id,
                kind: playlist.kind.into(),
                name: playlist.name,
                detail: playlist.subtitle,
                accents: MixAccents {
                    from: from.into(),
                    to: to.into(),
                },
                songs,
                duration_ms: playlist.length.map_or(0, |length| {
                    u64::try_from(length.as_millis()).unwrap_or(u64::MAX)
                }),
            });
        }

        Ok(ExplorePlaylistsFeed {
            playlists,
            services: u32::try_from(curated.services).unwrap_or(u32::MAX),
            failed: u32::try_from(curated.failed).unwrap_or(u32::MAX),
        })
    }

    pub fn explore(
        &self,
        scope: ExploreScope,
        time_zone: String,
        utc_offset_seconds: i32,
        refresh: bool,
    ) -> Result<ExploreFeed> {
        self.require_online()?;
        let sources = self.served.song_catalogs();
        if sources.is_empty() {
            return Ok(ExploreFeed {
                songs: Vec::new(),
                services: 0,
                failed: 0,
            });
        }
        let searching = &self.services.searching;
        let index = searching.index(&self.catalog)?;
        let now = SystemTime::now();
        let day = the_listeners_calendar(&time_zone, utc_offset_seconds).day_of(now);
        let direct = match scope {
            ExploreScope::Library => self.library_explore_artists(&index, now)?,
            ExploreScope::Saved { search } => {
                let (index, rows) = searching.saved(&self.catalog, &search.query())?;
                artists_for_explore(
                    rows.into_iter()
                        .map(|at| index.rows[at].track.artist.as_str()),
                )
            }
            ExploreScope::Filtered { filter } => searching
                .admitted_in_order(&self.catalog, &filter)?
                .map_or_else(Vec::new, |(index, rows)| {
                    artists_for_explore(rows.iter().map(|at| index.rows[*at].track.artist.as_str()))
                }),
        };
        let day_seeds = explore_seeds(&direct, &Kinship::of(&index.genres_of_artists()), day);
        let found =
            self.served
                .explore
                .find_with(day, day_seeds, &sources, refresh, self.catalog.as_ref());
        let keys: BTreeSet<_> = found.artists().into_iter().map(title_words).collect();
        self.require_online()?;
        let current = searching.index(&self.catalog)?;
        let held = held_for_keys(&current, &keys);
        let found = found.select(&held);
        let songs = self.explore_songs_of(&sources, found.songs)?;
        self.served.remember_explore_songs(&songs);
        Ok(ExploreFeed {
            songs,
            services: u32::try_from(found.services).unwrap_or(u32::MAX),
            failed: u32::try_from(found.failed).unwrap_or(u32::MAX),
        })
    }

    pub fn weave_mix_queue(
        &self,
        local_ids: Vec<u64>,
        suggestions: Vec<ExploreSong>,
        shuffled: bool,
    ) -> Result<Vec<QueueSong>> {
        self.served.remember_explore_songs(&suggestions);
        let index = self.services.searching.index(&self.catalog)?;
        let artists_by_id: BTreeMap<u64, &str> = index
            .rows
            .iter()
            .map(|row| (row.track.id, row.track.artist.as_str()))
            .collect();
        let mut local: Vec<(u64, String)> = local_ids
            .into_iter()
            .map(|id| {
                let artist = artists_by_id
                    .get(&id)
                    .copied()
                    .unwrap_or_default()
                    .to_owned();
                (id, artist)
            })
            .collect();
        let mut offers: Vec<ExploreOffer> = suggestions
            .iter()
            .filter_map(offer_from_explore_song)
            .collect();
        if shuffled {
            shuffle_slice(&mut local);
            shuffle_slice(&mut offers);
        }
        let woven = weave_mix(
            local,
            |(_, artist)| (!artist.is_empty()).then_some(artist.as_str()),
            offers,
        );
        Ok(woven
            .into_iter()
            .map(|entry| match entry {
                MixEntry::Local((id, _)) => QueueSong::Library { id },
                MixEntry::Suggested(offer) => QueueSong::Served {
                    uri: offer.location.to_uri(),
                },
            })
            .collect())
    }

    pub fn explore_download_for(&self, uri: String) -> Option<ExploreSong> {
        self.served.explore_song_at(&uri)
    }

    pub fn mix_suggestions_into_queue(
        &self,
        time_zone: String,
        utc_offset_seconds: i32,
    ) -> Result<u32> {
        self.require_online()?;
        let sources = self.served.song_catalogs();
        if sources.is_empty() {
            return Ok(0);
        }
        let player = self.playing.player()?;
        let snapshot = player.state();
        let queued = player.queued();
        if queued.rows.is_empty() {
            return Ok(0);
        }
        let position = snapshot.queue_position.unwrap_or(0);
        let already: BTreeSet<_> = queued
            .rows
            .iter()
            .map(|item| item.location.to_uri())
            .collect();
        let index = self.services.searching.index(&self.catalog)?;
        let by_uri: BTreeMap<&str, &str> = index
            .rows
            .iter()
            .map(|row| (row.track.uri.as_str(), row.track.artist.as_str()))
            .collect();
        let direct = artists_for_explore(
            queued
                .rows
                .iter()
                .filter_map(|item| by_uri.get(item.location.to_uri().as_str()).copied()),
        );
        if direct.is_empty() {
            return Ok(0);
        }
        let now = SystemTime::now();
        let day = the_listeners_calendar(&time_zone, utc_offset_seconds).day_of(now);
        let seeds = explore_seeds(&direct, &Kinship::of(&index.genres_of_artists()), day);
        let found =
            self.served
                .explore
                .find_with(day, seeds, &sources, false, self.catalog.as_ref());
        self.require_online()?;
        let keys: BTreeSet<_> = found.artists().into_iter().map(title_words).collect();
        let current = self.services.searching.index(&self.catalog)?;
        let held = held_for_keys(&current, &keys);
        let selected = found.select(&held);
        let fresh_offers: Vec<ExploreOffer> = selected
            .songs
            .into_iter()
            .filter(|offer| !already.contains(&offer.location.to_uri()))
            .take(QUEUE_SUGGESTIONS_AT_MOST)
            .collect();
        if fresh_offers.is_empty() {
            return Ok(0);
        }
        let songs = self.explore_songs_of(&sources, fresh_offers)?;
        self.served.remember_explore_songs(&songs);
        let uris: Vec<String> = songs.into_iter().map(|song| song.song.uri).collect();
        let upcoming_len = queued.rows.len().saturating_sub(position + 1);
        let step = if upcoming_len >= QUEUE_WEAVE_EVERY {
            QUEUE_WEAVE_EVERY
        } else {
            1
        };
        let mut inserted = 0u32;
        for (nth, uri) in uris.into_iter().enumerate() {
            let added = inserted as usize;
            let mut placed = false;
            self.edit_queue(|live, _| {
                if live.rows.is_empty() {
                    return Ok(None);
                }
                let mut minting = Unclaimed::beside(live.rows.iter().map(std::sync::Arc::as_ref));
                let location =
                    resonate_core::MediaLocation::from_uri(&uri).ok_or(Error::InvalidUri)?;
                let item = QueueItem::whole(minting.mint(), location);
                let target = (position + 1 + (nth + 1) * step + added).min(live.rows.len());
                placed = true;
                Ok(Some(Command::Insert {
                    items: vec![item],
                    at: Placement::At(target),
                    play: false,
                }))
            })?;
            if placed {
                inserted += 1;
            }
        }
        Ok(inserted)
    }

    pub(crate) fn check_explore_download(&self, song: &SearchSong) -> Result<()> {
        let wanted = ExploreTrack::named(
            &song.title,
            &song.artist,
            song.duration_ms.map(Duration::from_millis),
        );
        let index = self.services.searching.index(&self.catalog)?;
        if index
            .rows
            .iter()
            .any(|row| wanted.is_the_song(&explore_track(&row.track)))
        {
            return Err(Error::ExploreAlreadyHeld);
        }
        Ok(())
    }
}

impl MobilePlayer {
    fn explore_songs_of(
        &self,
        sources: &[SongCatalog],
        offers: Vec<ExploreOffer>,
    ) -> Result<Vec<ExploreSong>> {
        Ok(self
            .served
            .explored(sources, offers)?
            .into_iter()
            .map(|(offer, song)| ExploreSong {
                download: SearchSong {
                    title: song.title.clone(),
                    artist: song.artist.clone().unwrap_or_default(),
                    album: song.album.clone().unwrap_or_default(),
                    duration_ms: song.duration_ms,
                    disc: offer.song.disc,
                    position: song.track,
                    served: Some(song.uri.clone()),
                    served_catalog: Some(offer.catalog),
                    ..SearchSong::default()
                },
                service: offer.service,
                song,
            })
            .collect())
    }

    fn explore_profile(&self, index: &Index, now: SystemTime) -> Result<ExploreProfile> {
        let since = now
            .checked_sub(OF_LATE)
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let heard_lately = self
            .services
            .searching
            .heard_lately(&self.catalog, i64::try_from(since).unwrap_or(i64::MAX))?;
        let recent_artists = artists_heard_lately(heard_lately);
        let favourite_artists = artists_for_explore(
            index
                .rows
                .iter()
                .filter(|row| row.track.favourite)
                .map(|row| row.track.artist.as_str()),
        );
        let heard = artists_as_heard(index.artists_heard());
        let half_a_year_ago = now.checked_sub(HALF_A_YEAR).unwrap_or(UNIX_EPOCH);
        let left_behind_artists: Vec<String> = heard
            .iter()
            .filter(|artist| {
                artist.plays >= LEFT_BEHIND_PLAYS_AT_LEAST
                    && artist
                        .last_played
                        .is_some_and(|played| played < half_a_year_ago)
            })
            .take(6)
            .map(|artist| artist.name.clone())
            .collect();
        let mut by_held = heard;
        by_held.sort_by(|one, other| {
            other
                .held
                .cmp(&one.held)
                .then_with(|| one.name.cmp(&other.name))
        });
        let top_artists = by_held
            .into_iter()
            .take(6)
            .map(|artist| artist.name)
            .collect();
        let played_genres = genres_ranked_by(index.genres_played());
        let top_genres = if !played_genres.is_empty() {
            played_genres
        } else {
            genres_ranked_by(index.genres_held())
        };
        Ok(ExploreProfile {
            recent_artists,
            favourite_artists,
            top_artists,
            left_behind_artists,
            top_genres,
        })
    }

    fn library_explore_artists(&self, index: &Index, now: SystemTime) -> Result<Vec<String>> {
        let profile = self.explore_profile(index, now)?;
        if !profile.recent_artists.is_empty() {
            return Ok(profile.recent_artists);
        }
        if !profile.favourite_artists.is_empty() {
            return Ok(profile.favourite_artists);
        }
        Ok(profile.top_artists)
    }
}

fn held_for_keys(current: &Index, keys: &BTreeSet<String>) -> Vec<ExploreTrack> {
    current
        .rows
        .iter()
        .filter(|row| keys.contains(&title_words(&row.track.artist)))
        .map(|row| explore_track(&row.track))
        .collect()
}

fn explore_track(track: &Track) -> ExploreTrack {
    ExploreTrack::named(
        &track.title,
        &track.artist,
        (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
    )
}

fn offer_from_explore_song(song: &ExploreSong) -> Option<ExploreOffer> {
    let location = resonate_core::MediaLocation::from_uri(&song.song.uri)?;
    let id = Id::new(
        location
            .locator()
            .as_key()
            .and_then(|key| key.strip_prefix("track/"))
            .unwrap_or(&song.song.uri),
    )?;
    let cover = song.song.cover.as_deref().and_then(Id::new);
    Some(ExploreOffer {
        catalog: song.download.served_catalog.clone().unwrap_or_default(),
        service: song.service.clone(),
        location,
        song: Song {
            id,
            title: song.song.title.clone(),
            artist: song.song.artist.clone(),
            album: song.song.album.clone(),
            disc: song.download.disc,
            track: song.song.track,
            length: song.song.duration_ms.map(Duration::from_millis),
            suffix: song.song.format.clone(),
            cover,
            favourite: song.song.favourite,
            rating: resonate_core::titles::Rating::Unmarked,
        },
    })
}

fn shuffle_slice<T>(items: &mut [T]) {
    if items.len() < 2 {
        return;
    }
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x9e37_79b9_7f4a_7c15_u64, |since| since.as_nanos() as u64)
        | 1;
    for index in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let swap = (state as usize) % (index + 1);
        items.swap(index, swap);
    }
}
