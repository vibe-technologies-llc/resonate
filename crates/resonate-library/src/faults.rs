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

static SPOILED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[must_use = "the copies are left alone again when this is dropped"]
pub struct Spoiled {
    path: PathBuf,
}

pub fn spoil_the_copies_of(path: &Path) -> Spoiled {
    SPOILED.lock().push(path.to_path_buf());
    Spoiled {
        path: path.to_path_buf(),
    }
}

impl Drop for Spoiled {
    fn drop(&mut self) {
        let mut spoiled = SPOILED.lock();

        if let Some(at) = spoiled.iter().position(|path| *path == self.path) {
            spoiled.swap_remove(at);
        }
    }
}

const SPOILED_FROM_THE_END: u64 = 64;

pub(crate) fn spoil_if_asked(source: &Path, copy: &Path) {
    use std::io::{Read as _, Seek as _, SeekFrom, Write as _};

    if !SPOILED.lock().iter().any(|path| path == source) {
        return;
    }

    let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(copy)
    else {
        return;
    };
    let Ok(length) = file.seek(SeekFrom::End(0)) else {
        return;
    };
    let at = length.saturating_sub(SPOILED_FROM_THE_END);
    let mut byte = [0_u8; 1];

    if file.seek(SeekFrom::Start(at)).is_ok() && file.read_exact(&mut byte).is_ok() {
        byte[0] ^= 0x5a;

        let _ = file
            .seek(SeekFrom::Start(at))
            .and_then(|_| file.write_all(&byte));
    }
}
