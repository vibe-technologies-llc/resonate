use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket},
    time::Duration,
};

use mdns_sd::{Receiver, ScopedIp, ServiceDaemon, ServiceEvent, ServiceInfo};
use socket2::{Domain, Protocol, Socket, Type};

use crate::{
    Error, Result,
    key::{Derived, hex, random_bytes},
    seal::{opened, sealed},
    wire::Beacon,
};

pub const NEARBY_PORT: u16 = 41_734;
pub const NEARBY_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 41, 73);
pub const NEARBY_GROUP_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0x7273, 0xa306);
pub const BONJOUR_SERVICE: &str = "_resonate-sync._udp.local.";
pub const RECORD_KEY: &str = "b";
pub const BEACON_EVERY: Duration = Duration::from_secs(15);
pub const LARGEST_BEACON: usize = 512;
pub const ADDRESSES_BEACONED_AT_MOST: usize = 6;
pub const GLOBAL_ADDRESSES_OFFERED_AT_MOST: usize = 4;
pub const IPV6_ADDRESSES_BEACONED_AT_MOST: usize = 2;
const BEACON_MAGIC: &[u8; 4] = b"RSN1";
const ONE_HOP: u32 = 1;
const INSTANCE_BYTES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nearby {
    pub broadcast: Option<Broadcast>,
    pub bonjour: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Broadcast {
    pub port: u16,
    pub told: Vec<SocketAddr>,
}

impl Nearby {
    pub fn on_the_network() -> Self {
        Self {
            broadcast: Some(Broadcast {
                port: NEARBY_PORT,
                told: vec![
                    SocketAddr::new(IpAddr::V4(NEARBY_GROUP), NEARBY_PORT),
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), NEARBY_PORT),
                    SocketAddr::new(IpAddr::V6(NEARBY_GROUP_V6), NEARBY_PORT),
                ],
            }),
            bonjour: true,
        }
    }

    pub const fn told_by_the_platform() -> Self {
        Self {
            broadcast: None,
            bonjour: false,
        }
    }
}

pub fn bound(broadcast: &Broadcast) -> Result<UdpSocket> {
    let anywhere = |port| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let socket = match UdpSocket::bind(anywhere(broadcast.port)) {
        Ok(socket) => socket,
        Err(error) if broadcast.port != 0 => {
            tracing::debug!(%error, port = broadcast.port, "the beacon port is taken; answers alone will find this device");
            UdpSocket::bind(anywhere(0)).map_err(Error::BeaconBind)?
        }
        Err(error) => return Err(Error::BeaconBind(error)),
    };
    socket.set_nonblocking(true).map_err(Error::BeaconBind)?;
    if let Err(error) = socket.set_broadcast(true) {
        tracing::debug!(%error, "beacons cannot be broadcast");
    }
    if let Err(error) = socket.set_multicast_ttl_v4(ONE_HOP) {
        tracing::debug!(%error, "the beacons' reach could not be kept to this network");
    }
    for told in &broadcast.told {
        if let IpAddr::V4(group) = told.ip()
            && group.is_multicast()
            && let Err(error) = socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)
        {
            tracing::debug!(%error, %group, "the beacon group could not be joined");
        }
    }
    Ok(socket)
}

pub fn bound_v6(broadcast: &Broadcast, shared_port: u16) -> Option<UdpSocket> {
    let anywhere = |port| SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);
    let bound_on = |port| {
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_only_v6(true)?;
        socket.bind(&anywhere(port).into())?;
        socket.set_nonblocking(true)?;
        Ok::<_, std::io::Error>(UdpSocket::from(socket))
    };
    let socket = bound_on(shared_port)
        .or_else(|error| {
            tracing::debug!(%error, port = shared_port, "the beacon port is taken over IPv6; answers alone will find this device");
            bound_on(0)
        })
        .inspect_err(|error| tracing::debug!(%error, "beacons cannot be heard over IPv6"))
        .ok()?;
    let interfaces = ipv6_interfaces_here();
    for told in &broadcast.told {
        let IpAddr::V6(group) = told.ip() else {
            continue;
        };
        if !group.is_multicast() {
            continue;
        }
        for interface in &interfaces {
            if let Err(error) = socket.join_multicast_v6(&group, *interface) {
                tracing::debug!(%error, %group, interface, "the beacon group could not be joined over IPv6");
            }
        }
    }
    Some(socket)
}

