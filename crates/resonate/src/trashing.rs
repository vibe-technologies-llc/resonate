use std::{path::Path, sync::Arc};

use resonate_library::{Removal, Trashes};
#[cfg(unix)]
use resonate_mpris::trash_through_the_portal;

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
        match trash::delete(file) {
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

#[cfg(unix)]
fn through_the_portal_where_sandboxed_by(info: &Path) -> Option<Arc<dyn Trashes>> {
    info.exists().then(|| Arc::new(Portal) as Arc<dyn Trashes>)
}

#[cfg(all(test, unix))]
mod tests {
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
