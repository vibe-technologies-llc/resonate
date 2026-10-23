use std::{
    io::{self, Read},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use resonate_providers::{Delivery, Opened};
use resonate_tidal::HifiApi;
use rustls::{crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc},
    task::spawn_blocking,
    time::{sleep, timeout},
};
use tokio_rustls::TlsAcceptor;

use crate::{Error, Keys, Result};

const HTTP_ALPN: &[u8] = b"http/1.1";
const HEAD_BYTES_AT_MOST: usize = 8 * 1024;
const HEADERS_AT_MOST: usize = 32;
const ANSWER_BYTES_AT_MOST: usize = 1024 * 1024;
const CONNECTIONS_AT_MOST: usize = 64;
const REQUEST_WITHIN: Duration = Duration::from_secs(10);
const UPSTREAM_WITHIN: Duration = Duration::from_secs(30);
const ACCEPT_RETRY_AFTER: Duration = Duration::from_millis(100);
const DOWNLOADS_AT_ONCE: usize = 2;
const FILE_CHUNK_BYTES: usize = 64 * 1024;
const FILE_CHUNKS_AHEAD: usize = 4;
const FILE_BYTES_AT_MOST: u64 = 1024 * 1024 * 1024;
const CHUNK_WRITTEN_WITHIN: Duration = Duration::from_secs(60);
const FILE_ROUTE: &str = "/file/";
const FILE_TYPE: &str = "audio/flac";
const BEARER: &str = "Bearer ";
const SEARCH_ROUTE: &str = "/search/";
const REQUESTS_ROUTE: &str = "/playback/requests/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verb {
    Get,
    Delete,
}

enum Admitted {
    Forward(Verb, String),
    File(u64),
}

#[derive(Debug, PartialEq, Eq)]
enum Refused {
    Unwelcome,
    Method,
    Route,
}

#[derive(Debug, PartialEq, Eq)]
enum Unreached {
    Connect,
    Unreadable,
    TooLarge,
}

struct Answer {
    status: u16,
    content_type: Option<String>,
    retry_after: Option<String>,
    body: Vec<u8>,
}

pub struct Gateway {
    keys: Keys,
    upstream: SocketAddr,
    hifi: Arc<HifiApi>,
    seats: Arc<Semaphore>,
    downloads: Semaphore,
}

pub fn gateway_tls(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<TlsAcceptor> {
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&TLS13])
        .map_err(|source| Error::Tls(Box::new(source)))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|source| Error::Tls(Box::new(source)))?;
    tls.alpn_protocols = vec![HTTP_ALPN.to_vec()];
    Ok(TlsAcceptor::from(Arc::new(tls)))
}

impl Gateway {
    pub fn new(keys: Keys, upstream: SocketAddr) -> Arc<Self> {
        Self::through(keys, upstream, HifiApi::at(&format!("http://{upstream}")))
    }

    pub fn through(keys: Keys, upstream: SocketAddr, hifi: HifiApi) -> Arc<Self> {
        Arc::new(Self {
            keys,
            upstream,
            hifi: Arc::new(hifi),
            seats: Arc::new(Semaphore::new(CONNECTIONS_AT_MOST)),
            downloads: Semaphore::new(DOWNLOADS_AT_ONCE),
        })
    }

