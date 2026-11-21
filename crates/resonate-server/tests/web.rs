use std::{
    env, fs, io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use resonate_server::{Allowance, Limits, Server, Store};
use resonate_sync::{
    DeviceId, Follow, Incoming, Me, Outgoing, Platform, Settings, Standing, Sync, SyncKey, connect,
    sealed, to_the_web, web_address,
    wire::{FromServer, PROTOCOL_VERSION, Record, Refusal, ToServer, encoded_bundle},
};
use rustls::RootCertStore;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::{scripted::Scripted, shelf::Shelf};

const SETTLES_WITHIN: Duration = Duration::from_secs(30);
const REFUSED_WITHIN: Duration = Duration::from_secs(5);
const KIBIBYTE: usize = 1024;
const BINARY_AND_FINAL: u8 = 0x82;
const MASKED_WITH_A_LENGTH_OF_EIGHT_BYTES: u8 = 0x80 | 127;
const UNMASKING_KEY: [u8; 4] = [0; 4];
const OPCODE: u8 = 0x0f;
const CLOSING: u8 = 0x08;
const UNMASKED_LENGTH: u8 = 0x7f;
const MEDIUM_LENGTH: u8 = 126;
const LONG_LENGTH: u8 = 127;

struct Hosted {
    address: SocketAddr,
    _runtime: tokio::runtime::Runtime,
}

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!(
        "resonate-web-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));

    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn hosted(limits: Limits, forwarded_by: Option<&str>) -> Hosted {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let listener = runtime
        .block_on(TcpListener::bind(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            0,
        ))))
        .expect("listening");
    let address = listener.local_addr().expect("an address");
    let server = Server::new(Store::in_memory().expect("a store"), limits);

    runtime.spawn(server.serve_web(listener, forwarded_by.map(str::to_owned)));
    Hosted {
        address,
        _runtime: runtime,
    }
}

fn device(hosted: &Hosted, folder: &Path, key: &SyncKey, name: &str, shelf: Arc<Shelf>) -> Sync {
    device_reaching(hosted.address, folder, key, name, shelf)
}

fn device_reaching(
    address: SocketAddr,
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
) -> Sync {
    let settings = Settings {
        server: resonate_sync::Server::At(format!("ws://{address}/")),
        trusted: None,
        stun: None,
        key: key.clone(),
        me: Me {
            name: name.into(),
            platform: Platform::Linux,
        },
        follow: Follow::default(),
        ledger: folder.join(format!("{name}.sqlite")),
        staging: folder.join(format!("{name}-staging")),
        transfers_allowed: true,
        relay_only: false,
        nearby: None,
    };
    Sync::start(settings, shelf).expect("started")
}

fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + SETTLES_WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");

        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn two_devices_share_a_playlist_through_the_server_over_a_web_socket() {
    let folder = scratch("pair");
    let hosted = hosted(Limits::default(), None);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));

    desk_shelf.make_playlist("Summer", &["A", "B"]);
    desk_shelf.favour_here("A", true);

    let desk = device(&hosted, &folder, &key, "Desk", Arc::clone(&desk_shelf));
    let phone = device(&hosted, &folder, &key, "Phone", Arc::clone(&phone_shelf));

    eventually("the playlist arriving", || {
        phone_shelf.playlist_named("Summer").is_some()
    });
    eventually("both devices seeing each other online", || {
        phone
            .shown()
            .devices
            .iter()
            .any(|seen| seen.name == "Desk" && seen.online)
    });

    assert_eq!(phone.shown().standing, Standing::Connected);

    eventually("the favourite arriving", || {
        phone_shelf.favourite_titles() == ["A"]
    });

    let (on_phone, _) = phone_shelf.playlist_named("Summer").expect("held");

    phone_shelf.rename_playlist(on_phone, "Summer days");
    eventually("the rename reaching the desk", || {
        desk_shelf.playlist_named("Summer days").is_some()
    });

    drop(phone);
    drop(desk);

    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_listener_behind_a_proxy_turns_away_a_connection_the_proxy_did_not_forward() {
    let folder = scratch("unforwarded");
    let hosted = hosted(Limits::default(), Some("CF-Connecting-IP"));
    let key = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let desk = device(&hosted, &folder, &key, "Desk", shelf);

    eventually("the connection being turned away", || {
        desk.shown().standing == Standing::Unreachable
    });

    drop(desk);

    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn an_address_that_makes_more_spaces_than_it_is_allowed_in_a_day_is_turned_away() {
    let folder = scratch("spaces");
    let limits = Limits {
        allowance: Allowance {
            spaces_per_address_per_day: 1,
            ..Allowance::default()
        },
        ..Limits::default()
    };
    let hosted = hosted(limits, None);
    let first = SyncKey::minted().expect("a key");
    let second = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let allowed = device(&hosted, &folder, &first, "First", Arc::clone(&shelf));

    eventually("the first space being made", || {
        allowed.shown().standing == Standing::Connected
    });

    let refused = device(&hosted, &folder, &second, "Second", shelf);

    eventually("the second space being refused", || {
        refused.shown().standing == Standing::Refused(Refusal::TooFast)
    });

    drop(refused);
    drop(allowed);

    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_known_space_is_not_counted_against_the_spaces_an_address_may_make() {
    let folder = scratch("known");
    let limits = Limits {
        allowance: Allowance {
            spaces_per_address_per_day: 1,
            ..Allowance::default()
        },
        ..Limits::default()
    };
    let hosted = hosted(limits, None);
    let key = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let desk = device(&hosted, &folder, &key, "Desk", Arc::clone(&shelf));
    let phone = device(&hosted, &folder, &key, "Phone", shelf);

    eventually("both devices connecting", || {
        desk.shown().standing == Standing::Connected
            && phone.shown().standing == Standing::Connected
    });

    drop(phone);
    drop(desk);

    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn saved_queries_compacted_away_by_a_build_without_them_are_told_to_the_server_again() {
    let folder = scratch("compacted-away");
    let hosted = hosted(Limits::default(), None);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));

    desk_shelf.make_query("Recent", "added:<30d");

    let desk = device(&hosted, &folder, &key, "Desk", desk_shelf);

    eventually("the server acknowledging the desk's initial sync", || {
        let shown = desk.shown();
        shown.standing == Standing::Connected && shown.settled && shown.owed == 0
    });

    let derived = key.derived().expect("derived");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");

    let compact = async {
        let address = web_address(&format!("ws://{}/", hosted.address)).expect("an address");
        let tls = to_the_web(RootCertStore::empty()).expect("a configuration");

        let (mut outgoing, mut incoming) = connect(&address, &tls).await.expect("connected");

        outgoing
            .send(
                &ToServer::Hello {
                    version: PROTOCOL_VERSION,
                    space: derived.space,
                    token: derived.token.clone(),
                    device: DeviceId::minted().expect("a device"),
                    after: 0,
                }
                .encoded(),
            )
            .await
            .expect("said hello");

        let heard = incoming.read().await.expect("read").expect("a frame");
        let head = match FromServer::decoded(&heard).expect("a frame") {
            FromServer::Welcome { head, .. } => head,
            frame => panic!("expected a welcome, got {frame:?}"),
        };

        assert!(head > 0, "the server has no records to compact");

        let without_queries =
            sealed(&derived.sealer, derived.space, &encoded_bundle(&[])).expect("sealed");

        outgoing
            .send(
                &ToServer::Compact {
                    through: head,
                    parts: vec![without_queries],
                }
                .encoded(),
            )
            .await
            .expect("compacted");
        loop {
            let heard = incoming.read().await.expect("read").expect("a frame");
            match FromServer::decoded(&heard).expect("a frame") {
                FromServer::Compacted { through } => {
                    assert_eq!(through, head);

                    break;
                }
                FromServer::Refused(refusal) => panic!("compaction was refused: {refusal:?}"),
                _ => {}
            }
        }
    };

    runtime.block_on(async {
        timeout(SETTLES_WITHIN, compact)
            .await
            .expect("the server acknowledged the compaction in time");
    });

    let phone = device(&hosted, &folder, &key, "Phone", Arc::clone(&phone_shelf));

    eventually(
        "the query reaching a device that joined from the server alone",
        || phone_shelf.query_named("Recent").is_some(),
    );

    drop(phone);
    drop(desk);

    let _ = fs::remove_dir_all(&folder);
}

fn hello(key: &SyncKey) -> Vec<u8> {
    let derived = key.derived().expect("derived");
    ToServer::Hello {
        version: PROTOCOL_VERSION,
        space: derived.space,
        token: derived.token,
        device: DeviceId::minted().expect("a device"),
        after: 0,
    }
    .encoded()
}

async fn welcomed_over_the_web(hosted: &Hosted, key: &SyncKey) -> (Outgoing, Incoming) {
    let address = web_address(&format!("ws://{}/", hosted.address)).expect("an address");
    let tls = to_the_web(RootCertStore::empty()).expect("a configuration");

    let (mut outgoing, mut incoming) = connect(&address, &tls).await.expect("connected");

    outgoing.send(&hello(key)).await.expect("said hello");
    loop {
        let heard = incoming.read().await.expect("read").expect("a frame");
        if matches!(
            FromServer::decoded(&heard).expect("a frame"),
            FromServer::Welcome { .. }
        ) {
            return (outgoing, incoming);
        }
    }
}

async fn refused_over_the_web(incoming: &mut Incoming) -> Option<Refusal> {
    while let Ok(Some(heard)) = incoming.read().await {
        if let Ok(FromServer::Refused(refusal)) = FromServer::decoded(&heard) {
            return Some(refusal);
        }
    }
    None
}

fn framed_by_hand(payload: &[u8]) -> Vec<u8> {
    let mut framed = announced_by_hand(payload.len());

    framed.extend_from_slice(payload);
    framed
}

fn announced_by_hand(length: usize) -> Vec<u8> {
    let mut header = vec![BINARY_AND_FINAL, MASKED_WITH_A_LENGTH_OF_EIGHT_BYTES];

    header.extend_from_slice(&u64::try_from(length).expect("a length").to_be_bytes());
    header.extend_from_slice(&UNMASKING_KEY);
    header
}

async fn upgraded_by_hand(hosted: &Hosted) -> TcpStream {
    let mut stream = TcpStream::connect(hosted.address).await.expect("connected");

    stream
        .write_all(
            format!(
                "GET / HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                 Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
                hosted.address
            )
            .as_bytes(),
        )
        .await
        .expect("asked to upgrade");

    let mut answer = Vec::new();
    let mut byte = [0; 1];

    while !answer.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.expect("an answer");
        answer.push(byte[0]);
    }
    stream
}

