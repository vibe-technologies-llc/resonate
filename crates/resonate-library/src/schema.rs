use rusqlite::{Connection, Transaction, TransactionBehavior, functions::FunctionFlags};

use crate::{Error, Result, SchemaFingerprint, StoreOp, store};

const UNSTAMPED: u32 = 0;

const MIGRATIONS: &[&str] = &[
    LISTENS_NAME_THEIR_ORIGIN,
    SONGS_RECEIVED_NAME_THEIR_RECORDING,
    SONGS_LEARNT_UNTIMED_ARE_LEARNT_AGAIN,
    A_RUN_KEEPS_THE_SHEET_LINES_IT_REPLACED,
    A_MODELS_EDITS_ARE_OFFERED_TO_THE_WINDOW,
    A_STUDY_KEEPS_WHERE_THE_MUSIC_SOUNDS,
    A_RELEASE_KEEPS_ITS_CREDIT,
    RELEASES_LANDED_WITHOUT_THEIR_CREDIT_ARE_ASKED_AGAIN,
    AN_UNHELD_LISTEN_KEEPS_WHAT_IT_WAS,
];

const STAMPS: [SchemaFingerprint; MIGRATIONS.len() + 1] = stamps();

pub const SCHEMA_FINGERPRINT: SchemaFingerprint = STAMPS[MIGRATIONS.len()];

const FNV_OFFSET_BASIS: u32 = 0x811c_9dc5;

const FNV_PRIME: u32 = 0x0100_0193;

const LISTENS_NAME_THEIR_ORIGIN: &str = "
ALTER TABLE listens ADD COLUMN origin_device BLOB;
ALTER TABLE listens ADD COLUMN origin_id INTEGER;
CREATE UNIQUE INDEX listens_by_origin ON listens(origin_device, origin_id) WHERE origin_device IS NOT NULL;
";

const SONGS_RECEIVED_NAME_THEIR_RECORDING: &str = "
CREATE TABLE songs_received (
    recording TEXT PRIMARY KEY,
    path      TEXT NOT NULL
) STRICT;
CREATE INDEX songs_received_by_path ON songs_received(path);
";

const SONGS_LEARNT_UNTIMED_ARE_LEARNT_AGAIN: &str = "
DELETE FROM discography_songs_read WHERE release_group IN (
    SELECT release_group FROM discography_songs
     GROUP BY release_group
    HAVING count(length_ms) = 0
);
";

const A_RUN_KEEPS_THE_SHEET_LINES_IT_REPLACED: &str = "
CREATE TABLE retagged_sheets (
    sheet TEXT NOT NULL,
    file  TEXT NOT NULL,
    scope INTEGER NOT NULL,
    field TEXT NOT NULL,
    was   TEXT,
    wrote TEXT,
    PRIMARY KEY (sheet, scope, field)
) STRICT, WITHOUT ROWID;
";

