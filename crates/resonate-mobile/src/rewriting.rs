use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{
    file_facts::sync_directory,
    naming,
    writer::{self, Held, Writer},
};

use crate::{
    Error, Result,
    documents::{DOCUMENT_REFUSED, owned},
    error::RewriteOp,
    foreign::{Callback, answered},
};

const STAGING: &str = "tag-staging";
const UNRESTORED: &str = "tag-unrestored";
const HELD: &str = "held";
const ORIGINAL: &str = "original";
const WORKING: &str = "working";
const PENDING: &str = "pending";
const PENDING_STAGED: &str = "pending.staged";
const PENDING_FORM: &str = "1";
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
    pending: bool,
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
            pending: false,
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

    pub(crate) fn set_aside(&self) -> Result<u32> {
        let refused = |source| Error::TagsDocument {
            op: RewriteOp::CopiedAside,
            source,
        };

        fs::copy(self.working(), self.original()).map_err(refused)?;
        File::open(self.original())
            .and_then(|original| original.sync_all())
            .and_then(|()| sync_directory(&self.folder.join(ORIGINAL)))
            .map_err(refused)?;
        crc_of_file(&self.original())
    }

    pub(crate) fn pend(&mut self, uri: &str, original: u32) -> Result<()> {
        let staged = self.folder.join(PENDING_STAGED);
        let record = format!("{PENDING_FORM}\n{original:08x}\n{uri}");

        let written = File::create_new(&staged)
            .and_then(|mut file| {
                file.write_all(record.as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&staged, self.folder.join(PENDING)))
            .and_then(|()| sync_directory(&self.folder));

        if let Err(source) = written {
            let _ = fs::remove_file(&staged);
            let _ = fs::remove_file(self.folder.join(PENDING));

            return Err(Error::TagsDocument {
                op: RewriteOp::Recorded,
                source,
            });
        }
        self.pending = true;
        Ok(())
    }

    pub(crate) fn settled(&mut self) {
        if let Err(error) = fs::remove_file(self.folder.join(PENDING)) {
            tracing::warn!(%error, folder = %self.folder.display(), "a settled rewrite's record could not be removed");
        }
        self.pending = false;
    }

    pub(crate) fn kept_aside(&mut self, under: &Path) -> PathBuf {
        let unrestored = under.join(UNRESTORED);
        let Some(name) = self.folder.file_name().map(OsStr::to_owned) else {
            return self.original();
        };

        let kept = unrestored.join(name);
        let moved = fs::create_dir_all(&unrestored)
            .and_then(|()| fs::rename(&self.folder, &kept))
            .and_then(|()| sync_directory(&unrestored));

        match moved {
            Ok(()) => {
                self.folder = kept;

                let _ = sync_directory(&under.join(STAGING));
            }
            Err(error) => {
                tracing::warn!(%error, folder = %self.folder.display(), "an unrestored original stays where it was staged");
            }
        }
        self.original()
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let removed = if self.pending {
            fs::remove_dir_all(self.folder.join(WORKING))
        } else {
            fs::remove_dir_all(&self.folder)
        };
        if let Err(error) = removed {
            tracing::debug!(%error, folder = %self.folder.display(), "a staged song could not be removed");
        }
    }
}

pub(crate) struct Pending {
    folder: PathBuf,
    pub(crate) uri: String,
    pub(crate) original_crc: u32,
    _held: Held,
}

impl Pending {
    pub(crate) fn original(&self) -> PathBuf {
        let originals = self.folder.join(ORIGINAL);

        fs::read_dir(&originals)
            .ok()
            .and_then(|mut entries| entries.find_map(|entry| entry.ok()))
            .map_or(originals, |entry| entry.path())
    }

    pub(crate) fn settled(self) {
        let removed = fs::remove_file(self.folder.join(PENDING))
            .and_then(|()| fs::remove_dir_all(&self.folder));

        match removed {
            Ok(()) => tracing::info!(uri = self.uri, "an interrupted tag rewrite was settled"),
            Err(error) => {
                tracing::warn!(%error, folder = %self.folder.display(), "a settled rewrite could not be removed")
            }
        }
    }
}

pub(crate) fn pending_under(under: &Path) -> Vec<Pending> {
    [STAGING, UNRESTORED]
        .iter()
        .filter_map(|area| fs::read_dir(under.join(area)).ok())
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|folder| folder.join(PENDING).is_file())
        .filter_map(claimed)
        .collect()
}

