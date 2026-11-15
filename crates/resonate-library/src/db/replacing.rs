use ahash::AHashSet;
use resonate_core::{PlaylistId, ReleaseTrackId, TrackId};
use rusqlite::{Connection, OptionalExtension as _, params, types::Value};

use super::{Library, rows::rows};
use crate::{
    Error, Quality, Replaceable, Result, SongHeard, StoreOp, TheSong, Track, TrackQuery, Version,
};

const HELD_WEIGHED_AT_MOST: usize = 64;

const THE_SAME_SONG_ELSEWHERE: &str = "SELECT other.id,
            (lossy.mbid IS NOT NULL AND other.mbid = lossy.mbid)
       FROM tracks lossy
       JOIN tracks other ON other.id != lossy.id
        AND ((lossy.mbid IS NOT NULL AND other.mbid = lossy.mbid)
          OR other.id = lossy.alternative_of
          OR (lossy.alternative_of IS NOT NULL AND other.alternative_of = lossy.alternative_of))
      WHERE lossy.id = ?1";

const THE_PLAYLISTS_HOLDING_A_LOSSY_SONG: &str = concat!(
    "SELECT DISTINCT e.playlist_id FROM playlist_entries e JOIN tracks ON ",
    "tracks.path = e.path AND tracks.span_start = e.span_start",
    " WHERE tracks.span_frames IS NULL AND NOT (",
    lossless_codecs!(),
    ")"
);

impl Library {
    pub fn lossy_songs_of(&self, playlist: PlaylistId) -> Result<Vec<Replaceable>> {
        let mut seen = AHashSet::new();
        let mut found = Vec::new();
        for entry in self.playlist_entries(playlist, None)? {
            let Some(track) = entry.track else { continue };
            if track.span.is_some() || track.codec.is_lossless() || !seen.insert(track.id) {
                continue;
            }
            let held = self.lossless_copy_held_for(&track)?;
            let (release_track, pending) = self
                .inner
                .read(|connection| release_track_and_pending(connection, track.id))?;
            found.push(Replaceable {
                track,
                held,
                release_track,
                pending,
            });
        }
        Ok(found)
    }

    pub fn playlists_with_lossy_songs(&self) -> Result<AHashSet<PlaylistId>> {
        let ids = self.inner.read(|connection| {
            rows(
                connection,
                THE_PLAYLISTS_HOLDING_A_LOSSY_SONG,
                Vec::new(),
                |row| row.get::<_, i64>(0).map(Ok),
            )
        })?;
        ids.into_iter()
            .map(|id| Ok(PlaylistId::new(id.cast_unsigned())?))
            .collect()
    }

    fn lossless_copy_held_for(&self, lossy: &Track) -> Result<Option<Track>> {
        let song = TheSong::of(lossy);
        let related = self
            .inner
            .read(|connection| related_ids(connection, lossy.id))?;
        let mut candidates = self.alternatives_of(lossy.id)?;
        let found = self.tracks(&TrackQuery {
            text: Some(TheSong::words(&lossy.title, lossy.artist.as_deref())),
            limit: Some(HELD_WEIGHED_AT_MOST),
            ..TrackQuery::default()
        })?;
        candidates.extend(self.tracks_with_ids(&related.ids)?);
        candidates.extend(found);

        let mut offered: Vec<Track> = Vec::new();
        for copy in candidates {
            let fits = copy.span.is_none()
                && copy.id != lossy.id
                && !offered.iter().any(|held| held.id == copy.id)
                && (song.is_bettered_by(SongHeard::of(&copy))
                    || (related.sharing.contains(&copy.id) && copy.codec.is_lossless()));
            if fits {
                offered.push(copy);
            }
        }
        offered.sort_by_key(|copy| Quality::of(copy).best_first());

        let chosen = match self.version_chosen_for(lossy.id)? {
            Some(Version::Held(copy)) => Some(copy),
            Some(Version::Served(_)) | None => None,
        };
        let position = chosen
            .and_then(|chosen| offered.iter().position(|copy| copy.id == chosen))
            .unwrap_or(0);
        Ok((!offered.is_empty()).then(|| offered.swap_remove(position)))
    }
}

struct Related {
    ids: Vec<TrackId>,
    sharing: AHashSet<TrackId>,
}

fn related_ids(connection: &Connection, track: TrackId) -> Result<Related> {
    let found = rows(
        connection,
        THE_SAME_SONG_ELSEWHERE,
        vec![Value::Integer(track.get() as i64)],
        |row| Ok(Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))),
    )?;
    let mut related = Related {
        ids: Vec::new(),
        sharing: AHashSet::new(),
    };
    for (id, shares) in found {
        let id = TrackId::new(id.cast_unsigned())?;
        related.ids.push(id);
        if shares {
            related.sharing.insert(id);
        }
    }
    Ok(related)
}

fn release_track_and_pending(
    connection: &Connection,
    track: TrackId,
) -> Result<(Option<ReleaseTrackId>, bool)> {
    let at = track.get() as i64;
    let release_track: Option<i64> = connection
        .query_row(
            "SELECT id FROM release_tracks WHERE track_id = ?1",
            params![at],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let pending: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM replacements WHERE track_id = ?1)",
            params![at],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let release_track = release_track
        .map(|id| ReleaseTrackId::new(id.cast_unsigned()))
        .transpose()?;
    Ok((release_track, pending))
}
