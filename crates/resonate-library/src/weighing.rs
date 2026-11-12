use std::path::{Path, PathBuf};

use resonate_vault::{Form, Reframing, Vault};

use crate::{Error, Library, Released, Result};

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
    pub fn weigh_the_vault(&self, mending: bool) -> Result<VaultWeighing> {
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

        let mut moved_keys = Vec::new();
        for object in &objects {
            let reads_back = match vault.verify(&object.path, object.form) {
                Ok(reads_back) => reads_back,
                Err(error) if vault.failed_itself(&error) => {
                    weighed.unreached.push(VaultUnreached {
                        path: object.path.clone(),
                        error,
                    });
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
        }

        let mut covers_moved_keys = Vec::new();
        for cover in &covers {
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
        }

        if !mending {
            return Ok(weighed);
        }

        if !weighed.unframed.is_empty() {
            weighed.reframed = Some(reframe(vault, &weighed.unframed)?);
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

fn reframe(vault: &Vault, unframed: &[PathBuf]) -> Result<VaultReframed> {
    let mut reframed = VaultReframed::default();
    for path in unframed {
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