    pub async fn serve(self: Arc<Self>, listener: TcpListener, acceptor: TlsAcceptor) {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                sleep(ACCEPT_RETRY_AFTER).await;
                continue;
            };
            let Ok(seat) = Arc::clone(&self.seats).acquire_owned().await else {
                return;
            };
            let gateway = Arc::clone(&self);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _seat = seat;
                let served = async {
                    let tls = timeout(REQUEST_WITHIN, acceptor.accept(stream))
                        .await
                        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
                    gateway.answer(tls).await
                }
                .await;
                if let Err(error) = served {
                    tracing::debug!(%peer, %error, "a hifi request ended");
                }
            });
        }
    }

    async fn answer<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        mut stream: S,
    ) -> std::io::Result<()> {
        let head = match timeout(REQUEST_WITHIN, read_head(&mut stream)).await {
            Ok(Ok(Some(head))) => head,
            Ok(Ok(None)) | Err(_) => return reply(&mut stream, 400, &[], b"").await,
            Ok(Err(error)) => return Err(error),
        };
        match self.admitted(&head) {
            Err(Refused::Unwelcome) => {
                reply(
                    &mut stream,
                    401,
                    &[("WWW-Authenticate", "Bearer".to_owned())],
                    b"",
                )
                .await
            }
            Err(Refused::Method) => {
                reply(
                    &mut stream,
                    405,
                    &[("Allow", "GET, DELETE".to_owned())],
                    b"",
                )
                .await
            }
            Err(Refused::Route) => reply(&mut stream, 404, &[], b"").await,
            Ok(Admitted::File(id)) => self.filed(&mut stream, id).await,
            Ok(Admitted::Forward(verb, target)) => match self.relayed(verb, &target).await {
                Ok(answer) => {
                    let mut headers = Vec::new();
                    if let Some(kind) = answer.content_type {
                        headers.push(("Content-Type", kind));
                    }
                    if let Some(wait) = answer.retry_after {
                        headers.push(("Retry-After", wait));
                    }
                    reply(&mut stream, answer.status, &headers, &answer.body).await
                }
                Err(unreached) => {
                    tracing::warn!(?unreached, upstream = %self.upstream, "the hifi-api did not answer");
                    reply(&mut stream, 502, &[], b"").await
                }
            },
        }
    }

    fn admitted(&self, head: &Head) -> std::result::Result<Admitted, Refused> {
        let secret = head.bearer.as_deref().ok_or(Refused::Unwelcome)?;
        if !self.keys.admits(secret.as_bytes()) {
            return Err(Refused::Unwelcome);
        }
        let verb = match head.method.as_str() {
            "GET" => Verb::Get,
            "DELETE" => Verb::Delete,
            _ => return Err(Refused::Method),
        };
        if verb == Verb::Get
            && let Some(id) = file_of(&head.target)
        {
            return Ok(Admitted::File(id));
        }
        routed(verb, &head.target)
            .map(|target| Admitted::Forward(verb, target))
            .ok_or(Refused::Route)
    }

    async fn filed<S: AsyncWrite + Unpin>(&self, stream: &mut S, id: u64) -> io::Result<()> {
        let Ok(_turn) = self.downloads.acquire().await else {
            return reply(stream, 503, &[], b"").await;
        };
        let hifi = Arc::clone(&self.hifi);
        let opened = spawn_blocking(move || opened_track(&hifi, id)).await;
        match opened {
            Ok(Ok(Some((reader, length)))) => {
                tracing::info!(track = id, "a track is being sent");
                send_file(stream, reader, length).await
            }
            Ok(Ok(None)) => reply(stream, 404, &[], b"").await,
            Ok(Err(error)) => {
                tracing::warn!(track = id, %error, "the hifi-api could not supply the track");
                reply(stream, 502, &[], b"").await
            }
            Err(_) => reply(stream, 502, &[], b"").await,
        }
    }

    async fn relayed(&self, verb: Verb, target: &str) -> std::result::Result<Answer, Unreached> {
        let asked = timeout(UPSTREAM_WITHIN, async {
            let mut stream = TcpStream::connect(self.upstream)
                .await
                .map_err(|_| Unreached::Connect)?;
            let method = match verb {
                Verb::Get => "GET",
                Verb::Delete => "DELETE",
            };
            let request = format!(
                "{method} {target} HTTP/1.0\r\nHost: {}\r\nAccept: application/json\r\n\r\n",
                self.upstream
            );
            stream
                .write_all(request.as_bytes())
                .await
                .map_err(|_| Unreached::Connect)?;
            let mut read = Vec::new();
            (&mut stream)
                .take(ANSWER_BYTES_AT_MOST as u64 + 1)
                .read_to_end(&mut read)
                .await
                .map_err(|_| Unreached::Unreadable)?;
            Ok(read)
        })
        .await
        .map_err(|_| Unreached::Connect)??;
        answered(&asked)
    }
}

struct Head {
    method: String,
    target: String,
    bearer: Option<String>,
}

async fn read_head<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<Option<Head>> {
    let mut buffer = Vec::with_capacity(1024);
    loop {
        let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&buffer) {
            Ok(httparse::Status::Complete(_)) => {
                let bearer = request
                    .headers
                    .iter()
                    .find(|header| header.name.eq_ignore_ascii_case("authorization"))
                    .and_then(|header| std::str::from_utf8(header.value).ok())
                    .and_then(|value| {
                        let scheme = value.get(..BEARER.len())?;
                        scheme
                            .eq_ignore_ascii_case(BEARER)
                            .then(|| value[BEARER.len()..].trim().to_owned())
                    });
                return Ok(Some(Head {
                    method: request.method.unwrap_or_default().to_owned(),
                    target: request.path.unwrap_or_default().to_owned(),
                    bearer,
                }));
            }
            Ok(httparse::Status::Partial) if buffer.len() < HEAD_BYTES_AT_MOST => {}
            Ok(httparse::Status::Partial) | Err(_) => return Ok(None),
        }
        let mut chunk = [0; 1024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn routed(verb: Verb, target: &str) -> Option<String> {
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };
    let allowed = match (verb, path) {
        (Verb::Get, SEARCH_ROUTE) => true,
        (_, path) => path.strip_prefix(REQUESTS_ROUTE).is_some_and(|id| {
            !id.is_empty()
                && id
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || letter == '-')
        }),
    };
    let queried_cleanly =
        query.is_none_or(|query| query.bytes().all(|byte| byte.is_ascii_graphic()));
    (allowed && queried_cleanly).then(|| target.to_owned())
}

