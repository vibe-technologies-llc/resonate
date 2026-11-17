use std::{
    fs::File,
    io,
    os::fd::{AsFd as _, AsRawFd as _, BorrowedFd},
};

use libc::c_int;

use crate::error::{Error, LeaseOp, Result};

const F_SETSIG: c_int = 10;
const NO_OWNER: c_int = 0;
const IGNORED_UNLESS_HANDLED: c_int = libc::SIGURG;

pub struct Lease<'f> {
    file: BorrowedFd<'f>,
}

impl<'f> Lease<'f> {
    pub fn taken(file: &'f File) -> Result<Self> {
        let file = file.as_fd();

        set(file, F_SETSIG, IGNORED_UNLESS_HANDLED).map_err(|source| Error::Fcntl {
            op: LeaseOp::Signal,
            source,
        })?;
        match set(file, libc::F_SETLEASE, libc::F_WRLCK) {
            Ok(()) => {}
            Err(conflict) if conflict.raw_os_error() == Some(libc::EAGAIN) => {
                return Err(Error::OpenElsewhere);
            }
            Err(source) => {
                return Err(Error::Fcntl {
                    op: LeaseOp::Take,
                    source,
                });
            }
        }

        let lease = Self { file };
        set(file, libc::F_SETOWN, NO_OWNER).map_err(|source| Error::Fcntl {
            op: LeaseOp::Disown,
            source,
        })?;
        Ok(lease)
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let _ = set(self.file, libc::F_SETLEASE, libc::F_UNLCK);
    }
}

fn set(file: BorrowedFd<'_>, command: c_int, argument: c_int) -> io::Result<()> {
    let answered = unsafe { libc::fcntl(file.as_raw_fd(), command, argument) };
    if answered == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs::{self, File, OpenOptions},
        io,
        os::unix::fs::OpenOptionsExt as _,
        path::PathBuf,
        process,
    };

    use super::Lease;
    use crate::error::Error;

    struct Scratch(PathBuf);

    impl Scratch {
        fn named(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-lease-{}-{name}", process::id()));
            fs::write(&path, b"held").expect("a scratch file");
            Self(path)
        }

        fn written(&self) -> File {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.0)
                .expect("the file opened to write")
        }

        fn opened_without_waiting(&self) -> io::Result<File> {
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&self.0)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_file_nobody_else_opens_is_leased_and_a_newcomer_waits_until_it_is_released() {
        let file = Scratch::named("alone");
        let writing = file.written();

        let lease = Lease::taken(&writing).expect("a lease on a file nobody else opens");
        let while_leased = file.opened_without_waiting().map(drop);
        drop(lease);
        let once_released = file.opened_without_waiting().map(drop);

        assert_eq!(
            while_leased.map_err(|refused| refused.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        assert!(once_released.is_ok());
    }

    #[test]
    fn a_file_another_handle_holds_is_open_elsewhere() {
        let file = Scratch::named("held");
        let writing = file.written();
        let reader = File::open(&file.0).expect("a reader");

        let leased = Lease::taken(&writing);
        drop(reader);

        assert!(matches!(leased, Err(Error::OpenElsewhere)));
    }
}
