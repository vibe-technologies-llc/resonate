use std::net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6};

use mdns_sd::{ResolvedService, ScopedIp};

use crate::{CastId, Found};

pub const CAST_SERVICE: &str = "_googlecast._tcp.local.";

const AUDIO_OUT: u64 = 0x04;

const ID_LETTERS_AT_MOST: usize = 64;

const NO_SCOPE: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admitting {
    Routable,
    LoopbackToo,
}

impl Admitting {
    fn admits(self, at: SocketAddr) -> bool {
        let ip = at.ip().to_canonical();
        let unscoped_link_local = match at {
            SocketAddr::V6(at) => at.ip().is_unicast_link_local() && at.scope_id() == NO_SCOPE,
            SocketAddr::V4(_) => false,
        };
        let loopback_refused = self == Self::Routable && ip.is_loopback();

        at.port() != 0
            && !ip.is_unspecified()
            && !ip.is_multicast()
            && !unscoped_link_local
            && !loopback_refused
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Record<'a> {
    pub(crate) id: Option<&'a str>,
    pub(crate) friendly: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) capabilities: Option<&'a str>,
}

impl Record<'_> {
    pub(crate) fn found(self, addresses: &[SocketAddr], admitting: Admitting) -> Option<Found> {
        let id = self.id.map(str::trim).filter(|id| {
            !id.is_empty()
                && id.len() <= ID_LETTERS_AT_MOST
                && id
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || letter == '-')
        })?;

        if let Some(capabilities) = self.capabilities
            && let Ok(capabilities) = capabilities.trim().parse::<u64>()
            && capabilities & AUDIO_OUT == 0
        {
            return None;
        }

        let at = preferred(addresses.iter().copied().filter(|at| admitting.admits(*at)))?;
        let name = [self.friendly, self.model]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|name| !name.is_empty())
            .unwrap_or(id);
        Some(Found {
            id: CastId::new(id),
            name: name.to_owned(),
            at,
        })
    }
}

pub(crate) fn socket_of(ip: &ScopedIp, port: u16) -> SocketAddr {
    match ip {
        ScopedIp::V6(scoped) => {
            let scope = if scoped.addr().is_unicast_link_local() {
                scoped.scope_id().index
            } else {
                NO_SCOPE
            };
            SocketAddr::V6(SocketAddrV6::new(*scoped.addr(), port, 0, scope))
        }
        other => match other.to_ip_addr() {
            IpAddr::V4(ip) => SocketAddr::V4(SocketAddrV4::new(ip, port)),
            IpAddr::V6(ip) => SocketAddr::V6(SocketAddrV6::new(ip, port, 0, NO_SCOPE)),
        },
    }
}

pub(crate) fn preferred(addresses: impl IntoIterator<Item = SocketAddr>) -> Option<SocketAddr> {
    let mut addresses: Vec<SocketAddr> = addresses.into_iter().collect();

    addresses.sort();
    addresses
        .iter()
        .find(|at| at.is_ipv4())
        .or_else(|| addresses.first())
        .copied()
}

