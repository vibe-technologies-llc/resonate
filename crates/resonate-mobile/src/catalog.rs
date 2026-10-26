use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use rusqlite::{
    CachedStatement, Connection, OptionalExtension, TransactionBehavior, functions::FunctionFlags,
    hooks::Action, params,
};

use crate::{Error, Result, StoreOp, Track};

const LISTENS_NAME_THEIR_ORIGIN: &str = "
ALTER TABLE listens ADD COLUMN origin_device BLOB;
ALTER TABLE listens ADD COLUMN origin_id INTEGER;
CREATE UNIQUE INDEX listens_by_origin ON listens(origin_device, origin_id) WHERE origin_device IS NOT NULL;
";

const LISTENS_NEVER_REUSE_AN_ID: &str = "
CREATE TABLE listens_rebuilt (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    track         INTEGER REFERENCES documents(id),
    began         INTEGER NOT NULL,
    heard_ms      INTEGER NOT NULL,
    counted       INTEGER NOT NULL DEFAULT 0,
    elsewhere     INTEGER NOT NULL DEFAULT 0,
    origin_device BLOB,
    origin_id     INTEGER
) STRICT;
INSERT INTO listens_rebuilt (id, track, began, heard_ms, counted, elsewhere, origin_device, origin_id)
    SELECT id, track, began, heard_ms, counted, elsewhere, origin_device, origin_id FROM listens;
DROP TABLE listens;
ALTER TABLE listens_rebuilt RENAME TO listens;
CREATE INDEX listens_track ON listens(track, began);
CREATE UNIQUE INDEX listens_by_origin ON listens(origin_device, origin_id) WHERE origin_device IS NOT NULL;
DELETE FROM sqlite_sequence WHERE name = 'listens';
INSERT INTO sqlite_sequence (name, seq)
    SELECT 'listens', max(
        coalesce((SELECT max(id) FROM listens), 0),
        coalesce((SELECT max(through) FROM submissions), 0)
    );
";

const DOCUMENTS_KEEP_WHEN_THEY_WERE_ADDED: &str = "
ALTER TABLE documents ADD COLUMN added INTEGER;
CREATE TRIGGER documents_dated AFTER INSERT ON documents BEGIN UPDATE documents SET added = unixepoch() WHERE id = new.id AND new.added IS NULL; END;
";

const DOCUMENTS_FROM_BEFORE_DATES_TAKE_THEIR_FILES_TIME: &str = "
UPDATE documents SET added = CASE WHEN modified > 0 THEN min(modified / 1000, unixepoch()) ELSE unixepoch() END WHERE added IS NULL;
";

const STUDIES_KEEP_WHAT_WAS_HEARD: &str = "
ALTER TABLE studies ADD COLUMN agreement TEXT;
CREATE TRIGGER documents_forget_what_was_heard AFTER UPDATE OF size, modified ON documents WHEN old.size IS NOT new.size OR old.modified IS NOT new.modified BEGIN UPDATE studies SET agreement = NULL WHERE track = new.id; END;
";

const PLAYLISTS_FILL_THEMSELVES_FROM_A_SAVED_SEARCH: &str = "
CREATE TABLE playlist_queries (
    playlist INTEGER PRIMARY KEY REFERENCES playlists(id) ON DELETE CASCADE,
    text     TEXT,
    sort     INTEGER NOT NULL,
    reading  INTEGER NOT NULL,
    max_rows INTEGER
) STRICT;
";

const MIGRATIONS: &[&str] = &[
    V1,
    LISTENS_NAME_THEIR_ORIGIN,
    LISTENS_NEVER_REUSE_AN_ID,
    DOCUMENTS_KEEP_WHEN_THEY_WERE_ADDED,
    DOCUMENTS_FROM_BEFORE_DATES_TAKE_THEIR_FILES_TIME,
    STUDIES_KEEP_WHAT_WAS_HEARD,
    PLAYLISTS_FILL_THEMSELVES_FROM_A_SAVED_SEARCH,
];

#[cfg(test)]
pub(crate) const FIRST_VERSION: &str = V1;

