use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::writer::{self, Held, Writer};
use rusqlite::{Connection, OpenFlags};

use super::{
    AMENDED_FILE, AmendedTo, Mirror,
    amended::{Amended, WAITED_FOR_A_WRITER, amended_to_in},
    import::removed,
    mirror_in,
    replacing::Replacing,
};
use crate::{CopyOp, Error, Result, Store, StoreOp};

const SNAPSHOT_STEM: &str = "musicbrainz.amended.offered-";
const SNAPSHOT_EXTENSION: &str = ".sqlite";
const UNLOCKED: &str = "?nolock=1";
const LEFT_AS_WRITTEN_IN_A_URI: &[u8] = b"/-_.~";

static SNAPSHOTS_MADE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyPart {
    TheCopy,
    ItsAmendments,
}

pub struct Offered {
    pub dump: String,
    pub sequence: u64,
    pub size: u64,
    pub file: OfferedFile,
}

pub struct OfferedFile {
    file: Option<File>,
    snapshot: Option<(Held, PathBuf)>,
}

impl Read for OfferedFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.file.as_mut() {
            Some(file) => file.read(buffer),
            None => Ok(0),
        }
    }
}

impl Seek for OfferedFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match self.file.as_mut() {
            Some(file) => file.seek(to),
            None => Ok(0),
        }
    }
}

impl Drop for OfferedFile {
    fn drop(&mut self) {
        self.file = None;
        if let Some((held, path)) = self.snapshot.take() {
            drop(held);

            let _ = removed(&path);
        }
    }
}

fn copy_failed(path: &Path, op: CopyOp) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::CopyFile {
        path: path.to_path_buf(),
        op,
        source,
    }
}

impl Mirror {
    pub fn offered(&self, part: CopyPart) -> Result<Option<Offered>> {
        match part {
            CopyPart::TheCopy => self.offered_copy().map(Some),
            CopyPart::ItsAmendments => self.offered_amendments(),
        }
    }

    fn offered_copy(&self) -> Result<Offered> {
        let path = self.path();
        let file = File::open(path).map_err(copy_failed(path, CopyOp::Open))?;

        let size = file
            .metadata()
            .map_err(copy_failed(path, CopyOp::Measure))?
            .len();
        Ok(Offered {
            dump: self.made().dump.clone(),
            sequence: 0,
            size,
            file: OfferedFile {
                file: Some(file),
                snapshot: None,
            },
        })
    }

    fn offered_amendments(&self) -> Result<Option<Offered>> {
        if self.amended_to().is_none() {
            return Ok(None);
        }

        let Some(folder) = self.path().parent() else {
            return Ok(None);
        };

        swept_snapshots(folder);

        let snapshot = folder.join(format!(
            "{SNAPSHOT_STEM}{}-{}{SNAPSHOT_EXTENSION}",
            process::id(),
            SNAPSHOTS_MADE.fetch_add(1, Ordering::Relaxed)
        ));
        let held = Held::made(&snapshot).map_err(copy_failed(&snapshot, CopyOp::Hold))?;
        let taken = snapshot_of(&folder.join(AMENDED_FILE), &snapshot, &self.made().dump);

        let to = match taken {
            Ok(Some(to)) => to,
            Ok(None) | Err(_) => {
                drop(held);

                let _ = removed(&snapshot);

                return taken.map(|_| None);
            }
        };
        let opened = File::open(&snapshot).and_then(|file| Ok((file.metadata()?.len(), file)));
        let (size, file) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                drop(held);

                let _ = removed(&snapshot);

                return Err(copy_failed(&snapshot, CopyOp::Open)(error));
            }
        };
        Ok(Some(Offered {
            dump: to.dump,
            sequence: to.sequence,
            size,
            file: OfferedFile {
                file: Some(file),
                snapshot: Some((held, snapshot)),
            },
        }))
    }
}

fn snapshot_of(amended: &Path, snapshot: &Path, dump: &str) -> Result<Option<AmendedTo>> {
    let read_by_uri = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    let unlocked = unlocked_uri_of(snapshot);

    let connection = Connection::open_with_flags(amended, read_by_uri)
        .map_err(Error::store(Store::Amended, StoreOp::Open))?;

    connection
        .busy_timeout(WAITED_FOR_A_WRITER)
        .map_err(Error::store(Store::Amended, StoreOp::Configure))?;
    connection
        .execute("VACUUM INTO ?1", [&unlocked])
        .map_err(Error::store(Store::Amended, StoreOp::Read))?;
    drop(connection);

    let taken = Connection::open_with_flags(&unlocked, read_by_uri)
        .map_err(Error::store(Store::Amended, StoreOp::Open))?;
    Ok(amended_to_in(&taken, "")?.filter(|to| to.dump == dump))
}

