use std::{
    collections::{BTreeMap, HashMap, hash_map::Entry},
    io::{self, Write},
    net::{self, IpAddr, Shutdown, SocketAddr},
    sync::{Arc, Weak},
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
    http::{self, HEAD_BYTES_AT_MOST},
};

const LISTENER: Source = Source(0);

const WAKER: Source = Source(1);

const FIRST_HEAD: usize = 2;

const EVENTS_AT_MOST: usize = 64;

const REFUSAL_WRITTEN_WITHIN: Duration = Duration::from_secs(1);

const ACCEPT_RETRIED_AFTER: Duration = Duration::from_millis(100);

pub(crate) const BUSY: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub answers_at_most: usize,
    pub answers_a_peer_holds_at_most: usize,
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

#[derive(Default)]
struct Slots {
    taken: usize,
    by_peer: HashMap<IpAddr, usize>,
}

struct Answering {
    slots: Arc<Mutex<Slots>>,
    peer: IpAddr,
}

impl Answering {
    fn begin(slots: &Arc<Mutex<Slots>>, peer: IpAddr, limits: &Limits) -> Option<Self> {
        let peer = peer.to_canonical();
        let mut held = slots.lock();
        let peer_holds = held.by_peer.get(&peer).copied().unwrap_or(0);
        if held.taken >= limits.answers_at_most || peer_holds >= limits.answers_a_peer_holds_at_most
        {
            return None;
        }
        held.taken += 1;
        *held.by_peer.entry(peer).or_insert(0) += 1;
        Some(Self {
            slots: Arc::clone(slots),
            peer,
        })
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        let mut held = self.slots.lock();
        held.taken = held.taken.saturating_sub(1);
        if let Entry::Occupied(mut holding) = held.by_peer.entry(self.peer) {
            *holding.get_mut() = holding.get().saturating_sub(1);
            if *holding.get() == 0 {
                holding.remove();
            }
        }
    }
}

