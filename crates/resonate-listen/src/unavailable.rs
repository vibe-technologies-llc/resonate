use std::time::Duration;

use resonate_audio::NodeName;

use crate::{Clip, Error, Hearing, Listening, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    pub name: NodeName,
    pub description: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct Listener;

impl Listener {
    pub fn new(_: &str) -> Self {
        Self
    }

    pub fn microphones(&self) -> Result<Vec<Microphone>> {
        Ok(Vec::new())
    }

    pub fn record(&self, _: &Listening, _: Duration, _: &Hearing) -> Result<Clip> {
        Err(Error::Unavailable)
    }

    pub fn play_and_record(
        &self,
        _: &[f32],
        _: Option<&NodeName>,
        _: &Listening,
        _: Duration,
        _: &Hearing,
    ) -> Result<Clip> {
        Err(Error::Unavailable)
    }
}
