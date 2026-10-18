use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{naming::named_within, writer::Held};
use rusqlite::{
    CachedStatement, Connection, OptionalExtension, TransactionBehavior, functions::FunctionFlags,
    hooks::Action, params,
};

use crate::{Error, Result, StoreOp, Track};

const V1: &str = "CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    uri TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    rate INTEGER NOT NULL,
    codec TEXT NOT NULL,
    favourite INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE resume (id INTEGER PRIMARY KEY CHECK (id = 1), row_at INTEGER NOT NULL, position_ms INTEGER NOT NULL) STRICT;
CREATE TABLE queue (position INTEGER PRIMARY KEY, track INTEGER NOT NULL REFERENCES documents(id)) STRICT;";

const V2: &str = "ALTER TABLE documents ADD COLUMN album_artist TEXT NOT NULL DEFAULT '';
ALTER TABLE documents ADD COLUMN genre TEXT NOT NULL DEFAULT '';
ALTER TABLE documents ADD COLUMN date TEXT NOT NULL DEFAULT '';
ALTER TABLE documents ADD COLUMN track_number INTEGER;
ALTER TABLE documents ADD COLUMN disc_number INTEGER;
ALTER TABLE documents ADD COLUMN bits INTEGER;
ALTER TABLE documents ADD COLUMN channels INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN recording TEXT;
ALTER TABLE documents ADD COLUMN release TEXT;
ALTER TABLE documents ADD COLUMN artist_mbid TEXT;
ALTER TABLE documents ADD COLUMN art_revision INTEGER NOT NULL DEFAULT 0;
CREATE TABLE artwork (track INTEGER PRIMARY KEY REFERENCES documents(id), bytes BLOB NOT NULL, origin TEXT NOT NULL) STRICT;
CREATE TABLE lyrics (track INTEGER PRIMARY KEY REFERENCES documents(id), text TEXT, document TEXT, origin TEXT NOT NULL, asked INTEGER NOT NULL DEFAULT (unixepoch())) STRICT;
CREATE TABLE listens (id INTEGER PRIMARY KEY, track INTEGER REFERENCES documents(id), began INTEGER NOT NULL, heard_ms INTEGER NOT NULL, counted INTEGER NOT NULL DEFAULT 0) STRICT;
CREATE INDEX listens_track ON listens(track, began);
CREATE TABLE studies (track INTEGER PRIMARY KEY REFERENCES documents(id), verdict TEXT NOT NULL, peak REAL NOT NULL, rms REAL NOT NULL, true_peak REAL NOT NULL, loudness REAL, dynamic_range INTEGER, clipped INTEGER NOT NULL, bits INTEGER) STRICT;";

const V3: &str = "CREATE TABLE playlists (id INTEGER PRIMARY KEY, name TEXT NOT NULL, folded TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL DEFAULT 0) STRICT;
CREATE TABLE playlist_rows (playlist INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE, position INTEGER NOT NULL, track INTEGER NOT NULL REFERENCES documents(id), PRIMARY KEY(playlist, position)) STRICT;";

const V4: &str =
    "CREATE TABLE submissions (service TEXT PRIMARY KEY, through INTEGER NOT NULL) STRICT;";

const V5: &str = "CREATE TABLE wanted (id INTEGER PRIMARY KEY, identity TEXT NOT NULL UNIQUE, recording TEXT NOT NULL, title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, duration_ms INTEGER, isrcs TEXT NOT NULL, added INTEGER NOT NULL DEFAULT (unixepoch())) STRICT;";

const V6: &str = "ALTER TABLE listens ADD COLUMN elsewhere INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playlists ADD COLUMN created INTEGER NOT NULL DEFAULT 0;
CREATE TABLE received (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, extension TEXT NOT NULL, title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, recording TEXT, duration_ms INTEGER) STRICT;";

