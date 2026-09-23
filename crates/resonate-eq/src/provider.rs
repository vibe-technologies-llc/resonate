use std::sync::Arc;

use resonate_core::{SourceId, eq::Profile};

use crate::{Catalogue, DeviceId, Result};

const UNCORRECTED: &str = "none";

pub trait Corrections: Send + Sync {
    fn source(&self) -> &SourceId;

    fn catalogue(&self) -> Result<Arc<Catalogue>>;

    fn profile(&self, device: &DeviceId) -> Result<Option<Profile>>;
}

pub struct Uncorrected {
    source: SourceId,
}

impl Default for Uncorrected {
    fn default() -> Self {
        Self {
            source: SourceId::new(UNCORRECTED).unwrap_or_else(|_| SourceId::local()),
        }
    }
}

impl Corrections for Uncorrected {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn catalogue(&self) -> Result<Arc<Catalogue>> {
        Ok(Arc::new(Catalogue::empty()))
    }

    fn profile(&self, _device: &DeviceId) -> Result<Option<Profile>> {
        Ok(None)
    }
}

pub struct Corrected {
    sources: Vec<Arc<dyn Corrections>>,
}

impl Corrected {
    pub fn uncorrected() -> Self {
        Self {
            sources: vec![Arc::new(Uncorrected::default())],
        }
    }

    #[must_use]
    pub fn and(mut self, source: Arc<dyn Corrections>) -> Self {
        self.sources.retain(|held| held.source() != source.source());
        self.sources.push(source);
        self
    }

    pub fn names(&self) -> Vec<SourceId> {
        self.sources
            .iter()
            .map(|source| source.source().clone())
            .collect()
    }

    pub fn has_a_source(&self) -> bool {
        self.sources
            .iter()
            .any(|source| source.source().as_str() != UNCORRECTED)
    }

    pub fn catalogue(&self) -> Result<Arc<Catalogue>> {
        let mut refused = None;

        for source in &self.sources {
            match source.catalogue() {
                Ok(catalogue) if !catalogue.is_empty() => return Ok(catalogue),
                Ok(_) => {}
                Err(error) => {
                    tracing::debug!(%error, source = %source.source(), "a correction source refused");
                    refused = refused.or(Some(error));
                }
            }
        }

        match refused {
            Some(error) => Err(error),
            None => Ok(Arc::new(Catalogue::empty())),
        }
    }

    pub fn profile(&self, device: &DeviceId) -> Result<Option<Profile>> {
        let mut refused = None;

        for source in &self.sources {
            match source.profile(device) {
                Ok(Some(profile)) => return Ok(Some(profile)),
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%error, source = %source.source(), "a correction source refused");
                    refused = refused.or(Some(error));
                }
            }
        }

        match refused {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
}

impl Default for Corrected {
    fn default() -> Self {
        Self::uncorrected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Device, EqOp, Error};

    struct Held {
        source: SourceId,
        catalogue: Arc<Catalogue>,
    }

    impl Corrections for Held {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn catalogue(&self) -> Result<Arc<Catalogue>> {
            Ok(Arc::clone(&self.catalogue))
        }

        fn profile(&self, _device: &DeviceId) -> Result<Option<Profile>> {
            Ok(Some(Profile::flat()))
        }
    }

    struct Refusing {
        source: SourceId,
    }

    impl Corrections for Refusing {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn catalogue(&self) -> Result<Arc<Catalogue>> {
            Err(Error::Unreadable {
                provider: self.source.clone(),
                op: EqOp::Index,
            })
        }

        fn profile(&self, _device: &DeviceId) -> Result<Option<Profile>> {
            Err(Error::Unreadable {
                provider: self.source.clone(),
                op: EqOp::Fetch,
            })
        }
    }

    fn named(name: &str) -> SourceId {
        SourceId::new(name).expect("a lowercase name")
    }

    fn holding(name: &str) -> Arc<Held> {
        Arc::new(Held {
            source: named(name),
            catalogue: Arc::new(Catalogue::of(vec![Device {
                id: DeviceId::new("a/over-ear/Thing").expect("a path"),
                label: "Thing".to_owned(),
                measured_by: name.to_owned(),
                rig: None,
            }])),
        })
    }

    fn thing() -> DeviceId {
        DeviceId::new("a/over-ear/Thing").expect("a path")
    }

    #[test]
    fn a_build_with_nothing_behind_it_knows_of_no_device_and_says_so() {
        let corrected = Corrected::uncorrected();

        assert!(!corrected.has_a_source());
        assert!(corrected.catalogue().expect("nothing failed").is_empty());
        assert_eq!(corrected.profile(&thing()).expect("nothing failed"), None);
        assert_eq!(corrected.names(), vec![named(UNCORRECTED)]);
    }

    #[test]
    fn the_first_source_that_answers_is_the_one_the_pane_draws() {
        let corrected = Corrected::uncorrected()
            .and(holding("first"))
            .and(holding("second"));

        assert!(corrected.has_a_source());
        let catalogue = corrected.catalogue().expect("nothing failed");
        assert_eq!(catalogue.len(), 1);
        assert_eq!(
            catalogue
                .devices()
                .first()
                .map(|device| &device.measured_by),
            Some(&"first".to_owned())
        );
    }

    #[test]
    fn a_source_that_refuses_does_not_stop_the_one_behind_it() {
        let corrected = Corrected::uncorrected()
            .and(Arc::new(Refusing {
                source: named("first"),
            }))
            .and(holding("second"));

        assert_eq!(corrected.catalogue().expect("the second answered").len(), 1);
        assert!(
            corrected
                .profile(&thing())
                .expect("the second answered")
                .is_some()
        );
    }

    #[test]
    fn a_refusal_is_only_reported_where_nothing_else_answered() {
        let corrected = Corrected::uncorrected().and(Arc::new(Refusing {
            source: named("first"),
        }));

        assert!(matches!(
            corrected.catalogue(),
            Err(Error::Unreadable { .. })
        ));
        assert!(matches!(
            corrected.profile(&thing()),
            Err(Error::Unreadable { .. })
        ));
    }

    #[test]
    fn a_source_added_twice_under_one_name_replaces_the_one_before_it() {
        let corrected = Corrected::uncorrected()
            .and(holding("autoeq"))
            .and(holding("autoeq"));

        assert_eq!(corrected.names(), vec![named(UNCORRECTED), named("autoeq")]);
    }
}
