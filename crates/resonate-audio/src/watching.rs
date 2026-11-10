use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, never, select};

use crate::{SinkChange, SinkInfo, changes::changes_between};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
    pub surveyed_at_least_every: Duration,
    pub settled_after_a_nudge: Duration,
}

impl Pace {
    pub const ENDPOINTS: Self = Self {
        surveyed_at_least_every: Duration::from_secs(2),
        settled_after_a_nudge: Duration::from_millis(100),
    };
}

enum Woken {
    Stopped,
    Nudged,
    Due,
}

pub fn watch(
    mut before: Vec<SinkInfo>,
    mut survey: impl FnMut() -> Option<Vec<SinkInfo>>,
    told: &Sender<SinkChange>,
    nudged: &Receiver<()>,
    stopped: &Receiver<()>,
    pace: Pace,
) {
    let mut nudges = nudged.clone();
    loop {
        let woken = select! {
            recv(stopped) -> _ => Woken::Stopped,
            recv(nudges) -> nudge => match nudge {
                Ok(()) => Woken::Nudged,
                Err(_) => {
                    nudges = never();
                    continue;
                }
            },
            default(pace.surveyed_at_least_every) => Woken::Due,
        };
        match woken {
            Woken::Stopped => return,
            Woken::Nudged => match stopped.recv_timeout(pace.settled_after_a_nudge) {
                Err(RecvTimeoutError::Timeout) => while nudges.try_recv().is_ok() {},
                Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            },
            Woken::Due => {}
        }
        let Some(now) = survey() else {
            continue;
        };
        for change in changes_between(&before, &now) {
            let _ = told.send(change);
        }
        before = now;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread::{self, JoinHandle},
    };

    use crossbeam_channel::{bounded, unbounded};

    use super::*;
    use crate::{NodeName, SinkId};

    const ANSWERED_WITHIN: Duration = Duration::from_secs(5);
    const NEVER_DUE: Pace = Pace {
        surveyed_at_least_every: Duration::from_secs(3_600),
        settled_after_a_nudge: Duration::from_millis(20),
    };

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

    struct Watched {
        devices: Sender<Vec<SinkInfo>>,
        nudge: Sender<()>,
        stop: Sender<()>,
        changes: Receiver<SinkChange>,
        surveys: Arc<AtomicUsize>,
        watcher: JoinHandle<()>,
    }

    fn watched(pace: Pace) -> Watched {
        let (devices, plugged) = unbounded::<Vec<SinkInfo>>();
        let (nudge, nudged) = bounded(1);
        let (stop, stopped) = bounded(1);
        let (told, changes) = unbounded();
        let surveys = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&surveys);
        let watcher = thread::spawn(move || {
            let mut present = vec![sink(1)];
            let survey = move || {
                while let Ok(now) = plugged.try_recv() {
                    present = now;
                }
                counted.fetch_add(1, Ordering::AcqRel);
                Some(present.clone())
            };
            watch(vec![sink(1)], survey, &told, &nudged, &stopped, pace);
        });
        Watched {
            devices,
            nudge,
            stop,
            changes,
            surveys,
            watcher,
        }
    }

    impl Watched {
        fn stopped(self) {
            let _ = self.stop.send(());
            assert!(self.watcher.join().is_ok());
        }
    }

    #[test]
    fn a_device_the_system_tells_of_is_surveyed_without_waiting_for_the_poll() {
        let watched = watched(NEVER_DUE);

        watched
            .devices
            .send(vec![sink(1), sink(2)])
            .expect("the watcher is listening");
        watched.nudge.send(()).expect("the watcher is listening");

        assert_eq!(
            watched.changes.recv_timeout(ANSWERED_WITHIN),
            Ok(SinkChange::Added(SinkId::new(2)))
        );
        watched.stopped();
    }

    #[test]
    fn a_device_nobody_told_of_is_found_by_the_poll() {
        let watched = watched(Pace {
            surveyed_at_least_every: Duration::from_millis(10),
            ..NEVER_DUE
        });

        watched
            .devices
            .send(Vec::new())
            .expect("the watcher is listening");

        assert_eq!(
            watched.changes.recv_timeout(ANSWERED_WITHIN),
            Ok(SinkChange::Removed(SinkId::new(1)))
        );
        watched.stopped();
    }

    #[test]
    fn a_burst_of_notifications_is_one_survey() {
        let watched = watched(Pace {
            settled_after_a_nudge: Duration::from_millis(200),
            ..NEVER_DUE
        });

        watched.nudge.send(()).expect("the watcher is listening");
        thread::sleep(Duration::from_millis(20));
        for _ in 0..3 {
            let _ = watched.nudge.try_send(());
        }
        thread::sleep(Duration::from_millis(400));

        assert_eq!(watched.surveys.load(Ordering::Acquire), 1);
        watched.stopped();
    }

    #[test]
    fn the_poll_goes_on_once_the_system_stops_telling() {
        let watched = watched(Pace {
            surveyed_at_least_every: Duration::from_millis(10),
            ..NEVER_DUE
        });
        let Watched {
            devices,
            nudge,
            stop,
            changes,
            surveys,
            watcher,
        } = watched;

        drop(nudge);
        devices
            .send(vec![sink(1), sink(3)])
            .expect("the watcher is listening");

        assert_eq!(
            changes.recv_timeout(ANSWERED_WITHIN),
            Ok(SinkChange::Added(SinkId::new(3)))
        );
        assert!(surveys.load(Ordering::Acquire) >= 1);
        let _ = stop.send(());
        assert!(watcher.join().is_ok());
    }
}
