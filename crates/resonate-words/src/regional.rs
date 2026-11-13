use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
};

use icu_locale_core::{
    LanguageIdentifier, Locale,
    extensions::unicode::{key, value},
    subtags::Region,
};
use parking_lot::RwLock;

use crate::{dates::Dates, figures::Figures, locale::tagged};

const THE_DEFAULT_REGION: &str = "en-US";

static CURRENT: LazyLock<RwLock<Conventions>> =
    LazyLock::new(|| RwLock::new(Conventions::default()));
static GENERATION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static BUILT: RefCell<Option<(u64, Rc<Settled>)>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conventions {
    numbers: Locale,
    dates: Option<Region>,
}

pub(crate) struct Settled {
    pub(crate) figures: Figures,
    pub(crate) dates: Dates,
}

impl Settled {
    fn of(conventions: &Conventions) -> Self {
        Self {
            figures: Figures::of(&conventions.numbers),
            dates: Dates::of(conventions.dates),
        }
    }
}

impl Default for Conventions {
    fn default() -> Self {
        Self::from_tags(&[THE_DEFAULT_REGION])
    }
}

impl Conventions {
    pub fn from_tags(tags: &[impl AsRef<str>]) -> Self {
        let first = tags.iter().find_map(|tag| parsed(tag.as_ref()));

        Self::from_locales(first.clone(), first)
    }

    pub fn from_environment() -> Self {
        let (numbers, dates) = named_by_the_system();

        Self::from_locales(
            numbers.as_deref().and_then(parsed),
            dates.as_deref().and_then(parsed),
        )
    }

    pub fn numbers(&self) -> &Locale {
        &self.numbers
    }

    pub fn dates_region(&self) -> Option<Region> {
        self.dates
    }

    fn from_locales(numbers: Option<Locale>, dates: Option<Locale>) -> Self {
        let standard = || parsed(THE_DEFAULT_REGION).unwrap_or(Locale::UNKNOWN);

        Self {
            numbers: numbers.unwrap_or_else(standard),
            dates: dates.map_or_else(|| standard().id.region, |dates| dates.id.region),
        }
    }
}

pub fn settle(conventions: &Conventions) {
    *CURRENT.write() = conventions.clone();
    GENERATION.fetch_add(1, Ordering::Release);
}

pub fn settled_conventions() -> Conventions {
    CURRENT.read().clone()
}

pub(crate) fn settled() -> Rc<Settled> {
    let generation = GENERATION.load(Ordering::Acquire);

    BUILT.with(|built| {
        let mut built = built.borrow_mut();
        match built.as_ref() {
            Some((held, settled)) if *held == generation => Rc::clone(settled),
            _ => {
                let settled = Rc::new(Settled::of(&CURRENT.read()));
                *built = Some((generation, Rc::clone(&settled)));
                settled
            }
        }
    })
}

pub(crate) fn latin_digits(identifier: LanguageIdentifier) -> Locale {
    let mut locale = Locale::from(identifier);
    locale
        .extensions
        .unicode
        .keywords
        .set(key!("nu"), value!("latn"));
    locale
}

fn parsed(tag: &str) -> Option<Locale> {
    let tag = tagged(tag)?;
    let locale = Locale::try_from_str(&tag).ok()?;
    let mut identifier = locale.id;
    identifier.variants.clear();

    Some(latin_digits(identifier))
}

#[cfg(not(any(windows, target_os = "macos")))]
const NUMBERS_NAMED_IN: [&str; 3] = ["LC_ALL", "LC_NUMERIC", "LANG"];
#[cfg(not(any(windows, target_os = "macos")))]
const DATES_NAMED_IN: [&str; 3] = ["LC_ALL", "LC_TIME", "LANG"];

#[cfg(not(any(windows, target_os = "macos")))]
fn named_by_the_system() -> (Option<String>, Option<String>) {
    named_in(|variable| std::env::var(variable).ok())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn named_in(environment: impl Fn(&str) -> Option<String>) -> (Option<String>, Option<String>) {
    let first = |variables: [&str; 3]| {
        variables
            .into_iter()
            .find_map(|variable| environment(variable).filter(|value| !value.is_empty()))
    };

    (first(NUMBERS_NAMED_IN), first(DATES_NAMED_IN))
}

#[cfg(any(windows, target_os = "macos"))]
fn named_by_the_system() -> (Option<String>, Option<String>) {
    let locale = sys_locale::get_locale();

    (locale.clone(), locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(conventions: &Conventions) -> String {
        let mut identifier = conventions.numbers().id.clone();
        identifier.variants.clear();
        identifier.to_string()
    }

    #[test]
    fn a_phone_names_its_conventions_by_the_first_tag_it_passes() {
        let hindi = Conventions::from_tags(&["hi-IN-u-nu-deva", "en-US"]);

        assert_eq!(tag(&hindi), "hi-IN");
        assert_eq!(
            hindi.dates_region().map(|region| region.to_string()),
            Some("IN".to_owned())
        );
        assert_eq!(
            hindi.numbers().extensions.unicode.keywords.get(&key!("nu")),
            Some(&value!("latn"))
        );
    }

    #[test]
    fn no_tag_leaves_the_american_conventions() {
        assert_eq!(
            Conventions::from_tags(&[] as &[&str]),
            Conventions::default()
        );
        assert_eq!(tag(&Conventions::default()), "en-US");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn environment(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<String> {
        move |variable| {
            pairs
                .iter()
                .find(|(name, _)| *name == variable)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn numbers_and_dates_each_read_their_own_variable_before_lang() {
        let (numbers, dates) = named_in(environment(&[
            ("LANG", "en_US.UTF-8"),
            ("LC_NUMERIC", "de_DE.UTF-8"),
            ("LC_TIME", "en_GB.UTF-8"),
        ]));

        assert_eq!(numbers.as_deref(), Some("de_DE.UTF-8"));
        assert_eq!(dates.as_deref(), Some("en_GB.UTF-8"));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn lc_all_outranks_every_other_variable_and_an_empty_one_is_passed_over() {
        let (numbers, dates) = named_in(environment(&[
            ("LC_ALL", "fr_FR.UTF-8"),
            ("LC_NUMERIC", "de_DE.UTF-8"),
            ("LC_TIME", "de_DE.UTF-8"),
        ]));
        assert_eq!(numbers.as_deref(), Some("fr_FR.UTF-8"));
        assert_eq!(dates.as_deref(), Some("fr_FR.UTF-8"));

        let (numbers, dates) = named_in(environment(&[("LC_ALL", ""), ("LANG", "hi_IN.UTF-8")]));
        assert_eq!(numbers.as_deref(), Some("hi_IN.UTF-8"));
        assert_eq!(dates.as_deref(), Some("hi_IN.UTF-8"));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn the_plain_locale_or_none_leaves_the_american_conventions() {
        for named in [Some("C"), Some("POSIX"), Some("C.UTF-8"), None] {
            let from = Conventions::from_locales(named.and_then(parsed), named.and_then(parsed));

            assert_eq!(from, Conventions::default());
        }
    }
}