struct Holding {
    stream: TcpStream,
    heard: Vec<u8>,
}

impl Holding {
    async fn part_of_a_message(hosted: &Hosted, key: &SyncKey) -> Self {
        let mut stream = upgraded_by_hand(hosted).await;

        stream
            .write_all(&framed_by_hand(&hello(key)))
            .await
            .expect("said hello");
        stream
            .write_all(&announced_by_hand(1024 * KIBIBYTE))
            .await
            .expect("announced");
        stream
            .write_all(&vec![7; 240 * KIBIBYTE])
            .await
            .expect("sent part");

        Self {
            stream,
            heard: Vec::new(),
        }
    }

    fn turned_away(&mut self) -> bool {
        let mut chunk = [0; 4 * KIBIBYTE];

        loop {
            match self.stream.try_read(&mut chunk) {
                Ok(0) => return true,
                Ok(read) => self.heard.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return true,
            }
        }

        framed_by_the_server(&self.heard)
            .into_iter()
            .any(|(opcode, payload)| {
                opcode == CLOSING
                    || matches!(FromServer::decoded(payload), Ok(FromServer::Refused(_)))
            })
    }
}

fn framed_by_the_server(mut heard: &[u8]) -> Vec<(u8, &[u8])> {
    let mut frames = Vec::new();

    while let [first, second, rest @ ..] = heard {
        let (length, rest) = match second & UNMASKED_LENGTH {
            MEDIUM_LENGTH => match rest.split_first_chunk::<2>() {
                Some((length, rest)) => (usize::from(u16::from_be_bytes(*length)), rest),
                None => break,
            },
            LONG_LENGTH => match rest.split_first_chunk::<8>() {
                Some((length, rest)) => (
                    usize::try_from(u64::from_be_bytes(*length)).expect("a length"),
                    rest,
                ),
                None => break,
            },
            short => (usize::from(short), rest),
        };

        let Some((payload, after)) = rest.split_at_checked(length) else {
            break;
        };

        frames.push((first & OPCODE, payload));
        heard = after;
    }

    frames
}