const V1: &str = "CREATE TABLE documents (
    id            INTEGER PRIMARY KEY,
    uri           TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    title         TEXT NOT NULL,
    artist        TEXT NOT NULL,
    album         TEXT NOT NULL,
    duration_ms   INTEGER NOT NULL,
    rate          INTEGER NOT NULL,
    codec         TEXT NOT NULL,
    favourite     INTEGER NOT NULL DEFAULT 0,
    album_artist  TEXT NOT NULL DEFAULT '',
    genre         TEXT NOT NULL DEFAULT '',
    date          TEXT NOT NULL DEFAULT '',
    track_number  INTEGER,
    disc_number   INTEGER,
    bits          INTEGER,
    channels      INTEGER NOT NULL DEFAULT 0,
    recording     TEXT,
    release       TEXT,
    artist_mbid   TEXT,
    art_revision  INTEGER NOT NULL DEFAULT 0,
    asked         INTEGER,
    answered      INTEGER,
    asks          INTEGER NOT NULL DEFAULT 0,
    refusals      INTEGER NOT NULL DEFAULT 0,
    release_group TEXT,
    album_key     TEXT,
    isrc          TEXT,
    art_changed   INTEGER NOT NULL DEFAULT 0,
    missing       INTEGER NOT NULL DEFAULT 0,
    size          INTEGER,
    modified      INTEGER,
    indexed_by    INTEGER
) STRICT;