fn unlocked_uri_of(path: &Path) -> String {
    let mut uri = String::from("file:");

    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || LEFT_AS_WRITTEN_IN_A_URI.contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri.push_str(UNLOCKED);
    uri
}

fn swept_snapshots(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(stamp) = name
            .to_str()
            .and_then(|name| name.strip_prefix(SNAPSHOT_STEM))
            .and_then(|rest| rest.strip_suffix(SNAPSHOT_EXTENSION))
        else {
            continue;
        };
        let stamped = stamp
            .split_once('-')
            .and_then(|(pid, _)| pid.parse().ok())
            .unwrap_or(0);
        if writer::writer_of(&entry.path(), stamped) == Writer::Gone {
            let _ = removed(&entry.path());
        }
    }
}

pub fn exported_after(offered: &str, held: &str) -> bool {
    match (dump_stamp(offered), dump_stamp(held)) {
        (Some(offered), Some(held)) => offered > held,
        _ => false,
    }
}

fn dump_stamp(dump: &str) -> Option<Vec<u64>> {
    dump.split('-')
        .map(|part| {
            Some(part)
                .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
                .and_then(|part| part.parse().ok())
        })
        .collect()
}

pub fn removed_copy(folder: &Path) -> Result<()> {
    super::removed_beside_the_mirror(folder)?;
    removed(&mirror_in(folder))
}

pub fn received_copy(folder: &Path, staged: &Path) -> Result<()> {
    let landed = Mirror::open(staged)?;

    drop(landed);

    let mut replacing = Replacing::within(folder, || Ok(()));

    replacing.synced(staged)?;
    replacing.folder_synced()?;
    replacing.placed(staged, &mirror_in(folder))?;
    replacing.removed(&folder.join(AMENDED_FILE))
}

pub fn received_amendments(folder: &Path, staged: &Path) -> Result<()> {
    let copy = Mirror::open(&mirror_in(folder))?;
    let held = Amended::open(staged, &copy.made().dump)?;

    if held.is_none() {
        return Err(Error::AmendmentsOfAnotherExport {
            path: staged.to_path_buf(),
            copy: copy.made().dump.clone(),
        });
    }
    drop(held);
    drop(copy);

    let mut replacing = Replacing::within(folder, || Ok(()));

    replacing.synced(staged)?;
    replacing.folder_synced()?;
    replacing.placed(staged, &folder.join(AMENDED_FILE))
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::{super::amended::made_at, *};

    #[test]
    fn a_snapshot_is_named_to_sqlite_by_a_uri_that_keeps_every_letter_of_its_path() {
        assert_eq!(
            unlocked_uri_of(Path::new("/tmp/a b?#%/x.sqlite")),
            "file:/tmp/a%20b%3F%23%25/x.sqlite?nolock=1"
        );
        assert_eq!(
            unlocked_uri_of(Path::new(r"C:\Users\RUNNER~1\x.sqlite")),
            "file:C%3A%5CUsers%5CRUNNER~1%5Cx.sqlite?nolock=1"
        );
        assert_eq!(
            unlocked_uri_of(Path::new("/m\u{fc}sik/x.sqlite")),
            "file:/m%C3%BCsik/x.sqlite?nolock=1"
        );
    }

    #[test]
    fn an_export_is_after_another_only_where_both_are_dumps_and_it_was_made_later() {
        assert!(exported_after("20261111-002335", "20261104-002222"));
        assert!(exported_after("20261111-002336", "20261111-002335"));
        assert!(exported_after("20270101-000000", "20261231-235959"));
        assert!(!exported_after("20261104-002222", "20261111-002335"));
        assert!(!exported_after("20261111-002335", "20261111-002335"));
        assert!(!exported_after("LATEST", "20261111-002335"));
        assert!(!exported_after("20261111-002335", ""));
        assert!(!exported_after("20261111--002335", "20261104-002222"));
    }

    #[test]
    fn a_held_snapshot_is_taken_in_a_folder_whose_name_a_uri_would_misread() {
        let folder = env::temp_dir().join(format!(
            "resonate-musicbrainz-snapshot #% & {}",
            process::id()
        ));
        let _ = fs::remove_dir_all(&folder);

        fs::create_dir_all(&folder).expect("a scratch folder");

        let amended = folder.join(AMENDED_FILE);
        let snapshot = folder.join(format!("{SNAPSHOT_STEM}1-0{SNAPSHOT_EXTENSION}"));

        made_at(&amended, "20261104-002222", 7, 1).expect("the amendments laid out");

        let held = Held::made(&snapshot).expect("the snapshot held");
        let taken = snapshot_of(&amended, &snapshot, "20261104-002222").expect("a snapshot");
        let size = fs::metadata(&snapshot).expect("the snapshot").len();

        drop(held);

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(taken.map(|to| to.sequence), Some(7));
        assert!(size > 0);
    }
}