const A_MODELS_EDITS_ARE_OFFERED_TO_THE_WINDOW: &str = "
CREATE TABLE offered_steps (
    id             INTEGER PRIMARY KEY,
    edit           INTEGER NOT NULL,
    playlist_id    INTEGER NOT NULL,
    reached        INTEGER NOT NULL,
    first          INTEGER NOT NULL,
    holds          INTEGER NOT NULL,
    leaves         INTEGER NOT NULL,
    left_at        INTEGER,
    name           TEXT NOT NULL,
    was_held       INTEGER NOT NULL,
    created        INTEGER NOT NULL,
    modified       INTEGER NOT NULL,
    played         INTEGER,
    plays          INTEGER NOT NULL,
    pinned         INTEGER,
    kept_order     INTEGER,
    kept_reading   INTEGER,
    queried        INTEGER NOT NULL,
    query_text     TEXT,
    query_sort     INTEGER,
    query_reading  INTEGER,
    query_max_rows INTEGER,
    rows_held      INTEGER NOT NULL,
    row_count      INTEGER NOT NULL
) STRICT;
CREATE TABLE offered_step_rows (
    step        INTEGER NOT NULL REFERENCES offered_steps(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    path        TEXT NOT NULL,
    span_start  INTEGER NOT NULL,
    span_frames INTEGER,
    PRIMARY KEY (step, position)
) STRICT, WITHOUT ROWID;
";

const A_STUDY_KEEPS_WHERE_THE_MUSIC_SOUNDS: &str = "
ALTER TABLE track_studies ADD COLUMN band_levels BLOB;
";

const A_RELEASE_KEEPS_ITS_CREDIT: &str = "
CREATE TABLE release_credits (
    album_id  INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    name      TEXT NOT NULL,
    joined_by TEXT NOT NULL,
    mbid      TEXT,
    PRIMARY KEY (album_id, position)
) STRICT, WITHOUT ROWID;
";

const RELEASES_LANDED_WITHOUT_THEIR_CREDIT_ARE_ASKED_AGAIN: &str = "
UPDATE albums SET answered = 1
 WHERE answered IS NOT NULL AND mbid IS NOT NULL AND release_title IS NOT NULL
   AND pressing_fit IS NULL
   AND NOT EXISTS (SELECT 1 FROM release_credits c WHERE c.album_id = albums.id);
";

const AN_UNHELD_LISTEN_KEEPS_WHAT_IT_WAS: &str = "
ALTER TABLE unheld_listens ADD COLUMN listen_id INTEGER;
ALTER TABLE unheld_listens ADD COLUMN elsewhere INTEGER NOT NULL DEFAULT 0;
ALTER TABLE unheld_listens ADD COLUMN origin_device BLOB;
ALTER TABLE unheld_listens ADD COLUMN origin_id INTEGER;
";

const V1: &str = "
CREATE TABLE roots (
    id   INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE artists (
    id               INTEGER PRIMARY KEY,
    key              TEXT NOT NULL UNIQUE,
    name             TEXT NOT NULL,
    mbid             TEXT,
    sort_name        TEXT,
    kind             TEXT,
    gender           TEXT,
    country          TEXT,
    area             TEXT,
    began_in         TEXT,
    began            TEXT,
    ended            TEXT,
    has_ended        INTEGER NOT NULL DEFAULT 0,
    disambiguation   TEXT,
    portrait         BLOB,
    portrait_format  INTEGER,
    asked            INTEGER,
    asks             INTEGER NOT NULL DEFAULT 0,
    refusals         INTEGER NOT NULL DEFAULT 0,
    answered         INTEGER,
    favourite        INTEGER,
    releases_unread  INTEGER NOT NULL DEFAULT 0,
    releases_read_to INTEGER NOT NULL DEFAULT 0,
    portrait_asked   INTEGER,
    tagged_sort      TEXT,
    found_elsewhere  INTEGER
) STRICT;

CREATE TABLE albums (
    id              INTEGER PRIMARY KEY,
    title           TEXT NOT NULL,
    release_title   TEXT,
    artist_id       INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    year            INTEGER,
    tagged_tracks   INTEGER,
    cover_art       BLOB,
    cover_format    INTEGER,
    cover_source    INTEGER NOT NULL DEFAULT 0,
    cover_key       TEXT,
    cover_path      TEXT,
    mbid            TEXT,
    release_group   TEXT,
    date            TEXT,
    country         TEXT,
    label           TEXT,
    catalog_number  TEXT,
    barcode         TEXT,
    kind            TEXT,
    disambiguation  TEXT,
    asked           INTEGER,
    asks            INTEGER NOT NULL DEFAULT 0,
    refusals        INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER,
    favourite       INTEGER,
    found_elsewhere INTEGER,
    cover_asked     INTEGER,
    front_cover     INTEGER,
    tagged_sort     TEXT,
    cover_print     TEXT,
    pressing_fit    INTEGER
) STRICT;

CREATE TABLE tracks (
    id                   INTEGER PRIMARY KEY,
    root_id              INTEGER REFERENCES roots(id) ON DELETE CASCADE,
    path                 TEXT NOT NULL,
    span_start           INTEGER NOT NULL DEFAULT 0,
    span_frames          INTEGER,
    title                TEXT NOT NULL,
    artist               TEXT,
    artist_id            INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    album_id             INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    track_number         INTEGER,
    disc_number          INTEGER,
    duration             INTEGER,
    sample_rate          INTEGER NOT NULL,
    channels             INTEGER NOT NULL,
    sample_format        INTEGER NOT NULL,
    codec                INTEGER NOT NULL,
    rg_track_gain        REAL,
    rg_track_peak        REAL,
    rg_album_gain        REAL,
    rg_album_peak        REAL,
    file_size            INTEGER NOT NULL,
    modified             INTEGER NOT NULL,
    sheet_modified       INTEGER,
    added                INTEGER NOT NULL,
    seen                 INTEGER NOT NULL,
    plays                INTEGER NOT NULL DEFAULT 0,
    played               INTEGER,
    mbid                 TEXT,
    artist_mbid          TEXT,
    release_track_mbid   TEXT,
    isrc                 TEXT,
    tagged_title         TEXT,
    tagged_artist        TEXT,
    release_title        TEXT,
    genre                TEXT,
    lyrics               TEXT,
    alternative_of       INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    vault_key            TEXT,
    vault_path           TEXT,
    favourite            INTEGER,
    asked                INTEGER,
    asks                 INTEGER NOT NULL DEFAULT 0,
    refusals             INTEGER NOT NULL DEFAULT 0,
    answered             INTEGER,
    hidden               INTEGER NOT NULL DEFAULT 0,
    probe_again          INTEGER NOT NULL DEFAULT 0,
    named_by_its_stem    INTEGER NOT NULL DEFAULT 0,
    packets              INTEGER,
    title_sort           TEXT,
    artist_sort          TEXT,
    title_filed          TEXT GENERATED ALWAYS AS (
         CASE WHEN title_sort IS NOT NULL AND title IS tagged_title THEN title_sort ELSE title END
     ) VIRTUAL,
    artist_filed         TEXT,
    title_words          TEXT,
    vault_bits           INTEGER,
    numbered_by_its_stem INTEGER NOT NULL DEFAULT 0,
    file_name            TEXT GENERATED ALWAYS AS (
         replace(path, rtrim(path, replace(path, '/', '')), '')
     ) VIRTUAL,
    track_total          INTEGER,
    walked               TEXT,
    UNIQUE (path, span_start)
) STRICT;

CREATE TABLE album_keys (
    key      TEXT PRIMARY KEY,
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE
) STRICT;

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
    max_rows    INTEGER,
    reading     INTEGER NOT NULL DEFAULT 0
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
    song           TEXT,
    PRIMARY KEY (artist_id, mbid)
) STRICT;

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
    offered          TEXT,
    misses           INTEGER NOT NULL DEFAULT 0
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
    lyricsfile TEXT,
    PRIMARY KEY (path, span_start)
) STRICT;

