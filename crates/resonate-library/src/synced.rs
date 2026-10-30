use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime},
};

use resonate_codec::TagSet;
use resonate_core::{
    FrameSpan, Mbid, MediaLocation, PlaylistId, Recording, WholeFile, folded_letters,
};
use resonate_providers::Extension;
use resonate_vault::{Keeping, Kept, Offered, Vault};
use rusqlite::{Connection, OptionalExtension as _, Row as SqlRow, Transaction, params};

use crate::{
    Cut, Error, Library, OrderedColumn, PassHandle, Result, SavedQuery, ScanOptions, StoreOp,
    db::Inner,
    filed::{self, Beside, Unfiled},
    organise::{Named, Naming},
    playlist::{self, Row},
    store, undo, vaulted, volumes,
};

pub(crate) const RECORDING_COLUMNS: &str = "t.title, coalesce(t.artist, r.name, ''), coalesce(a.title, ''), t.mbid, t.duration, t.sample_rate";

pub(crate) const RECORDING_JOINS: &str = "LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = t.artist_id";

const LISTEN_SETTLES_AFTER: Duration = Duration::from_secs(15 * 60);

const THE_SAME_LENGTH_WITHIN: Duration = Duration::from_secs(2);

const FIRST_ALIAS: usize = 2;

const DEVICES: &str = "devices";

const SCANS_TRIED_AT_MOST: u32 = 30;

