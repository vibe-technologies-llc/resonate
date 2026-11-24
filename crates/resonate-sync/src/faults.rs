use std::{io, sync::Arc};

use parking_lot::Mutex;

use crate::{Change, DeviceId};

static WATCHED: Mutex<Vec<Watch>> = Mutex::new(Vec::new());

struct Watch {
    device: DeviceId,
    seen: Arc<Mutex<Seen>>,
}

#[derive(Default)]
struct Seen {
    failing: usize,
    failed: Vec<Told>,
    told: Vec<Told>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub connection: usize,
    pub whole: bool,
    pub changes: Vec<Change>,
}

#[must_use = "tells to the device are no longer watched once this is dropped"]
pub struct Tells {
    device: DeviceId,
    seen: Arc<Mutex<Seen>>,
}

impl Tells {
    pub fn fail_next(&self, times: usize) {
        self.seen.lock().failing += times;
    }

    pub fn failed(&self) -> Vec<Told> {
        self.seen.lock().failed.clone()
    }

    pub fn told(&self) -> Vec<Told> {
        self.seen.lock().told.clone()
    }
}

impl Drop for Tells {
    fn drop(&mut self) {
        WATCHED
            .lock()
            .retain(|watch| watch.device != self.device || !Arc::ptr_eq(&watch.seen, &self.seen));
    }
}

pub fn watch_tells_to(device: DeviceId) -> Tells {
    let seen = Arc::new(Mutex::new(Seen::default()));

    WATCHED.lock().push(Watch {
        device,
        seen: Arc::clone(&seen),
    });

    Tells { device, seen }
}

pub(crate) fn before_telling(
    device: DeviceId,
    connection: usize,
    whole: bool,
    changes: &[Change],
) -> io::Result<()> {
    let watched = WATCHED.lock();
    let Some(watch) = watched.iter().find(|watch| watch.device == device) else {
        return Ok(());
    };
    let mut seen = watch.seen.lock();
    let told = Told {
        connection,
        whole,
        changes: changes.to_vec(),
    };

    if seen.failing > 0 {
        seen.failing -= 1;
        seen.failed.push(told);
        return Err(io::Error::from(io::ErrorKind::BrokenPipe));
    }
    seen.told.push(told);
    Ok(())
}
