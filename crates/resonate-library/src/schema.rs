use rusqlite::Connection;

use crate::{Error, Result, SchemaFingerprint, StoreOp};

pub const SCHEMA_FINGERPRINT: SchemaFingerprint = fingerprint_after(V1, MIGRATIONS);

const MIGRATIONS: &[&str] = &[
    "ALTER TABLE albums ADD COLUMN cover_asked INTEGER;",
    "ALTER TABLE tracks ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;",
    "CREATE TABLE track_credits (
         track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
         PRIMARY KEY (track_id, artist_id)
     ) STRICT;
     CREATE INDEX track_credits_by_artist ON track_credits(artist_id, track_id);",
    "CREATE TABLE staged_writes (
         path TEXT PRIMARY KEY,
         pid  INTEGER NOT NULL
     ) STRICT;",
    "CREATE TABLE likenesses (
         picture  TEXT PRIMARY KEY,
         likeness BLOB
     ) STRICT, WITHOUT ROWID;",
    "CREATE TABLE submissions (
         service TEXT PRIMARY KEY,
         through INTEGER NOT NULL
     ) STRICT, WITHOUT ROWID;",
    "ALTER TABLE resume ADD COLUMN next_first INTEGER;
     ALTER TABLE resume ADD COLUMN next_last INTEGER;",
];

const FNV_OFFSET_BASIS: u32 = 0x811c_9dc5;

const FNV_PRIME: u32 = 0x0100_0193;

const UNSTAMPED: u32 = 0;

