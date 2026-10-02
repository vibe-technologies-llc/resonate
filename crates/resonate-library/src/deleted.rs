use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use ahash::AHashSet;
use resonate_core::{MediaLocation, TrackId};
use rusqlite::{Connection, OptionalExtension as _, Transaction, params};

use crate::{Error, Result, StoreOp, alternatives, store};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    pub tracks: u64,
    pub files: u64,
    pub kept: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Removal {
    Removed,
    AlreadyGone,
    Kept,
}

pub(crate) struct Forgotten {
    pub(crate) tracks: u64,
    pub(crate) vault_keys: AHashSet<String>,
}

pub(crate) fn files_of(connection: &Connection, tracks: &[TrackId]) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for &track in tracks {
        let path = connection
            .query_row(
                "SELECT path FROM tracks WHERE id = ?1",
                params![track.get() as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?
            .ok_or(Error::UnknownTrack(track))?;
        files.insert(PathBuf::from(path));
    }
    Ok(files)
}

pub(crate) fn removed(path: &Path) -> Removal {
    match fs::remove_file(path) {
        Ok(()) => Removal::Removed,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Removal::AlreadyGone,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "a file asked to be deleted could not be removed");
            Removal::Kept
        }
    }
}

pub(crate) fn forget_files(tx: &Transaction<'_>, files: &[PathBuf]) -> Result<Forgotten> {
    let mut forgotten = Forgotten {
        tracks: 0,
        vault_keys: AHashSet::new(),
    };
    for path in files {
        let named = store::path_text(path)?;
        let offered = MediaLocation::local(path).to_uri();

        let mut keys = tx
            .prepare(
                "SELECT DISTINCT vault_key FROM tracks WHERE path = ?1 AND vault_key IS NOT NULL",
            )
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let held = keys
            .query_map(params![named], |row| row.get::<_, String>(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        forgotten.vault_keys.extend(held);

        tx.execute(
            "DELETE FROM wants
              WHERE offered = ?2
                 OR release_track_id IN
                    (SELECT id FROM release_tracks
                      WHERE track_id IN (SELECT id FROM tracks WHERE path = ?1))",
            params![named, offered],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
        forgotten.tracks +=
            tx.execute("DELETE FROM tracks WHERE path = ?1", params![named])
                .map_err(|source| Error::store(StoreOp::Delete, source))? as u64;
    }
    if forgotten.tracks > 0 {
        store::sweep_orphans(tx)?;
        alternatives::settle(tx)?;
    }
    Ok(forgotten)
}