const SCAN_TRIED_AGAIN_AFTER: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedPlaylist {
    pub id: PlaylistId,
    pub created: i64,
    pub name: String,
    pub entries: Vec<Option<Recording>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedQueryBody {
    pub name: String,
    pub text: Option<String>,
    pub sort: u8,
    pub reading: u8,
    pub rows: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedQuery {
    pub id: PlaylistId,
    pub created: i64,
    pub body: SyncedQueryBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedListen {
    pub id: u64,
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldTags {
    pub album_artist: Option<String>,
    pub date: Option<String>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub compilation: bool,
}

impl HeldTags {
    fn of(tags: TagSet) -> Self {
        Self {
            album_artist: tags.album_artist,
            date: tags.date,
            track_number: tags.track_number,
            track_total: tags.track_total,
            disc_number: tags.disc_number,
            disc_total: tags.disc_total,
            compilation: tags.compilation,
        }
    }

    fn fill(&self, declared: &mut TagSet) {
        fill_gap(&mut declared.album_artist, &self.album_artist);
        fill_gap(&mut declared.date, &self.date);
        fill_gap(&mut declared.track_number, &self.track_number);
        fill_gap(&mut declared.track_total, &self.track_total);
        fill_gap(&mut declared.disc_number, &self.disc_number);
        fill_gap(&mut declared.disc_total, &self.disc_total);
        declared.compilation |= self.compilation;
    }
}

fn fill_gap<T: Clone>(declared: &mut Option<T>, passed_on: &Option<T>) {
    if declared.is_none() {
        declared.clone_from(passed_on);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldSong {
    pub recording: Recording,
    pub path: PathBuf,
    pub extension: String,
    pub size: u64,
}

enum Streaming {
    Plain(File),
    Vaulted(Offered),
}

pub struct OpenedSong {
    pub size: u64,
    pub extension: String,
    stream: Streaming,
}

impl Read for OpenedSong {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        match &mut self.stream {
            Streaming::Plain(file) => file.read(into),
            Streaming::Vaulted(offered) => offered.read(into),
        }
    }
}

impl Seek for OpenedSong {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match &mut self.stream {
            Streaming::Plain(file) => file.seek(to),
            Streaming::Vaulted(offered) => offered.seek(to),
        }
    }
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub(crate) fn offered(library: &Library, songs: Vec<HeldSong>) -> Vec<HeldSong> {
    let Some(vault) = library.vault() else {
        return songs;
    };
    songs
        .into_iter()
        .filter_map(|song| {
            if !vault.holds(&song.path) {
                return Some(song);
            }
            match vault.offer(&song.path) {
                Ok(offered) => Some(HeldSong {
                    extension: offered.extension.clone(),
                    size: offered.size,
                    ..song
                }),
                Err(error) => {
                    tracing::debug!(%error, path = %song.path.display(), "a vaulted song is not offered to other devices");
                    None
                }
            }
        })
        .collect()
}

pub(crate) fn open_song(library: &Library, path: &Path) -> Result<Option<OpenedSong>> {
    if let Some(vault) = library.vault()
        && vault.holds(path)
    {
        return match vault.offer(path) {
            Ok(offered) => Ok(Some(OpenedSong {
                size: offered.size,
                extension: offered.extension.clone(),
                stream: Streaming::Vaulted(offered),
            })),
            Err(error) if vault.failed_itself(&error) => Err(Error::Vault {
                path: vault.root().to_path_buf(),
                source: Box::new(error),
            }),
            Err(error) => {
                tracing::debug!(%error, path = %path.display(), "a vaulted song could not be opened to send");
                Ok(None)
            }
        };
    }
    let Ok(file) = File::open(path) else {
        return Ok(None);
    };
    let Ok(size) = file.metadata().map(|metadata| metadata.len()) else {
        return Ok(None);
    };

    Ok(Some(OpenedSong {
        size,
        extension: extension_of(path),
        stream: Streaming::Plain(file),
    }))
}

pub(crate) fn tags_of(inner: &Inner, path: &Path) -> Result<HeldTags> {
    Ok(vaulted::declared_by(inner, path)?.map_or_else(HeldTags::default, HeldTags::of))
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

pub(crate) fn queries(inner: &Inner) -> Result<Vec<SyncedQuery>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare(
                "SELECT p.id, p.created, p.name, q.text, q.sort, q.reading, q.max_rows
                   FROM playlists p JOIN playlist_queries q ON q.playlist_id = p.id
                  ORDER BY p.id",
            )
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let listed = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        listed
            .into_iter()
            .map(|(id, created, name, text, sort, reading, rows)| {
                Ok(SyncedQuery {
                    id: PlaylistId::new(id.cast_unsigned())?,
                    created,
                    body: SyncedQueryBody {
                        name,
                        text,
                        sort: u8::try_from(sort).unwrap_or(u8::MAX),
                        reading: u8::try_from(reading).unwrap_or(u8::MAX),
                        rows: rows.and_then(|rows| u32::try_from(rows).ok()),
                    },
                })
            })
            .collect()
    })
}

fn query_of(body: &SyncedQueryBody) -> Result<Option<SavedQuery>> {
    let placeholder = PlaylistId::new(1)?;
    let Ok(sort) = store::sort_of(placeholder, i64::from(body.sort)) else {
        return Ok(None);
    };
    let Ok(reading) =
        store::direction_of(placeholder, OrderedColumn::Reading, i64::from(body.reading))
    else {
        return Ok(None);
    };
    Ok(Some(SavedQuery {
        text: body.text.clone(),
        sort,
        reading,
        limit: body.rows.map(|rows| rows as usize),
    }))
}

pub(crate) fn write_query(
    inner: &Inner,
    at: Option<(PlaylistId, i64)>,
    body: &SyncedQueryBody,
) -> Result<Option<SyncedQuery>> {
    let Some(query) = query_of(body)? else {
        return Ok(None);
    };
    let written = inner.write(|transaction| {
        let held = at.filter(|(id, created)| {
            transaction
                .query_row(
                    "SELECT 1 FROM playlists p JOIN playlist_queries q ON q.playlist_id = p.id
                      WHERE p.id = ?1 AND p.created = ?2",
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
            None => made(transaction, &body.name)?,
        };
        let name = named(transaction, id, &body.name)?;
        transaction
            .execute(
                "DELETE FROM playlist_queries WHERE playlist_id = ?1",
                params![id.get().cast_signed()],
            )
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
        playlist::write_query(transaction, id, &query)?;
        let created: i64 = transaction
            .query_row(
                "SELECT created FROM playlists WHERE id = ?1",
                params![id.get().cast_signed()],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        Ok(SyncedQuery {
            id,
            created,
            body: SyncedQueryBody {
                name,
                ..body.clone()
            },
        })
    })?;
    undo::overtaken(
        inner,
        &[written.id],
        &[playlist::folded(&written.body.name)],
    );
    inner.playlists_changed();
    inner.arrived();
    Ok(Some(written))
}

fn named(transaction: &Transaction<'_>, id: PlaylistId, wanted: &str) -> Result<String> {
    let name = a_free_name(transaction, wanted, Some(id))?;
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
    Ok(name)
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

pub(crate) fn recording_at(row: &SqlRow<'_>, first: usize) -> rusqlite::Result<Option<Recording>> {
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
        let name = named(transaction, id, name)?;
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
    let name = a_free_name(transaction, name, None)?;
    transaction
        .execute(
            "INSERT INTO playlists (name, folded, created, modified) VALUES (?1, ?2, ?3, ?3)",
            params![name, playlist::folded(&name), now],
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

fn a_free_name(
    transaction: &Transaction<'_>,
    name: &str,
    keeping: Option<PlaylistId>,
) -> Result<String> {
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
        if holder.is_none_or(|holder| {
            keeping.is_some_and(|keeping| holder.cast_unsigned() == keeping.get())
        }) {
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

pub(crate) fn resolved(connection: &Connection, recording: &Recording) -> Result<Option<Cut>> {
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
    if let Some(received) = received_as(connection, recording)? {
        return Ok(Some(received));
    }
    Ok(named_alike(connection, recording, Among::Shown)?
        .into_iter()
        .next()
        .map(|(cut, _)| cut))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Among {
    Shown,
    HiddenToo,
}

impl Among {
    fn clause(self) -> &'static str {
        match self {
            Self::Shown => "AND t.hidden = 0",
            Self::HiddenToo => "",
        }
    }
}

fn named_alike(
    connection: &Connection,
    recording: &Recording,
    among: Among,
) -> Result<Vec<(Cut, Option<String>)>> {
    let words = resonate_core::title_words(&recording.title);
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT t.path, t.span_start, t.span_frames, t.vault_path, {RECORDING_COLUMNS}
               FROM tracks t {RECORDING_JOINS}
              WHERE t.title_words = ?1 {}
              ORDER BY t.alternative_of IS NOT NULL, t.id",
            among.clause()
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let candidates = statement
        .query_map(params![words], |row| {
            Ok((
                cut_at(row)?,
                row.get::<_, Option<String>>(3)?,
                recording_at(row, 4)?,
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let artist = folded_letters(recording.artist.trim());
    let album = folded_letters(recording.album.trim());
    Ok(candidates
        .into_iter()
        .filter(|(_, _, held)| {
            held.as_ref().is_some_and(|held| {
                folded_letters(held.artist.trim()) == artist
                    && (album.is_empty() || folded_letters(held.album.trim()) == album)
                    && lasts_alike(held.length, recording.length)
            })
        })
        .map(|(cut, within, _)| (cut, within))
        .collect())
}

struct HeldRow {
    path: PathBuf,
    within_the_vault: Option<PathBuf>,
}

impl HeldRow {
    fn of(path: &str, within_the_vault: Option<&str>) -> Self {
        Self {
            path: store::path_of(path),
            within_the_vault: within_the_vault.map(store::path_of),
        }
    }

    fn read(row: &SqlRow<'_>) -> rusqlite::Result<Self> {
        Ok(Self::of(
            &row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.as_deref(),
        ))
    }

    fn stands(&self, vault: Option<&Vault>) -> bool {
        self.path.exists()
            || self.within_the_vault.as_deref().is_some_and(|within| {
                vault.is_none_or(|vault| vault.at(within).is_ok_and(|object| object.exists()))
            })
    }
}

fn every_row_holding(connection: &Connection, recording: &Recording) -> Result<Vec<HeldRow>> {
    let key = recording.key();
    let named = recording.named_key();
    let mut rows = Vec::new();
    if let Some(mbid) = &recording.mbid {
        let mut statement = connection
            .prepare_cached("SELECT path, vault_path FROM tracks WHERE mbid = ?1")
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let held = statement
            .query_map(params![mbid.as_str()], HeldRow::read)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        rows.extend(held);
    }
    let mut statement = connection
        .prepare_cached(
            "SELECT s.path, t.vault_path
               FROM songs_received s LEFT JOIN tracks t ON t.path = s.path
              WHERE s.recording IN (?1, ?2)",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let received = statement
        .query_map(params![key.as_str(), named.as_str()], HeldRow::read)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    rows.extend(received);
    for (cut, within) in named_alike(connection, recording, Among::HiddenToo)? {
        if let Some(path) = cut.location.as_path() {
            rows.push(HeldRow {
                path: path.to_path_buf(),
                within_the_vault: within.as_deref().map(store::path_of),
            });
        }
    }
    Ok(rows)
}

pub(crate) fn holds(inner: &Inner, vault: Option<&Vault>, recording: &Recording) -> Result<bool> {
    inner.read(|connection| {
        let rows = every_row_holding(connection, recording)?;
        if rows.iter().any(|row| row.stands(vault)) {
            return Ok(true);
        }
        if rows.is_empty() {
            return Ok(false);
        }
        let away = volumes::absent(connection)?;
        Ok(rows
            .iter()
            .any(|row| volumes::is_on_an_absent_one(&row.path, &away)))
    })
}

pub(crate) fn note_deleted(tx: &Transaction<'_>, path: &str) -> Result<()> {
    let now = store::to_nanos(SystemTime::now());
    let mut statement = tx
        .prepare_cached(&format!(
            "SELECT {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS} WHERE t.path = ?1"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let named = statement
        .query_map(params![path], |row| recording_at(row, 0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    for recording in named.into_iter().flatten() {
        for key in [recording.key(), recording.named_key()] {
            tx.execute(
                "INSERT OR REPLACE INTO songs_deleted (recording, deleted) VALUES (?1, ?2)",
                params![key.as_str(), now],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO songs_deleted (recording, deleted)
         SELECT recording, ?2 FROM songs_received WHERE path = ?1",
        params![path, now],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    Ok(())
}

pub(crate) fn deleted_here(inner: &Inner, recording: &Recording) -> Result<bool> {
    inner.read(|connection| {
        connection
            .query_row(
                "SELECT 1 FROM songs_deleted WHERE recording IN (?1, ?2)",
                params![recording.key().as_str(), recording.named_key().as_str()],
                |_| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

pub(crate) fn forgive_deletion(inner: &Inner, recording: &Recording) -> Result<()> {
    inner.write(|transaction| {
        transaction
            .execute(
                "DELETE FROM songs_deleted WHERE recording IN (?1, ?2)",
                params![recording.key().as_str(), recording.named_key().as_str()],
            )
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Delete, source))
    })
}

pub(crate) fn forgive_deletions(inner: &Inner) -> Result<()> {
    inner.write(|transaction| {
        transaction
            .execute("DELETE FROM songs_deleted", [])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Delete, source))
    })
}

pub(crate) fn same_file_held(
    inner: &Inner,
    vault: Option<&Vault>,
    file: WholeFile,
    landed: &Path,
) -> Result<Option<PathBuf>> {
    let landed = store::path_text(landed).into_owned();
    let rows = inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(
                "SELECT path, vault_path FROM tracks
                  WHERE packets = ?1 AND duration = ?2 AND sample_rate = ?3
                    AND span_start = 0 AND span_frames IS NULL AND path <> ?4
                  ORDER BY alternative_of IS NOT NULL, id",
            )
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map(
                params![
                    file.packets.cast_signed(),
                    file.frames.cast_signed(),
                    file.rate,
                    landed
                ],
                HeldRow::read,
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    Ok(rows
        .into_iter()
        .find(|row| row.stands(vault))
        .map(|row| row.path))
}

fn received_as(connection: &Connection, recording: &Recording) -> Result<Option<Cut>> {
    let key = recording.key();
    let named = recording.named_key();
    connection
        .query_row(
            "SELECT t.path, t.span_start, t.span_frames
               FROM songs_received s JOIN tracks t ON t.path = s.path
              WHERE s.recording IN (?1, ?2) AND t.hidden = 0
              ORDER BY t.alternative_of IS NOT NULL, t.id LIMIT 1",
            params![key.as_str(), named.as_str()],
            cut_at,
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceivedAt {
    NewlyPlaced,
    AlreadyHeld,
}

pub(crate) fn note_received_at(
    inner: &Inner,
    recording: &Recording,
    path: &Path,
    at: ReceivedAt,
) -> Result<()> {
    let text = store::path_text(path).into_owned();
    let keys = [recording.key(), recording.named_key()];
    inner.write(|transaction| {
        if at == ReceivedAt::NewlyPlaced {
            transaction
                .execute("DELETE FROM songs_received WHERE path = ?1", params![text])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
        }
        for key in &keys {
            transaction
                .execute(
                    "INSERT INTO songs_received (recording, path) VALUES (?1, ?2)
                     ON CONFLICT (recording) DO UPDATE SET path = excluded.path",
                    params![key.as_str(), text],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
        Ok(())
    })
}

pub(crate) fn lasts_alike(held: Option<Duration>, wanted: Option<Duration>) -> bool {
    match (held, wanted) {
        (Some(held), Some(wanted)) => held.abs_diff(wanted) <= THE_SAME_LENGTH_WITHIN,
        _ => true,
    }
}

fn cut_at(row: &SqlRow<'_>) -> rusqlite::Result<Cut> {
    Ok(Cut {
        location: MediaLocation::local(store::path_of(&row.get::<_, String>(0)?)),
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
    inner.read(|connection| recording_in(connection, location, span))
}

pub(crate) fn held_as(inner: &Inner, recording: &Recording) -> Result<Option<Recording>> {
    inner.read(|connection| match resolved(connection, recording)? {
        Some(cut) => recording_in(connection, &cut.location, cut.span),
        None => Ok(None),
    })
}

fn recording_in(
    connection: &Connection,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Result<Option<Recording>> {
    let Some(path) = location.as_path() else {
        return Ok(None);
    };
    let text = store::path_text(path).into_owned();
    let (start, _) = store::span_columns(span);
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
}

pub(crate) fn favourites(inner: &Inner) -> Result<Vec<Recording>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare(&format!(
                "SELECT {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS}
                  WHERE t.favourite IS NOT NULL ORDER BY t.favourite"
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
    origin: &[u8],
    listen: u64,
    at: i64,
    heard: Duration,
) -> Result<bool> {
    let credited = inner.write(|transaction| {
        let Some(cut) = resolved(transaction, recording)? else {
            return Ok(Credited::NotHere);
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
            return Ok(Credited::NotHere);
        };
        let heard = i64::try_from(heard.as_nanos()).unwrap_or(i64::MAX);
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO listens (track_id, at, began, heard, elsewhere, origin_device, origin_id) VALUES (?1, ?2, ?2, ?3, 1, ?4, ?5)",
                params![track, at, heard, origin, listen.cast_signed()],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        if inserted == 0 {
            return Ok(Credited::Already);
        }
        transaction
            .execute(
                "UPDATE tracks SET plays = plays + 1, played = max(coalesce(played, 0), ?2) WHERE id = ?1",
                params![track, at],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        Ok(Credited::Newly)
    })?;
    if credited == Credited::Newly {
        inner.arrived();
    }
    Ok(credited != Credited::NotHere)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Credited {
    Newly,
    Already,
    NotHere,
}

pub(crate) fn holdings(inner: &Inner) -> Result<Vec<HeldSong>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare(&format!(
                "SELECT t.path, t.file_size, {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS}
                  WHERE t.hidden = 0 AND t.span_start = 0 AND t.span_frames IS NULL
                    AND ((t.vault_key IS NULL AND t.root_id IS NOT NULL)
                         OR (t.vault_key IS NOT NULL AND t.root_id IS NULL))"
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
                recording.map(|recording| {
                    let path = store::path_of(&path);
                    HeldSong {
                        recording,
                        extension: extension_of(&path),
                        path,
                        size: size.cast_unsigned(),
                    }
                })
            })
            .collect())
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SongFiled {
    Filed(PathBuf),
    Vaulted(PathBuf),
    Nowhere,
    Refused,
}

pub(crate) fn file_song(
    library: &Library,
    recording: &Recording,
    tags: &HeldTags,
    extension: &str,
    reader: &mut dyn Read,
) -> Result<SongFiled> {
    if library.vault().is_some() {
        return vaulted(library, recording, tags, extension, reader);
    }
    let Some(into) = library.delivering_into() else {
        return Ok(SongFiled::Nowhere);
    };
    let Ok(extension) = Extension::new(extension) else {
        tracing::warn!("a song from another device named an extension that is not letters");
        return Ok(SongFiled::Refused);
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
        extension: Some(extension.as_str()),
    };
    let Some(relative) = into.layout.render(&named, Naming::of(&folder)) else {
        return Ok(SongFiled::Refused);
    };
    let whole = folder.join(relative);
    match filed::landed_beside(library, &whole, reader) {
        Ok(Beside::Standing(path)) => {
            library.note_song_received_at(recording, &path, ReceivedAt::AlreadyHeld)?;
            scanned_once_free(library, root);
            Ok(SongFiled::Filed(path))
        }
        Ok(Beside::Placed { path, file }) => {
            let held = match file {
                Some(file) => library.same_file_held(file, &path)?,
                None => None,
            };
            if let Some(held) = held {
                if let Err(error) = fs::remove_file(&path) {
                    tracing::warn!(%error, path = %path.display(), "a song from another device already held could not be taken away again");
                }
                library.note_song_received_at(recording, &held, ReceivedAt::AlreadyHeld)?;
                return Ok(SongFiled::Filed(held));
            }
            library.note_song_received_at(recording, &path, ReceivedAt::NewlyPlaced)?;
            scanned_once_free(library, root);
            Ok(SongFiled::Filed(path))
        }
        Err(Unfiled::Catalog(error)) => Err(error),
        Err(unfiled) => {
            tracing::warn!(?unfiled, path = %whole.display(), "a song from another device could not be filed");
            Ok(SongFiled::Refused)
        }
    }
}

fn vaulted(
    library: &Library,
    recording: &Recording,
    tags: &HeldTags,
    extension: &str,
    reader: &mut dyn Read,
) -> Result<SongFiled> {
    let Some(vault) = library.vault() else {
        return Ok(SongFiled::Nowhere);
    };
    let mut kept = match vault.keep_delivered(reader, extension) {
        Ok(Keeping::Kept(kept)) => kept,
        Ok(Keeping::Refused(refusal)) => {
            tracing::warn!(
                refused = refusal.as_str(),
                "a song from another device was refused by the vault"
            );
            return Ok(SongFiled::Refused);
        }
        Err(error) if vault.failed_itself(&error) => {
            return Err(Error::Vault {
                path: vault.root().to_path_buf(),
                source: Box::new(error),
            });
        }
        Err(error) => {
            tracing::warn!(%error, "a song from another device could not be read into the vault");
            return Ok(SongFiled::Refused);
        }
    };
    if let Some(held) = library.vaulted_already(&kept)? {
        library.note_song_received_at(recording, &held, ReceivedAt::AlreadyHeld)?;
        return Ok(SongFiled::Vaulted(kept.path));
    }
    tags.fill(&mut kept.declared);
    let taken_from = format!("{DEVICES}:{}", recording.key());
    let at = match library.note_received(recording, &kept, &taken_from)? {
        Some(_) => ReceivedAt::NewlyPlaced,
        None => ReceivedAt::AlreadyHeld,
    };
    library.note_song_received_at(recording, &kept.path, at)?;

    Ok(SongFiled::Vaulted(kept.path))
}

pub(crate) fn vaulted_already(inner: &Inner, kept: &Kept) -> Result<Option<PathBuf>> {
    let key = kept.key.to_string();
    let path = store::path_text(&kept.path).into_owned();
    inner.read(|connection| {
        connection
            .query_row(
                "SELECT path FROM tracks
                  WHERE (vault_key = ?1 OR path = ?2) AND span_start = 0
                  ORDER BY path = ?2 DESC, root_id IS NULL DESC, id LIMIT 1",
                params![key, path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|held| held.map(|held| store::path_of(&held)))
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

fn scanned_once_free(library: &Library, root: PathBuf) {
    let mut tries = 1;
    loop {
        match library
            .scan(scanning(root.clone()))
            .and_then(PassHandle::join)
        {
            Err(Error::AlreadyWalking) if tries < SCANS_TRIED_AT_MOST => {
                tries += 1;
                thread::sleep(SCAN_TRIED_AGAIN_AFTER);
            }
            Err(error) => {
                tracing::warn!(%error, "the folder a song from another device was filed in could not be read yet");
                return;
            }
            Ok(_) => return,
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

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use super::*;

    const RATE: u32 = 8_000;

    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(named: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-synced-{}-{named}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a writable temporary directory");
            Self {
                path: path.canonicalize().expect("a canonical path"),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(
            &u32::try_from(payload.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn named_wav(title: &str) -> Vec<u8> {
        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&RATE.to_le_bytes());
        format.extend_from_slice(&(RATE * 2).to_le_bytes());
        format.extend_from_slice(&2_u16.to_le_bytes());
        format.extend_from_slice(&16_u16.to_le_bytes());

        let mut info = b"INFO".to_vec();
        for (id, value) in [
            (b"INAM", title),
            (b"IART", "Pink Floyd"),
            (b"IPRD", "Meddle"),
        ] {
            let mut text = value.as_bytes().to_vec();
            text.push(0);
            chunk(&mut info, id, &text);
        }

        let mut body = b"WAVE".to_vec();
        chunk(&mut body, b"fmt ", &format);
        chunk(&mut body, b"LIST", &info);
        chunk(&mut body, b"data", &vec![0; RATE as usize * 2]);

        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&u32::try_from(body.len()).unwrap_or(u32::MAX).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    fn song(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: Some(Duration::from_secs(1)),
        }
    }

    #[test]
    fn a_song_filed_but_not_yet_catalogued_is_held_while_its_file_stands() {
        let scratch = Scratch::new("filed-unscanned");
        let path = scratch.path.join("San Tropez.wav");
        fs::write(&path, named_wav("San Tropez")).expect("a written song");
        let library = Library::open_in_memory().expect("a catalog");
        let asked = song("San Tropez (2011 Remaster)");

        library
            .note_song_received_at(&asked, &path, ReceivedAt::NewlyPlaced)
            .expect("noted");
        let held = library.holds_recording(&asked).expect("asked");
        fs::remove_file(&path).expect("the song goes");
        let gone = library.holds_recording(&asked).expect("asked");

        assert!(held);
        assert!(!gone);
    }

    #[test]
    fn a_song_on_a_volume_that_is_away_is_still_held() {
        let scratch = Scratch::new("volume-away");
        let drive = scratch.path.join("drive");
        fs::create_dir_all(&drive).expect("a folder");
        fs::write(drive.join("echoes.wav"), named_wav("Echoes")).expect("a written song");
        let library = Library::open_in_memory().expect("a catalog");
        library
            .scan(ScanOptions {
                roots: vec![drive.clone()],
                incremental: true,
                follow_symlinks: false,
                extract_cover_art: false,
                workers: NonZeroUsize::MIN,
            })
            .and_then(PassHandle::join)
            .expect("a scan");
        let echoes = song("Echoes");
        assert!(library.holds_recording(&echoes).expect("asked"));

        fs::rename(&drive, scratch.path.join("elsewhere")).expect("the drive goes");
        let unremembered = library.holds_recording(&echoes).expect("asked");
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO volumes (path) VALUES (?1)",
                        params![store::path_text(&drive)],
                    )
                    .map(drop)
                    .map_err(|source| Error::store(StoreOp::Insert, source))
            })
            .expect("a remembered volume");
        let remembered = library.holds_recording(&echoes).expect("asked");

        assert!(!unremembered);
        assert!(remembered);
    }
}