pub(crate) fn found_in(resolved: &ResolvedService, admitting: Admitting) -> Option<Found> {
    let port = resolved.get_port();
    let addresses: Vec<SocketAddr> = resolved
        .get_addresses()
        .iter()
        .map(|ip| socket_of(ip, port))
        .collect();

    Record {
        id: resolved.get_property_val_str("id"),
        friendly: resolved.get_property_val_str("fn"),
        model: resolved.get_property_val_str("md"),
        capabilities: resolved.get_property_val_str("ca"),
    }
    .found(&addresses, admitting)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    const KITCHEN: Record<'static> = Record {
        id: Some("8c3f1b2a9d0e4f5a6b7c8d9e0f1a2b3c"),
        friendly: Some("Kitchen speaker"),
        model: Some("Google Home"),
        capabilities: Some("2052"),
    };

    const LINK_LOCAL: Ipv6Addr = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x1c2b, 0x3aff, 0xfe4d, 0x5e6f);

    fn v4(ip: [u8; 4], port: u16) -> SocketAddr {
        SocketAddr::from((ip, port))
    }

    fn v6(ip: Ipv6Addr, port: u16, scope: u32) -> SocketAddr {
        SocketAddr::V6(SocketAddrV6::new(ip, port, 0, scope))
    }

    #[test]
    fn a_record_names_the_receiver_by_its_id_and_friendly_name_at_its_ipv4_address() {
        let addresses = [
            v6(
                Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
                8009,
                NO_SCOPE,
            ),
            v4([192, 168, 1, 30], 8009),
        ];
        let found = KITCHEN
            .found(&addresses, Admitting::Routable)
            .expect("a receiver");

        assert_eq!(found.id.as_str(), "8c3f1b2a9d0e4f5a6b7c8d9e0f1a2b3c");
        assert_eq!(found.name, "Kitchen speaker");
        assert_eq!(found.at, "192.168.1.30:8009".parse().expect("an address"));
    }

    #[test]
    fn a_record_without_an_id_an_address_or_audio_out_is_not_a_receiver() {
        let address = [v4([192, 168, 1, 30], 8009)];
        let unnamed = Record {
            friendly: None,
            ..KITCHEN
        };

        assert!(
            Record {
                id: None,
                ..KITCHEN
            }
            .found(&address, Admitting::Routable)
            .is_none()
        );
        assert!(
            Record {
                id: Some("a/b"),
                ..KITCHEN
            }
            .found(&address, Admitting::Routable)
            .is_none()
        );
        assert!(KITCHEN.found(&[], Admitting::Routable).is_none());
        assert!(
            Record {
                capabilities: Some("1"),
                ..KITCHEN
            }
            .found(&address, Admitting::Routable)
            .is_none()
        );
        assert_eq!(
            unnamed
                .found(&address, Admitting::Routable)
                .expect("a receiver")
                .name,
            "Google Home"
        );
    }

    #[test]
    fn a_record_at_loopback_port_zero_or_nowhere_routable_is_refused() {
        let unreachable = [
            v4([127, 0, 0, 1], 8009),
            v4([127, 3, 2, 1], 8009),
            v6(Ipv6Addr::LOCALHOST, 8009, NO_SCOPE),
            v6(Ipv4Addr::LOCALHOST.to_ipv6_mapped(), 8009, NO_SCOPE),
            v4([192, 168, 1, 30], 0),
            v4([0, 0, 0, 0], 8009),
            v4([224, 0, 0, 251], 8009),
            v6(LINK_LOCAL, 8009, NO_SCOPE),
        ];

        for at in unreachable {
            assert_eq!(KITCHEN.found(&[at], Admitting::Routable), None, "{at}");
        }

        assert_eq!(
            KITCHEN
                .found(
                    &[v4([127, 0, 0, 1], 8009), v4([192, 168, 1, 30], 8009)],
                    Admitting::Routable
                )
                .map(|found| found.at),
            Some(v4([192, 168, 1, 30], 8009))
        );
    }

    #[test]
    fn a_receiver_on_loopback_is_admitted_only_where_the_casting_asks_for_it() {
        let at = v4([127, 0, 0, 1], 8009);

        assert_eq!(
            KITCHEN
                .found(&[at], Admitting::LoopbackToo)
                .map(|found| found.at),
            Some(at)
        );
        assert_eq!(
            KITCHEN.found(&[v4([127, 0, 0, 1], 0)], Admitting::LoopbackToo),
            None
        );
    }

    #[test]
    fn a_receiver_answering_only_on_a_link_local_address_is_reached_through_its_scope() {
        let scoped = v6(LINK_LOCAL, 8009, 3);

        let found = KITCHEN
            .found(
                &[v6(LINK_LOCAL, 8009, NO_SCOPE), scoped],
                Admitting::Routable,
            )
            .expect("a receiver");

        assert_eq!(found.at, scoped);
        assert_eq!(
            socket_of(&ScopedIp::from(IpAddr::V6(LINK_LOCAL)), 8009),
            v6(LINK_LOCAL, 8009, NO_SCOPE)
        );
        assert_eq!(
            socket_of(
                &ScopedIp::from(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30))),
                8009
            ),
            v4([192, 168, 1, 30], 8009)
        );
    }
}
