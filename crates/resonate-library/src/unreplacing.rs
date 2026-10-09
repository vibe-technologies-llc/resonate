use std::{fs, io, path::Path};

use rustix::{
    fs::{CWD, RenameFlags, renameat_with},
    io::Errno,
};

pub(crate) fn renamed_over_nothing(from: &Path, to: &Path) -> io::Result<()> {
    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => linked_over_nothing(from, to),
        Err(errno) => Err(errno.into()),
    }
}

fn linked_over_nothing(from: &Path, to: &Path) -> io::Result<()> {
    match fs::hard_link(from, to) {
        Ok(()) => fs::remove_file(from),
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
}
