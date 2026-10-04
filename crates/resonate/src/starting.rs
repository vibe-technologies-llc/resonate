use std::{
    env,
    fs::{File, TryLockError},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const LOCK_NAME: &str = "resonate-starting.lock";
const WAITS_AT_MOST: Duration = Duration::from_secs(15);
const TRIED_EVERY: Duration = Duration::from_millis(50);

pub(crate) struct Starting {
    _held: File,
}

pub(crate) fn one_window_at_a_time() -> Option<Starting> {
    taken(&lock_path(), WAITS_AT_MOST)
}

fn lock_path() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(env::temp_dir, PathBuf::from)
        .join(LOCK_NAME)
}

fn taken(path: &Path, waits_at_most: Duration) -> Option<Starting> {
    let file = match File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => {
            tracing::debug!(%error, path = %path.display(), "no lock to start under; a second launch may open a second window");
            return None;
        }
    };

    let deadline = Instant::now() + waits_at_most;
    loop {
        match file.try_lock() {
            Ok(()) => return Some(Starting { _held: file }),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(TRIED_EVERY);
            }
            Err(TryLockError::WouldBlock) => {
                tracing::warn!("another launch is slow to start; starting beside it");
                return None;
            }
            Err(TryLockError::Error(error)) => {
                tracing::debug!(%error, "the start lock could not be taken; a second launch may open a second window");
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::*;

    #[test]
    fn a_second_launch_waits_for_the_first_to_be_reachable() {
        let path = env::temp_dir().join(format!("resonate-starting-{}.lock", process::id()));

        let first = taken(&path, Duration::ZERO);
        let second = taken(&path, Duration::ZERO);

        assert!(first.is_some(), "the first launch took no lock");
        assert!(second.is_none(), "a second launch started beside the first");

        drop(first);
        let after = taken(&path, Duration::ZERO);

        assert!(
            after.is_some(),
            "the lock was not let go of once the first was reachable"
        );
        let _ = fs::remove_file(&path);
    }
}