fn claimed(folder: PathBuf) -> Option<Pending> {
    let held = match Held::taken_over(&folder.join(HELD)) {
        Ok(held) => held,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return None,
        Err(error) => {
            tracing::warn!(%error, folder = %folder.display(), "an interrupted tag rewrite could not be claimed");
            return None;
        }
    };
    let record = fs::read_to_string(folder.join(PENDING)).ok();
    let Some((uri, original_crc)) = record.as_deref().and_then(read_record) else {
        tracing::warn!(folder = %folder.display(), "an interrupted tag rewrite's record could not be read");
        return None;
    };
    Some(Pending {
        folder,
        uri,
        original_crc,
        _held: held,
    })
}

fn read_record(record: &str) -> Option<(String, u32)> {
    let mut lines = record.splitn(3, '\n');

    if lines.next()? != PENDING_FORM {
        return None;
    }

    let original_crc = u32::from_str_radix(lines.next()?, 16).ok()?;
    let uri = lines.next().filter(|uri| !uri.is_empty())?;
    Some((uri.to_owned(), original_crc))
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

        if folder.join(PENDING).exists()
            || writer::writer_of(&folder.join(HELD), stamped) == Writer::Living
        {
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
    let failed = |source| Error::TagsDocument {
        op: RewriteOp::Written,
        source,
    };
    let mut source = File::open(from).map_err(failed)?;
    let descriptor = answered(Callback::RewriteDocument, || {
        rewriting.rewrite_document(uri.to_owned())
    })
    .unwrap_or(DOCUMENT_REFUSED);
    let mut document = owned(descriptor).map_err(|source| Error::TagsDocument {
        op: RewriteOp::Opened,
        source,
    })?;

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
    fn a_stage_a_dead_writer_left_with_a_pending_rewrite_is_never_swept_but_claimed() {
        let data = under("pending");
        let dead = data.join(STAGING).join(format!("{NEVER_A_PROCESS}-0"));
        let recordless = data.join(STAGING).join(format!("{NEVER_A_PROCESS}-1"));

        for folder in [&dead, &recordless] {
            fs::create_dir_all(folder.join(ORIGINAL)).expect("a dead stage");
            fs::write(folder.join(HELD), b"").expect("its held file");
            fs::write(folder.join(ORIGINAL).join("song.flac"), b"bytes").expect("its copy");
        }
        fs::write(dead.join(PENDING), "1\n0000002a\ncontent://test/song.flac").expect("its record");

        let stage = Stage::made(&data, "song.flac").expect("a stage");

        assert!(dead.join(ORIGINAL).join("song.flac").is_file());
        assert!(!recordless.exists());

        let pending = pending_under(&data);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].uri, "content://test/song.flac");
        assert_eq!(pending[0].original_crc, 42);
        assert_eq!(pending[0].original(), dead.join(ORIGINAL).join("song.flac"));
        assert!(pending_under(&data).is_empty());

        drop((stage, pending));

        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_pending_rewrite_held_by_a_living_stage_is_not_claimed_until_the_stage_is_dropped() {
        let data = under("living");

        let mut stage = Stage::made(&data, "song.flac").expect("a stage");

        fs::write(stage.working(), b"bytes").expect("a working copy");

        let original = stage.set_aside().expect("an original set aside");

        stage
            .pend("content://test/song.flac", original)
            .expect("a record");

        assert!(pending_under(&data).is_empty());

        let folder = stage.folder.clone();

        drop(stage);

        let pending = pending_under(&data);

        assert!(!folder.join(WORKING).exists());
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].original_crc, original);
        assert_eq!(
            fs::read(pending[0].original()).expect("the original"),
            b"bytes"
        );

        pending.into_iter().for_each(Pending::settled);

        assert!(!folder.exists());

        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_record_in_a_form_not_known_is_not_read() {
        assert_eq!(
            read_record("1\n0000002a\ncontent://test/a.flac"),
            Some(("content://test/a.flac".to_owned(), 42))
        );
        assert_eq!(read_record("2\n0000002a\ncontent://test/a.flac"), None);
        assert_eq!(read_record("1\nzz\ncontent://test/a.flac"), None);
        assert_eq!(read_record("1\n0000002a\n"), None);
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
