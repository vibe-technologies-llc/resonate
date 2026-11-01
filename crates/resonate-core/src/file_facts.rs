use std::{
    fs::{self, Metadata},
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use crate::volume_mounts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    device: u64,
    node: u64,
    length: u64,
    modified: (i64, i64),
}

impl FileStamp {
    pub fn of(metadata: &Metadata) -> Self {
        Self {
            device: device_of(metadata),
            node: node_of(metadata),
            length: metadata.len(),
            modified: modified_of(metadata),
        }
    }

    pub fn at(path: &Path) -> io::Result<Self> {
        fs::metadata(path).map(|metadata| Self::of(&metadata))
    }

    pub const fn length(self) -> u64 {
        self.length
    }
}

#[cfg(unix)]
fn device_of(metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;

    metadata.dev()
}

#[cfg(unix)]
fn node_of(metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;

    metadata.ino()
}

#[cfg(unix)]
fn modified_of(metadata: &Metadata) -> (i64, i64) {
    use std::os::unix::fs::MetadataExt as _;

    (metadata.mtime(), metadata.mtime_nsec())
}

#[cfg(windows)]
const fn device_of(_: &Metadata) -> u64 {
    0
}

#[cfg(windows)]
const fn node_of(_: &Metadata) -> u64 {
    0
}

#[cfg(windows)]
fn modified_of(metadata: &Metadata) -> (i64, i64) {
    let since_the_epoch = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    (
        i64::try_from(since_the_epoch.as_secs()).unwrap_or_default(),
        i64::from(since_the_epoch.subsec_nanos()),
    )
}

#[cfg(unix)]
pub fn changed_at(metadata: &Metadata) -> Option<SystemTime> {
    use std::os::unix::fs::MetadataExt as _;

    let seconds = u64::try_from(metadata.ctime()).ok()?;
    let nanos = u32::try_from(metadata.ctime_nsec()).ok()?;
    UNIX_EPOCH.checked_add(std::time::Duration::new(seconds, nanos))
}

#[cfg(windows)]
pub fn changed_at(metadata: &Metadata) -> Option<SystemTime> {
    metadata.modified().ok()
}

#[cfg(unix)]
pub fn is_the_same_entry(one: &Path, other: &Path) -> bool {
    let (Ok(one), Ok(other)) = (fs::symlink_metadata(one), fs::symlink_metadata(other)) else {
        return false;
    };
    (device_of(&one), node_of(&one)) == (device_of(&other), node_of(&other))
}

#[cfg(unix)]
pub fn is_the_same_folder(one: &Path, other: &Path) -> bool {
    let (Ok(one), Ok(other)) = (fs::metadata(one), fs::metadata(other)) else {
        return false;
    };
    (device_of(&one), node_of(&one)) == (device_of(&other), node_of(&other))
}

#[cfg(windows)]
pub fn is_the_same_entry(one: &Path, other: &Path) -> bool {
    same_file::is_same_file(one, other).unwrap_or(false)
}

#[cfg(windows)]
pub fn is_the_same_folder(one: &Path, other: &Path) -> bool {
    same_file::is_same_file(one, other).unwrap_or(false)
}

#[cfg(unix)]
pub fn volume_of(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| device_of(&metadata))
}

#[cfg(windows)]
pub fn volume_of(path: &Path) -> Option<u64> {
    use std::hash::{DefaultHasher, Hash as _, Hasher as _};

    fs::metadata(path).ok()?;
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    let mut hasher = DefaultHasher::new();
    volume_name_of(&real).hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(windows)]
fn volume_name_of(real: &Path) -> String {
    use std::path::Component;

    let prefix = match real.components().next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().into_owned(),
        _ => String::new(),
    };
    volume_mounts::identity_of(
        &prefix,
        volume_mounts::nearest_mounted_volume(real, volume_mounted_at),
    )
}

#[cfg(windows)]
fn volume_mounted_at(folder: &Path) -> Option<std::path::PathBuf> {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    let attributes = fs::symlink_metadata(folder).ok()?.file_attributes();
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return None;
    }
    fs::read_link(folder).ok()
}

#[cfg(unix)]
pub fn sync_directory(folder: &Path) -> io::Result<()> {
    fs::File::open(folder)?.sync_all()
}

#[cfg(windows)]
pub fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn sync_file(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(windows)]
pub fn sync_file(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let folder = env::temp_dir().join(format!("resonate-facts-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_stamp_changes_with_the_length_of_the_file() {
        let folder = scratch("stamp");
        let file = folder.join("a");

        fs::write(&file, b"one").expect("a file");
        let before = FileStamp::at(&file).expect("a stamp");
        fs::write(&file, b"three").expect("a longer file");
        let after = FileStamp::at(&file).expect("a stamp");

        assert_ne!(before, after);
        assert_eq!(after.length(), 5);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_is_the_same_entry_as_itself_and_not_as_another() {
        let folder = scratch("entry");
        let (one, other) = (folder.join("one"), folder.join("other"));
        fs::write(&one, b"x").expect("a file");
        fs::write(&other, b"x").expect("a file");

        assert!(is_the_same_entry(&one, &one));
        assert!(!is_the_same_entry(&one, &other));
        assert!(is_the_same_folder(&folder, &folder));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_folder_has_a_volume_and_a_missing_path_has_none() {
        let folder = scratch("volume");

        assert!(volume_of(&folder).is_some());
        assert_eq!(volume_of(&folder.join("missing")), None);
        assert!(sync_directory(&folder).is_ok());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_has_a_change_time() {
        let folder = scratch("changed");
        let file = folder.join("a");
        fs::write(&file, b"x").expect("a file");

        let metadata = fs::metadata(&file).expect("metadata");

        assert!(changed_at(&metadata).is_some());
        let _ = fs::remove_dir_all(&folder);
    }
}
