use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use ahash::AHashSet;
use resonate_core::{Recording, TrackId, naming};
use rusqlite::{OptionalExtension as _, params};

use super::{
    Library, Walk,
    statements::{ASK_AGAIN_FOR_WHAT_A_FORGOTTEN_DELIVERY_HELD, REMEMBER_THE_DELIVERY_FORGOTTEN},
};
use crate::{
    Error, Result, StoreOp,
    copies::{self, Copies, IdenticalCopy, Rechecked},
    deleted::{self, Deleted, Found, Removal, Why},
    everywhere::{self, DeletedEverywhere, Naming},
    forgetting::{Forgetting, Rows, forget_rows},
    organise::{self},
    sheet, store,
    volumes::{self, OutOfReach},
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

            let forgotten = forget_rows(
                transaction,
                Forgetting::Delivery,
                &[Rows::RootlessAtPath(&path)],
            )?;
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

        let walk = self.walk_the_tree()?;
        let Rechecked { standing, changed } = copies::still_identical(&self.inner, numbered)?;

        if !standing.is_empty() {
            self.inner.write(|transaction| {
                for copy in &standing {
                    copies::handed_over(transaction, &copy.hand_over())?;
                }
                Ok(())
            })?;
            self.inner.playlists_changed();
        }

        let copies: Vec<TrackId> = standing.iter().map(|copy| copy.copy_id()).collect();
        let mut folded = match copies.is_empty() {
            true => Deleted::default(),
            false => self.remove_tracks_under(&walk, &copies, Why::Folded)?,
        };

        folded.changed_since_found = changed.into_iter().map(|copy| copy.copy.clone()).collect();
        Ok(folded)
    }

    pub fn delete_tracks(&self, tracks: &[TrackId]) -> Result<Deleted> {
        self.remove_tracks(tracks, Why::Deleted)
    }

    pub fn delete_tracks_everywhere(&self, tracks: &[TrackId]) -> Result<Deleted> {
        self.remove_tracks(tracks, Why::DeletedEverywhere)
    }

    pub fn take_deletion(&self, recording: &Recording, before: i64) -> Result<Deleted> {
        let older: Vec<TrackId> = everywhere::copies_of(&self.inner, recording, Naming::Surely)?
            .into_iter()
            .filter(|copy| copy.whole_file && copy.added < before)
            .map(|copy| copy.track)
            .collect();

        if older.is_empty() {
            return Ok(Deleted::default());
        }
        self.delete_tracks(&older)
    }

    pub fn added_by_hand_since(&self, recording: &Recording, since: i64) -> Result<bool> {
        Ok(
            everywhere::copies_of(&self.inner, recording, Naming::Alike)?
                .iter()
                .any(|copy| copy.added > since && !copy.received),
        )
    }

    pub fn deleted_everywhere_to_tell(&self) -> Result<Vec<DeletedEverywhere>> {
        everywhere::waiting_to_be_told(&self.inner)
    }

    pub fn deletions_told(&self, ids: &[i64]) -> Result<()> {
        everywhere::told(&self.inner, ids)
    }

    fn remove_tracks(&self, tracks: &[TrackId], why: Why) -> Result<Deleted> {
        let walk = self.walk_the_tree()?;
        self.remove_tracks_under(&walk, tracks, why)
    }

    pub(super) fn remove_tracks_under(
        &self,
        walk: &Walk,
        tracks: &[TrackId],
        why: Why,
    ) -> Result<Deleted> {
        self.settle_removals_under_way(walk)?;

        let files = self
            .inner
            .read(|connection| deleted::files_of(connection, tracks))?;

        self.inner
            .write(|transaction| deleted::note_before_removing(transaction, &files, why))?;

        let data_home = self.inner.trash_home.lock().clone();
        let through = self.inner.trashes.lock().clone();
        let through = through.as_deref();
        let mut out_of_reach = None;
        let mut done = Deleted::default();
        let mut gone = Vec::with_capacity(files.len());
        let mut emptied = Vec::new();
        let mut kept = Vec::new();

        for path in files {
            let sidecars = deleted::sidecars_of(&path);
            match deleted::removed(&path, data_home.as_deref(), through) {
                Removal::Removed => {
                    done.files += 1;
                    done.sidecars_kept
                        .extend(sidecars.into_iter().filter(|sidecar| {
                            deleted::removed(sidecar, data_home.as_deref(), through)
                                == Removal::Kept
                        }));
                    emptied.extend(path.parent().map(Path::to_path_buf));
                    gone.push(path);
                }
                Removal::AlreadyGone => {
                    if out_of_reach.is_none() {
                        out_of_reach = Some(self.out_of_reach()?);
                    }
                    if out_of_reach
                        .as_mut()
                        .is_some_and(|reach| reach.has_gone(&path))
                    {
                        gone.push(path);
                    } else {
                        tracing::warn!(path = %path.display(), "a file asked to be deleted is out of reach, so its rows stay");
                        done.kept += 1;
                        kept.push(path);
                    }
                }
                Removal::Kept => {
                    done.kept += 1;
                    kept.push(path);
                }
            }
        }
        self.prune_the_emptied(&emptied)?;

        done.tracks = self.settle_removals(&gone, &kept)?;
        Ok(done)
    }

    pub(super) fn settle_removals_left_under_way(&self) -> Result<()> {
        if !self.inner.read(deleted::any_under_way)? {
            return Ok(());
        }
        match self.walk_the_tree() {
            Ok(walk) => self.settle_removals_under_way(&walk),
            Err(Error::AlreadyWalking) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn settle_removals_under_way(&self, _walk: &Walk) -> Result<()> {
        let under_way = self.inner.read(deleted::under_way)?;

        if under_way.is_empty() {
            return Ok(());
        }

        let mut out_of_reach = self.out_of_reach()?;
        let mut gone = Vec::new();
        let mut kept = Vec::new();

        for path in under_way {
            match deleted::found_at(&path, &mut out_of_reach) {
                Found::Gone => gone.push(path),
                Found::Stands => kept.push(path),
                Found::OutOfReach => {
                    tracing::info!(path = %path.display(), "a removal left under way waits for its file to be in reach again");
                }
            }
        }
        tracing::info!(
            gone = gone.len(),
            kept = kept.len(),
            "settling the removals a run left under way"
        );
        self.settle_removals(&gone, &kept).map(drop)
    }

    fn out_of_reach(&self) -> Result<OutOfReach> {
        let held = self.inner.read(volumes::held)?;

        Ok(OutOfReach::of(held, self.roots()?))
    }

    fn settle_removals(&self, gone: &[PathBuf], kept: &[PathBuf]) -> Result<u64> {
        if gone.is_empty() && kept.is_empty() {
            return Ok(0);
        }

        let forgotten = self
            .inner
            .write(|transaction| deleted::settle(transaction, gone, kept))?;

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
        Ok(forgotten.tracks)
    }
}
