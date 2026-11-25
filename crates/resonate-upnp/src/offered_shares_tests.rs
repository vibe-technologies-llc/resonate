use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use resonate_core::{AlbumId, TrackId};

use super::{Bounds, MediaServer, Offering, SHIPPED};
use crate::{
    Udn,
    catalog::{
        AudioKind, Catalog, Content, Held, Kind, Opened, Page, Picture, Query, Searched, Shelf,
        Shelved, Song, Within,
    },
    door::Limits,
    testing::{connected_from_another_address, heard_on_another_address},
};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

const HEARD_WITHIN: Duration = Duration::from_secs(5);

const TRICKLED_EVERY: Duration = Duration::from_millis(10);

const TRICKLED_IN: usize = 1 << 10;

const COVER_BYTES: usize = 32 << 20;

const SONG_BYTES: u64 = 64 << 20;

const CUT_AFTER: Duration = Duration::from_millis(300);

const ASKED_AGAIN_AFTER: Duration = Duration::from_millis(50);

struct Large;

struct Zeros {
    length: u64,
    at: u64,
}

impl Read for Zeros {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let left = usize::try_from(self.length.saturating_sub(self.at)).unwrap_or(usize::MAX);
        let read = left.min(into.len());

        into[..read].fill(0);
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Zeros {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.at = match to {
            SeekFrom::Start(at) => at,
            SeekFrom::End(back) => self.length.saturating_add_signed(back),
            SeekFrom::Current(on) => self.at.saturating_add_signed(on),
        };
        Ok(self.at)
    }
}

impl Catalog for Large {
    type Failure = io::Error;

    fn searched(&self, _: Kind, _: Within, _: &Query, _: Page) -> io::Result<Searched> {
        Ok(Searched::Unanswered)
    }

    fn shelved(&self, _: Shelf, _: Page) -> io::Result<Option<Shelved>> {
        Ok(None)
    }

    fn shelf(&self, _: Shelf) -> io::Result<Option<Held>> {
        Ok(None)
    }

    fn song(&self, _: TrackId) -> io::Result<Option<Song>> {
        Ok(None)
    }

    fn bytes_of(&self, _: TrackId) -> io::Result<Option<Opened>> {
        Ok(Some(Opened {
            bytes: Box::new(Zeros {
                length: SONG_BYTES,
                at: 0,
            }),
            kind: AudioKind::Flac,
            content: Content::Untouched,
        }))
    }

    fn cover(&self, _: AlbumId) -> io::Result<Option<Picture>> {
        Ok(Some(Picture {
            bytes: vec![0; COVER_BYTES],
            mime: "image/jpeg",
        }))
    }

    fn revision(&self) -> u32 {
        1
    }
}

fn started(bounds: Bounds) -> MediaServer<Large> {
    MediaServer::start_bounded(
        Arc::new(Large),
        Udn::new("uuid:4c1b3a27-0000-4000-8000-000000000002"),
        Offering {
            heard_on: SocketAddr::new(LOOPBACK, 0),
            told: SocketAddr::new(LOOPBACK, 9),
            every: Duration::from_secs(60),
            serving_on: LOOPBACK,
            name: None,
        },
        bounds,
    )
    .expect("the server starts")
}

fn at(server: &MediaServer<Large>) -> SocketAddr {
    SocketAddr::new(LOOPBACK, server.port())
}

fn status_of(stream: &mut TcpStream) -> Option<u16> {
    let mut held = Vec::new();
    let mut byte = [0; 1];

    while !held.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => held.push(byte[0]),
            Ok(_) | Err(_) => return None,
        }
    }
    String::from_utf8_lossy(&held)
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
}

fn asked_on(mut stream: TcpStream, server: SocketAddr, path: &str) -> Option<u16> {
    stream
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: {server}\r\n\r\n").as_bytes())
        .expect("asks");
    status_of(&mut stream)
}

fn asked(server: SocketAddr, path: &str) -> Option<u16> {
    asked_on(TcpStream::connect(server).expect("connects"), server, path)
}

fn is_closed(stream: &mut TcpStream) -> bool {
    stream
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");

    let mut byte = [0; 1];
    match stream.read(&mut byte) {
        Ok(read) => read == 0,
        Err(error) => error.kind() == io::ErrorKind::ConnectionReset,
    }
}

#[test]
fn thirty_two_idle_connections_from_one_address_leave_another_address_answered() {
    let server = started(SHIPPED);
    let server_at = at(&server);

    let idle: Vec<TcpStream> = (0..SHIPPED.limits.heads_at_most)
        .map(|_| TcpStream::connect(server_at).expect("connects"))
        .collect();
    let mut past_its_share = TcpStream::connect(server_at).expect("connects");

    assert!(is_closed(&mut past_its_share));

    let Some(elsewhere) = connected_from_another_address(server_at) else {
        return;
    };

    assert_eq!(
        asked_on(elsewhere, server_at, "/description.xml"),
        Some(200)
    );

    drop(idle);
}

struct Trickling {
    reading: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
}

