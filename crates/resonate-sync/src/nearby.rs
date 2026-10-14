use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::Duration,
};

use crate::{
    Error, Result,
    key::Derived,
    seal::{opened, sealed},
    wire::Beacon,
};

pub const NEARBY_PORT: u16 = 41_734;
pub const NEARBY_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 41, 73);
pub const BEACON_EVERY: Duration = Duration::from_secs(15);
pub const LARGEST_BEACON: usize = 512;
const BEACON_MAGIC: &[u8; 4] = b"RSN1";
const ONE_HOP: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nearby {
    pub port: u16,
    pub told: Vec<SocketAddr>,
}

impl Nearby {
    pub fn on_the_network() -> Self {
        Self {
            port: NEARBY_PORT,
            told: vec![
                SocketAddr::new(IpAddr::V4(NEARBY_GROUP), NEARBY_PORT),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), NEARBY_PORT),
            ],
        }
    }
}

pub fn bound(nearby: &Nearby) -> Result<UdpSocket> {
    let anywhere = |port| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let socket = match UdpSocket::bind(anywhere(nearby.port)) {
        Ok(socket) => socket,
        Err(error) if nearby.port != 0 => {
            tracing::debug!(%error, port = nearby.port, "the beacon port is taken; answers alone will find this device");
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
    for told in &nearby.told {
        if let IpAddr::V4(group) = told.ip()
            && group.is_multicast()
            && let Err(error) = socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)
        {
            tracing::debug!(%error, %group, "the beacon group could not be joined");
        }
    }
    Ok(socket)
}

pub fn beacon_bytes(derived: &Derived, beacon: Beacon) -> Result<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceId, SyncKey};

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
        let beacon = Beacon {
            device: DeviceId([7; 16]),
            port: 50_123,
            answering: true,
        };

        let bytes = beacon_bytes(&here, beacon).expect("sealed");
        let again = beacon_bytes(&here, beacon).expect("sealed");

        assert_eq!(beacon_of(&here, &bytes), Some(beacon));
        assert_eq!(beacon_of(&elsewhere, &bytes), None);
        assert_eq!(beacon_of(&here, &bytes[BEACON_MAGIC.len()..]), None);
        assert_ne!(bytes, again);
        assert!(bytes.len() <= LARGEST_BEACON);
    }
}
