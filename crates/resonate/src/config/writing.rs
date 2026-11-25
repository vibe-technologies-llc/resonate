use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::file_facts;
use resonate_engine::{Plays, Trial};
use resonate_eq::Binding;
use toml_edit::{DocumentMut, Item, Table};

#[cfg(unix)]
use super::places::READ_BY_ITS_OWNER_ALONE;
use super::reading::{REMOVED_CONTACT, Retired, document};
use crate::{ConfigKey, Error, Result, ValueKind};

const LOCK_SUFFIX: &str = ".lock";
const LINKS_FOLLOWED_AT_MOST: usize = 40;

pub(super) const STAGING_SUFFIX: &str = ".new";

#[cfg(windows)]
const WRITERS_LOCK_SUFFIX: &str = ".writing";

#[cfg(unix)]
const ITS_OWNERS_BITS: u32 = 0o700;

static STAGED: AtomicU64 = AtomicU64::new(0);

pub fn learnt_written(plays: &Plays) -> toml_edit::Value {
    plays
        .trials()
        .map(Trial::spelling)
        .collect::<toml_edit::Array>()
        .into()
}

pub fn written(binding: &Binding) -> toml_edit::Value {
    match binding {
        Binding::Profile(name) => name.as_str().into(),
        Binding::Own => true.into(),
    }
}

pub struct Editing<'d> {
    document: &'d mut DocumentMut,
    target: &'d Path,
    changed: bool,
}

impl Editing<'_> {
    pub fn store(&mut self, key: ConfigKey, value: impl Into<toml_edit::Value>) {
        replaced_in_place(&mut self.document[key.as_str()], value.into());
        self.retire_the_spellings_of(key);
        self.changed = true;
    }

    pub fn clear(&mut self, key: ConfigKey) {
        self.changed |= self.document.remove(key.as_str()).is_some();
        self.retire_the_spellings_of(key);
    }

    fn retire_the_spellings_of(&mut self, key: ConfigKey) {
        for spelt in Retired::spellings_of(key) {
            self.changed |= self.document.remove(spelt).is_some();
        }
    }

    pub fn store_in_table(
        &mut self,
        key: ConfigKey,
        entry: &str,
        value: impl Into<toml_edit::Value>,
    ) -> Result<()> {
        let held = self.document.entry(key.as_str()).or_insert_with(|| {
            let mut fresh = Table::new();

            fresh.decor_mut().set_prefix("\n");
            Item::Table(fresh)
        });
        let table = held.as_table_like_mut().ok_or_else(|| Error::ConfigType {
            path: self.target.to_path_buf(),
            key,
            expected: ValueKind::Table,
        })?;

        match table.get_mut(entry) {
            Some(held) => replaced_in_place(held, value.into()),
            None => {
                table.insert(entry, toml_edit::value(value));
            }
        }
        self.changed = true;
        Ok(())
    }

    pub fn clear_in_table(&mut self, key: ConfigKey, entry: &str) {
        let Some(table) = self
            .document
            .get_mut(key.as_str())
            .and_then(Item::as_table_like_mut)
        else {
            return;
        };

        if table.remove(entry).is_none() {
            return;
        }
        if table.is_empty() {
            self.document.remove(key.as_str());
        }
        self.changed = true;
    }
}

fn replaced_in_place(held: &mut Item, value: toml_edit::Value) {
    let decor = held.as_value().map(|was| was.decor().clone());

    *held = toml_edit::value(value);
    if let (Some(decor), Some(now)) = (decor, held.as_value_mut()) {
        *now.decor_mut() = decor;
    }
}

pub fn store(path: &Path, key: ConfigKey, value: impl Into<toml_edit::Value>) -> Result<()> {
    edit(path, |editing| {
        editing.store(key, value);
        Ok(())
    })
}

pub fn clear(path: &Path, key: ConfigKey) -> Result<()> {
    edit(path, |editing| {
        editing.clear(key);
        Ok(())
    })
}

pub fn store_in_table(
    path: &Path,
    key: ConfigKey,
    entry: &str,
    value: impl Into<toml_edit::Value>,
) -> Result<()> {
    edit(path, |editing| editing.store_in_table(key, entry, value))
}