const V1: &str = "
CREATE TABLE roots (
    id   INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE artists (
    id              INTEGER PRIMARY KEY,
    key             TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    mbid            TEXT,
    sort_name       TEXT,
    kind            TEXT,
    gender          TEXT,
    country         TEXT,
    area            TEXT,
    began_in        TEXT,
    began           TEXT,
    ended           TEXT,
    has_ended       INTEGER NOT NULL DEFAULT 0,
    disambiguation  TEXT,
    portrait        BLOB,
    portrait_format INTEGER,
    asked           INTEGER,
    asks            INTEGER NOT NULL DEFAULT 0,
    refusals        INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER,
    favourite       INTEGER
) STRICT;

CREATE TABLE albums (
    id             INTEGER PRIMARY KEY,
    title          TEXT NOT NULL,
    release_title  TEXT,
    artist_id      INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    year           INTEGER,
    tagged_tracks  INTEGER,
    cover_art      BLOB,
    cover_format   INTEGER,
    cover_source   INTEGER NOT NULL DEFAULT 0,
    cover_key      TEXT,
    cover_path     TEXT,
    mbid           TEXT,
    release_group  TEXT,
    date           TEXT,
    country        TEXT,
    label          TEXT,
    catalog_number TEXT,
    barcode        TEXT,
    kind           TEXT,
    disambiguation TEXT,
    asked          INTEGER,
    asks           INTEGER NOT NULL DEFAULT 0,
    refusals       INTEGER NOT NULL DEFAULT 0,
    answered       INTEGER,
    favourite      INTEGER,
    found_elsewhere INTEGER
) STRICT;

CREATE TABLE tracks (
    id                 INTEGER PRIMARY KEY,
    root_id            INTEGER REFERENCES roots(id) ON DELETE CASCADE,
    path               TEXT NOT NULL,
    span_start         INTEGER NOT NULL DEFAULT 0,
    span_frames        INTEGER,
    title              TEXT NOT NULL,
    artist             TEXT,
    artist_id          INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    album_id           INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    track_number       INTEGER,
    disc_number        INTEGER,
    duration           INTEGER,
    sample_rate        INTEGER NOT NULL,
    channels           INTEGER NOT NULL,
    sample_format      INTEGER NOT NULL,
    codec              INTEGER NOT NULL,
    rg_track_gain      REAL,
    rg_track_peak      REAL,
    rg_album_gain      REAL,
    rg_album_peak      REAL,
    file_size          INTEGER NOT NULL,
    modified           INTEGER NOT NULL,
    sheet_modified     INTEGER,
    added              INTEGER NOT NULL,
    seen               INTEGER NOT NULL,
    plays              INTEGER NOT NULL DEFAULT 0,
    played             INTEGER,
    mbid               TEXT,
    artist_mbid        TEXT,
    release_track_mbid TEXT,
    isrc               TEXT,
    tagged_title       TEXT,
    tagged_artist      TEXT,
    release_title      TEXT,
    genre              TEXT,
    lyrics             TEXT,
    alternative_of     INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    vault_key          TEXT,
    vault_path         TEXT,
    favourite          INTEGER,
    asked              INTEGER,
    asks               INTEGER NOT NULL DEFAULT 0,
    refusals           INTEGER NOT NULL DEFAULT 0,
    answered           INTEGER,
    UNIQUE (path, span_start)
) STRICT;

CREATE TABLE album_keys (
    key      TEXT PRIMARY KEY,
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE
) STRICT;

CREATE INDEX album_keys_by_album ON album_keys(album_id);

CREATE INDEX tracks_by_album ON tracks(album_id, disc_number, track_number,
                                       title COLLATE NOCASE);
CREATE INDEX tracks_by_artist_name ON tracks(artist COLLATE NOCASE, album_id, disc_number,
                                             track_number);
CREATE INDEX tracks_by_plays ON tracks(plays DESC, title COLLATE NOCASE);
CREATE INDEX tracks_by_played ON tracks(played DESC, title COLLATE NOCASE);
CREATE INDEX tracks_by_favourite ON tracks(favourite DESC, title COLLATE NOCASE);
CREATE INDEX tracks_by_title ON tracks(title COLLATE NOCASE);
CREATE INDEX tracks_by_added ON tracks(added DESC);
CREATE INDEX tracks_by_duration ON tracks(duration);
CREATE INDEX tracks_by_artist ON tracks(artist_id);
CREATE INDEX tracks_by_alternative ON tracks(alternative_of);
CREATE INDEX tracks_by_root ON tracks(root_id, seen);
CREATE INDEX tracks_by_asking ON tracks(answered, asked);
CREATE INDEX albums_by_artist ON albums(artist_id);
CREATE INDEX albums_by_asking ON albums(answered, asked);
CREATE INDEX artists_by_asking ON artists(answered, asked);

CREATE VIRTUAL TABLE tracks_fts USING fts5(
    title,
    artist,
    album,
    genre,
    lyrics,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER tracks_fts_gone AFTER DELETE ON tracks BEGIN
    DELETE FROM tracks_fts WHERE rowid = old.id;
END;

CREATE TABLE track_studies (
    track_id       INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    studied        INTEGER NOT NULL,
    studied_under  INTEGER NOT NULL,
    verdict        TEXT NOT NULL,
    cutoff_hz      INTEGER,
    cutoff_drop    REAL,
    lossy_guess    TEXT,
    upsampled_from INTEGER,
    bits_in_use    INTEGER,
    declared_bits  INTEGER,
    peak           REAL NOT NULL,
    rms            REAL NOT NULL,
    true_peak      REAL NOT NULL,
    loudness       REAL,
    loudness_range REAL,
    dynamic_range  INTEGER,
    clipped        INTEGER NOT NULL,
    mono_as_stereo INTEGER NOT NULL,
    print          TEXT,
    print_length   INTEGER,
    recognised     INTEGER,
    heard_as       TEXT,
    heard_score    INTEGER,
    heard_title    TEXT,
    heard_artist   TEXT,
    agreement      TEXT
) STRICT;

CREATE INDEX track_studies_by_verdict ON track_studies(verdict);
CREATE INDEX track_studies_by_agreement ON track_studies(agreement);

CREATE TRIGGER track_studies_forget_a_changed_file
AFTER UPDATE OF file_size, modified, span_frames ON tracks
WHEN old.file_size IS NOT new.file_size
  OR old.modified IS NOT new.modified
  OR old.span_frames IS NOT new.span_frames
BEGIN
    DELETE FROM track_studies WHERE track_id = new.id;
END;

CREATE TABLE listens (
    id       INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    at       INTEGER NOT NULL,
    heard    INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX listens_by_track ON listens(track_id, at);
CREATE INDEX listens_by_time ON listens(at);

CREATE TABLE playlists (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL,
    folded       TEXT NOT NULL UNIQUE,
    created      INTEGER NOT NULL,
    modified     INTEGER NOT NULL,
    played       INTEGER,
    plays        INTEGER NOT NULL DEFAULT 0,
    kept_order   INTEGER,
    kept_reading INTEGER,
    pinned       INTEGER
) STRICT;

CREATE TABLE playlist_entries (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    path        TEXT NOT NULL,
    span_start  INTEGER NOT NULL DEFAULT 0,
    span_frames INTEGER,
    PRIMARY KEY (playlist_id, position)
) STRICT;

CREATE TABLE playlist_queries (
    playlist_id INTEGER PRIMARY KEY REFERENCES playlists(id) ON DELETE CASCADE,
    text        TEXT,
    sort        INTEGER NOT NULL,
    max_rows    INTEGER
) STRICT;

CREATE TABLE release_tracks (
    id             INTEGER PRIMARY KEY,
    album_id       INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    disc           INTEGER NOT NULL,
    position       INTEGER NOT NULL,
    number         TEXT,
    title          TEXT NOT NULL,
    artist         TEXT,
    recording_mbid TEXT,
    track_mbid     TEXT,
    length_ms      INTEGER,
    isrc           TEXT,
    folded         TEXT NOT NULL,
    track_id       INTEGER REFERENCES tracks(id) ON DELETE SET NULL
) STRICT;

CREATE INDEX release_tracks_by_album ON release_tracks(album_id, track_id);
CREATE INDEX release_tracks_in_order ON release_tracks(album_id, disc, position);
CREATE INDEX release_tracks_by_track ON release_tracks(track_id);

CREATE TABLE release_media (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    format   TEXT,
    title    TEXT,
    PRIMARY KEY (album_id, position)
) STRICT;

CREATE TABLE artist_genres (
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    weight    INTEGER NOT NULL,
    PRIMARY KEY (artist_id, name)
) STRICT;

CREATE TABLE artist_links (
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    relation  INTEGER NOT NULL,
    provider  INTEGER NOT NULL,
    url       TEXT NOT NULL,
    PRIMARY KEY (artist_id, url)
) STRICT;

CREATE TABLE artist_releases (
    artist_id      INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    mbid           TEXT NOT NULL,
    title          TEXT NOT NULL,
    kind           TEXT,
    first_released TEXT,
    folded         TEXT NOT NULL,
    PRIMARY KEY (artist_id, mbid)
) STRICT;

CREATE INDEX artist_releases_by_group ON artist_releases(mbid);
CREATE INDEX albums_by_release_group ON albums(release_group);

CREATE TABLE album_links (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    relation INTEGER NOT NULL,
    provider INTEGER NOT NULL,
    url      TEXT NOT NULL,
    PRIMARY KEY (album_id, url)
) STRICT;

CREATE TABLE release_track_links (
    release_track_id INTEGER NOT NULL REFERENCES release_tracks(id) ON DELETE CASCADE,
    relation         INTEGER NOT NULL,
    provider         INTEGER NOT NULL,
    url              TEXT NOT NULL,
    PRIMARY KEY (release_track_id, url)
) STRICT;

CREATE TABLE wants (
    id               INTEGER PRIMARY KEY,
    release_track_id INTEGER NOT NULL UNIQUE REFERENCES release_tracks(id) ON DELETE CASCADE,
    wanted           INTEGER NOT NULL,
    tried            INTEGER,
    offered          TEXT
) STRICT;

CREATE TABLE vault_objects (
    key           TEXT PRIMARY KEY,
    form          INTEGER NOT NULL,
    path          TEXT NOT NULL,
    bytes         INTEGER NOT NULL,
    sample_rate   INTEGER NOT NULL,
    channels      INTEGER NOT NULL,
    sample_format INTEGER NOT NULL,
    frames        INTEGER,
    taken_from    TEXT NOT NULL,
    took          INTEGER NOT NULL,
    was_bytes     INTEGER NOT NULL,
    was_codec     INTEGER NOT NULL,
    encoding      INTEGER NOT NULL,
    validated     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX vault_objects_by_path ON vault_objects(path);
CREATE INDEX tracks_by_vault ON tracks(vault_key);

CREATE TABLE enrichment (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    refresh INTEGER NOT NULL DEFAULT 0,
    began   INTEGER NOT NULL
) STRICT;

CREATE TABLE corrections_index (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    text  TEXT NOT NULL,
    taken INTEGER NOT NULL
) STRICT;

CREATE TABLE corrections_kept (
    device TEXT NOT NULL PRIMARY KEY,
    text   TEXT,
    taken  INTEGER NOT NULL
) STRICT;

CREATE TABLE lyrics_kept (
    path       TEXT NOT NULL,
    span_start INTEGER NOT NULL DEFAULT 0,
    text       TEXT,
    synced     INTEGER NOT NULL DEFAULT 0,
    taken      INTEGER NOT NULL,
    PRIMARY KEY (path, span_start)
) STRICT;

CREATE TABLE resume (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    row     INTEGER NOT NULL,
    at      INTEGER NOT NULL,
    shuffle INTEGER NOT NULL DEFAULT 0,
    taken   INTEGER NOT NULL
) STRICT;

CREATE TABLE resume_rows (
    position    INTEGER PRIMARY KEY,
    uri         TEXT NOT NULL,
    span_start  INTEGER NOT NULL DEFAULT 0,
    span_frames INTEGER
) STRICT;

CREATE TABLE resume_order (
    position  INTEGER PRIMARY KEY,
    loaded_at INTEGER NOT NULL
) STRICT;
";

const WRITER_PAGE_CACHE_KIB: u32 = 8_192;

const READER_PAGE_CACHE_KIB: u32 = 2_048;

const MEMORY_MAPPED_BYTES: u64 = 256 << 20;

const ROWS_PER_INDEX_SAMPLED: u32 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Writing,
    Reading,
}

impl Role {
    const fn page_cache_kib(self) -> u32 {
        match self {
            Self::Writing => WRITER_PAGE_CACHE_KIB,
            Self::Reading => READER_PAGE_CACHE_KIB,
        }
    }
}

pub fn configure(connection: &Connection, role: Role) -> Result<()> {
    let page_cache_kib = role.page_cache_kib();

    connection
        .execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA busy_timeout = 10000;
             PRAGMA cache_size = -{page_cache_kib};
             PRAGMA mmap_size = {MEMORY_MAPPED_BYTES};"
        ))
        .map_err(|source| Error::store(StoreOp::Open, source))
}

pub fn restate_the_statistics(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(&format!(
            "PRAGMA analysis_limit = {ROWS_PER_INDEX_SAMPLED};
             PRAGMA optimize;"
        ))
        .map_err(|source| Error::store(StoreOp::Analyse, source))
}

pub fn lay_out(connection: &Connection) -> Result<()> {
    if stamped(connection)? == Some(SCHEMA_FINGERPRINT) {
        return Ok(());
    }
    lay_out_through(connection, V1, MIGRATIONS)
}

fn lay_out_through(connection: &Connection, first: &str, steps: &[&str]) -> Result<()> {
    let expected = fingerprint_after(first, steps);
    let found = stamped(connection)?;
    if found == Some(expected) {
        return Ok(());
    }
    let taken = match found {
        None => None,
        Some(found) => Some(
            steps_taken(first, steps, found).ok_or(Error::SchemaMismatch { found, expected })?,
        ),
    };

    let transaction = connection
        .unchecked_transaction()
        .map_err(|source| Error::store(StoreOp::Transaction, source))?;
    if taken.is_none() {
        transaction
            .execute_batch(first)
            .map_err(|source| Error::store(StoreOp::LayOut, source))?;
    }
    for step in &steps[taken.unwrap_or(0)..] {
        transaction
            .execute_batch(step)
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
    }
    transaction
        .pragma_update(None, "user_version", as_signed(expected.0))
        .map_err(|source| Error::store(StoreOp::LayOut, source))?;
    transaction
        .commit()
        .map_err(|source| Error::store(StoreOp::Transaction, source))
}

fn steps_taken(first: &str, steps: &[&str], found: SchemaFingerprint) -> Option<usize> {
    (0..=steps.len()).find(|taken| fingerprint_after(first, &steps[..*taken]) == found)
}

fn stamped(connection: &Connection) -> Result<Option<SchemaFingerprint>> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
        .map(|stamp| stamp.cast_unsigned())
        .map(|stamp| (stamp != UNSTAMPED).then_some(SchemaFingerprint(stamp)))
        .map_err(|source| Error::store(StoreOp::Query, source))
}

