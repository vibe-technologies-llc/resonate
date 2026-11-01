#[cfg(any(windows, test))]
use std::{io, thread};
use std::{path::Path, sync::Arc};

use resonate_library::{Removal, Trashes};
#[cfg(unix)]
use resonate_mpris::trash_through_the_portal;
#[cfg(any(windows, test))]
use thiserror::Error;

#[cfg(any(windows, test))]
const RECYCLING_THREAD: &str = "resonate-recycle";

#[cfg(unix)]
const FLATPAK_INFO: &str = "/.flatpak-info";

#[cfg(unix)]
struct Portal;

#[cfg(unix)]
impl Trashes for Portal {
    fn trash(&self, file: &Path) -> Removal {
        match trash_through_the_portal(file) {
            Ok(()) => Removal::Removed,
            Err(trashing) if trashing.is_for_a_file_that_is_gone() => Removal::AlreadyGone,
            Err(trashing) => {
                tracing::warn!(path = %file.display(), error = ?trashing, "a file asked to be deleted could not be moved to the desktop's trash");
                Removal::Kept
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn the_desktops() -> Option<Arc<dyn Trashes>> {
    through_the_portal_where_sandboxed_by(Path::new(FLATPAK_INFO))
}

#[cfg(windows)]
struct RecycleBin;

#[cfg(windows)]
impl Trashes for RecycleBin {
    fn trash(&self, file: &Path) -> Removal {
        match recycled(file) {
            Ok(()) => Removal::Removed,
            Err(_) if !file.exists() => Removal::AlreadyGone,
            Err(error) => {
                tracing::warn!(path = %file.display(), %error, "a file asked to be deleted could not be moved to the recycle bin");
                Removal::Kept
            }
        }
    }
}

#[cfg(windows)]
pub(crate) fn the_desktops() -> Option<Arc<dyn Trashes>> {
    Some(Arc::new(RecycleBin))
}

#[cfg(windows)]
#[derive(Debug, Error)]
enum Unrecycled {
    #[error(transparent)]
    Thread(#[from] Unthreaded),
    #[error(transparent)]
    Bin(#[from] trash::Error),
}

#[cfg(windows)]
fn recycled(file: &Path) -> Result<(), Unrecycled> {
    on_a_thread_of_its_own(|| trash::delete(file))??;
    Ok(())
}

#[cfg(any(windows, test))]
#[derive(Debug, Error)]
enum Unthreaded {
    #[error("the thread could not be started: {0}")]
    Spawn(#[source] io::Error),
    #[error("the thread panicked")]
    Panicked,
}

#[cfg(any(windows, test))]
fn on_a_thread_of_its_own<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T, Unthreaded> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name(RECYCLING_THREAD.to_owned())
            .spawn_scoped(scope, work)
            .map_err(Unthreaded::Spawn)?
            .join()
            .map_err(|_| Unthreaded::Panicked)
    })
}

#[cfg(unix)]
fn through_the_portal_where_sandboxed_by(info: &Path) -> Option<Arc<dyn Trashes>> {
    info.exists().then(|| Arc::new(Portal) as Arc<dyn Trashes>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recycle_bin_is_reached_on_a_thread_that_no_other_work_has_entered_an_apartment_on() {
        let caller = thread::current().id();

        let worker = on_a_thread_of_its_own(|| thread::current().id()).expect("a thread");
        let named = on_a_thread_of_its_own(|| thread::current().name().map(str::to_owned))
            .expect("a thread");

        assert_ne!(worker, caller);
        assert_eq!(named.as_deref(), Some(RECYCLING_THREAD));
    }

    #[test]
    fn a_thread_that_panics_is_a_refusal_not_a_panic_in_the_caller() {
        let refused =
            on_a_thread_of_its_own(|| -> () { panic!("an apartment that would not open") });

        assert!(matches!(refused, Err(Unthreaded::Panicked)));
    }

    #[test]
    fn a_thread_hands_back_what_its_work_made() {
        let made = on_a_thread_of_its_own(|| 7 * 6).expect("a thread");

        assert_eq!(made, 42);
    }
}

#[cfg(all(test, unix))]
mod portal_tests {
    use std::fs;

    use super::*;

    #[test]
    fn the_portal_trashes_only_where_a_flatpak_sandbox_says_it_holds_the_process() {
        let folder = std::env::temp_dir().join(format!("resonate-trashing-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("a folder");
        let info = folder.join("flatpak-info");

        assert!(through_the_portal_where_sandboxed_by(&info).is_none());

        fs::write(&info, "[Application]\n").expect("a sandbox's info");

        assert!(through_the_portal_where_sandboxed_by(&info).is_some());
        fs::remove_dir_all(&folder).expect("the folder goes");
    }
}
