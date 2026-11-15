use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use socket2::{Domain, Protocol, SockRef, Socket, Type};

use crate::{
    Error, Result, Udn,
    directory::{CONNECTION_MANAGER, CONTENT_DIRECTORY},
    http::{HEADERS_AT_MOST, SERVED_AS},
    neighbours,
    presented::{DESCRIBED_AT, SERVER_TYPE},
    serve::host_toward,
    ssdp::SSDP,
};

const ALIVE_FOR: Duration = Duration::from_secs(1800);

const TOLD_TIMES: usize = 2;

const HOPS: u32 = 2;

const SEARCH_BYTES_AT_MOST: usize = 8 << 10;

const STOP_HEARD_WITHIN: Duration = Duration::from_millis(250);

const ANSWERS_A_SECOND_AT_MOST: u32 = 100;

const ROOT_DEVICE: &str = "upnp:rootdevice";

const EVERYTHING: &str = "ssdp:all";

const DISCOVER: &str = "\"ssdp:discover\"";

pub(crate) struct Announcing {
    pub heard_on: SocketAddr,
    pub told: SocketAddr,
    pub every: Duration,
    pub serving_on: IpAddr,
    pub port: u16,
    pub udn: Udn,
}

pub(crate) struct Announcer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    heard_at: SocketAddr,
}

impl Announcer {
    pub(crate) fn start(announcing: Announcing) -> Result<Self> {
        let failed = |source| Error::Announce { source };

        let socket = bound(announcing.heard_on).map_err(failed)?;
        socket
            .set_read_timeout(Some(STOP_HEARD_WITHIN))
            .map_err(failed)?;
        let heard_at = socket.local_addr().map_err(failed)?;
        if announcing.told.ip().is_multicast() {
            join_the_group(&socket, announcing.told.ip());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("resonate-upnp-announce".to_owned())
            .spawn(move || announce(&socket, &announcing, &stopped))
            .map_err(failed)?;
        Ok(Self {
            stop,
            thread: Some(thread),
            heard_at,
        })
    }

    pub(crate) const fn heard_at(&self) -> SocketAddr {
        self.heard_at
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("the media server's announcer ended in a panic");
        }
    }
}

fn bound(heard_on: SocketAddr) -> io::Result<UdpSocket> {
    match bound_at(heard_on) {
        Err(error) if heard_on.port() != 0 && error.kind() == io::ErrorKind::AddrInUse => {
            tracing::warn!(%error, port = heard_on.port(), "the SSDP port is held without sharing, so only announcements find the media server");
            bound_at(SocketAddr::new(heard_on.ip(), 0))
        }
        bound => bound,
    }
}

fn bound_at(heard_on: SocketAddr) -> io::Result<UdpSocket> {
    let domain = if heard_on.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&heard_on.into())?;
    let socket = UdpSocket::from(socket);
    if heard_on.is_ipv4() {
        socket.set_multicast_ttl_v4(HOPS)?;
    }
    Ok(socket)
}

fn join_the_group(socket: &UdpSocket, group: IpAddr) {
    let IpAddr::V4(group) = group else {
        return;
    };
    let mut joined = false;
    for interface in interfaces() {
        match socket.join_multicast_v4(&group, &interface) {
            Ok(()) => joined = true,
            Err(error) => {
                tracing::debug!(%error, %interface, "the SSDP group could not be joined there");
            }
        }
    }
    if !joined && let Err(error) = socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED) {
        tracing::warn!(%error, "the SSDP group could not be joined, so no search is heard");
    }
}

fn interfaces() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut addresses: Vec<Ipv4Addr> = interfaces
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up() && !interface.is_loopback() && !interface.is_p2p()
        })
        .filter_map(|interface| match interface.ip() {
            IpAddr::V4(ip) if neighbours::is_a_neighbour(IpAddr::V4(ip)) => Some(ip),
            _ => None,
        })
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}

struct Answering {
    began: Instant,
    sent: u32,
}

impl Answering {
    fn room_for(&mut self, answers: u32) -> bool {
        if self.began.elapsed() >= Duration::from_secs(1) {
            self.began = Instant::now();
            self.sent = 0;
        }
        if self.sent.saturating_add(answers) > ANSWERS_A_SECOND_AT_MOST {
            return false;
        }
        self.sent += answers;
        true
    }
}

fn announce(socket: &UdpSocket, announcing: &Announcing, stop: &AtomicBool) {
    let mut buffer = vec![0; SEARCH_BYTES_AT_MOST];
    let mut answering = Answering {
        began: Instant::now(),
        sent: 0,
    };
    tell(socket, announcing, Nts::Alive);
    let mut told_at = Instant::now();
    while !stop.load(Ordering::Acquire) {
        if told_at.elapsed() >= announcing.every {
            tell(socket, announcing, Nts::Alive);
            told_at = Instant::now();
        }
        let (read, sender) = match socket.recv_from(&mut buffer) {
            Ok(heard) => heard,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionReset
                ) =>
            {
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, "the media server stopped hearing searches");
                break;
            }
        };
        if !neighbours::is_a_neighbour(sender.ip()) {
            continue;
        }
        let Some(asked) = buffer.get(..read).and_then(searched_for) else {
            continue;
        };
        let answers = answers_to(&asked, &announcing.udn);
        if answers.is_empty() || !answering.room_for(answers.len() as u32) {
            continue;
        }
        let Ok(host) = host_toward(announcing.serving_on, sender) else {
            continue;
        };
        for (st, usn) in answers {
            let answer = format!(
                "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age={}\r\nEXT:\r\nLOCATION: \
                 http://{host}:{}{DESCRIBED_AT}\r\nSERVER: {SERVED_AS}\r\nST: {st}\r\nUSN: \
                 {usn}\r\nContent-Length: 0\r\n\r\n",
                ALIVE_FOR.as_secs(),
                announcing.port,
            );
            if let Err(error) = socket.send_to(answer.as_bytes(), sender) {
                tracing::debug!(%error, %sender, "a search could not be answered");
            }
        }
    }
    tell(socket, announcing, Nts::ByeBye);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nts {
    Alive,
    ByeBye,
}

