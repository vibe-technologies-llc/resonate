use std::{
    io, mem,
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded, select};
use parking_lot::{Condvar, Mutex};
use resonate_audio::{Error, Operation, SinkInfo, SinkResult};

pub struct Surveys {
    asks: Sender<()>,
    answers: Arc<Answers>,
    first: Receiver<Vec<SinkInfo>>,
}

#[derive(Default)]
struct Answers {
    held: Mutex<Held>,
    landed: Condvar,
}

#[derive(Default)]
struct Held {
    asked: bool,
    answered: u64,
    latest: Option<SinkResult<Vec<SinkInfo>>>,
    listed_last: Option<Vec<SinkInfo>>,
}

impl Surveys {
    pub fn start(
        mut survey: impl FnMut() -> SinkResult<Vec<SinkInfo>> + Send + 'static,
    ) -> SinkResult<Self> {
        let (asks, asked) = bounded::<()>(1);
        let (told_first, first) = bounded(1);
        let answers = Arc::new(Answers::default());
        let answering = Arc::clone(&answers);

        thread::Builder::new()
            .name("resonate-wasapi-survey".into())
            .spawn(move || {
                let mut first_told = false;

                for () in asked {
                    let answer = survey();

                    if let Ok(listed) = &answer
                        && !first_told
                    {
                        first_told = true;

                        let _ = told_first.try_send(listed.clone());
                    }

                    let mut held = answering.held.lock();

                    if let Ok(listed) = &answer {
                        held.listed_last = Some(listed.clone());
                    }
                    held.asked = false;
                    held.answered += 1;
                    held.latest = Some(answer);
                    answering.landed.notify_all();
                }
            })
            .map_err(|source| Error::Backend {
                op: Operation::Connect,
                source,
            })?;
        Ok(Self {
            asks,
            answers,
            first,
        })
    }

    pub fn answered_within(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        match self.within(timeout) {
            Some(answer) => answer,
            None => self.listed_last().ok_or(Error::OpenUnanswered),
        }
    }

    pub fn within(&self, timeout: Duration) -> Option<SinkResult<Vec<SinkInfo>>> {
        let deadline = Instant::now().checked_add(timeout);
        let mut held = self.answers.held.lock();

        let since = held.answered;

        if !held.asked {
            match self.asks.try_send(()) {
                Ok(()) | Err(TrySendError::Full(())) => held.asked = true,
                Err(TrySendError::Disconnected(())) => return Some(Err(Error::LoopStopped)),
            }
        }
        while held.answered == since {
            match deadline {
                Some(deadline) => {
                    if self
                        .answers
                        .landed
                        .wait_until(&mut held, deadline)
                        .timed_out()
                    {
                        return None;
                    }
                }
                None => self.answers.landed.wait(&mut held),
            }
        }
        held.latest.as_mut().map(|latest| match latest {
            Ok(listed) => Ok(listed.clone()),
            Err(error) => Err(mem::replace(error, retold(error))),
        })
    }

    pub fn listed_last(&self) -> Option<Vec<SinkInfo>> {
        self.answers.held.lock().listed_last.clone()
    }

    pub fn baseline(
        &self,
        stopped: &Receiver<()>,
        unasked_for: Duration,
        within: Duration,
    ) -> Option<Vec<SinkInfo>> {
        select! {
            recv(stopped) -> _ => None,
            recv(self.first) -> listed => listed.ok(),
            default(unasked_for) => Some(
                self.within(within)
                    .and_then(Result::ok)
                    .unwrap_or_default(),
            ),
        }
    }
}

fn retold(error: &Error) -> Error {
    match error {
        Error::Backend { op, source } => Error::Backend {
            op: *op,
            source: source.raw_os_error().map_or_else(
                || io::Error::from(source.kind()),
                io::Error::from_raw_os_error,
            ),
        },
        Error::LoopStopped => Error::LoopStopped,
        Error::Disconnected => Error::Disconnected,
        Error::OpenUnanswered => Error::OpenUnanswered,
        Error::NoSink => Error::NoSink,
        Error::SinkGone { node } => Error::SinkGone { node: *node },
        Error::StreamFailed { node, previous } => Error::StreamFailed {
            node: *node,
            previous: *previous,
        },
        Error::StreamRefused {
            node,
            previous,
            op,
            status,
        } => Error::StreamRefused {
            node: *node,
            previous: *previous,
            op: *op,
            status: *status,
        },
        Error::FormatRefused => Error::FormatRefused,
    }
}

