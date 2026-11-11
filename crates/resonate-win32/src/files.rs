use std::{fs::File, path::Path};

use crate::error::{Error, Op, Raw, Result, refused_on};
#[cfg(windows)]
use crate::system::Windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Information {
    pub(crate) volume: u32,
    pub(crate) index_high: u32,
    pub(crate) index_low: u32,
}

pub(crate) trait Identifying {
    fn information_of(&self, file: &File) -> Raw<Information>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub volume: u32,
    pub index: u64,
}

pub(crate) fn identity_with(identifying: &impl Identifying, path: &Path) -> Result<FileIdentity> {
    let file = File::open(path).map_err(|source| Error::Io {
        op: Op::ReadFileIdentity,
        path: path.to_path_buf(),
        source,
    })?;
    let information = identifying
        .information_of(&file)
        .map_err(refused_on(Op::ReadFileIdentity, path))?;

    Ok(FileIdentity {
        volume: information.volume,
        index: (u64::from(information.index_high) << 32) | u64::from(information.index_low),
    })
}

#[cfg(windows)]
pub fn file_identity(path: &Path) -> Result<FileIdentity> {
    identity_with(&Windows, path)
}

#[cfg(windows)]
mod real {
    use std::{fs::File, os::windows::io::AsRawHandle as _};

    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };

    use super::{Identifying, Information};
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Identifying for Windows {
        fn information_of(&self, file: &File) -> Raw<Information> {
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            unsafe {
                GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut information)
            }
            .map_err(hresult)?;
            Ok(Information {
                volume: information.dwVolumeSerialNumber,
                index_high: information.nFileIndexHigh,
                index_low: information.nFileIndexLow,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{
            env,
            fs::{self, OpenOptions},
            io::Write as _,
            process,
        };

        use super::super::*;

        #[test]
        fn a_file_written_in_place_keeps_its_identity_and_one_renamed_over_it_does_not() {
            let folder = env::temp_dir().join(format!("resonate-identity-{}", process::id()));
            fs::create_dir_all(&folder).expect("a folder");
            let held = folder.join("held.bin");
            let staged = folder.join("staged.bin");
            fs::write(&held, b"first").expect("a file");

            let before = file_identity(&held).expect("the file's identity");
            OpenOptions::new()
                .write(true)
                .open(&held)
                .and_then(|mut file| file.write_all(b"FIRST"))
                .expect("a write in place");
            let in_place = file_identity(&held).expect("the identity after a write in place");
            fs::write(&staged, b"second").expect("a staged copy");
            fs::rename(&staged, &held).expect("the copy renamed over the file");
            let renamed = file_identity(&held).expect("the identity after a rename over it");
            let _ = fs::remove_dir_all(&folder);

            assert_eq!(in_place, before);
            assert_ne!(renamed, before);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;
    use crate::error::HResult;

    struct Fake(Raw<Information>);

    impl Identifying for Fake {
        fn information_of(&self, _: &File) -> Raw<Information> {
            self.0
        }
    }

    fn an_existing_file() -> std::path::PathBuf {
        env::current_exe().expect("this test's own executable")
    }

    #[test]
    fn the_index_is_its_high_word_over_its_low_word() {
        let fake = Fake(Ok(Information {
            volume: 0xABCD,
            index_high: 0x0001_0002,
            index_low: 0x0003_0004,
        }));

        let identity = identity_with(&fake, &an_existing_file()).expect("an identity");

        assert_eq!(
            identity,
            FileIdentity {
                volume: 0xABCD,
                index: 0x0001_0002_0003_0004
            }
        );
    }

    #[test]
    fn a_file_that_will_not_open_is_an_io_error_naming_it() {
        let missing = an_existing_file().join("absent");

        let error = identity_with(&Fake(Err(HResult(-1))), &missing).expect_err("refused");

        assert!(matches!(
            error,
            Error::Io { op: Op::ReadFileIdentity, ref path, .. } if *path == missing
        ));
    }

    #[test]
    fn a_refused_read_names_the_file_and_the_code() {
        let path = an_existing_file();

        let error = identity_with(&Fake(Err(HResult(-8))), &path).expect_err("refused");

        assert!(matches!(
            error,
            Error::WindowsOnFile { op: Op::ReadFileIdentity, path: ref named, source: HResult(-8) }
                if *named == path
        ));
    }
}
