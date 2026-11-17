use rusqlite::Connection;

use super::{Kept, MIGRATIONS};

const STEPS_RELEASED: &[(&str, u64)] = &[
    ("V1", 0xddc7_6fa0_1522_807b),
    ("AN_ANSWERS_NUMBER_NEVER_GOES_BACK", 0x422a_1a27_329b_5e0a),
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
fn a_kept_store_counted_at_any_released_step_opens_and_is_carried_to_the_last() {
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

        let opened = Kept::over(connection).expect("a store a release laid opens");

        let version: u32 = opened
            .connection
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("the count read");
        assert_eq!(usize::try_from(version).expect("a count"), MIGRATIONS.len());
    }
}
