use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use resonate_core::SourceId;

use crate::{Delivered, Identity, Obtained, Result};

const UNPROVIDED: &str = "unprovided";
const LOOKED_AT_EVERY: Duration = Duration::from_millis(50);

pub const TURNED_TO_APART: Duration = Duration::from_millis(1_500);

pub const WAITED_ON_AFTER_AN_OFFER: Duration = Duration::from_secs(3);

pub trait Provider: Send + Sync {
    fn source(&self) -> &SourceId;

    fn find(&self, identity: &Identity) -> Result<Obtained>;
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

    fn find(&self, _identity: &Identity) -> Result<Obtained> {
        Ok(Obtained::Nothing)
    }
}

pub struct Asking<'a> {
    pub within: Duration,
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    pub turning_to: &'a (dyn Fn(&SourceId) + Sync),
    pub declined: &'a (dyn Fn(&Delivered) -> bool + Sync),
    pub passing: &'a [SourceId],
    pub apart: Duration,
    pub grace: Duration,
}

#[derive(Debug, Default)]
pub struct Answer {
    pub delivered: Option<Delivered>,
    pub refused: u64,
    pub late: u64,
    pub passed_over: u64,
    pub declined: u64,
    pub narrowed: bool,
    pub cancelled: bool,
    pub heard: Vec<SourceId>,
    pub held_back: Vec<Delivered>,
}

impl Answer {
    fn taking(mut self, offers: Vec<Option<Delivered>>) -> Self {
        let mut offered = offers.into_iter().flatten();
        self.delivered = offered.next();
        self.held_back = offered.collect();
        self
    }

    pub fn heard_from_every_provider(&self) -> bool {
        !self.narrowed
            && self.refused == 0
            && self.late == 0
            && self.passed_over == 0
            && !self.cancelled
    }
}

#[derive(Clone, Debug, Default)]
pub struct Away {
    providers: Vec<SourceId>,
}

impl Away {
    pub fn note(&mut self, provider: &SourceId) {
        if !self.holds(provider) {
            tracing::warn!(%provider, "a provider is away and is not asked again this poll");
            self.providers.push(provider.clone());
        }
    }

    pub fn holds(&self, provider: &SourceId) -> bool {
        self.providers.contains(provider)
    }

    pub fn join(&mut self, other: &Self) {
        for provider in &other.providers {
            if !self.holds(provider) {
                self.providers.push(provider.clone());
            }
        }
    }
}

pub struct Providers {
    providers: Vec<Arc<dyn Provider>>,
    narrowed: bool,
}

impl Providers {
    pub fn none() -> Self {
        Self {
            providers: vec![Arc::new(Unprovided::default())],
            narrowed: false,
        }
    }

