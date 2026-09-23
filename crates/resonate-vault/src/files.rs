use std::{fs::File, path::PathBuf};

use resonate_codec::{
    Error as CodecError, FormatHint, Media, MediaProvider, Reading, Result as CodecResult,
};
use resonate_core::{MediaLocation, SourceId};

use crate::{
    unpacking::Unpacking,
    vault::{COMPRESSED_EXTENSION, Vault},
};

pub struct VaultFiles {
    source: SourceId,
    root: PathBuf,
}

impl VaultFiles {
    pub fn over(vault: &Vault) -> Self {
        Self {
            source: SourceId::local(),
            root: vault.root().to_path_buf(),
        }
    }
}

impl MediaProvider for VaultFiles {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> CodecResult<Media> {
        let path = location
            .as_path()
            .ok_or_else(|| CodecError::LocatorNotUsable {
                location: location.clone(),
            })?;

        let packed =
            path.starts_with(&self.root) && location.extension() == Some(COMPRESSED_EXTENSION);

        if packed {
            let unpacking = Unpacking::open(path).map_err(|source| CodecError::Io {
                location: location.clone(),
                source,
            })?;
            return Ok(Media {
                stream: Box::new(Reading::new(unpacking)),
                hint: inner_hint(location),
            });
        }

        let file = File::open(path).map_err(|source| CodecError::Io {
            location: location.clone(),
            source,
        })?;
        Ok(Media {
            stream: Box::new(Reading::new(file)),
            hint: location
                .extension()
                .map(|extension| FormatHint::Extension(extension.into())),
        })
    }
}

fn inner_hint(location: &MediaLocation) -> Option<FormatHint> {
    let path = location.as_path()?;
    let stem = path.file_stem()?.to_str()?;
    let inner = stem.rsplit_once('.')?.1;
    Some(FormatHint::Extension(inner.into()))
}
