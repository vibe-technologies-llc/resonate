use std::{
    collections::BTreeMap,
    hash::{BuildHasher, RandomState},
    io::{self, Write},
    net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{
    Error, Result, ServeOp,
    feed::Feed,
    http::{self, HEAD_BYTES_AT_MOST, HEADERS_AT_MOST},
};

const STREAMS_UNDER: &str = "/resonate/";

const ACCEPTS_EVERY: Duration = Duration::from_millis(100);

const HEADS_POLLED_EVERY: Duration = Duration::from_millis(10);

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
}

impl Server {
    pub fn start(serving_on: IpAddr) -> Result<Arc<Self>> {
        Self::start_limited(serving_on, Limits::SHIPPED)
    }

    fn start_limited(serving_on: IpAddr, limits: Limits) -> Result<Arc<Self>> {
        let failed = |op| move |source| Error::Serve { op, source };

        let listener =
            TcpListener::bind(SocketAddr::new(serving_on, 0)).map_err(failed(ServeOp::Bind))?;
        listener
            .set_nonblocking(true)
            .map_err(failed(ServeOp::Listen))?;
        let port = listener
            .local_addr()
            .map_err(failed(ServeOp::Listen))?
            .port();
        let server = Arc::new(Self {
            serving_on,
            port,
            feeds: Mutex::new(BTreeMap::new()),
            minted: AtomicU64::new(0),
            keys: RandomState::new(),
        });

        let held = Arc::downgrade(&server);
        thread::Builder::new()
            .name("resonate-upnp-serve".to_owned())
            .spawn(move || accept(&listener, &held, limits))
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
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            held: Vec::new(),
            since: Instant::now(),
        })
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
                    return if self.since.elapsed() > head_ends_within {
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
}

fn accept(listener: &TcpListener, held: &Weak<Server>, limits: Limits) {
    let fetching = Arc::new(AtomicUsize::new(0));
    let mut pending: Vec<Pending> = Vec::new();
    loop {
        let Some(server) = held.upgrade() else {
            return;
        };
        loop {
            match listener.accept() {
                Ok((stream, _)) if pending.len() < limits.heads_at_most => {
                    match Pending::new(stream) {
                        Ok(asking) => pending.push(asking),
                        Err(error) => {
                            tracing::debug!(%error, "a renderer's connection could not be read");
                        }
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    tracing::warn!(%error, "the stream server stopped accepting renderers");
                    return;
                }
            }
        }

        let mut waiting = Vec::with_capacity(pending.len());
        for mut asking in pending.drain(..) {
            match asking.advance(&server, limits.head_ends_within) {
                Next::Waiting => waiting.push(asking),
                Next::Dropped => {}
                Next::Refused(refusal) => refuse(asking.stream, refusal),
                Next::Fetch { head_only, feed } => {
                    match Fetching::begin(&fetching, limits.fetches_at_most) {
                        Some(slot) => begin_fetch(asking.stream, head_only, feed, slot),
                        None => refuse(asking.stream, BUSY),
                    }
                }
            }
        }
        pending = waiting;

        drop(server);
        thread::sleep(if pending.is_empty() {
            ACCEPTS_EVERY
        } else {
            HEADS_POLLED_EVERY
        });
    }
}

fn refuse(mut stream: TcpStream, refusal: &[u8]) {
    let written = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_write_timeout(Some(REFUSAL_WRITTEN_WITHIN)))
        .and_then(|()| stream.write_all(refusal));
    if let Err(error) = written {
        tracing::debug!(%error, "a renderer's refusal could not be written");
    }
}

fn begin_fetch(stream: TcpStream, head_only: bool, feed: Arc<Feed>, slot: Fetching) {
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

fn answer(feed: &Feed, mut stream: TcpStream, head_only: bool) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let served = feed.serve(&mut stream, head_only);
    let _ = stream.shutdown(Shutdown::Both);
    served
}

#[cfg(test)]
mod tests {
    use std::{io::Read, net::Ipv4Addr};

    use super::*;
    use crate::testing::silent_feed as feed;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    const HEARD_WITHIN: Duration = Duration::from_secs(5);

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
}
