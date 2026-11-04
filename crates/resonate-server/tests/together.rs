use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    process,
    time::{Duration, Instant},
};

use resonate_server::{Limits, Server, Store};
use resonate_sync::{
    DeviceId, Incoming, Outgoing, SpaceId, Token, connect, to_the_web, web_address,
    wire::{FromServer, PROTOCOL_VERSION, Refusal, SpaceAsked, ToServer},
};
use rustls::RootCertStore;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const ANSWERS_WITHIN: Duration = Duration::from_secs(10);
const SEALED: [u8; 32] = [7; 32];

const ROOM: SpaceId = SpaceId([1; 16]);
const SPACE: SpaceId = SpaceId([2; 16]);

fn token(of: u8) -> Token {
    Token([of; 32])
}

fn device(of: u8) -> DeviceId {
    DeviceId([of; 16])
}

async fn hosted(limits: Limits) -> SocketAddr {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("listening");
    let address = listener.local_addr().expect("an address");
    let server = Server::new(Store::in_memory().expect("a store"), limits);
    tokio::spawn(server.serve_web(listener, None));

    address
}

struct Link {
    outgoing: Outgoing,
    incoming: Incoming,
}

impl Link {
    async fn to(server: SocketAddr) -> Self {
        let address = web_address(&format!("ws://{server}/")).expect("an address");
        let tls = to_the_web(RootCertStore::empty()).expect("a configuration");
        let (outgoing, incoming) = connect(&address, &tls).await.expect("connected");

        Self { outgoing, incoming }
    }

    async fn say(&mut self, frame: ToServer) {
        self.outgoing
            .send(&frame.encoded())
            .await
            .expect("the frame went");
    }

    async fn heard(&mut self) -> Option<FromServer> {
        let heard = timeout(ANSWERS_WITHIN, self.incoming.read())
            .await
            .expect("the server answered in time");

        match heard {
            Ok(Some(bytes)) => Some(FromServer::decoded(&bytes).expect("a frame")),
            Ok(None) | Err(_) => None,
        }
    }

    async fn heard_past_presence(&mut self) -> Option<FromServer> {
        loop {
            match self.heard().await? {
                FromServer::RoomArrived { .. } | FromServer::RoomLeft { .. } => {}
                frame => return Some(frame),
            }
        }
    }
}

async fn in_the_room(server: SocketAddr, member: u8, host: Option<Token>) -> Link {
    let mut link = Link::to(server).await;
    link.say(ToServer::Room {
        version: PROTOCOL_VERSION,
        room: ROOM,
        token: token(5),
        host,
        member: device(member),
    })
    .await;

    link
}

async fn share_opened(server: SocketAddr) -> Link {
    let mut link = Link::to(server).await;
    link.say(ToServer::Share {
        version: PROTOCOL_VERSION,
        space: SPACE,
        token: token(5),
        viewer: token(6).digest(),
        device: device(1),
        after: 0,
    })
    .await;
    assert!(matches!(
        link.heard().await,
        Some(FromServer::Welcome { .. })
    ));

    link
}

#[tokio::test]
async fn a_host_is_heard_by_every_guest_a_guest_by_the_host_alone_and_the_clock_answers() {
    let server = hosted(Limits::default()).await;

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(matches!(
        host.heard().await,
        Some(FromServer::RoomWelcome { members, host }) if members.is_empty() && host == device(1)
    ));
    let mut one = in_the_room(server, 2, None).await;
    assert!(matches!(
        one.heard().await,
        Some(FromServer::RoomWelcome { host, .. }) if host == device(1)
    ));
    let mut other = in_the_room(server, 3, None).await;
    assert!(matches!(
        other.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));

    host.say(ToServer::RoomSay {
        to: None,
        sealed: vec![1, 2, 3],
    })
    .await;
    let told = FromServer::RoomHeard {
        from: device(1),
        sealed: vec![1, 2, 3],
    };
    assert_eq!(one.heard_past_presence().await, Some(told.clone()));
    assert_eq!(other.heard_past_presence().await, Some(told));

    one.say(ToServer::RoomSay {
        to: Some(device(3)),
        sealed: vec![4],
    })
    .await;
    assert_eq!(
        host.heard_past_presence().await,
        Some(FromServer::RoomHeard {
            from: device(2),
            sealed: vec![4]
        })
    );

    other.say(ToServer::RoomPing { nonce: 11 }).await;
    assert!(
        matches!(
            other.heard_past_presence().await,
            Some(FromServer::RoomPong { nonce: 11, .. })
        ),
        "a guest's word to a guest reached it, or the clock did not answer"
    );
}

#[tokio::test]
async fn a_guest_finds_no_room_before_its_host_and_a_wrong_token_is_turned_away() {
    let server = hosted(Limits::default()).await;

    let mut early = in_the_room(server, 2, None).await;
    assert_eq!(
        early.heard().await,
        Some(FromServer::Refused(Refusal::NoSuchRoom))
    );

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(matches!(
        host.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));

    let mut forged = in_the_room(server, 3, Some(token(8))).await;
    assert_eq!(
        forged.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );
}

