use std::{
    io::{self, Write},
    net::TcpStream,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_audio::{AudioSource, GraphTime, StreamClock};
use resonate_core::{Ratio, StreamSpec};

use crate::{Carriage, http::USER_AGENT, wire::Wire};

const QUANTA_A_SECOND: u64 = 100;

const IDLE_TICK: Duration = Duration::from_millis(10);

const SOURCE_HANDED_WITHIN: Duration = Duration::from_secs(2);

pub(crate) const WRITE_WAKES_EVERY: Duration = Duration::from_millis(250);

const NOT_ENDED: u64 = u64::MAX;

const NANOS_A_SECOND: u128 = 1_000_000_000;

pub struct Feed {
    spec: StreamSpec,
    carriage: Carriage,
    source: Mutex<Option<Box<dyn AudioSource>>>,
    clock: Arc<StreamClock>,
    active: AtomicBool,
    draining: AtomicBool,
    closed: AtomicBool,
    connections: AtomicU64,
    served: AtomicU64,
    ended_at: AtomicU64,
}

impl Feed {
    pub fn new(
        spec: StreamSpec,
        carriage: Carriage,
        source: Box<dyn AudioSource>,
        clock: Arc<StreamClock>,
    ) -> Self {
        Self {
            spec,
            carriage,
            source: Mutex::new(Some(source)),
            clock,
            active: AtomicBool::new(false),
            draining: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            connections: AtomicU64::new(0),
            served: AtomicU64::new(0),
            ended_at: AtomicU64::new(NOT_ENDED),
        }
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    pub fn drain(&self) {
        self.draining.store(true, Ordering::Release);
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub fn ended_at(&self) -> Option<u64> {
        Some(self.ended_at.load(Ordering::Acquire)).filter(|frames| *frames != NOT_ENDED)
    }

    pub fn frames_of(&self, heard: Duration) -> u64 {
        let frames = heard.as_nanos() * u128::from(self.spec.rate.hz()) / NANOS_A_SECOND;
        u64::try_from(frames).unwrap_or(u64::MAX)
    }

    pub fn heard_through(&self, heard: Duration) {
        let served = self.served.load(Ordering::Acquire);
        let ahead = served.saturating_sub(self.frames_of(heard));
        let rate = NonZeroU32::new(self.spec.rate.hz()).unwrap_or(NonZeroU32::MIN);
        self.clock.note(
            GraphTime {
                delay: i64::try_from(ahead).unwrap_or(i64::MAX),
                buffered: 0,
                queued: 0,
                tick: Ratio {
                    numer: NonZeroU32::MIN,
                    denom: rate,
                },
            },
            self.spec.rate,
        );
    }

    pub(crate) fn serve(&self, stream: &mut TcpStream, head_only: bool) -> io::Result<()> {
        let mine = self
            .connections
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        stream.set_write_timeout(Some(WRITE_WAKES_EVERY))?;
        stream.write_all(self.answer_head().as_bytes())?;
        if head_only {
            return Ok(());
        }
        let (mut wire, head) = Wire::begin(&self.carriage, self.spec).map_err(io::Error::other)?;
        stream.write_all(&head)?;

        let Some(mut source) = self.taken(mine) else {
            return Ok(());
        };
        let poured = self.pour(stream, source.as_mut(), &mut wire, mine);
        *self.source.lock() = Some(source);
        poured
    }

    fn answer_head(&self) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nConnection: close\r\nCache-Control: \
             no-cache\r\nAccept-Ranges: none\r\nServer: {USER_AGENT}\r\ntransferMode.dlna.org: \
             Streaming\r\ncontentFeatures.dlna.org: DLNA.ORG_OP=00;DLNA.ORG_CI=0\r\n\r\n",
            self.carriage.kind_of(self.spec)
        )
    }

    fn superseded(&self, mine: u64) -> bool {
        self.closed.load(Ordering::Acquire) || self.connections.load(Ordering::Acquire) != mine
    }

    fn taken(&self, mine: u64) -> Option<Box<dyn AudioSource>> {
        let since = Instant::now();
        loop {
            if let Some(source) = self.source.lock().take() {
                return Some(source);
            }
            if self.superseded(mine) || since.elapsed() > SOURCE_HANDED_WITHIN {
                return None;
            }
            thread::sleep(IDLE_TICK);
        }
    }

    fn pour(
        &self,
        stream: &mut TcpStream,
        source: &mut dyn AudioSource,
        wire: &mut Wire,
        mine: u64,
    ) -> io::Result<()> {
        let rate = u64::from(self.spec.rate.hz());
        let stride = self.spec.bytes_per_frame().get() as usize;
        let quantum = (rate / QUANTA_A_SECOND).max(1);
        let mut ring = vec![0; quantum as usize * stride];
        let mut carried = Vec::new();

        let mut from = Instant::now();
        let mut sent = 0_u64;
        loop {
            if self.superseded(mine) {
                return Ok(());
            }
            if !self.active.load(Ordering::Acquire) {
                thread::sleep(IDLE_TICK);
                from = Instant::now();
                sent = 0;
                continue;
            }

            let due = frames_in(from.elapsed(), rate);
            if sent > due {
                thread::sleep(lasting(sent - due, rate).min(IDLE_TICK));
                continue;
            }
            if due > sent + quantum {
                from += lasting(due - sent - quantum, rate);
            }

            let filled = source.fill_stamped(&mut ring, self.clock.stamp());
            let whole = filled / stride * stride;
            let pulled = (whole / stride) as u64;
            let ending = self.draining.load(Ordering::Acquire) && filled < ring.len();

            let mut written = wire
                .carry(ring.get(..whole).unwrap_or_default(), pulled, &mut carried)
                .map_err(io::Error::other)?;
            if !self.written(stream, &carried, mine)? {
                return Ok(());
            }
            if ending {
                written += wire.finish(&mut carried).map_err(io::Error::other)?;
                if !self.written(stream, &carried, mine)? {
                    return Ok(());
                }
            }
            sent = sent.saturating_add(pulled);
            let served = self.served.fetch_add(written, Ordering::AcqRel) + written;

            if ending {
                self.ended_at.store(served, Ordering::Release);
                return Ok(());
            }
        }
    }

    fn written(&self, stream: &mut TcpStream, mut bytes: &[u8], mine: u64) -> io::Result<bool> {
        while !bytes.is_empty() {
            match stream.write(bytes) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(written) => bytes = bytes.get(written..).unwrap_or_default(),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    if self.superseded(mine) {
                        return Ok(false);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }
}

fn frames_in(elapsed: Duration, rate: u64) -> u64 {
    u64::try_from(elapsed.as_nanos() * u128::from(rate) / NANOS_A_SECOND).unwrap_or(u64::MAX)
}

fn lasting(frames: u64, rate: u64) -> Duration {
    let nanos = u128::from(frames) * NANOS_A_SECOND / u128::from(rate.max(1));
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_their_lasting_agree() {
        assert_eq!(frames_in(Duration::from_millis(500), 44_100), 22_050);
        assert_eq!(lasting(44_100, 44_100), Duration::from_secs(1));
        assert_eq!(lasting(441, 44_100), Duration::from_millis(10));
    }
}
