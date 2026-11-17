use rusqlite::Connection;

use super::{Ledger, MIGRATIONS};

const STEPS_RELEASED: &[(&str, u64)] = &[
    ("V1", 0xcef1_16d7_c8d7_4f77),
    ("ANSWERS_TRAVEL", 0xe874_c1a7_4bec_3f8e),
    ("SONGS_REFUSED_ARE_REMEMBERED", 0xb3d9_2d0f_5900_f950),
    ("NAMESAKES_MADE_APART_BECOME_ONE", 0xc7ce_4afe_b760_88bf),
    ("SHARED_PLAYLISTS_STAND_APART", 0xfdea_32d2_ad71_4393),
    ("LAYOUTS_TRAVEL", 0x959f_8ed5_dc59_8e42),
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
            "{name} was edited after a release laid it; append a step instead"
        );
    }
}

#[test]
fn a_ledger_counted_at_any_released_step_opens_and_is_carried_to_the_last() {
    for counted in 1..=STEPS_RELEASED.len() {
        let connection = Connection::open_in_memory().expect("a ledger in memory");
        for step in &MIGRATIONS[..counted] {
            connection.execute_batch(step).expect("a released step");
        }
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(counted).expect("a count"),
            )
            .expect("the count a release wrote");

        let ledger = Ledger::over(connection).expect("a ledger a release laid opens");

        let version: u32 = ledger
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("the count read");
        assert_eq!(usize::try_from(version).expect("a count"), MIGRATIONS.len());
    }
}
