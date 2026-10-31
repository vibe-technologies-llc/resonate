use std::path::{Path, PathBuf};

use resonate_core::AppId;

use crate::{Error, Result, activity::Activity};

pub(crate) enum Session {}

impl Session {
    pub(crate) fn find(_: AppId, _: Option<&Path>) -> Result<(Self, PathBuf)> {
        Err(Error::NoDiscord)
    }

    pub(crate) fn set(&mut self, _: Option<&Activity>) -> Result<()> {
        match *self {}
    }

    pub(crate) fn drain(&mut self) -> Result<()> {
        match *self {}
    }
}
