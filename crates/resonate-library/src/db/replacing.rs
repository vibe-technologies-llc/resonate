use std::{path::PathBuf, time::SystemTime};

use ahash::AHashSet;
use resonate_core::{MediaLocation, PlaylistId, ReleaseTrackId, TrackId};
use rusqlite::{Connection, OptionalExtension as _, Transaction, params, types::Value};

use super::{Library, Walk, rows::rows};
use crate::{
    Deleted, Error, Quality, Replaceable, Replacing, Result, SongHeard, StoreOp, TheSong, Track,
    TrackQuery, Version, deleted::Why, elsewhere, replacing, store,
};

const HELD_WEIGHED_AT_MOST: usize = 64;

const THE_SAME_SONG_ELSEWHERE: &str = "SELECT other.id,
            (lossy.mbid IS NOT NULL AND other.mbid IS lossy.mbid)
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

const ANY_REPLACEMENT_WAITS: &str = "SELECT EXISTS (SELECT 1 FROM replacements)";

const THE_REPLACEMENTS_REJOIN_THE_ROWS_THEIR_SONGS_HOLD: &str = "UPDATE replacements
        SET release_track_id = (SELECT id FROM release_tracks
                                 WHERE release_tracks.track_id = replacements.track_id)
      WHERE release_track_id IS NULL
  RETURNING release_track_id";

const THE_REPLACEMENTS_WAITING: &str = "SELECT r.track_id, w.offered
       FROM replacements r
       LEFT JOIN wants w ON w.release_track_id = r.release_track_id
      ORDER BY r.track_id";

const THE_COPY_DELIVERED: &str = concat!(
    "SELECT tracks.id FROM tracks",
    " WHERE tracks.path = ?1 AND tracks.span_start = 0 AND tracks.span_frames IS NULL",
    " AND tracks.id != ?2 AND (",
    lossless_codecs!(),
    ")"
);

const THE_COPIES_OF_THE_SAME_RECORDING: &str = concat!(
    "SELECT tracks.id FROM replacements r",
    " JOIN tracks lossy ON lossy.id = r.track_id",
    " LEFT JOIN release_tracks rt ON rt.id = r.release_track_id",
    " JOIN tracks ON tracks.mbid IN (lossy.mbid, rt.recording_mbid)",
    " WHERE r.track_id = ?1 AND tracks.id != lossy.id",
    " AND tracks.span_start = 0 AND tracks.span_frames IS NULL AND (",
    lossless_codecs!(),
    ") ORDER BY tracks.id"
);

const THE_REPLACEMENT_STILL_WAITS: &str =
    "SELECT EXISTS (SELECT 1 FROM replacements WHERE track_id = ?1)";

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

#[derive(Clone, Copy)]
struct Evidenced {
    taken: TrackId,
    kept: TrackId,
}

struct WholeFile {
    path: PathBuf,
    lossless: bool,
    vaulted: bool,
}

impl WholeFile {
    fn standing(&self) -> Standing {
        if !self.lossless {
            Standing::Lossy
        } else if self.vaulted || self.path.is_file() {
            Standing::Lossless
        } else {
            Standing::Gone
        }
    }
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

    pub fn settle_replacements(&self) -> Result<Deleted> {
        if !self.inner.read(any_replacement_waits)? {
            return Ok(Deleted::default());
        }
        let walk = match self.walk_the_tree() {
            Ok(walk) => walk,
            Err(Error::AlreadyWalking) => {
                tracing::debug!(
                    "the replacements are settled after the pass walking the tree, or the next"
                );
                return Ok(Deleted::default());
            }
            Err(error) => return Err(error),
        };
        self.rejoin_the_replacements()?;
        self.settled_under(&walk)
    }

    pub(crate) fn rejoin_the_replacements(&self) -> Result<u64> {
        let now = SystemTime::now();
        self.inner.write(|transaction| {
            let rejoined = rows(
                transaction,
                THE_REPLACEMENTS_REJOIN_THE_ROWS_THEIR_SONGS_HOLD,
                Vec::new(),
                |row| row.get::<_, Option<i64>>(0).map(Ok),
            )?;
            let mut joined = 0;
            for row in rejoined.into_iter().flatten() {
                elsewhere::wanted(transaction, row, now)?;
                joined += 1;
            }
            Ok(joined)
        })
    }

