use std::{
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use parking_lot::Mutex;
use resonate_server::{Limits, Store};
use resonate_sync::{
    SpaceId, read_frame, send_frame,
    wire::{Refusal, ToServer},
};
use rusqlite::{Connection, params};
use tokio::time::timeout;

use crate::frames::{announce, refused_with, serving_over, welcomed};

const SPACE: SpaceId = SpaceId([5; 16]);
const THE_BINARY_ENDS_WITHIN: Duration = Duration::from_secs(10);
const A_SWEEP_LANDS_WITHIN: Duration = Duration::from_secs(10);
const A_SESSION_ENDS_WITHIN: Duration = Duration::from_secs(5);
const LOOKED_AGAIN_AFTER: Duration = Duration::from_millis(20);
const CLAP_REFUSED_ITS_ARGUMENTS: i32 = 2;
const MEBIBYTE: u32 = 1024 * 1024;
const ESCAPE: char = '\u{1b}';

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!(
        "resonate-server-upkeep-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));

    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| i64::try_from(since.as_secs()).expect("seconds"))
        .expect("after the epoch")
}

fn pem(label: &str, der: &[u8]) -> String {
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(der)
    )
}

fn credentials_in(folder: &Path) -> (PathBuf, PathBuf) {
    let signing = rcgen::KeyPair::generate().expect("a key");
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .and_then(|params| params.self_signed(&signing))
        .expect("a certificate");
    let chain = folder.join("chain.pem");
    let key = folder.join("key.pem");

    fs::write(&chain, pem("CERTIFICATE", certificate.der())).expect("written");
    fs::write(&key, pem("PRIVATE KEY", &signing.serialize_der())).expect("written");
    (chain, key)
}

fn a_space_touched_at(store: &Path, touched: i64) {
    drop(Store::open(store).expect("a store laid out"));

    Connection::open(store)
        .expect("the store")
        .execute(
            "INSERT INTO spaces (id, token, created, touched, head) VALUES (?1, ?2, ?3, ?3, 1)",
            params![SPACE.0.as_slice(), [3_u8; 32].as_slice(), touched],
        )
        .expect("a space");
}

fn spaces_in(store: &Path) -> i64 {
    Connection::open(store)
        .expect("the store")
        .query_row("SELECT count(*) FROM spaces", [], |row| row.get(0))
        .expect("counted")
}

fn stagings_in(store: &Path) -> i64 {
    Connection::open(store)
        .expect("the store")
        .query_row("SELECT count(*) FROM stagings", [], |row| row.get(0))
        .expect("counted")
}

fn ended_within(mut child: Child, patience: Duration) -> Option<Output> {
    let began = Instant::now();
    loop {
        if child.try_wait().expect("asked").is_some() {
            return Some(child.wait_with_output().expect("its output"));
        }
        if began.elapsed() > patience {
            let _ = child.kill();
            let _ = child.wait();

            return None;
        }
        thread::sleep(LOOKED_AGAIN_AFTER);
    }
}

fn the_server(arguments: &[&OsStr]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_resonate-server"))
        .args(arguments)
        .env_remove("NO_COLOR")
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server started")
}

#[test]
fn a_retention_of_zero_days_is_refused_before_the_store_is_opened() {
    let folder = scratch("zero-days");
    let (chain, key) = credentials_in(&folder);
    let store = folder.join("records.sqlite");

    a_space_touched_at(&store, now() - 5);

    let server = the_server(&[
        "--listen".as_ref(),
        "127.0.0.1:0".as_ref(),
        "--certificate".as_ref(),
        chain.as_os_str(),
        "--key".as_ref(),
        key.as_os_str(),
        "--store".as_ref(),
        store.as_os_str(),
        "--idle-days".as_ref(),
        "0".as_ref(),
    ]);

    let ended = ended_within(server, THE_BINARY_ENDS_WITHIN);
    let spaces = spaces_in(&store);

    fs::remove_dir_all(&folder).ok();

    let output = ended.expect("the server took a retention of zero days and kept running");

    assert_eq!(output.status.code(), Some(CLAP_REFUSED_ITS_ARGUMENTS));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--idle-days"));
    assert_eq!(
        spaces, 1,
        "a space was swept by a retention the server should have refused"
    );
}

#[test]
fn what_the_server_logs_to_a_journal_carries_plain_levels_and_no_terminal_escapes() {
    let folder = scratch("plain-journal");
    let missing = folder.join("missing.pem");
    let store = folder.join("records.sqlite");

    let server = the_server(&[
        "--listen".as_ref(),
        "127.0.0.1:0".as_ref(),
        "--certificate".as_ref(),
        missing.as_os_str(),
        "--key".as_ref(),
        missing.as_os_str(),
        "--store".as_ref(),
        store.as_os_str(),
    ]);

    let ended = ended_within(server, THE_BINARY_ENDS_WITHIN);

    fs::remove_dir_all(&folder).ok();

    let output = ended.expect("the server stopped on a missing certificate");
    let journal = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    assert!(journal.contains(" ERROR "), "{journal}");
    assert!(journal.contains("the server stopped"), "{journal}");
    assert!(!journal.contains(ESCAPE), "{journal:?}");
}

