use std::{fs::Metadata, io, path::Path};
#[cfg(unix)]
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::{DirBuilderExt as _, MetadataExt as _},
};

#[cfg(unix)]
const OURS_ALONE: u32 = 0o700;
#[cfg(unix)]
const OPEN_TO_OTHERS: u32 = 0o077;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    #[cfg(unix)]
    uid: u32,
}

impl Owner {
    #[cfg(unix)]
    pub fn this_user() -> Self {
        Self::user(rustix::process::geteuid().as_raw())
    }

    #[cfg(not(unix))]
    pub const fn this_user() -> Self {
        Self {}
    }

    #[cfg(unix)]
    pub const fn user(uid: u32) -> Self {
        Self { uid }
    }

    #[cfg(unix)]
    pub const fn id(self) -> u32 {
        self.uid
    }

    #[cfg(unix)]
    pub fn owns(self, metadata: &Metadata) -> bool {
        metadata.uid() == self.uid
    }

    #[cfg(not(unix))]
    pub const fn owns(self, _: &Metadata) -> bool {
        true
    }

    #[cfg(unix)]
    pub fn keeps_alone(self, folder: &Path) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(OURS_ALONE)
            .create(folder)?;
        match self.is_kept_alone(folder) {
            true => Ok(()),
            false => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        }
    }

    #[cfg(not(unix))]
    pub fn keeps_alone(self, folder: &Path) -> io::Result<()> {
        std::fs::create_dir_all(folder)
    }

    #[cfg(unix)]
    pub fn is_kept_alone(self, folder: &Path) -> bool {
        let Ok(standing) = fs::symlink_metadata(folder) else {
            return false;
        };
        standing.is_dir() && self.owns(&standing) && standing.mode() & OPEN_TO_OTHERS == 0
    }

    #[cfg(not(unix))]
    pub fn is_kept_alone(self, folder: &Path) -> bool {
        folder.is_dir()
    }

    #[cfg(unix)]
    pub fn owns_the_file_at(self, path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|standing| standing.is_file() && self.owns(&standing))
    }

    #[cfg(not(unix))]
    pub fn owns_the_file_at(self, path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|standing| standing.is_file() && self.owns(&standing))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        env,
        os::unix::fs::{PermissionsExt as _, symlink},
        path::PathBuf,
        process,
    };

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-owner-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a scratch folder");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).expect("a folder").mode() & 0o777
    }

    #[test]
    fn a_folder_is_made_closed_to_everyone_else() {
        let scratch = Scratch::new("made");
        let folder = scratch.0.join("mine");

        Owner::this_user().keeps_alone(&folder).expect("a folder");

        assert_eq!(mode_of(&folder), OURS_ALONE);
        assert!(Owner::this_user().is_kept_alone(&folder));
    }

    #[test]
    fn a_folder_open_to_others_or_made_by_another_user_is_refused() {
        let scratch = Scratch::new("refused");
        let open = scratch.0.join("open");
        fs::create_dir(&open).expect("a folder");
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).expect("opened");

        let ours = Owner::this_user();
        let theirs = Owner::user(ours.id().wrapping_add(1));

        assert_eq!(
            ours.keeps_alone(&open).expect_err("open").kind(),
            io::ErrorKind::PermissionDenied
        );

        fs::set_permissions(&open, fs::Permissions::from_mode(0o700)).expect("closed");

        assert!(ours.is_kept_alone(&open));
        assert!(!theirs.is_kept_alone(&open));
        assert_eq!(
            theirs.keeps_alone(&open).expect_err("not theirs").kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn a_link_to_a_folder_is_not_the_folder() {
        let scratch = Scratch::new("linked");
        let real = scratch.0.join("real");
        let link = scratch.0.join("link");
        Owner::this_user().keeps_alone(&real).expect("a folder");
        symlink(&real, &link).expect("a link");

        assert!(!Owner::this_user().is_kept_alone(&link));
        assert!(Owner::this_user().keeps_alone(&link).is_err());
    }

    #[test]
    fn a_file_is_owned_only_by_its_owner_and_a_link_is_no_file() {
        let scratch = Scratch::new("files");
        let file = scratch.0.join("file");
        let link = scratch.0.join("link");
        fs::write(&file, b"x").expect("a file");
        symlink(&file, &link).expect("a link");

        let ours = Owner::this_user();
        let theirs = Owner::user(ours.id().wrapping_add(1));

        assert!(ours.owns_the_file_at(&file));
        assert!(!theirs.owns_the_file_at(&file));
        assert!(!ours.owns_the_file_at(&link));
    }
}
