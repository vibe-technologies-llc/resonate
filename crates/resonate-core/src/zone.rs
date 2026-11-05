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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RemoteTrust {
    #[default]
    Everything,
    Transport,
    Nothing,
}

pub const REMOTE_TRUST_TITLE: &str = "Driven by your other devices";

pub const REMOTE_TRUST_ASKED: &str = "What your other devices may do here";

pub const REMOTE_TRUST_TOLD: &str = "Your other devices drive this one from their list of devices.";

impl RemoteTrust {
    pub const ALL: [Self; 3] = [Self::Everything, Self::Transport, Self::Nothing];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Everything => "Everything",
            Self::Transport => "Playback only",
            Self::Nothing => "Nothing",
        }
    }

    pub const fn meaning(self) -> &'static str {
        match self {
            Self::Everything => "Play, pause, skip, seek, set the volume and change the queue",
            Self::Transport => {
                "Play, pause, skip, seek and set the volume, but not move or remove songs"
            }
            Self::Nothing => "They cannot see or change what plays here",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Everything => "everything",
            Self::Transport => "transport",
            Self::Nothing => "nothing",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|trust| trust.as_str() == text)
    }

    pub const fn shows(self) -> bool {
        !matches!(self, Self::Nothing)
    }

    pub const fn edits_the_queue(self) -> bool {
        matches!(self, Self::Everything)
    }
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
