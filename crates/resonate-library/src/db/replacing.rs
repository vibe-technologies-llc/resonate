use std::time::SystemTime;

use ahash::AHashSet;
use resonate_core::{PlaylistId, ReleaseTrackId, TrackId};
use rusqlite::{Connection, OptionalExtension as _, Transaction, params, types::Value};

use super::{Library, rows::rows};
use crate::{
    Error, Quality, Replaceable, Replacing, Result, SongHeard, StoreOp, TheSong, Track, TrackQuery,
    Version, deleted::Why, elsewhere, replacing, store,
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

const HOW_A_WHOLE_FILE_STANDS: &str = concat!(
    "SELECT tracks.path, (",
    lossless_codecs!(),
    "), tracks.vault_key IS NOT NULL FROM tracks",
    " WHERE tracks.id = ?1 AND tracks.span_start = 0 AND tracks.span_frames IS NULL"
);

const THE_RELEASE_ROW_IS_STILL_THE_TRACKS: &str =
    "SELECT EXISTS (SELECT 1 FROM release_tracks WHERE id = ?1 AND track_id = ?2)";

const A_LOSSY_SONG_WAITS: &str =
    "INSERT OR REPLACE INTO replacements (track_id, release_track_id, asked) VALUES (?1, ?2, ?3)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    Gone,
    Lossy,
    Lossless,
}

struct Settled {
    replacing: Replacing,
    taken: Vec<TrackId>,
}

impl Library {
    pub fn replace(&self, chosen: &[Replaceable]) -> Result<Replacing> {
        let walk = self.walk_the_tree()?;
        let now = SystemTime::now();
        let Settled {
            mut replacing,
            taken,
        } = self
            .inner
            .write(|transaction| settled(transaction, chosen, now))?;

        if !taken.is_empty() {
            self.inner.playlists_changed();
            replacing.trashed = self.remove_tracks_under(&walk, &taken, Why::Folded)?;
        }
        Ok(replacing)
    }

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

fn settled(
    transaction: &Transaction<'_>,
    chosen: &[Replaceable],
    now: SystemTime,
) -> Result<Settled> {
    let mut seen = AHashSet::new();
    let mut settled = Settled {
        replacing: Replacing::default(),
        taken: Vec::new(),
    };
    for one in chosen {
        if !seen.insert(one.track.id) || standing(transaction, &one.track)? != Standing::Lossy {
            continue;
        }
        let held = match &one.held {
            Some(copy) if standing(transaction, copy)? == Standing::Lossless => Some(copy.id),
            Some(_) | None => None,
        };
        let release_track = match one.release_track {
            Some(row) if still_held_by(transaction, row, one.track.id)? => Some(row),
            Some(_) | None => None,
        };

        if let Some(kept) = held {
            replacing::folded(transaction, kept, one.track.id)?;
            settled.taken.push(one.track.id);
            settled.replacing.folded += 1;
        } else if let Some(row) = release_track {
            transaction
                .execute(
                    A_LOSSY_SONG_WAITS,
                    params![
                        one.track.id.get() as i64,
                        row.get() as i64,
                        store::to_nanos(now)
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            elsewhere::rearmed(transaction, row, now)?;
            settled.replacing.wanted += 1;
        } else {
            settled.replacing.unmatched += 1;
        }
    }
    Ok(settled)
}

fn standing(connection: &Connection, track: &Track) -> Result<Standing> {
    let Some(named) = track.location.as_path() else {
        return Ok(Standing::Gone);
    };
    let found: Option<(String, bool, bool)> = connection
        .query_row(
            HOW_A_WHOLE_FILE_STANDS,
            params![track.id.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let Some((path, lossless, vaulted)) = found else {
        return Ok(Standing::Gone);
    };
    let path = store::path_of(&path);
    Ok(if path != named {
        Standing::Gone
    } else if !lossless {
        Standing::Lossy
    } else if vaulted || path.is_file() {
        Standing::Lossless
    } else {
        Standing::Gone
    })
}

fn still_held_by(connection: &Connection, row: ReleaseTrackId, track: TrackId) -> Result<bool> {
    connection
        .query_row(
            THE_RELEASE_ROW_IS_STILL_THE_TRACKS,
            params![row.get() as i64, track.get() as i64],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))
}