impl Trickling {
    fn begun(server: SocketAddr, path: &str) -> Self {
        let mut stream = TcpStream::connect(server).expect("connects");

        stream
            .set_read_timeout(Some(HEARD_WITHIN))
            .expect("a read timeout");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: {server}\r\n\r\n").as_bytes())
            .expect("asks");

        assert_eq!(status_of(&mut stream), Some(200));

        let reading = Arc::new(AtomicBool::new(true));
        let still = Arc::clone(&reading);
        let thread = thread::spawn(move || {
            let mut buffer = [0; TRICKLED_IN];

            while still.load(Ordering::Acquire) {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => thread::sleep(TRICKLED_EVERY),
                }
            }
        });
        Self { reading, thread }
    }

    fn stop(self) {
        self.reading.store(false, Ordering::Release);

        let _ = self.thread.join();
    }
}

fn answered_once_free(server: SocketAddr) -> Duration {
    let began = Instant::now();

    while began.elapsed() < HEARD_WITHIN {
        if asked(server, "/description.xml") == Some(200) {
            return began.elapsed();
        }
        thread::sleep(ASKED_AGAIN_AFTER);
    }
    panic!("the answer held was never cut, so no other was answered");
}

fn one_answer_at_a_time() -> Bounds {
    Bounds {
        limits: Limits {
            answers_at_most: 1,
            answers_a_peer_holds_at_most: 1,
            ..SHIPPED.limits
        },
        written_within: CUT_AFTER,
        a_listener_waits_at_most: CUT_AFTER,
        ..SHIPPED
    }
}

#[test]
fn a_reader_trickling_an_answer_is_cut_at_the_whole_deadline_and_its_slot_is_freed() {
    let server = started(one_answer_at_a_time());
    let server_at = at(&server);

    let trickling = Trickling::begun(server_at, "/cover/album-1");
    let freed_after = answered_once_free(server_at);

    trickling.stop();

    assert!(freed_after < HEARD_WITHIN, "freed after {freed_after:?}");
}

#[test]
fn a_reader_trickling_a_song_slower_than_a_piece_within_the_wait_is_cut() {
    let server = started(one_answer_at_a_time());
    let server_at = at(&server);

    let trickling = Trickling::begun(server_at, "/resonate/song-1.flac");
    let freed_after = answered_once_free(server_at);

    trickling.stop();

    assert!(freed_after < HEARD_WITHIN, "freed after {freed_after:?}");
}

fn accepted_within(listener: &TcpListener, within: Duration) -> Option<TcpStream> {
    listener.set_nonblocking(true).expect("a listener");

    let deadline = Instant::now() + within;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).expect("a stream");
                return Some(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("no event could be heard: {error}"),
        }
    }
}

fn subscribed(server: SocketAddr, callback: &TcpListener) -> Option<u16> {
    let mut stream = TcpStream::connect(server).expect("connects");
    let callback = callback.local_addr().expect("an address");

    stream
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");
    stream
        .write_all(
            format!(
                "SUBSCRIBE /event/ContentDirectory HTTP/1.1\r\nHost: {server}\r\nCALLBACK: \
                 <http://{callback}/notify>\r\nNT: upnp:event\r\nTIMEOUT: Second-300\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("subscribes");
    status_of(&mut stream)
}

#[test]
fn a_hung_subscriber_does_not_hold_back_another_subscribers_telling() {
    let server = started(Bounds {
        told_within: Duration::from_secs(60),
        ..SHIPPED
    });
    let server_at = at(&server);
    let hung = TcpListener::bind(SocketAddr::new(LOOPBACK, 0)).expect("a callback");
    let answering = TcpListener::bind(SocketAddr::new(LOOPBACK, 0)).expect("a callback");

    assert_eq!(subscribed(server_at, &hung), Some(200));

    let held_unanswered = accepted_within(&hung, HEARD_WITHIN).expect("the hung one is told");

    assert_eq!(subscribed(server_at, &answering), Some(200));

    let mut told = accepted_within(&answering, HEARD_WITHIN)
        .expect("the other subscriber is told while the hung one is");

    told.set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");

    let mut asked = [0; 7];

    told.read_exact(&mut asked).expect("an event");

    assert_eq!(&asked, b"NOTIFY ");

    drop(held_unanswered);
}

fn searched(socket: &UdpSocket, heard_at: SocketAddr, st: &str) {
    let search = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: \
         1\r\nST: {st}\r\n\r\n"
    );

    socket
        .send_to(search.as_bytes(), heard_at)
        .expect("searches");
}

#[test]
fn one_senders_search_flood_leaves_another_senders_search_answered() {
    let server = started(SHIPPED);
    let flooding = UdpSocket::bind(SocketAddr::new(LOOPBACK, 0)).expect("a socket");

    let Some(elsewhere) = heard_on_another_address() else {
        return;
    };

    elsewhere
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");

    for _ in 0..100 {
        searched(&flooding, server.heard_at(), "ssdp:all");
    }
    searched(&elsewhere, server.heard_at(), crate::SERVER_TYPE);

    let mut answer = [0; 2048];

    let read = elsewhere.recv(&mut answer).expect("an answer");

    assert!(answer[..read].starts_with(b"HTTP/1.1 200 OK"));
}
