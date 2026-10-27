use std::{
    io::{self, Read, Seek, SeekFrom},
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
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::spawn_blocking,
    time::{sleep, timeout},
};
use tokio_rustls::TlsAcceptor;

use crate::{
    Cache, Error, Fill, Hit, Keys, Result, Searches,
    ranges::{Span, Unsatisfiable, Wanted, windowed},
};

const HTTP_ALPN: &[u8] = b"http/1.1";
const HEAD_BYTES_AT_MOST: usize = 8 * 1024;
const HEADERS_AT_MOST: usize = 32;
const ANSWER_BYTES_AT_MOST: usize = 1024 * 1024;
const CONNECTIONS_AT_MOST: usize = 64;
const REQUEST_WITHIN: Duration = Duration::from_secs(10);
const UPSTREAM_WITHIN: Duration = Duration::from_secs(30);
const ACCEPT_RETRY_AFTER: Duration = Duration::from_millis(100);
const DOWNLOADS_AT_ONCE: usize = 1;
const BUSY_RETRY_AFTER_SECONDS: u64 = 5;
const FILE_CHUNK_BYTES: usize = 64 * 1024;
const FILE_CHUNKS_AHEAD: usize = 4;
const FILE_BYTES_AT_MOST: u64 = 1024 * 1024 * 1024;
const CHUNK_WRITTEN_WITHIN: Duration = Duration::from_secs(60);
const FILE_ROUTE: &str = "/file/";
const FILE_TYPE: &str = "audio/flac";
const FLAC_MARKER: &[u8] = b"fLaC";
const SEARCH_BYTES_AT_MOST: usize = 8 * 1024 * 1024;
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
    File { id: u64, wanted: Option<Wanted> },
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

#[derive(Default)]
pub struct Keeping {
    pub tracks: Option<Cache>,
    pub searches_for: Duration,
}