pub fn ipv6_interfaces_here() -> Vec<u32> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut indexes = interfaces
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up() && !interface.is_loopback() && !interface.is_p2p()
        })
        .filter(|interface| interface.ip().is_ipv6())
        .filter_map(|interface| interface.index)
        .collect::<Vec<_>>();
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}

pub fn told_over_ipv6(told: SocketAddr) -> Vec<SocketAddr> {
    let SocketAddr::V6(address) = told else {
        return Vec::new();
    };
    if !address.ip().is_multicast() {
        return vec![told];
    }
    ipv6_interfaces_here()
        .into_iter()
        .map(|interface| {
            SocketAddr::V6(SocketAddrV6::new(
                *address.ip(),
                address.port(),
                0,
                interface,
            ))
        })
        .collect()
}

pub fn beacon_addresses_here() -> Vec<IpAddr> {
    let mut addresses = addresses_here();
    addresses.extend(
        global_addresses_here()
            .into_iter()
            .take(IPV6_ADDRESSES_BEACONED_AT_MOST),
    );
    addresses
}

pub fn addresses_here() -> Vec<IpAddr> {
    let interfaces = match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces,
        Err(error) => {
            tracing::debug!(%error, "this device's addresses could not be read");
            return Vec::new();
        }
    };
    let mut addresses = interfaces
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up() && !interface.is_loopback() && !interface.is_p2p()
        })
        .map(|interface| interface.ip())
        .filter(IpAddr::is_ipv4)
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    addresses.truncate(ADDRESSES_BEACONED_AT_MOST);
    addresses
}

pub fn global_addresses_here() -> Vec<IpAddr> {
    let interfaces = match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces,
        Err(error) => {
            tracing::debug!(%error, "this device's addresses could not be read");
            return Vec::new();
        }
    };
    let mut addresses = interfaces
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up() && !interface.is_loopback() && !interface.is_p2p()
        })
        .map(|interface| interface.ip())
        .filter(is_reachable_over_ipv6)
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    addresses.truncate(GLOBAL_ADDRESSES_OFFERED_AT_MOST);
    addresses
}

pub fn is_reachable_over_ipv6(address: &IpAddr) -> bool {
    let IpAddr::V6(address) = address else {
        return false;
    };
    let link_local = address.segments()[0] & 0xffc0 == 0xfe80;
    !(address.is_unspecified() || address.is_loopback() || address.is_multicast() || link_local)
}

pub fn beacon_bytes(derived: &Derived, beacon: &Beacon) -> Result<Vec<u8>> {
    let mut bytes = BEACON_MAGIC.to_vec();
    bytes.extend(sealed(&derived.sealer, derived.space, &beacon.encoded())?);
    Ok(bytes)
}

pub fn beacon_of(derived: &Derived, bytes: &[u8]) -> Option<Beacon> {
    let blob = bytes.strip_prefix(BEACON_MAGIC)?;
    opened(&derived.sealer, derived.space, blob)
        .and_then(|plain| Beacon::decoded(&plain))
        .ok()
}

pub fn record_of(derived: &Derived, beacon: &Beacon) -> Result<String> {
    beacon_bytes(derived, beacon).map(|bytes| hex(&bytes))
}

pub fn beacon_recorded(derived: &Derived, record: &str) -> Option<Beacon> {
    beacon_of(derived, &unhexed(record)?)
}

fn unhexed(text: &str) -> Option<Vec<u8>> {
    let digits = text.trim().as_bytes();
    let (pairs, odd) = digits.as_chunks::<2>();
    if !odd.is_empty() || pairs.len() > LARGEST_BEACON {
        return None;
    }
    pairs
        .iter()
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()
        })
        .collect()
}

pub struct Bonjour {
    daemon: ServiceDaemon,
    browsing: Receiver<ServiceEvent>,
    instance: String,
    advertised: Option<String>,
}

pub enum Found {
    Record { record: String, at: Vec<SocketAddr> },
    Nothing,
    Ended,
}