const fn as_signed(stamp: u32) -> i32 {
    stamp.cast_signed()
}

const fn fingerprint_after(first: &str, steps: &[&str]) -> SchemaFingerprint {
    let mut hashed = hashed_on(FNV_OFFSET_BASIS, first);
    let mut step = 0;

    while step < steps.len() {
        hashed = hashed_on(hashed, steps[step]);
        step += 1;
    }

    SchemaFingerprint(never_unstamped(hashed))
}

const fn hashed_on(from: u32, text: &str) -> u32 {
    let letters = text.as_bytes();
    let mut hashed = from;
    let mut at = 0;

    while at < letters.len() {
        hashed ^= letters[at] as u32;
        hashed = hashed.wrapping_mul(FNV_PRIME);
        at += 1;
    }

    hashed
}

const fn never_unstamped(hashed: u32) -> u32 {
    match hashed {
        UNSTAMPED => FNV_PRIME,
        stamped => stamped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> Connection {
        let connection = Connection::open_in_memory().expect("a database in memory");
        configure(&connection, Role::Writing).expect("the pragmas apply");
        connection
    }

    #[test]
    fn a_fresh_catalog_is_laid_out_and_stamped_with_what_this_build_writes() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");

        assert_eq!(
            stamped(&connection).expect("the stamp reads back"),
            Some(SCHEMA_FINGERPRINT)
        );
    }

    #[test]
    fn a_catalog_this_build_wrote_is_opened_again_without_being_laid_out_twice() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");

        assert!(
            lay_out(&connection).is_ok(),
            "a catalog this build wrote was not opened again"
        );
    }

    #[test]
    fn a_catalog_another_schema_wrote_is_refused_rather_than_read() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");
        let elsewhere = SchemaFingerprint(SCHEMA_FINGERPRINT.0.wrapping_add(1));
        connection
            .pragma_update(None, "user_version", as_signed(elsewhere.0))
            .expect("the stamp is written");

        assert!(
            matches!(
                lay_out(&connection),
                Err(Error::SchemaMismatch { found, expected })
                    if found == elsewhere && expected == SCHEMA_FINGERPRINT
            ),
            "a catalog written to another schema was not refused"
        );
    }

    #[test]
    fn a_fingerprint_past_what_a_signed_stamp_holds_reads_back_whole() {
        let connection = opened();
        let highest = SchemaFingerprint(u32::MAX);
        connection
            .pragma_update(None, "user_version", as_signed(highest.0))
            .expect("the stamp is written");

        assert_eq!(
            stamped(&connection).expect("the stamp reads back"),
            Some(highest),
            "a fingerprint above i32::MAX did not survive the pragma"
        );
    }

    const FIRST: &str = "CREATE TABLE held (id INTEGER PRIMARY KEY) STRICT;";

    const NAMED: &str = "ALTER TABLE held ADD COLUMN name TEXT;";

    const COUNTED: &str = "ALTER TABLE held ADD COLUMN plays INTEGER NOT NULL DEFAULT 0;";

    const REFUSED: &str = "ALTER TABLE nowhere ADD COLUMN name TEXT;";

    const V1_AS_FIRST_STAMPED: SchemaFingerprint = SchemaFingerprint(0x0ca1_e683);

    fn columns_of_held(connection: &Connection) -> Vec<String> {
        connection
            .prepare("SELECT name FROM pragma_table_info('held') ORDER BY cid")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("the table's columns read back")
    }

    #[test]
    fn the_first_schema_is_never_edited_where_it_stands() {
        assert_eq!(
            fingerprint_after(V1, &[]),
            V1_AS_FIRST_STAMPED,
            "V1 was edited in place, which strands every catalog it wrote: put the change in a \
             step of MIGRATIONS instead"
        );
    }

    #[test]
    fn a_catalog_stamped_before_the_steps_is_carried_through_every_one_it_missed() {
        let connection = opened();
        lay_out_through(&connection, FIRST, &[]).expect("the first schema applies");
        connection
            .execute("INSERT INTO held (id) VALUES (7)", [])
            .expect("a row is written");

        lay_out_through(&connection, FIRST, &[NAMED, COUNTED]).expect("the steps apply");

        assert_eq!(columns_of_held(&connection), ["id", "name", "plays"]);
        assert_eq!(
            stamped(&connection).expect("the stamp reads back"),
            Some(fingerprint_after(FIRST, &[NAMED, COUNTED]))
        );
        assert_eq!(
            connection
                .query_row("SELECT id FROM held", [], |row| row.get::<_, i64>(0))
                .expect("the row survived"),
            7
        );
    }

    #[test]
    fn a_catalog_part_of_the_way_through_takes_only_the_steps_after_it() {
        let connection = opened();
        lay_out_through(&connection, FIRST, &[NAMED]).expect("the first step applies");

        lay_out_through(&connection, FIRST, &[NAMED, COUNTED]).expect("the second applies");

        assert_eq!(columns_of_held(&connection), ["id", "name", "plays"]);
    }

    #[test]
    fn an_existing_catalog_gains_visible_tracks_without_losing_rows() {
        let connection = opened();
        lay_out_through(&connection, V1, &MIGRATIONS[..1]).expect("the previous schema applies");
        connection
            .execute(
                "INSERT INTO tracks (path, title, sample_rate, channels, sample_format, codec, file_size, modified, added, seen)
                 VALUES ('song.wav', 'Song', 44100, 2, 1, 1, 10, 1, 1, 1)",
                [],
            )
            .expect("a track is stored");

        lay_out(&connection).expect("the catalog migrates");
        assert_eq!(
            connection
                .query_row(
                    "SELECT hidden FROM tracks WHERE path = 'song.wav'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("the track remains visible"),
            0
        );
        lay_out(&connection).expect("opening again is idempotent");
    }

    #[test]
    fn a_fresh_catalog_is_laid_out_and_taken_through_every_step() {
        let connection = opened();

        lay_out_through(&connection, FIRST, &[NAMED, COUNTED]).expect("the schema applies");

        assert_eq!(columns_of_held(&connection), ["id", "name", "plays"]);
    }

    #[test]
    fn a_step_that_fails_leaves_the_catalog_as_it_was() {
        let connection = opened();
        lay_out_through(&connection, FIRST, &[]).expect("the first schema applies");

        assert!(matches!(
            lay_out_through(&connection, FIRST, &[NAMED, REFUSED]),
            Err(Error::Store {
                op: StoreOp::Migrate,
                ..
            })
        ));
        assert_eq!(columns_of_held(&connection), ["id"]);
        assert_eq!(
            stamped(&connection).expect("the stamp reads back"),
            Some(fingerprint_after(FIRST, &[]))
        );
    }

    #[test]
    fn a_schema_that_has_changed_is_a_fingerprint_that_has_changed() {
        assert_ne!(
            fingerprint_after(V1, &[]),
            fingerprint_after(&V1.replace("cover_source", "cover_origin"), &[]),
            "two schemas differing by a column name fingerprint the same"
        );
        assert_ne!(
            never_unstamped(UNSTAMPED),
            UNSTAMPED,
            "a schema hashing to nothing reads as a catalog nothing has stamped"
        );
        assert_eq!(never_unstamped(FNV_OFFSET_BASIS), FNV_OFFSET_BASIS);
    }
}
