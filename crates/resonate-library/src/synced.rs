use std::{
    io::Read,
    num::NonZeroUsize,
    path::PathBuf,
    thread,
    time::{Duration, SystemTime},
};

use resonate_core::{FrameSpan, Mbid, MediaLocation, PlaylistId, Recording, folded_letters};
use rusqlite::{Connection, OptionalExtension as _, Row as SqlRow, Transaction, params};

use crate::{
    Cut, Error, Library, PassHandle, Result, ScanOptions, StoreOp,
    db::Inner,
    filed::{self, Unfiled},
    organise::{Named, Naming},
    playlist::{self, Row},
    store, undo,
};

const RECORDING_COLUMNS: &str = "t.title, coalesce(t.artist, r.name, ''), coalesce(a.title, ''), t.mbid, t.duration, t.sample_rate";

const RECORDING_JOINS: &str = "LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = t.artist_id";

const LISTEN_SETTLES_AFTER: Duration = Duration::from_secs(15 * 60);

const THE_SAME_LENGTH_WITHIN: Duration = Duration::from_secs(2);

const FIRST_ALIAS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedPlaylist {
    pub id: PlaylistId,
    pub created: i64,
    pub name: String,
    pub entries: Vec<Option<Recording>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedListen {
    pub id: u64,
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldSong {
    pub recording: Recording,
    pub path: PathBuf,
    pub size: u64,
}

pub(crate) fn playlists(inner: &Inner) -> Result<Vec<SyncedPlaylist>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare(
                "SELECT p.id, p.created, p.name FROM playlists p
                  WHERE NOT EXISTS (SELECT 1 FROM playlist_queries q WHERE q.playlist_id = p.id)
                  ORDER BY p.id",
            )
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let listed = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        listed
            .into_iter()
            .map(|(id, created, name)| {
                let id = PlaylistId::new(id.cast_unsigned())?;
                Ok(SyncedPlaylist {
                    id,
                    created,
                    name,
                    entries: entries_of(connection, id)?,
                })
            })
            .collect()
    })
}

fn entries_of(connection: &Connection, id: PlaylistId) -> Result<Vec<Option<Recording>>> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {RECORDING_COLUMNS} FROM playlist_entries e
               LEFT JOIN tracks t ON t.path = e.path AND t.span_start = e.span_start
               {RECORDING_JOINS}
              WHERE e.playlist_id = ?1 ORDER BY e.position"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params![id.get().cast_signed()], |row| recording_at(row, 0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn recording_at(row: &SqlRow<'_>, first: usize) -> rusqlite::Result<Option<Recording>> {
    let Some(title) = row.get::<_, Option<String>>(first)? else {
        return Ok(None);
    };
    let artist: String = row.get(first + 1)?;
    let album: String = row.get(first + 2)?;
    let mbid = row
        .get::<_, Option<String>>(first + 3)?
        .and_then(|mbid| Mbid::new(&mbid).ok());
    let duration: Option<i64> = row.get(first + 4)?;
    let rate: i64 = row.get(first + 5)?;
    let length = duration
        .zip(u64::try_from(rate).ok().filter(|rate| *rate > 0))
        .map(|(frames, rate)| Duration::from_secs_f64(frames.cast_unsigned() as f64 / rate as f64));
    Ok(Some(Recording {
        mbid,
        title,
        artist,
        album,
        length,
    }))
}

