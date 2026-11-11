use std::{
    collections::BTreeMap,
    hash::{BuildHasher, RandomState},
    io::{self, Write},
    net::{self, IpAddr, Shutdown, SocketAddr, UdpSocket},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use mio::{
    Events, Interest, Poll, Registry, Token as Source, Waker,
    net::{TcpListener, TcpStream},
};
use parking_lot::Mutex;

use crate::{
    Error, Result, ServeOp,
    feed::Feed,
    http::{self, HEAD_BYTES_AT_MOST, HEADERS_AT_MOST},
};

const STREAMS_UNDER: &str = "/resonate/";

const LISTENER: Source = Source(0);

const WAKER: Source = Source(1);

const FIRST_HEAD: usize = 2;

const EVENTS_AT_MOST: usize = 64;

const HEAD_ENDS_WITHIN: Duration = Duration::from_secs(5);

const REFUSAL_WRITTEN_WITHIN: Duration = Duration::from_secs(1);

const FETCHES_AT_MOST: usize = 16;

const HEADS_AT_MOST: usize = 32;

const TOKEN_DIGITS: usize = 16;

const NOT_HERE: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const NOT_ALLOWED: &[u8] =
    b"HTTP/1.1 405 Method Not Allowed\r\nAllow: GET, HEAD\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const BUSY: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

#[derive(Clone, Copy)]
struct Limits {
    fetches_at_most: usize,
    heads_at_most: usize,
    head_ends_within: Duration,
}

impl Limits {
    const SHIPPED: Self = Self {
        fetches_at_most: FETCHES_AT_MOST,
        heads_at_most: HEADS_AT_MOST,
        head_ends_within: HEAD_ENDS_WITHIN,
    };
}

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
        Self::start_limited(serving_on, Limits::SHIPPED)
    }

    fn start_limited(serving_on: IpAddr, limits: Limits) -> Result<Arc<Self>> {
        let failed = |op| move |source| Error::Serve { op, source };

        let mut listener =
            TcpListener::bind(SocketAddr::new(serving_on, 0)).map_err(failed(ServeOp::Bind))?;
        let port = listener
            .local_addr()
            .map_err(failed(ServeOp::Listen))?
            .port();
        let poll = Poll::new().map_err(failed(ServeOp::Listen))?;
        poll.registry()
            .register(&mut listener, LISTENER, Interest::READABLE)
            .map_err(failed(ServeOp::Listen))?;
        let waker = Waker::new(poll.registry(), WAKER).map_err(failed(ServeOp::Listen))?;
        let server = Arc::new(Self {
            serving_on,
            port,
            feeds: Mutex::new(BTreeMap::new()),
            minted: AtomicU64::new(0),
            keys: RandomState::new(),
            waker,
            #[cfg(test)]
            polls: AtomicUsize::new(0),
        });

        let held = Arc::downgrade(&server);
        thread::Builder::new()
            .name("resonate-upnp-serve".to_owned())
            .spawn(move || accept(poll, &listener, &held, limits))
            .map_err(failed(ServeOp::Listen))?;
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
        let ip = if self.serving_on.is_unspecified() {
            let route = |source| Error::Serve {
                op: ServeOp::Route,
                source,
            };
            let probe = UdpSocket::bind(SocketAddr::new(self.serving_on, 0)).map_err(route)?;
            probe.connect(renderer).map_err(route)?;
            probe.local_addr().map_err(route)?.ip()
        } else {
            self.serving_on
        };
        let host = match ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
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

struct Fetching(Arc<AtomicUsize>);

impl Fetching {
    fn begin(count: &Arc<AtomicUsize>, at_most: usize) -> Option<Self> {
        let before = count.fetch_add(1, Ordering::AcqRel);
        if before >= at_most {
            count.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self(Arc::clone(count)))
    }
}

impl Drop for Fetching {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Pending {
    stream: TcpStream,
    held: Vec<u8>,
    since: Instant,
}

enum Next {
    Waiting,
    Dropped,
    Refused(&'static [u8]),
    Fetch { head_only: bool, feed: Arc<Feed> },
}

impl Pending {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            held: Vec::new(),
            since: Instant::now(),
        }
    }

    fn waits_another(&self, head_ends_within: Duration) -> Duration {
        head_ends_within.saturating_sub(self.since.elapsed())
    }

    fn advance(&mut self, server: &Server, head_ends_within: Duration) -> Next {
        loop {
            if http::head_end(&self.held).is_some() {
                return self.judged(server);
            }
            if self.held.len() > HEAD_BYTES_AT_MOST {
                return Next::Dropped;
            }
            match http::read_more(&mut self.stream, &mut self.held) {
                Ok(0) => return Next::Dropped,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return if self.waits_another(head_ends_within).is_zero() {
                        Next::Dropped
                    } else {
                        Next::Waiting
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Next::Dropped,
            }
        }
    }

    fn judged(&self, server: &Server) -> Next {
        let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&self.held) {
            Ok(httparse::Status::Complete(_)) => {}
            Ok(httparse::Status::Partial) | Err(_) => return Next::Dropped,
        }
        let head_only = match request.method.unwrap_or_default() {
            "GET" => false,
            "HEAD" => true,
            _ => return Next::Refused(NOT_ALLOWED),
        };
        match server.feed_at(request.path.unwrap_or_default()) {
            Some(feed) => Next::Fetch { head_only, feed },
            None => Next::Refused(NOT_HERE),
        }
    }

    fn handed_over(mut self, registry: &Registry) -> io::Result<net::TcpStream> {
        registry.deregister(&mut self.stream)?;
        Ok(net::TcpStream::from(self.stream))
    }

    fn let_go(mut self, registry: &Registry) {
        let _ = registry.deregister(&mut self.stream);
    }
}

struct Heads {
    waiting: BTreeMap<Source, Pending>,
    next: usize,
}

impl Heads {
    fn new() -> Self {
        Self {
            waiting: BTreeMap::new(),
            next: FIRST_HEAD,
        }
    }

    fn admit(&mut self, registry: &Registry, mut stream: TcpStream) -> Option<Source> {
        let source = Source(self.next);
        if let Err(error) = registry.register(&mut stream, source, Interest::READABLE) {
            tracing::debug!(%error, "a renderer's connection could not be read");
            return None;
        }
        self.next = self.next.wrapping_add(1).max(FIRST_HEAD);
        self.waiting.insert(source, Pending::new(stream));
        Some(source)
    }

    fn nearest_deadline(&self, head_ends_within: Duration) -> Option<Duration> {
        self.waiting
            .values()
            .map(|asking| asking.waits_another(head_ends_within))
            .min()
    }

    fn expired(&self, head_ends_within: Duration) -> Vec<Source> {
        self.waiting
            .iter()
            .filter(|(_, asking)| asking.waits_another(head_ends_within).is_zero())
            .map(|(source, _)| *source)
            .collect()
    }
}

fn admitted(
    listener: &TcpListener,
    registry: &Registry,
    heads: &mut Heads,
    heads_at_most: usize,
) -> Option<Vec<Source>> {
    let mut admitted = Vec::new();
    loop {
        match listener.accept() {
            Ok((stream, _)) if heads.waiting.len() < heads_at_most => {
                admitted.extend(heads.admit(registry, stream));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Some(admitted),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(%error, "the stream server stopped accepting renderers");
                return None;
            }
        }
    }
}

fn accept(mut poll: Poll, listener: &TcpListener, held: &Weak<Server>, limits: Limits) {
    let fetching = Arc::new(AtomicUsize::new(0));
    let mut heads = Heads::new();
    let mut events = Events::with_capacity(EVENTS_AT_MOST);
    loop {
        let timeout = heads.nearest_deadline(limits.head_ends_within);
        match poll.poll(&mut events, timeout) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                tracing::warn!(%error, "the stream server stopped waiting on renderers");
                return;
            }
        }
        let Some(server) = held.upgrade() else {
            return;
        };
        #[cfg(test)]
        server.polls.fetch_add(1, Ordering::Relaxed);

        let mut ready: Vec<Source> = Vec::new();
        for event in &events {
            match event.token() {
                LISTENER => {
                    let Some(admitted) =
                        admitted(listener, poll.registry(), &mut heads, limits.heads_at_most)
                    else {
                        return;
                    };
                    ready.extend(admitted);
                }
                WAKER => {}
                source => ready.push(source),
            }
        }
        ready.extend(heads.expired(limits.head_ends_within));
        ready.sort_unstable();
        ready.dedup();

        for source in ready {
            let Some(mut asking) = heads.waiting.remove(&source) else {
                continue;
            };
            match asking.advance(&server, limits.head_ends_within) {
                Next::Waiting => {
                    heads.waiting.insert(source, asking);
                }
                Next::Dropped => asking.let_go(poll.registry()),
                Next::Refused(refusal) => handed(asking, &poll, |stream| refuse(stream, refusal)),
                Next::Fetch { head_only, feed } => handed(asking, &poll, |stream| {
                    match Fetching::begin(&fetching, limits.fetches_at_most) {
                        Some(slot) => begin_fetch(stream, head_only, feed, slot),
                        None => refuse(stream, BUSY),
                    }
                }),
            }
        }
    }
}

