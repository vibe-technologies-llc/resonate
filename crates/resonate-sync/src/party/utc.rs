use std::{net::SocketAddr, sync::Arc, time::Duration};

use parking_lot::RwLock;
use tokio::{
    net::{UdpSocket, lookup_host},
    time::{sleep, timeout},
};

use crate::{
    key::random_bytes,
    party::time::{TIME_ASKED_EVERY, UtcClock},
};

const TIME_PORT: u16 = 123;

const PACKET_BYTES: usize = 48;

const ASKED_AS_A_CLIENT_OF_VERSION_FOUR: u8 = 0b0010_0011;

const ANSWERED_AS_A_SERVER: u8 = 4;

const UNSYNCHRONISED: u8 = 3;

const SECONDS_FROM_1900_TO_1970: u64 = 2_208_988_800;

const NANOS_A_SECOND: u128 = 1_000_000_000;

const ANSWERED_WITHIN: Duration = Duration::from_secs(2);

const ASKED_AT_FIRST: usize = 4;

const ASKED_AT_FIRST_APART: Duration = Duration::from_secs(2);

fn asked(nonce: [u8; 8]) -> [u8; PACKET_BYTES] {
    let mut packet = [0; PACKET_BYTES];

    packet[0] = ASKED_AS_A_CLIENT_OF_VERSION_FOUR;
    packet[40..48].copy_from_slice(&nonce);
    packet
}

fn nanos_since_1970(stamp: &[u8]) -> Option<u128> {
    let seconds = u64::from(u32::from_be_bytes(stamp.get(0..4)?.try_into().ok()?));
    let fraction = u128::from(u32::from_be_bytes(stamp.get(4..8)?.try_into().ok()?));
    let seconds = seconds.checked_sub(SECONDS_FROM_1900_TO_1970)?;

    Some(u128::from(seconds) * NANOS_A_SECOND + ((fraction * NANOS_A_SECOND) >> 32))
}

fn answered(packet: &[u8], nonce: [u8; 8]) -> Option<(u128, u128)> {
    if packet.len() < PACKET_BYTES {
        return None;
    }

    let leap = packet[0] >> 6;
    let mode = packet[0] & 0b111;
    let stratum = packet[1];

    if leap == UNSYNCHRONISED || mode != ANSWERED_AS_A_SERVER || !(1..16).contains(&stratum) {
        return None;
    }
    if packet[24..32] != nonce {
        return None;
    }

    Some((
        nanos_since_1970(&packet[32..40])?,
        nanos_since_1970(&packet[40..48])?,
    ))
}

async fn reached(server: &str) -> Option<SocketAddr> {
    if let Ok(mut found) = lookup_host(server).await
        && let Some(address) = found.next()
    {
        return Some(address);
    }

    lookup_host((server, TIME_PORT)).await.ok()?.next()
}

async fn asked_once(
    socket: &UdpSocket,
    now_nanos: &(dyn Fn() -> u64 + Send + Sync),
) -> Option<(u64, u128, u128, u64)> {
    let nonce: [u8; 8] = random_bytes().ok()?;
    let sent_at = now_nanos();

    socket.send(&asked(nonce)).await.ok()?;

    let mut heard = [0; PACKET_BYTES * 2];
    timeout(ANSWERED_WITHIN, async {
        loop {
            let read = socket.recv(&mut heard).await.ok()?;
            let heard_at = now_nanos();
            if let Some((received, replied)) = answered(&heard[..read], nonce) {
                return Some((sent_at, received, replied, heard_at));
            }
        }
    })
    .await
    .ok()
    .flatten()
}

pub async fn keep_utc(
    server: String,
    now_nanos: Arc<dyn Fn() -> u64 + Send + Sync>,
    utc: Arc<RwLock<UtcClock>>,
) {
    let mut asked = 0;
    loop {
        if let Some(address) = reached(&server).await {
            let any: SocketAddr = if address.is_ipv4() {
                ([0, 0, 0, 0], 0).into()
            } else {
                (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
            };
            if let Ok(socket) = UdpSocket::bind(any).await
                && socket.connect(address).await.is_ok()
                && let Some((sent_at, received, replied, heard_at)) =
                    asked_once(&socket, now_nanos.as_ref()).await
            {
                utc.write().answered(sent_at, received, replied, heard_at);
            }
        }

        asked += 1;
        sleep(if asked < ASKED_AT_FIRST {
            ASKED_AT_FIRST_APART
        } else {
            TIME_ASKED_EVERY
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_says_nothing_but_its_version_its_mode_and_a_stamp_of_chance() {
        let nonce = [7; 8];
        let packet = asked(nonce);

        assert_eq!(packet[0], 0b0010_0011);
        assert!(packet[1..40].iter().all(|byte| *byte == 0));
        assert_eq!(packet[40..48], nonce);
    }

    fn answer(first: u8, stratum: u8, origin: [u8; 8]) -> [u8; PACKET_BYTES] {
        let mut packet = [0; PACKET_BYTES];

        packet[0] = first;
        packet[1] = stratum;
        packet[24..32].copy_from_slice(&origin);

        let seconds = u32::try_from(SECONDS_FROM_1900_TO_1970 + 10).unwrap();

        packet[32..36].copy_from_slice(&seconds.to_be_bytes());
        packet[36..40].copy_from_slice(&(1_u32 << 31).to_be_bytes());
        packet[40..44].copy_from_slice(&seconds.to_be_bytes());
        packet[44..48].copy_from_slice(&(3_u32 << 30).to_be_bytes());
        packet
    }

    #[test]
    fn an_answer_is_read_only_from_a_synchronised_server_echoing_the_stamp_it_was_sent() {
        let nonce = [9; 8];

        assert_eq!(
            answered(&answer(0b0010_0100, 2, nonce), nonce),
            Some((10_500_000_000, 10_750_000_000))
        );
        assert_eq!(answered(&answer(0b1110_0100, 2, nonce), nonce), None);
        assert_eq!(answered(&answer(0b0010_0011, 2, nonce), nonce), None);
        assert_eq!(answered(&answer(0b0010_0100, 0, nonce), nonce), None);
        assert_eq!(answered(&answer(0b0010_0100, 2, [1; 8]), nonce), None);
        assert_eq!(answered(&[0; 20], nonce), None);
    }
}
