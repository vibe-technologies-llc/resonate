use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{
    naming,
    writer::{self, Held, Writer},
};

use crate::{
    Error, Result,
    documents::{DOCUMENT_NOT_FOUND, owned},
    error::RewriteOp,
};

const STAGING: &str = "tag-staging";
const UNRESTORED: &str = "tag-unrestored";
const HELD: &str = "held";
const ORIGINAL: &str = "original";
const WORKING: &str = "working";
const UNNAMED_SONG: &str = "song";
const READ_IN_CHUNKS_OF: usize = 1 << 16;

static STAGED: AtomicU64 = AtomicU64::new(0);

#[uniffi::export(callback_interface)]
pub trait DocumentRewriting: Send + Sync {
    fn rewrite_document(&self, uri: String) -> i32;
}

pub(crate) struct Stage {
    folder: PathBuf,
    leaf: OsString,
    _held: Held,
}

impl Stage {
    pub(crate) fn made(under: &Path, name: &str) -> Result<Self> {
        let staging = under.join(STAGING);
        let refused = |source| Error::TagsDocument {
            op: RewriteOp::CopiedAside,
            source,
        };
        fs::create_dir_all(&staging).map_err(refused)?;
        sweep(&staging);

        let folder = staging.join(format!(
            "{}-{}",
            process::id(),
            STAGED.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&folder).map_err(refused)?;
        let held = match Held::made(&folder.join(HELD)) {
            Ok(held) => held,
            Err(source) => {
                let _ = fs::remove_dir(&folder);
                return Err(refused(source));
            }
        };
        let stage = Self {
            folder,
            leaf: leaf_of(name),
            _held: held,
        };
        fs::create_dir(stage.folder.join(ORIGINAL)).map_err(refused)?;
        fs::create_dir(stage.folder.join(WORKING)).map_err(refused)?;
        Ok(stage)
    }

    pub(crate) fn original(&self) -> PathBuf {
        self.folder.join(ORIGINAL).join(&self.leaf)
    }

    pub(crate) fn working(&self) -> PathBuf {
        self.folder.join(WORKING).join(&self.leaf)
    }

    pub(crate) fn keep_original(&self, under: &Path) -> Result<PathBuf> {
        let unrestored = under.join(UNRESTORED);
        let refused = |source| Error::TagsDocument {
            op: RewriteOp::PutBack,
            source,
        };
        fs::create_dir_all(&unrestored).map_err(refused)?;
        let kept = unrestored.join(naming::named_within(
            "",
            self.leaf.as_os_str(),
            format!(".{}", self.folder_name()),
        ));
        fs::rename(self.original(), &kept).map_err(refused)?;
        Ok(kept)
    }

    fn folder_name(&self) -> String {
        self.folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.folder) {
            tracing::debug!(%error, folder = %self.folder.display(), "a staged song could not be removed");
        }
    }
}

fn leaf_of(name: &str) -> OsString {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| OsStr::new(UNNAMED_SONG));
    let extension = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    naming::named_within("", stem, extension)
}

fn sweep(staging: &Path) {
    let Ok(entries) = fs::read_dir(staging) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(stamped) = name
            .to_str()
            .and_then(|name| name.split_once('-'))
            .and_then(|(stamp, _)| stamp.parse::<u32>().ok())
        else {
            continue;
        };
        let folder = entry.path();
        if writer::writer_of(&folder.join(HELD), stamped) == Writer::Living {
            continue;
        }
        match fs::remove_dir_all(&folder) {
            Ok(()) => {
                tracing::info!(folder = %folder.display(), "swept a staged song a tag edit left behind")
            }
            Err(error) => {
                tracing::debug!(%error, folder = %folder.display(), "a staged song a tag edit left behind could not be swept")
            }
        }
    }
}

pub(crate) fn put_back(rewriting: &dyn DocumentRewriting, uri: &str, from: &Path) -> Result<()> {
    let descriptor = rewriting.rewrite_document(uri.to_owned());
    if descriptor < 0 {
        let kind = match descriptor {
            DOCUMENT_NOT_FOUND => io::ErrorKind::NotFound,
            _ => io::ErrorKind::PermissionDenied,
        };
        return Err(Error::TagsDocument {
            op: RewriteOp::Opened,
            source: kind.into(),
        });
    }

    let mut document = owned(descriptor);
    let failed = |source| Error::TagsDocument {
        op: RewriteOp::Written,
        source,
    };
    let mut source = File::open(from).map_err(failed)?;
    io::copy(&mut source, &mut document).map_err(failed)?;
    if let Err(error) = document.sync_all() {
        tracing::debug!(%error, "a rewritten song could not be flushed to its provider");
    }
    Ok(())
}

pub(crate) fn crc_of(stream: &mut impl Read) -> io::Result<u32> {
    let mut crc = crc32fast::Hasher::new();
    let mut chunk = vec![0; READ_IN_CHUNKS_OF];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(crc.finalize()),
            Ok(read) => crc.update(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn crc_of_file(path: &Path) -> Result<u32> {
    File::open(path)
        .and_then(|mut file| crc_of(&mut file))
        .map_err(|source| Error::TagsDocument {
            op: RewriteOp::ReadBack,
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    const NEVER_A_PROCESS: u32 = 999_999_999;

    fn under(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-rewriting-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_stage_a_dead_writer_left_is_swept_and_a_living_one_is_not() {
        let data = under("sweep");
        let dead = data.join(STAGING).join(format!("{NEVER_A_PROCESS}-0"));
        fs::create_dir_all(dead.join(ORIGINAL)).expect("a dead stage");
        fs::write(dead.join(HELD), b"").expect("its held file");
        fs::write(dead.join(ORIGINAL).join("song.flac"), b"bytes").expect("its copy");

        let living = Stage::made(&data, "song.flac").expect("a stage");
        let beside = Stage::made(&data, "other.flac").expect("a second stage");

        assert!(!dead.exists());
        assert!(living.folder.exists());
        assert!(beside.folder.exists());
        drop((living, beside));
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_stage_names_its_files_as_the_song_is_named_and_leaves_nothing_when_dropped() {
        let data = under("names");

        let stage = Stage::made(&data, "Echoes.FLAC").expect("a stage");
        let folder = stage.folder.clone();

        assert_eq!(stage.working().file_name(), Some(OsStr::new("Echoes.FLAC")));
        assert_ne!(stage.working(), stage.original());
        drop(stage);
        assert!(!folder.exists());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_name_at_the_limit_is_cut_on_a_letter_and_keeps_its_extension() {
        let long = format!("{}.flac", "é".repeat(200));

        let leaf = leaf_of(&long);

        assert!(leaf.len() <= naming::NAME_BYTES_AT_MOST);
        assert!(leaf.to_string_lossy().ends_with(".flac"));
        assert_eq!(leaf_of(""), OsString::from("song"));
    }
}
