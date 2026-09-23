use std::fmt;

use crate::{Error, Result};

const MBID_LEN: usize = 36;
const MBID_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mbid(Box<str>);

impl Mbid {
    pub fn new(text: &str) -> Result<Self> {
        if text.len() != MBID_LEN {
            return Err(Error::NotAnMbid);
        }
        let mut groups = text.split('-');
        let mut lowered = String::with_capacity(MBID_LEN);
        for expected in MBID_GROUPS {
            let group = groups.next().ok_or(Error::NotAnMbid)?;
            if group.len() != expected || !group.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::NotAnMbid);
            }
            if !lowered.is_empty() {
                lowered.push('-');
            }
            lowered.push_str(&group.to_ascii_lowercase());
        }
        if groups.next().is_some() {
            return Err(Error::NotAnMbid);
        }

        Ok(Self(lowered.into_boxed_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Mbid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

const ISRC_COUNTRY_LETTERS: usize = 2;
const ISRC_REGISTRANT_CHARACTERS: usize = 3;
const ISRC_YEAR_AND_DESIGNATION_DIGITS: usize = 7;
const ISRC_REGISTRANT_ENDS: usize = ISRC_COUNTRY_LETTERS + ISRC_REGISTRANT_CHARACTERS;
const ISRC_LEN: usize = ISRC_REGISTRANT_ENDS + ISRC_YEAR_AND_DESIGNATION_DIGITS;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Isrc(Box<str>);

impl Isrc {
    pub fn new(text: &str) -> Result<Self> {
        let mut shouted = String::with_capacity(ISRC_LEN);
        for letter in text.chars().filter(|letter| *letter != '-') {
            if shouted.len() == ISRC_LEN || !belongs_at(letter, shouted.len()) {
                return Err(Error::NotAnIsrc);
            }
            shouted.push(letter.to_ascii_uppercase());
        }
        if shouted.len() != ISRC_LEN {
            return Err(Error::NotAnIsrc);
        }

        Ok(Self(shouted.into_boxed_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn belongs_at(letter: char, place: usize) -> bool {
    if place < ISRC_COUNTRY_LETTERS {
        letter.is_ascii_alphabetic()
    } else if place < ISRC_REGISTRANT_ENDS {
        letter.is_ascii_alphanumeric()
    } else {
        letter.is_ascii_digit()
    }
}

impl fmt::Display for Isrc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";

    #[test]
    fn an_mbid_is_the_dashed_uuid_text_and_reads_back_lowercased() {
        let lowered = Mbid::new(ECHOES).expect("a well-formed mbid");
        assert_eq!(lowered.as_str(), ECHOES);
        assert_eq!(lowered.to_string(), ECHOES);

        let shouted = Mbid::new(&ECHOES.to_ascii_uppercase()).expect("case is not part of an mbid");
        assert_eq!(shouted, lowered);
    }

    #[test]
    fn text_that_is_not_a_dashed_uuid_is_refused() {
        for refused in [
            "",
            "83d91898776347d7b03bb92132375c47",
            "83d91898-7763-47d7-b03b-b92132375c4",
            "83d91898-7763-47d7-b03b-b92132375c477",
            "83d91898-7763-47d7-b03b-b92132375c4g",
            "83d918987-763-47d7-b03b-b92132375c47",
            " 83d91898-7763-47d7-b03b-b92132375c4",
            "83d91898-7763-47d7-b03b-b921323-5c47",
        ] {
            assert!(
                matches!(Mbid::new(refused), Err(Error::NotAnMbid)),
                "{refused:?} was taken for an mbid"
            );
        }
    }

    #[test]
    fn an_isrc_is_twelve_characters_and_reads_back_uppercased_without_its_dashes() {
        let isrc = Isrc::new("GB-AYE-71-00389").expect("a well-formed isrc");
        assert_eq!(isrc.as_str(), "GBAYE7100389");
        assert_eq!(isrc.to_string(), "GBAYE7100389");
        assert_eq!(
            Isrc::new("gbaye7100389").expect("case is not part of an isrc"),
            isrc
        );
        assert_eq!(Isrc::new("GBAYE7100389").expect("a bare isrc"), isrc);
        assert_eq!(
            Isrc::new("-GB-AYE-71-00389-").expect("a dash is never a letter"),
            isrc
        );
        assert_eq!(
            Isrc::new("US-S1Z-99-00001")
                .expect("a registrant may hold digits")
                .as_str(),
            "USS1Z9900001"
        );
    }

    #[test]
    fn text_that_is_not_an_isrc_is_refused() {
        for refused in [
            "",
            "GBAYE710038",
            "GBAYE71003899",
            "G1AYE7100389",
            "GBAYE710038X",
            "GB AYE7100389",
            "GBAY£7100389",
            "GBAYE-7100389-0",
        ] {
            assert!(
                matches!(Isrc::new(refused), Err(Error::NotAnIsrc)),
                "{refused:?} was taken for an isrc"
            );
        }
    }
}
