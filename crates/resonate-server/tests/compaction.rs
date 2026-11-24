use std::{
    env, fs,
    path::PathBuf,
    process,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use resonate_server::{Capacity, Limits, Store};
use resonate_sync::{
    read_frame, send_frame,
    wire::{FromServer, LARGEST_FRAME, Record, Refusal, ToServer},
};
use rusqlite::Connection;
use tokio::time::{sleep, timeout};

use crate::frames::{Serving, Session, serving_over, serving_with, welcomed};

const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const DESK: u8 = 1;
const TABLET: u8 = 3;
const PART_BYTES: usize = 192 * 1024;
const STORED_WITHIN: Duration = Duration::from_secs(10);
const POLLED_EVERY: Duration = Duration::from_millis(20);
const RECORD_BYTES: usize = 40;
const PRIOR_SNAPSHOT_BYTES: usize = 30;
const STAGED_BYTES: usize = 100;
const SPACE_CAP: u64 = 200;
const ONE_FRAME_TOTAL_CAP: u64 = 200;
const STAGED_TOTAL_CAP: u64 = 300;
const ONE_FRAME_PART_AT_THE_CAP: usize = 160;
const STAGED_LAST_PART_AT_THE_SPACE_CAP: usize = 60;
const STAGED_LAST_PART_AT_THE_TOTAL_CAP: usize = 160;

fn logged() -> Vec<Vec<u8>> {
    (1..=3).map(|seq| vec![seq; RECORD_BYTES]).collect()
}

fn snapshot_parts(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|part| vec![u8::try_from(part % 251).expect("a byte"); PART_BYTES])
        .collect()
}

async fn say(session: &mut Session, frame: &ToServer) {
    send_frame(&mut session.send, &frame.encoded())
        .await
        .expect("said");
}

async fn heard(session: &mut Session, wanted: impl Fn(&FromServer) -> bool) -> FromServer {
    timeout(ANSWERED_WITHIN, async {
        loop {
            let frame = read_frame(&mut session.recv)
                .await
                .expect("read")
                .expect("a frame");
            let frame = FromServer::decoded(&frame).expect("decoded");
            if wanted(&frame) {
                return frame;
            }
        }
    })
    .await
    .expect("answered in time")
}

async fn next_frame(session: &mut Session) -> FromServer {
    heard(session, |_| true).await
}

async fn appended(desk: &mut Session) {
    say(desk, &ToServer::Append { blobs: logged() }).await;
    heard(desk, |frame| {
        matches!(frame, FromServer::Appended { count: 3 })
    })
    .await;
}

fn the_third_record() -> FromServer {
    FromServer::Records {
        entries: vec![Record {
            seq: 3,
            blob: Some(vec![3; RECORD_BYTES]),
        }],
    }
}

#[tokio::test]
async fn a_compaction_in_one_frame_from_an_older_device_still_replaces_the_log() {
    let serving = serving_with(Limits::default());
    let mut desk = welcomed(&serving, DESK).await;

    appended(&mut desk).await;

    let parts = snapshot_parts(2);

    say(
        &mut desk,
        &ToServer::Compact {
            through: 2,
            parts: parts.clone(),
        },
    )
    .await;
    heard(&mut desk, |frame| {
        *frame == FromServer::Compacted { through: 2 }
    })
    .await;

    let mut tablet = welcomed(&serving, TABLET).await;

    let snapshot = next_frame(&mut tablet).await;
    let records = next_frame(&mut tablet).await;

    assert_eq!(snapshot, FromServer::Snapshot { through: 2, parts });
    assert_eq!(records, the_third_record());
}