struct Pending {
    stream: TcpStream,
    peer: IpAddr,
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
    fn new(stream: TcpStream, peer: IpAddr) -> Self {
        Self {
            stream,
            peer,
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

    fn admit(
        &mut self,
        registry: &Registry,
        mut stream: TcpStream,
        peer: IpAddr,
    ) -> Option<Source> {
        let source = Source(self.next);
        if let Err(error) = registry.register(&mut stream, source, Interest::READABLE) {
            tracing::debug!(%error, "a connection could not be read");
            return None;
        }
        self.next = self.next.wrapping_add(1).max(FIRST_HEAD);
        self.waiting.insert(source, Pending::new(stream, peer));
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Accepted {
    Skipped,
    Starved,
}

fn accepted_after(error: &io::Error) -> Accepted {
    match error.kind() {
        io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionRefused
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::NotConnected
        | io::ErrorKind::TimedOut
        | io::ErrorKind::UnexpectedEof => Accepted::Skipped,
        _ => Accepted::Starved,
    }
}

struct Admission {
    sources: Vec<Source>,
    starved: bool,
}

fn admitted<D: Door>(
    listener: &TcpListener,
    registry: &Registry,
    heads: &mut Heads,
    door: &D,
    heads_at_most: usize,
) -> Admission {
    let mut sources = Vec::new();
    loop {
        match listener.accept() {
            Ok((stream, peer)) if heads.waiting.len() < heads_at_most && door.admits(peer.ip()) => {
                sources.extend(heads.admit(registry, stream, peer.ip()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Admission {
                    sources,
                    starved: false,
                };
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => match accepted_after(&error) {
                Accepted::Skipped => {
                    tracing::debug!(%error, "a connection ended before it was accepted");
                }
                Accepted::Starved => {
                    tracing::warn!(%error, "a server could not accept a connection, so it tries again shortly");
                    return Admission {
                        sources,
                        starved: true,
                    };
                }
            },
        }
    }
}

fn accept<D: Door>(mut poll: Poll, listener: &TcpListener, held: &Weak<D>, limits: Limits) {
    let answering = Arc::new(Mutex::new(Slots::default()));
    let mut heads = Heads::new();
    let mut events = Events::with_capacity(EVENTS_AT_MOST);
    let mut retry_at: Option<Instant> = None;
    loop {
        let timeout = [
            heads.nearest_deadline(limits.head_ends_within),
            retry_at.map(|at| at.saturating_duration_since(Instant::now())),
        ]
        .into_iter()
        .flatten()
        .min();
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
        let mut accepting = retry_at.is_some_and(|at| Instant::now() >= at);
        for event in &events {
            match event.token() {
                LISTENER => accepting = true,
                WAKER => {}
                source => ready.push(source),
            }
        }
        if accepting {
            let admission = admitted(
                listener,
                poll.registry(),
                &mut heads,
                door.as_ref(),
                limits.heads_at_most,
            );
            retry_at = admission
                .starved
                .then(|| Instant::now() + ACCEPT_RETRIED_AFTER);
            ready.extend(admission.sources);
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
                Next::Taken(task) => {
                    let peer = asking.peer;
                    handed(asking, &poll, |stream, held| {
                        match Answering::begin(&answering, peer, &limits) {
                            Some(slot) => begin_answer::<D>(stream, task, held, slot),
                            None => refuse(stream, BUSY),
                        }
                    });
                }
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

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const SHARED: Limits = Limits {
        answers_at_most: 4,
        answers_a_peer_holds_at_most: 2,
        heads_at_most: 8,
        head_ends_within: Duration::from_secs(5),
    };

    fn peer(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, last])
    }

    #[test]
    fn a_connection_that_ended_before_it_was_accepted_is_skipped_and_anything_else_is_retried() {
        let skipped = [
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::NotConnected,
            io::ErrorKind::TimedOut,
            io::ErrorKind::UnexpectedEof,
        ];
        let starved = [
            io::ErrorKind::OutOfMemory,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
        ];

        for kind in skipped {
            assert_eq!(accepted_after(&io::Error::from(kind)), Accepted::Skipped);
        }
        for kind in starved {
            assert_eq!(accepted_after(&io::Error::from(kind)), Accepted::Starved);
        }
        assert_eq!(
            accepted_after(&io::Error::other("no file descriptors left")),
            Accepted::Starved
        );
    }

    #[cfg(unix)]
    #[test]
    fn running_out_of_file_descriptors_is_retried_not_the_end_of_the_server() {
        const TOO_MANY_OPEN_FILES: i32 = 24;

        assert_eq!(
            accepted_after(&io::Error::from_raw_os_error(TOO_MANY_OPEN_FILES)),
            Accepted::Starved
        );
    }

    #[test]
    fn one_peer_holds_no_more_than_its_share_of_the_answers_and_the_rest_stay_for_others() {
        let slots = Arc::new(Mutex::new(Slots::default()));

        let first = Answering::begin(&slots, peer(1), &SHARED);
        let second = Answering::begin(&slots, peer(1), &SHARED);
        let past_its_share = Answering::begin(&slots, peer(1), &SHARED);
        let mapped = Answering::begin(
            &slots,
            IpAddr::V6(Ipv4Addr::new(192, 168, 1, 1).to_ipv6_mapped()),
            &SHARED,
        );
        let another = Answering::begin(&slots, peer(2), &SHARED);

        assert!(first.is_some() && second.is_some());
        assert!(past_its_share.is_none());
        assert!(mapped.is_none(), "a mapped address escaped its share");
        assert!(another.is_some());
        drop(first);
        assert!(Answering::begin(&slots, peer(1), &SHARED).is_some());
    }

    #[test]
    fn every_peer_together_holds_no_more_than_the_answers_at_most() {
        let slots = Arc::new(Mutex::new(Slots::default()));

        let held: Vec<_> = (1..=4)
            .map(|last| Answering::begin(&slots, peer(last), &SHARED))
            .collect();

        assert!(held.iter().all(Option::is_some));
        assert!(Answering::begin(&slots, peer(5), &SHARED).is_none());
        drop(held);
        assert!(slots.lock().by_peer.is_empty());
        assert_eq!(slots.lock().taken, 0);
    }
}
