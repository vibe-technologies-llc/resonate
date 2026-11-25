use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

#[cfg(unix)]
fn without_waiting(options: &mut OpenOptions) -> &mut OpenOptions {
    use std::os::unix::fs::OpenOptionsExt as _;

    use rustix::fs::OFlags;

    options.custom_flags((OFlags::NONBLOCK | OFlags::NOCTTY).bits().cast_signed())
}

#[cfg(not(unix))]
fn without_waiting(options: &mut OpenOptions) -> &mut OpenOptions {
    options
}

pub(crate) fn opened(path: &Path) -> io::Result<Option<File>> {
    let file = without_waiting(OpenOptions::new().read(true)).open(path)?;

    Ok(file.metadata()?.is_file().then_some(file))
}

#[cfg(all(test, unix))]
mod tests {
    use std::{env, fs, process, time::Duration};

    use rustix::fs::{CWD, FileType, Mode, mknodat};

    use super::*;

    struct Folder(std::path::PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-regular-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);

            fs::create_dir_all(&path).expect("a writable temporary directory");
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_regular_file_is_opened_and_a_folder_or_a_missing_name_is_not() {
        let folder = Folder::new("kinds");
        let sheet = folder.0.join("Song.lrc");

        fs::write(&sheet, b"[00:01.00]la").expect("a sheet");

        assert!(opened(&sheet).expect("it opens").is_some());
        assert!(opened(&folder.0).expect("it opens").is_none());
        assert_eq!(
            opened(&folder.0.join("Gone.lrc"))
                .map(|file| file.is_some())
                .map_err(|error| error.kind()),
            Err(io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn a_fifo_is_passed_over_at_once_with_no_writer_to_wake_it() {
        let folder = Folder::new("fifo");
        let fifo = folder.0.join("Song.lrc");

        mknodat(CWD, &fifo, FileType::Fifo, Mode::RUSR | Mode::WUSR, 0).expect("a fifo");

        let (told, hears) = std::sync::mpsc::channel();
        let asked = fifo.clone();

        std::thread::spawn(move || {
            let _ = told.send(
                opened(&asked)
                    .map(|file| file.is_some())
                    .map_err(|error| error.kind()),
            );
        });

        assert_eq!(
            hears
                .recv_timeout(Duration::from_secs(10))
                .expect("it did not block"),
            Ok(false)
        );
    }
}
