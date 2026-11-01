use std::{
    env::{self, VarError},
    fs, io,
    num::ParseIntError,
    path::PathBuf,
};

use thiserror::Error;
use win_resource_builder::{
    resource::{InsertError, ResourceFile},
    version_info::{Version, VersionInfo, VersionInfoError},
};

use crate::icon_resources;

const ICON_FILE: &str = "../../packaging/windows/resonate.ico";
const RESOURCE_FILE: &str = "resonate.res";
const WINDOW_ICON: u16 = 1;
const PRODUCT_NAME: &str = "Resonate";
const FILE_DESCRIPTION: &str = "Resonate music player";
const INTERNAL_NAME: &str = "resonate";
const ORIGINAL_FILENAME: &str = "resonate.exe";
const PRERELEASE_BUILD: u16 = 0;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{name} is not set")]
    Unset {
        name: &'static str,
        #[source]
        source: VarError,
    },
    #[error("{name} is not a version part")]
    NotAVersionPart {
        name: &'static str,
        #[source]
        source: ParseIntError,
    },
    #[error("{} could not be written", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the icon is refused")]
    Icons(#[from] icon_resources::Error),
    #[error("the version information is refused")]
    Version(#[from] VersionInfoError),
    #[error("a resource is refused")]
    Insert(#[from] InsertError),
}

fn variable(name: &'static str) -> Result<String, Error> {
    env::var(name).map_err(|source| Error::Unset { name, source })
}

fn version_part(name: &'static str) -> Result<u16, Error> {
    variable(name)?
        .parse()
        .map_err(|source| Error::NotAVersionPart { name, source })
}

fn version() -> Result<Version, Error> {
    Ok(Version::new(
        version_part("CARGO_PKG_VERSION_MAJOR")?,
        version_part("CARGO_PKG_VERSION_MINOR")?,
        version_part("CARGO_PKG_VERSION_PATCH")?,
        PRERELEASE_BUILD,
    ))
}

fn version_info() -> Result<VersionInfo, Error> {
    let licence = variable("CARGO_PKG_LICENSE")?;

    Ok(VersionInfo::options()
        .company_name(PRODUCT_NAME)
        .file_description(FILE_DESCRIPTION)
        .internal_name(INTERNAL_NAME)
        .legal_copyright(format!("Licensed under {licence}"))
        .comments(variable("CARGO_PKG_VERSION")?)
        .create(PRODUCT_NAME, ORIGINAL_FILENAME, version()?)?)
}

fn read_icon(manifest_directory: &str) -> Result<Vec<u8>, Error> {
    let path = PathBuf::from(manifest_directory).join(ICON_FILE);

    println!("cargo::rerun-if-changed={}", path.display());
    fs::read(&path).map_err(|source| Error::Io { path, source })
}

pub fn embed() -> Result<(), Error> {
    let manifest_directory = variable("CARGO_MANIFEST_DIR")?;
    let out = PathBuf::from(variable("OUT_DIR")?).join(RESOURCE_FILE);

    let mut version = ResourceFile::new();
    version.insert_version_info(&version_info()?)?;

    let icons = icon_resources::records(&read_icon(&manifest_directory)?, WINDOW_ICON)?;
    let resources = icon_resources::joined(&icons, &version.to_res())?;
    fs::write(&out, resources).map_err(|source| Error::Io {
        path: out.clone(),
        source,
    })?;

    println!("cargo::rustc-link-arg-bins={}", out.display());
    Ok(())
}
