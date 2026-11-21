use std::{io, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use resonate_audio::{Error, Operation, Result, StreamState};

use super::oboe_state::Claim;

pub(crate) const OPEN_WITHIN: Duration = Duration::from_secs(3);
const DRAINING_POLL: Duration = Duration::from_millis(10);
const STREAMING_POLL: Duration = Duration::from_millis(100);
const FAILURE_POLL: Duration = Duration::from_millis(500);

pub(crate) fn between_commands(state: StreamState, draining: bool) -> Duration {
    if draining {
        DRAINING_POLL
    } else if state == StreamState::Streaming {
        STREAMING_POLL
    } else {
        FAILURE_POLL
    }
}

pub(crate) fn opened_within<T>(
    started: &Receiver<Result<T>>,
    within: Duration,
    claim: &Claim,
) -> Result<T> {
    match started.recv_timeout(within) {
        Ok(opened) => opened,
        Err(RecvTimeoutError::Timeout) if claim.abandon() => Err(Error::Backend {
            op: Operation::Open,
            source: io::ErrorKind::TimedOut.into(),
        }),
        Err(RecvTimeoutError::Timeout) => started.try_recv().unwrap_or(Err(Error::LoopStopped)),
        Err(RecvTimeoutError::Disconnected) => Err(Error::LoopStopped),
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;

    use super::*;
    use crate::output::{
        formats::{OutputEncoding, OutputFormat, OutputPath, OutputStream},
        oboe_state::Routing,
    };

    const BRIEFLY: Duration = Duration::from_millis(20);

    fn mixed() -> OutputStream {
        OutputStream {
            format: OutputFormat {
                rate: 48_000,
                encoding: OutputEncoding::Float,
                channels: 2,
            },
            path: OutputPath::Mixed,
        }
    }

    #[test]
    fn an_open_that_never_answers_times_out_instead_of_blocking() {
        let routing = Routing::new();
        let claim = routing.claim();
        let (_never_sends, started) = bounded::<Result<()>>(1);

        let answer = opened_within(&started, BRIEFLY, &claim);

        assert!(claim.abandoned());
        assert!(matches!(
            answer,
            Err(Error::Backend {
                op: Operation::Open,
                ..
            })
        ));
    }

    #[test]
    fn a_stream_that_opens_after_its_open_timed_out_moves_nothing_and_is_not_told() {
        let routing = Routing::new();
        let claim = routing.claim();
        let (ready, started) = bounded::<Result<()>>(1);

        let answer = opened_within(&started, BRIEFLY, &claim);
        let told = routing.answer(&claim, 31, mixed(), || {
            let _ = ready.send(Ok(()));
        });

        assert!(answer.is_err());
        assert_eq!(told, None);
        assert_eq!(routing.actual(), 0);
        assert_eq!(routing.stream(), None);
        assert!(started.is_empty());
    }

    #[test]
    fn an_open_answered_before_its_wait_ran_out_is_taken_and_not_abandoned() {
        let routing = Routing::new();
        let claim = routing.claim();
        let (ready, started) = bounded::<Result<()>>(1);

        let told = routing.answer(&claim, 31, mixed(), || {
            let _ = ready.send(Ok(()));
        });
        let answer = opened_within(&started, Duration::ZERO, &claim);

        assert!(told.is_some());
        assert!(answer.is_ok());
        assert!(!claim.abandoned());
    }

    #[test]
    fn an_open_that_answers_is_passed_through() {
        let routing = Routing::new();
        let claim = routing.claim();
        let (ready, started) = bounded(2);

        ready.send(Ok(())).unwrap();

        assert!(opened_within(&started, OPEN_WITHIN, &claim).is_ok());

        ready.send(Err(Error::FormatRefused)).unwrap();

        assert!(matches!(
            opened_within(&started, OPEN_WITHIN, &claim),
            Err(Error::FormatRefused)
        ));
    }

    #[test]
    fn an_output_thread_that_died_before_answering_is_a_stopped_loop() {
        let routing = Routing::new();
        let claim = routing.claim();
        let (ready, started) = bounded::<Result<()>>(1);

        drop(ready);

        assert!(matches!(
            opened_within(&started, OPEN_WITHIN, &claim),
            Err(Error::LoopStopped)
        ));
    }

    #[test]
    fn a_stream_not_streaming_wakes_least_often_and_a_draining_one_most() {
        let paused = between_commands(StreamState::Paused, false);
        let streaming = between_commands(StreamState::Streaming, false);
        let draining = between_commands(StreamState::Streaming, true);

        assert!(draining < streaming);
        assert!(streaming < paused);
        assert!(paused <= Duration::from_secs(1));
    }
}
