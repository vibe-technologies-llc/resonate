use std::{collections::VecDeque, net::SocketAddr, sync::Arc, time::Duration};

use parking_lot::RwLock;
use tokio::{
    net::{UdpSocket, lookup_host},
    time::{sleep, timeout},
};

use crate::key::random_bytes;

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

pub const TIME_ASKED_EVERY: Duration = Duration::from_secs(64);

const SAMPLES_KEPT: usize = 8;

pub const TIGHT_ROUND_TRIP: Duration = Duration::from_millis(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    round_trip: u64,
    utc_ahead_by: i128,
}

#[derive(Clone, Debug, Default)]
pub struct UtcClock {
    samples: VecDeque<Sample>,
}

impl UtcClock {
    pub fn answered(&mut self, sent_at: u64, received: u128, replied: u128, heard_at: u64) {
        let Some(here_took) = heard_at.checked_sub(sent_at) else {
            return;
        };
        let Some(there_took) = replied.checked_sub(received) else {
            return;
        };
        let Some(round_trip) = u128::from(here_took).checked_sub(there_took) else {
            return;
        };
        let Ok(round_trip) = u64::try_from(round_trip) else {
            return;
        };
        let (Ok(received), Ok(replied)) = (i128::try_from(received), i128::try_from(replied))
        else {
            return;
        };

        let utc_ahead_by =
            ((received - i128::from(sent_at)) + (replied - i128::from(heard_at))) / 2;
        if self.samples.len() == SAMPLES_KEPT {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            round_trip,
            utc_ahead_by,
        });
    }

    fn best(&self) -> Option<&Sample> {
        self.samples.iter().min_by_key(|sample| sample.round_trip)
    }

    pub fn round_trip(&self) -> Option<Duration> {
        self.best()
            .map(|sample| Duration::from_nanos(sample.round_trip))
    }

    pub fn utc_ahead_by(&self) -> Option<i128> {
        self.best().map(|sample| sample.utc_ahead_by)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyTimed {
    pub server_ahead_of_utc: i64,
    pub server_round_trip: u64,
    pub utc_round_trip: u64,
}

impl PartyTimed {
    pub fn of(server_ahead_by: i128, server_round_trip: Duration, utc: &UtcClock) -> Option<Self> {
        let utc_ahead_by = utc.utc_ahead_by()?;
        let utc_round_trip = utc.round_trip()?;

        Some(Self {
            server_ahead_of_utc: i64::try_from(server_ahead_by - utc_ahead_by).ok()?,
            server_round_trip: u64::try_from(server_round_trip.as_nanos()).ok()?,
            utc_round_trip: u64::try_from(utc_round_trip.as_nanos()).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyApart {
    pub nanos: i64,
    pub corrected: bool,
}

impl PartyApart {
    pub fn weighed(mine: PartyTimed, hosts: PartyTimed) -> Self {
        let nanos = mine
            .server_ahead_of_utc
            .saturating_sub(hosts.server_ahead_of_utc);
        let tight = u64::try_from(TIGHT_ROUND_TRIP.as_nanos()).unwrap_or(u64::MAX);
        let utc_doubt = (mine.utc_round_trip.saturating_add(hosts.utc_round_trip)) / 2;
        let path_doubt = (mine
            .server_round_trip
            .saturating_add(hosts.server_round_trip))
            / 2;
        let both_tight = mine.utc_round_trip <= tight && hosts.utc_round_trip <= tight;
        let plausible = nanos.unsigned_abs() <= path_doubt.saturating_add(utc_doubt);

        Self {
            nanos,
            corrected: both_tight && utc_doubt < path_doubt && plausible,
        }
    }

    pub fn correction(self) -> i128 {
        if self.corrected {
            i128::from(self.nanos)
        } else {
            0
        }
    }
}

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

    const MS: u64 = 1_000_000;

    fn timed(apart_ms: i64, server_round_trip_ms: u64, utc_round_trip_ms: u64) -> PartyTimed {
        PartyTimed {
            server_ahead_of_utc: apart_ms * 1_000_000,
            server_round_trip: server_round_trip_ms * MS,
            utc_round_trip: utc_round_trip_ms * MS,
        }
    }

    #[test]
    fn the_quickest_answer_sets_what_utc_reads_here_and_the_servers_own_wait_is_not_counted() {
        let mut utc = UtcClock::default();

        utc.answered(
            0,
            u128::from(1_000 * MS + 50 * MS),
            u128::from(1_000 * MS + 60 * MS),
            120 * MS,
        );
        utc.answered(
            200 * MS,
            u128::from(1_210 * MS),
            u128::from(1_211 * MS),
            221 * MS,
        );

        assert_eq!(utc.round_trip(), Some(Duration::from_millis(20)));
        assert_eq!(utc.utc_ahead_by(), Some(i128::from(1_000 * MS)));
    }

    #[test]
    fn an_answer_that_came_back_before_it_left_or_took_longer_there_than_here_is_not_believed() {
        let mut utc = UtcClock::default();

        utc.answered(100, 5_000, 5_001, 50);
        utc.answered(0, 5_000, 6_000, 10);

        assert_eq!(utc.utc_ahead_by(), None);
    }

    #[test]
    fn only_the_latest_answers_are_weighed() {
        let mut utc = UtcClock::default();

        utc.answered(0, 0, 0, MS);
        for at in 1..=u64::try_from(SAMPLES_KEPT).unwrap() {
            let sent = at * 1_000 * MS;
            utc.answered(sent, u128::from(sent), u128::from(sent), sent + 10 * MS);
        }

        assert_eq!(utc.round_trip(), Some(Duration::from_millis(10)));
    }

    #[test]
    fn a_guest_far_from_the_server_and_near_its_time_server_is_corrected_by_what_the_host_says() {
        let apart = PartyApart::weighed(timed(-30, 120, 8), timed(0, 20, 6));

        assert_eq!(apart.nanos, -30_000_000);
        assert!(apart.corrected);
        assert_eq!(apart.correction(), -30_000_000);
    }

    #[test]
    fn a_time_server_slower_to_reach_than_the_party_corrects_nothing() {
        let apart = PartyApart::weighed(timed(3, 4, 20), timed(0, 4, 20));

        assert_eq!(apart.nanos, 3_000_000);
        assert!(!apart.corrected);
        assert_eq!(apart.correction(), 0);
    }

    #[test]
    fn a_loose_time_server_on_either_side_corrects_nothing() {
        assert!(!PartyApart::weighed(timed(10, 200, 50), timed(0, 200, 5)).corrected);
        assert!(!PartyApart::weighed(timed(10, 200, 5), timed(0, 200, 50)).corrected);
    }

    #[test]
    fn a_gap_wider_than_both_paths_could_make_is_shown_and_never_corrected() {
        let apart = PartyApart::weighed(timed(500, 60, 4), timed(0, 40, 4));

        assert_eq!(apart.nanos, 500_000_000);
        assert!(!apart.corrected);
    }

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
