use rusqlite::Connection;

use super::{MIGRATIONS, Store};

const STEPS_RELEASED: &[(&str, u64)] = &[
    ("V1", 0xace1_f7c1_866f_75de),
    ("RECORDS_ARE_KNOWN_BY_THEIR_DIGEST", 0x4122_55a6_1fcc_5987),
    ("A_SPACE_MAY_BE_SHARED_AND_CLOSED", 0xb3cd_0234_bb4e_f1eb),
    (
        "A_CLOSING_WORD_IS_COUNTED_IN_ITS_SPACE",
        0xa105_fe7c_9bff_e1e2,
    ),
    ("A_COMPACTION_MAY_BE_STAGED_IN_PARTS", 0xaacc_78ac_5a4d_8640),
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
fn a_store_counted_at_any_released_step_opens_and_is_carried_to_the_last() {
    for counted in 1..=STEPS_RELEASED.len() {
        let connection = Connection::open_in_memory().expect("a store in memory");
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

        let opened = Store::over(connection).expect("a store a release laid opens");

        let version: u32 = opened
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("the count read");
        assert_eq!(usize::try_from(version).expect("a count"), MIGRATIONS.len());
    }
}
