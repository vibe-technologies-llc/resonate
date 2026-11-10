use std::{fmt, process};

use crate::{
    device::Device,
    error::{Error, Result},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pid(i32);

impl Pid {
    pub(crate) const fn of(raw: i32) -> Self {
        Self(raw)
    }

    pub(crate) fn of_this_process() -> Self {
        Self(i32::try_from(process::id()).unwrap_or(i32::MAX))
    }

    pub const fn get(self) -> i32 {
        self.0
    }
}

impl fmt::Display for Pid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct Hog {
    device: Device,
}

impl Hog {
    pub fn taken(device: Device) -> Result<Self> {
        if let Some(by) = device.hogged_by()? {
            return Err(Error::Hogged { by });
        }
        device.toggle_the_hog()?;
        match device.hogged_by()? {
            Some(by) if by == Pid::of_this_process() => Ok(Self { device }),
            Some(by) => Err(Error::Hogged { by }),
            None => Err(Error::HogUntaken),
        }
    }
}

impl Drop for Hog {
    fn drop(&mut self) {
        if self.device.hogged_by().ok().flatten() == Some(Pid::of_this_process()) {
            let _ = self.device.toggle_the_hog();
        }
    }
}
