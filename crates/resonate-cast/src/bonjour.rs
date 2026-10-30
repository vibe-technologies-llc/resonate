use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ResolvedService, ScopedIp};

use crate::{CastId, Found};

pub const CAST_SERVICE: &str = "_googlecast._tcp.local.";

const AUDIO_OUT: u64 = 0x04;

const ID_LETTERS_AT_MOST: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Record<'a> {
    pub(crate) id: Option<&'a str>,
    pub(crate) friendly: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) capabilities: Option<&'a str>,
}

impl Record<'_> {
    pub(crate) fn found(self, addresses: &[IpAddr], port: u16) -> Option<Found> {
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
        let ip = addresses
            .iter()
            .find(|ip| ip.is_ipv4())
            .or_else(|| addresses.first())?;
        let name = [self.friendly, self.model]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|name| !name.is_empty())
            .unwrap_or(id);
        Some(Found {
            id: CastId::new(id),
            name: name.to_owned(),
            at: SocketAddr::new(*ip, port),
        })
    }
}

pub(crate) fn found_in(resolved: &ResolvedService) -> Option<Found> {
    let mut addresses: Vec<IpAddr> = resolved
        .get_addresses()
        .iter()
        .map(ScopedIp::to_ip_addr)
        .collect();
    addresses.sort();
    Record {
        id: resolved.get_property_val_str("id"),
        friendly: resolved.get_property_val_str("fn"),
        model: resolved.get_property_val_str("md"),
        capabilities: resolved.get_property_val_str("ca"),
    }
    .found(&addresses, resolved.get_port())
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

    #[test]
    fn a_record_names_the_receiver_by_its_id_and_friendly_name_at_its_ipv4_address() {
        let addresses = [
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30)),
        ];
        let found = KITCHEN.found(&addresses, 8009).expect("a receiver");

        assert_eq!(found.id.as_str(), "8c3f1b2a9d0e4f5a6b7c8d9e0f1a2b3c");
        assert_eq!(found.name, "Kitchen speaker");
        assert_eq!(found.at, "192.168.1.30:8009".parse().expect("an address"));
    }

    #[test]
    fn a_record_without_an_id_an_address_or_audio_out_is_not_a_receiver() {
        let address = [IpAddr::V4(Ipv4Addr::LOCALHOST)];
        let unnamed = Record {
            friendly: None,
            ..KITCHEN
        };

        assert!(
            Record {
                id: None,
                ..KITCHEN
            }
            .found(&address, 8009)
            .is_none()
        );
        assert!(
            Record {
                id: Some("a/b"),
                ..KITCHEN
            }
            .found(&address, 8009)
            .is_none()
        );
        assert!(KITCHEN.found(&[], 8009).is_none());
        assert!(
            Record {
                capabilities: Some("1"),
                ..KITCHEN
            }
            .found(&address, 8009)
            .is_none()
        );
        assert_eq!(
            unnamed.found(&address, 8009).expect("a receiver").name,
            "Google Home"
        );
    }
}
