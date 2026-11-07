use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
    process,
    time::{Duration, SystemTime},
};

const RUNNING_PROCESSES: &str = "/proc";

pub const UNHELD_AND_UNTOUCHED_FOR: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    Living,
    Gone,
}

pub struct Held(File);

impl Held {
    pub fn made(path: &Path) -> io::Result<Self> {
        Ok(Self::over(File::create_new(path)?))
    }

    pub fn taken_over(path: &Path) -> io::Result<Self> {
        let file = holding::claimed(
            path,
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )?;
        file.set_len(0)?;
        Ok(Self(file))
    }

    pub fn resumed(path: &Path) -> io::Result<Self> {
        holding::claimed(path, OpenOptions::new().read(true).write(true)).map(Self)
    }

    pub fn over(file: File) -> Self {
        holding::hold(&file);
        Self(file)
    }

    pub const fn file(&self) -> &File {
        &self.0
    }
}

pub fn writer_of(path: &Path, stamped: u32) -> Writer {
    match holding::looked_at(path) {
        Looked::Held => Writer::Living,
        Looked::Unheld(file) if unheld_long_enough(&file) => Writer::Gone,
        Looked::Unheld(_) | Looked::Unknowable => by_its_stamp(stamped),
    }
}

enum Looked {
    Held,
    Unheld(File),
    Unknowable,
}

#[cfg(unix)]
mod holding {
    use std::{
        fs::{File, OpenOptions, TryLockError},
        io,
        path::Path,
    };

    use super::Looked;

    pub(super) fn claimed(path: &Path, options: &OpenOptions) -> io::Result<File> {
        let file = options.open(path)?;
        match file.try_lock() {
            Ok(()) | Err(TryLockError::Error(_)) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(io::ErrorKind::WouldBlock.into()),
        }
    }

    pub(super) fn hold(file: &File) {
        let _ = file.lock();
    }

    pub(super) fn looked_at(path: &Path) -> Looked {
        let opened = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .or_else(|_| File::open(path));
        let Ok(file) = opened else {
            return Looked::Unknowable;
        };
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => Looked::Held,
            Err(TryLockError::Error(_)) => Looked::Unknowable,
            Ok(()) => Looked::Unheld(file),
        }
    }
}

#[cfg(windows)]
mod holding {
    use std::{
        fs::{File, OpenOptions},
        io,
        os::windows::fs::OpenOptionsExt as _,
        path::Path,
    };

    use super::Looked;

    const ERROR_SHARING_VIOLATION: i32 = 32;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;

    pub(super) fn claimed(path: &Path, options: &OpenOptions) -> io::Result<File> {
        match looked_at(path) {
            Looked::Held => Err(io::ErrorKind::WouldBlock.into()),
            Looked::Unheld(probe) => {
                drop(probe);
                options.open(path)
            }
            Looked::Unknowable => options.open(path),
        }
    }

    pub(super) const fn hold(_: &File) {}

    pub(super) fn looked_at(path: &Path) -> Looked {
        match OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_DELETE)
            .open(path)
        {
            Ok(file) => Looked::Unheld(file),
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => Looked::Held,
            Err(_) => Looked::Unknowable,
        }
    }
}

fn unheld_long_enough(file: &File) -> bool {
    file.metadata()
        .and_then(|held| held.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|untouched| untouched >= UNHELD_AND_UNTOUCHED_FOR)
}

fn by_its_stamp(stamped: u32) -> Writer {
    let processes = Path::new(RUNNING_PROCESSES);
    let living = stamped == process::id()
        || !processes.is_dir()
        || processes.join(stamped.to_string()).exists();
    match living {
        true => Writer::Living,
        false => Writer::Gone,
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    use super::*;

    const NEVER_A_PROCESS: u32 = 999_999_999;
    const INIT: u32 = 1;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-writer-{}-{name}", process::id()));
            let _ = fs::remove_file(&path);
            Self(path)
        }

        fn aged(&self) {
            let long_ago = SystemTime::now() - UNHELD_AND_UNTOUCHED_FOR * 2;
            File::options()
                .write(true)
                .open(&self.0)
                .and_then(|file| file.set_modified(long_ago))
                .expect("an aged file");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_held_file_is_living_whatever_process_its_stamp_names() {
        let scratch = Scratch::new("held");

        let held = Held::made(&scratch.0).expect("a held file");
        scratch.aged();

        assert_eq!(writer_of(&scratch.0, NEVER_A_PROCESS), Writer::Living);
        drop(held);
        assert_eq!(writer_of(&scratch.0, NEVER_A_PROCESS), Writer::Gone);
    }

    #[test]
    fn an_unheld_file_left_alone_is_gone_though_its_stamp_names_a_running_process() {
        let scratch = Scratch::new("unheld");

        fs::write(&scratch.0, b"left").expect("a file");
        let fresh = writer_of(&scratch.0, INIT);
        scratch.aged();
        let aged = writer_of(&scratch.0, INIT);
        let ours = writer_of(&scratch.0, process::id());

        assert_eq!(fresh, Writer::Living);
        assert_eq!(aged, Writer::Gone);
        assert_eq!(ours, Writer::Gone);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_fresh_unheld_file_is_gone_only_where_its_stamp_names_no_running_process() {
        let scratch = Scratch::new("fresh");

        fs::write(&scratch.0, b"just made").expect("a file");

        assert_eq!(writer_of(&scratch.0, process::id()), Writer::Living);
        assert_eq!(writer_of(&scratch.0, NEVER_A_PROCESS), Writer::Gone);
    }

    #[test]
    fn a_file_taken_over_is_emptied_unless_a_living_writer_holds_it() {
        let scratch = Scratch::new("taken");

        fs::write(&scratch.0, b"what a dead run left").expect("a file");
        let taken = Held::taken_over(&scratch.0).expect("taken over");
        let refused = Held::taken_over(&scratch.0).map(drop);

        assert_eq!(fs::read(&scratch.0).expect("the file"), b"");
        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        drop(taken);
    }

    #[test]
    fn a_file_resumed_keeps_what_it_held_unless_a_living_writer_holds_it() {
        let scratch = Scratch::new("resumed");

        fs::write(&scratch.0, b"what a dead run left").expect("a file");
        let resumed = Held::resumed(&scratch.0).expect("resumed");
        let refused = Held::resumed(&scratch.0).map(drop);

        assert_eq!(
            fs::read(&scratch.0).expect("the file"),
            b"what a dead run left"
        );
        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        drop(resumed);
        assert!(Held::resumed(&scratch.0).is_ok());
    }

    #[test]
    fn a_file_that_is_not_there_cannot_be_resumed() {
        let scratch = Scratch::new("absent");

        let refused = Held::resumed(&scratch.0).map(drop);

        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(io::ErrorKind::NotFound)
        );
    }
}
