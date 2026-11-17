use std::time::Duration;

use resonate_server::Limits;
use resonate_sync::{
    read_frame, send_frame,
    wire::{FromServer, LARGEST_FRAME, Record, Refusal, ToServer},
};
use tokio::time::timeout;

use crate::frames::{Session, serving_with, welcomed};

const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const DESK: u8 = 1;
const TABLET: u8 = 3;
const PART_BYTES: usize = 192 * 1024;

fn logged() -> Vec<Vec<u8>> {
    (1..=3).map(|seq| vec![seq; 40]).collect()
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
            blob: Some(vec![3; 40]),
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
    let serving = serving_with(Limits::default());
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
    desk.connection.close(0_u32.into(), b"gone");
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