impl Bonjour {
    pub fn started() -> Option<Self> {
        let daemon = ServiceDaemon::new()
            .inspect_err(|error| tracing::debug!(%error, "Bonjour could not start"))
            .ok()?;
        let browsing = daemon
            .browse(BONJOUR_SERVICE)
            .inspect_err(|error| tracing::debug!(%error, "Bonjour could not browse"))
            .ok()?;
        let instance = hex(&random_bytes::<INSTANCE_BYTES>().ok()?);
        Some(Self {
            daemon,
            browsing,
            instance,
            advertised: None,
        })
    }

    pub fn advertise(&mut self, record: &str, port: u16) {
        if self.advertised.as_deref() == Some(record) {
            return;
        }
        let host = format!("{}.local.", self.instance);
        let info = ServiceInfo::new(
            BONJOUR_SERVICE,
            &self.instance,
            &host,
            "",
            port,
            &[(RECORD_KEY, record)][..],
        )
        .map(ServiceInfo::enable_addr_auto);
        match info.and_then(|info| self.daemon.register(info)) {
            Ok(()) => self.advertised = Some(record.to_owned()),
            Err(error) => {
                tracing::debug!(%error, "this device could not be advertised over Bonjour")
            }
        }
    }

    pub fn withdraw(&mut self) {
        if self.advertised.take().is_none() {
            return;
        }
        let fullname = format!("{}.{BONJOUR_SERVICE}", self.instance);
        if let Err(error) = self.daemon.unregister(&fullname) {
            tracing::debug!(%error, "this device's Bonjour record could not be withdrawn");
        }
    }

    pub async fn found(&self) -> Found {
        match self.browsing.recv_async().await {
            Ok(ServiceEvent::ServiceResolved(resolved)) => {
                match resolved.get_property_val_str(RECORD_KEY) {
                    Some(record) => Found::Record {
                        record: record.to_owned(),
                        at: resolved.get_addresses().iter().map(seen_at).collect(),
                    },
                    None => Found::Nothing,
                }
            }
            Ok(_) => Found::Nothing,
            Err(_) => Found::Ended,
        }
    }

    pub fn end(self) {
        if let Err(error) = self.daemon.shutdown() {
            tracing::debug!(%error, "Bonjour did not stop cleanly");
        }
    }
}

