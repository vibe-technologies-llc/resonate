use std::io;

use resonate_audio::{Error, Operation};
use wasapi::WasapiError;

pub(crate) struct Apartment {
    entered: bool,
}

impl Apartment {
    pub(crate) fn enter() -> Self {
        Self {
            entered: wasapi::initialize_mta().is_ok(),
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.entered {
            wasapi::deinitialize();
        }
    }
}

pub(crate) fn backend_error(op: Operation, source: WasapiError) -> Error {
    Error::Backend {
        op,
        source: io::Error::other(source),
    }
}

pub(crate) fn initialize_error(op: Operation, error: WasapiError) -> Error {
    match error {
        WasapiError::UnsupportedFormat => Error::FormatRefused,
        other => backend_error(op, other),
    }
}
