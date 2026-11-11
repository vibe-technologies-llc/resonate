use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded};
use resonate_audio::SinkInfo;

use crate::{SinkResult, Surveyor};

pub(crate) type Surveyed = SinkResult<Vec<SinkInfo>>;

pub(crate) struct Surveying {
    asks: Option<Sender<()>>,
    answers: Receiver<Surveyed>,
    asked: bool,
    thread: Option<JoinHandle<()>>,
}

impl Surveying {
    pub(crate) fn start(surveyor: Arc<dyn Surveyor>, within: Duration) -> Option<Self> {
        let (asks, asked) = bounded::<()>(1);
        let (answer, answers) = bounded(1);
        let thread = thread::Builder::new()
            .name("resonate-sinks".to_owned())
            .spawn(move || {
                for () in asked {
                    if answer.send(surveyor.enumerate_sinks(within)).is_err() {
                        return;
                    }
                }
            })
            .inspect_err(|error| {
                tracing::warn!(%error, "no thread could be started to follow the sinks");
            })
            .ok()?;

        Some(Self {
            asks: Some(asks),
            answers,
            asked: false,
            thread: Some(thread),
        })
    }

    pub(crate) const fn answers(&self) -> &Receiver<Surveyed> {
        &self.answers
    }

    pub(crate) fn ask(&mut self) -> bool {
        if self.asked {
            return false;
        }
        let Some(asks) = self.asks.as_ref() else {
            return false;
        };
        self.asked = asks.try_send(()).is_ok();
        self.asked
    }

    pub(crate) const fn is_asked(&self) -> bool {
        self.asked
    }

    pub(crate) fn answered(&mut self) -> Option<Surveyed> {
        match self.answers.try_recv() {
            Ok(found) => {
                self.asked = false;
                Some(found)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.asked = false;
                self.asks = None;
                None
            }
        }
    }

    pub(crate) const fn is_running(&self) -> bool {
        self.asks.is_some()
    }

    pub(crate) fn stop(&mut self) {
        self.asks = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        while self.answers.try_recv().is_ok() {}
    }
}

impl Drop for Surveying {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;
    use resonate_audio::Error;

    use super::*;

    enum Doing {
        Answer,
        Panic,
        Wait(Receiver<()>),
    }

    struct Scripted {
        doing: Mutex<Vec<Doing>>,
        asked: Mutex<u32>,
    }

    impl Scripted {
        fn of(doing: Vec<Doing>) -> Arc<Self> {
            Arc::new(Self {
                doing: Mutex::new(doing),
                asked: Mutex::new(0),
            })
        }
    }

    impl Surveyor for Scripted {
        fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
            *self.asked.lock() += 1;
            let doing = self.doing.lock().pop().unwrap_or(Doing::Answer);
            match doing {
                Doing::Answer => Ok(Vec::new()),
                Doing::Panic => panic!("the surveyor failed"),
                Doing::Wait(released) => {
                    let _ = released.recv();
                    Err(Error::NoSink)
                }
            }
        }
    }

    fn answered_in_time(surveying: &mut Surveying) -> Option<Surveyed> {
        loop {
            if let Some(found) = surveying.answered() {
                return Some(found);
            }
            if !surveying.is_running() {
                return None;
            }
            thread::yield_now();
        }
    }

    #[test]
    fn a_survey_is_asked_once_until_it_answers_and_then_can_be_asked_again() {
        let surveyor = Scripted::of(vec![Doing::Answer, Doing::Answer]);
        let mut surveying =
            Surveying::start(surveyor.clone(), Duration::from_secs(1)).expect("a worker");

        let first = surveying.ask();
        let again = surveying.ask();
        let answered = answered_in_time(&mut surveying).expect("an answer");
        let after = surveying.ask();
        let second = answered_in_time(&mut surveying).expect("an answer");

        assert!(first);
        assert!(!again);
        assert!(answered.is_ok());
        assert!(after);
        assert!(second.is_ok());
        assert_eq!(*surveyor.asked.lock(), 2);
        assert!(!surveying.is_asked());
    }

    #[test]
    fn a_survey_worker_that_ends_is_no_longer_asked_and_no_longer_waited_on() {
        let surveyor = Scripted::of(vec![Doing::Panic]);
        let mut surveying = Surveying::start(surveyor, Duration::from_secs(1)).expect("a worker");

        let asked = surveying.ask();
        let found = answered_in_time(&mut surveying);

        assert!(asked);
        assert!(found.is_none());
        assert!(!surveying.is_asked());
        assert!(!surveying.is_running());
        assert!(!surveying.ask());
        assert!(surveying.answered().is_none());
    }

    #[test]
    fn stopping_waits_for_a_survey_in_flight_and_a_stopped_survey_is_not_asked() {
        let (release, released) = crossbeam_channel::unbounded();
        let surveyor = Scripted::of(vec![Doing::Wait(released)]);
        let mut surveying =
            Surveying::start(surveyor.clone(), Duration::from_secs(1)).expect("a worker");

        surveying.ask();
        let releasing = thread::spawn(move || {
            let _ = release.send(());
        });
        surveying.stop();
        releasing.join().expect("released");

        assert!(surveying.thread.is_none());
        assert!(!surveying.is_running());
        assert!(!surveying.ask());
        assert!(surveying.answered().is_none());
    }
}
