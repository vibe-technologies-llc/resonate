use std::{fs::File, os::windows::io::AsRawHandle as _, path::Path};

use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
};

use crate::error::{Error, Op, Result, refused_on};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub volume: u32,
    pub index: u64,
}

pub fn file_identity(path: &Path) -> Result<FileIdentity> {
    let file = File::open(path).map_err(|source| Error::Io {
        op: Op::ReadFileIdentity,
        path: path.to_path_buf(),
        source,
    })?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut information) }
        .map_err(refused_on(Op::ReadFileIdentity, path))?;

    Ok(FileIdentity {
        volume: information.dwVolumeSerialNumber,
        index: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs::{self, OpenOptions},
        io::Write as _,
        process,
    };

    use super::*;

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
