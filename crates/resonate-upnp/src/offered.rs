use std::{
    fmt::Write as _,
    hash::{BuildHasher, RandomState},
    io::{self, Read, Seek, SeekFrom, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use mio::Waker;
use resonate_core::{AlbumId, TrackId};

use crate::{
    Error, Result, ServeOp, Udn,
    announce::{Announcer, Announcing},
    catalog::{Catalog, Object, Shelf},
    didl::{COVERS_UNDER, DLNA_FEATURES, SONGS_UNDER},
    directory::{self, Answered, Asking, Service},
    door::{BUSY, Door, Doorway, Judged, Limits},
    events::{self, Kept, Subscribers, Subscribing},
    http::{self, HEADERS_AT_MOST, SERVED_AS},
    naming, neighbours, presented,
    serve::NOT_HERE,
    ssdp::SSDP,
};

const SSDP_PORT: u16 = 1900;

const TOLD_AGAIN_EVERY: Duration = Duration::from_secs(600);

const ANSWERS_AT_MOST: usize = 16;

const HEADS_AT_MOST: usize = 32;

const HEAD_ENDS_WITHIN: Duration = Duration::from_secs(5);

const BODY_READ_WITHIN: Duration = Duration::from_secs(5);

const BODY_BYTES_AT_MOST: usize = 64 << 10;

const WRITTEN_WITHIN: Duration = Duration::from_secs(5);

const A_LISTENER_WAITS_AT_MOST: Duration = Duration::from_secs(120);

const COPIED_IN: usize = 64 << 10;

const UUID_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

const UUID_PREFIX: &str = "uuid:";

const NOT_ALLOWED: &[u8] = b"HTTP/1.1 405 Method Not Allowed\r\nAllow: GET, HEAD, POST, \
      SUBSCRIBE, UNSUBSCRIBE\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const MALFORMED: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const UNMET: &[u8] =
    b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const ENDED: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const REVISION_READ_EVERY: Duration = Duration::from_secs(1);

const EVENTS_WAIT_AT_MOST: Duration = Duration::from_millis(250);

const TOO_LARGE: &[u8] =
    b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const LENGTH_WANTED: &[u8] =
    b"HTTP/1.1 411 Length Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const FAILED: &[u8] =
    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

const XML: &str = "text/xml; charset=\"utf-8\"";

const SHIPPED: Limits = Limits {
    answers_at_most: ANSWERS_AT_MOST,
    heads_at_most: HEADS_AT_MOST,
    head_ends_within: HEAD_ENDS_WITHIN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offering {
    pub heard_on: SocketAddr,
    pub told: SocketAddr,
    pub every: Duration,
    pub serving_on: IpAddr,
}

impl Offering {
    pub const fn on_the_network() -> Self {
        Self {
            heard_on: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, SSDP_PORT)),
            told: SSDP,
            every: TOLD_AGAIN_EVERY,
            serving_on: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }
}

impl Udn {
    pub fn minted() -> Self {
        let keys = RandomState::new();
        let high = keys.hash_one(0_u8).to_be_bytes();
        let low = keys.hash_one(1_u8).to_be_bytes();
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&high);
        bytes[8..].copy_from_slice(&low);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;

        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut written = String::from(UUID_PREFIX);
        let mut at = 0;
        for (group, length) in UUID_GROUPS.into_iter().enumerate() {
            if group > 0 {
                written.push('-');
            }
            written.push_str(&hex[at..at + length]);
            at += length;
        }
        Self::new(written)
    }

    pub fn read(text: &str) -> Option<Self> {
        let text = text.trim();
        let groups: Vec<&str> = text.strip_prefix(UUID_PREFIX)?.split('-').collect();
        let well_formed = groups.len() == UUID_GROUPS.len()
            && groups.iter().zip(UUID_GROUPS).all(|(group, length)| {
                group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        well_formed.then(|| Self::new(text))
    }
}

pub struct MediaServer<C: Catalog> {
    announcer: Announcer,
    hall: Arc<Hall<C>>,
    port: u16,
}

impl<C: Catalog> MediaServer<C> {
    pub fn start(catalog: Arc<C>, udn: Udn, offering: Offering) -> Result<Self> {
        let (doorway, opened) = Doorway::bind(offering.serving_on)?;
        let (told, heard) = crossbeam_channel::bounded(1);
        let shared = Arc::new(Shared {
            catalog,
            udn: udn.clone(),
            stopped: AtomicBool::new(false),
            subscribers: Subscribers::default(),
            told,
        });
        let evented = Arc::downgrade(&shared);
        thread::Builder::new()
            .name("resonate-upnp-events".to_owned())
            .spawn(move || tell_each_change(&evented, &heard))
            .map_err(|source| Error::Serve {
                op: ServeOp::Listen,
                source,
            })?;
        let hall = Arc::new(Hall {
            shared,
            waker: opened.waker,
        });
        doorway.open(Arc::downgrade(&hall), SHIPPED, "resonate-upnp-offer")?;
        let announcer = Announcer::start(Announcing {
            heard_on: offering.heard_on,
            told: offering.told,
            every: offering.every,
            serving_on: offering.serving_on,
            port: opened.port,
            udn,
        })?;
        Ok(Self {
            announcer,
            hall,
            port: opened.port,
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub const fn heard_at(&self) -> SocketAddr {
        self.announcer.heard_at()
    }

    pub fn udn(&self) -> &Udn {
        &self.hall.shared.udn
    }
}

impl<C: Catalog> Drop for MediaServer<C> {
    fn drop(&mut self) {
        self.hall.shared.stopped.store(true, Ordering::Release);
    }
}

struct Shared<C> {
    catalog: Arc<C>,
    udn: Udn,
    stopped: AtomicBool,
    subscribers: Subscribers,
    told: Sender<()>,
}

fn tell_each_change<C: Catalog>(shared: &Weak<Shared<C>>, heard: &Receiver<()>) {
    let mut read_at: Option<Instant> = None;
    let mut revision = 0;
    loop {
        match heard.recv_timeout(EVENTS_WAIT_AT_MOST) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let Some(shared) = shared.upgrade() else {
            return;
        };
        if shared.stopped.load(Ordering::Acquire) {
            return;
        }
        if read_at.is_none_or(|read_at| read_at.elapsed() >= REVISION_READ_EVERY) {
            revision = shared.catalog.revision();
            read_at = Some(Instant::now());
        }
        for telling in shared.subscribers.due(revision) {
            if shared.stopped.load(Ordering::Acquire) {
                return;
            }
            shared.subscribers.settle(&telling, events::tell(&telling));
        }
    }
}

struct Hall<C> {
    shared: Arc<Shared<C>>,
    waker: Waker,
}

impl<C> Drop for Hall<C> {
    fn drop(&mut self) {
        if let Err(error) = self.waker.wake() {
            tracing::debug!(%error, "the media server's thread could not be woken to end");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Description,
    Scpd(Service),
    Control(Service),
    Event(Service),
    Song(TrackId),
    Cover(AlbumId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Head,
    Post,
    Subscribe,
    Unsubscribe,
}

pub(crate) enum Routed {
    Unreadable,
    NotAllowed,
    NotHere,
    To(Route, Method),
}

pub(crate) fn routed(held: &[u8]) -> Routed {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(held) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) | Err(_) => return Routed::Unreadable,
    }
    let method = match request.method.unwrap_or_default() {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "SUBSCRIBE" => Method::Subscribe,
        "UNSUBSCRIBE" => Method::Unsubscribe,
        _ => return Routed::NotAllowed,
    };
    let Some(route) = request.path.and_then(route_of) else {
        return Routed::NotHere;
    };
    let allowed = match route {
        Route::Control(_) => method == Method::Post,
        Route::Event(_) => matches!(method, Method::Subscribe | Method::Unsubscribe),
        Route::Description | Route::Scpd(_) | Route::Song(_) | Route::Cover(_) => {
            matches!(method, Method::Get | Method::Head)
        }
    };
    if !allowed {
        return Routed::NotAllowed;
    }
    Routed::To(route, method)
}

pub(crate) fn route_of(path: &str) -> Option<Route> {
    let path = path.split(['?', '#']).next()?;
    if path == presented::DESCRIBED_AT {
        return Some(Route::Description);
    }
    for service in Service::ALL {
        if path == presented::description_path(service) {
            return Some(Route::Scpd(service));
        }
        if path == presented::control_path(service) {
            return Some(Route::Control(service));
        }
        if path == presented::event_path(service) {
            return Some(Route::Event(service));
        }
    }
    if let Some(named) = path.strip_prefix(SONGS_UNDER) {
        let (song, extension) = named.split_once('.')?;
        let well_formed =
            !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric());
        return match (well_formed, Object::read(song)?) {
            (true, Object::Song(id)) => Some(Route::Song(id)),
            _ => None,
        };
    }
    match Object::read(path.strip_prefix(COVERS_UNDER)?)? {
        Object::Shelf(Shelf::Album(album)) => Some(Route::Cover(album)),
        _ => None,
    }
}

pub(crate) struct Task<C> {
    shared: Arc<Shared<C>>,
    route: Route,
    method: Method,
}

impl<C: Catalog> Door for Hall<C> {
    type Task = Task<C>;

    const ANSWERED_ON: &'static str = "resonate-upnp-offered";

    fn admits(&self, peer: IpAddr) -> bool {
        neighbours::is_a_neighbour(peer)
    }

    fn judged(&self, held: &[u8]) -> Judged<Task<C>> {
        match routed(held) {
            Routed::Unreadable => Judged::Unreadable,
            Routed::NotAllowed => Judged::Refused(NOT_ALLOWED),
            Routed::NotHere => Judged::Refused(NOT_HERE),
            Routed::To(route, method) => Judged::Taken(Task {
                shared: Arc::clone(&self.shared),
                route,
                method,
            }),
        }
    }

    fn answer(task: Task<C>, mut stream: TcpStream, held: Vec<u8>) -> io::Result<()> {
        let head_only = task.method == Method::Head;
        let shared = &task.shared;
        stream.set_write_timeout(Some(WRITTEN_WITHIN))?;
        match task.route {
            Route::Description => {
                let described = presented::device(&shared.udn, naming::server_name());
                said(&mut stream, "200 OK", XML, described.as_bytes(), head_only)
            }
            Route::Scpd(service) => {
                let described = presented::service(service);
                said(&mut stream, "200 OK", XML, described.as_bytes(), head_only)
            }
            Route::Control(service) => controlled(shared, service, stream, held),
            Route::Event(service) => subscribed(shared, service, task.method, stream, &held),
            Route::Song(id) => match shared.catalog.bytes_of(id) {
                Ok(Some(opened)) => {
                    let range = header_in(&held, "Range");
                    poured(
                        stream,
                        opened.bytes,
                        opened.kind.mime(),
                        range.as_deref(),
                        head_only,
                        &shared.stopped,
                    )
                }
                Ok(None) => stream.write_all(NOT_HERE),
                Err(error) => {
                    tracing::warn!(%error, song = id.get(), "a song asked for could not be opened");
                    stream.write_all(FAILED)
                }
            },
            Route::Cover(album) => match shared.catalog.cover(album) {
                Ok(Some(picture)) => said(
                    &mut stream,
                    "200 OK",
                    picture.mime,
                    &picture.bytes,
                    head_only,
                ),
                Ok(None) => stream.write_all(NOT_HERE),
                Err(error) => {
                    tracing::warn!(%error, album = album.get(), "a cover asked for could not be read");
                    stream.write_all(FAILED)
                }
            },
        }
    }
}

fn header_in(held: &[u8], name: &str) -> Option<String> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut request = httparse::Request::new(&mut headers);
    request.parse(held).ok()?;
    request
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .and_then(|header| std::str::from_utf8(header.value).ok())
        .map(|value| value.trim().to_owned())
}

fn said(
    stream: &mut TcpStream,
    status: &str,
    kind: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nServer: \
         {SERVED_AS}\r\nEXT:\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if head_only {
        return Ok(());
    }
    stream.write_all(body)
}

fn subscribed<C: Catalog>(
    shared: &Shared<C>,
    service: Service,
    method: Method,
    mut stream: TcpStream,
    held: &[u8],
) -> io::Result<()> {
    let method = match method {
        Method::Unsubscribe => events::Method::Unsubscribe,
        Method::Get | Method::Head | Method::Post | Method::Subscribe => events::Method::Subscribe,
    };
    let peer = stream.peer_addr()?.ip();
    let subscribers = &shared.subscribers;
    let (sid, kept) = match events::subscribing(held, method, peer) {
        Subscribing::Malformed => return stream.write_all(MALFORMED),
        Subscribing::Unmet => return stream.write_all(UNMET),
        Subscribing::Ended { sid } => {
            if !subscribers.end(service, &sid) {
                return stream.write_all(UNMET);
            }
            return stream.write_all(ENDED);
        }
        Subscribing::Renewed { sid, kept } => match subscribers.renew(service, &sid, kept) {
            Kept::Granted(kept) => (sid, kept),
            Kept::Unknown => return stream.write_all(UNMET),
        },
        Subscribing::New { callback, kept } => {
            match subscribers.subscribe(service, callback, kept) {
                Some(sid) => (sid, kept),
                None => return stream.write_all(BUSY),
            }
        }
    };
    let answered = write_subscribed(&mut stream, &sid, kept);
    subscribers.answered(&sid);
    let _ = shared.told.try_send(());
    answered
}

fn write_subscribed(stream: &mut TcpStream, sid: &str, kept: Duration) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 200 OK\r\nSID: {sid}\r\nTIMEOUT: Second-{}\r\nServer: {SERVED_AS}\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n",
        kept.as_secs()
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

fn controlled<C: Catalog>(
    shared: &Shared<C>,
    service: Service,
    mut stream: TcpStream,
    held: Vec<u8>,
) -> io::Result<()> {
    let Some(length) = header_in(&held, "Content-Length").and_then(|length| length.parse().ok())
    else {
        return stream.write_all(LENGTH_WANTED);
    };
    if length > BODY_BYTES_AT_MOST {
        return stream.write_all(TOO_LARGE);
    }
    let Some(head_ends) = http::head_end(&held) else {
        return stream.write_all(FAILED);
    };
    let mut body = held.get(head_ends..).unwrap_or_default().to_vec();
    let deadline = Instant::now() + BODY_READ_WITHIN;
    while body.len() < length {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(left))?;
        if http::read_more(&mut stream, &mut body)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    }
    body.truncate(length);

    let base = match stream.local_addr()? {
        SocketAddr::V4(at) => format!("http://{at}"),
        SocketAddr::V6(at) => format!("http://[{}]:{}", at.ip(), at.port()),
    };
    let asking = Asking {
        catalog: shared.catalog.as_ref(),
        base: &base,
    };
    let answered = std::str::from_utf8(&body)
        .ok()
        .and_then(|body| directory::called(body, service))
        .map_or(
            Answered::Faulted(directory::Fault::InvalidAction),
            |called| asking.answer(service, &called),
        );
    match answered {
        Answered::Said(envelope) => said(&mut stream, "200 OK", XML, envelope.as_bytes(), false),
        Answered::Faulted(fault) => {
            let envelope = directory::fault_envelope(fault);
            said(
                &mut stream,
                "500 Internal Server Error",
                XML,
                envelope.as_bytes(),
                false,
            )
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wanted {
    Whole,
    Part { from: u64, to: u64 },
    Unsatisfiable,
}

pub(crate) fn wanted(range: Option<&str>, length: u64) -> Wanted {
    let Some(asked) = range
        .and_then(|range| range.trim().strip_prefix("bytes="))
        .filter(|asked| !asked.contains(','))
    else {
        return Wanted::Whole;
    };
    let Some((from, to)) = asked.trim().split_once('-') else {
        return Wanted::Whole;
    };
    let read = |figure: &str| -> Option<Option<u64>> {
        let figure = figure.trim();
        if figure.is_empty() {
            return Some(None);
        }
        figure
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| figure.parse().ok().map(Some))
            .flatten()
    };
    let (Some(from), Some(to)) = (read(from), read(to)) else {
        return Wanted::Whole;
    };
    let last = match length.checked_sub(1) {
        Some(last) => last,
        None => return Wanted::Unsatisfiable,
    };
    match (from, to) {
        (Some(from), _) if from > last => Wanted::Unsatisfiable,
        (Some(from), Some(to)) if to < from => Wanted::Whole,
        (Some(from), to) => Wanted::Part {
            from,
            to: to.map_or(last, |to| to.min(last)),
        },
        (None, Some(0)) => Wanted::Unsatisfiable,
        (None, Some(suffix)) => Wanted::Part {
            from: length.saturating_sub(suffix),
            to: last,
        },
        (None, None) => Wanted::Whole,
    }
}

fn poured(
    mut stream: TcpStream,
    mut bytes: Box<dyn crate::catalog::Bytes>,
    kind: &str,
    range: Option<&str>,
    head_only: bool,
    stopped: &AtomicBool,
) -> io::Result<()> {
    let length = bytes.seek(SeekFrom::End(0))?;
    let (status, from, to) = match wanted(range, length) {
        Wanted::Whole => ("200 OK", 0, length.checked_sub(1)),
        Wanted::Part { from, to } => ("206 Partial Content", from, Some(to)),
        Wanted::Unsatisfiable => {
            let refusal = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return stream.write_all(refusal.as_bytes());
        }
    };
    let carried = to.map_or(0, |to| to - from + 1);

    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {carried}\r\n\
         Accept-Ranges: bytes\r\ncontentFeatures.dlna.org: {DLNA_FEATURES}\r\n\
         transferMode.dlna.org: Streaming\r\nServer: {SERVED_AS}\r\nConnection: close\r\n"
    );
    if let (Wanted::Part { .. }, Some(to)) = (wanted(range, length), to) {
        let _ = write!(head, "Content-Range: bytes {from}-{to}/{length}\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if head_only || carried == 0 {
        return Ok(());
    }

    stream.set_write_timeout(Some(A_LISTENER_WAITS_AT_MOST))?;
    bytes.seek(SeekFrom::Start(from))?;
    let mut left = carried;
    let mut buffer = vec![0; COPIED_IN];
    while left > 0 {
        if stopped.load(Ordering::Acquire) {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        let wanted_now = usize::try_from(left).unwrap_or(usize::MAX).min(COPIED_IN);
        let read = bytes.read(&mut buffer[..wanted_now])?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        stream.write_all(&buffer[..read])?;
        left -= read as u64;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_udn_reads_back_and_a_malformed_one_does_not() {
        let minted = Udn::minted();

        assert_eq!(Udn::read(minted.as_str()), Some(minted.clone()));
        assert_ne!(Udn::minted(), minted);
        assert_eq!(minted.as_str().len(), UUID_PREFIX.len() + 36);
        for unread in [
            "",
            "uuid:",
            "4c1b3a27-0000-4000-8000-000000000001",
            "uuid:4c1b3a27-0000-4000-8000-00000000000g",
            "uuid:4c1b3a27-0000-4000-8000-0000000000001",
            "uuid:4c1b3a27-0000-4000-8000",
        ] {
            assert_eq!(Udn::read(unread), None, "{unread:?}");
        }
    }

    #[test]
    fn a_range_is_read_as_one_span_of_the_file_and_anything_else_asks_for_the_whole() {
        assert_eq!(wanted(None, 100), Wanted::Whole);
        assert_eq!(
            wanted(Some("bytes=0-9"), 100),
            Wanted::Part { from: 0, to: 9 }
        );
        assert_eq!(
            wanted(Some("bytes=90-"), 100),
            Wanted::Part { from: 90, to: 99 }
        );
        assert_eq!(
            wanted(Some("bytes=-10"), 100),
            Wanted::Part { from: 90, to: 99 }
        );
        assert_eq!(
            wanted(Some("bytes=-1000"), 100),
            Wanted::Part { from: 0, to: 99 }
        );
        assert_eq!(
            wanted(Some("bytes=50-1000"), 100),
            Wanted::Part { from: 50, to: 99 }
        );
        assert_eq!(wanted(Some("bytes=100-"), 100), Wanted::Unsatisfiable);
        assert_eq!(wanted(Some("bytes=0-"), 0), Wanted::Unsatisfiable);
        assert_eq!(wanted(Some("bytes=-0"), 100), Wanted::Unsatisfiable);
        assert_eq!(wanted(Some("bytes=0-1,5-6"), 100), Wanted::Whole);
        assert_eq!(wanted(Some("bytes=9-0"), 100), Wanted::Whole);
        assert_eq!(wanted(Some("bytes=+1-2"), 100), Wanted::Whole);
        assert_eq!(wanted(Some("items=0-1"), 100), Wanted::Whole);
    }

    #[test]
    fn a_path_is_read_only_as_a_route_the_server_minted() {
        let song = TrackId::new(9).expect("an id");
        let album = AlbumId::new(4).expect("an id");

        assert_eq!(route_of("/description.xml"), Some(Route::Description));
        assert_eq!(
            route_of("/control/ContentDirectory"),
            Some(Route::Control(Service::ContentDirectory))
        );
        assert_eq!(
            route_of("/ConnectionManager.xml"),
            Some(Route::Scpd(Service::ConnectionManager))
        );
        assert_eq!(
            route_of("/event/ContentDirectory"),
            Some(Route::Event(Service::ContentDirectory))
        );
        assert_eq!(route_of("/resonate/song-9.flac"), Some(Route::Song(song)));
        assert_eq!(
            route_of("/resonate/song-9.flac?x=1"),
            Some(Route::Song(song))
        );
        assert_eq!(route_of("/cover/album-4"), Some(Route::Cover(album)));
        for refused in [
            "/",
            "/resonate/song-9",
            "/resonate/song-09.flac",
            "/resonate/album-4.flac",
            "/resonate/../../etc/passwd",
            "/resonate//home/listener/a.flac",
            "/resonate/song-9.fl/ac",
            "/cover/song-9",
            "/etc/passwd",
            "/control/AVTransport",
            "/event/AVTransport",
        ] {
            assert_eq!(route_of(refused), None, "{refused}");
        }
    }
}
