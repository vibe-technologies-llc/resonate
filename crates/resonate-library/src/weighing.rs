use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use resonate_vault::{Form, Reframing, Vault};

use crate::{
    Error, Library, Released, Result,
    pass::{Cancelling, PassHandle, PassKind, VaultWeighingHandle},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VaultWeighingStats {
    pub to_weigh: u64,
    pub weighed: u64,
}

#[derive(Debug, Default)]
pub struct VaultWeighingProgress {
    to_weigh: AtomicU64,
    weighed: AtomicU64,
    cancelled: AtomicBool,
}

impl VaultWeighingProgress {
    pub fn snapshot(&self) -> VaultWeighingStats {
        VaultWeighingStats {
            to_weigh: self.to_weigh.load(Ordering::Relaxed),
            weighed: self.weighed.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn counted(&self, to_weigh: u64) {
        self.to_weigh.store(to_weigh, Ordering::Relaxed);
    }

    fn weighed_one(&self) {
        self.weighed.fetch_add(1, Ordering::Relaxed);
    }
}

impl Cancelling for VaultWeighingProgress {
    fn cancel(&self) {
        VaultWeighingProgress::cancel(self);
    }
}

#[derive(Debug)]
pub struct VaultUnreached {
    pub path: PathBuf,
    pub error: resonate_vault::Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VaultReframed {
    pub repacked: u64,
    pub not_held: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VaultMended {
    pub released: Released,
    pub covers_let_go: u64,
}

#[derive(Debug, Default)]
pub struct VaultWeighing {
    pub objects: u64,
    pub objects_held: u64,
    pub objects_moved: Vec<PathBuf>,
    pub covers: u64,
    pub covers_held: u64,
    pub covers_moved: Vec<PathBuf>,
    pub unreached: Vec<VaultUnreached>,
    pub unframed: Vec<PathBuf>,
    pub reframed: Option<VaultReframed>,
    pub mended: Option<VaultMended>,
    pub cancelled: bool,
}

impl VaultWeighing {
    pub fn held_nothing(&self) -> bool {
        self.objects == 0 && self.covers == 0
    }

    pub fn failing(&self) -> usize {
        self.objects_moved.len() + self.covers_moved.len()
    }

    pub fn has_something_to_mend(&self) -> bool {
        self.failing() > 0 || !self.unframed.is_empty()
    }
}

impl Library {
    pub fn weigh_the_vault(&self, mending: bool) -> Result<VaultWeighingHandle> {
        if self.vault().is_none() {
            return Err(Error::NoVault);
        }

        let library = self.shared();
        let progress = Arc::new(VaultWeighingProgress::default());
        let owned = Arc::clone(&progress);
        let thread = thread::Builder::new()
            .name("resonate-weigh".to_owned())
            .spawn(move || library.weigh(mending, &progress))
            .map_err(|source| Error::ThreadSpawn { source })?;

        Ok(PassHandle::of(PassKind::Weigh, owned, thread))
    }

    fn weigh(&self, mending: bool, progress: &VaultWeighingProgress) -> Result<VaultWeighing> {
        let vault = self.vault().ok_or(Error::NoVault)?;
        let refused = |source| Error::Vault {
            path: vault.root().to_path_buf(),
            source: Box::new(source),
        };

        let objects = self.vault_objects()?;
        let covers = vault.covers().map_err(refused)?;
        let mut weighed = VaultWeighing {
            objects: objects.len() as u64,
            covers: covers.len() as u64,
            ..VaultWeighing::default()
        };
        if weighed.held_nothing() {
            return Ok(weighed);
        }
        progress.counted(weighed.objects + weighed.covers);

        let mut moved_keys = Vec::new();
        for object in &objects {
            if progress.is_cancelled() {
                weighed.cancelled = true;
                return Ok(weighed);
            }
            let reads_back = match vault.verify(&object.path, object.form) {
                Ok(reads_back) => reads_back,
                Err(error) if vault.failed_itself(&error) => {
                    weighed.unreached.push(VaultUnreached {
                        path: object.path.clone(),
                        error,
                    });
                    progress.weighed_one();
                    continue;
                }
                Err(error) => {
                    tracing::debug!(%error, path = %object.path.display(), "a vault object failed as it was read back");
                    false
                }
            };
            self.note_validated(&object.key, reads_back)?;
            if reads_back {
                weighed.objects_held += 1;
                if packed_before_the_frames(vault, object.form, &object.path) {
                    weighed.unframed.push(object.path.clone());
                }
            } else {
                moved_keys.push(object.key);
                weighed.objects_moved.push(object.path.clone());
            }
            progress.weighed_one();
        }

        let mut covers_moved_keys = Vec::new();
        for cover in &covers {
            if progress.is_cancelled() {
                weighed.cancelled = true;
                return Ok(weighed);
            }
            match vault.verify_cover(&cover.path) {
                Ok(true) => weighed.covers_held += 1,
                Ok(false) => {
                    covers_moved_keys.push(cover.key);
                    weighed.covers_moved.push(cover.path.clone());
                }
                Err(error) if vault.failed_itself(&error) => {
                    weighed.unreached.push(VaultUnreached {
                        path: cover.path.clone(),
                        error,
                    });
                }
                Err(error) => {
                    tracing::debug!(%error, path = %cover.path.display(), "a vault cover failed as it was read back");
                    covers_moved_keys.push(cover.key);
                    weighed.covers_moved.push(cover.path.clone());
                }
            }
            progress.weighed_one();
        }

        if !mending {
            return Ok(weighed);
        }

        if !weighed.unframed.is_empty() {
            weighed.reframed = Some(reframe(vault, &weighed.unframed, progress)?);
            if progress.is_cancelled() {
                weighed.cancelled = true;
                return Ok(weighed);
            }
        }
        if weighed.failing() > 0 {
            weighed.mended = Some(VaultMended {
                released: self.release_the_rows_of(&moved_keys)?,
                covers_let_go: self.let_go_of_vault_covers(&covers_moved_keys)?,
            });
        }
        Ok(weighed)
    }
}

fn packed_before_the_frames(vault: &Vault, form: Form, path: &Path) -> bool {
    form == Form::Wave
        && vault
            .packed_before_the_frames(path)
            .inspect_err(|error| {
                tracing::debug!(
                    %error,
                    path = %path.display(),
                    "a vault object's frames could not be walked"
                );
            })
            .unwrap_or(false)
}

fn reframe(
    vault: &Vault,
    unframed: &[PathBuf],
    progress: &VaultWeighingProgress,
) -> Result<VaultReframed> {
    let mut reframed = VaultReframed::default();
    for path in unframed {
        if progress.is_cancelled() {
            break;
        }
        let outcome = vault.reframe(path).map_err(|source| Error::Vault {
            path: path.clone(),
            source: Box::new(source),
        })?;
        match outcome {
            Reframing::Reframed => reframed.repacked += 1,
            Reframing::AlreadyFramed => {}
            Reframing::NotHeld => reframed.not_held.push(path.clone()),
        }
    }
    Ok(reframed)
}