fn answered(read: &[u8]) -> std::result::Result<Answer, Unreached> {
    if read.len() > ANSWER_BYTES_AT_MOST {
        return Err(Unreached::TooLarge);
    }
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut response = httparse::Response::new(&mut headers);
    let httparse::Status::Complete(head) =
        response.parse(read).map_err(|_| Unreached::Unreadable)?
    else {
        return Err(Unreached::Unreadable);
    };
    let status = response.code.ok_or(Unreached::Unreadable)?;
    let named = |name: &str| {
        response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(|value| value.trim().to_owned())
    };
    let body = read[head..].to_vec();
    let promised = named("content-length").and_then(|length| length.parse::<usize>().ok());
    if promised.is_some_and(|length| length != body.len()) {
        return Err(Unreached::Unreadable);
    }
    Ok(Answer {
        status,
        content_type: named("content-type").filter(|kind| {
            kind.bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        }),
        retry_after: named("retry-after")
            .filter(|wait| wait.bytes().all(|byte| byte.is_ascii_digit())),
        body,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

async fn reply<S: AsyncWrite + Unpin>(
    stream: &mut S,
    status: u16,
    headers: &[(&str, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        reason(status),
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

type Opening = (Box<dyn Read + Send>, Option<u64>);

fn opened_track(hifi: &HifiApi, id: u64) -> resonate_providers::Result<Option<Opening>> {
    match hifi.track(id)? {
        Some(Delivery::Stream { opening, .. }) => match opening.open()? {
            Opened::Reading { reader, length } => Ok(Some((reader, length))),
            Opened::Gone => Ok(None),
        },
        Some(Delivery::File(_)) | None => Ok(None),
    }
}

fn file_of(target: &str) -> Option<u64> {
    let (path, query) = target.split_once('?')?;
    if path != FILE_ROUTE {
        return None;
    }
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))
        .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|id| id.parse().ok())
}

async fn send_file<S: AsyncWrite + Unpin>(
    stream: &mut S,
    mut reader: Box<dyn Read + Send>,
    length: Option<u64>,
) -> io::Result<()> {
    let (sender, mut receiver) = mpsc::channel::<io::Result<Vec<u8>>>(FILE_CHUNKS_AHEAD);
    spawn_blocking(move || {
        let mut sent = 0_u64;
        loop {
            let mut chunk = vec![0; FILE_CHUNK_BYTES];
            let outcome = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    chunk.truncate(read);
                    sent += read as u64;
                    if sent > FILE_BYTES_AT_MOST {
                        Err(io::Error::from(io::ErrorKind::FileTooLarge))
                    } else {
                        Ok(chunk)
                    }
                }
                Err(error) => Err(error),
            };
            let failed = outcome.is_err();
            if sender.blocking_send(outcome).is_err() || failed {
                break;
            }
        }
    });
    let framing = match length {
        Some(length) => format!("Content-Length: {length}"),
        None => "Transfer-Encoding: chunked".to_owned(),
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {FILE_TYPE}\r\n{framing}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"
    );
    written(stream, head.as_bytes()).await?;
    let mut total = 0_u64;
    while let Some(chunk) = receiver.recv().await {
        let chunk = chunk?;
        total += chunk.len() as u64;
        if length.is_none() {
            written(stream, format!("{:x}\r\n", chunk.len()).as_bytes()).await?;
        }
        written(stream, &chunk).await?;
        if length.is_none() {
            written(stream, b"\r\n").await?;
        }
    }
    if length.is_some_and(|promised| promised != total) {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    if length.is_none() {
        written(stream, b"0\r\n\r\n").await?;
    }
    stream.shutdown().await
}

async fn written<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> io::Result<()> {
    timeout(CHUNK_WRITTEN_WITHIN, stream.write_all(bytes))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}
