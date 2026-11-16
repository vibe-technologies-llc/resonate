use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use ahash::AHashSet;
use resonate_core::{MediaLocation, PlaylistId, ReleaseTrackId, TrackId};
use resonate_vault::Vault;
use rusqlite::{Connection, OptionalExtension as _, Transaction, params, types::Value};

use super::{Library, Walk, rows::rows};
use crate::{
    Error, Fold, Quality, Replaceable, Replaced, Replacing, Result, SongHeard, StoreOp, TheSong,
    Track, TrackQuery, Version, deleted::Why, elsewhere, linked::LENGTHS_AGREE_WITHIN, replacing,
    store,
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
    "), tracks.vault_path FROM tracks",
    " WHERE tracks.id = ?1 AND tracks.span_start = 0 AND tracks.span_frames IS NULL"
);

const THE_RELEASE_ROW_IS_STILL_THE_TRACKS: &str =
    "SELECT EXISTS (SELECT 1 FROM release_tracks WHERE id = ?1 AND track_id = ?2)";

const ANY_REPLACEMENT_WAITS: &str = "SELECT EXISTS (SELECT 1 FROM replacements)";

const THE_REPLACEMENTS_REJOIN_THE_ROWS_THEIR_SONGS_HOLD: &str = "UPDATE replacements
        SET release_track_id = (SELECT id FROM release_tracks
                                 WHERE release_tracks.track_id = replacements.track_id)
      WHERE release_track_id IS NULL
         OR NOT EXISTS (SELECT 1 FROM release_tracks
                         WHERE release_tracks.id = replacements.release_track_id
                           AND release_tracks.track_id = replacements.track_id)
  RETURNING track_id, release_track_id";

const WHAT_THE_ROWS_WANT_WAS_OFFERED: &str =
    "SELECT offered FROM wants WHERE release_track_id = ?1";

const THE_REPLACEMENT_WAITS_ON_A_ROW_OF_ITS_OWN: &str = "SELECT r.release_track_id IS NULL
            OR coalesce(rt.track_id IN (?1, ?2), 0)
       FROM replacements r
       LEFT JOIN release_tracks rt ON rt.id = r.release_track_id
      WHERE r.track_id = ?1";

const HOW_LONG_EACH_LASTS: &str =
    "SELECT lossy.duration, lossy.sample_rate, kept.duration, kept.sample_rate
       FROM tracks lossy, tracks kept
      WHERE lossy.id = ?1 AND kept.id = ?2";

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

struct Awaiting {
    taken: TrackId,
    delivered: Option<TrackId>,
    recorded: Vec<TrackId>,
}

struct WholeFile {
    path: PathBuf,
    within_the_vault: Option<PathBuf>,
    lossless: bool,
}

impl WholeFile {
    fn standing(&self, vault: Option<&Vault>) -> Standing {
        if !self.lossless {
            Standing::Lossy
        } else if self.path.is_file() || self.object_is_held(vault) {
            Standing::Lossless
        } else {
            Standing::Gone
        }
    }

    fn object_is_held(&self, vault: Option<&Vault>) -> bool {
        self.within_the_vault
            .as_deref()
            .zip(vault)
            .is_some_and(|(within, vault)| vault.at(within).is_ok_and(|object| object.is_file()))
    }
}

impl Library {
    pub fn replace(&self, chosen: &[Replaceable]) -> Result<Replacing> {
        let walk = self.walk_the_tree()?;
        let now = SystemTime::now();
        let vault = self.inner.vault.as_deref();
        let Settled {
            mut replacing,
            taken,
        } = self
            .inner
            .write(|transaction| settled(transaction, chosen, now, vault))?;

        if !taken.is_empty() {
            self.inner.playlists_changed();
            replacing.trashed = self.remove_tracks_under(&walk, &taken, Why::Folded)?;
        }
        Ok(replacing)
    }

