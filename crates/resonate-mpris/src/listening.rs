use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    mem,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;

use crate::{
    Result,
    answering::Answering,
    wire::{read_frame, write_frame},
};

pub(crate) type Sever = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Connected {
    next: u64,
    severs: BTreeMap<u64, Sever>,
}

impl Connected {
    fn hold(&mut self, sever: Sever) -> u64 {
        let id = self.next;
        self.next += 1;
        self.severs.insert(id, sever);
        id
    }

    fn sever_every_one(&mut self) {
        for sever in mem::take(&mut self.severs).into_values() {
            sever();
        }
    }
}

pub(crate) const CONNECTIONS_AT_MOST: usize = 32;
const ACCEPT_FAILURES_AT_MOST: u32 = 5;
const ACCEPT_RETRY_AFTER: Duration = Duration::from_millis(100);
const CONNECTION_THREAD: &str = "resonate-mpris-pipe-connection";

pub(crate) trait Serves: Send + Sync {
    fn reply_to(&self, frame: &[u8]) -> Option<Vec<u8>>;

    fn close(&self);
}

impl Serves for Answering {
    fn reply_to(&self, frame: &[u8]) -> Option<Vec<u8>> {
        Self::reply_to(self, frame)
    }

    fn close(&self) {
        Self::close(self);
    }
}

fn serve_while_held<S: Read + Write>(mut stream: S, held: &Weak<dyn Serves>) -> Result<()> {
    while let Some(frame) = read_frame(&mut stream)? {
        let Some(serving) = held.upgrade() else {
            return Ok(());
        };
        let Some(reply) = serving.reply_to(&frame) else {
            return Ok(());
        };
        drop(serving);
        write_frame(&mut stream, &reply)?;
    }
    Ok(())
}

