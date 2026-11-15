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