pub fn joined_within(thread: JoinHandle<()>, ended: &Receiver<()>, within: Duration) -> bool {
    match ended.recv_timeout(within) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {
            let _ = thread.join();

            true
        }
        Err(RecvTimeoutError::Timeout) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use resonate_audio::{NodeName, SinkId};

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const BOUND: Duration = Duration::from_millis(100);
    const LATE_BY_AT_MOST: Duration = Duration::from_millis(400);
    const NEVER: Duration = Duration::from_secs(3_600);

    fn sink(id: u32) -> SinkInfo {
        SinkInfo {
            id: SinkId::new(id),
            name: NodeName::new(format!("wasapi:{id}")),
            aliases: Vec::new(),
            description: String::new(),
            is_default: false,
            is_hardware: true,
            port: None,
            profile: None,
            formats: Vec::new(),
            allowed_rates: Vec::new(),
            current_rate: None,
        }
    }

    enum Next {
        List(Vec<SinkInfo>),
        Stall(Receiver<()>),
        StallAndRefuse(Receiver<()>),
    }

    fn refused() -> Error {
        Error::Backend {
            op: Operation::Connect,
            source: io::ErrorKind::ConnectionRefused.into(),
        }
    }

    struct Scripted {
        surveys: Surveys,
        script: Sender<Next>,
        counted: Arc<AtomicUsize>,
    }

    fn scripted() -> Scripted {
        let (script, scripted) = crossbeam_channel::unbounded::<Next>();
        let counted = Arc::new(AtomicUsize::new(0));
        let counting = Arc::clone(&counted);

        let surveys = Surveys::start(move || {
            counting.fetch_add(1, Ordering::AcqRel);
            match scripted.recv_timeout(WAIT) {
                Ok(Next::List(listed)) => Ok(listed),
                Ok(Next::Stall(released)) => {
                    let _ = released.recv();
                    Ok(Vec::new())
                }
                Ok(Next::StallAndRefuse(released)) => {
                    let _ = released.recv();
                    Err(refused())
                }
                Err(_) => Err(refused()),
            }
        })
        .expect("a survey worker");

        Scripted {
            surveys,
            script,
            counted,
        }
    }

    impl Scripted {
        fn next(&self, next: Next) {
            self.script.send(next).expect("the worker is listening");
        }

        fn surveyed(&self) -> usize {
            self.counted.load(Ordering::Acquire)
        }
    }

    fn timed<T>(call: impl FnOnce() -> T) -> (T, Duration) {
        let started = Instant::now();
        let answer = call();

        (answer, started.elapsed())
    }

    #[test]
    fn a_survey_that_never_returns_is_answered_within_the_timeout_by_the_last_list() {
        let scripted = scripted();
        let (release, released) = bounded::<()>(1);

        scripted.next(Next::List(vec![sink(1), sink(2)]));

        let listed = scripted
            .surveys
            .answered_within(WAIT)
            .expect("the first list");

        scripted.next(Next::Stall(released));

        let (stalled, took) = timed(|| scripted.surveys.answered_within(BOUND));
        let (again, took_again) = timed(|| scripted.surveys.answered_within(BOUND));

        assert_eq!(listed.len(), 2);
        assert_eq!(stalled.expect("the last list").len(), 2);
        assert!(took < BOUND + LATE_BY_AT_MOST, "answered after {took:?}");
        assert_eq!(again.expect("the last list").len(), 2);
        assert!(took_again < BOUND + LATE_BY_AT_MOST);
        assert_eq!(scripted.surveyed(), 2);

        let _ = release.send(());
    }

    #[test]
    fn a_first_survey_that_never_returns_is_unanswered_within_the_timeout() {
        let scripted = scripted();
        let (release, released) = bounded::<()>(1);

        scripted.next(Next::Stall(released));

        let (stalled, took) = timed(|| scripted.surveys.answered_within(BOUND));

        assert!(matches!(stalled, Err(Error::OpenUnanswered)));
        assert!(took < BOUND + LATE_BY_AT_MOST, "answered after {took:?}");

        let _ = release.send(());
    }

    #[test]
    fn a_survey_in_flight_is_waited_on_rather_than_asked_again() {
        let scripted = scripted();
        let (release, released) = bounded::<()>(1);

        scripted.next(Next::Stall(released));

        let _ = scripted.surveys.within(BOUND);

        scripted.next(Next::List(vec![sink(3)]));

        let releasing = thread::spawn(move || {
            thread::sleep(BOUND);

            let _ = release.send(());
        });
        let landed = scripted.surveys.within(WAIT);
        let fresh = scripted.surveys.within(WAIT);

        releasing.join().expect("released");

        assert!(matches!(landed, Some(Ok(listed)) if listed.is_empty()));
        assert!(matches!(fresh, Some(Ok(listed)) if listed.len() == 1));
        assert_eq!(scripted.surveyed(), 2);
    }

    #[test]
    fn a_refused_survey_is_told_to_everyone_waiting_on_it() {
        let scripted = scripted();
        let (release, released) = bounded::<()>(1);

        scripted.next(Next::StallAndRefuse(released));

        let unanswered = scripted.surveys.within(Duration::ZERO);

        let answers = thread::scope(|scope| {
            let waiting: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| scripted.surveys.within(WAIT)))
                .collect();

            thread::sleep(BOUND);

            let _ = release.send(());

            waiting
                .into_iter()
                .map(|waiter| waiter.join().expect("a waiter"))
                .collect::<Vec<_>>()
        });

        assert!(unanswered.is_none());
        assert!(answers.iter().all(|answer| matches!(
            answer,
            Some(Err(Error::Backend { op: Operation::Connect, source }))
                if source.kind() == io::ErrorKind::ConnectionRefused
        )));
    }

    #[test]
    fn the_watcher_takes_its_baseline_from_the_first_survey_anyone_asked_for() {
        let scripted = scripted();
        let (_stop, stopped) = bounded::<()>(1);

        let surveyed_at_start = scripted.surveyed();

        scripted.next(Next::List(vec![sink(4)]));

        let baseline = thread::scope(|scope| {
            let watcher = scope.spawn(|| scripted.surveys.baseline(&stopped, NEVER, WAIT));
            let engines = scripted.surveys.answered_within(WAIT);

            assert_eq!(engines.expect("the engine's survey").len(), 1);

            watcher.join().expect("the watcher")
        });

        assert_eq!(surveyed_at_start, 0);
        assert_eq!(baseline.map(|listed| listed.len()), Some(1));
        assert_eq!(scripted.surveyed(), 1);
    }

    #[test]
    fn a_watcher_nobody_surveyed_for_takes_its_own() {
        let scripted = scripted();
        let (_stop, stopped) = bounded::<()>(1);

        scripted.next(Next::List(vec![sink(5)]));

        let unasked = scripted.surveys.baseline(&stopped, BOUND, WAIT);

        assert_eq!(unasked.map(|listed| listed.len()), Some(1));
        assert_eq!(scripted.surveyed(), 1);
    }

    #[test]
    fn a_watcher_stopped_before_any_survey_takes_none() {
        let scripted = scripted();
        let (stop, stopped) = bounded::<()>(1);

        stop.send(()).expect("the stop is heard");

        let baseline = scripted.surveys.baseline(&stopped, NEVER, WAIT);

        assert!(baseline.is_none());
        assert_eq!(scripted.surveyed(), 0);
    }

    #[test]
    fn a_thread_that_never_ends_is_let_go_within_the_bound() {
        let (_ending, ended) = bounded::<()>(1);
        let (release, released) = bounded::<()>(1);
        let stuck = thread::spawn(move || {
            let _ = released.recv();
        });
        let (finishing, finished) = bounded::<()>(1);
        let done = thread::spawn(move || drop(finishing));

        let (joined, took) = timed(|| joined_within(stuck, &ended, BOUND));
        let ended_joined = joined_within(done, &finished, WAIT);

        assert!(!joined);
        assert!(took < BOUND + LATE_BY_AT_MOST);
        assert!(ended_joined);

        let _ = release.send(());
    }
}