fn tell(socket: &UdpSocket, announcing: &Announcing, nts: Nts) {
    let told = announcing.told;
    let hosts: Vec<(Option<Ipv4Addr>, String)> = if told.ip().is_multicast() {
        interfaces()
            .into_iter()
            .map(|interface| (Some(interface), interface.to_string()))
            .collect()
    } else {
        host_toward(announcing.serving_on, told)
            .map(|host| vec![(None, host)])
            .unwrap_or_default()
    };
    for (interface, host) in hosts {
        if let Some(interface) = interface
            && let Err(error) = SockRef::from(socket).set_multicast_if_v4(&interface)
        {
            tracing::debug!(%error, %interface, "the media server could not be announced there");
            continue;
        }
        for (nt, usn) in notified(&announcing.udn) {
            let told_as = match nts {
                Nts::Alive => format!(
                    "NOTIFY * HTTP/1.1\r\nHOST: {SSDP}\r\nCACHE-CONTROL: max-age={}\r\nLOCATION: \
                     http://{host}:{}{DESCRIBED_AT}\r\nNT: {nt}\r\nNTS: ssdp:alive\r\nSERVER: \
                     {SERVED_AS}\r\nUSN: {usn}\r\n\r\n",
                    ALIVE_FOR.as_secs(),
                    announcing.port,
                ),
                Nts::ByeBye => format!(
                    "NOTIFY * HTTP/1.1\r\nHOST: {SSDP}\r\nNT: {nt}\r\nNTS: ssdp:byebye\r\nUSN: \
                     {usn}\r\n\r\n"
                ),
            };
            for _ in 0..TOLD_TIMES {
                if let Err(error) = socket.send_to(told_as.as_bytes(), told) {
                    tracing::debug!(%error, %told, "the media server could not be announced");
                }
            }
        }
    }
}

fn notified(udn: &Udn) -> [(String, String); 5] {
    let udn = udn.as_str();
    let typed = |kind: &str| (kind.to_owned(), format!("{udn}::{kind}"));
    [
        typed(ROOT_DEVICE),
        (udn.to_owned(), udn.to_owned()),
        typed(SERVER_TYPE),
        typed(CONTENT_DIRECTORY),
        typed(CONNECTION_MANAGER),
    ]
}

pub(crate) fn searched_for(asked: &[u8]) -> Option<String> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(asked) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) | Err(_) => return None,
    }
    if request.method != Some("M-SEARCH") || request.path != Some("*") {
        return None;
    }
    let named = |name: &str| {
        request
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(str::trim)
    };
    if named("MAN") != Some(DISCOVER) {
        return None;
    }
    named("ST").map(str::to_owned)
}

pub(crate) fn answers_to(searched: &str, udn: &Udn) -> Vec<(String, String)> {
    let notified = notified(udn);
    if searched == EVERYTHING {
        return notified.to_vec();
    }
    notified
        .into_iter()
        .filter(|(nt, _)| nt == searched)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn udn() -> Udn {
        Udn::new("uuid:4c1b3a27-0000-4000-8000-000000000001")
    }

    #[test]
    fn a_search_is_answered_for_everything_the_server_is_and_nothing_else() {
        let asked = |st: &str| answers_to(st, &udn()).len();

        assert_eq!(asked(EVERYTHING), 5);
        assert_eq!(asked(ROOT_DEVICE), 1);
        assert_eq!(asked("uuid:4c1b3a27-0000-4000-8000-000000000001"), 1);
        assert_eq!(asked(SERVER_TYPE), 1);
        assert_eq!(asked(CONTENT_DIRECTORY), 1);
        assert_eq!(asked(CONNECTION_MANAGER), 1);
        assert_eq!(asked("urn:schemas-upnp-org:device:MediaRenderer:1"), 0);
        assert_eq!(asked("urn:schemas-upnp-org:device:MediaServer:2"), 0);
    }

    #[test]
    fn only_a_discovering_search_is_read() {
        let search = b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \
                       \"ssdp:discover\"\r\nMX: 2\r\nST: ssdp:all\r\n\r\n";
        let notify = b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nNT: \
                       upnp:rootdevice\r\nNTS: ssdp:alive\r\n\r\n";
        let undiscovering = b"M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n";

        assert_eq!(searched_for(search).as_deref(), Some(EVERYTHING));
        assert_eq!(searched_for(notify), None);
        assert_eq!(searched_for(undiscovering), None);
    }

    #[test]
    fn answers_past_the_most_a_second_are_held_back() {
        let mut answering = Answering {
            began: Instant::now(),
            sent: 0,
        };

        let room: Vec<bool> = (0..30).map(|_| answering.room_for(5)).collect();

        assert_eq!(
            room.iter().filter(|room| **room).count() as u32,
            ANSWERS_A_SECOND_AT_MOST / 5
        );
    }
}