#[tokio::test]
async fn a_state_past_a_frames_share_is_compacted_in_parts_and_a_device_joining_reads_it_in_frames()
{
    let serving = serving_with(Limits::default());
    let mut desk = welcomed(&serving, DESK).await;

    appended(&mut desk).await;

    let parts = snapshot_parts(LARGEST_FRAME / 2 / PART_BYTES + 8);
    let (staged, last) = parts.split_at(parts.len() / 2);
    let (first_staged, second_staged) = staged.split_at(staged.len() / 2);

    say(&mut desk, &ToServer::Stage).await;

    let staging = heard(&mut desk, |frame| *frame == FromServer::Staging).await;

    for staged in [first_staged, second_staged] {
        say(
            &mut desk,
            &ToServer::Staged {
                parts: staged.to_vec(),
            },
        )
        .await;
    }
    say(
        &mut desk,
        &ToServer::CompactStaged {
            through: 2,
            parts: last.to_vec(),
        },
    )
    .await;
    heard(&mut desk, |frame| {
        *frame == FromServer::Compacted { through: 2 }
    })
    .await;

    let mut tablet = welcomed(&serving, TABLET).await;

    let FromServer::Snapshot {
        through: first_through,
        parts: first_parts,
    } = next_frame(&mut tablet).await
    else {
        panic!("the snapshot's first frame");
    };
    let FromServer::Snapshot {
        through: last_through,
        parts: last_parts,
    } = next_frame(&mut tablet).await
    else {
        panic!("the snapshot's last frame");
    };
    let records = next_frame(&mut tablet).await;

    assert_eq!(staging, FromServer::Staging);
    assert_eq!(first_through, 0);
    assert_eq!(last_through, 2);
    assert!(first_parts.iter().map(Vec::len).sum::<usize>() <= LARGEST_FRAME / 2);
    assert_eq!([first_parts, last_parts].concat(), parts);
    assert_eq!(records, the_third_record());
}

#[tokio::test]
async fn a_device_leaving_part_way_through_a_compaction_leaves_the_old_log_whole() {
    let watching = Watching::named("leaving");
    let serving = watching.serving(Limits::default(), Capacity::UNLIMITED);
    let mut desk = welcomed(&serving, DESK).await;

    appended(&mut desk).await;

    say(&mut desk, &ToServer::Stage).await;
    heard(&mut desk, |frame| *frame == FromServer::Staging).await;
    say(
        &mut desk,
        &ToServer::Staged {
            parts: snapshot_parts(4),
        },
    )
    .await;

    watching
        .until("the staged parts were never stored", |retained| {
            retained.staged.len() == 4
        })
        .await;
    desk.connection.close(0_u32.into(), b"gone");
    watching
        .until("the staging outlived its session", |retained| {
            retained.stagings == 0
        })
        .await;

    let mut tablet = welcomed(&serving, TABLET).await;

    let caught_up = next_frame(&mut tablet).await;
    let mut desk_again = welcomed(&serving, DESK).await;

    say(
        &mut desk_again,
        &ToServer::CompactStaged {
            through: 2,
            parts: snapshot_parts(1),
        },
    )
    .await;

    let finished_by_another_session = heard(&mut desk_again, |frame| {
        matches!(frame, FromServer::Refused(_) | FromServer::Compacted { .. })
    })
    .await;
    let mut reader = welcomed(&serving, 4).await;

    let still_whole = next_frame(&mut reader).await;

    let whole = FromServer::Records {
        entries: logged()
            .into_iter()
            .zip(1..)
            .map(|(blob, seq)| Record {
                seq,
                blob: Some(blob),
            })
            .collect(),
    };

    assert_eq!(caught_up, whole);
    assert_eq!(
        finished_by_another_session,
        FromServer::Refused(Refusal::Stale)
    );
    assert_eq!(still_whole, whole);
}

#[derive(Debug, PartialEq, Eq)]
struct Retained {
    records: Vec<(i64, Vec<u8>)>,
    snapshot: Vec<(i64, Vec<u8>)>,
    staged: Vec<(i64, Vec<u8>)>,
    stagings: i64,
    staging_bytes: i64,
    held: i64,
    head: i64,
    through: i64,
}

