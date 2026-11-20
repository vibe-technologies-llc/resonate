use std::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

use unic_langid::{CharacterDirection, LanguageIdentifier};

static SPOKEN: AtomicUsize = AtomicUsize::new(Language::English.index());

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    #[default]
    English,
    French,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::French];
    pub const COUNT: usize = Self::ALL.len();

    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::French => "fr",
        }
    }

    pub const fn endonym(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::French => "Français",
        }
    }

    pub const fn by_tag(tag: &str) -> Self {
        match tag.as_bytes() {
            b"en" => Self::English,
            b"fr" => Self::French,
            _ => panic!("no language carries this tag"),
        }
    }

    pub fn parse(tag: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.tag().eq_ignore_ascii_case(tag))
    }

    pub fn is_right_to_left(self) -> bool {
        is_right_to_left(&self.identifier())
    }

    pub(crate) fn identifier(self) -> LanguageIdentifier {
        self.tag().parse().unwrap_or_default()
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

pub(crate) fn is_right_to_left(identifier: &LanguageIdentifier) -> bool {
    identifier.character_direction() == CharacterDirection::RTL
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.tag())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LanguageChoice {
    #[default]
    System,
    Chosen(Language),
}

impl LanguageChoice {
    const SYSTEM: &str = "system";

    pub const ALL: [Self; 1 + Language::COUNT] = [
        Self::System,
        Self::Chosen(Language::English),
        Self::Chosen(Language::French),
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => Self::SYSTEM,
            Self::Chosen(language) => language.tag(),
        }
    }

    pub fn parse(spelled: &str) -> Option<Self> {
        match spelled {
            Self::SYSTEM => Some(Self::System),
            tag => Language::parse(tag).map(Self::Chosen),
        }
    }

    pub fn language(self, the_listener_reads: &[String]) -> Language {
        match self {
            Self::System => negotiated(the_listener_reads),
            Self::Chosen(language) => language,
        }
    }
}

impl fmt::Display for LanguageChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub fn negotiated(requested: &[impl AsRef<str>]) -> Language {
    requested
        .iter()
        .filter_map(|tag| primary_of(tag.as_ref()))
        .find_map(|primary| Language::parse(&primary))
        .unwrap_or_default()
}

fn primary_of(tag: &str) -> Option<String> {
    let primary = tag.split(['-', '_', '.', '@']).next()?.to_ascii_lowercase();

    (!primary.is_empty()).then_some(primary)
}

pub fn speak(language: Language) {
    SPOKEN.store(language.index(), Ordering::Release);
}

pub fn spoken() -> Language {
    Language::ALL
        .get(SPOKEN.load(Ordering::Acquire))
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_is_read_back_from_its_spelling() {
        for choice in LanguageChoice::ALL {
            assert_eq!(LanguageChoice::parse(choice.as_str()), Some(choice));
        }

        assert_eq!(LanguageChoice::parse("klingon"), None);
        assert_eq!(LanguageChoice::default(), LanguageChoice::System);
    }

    #[test]
    fn a_listener_is_answered_in_the_first_language_they_read_that_is_spoken_here() {
        assert_eq!(negotiated(&["en-GB", "ja-JP"]), Language::English);
        assert_eq!(
            negotiated(&["ja_JP.UTF-8", "en_US.UTF-8"]),
            Language::English
        );
        assert_eq!(negotiated(&["de-AT"]), Language::English);
        assert_eq!(
            negotiated(&["ja-JP", "fr_CA.UTF-8", "en"]),
            Language::French
        );
        assert_eq!(negotiated(&[] as &[&str]), Language::English);
    }

    #[test]
    fn a_chosen_language_outranks_the_systems() {
        let reads = ["fr-CA".to_owned()];

        assert_eq!(
            LanguageChoice::Chosen(Language::English).language(&reads),
            Language::English
        );
        assert_eq!(LanguageChoice::System.language(&reads), Language::French);
    }

    #[test]
    fn every_language_is_found_by_its_tag() {
        for language in Language::ALL {
            assert_eq!(Language::by_tag(language.tag()), language);
            assert_eq!(Language::parse(language.tag()), Some(language));
            assert_eq!(Language::ALL[language.index()], language);
        }
    }
}
