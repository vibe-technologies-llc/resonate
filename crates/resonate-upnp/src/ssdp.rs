use std::{
    collections::BTreeMap,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, UdpSocket},
    time::{Duration, Instant},
};

use crate::{
    Error, Located, Result, SearchOp, Udn,
    http::{HEADERS_AT_MOST, USER_AGENT},
};

pub const SSDP: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900));

pub const RENDERER_TYPE: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

pub const SERVER_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";

const RENDERER_KIND: &str = ":device:MediaRenderer:";

const SERVER_KIND: &str = ":device:MediaServer:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sought {
    Renderers,
    Servers,
}

impl Sought {
    const fn search_target(self) -> &'static str {
        match self {
            Self::Renderers => RENDERER_TYPE,
            Self::Servers => SERVER_TYPE,
        }
    }

    const fn kind(self) -> &'static str {
        match self {
            Self::Renderers => RENDERER_KIND,
            Self::Servers => SERVER_KIND,
        }
    }

    pub(crate) fn admits(self, answered: &Answered) -> bool {
        let at_the_sender = answered.location.is_at(answered.sender);
        match self {
            Self::Renderers => at_the_sender,
            Self::Servers => {
                at_the_sender
                    || answered
                        .location
                        .could_be_another_address_beside(answered.sender)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Answered {
    pub location: Located,
    pub sender: IpAddr,
    pub udn: Option<Udn>,
}

const SEARCHES_SENT: usize = 2;

const SEARCH_HOPS: u32 = 2;

const LONGEST_WAIT_ASKED_SECONDS: u64 = 5;

const ANSWER_BYTES_AT_MOST: usize = 8 << 10;

pub(crate) const LOCATIONS_PER_SENDER_AT_MOST: usize = 4;

pub(crate) const LOCATIONS_AT_MOST: usize = 128;

#[derive(Default)]
pub(crate) struct Heard {
    answers: BTreeMap<Located, Answered>,
    per_sender: BTreeMap<IpAddr, usize>,
}

impl Heard {
    pub(crate) fn take(&mut self, answered: Answered) -> bool {
        if self.answers.len() >= LOCATIONS_AT_MOST || self.answers.contains_key(&answered.location)
        {
            return false;
        }

        let from_sender = self.per_sender.entry(answered.sender).or_default();

        if *from_sender >= LOCATIONS_PER_SENDER_AT_MOST {
            return false;
        }
        *from_sender += 1;
        self.answers
            .insert(answered.location.clone(), answered)
            .is_none()
    }

    pub(crate) fn into_answers(self) -> Vec<Answered> {
        self.answers.into_values().collect()
    }
}

pub(crate) fn search(to: SocketAddr, within: Duration, sought: Sought) -> Result<Vec<Answered>> {
    let failed = |op| move |source| Error::Search { op, source };

    let socket = UdpSocket::bind(answered_on(to)).map_err(failed(SearchOp::Bind))?;

    if to.ip().is_multicast() && to.is_ipv4() {
        socket
            .set_multicast_ttl_v4(SEARCH_HOPS)
            .map_err(failed(SearchOp::Bind))?;
    }

    let asking = asking(to, within, sought);

    for _ in 0..SEARCHES_SENT {
        socket
            .send_to(asking.as_bytes(), to)
            .map_err(failed(SearchOp::Send))?;
    }

    let until = Instant::now() + within;
    let mut found = Heard::default();
    let mut buffer = vec![0; ANSWER_BYTES_AT_MOST];

    loop {
        let left = until.saturating_duration_since(Instant::now());

        if left.is_zero() {
            break;
        }
        socket
            .set_read_timeout(Some(left))
            .map_err(failed(SearchOp::Receive))?;
        match socket.recv_from(&mut buffer) {
            Ok((read, sender)) => {
                if let Some(answered) = buffer
                    .get(..read)
                    .and_then(|answer| location_answered(answer, sender.ip(), sought))
                {
                    found.take(answered);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset
                ) => {}
            Err(error) => return Err(failed(SearchOp::Receive)(error)),
        }
    }
    Ok(found.into_answers())
}

fn answered_on(to: SocketAddr) -> SocketAddr {
    let ip = match to.ip() {
        IpAddr::V4(ip) if ip.is_loopback() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(ip) if ip.is_loopback() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    SocketAddr::new(ip, 0)
}

fn asking(to: SocketAddr, within: Duration, sought: Sought) -> String {
    let wait = within.as_secs().clamp(1, LONGEST_WAIT_ASKED_SECONDS);
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {to}\r\nMAN: \"ssdp:discover\"\r\nMX: {wait}\r\nST: \
         {}\r\nUSER-AGENT: {USER_AGENT}\r\n\r\n",
        sought.search_target()
    )
}

pub(crate) fn location_answered(answer: &[u8], sender: IpAddr, sought: Sought) -> Option<Answered> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];

    let mut response = httparse::Response::new(&mut headers);

    match response.parse(answer) {
        Ok(httparse::Status::Complete(_)) if response.code == Some(200) => {}
        Ok(_) | Err(_) => return None,
    }

    let named = |name: &str| {
        response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .and_then(|header| std::str::from_utf8(header.value).ok())
    };

    if !named("ST").is_some_and(|kind| kind.contains(sought.kind())) {
        return None;
    }

    let location = named("LOCATION").and_then(|location| Located::parse(location).ok())?;
    let answered = Answered {
        location,
        sender: sender.to_canonical(),
        udn: named("USN").and_then(udn_of_usn),
    };
    sought.admits(&answered).then_some(answered)
}

fn udn_of_usn(usn: &str) -> Option<Udn> {
    let udn = usn.split("::").next()?.trim();
    let readable = !udn.is_empty() && !udn.chars().any(char::is_control);
    readable.then(|| Udn::new(udn))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renderers_answer_names_where_its_description_is_and_another_device_is_passed_over() {
        let renderer = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLocation: \
                         http://10.0.0.9:49152/desc.xml\r\nST: \
                         urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:a::x\r\n\r\n";
        let server = b"HTTP/1.1 200 OK\r\nLOCATION: http://10.0.0.8/d.xml\r\nST: \
                       urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n";

        let sender: IpAddr = "10.0.0.9".parse().expect("an ip");

        assert_eq!(
            location_answered(renderer, sender, Sought::Renderers)
                .map(|answered| answered.location.to_string()),
            Some("http://10.0.0.9:49152/desc.xml".to_owned())
        );
        assert_eq!(location_answered(server, sender, Sought::Renderers), None);
        assert_eq!(
            location_answered(b"NOTIFY * HTTP/1.1\r\n\r\n", sender, Sought::Renderers),
            None
        );
    }

    #[test]
    fn a_location_naming_a_host_other_than_the_one_that_answered_is_passed_over() {
        let renderer = b"HTTP/1.1 200 OK\r\nLOCATION: http://127.0.0.1:8080/desc.xml\r\nST: \
                         urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        let named = b"HTTP/1.1 200 OK\r\nLOCATION: http://renderer.local/desc.xml\r\nST: \
                      urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        let sender: IpAddr = "10.0.0.9".parse().expect("an ip");

        assert_eq!(location_answered(renderer, sender, Sought::Renderers), None);
        assert_eq!(location_answered(named, sender, Sought::Renderers), None);
    }

    #[test]
    fn a_server_naming_an_address_it_does_not_hold_is_heard_only_where_that_is_a_neighbour() {
        let answer = |location: &str| {
            format!(
                "HTTP/1.1 200 OK\r\nLOCATION: http://{location}/d.xml\r\nST: \
                 urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n"
            )
        };
        let sender: IpAddr = "192.168.1.5".parse().expect("an ip");

        let heard = |location: &str| {
            location_answered(answer(location).as_bytes(), sender, Sought::Servers)
                .map(|answered| answered.location.to_string())
        };

        assert_eq!(
            heard("192.168.1.5:8200"),
            Some("http://192.168.1.5:8200/d.xml".to_owned())
        );
        assert_eq!(
            heard("172.17.0.2:8200"),
            Some("http://172.17.0.2:8200/d.xml".to_owned())
        );
        assert_eq!(
            heard("[fe80::2]:8200"),
            Some("http://[fe80::2]:8200/d.xml".to_owned())
        );
        assert_eq!(heard("8.8.8.8:8200"), None);
        assert_eq!(heard("[2001:db8::1]:8200"), None);
        assert_eq!(heard("100.64.0.1:8200"), None);
        assert_eq!(heard("127.0.0.1:8200"), None);
    }

    #[test]
    fn a_sender_naming_many_locations_is_kept_to_a_few_and_the_whole_to_a_cap() {
        let at = |host: u8, port: u16| {
            Located::parse(&format!("http://10.0.{host}.9:{port}/d.xml")).expect("a location")
        };
        let sender = |host: u8| -> IpAddr { format!("10.0.{host}.9").parse().expect("an ip") };

        let answered = |location: Located, sender: IpAddr| Answered {
            location,
            sender,
            udn: None,
        };

        let mut heard = Heard::default();

        let from_one: Vec<bool> = (0..50)
            .map(|port| heard.take(answered(at(0, 1000 + port), sender(0))))
            .collect();
        let again = heard.take(answered(at(0, 1000), sender(0)));

        assert_eq!(
            from_one.iter().filter(|taken| **taken).count(),
            LOCATIONS_PER_SENDER_AT_MOST
        );
        assert!(!again);

        for host in 1..=255 {
            heard.take(answered(at(host, 80), sender(host)));
        }

        assert_eq!(heard.into_answers().len(), LOCATIONS_AT_MOST);
    }

    #[test]
    fn the_search_asks_for_media_renderers_at_the_address_it_is_sent_to() {
        let asked = asking(SSDP, Duration::from_secs(2), Sought::Renderers);

        assert!(asked.starts_with("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n"));
        assert!(asked.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n"));
        assert!(asked.contains("MX: 2\r\n"));
        assert!(asked.contains("USER-AGENT: resonate\r\n"));
        assert!(asked.ends_with("\r\n\r\n"));
    }

    #[test]
    fn a_search_for_media_servers_asks_for_them_and_takes_only_their_answers() {
        let renderer = b"HTTP/1.1 200 OK\r\nLOCATION: http://10.0.0.9:49152/desc.xml\r\nST: \
                         urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        let server = b"HTTP/1.1 200 OK\r\nLOCATION: http://10.0.0.9:8200/rootDesc.xml\r\nST: \
                       urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n";
        let sender: IpAddr = "10.0.0.9".parse().expect("an ip");

        let asked = asking(SSDP, Duration::from_secs(2), Sought::Servers);

        assert!(asked.contains("ST: urn:schemas-upnp-org:device:MediaServer:1\r\n"));
        assert_eq!(
            location_answered(server, sender, Sought::Servers)
                .map(|answered| answered.location.to_string()),
            Some("http://10.0.0.9:8200/rootDesc.xml".to_owned())
        );
        assert_eq!(location_answered(renderer, sender, Sought::Servers), None);
    }

    #[test]
    fn a_media_server_may_name_another_of_its_addresses_with_its_udn_but_never_this_machine() {
        let docker = b"HTTP/1.1 200 OK\r\nLOCATION: http://172.17.0.2:8200/rootDesc.xml\r\nST: \
                       urn:schemas-upnp-org:device:MediaServer:1\r\nUSN: \
                       uuid:4d696e69-444c::urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n";
        let inward = b"HTTP/1.1 200 OK\r\nLOCATION: http://127.0.0.1:631/\r\nST: \
                       urn:schemas-upnp-org:device:MediaServer:1\r\nUSN: uuid:x\r\n\r\n";
        let named = b"HTTP/1.1 200 OK\r\nLOCATION: http://nas.local:8200/d.xml\r\nST: \
                      urn:schemas-upnp-org:device:MediaServer:1\r\nUSN: uuid:x\r\n\r\n";
        let renderer = b"HTTP/1.1 200 OK\r\nLOCATION: http://172.17.0.2:49152/desc.xml\r\nST: \
                         urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        let sender: IpAddr = "192.168.1.5".parse().expect("an ip");

        let answered = location_answered(docker, sender, Sought::Servers).expect("admitted");

        assert_eq!(
            answered.location.to_string(),
            "http://172.17.0.2:8200/rootDesc.xml"
        );
        assert_eq!(answered.sender, sender);
        assert_eq!(answered.udn, Some(Udn::new("uuid:4d696e69-444c")));
        assert_eq!(location_answered(inward, sender, Sought::Servers), None);
        assert_eq!(location_answered(named, sender, Sought::Servers), None);
        assert_eq!(location_answered(renderer, sender, Sought::Renderers), None);
    }
}
