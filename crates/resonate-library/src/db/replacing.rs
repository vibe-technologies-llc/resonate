use std::{
    iter,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use parking_lot::Mutex;
use resonate_core::{
    Locator, MediaLocation, PlaylistId, ReleaseTrackId, SourceId, TrackId, WantId,
};
use resonate_providers::{
    Asking, Away, Delivered, Delivery, Identity, Providers, TURNED_TO_APART,
    WAITED_ON_AFTER_AN_OFFER,
};
use resonate_vault::Vault;
use rusqlite::{Connection, OptionalExtension as _, Transaction, params, types::Value};

use super::{
    Library, Walk,
    rows::{grouped_links, rows},
};
use crate::{
    DeliveryFolder, Error, Fold, Looked, LookingFor, Quality, Replaceable, Replaced, Replacing,
    Result, SongHeard, StoreOp, TheSong, Track, TrackQuery, Version,
    deleted::Why,
    elsewhere,
    foresight::Unheard,
    linked::LENGTHS_AGREE_WITHIN,
    replacing, store,
    supply::{ANSWERS_WITHIN, ForgottenDelivery, WANTS_ASKED_AT_ONCE},
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

const THE_ROW_A_REPLACEMENT_LOOKS_FOR: &str = concat!(
    "SELECT rt.title, coalesce(rt.artist, ar.name), ",
    album_title!(),
    ", rt.recording_mbid, rt.track_mbid, a.mbid,
            coalesce(rt.isrc || ' ', '') ||
                coalesce((SELECT group_concat(c.isrc, ' ') FROM release_track_isrcs c
                           WHERE c.release_track_id = rt.id
                             AND c.isrc IS NOT rt.isrc), ''),
            rt.length_ms, rt.disc, rt.position, rt.album_id,
            (SELECT w.id FROM wants w WHERE w.release_track_id = rt.id)
       FROM release_tracks rt
       JOIN albums a ON a.id = rt.album_id
       LEFT JOIN artists ar ON ar.id = a.artist_id
      WHERE rt.id = ?1 AND rt.track_id = ?2"
);

const THE_LINKS_OF_THE_ROW: &str = "SELECT release_track_id, relation, provider, url
       FROM release_track_links
      WHERE release_track_id = ?1
      ORDER BY url";

const THE_LINKS_OF_ITS_RELEASE: &str = "SELECT album_id, relation, provider, url
       FROM album_links
      WHERE album_id = ?1
      ORDER BY url";

const WHAT_A_LOOK_FOUND: &str = "SELECT offered_by, offered_at FROM replacements_looked_for
      WHERE track_id = ?1 AND release_track_id IS ?2";

const A_LOOK_IS_KEPT: &str = "INSERT OR REPLACE INTO replacements_looked_for
            (track_id, release_track_id, offered_by, offered_at, looked)
     VALUES (?1, ?2, ?3, ?4, ?5)";

const A_KEEP_WAITS_FOR_THE_SONG: &str = "SELECT EXISTS (
            SELECT 1 FROM replacements_kept rk
              JOIN kept_songs k ON k.id = rk.kept_id
             WHERE rk.track_id = ?1 AND k.landed IS NULL AND k.gone = 0)";

const A_SONG_IS_KEPT_FOR_A_LOSSY_ONE: &str = concat!(
    "INSERT INTO kept_songs (source, video, title, artist, album, length, asked)
     VALUES (?1, ?2, ?3, ?4, (SELECT ",
    album_title!(),
    " FROM albums a WHERE a.id = ?5), ?6, ?7)
     ON CONFLICT (source, video) WHERE playlist IS NULL
     DO UPDATE SET gone = 0, tried = NULL, tries = 0
     RETURNING id"
);

const THE_KEEP_REPLACES_THE_SONG: &str =
    "INSERT OR REPLACE INTO replacements_kept (track_id, kept_id) VALUES (?1, ?2)";

const THE_COPY_KEPT_FOR_IT: &str = concat!(
    "SELECT tracks.id FROM replacements_kept rk",
    " JOIN kept_songs k ON k.id = rk.kept_id",
    " JOIN tracks ON tracks.path = k.landed",
    " WHERE rk.track_id = ?1 AND tracks.id != rk.track_id",
    " AND tracks.span_start = 0 AND tracks.span_frames IS NULL AND (",
    lossless_codecs!(),
    ") ORDER BY tracks.id"
);

const A_SONG_IS_KEYED: &str = "track/";

const NAMED_ONLY_FOR_LOSSY_SOUND: [&str; 12] = [
    "aac", "ac3", "eac3", "m4b", "mp2", "mp3", "mpc", "ogg", "opus", "spx", "webm", "wma",
];

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
    kept: Vec<TrackId>,
    recorded: Vec<TrackId>,
}

struct RowAsked {
    identity: Identity,
    want: Option<WantId>,
}

type Forgotten = AHashMap<WantId, Vec<ForgottenDelivery>>;

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
        let kept_into = self.delivering_into();
        let Settled {
            mut replacing,
            taken,
        } = self
            .inner
            .write(|transaction| settled(transaction, chosen, now, vault, kept_into.as_ref()))?;

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
                .chain(one.kept.iter().copied())
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
            let (release_track, pending, looked) = self.inner.read(|connection| {
                let (release_track, pending) = release_track_and_pending(connection, track.id)?;
                let looked = what_a_look_found(connection, track.id, release_track)?;
                Ok((release_track, pending, looked))
            })?;
            found.push(Replaceable {
                track,
                held,
                release_track,
                pending,
                looked,
            });
        }
        Ok(found)
    }

    pub fn look_for_replacements(
        &self,
        providers: &Providers,
        rows: &[Replaceable],
        looking: &LookingFor<'_>,
    ) -> Result<()> {
        let providers = providers.without_last_resorts();
        let asked: Vec<&Replaceable> = rows.iter().filter(|row| row.can_be_looked_for()).collect();
        if asked.is_empty() || !providers.has_a_source() {
            return Ok(());
        }
        let forgotten = self.forgotten_deliveries()?;
        let next = AtomicUsize::new(0);
        let away = Mutex::new(Away::default());
        let lane = || {
            while !(looking.cancelled)() {
                let Some(row) = asked.get(next.fetch_add(1, Ordering::Relaxed)) else {
                    return;
                };
                match self.looked_for(&providers, row, &forgotten, &away, looking.cancelled) {
                    Ok(Some(looked)) => (looking.told)(row.track.id, looked),
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(%error, "where a lossless copy might be found was not kept");
                        (looking.told)(row.track.id, Looked::Unheard);
                    }
                }
            }
        };
        thread::scope(|scope| {
            let lanes = WANTS_ASKED_AT_ONCE.get().min(asked.len());
            for _ in 0..lanes {
                let started = thread::Builder::new()
                    .name("resonate-looking".to_owned())
                    .spawn_scoped(scope, lane);
                if let Err(error) = started {
                    tracing::warn!(%error, "a lane looking for lossless copies did not start");
                }
            }
        });
        Ok(())
    }

    fn looked_for(
        &self,
        providers: &Providers,
        lossy: &Replaceable,
        forgotten: &Forgotten,
        away: &Mutex<Away>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<Looked>> {
        let row = lossy.release_track;
        let asked = match row {
            Some(row) => {
                let Some(asked) = self
                    .inner
                    .read(|connection| row_asked(connection, row, lossy.track.id))?
                else {
                    return Ok(None);
                };
                asked
            }
            None => RowAsked {
                identity: identity_of(&lossy.track),
                want: None,
            },
        };
        let forgotten_for_it = asked
            .want
            .and_then(|want| forgotten.get(&want))
            .map_or(&[][..], Vec::as_slice);
        let declined = |delivered: &Delivered| {
            offered_for_lossy_sound(delivered)
                || forgotten_for_it
                    .iter()
                    .any(|forgotten| forgotten.declines(delivered))
                || self.a_keep_holds_the_offer(delivered)
        };
        let mut held_away = away.lock().clone();
        let answer = providers.first(
            &asked.identity,
            &Asking {
                within: ANSWERS_WITHIN,
                cancelled,
                turning_to: &|_| {},
                declined: &declined,
                passing: &[],
                apart: TURNED_TO_APART,
                grace: WAITED_ON_AFTER_AN_OFFER,
                earlier_rounds_said_no: true,
            },
            &mut held_away,
        );
        away.lock().join(&held_away);
        if answer.cancelled {
            return Ok(None);
        }
        let heard_from_everyone = answer.heard_from_every_provider();
        let unheard = Unheard {
            refused: answer.refused,
            late: answer.late,
            passed_over: answer.passed_over,
        };
        let looked = match answer.delivered {
            Some(delivered) => {
                let looked = Looked::Offered {
                    by: delivered.provider.clone(),
                    at: delivered.taken_from(),
                };
                if let Some(recording) = asked.identity.recording.clone() {
                    let offers = iter::once(delivered).chain(answer.held_back).collect();
                    self.foresight()
                        .keep(recording, answer.heard, offers, unheard);
                }
                looked
            }
            None if heard_from_everyone => Looked::Unoffered,
            None => return Ok(Some(Looked::Unheard)),
        };
        let (offered_by, offered_at) = match &looked {
            Looked::Offered { by, at } => (Some(by.as_str().to_owned()), Some(at.to_uri())),
            Looked::Unoffered | Looked::Unheard => (None, None),
        };
        let now = store::to_nanos(SystemTime::now());
        self.inner.write(|transaction| {
            transaction
                .execute(
                    A_LOOK_IS_KEPT,
                    params![
                        lossy.track.id.get() as i64,
                        row.map(|row| row.get() as i64),
                        offered_by,
                        offered_at,
                        now
                    ],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })?;
        Ok(Some(looked))
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
    let kept_for_it: bool = connection
        .query_row(A_KEEP_WAITS_FOR_THE_SONG, params![at], |row| row.get(0))
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let release_track = release_track
        .map(|id| ReleaseTrackId::new(id.cast_unsigned()))
        .transpose()?;
    Ok((release_track, pending || kept_for_it))
}

fn what_a_look_found(
    connection: &Connection,
    track: TrackId,
    row: Option<ReleaseTrackId>,
) -> Result<Option<Looked>> {
    let found: Option<(Option<String>, Option<String>)> = connection
        .query_row(
            WHAT_A_LOOK_FOUND,
            params![track.get() as i64, row.map(|row| row.get() as i64)],
            |found| Ok((found.get(0)?, found.get(1)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(match found {
        Some((Some(by), Some(at))) => {
            let Some(at) = MediaLocation::from_uri(&at) else {
                return Ok(None);
            };
            Some(Looked::Offered {
                by: SourceId::new(&by)?,
                at,
            })
        }
        Some((None, _)) => Some(Looked::Unoffered),
        Some((Some(_), None)) | None => None,
    })
}

fn identity_of(lossy: &Track) -> Identity {
    Identity {
        artist: lossy.artist.clone(),
        length: lossy
            .duration
            .map(|frames| frames.to_duration(lossy.spec.rate)),
        ..Identity::named(lossy.title.as_str())
    }
}

fn kept_for(
    transaction: &Transaction<'_>,
    lossy: &Track,
    by: &SourceId,
    at: &MediaLocation,
    now: SystemTime,
) -> Result<()> {
    let key = match at.locator() {
        Locator::Key(key) => key.strip_prefix(A_SONG_IS_KEYED).unwrap_or(key).to_owned(),
        Locator::Path(path) => store::path_text(path).into_owned(),
    };
    let length = lossy
        .duration
        .map(|frames| frames.to_duration(lossy.spec.rate).as_millis() as i64);
    let kept: i64 = transaction
        .query_row(
            A_SONG_IS_KEPT_FOR_A_LOSSY_ONE,
            params![
                by.as_str(),
                key,
                lossy.title,
                lossy.artist,
                lossy.album_id.map(|album| album.get() as i64),
                length,
                store::to_nanos(now)
            ],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    transaction
        .execute(
            THE_KEEP_REPLACES_THE_SONG,
            params![lossy.id.get() as i64, kept],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Insert, source))
}

fn row_asked(
    connection: &Connection,
    row: ReleaseTrackId,
    track: TrackId,
) -> Result<Option<RowAsked>> {
    struct Read {
        title: String,
        artist: Option<String>,
        album_title: String,
        recording: Option<String>,
        track: Option<String>,
        release: Option<String>,
        isrcs: String,
        length_ms: Option<i64>,
        disc: i64,
        position: i64,
        album: i64,
        want: Option<i64>,
    }
    let read = connection
        .query_row(
            THE_ROW_A_REPLACEMENT_LOOKS_FOR,
            params![row.get() as i64, track.get() as i64],
            |found| {
                Ok(Read {
                    title: found.get(0)?,
                    artist: found.get(1)?,
                    album_title: found.get(2)?,
                    recording: found.get(3)?,
                    track: found.get(4)?,
                    release: found.get(5)?,
                    isrcs: found.get(6)?,
                    length_ms: found.get(7)?,
                    disc: found.get(8)?,
                    position: found.get(9)?,
                    album: found.get(10)?,
                    want: found.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let Some(read) = read else {
        return Ok(None);
    };
    let links = grouped_links(
        connection,
        THE_LINKS_OF_THE_ROW,
        vec![Value::Integer(row.get() as i64)],
    )?
    .remove(&(row.get() as i64))
    .unwrap_or_default();
    let release_links = grouped_links(
        connection,
        THE_LINKS_OF_ITS_RELEASE,
        vec![Value::Integer(read.album)],
    )?
    .remove(&read.album)
    .unwrap_or_default();
    let want = read
        .want
        .map(|want| WantId::new(want.cast_unsigned()))
        .transpose()?;
    Ok(Some(RowAsked {
        identity: Identity {
            recording: store::mbid_in(read.recording.as_deref()),
            track: store::mbid_in(read.track.as_deref()),
            release: store::mbid_in(read.release.as_deref()),
            isrcs: read
                .isrcs
                .split_whitespace()
                .filter_map(|code| store::isrc_in(Some(code)))
                .collect(),
            title: read.title,
            artist: read.artist,
            album: Some(read.album_title),
            length: read
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0).cast_unsigned())),
            disc: Some(u32::try_from(read.disc.max(0)).unwrap_or(u32::MAX)),
            position: Some(u32::try_from(read.position.max(0)).unwrap_or(u32::MAX)),
            links,
            release_links,
            gone: Vec::new(),
        },
        want,
    }))
}

fn offered_for_lossy_sound(delivered: &Delivered) -> bool {
    let extension = match &delivered.delivery {
        Delivery::File(path) => path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default(),
        Delivery::Stream { extension, .. } => extension.as_str(),
    };
    NAMED_ONLY_FOR_LOSSY_SOUND
        .iter()
        .any(|lossy| lossy.eq_ignore_ascii_case(extension))
}

fn settled(
    transaction: &Transaction<'_>,
    chosen: &[Replaceable],
    now: SystemTime,
    vault: Option<&Vault>,
    kept_into: Option<&DeliveryFolder>,
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
        } else if let Some((by, at)) = one.offer() {
            if kept_into.is_none() {
                settled.replacing.unfiled += 1;
                continue;
            }
            transaction
                .execute(
                    A_LOSSY_SONG_WAITS,
                    params![
                        one.track.id.get() as i64,
                        Option::<i64>::None,
                        store::to_nanos(now)
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            kept_for(transaction, &one.track, by, at, now)?;
            settled.replacing.kept += 1;
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
        let kept = rows(
            connection,
            THE_COPY_KEPT_FOR_IT,
            vec![Value::Integer(taken.get() as i64)],
            |row| row.get::<_, i64>(0).map(Ok),
        )?
        .into_iter()
        .map(|id| Ok(TrackId::new(id.cast_unsigned())?))
        .collect::<Result<Vec<_>>>()?;
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
            kept,
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
