use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_core::{
    Accent, Calendar, MediaLocation, SourceId, TrackId, title_words,
    titles::{self, Rating},
};
use resonate_providers::{
    A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY, Id, Named, Song, SongCatalog, led_by,
};
use rusqlite::{OptionalExtension as _, params};

use crate::{
    Error, Kinship, Library, Result, SavedQuery, StoreOp, TrackQuery, store,
    suggest::{self, HALF_A_YEAR},
    words,
};

const ARTISTS_AT_MOST: usize = 4;
const RELATED_ARTISTS_FIRST: usize = 2;
const ROTATING_ARTISTS_AT_MOST: usize = 6;
const CANDIDATES_PER_SEARCH: usize = 32;
const SONGS_PER_ARTIST: usize = 3;
const SONGS_PER_PRIMARY_ARTIST: usize = 4;
const SONGS_PER_ALBUM_AT_MOST: usize = 2;
const PLAYLIST_SONGS_AT_MOST: usize = 16;
const BEYOND_ARTIST_PLAYLISTS: usize = 2;
const BEYOND_RELATED_SEEDS: usize = 3;
const GENRE_PLAYLIST_SEEDS: usize = 4;
const UNIQUE_SEEDS_AT_MOST: usize = 10;
const CACHE_ARTISTS: usize = 64;
const EXPLORE_COVER_BYTES_AT_MOST: usize = 2 * 1024 * 1024;
const EXPLORE_COVERS_HELD: u32 = 256;
const EXPLORE_COVER_BUDGET: u32 = 64 * 1024 * 1024;
const LOCAL_PER_SUGGESTION: usize = 3;
const SHORT_MIX_LOCAL_PER_SUGGESTION: usize = 2;
const MEDIUM_MIX_AT_LEAST: usize = 7;
const SHORT_MIX_AT_LEAST: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExploreOffer {
    pub catalog: String,
    pub service: String,
    pub location: MediaLocation,
    pub song: Song,
}

