use crate::{Error, Result, WindowsOp};

pub(crate) struct Apartment;

impl Apartment {
    pub(crate) fn enter() -> Result<Self> {
        wasapi::initialize_mta()
            .ok()
            .map_err(|source| Error::windows(WindowsOp::Enter, source))?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}
