use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

static DENIED: Mutex<Vec<Denial>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, PartialEq, Eq)]
enum Read {
    Listing,
    Stat,
}

struct Denial {
    read: Read,
    path: PathBuf,
    refused: Arc<AtomicUsize>,
}

#[must_use = "the read is allowed again when this is dropped"]
pub struct Denied {
    read: Read,
    path: PathBuf,
    refused: Arc<AtomicUsize>,
}

impl Denied {
    pub fn refusals(&self) -> usize {
        self.refused.load(Ordering::Relaxed)
    }
}

pub fn deny_listing(path: &Path) -> Denied {
    deny(Read::Listing, path)
}

pub fn deny_stat(path: &Path) -> Denied {
    deny(Read::Stat, path)
}

fn deny(read: Read, path: &Path) -> Denied {
    let refused = Arc::new(AtomicUsize::new(0));

    DENIED.lock().push(Denial {
        read,
        path: path.to_path_buf(),
        refused: Arc::clone(&refused),
    });

    Denied {
        read,
        path: path.to_path_buf(),
        refused,
    }
}

impl Drop for Denied {
    fn drop(&mut self) {
        let mut denied = DENIED.lock();

        if let Some(at) = denied
            .iter()
            .position(|denial| denial.read == self.read && denial.path == self.path)
        {
            denied.swap_remove(at);
        }
    }
}

fn before(read: Read, path: &Path) -> io::Result<()> {
    match DENIED
        .lock()
        .iter()
        .find(|denial| denial.read == read && denial.path == path)
    {
        Some(denial) => {
            denial.refused.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
        None => Ok(()),
    }
}

pub(crate) fn before_listing(path: &Path) -> io::Result<()> {
    before(Read::Listing, path)
}

pub(crate) fn before_stat(path: &Path) -> io::Result<()> {
    before(Read::Stat, path)
}