impl Retained {
    fn rows_weigh(&self) -> i64 {
        [&self.records, &self.snapshot]
            .into_iter()
            .flatten()
            .map(|(_, blob)| i64::try_from(blob.len()).expect("a size"))
            .sum()
    }
}

struct Watching {
    folder: PathBuf,
}

impl Watching {
    fn named(name: &str) -> Self {
        let folder = env::temp_dir().join(format!(
            "resonate-compaction-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));

        fs::create_dir_all(&folder).expect("a scratch folder");
        Self { folder }
    }

    fn serving(&self, limits: Limits, capacity: Capacity) -> Serving {
        let store = Store::open(&self.folder.join("store.sqlite"))
            .expect("a store")
            .limited(capacity);

        serving_over(store, limits)
    }

    fn connection(&self) -> Connection {
        let connection = Connection::open(self.folder.join("store.sqlite")).expect("the store");

        connection
            .busy_timeout(Duration::from_secs(5))
            .expect("a timeout");
        connection
    }

    fn blobs(connection: &Connection, query: &str) -> Vec<(i64, Vec<u8>)> {
        connection
            .prepare(query)
            .expect("prepared")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("queried")
            .collect::<Result<_, _>>()
            .expect("rows")
    }

    fn retained(&self) -> Retained {
        let connection = self.connection();
        let (held, head, through) = connection
            .query_row(
                "SELECT bytes, head, snapshot_through FROM spaces",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the space");
        let (stagings, staging_bytes) = connection
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(bytes), 0) FROM stagings",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the stagings");

        Retained {
            records: Self::blobs(&connection, "SELECT seq, blob FROM records ORDER BY seq"),
            snapshot: Self::blobs(
                &connection,
                "SELECT part, blob FROM snapshots ORDER BY part",
            ),
            staged: Self::blobs(&connection, "SELECT part, blob FROM staged ORDER BY part"),
            stagings,
            staging_bytes,
            held,
            head,
            through,
        }
    }

    async fn until(&self, what: &str, reached: impl Fn(&Retained) -> bool) {
        let deadline = Instant::now() + STORED_WITHIN;

        while !reached(&self.retained()) {
            assert!(Instant::now() < deadline, "{what}");

            sleep(POLLED_EVERY).await;
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.folder).ok();
    }
}

fn prior_snapshot() -> Vec<Vec<u8>> {
    vec![vec![8; PRIOR_SNAPSHOT_BYTES]]
}

async fn appended_and_compacted_through_the_first(desk: &mut Session) {
    appended(desk).await;
    say(
        desk,
        &ToServer::Compact {
            through: 1,
            parts: prior_snapshot(),
        },
    )
    .await;
    heard(desk, |frame| *frame == FromServer::Compacted { through: 1 }).await;
}

async fn refused(desk: &mut Session) -> FromServer {
    heard(desk, |frame| matches!(frame, FromServer::Refused(_))).await
}

async fn compacted_through_the_second(desk: &mut Session) {
    heard(desk, |frame| *frame == FromServer::Compacted { through: 2 }).await;
}

async fn compact_in_one_frame(desk: &mut Session, last: usize) {
    say(
        desk,
        &ToServer::Compact {
            through: 2,
            parts: vec![vec![9; last]],
        },
    )
    .await;
}

async fn compact_in_parts(desk: &mut Session, last: usize) {
    say(desk, &ToServer::Stage).await;
    heard(desk, |frame| *frame == FromServer::Staging).await;
    say(
        desk,
        &ToServer::Staged {
            parts: vec![vec![7; STAGED_BYTES]],
        },
    )
    .await;
    say(
        desk,
        &ToServer::CompactStaged {
            through: 2,
            parts: vec![vec![9; last]],
        },
    )
    .await;
}

fn the_prior_log_is(retained: &Retained) {
    assert_eq!(retained.head, 3);
    assert_eq!(retained.through, 1);
    assert_eq!(
        retained.records,
        vec![(2, vec![2; RECORD_BYTES]), (3, vec![3; RECORD_BYTES])]
    );
    assert_eq!(retained.snapshot, vec![(0, vec![8; PRIOR_SNAPSHOT_BYTES])]);
    assert_eq!(retained.stagings, 0);
    assert_eq!(retained.staged, Vec::new());
    assert_eq!(retained.held, retained.rows_weigh());
}

async fn a_refused_compaction_keeps_the_prior_log_byte_for_byte(
    watching: &Watching,
    serving: &Serving,
    compact: impl AsyncFn(&mut Session, usize),
    just_past: usize,
    at_the_cap: usize,
) -> Retained {
    let mut desk = welcomed(serving, DESK).await;

    appended_and_compacted_through_the_first(&mut desk).await;

    let before = watching.retained();

    the_prior_log_is(&before);

    compact(&mut desk, just_past).await;

    let refusal = refused(&mut desk).await;
    let after = watching.retained();

    assert_eq!(refusal, FromServer::Refused(Refusal::SpaceFull));
    assert_eq!(after, before);

    compact(&mut desk, at_the_cap).await;
    compacted_through_the_second(&mut desk).await;

    watching.retained()
}

#[tokio::test]
async fn a_one_frame_compaction_past_the_space_cap_is_refused_and_keeps_the_prior_log_and_snapshot()
{
    let watching = Watching::named("space-one-frame");
    let serving = watching.serving(
        Limits {
            space_bytes: SPACE_CAP,
            ..Limits::default()
        },
        Capacity::UNLIMITED,
    );

    let compacted = a_refused_compaction_keeps_the_prior_log_byte_for_byte(
        &watching,
        &serving,
        compact_in_one_frame,
        ONE_FRAME_PART_AT_THE_CAP + 1,
        ONE_FRAME_PART_AT_THE_CAP,
    )
    .await;

    assert_eq!(compacted.records, vec![(3, vec![3; RECORD_BYTES])]);
    assert_eq!(
        compacted.snapshot,
        vec![(0, vec![9; ONE_FRAME_PART_AT_THE_CAP])]
    );
    assert_eq!(compacted.held, i64::try_from(SPACE_CAP).expect("a size"));
    assert_eq!(compacted.held, compacted.rows_weigh());
}

#[tokio::test]
async fn a_one_frame_compaction_past_the_total_cap_is_refused_and_keeps_the_prior_log_and_snapshot()
{
    let watching = Watching::named("total-one-frame");
    let serving = watching.serving(
        Limits::default(),
        Capacity {
            spaces: u64::MAX,
            bytes: ONE_FRAME_TOTAL_CAP,
        },
    );

    let compacted = a_refused_compaction_keeps_the_prior_log_byte_for_byte(
        &watching,
        &serving,
        compact_in_one_frame,
        ONE_FRAME_PART_AT_THE_CAP + 1,
        ONE_FRAME_PART_AT_THE_CAP,
    )
    .await;

    assert_eq!(compacted.records, vec![(3, vec![3; RECORD_BYTES])]);
    assert_eq!(
        compacted.snapshot,
        vec![(0, vec![9; ONE_FRAME_PART_AT_THE_CAP])]
    );
    assert_eq!(
        compacted.held,
        i64::try_from(ONE_FRAME_TOTAL_CAP).expect("a size")
    );
    assert_eq!(compacted.held, compacted.rows_weigh());
}

#[tokio::test]
async fn a_staged_compaction_past_the_space_cap_is_refused_and_keeps_the_prior_log_and_snapshot() {
    let watching = Watching::named("space-staged");
    let serving = watching.serving(
        Limits {
            space_bytes: SPACE_CAP,
            ..Limits::default()
        },
        Capacity::UNLIMITED,
    );

    let compacted = a_refused_compaction_keeps_the_prior_log_byte_for_byte(
        &watching,
        &serving,
        compact_in_parts,
        STAGED_LAST_PART_AT_THE_SPACE_CAP + 1,
        STAGED_LAST_PART_AT_THE_SPACE_CAP,
    )
    .await;

    assert_eq!(compacted.records, vec![(3, vec![3; RECORD_BYTES])]);
    assert_eq!(
        compacted.snapshot,
        vec![
            (0, vec![7; STAGED_BYTES]),
            (1, vec![9; STAGED_LAST_PART_AT_THE_SPACE_CAP])
        ]
    );
    assert_eq!(compacted.stagings, 0);
    assert_eq!(compacted.staged, Vec::new());
    assert_eq!(compacted.held, i64::try_from(SPACE_CAP).expect("a size"));
    assert_eq!(compacted.held, compacted.rows_weigh());
}

#[tokio::test]
async fn a_staged_compaction_past_the_total_cap_is_refused_and_keeps_the_prior_log_and_snapshot() {
    let watching = Watching::named("total-staged");
    let serving = watching.serving(
        Limits::default(),
        Capacity {
            spaces: u64::MAX,
            bytes: STAGED_TOTAL_CAP,
        },
    );

    let compacted = a_refused_compaction_keeps_the_prior_log_byte_for_byte(
        &watching,
        &serving,
        compact_in_parts,
        STAGED_LAST_PART_AT_THE_TOTAL_CAP + 1,
        STAGED_LAST_PART_AT_THE_TOTAL_CAP,
    )
    .await;

    assert_eq!(compacted.records, vec![(3, vec![3; RECORD_BYTES])]);
    assert_eq!(
        compacted.snapshot,
        vec![
            (0, vec![7; STAGED_BYTES]),
            (1, vec![9; STAGED_LAST_PART_AT_THE_TOTAL_CAP])
        ]
    );
    assert_eq!(compacted.stagings, 0);
    assert_eq!(compacted.staged, Vec::new());
    assert_eq!(
        compacted.held,
        i64::try_from(STAGED_TOTAL_CAP).expect("a size")
    );
    assert_eq!(compacted.held, compacted.rows_weigh());
}

#[tokio::test]
async fn a_device_leaving_after_it_staged_frees_what_it_staged_without_waiting_for_a_sweep() {
    const WHAT_ONE_MORE_NEEDS: usize = 100;

    let watching = Watching::named("freed");
    let serving = watching.serving(
        Limits::default(),
        Capacity {
            spaces: u64::MAX,
            bytes: 400,
        },
    );
    let mut desk = welcomed(&serving, DESK).await;
    let mut tablet = welcomed(&serving, TABLET).await;

    appended(&mut desk).await;
    say(&mut desk, &ToServer::Stage).await;
    heard(&mut desk, |frame| *frame == FromServer::Staging).await;
    say(
        &mut desk,
        &ToServer::Staged {
            parts: vec![vec![5; 200]],
        },
    )
    .await;

    watching
        .until("the staged part was never stored", |retained| {
            retained.staging_bytes == 200 && retained.staged.len() == 1
        })
        .await;

    let one_more = ToServer::Append {
        blobs: vec![vec![6; WHAT_ONE_MORE_NEEDS]],
    };

    say(&mut tablet, &one_more).await;

    let while_staged = heard(&mut tablet, |frame| {
        matches!(frame, FromServer::Appended { .. } | FromServer::Refused(_))
    })
    .await;

    desk.connection.close(0_u32.into(), b"gone");

    watching
        .until("the staging outlived its session", |retained| {
            retained.stagings == 0 && retained.staging_bytes == 0
        })
        .await;

    say(&mut tablet, &one_more).await;

    let once_gone = heard(&mut tablet, |frame| {
        matches!(frame, FromServer::Appended { .. } | FromServer::Refused(_))
    })
    .await;

    assert_eq!(while_staged, FromServer::Refused(Refusal::SpaceFull));
    assert_eq!(once_gone, FromServer::Appended { count: 1 });
}
