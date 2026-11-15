use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const UNIQUE_LOCAL: u16 = 0xfc00;

const UNIQUE_LOCAL_MASK: u16 = 0xfe00;

const LINK_LOCAL: u16 = 0xfe80;

const LINK_LOCAL_MASK: u16 = 0xffc0;

pub(crate) fn is_a_neighbour(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_a_neighbour_v4(ip),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map_or_else(|| is_a_neighbour_v6(ip), is_a_neighbour_v4),
    }
}

fn is_a_neighbour_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_link_local() || ip.is_loopback()
}

fn is_a_neighbour_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback()
        || first & UNIQUE_LOCAL_MASK == UNIQUE_LOCAL
        || first & LINK_LOCAL_MASK == LINK_LOCAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_address_of_the_local_network_is_a_neighbour() {
        let neighbour = |ip: &str| is_a_neighbour(ip.parse().expect("an address"));

        for near in [
            "192.168.1.20",
            "10.0.0.9",
            "172.16.4.1",
            "169.254.3.3",
            "127.0.0.1",
            "::1",
            "fe80::1",
            "fd12::7",
            "::ffff:192.168.1.20",
        ] {
            assert!(neighbour(near), "{near}");
        }
        for far in [
            "8.8.8.8",
            "100.64.0.1",
            "2001:db8::1",
            "::ffff:8.8.8.8",
            "0.0.0.0",
        ] {
            assert!(!neighbour(far), "{far}");
        }
    }
}
