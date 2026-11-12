use std::{
    panic::{self, AssertUnwindSafe},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use objc2_core_foundation::{CFRunLoop, CFRunLoopRunResult, kCFRunLoopDefaultMode};

const LOOKED_AT_EVERY_SECONDS: f64 = 0.25;
const RESTS_WITH_NOTHING_TO_RUN: Duration = Duration::from_millis(20);

pub fn while_the_main_loop_runs<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        let working = scope.spawn(|| {
            let worked = panic::catch_unwind(AssertUnwindSafe(work));
            done.store(true, Ordering::Release);
            worked
        });
        let mode = unsafe { kCFRunLoopDefaultMode };
        while !done.load(Ordering::Acquire) {
            if CFRunLoop::run_in_mode(mode, LOOKED_AT_EVERY_SECONDS, true)
                == CFRunLoopRunResult::Finished
            {
                thread::sleep(RESTS_WITH_NOTHING_TO_RUN);
            }
        }
        match working.join() {
            Ok(Ok(worked)) => worked,
            Ok(Err(panicked)) | Err(panicked) => panic::resume_unwind(panicked),
        }
    })
}
