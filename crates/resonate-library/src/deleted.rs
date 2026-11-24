use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ahash::{AHashMap, AHashSet};
use resonate_codec::the_folder_a_cue_names;
use resonate_core::TrackId;
use rusqlite::{Connection, OptionalExtension as _, Transaction, params, params_from_iter};

use crate::{
    Error, Result, StoreOp, everywhere,
    forgetting::{Forgetting, Rows, forget_rows},
    scan, store, synced,
    volumes::OutOfReach,
};

const LYRICS_EXTENSION: &str = "lrc";

const PATHS_ASKED_AT_ONCE: usize = 256;

const VAULTED_ROWS_OF_A_ROOT_LEFT_ROOTLESS: &str =
    "UPDATE tracks SET root_id = NULL WHERE root_id = ?1 AND vault_key IS NOT NULL";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    pub tracks: u64,
    pub files: u64,
    pub kept: u64,
    pub sidecars_kept: Vec<PathBuf>,
    pub changed_since_found: Vec<PathBuf>,
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

impl Why {
    const fn stored(self) -> i64 {
        match self {
            Self::Folded => FOLDED,
            Self::Deleted => DELETED_HERE,
            Self::DeletedEverywhere => DELETED_EVERYWHERE,
        }
    }

    const fn marks(self) -> bool {
        !matches!(self, Self::Folded)
    }
}

const FOLDED: i64 = 0;
const DELETED_HERE: i64 = 1;
const DELETED_EVERYWHERE: i64 = 2;

const NOTE_A_REMOVAL_UNDER_WAY: &str = "INSERT INTO removals_pending (path, why) VALUES (?1, ?2)
     ON CONFLICT (path) DO UPDATE SET why = max(why, excluded.why)";

const NOTE_A_MARK_A_REMOVAL_MADE: &str =
    "INSERT OR IGNORE INTO removal_marks (path, recording, anew) VALUES (?1, ?2, ?3)";

const MARKS_A_REMOVAL_MADE_ANEW: &str =
    "SELECT recording FROM removal_marks WHERE path = ?1 AND anew = 1";

const A_MARK_ANOTHER_REMOVAL_HOLDS: &str =
    "SELECT EXISTS (SELECT 1 FROM removal_marks WHERE recording = ?1)";

const A_KEY_A_REMOVED_FILE_HELD_IS_NO_LONGER_ANEW: &str = "UPDATE removal_marks SET anew = 0
  WHERE path <> ?1 AND recording IN (SELECT recording FROM removal_marks WHERE path = ?1)";

const WHY_A_REMOVAL_WAS_ASKED: &str = "SELECT why FROM removals_pending WHERE path = ?1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Found {
    Stands,
    Gone,
    OutOfReach,
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

#[derive(Debug, Default)]
pub(crate) struct Run {
    #[cfg(unix)]
    suffixes: crate::trash::Suffixes,
}

pub(crate) fn removed(
    path: &Path,
    data_home: Option<&Path>,
    through: Option<&dyn Trashes>,
    run: &mut Run,
) -> Removal {
    if let Some(through) = through {
        return through.trash(path);
    }

    sent_to_the_hand_written_trash(path, data_home, run)
}

#[cfg(unix)]
fn sent_to_the_hand_written_trash(path: &Path, data_home: Option<&Path>, run: &mut Run) -> Removal {
    use std::time::SystemTime;

    use crate::{TrashOp, trash};

    match trash::send(path, data_home, SystemTime::now(), &mut run.suffixes) {
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
fn sent_to_the_hand_written_trash(path: &Path, _: Option<&Path>, _: &mut Run) -> Removal {
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

pub(crate) fn note_before_removing(
    tx: &Transaction<'_>,
    files: &BTreeSet<PathBuf>,
    why: Why,
) -> Result<()> {
    for path in files {
        let text = store::path_text(path);

        store::cached(tx, NOTE_A_REMOVAL_UNDER_WAY, params![text, why.stored()])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        if !why.marks() {
            continue;
        }
        for key in synced::keys_deleted_with(tx, &text)? {
            let anew = !synced::is_marked_deleted(tx, &key)?;

            store::cached(tx, NOTE_A_MARK_A_REMOVAL_MADE, params![text, key, anew])
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            synced::mark_deleted(tx, &key)?;
        }
    }
    Ok(())
}

pub(crate) fn under_way(connection: &Connection) -> Result<Vec<PathBuf>> {
    connection
        .prepare_cached("SELECT path FROM removals_pending ORDER BY path")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        })
        .map(|paths| paths.iter().map(|path| store::path_of(path)).collect())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn any_under_way(connection: &Connection) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM removals_pending)",
            [],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn found_at(path: &Path, out_of_reach: &mut OutOfReach) -> Found {
    if fs::symlink_metadata(path).is_ok() {
        return Found::Stands;
    }
    if out_of_reach.has_gone(path) {
        return Found::Gone;
    }
    Found::OutOfReach
}

pub(crate) fn settle(
    tx: &Transaction<'_>,
    gone: &[PathBuf],
    kept: &[PathBuf],
) -> Result<Forgotten> {
    take_back_for(tx, kept)?;
    tell_every_device(tx, gone)?;

    let forgotten = forget_files(tx, gone)?;

    for path in gone {
        let text = store::path_text(path);

        store::cached(
            tx,
            A_KEY_A_REMOVED_FILE_HELD_IS_NO_LONGER_ANEW,
            params![text],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
        no_longer_under_way(tx, &text)?;
    }
    Ok(forgotten)
}

fn take_back_for(tx: &Transaction<'_>, kept: &[PathBuf]) -> Result<()> {
    let mut made_anew = Vec::new();

    for path in kept {
        let text = store::path_text(path);
        let marks = tx
            .prepare_cached(MARKS_A_REMOVAL_MADE_ANEW)
            .and_then(|mut statement| {
                statement
                    .query_map(params![text], |row| row.get::<_, String>(0))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        made_anew.extend(marks);
        no_longer_under_way(tx, &text)?;
    }
    for key in made_anew {
        let held_by_another: bool =
            store::queried(tx, A_MARK_ANOTHER_REMOVAL_HOLDS, params![key], |row| {
                row.get(0)
            })
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        if !held_by_another {
            synced::unmark_deleted(tx, &key)?;
        }
    }
    Ok(())
}

fn tell_every_device(tx: &Transaction<'_>, gone: &[PathBuf]) -> Result<()> {
    for path in gone {
        let text = store::path_text(path);
        let why: Option<i64> = tx
            .prepare_cached(WHY_A_REMOVAL_WAS_ASKED)
            .and_then(|mut statement| {
                statement
                    .query_row(params![text], |row| row.get(0))
                    .optional()
            })
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        if why != Some(DELETED_EVERYWHERE) {
            continue;
        }
        for recording in synced::recordings_at(tx, &text)? {
            everywhere::to_be_told(tx, &recording)?;
        }
    }
    Ok(())
}

fn no_longer_under_way(tx: &Transaction<'_>, path: &str) -> Result<()> {
    for sql in [
        "DELETE FROM removal_marks WHERE path = ?1",
        "DELETE FROM removals_pending WHERE path = ?1",
    ] {
        store::cached(tx, sql, params![path])
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }
    Ok(())
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