#[tokio::test]
async fn a_room_ends_once_its_host_stays_gone_and_lasts_if_the_host_comes_back() {
    let server = hosted(Limits {
        host_back_within: Duration::from_millis(300),
        ..Limits::default()
    })
    .await;

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(host.heard().await.is_some());
    let mut guest = in_the_room(server, 2, None).await;
    assert!(guest.heard().await.is_some());

    host.say(ToServer::Bye).await;
    drop(host);
    let mut back = in_the_room(server, 1, Some(token(9))).await;
    assert!(back.heard().await.is_some());
    tokio::time::sleep(Duration::from_millis(500)).await;

    back.say(ToServer::RoomSay {
        to: None,
        sealed: vec![1],
    })
    .await;
    assert!(matches!(
        guest.heard_past_presence().await,
        Some(FromServer::RoomHeard { .. })
    ));

    back.say(ToServer::Bye).await;
    drop(back);
    assert_eq!(
        guest.heard_past_presence().await,
        Some(FromServer::RoomEnded)
    );
}

#[tokio::test]
async fn the_host_turns_a_guest_out_and_a_host_may_end_the_room_for_everyone() {
    let server = hosted(Limits::default()).await;

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(host.heard().await.is_some());
    let mut turned_out = in_the_room(server, 2, None).await;
    assert!(turned_out.heard().await.is_some());
    let mut stays = in_the_room(server, 3, None).await;
    assert!(stays.heard().await.is_some());

    host.say(ToServer::RoomKick { member: device(2) }).await;
    assert_eq!(
        turned_out.heard_past_presence().await,
        Some(FromServer::RoomEnded)
    );

    host.say(ToServer::Close {
        sealed: vec![0; 28],
    })
    .await;
    assert_eq!(
        stays.heard_past_presence().await,
        Some(FromServer::RoomEnded)
    );
}

#[tokio::test]
async fn a_viewer_reads_a_shared_space_and_can_write_nothing_to_it() {
    let server = hosted(Limits::default()).await;

    let mut editor = share_opened(server).await;
    editor
        .say(ToServer::Append {
            blobs: vec![SEALED.to_vec()],
        })
        .await;
    assert!(matches!(
        editor.heard().await,
        Some(FromServer::Records { .. } | FromServer::Appended { .. })
    ));

    let mut viewer = Link::to(server).await;
    viewer
        .say(ToServer::View {
            version: PROTOCOL_VERSION,
            space: SPACE,
            token: token(6),
            device: device(40),
            after: 0,
        })
        .await;
    assert!(matches!(
        viewer.heard().await,
        Some(FromServer::Welcome { head: 1, online, .. }) if online.is_empty()
    ));
    let Some(FromServer::Records { entries }) = viewer.heard().await else {
        panic!("a viewer was handed no records");
    };
    assert_eq!(entries[0].blob.as_deref(), Some(SEALED.as_slice()));
    assert_eq!(viewer.heard().await, None, "a view is one read, then over");

    let mut wrote_with_the_viewers_token = Link::to(server).await;
    wrote_with_the_viewers_token
        .say(ToServer::Hello {
            version: PROTOCOL_VERSION,
            space: SPACE,
            token: token(6),
            device: device(41),
            after: 0,
        })
        .await;
    assert_eq!(
        wrote_with_the_viewers_token.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );

    let mut stranger = Link::to(server).await;
    stranger
        .say(ToServer::View {
            version: PROTOCOL_VERSION,
            space: SPACE,
            token: token(7),
            device: device(42),
            after: 0,
        })
        .await;
    assert_eq!(
        stranger.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );
}

#[tokio::test]
async fn a_closed_space_keeps_the_first_word_and_takes_no_more_and_tells_everyone() {
    let server = hosted(Limits::default()).await;

    let mut editor = share_opened(server).await;
    editor
        .say(ToServer::Close {
            sealed: SEALED.to_vec(),
        })
        .await;
    assert_eq!(
        editor.heard().await,
        Some(FromServer::Closed {
            sealed: SEALED.to_vec()
        })
    );

    editor
        .say(ToServer::Close {
            sealed: vec![9; 32],
        })
        .await;
    assert_eq!(
        editor.heard().await,
        Some(FromServer::Closed {
            sealed: SEALED.to_vec()
        }),
        "a later close replaced the first"
    );

    editor
        .say(ToServer::Append {
            blobs: vec![vec![1; 32]],
        })
        .await;
    assert_eq!(
        editor.heard().await,
        Some(FromServer::Closed {
            sealed: SEALED.to_vec()
        })
    );
    editor
        .say(ToServer::Compact {
            through: 1,
            parts: vec![vec![1; 32]],
        })
        .await;
    assert_eq!(
        editor.heard().await,
        Some(FromServer::Closed {
            sealed: SEALED.to_vec()
        })
    );

    let mut viewer = Link::to(server).await;
    viewer
        .say(ToServer::View {
            version: PROTOCOL_VERSION,
            space: SPACE,
            token: token(6),
            device: device(40),
            after: 0,
        })
        .await;
    assert!(matches!(
        viewer.heard().await,
        Some(FromServer::Welcome { head: 0, .. })
    ));
    assert_eq!(
        viewer.heard().await,
        Some(FromServer::Closed {
            sealed: SEALED.to_vec()
        })
    );
}

