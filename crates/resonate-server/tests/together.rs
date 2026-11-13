use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use parking_lot::Mutex;
use resonate_core::Recording;
use resonate_server::{Limits, Server, Store};
use resonate_sync::{
    DeviceId, Incoming, Outgoing, SpaceId, Token, connect,
    party::{
        Going, HANDED_OVER_WITHIN, LINES_A_PERSON_SAYS_AT_MOST, Mine, Party, PartyChat,
        PartyFinder, PartyHanding, PartyNow, PartyReach, PartyReaction, PartySeat, PartySettings,
        PartySong, PartyWantingASong, SeatHeard, Standing,
    },
    to_the_web, web_address,
    wire::{FromServer, PROTOCOL_VERSION, Refusal, SpaceAsked, ToServer},
};
use rustls::RootCertStore;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
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
    seated_in_the_room(server, member, host, None).await
}

async fn seated_in_the_room(
    server: SocketAddr,
    member: u8,
    host: Option<Token>,
    seat: Option<Token>,
) -> Link {
    let mut link = Link::to(server).await;
    link.say(ToServer::Room {
        version: PROTOCOL_VERSION,
        room: ROOM,
        token: token(5),
        host,
        member: device(member),
        seat,
    })
    .await;

    link
}

async fn share_opened_as(server: SocketAddr, who: u8) -> Link {
    let mut link = Link::to(server).await;
    link.say(ToServer::Share {
        version: PROTOCOL_VERSION,
        space: SPACE,
        token: token(5),
        viewer: token(6).digest(),
        device: device(who),
        after: 0,
    })
    .await;
    assert!(matches!(
        link.heard().await,
        Some(FromServer::Welcome { .. })
    ));

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
async fn a_guest_cannot_take_another_guests_place_by_joining_under_its_id() {
    let server = hosted(Limits::default()).await;

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(host.heard().await.is_some());
    let mut victim = seated_in_the_room(server, 2, None, Some(token(20))).await;
    assert!(victim.heard().await.is_some());

    let mut thief = seated_in_the_room(server, 2, None, Some(token(21))).await;
    let mut unseated = in_the_room(server, 2, None).await;
    assert_eq!(
        thief.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );
    assert_eq!(
        unseated.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );

    victim.say(ToServer::RoomPing { nonce: 3 }).await;
    assert!(matches!(
        victim.heard_past_presence().await,
        Some(FromServer::RoomPong { nonce: 3, .. })
    ));
    victim
        .say(ToServer::RoomSay {
            to: None,
            sealed: vec![5],
        })
        .await;
    assert_eq!(
        host.heard_past_presence().await,
        Some(FromServer::RoomHeard {
            from: device(2),
            sealed: vec![5]
        })
    );

    let mut back = seated_in_the_room(server, 2, None, Some(token(20))).await;
    assert!(matches!(
        back.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));
}

#[tokio::test]
async fn a_guest_without_a_secret_of_its_own_still_joins_as_before() {
    let server = hosted(Limits::default()).await;

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(host.heard().await.is_some());
    let mut old = in_the_room(server, 2, None).await;

    assert!(matches!(
        old.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));
}

#[tokio::test]
async fn a_guest_without_a_secret_keeps_its_place_while_its_session_lives_and_gets_it_back_after() {
    let server = hosted(Limits::default()).await;

    let mut host = in_the_room(server, 1, Some(token(9))).await;
    assert!(host.heard().await.is_some());
    let mut old = in_the_room(server, 2, None).await;
    assert!(matches!(
        old.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));

    let mut thief = in_the_room(server, 2, None).await;
    let mut thief_with_a_secret = seated_in_the_room(server, 2, None, Some(token(21))).await;
    assert_eq!(
        thief.heard().await,
        Some(FromServer::Refused(Refusal::Busy))
    );
    assert_eq!(
        thief_with_a_secret.heard().await,
        Some(FromServer::Refused(Refusal::Busy))
    );

    old.say(ToServer::RoomPing { nonce: 3 }).await;
    assert!(matches!(
        old.heard_past_presence().await,
        Some(FromServer::RoomPong { nonce: 3, .. })
    ));

    old.say(ToServer::Bye).await;
    drop(old);
    loop {
        match host.heard().await {
            Some(FromServer::RoomLeft { member }) if member == device(2) => break,
            Some(_) => {}
            None => panic!("the host stopped hearing before the guest left"),
        }
    }

    let mut back = in_the_room(server, 2, None).await;
    assert!(matches!(
        back.heard().await,
        Some(FromServer::RoomWelcome { .. })
    ));
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
async fn a_space_made_by_a_share_is_never_answered_to_a_hello_so_the_two_budgets_stay_apart() {
    let server = hosted(Limits::default()).await;

    let _editor = share_opened(server).await;
    let mut as_a_personal_space = Link::to(server).await;
    as_a_personal_space
        .say(ToServer::Hello {
            version: PROTOCOL_VERSION,
            space: SPACE,
            token: token(5),
            device: device(43),
            after: 0,
        })
        .await;
    let mut shared_again = share_opened_as(server, 44).await;
    shared_again
        .say(ToServer::Append {
            blobs: vec![SEALED.to_vec()],
        })
        .await;

    assert_eq!(
        as_a_personal_space.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );
    assert!(matches!(
        shared_again.heard().await,
        Some(FromServer::Records { .. } | FromServer::Appended { .. })
    ));
}

#[tokio::test]
async fn a_view_link_cannot_claim_a_shared_list_before_its_owner_does() {
    let server = hosted(Limits::default()).await;
    let edit = resonate_sync::shared::Access::minted().expect("a key");
    let owner = edit.keys().expect("derived");
    let viewing = edit
        .viewing()
        .and_then(|view| view.keys())
        .expect("derived");
    let claimed_with = |token: Token, who: u8| ToServer::Share {
        version: PROTOCOL_VERSION,
        space: viewing.space,
        token,
        viewer: viewing.read_token.digest(),
        device: device(who),
        after: 0,
    };

    let mut with_the_read_token = Link::to(server).await;
    with_the_read_token
        .say(claimed_with(viewing.read_token.clone(), 40))
        .await;
    let mut with_a_token_of_its_own = Link::to(server).await;
    with_a_token_of_its_own
        .say(claimed_with(token(77), 41))
        .await;
    let mut the_owner = Link::to(server).await;
    the_owner
        .say(claimed_with(
            owner.write_token.expect("an editor writes"),
            1,
        ))
        .await;

    assert_eq!(
        with_the_read_token.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );
    assert_eq!(
        with_a_token_of_its_own.heard().await,
        Some(FromServer::Refused(Refusal::Token))
    );
    assert!(matches!(
        the_owner.heard().await,
        Some(FromServer::Welcome { .. })
    ));
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
async fn a_space_closed_again_tells_only_the_one_asking_not_every_device_once_more() {
    let server = hosted(Limits::default()).await;

    let mut first = share_opened_as(server, 1).await;
    let mut second = share_opened_as(server, 2).await;
    assert!(matches!(
        first.heard().await,
        Some(FromServer::Arrived { .. })
    ));

    first
        .say(ToServer::Close {
            sealed: SEALED.to_vec(),
        })
        .await;
    let closed = FromServer::Closed {
        sealed: SEALED.to_vec(),
    };
    assert_eq!(first.heard().await, Some(closed.clone()));
    assert_eq!(second.heard().await, Some(closed.clone()));

    second
        .say(ToServer::Close {
            sealed: vec![9; 32],
        })
        .await;
    assert_eq!(second.heard().await, Some(closed));

    let told_again = timeout(Duration::from_millis(500), first.incoming.read()).await;
    assert!(told_again.is_err(), "the other device was told again");
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
        "resonate-together-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
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

fn party_settings(server: SocketAddr, name: &str) -> PartySettings {
    let began = std::time::Instant::now();

    PartySettings {
        server: format!("ws://{server}"),
        trusted: None,
        name: name.to_owned(),
        now_nanos: std::sync::Arc::new(move || {
            u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX)
        }),
    }
}

async fn until(what: &str, reached: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !reached() {
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn a_song(row: u64) -> PartySong {
    PartySong {
        row,
        recording: Recording {
            mbid: None,
            title: "Song".to_owned(),
            artist: "Artist".to_owned(),
            album: "Album".to_owned(),
            length: Some(Duration::from_secs(200)),
        },
        isrc: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_tells_its_guests_the_song_and_their_names_and_ends_with_its_host_turning_one_out()
{
    let server = hosted(Limits::default()).await;
    let host = Party::host(party_settings(server, "Host")).expect("the host starts");

    until("the host to reach the server", || {
        host.shown().reach == PartyReach::Here
    })
    .await;

    let now = PartyNow {
        song: Some(a_song(1)),
        next: vec![a_song(2)],
        playing: true,
        position_nanos: 5,
        anchor_nanos: 7,
        coarse: false,
    };
    host.tell(now.clone());

    let code = host.shown().code;
    let guest = Party::join(party_settings(server, "Guest"), &code).expect("the guest starts");

    until("the guest to hear the song", || {
        guest.shown().now.as_ref() == Some(&now)
    })
    .await;
    until("the host to learn the guest's name", || {
        host.shown().people.iter().any(|one| one.name == "Guest")
    })
    .await;
    until("the guest to learn the host's name", || {
        guest
            .shown()
            .people
            .iter()
            .any(|one| one.host && one.name == "Host")
    })
    .await;
    until("the guest's clock to be set", || {
        guest.to_server(1).is_some()
    })
    .await;

    guest.ask_for(a_song(9));
    until("the host to be asked for the song", || {
        host.shown().asked_for.len() == 1
    })
    .await;

    let member = guest.shown().me;
    host.turn_out(member);
    until("the guest to be told the party is over", || {
        guest.shown().reach == PartyReach::Ended
    })
    .await;
}

fn words_heard(party: &Party) -> Vec<(String, PartyChat)> {
    party
        .shown()
        .chat
        .into_iter()
        .map(|line| (line.name, line.chat))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn what_anyone_says_reaches_everyone_through_the_host_and_a_late_guest_hears_what_came_before()
 {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let code = host.shown().code;
    let ada = Party::join(party_settings(server, "Ada"), &code).expect("Ada joins");
    let bo = Party::join(party_settings(server, "Bo"), &code).expect("Bo joins");
    until("everyone to be named to the host", || {
        let people = host.shown().people;
        people.iter().any(|one| one.name == "Ada") && people.iter().any(|one| one.name == "Bo")
    })
    .await;

    ada.say(PartyChat::Words("Turn it up".to_owned()));
    until("the host to hear Ada", || host.shown().chat.len() == 1).await;
    host.say(PartyChat::Reaction(PartyReaction::Fire));

    let heard = vec![
        ("Ada".to_owned(), PartyChat::Words("Turn it up".to_owned())),
        ("Host".to_owned(), PartyChat::Reaction(PartyReaction::Fire)),
    ];
    for (who, party) in [("the host", &host), ("Ada", &ada), ("Bo", &bo)] {
        until(&format!("{who} to hear both lines"), || {
            words_heard(party) == heard
        })
        .await;
    }

    let late = Party::join(party_settings(server, "Cy"), &code).expect("Cy joins");
    until("the late guest to hear what came before", || {
        words_heard(&late) == heard
    })
    .await;
    let ids: Vec<u64> = host.shown().chat.iter().map(|line| line.id).collect();
    assert_eq!(
        late.shown()
            .chat
            .iter()
            .map(|line| line.id)
            .collect::<Vec<_>>(),
        ids
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_saying_more_than_its_share_is_heard_only_as_far_as_its_share() {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let guest =
        Party::join(party_settings(server, "Guest"), &host.shown().code).expect("the guest joins");
    until("the guest to arrive", || {
        guest.shown().reach == PartyReach::Here && host.shown().people.len() == 2
    })
    .await;

    for said in 0..LINES_A_PERSON_SAYS_AT_MOST + 5 {
        guest.say(PartyChat::Words(said.to_string()));
    }
    until("the guest's share to reach the host", || {
        host.shown().chat.len() == LINES_A_PERSON_SAYS_AT_MOST
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(host.shown().chat.len(), LINES_A_PERSON_SAYS_AT_MOST);
    until("the guest to hear its share back", || {
        guest.shown().chat.len() == LINES_A_PERSON_SAYS_AT_MOST
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_is_at_a_party_nobody_began() {
    let server = hosted(Limits::default()).await;
    let key = resonate_sync::party::PartyKey::hosted_by(&resonate_sync::Token([7; 32]))
        .expect("a key is minted");
    let guest = Party::join(party_settings(server, ""), &key.code()).expect("the guest starts");

    until("the guest to be told there is no such party", || {
        guest.shown().reach == PartyReach::NoSuchParty
    })
    .await;
}

struct Doorman {
    address: SocketAddr,
    server: SocketAddr,
    closing: Arc<AtomicU32>,
    serving: Option<JoinHandle<()>>,
}

impl Doorman {
    async fn before(server: SocketAddr, clean_closes: u32) -> Self {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("listening");
        let address = listener.local_addr().expect("an address");
        let closing = Arc::new(AtomicU32::new(clean_closes));
        let serving = tokio::spawn(Self::serve(listener, server, Arc::clone(&closing)));

        Self {
            address,
            server,
            closing,
            serving: Some(serving),
        }
    }

    async fn serve(listener: TcpListener, server: SocketAddr, closing: Arc<AtomicU32>) {
        let mut links = JoinSet::new();
        while let Ok((client, _)) = listener.accept().await {
            let closes_cleanly = closing
                .try_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                    left.checked_sub(1)
                })
                .is_ok();
            if closes_cleanly {
                links.spawn(async move {
                    if let Ok(mut socket) = tokio_tungstenite::accept_async(client).await {
                        let _ = timeout(ANSWERS_WITHIN, socket.next()).await;
                        let _ = socket.close(None).await;
                        let _ = timeout(ANSWERS_WITHIN, async {
                            while socket.next().await.is_some() {}
                        })
                        .await;
                    }
                });
            } else {
                links.spawn(async move {
                    let mut client = client;
                    if let Ok(mut upstream) = TcpStream::connect(server).await {
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                    }
                });
            }
        }
    }

    async fn go_away(&mut self) {
        if let Some(serving) = self.serving.take() {
            serving.abort();
            let _ = serving.await;
        }
    }

    async fn come_back(&mut self) {
        let listener = TcpListener::bind(self.address)
            .await
            .expect("listening again");
        self.serving = Some(tokio::spawn(Self::serve(
            listener,
            self.server,
            Arc::clone(&self.closing),
        )));
    }
}

async fn a_party_in_the_room(server: SocketAddr) -> Party {
    let host = Party::host(party_settings(server, "Host")).expect("the host starts");
    until("the host to reach the server", || {
        host.shown().reach == PartyReach::Here
    })
    .await;

    host
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guests_request_made_while_it_reconnects_reaches_the_host_once_it_is_back() {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let mut door = Doorman::before(server, 0).await;
    let guest = Party::join(party_settings(door.address, "Guest"), &host.shown().code)
        .expect("the guest starts");
    until("the guest to reach the party", || {
        guest.shown().reach == PartyReach::Here
    })
    .await;

    door.go_away().await;
    until("the guest to find itself cut off", || {
        guest.shown().reach == PartyReach::Reconnecting
    })
    .await;
    guest.ask_for(a_song(9));
    door.come_back().await;

    until("the request to reach the host", || {
        host.shown().asked_for.len() == 1
    })
    .await;
    assert_eq!(host.shown().asked_for[0].song, a_song(9));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hosts_turn_out_made_while_it_reconnects_reaches_the_room_once_it_is_back() {
    let server = hosted(Limits::default()).await;
    let mut door = Doorman::before(server, 0).await;
    let host = Party::host(party_settings(door.address, "Host")).expect("the host starts");
    until("the host to reach the server", || {
        host.shown().reach == PartyReach::Here
    })
    .await;
    let guest =
        Party::join(party_settings(server, "Guest"), &host.shown().code).expect("the guest starts");
    until("the host to see the guest", || {
        host.shown().people.len() == 2
    })
    .await;

    door.go_away().await;
    until("the host to find itself cut off", || {
        host.shown().reach == PartyReach::Reconnecting
    })
    .await;
    host.turn_out(guest.shown().me);
    door.come_back().await;

    until("the guest to be turned out", || {
        guest.shown().reach == PartyReach::Ended
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_pressed_twice_while_its_search_runs_is_claimed_once_and_free_again_once_dropped()
{
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let guest =
        Party::join(party_settings(server, "Guest"), &host.shown().code).expect("the guest starts");
    until("the guest to reach the party", || {
        guest.shown().reach == PartyReach::Here
    })
    .await;
    guest.ask_for(a_song(8));
    until("the request to reach the host", || {
        host.shown().asked_for.len() == 1
    })
    .await;
    let request = host.shown().asked_for[0].clone();

    let first = host.claim_a_request(&request);
    let second = host.claim_a_request(&request);

    assert!(first.is_some());
    assert!(second.is_none());
    drop(first);
    let again = host.claim_a_request(&request);
    assert!(again.is_some());

    host.take_a_request(request);
    assert!(host.shown().asked_for.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_dismissed_is_the_one_named_whatever_else_has_come_and_gone() {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let guest =
        Party::join(party_settings(server, "Guest"), &host.shown().code).expect("the guest starts");
    until("the guest to reach the party", || {
        guest.shown().reach == PartyReach::Here
    })
    .await;

    guest.ask_for(a_song(8));
    guest.ask_for(a_song(9));
    until("both requests to reach the host", || {
        host.shown().asked_for.len() == 2
    })
    .await;
    let second: PartyWantingASong = host.shown().asked_for[1].clone();
    let first: PartyWantingASong = host.shown().asked_for[0].clone();

    host.take_a_request(first.clone());
    host.take_a_request(first);
    until("the first request to go", || {
        host.shown().asked_for.len() == 1
    })
    .await;

    assert_eq!(host.shown().asked_for, [second]);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_clean_close_before_the_welcome_is_a_connection_lost_and_not_a_server_too_old() {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let door = Doorman::before(server, 1).await;

    let guest = Party::join(party_settings(door.address, "Guest"), &host.shown().code)
        .expect("the guest starts");

    until("the guest to reach the party past the close", || {
        guest.shown().reach == PartyReach::Here
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_closes_every_connection_before_the_welcome_reads_as_too_old() {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let door = Doorman::before(server, u32::MAX).await;

    let guest = Party::join(party_settings(door.address, "Guest"), &host.shown().code)
        .expect("the guest starts");

    until("the guest to read the server as too old", || {
        guest.shown().reach == PartyReach::ServerTooOld
    })
    .await;
}

type TakenUp = (Vec<u64>, u64, bool);

#[derive(Clone, Default)]
struct Told {
    taken_up: Arc<Mutex<Vec<TakenUp>>>,
    kept_ahead: Arc<Mutex<Vec<Vec<u64>>>>,
    paused: Arc<AtomicBool>,
    put_back: Arc<AtomicBool>,
}

struct FakeSeat {
    now_nanos: Arc<dyn Fn() -> u64 + Send + Sync>,
    told: Told,
    hosting: bool,
    row: Arc<Mutex<Option<u64>>>,
    next: Arc<Mutex<Vec<u64>>>,
    began: u64,
}

impl PartySeat for FakeSeat {
    type Row = u64;

    fn heard(&self) -> SeatHeard {
        let at = (self.now_nanos)();
        let row = *self.row.lock();
        let paused = self.told.paused.load(Ordering::Acquire);
        let mine = match (row, paused) {
            (None, _) => Mine::Stopped,
            (Some(_), true) => Mine::Paused,
            (Some(_), false) => Mine::Playing,
        };

        SeatHeard {
            mine,
            row,
            position_nanos: row.map(|_| at.saturating_sub(self.began)),
            at_nanos: at,
            coarse: false,
        }
    }

    fn stamp_of(&self, row: &u64) -> u64 {
        row + 1_000
    }

    fn songs_up_next(&self) -> (Option<PartySong>, Vec<PartySong>) {
        if self.hosting {
            return (
                Some(a_song(1)),
                self.next.lock().iter().copied().map(a_song).collect(),
            );
        }

        let ahead = self
            .told
            .kept_ahead
            .lock()
            .last()
            .cloned()
            .unwrap_or_default();
        (
            self.row.lock().map(a_song),
            ahead.iter().map(|row| a_song(self.stamp_of(row))).collect(),
        )
    }

    fn added_here(&self, _known: &[u64]) -> Vec<PartySong> {
        Vec::new()
    }

    fn take_up(&self, rows: &[u64], position: u64, _at: u64, sounding: bool) -> bool {
        self.told
            .taken_up
            .lock()
            .push((rows.to_vec(), position, sounding));
        *self.row.lock() = rows.first().map(|row| row + 1_000);
        true
    }

    fn keep_ahead(&self, rows: &[u64]) -> bool {
        self.told.kept_ahead.lock().push(rows.to_vec());
        true
    }

    fn nudge(&self, _position: u64, _at: u64) {}

    fn seek(&self, _to: u64) {}

    fn pause(&self) {
        self.told.paused.store(true, Ordering::Release);
    }

    fn play(&self) {
        self.told.paused.store(false, Ordering::Release);
    }

    fn put_back(&self) {
        self.told.put_back.store(true, Ordering::Release);
    }
}

struct HeldOnly(u64);

impl PartyFinder for HeldOnly {
    type Row = u64;

    fn found(&self, song: &PartySong) -> Option<u64> {
        (song.row <= self.0).then_some(song.row)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_takes_up_what_the_host_plays_as_far_as_it_holds_and_puts_its_queue_back_on_leaving()
 {
    let server = hosted(Limits::default()).await;
    let host_settings = party_settings(server, "Host");
    let host_now = Arc::clone(&host_settings.now_nanos);
    let host = Party::host(host_settings).expect("the host starts");
    let code = host.shown().code;
    let host_told = Told::default();
    let host_seat = FakeSeat {
        began: host_now(),
        now_nanos: host_now,
        told: host_told,
        hosting: true,
        row: Arc::new(Mutex::new(Some(1))),
        next: Arc::new(Mutex::new(vec![2])),
    };
    let _hosted = Going::begin(host, host_seat, Arc::new(HeldOnly(2))).expect("the host's seat");

    let guest_settings = party_settings(server, "Guest");
    let guest_now = Arc::clone(&guest_settings.now_nanos);
    let told = Told::default();
    let guest_seat = FakeSeat {
        began: guest_now(),
        now_nanos: guest_now,
        told: told.clone(),
        hosting: false,
        row: Arc::new(Mutex::new(None)),
        next: Arc::new(Mutex::new(vec![2])),
    };
    let guest = Party::join(guest_settings, &code).expect("the guest joins");
    let mut going =
        Going::begin(guest, guest_seat, Arc::new(HeldOnly(1))).expect("the guest's seat");

    until("the guest to take the song up", || {
        !told.taken_up.lock().is_empty()
    })
    .await;
    {
        let taken = told.taken_up.lock();
        assert_eq!(taken[0].0, vec![1], "a next song it lacks stops the run");
        assert!(taken[0].2);
    }
    until("the guest to stand in step", || {
        going.followed().standing == Standing::InStep
    })
    .await;

    going.leave();

    assert!(told.put_back.load(Ordering::Acquire));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_holding_nothing_sits_out_and_is_not_moved() {
    let server = hosted(Limits::default()).await;
    let host_settings = party_settings(server, "Host");
    let host_now = Arc::clone(&host_settings.now_nanos);
    let host = Party::host(host_settings).expect("the host starts");
    let code = host.shown().code;
    let host_seat = FakeSeat {
        began: host_now(),
        now_nanos: host_now,
        told: Told::default(),
        hosting: true,
        row: Arc::new(Mutex::new(Some(1))),
        next: Arc::new(Mutex::new(vec![2])),
    };
    let _hosted = Going::begin(host, host_seat, Arc::new(HeldOnly(2))).expect("the host's seat");

    let guest_settings = party_settings(server, "Guest");
    let guest_now = Arc::clone(&guest_settings.now_nanos);
    let told = Told::default();
    let guest_seat = FakeSeat {
        began: guest_now(),
        now_nanos: guest_now,
        told: told.clone(),
        hosting: false,
        row: Arc::new(Mutex::new(None)),
        next: Arc::new(Mutex::new(vec![2])),
    };
    let guest = Party::join(guest_settings, &code).expect("the guest joins");
    let going = Going::begin(guest, guest_seat, Arc::new(HeldOnly(0))).expect("the guest's seat");

    until("the guest to sit the song out", || {
        matches!(going.followed().standing, Standing::SittingOut(_))
    })
    .await;

    assert!(told.taken_up.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guests_queue_is_kept_current_as_the_host_edits_past_the_songs_found_ahead() {
    let server = hosted(Limits::default()).await;
    let host_settings = party_settings(server, "Host");
    let host_now = Arc::clone(&host_settings.now_nanos);
    let host = Party::host(host_settings).expect("the host starts");
    let code = host.shown().code;
    let host_next = Arc::new(Mutex::new(vec![2, 3]));
    let host_seat = FakeSeat {
        began: host_now(),
        now_nanos: host_now,
        told: Told::default(),
        hosting: true,
        row: Arc::new(Mutex::new(Some(1))),
        next: Arc::clone(&host_next),
    };
    let _hosted = Going::begin(host, host_seat, Arc::new(HeldOnly(9))).expect("the host's seat");

    let guest_settings = party_settings(server, "Guest");
    let guest_now = Arc::clone(&guest_settings.now_nanos);
    let told = Told::default();
    let guest_seat = FakeSeat {
        began: guest_now(),
        now_nanos: guest_now,
        told: told.clone(),
        hosting: false,
        row: Arc::new(Mutex::new(None)),
        next: Arc::new(Mutex::new(Vec::new())),
    };
    let guest = Party::join(guest_settings, &code).expect("the guest joins");
    let _going = Going::begin(guest, guest_seat, Arc::new(HeldOnly(9))).expect("the guest's seat");

    until("the guest to take the queue up", || {
        !told.taken_up.lock().is_empty()
    })
    .await;
    assert_eq!(told.taken_up.lock()[0].0, vec![1, 2, 3]);

    *host_next.lock() = vec![2, 3, 4, 5];
    until("the guest to be given the song the host added", || {
        !told.kept_ahead.lock().is_empty()
    })
    .await;
    assert_eq!(told.kept_ahead.lock()[0], vec![2, 3, 4]);

    *host_next.lock() = vec![2, 4, 5];
    until(
        "the guest to be given the queue without the song the host took out",
        || told.kept_ahead.lock().len() == 2,
    )
    .await;
    assert_eq!(told.kept_ahead.lock()[1], vec![2, 4, 5]);
    let taken = told.taken_up.lock().clone();
    assert_eq!(
        taken.len(),
        1,
        "the song playing was never taken up again: {taken:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_clock_of_two_seats_on_the_live_path_agrees_within_what_is_recorded() {
    let Ok(server) = env::var("RESONATE_PARTY_SERVER") else {
        return;
    };
    let began = Instant::now();
    let now: Arc<dyn Fn() -> u64 + Send + Sync> =
        Arc::new(move || u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX));
    let settings = |name: &str| PartySettings {
        server: server.clone(),
        trusted: None,
        name: name.to_owned(),
        now_nanos: Arc::clone(&now),
    };
    let host = Party::host(settings("Host")).expect("the host starts");
    let guest = Party::join(settings("Guest"), &host.shown().code).expect("the guest joins");
    let measured_for = Duration::from_secs(
        env::var("RESONATE_PARTY_SECONDS")
            .ok()
            .and_then(|seconds| seconds.parse().ok())
            .unwrap_or(30),
    );

    until("both clocks to be set", || {
        host.to_server(0).is_some() && guest.to_server(0).is_some()
    })
    .await;

    let mut most_apart = 0_u64;
    let mut sum = 0_u128;
    let mut samples = 0_u128;
    let end = Instant::now() + measured_for;
    while Instant::now() < end {
        let here = now();
        let apart = host
            .to_server(here)
            .expect("the host has a clock")
            .abs_diff(guest.to_server(here).expect("the guest has a clock"));
        most_apart = most_apart.max(apart);
        sum += u128::from(apart);
        samples += 1;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let trip = guest.shown().round_trip.expect("a round trip was measured");
    println!(
        "round trip {trip:?}, clocks apart on average {:?}, at most {:?}",
        Duration::from_nanos(u64::try_from(sum / samples.max(1)).unwrap_or(u64::MAX)),
        Duration::from_nanos(most_apart)
    );
    assert!(
        most_apart < 100_000_000,
        "two seats disagreed by {most_apart} ns"
    );
}

fn hosted_on(
    server: SocketAddr,
    name: &str,
    seat: impl FnOnce(Arc<dyn Fn() -> u64 + Send + Sync>) -> FakeSeat,
) -> (Party, FakeSeat) {
    let settings = party_settings(server, name);
    let now = Arc::clone(&settings.now_nanos);

    (Party::host(settings).expect("the host starts"), seat(now))
}

fn the_host_among(party: &Party) -> Option<String> {
    party
        .shown()
        .people
        .into_iter()
        .find(|person| person.host)
        .map(|person| person.name)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_host_hands_the_party_to_a_guest_who_hosts_it_on_for_everyone_once_the_first_has_gone()
{
    let server = hosted(Limits {
        host_back_within: Duration::from_millis(300),
        ..Limits::default()
    })
    .await;
    let mut host = a_party_in_the_room(server).await;
    let code = host.shown().code;
    let ada = Party::join(party_settings(server, "Ada"), &code).expect("Ada joins");
    let bo = Party::join(party_settings(server, "Bo"), &code).expect("Bo joins");
    until("everyone to be named to the host", || {
        let people = host.shown().people;
        people.iter().any(|one| one.name == "Ada") && people.iter().any(|one| one.name == "Bo")
    })
    .await;
    let ada_is = ada.shown().me;

    host.hand_over(ada_is);
    until("Ada to host", || {
        let shown = ada.shown();
        shown.hosting && shown.reach == PartyReach::Here
    })
    .await;
    until("the first host to know it handed the party over", || {
        host.shown().reach == PartyReach::HandedOver
    })
    .await;
    until("Bo to follow Ada", || {
        the_host_among(&bo).as_deref() == Some("Ada")
    })
    .await;
    host.leave();
    tokio::time::sleep(Duration::from_secs(1)).await;
    ada.tell(PartyNow {
        song: Some(a_song(4)),
        next: Vec::new(),
        playing: true,
        position_nanos: 0,
        anchor_nanos: 0,
        coarse: false,
    });
    ada.say(PartyChat::Words("Mine now".to_owned()));

    until("Bo to hear what Ada plays", || {
        bo.shown().now.and_then(|now| now.song).map(|song| song.row) == Some(4)
    })
    .await;
    until("Bo to hear what Ada says", || {
        words_heard(&bo).contains(&("Ada".to_owned(), PartyChat::Words("Mine now".to_owned())))
    })
    .await;
    assert_eq!(
        bo.shown().reach,
        PartyReach::Here,
        "the room outlived its first host"
    );
    assert_eq!(ada.shown().handing, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_handed_to_someone_who_never_takes_it_stays_the_hosts() {
    let server = hosted(Limits::default()).await;
    let host = a_party_in_the_room(server).await;
    let key =
        resonate_sync::party::PartyKey::from_code(&host.shown().code).expect("the code reads");
    let keys = key.keys().expect("the keys");
    let mut deaf = Link::to(server).await;
    deaf.say(ToServer::Room {
        version: PROTOCOL_VERSION,
        room: keys.room,
        token: keys.token.clone(),
        host: None,
        member: device(9),
        seat: Some(token(9)),
    })
    .await;
    until("the deaf guest to arrive", || {
        host.shown().people.len() == 2
    })
    .await;

    host.hand_over(device(9));
    until("the host to be handing over", || {
        host.shown().handing == Some(PartyHanding::To(device(9)))
    })
    .await;
    tokio::time::sleep(HANDED_OVER_WITHIN).await;

    until("the host to give up on the handing", || {
        host.shown().handing == Some(PartyHanding::NotTaken(device(9)))
    })
    .await;
    assert!(host.shown().hosting);
    assert_eq!(host.shown().reach, PartyReach::Here);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_taking_the_party_over_plays_on_under_the_rows_everyone_already_follows() {
    let server = hosted(Limits {
        host_back_within: Duration::from_millis(300),
        ..Limits::default()
    })
    .await;
    let (host, host_seat) = hosted_on(server, "Host", |now| FakeSeat {
        began: now(),
        now_nanos: now,
        told: Told::default(),
        hosting: true,
        row: Arc::new(Mutex::new(Some(1))),
        next: Arc::new(Mutex::new(vec![2, 3])),
    });
    let code = host.shown().code;
    let mut hosted = Going::begin(host, host_seat, Arc::new(HeldOnly(3))).expect("the host's seat");

    let ada_settings = party_settings(server, "Ada");
    let ada_now = Arc::clone(&ada_settings.now_nanos);
    let told = Told::default();
    let ada_seat = FakeSeat {
        began: ada_now(),
        now_nanos: ada_now,
        told: told.clone(),
        hosting: false,
        row: Arc::new(Mutex::new(None)),
        next: Arc::new(Mutex::new(Vec::new())),
    };
    let ada = Party::join(ada_settings, &code).expect("Ada joins");
    let mut going = Going::begin(ada, ada_seat, Arc::new(HeldOnly(3))).expect("Ada's seat");
    let bo = Party::join(party_settings(server, "Bo"), &code).expect("Bo joins");
    until("Ada to stand in step", || {
        going.followed().standing == Standing::InStep
    })
    .await;
    until("Bo to arrive", || bo.shown().reach == PartyReach::Here).await;

    hosted.party().hand_over(going.shown().me);
    until("Ada to host", || going.shown().hosting).await;
    until("the first host to know", || {
        hosted.shown().reach == PartyReach::HandedOver
    })
    .await;
    hosted.leave();
    until("Bo to follow Ada", || {
        the_host_among(&bo).as_deref() == Some("Ada")
    })
    .await;
    until("Ada to have kept on what the host had next", || {
        told.kept_ahead.lock().last() == Some(&vec![2, 3])
    })
    .await;

    until("Ada to name the song by the row the party knows", || {
        going.shown().now.is_some_and(|now| {
            now.song.as_ref().map(|song| song.row) == Some(1)
                && now.next.iter().map(|song| song.row).collect::<Vec<_>>() == [2, 3]
        })
    })
    .await;
    going.leave();
    assert!(
        !told.put_back.load(Ordering::Acquire),
        "a host keeps the queue it plays"
    );
}