fn seen_at(resolved: &ScopedIp) -> SocketAddr {
    match resolved {
        ScopedIp::V6(address) => SocketAddr::V6(SocketAddrV6::new(
            *address.addr(),
            0,
            0,
            if address.addr().is_unicast_link_local() {
                address.scope_id().index
            } else {
                0
            },
        )),
        other => SocketAddr::new(other.to_ip_addr(), 0),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;
    use crate::{DeviceId, SyncKey};

    fn beacon(addresses: usize) -> Beacon {
        Beacon {
            device: DeviceId([7; 16]),
            port: 50_123,
            answering: true,
            addresses: (0..addresses)
                .map(|at| IpAddr::V4(Ipv4Addr::new(192, 168, 255, u8::try_from(at).unwrap_or(0))))
                .collect(),
        }
    }

    #[test]
    fn a_beacon_opens_under_its_own_code_alone() {
        let here = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let elsewhere = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let beacon = beacon(2);

        let bytes = beacon_bytes(&here, &beacon).expect("sealed");
        let again = beacon_bytes(&here, &beacon).expect("sealed");

        assert_eq!(beacon_of(&here, &bytes), Some(beacon));
        assert_eq!(beacon_of(&elsewhere, &bytes), None);
        assert_eq!(beacon_of(&here, &bytes[BEACON_MAGIC.len()..]), None);
        assert_ne!(bytes, again);
        assert!(bytes.len() <= LARGEST_BEACON);
    }

    #[test]
    fn the_fullest_record_fits_one_bonjour_text_string_and_reads_back() {
        let here = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let fullest = Beacon {
            addresses: vec![IpAddr::V6(Ipv6Addr::LOCALHOST); ADDRESSES_BEACONED_AT_MOST],
            ..beacon(0)
        };

        let record = record_of(&here, &fullest).expect("recorded");

        assert!(
            RECORD_KEY.len() + 1 + record.len() <= usize::from(u8::MAX),
            "{} bytes",
            record.len()
        );
        assert_eq!(beacon_recorded(&here, &record), Some(fullest));
        assert_eq!(beacon_recorded(&here, &record[1..]), None);
        assert_eq!(beacon_recorded(&here, "zz"), None);
    }

    #[test]
    fn the_fullest_record_of_distinct_addresses_still_fits_one_bonjour_text_string() {
        let here = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let mut addresses = [
            [10, 91, 200, 3],
            [47, 90, 189, 32],
            [84, 89, 178, 61],
            [121, 94, 167, 90],
            [158, 93, 156, 119],
            [195, 100, 145, 148],
        ]
        .map(|octets| IpAddr::V4(Ipv4Addr::from(octets)))
        .to_vec();
        addresses.extend([
            IpAddr::V6(Ipv6Addr::new(
                0x2a02, 0x8f1c, 0x5e71, 0x93aa, 0x1f0d, 0xc4e2, 0x7b19, 0xd05e,
            )),
            IpAddr::V6(Ipv6Addr::new(
                0x2a02, 0x9d27, 0x6ab8, 0x93ab, 0x4a84, 0xc4d3, 0x7b1a, 0xd54f,
            )),
        ]);
        let fullest = Beacon {
            addresses,
            ..beacon(0)
        };

        let record = record_of(&here, &fullest).expect("recorded");

        assert!(
            RECORD_KEY.len() + 1 + record.len() <= usize::from(u8::MAX),
            "{} bytes",
            record.len()
        );
        assert_eq!(beacon_recorded(&here, &record), Some(fullest));
    }

    #[test]
    fn a_beacon_sent_to_the_ipv6_loopback_is_heard_and_opens_under_its_code() {
        let broadcast = Broadcast {
            port: 0,
            told: Vec::new(),
        };
        let (Some(sender), Some(listener)) = (bound_v6(&broadcast, 0), bound_v6(&broadcast, 0))
        else {
            return;
        };
        let here = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let beacon = beacon(1);
        let bytes = beacon_bytes(&here, &beacon).expect("sealed");
        let to = SocketAddr::new(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            listener.local_addr().expect("an address").port(),
        );

        sender.send_to(&bytes, to).expect("sent");
        let mut heard = vec![0; LARGEST_BEACON];
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (length, from) = loop {
            match listener.recv_from(&mut heard) {
                Ok(received) => break received,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("nothing was heard over IPv6: {error}"),
            }
        };

        assert!(from.is_ipv6());
        assert_eq!(beacon_of(&here, &heard[..length]), Some(beacon));
    }

    #[test]
    fn a_unicast_ipv6_address_is_told_as_it_stands_and_an_ipv4_one_is_not_told_over_ipv6() {
        let unicast: SocketAddr = "[::1]:41734".parse().expect("an address");
        let v4: SocketAddr = "127.0.0.1:41734".parse().expect("an address");

        assert_eq!(told_over_ipv6(unicast), vec![unicast]);
        assert!(told_over_ipv6(v4).is_empty());
    }

    #[test]
    fn a_multicast_group_is_told_once_on_each_interface_that_holds_an_ipv6_address() {
        let group = SocketAddr::new(IpAddr::V6(NEARBY_GROUP_V6), NEARBY_PORT);

        let told = told_over_ipv6(group);

        assert_eq!(told.len(), ipv6_interfaces_here().len());
        assert!(told.iter().all(|told| matches!(
            told,
            SocketAddr::V6(address) if address.ip() == &NEARBY_GROUP_V6
                && address.port() == NEARBY_PORT
                && address.scope_id() != 0
        )));
    }

    #[test]
    fn only_an_address_other_devices_could_dial_over_ipv6_is_offered() {
        let reachable = |text: &str| is_reachable_over_ipv6(&text.parse().expect("an address"));

        assert!(reachable("2001:db8::7"));
        assert!(reachable("fd12:3456::1"));
        assert!(!reachable("fe80::1"));
        assert!(!reachable("::1"));
        assert!(!reachable("::"));
        assert!(!reachable("ff02::1"));
        assert!(!reachable("192.168.1.4"));
    }
}
