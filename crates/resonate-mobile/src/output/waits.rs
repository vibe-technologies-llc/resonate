use std::{io, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use resonate_audio::{Error, Operation, Result, StreamState};

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

pub(crate) fn opened_within(started: &Receiver<Result<()>>, within: Duration) -> Result<()> {
    match started.recv_timeout(within) {
        Ok(opened) => opened,
        Err(RecvTimeoutError::Timeout) => Err(Error::Backend {
            op: Operation::Open,
            source: io::ErrorKind::TimedOut.into(),
        }),
        Err(RecvTimeoutError::Disconnected) => Err(Error::LoopStopped),
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;

    use super::*;

    #[test]
    fn an_open_that_never_answers_times_out_instead_of_blocking() {
        let (_never_sends, started) = bounded::<Result<()>>(1);

        let answer = opened_within(&started, Duration::from_millis(20));

        assert!(matches!(
            answer,
            Err(Error::Backend {
                op: Operation::Open,
                ..
            })
        ));
    }

    #[test]
    fn an_open_that_answers_is_passed_through() {
        let (ready, started) = bounded(2);

        ready.send(Ok(())).unwrap();
        assert!(opened_within(&started, OPEN_WITHIN).is_ok());

        ready.send(Err(Error::FormatRefused)).unwrap();
        assert!(matches!(
            opened_within(&started, OPEN_WITHIN),
            Err(Error::FormatRefused)
        ));
    }

    #[test]
    fn an_output_thread_that_died_before_answering_is_a_stopped_loop() {
        let (ready, started) = bounded::<Result<()>>(1);

        drop(ready);

        assert!(matches!(
            opened_within(&started, OPEN_WITHIN),
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
