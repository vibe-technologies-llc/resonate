use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn report(active: bool) {
    ACTIVE.store(active, Ordering::Release);
}

pub fn wait(stopped: &(dyn Fn() -> bool + Sync)) {
    while ACTIVE.load(Ordering::Acquire) && !stopped() {
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn gaming_holds_background_work_until_resumed_and_cancellation_releases_it() {
        report(true);
        let (sent, received) = mpsc::channel();
        let worker = thread::spawn(move || {
            wait(&|| false);
            sent.send(()).expect("the receiver stays open");
        });
        assert!(received.recv_timeout(Duration::from_millis(150)).is_err());
        report(false);
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("work resumes");
        worker.join().expect("the worker returns");
        report(true);
        wait(&|| true);
        report(false);
    }
}