pub(crate) fn accepting<S: Read + Write + Send + 'static>(
    mut accept: impl FnMut() -> io::Result<S>,
    severed_by: impl Fn(&S) -> Option<Sever>,
    serving: &Arc<dyn Serves>,
    stop: &AtomicBool,
) {
    let open = Arc::new(AtomicUsize::new(0));
    let connected = Arc::new(Mutex::new(Connected::default()));
    let mut failures = 0;

    loop {
        let accepted = accept();
        if stop.load(Ordering::Acquire) {
            connected.lock().sever_every_one();
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

        let held = Arc::downgrade(serving);
        let counted = Arc::clone(&open);
        let severing = severed_by(&stream).map(|sever| connected.lock().hold(sever));
        let released = Arc::clone(&connected);
        let spawned = thread::Builder::new()
            .name(CONNECTION_THREAD.to_owned())
            .spawn(move || {
                if let Err(error) = serve_while_held(stream, &held) {
                    tracing::debug!(%error, "a client of the pipe was let go");
                }
                if let Some(id) = severing {
                    released.lock().severs.remove(&id);
                }
                counted.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            if let Some(id) = severing {
                connected.lock().severs.remove(&id);
            }
            open.fetch_sub(1, Ordering::AcqRel);
            tracing::warn!("a client of the pipe could not be given a thread");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        collections::VecDeque,
        os::unix::net::UnixStream,
        sync::mpsc::{self, Sender},
    };

    use parking_lot::Mutex;

    use super::*;

    struct Counting {
        served: Mutex<Sender<u8>>,
    }

    impl Serves for Counting {
        fn reply_to(&self, frame: &[u8]) -> Option<Vec<u8>> {
            let _ = self
                .served
                .lock()
                .send(frame.first().copied().unwrap_or_default());
            Some(frame.to_vec())
        }

        fn close(&self) {}
    }

    fn refused() -> io::Error {
        io::Error::from(io::ErrorKind::ConnectionAborted)
    }

    fn counting() -> (Arc<dyn Serves>, mpsc::Receiver<u8>) {
        let (served, arrived) = mpsc::channel();
        let serving: Arc<dyn Serves> = Arc::new(Counting {
            served: Mutex::new(served),
        });
        (serving, arrived)
    }

    #[test]
    fn every_client_accepted_is_served_and_a_refused_one_costs_the_rest_nothing() {
        let (serving, arrived) = counting();
        let stop = Arc::new(AtomicBool::new(false));
        let mut ends = Vec::new();
        let mut offered: VecDeque<io::Result<UnixStream>> = VecDeque::new();
        for byte in [1_u8, 2, 3] {
            let (near, mut far) = UnixStream::pair().expect("a pair");
            write_frame(&mut far, &[byte]).expect("a frame sent");
            ends.push(far);
            offered.push_back(Ok(near));
            if byte == 1 {
                offered.push_back(Err(refused()));
            }
        }

        let stopping = Arc::clone(&stop);
        accepting(
            move || {
                offered.pop_front().unwrap_or_else(|| {
                    stopping.store(true, Ordering::Release);
                    Err(refused())
                })
            },
            |_| None,
            &serving,
            &stop,
        );

        let mut heard: Vec<u8> = (0..3)
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
        let (serving, _arrived) = counting();
        let stop = AtomicBool::new(false);
        let mut asked = 0;

        accepting(
            || {
                asked += 1;
                Err::<UnixStream, _>(refused())
            },
            |_| None,
            &serving,
            &stop,
        );

        assert_eq!(asked, ACCEPT_FAILURES_AT_MOST);
    }

    #[test]
    fn a_client_still_connected_when_the_player_stops_serving_is_severed_and_its_thread_ends() {
        let (serving, arrived) = counting();
        let stop = Arc::new(AtomicBool::new(false));
        let (near, mut far) = UnixStream::pair().expect("a pair");
        write_frame(&mut far, &[1]).expect("a frame sent");
        let mut offered = VecDeque::from([near]);
        let (asked_again, waiting) = mpsc::channel::<()>();

        let stopping = Arc::clone(&stop);
        let accepted = thread::spawn(move || {
            accepting(
                move || match offered.pop_front() {
                    Some(stream) => Ok(stream),
                    None => {
                        let _ = waiting.recv_timeout(Duration::from_secs(5));
                        stopping.store(true, Ordering::Release);
                        Err(refused())
                    }
                },
                |stream: &UnixStream| {
                    let held = stream.try_clone().ok()?;
                    let sever: Sever = Box::new(move || {
                        let _ = held.shutdown(std::net::Shutdown::Both);
                    });
                    Some(sever)
                },
                &serving,
                &stop,
            );
        });
        arrived
            .recv_timeout(Duration::from_secs(5))
            .expect("the client served once");
        assert!(read_frame(&mut far).expect("a reply").is_some());

        asked_again.send(()).expect("the accept loop waits");
        accepted.join().expect("the accept loop ends");

        far.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("a timeout");
        assert!(
            read_frame(&mut far)
                .expect("the end of the stream")
                .is_none(),
            "the client's connection was left open"
        );
    }

    #[test]
    fn a_client_still_connected_does_not_keep_a_player_that_stopped_serving_alive() {
        let (serving, arrived) = counting();
        let held = Arc::downgrade(&serving);
        let stop = Arc::new(AtomicBool::new(false));
        let (near, mut far) = UnixStream::pair().expect("a pair");
        write_frame(&mut far, &[1]).expect("a frame sent");
        let mut offered = VecDeque::from([near]);

        let stopping = Arc::clone(&stop);
        accepting(
            move || {
                offered.pop_front().ok_or_else(|| {
                    stopping.store(true, Ordering::Release);
                    refused()
                })
            },
            |_| None,
            &serving,
            &stop,
        );
        arrived
            .recv_timeout(Duration::from_secs(5))
            .expect("the client served once");
        assert!(read_frame(&mut far).expect("a reply").is_some());

        drop(serving);

        assert!(
            held.upgrade().is_none(),
            "a client that stayed connected held the player"
        );
        drop(far);
    }
}