struct Source {
    reader: Box<dyn Read + Send>,
    length: Option<u64>,
    begins_at: u64,
    fill: Option<Fill>,
    _turn: Option<OwnedSemaphorePermit>,
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
    downloads: Arc<Semaphore>,
    tracks: Option<Cache>,
    searches: Searches,
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
        Self::through_keeping(keys, upstream, hifi, Keeping::default())
    }

    pub fn keeping(keys: Keys, upstream: SocketAddr, keeping: Keeping) -> Arc<Self> {
        let hifi = HifiApi::at(&format!("http://{upstream}"));
        Self::through_keeping(keys, upstream, hifi, keeping)
    }

    pub fn through_keeping(
        keys: Keys,
        upstream: SocketAddr,
        hifi: HifiApi,
        keeping: Keeping,
    ) -> Arc<Self> {
        Arc::new(Self {
            keys,
            upstream,
            hifi: Arc::new(hifi),
            seats: Arc::new(Semaphore::new(CONNECTIONS_AT_MOST)),
            downloads: Arc::new(Semaphore::new(DOWNLOADS_AT_ONCE)),
            tracks: keeping.tracks,
            searches: Searches::new(keeping.searches_for, SEARCH_BYTES_AT_MOST),
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
            Ok(Admitted::File { id, wanted }) => self.filed(&mut stream, id, wanted).await,
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
            let wanted = head.range.as_deref().and_then(Wanted::parse);
            return Ok(Admitted::File { id, wanted });
        }
        routed(verb, &head.target)
            .map(|target| Admitted::Forward(verb, target))
            .ok_or(Refused::Route)
    }

    async fn filed<S: AsyncWrite + Unpin>(
        &self,
        stream: &mut S,
        id: u64,
        wanted: Option<Wanted>,
    ) -> io::Result<()> {
        if let Some(hit) = self.kept(id) {
            return from_the_cache(stream, id, hit, wanted).await;
        }
        let Ok(turn) = Arc::clone(&self.downloads).try_acquire_owned() else {
            return busy(stream).await;
        };
        if let Some(hit) = self.kept(id) {
            drop(turn);
            return from_the_cache(stream, id, hit, wanted).await;
        }
        let hifi = Arc::clone(&self.hifi);
        let opened = spawn_blocking(move || (opened_track(&hifi, id), turn)).await;
        match opened {
            Ok((Ok(Some((reader, length))), turn)) => {
                let fill = self
                    .tracks
                    .as_ref()
                    .and_then(|tracks| tracks.fill(id, length));
                tracing::info!(track = id, cached = fill.is_some(), "a track is being sent");
                let source = Source {
                    reader,
                    length,
                    begins_at: 0,
                    fill,
                    _turn: Some(turn),
                };
                match windowed(wanted, length) {
                    Ok(span) => send_file(stream, source, span).await,
                    Err(Unsatisfiable) => unsatisfiable(stream, length.unwrap_or_default()).await,
                }
            }
            Ok((Ok(None), _)) => reply(stream, 404, &[], b"").await,
            Ok((Err(error), _)) => {
                tracing::warn!(track = id, %error, "the hifi-api could not supply the track");
                reply(stream, 502, &[], b"").await
            }
            Err(_) => reply(stream, 502, &[], b"").await,
        }
    }

    fn kept(&self, id: u64) -> Option<Hit> {
        self.tracks.as_ref()?.hit(id)
    }

    async fn relayed(&self, verb: Verb, target: &str) -> std::result::Result<Answer, Unreached> {
        let searching = verb == Verb::Get && is_search(target);
        if searching && let Some(recalled) = self.searches.recalled(target) {
            return Ok(Answer {
                status: 200,
                content_type: recalled.content_type,
                retry_after: None,
                body: recalled.body,
            });
        }
        let answer = self.forwarded(verb, target).await?;
        if searching && answer.status == 200 {
            self.searches
                .remember(target, answer.content_type.as_deref(), &answer.body);
        }
        Ok(answer)
    }

    async fn forwarded(&self, verb: Verb, target: &str) -> std::result::Result<Answer, Unreached> {
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
    range: Option<String>,
}

async fn read_head<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<Option<Head>> {
    let mut buffer = Vec::with_capacity(1024);
    loop {
        let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&buffer) {
            Ok(httparse::Status::Complete(_)) => {
                let named = |name: &str| {
                    request
                        .headers
                        .iter()
                        .find(|header| header.name.eq_ignore_ascii_case(name))
                        .and_then(|header| std::str::from_utf8(header.value).ok())
                };
                let bearer = named("authorization").and_then(|value| {
                    let scheme = value.get(..BEARER.len())?;
                    scheme
                        .eq_ignore_ascii_case(BEARER)
                        .then(|| value[BEARER.len()..].trim().to_owned())
                });
                let range = named("range").map(str::to_owned);
                return Ok(Some(Head {
                    method: request.method.unwrap_or_default().to_owned(),
                    target: request.path.unwrap_or_default().to_owned(),
                    bearer,
                    range,
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

fn is_search(target: &str) -> bool {
    target.split_once('?').map_or(target, |(path, _)| path) == SEARCH_ROUTE
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
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        416 => "Range Not Satisfiable",
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

async fn from_the_cache<S: AsyncWrite + Unpin>(
    stream: &mut S,
    id: u64,
    hit: Hit,
    wanted: Option<Wanted>,
) -> io::Result<()> {
    let Hit { mut file, bytes } = hit;
    let span = match windowed(wanted, Some(bytes)) {
        Ok(span) => span,
        Err(Unsatisfiable) => return unsatisfiable(stream, bytes).await,
    };
    let begins_at = span.map_or(0, |span| span.first);
    file.seek(SeekFrom::Start(begins_at))?;
    tracing::info!(track = id, "a track is being sent from the cache");
    let source = Source {
        reader: Box::new(file),
        length: Some(bytes),
        begins_at,
        fill: None,
        _turn: None,
    };
    send_file(stream, source, span).await
}

async fn unsatisfiable<S: AsyncWrite + Unpin>(stream: &mut S, total: u64) -> io::Result<()> {
    reply(
        stream,
        416,
        &[("Content-Range", format!("bytes */{total}"))],
        b"",
    )
    .await
}

fn pump(source: Source, window: Span, sender: mpsc::Sender<io::Result<Vec<u8>>>) {
    pump_source(source, window, &sender);
}

fn pump_source(mut source: Source, window: Span, sender: &mpsc::Sender<io::Result<Vec<u8>>>) {
    let mut at = source.begins_at;
    let mut attached = true;
    loop {
        let nothing_more_to_send = !attached || at > window.last;
        if nothing_more_to_send && source.fill.is_none() {
            break;
        }
        let mut chunk = vec![0; FILE_CHUNK_BYTES];
        match source.reader.read(&mut chunk) {
            Ok(0) => {
                if let Some(fill) = source.fill.take() {
                    kept_if_whole(fill, source.length, at);
                }
                break;
            }
            Ok(read) => {
                chunk.truncate(read);
                let end = at + read as u64;
                if end > FILE_BYTES_AT_MOST {
                    if attached {
                        let _ =
                            sender.blocking_send(Err(io::Error::from(io::ErrorKind::FileTooLarge)));
                    }
                    break;
                }
                if at == 0 && !chunk.starts_with(FLAC_MARKER) {
                    source.fill = None;
                }
                if let Some(fill) = source.fill.as_mut()
                    && let Err(error) = fill.write(&chunk)
                {
                    tracing::debug!(%error, "a track is no longer being cached");
                    source.fill = None;
                }
                if attached {
                    let from = window.first.saturating_sub(at).min(read as u64) as usize;
                    let to = window
                        .last
                        .saturating_add(1)
                        .saturating_sub(at)
                        .min(read as u64) as usize;
                    if from < to {
                        chunk.truncate(to);
                        chunk.drain(..from);
                        attached = sender.blocking_send(Ok(chunk)).is_ok();
                    }
                }
                at = end;
            }
            Err(error) => {
                if attached {
                    let _ = sender.blocking_send(Err(error));
                }
                break;
            }
        }
    }
}

fn kept_if_whole(fill: Fill, promised: Option<u64>, read: u64) {
    if read < FLAC_MARKER.len() as u64 || promised.is_some_and(|promised| promised != read) {
        return;
    }
    if let Err(error) = fill.commit() {
        tracing::warn!(%error, "a downloaded track could not be put in the cache");
    }
}

async fn busy<S: AsyncWrite + Unpin>(stream: &mut S) -> io::Result<()> {
    reply(
        stream,
        503,
        &[("Retry-After", BUSY_RETRY_AFTER_SECONDS.to_string())],
        b"",
    )
    .await
}

async fn send_file<S: AsyncWrite + Unpin>(
    stream: &mut S,
    source: Source,
    span: Option<Span>,
) -> io::Result<()> {
    let length = source.length;
    let window = span.unwrap_or(Span {
        first: 0,
        last: u64::MAX,
    });
    let promised = match span {
        Some(span) => Some(span.bytes()),
        None => length,
    };
    let (sender, mut receiver) = mpsc::channel::<io::Result<Vec<u8>>>(FILE_CHUNKS_AHEAD);
    spawn_blocking(move || pump(source, window, sender));

    let status = match span {
        Some(_) => "206 Partial Content",
        None => "200 OK",
    };
    let framing = match promised {
        Some(promised) => format!("Content-Length: {promised}"),
        None => "Transfer-Encoding: chunked".to_owned(),
    };
    let ranges = match (length, span) {
        (Some(total), Some(span)) => format!(
            "Accept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{total}\r\n",
            span.first, span.last
        ),
        (Some(_), None) => "Accept-Ranges: bytes\r\n".to_owned(),
        (None, _) => String::new(),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {FILE_TYPE}\r\n{framing}\r\n{ranges}Connection: close\r\nCache-Control: no-store\r\n\r\n"
    );
    written(stream, head.as_bytes()).await?;

    let mut total = 0_u64;
    while let Some(chunk) = receiver.recv().await {
        let chunk = chunk?;
        total += chunk.len() as u64;
        if promised.is_none() {
            written(stream, format!("{:x}\r\n", chunk.len()).as_bytes()).await?;
        }
        written(stream, &chunk).await?;
        if promised.is_none() {
            written(stream, b"\r\n").await?;
        }
    }
    if promised.is_some_and(|promised| promised != total) {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    if promised.is_none() {
        written(stream, b"0\r\n\r\n").await?;
    }
    stream.shutdown().await
}

async fn written<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> io::Result<()> {
    timeout(CHUNK_WRITTEN_WITHIN, stream.write_all(bytes))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}