#[tokio::test]
async fn a_staging_an_earlier_run_left_behind_is_reclaimed_as_the_server_opens() {
    let folder = scratch("left-staging");
    let store = folder.join("records.sqlite");

    a_space_touched_at(&store, now());
    {
        let earlier = Connection::open(&store).expect("the store");

        earlier
            .execute(
                "INSERT INTO stagings (space, id, touched, bytes, parts) VALUES (?1, 7, 1, 10, 1)",
                params![SPACE.0.as_slice()],
            )
            .expect("a staging");
        earlier
            .execute(
                "INSERT INTO staged (space, part, blob) VALUES (?1, 0, ?2)",
                params![SPACE.0.as_slice(), [9_u8; 10].as_slice()],
            )
            .expect("a part");
    }

    let left = stagings_in(&store);
    let _serving = serving_over(Store::open(&store).expect("a store"), Limits::default());

    let reclaimed = timeout(A_SWEEP_LANDS_WITHIN, async {
        while stagings_in(&store) > 0 {
            tokio::time::sleep(LOOKED_AGAIN_AFTER).await;
        }
    })
    .await;
    let spaces = spaces_in(&store);

    fs::remove_dir_all(&folder).ok();

    assert_eq!(left, 1);
    assert!(
        reclaimed.is_ok(),
        "the staging waited for a sweep an hour away"
    );
    assert_eq!(spaces, 1, "the space the staging belonged to went with it");
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn said(&self) -> String {
        String::from_utf8_lossy(&self.0.lock()).into_owned()
    }
}

#[tokio::test]
async fn every_pass_of_a_sweep_that_fails_is_reported_by_name() {
    let folder = scratch("failed-sweep");
    let store = folder.join("records.sqlite");

    a_space_touched_at(&store, 1);
    Connection::open(&store)
        .expect("the store")
        .execute_batch(&format!(
            "UPDATE spaces SET head = 0;
             INSERT INTO stagings (space, id, touched) VALUES (x'{space}', 7, 1);
             CREATE TRIGGER no_space_goes BEFORE DELETE ON spaces BEGIN SELECT RAISE(ABORT, 'refused'); END;
             CREATE TRIGGER no_staging_goes BEFORE DELETE ON stagings BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            space = "05".repeat(16)
        ))
        .expect("a store that refuses every sweep");

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _listening = tracing::subscriber::set_default(subscriber);
    let _serving = serving_over(Store::open(&store).expect("a store"), Limits::default());

    let reported = timeout(A_SWEEP_LANDS_WITHIN, async {
        loop {
            let said = captured.said();

            if ["pass=Unused", "pass=Staged", "pass=Idle"]
                .iter()
                .all(|pass| said.contains(pass))
            {
                return said;
            }
            tokio::time::sleep(LOOKED_AGAIN_AFTER).await;
        }
    })
    .await;
    let spaces = spaces_in(&store);

    fs::remove_dir_all(&folder).ok();

    let said = reported.unwrap_or_else(|_| captured.said());

    assert!(
        ["pass=Unused", "pass=Staged", "pass=Idle"]
            .iter()
            .all(|pass| said.contains(pass)),
        "{said}"
    );
    assert!(said.contains("a sweep failed"), "{said}");
    assert!(said.contains("WARN"), "{said}");
    assert_eq!(spaces, 1);
}

#[tokio::test]
async fn a_store_write_that_fails_ends_the_session_and_gives_back_the_bytes_its_reader_held() {
    let folder = scratch("failed-write");
    let store = folder.join("records.sqlite");
    let limits = Limits {
        frame_bytes: usize::try_from(MEBIBYTE).expect("a size"),
        ..Limits::default()
    };

    let serving = serving_over(Store::open(&store).expect("a store"), limits);

    Connection::open(&store)
        .expect("the store")
        .execute_batch(
            "CREATE TRIGGER no_record_is_kept BEFORE INSERT ON records BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .expect("a store that refuses every record");

    let mut failing = welcomed(&serving, 1).await;
    let held_by_the_failing_reader = MEBIBYTE * 3 / 5;
    let append = ToServer::Append {
        blobs: vec![vec![1; 64]],
    };

    send_frame(&mut failing.send, &append.encoded())
        .await
        .expect("said");
    announce(&mut failing.send, held_by_the_failing_reader).await;

    let ended = timeout(A_SESSION_ENDS_WITHIN, async {
        while let Ok(Some(_)) = read_frame(&mut failing.recv).await {}
    })
    .await;
    let mut next = welcomed(&serving, 2).await;
    let past_a_record = ToServer::Close {
        sealed: vec![0; usize::try_from(held_by_the_failing_reader).expect("a size")],
    };

    send_frame(&mut next.send, &past_a_record.encoded())
        .await
        .expect("said");

    let after = timeout(A_SESSION_ENDS_WITHIN, refused_with(&mut next.recv))
        .await
        .ok()
        .flatten();

    fs::remove_dir_all(&folder).ok();

    assert!(ended.is_ok(), "the failing session went on");
    assert_eq!(
        after,
        Some(Refusal::TooLarge),
        "the next frame found its bytes held by the reader of a session that had failed"
    );
}
