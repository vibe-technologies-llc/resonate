use std::{
    env,
    fs::{File, TryLockError},
    io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use resonate_core::owner::Owner;
#[cfg(unix)]
use rustix::fs::OFlags;

#[cfg(unix)]
const PRIVATE_FOLDER_STEM: &str = "resonate-";
#[cfg(unix)]
const OURS_ALONE: u32 = 0o600;
const LOCK_NAME: &str = "resonate-starting.lock";
const WAITS_AT_MOST: Duration = Duration::from_secs(15);
const TRIED_EVERY: Duration = Duration::from_millis(50);

pub(crate) struct Starting {
    _held: File,
}

pub(crate) fn one_window_at_a_time() -> Option<Starting> {
    taken(&lock_path()?, WAITS_AT_MOST)
}

fn lock_path() -> Option<PathBuf> {
    match env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(runtime) => Some(PathBuf::from(runtime).join(LOCK_NAME)),
        None => unshared_lock_path(),
    }
}

#[cfg(unix)]
fn unshared_lock_path() -> Option<PathBuf> {
    private_lock_in(&env::temp_dir(), Owner::this_user())
}

#[cfg(unix)]
fn private_lock_in(shared: &Path, owner: Owner) -> Option<PathBuf> {
    let folder = shared.join(format!("{PRIVATE_FOLDER_STEM}{}", owner.id()));
    match owner.keeps_alone(&folder) {
        Ok(()) => Some(folder.join(LOCK_NAME)),
        Err(error) => {
            tracing::debug!(%error, folder = %folder.display(), "no private folder to start under; a second launch may open a second window");
            None
        }
    }
}

#[cfg(windows)]
fn unshared_lock_path() -> Option<PathBuf> {
    Some(env::temp_dir().join(LOCK_NAME))
}

#[cfg(unix)]
fn opened(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(OURS_ALONE)
        .custom_flags(OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)
}

#[cfg(windows)]
fn opened(path: &Path) -> io::Result<File> {
    File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
}

fn taken(path: &Path, waits_at_most: Duration) -> Option<Starting> {
    let file = match opened(path) {
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

    #[cfg(unix)]
    mod private {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};

        use super::*;

        fn scratch(name: &str) -> PathBuf {
            let path = env::temp_dir().join(format!("resonate-starting-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);

            fs::create_dir_all(&path).expect("a scratch folder");
            path
        }

        #[test]
        fn the_lock_stands_in_a_folder_made_closed_to_everyone_else() {
            let shared = scratch("made");
            let ours = Owner::this_user();

            let lock = private_lock_in(&shared, ours).expect("a private lock path");

            let folder = lock.parent().expect("its folder");

            assert_eq!(fs::metadata(folder).expect("made").mode() & 0o777, 0o700);
            assert!(taken(&lock, Duration::ZERO).is_some());
            assert_eq!(fs::metadata(&lock).expect("taken").mode() & 0o777, 0o600);

            let _ = fs::remove_dir_all(&shared);
        }

        #[test]
        fn a_folder_another_user_made_or_left_open_is_no_place_for_the_lock() {
            let shared = scratch("refused");
            let ours = Owner::this_user();
            let theirs = Owner::user(ours.id().wrapping_add(1));

            let folder = shared.join(format!("resonate-{}", ours.id()));

            fs::create_dir(&folder).expect("a planted folder");
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).expect("opened");

            assert!(private_lock_in(&shared, ours).is_none());

            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).expect("closed");

            assert!(private_lock_in(&shared, ours).is_some());
            assert!(private_lock_in(&shared, theirs).is_none());

            let _ = fs::remove_dir_all(&shared);
        }

        #[test]
        fn a_lock_planted_as_a_link_is_not_followed() {
            let shared = scratch("linked");
            let target = shared.join("victim");
            let planted = shared.join("lock");

            fs::write(&target, b"untouched").expect("a file");
            symlink(&target, &planted).expect("a link");

            assert!(taken(&planted, Duration::ZERO).is_none());
            assert_eq!(fs::read(&target).expect("the file"), b"untouched");

            let _ = fs::remove_dir_all(&shared);
        }
    }
}