CREATE TABLE resume (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    row        INTEGER NOT NULL,
    at         INTEGER NOT NULL,
    shuffle    INTEGER NOT NULL DEFAULT 0,
    taken      INTEGER NOT NULL,
    next_first INTEGER,
    next_last  INTEGER
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

CREATE TABLE track_credits (
    track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    PRIMARY KEY (track_id, artist_id)
) STRICT;

CREATE TABLE staged_writes (
    path TEXT PRIMARY KEY,
    pid  INTEGER NOT NULL
) STRICT;

CREATE TABLE likenesses (
    picture  TEXT PRIMARY KEY,
    likeness BLOB
) STRICT, WITHOUT ROWID;

CREATE TABLE submissions (
    service TEXT PRIMARY KEY,
    through INTEGER NOT NULL,
    began   INTEGER
) STRICT, WITHOUT ROWID;

CREATE TABLE refused_releases (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    mbid     TEXT NOT NULL,
    PRIMARY KEY (album_id, mbid)
) STRICT, WITHOUT ROWID;

CREATE TABLE passes (
    at    INTEGER NOT NULL,
    heard INTEGER NOT NULL
) STRICT;

CREATE TABLE unheld_listens (
    path       TEXT NOT NULL,
    span_start INTEGER NOT NULL,
    at         INTEGER NOT NULL,
    began      INTEGER,
    heard      INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE playlist_plays (
    playlist_id INTEGER NOT NULL,
    at          INTEGER NOT NULL
) STRICT;

CREATE TABLE organised (
    position   INTEGER PRIMARY KEY,
    unit       INTEGER NOT NULL,
    part       INTEGER NOT NULL,
    rows       INTEGER NOT NULL,
    moved_from TEXT NOT NULL,
    moved_to   TEXT NOT NULL
) STRICT;

CREATE TABLE retagged (
    path       TEXT PRIMARY KEY,
    pictured   INTEGER NOT NULL,
    rated      INTEGER,
    picture    BLOB,
    picture_id INTEGER
) STRICT, WITHOUT ROWID;

CREATE TABLE retagged_fields (
    path        TEXT NOT NULL,
    field       TEXT NOT NULL,
    was         TEXT,
    wrote       TEXT,
    wrote_known INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (path, field)
) STRICT, WITHOUT ROWID;

CREATE TABLE volumes (
    path TEXT PRIMARY KEY
) STRICT, WITHOUT ROWID;

CREATE TABLE retagged_pictures (
    id      INTEGER PRIMARY KEY,
    picture BLOB NOT NULL
) STRICT;

CREATE TABLE unstudied (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    under    INTEGER NOT NULL
) STRICT;

CREATE TABLE lyrics_refused (
    path       TEXT NOT NULL,
    span_start INTEGER NOT NULL,
    refused    INTEGER NOT NULL,
    refusals   INTEGER NOT NULL,
    PRIMARY KEY (path, span_start)
) STRICT, WITHOUT ROWID;

CREATE TABLE vault_refused (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    under    INTEGER NOT NULL
) STRICT;

CREATE TABLE dismissed_releases (
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    mbid      TEXT NOT NULL,
    PRIMARY KEY (artist_id, mbid)
) STRICT, WITHOUT ROWID;

CREATE TABLE loves_told (
    service   TEXT NOT NULL,
    recording TEXT NOT NULL,
    PRIMARY KEY (service, recording)
) STRICT, WITHOUT ROWID;

CREATE TABLE index_refold_wanted (
    since INTEGER
) STRICT;

CREATE TABLE discography_songs (
    release_group  TEXT NOT NULL,
    recording_mbid TEXT NOT NULL,
    release_mbid   TEXT NOT NULL,
    release_title  TEXT NOT NULL,
    released       TEXT,
    kind           TEXT,
    disc           INTEGER NOT NULL,
    position       INTEGER NOT NULL,
    title          TEXT NOT NULL,
    artist         TEXT NOT NULL,
    length_ms      INTEGER,
    words          TEXT NOT NULL,
    folded         TEXT NOT NULL,
    PRIMARY KEY (release_group, recording_mbid)
) STRICT, WITHOUT ROWID;

CREATE TABLE discography_songs_read (
    release_group TEXT PRIMARY KEY,
    read          INTEGER NOT NULL,
    refusals      INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE unheld_covers (
    release_group TEXT PRIMARY KEY,
    cover         BLOB,
    asked         INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE forgotten_deliveries (
    want_id    INTEGER NOT NULL REFERENCES wants(id) ON DELETE CASCADE,
    taken_from TEXT NOT NULL,
    forgotten  INTEGER NOT NULL,
    PRIMARY KEY (want_id, taken_from)
) STRICT, WITHOUT ROWID;

CREATE TABLE dismissed_missing (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    disc     INTEGER NOT NULL,
    position INTEGER NOT NULL,
    folded   TEXT NOT NULL,
    PRIMARY KEY (album_id, disc, position, folded)
) STRICT, WITHOUT ROWID;

CREATE TABLE listens (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    at        INTEGER NOT NULL,
    heard     INTEGER NOT NULL DEFAULT 0,
    began     INTEGER,
    elsewhere INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE VIRTUAL TABLE tracks_fts USING fts5(
         title,
         artist,
         album,
         genre,
         lyrics,
         tokenize = 'unicode61 remove_diacritics 2',
         prefix = '1 2'
     );

CREATE TABLE settle_owed (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    owed INTEGER NOT NULL
) STRICT;

CREATE TABLE regroup_owed (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    owed INTEGER NOT NULL
) STRICT;

CREATE TABLE refused_offers (
    want_id    INTEGER NOT NULL REFERENCES wants(id) ON DELETE CASCADE,
    taken_from TEXT NOT NULL,
    refused    INTEGER NOT NULL,
    PRIMARY KEY (want_id, taken_from)
) STRICT, WITHOUT ROWID;

CREATE TABLE earlier_submissions (
    service TEXT PRIMARY KEY,
    through INTEGER NOT NULL,
    until   INTEGER NOT NULL
) STRICT;

CREATE TABLE lyrics_ahead (
    path       TEXT NOT NULL,
    span_start INTEGER NOT NULL,
    ahead_ms   INTEGER NOT NULL,
    PRIMARY KEY (path, span_start)
) STRICT, WITHOUT ROWID;

CREATE TABLE title_words_wanted (
    id INTEGER PRIMARY KEY
) STRICT;

CREATE TABLE regroup_owed_tracks (
    track_id INTEGER PRIMARY KEY
) STRICT;

CREATE TABLE object_deliveries (
    key        TEXT NOT NULL REFERENCES vault_objects(key) ON DELETE CASCADE,
    taken_from TEXT NOT NULL,
    took       INTEGER NOT NULL,
    PRIMARY KEY (key, taken_from)
) STRICT, WITHOUT ROWID;

CREATE TABLE kept_tags (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    field    TEXT NOT NULL,
    value    TEXT NOT NULL,
    PRIMARY KEY (track_id, field)
) STRICT, WITHOUT ROWID;

CREATE TABLE album_keys_composed_wanted (
    since INTEGER
) STRICT;

CREATE TABLE release_track_isrcs (
    release_track_id INTEGER NOT NULL REFERENCES release_tracks(id) ON DELETE CASCADE,
    isrc             TEXT NOT NULL,
    PRIMARY KEY (release_track_id, isrc)
) STRICT, WITHOUT ROWID;

CREATE VIRTUAL TABLE artist_releases_fts USING fts5(
         folded, content = 'artist_releases', content_rowid = 'rowid', tokenize = 'trigram'
     );

CREATE VIRTUAL TABLE artists_key_fts USING fts5(
         key, content = 'artists', content_rowid = 'id', tokenize = 'trigram'
     );

CREATE VIRTUAL TABLE release_tracks_fts USING fts5(
         folded, content = 'release_tracks', content_rowid = 'id', tokenize = 'trigram'
     );

CREATE INDEX album_keys_by_album ON album_keys(album_id);

CREATE INDEX tracks_by_added ON tracks(added DESC);

CREATE INDEX tracks_by_duration ON tracks(duration);

CREATE INDEX tracks_by_artist ON tracks(artist_id);

CREATE INDEX tracks_by_alternative ON tracks(alternative_of);

CREATE INDEX tracks_by_root ON tracks(root_id, seen);

CREATE INDEX tracks_by_asking ON tracks(answered, asked);

CREATE INDEX albums_by_artist ON albums(artist_id);

CREATE INDEX albums_by_asking ON albums(answered, asked);

CREATE INDEX artists_by_asking ON artists(answered, asked);

CREATE INDEX track_studies_by_verdict ON track_studies(verdict);

CREATE INDEX track_studies_by_agreement ON track_studies(agreement);

CREATE INDEX release_tracks_by_album ON release_tracks(album_id, track_id);

CREATE INDEX release_tracks_in_order ON release_tracks(album_id, disc, position);

CREATE INDEX release_tracks_by_track ON release_tracks(track_id);

CREATE INDEX artist_releases_by_group ON artist_releases(mbid);

CREATE INDEX albums_by_release_group ON albums(release_group);

CREATE INDEX vault_objects_by_path ON vault_objects(path);

CREATE INDEX tracks_by_vault ON tracks(vault_key);

CREATE INDEX track_credits_by_artist ON track_credits(artist_id, track_id);

CREATE INDEX passes_by_time ON passes(at);

CREATE INDEX unheld_listens_by_path ON unheld_listens(path, span_start);

CREATE INDEX playlist_plays_by_playlist ON playlist_plays(playlist_id, at);

CREATE INDEX unheld_listens_by_time ON unheld_listens(at);

CREATE INDEX vault_objects_by_sound ON vault_objects(frames, sample_rate, channels);

CREATE INDEX tracks_by_recording ON tracks(mbid);

CREATE INDEX release_tracks_by_recording ON release_tracks(recording_mbid);

CREATE INDEX discography_songs_by_recording ON discography_songs(recording_mbid);

CREATE INDEX listens_by_track ON listens(track_id, at);

CREATE INDEX listens_by_time ON listens(at);

CREATE INDEX tracks_by_credit ON tracks(artist);

CREATE INDEX playlist_entries_by_path ON playlist_entries(path);

CREATE INDEX resume_rows_by_uri ON resume_rows(uri);

CREATE INDEX tracks_by_album ON tracks(album_id, disc_number, track_number,
                                            title_filed COLLATE NOCASE);

CREATE INDEX tracks_by_plays ON tracks(plays DESC, title_filed COLLATE NOCASE);

CREATE INDEX tracks_by_played ON tracks(played DESC, title_filed COLLATE NOCASE);

CREATE INDEX tracks_by_favourite ON tracks(favourite DESC, title_filed COLLATE NOCASE);

CREATE INDEX tracks_by_title ON tracks(title_filed COLLATE NOCASE);

CREATE INDEX tracks_by_artist_name ON tracks(artist_filed COLLATE NOCASE, album_id,
                                                  disc_number, track_number);

CREATE INDEX tracks_by_artist_and_words ON tracks(artist_id, title_words);

CREATE INDEX tracks_by_title_words ON tracks(title_words);

CREATE INDEX tracks_by_isrc ON tracks(isrc);

CREATE INDEX albums_by_release ON albums(mbid);

CREATE INDEX artists_by_mbid ON artists(mbid);

CREATE INDEX tracks_by_file_name ON tracks(file_name COLLATE NOCASE);

CREATE TRIGGER tracks_fts_gone AFTER DELETE ON tracks BEGIN
    DELETE FROM tracks_fts WHERE rowid = old.id;
END;

CREATE TRIGGER track_studies_forget_a_changed_file
AFTER UPDATE OF file_size, modified, span_frames ON tracks
WHEN old.file_size IS NOT new.file_size
  OR old.modified IS NOT new.modified
  OR old.span_frames IS NOT new.span_frames
BEGIN
    DELETE FROM track_studies WHERE track_id = new.id;
END;

CREATE TRIGGER unstudied_forget_a_changed_file
     AFTER UPDATE OF file_size, modified, span_frames ON tracks
     WHEN old.file_size IS NOT new.file_size
       OR old.modified IS NOT new.modified
       OR old.span_frames IS NOT new.span_frames
     BEGIN
         DELETE FROM unstudied WHERE track_id = new.id;
     END;

CREATE TRIGGER vault_refused_forget_a_changed_file
     AFTER UPDATE OF file_size, modified, span_frames ON tracks
     WHEN old.file_size IS NOT new.file_size
       OR old.modified IS NOT new.modified
       OR old.span_frames IS NOT new.span_frames
     BEGIN
         DELETE FROM vault_refused WHERE track_id = new.id;
     END;

CREATE TRIGGER lyrics_forget_a_changed_file
     AFTER UPDATE OF file_size, modified, span_frames ON tracks
     WHEN old.file_size IS NOT new.file_size
       OR old.modified IS NOT new.modified
       OR old.span_frames IS NOT new.span_frames
     BEGIN
         DELETE FROM lyrics_kept
          WHERE path = old.path AND span_start = old.span_start;
         DELETE FROM lyrics_refused
          WHERE path = old.path AND span_start = old.span_start;
     END;

CREATE TRIGGER tracks_owe_a_settle_when_added AFTER INSERT ON tracks
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER tracks_owe_a_settle_when_named AFTER UPDATE OF artist, artist_id, album_id ON tracks
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER tracks_owe_a_settle_when_gone AFTER DELETE ON tracks
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER artists_owe_a_settle_when_added AFTER INSERT ON artists
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER artists_owe_a_settle_when_named AFTER UPDATE OF name, key ON artists
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER artists_owe_a_settle_when_gone AFTER DELETE ON artists
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER albums_owe_a_settle_when_gone AFTER DELETE ON albums
     BEGIN UPDATE settle_owed SET owed = 1; END;

CREATE TRIGGER tracks_file_their_artist_when_added AFTER INSERT ON tracks
     BEGIN
         UPDATE tracks SET artist_filed = CASE
             WHEN new.artist_sort IS NOT NULL AND new.artist IS new.tagged_artist
                 THEN new.artist_sort
             ELSE coalesce((SELECT coalesce(a.tagged_sort, a.sort_name) FROM artists a
                            WHERE a.id = new.artist_id AND a.name = new.artist COLLATE NOCASE),
                           new.artist)
         END
         WHERE id = new.id;
     END;

CREATE TRIGGER tracks_file_their_artist_when_named AFTER UPDATE OF
         artist, artist_id, artist_sort, tagged_artist ON tracks
     BEGIN
         UPDATE tracks SET artist_filed = CASE
             WHEN new.artist_sort IS NOT NULL AND new.artist IS new.tagged_artist
                 THEN new.artist_sort
             ELSE coalesce((SELECT coalesce(a.tagged_sort, a.sort_name) FROM artists a
                            WHERE a.id = new.artist_id AND a.name = new.artist COLLATE NOCASE),
                           new.artist)
         END
         WHERE id = new.id;
     END;

CREATE TRIGGER artists_file_their_tracks_when_sorted AFTER UPDATE OF
         name, tagged_sort, sort_name ON artists
     WHEN old.name IS NOT new.name
       OR old.tagged_sort IS NOT new.tagged_sort
       OR old.sort_name IS NOT new.sort_name
     BEGIN
         UPDATE tracks SET artist_filed = CASE
             WHEN artist_sort IS NOT NULL AND artist IS tagged_artist THEN artist_sort
             WHEN artist = new.name COLLATE NOCASE
                 THEN coalesce(new.tagged_sort, new.sort_name, artist)
             ELSE artist
         END
         WHERE artist_id = new.id;
     END;

CREATE TRIGGER albums_print_their_cover_when_added AFTER INSERT ON albums
     WHEN new.cover_art IS NOT NULL
     BEGIN
         UPDATE albums
            SET cover_print = length(new.cover_art) || ':' || hex(substr(new.cover_art, 1, 256))
          WHERE id = new.id;
     END;

CREATE TRIGGER albums_print_their_cover_when_changed AFTER UPDATE OF cover_art ON albums
     BEGIN
         UPDATE albums
            SET cover_print = length(new.cover_art) || ':' || hex(substr(new.cover_art, 1, 256))
          WHERE id = new.id;
     END;

CREATE TRIGGER tracks_owe_a_regroup_when_added AFTER INSERT ON tracks
     BEGIN
         INSERT INTO regroup_owed_tracks (track_id)
         SELECT NEW.id WHERE NOT EXISTS (SELECT 1 FROM regroup_owed_tracks WHERE track_id = NEW.id);
     END;

CREATE TRIGGER tracks_owe_a_regroup_when_gone BEFORE DELETE ON tracks
     BEGIN
         INSERT INTO regroup_owed_tracks (track_id)
         SELECT id FROM tracks
          WHERE alternative_of = OLD.id
            AND id NOT IN (SELECT track_id FROM regroup_owed_tracks);
     END;

CREATE TRIGGER tracks_owe_a_regroup_when_moved AFTER UPDATE OF
         album_id, artist, disc_number, track_number, title, duration, sample_rate,
         sample_format, codec, file_size, span_frames ON tracks
     BEGIN
         INSERT INTO regroup_owed_tracks (track_id)
         SELECT id FROM tracks
          WHERE (id = NEW.id OR id = NEW.alternative_of OR alternative_of = NEW.id
                 OR alternative_of = NEW.alternative_of)
            AND id NOT IN (SELECT track_id FROM regroup_owed_tracks);
     END;

CREATE TRIGGER albums_owe_a_regroup_when_named AFTER UPDATE OF
         title, release_title, artist_id ON albums
     BEGIN
         INSERT INTO regroup_owed_tracks (track_id)
         SELECT id FROM tracks
          WHERE album_id = NEW.id AND id NOT IN (SELECT track_id FROM regroup_owed_tracks);
     END;

CREATE TRIGGER artists_owe_a_regroup_when_named AFTER UPDATE OF name ON artists
     BEGIN
         INSERT INTO regroup_owed_tracks (track_id)
         SELECT t.id FROM tracks t JOIN albums a ON a.id = t.album_id
          WHERE a.artist_id = NEW.id AND t.id NOT IN (SELECT track_id FROM regroup_owed_tracks);
     END;

CREATE TRIGGER artist_releases_fts_added AFTER INSERT ON artist_releases BEGIN
         INSERT INTO artist_releases_fts (rowid, folded) VALUES (new.rowid, new.folded);
     END;

CREATE TRIGGER artist_releases_fts_gone AFTER DELETE ON artist_releases BEGIN
         INSERT INTO artist_releases_fts (artist_releases_fts, rowid, folded)
         VALUES ('delete', old.rowid, old.folded);
     END;

CREATE TRIGGER artist_releases_fts_refolded AFTER UPDATE OF folded ON artist_releases BEGIN
         INSERT INTO artist_releases_fts (artist_releases_fts, rowid, folded)
         VALUES ('delete', old.rowid, old.folded);
         INSERT INTO artist_releases_fts (rowid, folded) VALUES (new.rowid, new.folded);
     END;

CREATE TRIGGER artists_key_fts_added AFTER INSERT ON artists BEGIN
         INSERT INTO artists_key_fts (rowid, key) VALUES (new.id, new.key);
     END;

CREATE TRIGGER artists_key_fts_gone AFTER DELETE ON artists BEGIN
         INSERT INTO artists_key_fts (artists_key_fts, rowid, key)
         VALUES ('delete', old.id, old.key);
     END;

CREATE TRIGGER artists_key_fts_rekeyed AFTER UPDATE OF key ON artists BEGIN
         INSERT INTO artists_key_fts (artists_key_fts, rowid, key)
         VALUES ('delete', old.id, old.key);
         INSERT INTO artists_key_fts (rowid, key) VALUES (new.id, new.key);
     END;

CREATE TRIGGER release_tracks_fts_added AFTER INSERT ON release_tracks BEGIN
         INSERT INTO release_tracks_fts (rowid, folded) VALUES (new.id, new.folded);
     END;

CREATE TRIGGER release_tracks_fts_gone AFTER DELETE ON release_tracks BEGIN
         INSERT INTO release_tracks_fts (release_tracks_fts, rowid, folded)
         VALUES ('delete', old.id, old.folded);
     END;

CREATE TRIGGER release_tracks_fts_refolded AFTER UPDATE OF folded ON release_tracks BEGIN
         INSERT INTO release_tracks_fts (release_tracks_fts, rowid, folded)
         VALUES ('delete', old.id, old.folded);
         INSERT INTO release_tracks_fts (rowid, folded) VALUES (new.id, new.folded);
     END;

INSERT INTO settle_owed (id, owed) VALUES (1, 1);

INSERT INTO regroup_owed (id, owed) VALUES (1, 1);

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
        .create_scalar_function(
            store::WORDS_OF,
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let text: Option<String> = context.get(0)?;
                Ok(text.map(|text| store::words_of(&text)))
            },
        )
        .map_err(|source| Error::store(StoreOp::Open, source))?;
    connection
        .create_scalar_function(
            store::LISTS_WORDS,
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let list: Option<String> = context.get(0)?;
                let words: String = context.get(1)?;
                Ok(list.map(|list| store::lists_words(&list, &words)))
            },
        )
        .map_err(|source| Error::store(StoreOp::Open, source))?;
    connection
        .create_scalar_function(
            store::SPREAD_OF,
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(store::spread_of(context.get(0)?)),
        )
        .map_err(|source| Error::store(StoreOp::Open, source))?;
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

    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|source| Error::store(StoreOp::Transaction, source))?;

    let laid = match stamped(&transaction)? {
        Some(found) if found == SCHEMA_FINGERPRINT => return Ok(()),
        Some(found) => {
            STAMPS
                .iter()
                .position(|stamp| *stamp == found)
                .ok_or(Error::SchemaMismatch {
                    found,
                    expected: SCHEMA_FINGERPRINT,
                })?
        }
        None => {
            transaction
                .execute_batch(V1)
                .map_err(|source| Error::store(StoreOp::LayOut, source))?;
            0
        }
    };

    for step in &MIGRATIONS[laid..] {
        transaction
            .execute_batch(step)
            .map_err(|source| Error::store(StoreOp::LayOut, source))?;
    }
    transaction
        .pragma_update(None, "user_version", as_signed(SCHEMA_FINGERPRINT.0))
        .map_err(|source| Error::store(StoreOp::LayOut, source))?;
    transaction
        .commit()
        .map_err(|source| Error::store(StoreOp::Transaction, source))
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

const fn fingerprint_of(schema: &str) -> SchemaFingerprint {
    SchemaFingerprint(never_unstamped(hashed_on(FNV_OFFSET_BASIS, schema)))
}

const fn stamps() -> [SchemaFingerprint; MIGRATIONS.len() + 1] {
    let mut stamps = [fingerprint_of(V1); MIGRATIONS.len() + 1];
    let mut step = 0;

    while step < MIGRATIONS.len() {
        stamps[step + 1] =
            SchemaFingerprint(never_unstamped(hashed_on(stamps[step].0, MIGRATIONS[step])));
        step += 1;
    }

    stamps
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
    fn a_move_followed_finds_the_rows_naming_a_path_by_an_index() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");

        for (statement, index) in [
            (
                "UPDATE playlist_entries SET path = 'b' WHERE path = 'a'",
                "playlist_entries_by_path",
            ),
            (
                "UPDATE resume_rows SET uri = 'b' WHERE uri = 'a'",
                "resume_rows_by_uri",
            ),
            (
                "UPDATE playlists SET modified = 1 WHERE id IN
                 (SELECT playlist_id FROM playlist_entries WHERE path = 'a')",
                "playlist_entries_by_path",
            ),
        ] {
            let plan: Vec<String> = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {statement}"))
                .and_then(|mut plan| {
                    plan.query_map([], |row| row.get::<_, String>(3))
                        .and_then(Iterator::collect)
                })
                .expect("a plan");

            assert!(
                plan.iter().any(|step| step.contains(index)),
                "{statement} reads every row: {plan:?}"
            );
        }
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
    fn a_catalog_laid_out_at_the_first_release_is_carried_forward_not_refused() {
        let connection = opened();
        connection
            .execute_batch(V1)
            .expect("the first release's schema");
        connection
            .pragma_update(None, "user_version", as_signed(fingerprint_of(V1).0))
            .expect("the first release's stamp");

        lay_out(&connection).expect("the catalog is carried forward");

        let columns: Vec<String> = connection
            .prepare("SELECT name FROM pragma_table_info('listens')")
            .and_then(|mut query| {
                query
                    .query_map([], |row| row.get::<_, String>(0))
                    .and_then(Iterator::collect)
            })
            .expect("the columns");
        assert!(columns.contains(&"origin_device".to_owned()));
        assert!(columns.contains(&"origin_id".to_owned()));
        let kept: Vec<String> = connection
            .prepare("SELECT name FROM pragma_table_info('unheld_listens')")
            .and_then(|mut query| {
                query
                    .query_map([], |row| row.get::<_, String>(0))
                    .and_then(Iterator::collect)
            })
            .expect("the columns");
        for column in ["listen_id", "elsewhere", "origin_device", "origin_id"] {
            assert!(
                kept.contains(&column.to_owned()),
                "an unheld listen keeps no {column}"
            );
        }
        assert!(
            studies_hold_their_bands(&connection),
            "a study kept at the first release was not given a place for its bands"
        );
        assert_eq!(
            stamped(&connection).expect("the stamp reads back"),
            Some(SCHEMA_FINGERPRINT)
        );
        assert!(lay_out(&connection).is_ok());
    }

    fn studies_hold_their_bands(connection: &Connection) -> bool {
        connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('track_studies') WHERE name = 'band_levels'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("the columns")
            == 1
    }

    #[test]
    fn a_release_landed_before_its_credit_was_kept_is_asked_again_and_one_with_its_credit_is_not() {
        let connection = opened();
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == RELEASES_LANDED_WITHOUT_THEIR_CREDIT_ARE_ASKED_AGAIN)
            .expect("the step that asks again");
        connection
            .execute_batch(V1)
            .expect("the first release's schema");
        for step in &MIGRATIONS[..before] {
            connection.execute_batch(step).expect("an earlier step");
        }
        connection
            .pragma_update(None, "user_version", as_signed(STAMPS[before].0))
            .expect("the earlier stamp");
        connection
            .execute_batch(
                "INSERT INTO albums (id, title, mbid, release_title, answered)
                 VALUES (1, 'Uncredited', 'b84ee12a-09ef-421b-82de-0441a926375b', 'Meddle', 500),
                        (2, 'Credited', 'a5bdf1b3-0cd8-3a1e-9e3a-0a4b1a4c4f0e', 'Animals', 500),
                        (3, 'Never answered', NULL, NULL, NULL);
                 INSERT INTO release_credits (album_id, position, name, joined_by)
                 VALUES (2, 0, 'Pink Floyd', '');",
            )
            .expect("albums landed earlier");

        lay_out(&connection).expect("the catalog is carried forward");

        let answered = |id: i64| -> Option<i64> {
            connection
                .query_row("SELECT answered FROM albums WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .expect("an album")
        };
        assert_eq!(answered(1), Some(1));
        assert_eq!(answered(2), Some(500));
        assert_eq!(answered(3), None);
    }

    #[test]
    fn every_step_of_the_chain_stamps_a_catalog_differently() {
        assert_eq!(STAMPS[0], fingerprint_of(V1));
        for pair in STAMPS.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
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

    fn trigram_hits(connection: &Connection, index: &str, column: &str, piece: &str) -> Vec<i64> {
        connection
            .prepare(&format!(
                "SELECT rowid FROM {index} WHERE {column} LIKE ?1 ESCAPE '\\' ORDER BY rowid"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([format!("%{piece}%")], |row| row.get(0))?
                    .collect()
            })
            .expect("the index reads back")
    }

    fn unfinished_indexes(connection: &Connection) -> Vec<String> {
        [
            "artist_releases_fts",
            "artists_key_fts",
            "release_tracks_fts",
        ]
        .into_iter()
        .filter(|index| {
            connection
                .execute(
                    &format!("INSERT INTO {index} ({index}) VALUES ('integrity-check')"),
                    [],
                )
                .is_err()
        })
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn the_discography_index_follows_every_row_added_renamed_or_removed() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO artists (id, key, name) VALUES (1, 'sigur ros', 'Sigur Ros');
                 INSERT INTO artist_releases (artist_id, mbid, title, folded)
                 VALUES (1, 'a', 'Agaetis Byrjun', 'agaetis byrjun');
                 INSERT INTO albums (id, title, artist_id) VALUES (1, 'Takk', 1);
                 INSERT INTO release_tracks (id, album_id, disc, position, title, folded)
                 VALUES (4, 1, 1, 1, 'Glosoli', 'glosoli');",
            )
            .expect("the rows are stored");

        connection
            .execute_batch(
                "UPDATE artists SET key = 'sigur rosa' WHERE id = 1;
                 UPDATE artist_releases SET folded = 'valtari' WHERE mbid = 'a';
                 UPDATE release_tracks SET folded = 'hoppipolla' WHERE id = 4;",
            )
            .expect("the rows are renamed");

        assert_eq!(
            trigram_hits(&connection, "artists_key_fts", "key", "rosa"),
            [1]
        );
        assert_eq!(
            trigram_hits(&connection, "artist_releases_fts", "folded", "valt"),
            [1]
        );
        assert!(trigram_hits(&connection, "artist_releases_fts", "folded", "byrj").is_empty());
        assert_eq!(
            trigram_hits(&connection, "release_tracks_fts", "folded", "hoppi"),
            [4]
        );
        assert!(trigram_hits(&connection, "release_tracks_fts", "folded", "oso").is_empty());

        connection
            .execute_batch("DELETE FROM artists WHERE id = 1; DELETE FROM albums WHERE id = 1;")
            .expect("the rows cascade away");

        assert!(trigram_hits(&connection, "artists_key_fts", "key", "rosa").is_empty());
        assert!(trigram_hits(&connection, "artist_releases_fts", "folded", "valt").is_empty());
        assert!(trigram_hits(&connection, "release_tracks_fts", "folded", "hoppi").is_empty());
        assert!(unfinished_indexes(&connection).is_empty());
    }

    #[test]
    fn only_a_write_to_what_the_grouping_reads_owes_a_regroup_and_of_the_copies_it_touches() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO albums (id, title) VALUES (1, 'Meddle'), (2, 'Animals');
                 INSERT INTO tracks (id, path, title, album_id, alternative_of, sample_rate, channels, sample_format, codec, file_size, modified, added, seen)
                 VALUES (1, 'echoes.flac', 'Echoes', 1, NULL, 44100, 2, 1, 1, 10, 1, 1, 1),
                        (2, 'echoes.mp3', 'Echoes', 1, 1, 44100, 2, 1, 1, 10, 1, 1, 1),
                        (3, 'dogs.flac', 'Dogs', 2, NULL, 44100, 2, 1, 1, 10, 1, 1, 1);
                 UPDATE regroup_owed SET owed = 0;
                 DELETE FROM regroup_owed_tracks;",
            )
            .expect("the rows are stored");
        let owed = || -> (bool, Vec<i64>) {
            let whole = connection
                .query_row("SELECT owed FROM regroup_owed", [], |row| row.get(0))
                .expect("the flag reads back");
            let tracks = connection
                .prepare("SELECT track_id FROM regroup_owed_tracks ORDER BY track_id")
                .and_then(|mut statement| statement.query_map([], |row| row.get(0))?.collect())
                .expect("the owed tracks read back");
            (whole, tracks)
        };
        let forgive = || {
            connection
                .execute_batch("DELETE FROM regroup_owed_tracks;")
                .expect("the owed tracks are forgiven");
        };

        connection
            .execute_batch(
                "UPDATE tracks SET plays = 3, played = 2, favourite = 1, seen = 2 WHERE id = 1;
                 UPDATE albums SET cover_asked = 1 WHERE id = 1;",
            )
            .expect("the counts are written");
        assert_eq!(
            owed(),
            (false, Vec::new()),
            "a play, a favourite or a scan's stamp owed a regroup"
        );

        connection
            .execute(
                "UPDATE albums SET release_title = 'Meddle' WHERE id = 1",
                [],
            )
            .expect("the release is billed");
        assert_eq!(
            owed(),
            (false, vec![1, 2]),
            "an album billed under its release owed the regroup of its tracks alone"
        );

        forgive();
        connection
            .execute(
                "UPDATE tracks SET title = 'One of These Days' WHERE id = 2",
                [],
            )
            .expect("the copy is renamed");
        assert_eq!(
            owed(),
            (false, vec![1, 2]),
            "a copy renamed away did not owe the regroup of the song it was held under"
        );

        forgive();
        connection
            .execute("DELETE FROM tracks WHERE id = 3", [])
            .expect("a lone track goes");
        assert_eq!(
            owed(),
            (false, Vec::new()),
            "a lone track gone owed a regroup"
        );

        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("the best copy goes");
        assert_eq!(
            owed(),
            (false, vec![2]),
            "the best copy gone owed nothing of the copy held under it"
        );
    }

    #[test]
    fn a_track_is_filed_by_its_sort_names_only_while_it_is_billed_as_its_file_names_it() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO tracks (path, title, artist, tagged_title, tagged_artist, title_sort, artist_sort, sample_rate, channels, sample_format, codec, file_size, modified, added, seen)
                 VALUES ('as-tagged.flac', 'The End', 'The Doors', 'The End', 'The Doors', 'End, The', 'Doors, The', 44100, 2, 1, 1, 10, 1, 1, 1),
                        ('renamed.flac', 'The Wall', 'Pink Floyd', 'the wal', 'pink floid', 'Wal, The', 'Floid, Pink', 44100, 2, 1, 1, 10, 1, 1, 1),
                        ('unsorted.flac', 'Angel', 'Massive Attack', 'Angel', 'Massive Attack', NULL, NULL, 44100, 2, 1, 1, 10, 1, 1, 1);",
            )
            .expect("the rows are stored");

        let filed: Vec<(String, String, String)> = connection
            .prepare("SELECT path, title_filed, artist_filed FROM tracks ORDER BY path")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                    .collect()
            })
            .expect("the tracks read back");

        assert_eq!(
            filed,
            [
                (
                    "as-tagged.flac".to_owned(),
                    "End, The".to_owned(),
                    "Doors, The".to_owned()
                ),
                (
                    "renamed.flac".to_owned(),
                    "The Wall".to_owned(),
                    "Pink Floyd".to_owned()
                ),
                (
                    "unsorted.flac".to_owned(),
                    "Angel".to_owned(),
                    "Massive Attack".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn an_album_cover_is_printed_once_as_it_is_written_and_reads_as_it_always_did() {
        let connection = opened();
        let cover: Vec<u8> = (0..1_000_u32).map(|at| (at % 251) as u8).collect();
        let printed_as_before = |id: i64| -> (Option<String>, Option<String>) {
            connection
                .query_row(
                    "SELECT cover_print, length(cover_art) || ':' || hex(substr(cover_art, 1, 256))
                       FROM albums WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("the album reads back")
        };

        lay_out(&connection).expect("the schema applies");
        connection
            .execute(
                "INSERT INTO albums (id, title, cover_art, cover_format) VALUES (1, 'Kept', ?1, 1)",
                [&cover],
            )
            .expect("an album held");

        let (carried_print, carried_before) = printed_as_before(1);
        assert!(carried_print.is_some());
        assert_eq!(carried_print, carried_before);

        connection
            .execute(
                "INSERT INTO albums (id, title, cover_art, cover_format) VALUES (2, 'New', ?1, 1)",
                [&cover[..10]],
            )
            .expect("an album added");
        connection
            .execute("INSERT INTO albums (id, title) VALUES (3, 'Bare')", [])
            .expect("an album with no cover");
        let (added, added_before) = printed_as_before(2);
        assert_eq!(added, added_before);
        assert_eq!(printed_as_before(3), (None, None));

        connection
            .execute("UPDATE albums SET cover_art = ?1 WHERE id = 3", [&cover])
            .expect("a cover found");
        connection
            .execute("UPDATE albums SET cover_art = NULL WHERE id = 1", [])
            .expect("a cover dropped");
        assert_eq!(printed_as_before(3).0, carried_print);
        assert_eq!(printed_as_before(1), (None, None));
    }

    #[test]
    fn a_track_its_file_does_not_sort_is_filed_under_its_artists_sort_name() {
        let connection = opened();
        lay_out(&connection).expect("the schema applies");
        let filed = |path: &str| -> Option<String> {
            connection
                .query_row(
                    "SELECT artist_filed FROM tracks WHERE path = ?1",
                    [path],
                    |row| row.get(0),
                )
                .expect("the track reads back")
        };

        connection
            .execute_batch(
                "INSERT INTO artists (id, key, name, tagged_sort) VALUES (1, 'the doors', 'The Doors', 'Doors, The');
                 INSERT INTO artists (id, key, name) VALUES (2, 'the beatles', 'The Beatles');
                 INSERT INTO tracks (path, title, artist, artist_id, tagged_title, tagged_artist, sample_rate, channels, sample_format, codec, file_size, modified, added, seen)
                 VALUES ('untagged.flac', 'The End', 'The Doors', 1, 'The End', 'The Doors', 44100, 2, 1, 1, 10, 1, 1, 1),
                        ('featuring.flac', 'Riders', 'The Doors & Friends', 1, 'Riders', 'The Doors & Friends', 44100, 2, 1, 1, 10, 1, 1, 1),
                        ('beatles.flac', 'Help!', 'The Beatles', 2, 'Help!', 'The Beatles', 44100, 2, 1, 1, 10, 1, 1, 1);",
            )
            .expect("the rows are stored");

        assert_eq!(filed("untagged.flac").as_deref(), Some("Doors, The"));
        assert_eq!(
            filed("featuring.flac").as_deref(),
            Some("The Doors & Friends")
        );
        assert_eq!(filed("beatles.flac").as_deref(), Some("The Beatles"));

        connection
            .execute_batch("UPDATE artists SET sort_name = 'Beatles, The' WHERE id = 2;")
            .expect("a lookup sorts the artist");
        assert_eq!(filed("beatles.flac").as_deref(), Some("Beatles, The"));

        connection
            .execute_batch(
                "UPDATE tracks SET artist_sort = 'Fab Four' WHERE path = 'beatles.flac';",
            )
            .expect("the file names a sort of its own");
        assert_eq!(filed("beatles.flac").as_deref(), Some("Fab Four"));

        connection
            .execute_batch(
                "UPDATE tracks SET artist = 'Beatles', artist_id = NULL WHERE path = 'beatles.flac';",
            )
            .expect("a lookup renames the credit");
        assert_eq!(filed("beatles.flac").as_deref(), Some("Beatles"));
    }

    #[test]
    fn a_schema_that_has_changed_is_a_fingerprint_that_has_changed() {
        assert_ne!(
            fingerprint_of(V1),
            fingerprint_of(&V1.replace("cover_source", "cover_origin")),
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
