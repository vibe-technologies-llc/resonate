use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, UdpSocket},
    time::{Duration, Instant},
};

use crate::{
    Error, Located, Result, SearchOp,
    http::{HEADERS_AT_MOST, USER_AGENT},
};

pub const SSDP: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900));

pub const RENDERER_TYPE: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

const RENDERER_KIND: &str = ":device:MediaRenderer:";

const SEARCHES_SENT: usize = 2;

const SEARCH_HOPS: u32 = 2;

const LONGEST_WAIT_ASKED_SECONDS: u64 = 5;

const ANSWER_BYTES_AT_MOST: usize = 8 << 10;

pub(crate) fn search(to: SocketAddr, within: Duration) -> Result<Vec<Located>> {
    let failed = |op| move |source| Error::Search { op, source };

    let socket = UdpSocket::bind(answered_on(to)).map_err(failed(SearchOp::Bind))?;
    if to.ip().is_multicast() && to.is_ipv4() {
        socket
            .set_multicast_ttl_v4(SEARCH_HOPS)
            .map_err(failed(SearchOp::Bind))?;
    }
    let asking = asking(to, within);
    for _ in 0..SEARCHES_SENT {
        socket
            .send_to(asking.as_bytes(), to)
            .map_err(failed(SearchOp::Send))?;
    }

    let until = Instant::now() + within;
    let mut found = Vec::new();
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
            Ok((read, _)) => {
                if let Some(location) = buffer.get(..read).and_then(location_answered)
                    && !found.contains(&location)
                {
                    found.push(location);
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
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
            Err(error) => return Err(failed(SearchOp::Receive)(error)),
        }
    }
    Ok(found)
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

fn asking(to: SocketAddr, within: Duration) -> String {
    let wait = within.as_secs().clamp(1, LONGEST_WAIT_ASKED_SECONDS);
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {to}\r\nMAN: \"ssdp:discover\"\r\nMX: {wait}\r\nST: \
         {RENDERER_TYPE}\r\nUSER-AGENT: {USER_AGENT} UPnP/1.1\r\n\r\n"
    )
}

fn location_answered(answer: &[u8]) -> Option<Located> {
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
    if !named("ST").is_some_and(|kind| kind.contains(RENDERER_KIND)) {
        return None;
    }
    named("LOCATION").and_then(|location| Located::parse(location).ok())
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

        assert_eq!(
            location_answered(renderer).map(|location| location.to_string()),
            Some("http://10.0.0.9:49152/desc.xml".to_owned())
        );
        assert_eq!(location_answered(server), None);
        assert_eq!(location_answered(b"NOTIFY * HTTP/1.1\r\n\r\n"), None);
    }

    #[test]
    fn the_search_asks_for_media_renderers_at_the_address_it_is_sent_to() {
        let asked = asking(SSDP, Duration::from_secs(2));

        assert!(asked.starts_with("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n"));
        assert!(asked.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n"));
        assert!(asked.contains("MX: 2\r\n"));
        assert!(asked.ends_with("\r\n\r\n"));
    }
}