    fn settled_under(&self, walk: &Walk) -> Result<Deleted> {
        let evidenced = self.inner.read(evidenced)?;
        if evidenced.is_empty() {
            return Ok(Deleted::default());
        }
        let taken = self
            .inner
            .write(|transaction| folded_on(transaction, &evidenced))?;
        if taken.is_empty() {
            return Ok(Deleted::default());
        }
        self.inner.playlists_changed();
        self.remove_tracks_under(walk, &taken, Why::Folded)
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
            "SELECT EXISTS (SELECT 1 FROM replacements
                             WHERE track_id = ?1 AND release_track_id IS NOT NULL)",
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
    Ok(match whole_file(connection, track.id)? {
        Some(file) if file.path == named => file.standing(),
        Some(_) | None => Standing::Gone,
    })
}

fn standing_of(connection: &Connection, track: TrackId) -> Result<Standing> {
    Ok(whole_file(connection, track)?.map_or(Standing::Gone, |file| file.standing()))
}

fn whole_file(connection: &Connection, track: TrackId) -> Result<Option<WholeFile>> {
    let found: Option<(String, bool, bool)> = connection
        .query_row(
            HOW_A_WHOLE_FILE_STANDS,
            params![track.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(found.map(|(path, lossless, vaulted)| WholeFile {
        path: store::path_of(&path),
        lossless,
        vaulted,
    }))
}

fn any_replacement_waits(connection: &Connection) -> Result<bool> {
    connection
        .query_row(ANY_REPLACEMENT_WAITS, [], |row| row.get(0))
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn evidenced(connection: &Connection) -> Result<Vec<Evidenced>> {
    let waiting = rows(connection, THE_REPLACEMENTS_WAITING, Vec::new(), |row| {
        Ok(Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
        )))
    })?;
    let mut found = Vec::new();
    for (taken, offered) in waiting {
        let taken = TrackId::new(taken.cast_unsigned())?;
        if standing_of(connection, taken)? != Standing::Lossy {
            continue;
        }
        let mut candidates = delivered_for(connection, taken, offered.as_deref())?;
        candidates.extend(rows(
            connection,
            THE_COPIES_OF_THE_SAME_RECORDING,
            vec![Value::Integer(taken.get() as i64)],
            |row| row.get::<_, i64>(0).map(Ok),
        )?);
        for kept in candidates {
            let kept = TrackId::new(kept.cast_unsigned())?;
            if standing_of(connection, kept)? == Standing::Lossless {
                found.push(Evidenced { taken, kept });
                break;
            }
        }
    }
    Ok(found)
}

fn delivered_for(
    connection: &Connection,
    taken: TrackId,
    offered: Option<&str>,
) -> Result<Vec<i64>> {
    let Some(path) = offered
        .and_then(MediaLocation::from_uri)
        .and_then(|location| {
            location
                .as_path()
                .map(|path| store::path_text(path).into_owned())
        })
    else {
        return Ok(Vec::new());
    };
    connection
        .query_row(
            THE_COPY_DELIVERED,
            params![path, taken.get() as i64],
            |row| row.get(0),
        )
        .optional()
        .map(|found| found.into_iter().collect())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn folded_on(transaction: &Transaction<'_>, evidenced: &[Evidenced]) -> Result<Vec<TrackId>> {
    let mut taken = Vec::with_capacity(evidenced.len());
    for &Evidenced { taken: lossy, kept } in evidenced {
        let still_waits: bool = transaction
            .query_row(
                THE_REPLACEMENT_STILL_WAITS,
                params![lossy.get() as i64],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        if !still_waits
            || standing_of(transaction, lossy)? != Standing::Lossy
            || standing_of(transaction, kept)? != Standing::Lossless
        {
            continue;
        }
        replacing::folded(transaction, kept, lossy)?;
        taken.push(lossy);
    }
    Ok(taken)
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
