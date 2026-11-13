use std::{borrow::Cow, sync::OnceLock};

use fluent_bundle::{FluentArgs, FluentResource, FluentValue, concurrent::FluentBundle};
use unic_langid::LanguageIdentifier;

use crate::{Language, spoken};

pub struct Area {
    ids: &'static [&'static str],
    sources: &'static [(Language, &'static str)],
    built: [OnceLock<Built>; Language::COUNT],
}

struct Built {
    bundle: Option<FluentBundle<FluentResource>>,
    plain: Box<[OnceLock<Box<str>>]>,
}

pub struct Arg<'a>(FluentValue<'a>);

macro_rules! arguments_from {
    ($($kind:ty),+) => {
        $(
            impl From<$kind> for Arg<'_> {
                fn from(value: $kind) -> Self {
                    Self(FluentValue::from(value))
                }
            }
        )+
    };
}

arguments_from!(
    i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64, String
);

impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Self {
        Self(FluentValue::from(value))
    }
}

impl<'a> From<&'a String> for Arg<'a> {
    fn from(value: &'a String) -> Self {
        Self(FluentValue::from(value.as_str()))
    }
}

impl<'a> From<Cow<'a, str>> for Arg<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self(FluentValue::from(value))
    }
}

impl Area {
    pub const fn new(
        ids: &'static [&'static str],
        sources: &'static [(Language, &'static str)],
    ) -> Self {
        Self {
            ids,
            sources,
            built: [const { OnceLock::new() }; Language::COUNT],
        }
    }

    pub fn plain(&'static self, index: usize) -> &'static str {
        self.plain_in(spoken(), index)
    }

    pub fn formatted<const N: usize>(
        &'static self,
        index: usize,
        args: [(&str, Arg<'_>); N],
    ) -> String {
        let mut fluent = FluentArgs::with_capacity(N);
        for (name, Arg(value)) in args {
            fluent.set(name, value);
        }

        self.said_in(spoken(), index, Some(&fluent))
    }

    fn plain_in(&'static self, language: Language, index: usize) -> &'static str {
        let built = self.built_in(language);
        match built.plain.get(index) {
            Some(held) => held.get_or_init(|| self.said_in(language, index, None).into_boxed_str()),
            None => self.ids.get(index).copied().unwrap_or_default(),
        }
    }

    fn built_in(&'static self, language: Language) -> &'static Built {
        self.built[language.index()].get_or_init(|| Built {
            bundle: self.bundle_of(language),
            plain: self.ids.iter().map(|_| OnceLock::new()).collect(),
        })
    }

    fn bundle_of(&self, language: Language) -> Option<FluentBundle<FluentResource>> {
        let (_, source) = self.sources.iter().find(|(held, _)| *held == language)?;
        let identified = language
            .tag()
            .parse::<LanguageIdentifier>()
            .unwrap_or_default();
        let resource =
            FluentResource::try_new((*source).to_owned()).unwrap_or_else(|(resource, errors)| {
                tracing::warn!(%language, ?errors, "words that did not all read");
                resource
            });

        let mut bundle = FluentBundle::new_concurrent(vec![identified]);
        bundle.set_use_isolating(false);
        bundle.set_formatter(Some(crate::figures::fluent));
        if let Err(errors) = bundle.add_builtins() {
            tracing::warn!(%language, ?errors, "builtin functions that were not all taken");
        }
        if let Err(errors) = bundle.add_resource(resource) {
            tracing::warn!(%language, ?errors, "words that were not all taken");
        }
        Some(bundle)
    }

    fn said_in(
        &'static self,
        language: Language,
        index: usize,
        args: Option<&FluentArgs<'_>>,
    ) -> String {
        let Some(id) = self.ids.get(index).copied() else {
            return String::new();
        };
        let held = self
            .built_in(language)
            .bundle
            .as_ref()
            .and_then(|bundle| Some((bundle, bundle.get_message(id)?.value()?)));

        match held {
            Some((bundle, pattern)) => {
                let mut errors = Vec::new();
                let said = bundle
                    .format_pattern(pattern, args, &mut errors)
                    .into_owned();
                if !errors.is_empty() {
                    tracing::warn!(%language, id, ?errors, "a message said in part");
                }
                said
            }
            None if language != Language::English => {
                tracing::warn!(%language, id, "a message said in English, lacking in this language");
                self.said_in(Language::English, index, args)
            }
            None => id.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static QUEUE: Area = Area::new(
        &["just-now", "took-out", "from", "unwritten", "in-year"],
        &[(
            Language::English,
            "just-now = just now\ntook-out = { $count ->\n    [one] Took one track out\n   *[other] Took { $count } tracks out\n}\nfrom = { $name } from { $place }\nin-year = In { NUMBER($year, useGrouping: \"false\") }\n",
        )],
    );

    #[test]
    fn a_plain_message_is_said_once_and_kept() {
        let first = QUEUE.plain(0);
        let again = QUEUE.plain(0);

        assert_eq!(first, "just now");
        assert!(std::ptr::eq(first, again));
    }

    #[test]
    fn a_count_is_said_in_the_form_english_gives_it() {
        assert_eq!(
            QUEUE.formatted(1, [("count", Arg::from(1))]),
            "Took one track out"
        );
        assert_eq!(
            QUEUE.formatted(1, [("count", Arg::from(2))]),
            "Took 2 tracks out"
        );
        assert_eq!(
            QUEUE.formatted(1, [("count", Arg::from(0))]),
            "Took 0 tracks out"
        );
    }

    #[test]
    fn a_count_is_grouped_beside_a_year_that_asks_not_to_be() {
        assert_eq!(
            QUEUE.formatted(1, [("count", Arg::from(12_345))]),
            "Took 12,345 tracks out"
        );
        assert_eq!(
            QUEUE.formatted(1, [("count", Arg::from(1))]),
            "Took one track out"
        );
        assert_eq!(QUEUE.formatted(4, [("year", Arg::from(2026))]), "In 2026");
    }

    #[test]
    fn words_are_put_in_as_they_are_with_no_isolating_marks() {
        let name = String::from("Echoes");

        assert_eq!(
            QUEUE.formatted(
                2,
                [
                    ("name", Arg::from(&name)),
                    ("place", Arg::from("the queue"))
                ]
            ),
            "Echoes from the queue"
        );
    }

    #[test]
    fn a_message_no_language_holds_says_its_id() {
        assert_eq!(QUEUE.plain(3), "unwritten");
        assert_eq!(QUEUE.plain(9), "");
    }
}