#[tokio::test]
async fn heads_are_told_for_a_token_that_fits_and_kept_from_one_that_does_not() {
    let server = hosted(Limits::default()).await;

    let mut editor = share_opened(server).await;
    editor
        .say(ToServer::Append {
            blobs: vec![SEALED.to_vec()],
        })
        .await;
    assert!(editor.heard().await.is_some());

    let mut asking = Link::to(server).await;
    asking
        .say(ToServer::Heads {
            version: PROTOCOL_VERSION,
            asked: vec![
                SpaceAsked {
                    space: SPACE,
                    token: token(6),
                },
                SpaceAsked {
                    space: SpaceId([3; 16]),
                    token: token(6),
                },
            ],
        })
        .await;
    let Some(FromServer::Heads { heads }) = asking.heard().await else {
        panic!("no heads came back");
    };
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].space, SPACE);
    assert_eq!(heads[0].head, 1);
    assert!(!heads[0].closed);

    let mut stranger = Link::to(server).await;
    stranger
        .say(ToServer::Heads {
            version: PROTOCOL_VERSION,
            asked: vec![SpaceAsked {
                space: SPACE,
                token: token(99),
            }],
        })
        .await;
    assert_eq!(
        stranger.heard().await,
        Some(FromServer::Heads { heads: Vec::new() })
    );
}

#[tokio::test]
async fn a_page_is_answered_beside_the_socket_and_holds_nothing_of_the_invitation() {
    let server = hosted(Limits::default()).await;

    let mut stream = TcpStream::connect(server).await.expect("connected");
    stream
        .write_all(b"GET /p/ HTTP/1.1\r\nHost: resonate.example\r\n\r\n")
        .await
        .expect("asked");
    let mut answer = Vec::new();
    timeout(ANSWERS_WITHIN, stream.read_to_end(&mut answer))
        .await
        .expect("answered in time")
        .expect("read");
    let answer = String::from_utf8(answer).expect("text");

    assert!(answer.starts_with("HTTP/1.1 200 OK"));
    assert!(answer.contains("resonate://party/"));

    let mut still_a_socket = in_the_room(server, 1, Some(token(9))).await;
    assert!(matches!(
        still_a_socket.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));
}

#[test]
fn a_store_of_before_shared_spaces_is_carried_forward_with_its_records() {
    let folder: PathBuf = env::temp_dir().join(format!(
        "resonate-together-{}-{:?}",
        process::id(),
        Instant::now()
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    let path = folder.join("store.sqlite");
    let digest = token(5).digest();

    {
        let earlier = rusqlite::Connection::open(&path).expect("a store of before");
        earlier
            .execute_batch(
                "CREATE TABLE spaces (
                    id BLOB PRIMARY KEY, token BLOB NOT NULL, created INTEGER NOT NULL,
                    touched INTEGER NOT NULL, bytes INTEGER NOT NULL DEFAULT 0,
                    head INTEGER NOT NULL DEFAULT 0, snapshot_through INTEGER NOT NULL DEFAULT 0
                 ) STRICT;
                 CREATE TABLE records (
                    space BLOB NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
                    seq INTEGER NOT NULL, device BLOB NOT NULL, blob BLOB NOT NULL,
                    digest BLOB, PRIMARY KEY (space, seq)
                 ) STRICT, WITHOUT ROWID;
                 CREATE TABLE snapshots (
                    space BLOB NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
                    part INTEGER NOT NULL, blob BLOB NOT NULL, PRIMARY KEY (space, part)
                 ) STRICT, WITHOUT ROWID;
                 PRAGMA user_version = 2;",
            )
            .expect("laid out");
        earlier
            .execute(
                "INSERT INTO spaces (id, token, created, touched, head) VALUES (?1, ?2, 1, 1, 1)",
                rusqlite::params![SPACE.0.as_slice(), digest.as_slice()],
            )
            .expect("a space");
    }

    let mut store = Store::open(&path).expect("carried forward");
    let heads = store
        .heads(
            &[SpaceAsked {
                space: SPACE,
                token: token(5),
            }],
            2,
        )
        .expect("read");

    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].head, 1);
    assert!(!heads[0].closed);
    assert_eq!(store.closed(SPACE).expect("read"), None);

    fs::remove_dir_all(&folder).ok();
}
