use std::{
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use crate::answering::Answering;

pub(crate) const CONNECTIONS_AT_MOST: usize = 32;
const ACCEPT_FAILURES_AT_MOST: u32 = 5;
const ACCEPT_RETRY_AFTER: Duration = Duration::from_millis(100);
const CONNECTION_THREAD: &str = "resonate-mpris-pipe-connection";

pub(crate) trait Serves<S>: Send + Sync {
    fn serve(&self, stream: S);

    fn close(&self);
}

impl<S: Read + Write> Serves<S> for Answering {
    fn serve(&self, stream: S) {
        if let Err(error) = Self::serve(self, stream) {
            tracing::debug!(%error, "a client of the pipe was let go");
        }
    }

    fn close(&self) {
        Self::close(self);
    }
}

pub(crate) fn accepting<S: Send + 'static>(
    mut accept: impl FnMut() -> io::Result<S>,
    serving: &Arc<dyn Serves<S>>,
    stop: &AtomicBool,
) {
    let open = Arc::new(AtomicUsize::new(0));
    let mut failures = 0;

    loop {
        let accepted = accept();
        if stop.load(Ordering::Acquire) {
            return;
        }
        let stream = match accepted {
            Ok(stream) => {
                failures = 0;
                stream
            }
            Err(error) => {
                failures += 1;
                tracing::debug!(%error, "a client of the pipe was not accepted");
                if failures >= ACCEPT_FAILURES_AT_MOST {
                    tracing::warn!("the pipe has stopped accepting clients");
                    return;
                }
                thread::sleep(ACCEPT_RETRY_AFTER);
                continue;
            }
        };

        if open.fetch_add(1, Ordering::AcqRel) >= CONNECTIONS_AT_MOST {
            open.fetch_sub(1, Ordering::AcqRel);
            tracing::debug!("a client of the pipe was turned away: too many are connected");
            continue;
        }

        let serving = Arc::clone(serving);
        let counted = Arc::clone(&open);
        let spawned = thread::Builder::new()
            .name(CONNECTION_THREAD.to_owned())
            .spawn(move || {
                serving.serve(stream);
                counted.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            open.fetch_sub(1, Ordering::AcqRel);
            tracing::warn!("a client of the pipe could not be given a thread");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::mpsc::{self, Sender},
    };

    use parking_lot::Mutex;

    use super::*;

    struct Counting {
        served: Mutex<Sender<u32>>,
    }

    impl Serves<u32> for Counting {
        fn serve(&self, stream: u32) {
            let _ = self.served.lock().send(stream);
        }

        fn close(&self) {}
    }

    fn refused() -> io::Error {
        io::Error::from(io::ErrorKind::ConnectionAborted)
    }

    #[test]
    fn every_client_accepted_is_served_and_a_refused_one_costs_the_rest_nothing() {
        let (served, arrived) = mpsc::channel();
        let serving: Arc<dyn Serves<u32>> = Arc::new(Counting {
            served: Mutex::new(served),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let mut offered: VecDeque<io::Result<u32>> =
            VecDeque::from([Ok(1), Err(refused()), Ok(2), Ok(3)]);

        let stopping = Arc::clone(&stop);
        accepting(
            move || {
                offered.pop_front().unwrap_or_else(|| {
                    stopping.store(true, Ordering::Release);
                    Err(refused())
                })
            },
            &serving,
            &stop,
        );

        let mut heard: Vec<u32> = (0..3)
            .map(|_| {
                arrived
                    .recv_timeout(Duration::from_secs(5))
                    .expect("a client served")
            })
            .collect();
        heard.sort_unstable();
        assert_eq!(heard, vec![1, 2, 3]);
    }

    #[test]
    fn a_listener_that_keeps_refusing_is_given_up_on() {
        let (served, _arrived) = mpsc::channel();
        let serving: Arc<dyn Serves<u32>> = Arc::new(Counting {
            served: Mutex::new(served),
        });
        let stop = AtomicBool::new(false);
        let mut asked = 0;

        accepting(
            || {
                asked += 1;
                Err(refused())
            },
            &serving,
            &stop,
        );

        assert_eq!(asked, ACCEPT_FAILURES_AT_MOST);
    }
}
