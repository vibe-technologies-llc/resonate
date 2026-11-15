#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::{
    collections::BTreeMap,
    hash::{BuildHasher, RandomState},
    io,
    net::{self, IpAddr, SocketAddr, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use mio::Waker;
use parking_lot::Mutex;

use crate::{
    Error, Result, ServeOp,
    door::{Door, Doorway, Judged, Limits},
    feed::Feed,
    http::HEADERS_AT_MOST,
};

const STREAMS_UNDER: &str = "/resonate/";

const HEAD_ENDS_WITHIN: Duration = Duration::from_secs(5);

const FETCHES_AT_MOST: usize = 16;

const HEADS_AT_MOST: usize = 32;

const TOKEN_DIGITS: usize = 16;

pub(crate) const NOT_HERE: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const NOT_ALLOWED: &[u8] =
    b"HTTP/1.1 405 Method Not Allowed\r\nAllow: GET, HEAD\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const SHIPPED: Limits = Limits {
    answers_at_most: FETCHES_AT_MOST,
    heads_at_most: HEADS_AT_MOST,
    head_ends_within: HEAD_ENDS_WITHIN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token(u64);

pub struct Server {
    serving_on: IpAddr,
    port: u16,
    feeds: Mutex<BTreeMap<Token, Arc<Feed>>>,
    minted: AtomicU64,
    keys: RandomState,
    waker: Waker,
    #[cfg(test)]
    polls: AtomicUsize,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.waker.wake() {
            tracing::debug!(%error, "the stream server's thread could not be woken to end");
        }
    }
}

impl Server {
    pub fn start(serving_on: IpAddr) -> Result<Arc<Self>> {
        Self::start_limited(serving_on, SHIPPED)
    }

    fn start_limited(serving_on: IpAddr, limits: Limits) -> Result<Arc<Self>> {
        let (doorway, opened) = Doorway::bind(serving_on)?;
        let server = Arc::new(Self {
            serving_on,
            port: opened.port,
            feeds: Mutex::new(BTreeMap::new()),
            minted: AtomicU64::new(0),
            keys: RandomState::new(),
            waker: opened.waker,
            #[cfg(test)]
            polls: AtomicUsize::new(0),
        });
        doorway.open(Arc::downgrade(&server), limits, "resonate-upnp-serve")?;
        Ok(server)
    }

    pub fn offer(&self, feed: Arc<Feed>) -> Token {
        let count = self.minted.fetch_add(1, Ordering::Relaxed);
        let token = Token(self.keys.hash_one(count));
        self.feeds.lock().insert(token, feed);
        token
    }

    pub fn withdraw(&self, token: Token) {
        self.feeds.lock().remove(&token);
    }

    pub fn address_toward(
        &self,
        renderer: SocketAddr,
        token: Token,
        extension: &str,
    ) -> Result<String> {
        let host = host_toward(self.serving_on, renderer)?;
        Ok(format!(
            "http://{host}:{}{STREAMS_UNDER}{:016x}.{extension}",
            self.port, token.0
        ))
    }

    fn feed_at(&self, path: &str) -> Option<Arc<Feed>> {
        let token = token_in(path)?;
        self.feeds.lock().get(&token).cloned()
    }
}

pub(crate) fn host_toward(serving_on: IpAddr, toward: SocketAddr) -> Result<String> {
    let ip = if serving_on.is_unspecified() {
        let route = |source| Error::Serve {
            op: ServeOp::Route,
            source,
        };
        let probe = UdpSocket::bind(SocketAddr::new(serving_on, 0)).map_err(route)?;
        probe.connect(toward).map_err(route)?;
        probe.local_addr().map_err(route)?.ip()
    } else {
        serving_on
    };
    Ok(match ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    })
}

pub(crate) struct Fetch {
    head_only: bool,
    feed: Arc<Feed>,
}

impl Door for Server {
    type Task = Fetch;

    const ANSWERED_ON: &'static str = "resonate-upnp-feed";

    fn admits(&self, _: IpAddr) -> bool {
        true
    }

    fn judged(&self, held: &[u8]) -> Judged<Fetch> {
        match asked_in(held) {
            Asked::Unreadable => Judged::Unreadable,
            Asked::NotAllowed => Judged::Refused(NOT_ALLOWED),
            Asked::Fetch { head_only, path } => match self.feed_at(path) {
                Some(feed) => Judged::Taken(Fetch { head_only, feed }),
                None => Judged::Refused(NOT_HERE),
            },
        }
    }

    fn answer(fetch: Fetch, mut stream: net::TcpStream, _: Vec<u8>) -> io::Result<()> {
        fetch.feed.serve(&mut stream, fetch.head_only)
    }

    #[cfg(test)]
    fn polled(&self) {
        self.polls.fetch_add(1, Ordering::Relaxed);
    }
}

fn token_in(path: &str) -> Option<Token> {
    let path = path.split(['?', '#']).next()?;
    let named = path.strip_prefix(STREAMS_UNDER)?;
    let (token, extension) = named.split_once('.')?;
    let well_formed = token.len() == TOKEN_DIGITS
        && token.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !extension.is_empty()
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if !well_formed {
        return None;
    }
    u64::from_str_radix(token, 16).ok().map(Token)
}

enum Asked<'a> {
    Unreadable,
    NotAllowed,
    Fetch { head_only: bool, path: &'a str },
}

fn asked_in(held: &[u8]) -> Asked<'_> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(held) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) | Err(_) => return Asked::Unreadable,
    }
    let head_only = match request.method.unwrap_or_default() {
        "GET" => false,
        "HEAD" => true,
        _ => return Asked::NotAllowed,
    };
    Asked::Fetch {
        head_only,
        path: request.path.unwrap_or_default(),
    }
}

