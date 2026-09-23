use std::{
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use resonate_core::SourceId;

use crate::{Delivered, Identity, Obtained, Result};

const UNPROVIDED: &str = "unprovided";
const LOOKED_AT_EVERY: Duration = Duration::from_millis(50);

pub trait Provider: Send + Sync {
    fn source(&self) -> &SourceId;

    fn obtain(&self, identity: &Identity) -> Result<Obtained>;
}

pub struct Unprovided {
    source: SourceId,
}

impl Default for Unprovided {
    fn default() -> Self {
        Self {
            source: SourceId::new(UNPROVIDED).unwrap_or_else(|_| SourceId::local()),
        }
    }
}

impl Provider for Unprovided {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn obtain(&self, _identity: &Identity) -> Result<Obtained> {
        Ok(Obtained::Nothing)
    }
}

pub struct Asking<'a> {
    pub within: Duration,
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
}

#[derive(Debug, Default)]
pub struct Answer {
    pub delivered: Option<Delivered>,
    pub refused: u64,
    pub late: u64,
    pub cancelled: bool,
}

enum Asked {
    Answered(Result<Obtained>),
    Unasked,
    Late,
    Cancelled,
}

pub struct Providers {
    providers: Vec<Arc<dyn Provider>>,
}

impl Providers {
    pub fn none() -> Self {
        Self {
            providers: vec![Arc::new(Unprovided::default())],
        }
    }

    #[must_use]
    pub fn and(mut self, provider: Arc<dyn Provider>) -> Self {
        self.providers
            .retain(|held| held.source() != provider.source());
        self.providers.push(provider);
        self
    }

    pub fn names(&self) -> Vec<SourceId> {
        self.providers
            .iter()
            .map(|provider| provider.source().clone())
            .collect()
    }

    pub fn has_a_source(&self) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.source().as_str() != UNPROVIDED)
    }

    pub fn first(&self, identity: &Identity, asking: &Asking<'_>) -> Answer {
        let mut answer = Answer::default();

        for provider in &self.providers {
            match asked(provider, identity, asking) {
                Asked::Answered(Ok(Obtained::Found(delivery))) => {
                    answer.delivered = Some(Delivered {
                        provider: provider.source().clone(),
                        delivery,
                    });
                    return answer;
                }
                Asked::Answered(Ok(Obtained::Nothing)) => {}
                Asked::Answered(Err(error)) => {
                    tracing::warn!(%error, provider = %provider.source(), title = %identity.title, "a provider refused");
                    answer.refused += 1;
                }
                Asked::Unasked => answer.refused += 1,
                Asked::Late => {
                    tracing::warn!(provider = %provider.source(), title = %identity.title, within = ?asking.within, "a provider did not answer in time and was left behind");
                    answer.late += 1;
                }
                Asked::Cancelled => {
                    answer.cancelled = true;
                    return answer;
                }
            }
        }

        answer
    }
}

fn asked(provider: &Arc<dyn Provider>, identity: &Identity, asking: &Asking<'_>) -> Asked {
    let (told, answered) = mpsc::sync_channel(1);
    let asking_of = Arc::clone(provider);
    let about = identity.clone();
    let spawned = thread::Builder::new()
        .name(format!("resonate-provider-{}", provider.source()))
        .spawn(move || {
            let _ = told.send(asking_of.obtain(&about));
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, provider = %provider.source(), "a provider could not be given a thread to answer on");
        return Asked::Unasked;
    }

    let deadline = Instant::now() + asking.within;
    loop {
        if (asking.cancelled)() {
            return Asked::Cancelled;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Asked::Late;
        }
        match answered.recv_timeout(left.min(LOOKED_AT_EVERY)) {
            Ok(obtained) => return Asked::Answered(obtained),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                tracing::warn!(provider = %provider.source(), "a provider stopped without answering");
                return Asked::Unasked;
            }
        }
    }
}

impl Default for Providers {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use super::*;
    use crate::{Delivery, Error, ProviderOp};

    struct Fixed {
        source: SourceId,
        answer: fn() -> Result<Obtained>,
    }

