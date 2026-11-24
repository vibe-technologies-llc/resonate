use std::{env, fs, process};

use rusqlite::{Connection, functions::FunctionFlags};

use super::{Catalog, MIGRATIONS};

const STEPS_RELEASED: &[(&str, u64)] = &[
    ("V1", 0x08c04e42ffe635a3),
    ("LISTENS_NAME_THEIR_ORIGIN", 0x6dbec2d3df611d51),
    ("LISTENS_NEVER_REUSE_AN_ID", 0xbee40e6e166166bf),
    ("DOCUMENTS_KEEP_WHEN_THEY_WERE_ADDED", 0x4b7c485061f18996),
    (
        "DOCUMENTS_FROM_BEFORE_DATES_TAKE_THEIR_FILES_TIME",
        0x84429d6eeb6afa78,
    ),
    ("STUDIES_KEEP_WHAT_WAS_HEARD", 0xd06cb712a5a1ee27),
    (
        "PLAYLISTS_FILL_THEMSELVES_FROM_A_SAVED_SEARCH",
        0xac2fc6a4c679bed9,
    ),
    (
        "DOCUMENTS_THAT_WILL_NOT_DECODE_ARE_REMEMBERED",
        0xb47bd70d082d86c5,
    ),
    ("STUDIES_KEEP_WHERE_THE_MUSIC_SOUNDS", 0x8c47d79e22c4a110),
    ("ANSWERS_TRAVEL", 0x7e588e90a0bebfb6),
    (
        "SONGS_RECEIVED_ARE_HELD_BY_WHAT_WAS_ASKED",
        0x2096f18900f48472,
    ),
    (
        "DOCUMENTS_REMEMBER_WHEN_THEY_WERE_DOWNLOADED",
        0x4bbd3eb47645055e,
    ),
    ("SONGS_OF_A_WITHDRAWN_FOLDER_STAY_HELD", 0x8a475eb4e3b37cdd),
    ("SONGS_WAITING_KNOW_THEIR_FILE", 0x61c5a39f6498b241),
    (
        "STUDIES_KEEP_THEIR_PRINT_AND_WHAT_IT_WAS_HEARD_AS",
        0xd771d5d50f2e2364,
    ),
    ("STUDIES_KEEP_WHERE_THEIR_PEAKS_GATHER", 0x090f43eaf56a8b15),
    (
        "PEAKS_KEPT_AS_ONE_MOMENT_ARE_MEASURED_AGAIN",
        0x5164f6b4d101e4be,
    ),
    ("ZONES_KEEP_THEIR_OWN_QUEUES", 0x13fa05df0a5b6dfc),
    ("ZONES_KEEP_THEIR_VOLUME", 0xf8fba1b80c4e31e7),
    ("QUEUES_KEEP_SONGS_PLAYED_IN_PLACE", 0x2340f373669afaaf),
    (
        "LISTENS_NAME_THE_SERVED_SONG_THEY_HEARD",
        0x1fc33a886ff8aa7f,
    ),
    ("ZONES_KEEP_THEIR_RENDERER", 0x46886ddb4bccdcf9),
    ("SONGS_DELETED_HERE_ARE_REMEMBERED", 0x099272d6aae8de92),
    ("PLAYLISTS_REMEMBER_THEIR_LINK", 0x146c9ec19afc992a),
    ("PROFILES_ARE_FOLLOWED_WHOLE", 0x42ca165ef1d0aba6),
    ("ALBUMS_KEEP_THE_SONGS_OF_THEIR_RELEASE", 0x8a5ba88d81cc1062),
    ("LISTENS_ARE_READ_BY_WHEN_THEY_BEGAN", 0xd9cd3d1821a84a9a),
    (
        "A_PLAYLIST_MAY_BE_SHARED_WITH_OTHER_PEOPLE",
        0xcf1d835e85fc8623,
    ),
    ("ARTISTS_KEEP_THEIR_DISCOGRAPHY", 0xf49e17eee4ee3ed7),
    ("UNHELD_RELEASES_KEEP_THEIR_SLEEVE", 0xc4d8918152def0c0),
    ("DOCUMENTS_ARE_FOUND_BY_THEIR_ISRC", 0x70e0a1dc98c48c7f),
    ("A_YEAR_OVER_IS_KEPT_AS_IT_WAS_HEARD", 0x88fa7de26136368b),
    (
        "QUEUES_KEEP_THEIR_SHUFFLE_LIST_ORDER_AND_ROWS_PLAYED_NEXT",
        0x45b41227557ba17b,
    ),
    ("A_SONG_PLAYS_THE_VERSION_CHOSEN_FOR_IT", 0x106edc7339e1be92),
    ("RENDERERS_KEEP_WHAT_THEY_PLAYED", 0x7324449af6fb58a9),
    ("QUEUES_MAY_BE_LEFT_BEHIND_AT_OPENING", 0xbc8d243d332fb79c),
    (
        "FETCHED_COVERS_ARE_ASKED_FOR_A_LARGER_ONE",
        0xcf164a7f3b0cb3aa,
    ),
    ("LOSSY_SONGS_WAIT_TO_BE_REPLACED", 0x9f394dc465ffc3b2),
    (
        "SONGS_DELETED_EVERYWHERE_ARE_TOLD_AND_TAKEN",
        0xb7adf00eee24582b,
    ),
    ("LYRICS_KEEP_HOW_FAR_AHEAD_THEY_RUN", 0xa37cc5c588536d00),
    ("A_VERSION_IS_KEPT_BESIDE_ITS_SONG", 0xbc71afe39684696d),
    ("EXPLORE_SEARCHES_ARE_KEPT", 0x1f7a8c1dd82566a8),
];

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fingerprint_of(step: &str) -> u64 {
    step.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

#[test]
fn every_step_a_release_shipped_is_laid_as_it_shipped() {
    assert!(MIGRATIONS.len() >= STEPS_RELEASED.len());

    for (laid, (name, released)) in MIGRATIONS.iter().zip(STEPS_RELEASED) {
        assert_eq!(
            fingerprint_of(laid),
            *released,
            "{name} was edited after a release laid it on phones; append a step instead"
        );
    }
}

#[test]
fn a_catalog_stamped_at_any_released_step_opens_and_is_carried_to_the_last() {
    let folder = env::temp_dir().join(format!("resonate-mobile-released-{}", process::id()));
    let _ = fs::remove_dir_all(&folder);

    fs::create_dir_all(&folder).expect("a folder");

    for stamped in 1..=STEPS_RELEASED.len() {
        let path = folder.join(format!("documents-{stamped}.sqlite"));
        let laid = Connection::open(&path).expect("a catalog");

        laid.create_scalar_function(
            "folded",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
        )
        .expect("the fold");
        for step in &MIGRATIONS[..stamped] {
            laid.execute_batch(step).expect("a released step");
        }
        laid.pragma_update(
            None,
            "user_version",
            u32::try_from(stamped).expect("a count"),
        )
        .expect("the stamp");
        drop(laid);

        drop(Catalog::open(&path).expect("a catalog a release laid opens"));

        let version: u32 = Connection::open(&path)
            .expect("the catalog again")
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("the stamp read");

        assert_eq!(usize::try_from(version).expect("a count"), MIGRATIONS.len());
    }

    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_catalog_of_before_larger_covers_owes_its_fetched_covers_one_ask_and_keeps_them() {
    let folder = env::temp_dir().join(format!("resonate-mobile-larger-{}", process::id()));
    let _ = fs::remove_dir_all(&folder);

    fs::create_dir_all(&folder).expect("a folder");

    let path = folder.join("documents.sqlite");
    let before = MIGRATIONS
        .iter()
        .position(|step| *step == super::FETCHED_COVERS_ARE_ASKED_FOR_A_LARGER_ONE)
        .expect("the larger covers step");

    let laid = Connection::open(&path).expect("a catalog");

    laid.create_scalar_function(
        "folded",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
    )
    .expect("the fold");
    for step in &MIGRATIONS[..before] {
        laid.execute_batch(step).expect("an earlier step");
    }
    laid.execute_batch(
        "INSERT INTO pictures (id, crc, bytes) VALUES (1, 0, x'89504e47');
         INSERT INTO album_art (album_key, release, origin, picture)
         VALUES ('album', 'f5093c06-23e3-404f-aeaa-40f72885ee3a', 'coverartarchive', 1);",
    )
    .expect("an album with a fetched cover");
    laid.pragma_update(
        None,
        "user_version",
        u32::try_from(before).expect("a count"),
    )
    .expect("the stamp");
    drop(laid);

    drop(Catalog::open(&path).expect("the catalog opens"));

    let opened = Connection::open(&path).expect("the catalog again");
    let owed: bool = opened
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'larger_covers_wanted')",
            [],
            |row| row.get(0),
        )
        .expect("the schema reads back");
    let kept: i64 = opened
        .query_row(
            "SELECT picture FROM album_art WHERE album_key = 'album'",
            [],
            |row| row.get(0),
        )
        .expect("the album's cover");

    drop(opened);

    let _ = fs::remove_dir_all(&folder);

    assert!(owed, "the larger covers were not owed after the step");
    assert_eq!(kept, 1, "the step took a fetched cover away");
}

