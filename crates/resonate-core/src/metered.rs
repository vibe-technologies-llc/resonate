use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Meter {
    #[default]
    Unknown,
    Unmetered,
    Metered,
}

impl Meter {
    pub const fn is_metered(self) -> bool {
        matches!(self, Self::Metered)
    }

    pub const fn reported(metered: bool) -> Self {
        if metered {
            Self::Metered
        } else {
            Self::Unmetered
        }
    }
}

#[derive(Default)]
struct Standing {
    meter: Mutex<Meter>,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub struct Metering {
    standing: Arc<Standing>,
}

impl Metering {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn report(&self, meter: Meter) {
        let mut standing = self.standing.meter.lock();
        if *standing == meter {
            return;
        }
        *standing = meter;
        self.standing.changed.notify_all();
    }

    pub fn meter(&self) -> Meter {
        *self.standing.meter.lock()
    }

    pub fn is_metered(&self) -> bool {
        self.meter().is_metered()
    }
}

#[derive(Clone)]
pub struct Gate {
    metering: Metering,
    shut_while_metered: Arc<AtomicBool>,
}

impl Default for Gate {
    fn default() -> Self {
        Self::open()
    }
}

impl Gate {
    pub fn new(metering: Metering, shut_while_metered: bool) -> Self {
        Self {
            metering,
            shut_while_metered: Arc::new(AtomicBool::new(shut_while_metered)),
        }
    }

    pub fn open() -> Self {
        Self::new(Metering::new(), false)
    }

    pub fn shut_while_metered(&self, shut: bool) {
        self.shut_while_metered.store(shut, Ordering::Relaxed);
        let standing = &self.metering.standing;
        let _held = standing.meter.lock();
        standing.changed.notify_all();
    }

    pub fn is_shut_while_metered(&self) -> bool {
        self.shut_while_metered.load(Ordering::Relaxed)
    }

    pub fn is_shut(&self) -> bool {
        self.is_shut_while_metered() && self.metering.is_metered()
    }

    pub fn wait_while_shut(&self, at_most: Duration) -> bool {
        let deadline = Instant::now() + at_most;
        let standing = &self.metering.standing;
        let mut meter = standing.meter.lock();
        while self.is_shut_at(*meter) {
            if standing
                .changed
                .wait_until(&mut meter, deadline)
                .timed_out()
            {
                return self.is_shut_at(*meter);
            }
        }
        false
    }

    fn is_shut_at(&self, meter: Meter) -> bool {
        self.is_shut_while_metered() && meter.is_metered()
    }

    pub const fn metering(&self) -> &Metering {
        &self.metering
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Instant};

    use super::*;

    const LONG_ENOUGH: Duration = Duration::from_secs(10);

    #[test]
    fn a_connection_nobody_reported_on_is_unknown_and_not_metered() {
        let metering = Metering::new();

        assert_eq!(metering.meter(), Meter::Unknown);
        assert!(!metering.is_metered());
    }

    #[test]
    fn only_a_report_of_metered_is_metered() {
        let metering = Metering::new();

        metering.report(Meter::Metered);
        assert!(metering.is_metered());

        metering.report(Meter::Unknown);
        assert!(!metering.is_metered());

        metering.report(Meter::reported(true));
        metering.report(Meter::reported(false));
        assert_eq!(metering.meter(), Meter::Unmetered);
    }

    #[test]
    fn a_gate_is_shut_only_while_its_setting_is_on_and_the_connection_is_metered() {
        let metering = Metering::new();
        let gate = Gate::new(metering.clone(), false);

        metering.report(Meter::Metered);
        assert!(!gate.is_shut());

        gate.shut_while_metered(true);
        assert!(gate.is_shut());

        metering.report(Meter::Unmetered);
        assert!(!gate.is_shut());

        metering.report(Meter::Unknown);
        assert!(!gate.is_shut());
    }

    #[test]
    fn gates_made_from_one_metering_follow_it_and_their_clones_share_their_setting() {
        let metering = Metering::new();
        let downloads = Gate::new(metering.clone(), true);
        let lookups = Gate::new(metering.clone(), false);
        let clone = downloads.clone();

        metering.report(Meter::Metered);
        assert!(downloads.is_shut());
        assert!(!lookups.is_shut());

        clone.shut_while_metered(false);
        assert!(!downloads.is_shut());
    }

    #[test]
    fn a_waiter_is_woken_by_a_report_that_changes_the_connection() {
        let metering = Metering::new();
        metering.report(Meter::Metered);
        let gate = Gate::new(metering.clone(), true);

        let reporter = {
            let metering = metering.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                metering.report(Meter::Unmetered);
            })
        };
        let began = Instant::now();
        let still_shut = gate.wait_while_shut(LONG_ENOUGH);
        reporter.join().unwrap();

        assert!(!still_shut);
        assert!(began.elapsed() < LONG_ENOUGH);
    }

    #[test]
    fn a_waiter_is_woken_by_the_setting_being_turned_off() {
        let metering = Metering::new();
        metering.report(Meter::Metered);
        let gate = Gate::new(metering, true);

        let turner = {
            let gate = gate.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                gate.shut_while_metered(false);
            })
        };
        let began = Instant::now();
        let still_shut = gate.wait_while_shut(LONG_ENOUGH);
        turner.join().unwrap();

        assert!(!still_shut);
        assert!(began.elapsed() < LONG_ENOUGH);
    }

    #[test]
    fn a_gate_that_stays_shut_says_so_when_the_wait_runs_out() {
        let metering = Metering::new();
        metering.report(Meter::Metered);
        let gate = Gate::new(metering, true);

        assert!(gate.wait_while_shut(Duration::from_millis(20)));
    }

    #[test]
    fn an_open_gate_is_not_waited_on() {
        let gate = Gate::open();

        let began = Instant::now();

        assert!(!gate.wait_while_shut(LONG_ENOUGH));
        assert!(began.elapsed() < LONG_ENOUGH);
    }
}
