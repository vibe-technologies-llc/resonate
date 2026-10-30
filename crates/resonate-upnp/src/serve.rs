use std::{
    collections::BTreeMap,
    hash::{BuildHasher, RandomState},
    io::{self, Write},
    net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;

use crate::{
    Error, Result, ServeOp,
    feed::Feed,
    http::{self, HEAD_BYTES_AT_MOST, HEADERS_AT_MOST},
};

const STREAMS_UNDER: &str = "/resonate/";

const ACCEPTS_EVERY: Duration = Duration::from_millis(100);

const ASKED_WITHIN: Duration = Duration::from_secs(5);

const NOT_HERE: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const NOT_ALLOWED: &[u8] =
    b"HTTP/1.1 405 Method Not Allowed\r\nAllow: GET, HEAD\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

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
            .spawn(move || accept(&listener, &held))
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
        let named = path.strip_prefix(STREAMS_UNDER)?;
        let token = named.split('.').next()?;
        let token = Token(u64::from_str_radix(token, 16).ok()?);
        self.feeds.lock().get(&token).cloned()
    }
}

fn accept(listener: &TcpListener, held: &Weak<Server>) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let Some(server) = held.upgrade() else {
                    return;
                };
                let spawned = thread::Builder::new()
                    .name("resonate-upnp-feed".to_owned())
                    .spawn(move || {
                        if let Err(error) = answer(&server, stream) {
                            tracing::debug!(%error, "a renderer's fetch of a stream ended");
                        }
                    });
                if let Err(error) = spawned {
                    tracing::warn!(%error, "a renderer's fetch could not be answered");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if held.strong_count() == 0 {
                    return;
                }
                thread::sleep(ACCEPTS_EVERY);
            }
            Err(error) => {
                tracing::warn!(%error, "the stream server stopped accepting renderers");
                return;
            }
        }
    }
}

fn answer(server: &Server, mut stream: TcpStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(ASKED_WITHIN))?;

    let mut held = Vec::new();
    while http::head_end(&held).is_none() {
        if held.len() > HEAD_BYTES_AT_MOST || http::read_more(&mut stream, &mut held)? == 0 {
            return Ok(());
        }
    }
    let (method, path) = {
        let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&held) {
            Ok(httparse::Status::Complete(_)) => {}
            Ok(httparse::Status::Partial) | Err(_) => return Ok(()),
        }
        (
            request.method.unwrap_or_default().to_owned(),
            request.path.unwrap_or_default().to_owned(),
        )
    };

    let head_only = match method.as_str() {
        "GET" => false,
        "HEAD" => true,
        _ => {
            stream.write_all(NOT_ALLOWED)?;
            return Ok(());
        }
    };
    let Some(feed) = server.feed_at(&path) else {
        stream.write_all(NOT_HERE)?;
        return Ok(());
    };
    let served = feed.serve(&mut stream, head_only);
    let _ = stream.shutdown(Shutdown::Both);
    served
}
