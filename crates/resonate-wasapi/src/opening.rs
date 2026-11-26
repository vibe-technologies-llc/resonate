use resonate_audio::{Error, Operation, SinkId, StreamState};
use resonate_core::SampleRate;

const HUNDRED_NANOSECONDS_A_SECOND: u64 = 10_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status(pub i32);

impl Status {
    pub const DEVICE_INVALIDATED: Self = Self::of(0x8889_0004);
    pub const UNSUPPORTED_FORMAT: Self = Self::of(0x8889_0008);
    pub const DEVICE_IN_USE: Self = Self::of(0x8889_000A);
    pub const EXCLUSIVE_MODE_NOT_ALLOWED: Self = Self::of(0x8889_000E);
    pub const BUFFER_SIZE_NOT_ALIGNED: Self = Self::of(0x8889_0019);
    pub const NOT_FOUND: Self = Self::of(0x8007_0490);

    const fn of(code: u32) -> Self {
        Self(i32::from_ne_bytes(code.to_ne_bytes()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Gone,
    Held,
    Format,
}

impl Refusal {
    pub const fn of(status: Status) -> Option<Self> {
        match status {
            Status::DEVICE_INVALIDATED | Status::NOT_FOUND => Some(Self::Gone),
            Status::DEVICE_IN_USE | Status::EXCLUSIVE_MODE_NOT_ALLOWED => Some(Self::Held),
            Status::UNSUPPORTED_FORMAT => Some(Self::Format),
            _ => None,
        }
    }
}

pub fn refused_at_open(node: Option<SinkId>, op: Operation, status: Status) -> Option<Error> {
    let refused = match (Refusal::of(status)?, node) {
        (Refusal::Gone, Some(node)) => Error::SinkGone { node },
        (Refusal::Gone | Refusal::Held, None) => Error::NoSink,
        (Refusal::Held, Some(node)) => Error::StreamRefused {
            node,
            previous: StreamState::Connecting,
            op,
            status: status.0,
        },
        (Refusal::Format, _) => Error::FormatRefused,
    };
    Some(refused)
}

pub trait Initialize {
    type Client;
    type Fault;

    fn activated(&mut self) -> Result<Self::Client, Self::Fault>;
    fn initialized(
        &mut self,
        client: &mut Self::Client,
        period_hns: i64,
    ) -> Result<(), Self::Fault>;
    fn buffer_frames(&self, client: &Self::Client) -> Result<u32, Self::Fault>;
    fn status_of(fault: &Self::Fault) -> Option<Status>;
}

pub fn initialized<I: Initialize>(
    opening: &mut I,
    mut client: I::Client,
    period_hns: i64,
    rate: SampleRate,
) -> Result<I::Client, I::Fault> {
    let Err(fault) = opening.initialized(&mut client, period_hns) else {
        return Ok(client);
    };

    if I::status_of(&fault) != Some(Status::BUFFER_SIZE_NOT_ALIGNED) {
        return Err(fault);
    }

    let aligned_frames = opening.buffer_frames(&client)?;

    drop(client);

    let mut again = opening.activated()?;

    opening.initialized(&mut again, period_of(aligned_frames, rate))?;
    Ok(again)
}

pub fn period_of(frames: u32, rate: SampleRate) -> i64 {
    let hz = u64::from(rate.hz());
    let hundreds = (u64::from(frames) * HUNDRED_NANOSECONDS_A_SECOND + hz / 2) / hz;

    i64::try_from(hundreds).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Activated(u32),
        Initialized { client: u32, period_hns: i64 },
        BufferFrames(u32),
        LetGo(u32),
    }

    type Log = Arc<Mutex<Vec<Call>>>;

    struct Client {
        number: u32,
        log: Log,
    }

    impl Drop for Client {
        fn drop(&mut self) {
            self.log.lock().push(Call::LetGo(self.number));
        }
    }

    struct Fake {
        log: Log,
        activated: u32,
        refusals: Vec<Status>,
        aligned_frames: u32,
    }

    impl Fake {
        fn refusing(refusals: Vec<Status>) -> Self {
            Self {
                log: Arc::default(),
                activated: 0,
                refusals,
                aligned_frames: 1_024,
            }
        }

        fn first_client(&mut self) -> Client {
            self.activated().expect("a client")
        }

        fn calls(&self) -> Vec<Call> {
            self.log.lock().clone()
        }
    }

    impl Initialize for Fake {
        type Client = Client;
        type Fault = Status;

        fn activated(&mut self) -> Result<Client, Status> {
            self.activated += 1;
            self.log.lock().push(Call::Activated(self.activated));
            Ok(Client {
                number: self.activated,
                log: Arc::clone(&self.log),
            })
        }

        fn initialized(&mut self, client: &mut Client, period_hns: i64) -> Result<(), Status> {
            self.log.lock().push(Call::Initialized {
                client: client.number,
                period_hns,
            });
            if self.refusals.is_empty() {
                return Ok(());
            }
            Err(self.refusals.remove(0))
        }

        fn buffer_frames(&self, client: &Client) -> Result<u32, Status> {
            self.log.lock().push(Call::BufferFrames(client.number));
            Ok(self.aligned_frames)
        }

        fn status_of(fault: &Status) -> Option<Status> {
            Some(*fault)
        }
    }

    #[test]
    fn a_misaligned_buffer_is_asked_again_of_a_fresh_client_at_the_size_the_device_aligns_to() {
        let mut fake = Fake::refusing(vec![Status::BUFFER_SIZE_NOT_ALIGNED]);

        let first = fake.first_client();

        let opened = initialized(&mut fake, first, 100_000, SampleRate::HZ_44100);

        assert!(matches!(opened, Ok(Client { number: 2, .. })));
        assert_eq!(
            fake.calls(),
            [
                Call::Activated(1),
                Call::Initialized {
                    client: 1,
                    period_hns: 100_000
                },
                Call::BufferFrames(1),
                Call::LetGo(1),
                Call::Activated(2),
                Call::Initialized {
                    client: 2,
                    period_hns: 232_200
                },
            ]
        );
    }

    #[test]
    fn a_buffer_misaligned_twice_or_another_refusal_is_answered_without_another_try() {
        let mut twice = Fake::refusing(vec![
            Status::BUFFER_SIZE_NOT_ALIGNED,
            Status::BUFFER_SIZE_NOT_ALIGNED,
        ]);
        let mut held = Fake::refusing(vec![Status::DEVICE_IN_USE]);
        let first_twice = twice.first_client();
        let first_held = held.first_client();

        let misaligned = initialized(&mut twice, first_twice, 100_000, SampleRate::HZ_48000);
        let refused = initialized(&mut held, first_held, 100_000, SampleRate::HZ_48000);

        assert!(matches!(misaligned, Err(Status::BUFFER_SIZE_NOT_ALIGNED)));
        assert_eq!(
            twice
                .calls()
                .iter()
                .filter(|call| matches!(call, Call::Activated(_)))
                .count(),
            2
        );
        assert!(matches!(refused, Err(Status::DEVICE_IN_USE)));
        assert_eq!(
            held.calls(),
            [
                Call::Activated(1),
                Call::Initialized {
                    client: 1,
                    period_hns: 100_000
                },
                Call::LetGo(1),
            ]
        );
    }

    #[test]
    fn a_period_is_the_aligned_frames_in_hundreds_of_nanoseconds_rounded() {
        assert_eq!(period_of(480, SampleRate::HZ_48000), 100_000);
        assert_eq!(period_of(1_024, SampleRate::HZ_44100), 232_200);
        assert_eq!(period_of(441, SampleRate::HZ_44100), 100_000);
    }

    #[test]
    fn an_endpoint_unplugged_or_unknown_at_open_is_gone_and_one_held_is_refused() {
        let node = SinkId::new(7);

        let unplugged = refused_at_open(Some(node), Operation::Open, Status::DEVICE_INVALIDATED);
        let unknown = refused_at_open(Some(node), Operation::Open, Status::NOT_FOUND);
        let held = refused_at_open(Some(node), Operation::Format, Status::DEVICE_IN_USE);
        let barred = refused_at_open(
            Some(node),
            Operation::Format,
            Status::EXCLUSIVE_MODE_NOT_ALLOWED,
        );
        let no_default = refused_at_open(None, Operation::Open, Status::NOT_FOUND);
        let format = refused_at_open(Some(node), Operation::Format, Status::UNSUPPORTED_FORMAT);
        let other = refused_at_open(Some(node), Operation::Open, Status(-1));

        assert!(matches!(unplugged, Some(Error::SinkGone { node: gone }) if gone == node));
        assert!(matches!(unknown, Some(Error::SinkGone { node: gone }) if gone == node));
        assert!(matches!(
            held,
            Some(Error::StreamRefused {
                node: refused,
                previous: StreamState::Connecting,
                op: Operation::Format,
                status,
            }) if refused == node && status == Status::DEVICE_IN_USE.0
        ));
        assert!(matches!(barred, Some(Error::StreamRefused { .. })));
        assert!(matches!(no_default, Some(Error::NoSink)));
        assert!(matches!(format, Some(Error::FormatRefused)));
        assert!(other.is_none());
    }
}