pub(crate) fn write_playlist(
    inner: &Inner,
    at: Option<(PlaylistId, i64)>,
    name: &str,
    entries: &[Recording],
) -> Result<SyncedPlaylist> {
    let written = inner.write(|transaction| {
        let mut rows = Vec::with_capacity(entries.len());
        for entry in entries {
            if let Some(cut) = resolved(transaction, entry)? {
                rows.push(Row::of(&cut)?);
            }
        }
        let held = at.filter(|(id, created)| {
            transaction
                .query_row(
                    "SELECT 1 FROM playlists p WHERE p.id = ?1 AND p.created = ?2
                       AND NOT EXISTS (SELECT 1 FROM playlist_queries q WHERE q.playlist_id = p.id)",
                    params![id.get().cast_signed(), created],
                    |_| Ok(()),
                )
                .optional()
                .ok()
                .flatten()
                .is_some()
        });
        let id = match held {
            Some((id, _)) => id,
            None => made(transaction, name)?,
        };
        let name = a_free_name(transaction, name, id)?;
        let modified: i64 = transaction
            .query_row(
                "SELECT modified FROM playlists WHERE id = ?1",
                params![id.get().cast_signed()],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let now = store::to_nanos(SystemTime::now()).max(modified.saturating_add(1));
        transaction
            .execute(
                "UPDATE playlists SET name = ?2, folded = ?3, modified = ?4 WHERE id = ?1",
                params![id.get().cast_signed(), name, playlist::folded(&name), now],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        transaction
            .execute(
                "DELETE FROM playlist_entries WHERE playlist_id = ?1",
                params![id.get().cast_signed()],
            )
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
        for (position, row) in rows.iter().enumerate() {
            playlist::insert(
                transaction,
                id,
                i64::try_from(position).unwrap_or(i64::MAX),
                row,
            )?;
        }
        if let Some(kept) = playlist::kept_in(transaction, id)? {
            playlist::in_order(transaction, id, kept)?;
        }
        let created: i64 = transaction
            .query_row(
                "SELECT created FROM playlists WHERE id = ?1",
                params![id.get().cast_signed()],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        Ok(SyncedPlaylist {
            id,
            created,
            entries: entries_of(transaction, id)?,
            name,
        })
    })?;
    undo::overtaken(inner, &[written.id], &[playlist::folded(&written.name)]);
    inner.playlists_changed();
    inner.arrived();
    Ok(written)
}

fn made(transaction: &Transaction<'_>, name: &str) -> Result<PlaylistId> {
    let now = store::to_nanos(SystemTime::now());
    transaction
        .execute(
            "INSERT INTO playlists (name, folded, created, modified) VALUES (?1, ?2, ?3, ?3)",
            params![name, playlist::folded(name), now],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    let id = PlaylistId::new(transaction.last_insert_rowid().cast_unsigned())?;
    transaction
        .execute(
            "DELETE FROM playlist_plays WHERE playlist_id = ?1",
            params![id.get().cast_signed()],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(id)
}

fn a_free_name(transaction: &Transaction<'_>, name: &str, keeping: PlaylistId) -> Result<String> {
    let name = name.trim();
    let wanted = if name.is_empty() { "Synced" } else { name };
    let mut asked = FIRST_ALIAS - 1;
    loop {
        let candidate = match asked {
            1 => wanted.to_owned(),
            nth => format!("{wanted} ({nth})"),
        };
        let holder: Option<i64> = transaction
            .query_row(
                "SELECT id FROM playlists WHERE folded = ?1",
                params![playlist::folded(&candidate)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        if holder.is_none_or(|holder| holder.cast_unsigned() == keeping.get()) {
            return Ok(candidate);
        }
        asked += 1;
    }
}

pub(crate) fn remove_playlist(inner: &Inner, id: PlaylistId, created: i64) -> Result<bool> {
    let removed = inner.write(|transaction| {
        transaction
            .execute(
                "DELETE FROM playlists WHERE id = ?1 AND created = ?2",
                params![id.get().cast_signed(), created],
            )
            .map(|removed| removed > 0)
            .map_err(|source| Error::store(StoreOp::Delete, source))
    })?;
    if removed {
        if inner
            .playing()
            .is_some_and(|playing| playing.playlist == id)
        {
            inner.set_playing(None);
        }
        undo::overtaken(inner, &[id], &[]);
        inner.playlists_changed();
        inner.arrived();
    }
    Ok(removed)
}

fn resolved(connection: &Connection, recording: &Recording) -> Result<Option<Cut>> {
    if let Some(mbid) = &recording.mbid {
        let held = connection
            .query_row(
                "SELECT path, span_start, span_frames FROM tracks
                  WHERE mbid = ?1 AND hidden = 0
                  ORDER BY alternative_of IS NOT NULL, id LIMIT 1",
                params![mbid.as_str()],
                cut_at,
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        if held.is_some() {
            return Ok(held);
        }
    }
    let words = resonate_core::title_words(&recording.title);
    if words.is_empty() {
        return Ok(None);
    }
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT t.path, t.span_start, t.span_frames, {RECORDING_COLUMNS}
               FROM tracks t {RECORDING_JOINS}
              WHERE t.title_words = ?1 AND t.hidden = 0
              ORDER BY t.alternative_of IS NOT NULL, t.id"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let candidates = statement
        .query_map(params![words], |row| {
            Ok((cut_at(row)?, recording_at(row, 3)?))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let artist = folded_letters(recording.artist.trim());
    let album = folded_letters(recording.album.trim());
    let mut alike = candidates.into_iter().filter(|(_, held)| {
        held.as_ref().is_some_and(|held| {
            folded_letters(held.artist.trim()) == artist
                && (album.is_empty() || folded_letters(held.album.trim()) == album)
                && lasts_alike(held.length, recording.length)
        })
    });
    Ok(alike.next().map(|(cut, _)| cut))
}

fn lasts_alike(held: Option<Duration>, wanted: Option<Duration>) -> bool {
    match (held, wanted) {
        (Some(held), Some(wanted)) => held.abs_diff(wanted) <= THE_SAME_LENGTH_WITHIN,
        _ => true,
    }
}

fn cut_at(row: &SqlRow<'_>) -> rusqlite::Result<Cut> {
    Ok(Cut {
        location: MediaLocation::local(row.get::<_, String>(0)?),
        span: store::span(row.get(1)?, row.get(2)?),
    })
}

pub(crate) fn resolve(inner: &Inner, recording: &Recording) -> Result<Option<Cut>> {
    inner.read(|connection| resolved(connection, recording))
}

pub(crate) fn recording_of(
    inner: &Inner,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Result<Option<Recording>> {
    let Some(path) = location.as_path() else {
        return Ok(None);
    };
    let text = store::path_text(path)?.to_owned();
    let (start, _) = store::span_columns(span);
    inner.read(|connection| {
        connection
            .query_row(
                &format!(
                    "SELECT {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS}
                      WHERE t.path = ?1 AND t.span_start = ?2"
                ),
                params![text, start],
                |row| recording_at(row, 0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

pub(crate) fn favourites(inner: &Inner) -> Result<Vec<Recording>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare(&format!(
                "SELECT {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS}
                  WHERE t.favourite IS NOT NULL AND t.hidden = 0 ORDER BY t.favourite"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let favoured = statement
            .query_map([], |row| recording_at(row, 0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        Ok(favoured.into_iter().flatten().collect())
    })
}

pub(crate) fn favour(inner: &Inner, recording: &Recording, favoured: bool) -> Result<bool> {
    let changed = inner.write(|transaction| {
        let Some(cut) = resolved(transaction, recording)? else {
            return Ok(None);
        };
        let row = Row::of(&cut)?;
        let at = favoured.then(|| store::to_nanos(SystemTime::now()));
        let standing = if favoured { "IS NULL" } else { "IS NOT NULL" };
        transaction
            .execute(
                &format!(
                    "UPDATE tracks SET favourite = ?1
                      WHERE path = ?2 AND span_start = ?3 AND favourite {standing}"
                ),
                params![at, row.path, row.start()],
            )
            .map(|changed| Some(changed > 0))
            .map_err(|source| Error::store(StoreOp::Update, source))
    })?;
    if changed == Some(true) {
        inner.arrived();
    }
    Ok(changed.is_some())
}

pub(crate) fn listens_after(inner: &Inner, mark: u64, most: usize) -> Result<Vec<SyncedListen>> {
    let settled_before = store::to_nanos(
        SystemTime::now()
            .checked_sub(LISTEN_SETTLES_AFTER)
            .unwrap_or(SystemTime::UNIX_EPOCH),
    );
    inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT l.id, coalesce(l.began, l.at), l.heard, l.at, {RECORDING_COLUMNS}
                   FROM listens l
                   JOIN tracks t ON t.id = l.track_id
                   {RECORDING_JOINS}
                  WHERE l.id > ?1 AND l.elsewhere = 0
                  ORDER BY l.id LIMIT ?2"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let rows = statement
            .query_map(
                params![mark.cast_signed(), i64::try_from(most).unwrap_or(i64::MAX)],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        recording_at(row, 4)?,
                    ))
                },
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let mut listens = Vec::with_capacity(rows.len());
        for (id, began, heard, at, recording) in rows {
            if heard == 0 && at > settled_before {
                break;
            }
            if let Some(recording) = recording {
                listens.push(SyncedListen {
                    id: id.cast_unsigned(),
                    recording,
                    at: began,
                    heard: Duration::from_nanos(heard.cast_unsigned()),
                });
            }
        }
        Ok(listens)
    })
}

pub(crate) fn credit(
    inner: &Inner,
    recording: &Recording,
    at: i64,
    heard: Duration,
) -> Result<bool> {
    let credited = inner.write(|transaction| {
        let Some(cut) = resolved(transaction, recording)? else {
            return Ok(false);
        };
        let row = Row::of(&cut)?;
        let track: Option<i64> = transaction
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1 AND span_start = ?2",
                params![row.path, row.start()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let Some(track) = track else {
            return Ok(false);
        };
        let heard = i64::try_from(heard.as_nanos()).unwrap_or(i64::MAX);
        transaction
            .execute(
                "INSERT INTO listens (track_id, at, began, heard, elsewhere) VALUES (?1, ?2, ?2, ?3, 1)",
                params![track, at, heard],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        transaction
            .execute(
                "UPDATE tracks SET plays = plays + 1, played = max(coalesce(played, 0), ?2) WHERE id = ?1",
                params![track, at],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        Ok(true)
    })?;
    if credited {
        inner.arrived();
    }
    Ok(credited)
}

pub(crate) fn holdings(inner: &Inner) -> Result<Vec<HeldSong>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare(&format!(
                "SELECT t.path, t.file_size, {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS}
                  WHERE t.hidden = 0 AND t.span_start = 0 AND t.span_frames IS NULL
                    AND t.vault_key IS NULL AND t.root_id IS NOT NULL"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    recording_at(row, 2)?,
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        Ok(rows
            .into_iter()
            .filter_map(|(path, size, recording)| {
                recording.map(|recording| HeldSong {
                    recording,
                    path: PathBuf::from(path),
                    size: size.cast_unsigned(),
                })
            })
            .collect())
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SongFiled {
    Filed(PathBuf),
    Nowhere,
    Refused,
}

pub(crate) fn file_song(
    library: &Library,
    recording: &Recording,
    extension: &str,
    reader: &mut dyn Read,
) -> Result<SongFiled> {
    let Some(into) = library.delivering_into() else {
        return Ok(SongFiled::Nowhere);
    };
    let Some(folder) = into
        .path
        .canonicalize()
        .ok()
        .filter(|folder| folder.is_dir())
    else {
        return Ok(SongFiled::Nowhere);
    };
    let root = library
        .root_reaching(&folder)?
        .unwrap_or_else(|| folder.clone());
    let named = Named {
        album_artist: None,
        artist: Some(&recording.artist),
        album: (!recording.album.trim().is_empty()).then_some(recording.album.as_str()),
        title: &recording.title,
        year: None,
        disc: None,
        discs: 1,
        track: None,
        extension: Some(extension),
    };
    let Some(relative) = into.layout.render(&named, Naming::of(&folder)) else {
        return Ok(SongFiled::Refused);
    };
    let whole = folder.join(relative);
    match filed::landed_beside(library, &whole, reader) {
        Ok(path) => {
            if let Err(error) = library.scan(scanning(root)).and_then(PassHandle::join) {
                tracing::warn!(%error, "the folder a song from another device was filed in could not be read yet");
            }
            Ok(SongFiled::Filed(path))
        }
        Err(Unfiled::Catalog(error)) => Err(error),
        Err(unfiled) => {
            tracing::warn!(?unfiled, path = %whole.display(), "a song from another device could not be filed");
            Ok(SongFiled::Refused)
        }
    }
}

fn scanning(root: PathBuf) -> ScanOptions {
    ScanOptions {
        roots: vec![root],
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: true,
        workers: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
    }
}
