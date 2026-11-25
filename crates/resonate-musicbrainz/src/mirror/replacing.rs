use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use resonate_core::file_facts::{sync_directory, sync_file};

use crate::{Error, LandStep, Result};

const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

fn sidecar_of(path: &Path, suffix: &str) -> PathBuf {
    let mut named = OsString::from(path.as_os_str());

    named.push(suffix);
    PathBuf::from(named)
}

fn absent(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

fn sidecars_removed(path: &Path) -> io::Result<()> {
    for suffix in SIDECARS {
        absent(&sidecar_of(path, suffix))?;
    }
    Ok(())
}

pub(crate) fn files_removed(path: &Path) -> io::Result<()> {
    sidecars_removed(path)?;
    absent(path)
}

pub(crate) struct Replacing<'a, F> {
    folder: &'a Path,
    before: F,
}

impl<'a, F: FnMut() -> io::Result<()>> Replacing<'a, F> {
    pub(crate) const fn within(folder: &'a Path, before: F) -> Self {
        Self { folder, before }
    }

    fn step(
        &mut self,
        path: &Path,
        step: LandStep,
        taken: impl FnOnce() -> io::Result<()>,
    ) -> Result<()> {
        (self.before)()
            .and_then(|()| taken())
            .map_err(|source| Error::Landing {
                path: path.to_path_buf(),
                step,
                source,
            })
    }

    pub(crate) fn synced(&mut self, path: &Path) -> Result<()> {
        self.step(path, LandStep::SyncFile, || sync_file(path))
    }

    pub(crate) fn folder_synced(&mut self) -> Result<()> {
        let folder = self.folder;

        self.step(folder, LandStep::SyncFolder, || sync_directory(folder))
    }

    pub(crate) fn placed(&mut self, from: &Path, to: &Path) -> Result<()> {
        self.step(to, LandStep::ClearSidecars, || sidecars_removed(to))?;
        self.step(from, LandStep::Rename, || fs::rename(from, to))?;
        self.folder_synced()
    }

    pub(crate) fn removed(&mut self, path: &Path) -> Result<()> {
        self.step(path, LandStep::Remove, || files_removed(path))?;
        self.folder_synced()
    }
}
