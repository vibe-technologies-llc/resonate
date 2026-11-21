use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ahash::{AHashMap, AHashSet};
use resonate_codec::the_folder_a_cue_names;
use resonate_core::{Recording, TrackId};
use rusqlite::{Connection, Transaction, params, params_from_iter};

use crate::{
    Error, Result, StoreOp, everywhere,
    forgetting::{Forgetting, Rows, forget_rows},
    scan, store, synced,
};

const LYRICS_EXTENSION: &str = "lrc";

const PATHS_ASKED_AT_ONCE: usize = 256;

const VAULTED_ROWS_OF_A_ROOT_LEFT_ROOTLESS: &str =
    "UPDATE tracks SET root_id = NULL WHERE root_id = ?1 AND vault_key IS NOT NULL";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    Deleted,
    DeletedEverywhere,
    Folded,
}

pub(crate) struct Forgotten {
    pub(crate) tracks: u64,
    pub(crate) vault_keys: AHashSet<String>,
}

pub(crate) fn files_of(connection: &Connection, tracks: &[TrackId]) -> Result<BTreeSet<PathBuf>> {
    let mut paths: AHashMap<TrackId, String> = AHashMap::with_capacity(tracks.len());

    for batch in tracks.chunks(PATHS_ASKED_AT_ONCE) {
        let held = vec!["?"; batch.len()].join(",");
        let mut statement = connection
            .prepare_cached(&format!("SELECT id, path FROM tracks WHERE id IN ({held})"))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;

        let found = statement
            .query_map(
                params_from_iter(batch.iter().map(|track| track.get() as i64)),
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        for (id, path) in found {
            if let Some(track) = u64::try_from(id).ok().and_then(|id| TrackId::new(id).ok()) {
                paths.insert(track, path);
            }
        }
    }

    tracks
        .iter()
        .map(|track| {
            paths
                .get(track)
                .map(|path| store::path_of(path))
                .ok_or(Error::UnknownTrack(*track))
        })
        .collect()
}

pub(crate) fn removed(
    path: &Path,
    data_home: Option<&Path>,
    through: Option<&dyn Trashes>,
) -> Removal {
    if let Some(through) = through {
        return through.trash(path);
    }

    sent_to_the_hand_written_trash(path, data_home)
}

#[cfg(unix)]
fn sent_to_the_hand_written_trash(path: &Path, data_home: Option<&Path>) -> Removal {
    use std::time::SystemTime;

    use crate::{TrashOp, trash};

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

#[cfg(windows)]
fn sent_to_the_hand_written_trash(path: &Path, _: Option<&Path>) -> Removal {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Removal::AlreadyGone,
        _ => {
            tracing::warn!(path = %path.display(), "a file asked to be deleted has no trash to go to");
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

pub(crate) struct Marked {
    by_file: BTreeMap<PathBuf, Vec<String>>,
    already: AHashSet<String>,
    told_everywhere: BTreeMap<PathBuf, Vec<Recording>>,
}

pub(crate) fn note_before_removing(
    tx: &Transaction<'_>,
    files: &BTreeSet<PathBuf>,
    why: Why,
) -> Result<Marked> {
    let mut marked = Marked {
        by_file: BTreeMap::new(),
        already: AHashSet::new(),
        told_everywhere: BTreeMap::new(),
    };

    for path in files {
        if why == Why::DeletedEverywhere {
            marked.told_everywhere.insert(
                path.clone(),
                synced::recordings_at(tx, &store::path_text(path))?,
            );
        }

        let keys = synced::keys_deleted_with(tx, &store::path_text(path))?;

        for key in &keys {
            if !marked.by_file.values().any(|held| held.contains(key))
                && synced::is_marked_deleted(tx, key)?
            {
                marked.already.insert(key.clone());
            }
            synced::mark_deleted(tx, key)?;
        }
        marked.by_file.insert(path.clone(), keys);
    }

    Ok(marked)
}

impl Marked {
    pub(crate) fn tell_every_device(&self, tx: &Transaction<'_>, gone: &[PathBuf]) -> Result<()> {
        for recording in gone
            .iter()
            .filter_map(|path| self.told_everywhere.get(path))
            .flatten()
        {
            everywhere::to_be_told(tx, recording)?;
        }
        Ok(())
    }

    pub(crate) fn take_back_for(&self, tx: &Transaction<'_>, kept: &[PathBuf]) -> Result<()> {
        let standing: AHashSet<&String> = self
            .by_file
            .iter()
            .filter(|(path, _)| !kept.contains(path))
            .flat_map(|(_, keys)| keys)
            .collect();

        for key in kept
            .iter()
            .filter_map(|path| self.by_file.get(path))
            .flatten()
        {
            if !self.already.contains(key) && !standing.contains(key) {
                synced::unmark_deleted(tx, key)?;
            }
        }

        Ok(())
    }
}

pub(crate) fn forget_a_root(tx: &Transaction<'_>, root: i64) -> Result<()> {
    tx.execute(VAULTED_ROWS_OF_A_ROOT_LEFT_ROOTLESS, params![root])
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    forget_rows(tx, Forgetting::Folder, &[Rows::UnvaultedOfRoot(root)])?;
    tx.execute("DELETE FROM roots WHERE id = ?1", params![root])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(())
}

pub(crate) fn forget_files(tx: &Transaction<'_>, files: &[PathBuf]) -> Result<Forgotten> {
    let mut vault_keys = AHashSet::new();
    let named: Vec<String> = files
        .iter()
        .map(|path| store::path_text(path).into_owned())
        .collect();

    for path in &named {
        let held = tx
            .prepare_cached(
                "SELECT DISTINCT vault_key FROM tracks WHERE path = ?1 AND vault_key IS NOT NULL",
            )
            .and_then(|mut keys| {
                keys.query_map(params![path], |row| row.get::<_, String>(0))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        vault_keys.extend(held);
    }

    let rows: Vec<Rows<'_>> = named.iter().map(|path| Rows::AtPath(path)).collect();
    let tracks = forget_rows(tx, Forgetting::Deleted, &rows)?;
    Ok(Forgotten { tracks, vault_keys })
}
