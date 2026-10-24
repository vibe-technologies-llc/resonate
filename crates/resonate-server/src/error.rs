use std::{io, path::PathBuf, result};

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StoreOp {
    Open,
    Migrate,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CacheOp {
    Create,
    List,
    Inspect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PemOp {
    Certificate,
    Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfigField {
    Hifi,
    Web,
    ForwardedBy,
    Listen,
    Upstream,
    Keys,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} failed on the server's store")]
    Store {
        op: StoreOp,
        #[source]
        source: Box<rusqlite::Error>,
    },

    #[error("the store is stamped {found}, newer than this server reads")]
    StoreVersion { found: u32 },

    #[error("{op:?} failed on the cache at {}", path.display())]
    Cache {
        op: CacheOp,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{} does not read as a PEM {op:?}", path.display())]
    Pem {
        op: PemOp,
        path: PathBuf,
        #[source]
        source: rustls_pki_types::pem::Error,
    },

    #[error("{} holds no certificate", path.display())]
    NoCertificate { path: PathBuf },

    #[error("{} could not be read", path.display())]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{} is not valid TOML", path.display())]
    ConfigSyntax {
        path: PathBuf,
        #[source]
        source: Box<toml_edit::TomlError>,
    },

    #[error("the config's {field:?} is missing")]
    ConfigMissing { field: ConfigField },

    #[error("the config's {field:?} does not read")]
    ConfigInvalid { field: ConfigField },

    #[error("the system could not supply random bytes")]
    Random,

    #[error("the TLS configuration was refused")]
    Tls(#[source] Box<rustls::Error>),

    #[error("the QUIC crypto setup refused its TLS configuration")]
    QuicCrypto,

    #[error("the server could not listen on its address")]
    Listen(#[source] io::Error),

    #[error("the server runtime could not start")]
    Runtime(#[source] io::Error),

    #[error("the server could not listen for the signal to stop")]
    Signals(#[source] io::Error),

    #[error("the store's worker went away")]
    Worker,

    #[error(transparent)]
    Sync(#[from] resonate_sync::Error),
}

impl Error {
    pub fn store(op: StoreOp, source: rusqlite::Error) -> Self {
        Self::Store {
            op,
            source: Box::new(source),
        }
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
