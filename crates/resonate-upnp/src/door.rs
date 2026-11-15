use std::{
    collections::BTreeMap,
    io::{self, Write},
    net::{self, IpAddr, Shutdown, SocketAddr},
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use mio::{
    Events, Interest, Poll, Registry, Token as Source, Waker,
    net::{TcpListener, TcpStream},
};

use crate::{
    Error, Result, ServeOp,
    http::{self, HEAD_BYTES_AT_MOST},
};

const LISTENER: Source = Source(0);

const WAKER: Source = Source(1);

const FIRST_HEAD: usize = 2;

const EVENTS_AT_MOST: usize = 64;

const REFUSAL_WRITTEN_WITHIN: Duration = Duration::from_secs(1);

pub(crate) const BUSY: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub answers_at_most: usize,
    pub heads_at_most: usize,
    pub head_ends_within: Duration,
}

pub(crate) enum Judged<Task> {
    Unreadable,
    Refused(&'static [u8]),
    Taken(Task),
}

pub(crate) trait Door: Send + Sync + 'static {
    type Task: Send + 'static;

    const ANSWERED_ON: &'static str;

    fn admits(&self, peer: IpAddr) -> bool;

    fn judged(&self, held: &[u8]) -> Judged<Self::Task>;

    fn answer(task: Self::Task, stream: net::TcpStream, held: Vec<u8>) -> io::Result<()>;

    fn polled(&self) {}
}

pub(crate) struct Opened {
    pub port: u16,
    pub waker: Waker,
}

pub(crate) struct Doorway {
    poll: Poll,
    listener: TcpListener,
}

impl Doorway {
    pub(crate) fn bind(serving_on: IpAddr) -> Result<(Self, Opened)> {
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
        Ok((Self { poll, listener }, Opened { port, waker }))
    }

    pub(crate) fn open<D: Door>(self, door: Weak<D>, limits: Limits, named: &str) -> Result<()> {
        thread::Builder::new()
            .name(named.to_owned())
            .spawn(move || accept(self.poll, &self.listener, &door, limits))
            .map(drop)
            .map_err(|source| Error::Serve {
                op: ServeOp::Listen,
                source,
            })
    }
}

struct Answering(Arc<AtomicUsize>);

impl Answering {
    fn begin(count: &Arc<AtomicUsize>, at_most: usize) -> Option<Self> {
        let before = count.fetch_add(1, Ordering::AcqRel);
        if before >= at_most {
            count.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self(Arc::clone(count)))
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Pending {
    stream: TcpStream,
    held: Vec<u8>,
    since: Instant,
}

enum Next<Task> {
    Waiting,
    Dropped,
    Refused(&'static [u8]),
    Taken(Task),
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

    fn advance<D: Door>(&mut self, door: &D, head_ends_within: Duration) -> Next<D::Task> {
        loop {
            if http::head_end(&self.held).is_some() {
                return match door.judged(&self.held) {
                    Judged::Unreadable => Next::Dropped,
                    Judged::Refused(refusal) => Next::Refused(refusal),
                    Judged::Taken(task) => Next::Taken(task),
                };
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

    fn handed_over(mut self, registry: &Registry) -> io::Result<(net::TcpStream, Vec<u8>)> {
        registry.deregister(&mut self.stream)?;
        Ok((net::TcpStream::from(self.stream), self.held))
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
            tracing::debug!(%error, "a connection could not be read");
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

fn admitted<D: Door>(
    listener: &TcpListener,
    registry: &Registry,
    heads: &mut Heads,
    door: &D,
    heads_at_most: usize,
) -> Option<Vec<Source>> {
    let mut admitted = Vec::new();
    loop {
        match listener.accept() {
            Ok((stream, peer)) if heads.waiting.len() < heads_at_most && door.admits(peer.ip()) => {
                admitted.extend(heads.admit(registry, stream));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Some(admitted),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(%error, "a server stopped accepting connections");
                return None;
            }
        }
    }
}

fn accept<D: Door>(mut poll: Poll, listener: &TcpListener, held: &Weak<D>, limits: Limits) {
    let answering = Arc::new(AtomicUsize::new(0));
    let mut heads = Heads::new();
    let mut events = Events::with_capacity(EVENTS_AT_MOST);
    loop {
        let timeout = heads.nearest_deadline(limits.head_ends_within);
        match poll.poll(&mut events, timeout) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                tracing::warn!(%error, "a server stopped waiting on connections");
                return;
            }
        }
        let Some(door) = held.upgrade() else {
            return;
        };
        door.polled();

        let mut ready: Vec<Source> = Vec::new();
        for event in &events {
            match event.token() {
                LISTENER => {
                    let Some(admitted) = admitted(
                        listener,
                        poll.registry(),
                        &mut heads,
                        door.as_ref(),
                        limits.heads_at_most,
                    ) else {
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
            match asking.advance(door.as_ref(), limits.head_ends_within) {
                Next::Waiting => {
                    heads.waiting.insert(source, asking);
                }
                Next::Dropped => asking.let_go(poll.registry()),
                Next::Refused(refusal) => {
                    handed(asking, &poll, |stream, _| refuse(stream, refusal))
                }
                Next::Taken(task) => handed(asking, &poll, |stream, held| {
                    match Answering::begin(&answering, limits.answers_at_most) {
                        Some(slot) => begin_answer::<D>(stream, task, held, slot),
                        None => refuse(stream, BUSY),
                    }
                }),
            }
        }
    }
}

fn handed(asking: Pending, poll: &Poll, then: impl FnOnce(net::TcpStream, Vec<u8>)) {
    match asking.handed_over(poll.registry()) {
        Ok((stream, held)) => then(stream, held),
        Err(error) => tracing::debug!(%error, "a connection could not be taken over"),
    }
}

pub(crate) fn refuse(mut stream: net::TcpStream, refusal: &[u8]) {
    let written = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_write_timeout(Some(REFUSAL_WRITTEN_WITHIN)))
        .and_then(|()| stream.write_all(refusal));
    if let Err(error) = written {
        tracing::debug!(%error, "a refusal could not be written");
    }
}

fn begin_answer<D: Door>(stream: net::TcpStream, task: D::Task, held: Vec<u8>, slot: Answering) {
    let spawned = thread::Builder::new()
        .name(D::ANSWERED_ON.to_owned())
        .spawn(move || {
            let _slot = slot;
            let answered = stream
                .try_clone()
                .and_then(|kept| stream.set_nonblocking(false).map(|()| kept))
                .and_then(|kept| {
                    let answered = D::answer(task, stream, held);
                    let _ = kept.shutdown(Shutdown::Both);
                    answered
                });
            if let Err(error) = answered {
                tracing::debug!(%error, "an answer ended");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a request could not be answered");
    }
}