const V7: &str = "ALTER TABLE documents ADD COLUMN asked INTEGER;
ALTER TABLE documents ADD COLUMN answered INTEGER;
ALTER TABLE documents ADD COLUMN asks INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN refusals INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN release_group TEXT;
ALTER TABLE documents ADD COLUMN album_key TEXT;
ALTER TABLE documents ADD COLUMN isrc TEXT;
ALTER TABLE documents ADD COLUMN art_changed INTEGER NOT NULL DEFAULT 0;
UPDATE documents SET album_key = CASE WHEN trim(album) = '' THEN NULL ELSE folded(album) || char(31) || folded(CASE WHEN trim(album_artist) = '' THEN artist ELSE album_artist END) END;
CREATE INDEX documents_album_key ON documents(album_key);
CREATE INDEX documents_art_changed ON documents(art_changed);
CREATE TRIGGER documents_keyed AFTER INSERT ON documents BEGIN UPDATE documents SET album_key = CASE WHEN trim(new.album) = '' THEN NULL ELSE folded(new.album) || char(31) || folded(CASE WHEN trim(new.album_artist) = '' THEN new.artist ELSE new.album_artist END) END WHERE id = new.id; END;
CREATE TRIGGER documents_rekeyed AFTER UPDATE OF album, artist, album_artist ON documents BEGIN UPDATE documents SET album_key = CASE WHEN trim(new.album) = '' THEN NULL ELSE folded(new.album) || char(31) || folded(CASE WHEN trim(new.album_artist) = '' THEN new.artist ELSE new.album_artist END) END WHERE id = new.id; END;
CREATE TRIGGER documents_art_changed AFTER UPDATE OF art_revision ON documents BEGIN UPDATE documents SET art_changed = (SELECT max(art_changed) FROM documents) + 1 WHERE id = new.id; END;
CREATE TABLE album_art (album_key TEXT PRIMARY KEY, release TEXT, release_group TEXT, bytes BLOB, origin TEXT, cover_asked INTEGER, asked INTEGER, answered INTEGER, asks INTEGER NOT NULL DEFAULT 0, refusals INTEGER NOT NULL DEFAULT 0) STRICT;
CREATE TABLE artists (mbid TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT, country TEXT, area TEXT, genres TEXT, links TEXT, portrait BLOB, portrait_asked INTEGER, asked INTEGER, answered INTEGER, asks INTEGER NOT NULL DEFAULT 0, refusals INTEGER NOT NULL DEFAULT 0) STRICT;
CREATE TABLE enrichment (id INTEGER PRIMARY KEY CHECK (id = 1), started INTEGER NOT NULL, refresh INTEGER NOT NULL) STRICT;
ALTER TABLE wanted ADD COLUMN tried INTEGER;
ALTER TABLE wanted ADD COLUMN misses INTEGER NOT NULL DEFAULT 0;
ALTER TABLE wanted ADD COLUMN offered_at INTEGER;";

const V8: &str = "CREATE TABLE refused_offers (identity TEXT NOT NULL, taken_from TEXT NOT NULL, refused INTEGER NOT NULL, PRIMARY KEY(identity, taken_from)) STRICT;";

const V9: &str = "ALTER TABLE documents ADD COLUMN missing INTEGER NOT NULL DEFAULT 0;";

const V10: &str = "CREATE TABLE loves_told (service TEXT NOT NULL, recording TEXT NOT NULL, PRIMARY KEY (service, recording)) STRICT, WITHOUT ROWID;";

const V11: &str = "ALTER TABLE wanted ADD COLUMN release TEXT NOT NULL DEFAULT '';
ALTER TABLE wanted ADD COLUMN release_group TEXT NOT NULL DEFAULT '';
ALTER TABLE wanted ADD COLUMN artist_id TEXT NOT NULL DEFAULT '';
ALTER TABLE wanted ADD COLUMN disc INTEGER;
ALTER TABLE wanted ADD COLUMN position INTEGER;";