#[test]
fn a_message_still_arriving_on_one_web_socket_holds_its_bytes_from_every_other_session() {
    let hosted = hosted(
        Limits {
            frame_bytes: 256 * KIBIBYTE,
            ..Limits::default()
        },
        None,
    );
    let holder = SyncKey::minted().expect("a key");
    let asker = SyncKey::minted().expect("a key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");

    let refused = runtime.block_on(async {
        let mut holding = Holding::part_of_a_message(&hosted, &holder).await;

        let deadline = Instant::now() + SETTLES_WITHIN;
        loop {
            let (mut outgoing, mut incoming) = welcomed_over_the_web(&hosted, &asker).await;

            outgoing
                .send(&vec![0; 64 * KIBIBYTE])
                .await
                .expect("sent a frame");

            let refused = timeout(REFUSED_WITHIN, refused_over_the_web(&mut incoming))
                .await
                .ok()
                .flatten();

            if refused.is_some() || Instant::now() >= deadline {
                drop(holding);
                break refused;
            }
            if holding.turned_away() {
                holding = Holding::part_of_a_message(&hosted, &holder).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });

    assert_eq!(refused, Some(Refusal::Busy));
}

#[test]
fn a_ping_counts_toward_the_frames_a_web_socket_may_send_in_a_minute() {
    let hosted = hosted(
        Limits {
            frames_per_minute: 5,
            ..Limits::default()
        },
        None,
    );
    let key = SyncKey::minted().expect("a key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");

    let refused = runtime.block_on(async {
        let (mut outgoing, mut incoming) = welcomed_over_the_web(&hosted, &key).await;

        for _ in 0..10 {
            outgoing.keep_alive().await.expect("pinged");
        }
        timeout(REFUSED_WITHIN, refused_over_the_web(&mut incoming))
            .await
            .ok()
            .flatten()
    });

    assert_eq!(refused, Some(Refusal::TooFast));
}

const RECORDS_PAST_A_COMPACTION: u64 = 600;
const A_SESSION_KEPT_FOR: Duration = Duration::from_secs(2);

fn a_full_space(heard: &ToServer) -> Vec<FromServer> {
    match heard {
        ToServer::Hello { .. } => vec![
            FromServer::Welcome {
                online: Vec::new(),
                seen_at: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
                head: RECORDS_PAST_A_COMPACTION,
            },
            FromServer::Records {
                entries: (1..=RECORDS_PAST_A_COMPACTION)
                    .map(|seq| Record { seq, blob: None })
                    .collect(),
            },
        ],
        ToServer::Append { blobs } => vec![FromServer::Appended {
            count: u32::try_from(blobs.len()).expect("a count"),
        }],
        ToServer::Compact { .. } | ToServer::Stage => {
            vec![FromServer::Refused(Refusal::SpaceFull)]
        }
        _ => Vec::new(),
    }
}

#[test]
fn a_compaction_refused_for_a_full_space_backs_off_and_keeps_the_session() {
    let folder = scratch("full");
    let scripted = Scripted::answering(a_full_space);
    let key = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let desk = device_reaching(scripted.address, &folder, &key, "Desk", shelf);

    eventually("a compaction asked", || {
        scripted
            .heard()
            .iter()
            .any(|heard| matches!(heard, ToServer::Compact { .. }))
    });
    thread::sleep(A_SESSION_KEPT_FOR);

    let heard = scripted.heard();
    let hellos = heard
        .iter()
        .filter(|heard| matches!(heard, ToServer::Hello { .. }))
        .count();
    let compactions = heard
        .iter()
        .filter(|heard| matches!(heard, ToServer::Compact { .. } | ToServer::Stage))
        .count();

    assert_eq!(desk.shown().standing, Standing::Connected);
    assert_eq!(hellos, 1);
    assert_eq!(compactions, 1);

    drop(desk);

    let _ = fs::remove_dir_all(&folder);
}
