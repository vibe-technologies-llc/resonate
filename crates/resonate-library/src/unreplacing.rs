use std::{fs, io, path::Path};

#[cfg(unix)]
use rustix::{
    fs::{CWD, RenameFlags, renameat_with},
    io::Errno,
};

#[cfg(windows)]
pub(crate) fn renamed_over_nothing(from: &Path, to: &Path) -> io::Result<()> {
    linked_over_nothing(from, to)
}

#[cfg(unix)]
pub(crate) fn renamed_over_nothing(from: &Path, to: &Path) -> io::Result<()> {
    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => linked_over_nothing(from, to),
        Err(errno) => Err(errno.into()),
    }
}

fn linked_over_nothing(from: &Path, to: &Path) -> io::Result<()> {
    match fs::hard_link(from, to) {
        Ok(()) => fs::remove_file(from).inspect_err(|_| link_taken_back(to)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::CrossesDevices
            ) =>
        {
            Err(error)
        }
        Err(_) if fs::symlink_metadata(to).is_ok() => Err(io::ErrorKind::AlreadyExists.into()),
        Err(_) => fs::rename(from, to),
    }
}

fn link_taken_back(to: &Path) {
    if let Err(error) = fs::remove_file(to) {
        tracing::warn!(path = %to.display(), %error, "a link made for a move whose source stayed could not be taken back");
    }
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, process};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                env::temp_dir().join(format!("resonate-unreplacing-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a writable temporary directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_rename_onto_a_file_that_appeared_is_refused_and_both_are_left() {
        let scratch = Scratch::new("refused");
        let from = scratch.0.join("from.flac");
        let to = scratch.0.join("to.flac");
        fs::write(&from, b"moving").expect("written");
        fs::write(&to, b"standing").expect("written");

        let refused = renamed_over_nothing(&from, &to);

        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(io::ErrorKind::AlreadyExists)
        );
        assert_eq!(fs::read(&from).expect("still there"), b"moving");
        assert_eq!(fs::read(&to).expect("still there"), b"standing");
    }

    #[test]
    fn a_rename_onto_nothing_lands() {
        let scratch = Scratch::new("landed");
        let from = scratch.0.join("from.flac");
        let to = scratch.0.join("to.flac");
        fs::write(&from, b"moving").expect("written");

        renamed_over_nothing(&from, &to).expect("the rename lands");

        assert!(!from.exists());
        assert_eq!(fs::read(&to).expect("landed"), b"moving");
    }

    #[test]
    #[cfg(unix)]
    fn a_link_whose_source_cannot_be_unlinked_is_taken_back_and_the_source_left() {
        use std::os::unix::fs::PermissionsExt as _;

        const READ_AND_ENTER_ONLY: u32 = 0o555;
        const OWNED_AND_WRITABLE: u32 = 0o755;

        let scratch = Scratch::new("unlink-refused");
        let standing = scratch.0.join("standing");
        let landing = scratch.0.join("landing");
        fs::create_dir_all(&standing).expect("a folder to move from");
        fs::create_dir_all(&landing).expect("a folder to move to");
        let from = standing.join("from.flac");
        let to = landing.join("to.flac");
        fs::write(&from, b"moving").expect("written");
        fs::set_permissions(&standing, fs::Permissions::from_mode(READ_AND_ENTER_ONLY))
            .expect("the source's folder made read-only");
        let unlinks_anyway = fs::write(standing.join("probe"), b"").is_ok();

        let refused = (!unlinks_anyway).then(|| linked_over_nothing(&from, &to));
        let left = fs::read(&from);
        let landed = to.exists();

        fs::set_permissions(&standing, fs::Permissions::from_mode(OWNED_AND_WRITABLE))
            .expect("the source's folder made writable again");

        if let Some(refused) = refused {
            assert_eq!(
                refused.map_err(|error| error.kind()),
                Err(io::ErrorKind::PermissionDenied)
            );
            assert_eq!(left.expect("the source stays"), b"moving");
            assert!(
                !landed,
                "the link was left beside the source it could not move"
            );
        }
    }
}
