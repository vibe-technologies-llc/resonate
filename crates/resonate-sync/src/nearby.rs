use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::Duration,
};

use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::{
    Error, Result,
    key::{Derived, hex, random_bytes},
    seal::{opened, sealed},
    wire::Beacon,
};

pub const NEARBY_PORT: u16 = 41_734;
pub const NEARBY_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 41, 73);
pub const BONJOUR_SERVICE: &str = "_resonate-sync._udp.local.";
pub const RECORD_KEY: &str = "b";
pub const BEACON_EVERY: Duration = Duration::from_secs(15);
pub const LARGEST_BEACON: usize = 512;
pub const ADDRESSES_BEACONED_AT_MOST: usize = 6;
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
    Record { record: String, at: Vec<IpAddr> },
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
                        at: resolved
                            .get_addresses_v4()
                            .into_iter()
                            .map(IpAddr::V4)
                            .collect(),
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
}