    pub fn settle_replacements(&self) -> Result<Replaced> {
        if !self.inner.read(any_replacement_waits)? {
            return Ok(Replaced::default());
        }
        let walk = match self.walk_the_tree() {
            Ok(walk) => walk,
            Err(Error::AlreadyWalking) => {
                tracing::debug!(
                    "the replacements are settled after the pass walking the tree, or the next"
                );
                return Ok(Replaced::default());
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
                |row| Ok(Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))),
            )?;
            let mut joined = 0;
            for (lossy, row) in rejoined {
                let Some(row) = row else { continue };
                let lossy = TrackId::new(lossy.cast_unsigned())?;
                wanted_again_unless_it_delivered(transaction, row, lossy, now)?;
                joined += 1;
            }
            Ok(joined)
        })
    }

    fn settled_under(&self, walk: &Walk) -> Result<Replaced> {
        let evidenced = self.evidenced()?;
        if evidenced.is_empty() {
            return Ok(Replaced::default());
        }
        let vault = self.inner.vault.as_deref();
        let (taken, folds) = self
            .inner
            .write(|transaction| folded_on(transaction, vault, &evidenced))?;
        if taken.is_empty() {
            return Ok(Replaced::default());
        }
        self.inner.playlists_changed();
        Ok(Replaced {
            trashed: self.remove_tracks_under(walk, &taken, Why::Folded)?,
            folds,
        })
    }

    fn evidenced(&self) -> Result<Vec<Evidenced>> {
        let vault = self.inner.vault.as_deref();
        let awaiting = self.inner.read(awaiting)?;
        let mut found = Vec::new();
        for one in awaiting {
            let mut recorded = self.tracks_with_ids(&one.recorded)?;
            recorded.sort_by_key(|copy| Quality::of(copy).best_first());
            let candidates: Vec<TrackId> = one
                .delivered
                .into_iter()
                .chain(recorded.iter().map(|copy| copy.id))
                .collect();
            let kept = self.inner.read(|connection| {
                for &kept in &candidates {
                    if may_fold(connection, vault, one.taken, kept)? {
                        return Ok(Some(kept));
                    }
                }
                Ok(None)
            })?;
            if let Some(kept) = kept {
                found.push(Evidenced {
                    taken: one.taken,
                    kept,
                });
            }
        }
        Ok(found)
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
    vault: Option<&Vault>,
) -> Result<Settled> {
    let mut seen = AHashSet::new();
    let mut settled = Settled {
        replacing: Replacing::default(),
        taken: Vec::new(),
    };
    for one in chosen {
        if !seen.insert(one.track.id)
            || standing(transaction, vault, &one.track)? != Standing::Lossy
        {
            continue;
        }
        let held = match &one.held {
            Some(copy) if standing(transaction, vault, copy)? == Standing::Lossless => {
                Some(copy.id)
            }
            Some(_) | None => None,
        };
        let release_track = match one.release_track {
            Some(row) if still_held_by(transaction, row, one.track.id)? => Some(row),
            Some(_) | None => None,
        };

        if let Some(kept) = held {
            let fold = replacing::folded(transaction, kept, one.track.id)?;
            settled.replacing.folds.push(fold);
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
            let want = elsewhere::rearmed(transaction, row, now)?;
            settled.replacing.wanted += 1;
            settled.replacing.wants.push((row, want));
        } else {
            settled.replacing.unmatched += 1;
        }
    }
    Ok(settled)
}

fn standing(connection: &Connection, vault: Option<&Vault>, track: &Track) -> Result<Standing> {
    let Some(named) = track.location.as_path() else {
        return Ok(Standing::Gone);
    };
    Ok(match whole_file(connection, track.id)? {
        Some(file) if file.path == named => file.standing(vault),
        Some(_) | None => Standing::Gone,
    })
}

fn standing_of(connection: &Connection, vault: Option<&Vault>, track: TrackId) -> Result<Standing> {
    Ok(whole_file(connection, track)?.map_or(Standing::Gone, |file| file.standing(vault)))
}