#[cfg(fuzzing)]
pub(crate) fn read_a_request_head(held: &[u8]) {
    if let Asked::Fetch { path, .. } = asked_in(held) {
        let _ = token_in(path);
    }
    if let Ok(text) = std::str::from_utf8(held) {
        let _ = token_in(text);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{Ipv4Addr, TcpStream},
        thread,
        time::Instant,
    };

    use super::*;
    use crate::testing::silent_feed as feed;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    const HEARD_WITHIN: Duration = Duration::from_secs(5);

    const AN_ADMISSION: Duration = Duration::from_millis(100);

    const AN_IDLE_WHILE: Duration = Duration::from_millis(400);

    fn connected(server: &Server) -> TcpStream {
        let stream = TcpStream::connect(SocketAddr::new(LOOPBACK, server.port)).expect("connects");
        stream
            .set_read_timeout(Some(HEARD_WITHIN))
            .expect("a read timeout");
        stream
    }

    fn asking(server: &Server, path: &str) -> TcpStream {
        let mut stream = connected(server);
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .expect("asks");
        stream
    }

    fn status_of(stream: &mut TcpStream) -> String {
        let mut answer = [0; 12];
        stream.read_exact(&mut answer).expect("a status line");
        String::from_utf8_lossy(&answer).into_owned()
    }

    fn is_closed(stream: &mut TcpStream) -> bool {
        let mut byte = [0; 1];
        match stream.read(&mut byte) {
            Ok(read) => read == 0,
            Err(error) => error.kind() == io::ErrorKind::ConnectionReset,
        }
    }

    fn limited(answers_at_most: usize, heads_at_most: usize, within: Duration) -> Arc<Server> {
        Server::start_limited(
            LOOPBACK,
            Limits {
                answers_at_most,
                heads_at_most,
                head_ends_within: within,
            },
        )
        .expect("starts")
    }

    #[test]
    fn only_a_path_of_a_token_and_an_extension_names_a_stream() {
        let token = format!("{STREAMS_UNDER}00000000deadbeef.flac");

        assert_eq!(token_in(&token), Some(Token(0xdead_beef)));
        assert_eq!(token_in(&format!("{token}?x=1")), Some(Token(0xdead_beef)));
        assert_eq!(token_in("/resonate/deadbeef.flac"), None);
        assert_eq!(token_in("/resonate/00000000deadbeef"), None);
        assert_eq!(token_in("/resonate/00000000deadbeef."), None);
        assert_eq!(token_in("/resonate/00000000deadbeeg.flac"), None);
        assert_eq!(token_in("/resonate/+0000000deadbeef.flac"), None);
        assert_eq!(token_in("/other/00000000deadbeef.flac"), None);
    }

    #[test]
    fn a_connection_that_never_finishes_its_head_is_let_go_at_the_deadline() {
        let server = limited(4, 8, Duration::from_millis(200));
        let mut slow = connected(&server);
        slow.write_all(b"GET /resonate/").expect("starts a head");

        assert!(is_closed(&mut slow));
    }

    #[test]
    fn connections_past_the_ones_waiting_on_a_head_are_let_go_at_once() {
        let server = limited(4, 2, Duration::from_secs(30));
        let _first = connected(&server);
        let _second = connected(&server);
        let mut third = connected(&server);

        assert!(is_closed(&mut third));
    }

    #[test]
    fn a_path_naming_no_stream_is_refused_without_a_fetch() {
        let server = limited(1, 8, Duration::from_secs(30));
        let mut lost = asking(&server, "/resonate/0000000000000001.wav");
        let mut elsewhere = asking(&server, "/etc/passwd");

        assert!(status_of(&mut lost).contains("404"));
        assert!(status_of(&mut elsewhere).contains("404"));
    }

    #[test]
    fn fetches_past_the_most_are_told_to_come_back() {
        let server = limited(1, 8, Duration::from_secs(30));
        let first = feed();
        let second = feed();
        let first_token = server.offer(Arc::clone(&first));
        let second_token = server.offer(Arc::clone(&second));
        let path = |token: Token| format!("{STREAMS_UNDER}{:016x}.wav", token.0);

        let mut held = asking(&server, &path(first_token));
        assert!(status_of(&mut held).contains("200"));
        let mut turned_away = asking(&server, &path(second_token));

        assert!(status_of(&mut turned_away).contains("503"));
        first.close();
        second.close();
    }

    #[test]
    fn a_server_waiting_on_a_slow_head_wakes_for_nothing_until_it_moves_or_its_deadline_comes() {
        let server = limited(4, 8, Duration::from_secs(30));
        let mut slow = connected(&server);
        slow.write_all(b"GET /resonate/").expect("starts a head");
        thread::sleep(AN_ADMISSION);
        let before = server.polls.load(Ordering::Relaxed);

        thread::sleep(AN_IDLE_WHILE);

        assert_eq!(server.polls.load(Ordering::Relaxed), before);
        slow.write_all(b"0000000000000001.wav HTTP/1.1\r\n\r\n")
            .expect("goes on");
        assert!(status_of(&mut slow).contains("404"));
    }

    #[test]
    fn a_server_dropped_stops_listening_at_once() {
        let server = limited(4, 8, Duration::from_secs(30));
        let address = SocketAddr::new(LOOPBACK, server.port);

        drop(server);

        let deadline = Instant::now() + HEARD_WITHIN;
        while TcpStream::connect(address).is_ok() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(TcpStream::connect(address).is_err());
    }
}