#[test]
fn a_populated_first_release_catalog_keeps_its_songs_queue_favourites_and_listens_across_every_step()
 {
    let folder = env::temp_dir().join(format!("resonate-mobile-populated-{}", process::id()));
    let _ = fs::remove_dir_all(&folder);

    fs::create_dir_all(&folder).expect("a folder");

    let path = folder.join("documents.sqlite");
    let laid = Connection::open(&path).expect("a catalog");

    laid.create_scalar_function(
        "folded",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
    )
    .expect("the fold");
    laid.execute_batch(MIGRATIONS[0])
        .expect("the first release's schema");
    laid.execute_batch(
        "INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec, favourite, size, modified)
         VALUES (7, 'content://tree/a.flac', 'a.flac', 'First', 'Ada', 'Album', 180000, 44100, 'FLAC', 1, 1000, 1600000000000),
                (8, 'content://tree/b.flac', 'b.flac', 'Second', 'Ada', 'Album', 200000, 48000, 'FLAC', 0, 2000, NULL),
                (9, 'content://tree/c.flac', 'c.flac', 'Third', 'Bo', 'Other', 220000, 96000, 'FLAC', 1, 3000, 1650000000000);
         INSERT INTO resume (id, row_at, position_ms) VALUES (1, 2, 90500);
         INSERT INTO queue (position, track) VALUES (0, 9), (1, 7), (2, 8);
         INSERT INTO listens (id, track, began, heard_ms, counted, elsewhere)
         VALUES (11, 7, 1700000000, 180000, 1, 0), (13, 9, 1700000500, 30000, 0, 1);
         INSERT INTO submissions (service, through) VALUES ('listenbrainz', 20);
         INSERT INTO playlists (id, name, folded, revision, created) VALUES (3, 'Evening', 'evening', 4, 1700);
         INSERT INTO playlist_rows (playlist, position, track) VALUES (3, 0, 8), (3, 1, 7);
         INSERT INTO loves_told (service, recording) VALUES ('listenbrainz', '4d5e6f70-0000-4000-8000-000000000001');",
    )
    .expect("a populated catalog");
    laid.pragma_update(None, "user_version", 1_u32)
        .expect("the stamp");
    drop(laid);

    drop(Catalog::open(&path).expect("the catalog is carried to the last step"));

    let opened = Connection::open(&path).expect("the catalog again");

    let version: u32 = opened
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the stamp read");
    let documents: Vec<(i64, String, String, i64, i64)> = rows_of(
        &opened,
        "SELECT id, uri, title, favourite, coalesce(added, 0) FROM documents ORDER BY id",
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    );
    let queue: Vec<(i64, i64)> = rows_of(
        &opened,
        "SELECT position, track FROM queue ORDER BY position",
        |row| Ok((row.get(0)?, row.get(1)?)),
    );
    let resume: (i64, i64) = opened
        .query_row(
            "SELECT row_at, position_ms FROM resume WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the saved place");
    let listens: Vec<(i64, i64, i64, i64, i64, i64)> = rows_of(
        &opened,
        "SELECT id, track, began, heard_ms, counted, elsewhere FROM listens ORDER BY id",
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    );
    let playlist: (String, i64, i64) = opened
        .query_row(
            "SELECT name, revision, created FROM playlists WHERE id = 3",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the playlist");
    let playlist_rows: Vec<(i64, i64)> = rows_of(
        &opened,
        "SELECT position, track FROM playlist_rows WHERE playlist = 3 ORDER BY position",
        |row| Ok((row.get(0)?, row.get(1)?)),
    );
    let told: i64 = opened
        .query_row("SELECT count(*) FROM loves_told", [], |row| row.get(0))
        .expect("the loves told");

    opened
        .execute_batch("INSERT INTO listens (track, began, heard_ms) VALUES (7, 1700001000, 1000)")
        .expect("a listen after the upgrade");

    let next_listen = opened.last_insert_rowid();

    drop(opened);

    let _ = fs::remove_dir_all(&folder);

    assert_eq!(usize::try_from(version).expect("a count"), MIGRATIONS.len());
    assert_eq!(
        documents
            .iter()
            .map(|(id, uri, title, favourite, _)| (*id, uri.as_str(), title.as_str(), *favourite))
            .collect::<Vec<_>>(),
        vec![
            (7, "content://tree/a.flac", "First", 1),
            (8, "content://tree/b.flac", "Second", 0),
            (9, "content://tree/c.flac", "Third", 1),
        ]
    );
    assert_eq!(documents[0].4, 1_600_000_000);
    assert_eq!(documents[2].4, 1_650_000_000);
    assert!(documents[1].4 > 0, "a document with no time was dated");
    assert_eq!(queue, vec![(0, 9), (1, 7), (2, 8)]);
    assert_eq!(resume, (2, 90_500));
    assert_eq!(
        listens,
        vec![
            (11, 7, 1_700_000_000, 180_000, 1, 0),
            (13, 9, 1_700_000_500, 30_000, 0, 1),
        ]
    );
    assert_eq!(playlist, ("Evening".to_owned(), 4, 1700));
    assert_eq!(playlist_rows, vec![(0, 8), (1, 7)]);
    assert_eq!(told, 1);
    assert_eq!(next_listen, 21);
}

fn rows_of<T>(
    connection: &Connection,
    statement: &str,
    read: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Vec<T> {
    connection
        .prepare(statement)
        .and_then(|mut query| query.query_map([], read).and_then(Iterator::collect))
        .expect("the rows")
}
