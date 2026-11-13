use std::sync::OnceLock;

use icu_calendar::Date;
use icu_datetime::{
    DateTimeFormatter, DateTimeFormatterPreferences,
    fieldsets::{E, M, MD, YMD},
};
use icu_locale_core::{
    LanguageIdentifier, Locale,
    extensions::unicode::{key, value},
    subtags::Region,
};

use crate::{
    Language,
    regional::{latin_digits, settled},
    spoken,
};

const A_MONDAY: (i32, u8, u8) = (2024, 1, 1);
const THE_FIRST_OF_A_MONTH: u8 = 1;
const A_LEAP_YEAR: i32 = 2024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brevity {
    Brief,
    Full,
}

pub(crate) struct Dates {
    region: Option<Region>,
    built: [OnceLock<Built>; Language::COUNT],
}

struct Built {
    month_brief: Option<DateTimeFormatter<M>>,
    month_full: Option<DateTimeFormatter<M>>,
    weekday_brief: Option<DateTimeFormatter<E>>,
    weekday_full: Option<DateTimeFormatter<E>>,
    day_and_month_brief: Option<DateTimeFormatter<MD>>,
    day_and_month_full: Option<DateTimeFormatter<MD>>,
    day_month_and_year: Option<DateTimeFormatter<YMD>>,
}

impl Dates {
    pub(crate) fn of(region: Option<Region>) -> Self {
        Self {
            region,
            built: [const { OnceLock::new() }; Language::COUNT],
        }
    }

    fn built_in(&self, language: Language) -> &Built {
        self.built[language.index()].get_or_init(|| Built::of(&self.locale_of(language)))
    }

    fn locale_of(&self, language: Language) -> Locale {
        let mut identifier =
            LanguageIdentifier::try_from_str(language.tag()).unwrap_or(LanguageIdentifier::UNKNOWN);
        identifier.region = self.region;

        let mut locale = latin_digits(identifier);
        locale
            .extensions
            .unicode
            .keywords
            .set(key!("ca"), value!("gregory"));
        locale
    }
}

impl Built {
    fn of(locale: &Locale) -> Self {
        let preferences = || DateTimeFormatterPreferences::from(locale);

        Self {
            month_brief: DateTimeFormatter::try_new(preferences(), M::medium()).ok(),
            month_full: DateTimeFormatter::try_new(preferences(), M::long()).ok(),
            weekday_brief: DateTimeFormatter::try_new(preferences(), E::medium()).ok(),
            weekday_full: DateTimeFormatter::try_new(preferences(), E::long()).ok(),
            day_and_month_brief: DateTimeFormatter::try_new(preferences(), MD::medium()).ok(),
            day_and_month_full: DateTimeFormatter::try_new(preferences(), MD::long()).ok(),
            day_month_and_year: DateTimeFormatter::try_new(preferences(), YMD::medium()).ok(),
        }
    }
}

