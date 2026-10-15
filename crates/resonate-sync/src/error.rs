use std::{io, path::PathBuf, result};

use thiserror::Error;

use crate::{DeviceId, catalog::Refused, wire::Refusal};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StoreOp {
    Open,
    Migrate,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeflateOp {
    Pack,
    Unpack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TlsOp {
    Roots,
    Client,
    Server,
    Quic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamOp {
    Open,
    Write,
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileOp {
    Stage,
    Write,
    Read,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodeRefused {
    Length { found: usize },
    Symbol { at: usize },
    Check,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Malformed {
    Ended,
    Trailing { left: usize },
    Overlong { length: usize },
    Short { length: usize },
    Tag { tag: u8 },
    Number,
    NotText,
    NotAnMbid,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("the pairing code does not read: {0:?}")]
    Code(CodeRefused),

    #[error("this system gave no randomness")]
    NoRandomness,

    #[error("the sync keys could not be derived")]
    KeyDerivation,

    #[error("a sealed record does not open under this sync key")]
    Unsealed,

    #[error("a record of {length} bytes is past what one may hold")]
    TooLarge { length: usize },

    #[error("{op:?} failed on a record's compression")]
    Deflate {
        op: DeflateOp,
        #[source]
        source: io::Error,
    },

    #[error("a frame does not read: {0:?}")]
    Malformed(Malformed),

    #[error("{op:?} failed on the sync ledger")]
    Store {
        op: StoreOp,
        #[source]
        source: Box<rusqlite::Error>,
    },

    #[error("the sync ledger is stamped {found}, newer than this build reads")]
    LedgerVersion { found: u32 },

    #[error("the catalog refused: {0:?}")]
    Catalog(Refused),

    #[error("{op:?} failed setting up TLS")]
    Tls {
        op: TlsOp,
        #[source]
        source: Box<rustls::Error>,
    },

    #[error("the QUIC crypto setup refused its TLS configuration")]
    QuicCrypto,

    #[error("the peer certificate could not be made")]
    Certificate(#[source] Box<rcgen::Error>),

    #[error("the trusted certificate {} does not read", path.display())]
    TrustedCertificate {
        path: PathBuf,
        #[source]
        source: rustls_pki_types::pem::Error,
    },

    #[error("{server} does not resolve")]
    Resolve {
        server: Box<str>,
        #[source]
        source: io::Error,
    },

    #[error("{server} resolves to no address")]
    Unresolved { server: Box<str> },

    #[error("the sync socket could not be bound")]
    Bind(#[source] io::Error),

    #[error("the beacon socket could not be bound")]
    BeaconBind(#[source] io::Error),

    #[error("the sync runtime could not start")]
    Runtime(#[source] io::Error),

    #[error("the sync thread could not start")]
    Thread(#[source] io::Error),

    #[error("connecting failed")]
    Connect(#[source] Box<quinn::ConnectError>),

    #[error("the connection failed")]
    Connection(#[source] Box<quinn::ConnectionError>),

    #[error("{op:?} failed on a stream")]
    Stream {
        op: StreamOp,
        #[source]
        source: io::Error,
    },

    #[error("a stream was closed before it could be finished")]
    StreamClosed,

    #[error("a frame of {length} bytes is past the largest one may be")]
    Oversized { length: usize },

    #[error("the server refused: {0:?}")]
    Refused(Refusal),

    #[error("the server ended the session before welcoming this device")]
    Unwelcome,

    #[error("the server did not answer in time")]
    ServerSilent,

    #[error("the server holds this space afresh, behind what this device read")]
    SpaceMadeAnew,

    #[error("{op:?} failed on {}", path.display())]
    File {
        op: FileOp,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("device {0:?} could not be reached")]
    Unreachable(DeviceId),

    #[error("device {0:?} does not hold that song")]
    Missing(DeviceId),

    #[error("a device on this network did not answer a meeting")]
    Unmet,

    #[error("a device took changes before it was met")]
    Unintroduced,

    #[error("device {0:?} did not take the changes it was sent")]
    Untaken(DeviceId),

    #[error("the sync service has stopped")]
    Stopped,
}

impl Error {
    pub(crate) fn store(op: StoreOp, source: rusqlite::Error) -> Self {
        Self::Store {
            op,
            source: Box::new(source),
        }
    }

    pub(crate) fn tls(op: TlsOp, source: rustls::Error) -> Self {
        Self::Tls {
            op,
            source: Box::new(source),
        }
    }

    pub(crate) fn stream(op: StreamOp, source: impl Into<io::Error>) -> Self {
        Self::Stream {
            op,
            source: source.into(),
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