    impl Fixed {
        fn registered(name: &str, answer: fn() -> Result<Obtained>) -> Arc<dyn Provider> {
            Arc::new(Self {
                source: SourceId::new(name).expect("a nameable source"),
                answer,
            })
        }
    }

    impl Provider for Fixed {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn obtain(&self, _identity: &Identity) -> Result<Obtained> {
            (self.answer)()
        }
    }

    fn refusing() -> Result<Obtained> {
        Err(Error::Io {
            provider: SourceId::new("broken").expect("a nameable source"),
            op: ProviderOp::ReadFolder,
            source: io::Error::other("gone"),
        })
    }

    fn found() -> Result<Obtained> {
        Ok(Obtained::Found(Delivery::File(PathBuf::from(
            "/inbox/a.flac",
        ))))
    }

    fn nothing() -> Result<Obtained> {
        Ok(Obtained::Nothing)
    }

    #[test]
    fn the_stub_is_the_only_provider_until_one_is_registered_and_a_name_registers_once() {
        let none = Providers::none();
        assert!(!none.has_a_source());
        assert_eq!(
            none.names(),
            vec![SourceId::new(UNPROVIDED).expect("a nameable source")]
        );

        let registered = Providers::default()
            .and(Fixed::registered("shop", nothing))
            .and(Fixed::registered("shop", found));
        assert_eq!(registered.names().len(), 2);
        assert!(registered.has_a_source());
    }

    #[test]
    fn the_first_provider_to_deliver_answers_and_a_refusal_before_it_is_counted() {
        let providers = Providers::none()
            .and(Fixed::registered("broken", refusing))
            .and(Fixed::registered("empty", nothing))
            .and(Fixed::registered("shop", found))
            .and(Fixed::registered("later", found));

        let answer = providers.first(&Identity::named("Echoes"), &patient());

        assert_eq!(answer.refused, 1);
        let delivered = answer.delivered.expect("a delivery");
        assert_eq!(delivered.provider.as_str(), "shop");
        assert!(matches!(delivered.delivery, Delivery::File(_)));
    }

    #[test]
    fn nothing_registered_delivers_nothing() {
        let answer = Providers::none().first(&Identity::named("Echoes"), &patient());
        assert!(answer.delivered.is_none());
        assert_eq!(answer.refused, 0);
    }

    struct Silent {
        source: SourceId,
    }

    impl Provider for Silent {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn obtain(&self, _identity: &Identity) -> Result<Obtained> {
            thread::sleep(Duration::from_secs(5));
            found()
        }
    }

    fn silent() -> Arc<dyn Provider> {
        Arc::new(Silent {
            source: SourceId::new("silent").expect("a nameable source"),
        })
    }

    fn never() -> bool {
        false
    }

    fn always() -> bool {
        true
    }

    fn patient() -> Asking<'static> {
        Asking {
            within: Duration::from_secs(60),
            cancelled: &never,
        }
    }

    #[test]
    fn a_provider_that_does_not_answer_in_time_is_left_behind_and_the_next_is_asked() {
        let providers = Providers::none()
            .and(silent())
            .and(Fixed::registered("shop", found));
        let started = Instant::now();

        let answer = providers.first(
            &Identity::named("Echoes"),
            &Asking {
                within: Duration::from_millis(100),
                cancelled: &never,
            },
        );

        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(answer.late, 1);
        assert_eq!(answer.refused, 0);
        assert!(!answer.cancelled);
        let delivered = answer.delivered.expect("a delivery");
        assert_eq!(delivered.provider.as_str(), "shop");
    }

    #[test]
    fn a_cancelled_poll_stops_waiting_on_a_provider_and_asks_no_other() {
        let providers = Providers::none()
            .and(silent())
            .and(Fixed::registered("shop", found));
        let started = Instant::now();

        let answer = providers.first(
            &Identity::named("Echoes"),
            &Asking {
                within: Duration::from_secs(60),
                cancelled: &always,
            },
        );

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(answer.cancelled);
        assert!(answer.delivered.is_none());
    }
}
