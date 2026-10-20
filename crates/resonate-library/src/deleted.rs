use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use ahash::AHashSet;
use resonate_codec::the_folder_a_cue_names;
use resonate_core::{MediaLocation, TrackId};
use rusqlite::{Connection, OptionalExtension as _, Transaction, params};

use crate::{Error, Result, StoreOp, TrashOp, alternatives, scan, store, trash};

const LYRICS_EXTENSION: &str = "lrc";

const LISTENS_KEPT_AS_UNHELD: &str =
    "INSERT INTO unheld_listens (path, span_start, at, began, heard)
     SELECT t.path, t.span_start, l.at, l.began, l.heard
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE t.path = ?1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    pub tracks: u64,
    pub files: u64,
    pub kept: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    Removed,
    AlreadyGone,
    Kept,
}

pub trait Trashes: Send + Sync {
    fn trash(&self, file: &Path) -> Removal;
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
        files.insert(store::path_of(&path));
    }
    Ok(files)
}

pub(crate) fn removed(
    path: &Path,
    data_home: Option<&Path>,
    through: Option<&dyn Trashes>,
) -> Removal {
    if let Some(through) = through {
        return through.trash(path);
    }

    match trash::send(path, data_home, SystemTime::now()) {
        Ok(()) => Removal::Removed,
        Err(Error::Trash {
            op: TrashOp::Stat,
            source,
            ..
        }) if source.kind() == io::ErrorKind::NotFound => Removal::AlreadyGone,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "a file asked to be deleted could not be moved to the trash");
            Removal::Kept
        }
    }
}

pub(crate) fn sidecars_of(file: &Path) -> Vec<PathBuf> {
    let Some(folder) = file.parent() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let beside: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .map(|entry| entry.path())
        .collect();

    let mut sidecars = lyrics_of(file, &beside);
    sidecars.extend(
        beside
            .iter()
            .filter(|candidate| scan::is_a_sheet(candidate))
            .filter(|sheet| cuts_only(sheet, folder, file, &beside))
            .cloned(),
    );
    sidecars
}

fn lyrics_of(file: &Path, beside: &[PathBuf]) -> Vec<PathBuf> {
    let stem = file.file_stem();
    let shared_with_another_song = beside
        .iter()
        .any(|other| other != file && scan::is_audio(other) && other.file_stem() == stem);
    if shared_with_another_song {
        return Vec::new();
    }

    beside
        .iter()
        .filter(|other| other.file_stem() == stem)
        .filter(|other| {
            other
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case(LYRICS_EXTENSION))
        })
        .cloned()
        .collect()
}

fn cuts_only(sheet: &Path, folder: &Path, file: &Path, beside: &[PathBuf]) -> bool {
    let Some(read) = scan::read_sheet(sheet) else {
        return false;
    };

    let mut cuts_the_file = false;
    for cut in &read.files {
        if the_folder_a_cue_names(folder, &cut.named) != folder {
            return false;
        }
        match scan::claimed_beside(&cut.named, beside) {
            Some(claimed) if claimed == file => cuts_the_file = true,
            Some(_) | None => return false,
        }
    }
    cuts_the_file
}

pub(crate) fn forget_files(tx: &Transaction<'_>, files: &[PathBuf]) -> Result<Forgotten> {
    let mut forgotten = Forgotten {
        tracks: 0,
        vault_keys: AHashSet::new(),
    };
    for path in files {
        let named = store::path_text(path);
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

        tx.execute(LISTENS_KEPT_AS_UNHELD, params![named])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
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
        alternatives::settle_if_owed(tx)?;
    }
    Ok(forgotten)
}