pub fn clear_in_table(path: &Path, key: ConfigKey, entry: &str) -> Result<()> {
    edit(path, |editing| {
        editing.clear_in_table(key, entry);
        Ok(())
    })
}

pub fn edit(path: &Path, edits: impl FnOnce(&mut Editing<'_>) -> Result<()>) -> Result<()> {
    let target = followed(path);

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let _alone = held_alone(&target)?;
    let mut document = document(&target, &read_or_empty(&target)?)?;

    let changed = document.remove(REMOVED_CONTACT).is_some();
    let mut editing = Editing {
        document: &mut document,
        target: &target,
        changed,
    };

    edits(&mut editing)?;
    if !editing.changed {
        return Ok(());
    }
    write(&target, &document.to_string())
}

pub(super) fn followed(path: &Path) -> PathBuf {
    let mut standing = path.to_path_buf();

    for _ in 0..LINKS_FOLLOWED_AT_MOST {
        if let Ok(resolved) = fs::canonicalize(&standing) {
            return resolved;
        }

        let Ok(link) = fs::read_link(&standing) else {
            return standing;
        };

        standing = match standing.parent() {
            Some(folder) => folder.join(link),
            None => link,
        };
    }
    standing
}

fn held_alone(target: &Path) -> Result<File> {
    let refused = |source| Error::WriteConfig {
        path: target.to_path_buf(),
        source,
    };
    let folder = target.parent().unwrap_or_else(|| Path::new("."));

    let lock = writers_lock(target, folder).map_err(refused)?;

    lock.lock().map_err(refused)?;
    sweep_what_writers_left(target, folder);
    Ok(lock)
}

#[cfg(unix)]
fn writers_lock(_: &Path, folder: &Path) -> io::Result<File> {
    File::open(folder)
}

#[cfg(windows)]
fn writers_lock(target: &Path, _: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(beside(target, WRITERS_LOCK_SUFFIX))
}

fn sweep_what_writers_left(target: &Path, folder: &Path) {
    let Some(named) = target.file_name().and_then(|named| named.to_str()) else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let left = entry
            .file_name()
            .to_str()
            .is_some_and(|name| left_by_a_writer(name, named));
        if left && let Err(error) = fs::remove_file(entry.path()) {
            tracing::debug!(%error, path = %entry.path().display(), "what a writer left could not be swept");
        }
    }
}

fn left_by_a_writer(name: &str, target: &str) -> bool {
    let Some(rest) = name.strip_prefix(target) else {
        return false;
    };

    if rest == LOCK_SUFFIX {
        return true;
    }

    let Some(staged) = rest
        .strip_prefix('.')
        .and_then(|staged| staged.strip_suffix(STAGING_SUFFIX))
    else {
        return false;
    };
    let numbered = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    staged
        .split_once('-')
        .is_some_and(|(pid, nth)| numbered(pid) && numbered(nth))
}

pub(super) fn write(target: &Path, text: &str) -> Result<()> {
    let staged = beside(
        target,
        &format!(
            ".{}-{}{STAGING_SUFFIX}",
            process::id(),
            STAGED.fetch_add(1, Ordering::Relaxed)
        ),
    );
    file_facts::write_atomically(target, &staged, text.as_bytes(), |staged| {
        staged_for_its_owner_alone(staged, target)
    })
    .map_err(|unwritten| Error::WriteConfig {
        path: target.to_path_buf(),
        source: unwritten.source,
    })
}

#[cfg(unix)]
fn staged_for_its_owner_alone(staged: &Path, target: &Path) -> io::Result<File> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(READ_BY_ITS_OWNER_ALONE)
        .open(staged)?;

    if let Ok(standing) = fs::metadata(target) {
        let owners = standing.permissions().mode() & ITS_OWNERS_BITS;

        file.set_permissions(fs::Permissions::from_mode(owners))?;
    }
    Ok(file)
}

#[cfg(windows)]
fn staged_for_its_owner_alone(staged: &Path, _: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(staged)
}

fn beside(target: &Path, suffix: &str) -> PathBuf {
    let mut named = target.file_name().unwrap_or_default().to_os_string();

    named.push(suffix);
    target.with_file_name(named)
}

fn read_or_empty(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(Error::ReadConfig {
            path: path.to_path_buf(),
            source,
        }),
    }
}
