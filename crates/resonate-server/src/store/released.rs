use resonate_sync::{DeviceId, SpaceId};
use ring::digest::{SHA256, digest};
use rusqlite::{Connection, params};

use super::{Appended, CaughtUp, Compacted, MIGRATIONS, Staged, StagingId, Store};

const COMPACTED: SpaceId = SpaceId([1; 16]);
const LOGGED: SpaceId = SpaceId([6; 16]);
const DESK: DeviceId = DeviceId([2; 16]);
const TABLET: DeviceId = DeviceId([4; 16]);
const THE_FIRST_RELEASE_TO_KNOW_DIGESTS: usize = 2;
const THE_FIRST_RELEASE_TO_KNOW_INCARNATIONS: usize = 6;
const SEALED_BYTES: usize = 61;
const MOST: usize = 16;
const BYTES: usize = 1 << 20;
const SPACE_BYTES: u64 = 1 << 20;

const STEPS_RELEASED: &[(&str, u64)] = &[
    ("V1", 0xace1_f7c1_866f_75de),
    ("RECORDS_ARE_KNOWN_BY_THEIR_DIGEST", 0x4122_55a6_1fcc_5987),
    ("A_SPACE_MAY_BE_SHARED_AND_CLOSED", 0xb3cd_0234_bb4e_f1eb),
    (
        "A_CLOSING_WORD_IS_COUNTED_IN_ITS_SPACE",
        0xa105_fe7c_9bff_e1e2,
    ),
    ("A_COMPACTION_MAY_BE_STAGED_IN_PARTS", 0xaacc_78ac_5a4d_8640),
    ("EVERY_SPACE_CARRIES_AN_INCARNATION", 0x3f48_dd5a_ef73_6af7),
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

fn sealed(seed: u8) -> Vec<u8> {
    (0..SEALED_BYTES)
        .map(|at| {
            seed.wrapping_mul(31)
                .wrapping_add(u8::try_from(at).expect("a byte"))
        })
        .collect()
}

fn sealed_parts(seeds: &[u8]) -> Vec<Vec<u8>> {
    seeds.iter().copied().map(sealed).collect()
}

fn populated(counted: usize) -> Connection {
    let connection = Connection::open_in_memory().expect("a store in memory");
    let digested = counted >= THE_FIRST_RELEASE_TO_KNOW_DIGESTS;
    let space_laid = if counted >= THE_FIRST_RELEASE_TO_KNOW_INCARNATIONS {
        "INSERT INTO spaces (id, token, created, touched, head, bytes, snapshot_through, incarnation) VALUES (?1, ?2, 1, 1, ?3, ?4, ?5, random())"
    } else {
        "INSERT INTO spaces (id, token, created, touched, head, bytes, snapshot_through) VALUES (?1, ?2, 1, 1, ?3, ?4, ?5)"
    };

    for step in &MIGRATIONS[..counted] {
        connection.execute_batch(step).expect("a released step");
    }

    let held = |records: &[u8], parts: &[u8]| {
        i64::try_from((records.len() + parts.len()) * SEALED_BYTES).expect("a size")
    };

    for (space, head, through, records, parts) in [
        (COMPACTED, 5, 2, &[13, 14, 15][..], &[21, 22][..]),
        (LOGGED, 3, 0, &[31, 32, 33][..], &[][..]),
    ] {
        connection
            .execute(
                space_laid,
                params![
                    space.0.as_slice(),
                    [3_u8; 32].as_slice(),
                    head,
                    held(records, parts),
                    through
                ],
            )
            .expect("a space");

        for (seq, seed) in
            (head + 1 - i64::try_from(records.len()).expect("a count")..).zip(records)
        {
            let device = if seq % 2 == 0 { DESK } else { TABLET };
            let blob = sealed(*seed);

            connection
                .execute(
                    "INSERT INTO records (space, seq, device, blob) VALUES (?1, ?2, ?3, ?4)",
                    params![space.0.as_slice(), seq, device.0.as_slice(), blob],
                )
                .expect("a record");

            if digested && seq % 2 == 0 {
                connection
                    .execute(
                        "UPDATE records SET digest = ?3 WHERE space = ?1 AND seq = ?2",
                        params![space.0.as_slice(), seq, digest(&SHA256, &blob).as_ref()],
                    )
                    .expect("a digest");
            }
        }
        for (part, seed) in (0_i64..).zip(parts) {
            connection
                .execute(
                    "INSERT INTO snapshots (space, part, blob) VALUES (?1, ?2, ?3)",
                    params![space.0.as_slice(), part, sealed(*seed)],
                )
                .expect("a snapshot part");
        }
    }
    connection
        .pragma_update(
            None,
            "user_version",
            u32::try_from(counted).expect("a count"),
        )
        .expect("the count a release wrote");
    connection
}

fn snapshot_of(store: &Store, space: SpaceId, after: u64) -> (u64, Vec<Vec<u8>>, Option<u64>) {
    match store
        .catch_up(space, after, None, MOST, BYTES)
        .expect("caught up")
    {
        CaughtUp::Snapshot(snapshot) => (snapshot.through, snapshot.parts, snapshot.next),
        CaughtUp::Records(_) => panic!("a snapshot was owed to a reader at {after}"),
    }
}

fn records_of(store: &Store, space: SpaceId, after: u64) -> Vec<(u64, DeviceId, Vec<u8>)> {
    match store
        .catch_up(space, after, None, MOST, BYTES)
        .expect("caught up")
    {
        CaughtUp::Records(records) => records
            .into_iter()
            .map(|stored| (stored.seq, stored.device, stored.blob))
            .collect(),
        CaughtUp::Snapshot(_) => panic!("records were owed to a reader at {after}"),
    }
}

fn weighed(store: &Store, space: SpaceId) -> (i64, i64) {
    let kept: i64 = store
        .connection
        .query_row(
            "SELECT coalesce((SELECT sum(length(blob)) FROM records WHERE space = ?1), 0)
                  + coalesce((SELECT sum(length(blob)) FROM snapshots WHERE space = ?1), 0)",
            params![space.0.as_slice()],
            |row| row.get(0),
        )
        .expect("weighed");
    let counted: i64 = store
        .connection
        .query_row(
            "SELECT bytes FROM spaces WHERE id = ?1",
            params![space.0.as_slice()],
            |row| row.get(0),
        )
        .expect("counted");

    (kept, counted)
}

#[test]
fn a_store_of_any_release_keeps_its_snapshots_and_records_and_takes_new_work_once_carried_forward()
{
    for counted in 1..=STEPS_RELEASED.len() {
        let mut store = Store::over(populated(counted)).expect("a store a release laid opens");

        let compacted_head = store.head(COMPACTED).expect("head");
        let logged_head = store.head(LOGGED).expect("head");
        let first_snapshot = snapshot_of(&store, COMPACTED, 0);
        let after_the_snapshot = records_of(&store, COMPACTED, 2);
        let whole_log = records_of(&store, LOGGED, 0);
        let incarnations = (
            store.incarnation(COMPACTED).expect("read"),
            store.incarnation(LOGGED).expect("read"),
        );

        assert_eq!(compacted_head, (5, 2), "step {counted}");
        assert_eq!(logged_head, (3, 0), "step {counted}");
        assert_eq!(
            first_snapshot,
            (2, sealed_parts(&[21, 22]), None),
            "step {counted}"
        );
        assert_eq!(
            after_the_snapshot,
            vec![
                (3, TABLET, sealed(13)),
                (4, DESK, sealed(14)),
                (5, TABLET, sealed(15)),
            ],
            "step {counted}"
        );
        assert_eq!(
            whole_log,
            vec![
                (1, TABLET, sealed(31)),
                (2, DESK, sealed(32)),
                (3, TABLET, sealed(33)),
            ],
            "step {counted}"
        );
        assert!(incarnations.0.is_some() && incarnations.1.is_some());
        assert_ne!(incarnations.0, incarnations.1);

        let appended = store
            .append(COMPACTED, DESK, &[sealed(16)], SPACE_BYTES, 2)
            .expect("appended");
        let resent = store
            .append(COMPACTED, DESK, &[sealed(14)], SPACE_BYTES, 2)
            .expect("appended");
        let compacted = store
            .compact(COMPACTED, 4, &sealed_parts(&[41]), SPACE_BYTES, 2)
            .expect("compacted");

        assert_eq!(appended, Appended::Through(6), "step {counted}");
        assert_eq!(
            resent,
            Appended::Through(if counted >= THE_FIRST_RELEASE_TO_KNOW_DIGESTS {
                6
            } else {
                7
            }),
            "step {counted}"
        );
        assert_eq!(compacted, Compacted::Through(4), "step {counted}");
        assert_eq!(
            snapshot_of(&store, COMPACTED, 0),
            (4, sealed_parts(&[41]), None),
            "step {counted}"
        );
        assert_eq!(
            records_of(&store, COMPACTED, 4)
                .into_iter()
                .map(|(seq, _, blob)| (seq, blob))
                .take(2)
                .collect::<Vec<_>>(),
            vec![(5, sealed(15)), (6, sealed(16))],
            "step {counted}"
        );

        let staging = StagingId(7);

        store.stage(LOGGED, staging, 2).expect("staged");

        let staged = store
            .staged(LOGGED, staging, &sealed_parts(&[51, 52]), SPACE_BYTES, 2)
            .expect("staged");
        let swapped = store
            .compact_staged(LOGGED, staging, 2, &sealed_parts(&[53]), SPACE_BYTES, 2)
            .expect("compacted");

        assert_eq!(staged, Staged::Taken, "step {counted}");
        assert_eq!(swapped, Compacted::Through(2), "step {counted}");
        assert_eq!(
            snapshot_of(&store, LOGGED, 0),
            (2, sealed_parts(&[51, 52, 53]), None),
            "step {counted}"
        );
        assert_eq!(
            records_of(&store, LOGGED, 2),
            vec![(3, TABLET, sealed(33))],
            "step {counted}"
        );

        for space in [COMPACTED, LOGGED] {
            let (kept, counted_bytes) = weighed(&store, space);

            assert_eq!(kept, counted_bytes, "step {counted}");
        }
    }
}
