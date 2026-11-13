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
    amended::{Amended, amended_to_in},
    import::removed,
    mirror_in,
};
use crate::{CopyOp, Error, Result, Store, StoreOp};

const SNAPSHOT_STEM: &str = "musicbrainz.amended.offered-";
const SNAPSHOT_EXTENSION: &str = ".sqlite";

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
    let connection = Connection::open_with_flags(
        amended,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(Error::store(Store::Amended, StoreOp::Open))?;
    connection
        .execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])
        .map_err(Error::store(Store::Amended, StoreOp::Read))?;
    drop(connection);
    let taken = Connection::open_with_flags(
        snapshot,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(Error::store(Store::Amended, StoreOp::Open))?;
    Ok(amended_to_in(&taken, "")?.filter(|to| to.dump == dump))
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

pub fn removed_copy(folder: &Path) -> Result<()> {
    super::removed_beside_the_mirror(folder)?;
    removed(&mirror_in(folder))
}

pub fn received_copy(folder: &Path, staged: &Path) -> Result<()> {
    let landed = Mirror::open(staged)?;
    drop(landed);
    removed(&folder.join(AMENDED_FILE))?;
    fs::rename(staged, mirror_in(folder)).map_err(copy_failed(staged, CopyOp::Land))
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
    let amended = folder.join(AMENDED_FILE);
    removed(&amended)?;
    fs::rename(staged, &amended).map_err(copy_failed(staged, CopyOp::Land))
}