const V12: &str = "CREATE TABLE pictures (id INTEGER PRIMARY KEY, crc INTEGER NOT NULL, bytes BLOB NOT NULL) STRICT;
CREATE INDEX pictures_crc ON pictures(crc);
INSERT INTO pictures(crc, bytes) SELECT picture_crc(bytes), bytes FROM (SELECT bytes FROM artwork UNION SELECT bytes FROM album_art WHERE bytes IS NOT NULL);
CREATE TABLE artwork_v12 (track INTEGER PRIMARY KEY REFERENCES documents(id), picture INTEGER NOT NULL REFERENCES pictures(id), origin TEXT NOT NULL) STRICT;
INSERT INTO artwork_v12 SELECT a.track, (SELECT p.id FROM pictures p WHERE p.crc = picture_crc(a.bytes) AND p.bytes = a.bytes), a.origin FROM artwork a;
DROP TABLE artwork;
ALTER TABLE artwork_v12 RENAME TO artwork;
CREATE INDEX artwork_picture ON artwork(picture);
ALTER TABLE album_art ADD COLUMN picture INTEGER REFERENCES pictures(id);
UPDATE album_art SET picture = (SELECT p.id FROM pictures p WHERE p.crc = picture_crc(album_art.bytes) AND p.bytes = album_art.bytes) WHERE bytes IS NOT NULL;
ALTER TABLE album_art DROP COLUMN bytes;";

const V13: &str = "ALTER TABLE documents ADD COLUMN size INTEGER;
ALTER TABLE documents ADD COLUMN modified INTEGER;
ALTER TABLE documents ADD COLUMN indexed_by INTEGER;
CREATE INDEX documents_missing ON documents(id) WHERE missing = 1;
CREATE INDEX documents_folded_name ON documents(folded(name));
CREATE INDEX documents_folded_title ON documents(folded(title));
CREATE INDEX documents_recording ON documents(recording);";

const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12, V13];

const TABLES_SYNC_READS: &[&str] = &["documents", "playlists", "playlist_rows", "received"];

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
        connection
            .create_scalar_function(
                "picture_crc",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(picture_crc(context.get_raw(0).as_blob()?)),
            )
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if version as usize > MIGRATIONS.len() {
            return Err(Error::CatalogVersion { found: version });
        }
        if version > 0 && (version as usize) < MIGRATIONS.len() {
            keep_before_migrating(&connection, version)?;
        }
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if version as usize > MIGRATIONS.len() {
            return Err(Error::CatalogVersion { found: version });
        }
        for migration in MIGRATIONS.iter().skip(version as usize) {
            transaction
                .execute_batch(migration)
                .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        }
        transaction
            .pragma_update(None, "user_version", MIGRATIONS.len() as u32)
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
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
        Ok((ids, at, position))
    }
}

pub(crate) fn picture_crc(bytes: &[u8]) -> i64 {
    i64::from(crc32fast::hash(bytes))
}

fn keep_before_migrating(connection: &Connection, version: u32) -> Result<()> {
    let Some(path) = connection
        .path()
        .filter(|path| !path.is_empty())
        .map(Path::new)
    else {
        return Ok(());
    };
    let check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|source| Error::store(StoreOp::Read, source))?;
    if check != "ok" {
        return Err(Error::CatalogDamaged);
    }
    let kept = path.with_file_name(named_within(
        "",
        path.file_name().unwrap_or_default(),
        format!(".before-v{version}"),
    ));
    if kept.exists() {
        return Ok(());
    }
    let staged = kept.with_file_name(named_within(
        "",
        kept.file_name().unwrap_or_default(),
        ".part",
    ));
    let lock = kept.with_file_name(named_within(
        "",
        kept.file_name().unwrap_or_default(),
        ".lock",
    ));
    let held = match Held::taken_over(&lock) {
        Ok(held) => held,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(source) => return Err(Error::CatalogFile { path: lock, source }),
    };
    match fs::remove_file(&staged) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::CatalogFile {
                path: staged,
                source,
            });
        }
    }
    let target = staged.to_str().ok_or(Error::InvalidCatalogPath)?;
    connection
        .execute("VACUUM INTO ?1", [target])
        .map_err(|source| Error::store(StoreOp::Migrate, source))?;
    File::open(&staged)
        .and_then(|copy| copy.sync_all())
        .and_then(|()| fs::rename(&staged, &kept))
        .and_then(|()| {
            kept.parent().map_or(Ok(()), |folder| {
                File::open(folder).and_then(|folder| folder.sync_all())
            })
        })
        .map_err(|source| Error::CatalogFile { path: kept, source })?;
    if fs::remove_file(&lock).is_err() {
        tracing::debug!(lock = %lock.display(), "the backup's lock stays behind");
    }
    drop(held);
    Ok(())
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

