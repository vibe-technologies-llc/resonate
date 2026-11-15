use std::fmt;

use icu_locale_core::{
    LanguageIdentifier, Locale,
    subtags::{self, Region, Script, Variants, language, region},
};

use crate::{Conventions, locale::tagged};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionTag {
    language: subtags::Language,
    script: Option<Script>,
    region: Region,
}

const NAMED: [(RegionTag, &str); RegionTag::COUNT] = [
    (
        RegionTag::of(language!("en"), region!("US")),
        "United States",
    ),
    (
        RegionTag::of(language!("en"), region!("GB")),
        "United Kingdom",
    ),
    (RegionTag::of(language!("de"), region!("DE")), "Deutschland"),
    (RegionTag::of(language!("fr"), region!("FR")), "France"),
    (RegionTag::of(language!("de"), region!("CH")), "Schweiz"),
    (RegionTag::of(language!("en"), region!("IN")), "India"),
];

impl RegionTag {
    pub const COUNT: usize = 6;

    pub const OFFERED: [Self; Self::COUNT] = {
        let mut offered = [NAMED[0].0; Self::COUNT];
        let mut at = 0;
        while at < Self::COUNT {
            offered[at] = NAMED[at].0;
            at += 1;
        }
        offered
    };

    const fn of(language: subtags::Language, region: Region) -> Self {
        Self {
            language,
            script: None,
            region,
        }
    }

    pub fn parse(spelled: &str) -> Option<Self> {
        let locale = Locale::try_from_str(&tagged(spelled)?).ok()?;
        let LanguageIdentifier {
            language,
            script,
            region,
            ..
        } = locale.id;

        Some(Self {
            language: Some(language).filter(|language| !language.is_unknown())?,
            script,
            region: region?,
        })
    }

    pub fn endonym(self) -> Option<&'static str> {
        NAMED
            .iter()
            .find(|(offered, _)| *offered == self)
            .map(|(_, endonym)| *endonym)
    }

    pub fn conventions(self) -> Conventions {
        Conventions::from_tags(&[self.to_string()])
    }

    fn identifier(self) -> LanguageIdentifier {
        LanguageIdentifier {
            language: self.language,
            script: self.script,
            region: Some(self.region),
            variants: Variants::new(),
        }
    }
}

impl fmt::Display for RegionTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.identifier(), formatter)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RegionChoice {
    #[default]
    System,
    Chosen(RegionTag),
}

impl RegionChoice {
    const SYSTEM: &str = "system";

    pub const ALL: [Self; 1 + RegionTag::COUNT] = {
        let mut all = [Self::System; 1 + RegionTag::COUNT];
        let mut at = 0;
        while at < RegionTag::COUNT {
            all[at + 1] = Self::Chosen(RegionTag::OFFERED[at]);
            at += 1;
        }
        all
    };

    pub fn parse(spelled: &str) -> Option<Self> {
        match spelled {
            Self::SYSTEM => Some(Self::System),
            tag => RegionTag::parse(tag).map(Self::Chosen),
        }
    }

    pub fn conventions(self) -> Conventions {
        match self {
            Self::System => Conventions::from_environment(),
            Self::Chosen(tag) => tag.conventions(),
        }
    }
}

impl fmt::Display for RegionChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => formatter.write_str(Self::SYSTEM),
            Self::Chosen(tag) => fmt::Display::fmt(tag, formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{day_month_and_year, grouped, settle};

    #[test]
    fn every_region_offered_is_read_back_from_its_spelling_and_named() {
        for choice in RegionChoice::ALL {
            assert_eq!(RegionChoice::parse(&choice.to_string()), Some(choice));
        }
        for offered in RegionTag::OFFERED {
            assert!(offered.endonym().is_some(), "{offered}");
        }

        assert_eq!(RegionChoice::default(), RegionChoice::System);
        assert_eq!(RegionChoice::ALL[0].to_string(), "system");
    }

    #[test]
    fn a_tag_is_read_however_a_locale_spells_it_and_keeps_its_script() {
        let german = RegionTag::parse("de-DE");

        assert_eq!(RegionTag::parse("de_DE.UTF-8"), german);
        assert_eq!(RegionTag::parse("DE-de"), german);
        assert_eq!(
            RegionTag::parse("sr-Latn-RS").map(|tag| tag.to_string()),
            Some("sr-Latn-RS".to_owned())
        );
        assert_eq!(
            RegionTag::parse("pt-BR").map(|tag| (tag.to_string(), tag.endonym())),
            Some(("pt-BR".to_owned(), None))
        );
    }

    #[test]
    fn a_tag_naming_no_region_or_no_language_is_refused() {
        for refused in [
            "de", "klingon", "C", "POSIX", "und-DE", "", "de-DE-", "12-34",
        ] {
            assert_eq!(RegionChoice::parse(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_chosen_region_settles_its_own_grouping_and_date_order() {
        settle(&RegionChoice::parse("de-DE").unwrap().conventions());
        assert_eq!(grouped(1_234_567), "1.234.567");
        assert_eq!(day_month_and_year(2026, 11, 13), "13 Nov 2026");

        settle(&RegionChoice::parse("en-IN").unwrap().conventions());
        assert_eq!(grouped(1_234_567), "12,34,567");

        settle(&RegionChoice::parse("en-US").unwrap().conventions());
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(day_month_and_year(2026, 11, 13), "Nov 13, 2026");
    }
}
