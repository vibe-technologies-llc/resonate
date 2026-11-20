use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::title_words;
use resonate_library::{
    ExploreTrack, Kinship, OF_LATE, artists_for_explore, artists_heard_lately, explore_seeds,
};

use crate::{
    Error, MobilePlayer, Result, Track,
    catalog_extras::the_listeners_calendar,
    features::{SearchSong, TrackFilter},
    saved_searches::SavedSearch,
    served::ServedSong,
};

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ExploreScope {
    Library,
    Saved { search: SavedSearch },
    Filtered { filter: TrackFilter },
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ExploreSong {
    pub song: ServedSong,
    pub service: String,
    pub download: SearchSong,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ExploreFeed {
    pub songs: Vec<ExploreSong>,
    pub services: u32,
    pub failed: u32,
}

#[uniffi::export]
impl MobilePlayer {
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
            ExploreScope::Library => {
                let since = now
                    .checked_sub(OF_LATE)
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let heard = searching
                    .heard_lately(&self.catalog, i64::try_from(since).unwrap_or(i64::MAX))?;
                let favourite = || {
                    artists_for_explore(
                        index
                            .rows
                            .iter()
                            .filter(|row| row.track.favourite)
                            .map(|row| row.track.artist.as_str()),
                    )
                };
                let collection =
                    || artists_for_explore(index.rows.iter().map(|row| row.track.artist.as_str()));
                let artists = artists_heard_lately(heard);
                if artists.is_empty() {
                    let artists = favourite();
                    if artists.is_empty() {
                        collection()
                    } else {
                        artists
                    }
                } else {
                    artists
                }
            }
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
        let found = self.served.explore.find(day, day_seeds, &sources, refresh);
        let keys: BTreeSet<_> = found.artists().into_iter().map(title_words).collect();
        self.require_online()?;
        let current = searching.index(&self.catalog)?;
        let held: Vec<_> = current
            .rows
            .iter()
            .filter(|row| keys.contains(&title_words(&row.track.artist)))
            .map(|row| explore_track(&row.track))
            .collect();
        let found = found.select(&held);
        let songs = self
            .served
            .explored(&sources, found.songs)?
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
            .collect();
        Ok(ExploreFeed {
            songs,
            services: u32::try_from(found.services).unwrap_or(u32::MAX),
            failed: u32::try_from(found.failed).unwrap_or(u32::MAX),
        })
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

fn explore_track(track: &Track) -> ExploreTrack {
    ExploreTrack::named(
        &track.title,
        &track.artist,
        (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
    )
}