fn handed(asking: Pending, poll: &Poll, then: impl FnOnce(net::TcpStream)) {
    match asking.handed_over(poll.registry()) {
        Ok(stream) => then(stream),
        Err(error) => tracing::debug!(%error, "a renderer's connection could not be taken over"),
    }
}

fn refuse(mut stream: net::TcpStream, refusal: &[u8]) {
    let written = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_write_timeout(Some(REFUSAL_WRITTEN_WITHIN)))
        .and_then(|()| stream.write_all(refusal));
    if let Err(error) = written {
        tracing::debug!(%error, "a renderer's refusal could not be written");
    }
}

fn begin_fetch(stream: net::TcpStream, head_only: bool, feed: Arc<Feed>, slot: Fetching) {
    let spawned = thread::Builder::new()
        .name("resonate-upnp-feed".to_owned())
        .spawn(move || {
            let _slot = slot;
            if let Err(error) = answer(&feed, stream, head_only) {
                tracing::debug!(%error, "a renderer's fetch of a stream ended");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a renderer's fetch could not be answered");
    }
}

fn answer(feed: &Feed, mut stream: net::TcpStream, head_only: bool) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let served = feed.serve(&mut stream, head_only);
    let _ = stream.shutdown(Shutdown::Both);
    served
}

#[cfg(test)]
mod tests {
    use std::{
        io::Read,
        net::{Ipv4Addr, TcpStream},
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

    fn limited(fetches_at_most: usize, heads_at_most: usize, within: Duration) -> Arc<Server> {
        Server::start_limited(
            LOOPBACK,
            Limits {
                fetches_at_most,
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
