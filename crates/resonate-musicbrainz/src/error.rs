use std::{io, path::PathBuf, result};

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Malformed {
    Short,
    Overlong { length: usize },
    Tag { tag: u8 },
    Text,
    Isrc,
    Barcode,
    Number,
    Trailing,
    Unpacked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreOp {
    Open,
    Configure,
    LayOut,
    Read,
    Write,
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveOp {
    Open,
    Entry,
    Read,
    Stage,
    Rename,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackOp {
    Pack,
    Unpack,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("an encoded lookup is malformed: {0:?}")]
    Malformed(Malformed),

    #[error("the {store} store could not {op:?}")]
    Store {
        store: Store,
        op: StoreOp,
        #[source]
        source: Box<rusqlite::Error>,
    },

    #[error("the kept answers were laid out by a later build (version {found})")]
    KeptVersion { found: u32 },

    #[error("the mirror was built in format {found}, this build reads {reads}")]
    MirrorFormat { found: u32, reads: u32 },

    #[error("no mirror has been built at {path}")]
    NoMirror { path: PathBuf },

    #[error("the dump archive {path} could not {op:?}")]
    Archive {
        path: PathBuf,
        op: ArchiveOp,
        #[source]
        source: io::Error,
    },

    #[error("the dump's {table} table has a row of {found} columns where {wanted} were expected")]
    Columns {
        table: &'static str,
        found: usize,
        wanted: usize,
    },

    #[error("the dump lacks its {table} table")]
    MissingTable { table: &'static str },

    #[cfg(feature = "download")]
    #[error("the dump archive {file} does not match the checksum its directory lists")]
    Checksum { file: String },

    #[cfg(feature = "download")]
    #[error("the dump directory lists no checksum for {file}")]
    Unlisted { file: String },

    #[error("zstd could not {op:?}")]
    Pack {
        op: PackOp,
        #[source]
        source: io::Error,
    },

    #[error("the build was cancelled")]
    Cancelled,

    #[error("another run is building the mirror")]
    AlreadyBuilding,

    #[cfg(feature = "download")]
    #[error("the dump could not be fetched: {url}")]
    Fetch {
        url: String,
        #[source]
        source: io::Error,
    },

    #[cfg(feature = "download")]
    #[error("the dump server refused {url} with status {status}")]
    Refused { url: String, status: u16 },

    #[cfg(feature = "download")]
    #[error("the dump server named no latest export")]
    NoLatest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Mirror,
    Kept,
    Staging,
}

impl std::fmt::Display for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mirror => "mirror",
            Self::Kept => "kept answers",
            Self::Staging => "staging",
        })
    }
}

impl Error {
    pub(crate) fn store(store: Store, op: StoreOp) -> impl FnOnce(rusqlite::Error) -> Self {
        move |source| Self::Store {
            store,
            op,
            source: Box::new(source),
        }
    }

    pub(crate) fn archive(path: &std::path::Path, op: ArchiveOp) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_path_buf();
        move |source| Self::Archive { path, op, source }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