impl ExploreOffer {
    fn picture_key(&self) -> (bool, &str) {
        self.song
            .cover
            .as_ref()
            .map_or((false, self.song.id.as_str()), |cover| {
                (true, cover.as_str())
            })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExploreResult {
    pub songs: Vec<ExploreOffer>,
    pub services: usize,
    pub failed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExplorePlaylistKind {
    DiscoverMix,
    BeyondArtist,
    GenreDiscovery,
    FavouritesExpansion,
    RediscoverBeyond,
}

impl ExplorePlaylistKind {
    pub fn accents(self, name: &str) -> (Accent, Accent) {
        match self {
            Self::DiscoverMix => (Accent::Teal, Accent::Blue),
            Self::BeyondArtist | Self::GenreDiscovery => Accent::named_pair(name),
            Self::FavouritesExpansion => (Accent::Mauve, Accent::Red),
            Self::RediscoverBeyond => (Accent::Amber, Accent::Mauve),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExploreBlueprint {
    pub id: String,
    pub kind: ExplorePlaylistKind,
    pub name: String,
    pub subtitle: String,
    pub primary_seeds: Vec<String>,
    pub related_seeds: Vec<String>,
}

impl ExploreBlueprint {
    pub fn all_seeds(&self) -> impl Iterator<Item = &str> {
        self.primary_seeds
            .iter()
            .chain(&self.related_seeds)
            .map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorePlaylist {
    pub id: String,
    pub kind: ExplorePlaylistKind,
    pub name: String,
    pub subtitle: String,
    pub songs: Vec<ExploreOffer>,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplorePlaylists {
    pub playlists: Vec<ExplorePlaylist>,
    pub services: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExploreProfile {
    pub recent_artists: Vec<String>,
    pub favourite_artists: Vec<String>,
    pub top_artists: Vec<String>,
    pub left_behind_artists: Vec<String>,
    pub top_genres: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MixEntry<T> {
    Local(T),
    Suggested(Box<ExploreOffer>),
}

pub struct ExploreTrack {
    title: String,
    artist: String,
    length: Option<Duration>,
}

impl ExploreTrack {
    pub fn named(title: &str, artist: &str, length: Option<Duration>) -> Self {
        Self {
            title: title_words(titles::bare(title)),
            artist: title_words(artist),
            length,
        }
    }

    pub(crate) fn of(song: &Song) -> Self {
        Self::named(
            &song.title,
            song.artist.as_deref().unwrap_or_default(),
            song.length,
        )
    }

    pub fn is_the_song(&self, other: &Self) -> bool {
        !self.title.is_empty()
            && !self.artist.is_empty()
            && self.title == other.title
            && self.artist == other.artist
            && self.length.zip(other.length).is_none_or(|(one, other)| {
                one.abs_diff(other) <= A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY
            })
    }
}

pub fn artists_for_explore<'a>(artists: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut counted = BTreeMap::<String, (&str, usize)>::new();
    for artist in artists.into_iter().map(str::trim) {
        if let Some(key) = artist_key(artist) {
            let held = counted.entry(key).or_insert((artist, 0));
            held.0 = held.0.min(artist);
            held.1 += 1;
        }
    }
    let mut ranked: Vec<_> = counted.into_values().collect();
    ranked.sort_by(|one, other| other.1.cmp(&one.1).then_with(|| one.0.cmp(other.0)));
    ranked
        .into_iter()
        .take(ROTATING_ARTISTS_AT_MOST)
        .map(|(artist, _)| artist.to_owned())
        .collect()
}

fn artist_key(artist: &str) -> Option<String> {
    let key = title_words(artist);
    (!key.is_empty() && !artist.contains('"')).then_some(key)
}

fn deduplicated_artists(artists: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    artists
        .iter()
        .filter(|name| artist_key(name).is_some_and(|key| seen.insert(key)))
        .take(ROTATING_ARTISTS_AT_MOST)
        .cloned()
        .collect()
}

pub fn explore_seeds(direct: &[String], kinship: &Kinship, day: i64) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut direct: Vec<_> = direct
        .iter()
        .filter(|name| artist_key(name).is_some_and(|key| seen.insert(key)))
        .take(ROTATING_ARTISTS_AT_MOST)
        .cloned()
        .collect();
    let mut related: Vec<_> = direct
        .iter()
        .flat_map(|seed| kinship.related(seed))
        .filter(|name| artist_key(name).is_some_and(|key| seen.insert(key)))
        .take(ROTATING_ARTISTS_AT_MOST)
        .collect();
    rotate(&mut direct, day);
    rotate(&mut related, day);
    let direct_first =
        (ARTISTS_AT_MOST - related.len().min(RELATED_ARTISTS_FIRST)).min(direct.len());
    let mut chosen: Vec<_> = direct.drain(..direct_first).collect();
    chosen.extend(related.drain(..(ARTISTS_AT_MOST - chosen.len()).min(related.len())));
    chosen.extend(direct.into_iter().take(ARTISTS_AT_MOST - chosen.len()));
    chosen
}

pub fn explore_blueprints(
    profile: &ExploreProfile,
    kinship: &Kinship,
    day: i64,
) -> Vec<ExploreBlueprint> {
    let base_artists = if !profile.recent_artists.is_empty() {
        deduplicated_artists(&profile.recent_artists)
    } else if !profile.favourite_artists.is_empty() {
        deduplicated_artists(&profile.favourite_artists)
    } else {
        deduplicated_artists(&profile.top_artists)
    };
    if base_artists.is_empty() {
        return Vec::new();
    }

    let mut blueprints = Vec::new();

    let discover_seeds = explore_seeds(&base_artists, kinship, day);
    let base_keys: BTreeSet<_> = base_artists
        .iter()
        .filter_map(|name| artist_key(name))
        .collect();
    let (discover_primary, discover_related): (Vec<_>, Vec<_>) = discover_seeds
        .into_iter()
        .partition(|name| artist_key(name).is_some_and(|key| base_keys.contains(&key)));
    blueprints.push(ExploreBlueprint {
        id: "discover-mix".to_owned(),
        kind: ExplorePlaylistKind::DiscoverMix,
        name: words::explore::discover_mix().to_owned(),
        subtitle: words::explore::discover_mix_reason().to_owned(),
        primary_seeds: discover_primary,
        related_seeds: discover_related,
    });

    let mut beyond_pool = base_artists.clone();
    rotate(&mut beyond_pool, day.saturating_add(1));
    let mut beyond_added = 0;
    for seed in beyond_pool {
        if beyond_added >= BEYOND_ARTIST_PLAYLISTS {
            break;
        }
        let Some(key) = artist_key(&seed) else {
            continue;
        };
        let mut related = kinship.related(&seed);
        rotate(&mut related, day);
        related.truncate(BEYOND_RELATED_SEEDS);
        if related.is_empty() && base_artists.len() == 1 {
            continue;
        }
        blueprints.push(ExploreBlueprint {
            id: format!("beyond:{key}"),
            kind: ExplorePlaylistKind::BeyondArtist,
            name: words::explore::beyond_artist(&seed),
            subtitle: words::explore::beyond_artist_reason(&seed),
            primary_seeds: vec![seed],
            related_seeds: related,
        });
        beyond_added += 1;
    }

    if !profile.top_genres.is_empty() {
        let mut genres = profile.top_genres.clone();
        rotate(&mut genres, day);
        for genre in genres {
            let mut genre_artists = kinship.artists_in_genre(&genre);
            if genre_artists.is_empty() {
                continue;
            }
            rotate(&mut genre_artists, day);
            genre_artists.truncate(GENRE_PLAYLIST_SEEDS);
            let billed = suggest::billed_as(&genre);
            let genre_key = store::folded_letters(&genre);
            blueprints.push(ExploreBlueprint {
                id: format!("genre:{genre_key}"),
                kind: ExplorePlaylistKind::GenreDiscovery,
                name: words::explore::genre_discovery(&billed),
                subtitle: words::explore::genre_discovery_reason(&billed),
                primary_seeds: genre_artists,
                related_seeds: Vec::new(),
            });
            break;
        }
    }

    let favourites = deduplicated_artists(&profile.favourite_artists);
    if !favourites.is_empty() && favourites != base_artists {
        let seeds = explore_seeds(&favourites, kinship, day.saturating_add(2));
        let fav_keys: BTreeSet<_> = favourites
            .iter()
            .filter_map(|name| artist_key(name))
            .collect();
        let (primary, related): (Vec<_>, Vec<_>) = seeds
            .into_iter()
            .partition(|name| artist_key(name).is_some_and(|key| fav_keys.contains(&key)));
        blueprints.push(ExploreBlueprint {
            id: "favourites-expansion".to_owned(),
            kind: ExplorePlaylistKind::FavouritesExpansion,
            name: words::explore::favourites_expansion().to_owned(),
            subtitle: words::explore::favourites_expansion_reason().to_owned(),
            primary_seeds: primary,
            related_seeds: related,
        });
    }

    let left_behind = deduplicated_artists(&profile.left_behind_artists);
    if !left_behind.is_empty() {
        let seeds = explore_seeds(&left_behind, kinship, day);
        let left_keys: BTreeSet<_> = left_behind
            .iter()
            .filter_map(|name| artist_key(name))
            .collect();
        let (primary, related): (Vec<_>, Vec<_>) = seeds
            .into_iter()
            .partition(|name| artist_key(name).is_some_and(|key| left_keys.contains(&key)));
        blueprints.push(ExploreBlueprint {
            id: "rediscover-beyond".to_owned(),
            kind: ExplorePlaylistKind::RediscoverBeyond,
            name: words::explore::rediscover_beyond().to_owned(),
            subtitle: words::explore::rediscover_beyond_reason().to_owned(),
            primary_seeds: primary,
            related_seeds: related,
        });
    }

    blueprints
}

pub fn weave_mix<T>(
    local: Vec<T>,
    local_artist: impl Fn(&T) -> Option<&str>,
    suggestions: Vec<ExploreOffer>,
) -> Vec<MixEntry<T>> {
    if suggestions.is_empty() {
        return local.into_iter().map(MixEntry::Local).collect();
    }
    if local.is_empty() {
        return suggestions
            .into_iter()
            .map(|offer| MixEntry::Suggested(Box::new(offer)))
            .collect();
    }
    let cadence = if local.len() >= MEDIUM_MIX_AT_LEAST {
        LOCAL_PER_SUGGESTION
    } else if local.len() >= SHORT_MIX_AT_LEAST {
        SHORT_MIX_LOCAL_PER_SUGGESTION
    } else {
        1
    };

    let mut remaining: VecDeque<ExploreOffer> = suggestions.into_iter().collect();
    let mut woven = Vec::with_capacity(local.len() + remaining.len());
    let local_keys: Vec<Option<String>> = local
        .iter()
        .map(|track| local_artist(track).and_then(artist_key))
        .collect();

    for (index, track) in local.into_iter().enumerate() {
        let prev_key = local_keys[index].as_deref();
        let next_key = local_keys.get(index + 1).and_then(Option::as_deref);
        woven.push(MixEntry::Local(track));
        if (index + 1) % cadence == 0
            && !remaining.is_empty()
            && let Some(chosen) = take_spaced_offer(&mut remaining, prev_key, next_key)
        {
            woven.push(MixEntry::Suggested(Box::new(chosen)));
        }
    }

    woven
}

fn take_spaced_offer(
    remaining: &mut VecDeque<ExploreOffer>,
    prev_artist: Option<&str>,
    next_artist: Option<&str>,
) -> Option<ExploreOffer> {
    let key_of = |offer: &ExploreOffer| offer.song.artist.as_deref().and_then(artist_key);
    let ideal = remaining.iter().position(|offer| {
        let key = key_of(offer);
        key.as_deref() != prev_artist && key.as_deref() != next_artist
    });
    let away_from_prev = ideal.or_else(|| {
        remaining
            .iter()
            .position(|offer| key_of(offer).as_deref() != prev_artist)
    });
    let at = away_from_prev.unwrap_or(0);
    remaining.remove(at)
}

fn rotate(artists: &mut [String], day: i64) {
    if !artists.is_empty() {
        let by = day.rem_euclid(artists.len() as i64) as usize;
        artists.rotate_left(by);
    }
}

pub const EXPLORE_CACHE_DAYS: i64 = 7;

pub trait ExploreStore {
    fn read_cached(
        &self,
        catalog: &str,
        scheme: &str,
        artist_key: &str,
        earliest_day: i64,
        latest_day: i64,
    ) -> Option<(i64, Vec<Song>)>;

    fn keep_cached(&self, catalog: &str, scheme: &str, artist_key: &str, day: i64, songs: &[Song]);

    fn forget_before(&self, earliest_day: i64, latest_day: i64);
}

fn is_fresh(cached_day: i64, today: i64) -> bool {
    cached_day <= today && today.saturating_sub(cached_day) < EXPLORE_CACHE_DAYS
}

fn earliest_cached_day(today: i64) -> i64 {
    today.saturating_sub(EXPLORE_CACHE_DAYS - 1)
}

#[derive(Default)]
pub struct ExploreCache {
    artists: Mutex<VecDeque<CachedArtist>>,
    searching: Mutex<()>,
}

struct CachedArtist {
    day: i64,
    artist_key: String,
    source: SongCatalog,
    songs: Arc<[ExploreOffer]>,
}

pub struct Explored {
    songs: Vec<Vec<ExploreOffer>>,
    services: usize,
    failed: usize,
}

pub struct HydratedBlueprints {
    blueprints: Vec<ExploreBlueprint>,
    by_artist: BTreeMap<String, Vec<ExploreOffer>>,
    services: usize,
    failed: usize,
}

impl ExploreCache {
    pub fn find(
        &self,
        day: i64,
        seeds: Vec<String>,
        sources: &[SongCatalog],
        refresh: bool,
    ) -> Arc<Explored> {
        self.find_in(day, seeds, sources, refresh, None)
    }

    pub fn find_with(
        &self,
        day: i64,
        seeds: Vec<String>,
        sources: &[SongCatalog],
        refresh: bool,
        store: &dyn ExploreStore,
    ) -> Arc<Explored> {
        self.find_in(day, seeds, sources, refresh, Some(store))
    }

    fn find_in(
        &self,
        day: i64,
        seeds: Vec<String>,
        sources: &[SongCatalog],
        refresh: bool,
        store: Option<&dyn ExploreStore>,
    ) -> Arc<Explored> {
        let (by_artist, failed) = self.fetch_unique_artists(day, &seeds, sources, refresh, store);
        let songs = seeds
            .iter()
            .map(|artist| {
                artist_key(artist)
                    .and_then(|key| by_artist.get(&key).cloned())
                    .unwrap_or_default()
            })
            .collect();
        Arc::new(Explored {
            songs,
            services: sources.len(),
            failed,
        })
    }

    pub fn hydrate_blueprints(
        &self,
        day: i64,
        blueprints: Vec<ExploreBlueprint>,
        sources: &[SongCatalog],
        refresh: bool,
    ) -> HydratedBlueprints {
        self.hydrate_blueprints_in(day, blueprints, sources, refresh, None)
    }

    pub fn hydrate_blueprints_with(
        &self,
        day: i64,
        blueprints: Vec<ExploreBlueprint>,
        sources: &[SongCatalog],
        refresh: bool,
        store: &dyn ExploreStore,
    ) -> HydratedBlueprints {
        self.hydrate_blueprints_in(day, blueprints, sources, refresh, Some(store))
    }

    fn hydrate_blueprints_in(
        &self,
        day: i64,
        blueprints: Vec<ExploreBlueprint>,
        sources: &[SongCatalog],
        refresh: bool,
        store: Option<&dyn ExploreStore>,
    ) -> HydratedBlueprints {
        let mut unique = Vec::new();
        let mut seen = BTreeSet::new();
        for blueprint in &blueprints {
            for seed in blueprint.all_seeds() {
                if unique.len() >= UNIQUE_SEEDS_AT_MOST {
                    break;
                }
                if let Some(key) = artist_key(seed)
                    && seen.insert(key)
                {
                    unique.push(seed.to_owned());
                }
            }
        }
        let (by_artist, failed) = self.fetch_unique_artists(day, &unique, sources, refresh, store);
        HydratedBlueprints {
            blueprints,
            by_artist,
            services: sources.len(),
            failed,
        }
    }

    fn cached_for(
        &self,
        day: i64,
        artist_key: &str,
        source: &SongCatalog,
    ) -> Option<Arc<[ExploreOffer]>> {
        self.artists
            .lock()
            .iter()
            .find(|entry| {
                is_fresh(entry.day, day)
                    && entry.artist_key == artist_key
                    && entry.source.same_service(source)
            })
            .map(|entry| Arc::clone(&entry.songs))
    }

    fn cached_or_stored(
        &self,
        day: i64,
        artist_key: &str,
        source: &SongCatalog,
        store: Option<&dyn ExploreStore>,
    ) -> Option<Arc<[ExploreOffer]>> {
        if let Some(found) = self.cached_for(day, artist_key, source) {
            return Some(found);
        }
        let store = store?;
        let service = source.service();
        let (cached_day, songs) = store.read_cached(
            source.namespace(),
            service.scheme().as_str(),
            artist_key,
            earliest_cached_day(day),
            day,
        )?;
        let offers: Arc<[ExploreOffer]> = songs
            .into_iter()
            .map(|song| ExploreOffer {
                catalog: source.namespace().to_owned(),
                service: service.title().to_owned(),
                location: service.location_of(&Named::Song(song.id.clone())),
                song,
            })
            .collect();
        self.store_artist(
            cached_day,
            artist_key.to_owned(),
            source.clone(),
            Arc::clone(&offers),
        );
        Some(offers)
    }

    fn store_artist(
        &self,
        day: i64,
        artist_key: String,
        source: SongCatalog,
        songs: Arc<[ExploreOffer]>,
    ) {
        let mut artists = self.artists.lock();
        artists.retain(|entry| {
            !(entry.artist_key == artist_key && entry.source.same_service(&source))
        });
        artists.push_front(CachedArtist {
            day,
            artist_key,
            source,
            songs,
        });
        artists.truncate(CACHE_ARTISTS);
    }

    fn fetch_unique_artists(
        &self,
        day: i64,
        seeds: &[String],
        sources: &[SongCatalog],
        refresh: bool,
        store: Option<&dyn ExploreStore>,
    ) -> (BTreeMap<String, Vec<ExploreOffer>>, usize) {
        let mut by_artist: BTreeMap<String, Vec<ExploreOffer>> = BTreeMap::new();
        let mut missing_any = false;
        if !refresh {
            for source in sources {
                for artist in seeds {
                    let Some(key) = artist_key(artist) else {
                        continue;
                    };
                    if let Some(found) = self.cached_or_stored(day, &key, source, store) {
                        by_artist
                            .entry(key)
                            .or_default()
                            .extend(found.iter().cloned());
                    } else {
                        missing_any = true;
                    }
                }
            }
            if !missing_any {
                return (by_artist, 0);
            }
        }

        let _searching = self.searching.lock();
        if let Some(store) = store {
            store.forget_before(earliest_cached_day(day), day);
        }
        by_artist.clear();
        let mut failed = 0;
        for source in sources {
            let service = source.service();
            for artist in seeds {
                let Some(key) = artist_key(artist) else {
                    continue;
                };
                if !refresh && let Some(found) = self.cached_or_stored(day, &key, source, store) {
                    by_artist
                        .entry(key)
                        .or_default()
                        .extend(found.iter().cloned());
                    continue;
                }
                match service.search(artist) {
                    Ok(found) => {
                        let songs: Vec<Song> = found
                            .songs
                            .into_iter()
                            .filter(|song| {
                                song.artist.as_deref().is_some_and(|named| {
                                    title_words(named) == key || title_words(led_by(named)) == key
                                })
                            })
                            .take(CANDIDATES_PER_SEARCH)
                            .collect();
                        if let Some(store) = store {
                            store.keep_cached(
                                source.namespace(),
                                service.scheme().as_str(),
                                &key,
                                day,
                                &songs,
                            );
                        }
                        let matched: Arc<[ExploreOffer]> = songs
                            .into_iter()
                            .map(|song| ExploreOffer {
                                catalog: source.namespace().to_owned(),
                                service: service.title().to_owned(),
                                location: service.location_of(&Named::Song(song.id.clone())),
                                song,
                            })
                            .collect();
                        by_artist
                            .entry(key.clone())
                            .or_default()
                            .extend(matched.iter().cloned());
                        self.store_artist(day, key, source.clone(), matched);
                    }
                    Err(error) => {
                        tracing::debug!(%error, provider = %service.scheme(), "an explore search did not answer");
                        failed += 1;
                        break;
                    }
                }
            }
        }
        (by_artist, failed)
    }
}

impl Explored {
    pub fn artists(&self) -> BTreeSet<&str> {
        self.songs
            .iter()
            .flatten()
            .filter_map(|offer| offer.song.artist.as_deref())
            .collect()
    }

    pub fn select(&self, held: &[ExploreTrack]) -> ExploreResult {
        let songs = select_spaced(&self.songs, held, &[], SONGS_PER_ARTIST, usize::MAX);
        ExploreResult {
            songs,
            services: self.services,
            failed: self.failed,
        }
    }
}

impl HydratedBlueprints {
    pub fn artists(&self) -> BTreeSet<&str> {
        self.by_artist
            .values()
            .flatten()
            .filter_map(|offer| offer.song.artist.as_deref())
            .collect()
    }

    pub fn curate(&self, held: &[ExploreTrack]) -> ExplorePlaylists {
        let mut playlists = Vec::new();
        let mut used_across: Vec<ExploreTrack> = Vec::new();

        for blueprint in &self.blueprints {
            let mut groups: Vec<(Vec<ExploreOffer>, usize)> = Vec::new();
            for seed in &blueprint.primary_seeds {
                if let Some(key) = artist_key(seed)
                    && let Some(offers) = self.by_artist.get(&key)
                {
                    groups.push((offers.clone(), SONGS_PER_PRIMARY_ARTIST));
                }
            }
            for seed in &blueprint.related_seeds {
                if let Some(key) = artist_key(seed)
                    && let Some(offers) = self.by_artist.get(&key)
                {
                    groups.push((offers.clone(), SONGS_PER_ARTIST));
                }
            }

            let songs = curate_groups(&groups, held, &used_across, PLAYLIST_SONGS_AT_MOST);
            if songs.is_empty() {
                continue;
            }
            for offer in &songs {
                used_across.push(ExploreTrack::of(&offer.song));
            }
            let length = total_duration(&songs);
            playlists.push(ExplorePlaylist {
                id: blueprint.id.clone(),
                kind: blueprint.kind,
                name: blueprint.name.clone(),
                subtitle: blueprint.subtitle.clone(),
                songs,
                length,
            });
        }

        ExplorePlaylists {
            playlists,
            services: self.services,
            failed: self.failed,
        }
    }
}

fn curate_groups(
    groups: &[(Vec<ExploreOffer>, usize)],
    held: &[ExploreTrack],
    prefer_avoid: &[ExploreTrack],
    cap: usize,
) -> Vec<ExploreOffer> {
    let raw_groups: Vec<Vec<ExploreOffer>> = groups.iter().map(|(g, _)| g.clone()).collect();
    let max_per = groups
        .iter()
        .map(|(_, max)| *max)
        .max()
        .unwrap_or(SONGS_PER_ARTIST);
    let fresh = select_spaced(&raw_groups, held, prefer_avoid, max_per, cap);
    if !fresh.is_empty() || prefer_avoid.is_empty() {
        return fresh;
    }
    select_spaced(&raw_groups, held, &[], max_per, cap)
}

fn select_spaced(
    groups: &[Vec<ExploreOffer>],
    held: &[ExploreTrack],
    avoid: &[ExploreTrack],
    per_group: usize,
    cap: usize,
) -> Vec<ExploreOffer> {
    let mut songs = Vec::new();
    let mut accepted: Vec<ExploreTrack> = Vec::new();
    let mut located = BTreeSet::new();
    let mut albums = BTreeMap::<String, usize>::new();
    let mut iterators: Vec<_> = groups.iter().map(|group| group.iter()).collect();
    for _ in 0..per_group {
        for group in &mut iterators {
            if songs.len() >= cap {
                return songs;
            }
            let chosen = group.find_map(|candidate| {
                let track = ExploreTrack::of(&candidate.song);
                let album_key = candidate
                    .song
                    .album
                    .as_deref()
                    .map(title_words)
                    .filter(|key| !key.is_empty());
                let album_room = album_key.as_ref().is_none_or(|key| {
                    albums.get(key).copied().unwrap_or(0) < SONGS_PER_ALBUM_AT_MOST
                });
                (!track.title.is_empty()
                    && album_room
                    && !held
                        .iter()
                        .chain(avoid)
                        .chain(&accepted)
                        .any(|copy| track.is_the_song(copy))
                    && located.insert(&candidate.location))
                .then_some((candidate, track, album_key))
            });
            if let Some((candidate, track, album_key)) = chosen {
                if let Some(key) = album_key {
                    *albums.entry(key).or_default() += 1;
                }
                accepted.push(track);
                songs.push(candidate.clone());
            }
        }
    }
    songs
}

fn total_duration(songs: &[ExploreOffer]) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut any = false;
    for song in songs {
        if let Some(length) = song.song.length {
            total += length;
            any = true;
        }
    }
    any.then_some(total)
}

impl Library {
    pub fn explore_cover(&self, offer: &ExploreOffer, side: NonZeroU32) -> Option<Vec<u8>> {
        let today = Calendar::local().day_of(SystemTime::now());
        let (named, cover) = offer.picture_key();
        let read = self.inner().read(|connection| {
            connection
                .query_row(
                    "SELECT picture FROM explore_covers
                     WHERE catalog = ?1 AND scheme = ?2 AND named = ?3 AND cover = ?4
                       AND side = ?5 AND day BETWEEN ?6 AND ?7",
                    params![
                        offer.catalog,
                        offer.location.source().as_str(),
                        named,
                        cover,
                        side.get(),
                        earliest_cached_day(today),
                        today
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        });
        match read {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::debug!(%error, "a kept explore cover could not be read");
                None
            }
        }
    }

    pub fn keep_explore_cover(&self, offer: &ExploreOffer, side: NonZeroU32, picture: &[u8]) {
        if picture.is_empty() || picture.len() > EXPLORE_COVER_BYTES_AT_MOST {
            return;
        }
        let today = Calendar::local().day_of(SystemTime::now());
        let (named, cover) = offer.picture_key();
        let kept = self.inner().write(|transaction| {
            transaction
                .execute(
                    "INSERT OR REPLACE INTO explore_covers
                       (catalog, scheme, named, cover, side, day, picture)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        offer.catalog,
                        offer.location.source().as_str(),
                        named,
                        cover,
                        side.get(),
                        today,
                        picture
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            transaction
                .execute(
                    "DELETE FROM explore_covers
                     WHERE day NOT BETWEEN ?1 AND ?2 OR id IN (
                       SELECT id FROM (
                         SELECT id, row_number() OVER recent AS position,
                                sum(length(picture)) OVER recent AS bytes
                         FROM explore_covers WHERE day BETWEEN ?1 AND ?2
                         WINDOW recent AS (ORDER BY id DESC)
                       ) WHERE position > ?3 OR bytes > ?4
                     )",
                    params![
                        earliest_cached_day(today),
                        today,
                        EXPLORE_COVERS_HELD,
                        EXPLORE_COVER_BUDGET
                    ],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        });
        if let Err(error) = kept {
            tracing::debug!(%error, "an explore cover could not be kept");
        }
    }

    pub fn explore_blueprints(&self) -> Result<Vec<ExploreBlueprint>> {
        let now = SystemTime::now();
        let day = Calendar::local().day_of(now);
        let filing = self.inner().filing()?;
        let profile = self.explore_profile(now, &filing)?;
        Ok(explore_blueprints(&profile, filing.kinship(), day))
    }

    pub fn explore_playlists(
        &self,
        sources: &[SongCatalog],
        cache: &ExploreCache,
        refresh: bool,
    ) -> Result<ExplorePlaylists> {
        if sources.is_empty() {
            return Ok(ExplorePlaylists::default());
        }
        let now = SystemTime::now();
        let day = Calendar::local().day_of(now);
        let filing = self.inner().filing()?;
        let profile = self.explore_profile(now, &filing)?;
        let blueprints = explore_blueprints(&profile, filing.kinship(), day);
        let hydrated = cache.hydrate_blueprints_with(day, blueprints, sources, refresh, self);
        let held: Vec<_> = self
            .explore_tracks(hydrated.artists())?
            .into_iter()
            .map(|(_, track)| track)
            .collect();
        Ok(hydrated.curate(&held))
    }

    pub fn explore_for_artists(
        &self,
        direct: &[String],
        sources: &[SongCatalog],
        cache: &ExploreCache,
        refresh: bool,
    ) -> Result<ExploreResult> {
        if sources.is_empty() {
            return Ok(ExploreResult::default());
        }
        let now = SystemTime::now();
        let day = Calendar::local().day_of(now);
        let filing = self.inner().filing()?;
        let seeds = explore_seeds(direct, filing.kinship(), day);
        let found = cache.find_with(day, seeds, sources, refresh, self);
        let held: Vec<_> = self
            .explore_tracks(found.artists())?
            .into_iter()
            .map(|(_, track)| track)
            .collect();
        Ok(found.select(&held))
    }

    pub fn explore(
        &self,
        mix: Option<&SavedQuery>,
        sources: &[SongCatalog],
        cache: &ExploreCache,
        refresh: bool,
    ) -> Result<ExploreResult> {
        if sources.is_empty() {
            return Ok(ExploreResult::default());
        }
        let now = SystemTime::now();
        let direct = match mix {
            Some(query) => read_mix_artists(self, query)?,
            None => self.explore_artists(now)?,
        };
        self.explore_for_artists(&direct, sources, cache, refresh)
    }

    pub(crate) fn held_explored(
        &self,
        song: &ExploreTrack,
        artist: Option<&str>,
    ) -> Result<Option<TrackId>> {
        let Some(artist) = artist else {
            return Ok(None);
        };
        let rows = self.explore_tracks([artist])?;
        Ok(rows
            .into_iter()
            .find_map(|(id, track)| song.is_the_song(&track).then_some(id)))
    }

    fn explore_profile(&self, now: SystemTime, filing: &crate::Filing) -> Result<ExploreProfile> {
        let (recent_artists, played_genres) = suggest::what_you_play(self.inner(), now)?;
        let favourites = self.favourite_tracks(&TrackQuery::default())?;
        let favourite_artists = artists_for_explore(
            favourites
                .iter()
                .filter_map(|track| track.artist.as_deref()),
        );
        let mut heard = suggest::the_artists_as_heard(self.inner())?;
        let half_a_year_ago = now.checked_sub(HALF_A_YEAR).unwrap_or(UNIX_EPOCH);
        let left_behind_artists: Vec<String> = suggest::left_behind_of(&heard, half_a_year_ago)
            .into_iter()
            .take(ROTATING_ARTISTS_AT_MOST)
            .map(str::to_owned)
            .collect();
        heard.sort_by(|one, other| {
            other
                .held
                .cmp(&one.held)
                .then_with(|| one.name.cmp(&other.name))
        });
        let top_artists = heard
            .into_iter()
            .take(ROTATING_ARTISTS_AT_MOST)
            .map(|artist| artist.name)
            .collect();
        let top_genres = if !played_genres.is_empty() {
            played_genres
        } else {
            filing.genres().to_vec()
        };
        Ok(ExploreProfile {
            recent_artists,
            favourite_artists,
            top_artists,
            left_behind_artists,
            top_genres,
        })
    }

    fn explore_tracks<'a>(
        &self,
        artists: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<(TrackId, ExploreTrack)>> {
        let keys: BTreeSet<_> = artists.into_iter().map(store::folded_letters).collect();
        self.inner().read(|connection| {
            let mut tracks = Vec::new();
            let mut query = connection
                .prepare_cached(
                    "SELECT t.id, t.title, coalesce(t.artist, a.name), t.duration, t.sample_rate
                 FROM tracks t JOIN artists a ON a.id = t.artist_id
                 WHERE a.key = ?1",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            for key in keys {
                let read = query
                    .query_map(params![key], |row| {
                        let id: i64 = row.get(0)?;
                        let title: String = row.get(1)?;
                        let artist: String = row.get(2)?;
                        let frames: Option<i64> = row.get(3)?;
                        let rate: u32 = row.get(4)?;
                        Ok((
                            id,
                            ExploreTrack::named(
                                &title,
                                &artist,
                                frames.and_then(|frames| {
                                    (frames >= 0 && rate > 0).then(|| {
                                        Duration::from_secs_f64(frames as f64 / f64::from(rate))
                                    })
                                }),
                            ),
                        ))
                    })
                    .map_err(|source| Error::store(StoreOp::Query, source))?;
                for track in read {
                    let (id, track) =
                        track.map_err(|source| Error::store(StoreOp::Query, source))?;
                    tracks.push((TrackId::new(id.cast_unsigned())?, track));
                }
            }
            Ok(tracks)
        })
    }

    fn explore_artists(&self, now: SystemTime) -> Result<Vec<String>> {
        let heard = suggest::heard_of_late(self.inner(), now)?;
        if !heard.is_empty() {
            return Ok(heard);
        }
        let favourites = self.favourite_tracks(&TrackQuery::default())?;
        let artists = artists_for_explore(
            favourites
                .iter()
                .filter_map(|track| track.artist.as_deref()),
        );
        if !artists.is_empty() {
            return Ok(artists);
        }
        let mut heard = suggest::the_artists_as_heard(self.inner())?;
        heard.sort_by(|one, other| {
            other
                .held
                .cmp(&one.held)
                .then_with(|| one.name.cmp(&other.name))
        });
        Ok(heard
            .into_iter()
            .take(ROTATING_ARTISTS_AT_MOST)
            .map(|artist| artist.name)
            .collect())
    }
}

pub(crate) fn served_song_location(source: &str, served_id: &str) -> Option<MediaLocation> {
    let source = SourceId::new(source).ok()?;
    let id = resonate_providers::Id::new(served_id)?;
    Some(Named::Song(id).location(&source))
}

fn read_mix_artists(library: &Library, query: &SavedQuery) -> Result<Vec<String>> {
    let tracks = library.tracks(&TrackQuery::from(query))?;
    Ok(artists_for_explore(
        tracks.iter().filter_map(|track| track.artist.as_deref()),
    ))
}

const fn rating_code(rating: Rating) -> i64 {
    match rating {
        Rating::Unmarked => 0,
        Rating::Explicit => 1,
        Rating::Clean => 2,
    }
}

const fn rating_of(code: i64) -> Rating {
    match code {
        1 => Rating::Explicit,
        2 => Rating::Clean,
        _ => Rating::Unmarked,
    }
}

impl ExploreStore for Library {
    fn read_cached(
        &self,
        catalog: &str,
        scheme: &str,
        artist_key: &str,
        earliest_day: i64,
        latest_day: i64,
    ) -> Option<(i64, Vec<Song>)> {
        let read = self.inner().read(|connection| {
            let entry: Option<(i64, i64)> = connection
                .query_row(
                    "SELECT id, day FROM explore_artists
                     WHERE catalog = ?1 AND scheme = ?2 AND artist_key = ?3
                       AND day >= ?4 AND day <= ?5",
                    params![catalog, scheme, artist_key, earliest_day, latest_day],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            let Some((artist_id, day)) = entry else {
                return Ok(None);
            };
            let mut query = connection
                .prepare_cached(
                    "SELECT song_id, title, artist, album, disc, track, length_ms,
                            suffix, cover, favourite, rating
                     FROM explore_songs
                     WHERE artist_id = ?1
                     ORDER BY position",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let rows = query
                .query_map(params![artist_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                        row.get::<_, i64>(10)?,
                    ))
                })
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            let mut songs = Vec::new();
            for row in rows {
                let (
                    song_id,
                    title,
                    artist,
                    album,
                    disc,
                    track,
                    length_ms,
                    suffix,
                    cover,
                    favourite,
                    rating,
                ) = row.map_err(|source| Error::store(StoreOp::Query, source))?;
                let Some(id) = Id::new(&song_id) else {
                    continue;
                };
                songs.push(Song {
                    id,
                    title,
                    artist,
                    album,
                    disc: disc.and_then(|value| u32::try_from(value).ok()),
                    track: track.and_then(|value| u32::try_from(value).ok()),
                    length: length_ms
                        .and_then(|value| u64::try_from(value).ok())
                        .map(Duration::from_millis),
                    suffix,
                    cover: cover.as_deref().and_then(Id::new),
                    favourite: favourite.map(|value| value != 0),
                    rating: rating_of(rating),
                });
            }
            Ok(Some((day, songs)))
        });
        match read {
            Ok(cached) => cached,
            Err(error) => {
                tracing::debug!(%error, "an explore cache entry could not be read");
                None
            }
        }
    }

    fn keep_cached(&self, catalog: &str, scheme: &str, artist_key: &str, day: i64, songs: &[Song]) {
        let written = self.inner().write(|transaction| {
            transaction
                .execute(
                    "DELETE FROM explore_artists
                     WHERE catalog = ?1 AND scheme = ?2 AND artist_key = ?3",
                    params![catalog, scheme, artist_key],
                )
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
            transaction
                .execute(
                    "INSERT INTO explore_artists (catalog, scheme, artist_key, day)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![catalog, scheme, artist_key, day],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            let artist_id = transaction.last_insert_rowid();
            let mut insert = transaction
                .prepare_cached(
                    "INSERT INTO explore_songs
                        (artist_id, position, song_id, title, artist, album, disc,
                         track, length_ms, suffix, cover, favourite, rating)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            for (position, song) in songs.iter().enumerate() {
                let length_ms = song
                    .length
                    .and_then(|length| i64::try_from(length.as_millis()).ok());
                insert
                    .execute(params![
                        artist_id,
                        position as i64,
                        song.id.as_str(),
                        song.title,
                        song.artist,
                        song.album,
                        song.disc.map(i64::from),
                        song.track.map(i64::from),
                        length_ms,
                        song.suffix,
                        song.cover.as_ref().map(Id::as_str),
                        song.favourite.map(i64::from),
                        rating_code(song.rating),
                    ])
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }
            Ok(())
        });
        if let Err(error) = written {
            tracing::debug!(%error, "an explore cache entry could not be kept");
        }
    }

    fn forget_before(&self, earliest_day: i64, latest_day: i64) {
        let pruned = self.inner().write(|transaction| {
            transaction
                .execute(
                    "DELETE FROM explore_artists WHERE day < ?1 OR day > ?2",
                    params![earliest_day, latest_day],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        });
        if let Err(error) = pruned {
            tracing::debug!(%error, "stale explore cache entries could not be pruned");
        }
    }
}
