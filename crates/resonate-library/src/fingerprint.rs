use std::{sync::Arc, time::Duration};

use resonate_core::{Chromaprint, FrameSpan, MediaLocation, SourceId};

use crate::{RecordingMatch, Result};

const UNPRINTED: &str = "unprinted";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sounded {
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
    pub length: Option<Duration>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub print: Chromaprint,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recognition {
    pub matches: Vec<RecordingMatch>,
    pub refused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Printed {
    Nothing,
    Recognised(Vec<RecordingMatch>),
}

pub trait Fingerprints: Send + Sync {
    fn source(&self) -> &SourceId;

    fn recognise(&self, sounded: &Sounded) -> Result<Printed>;
}

pub struct NoFingerprints {
    source: SourceId,
}

impl Default for NoFingerprints {
    fn default() -> Self {
        Self {
            source: SourceId::new(UNPRINTED).unwrap_or_else(|_| SourceId::local()),
        }
    }
}

impl Fingerprints for NoFingerprints {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn recognise(&self, _sounded: &Sounded) -> Result<Printed> {
        Ok(Printed::Nothing)
    }
}

pub struct Fingerprinters {
    printers: Vec<Arc<dyn Fingerprints>>,
}

impl Fingerprinters {
    pub fn none() -> Self {
        Self {
            printers: vec![Arc::new(NoFingerprints::default())],
        }
    }

    #[must_use]
    pub fn and(mut self, printer: Arc<dyn Fingerprints>) -> Self {
        self.printers
            .retain(|held| held.source() != printer.source());
        self.printers.push(printer);
        self
    }

    pub fn names(&self) -> Vec<SourceId> {
        self.printers
            .iter()
            .map(|printer| printer.source().clone())
            .collect()
    }

    pub fn has_a_source(&self) -> bool {
        self.printers
            .iter()
            .any(|printer| printer.source().as_str() != UNPRINTED)
    }

    pub fn recognise(&self, sounded: &Sounded) -> Recognition {
        let mut refused = false;
        for printer in &self.printers {
            match printer.recognise(sounded) {
                Ok(Printed::Recognised(found)) if !found.is_empty() => {
                    return Recognition {
                        matches: found,
                        refused: false,
                    };
                }
                Ok(Printed::Recognised(_) | Printed::Nothing) => {}
                Err(error) => {
                    tracing::warn!(
                        %error,
                        source = %printer.source(),
                        sounded = %sounded.location,
                        "a fingerprinter refused"
                    );
                    refused = true;
                }
            }
        }
        Recognition {
            matches: Vec::new(),
            refused,
        }
    }
}

impl Default for Fingerprinters {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_is_the_only_fingerprinter_until_one_is_registered_and_a_name_registers_once() {
        let none = Fingerprinters::none();
        assert!(!none.has_a_source());
        assert_eq!(
            none.names(),
            vec![SourceId::new(UNPRINTED).expect("a nameable source")]
        );

        let registered = Fingerprinters::default()
            .and(Arc::new(NoFingerprints::default()))
            .and(Arc::new(NoFingerprints::default()));
        assert_eq!(registered.names().len(), 1);
        assert!(!registered.has_a_source());
    }
}
