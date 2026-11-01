use std::{
    fmt, io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    time::Duration,
};

use mdns_sd::{HostnameResolutionEvent, ScopedIp, ServiceDaemon};

use crate::{CAST_PORT, Error, Result};

const HOST_BYTES_AT_MOST: usize = 253;

const LABEL_BYTES_AT_MOST: usize = 63;

const LOCAL_SUFFIX: &str = ".local";

const BROWSE_RESOLVES_WITHIN: Duration = Duration::from_secs(3);

const BROWSE_ANSWER_SLACK: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host(Box<str>);

impl Host {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_suffix('.').unwrap_or(text);
        if text.is_empty() || text.len() > HOST_BYTES_AT_MOST {
            return None;
        }
        let labels: Vec<&str> = text.split('.').collect();
        let spelled = labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= LABEL_BYTES_AT_MOST
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '-' | '_'))
        });
        let numeric = labels
            .last()
            .is_some_and(|label| label.chars().all(|letter| letter.is_ascii_digit()));
        (spelled && !numeric).then(|| Self(text.to_ascii_lowercase().into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_local(&self) -> bool {
        self.0.len() > LOCAL_SUFFIX.len() && self.0.ends_with(LOCAL_SUFFIX)
    }

    fn mdns_name(&self) -> String {
        format!("{}.", self.0)
    }

    fn resolve(&self, port: u16, through_mdns: bool) -> Result<SocketAddr> {
        let source = match (self.as_str(), port).to_socket_addrs() {
            Ok(addresses) => match preferred(addresses) {
                Some(at) => return Ok(at),
                None => io::Error::from(io::ErrorKind::NotFound),
            },
            Err(source) => source,
        };
        if through_mdns
            && self.is_local()
            && let Some(ip) = self.through_the_browse()
        {
            return Ok(SocketAddr::new(ip, port));
        }
        Err(Error::Resolve {
            host: self.clone(),
            source,
        })
    }

    fn through_the_browse(&self) -> Option<IpAddr> {
        let daemon = ServiceDaemon::new().ok()?;
        let found = self.heard_from(&daemon);
        if let Err(error) = daemon.shutdown() {
            tracing::debug!(%error, "resolving a .local name did not stop cleanly");
        }
        found
    }

    fn heard_from(&self, daemon: &ServiceDaemon) -> Option<IpAddr> {
        let timeout_ms = u64::try_from(BROWSE_RESOLVES_WITHIN.as_millis()).ok()?;
        let events = daemon
            .resolve_hostname(&self.mdns_name(), Some(timeout_ms))
            .ok()?;
        loop {
            match events.recv_timeout(BROWSE_RESOLVES_WITHIN + BROWSE_ANSWER_SLACK) {
                Ok(HostnameResolutionEvent::AddressesFound(_, addresses)) => {
                    let mut addresses: Vec<IpAddr> =
                        addresses.iter().map(ScopedIp::to_ip_addr).collect();
                    addresses.sort();
                    return addresses
                        .iter()
                        .find(|ip| ip.is_ipv4())
                        .or_else(|| addresses.first())
                        .copied();
                }
                Ok(HostnameResolutionEvent::SearchTimeout(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Given {
    Address(SocketAddr),
    Named { host: Host, port: u16 },
}

impl Given {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Ok(at) = text.parse::<SocketAddr>() {
            return Some(Self::Address(at));
        }
        if let Ok(ip) = text.trim_matches(['[', ']']).parse::<IpAddr>() {
            return Some(Self::Address(SocketAddr::new(ip, CAST_PORT)));
        }
        let (host, port) = match text.rsplit_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().ok().filter(|port| *port != 0)?),
            None => (text, CAST_PORT),
        };
        Host::parse(host).map(|host| Self::Named { host, port })
    }

    pub fn resolve(&self, through_mdns: bool) -> Result<SocketAddr> {
        match self {
            Self::Address(at) => Ok(*at),
            Self::Named { host, port } => host.resolve(*port, through_mdns),
        }
    }

    pub const fn address(&self) -> Option<SocketAddr> {
        match self {
            Self::Address(at) => Some(*at),
            Self::Named { .. } => None,
        }
    }
}

impl From<SocketAddr> for Given {
    fn from(at: SocketAddr) -> Self {
        Self::Address(at)
    }
}

impl fmt::Display for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address(at) => at.fmt(f),
            Self::Named { host, port } => write!(f, "{host}:{port}"),
        }
    }
}

fn preferred(addresses: impl Iterator<Item = SocketAddr>) -> Option<SocketAddr> {
    let mut first = None;
    for at in addresses {
        if at.is_ipv4() {
            return Some(at);
        }
        first.get_or_insert(at);
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(host: &str, port: u16) -> Given {
        Given::Named {
            host: Host::parse(host).expect("a host name"),
            port,
        }
    }

    #[test]
    fn a_receiver_is_addressed_by_an_ip_with_the_cast_port_unless_one_is_given() {
        assert_eq!(
            Given::parse(" 192.168.1.20 "),
            Some(Given::Address(SocketAddr::from((
                [192, 168, 1, 20],
                CAST_PORT
            ))))
        );
        assert_eq!(
            Given::parse("192.168.1.20:8010"),
            Some(Given::Address(SocketAddr::from(([192, 168, 1, 20], 8010))))
        );
        assert_eq!(
            Given::parse("[fe80::1]"),
            Some(Given::Address(SocketAddr::new(
                "fe80::1".parse().expect("an address"),
                CAST_PORT
            )))
        );
        assert_eq!(
            Given::parse("[fe80::1]:8011").and_then(|given| given.address().map(|at| at.port())),
            Some(8011)
        );
        assert_eq!(Given::parse(""), None);
        assert_eq!(Given::parse("   "), None);
    }

    #[test]
    fn a_receiver_is_addressed_by_a_host_name_with_the_cast_port_unless_one_is_given() {
        assert_eq!(
            Given::parse(" Living-Room.local "),
            Some(named("living-room.local", CAST_PORT))
        );
        assert_eq!(
            Given::parse("speaker.lan:8010"),
            Some(named("speaker.lan", 8010))
        );
        assert_eq!(
            Given::parse("chromecast."),
            Some(named("chromecast", CAST_PORT))
        );
        assert_eq!(
            Given::parse("living-room.local").map(|given| given.to_string()),
            Some("living-room.local:8009".to_owned())
        );
    }

    #[test]
    fn what_is_neither_an_address_nor_a_host_name_is_refused() {
        for refused in [
            "living room.local",
            "speaker.local:",
            "speaker.local:0",
            "speaker.local:99999",
            "speaker.local:http",
            "-speaker.local",
            "speaker-.local",
            "a..local",
            ".local",
            "192.168.1.300",
            "192.168.1",
            "http://speaker.local",
            ":8009",
        ] {
            assert_eq!(Given::parse(refused), None, "{refused}");
        }
        assert_eq!(Given::parse(&"a".repeat(64)), None);
        assert!(Given::parse(&"a".repeat(63)).is_some());
        assert_eq!(Given::parse(&format!("{}.com", "a.".repeat(130))), None);
    }

    #[test]
    fn only_a_name_ending_in_local_is_a_candidate_for_the_browse() {
        let local = |text: &str| Host::parse(text).is_some_and(|host| host.is_local());

        assert!(local("kitchen.local"));
        assert!(local("kitchen.local."));
        assert!(!local("local"));
        assert!(!local("kitchen.lan"));
        assert!(!local("kitchen.localhost"));
        assert_eq!(
            Host::parse("Kitchen.local").map(|host| host.mdns_name()),
            Some("kitchen.local.".to_owned())
        );
    }

    #[test]
    fn a_name_is_resolved_to_an_address_with_its_port_and_an_address_is_itself() {
        let at = SocketAddr::from(([192, 168, 1, 20], 8009));

        assert_eq!(Given::Address(at).resolve(false).ok(), Some(at));

        let loopback = named("localhost", 8123).resolve(false).expect("localhost");
        assert!(loopback.ip().is_loopback());
        assert_eq!(loopback.port(), 8123);
    }

    #[test]
    fn a_name_nothing_resolves_is_an_error_naming_it() {
        let error = named("no-such-receiver.invalid", 8009)
            .resolve(false)
            .expect_err("nothing answers for .invalid");

        assert!(
            matches!(error, Error::Resolve { host, .. } if host.as_str() == "no-such-receiver.invalid")
        );
    }

    #[test]
    fn an_ipv4_address_is_preferred_to_an_ipv6_one() {
        let v6 = SocketAddr::new("::1".parse().expect("an address"), 8009);
        let v4 = SocketAddr::from(([127, 0, 0, 1], 8009));

        assert_eq!(preferred([v6, v4].into_iter()), Some(v4));
        assert_eq!(preferred([v6].into_iter()), Some(v6));
        assert_eq!(preferred(std::iter::empty()), None);
    }
}
