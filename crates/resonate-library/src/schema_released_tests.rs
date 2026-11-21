use rusqlite::Connection;

use super::{
    MIGRATIONS, Role, SONGS_LEARNT_UNTIMED_ARE_LEARNT_AGAIN, STAMPS, V1, as_signed, configure,
    lay_out,
};

const STAMPS_RELEASED: &[u32] = &[
    0x09eb_8e29,
    0xc7ba_52ad,
    0xda5d_4f8b,
    0xc054_c587,
    0x178b_bef8,
    0xdf70_110c,
    0xec95_9049,
    0xa0f6_f312,
    0xc84d_9064,
    0xcda5_c846,
    0x3897_1de2,
    0x9747_5e32,
    0x14f2_3504,
    0xd525_3219,
    0x04d3_7fe6,
    0xd778_64ce,
    0x5a24_1352,
    0x58ad_aee8,
    0xac46_9a76,
    0xa704_2615,
    0x6801_5946,
    0x6636_1486,
    0xfd62_70ff,
    0x1d2c_9468,
    0x7dc7_2980,
    0xf7e4_450b,
    0x5c90_3176,
    0xa361_5d95,
    0x618e_c682,
    0x4e83_443e,
    0x3025_d264,
    0xbe75_d32a,
    0x7085_f5a7,
    0xc503_ae4a,
    0x88c3_6cc9,
    0xb600_e100,
    0x3d06_7e7a,
    0xb716_c236,
    0x3e30_88f4,
];

const TIMED_GROUP: &str = "11111111-1111-4111-8111-111111111111";

const UNTIMED_GROUP: &str = "22222222-2222-4222-8222-222222222222";

#[test]
fn every_stamp_a_release_shipped_is_hashed_as_it_shipped() {
    assert!(STAMPS.len() >= STAMPS_RELEASED.len());

    for (step, (laid, released)) in STAMPS.iter().zip(STAMPS_RELEASED).enumerate() {
        assert_eq!(
            laid.0, *released,
            "the schema a release stamped {released:#010x} after {step} steps was edited; append a step instead"
        );
    }
}

#[test]
fn a_catalog_stamped_at_any_released_step_is_carried_to_the_last() {
    for stamped in 0..STAMPS_RELEASED.len() {
        let connection = laid_at(stamped);

        lay_out(&connection).expect("a catalog a release laid opens");

        assert_eq!(stamp_of(&connection), STAMPS[MIGRATIONS.len()].0);
    }
}

#[test]
fn groups_learnt_with_no_length_are_owed_a_lookup_and_timed_ones_keep_theirs() {
    let before = MIGRATIONS
        .iter()
        .position(|step| *step == SONGS_LEARNT_UNTIMED_ARE_LEARNT_AGAIN)
        .expect("the step learning untimed groups again");
    let connection = laid_at(before);

    connection
        .execute_batch(&format!(
            "INSERT INTO discography_songs
                    (release_group, recording_mbid, release_mbid, release_title, disc, position,
                     title, artist, length_ms, words, folded)
             VALUES ('{TIMED_GROUP}', 'a', 'r1', 'Timed', 1, 1, 'One', 'Them', 215000, 'one', 'one'),
                    ('{TIMED_GROUP}', 'b', 'r1', 'Timed', 1, 2, 'Two', 'Them', NULL, 'two', 'two'),
                    ('{UNTIMED_GROUP}', 'c', 'r2', 'Untimed', 1, 1, 'Three', 'Them', NULL, 'three', 'three'),
                    ('{UNTIMED_GROUP}', 'd', 'r2', 'Untimed', 1, 2, 'Four', 'Them', NULL, 'four', 'four');
             INSERT INTO discography_songs_read (release_group, read, refusals)
             VALUES ('{TIMED_GROUP}', 100, 0), ('{UNTIMED_GROUP}', 100, 0);"
        ))
        .expect("two groups learnt before the step");

    lay_out(&connection).expect("the step runs");

    let still_read: Vec<String> = connection
        .prepare("SELECT release_group FROM discography_songs_read ORDER BY release_group")
        .and_then(|mut query| {
            query
                .query_map([], |row| row.get(0))
                .and_then(Iterator::collect)
        })
        .expect("the groups read");
    let songs_kept: u32 = connection
        .query_row("SELECT count(*) FROM discography_songs", [], |row| {
            row.get(0)
        })
        .expect("the songs");

    assert_eq!(still_read, vec![TIMED_GROUP.to_owned()]);
    assert_eq!(songs_kept, 4);
}

fn laid_at(steps: usize) -> Connection {
    let connection = Connection::open_in_memory().expect("a catalog in memory");

    configure(&connection, Role::Writing).expect("the pragmas apply");

    connection
        .execute_batch(V1)
        .expect("the first release's schema");
    for step in &MIGRATIONS[..steps] {
        connection.execute_batch(step).expect("a released step");
    }
    connection
        .pragma_update(None, "user_version", as_signed(STAMPS_RELEASED[steps]))
        .expect("the stamp a release wrote");

    connection
}

fn stamp_of(connection: &Connection) -> u32 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
        .expect("the stamp")
        .cast_unsigned()
}
