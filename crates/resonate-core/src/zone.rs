use std::{fmt, num::NonZeroU32};

use crate::Volume;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZoneId(NonZeroU32);

impl ZoneId {
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    pub const fn new(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    pub const fn is_first(self) -> bool {
        self.0.get() == 1
    }

    pub const fn after(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

impl fmt::Display for ZoneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

pub const LONGEST_ZONE_NAME: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ZoneName(Box<str>);

impl ZoneName {
    pub fn given(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let kept: String = trimmed
            .chars()
            .filter(|letter| !letter.is_control())
            .take(LONGEST_ZONE_NAME)
            .collect();

        Some(Self(kept.trim_end().into()))
    }

    pub fn first_given_to(zone: ZoneId) -> Self {
        Self(format!("Zone {zone}").into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ZoneName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeptZone {
    pub id: ZoneId,
    pub name: ZoneName,
    pub device: Option<Box<str>>,
    pub volume: Option<Volume>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_is_numbered_from_one_and_named_by_its_number_until_renamed() {
        let second = ZoneId::FIRST.after().expect("a second zone");

        assert!(ZoneId::FIRST.is_first());
        assert!(!second.is_first());
        assert_eq!(second.get(), 2);
        assert_eq!(ZoneId::new(0), None);
        assert_eq!(ZoneName::first_given_to(second).as_str(), "Zone 2");
    }

    #[test]
    fn a_name_given_is_trimmed_held_to_one_line_and_cut_and_a_blank_one_is_none() {
        let long = "a".repeat(LONGEST_ZONE_NAME + 10);

        assert_eq!(ZoneName::given("   "), None);
        assert_eq!(
            ZoneName::given("  Living\u{7}room ").map(|name| name.as_str().to_owned()),
            Some("Livingroom".to_owned())
        );
        assert_eq!(
            ZoneName::given(&long).map(|name| name.as_str().chars().count()),
            Some(LONGEST_ZONE_NAME)
        );
    }
}
