use std::{
    io,
    path::{Path, PathBuf},
    result,
};

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlacOp {
    Configure,
    Fill,
    Frame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PictureOp {
    Open,
    Render,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VaultOp {
    MakeFolder,
    Resolve,
    Stage,
    Write,
    Settle,
    Discard,
    Read,
    Walk,
    Verify,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} at {path} failed")]
    Io {
        op: VaultOp,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} is not inside the vault")]
    OutsideTheVault { path: PathBuf },

    #[error("a vault key is thirty-two hexadecimal letters")]
    NotAKey,

    #[error("{op:?} against a media stream failed")]
    Codec {
        op: VaultOp,
        #[source]
        source: Box<resonate_codec::Error>,
    },

    #[error("a {channels}-channel {bits}-bit stream at {rate} Hz is not a FLAC stream")]
    Unencodable { rate: u32, channels: u8, bits: u8 },

    #[error("{op:?} of a FLAC stream found nothing this build can encode")]
    Encoding { op: FlacOp },

    #[error("writing a FLAC stream failed")]
    Written {
        #[source]
        source: Box<flacenc::error::OutputError<flacenc::bitsink::MemSink<u8>>>,
    },

    #[error("a picture this build names could not be read as one")]
    Picture {
        #[source]
        source: Box<image::ImageError>,
    },

    #[error("a picture could not be written as JXL")]
    PictureWritten {
        #[source]
        source: Box<zune_jpegxl::JxlEncodeErrors>,
    },

    #[error("a picture {width} by {height} holds no pixels this build can draw")]
    PictureUnsized { width: u32, height: u32 },

    #[error("a picture {width} by {height} did not read back as the one that went in")]
    PictureUnconfirmed { width: u32, height: u32 },

    #[error("{op:?} of a JXL picture found nothing this build can draw")]
    PictureRead { op: PictureOp },

    #[error(transparent)]
    Domain(#[from] resonate_core::Error),
}

impl Error {
    pub(crate) fn io(op: VaultOp, path: impl AsRef<Path>, source: io::Error) -> Self {
        Self::Io {
            op,
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub(crate) fn codec(op: VaultOp, source: resonate_codec::Error) -> Self {
        Self::Codec {
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

    #[test]
    fn an_error_travels_to_whichever_thread_asked() {
        const fn carried<T: Send + Sync>() {}
        carried::<Error>();
    }
}
