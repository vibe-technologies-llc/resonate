use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

#[cfg(unix)]
pub(super) const READ_BY_ITS_OWNER_ALONE: u32 = 0o600;

#[cfg(unix)]
const ENTERED_BY_ITS_OWNER_ALONE: u32 = 0o700;

#[cfg(all(unix, feature = "sync"))]
const STORE_SIDE_FILES: [&str; 2] = ["-wal", "-shm"];

pub(super) fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = env::var_os(var).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    known_folder(var).or_else(|| home_dir().map(|home| home.join(fallback)))
}

#[cfg(unix)]
const HOME_VARIABLE: &str = "HOME";

#[cfg(windows)]
const HOME_VARIABLE: &str = "USERPROFILE";

pub(super) fn home_dir() -> Option<PathBuf> {
    env::var_os(HOME_VARIABLE)
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn known_folder(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
fn known_folder(var: &str) -> Option<PathBuf> {
    home_dir().map(|home| home.join(library_folder_for(var)))
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn library_folder_for(var: &str) -> &'static str {
    match var {
        "XDG_CACHE_HOME" => "Library/Caches",
        _ => "Library/Application Support",
    }
}

#[cfg(windows)]
fn known_folder(var: &str) -> Option<PathBuf> {
    env::var_os(windows_variable_for(var))
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

#[cfg(any(windows, test))]
pub(super) fn windows_variable_for(var: &str) -> &'static str {
    match var {
        "XDG_CONFIG_HOME" => "APPDATA",
        _ => "LOCALAPPDATA",
    }
}

pub fn data_dir() -> Result<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share")
        .map(|dir| dir.join("resonate"))
        .ok_or(Error::NoDataDir)
}

#[cfg(all(feature = "ui", unix, not(target_os = "macos")))]
pub fn icon_theme_dir() -> Result<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share")
        .map(|dir| dir.join("icons").join("hicolor"))
        .ok_or(Error::NoDataDir)
}

pub fn library_path() -> Result<PathBuf> {
    Ok(private_data_dir()?.join("library.db"))
}

pub fn private_data_dir() -> Result<PathBuf> {
    let dir = data_dir()?;

    fs::create_dir_all(&dir).map_err(|source| Error::CreateDir {
        path: dir.clone(),
        source,
    })?;
    entered_by_its_owner_alone(&dir)?;
    Ok(dir)
}

#[cfg(unix)]
pub(super) fn entered_by_its_owner_alone(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let standing = fs::metadata(dir)
        .map_err(|source| Error::Private {
            path: dir.to_path_buf(),
            source,
        })?
        .permissions()
        .mode();

    if standing & 0o777 == ENTERED_BY_ITS_OWNER_ALONE {
        return Ok(());
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(ENTERED_BY_ITS_OWNER_ALONE)).map_err(
        |source| Error::Private {
            path: dir.to_path_buf(),
            source,
        },
    )
}

#[cfg(not(unix))]
fn entered_by_its_owner_alone(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(all(unix, feature = "sync"))]
pub fn private_store(store: &Path) -> Result<()> {
    use std::{
        fs::OpenOptions,
        os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    };

    let private = |path: &Path| {
        fs::set_permissions(path, fs::Permissions::from_mode(READ_BY_ITS_OWNER_ALONE)).map_err(
            |source| Error::Private {
                path: path.to_path_buf(),
                source,
            },
        )
    };

    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(READ_BY_ITS_OWNER_ALONE)
        .open(store)
        .map_err(|source| Error::Private {
            path: store.to_path_buf(),
            source,
        })?;
    private(store)?;
    for suffix in STORE_SIDE_FILES {
        let mut side = store.as_os_str().to_os_string();

        side.push(suffix);

        let side = PathBuf::from(side);
        if side.exists() {
            private(&side)?;
        }
    }
    Ok(())
}

#[cfg(all(not(unix), feature = "sync"))]
pub fn private_store(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(feature = "ui")]
pub fn analyses_dir() -> Option<PathBuf> {
    xdg_dir("XDG_CACHE_HOME", ".cache").map(|dir| dir.join("resonate").join("analyses"))
}

pub fn vault_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("vault"))
}

pub fn equaliser_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("equaliser"))
}

pub fn config_path() -> Result<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", ".config")
        .map(|dir| dir.join("resonate").join("config.toml"))
        .ok_or(Error::NoConfigDir)
}