    #[must_use]
    pub fn only(&self, source: &SourceId) -> Self {
        let others_left_out = self
            .providers
            .iter()
            .any(|provider| is_a_source(provider) && provider.source() != source);
        let narrowed = Self {
            narrowed: self.narrowed || others_left_out,
            ..Self::none()
        };

        match self
            .providers
            .iter()
            .find(|provider| provider.source() == source)
        {
            Some(provider) => narrowed.and(Arc::clone(provider)),
            None => narrowed,
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
        self.providers.iter().any(is_a_source)
    }

    pub fn first(&self, identity: &Identity, asking: &Asking<'_>, away: &mut Away) -> Answer {
        let mut answer = Answer {
            narrowed: self.narrowed,
            ..Answer::default()
        };
        let asked_of: Vec<&Arc<dyn Provider>> = self
            .providers
            .iter()
            .filter(|provider| is_a_source(provider) && !asking.passing.contains(provider.source()))
            .collect();
        let mut turns: Vec<Turn> = asked_of
            .iter()
            .map(|provider| {
                if away.holds(provider.source()) {
                    answer.passed_over += 1;
                    Turn::Over
                } else {
                    Turn::Waiting
                }
            })
            .collect();
        let mut offers: Vec<Option<Delivered>> = asked_of.iter().map(|_| None).collect();
        let (told, answers) = mpsc::channel();
        let began = Instant::now();
        let mut grace_ends: Option<Instant> = None;

        loop {
            let best = offers.iter().position(Option::is_some);
            if (asking.cancelled)() {
                answer.cancelled = best.is_none();
                return answer.taking(offers);
            }
            let now = Instant::now();
            for at in 0..asked_of.len() {
                let turned_to = began + asking.apart.saturating_mul(rank(at));
                let every_one_before_answered = turns[..at].iter().all(|turn| *turn == Turn::Over);
                if turns[at] != Turn::Waiting
                    || best.is_some_and(|best| at > best)
                    || !(every_one_before_answered || now >= turned_to)
                {
                    continue;
                }
                let provider = asked_of[at];
                (asking.turning_to)(provider.source());
                turns[at] = match asked(provider, identity, at, &told) {
                    Ok(()) => Turn::Asked { since: now },
                    Err(error) => {
                        tracing::warn!(%error, provider = %provider.source(), "a provider could not be given a thread to answer on");
                        answer.refused += 1;
                        answer.heard.push(provider.source().clone());
                        Turn::Over
                    }
                };
            }

            if let Some(best) = best {
                let earlier_still_asked = turns[..best].iter().any(|turn| *turn != Turn::Over);
                let grace_spent = grace_ends.is_some_and(|ends| now >= ends);
                if !earlier_still_asked || grace_spent {
                    return answer.taking(offers);
                }
            } else if turns.iter().all(|turn| *turn == Turn::Over) {
                return answer;
            }

            for (at, turn) in turns.iter_mut().enumerate() {
                if let Turn::Asked { since } = *turn
                    && now >= since + asking.within
                {
                    let provider = asked_of[at];
                    tracing::warn!(provider = %provider.source(), title = %identity.title, within = ?asking.within, "a provider did not answer in time and was left behind");
                    answer.late += 1;
                    answer.heard.push(provider.source().clone());
                    away.note(provider.source());
                    *turn = Turn::Over;
                }
            }

            let Ok((at, obtained)) = answers.recv_timeout(LOOKED_AT_EVERY) else {
                continue;
            };
            if !matches!(turns[at], Turn::Asked { .. }) {
                continue;
            }
            turns[at] = Turn::Over;
            let provider = asked_of[at];
            answer.heard.push(provider.source().clone());
            match obtained {
                Ok(Obtained::Found(delivery)) => {
                    let delivered = Delivered {
                        provider: provider.source().clone(),
                        delivery,
                    };
                    if (asking.declined)(&delivered) {
                        tracing::info!(provider = %provider.source(), taken_from = %delivered.taken_from(), title = %identity.title, "a delivery the poll declines was passed over");
                        answer.declined += 1;
                        continue;
                    }
                    offers[at] = Some(delivered);
                    grace_ends.get_or_insert_with(|| Instant::now() + asking.grace);
                }
                Ok(Obtained::Nothing) => {}
                Err(error) => {
                    tracing::warn!(%error, provider = %provider.source(), title = %identity.title, "a provider refused");
                    answer.refused += 1;
                    if error.is_the_provider_away() {
                        away.note(provider.source());
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    Waiting,
    Asked { since: Instant },
    Over,
}

fn rank(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

fn is_a_source(provider: &Arc<dyn Provider>) -> bool {
    provider.source().as_str() != UNPROVIDED
}

fn asked(
    provider: &Arc<dyn Provider>,
    identity: &Identity,
    at: usize,
    told: &mpsc::Sender<(usize, Result<Obtained>)>,
) -> std::io::Result<()> {
    let asking_of = Arc::clone(provider);
    let about = identity.clone();
    let told = told.clone();
    thread::Builder::new()
        .name(format!("resonate-provider-{}", provider.source()))
        .spawn(move || {
            let _ = told.send((at, asking_of.find(&about)));
        })
        .map(|_| ())
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
    use crate::{Delivery, Error, Extension, Opened, Opening, ProviderOp};

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

        fn find(&self, _identity: &Identity) -> Result<Obtained> {
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

        let answer = providers.first(&Identity::named("Echoes"), &patient(), &mut Away::default());

        assert_eq!(answer.refused, 1);
        let delivered = answer.delivered.expect("a delivery");
        assert_eq!(delivered.provider.as_str(), "shop");
        assert!(matches!(delivered.delivery, Delivery::File(_)));
    }

    struct Slow {
        source: SourceId,
        after: Duration,
        answer: fn() -> Result<Obtained>,
        asked: std::sync::atomic::AtomicBool,
    }

    impl Slow {
        fn registered(name: &str, after: Duration, answer: fn() -> Result<Obtained>) -> Arc<Self> {
            Arc::new(Self {
                source: SourceId::new(name).expect("a nameable source"),
                after,
                answer,
                asked: std::sync::atomic::AtomicBool::new(false),
            })
        }

        fn was_asked(&self) -> bool {
            self.asked.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Provider for Slow {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn find(&self, _identity: &Identity) -> Result<Obtained> {
            self.asked.store(true, std::sync::atomic::Ordering::SeqCst);
            thread::sleep(self.after);
            (self.answer)()
        }
    }

    fn racing(apart: u64, grace: u64) -> Asking<'static> {
        Asking {
            apart: Duration::from_millis(apart),
            grace: Duration::from_millis(grace),
            ..patient()
        }
    }

    fn delivered_by(answer: &Answer) -> Option<&str> {
        answer
            .delivered
            .as_ref()
            .map(|delivered| delivered.provider.as_str())
    }

    #[test]
    fn a_slow_provider_does_not_hold_back_one_registered_after_it() {
        let providers = Providers::none()
            .and(Slow::registered("slow", Duration::from_secs(2), nothing))
            .and(Fixed::registered("shop", found));
        let started = Instant::now();

        let answer = providers.first(
            &Identity::named("Echoes"),
            &racing(100, 200),
            &mut Away::default(),
        );

        assert_eq!(delivered_by(&answer), Some("shop"));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn an_earlier_provider_answering_within_the_grace_is_taken_over_a_later_one_that_offered_first()
    {
        let providers = Providers::none()
            .and(Slow::registered("inbox", Duration::from_millis(300), found))
            .and(Fixed::registered("shop", found));

        let answer = providers.first(
            &Identity::named("Echoes"),
            &racing(100, 2_000),
            &mut Away::default(),
        );

        assert_eq!(delivered_by(&answer), Some("inbox"));
    }

    #[test]
    fn the_second_provider_registered_is_turned_to_one_pace_in_and_not_two() {
        const APART: u64 = 300;

        let providers = Providers::none()
            .and(Slow::registered("slow", Duration::from_secs(2), nothing))
            .and(Fixed::registered("shop", found));
        let started = Instant::now();

        let answer = providers.first(
            &Identity::named("Echoes"),
            &racing(APART, 100),
            &mut Away::default(),
        );

        assert_eq!(delivered_by(&answer), Some("shop"));
        assert!(
            started.elapsed() < Duration::from_millis(APART * 2),
            "{:?}",
            started.elapsed()
        );
    }

    struct Offers {
        source: SourceId,
        after: Duration,
        opened: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Offers {
        fn registered(name: &str, after: Duration) -> Arc<Self> {
            Arc::new(Self {
                source: SourceId::new(name).expect("a nameable source"),
                after,
                opened: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            })
        }

        fn was_opened(&self) -> bool {
            self.opened.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Provider for Offers {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn find(&self, _identity: &Identity) -> Result<Obtained> {
            thread::sleep(self.after);
            let opened = Arc::clone(&self.opened);
            Ok(Obtained::Found(Delivery::Stream {
                key: self.source.as_str().into(),
                extension: Extension::new("flac")?,
                opening: Opening::new(move || {
                    opened.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(Opened::Reading(Box::new(io::empty())))
                }),
            }))
        }
    }

    #[test]
    fn an_offer_not_taken_is_handed_back_unopened_behind_the_one_taken() {
        let inbox = Offers::registered("inbox", Duration::from_millis(300));
        let shop = Offers::registered("shop", Duration::ZERO);
        let providers = Providers::none()
            .and(Arc::clone(&inbox) as Arc<dyn Provider>)
            .and(Arc::clone(&shop) as Arc<dyn Provider>);

        let answer = providers.first(
            &Identity::named("Echoes"),
            &racing(100, 2_000),
            &mut Away::default(),
        );

        assert_eq!(delivered_by(&answer), Some("inbox"));
        assert_eq!(
            answer
                .held_back
                .iter()
                .map(|offer| offer.provider.as_str())
                .collect::<Vec<_>>(),
            ["shop"]
        );
        assert_eq!(answer.heard.len(), 2);
        assert!(!inbox.was_opened() && !shop.was_opened());
    }

    #[test]
    fn a_later_offer_is_taken_once_the_grace_runs_out() {
        let providers = Providers::none()
            .and(Slow::registered("slow", Duration::from_secs(2), found))
            .and(Fixed::registered("shop", found));
        let started = Instant::now();

        let answer = providers.first(
            &Identity::named("Echoes"),
            &racing(100, 200),
            &mut Away::default(),
        );

        assert_eq!(delivered_by(&answer), Some("shop"));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_provider_is_not_asked_before_its_turn_while_one_before_it_may_still_answer() {
        let later = Slow::registered("later", Duration::ZERO, found);
        let providers = Providers::none()
            .and(Slow::registered("inbox", Duration::from_millis(200), found))
            .and(Arc::clone(&later) as Arc<dyn Provider>);

        let answer = providers.first(
            &Identity::named("Echoes"),
            &racing(2_000, 2_000),
            &mut Away::default(),
        );

        assert_eq!(delivered_by(&answer), Some("inbox"));
        assert!(!later.was_asked(), "a provider was asked before its turn");
    }

    #[test]
    fn nothing_registered_delivers_nothing() {
        let answer =
            Providers::none().first(&Identity::named("Echoes"), &patient(), &mut Away::default());
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

        fn find(&self, _identity: &Identity) -> Result<Obtained> {
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

    fn unheard(_: &SourceId) {}

    fn declining_nothing(_: &Delivered) -> bool {
        false
    }

    fn patient() -> Asking<'static> {
        Asking {
            within: Duration::from_secs(60),
            cancelled: &never,
            turning_to: &unheard,
            declined: &declining_nothing,
            passing: &[],
            apart: TURNED_TO_APART,
            grace: WAITED_ON_AFTER_AN_OFFER,
        }
    }

    #[test]
    fn a_provider_passed_for_the_ask_is_left_out_and_not_counted_passed_over() {
        let providers = Providers::none()
            .and(Fixed::registered("first", found))
            .and(Fixed::registered("second", found));
        let first = SourceId::new("first").expect("a nameable source");

        let answer = providers.first(
            &Identity::named("Echoes"),
            &Asking {
                passing: std::slice::from_ref(&first),
                ..patient()
            },
            &mut Away::default(),
        );

        assert_eq!(
            answer
                .delivered
                .as_ref()
                .map(|delivered| delivered.provider.to_string()),
            Some("second".to_owned())
        );
        assert_eq!(answer.passed_over, 0);
        assert!(answer.heard_from_every_provider());
        assert!(!answer.heard.contains(&first));
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
                ..patient()
            },
            &mut Away::default(),
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
                cancelled: &always,
                ..patient()
            },
            &mut Away::default(),
        );

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(answer.cancelled);
        assert!(answer.delivered.is_none());
    }

    fn unreachable() -> Result<Obtained> {
        Err(Error::Io {
            provider: SourceId::new("server").expect("a nameable source"),
            op: ProviderOp::Search,
            source: io::Error::from(io::ErrorKind::ConnectionRefused),
        })
    }

    fn not_found() -> Result<Obtained> {
        Err(Error::Refused {
            provider: SourceId::new("server").expect("a nameable source"),
            op: ProviderOp::Download,
            status: 404,
        })
    }

    #[test]
    fn a_provider_that_cannot_be_reached_is_passed_over_for_the_rest_of_the_poll() {
        let providers = Providers::none()
            .and(Fixed::registered("server", unreachable))
            .and(Fixed::registered("inbox", nothing));
        let mut away = Away::default();

        let first = providers.first(&Identity::named("Echoes"), &patient(), &mut away);
        let second = providers.first(&Identity::named("Time"), &patient(), &mut away);

        assert_eq!((first.refused, first.passed_over), (1, 0));
        assert_eq!((second.refused, second.passed_over), (0, 1));
        assert!(away.holds(&SourceId::new("server").expect("a nameable source")));
        assert!(!first.heard_from_every_provider());
        assert!(!second.heard_from_every_provider());
    }

    #[test]
    fn a_provider_refusing_one_want_is_asked_about_the_next() {
        let providers = Providers::none().and(Fixed::registered("server", not_found));
        let mut away = Away::default();

        let first = providers.first(&Identity::named("Echoes"), &patient(), &mut away);
        let second = providers.first(&Identity::named("Time"), &patient(), &mut away);

        assert_eq!((first.refused, second.refused), (1, 1));
        assert_eq!(second.passed_over, 0);
    }

    #[test]
    fn a_provider_left_behind_is_not_waited_on_again_in_the_same_poll() {
        let providers = Providers::none().and(silent());
        let mut away = Away::default();
        let hurried = Asking {
            within: Duration::from_millis(100),
            ..patient()
        };

        let first = providers.first(&Identity::named("Echoes"), &hurried, &mut away);
        let started = Instant::now();
        let second = providers.first(&Identity::named("Time"), &hurried, &mut away);

        assert_eq!(first.late, 1);
        assert_eq!((second.late, second.passed_over), (0, 1));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn only_an_answer_every_provider_gave_is_heard_from_every_provider() {
        let providers = Providers::none().and(Fixed::registered("inbox", nothing));

        let answer = providers.first(&Identity::named("Echoes"), &patient(), &mut Away::default());

        assert!(answer.heard_from_every_provider());
    }

    fn found_elsewhere() -> Result<Obtained> {
        Ok(Obtained::Found(Delivery::File(PathBuf::from(
            "/shop/b.flac",
        ))))
    }

    fn turned_to(providers: &Providers, asking: &Asking<'_>) -> (Answer, Vec<String>) {
        let (told, heard) = mpsc::channel();
        let turning_to = move |source: &SourceId| {
            let _ = told.send(source.as_str().to_owned());
        };
        let answer = providers.first(
            &Identity::named("Echoes"),
            &Asking {
                turning_to: &turning_to,
                ..*asking
            },
            &mut Away::default(),
        );

        (answer, heard.try_iter().collect())
    }

    #[test]
    fn each_provider_turned_to_is_told_and_the_stub_never_is() {
        let providers = Providers::none()
            .and(Fixed::registered("empty", nothing))
            .and(Fixed::registered("shop", found))
            .and(Fixed::registered("later", found));

        let (answer, heard) = turned_to(&providers, &patient());

        assert_eq!(heard, ["empty", "shop"]);
        assert_eq!(
            answer.delivered.map(|delivered| delivered.provider),
            Some(SourceId::new("shop").expect("a nameable source"))
        );
    }

    #[test]
    fn a_delivery_the_poll_declines_is_passed_over_and_the_next_provider_asked() {
        let providers = Providers::none()
            .and(Fixed::registered("inbox", found))
            .and(Fixed::registered("shop", found_elsewhere));
        let forgotten = |delivered: &Delivered| delivered.provider.as_str() == "inbox";

        let (answer, heard) = turned_to(
            &providers,
            &Asking {
                declined: &forgotten,
                ..patient()
            },
        );

        assert_eq!(heard, ["inbox", "shop"]);
        assert_eq!(answer.declined, 1);
        assert!(answer.heard_from_every_provider());
        let delivered = answer.delivered.expect("the next provider's delivery");
        assert_eq!(delivered.provider.as_str(), "shop");
        assert_eq!(delivered.taken_from().to_uri(), "file:///shop/b.flac");
    }

    #[test]
    fn a_registry_narrowed_to_one_provider_asks_it_alone_and_never_hears_from_every_provider() {
        let inbox = SourceId::new("inbox").expect("a nameable source");
        let providers = Providers::none()
            .and(Fixed::registered("inbox", nothing))
            .and(Fixed::registered("shop", found));

        let (answer, heard) = turned_to(&providers.only(&inbox), &patient());

        assert_eq!(heard, ["inbox"]);
        assert!(answer.delivered.is_none());
        assert!(answer.narrowed);
        assert!(!answer.heard_from_every_provider());

        let alone = Providers::none().and(Fixed::registered("inbox", nothing));
        let (answer, _) = turned_to(&alone.only(&inbox), &patient());
        assert!(
            answer.heard_from_every_provider(),
            "a registry holding the inbox alone left nobody out"
        );

        let absent = providers.only(&SourceId::new("server").expect("a nameable source"));
        assert!(!absent.has_a_source());
        let (answer, heard) = turned_to(&absent, &patient());
        assert!(heard.is_empty());
        assert!(!answer.heard_from_every_provider());
    }
}
