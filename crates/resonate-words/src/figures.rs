use fluent_bundle::{FluentValue, types::FluentNumber};
use icu_decimal::{
    DecimalFormatter, DecimalFormatterPreferences,
    input::Decimal,
    options::{DecimalFormatterOptions, GroupingStrategy},
};
use icu_locale_core::Locale;

use crate::regional::settled;

pub(crate) struct Figures {
    grouped: Option<DecimalFormatter>,
    ungrouped: Option<DecimalFormatter>,
}

impl Figures {
    pub(crate) fn of(locale: &Locale) -> Self {
        let formatter = |strategy: Option<GroupingStrategy>| {
            let mut options = DecimalFormatterOptions::default();
            options.grouping_strategy = strategy;

            DecimalFormatter::try_new(DecimalFormatterPreferences::from(locale), options).ok()
        };

        Self {
            grouped: formatter(None),
            ungrouped: formatter(Some(GroupingStrategy::Never)),
        }
    }

    fn fixed(&self, value: f64, fraction_digits: usize) -> String {
        let plain = format!("{value:.fraction_digits$}");

        Decimal::try_from_str(&plain)
            .ok()
            .and_then(|decimal| self.said(&decimal, true))
            .unwrap_or(plain)
    }

    fn numbered(&self, number: &FluentNumber) -> Option<String> {
        let plain = number.as_string();
        let mut decimal = Decimal::try_from_str(&plain).ok()?;
        if let Some(most) = number.options.maximum_fraction_digits {
            decimal.round(-i16::try_from(most).ok()?);
        }
        if let Some(least) = number.options.minimum_fraction_digits {
            decimal.pad_end(-i16::try_from(least).ok()?);
        }

        self.said(&decimal, number.options.use_grouping)
    }

    fn said(&self, decimal: &Decimal, grouping: bool) -> Option<String> {
        let formatter = match grouping {
            true => self.grouped.as_ref(),
            false => self.ungrouped.as_ref(),
        }?;

        Some(formatter.format(decimal).to_string())
    }
}

pub fn grouped(count: u64) -> String {
    settled()
        .figures
        .said(&Decimal::from(count), true)
        .unwrap_or_else(|| count.to_string())
}

pub fn decimal(value: f64, fraction_digits: usize) -> String {
    settled().figures.fixed(value, fraction_digits)
}

pub(crate) fn fluent<M>(value: &FluentValue<'_>, _: &M) -> Option<String> {
    let FluentValue::Number(number) = value else {
        return None;
    };

    settled().figures.numbered(number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Conventions;

    fn said_in(tag: &str, count: u64) -> String {
        let conventions = Conventions::from_tags(&[tag]);

        Figures::of(conventions.numbers())
            .said(&Decimal::from(count), true)
            .unwrap()
    }

    #[test]
    fn a_count_is_grouped_the_way_each_region_writes_it() {
        assert_eq!(said_in("en-US", 1_234_567), "1,234,567");
        assert_eq!(said_in("en-GB", 1_234_567), "1,234,567");
        assert_eq!(said_in("de-DE", 1_234_567), "1.234.567");
        assert_eq!(said_in("fr-FR", 1_234_567), "1\u{202f}234\u{202f}567");
        assert_eq!(said_in("hi-IN", 1_234_567), "12,34,567");
        assert_eq!(said_in("de-CH", 1_234_567), "1'234'567");
    }

    #[test]
    fn a_count_under_four_digits_carries_no_mark_and_some_regions_wait_for_five() {
        assert_eq!(said_in("en-US", 999), "999");
        assert_eq!(said_in("en-US", 1_000), "1,000");
        assert_eq!(said_in("es-ES", 1_000), "1000");
        assert_eq!(said_in("es-ES", 10_000), "10.000");
    }

    #[test]
    fn digits_stay_latin_whatever_the_region_writes_its_own_in() {
        assert_eq!(said_in("ar-EG", 1_234), "1,234");
        assert_eq!(said_in("hi-IN-u-nu-deva", 1_234), "1,234");
    }

    #[test]
    fn a_number_a_catalog_says_is_grouped_unless_it_asks_not_to_be() {
        let german = Conventions::from_tags(&["de-DE"]);
        let figures = Figures::of(german.numbers());

        let count = FluentNumber::from(12_345);
        assert_eq!(figures.numbered(&count), Some("12.345".to_owned()));

        let mut year = FluentNumber::from(2026);
        year.options.use_grouping = false;
        assert_eq!(figures.numbered(&year), Some("2026".to_owned()));

        assert_eq!(figures.fixed(1234.5, 1), "1.234,5");
        assert_eq!(figures.fixed(0.25, 2), "0,25");
    }

    #[test]
    fn the_american_conventions_stand_until_a_region_is_settled() {
        assert_eq!(grouped(12_345), "12,345");
        assert_eq!(decimal(1234.5, 1), "1,234.5");
    }
}