fn whole_file(connection: &Connection, track: TrackId) -> Result<Option<WholeFile>> {
    let found: Option<(String, bool, Option<String>)> = connection
        .query_row(
            HOW_A_WHOLE_FILE_STANDS,
            params![track.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(found.map(|(path, lossless, within)| WholeFile {
        path: store::path_of(&path),
        within_the_vault: within.as_deref().map(store::path_of),
        lossless,
    }))
}

fn any_replacement_waits(connection: &Connection) -> Result<bool> {
    connection
        .query_row(ANY_REPLACEMENT_WAITS, [], |row| row.get(0))
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn awaiting(connection: &Connection) -> Result<Vec<Awaiting>> {
    let waiting = rows(connection, THE_REPLACEMENTS_WAITING, Vec::new(), |row| {
        Ok(Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
        )))
    })?;
    let mut found = Vec::with_capacity(waiting.len());
    for (taken, offered) in waiting {
        let taken = TrackId::new(taken.cast_unsigned())?;
        let delivered = delivered_for(connection, taken, offered.as_deref())?
            .map(|id| TrackId::new(id.cast_unsigned()))
            .transpose()?;
        let recorded = rows(
            connection,
            THE_COPIES_OF_THE_SAME_RECORDING,
            vec![Value::Integer(taken.get() as i64)],
            |row| row.get::<_, i64>(0).map(Ok),
        )?
        .into_iter()
        .map(|id| Ok(TrackId::new(id.cast_unsigned())?))
        .collect::<Result<Vec<_>>>()?;
        found.push(Awaiting {
            taken,
            delivered,
            recorded,
        });
    }
    Ok(found)
}

fn may_fold(
    connection: &Connection,
    vault: Option<&Vault>,
    lossy: TrackId,
    kept: TrackId,
) -> Result<bool> {
    Ok(waits_on_a_row_of_its_own(connection, lossy, kept)?
        && standing_of(connection, vault, lossy)? == Standing::Lossy
        && standing_of(connection, vault, kept)? == Standing::Lossless
        && lengths_agree(connection, lossy, kept)?)
}

fn waits_on_a_row_of_its_own(
    connection: &Connection,
    lossy: TrackId,
    kept: TrackId,
) -> Result<bool> {
    connection
        .query_row(
            THE_REPLACEMENT_WAITS_ON_A_ROW_OF_ITS_OWN,
            params![lossy.get() as i64, kept.get() as i64],
            |row| row.get(0),
        )
        .optional()
        .map(|own| own.unwrap_or(false))
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn lengths_agree(connection: &Connection, lossy: TrackId, kept: TrackId) -> Result<bool> {
    let lengths = connection
        .query_row(
            HOW_LONG_EACH_LASTS,
            params![lossy.get() as i64, kept.get() as i64],
            |row| {
                Ok((
                    lasting(row.get(0)?, row.get(1)?),
                    lasting(row.get(2)?, row.get(3)?),
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(match lengths {
        Some((Some(lossy), Some(kept))) => lossy.abs_diff(kept) <= LENGTHS_AGREE_WITHIN,
        Some(_) => true,
        None => false,
    })
}

fn lasting(frames: Option<i64>, rate: i64) -> Option<Duration> {
    let frames = u64::try_from(frames?).ok()?;
    let rate = u64::try_from(rate).ok().filter(|rate| *rate > 0)?;
    Some(
        Duration::from_secs(frames / rate)
            + Duration::from_nanos(frames % rate * 1_000_000_000 / rate),
    )
}

fn wanted_again_unless_it_delivered(
    transaction: &Transaction<'_>,
    row: i64,
    lossy: TrackId,
    now: SystemTime,
) -> Result<()> {
    elsewhere::wanted(transaction, row, now)?;
    let offered: Option<String> = transaction
        .query_row(WHAT_THE_ROWS_WANT_WAS_OFFERED, params![row], |found| {
            found.get(0)
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .flatten();
    let lossy_at = MediaLocation::local(replacing::path_of(transaction, lossy)?).to_uri();
    if offered.as_deref() == Some(lossy_at.as_str()) {
        elsewhere::rearmed(transaction, ReleaseTrackId::new(row.cast_unsigned())?, now)?;
    }
    Ok(())
}

fn delivered_for(
    connection: &Connection,
    taken: TrackId,
    offered: Option<&str>,
) -> Result<Option<i64>> {
    let Some(path) = offered
        .and_then(MediaLocation::from_uri)
        .and_then(|location| {
            location
                .as_path()
                .map(|path| store::path_text(path).into_owned())
        })
    else {
        return Ok(None);
    };
    connection
        .query_row(
            THE_COPY_DELIVERED,
            params![path, taken.get() as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn folded_on(
    transaction: &Transaction<'_>,
    vault: Option<&Vault>,
    evidenced: &[Evidenced],
) -> Result<(Vec<TrackId>, Vec<Fold>)> {
    let mut taken = Vec::with_capacity(evidenced.len());
    let mut folds = Vec::with_capacity(evidenced.len());
    for &Evidenced { taken: lossy, kept } in evidenced {
        if !may_fold(transaction, vault, lossy, kept)? {
            continue;
        }
        folds.push(replacing::folded(transaction, kept, lossy)?);
        taken.push(lossy);
    }
    Ok((taken, folds))
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