CREATE TABLE resume (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    row_at      INTEGER NOT NULL,
    position_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE queue (
    position INTEGER PRIMARY KEY,
    track    INTEGER NOT NULL REFERENCES documents(id)
) STRICT;

CREATE TABLE lyrics (
    track    INTEGER PRIMARY KEY REFERENCES documents(id),
    text     TEXT,
    document TEXT,
    origin   TEXT NOT NULL,
    asked    INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE listens (
    id        INTEGER PRIMARY KEY,
    track     INTEGER REFERENCES documents(id),
    began     INTEGER NOT NULL,
    heard_ms  INTEGER NOT NULL,
    counted   INTEGER NOT NULL DEFAULT 0,
    elsewhere INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE studies (
    track         INTEGER PRIMARY KEY REFERENCES documents(id),
    verdict       TEXT NOT NULL,
    peak          REAL NOT NULL,
    rms           REAL NOT NULL,
    true_peak     REAL NOT NULL,
    loudness      REAL,
    dynamic_range INTEGER,
    clipped       INTEGER NOT NULL,
    bits          INTEGER
) STRICT;

CREATE TABLE playlists (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    folded   TEXT NOT NULL UNIQUE,
    revision INTEGER NOT NULL DEFAULT 0,
    created  INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE playlist_rows (
    playlist INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    track    INTEGER NOT NULL REFERENCES documents(id),
    PRIMARY KEY(playlist, position)
) STRICT;

CREATE TABLE submissions (
    service TEXT PRIMARY KEY,
    through INTEGER NOT NULL
) STRICT;

CREATE TABLE wanted (
    id            INTEGER PRIMARY KEY,
    identity      TEXT NOT NULL UNIQUE,
    recording     TEXT NOT NULL,
    title         TEXT NOT NULL,
    artist        TEXT NOT NULL,
    album         TEXT NOT NULL,
    duration_ms   INTEGER,
    isrcs         TEXT NOT NULL,
    added         INTEGER NOT NULL DEFAULT (unixepoch()),
    tried         INTEGER,
    misses        INTEGER NOT NULL DEFAULT 0,
    offered_at    INTEGER,
    release       TEXT NOT NULL DEFAULT '',
    release_group TEXT NOT NULL DEFAULT '',
    artist_id     TEXT NOT NULL DEFAULT '',
    disc          INTEGER,
    position      INTEGER
) STRICT;

CREATE TABLE received (
    id          INTEGER PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    extension   TEXT NOT NULL,
    title       TEXT NOT NULL,
    artist      TEXT NOT NULL,
    album       TEXT NOT NULL,
    recording   TEXT,
    duration_ms INTEGER
) STRICT;

CREATE TABLE album_art (
    album_key     TEXT PRIMARY KEY,
    release       TEXT,
    release_group TEXT,
    origin        TEXT,
    cover_asked   INTEGER,
    asked         INTEGER,
    answered      INTEGER,
    asks          INTEGER NOT NULL DEFAULT 0,
    refusals      INTEGER NOT NULL DEFAULT 0,
    picture       INTEGER REFERENCES pictures(id)
) STRICT;

CREATE TABLE artists (
    mbid           TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    kind           TEXT,
    country        TEXT,
    area           TEXT,
    genres         TEXT,
    links          TEXT,
    portrait       BLOB,
    portrait_asked INTEGER,
    asked          INTEGER,
    answered       INTEGER,
    asks           INTEGER NOT NULL DEFAULT 0,
    refusals       INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE enrichment (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    started INTEGER NOT NULL,
    refresh INTEGER NOT NULL
) STRICT;

CREATE TABLE refused_offers (
    identity   TEXT NOT NULL,
    taken_from TEXT NOT NULL,
    refused    INTEGER NOT NULL,
    PRIMARY KEY(identity, taken_from)
) STRICT;

CREATE TABLE loves_told (
    service   TEXT NOT NULL,
    recording TEXT NOT NULL,
    PRIMARY KEY (service, recording)
) STRICT, WITHOUT ROWID;

CREATE TABLE pictures (
    id    INTEGER PRIMARY KEY,
    crc   INTEGER NOT NULL,
    bytes BLOB NOT NULL
) STRICT;

CREATE TABLE artwork (
    track   INTEGER PRIMARY KEY REFERENCES documents(id),
    picture INTEGER NOT NULL REFERENCES pictures(id),
    origin  TEXT NOT NULL
) STRICT;

CREATE INDEX listens_track ON listens(track, began);

CREATE INDEX documents_album_key ON documents(album_key);

CREATE INDEX documents_art_changed ON documents(art_changed);

CREATE INDEX pictures_crc ON pictures(crc);

CREATE INDEX artwork_picture ON artwork(picture);

CREATE INDEX documents_missing ON documents(id) WHERE missing = 1;

CREATE INDEX documents_folded_name ON documents(folded(name));

CREATE INDEX documents_folded_title ON documents(folded(title));

CREATE INDEX documents_recording ON documents(recording);

CREATE TRIGGER documents_keyed AFTER INSERT ON documents BEGIN UPDATE documents SET album_key = CASE WHEN trim(new.album) = '' THEN NULL ELSE folded(new.album) || char(31) || folded(CASE WHEN trim(new.album_artist) = '' THEN new.artist ELSE new.album_artist END) END WHERE id = new.id; END;

CREATE TRIGGER documents_rekeyed AFTER UPDATE OF album, artist, album_artist ON documents BEGIN UPDATE documents SET album_key = CASE WHEN trim(new.album) = '' THEN NULL ELSE folded(new.album) || char(31) || folded(CASE WHEN trim(new.album_artist) = '' THEN new.artist ELSE new.album_artist END) END WHERE id = new.id; END;

CREATE TRIGGER documents_art_changed AFTER UPDATE OF art_revision ON documents BEGIN UPDATE documents SET art_changed = (SELECT max(art_changed) FROM documents) + 1 WHERE id = new.id; END;
";

const TABLES_SYNC_READS: &[&str] = &[
    "documents",
    "playlists",
    "playlist_rows",
    "playlist_queries",
    "received",
];

pub(crate) struct Catalog(
    pub(crate) Mutex<Connection>,
    Arc<AtomicU64>,
    pub(crate) Mutex<HashMap<(i64, i64), i64>>,
    Mutex<Option<KeptTrack>>,
);

struct KeptTrack {
    uri: String,
    revision: u64,
    track: Track,
}

const ROWS_READ_PER_HOLD: usize = 256;

impl Catalog {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut connection =
            Connection::open(path).map_err(|source| Error::store(StoreOp::Open, source))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        connection
            .create_scalar_function(
                "folded",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
            )
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::LayOut, source))?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let laid = usize::try_from(version).unwrap_or(usize::MAX);
        if laid > MIGRATIONS.len() {
            return Err(Error::CatalogVersion { found: version });
        }
        for step in &MIGRATIONS[laid..] {
            transaction
                .execute_batch(step)
                .map_err(|source| Error::store(StoreOp::LayOut, source))?;
        }
        transaction
            .pragma_update(
                None,
                "user_version",
                u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX),
            )
            .map_err(|source| Error::store(StoreOp::LayOut, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::LayOut, source))?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",
            )
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        let revision = Arc::new(AtomicU64::new(0));
        let written = Arc::clone(&revision);
        connection
            .update_hook(Some(
                move |action: Action, _database: &str, table: &str, _row: i64| {
                    if TABLES_SYNC_READS.contains(&table)
                        || (table == "listens" && action != Action::SQLITE_UPDATE)
                    {
                        written.fetch_add(1, Ordering::AcqRel);
                    }
                },
            ))
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        Ok(Self(
            Mutex::new(connection),
            revision,
            Mutex::new(HashMap::new()),
            Mutex::new(None),
        ))
    }

    pub(crate) fn revision(&self) -> u64 {
        self.1.load(Ordering::Acquire)
    }

    pub(crate) fn upsert(&self, track: Track) -> Result<Track> {
        let connection = self.0.lock();
        connection
            .execute(
                "INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(uri) DO UPDATE SET name = excluded.name, title = CASE WHEN excluded.title = excluded.name AND documents.title != documents.name THEN documents.title ELSE excluded.title END,
            artist = coalesce(nullif(excluded.artist, ''), documents.artist), album = coalesce(nullif(excluded.album, ''), documents.album), duration_ms = excluded.duration_ms,
            rate = excluded.rate, codec = excluded.codec, missing = 0",
                params![
                    track.uri,
                    track.name,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_ms.min(i64::MAX as u64) as i64,
                    track.rate,
                    track.codec
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        connection
            .query_row("SELECT * FROM documents WHERE uri = ?1", [track.uri], read)
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    #[cfg(test)]
    pub(crate) fn tracks(&self, text: &str, offset: u32, limit: u32) -> Result<Vec<Track>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT * FROM documents WHERE missing = 0 AND
            instr(folded(title || ' ' || artist || ' ' || album || ' ' || album_artist), folded(?1)) > 0
            ORDER BY title COLLATE NOCASE, id LIMIT ?2 OFFSET ?3",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let found = query
            .query_map(params![text, limit.min(500), offset], read)
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        found
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn uris(&self) -> Result<Vec<String>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare("SELECT uri FROM documents ORDER BY id")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let found = query
            .query_map([], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        found
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn track(&self, id: u64) -> Result<Track> {
        self.0
            .lock()
            .query_row("SELECT * FROM documents WHERE id = ?1", [id as i64], read)
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .ok_or(Error::UnknownTrack)
    }

    pub(crate) fn track_uri(&self, uri: &str) -> Result<Track> {
        self.0
            .lock()
            .query_row("SELECT * FROM documents WHERE uri = ?1", [uri], read)
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .ok_or(Error::UnknownTrack)
    }

    pub(crate) fn kept_track_uri(&self, uri: &str) -> Result<Track> {
        let revision = self.revision();
        let mut kept = self.3.lock();
        if let Some(kept) = kept
            .as_ref()
            .filter(|kept| kept.revision == revision && kept.uri == uri)
        {
            return Ok(kept.track.clone());
        }
        let track = self.track_uri(uri)?;
        *kept = Some(KeptTrack {
            uri: uri.to_owned(),
            revision,
            track: track.clone(),
        });
        Ok(track)
    }

    pub(crate) fn tracks_at(&self, uris: &[String]) -> Result<Vec<Track>> {
        self.each_in_holds(
            uris,
            "SELECT * FROM documents WHERE uri = ?1",
            |query, uri| query.query_row([uri], read),
        )
    }

    pub(crate) fn ids_at(&self, uris: &[String]) -> Result<Vec<u64>> {
        self.each_in_holds(
            uris,
            "SELECT id FROM documents WHERE uri = ?1",
            |query, uri| query.query_row([uri], |row| row.get::<_, i64>(0).map(|id| id as u64)),
        )
    }

    pub(crate) fn tracks_by_id(&self, ids: &[u64]) -> Result<Vec<Track>> {
        self.each_in_holds(ids, "SELECT * FROM documents WHERE id = ?1", |query, id| {
            query.query_row([*id as i64], read)
        })
    }

    fn each_in_holds<Key, Row>(
        &self,
        keys: &[Key],
        sql: &str,
        mut asked: impl FnMut(&mut CachedStatement<'_>, &Key) -> rusqlite::Result<Row>,
    ) -> Result<Vec<Row>> {
        let mut rows = Vec::with_capacity(keys.len());
        for held in keys.chunks(ROWS_READ_PER_HOLD) {
            let connection = self.0.lock();
            let mut query = connection
                .prepare_cached(sql)
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            for key in held {
                rows.push(
                    asked(&mut query, key)
                        .optional()
                        .map_err(|source| Error::store(StoreOp::Read, source))?
                        .ok_or(Error::UnknownTrack)?,
                );
            }
        }
        Ok(rows)
    }

    pub(crate) fn favourite(&self, id: u64, favourite: bool) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE documents SET favourite = ?2 WHERE id = ?1",
                params![id as i64, favourite],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn save_queue(&self, ids: &[u64], at: u32, position_ms: u64) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute("DELETE FROM queue", [])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        for (position, id) in ids.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO queue VALUES (?1, ?2)",
                    params![position as i64, *id as i64],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction.execute("INSERT INTO resume VALUES (1, ?1, ?2) ON CONFLICT(id) DO UPDATE SET row_at = excluded.row_at, position_ms = excluded.position_ms", params![at, position_ms.min(i64::MAX as u64) as i64])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub(crate) fn save_place(&self, row: u32, position_ms: u64) -> Result<()> {
        self.0.lock().execute("INSERT INTO resume VALUES (1, ?1, ?2) ON CONFLICT(id) DO UPDATE SET row_at = excluded.row_at, position_ms = excluded.position_ms",
            params![row, position_ms.min(i64::MAX as u64) as i64]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn resume(&self) -> Result<(Vec<u64>, u32, u64)> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare("SELECT track FROM queue ORDER BY position")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let ids = query
            .query_map([], |row| row.get::<_, i64>(0).map(|id| id as u64))
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<u64>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let (at, position) = connection
            .query_row(
                "SELECT row_at, position_ms FROM resume WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .unwrap_or((0, 0));
        let (at, position) = place_among_rows(&connection, at, position)
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok((ids, at as u32, position))
    }
}

pub(crate) fn place_among_rows(
    connection: &rusqlite::Connection,
    row_at: i64,
    position_ms: u64,
) -> rusqlite::Result<(usize, u64)> {
    let (before, standing, rows) = connection.query_row(
        "SELECT (SELECT count(*) FROM queue WHERE position < ?1), EXISTS(SELECT 1 FROM queue WHERE position = ?1), (SELECT count(*) FROM queue)",
        [row_at],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;
    let before = usize::try_from(before).unwrap_or_default();
    let last = usize::try_from(rows).unwrap_or_default().saturating_sub(1);

    if standing {
        Ok((before, position_ms))
    } else {
        Ok((before.min(last), 0))
    }
}

pub(crate) fn picture_crc(bytes: &[u8]) -> i64 {
    i64::from(crc32fast::hash(bytes))
}

pub(crate) fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get::<_, i64>(0)? as u64,
        uri: row.get(1)?,
        name: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        album: row.get(5)?,
        duration_ms: row.get::<_, i64>(6)? as u64,
        rate: row.get(7)?,
        codec: row.get(8)?,
        favourite: row.get(9)?,
        album_artist: row.get(10)?,
        genre: row.get(11)?,
        date: row.get(12)?,
        track_number: row.get(13)?,
        disc_number: row.get(14)?,
        bits: row.get(15)?,
        channels: row.get(16)?,
        recording: row.get(17)?,
        release: row.get(18)?,
        artist_mbid: row.get(19)?,
        art_revision: row.get::<_, i64>(20)? as u64,
        queue_id: None,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn rescanning_and_reopening_keep_identity_favourites_and_the_saved_place() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-mobile-{unique}.sqlite"));
        let catalog = Catalog::open(&path).expect("a catalog");
        let first = catalog
            .upsert(Track {
                id: 0,
                uri: "content://music/a.flac".into(),
                name: "a.flac".into(),
                title: "A".into(),
                artist: "Artist".into(),
                album: "Album".into(),
                duration_ms: 3000,
                rate: 44100,
                codec: "FLAC".into(),
                favourite: false,
                ..Track::default()
            })
            .expect("an indexed document");
        catalog.favourite(first.id, true).expect("a favourite");
        let rescanned = catalog
            .upsert(Track {
                title: "Renamed".into(),
                ..first.clone()
            })
            .expect("a rescan");
        assert_eq!(first.id, rescanned.id);
        assert!(rescanned.favourite);
        catalog
            .save_queue(&[first.id], 0, 1500)
            .expect("a saved queue");
        catalog
            .save_place(0, 2000)
            .expect("a moved place keeps the queue");
        drop(catalog);
        let reopened = Catalog::open(&path).expect("an existing catalog");
        assert_eq!(
            reopened.resume().expect("a saved place"),
            (vec![first.id], 0, 2000)
        );
        assert_eq!(
            reopened.tracks("Renamed", 0, 100).expect("a search").len(),
            1
        );
        assert_eq!(
            reopened
                .tracks("missing", 0, 100)
                .expect("an empty search")
                .len(),
            0
        );
        drop(reopened);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_deleted_listens_id_is_never_handed_to_the_next_listen() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let connection = catalog.0.lock();

        connection
            .execute_batch(
                "INSERT INTO listens (track, began, heard_ms) VALUES (NULL, 1, 1000), (NULL, 2, 1000);
                 DELETE FROM listens WHERE id = 2;
                 INSERT INTO listens (track, began, heard_ms) VALUES (NULL, 3, 1000);",
            )
            .expect("listens written and one deleted");
        let newest: i64 = connection
            .query_row("SELECT max(id) FROM listens", [], |row| row.get(0))
            .expect("the newest id");

        assert_eq!(newest, 3);
    }

    #[test]
    fn a_catalog_whose_listens_reused_ids_is_carried_forward_past_what_was_submitted() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-mobile-listens-{unique}.sqlite"));
        let connection = Connection::open(&path).expect("a database");
        connection
            .create_scalar_function(
                "folded",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
            )
            .expect("the fold");
        connection.execute_batch(V1).expect("the first version");
        connection
            .execute_batch(LISTENS_NAME_THEIR_ORIGIN)
            .expect("the second");
        connection
            .execute_batch(
                "INSERT INTO listens (track, began, heard_ms) VALUES (NULL, 1, 1000), (NULL, 2, 1000);
                 INSERT INTO submissions (service, through) VALUES ('listenbrainz', 5);",
            )
            .expect("listens and a mark");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("its stamp");
        drop(connection);

        let opened = Catalog::open(&path).expect("carried forward");
        let held = opened.0.lock();
        held.execute(
            "INSERT INTO listens (track, began, heard_ms) VALUES (NULL, 3, 1000)",
            [],
        )
        .expect("a listen");
        let next: i64 = held
            .query_row("SELECT max(id) FROM listens", [], |row| row.get(0))
            .expect("the newest id");
        let kept: i64 = held
            .query_row(
                "SELECT count(*) FROM listens WHERE id IN (1, 2)",
                [],
                |row| row.get(0),
            )
            .expect("the old rows");

        assert_eq!(next, 6);
        assert_eq!(kept, 2);
        drop(held);
        drop(opened);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_catalog_of_before_dates_added_dates_its_rows_by_their_files_and_forgets_a_hearing_with_the_file()
     {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-mobile-added-{unique}.sqlite"));
        let connection = Connection::open(&path).expect("a database");
        connection
            .create_scalar_function(
                "folded",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
            )
            .expect("the fold");
        for step in [V1, LISTENS_NAME_THEIR_ORIGIN, LISTENS_NEVER_REUSE_AN_ID] {
            connection.execute_batch(step).expect("an earlier version");
        }
        connection
            .execute_batch(
                "INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec, size, modified)
                 VALUES ('content://music/old.flac', 'old.flac', 'Old', 'A', 'B', 1000, 44100, 'flac', 10, 1700000000123);
                 INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec)
                 VALUES ('content://music/unstamped.flac', 'unstamped.flac', 'Unstamped', 'A', 'B', 1000, 44100, 'flac');
                 INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec, size, modified)
                 VALUES ('content://music/future.flac', 'future.flac', 'Future', 'A', 'B', 1000, 44100, 'flac', 10, 99999999999999)",
            )
            .expect("documents of before");
        connection
            .pragma_update(None, "user_version", 3)
            .expect("its stamp");
        drop(connection);

        let opened = Catalog::open(&path).expect("migrated");
        opened
            .upsert(Track {
                uri: "content://music/new.flac".into(),
                name: "new.flac".into(),
                title: "New".into(),
                ..Track::default()
            })
            .expect("a new document");
        let held = opened.0.lock();
        let added = |uri: &str| -> Option<i64> {
            held.query_row("SELECT added FROM documents WHERE uri = ?1", [uri], |row| {
                row.get(0)
            })
            .expect("a document")
        };
        let now: i64 = held
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .expect("a clock");

        assert_eq!(added("content://music/old.flac"), Some(1_700_000_000));
        assert!(added("content://music/unstamped.flac").is_some_and(|at| at > 1_700_000_000));
        assert!(added("content://music/future.flac").is_some_and(|at| at <= now));
        assert!(added("content://music/new.flac").is_some_and(|at| at > 0));

        held.execute_batch(
            "INSERT INTO studies (track, verdict, peak, rms, true_peak, clipped, agreement)
             SELECT id, 'genuine', 0.5, 0.1, 0.5, 0, 'disagrees' FROM documents WHERE name = 'old.flac'",
        )
        .expect("a hearing");
        let heard = || -> Option<String> {
            held.query_row(
                "SELECT agreement FROM studies WHERE track = (SELECT id FROM documents WHERE name = 'old.flac')",
                [],
                |row| row.get(0),
            )
            .expect("a study")
        };
        held.execute(
            "UPDATE documents SET title = 'Older' WHERE name = 'old.flac'",
            [],
        )
        .expect("a retitle");
        let after_retitle = heard();
        held.execute("UPDATE documents SET size = 11 WHERE name = 'old.flac'", [])
            .expect("a changed file");
        let after_change = heard();

        assert_eq!(after_retitle.as_deref(), Some("disagrees"));
        assert_eq!(after_change, None);
        drop(held);
        drop(opened);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rows_read_across_several_holds_keep_the_order_asked() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let songs = (0..ROWS_READ_PER_HOLD * 2 + 3)
            .map(|at| {
                catalog
                    .upsert(Track {
                        uri: format!("content://music/{at}.flac"),
                        name: format!("{at}.flac"),
                        title: format!("{at}"),
                        ..Track::default()
                    })
                    .expect("an indexed document")
            })
            .rev()
            .collect::<Vec<_>>();
        let uris = songs
            .iter()
            .map(|song| song.uri.clone())
            .collect::<Vec<_>>();
        let ids = songs.iter().map(|song| song.id).collect::<Vec<_>>();

        let read = catalog.tracks_at(&uris).expect("rows by uri");
        let numbered = catalog.ids_at(&uris).expect("ids by uri");
        let found = catalog.tracks_by_id(&ids).expect("rows by id");

        assert_eq!(read.iter().map(|song| song.id).collect::<Vec<_>>(), ids);
        assert_eq!(numbered, ids);
        assert_eq!(
            found
                .iter()
                .map(|song| song.uri.clone())
                .collect::<Vec<_>>(),
            uris
        );
        assert!(matches!(
            catalog.ids_at(&["content://music/none.flac".into()]),
            Err(Error::UnknownTrack)
        ));
    }
}