fn iso(year: i64, month: i64, day: i64) -> Option<Date<icu_calendar::Iso>> {
    Date::try_new_iso(
        i32::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()
}

fn numeric(year: i64, month: i64, day: i64) -> String {
    format!("{year}-{month:02}-{day:02}")
}

macro_rules! said_by {
    ($field:ident, $year:expr, $month:expr, $day:expr) => {{
        let settled = settled();
        let built = settled.dates.built_in(spoken());

        match (&built.$field, iso($year, $month, $day)) {
            (Some(formatter), Some(date)) => formatter.format(&date).to_string(),
            _ => numeric($year, $month, $day),
        }
    }};
}

pub fn month_name(brevity: Brevity, month: i64) -> String {
    match brevity {
        Brevity::Brief => said_by!(
            month_brief,
            A_LEAP_YEAR.into(),
            month,
            THE_FIRST_OF_A_MONTH.into()
        ),
        Brevity::Full => said_by!(
            month_full,
            A_LEAP_YEAR.into(),
            month,
            THE_FIRST_OF_A_MONTH.into()
        ),
    }
}

pub fn weekday_name(brevity: Brevity, days_after_monday: usize) -> String {
    let (year, month, day) = A_MONDAY;
    let day = i64::from(day) + i64::try_from(days_after_monday).unwrap_or_default();

    match brevity {
        Brevity::Brief => said_by!(weekday_brief, year.into(), month.into(), day),
        Brevity::Full => said_by!(weekday_full, year.into(), month.into(), day),
    }
}

pub fn day_and_month(brevity: Brevity, month: i64, day: i64) -> String {
    match brevity {
        Brevity::Brief => said_by!(day_and_month_brief, A_LEAP_YEAR.into(), month, day),
        Brevity::Full => said_by!(day_and_month_full, A_LEAP_YEAR.into(), month, day),
    }
}

pub fn day_month_and_year(year: i64, month: i64, day: i64) -> String {
    said_by!(day_month_and_year, year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Conventions;

    fn in_region(tag: &str) -> Dates {
        Dates::of(Conventions::from_tags(&[tag]).dates_region())
    }

    fn day_month_year_in(tag: &str) -> String {
        let dates = in_region(tag);
        let built = dates.built_in(Language::English);
        let date = iso(2026, 11, 13).unwrap();

        built
            .day_month_and_year
            .as_ref()
            .unwrap()
            .format(&date)
            .to_string()
    }

    fn day_and_month_in(tag: &str, brevity: Brevity) -> String {
        let dates = in_region(tag);
        let built = dates.built_in(Language::English);
        let date = iso(2024, 11, 13).unwrap();
        let formatter = match brevity {
            Brevity::Brief => &built.day_and_month_brief,
            Brevity::Full => &built.day_and_month_full,
        };

        formatter.as_ref().unwrap().format(&date).to_string()
    }

    #[test]
    fn a_date_is_ordered_the_way_the_region_writes_it_in_the_language_spoken() {
        assert_eq!(day_month_year_in("en-US"), "Nov 13, 2026");
        assert_eq!(day_month_year_in("en-GB"), "13 Nov 2026");
        assert_eq!(day_month_year_in("de-DE"), "13 Nov 2026");
        assert_eq!(day_month_year_in("fr-FR"), "13 Nov 2026");
        assert_eq!(day_month_year_in("hi-IN"), "13 Nov 2026");
    }

    #[test]
    fn a_day_and_month_is_worded_in_the_language_spoken_ordered_by_the_region() {
        assert_eq!(day_and_month_in("en-US", Brevity::Full), "November 13");
        assert_eq!(day_and_month_in("en-GB", Brevity::Full), "13 November");
        assert_eq!(day_and_month_in("en-US", Brevity::Brief), "Nov 13");
        assert_eq!(day_and_month_in("en-GB", Brevity::Brief), "13 Nov");
    }

    #[test]
    fn month_and_weekday_names_come_from_the_language_whatever_the_region() {
        let dates = in_region("de-DE");
        let built = dates.built_in(Language::English);
        let say = |formatter: &Option<DateTimeFormatter<M>>, month| {
            formatter
                .as_ref()
                .unwrap()
                .format(&iso(2024, month, 1).unwrap())
                .to_string()
        };

        assert_eq!(say(&built.month_brief, 11), "Nov");
        assert_eq!(say(&built.month_full, 11), "November");

        let say_day = |formatter: &Option<DateTimeFormatter<E>>, day| {
            formatter
                .as_ref()
                .unwrap()
                .format(&iso(2024, 1, day).unwrap())
                .to_string()
        };
        assert_eq!(say_day(&built.weekday_brief, 1), "Mon");
        assert_eq!(say_day(&built.weekday_full, 7), "Sunday");
    }

    #[test]
    fn the_calendar_stays_gregorian_in_a_region_that_keeps_another() {
        assert!(day_month_year_in("th-TH").contains("2026"));
    }

    #[test]
    fn a_date_no_calendar_holds_is_written_by_its_figures() {
        assert_eq!(numeric(2026, 13, 40), "2026-13-40");
        assert_eq!(month_name(Brevity::Brief, 13), "2024-13-01");
        assert_eq!(day_month_and_year(2026, 11, 13), "Nov 13, 2026");
    }
}
