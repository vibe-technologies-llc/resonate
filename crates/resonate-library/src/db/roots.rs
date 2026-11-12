use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use ahash::AHashSet;
use resonate_core::{TrackId, naming};
use rusqlite::{OptionalExtension as _, params};

use super::{
    Library,
    statements::{ASK_AGAIN_FOR_WHAT_A_FORGOTTEN_DELIVERY_HELD, REMEMBER_THE_DELIVERY_FORGOTTEN},
};
use crate::{
    Error, Result, StoreOp, alternatives,
    copies::{self, Copies, IdenticalCopy},
    deleted::{self, Deleted, Removal, Why},
    organise::{self},
    sheet, store,
};

impl Library {
    pub fn roots(&self) -> Result<Vec<PathBuf>> {
        self.inner.read(|connection| {
            let mut statement = connection
                .prepare("SELECT path FROM roots ORDER BY path")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let found = statement
                .query_map([], |row| row.get::<_, String>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            Ok(found.iter().map(|path| store::path_of(path)).collect())
        })
    }

    pub fn add_root(&self, path: &Path) -> Result<()> {
        if !path.is_dir() {
            return Err(Error::RootNotADirectory {
                path: path.to_path_buf(),
            });
        }
        let canonical = naming::composed(path.canonicalize().map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?);
        let _walking = self.inner.walk_the_tree()?;

        self.inner
            .write(|transaction| store::register_root(transaction, &canonical).map(drop))
    }

    pub fn remove_root(&self, path: &Path) -> Result<bool> {
        let canonical =
            naming::composed(path.canonicalize().unwrap_or_else(|_| sheet::settled(path)));
        let _walking = self.inner.walk_the_tree()?;

        self.inner.write(|transaction| {
            let Some(id) = transaction
                .query_row(
                    "SELECT id FROM roots WHERE path = ?1",
                    params![store::path_text(&canonical)],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(false);
            };

            deleted::forget_a_root(transaction, id)?;
            Ok(true)
        })
    }

    pub fn forget_delivered(&self, path: &Path) -> Result<bool> {
        let forgotten_at = store::to_nanos(SystemTime::now());

        self.inner.write(|transaction| {
            let path = store::path_text(path);
            transaction
                .execute(REMEMBER_THE_DELIVERY_FORGOTTEN, params![path, forgotten_at])
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            transaction
                .execute(ASK_AGAIN_FOR_WHAT_A_FORGOTTEN_DELIVERY_HELD, params![path])
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            let forgotten = transaction
                .execute(
                    "DELETE FROM tracks WHERE root_id IS NULL AND path = ?1",
                    params![path],
                )
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
            if forgotten > 0 {
                store::sweep_orphans(transaction)?;
                alternatives::settle_if_owed(transaction)?;
            }
            Ok(forgotten > 0)
        })
    }

    pub(super) fn prune_the_emptied(&self, folders: &[PathBuf]) -> Result<()> {
        let roots: AHashSet<PathBuf> = self.roots()?.into_iter().collect();
        organise::prune_the_emptied(folders, &roots);
        Ok(())
    }

    pub fn identical_copies(&self) -> Result<Copies> {
        self.inner.read(copies::found)
    }

    pub fn fold_copies(&self, numbered: &[IdenticalCopy]) -> Result<Deleted> {
        if numbered.is_empty() {
            return Ok(Deleted::default());
        }
        self.inner.write(|transaction| {
            for copy in numbered {
                copies::handed_over(transaction, copy)?;
            }
            Ok(())
        })?;
        self.inner.playlists_changed();
        let copies: Vec<TrackId> = numbered.iter().map(IdenticalCopy::copy_id).collect();
        self.remove_tracks(&copies, Why::Folded)
    }

    pub fn delete_tracks(&self, tracks: &[TrackId]) -> Result<Deleted> {
        self.remove_tracks(tracks, Why::Deleted)
    }

    fn remove_tracks(&self, tracks: &[TrackId], why: Why) -> Result<Deleted> {
        let _walk = self.walk_the_tree()?;
        let files = self
            .inner
            .read(|connection| deleted::files_of(connection, tracks))?;

        if why == Why::Deleted {
            self.inner
                .write(|transaction| deleted::note_before_removing(transaction, &files))?;
        }

        let data_home = self.inner.trash_home.lock().clone();
        let through = self.inner.trashes.lock().clone();
        let through = through.as_deref();
        let mut done = Deleted::default();
        let mut gone = Vec::with_capacity(files.len());
        let mut emptied = Vec::new();
        for path in files {
            let sidecars = deleted::sidecars_of(&path);
            match deleted::removed(&path, data_home.as_deref(), through) {
                Removal::Removed => {
                    done.files += 1;
                    for sidecar in &sidecars {
                        deleted::removed(sidecar, data_home.as_deref(), through);
                    }
                    emptied.extend(path.parent().map(Path::to_path_buf));
                    gone.push(path);
                }
                Removal::AlreadyGone => gone.push(path),
                Removal::Kept => done.kept += 1,
            }
        }
        self.prune_the_emptied(&emptied)?;

        let forgotten = self
            .inner
            .write(|transaction| deleted::forget_files(transaction, &gone))?;
        done.tracks = forgotten.tracks;
        if let Some(vault) = self.vault().cloned() {
            for object in self
                .vault_objects_nothing_names()?
                .into_iter()
                .filter(|object| forgotten.vault_keys.contains(&object.key.to_string()))
            {
                match vault.forget(&object.path) {
                    Ok(_) => {
                        self.forget_vault_object(&object.key)?;
                    }
                    Err(error) => {
                        tracing::warn!(path = %object.path.display(), %error, "a deleted track's vault object stays for the next prune");
                    }
                }
            }
        }
        Ok(done)
    }
}