#[cfg(test)]
mod migration_tests {
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn the_first_android_catalog_is_backed_up_and_migrated_without_losing_listener_state() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-migration-{unique}.sqlite"));
        let old = Connection::open(&path).expect("an old catalog");
        old.execute_batch(V1).expect("the shipped schema");
        old.execute_batch("PRAGMA user_version = 1; INSERT INTO documents VALUES (27, 'content://test/song.flac', 'song.flac', 'Song', 'Artist', 'Album', 30000, 44100, 'FLAC', 1); INSERT INTO queue VALUES (0, 27); INSERT INTO resume VALUES (1, 0, 17250);").expect("a favourite and saved queue");
        drop(old);
        let catalog = Catalog::open(&path).expect("a migrated catalog");
        assert_eq!(
            catalog.resume().expect("the saved place"),
            (vec![27], 0, 17250)
        );
        assert!(catalog.track(27).expect("the original id").favourite);
        catalog
            .keep_artwork(27, &[1, 2, 3], "test", false)
            .expect("new artwork storage");
        catalog
            .keep_lyrics(27, Some("[00:01.00]Words"), None, "embedded")
            .expect("new lyric storage");
        let kept = path.with_file_name(named_within(
            "",
            path.file_name().expect("a filename"),
            ".before-v1",
        ));
        let backup = Connection::open(&kept).expect("the previous catalog");
        let version: u32 = backup
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("a backup version");
        assert_eq!(version, 1);
        assert_eq!(
            backup
                .query_row("SELECT position_ms FROM resume", [], |row| row
                    .get::<_, i64>(0))
                .expect("the original place"),
            17250
        );
        drop(backup);
        drop(catalog);
        let reopened = Catalog::open(&path).expect("an idempotent reopen");
        assert_eq!(
            reopened.artwork(27).expect("kept artwork"),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            reopened
                .lyric_text(27)
                .expect("kept lyrics")
                .expect("a lyric row")
                .0
                .as_deref(),
            Some("[00:01.00]Words")
        );
        drop(reopened);
        fs::remove_file(path).expect("remove the test catalog");
        fs::remove_file(kept).expect("remove the test backup");
    }

    #[test]
    fn an_eleventh_catalog_keeps_each_cover_once_however_many_songs_and_albums_hold_it() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-migration-v11-{unique}.sqlite"));
        let old = Connection::open(&path).expect("an eleventh catalog");
        old.create_scalar_function(
            "folded",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
        )
        .expect("the fold");
        for migration in &MIGRATIONS[..11] {
            old.execute_batch(migration).expect("a shipped schema step");
        }
        old.execute_batch("PRAGMA user_version = 11; INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec) VALUES (1, 'content://test/a.flac', 'a.flac', 'A', 'X', 'One', 1000, 44100, 'FLAC'), (2, 'content://test/b.flac', 'b.flac', 'B', 'X', 'One', 1000, 44100, 'FLAC'), (3, 'content://test/c.flac', 'c.flac', 'C', 'Y', 'Two', 1000, 44100, 'FLAC'); INSERT INTO artwork VALUES (1, x'010203', 'folder'), (2, x'010203', 'folder'), (3, x'040506', 'embedded'); INSERT INTO album_art (album_key, bytes, origin) VALUES ('one', x'010203', 'archive'), ('two', x'070809', 'archive'), ('three', NULL, NULL);").expect("covers held by songs and albums");
        drop(old);

        let catalog = Catalog::open(&path).expect("a migrated catalog");
        let held: i64 = catalog
            .0
            .lock()
            .query_row("SELECT count(*) FROM pictures", [], |row| row.get(0))
            .expect("a picture count");
        let bare: i64 = catalog
            .0
            .lock()
            .query_row(
                "SELECT count(*) FROM album_art WHERE picture IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("an album without a picture");

        assert_eq!(held, 3);
        assert_eq!(bare, 1);
        assert_eq!(catalog.artwork(1).expect("a cover"), Some(vec![1, 2, 3]));
        assert_eq!(catalog.artwork(2).expect("a cover"), Some(vec![1, 2, 3]));
        assert_eq!(catalog.artwork(3).expect("a cover"), Some(vec![4, 5, 6]));

        drop(catalog);
        for suffix in ["", ".before-v11"] {
            let name = if suffix.is_empty() {
                path.clone()
            } else {
                path.with_file_name(named_within(
                    "",
                    path.file_name().expect("a filename"),
                    suffix,
                ))
            };
            fs::remove_file(name).expect("remove the test catalog");
        }
    }

    #[test]
    fn a_fifth_android_catalog_is_backed_up_and_its_listens_and_playlists_carried_into_sync() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-migration-v5-{unique}.sqlite"));
        let old = Connection::open(&path).expect("a fifth catalog");
        for migration in [V1, V2, V3, V4, V5] {
            old.execute_batch(migration).expect("a shipped schema step");
        }
        old.execute_batch("PRAGMA user_version = 5; INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec, favourite) VALUES (4, 'content://test/song.flac', 'song.flac', 'Song', 'Artist', 'Album', 30000, 44100, 'FLAC', 1); INSERT INTO listens (id, track, began, heard_ms, counted) VALUES (9, 4, 1700000000, 30000, 1); INSERT INTO playlists (id, name, folded, revision) VALUES (2, 'Evening', 'evening', 3); INSERT INTO playlist_rows VALUES (2, 0, 4); INSERT INTO submissions VALUES ('listenbrainz', 9);").expect("a listen, a playlist and a scrobbling mark");
        drop(old);

        let catalog = Catalog::open(&path).expect("a migrated catalog");
        let connection = catalog.0.lock();

        let listen: (i64, i64) = connection
            .query_row(
                "SELECT counted, elsewhere FROM listens WHERE id = 9",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the kept listen");
        let playlist: (String, i64, i64) = connection
            .query_row(
                "SELECT name, revision, created FROM playlists WHERE id = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the kept playlist");
        let waiting: i64 = connection
            .query_row("SELECT count(*) FROM received", [], |row| row.get(0))
            .expect("the received songs table");
        let mark: i64 = connection
            .query_row("SELECT through FROM submissions", [], |row| row.get(0))
            .expect("the kept scrobbling mark");

        assert_eq!(listen, (1, 0));
        assert_eq!(playlist, ("Evening".to_owned(), 3, 0));
        assert_eq!(waiting, 0);
        assert_eq!(mark, 9);

        drop(connection);
        drop(catalog);
        let kept = path.with_file_name(named_within(
            "",
            path.file_name().expect("a filename"),
            ".before-v5",
        ));
        let backup = Connection::open(&kept).expect("the previous catalog");
        let version: u32 = backup
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("a backup version");
        assert_eq!(version, 5);
        drop(backup);
        fs::remove_file(path).expect("remove the test catalog");
        fs::remove_file(kept).expect("remove the test backup");
    }

    #[test]
    fn a_sixth_android_catalog_keeps_its_listener_state_and_learns_its_album_keys() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-migration-v6-{unique}.sqlite"));
        let old = Connection::open(&path).expect("a sixth catalog");
        for migration in [V1, V2, V3, V4, V5, V6] {
            old.execute_batch(migration).expect("a shipped schema step");
        }
        old.execute_batch("PRAGMA user_version = 6; INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec, favourite) VALUES (12, 'content://test/one.flac', 'one.flac', 'One', 'Artist', 'Début', 30000, 44100, 'FLAC', 1), (13, 'content://test/two.flac', 'two.flac', 'Two', 'Artist', '', 30000, 44100, 'FLAC', 0); INSERT INTO queue VALUES (0, 13), (1, 12); INSERT INTO resume VALUES (1, 1, 9000); INSERT INTO wanted (id, identity, recording, title, artist, album, isrcs) VALUES (3, 'one', 'b1a9c0e9-d987-4042-ae91-78d6a3267d69', 'One', 'Artist', 'Début', '');").expect("a favourite, a saved queue and a want");
        drop(old);

        let catalog = Catalog::open(&path).expect("a migrated catalog");
        let connection = catalog.0.lock();

        let keys: Vec<Option<String>> = connection
            .prepare("SELECT album_key FROM documents ORDER BY id")
            .expect("a key query")
            .query_map([], |row| row.get(0))
            .expect("the keys")
            .collect::<rusqlite::Result<_>>()
            .expect("every key");
        let want: (i64, Option<i64>, Option<i64>) = connection
            .query_row(
                "SELECT misses, tried, offered_at FROM wanted WHERE id = 3",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the kept want");
        let unasked: i64 = connection
            .query_row(
                "SELECT count(*) FROM documents WHERE asked IS NULL AND asks = 0 AND refusals = 0",
                [],
                |row| row.get(0),
            )
            .expect("unasked documents");

        assert_eq!(
            keys,
            vec![
                Some(format!(
                    "{}\u{1f}{}",
                    resonate_core::folded_letters("Début"),
                    resonate_core::folded_letters("Artist")
                )),
                None
            ]
        );
        let refusals: i64 = connection
            .query_row("SELECT count(*) FROM refused_offers", [], |row| row.get(0))
            .expect("the refusals table");

        assert_eq!(want, (0, None, None));
        assert_eq!(unasked, 2);
        assert_eq!(refusals, 0);

        drop(connection);
        assert_eq!(
            catalog.resume().expect("the saved place"),
            (vec![13, 12], 1, 9000)
        );
        assert!(catalog.track(12).expect("the original id").favourite);
        assert!(!catalog.track(13).expect("the original id").favourite);
        drop(catalog);

        let kept = path.with_file_name(named_within(
            "",
            path.file_name().expect("a filename"),
            ".before-v6",
        ));
        let backup = Connection::open(&kept).expect("the previous catalog");
        let version: u32 = backup
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("a backup version");
        assert_eq!(version, 6);
        drop(backup);
        fs::remove_file(path).expect("remove the test catalog");
        fs::remove_file(kept).expect("remove the test backup");
    }

    #[test]
    fn a_copy_left_half_made_by_a_failed_migration_does_not_stop_the_next() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("resonate-migration-retried-{unique}.sqlite"));
        let old = Connection::open(&path).expect("a sixth catalog");
        for migration in [V1, V2, V3, V4, V5, V6] {
            old.execute_batch(migration).expect("a shipped schema step");
        }
        old.execute_batch("PRAGMA user_version = 6; INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec, favourite) VALUES (7, 'resonate-folder://local/one.flac', 'one.flac', 'One', 'Artist', 'Album', 30000, 44100, 'FLAC', 1);").expect("a favourite");
        drop(old);
        let kept = path.with_file_name(named_within(
            "",
            path.file_name().expect("a filename"),
            ".before-v6",
        ));
        let staged = kept.with_file_name(named_within(
            "",
            kept.file_name().expect("a filename"),
            ".part",
        ));
        let lock = kept.with_file_name(named_within(
            "",
            kept.file_name().expect("a filename"),
            ".lock",
        ));

        fs::write(&staged, b"half of an earlier copy").expect("a half-made copy");
        fs::write(&lock, b"").expect("a lock left behind");
        let catalog = Catalog::open(&path).expect("a migrated catalog");

        assert!(catalog.track(7).expect("the original id").favourite);
        assert!(!staged.exists());
        assert!(!lock.exists());

        drop(catalog);
        let backup = Connection::open(&kept).expect("the previous catalog");
        let version: u32 = backup
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("a backup version");

        assert_eq!(version, 6);

        drop(backup);
        fs::remove_file(path).expect("remove the test catalog");
        fs::remove_file(kept).expect("remove the test backup");
    }
}
