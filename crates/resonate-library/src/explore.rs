use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{Calendar, MediaLocation, TrackId, title_words, titles};
use resonate_providers::{A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY, Named, Song, SongCatalog, led_by};
use rusqlite::params;

use crate::{Error, Kinship, Library, Result, SavedQuery, StoreOp, TrackQuery, store, suggest};

const ARTISTS_AT_MOST: usize = 4;
const RELATED_ARTISTS_FIRST: usize = 2;
const ROTATING_ARTISTS_AT_MOST: usize = 6;
const CANDIDATES_PER_SEARCH: usize = 32;
const SONGS_PER_ARTIST: usize = 3;
const CACHE_ENTRIES: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExploreOffer {
    pub catalog: String,
    pub service: String,
    pub location: MediaLocation,
    pub song: Song,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExploreResult {
    pub songs: Vec<ExploreOffer>,
    pub services: usize,
    pub failed: usize,
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

fn rotate(artists: &mut [String], day: i64) {
    if !artists.is_empty() {
        let by = day.rem_euclid(artists.len() as i64) as usize;
        artists.rotate_left(by);
    }
}

#[derive(Default)]
pub struct ExploreCache {
    cached: Mutex<VecDeque<Cached>>,
    searching: Mutex<()>,
}

struct Cached {
    day: i64,
    seeds: Vec<String>,
    sources: Vec<SongCatalog>,
    found: Arc<Explored>,
}

pub struct Explored {
    songs: Vec<Vec<ExploreOffer>>,
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
        let answers = |entry: &Cached| {
            entry.day == day
                && entry.seeds == seeds
                && entry.sources.len() == sources.len()
                && entry
                    .sources
                    .iter()
                    .zip(sources)
                    .all(|(one, other)| one.same_service(other))
        };
        let cached = || {
            self.cached
                .lock()
                .iter()
                .find(|entry| answers(entry))
                .map(|entry| Arc::clone(&entry.found))
        };
        if !refresh && let Some(found) = cached() {
            return found;
        }
        let _searching = self.searching.lock();
        if !refresh && let Some(found) = cached() {
            return found;
        }
        let found = Arc::new(Explored::ask(&seeds, sources));
        if found.failed == 0 || found.songs.iter().any(|songs| !songs.is_empty()) {
            let mut cached = self.cached.lock();
            cached.retain(|entry| !answers(entry));
            cached.push_front(Cached {
                day,
                seeds,
                sources: sources.to_vec(),
                found: Arc::clone(&found),
            });
            cached.truncate(CACHE_ENTRIES);
        }
        found
    }
}

impl Explored {
    fn ask(seeds: &[String], sources: &[SongCatalog]) -> Self {
        let mut songs = vec![Vec::new(); seeds.len()];
        let mut failed = 0;
        for source in sources {
            let service = source.service();
            for (artist, songs) in seeds.iter().zip(&mut songs) {
                let key = title_words(artist);
                match service.search(artist) {
                    Ok(found) => songs.extend(
                        found
                            .songs
                            .into_iter()
                            .filter(|song| {
                                song.artist.as_deref().is_some_and(|artist| {
                                    title_words(artist) == key || title_words(led_by(artist)) == key
                                })
                            })
                            .take(CANDIDATES_PER_SEARCH)
                            .map(|song| ExploreOffer {
                                catalog: source.namespace().to_owned(),
                                service: service.title().to_owned(),
                                location: service.location_of(&Named::Song(song.id.clone())),
                                song,
                            }),
                    ),
                    Err(error) => {
                        tracing::debug!(%error, provider = %service.scheme(), "an explore search did not answer");
                        failed += 1;
                        break;
                    }
                }
            }
        }
        Self {
            songs,
            services: sources.len(),
            failed,
        }
    }

    pub fn artists(&self) -> BTreeSet<&str> {
        self.songs
            .iter()
            .flatten()
            .filter_map(|offer| offer.song.artist.as_deref())
            .collect()
    }

    pub fn select(&self, held: &[ExploreTrack]) -> ExploreResult {
        let mut songs = Vec::new();
        let mut accepted: Vec<ExploreTrack> = Vec::new();
        let mut located = BTreeSet::new();
        let mut artists: Vec<_> = self.songs.iter().map(|group| group.iter()).collect();
        for _ in 0..SONGS_PER_ARTIST {
            for artist in &mut artists {
                let chosen = artist.find_map(|candidate| {
                    let track = ExploreTrack::of(&candidate.song);
                    (!track.title.is_empty()
                        && !held
                            .iter()
                            .chain(&accepted)
                            .any(|copy| track.is_the_song(copy))
                        && located.insert(&candidate.location))
                    .then_some((candidate, track))
                });
                if let Some((candidate, track)) = chosen {
                    accepted.push(track);
                    songs.push(candidate.clone());
                }
            }
        }
        ExploreResult {
            songs,
            services: self.services,
            failed: self.failed,
        }
    }
}

impl Library {
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
        let day = Calendar::local().day_of(now);
        let direct = match mix {
            Some(query) => read_mix_artists(self, query)?,
            None => self.explore_artists(now)?,
        };
        let filing = self.inner().filing()?;
        let seeds = explore_seeds(&direct, filing.kinship(), day);
        let found = cache.find(day, seeds, sources, refresh);
        let held: Vec<_> = self
            .explore_tracks(found.artists())?
            .into_iter()
            .map(|(_, track)| track)
            .collect();
        Ok(found.select(&held))
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

fn read_mix_artists(library: &Library, query: &SavedQuery) -> Result<Vec<String>> {
    let tracks = library.tracks(&TrackQuery::from(query))?;
    Ok(artists_for_explore(
        tracks.iter().filter_map(|track| track.artist.as_deref()),
    ))
}
