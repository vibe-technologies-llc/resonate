use std::{fmt, time::Duration};

use crate::{Mbid, folded_letters};

const KEY_SEPARATOR: char = '\u{1f}';
const MBID_PREFIX: &str = "mbid:";
const NAMED_PREFIX: &str = "named:";
const SURELY_THE_SAME_LENGTH_WITHIN: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Recording {
    pub mbid: Option<Mbid>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordingKey(Box<str>);

impl Recording {
    pub fn key(&self) -> RecordingKey {
        match &self.mbid {
            Some(mbid) => RecordingKey(format!("{MBID_PREFIX}{mbid}").into_boxed_str()),
            None => self.named_key(),
        }
    }

    pub fn named_key(&self) -> RecordingKey {
        let named = format!(
            "{NAMED_PREFIX}{}{KEY_SEPARATOR}{}{KEY_SEPARATOR}{}",
            folded_letters(self.artist.trim()),
            title_words(&self.title),
            folded_letters(self.album.trim())
        );
        RecordingKey(named.into_boxed_str())
    }

    pub fn is_named(&self) -> bool {
        !self.title.trim().is_empty()
    }

    pub fn names_the_same_song_as(&self, other: &Self) -> bool {
        if let (Some(mine), Some(theirs)) = (&self.mbid, &other.mbid) {
            return mine == theirs;
        }
        self.named_key() == other.named_key()
    }

    pub fn surely_names(&self, held: &Self) -> bool {
        if let (Some(mine), Some(theirs)) = (&self.mbid, &held.mbid) {
            return mine == theirs;
        }

        let words = title_words(&self.title);
        let album = folded_letters(self.album.trim());
        let lengths_agree = match (self.length, held.length) {
            (Some(mine), Some(theirs)) => {
                Some(mine.abs_diff(theirs) <= SURELY_THE_SAME_LENGTH_WITHIN)
            }
            _ => None,
        };

        let named_alike = !words.is_empty()
            && words == title_words(&held.title)
            && folded_letters(self.artist.trim()) == folded_letters(held.artist.trim())
            && album == folded_letters(held.album.trim());
        let told_apart = !album.is_empty() || lengths_agree.is_some();

        named_alike && told_apart && lengths_agree != Some(false)
    }
}

impl RecordingKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn title_words(text: &str) -> String {
    folded_letters(text)
        .split(|glyph: char| !glyph.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";

    fn song(title: &str, artist: &str, album: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            length: Some(Duration::from_secs(200)),
        }
    }

    #[test]
    fn a_recording_with_an_mbid_is_keyed_by_it_alone() {
        let mut first = song("Echoes", "Pink Floyd", "Meddle");
        let mut second = song("echoes (live)", "Floyd", "Pompeii");

        first.mbid = Some(Mbid::new(ECHOES).expect("an mbid"));
        second.mbid = first.mbid.clone();

        assert_eq!(first.key(), second.key());
        assert_eq!(first.key().as_str(), format!("mbid:{ECHOES}"));
    }

    #[test]
    fn a_recording_without_an_mbid_is_keyed_by_its_folded_names() {
        let typed = song("Héroes", "David  Bowie", "Heroes");
        let shouted = song("HEROES!", "david  bowie", " HEROES ");

        assert_eq!(typed.key(), shouted.key());
        assert_ne!(typed.key(), song("Heroes", "David Bowie", "Low").key());
    }

    #[test]
    fn one_side_knowing_the_mbid_still_matches_by_names() {
        let named = song("Echoes", "Pink Floyd", "Meddle");
        let mut identified = named.clone();

        identified.mbid = Some(Mbid::new(ECHOES).expect("an mbid"));

        assert!(named.names_the_same_song_as(&identified));
        assert_ne!(named.key(), identified.key());
    }

    #[test]
    fn a_deletion_surely_names_only_the_recording_its_id_or_its_album_and_length_tell() {
        let told = song("Echoes", "Pink Floyd", "Meddle");
        let mut identified = told.clone();
        let mut conflicting = told.clone();
        let mut live = told.clone();

        identified.mbid = Some(Mbid::new(ECHOES).expect("an mbid"));
        conflicting.mbid =
            Some(Mbid::new("0bd1c7a3-9f2e-4c6b-8a1d-5e7f3b2c4d6a").expect("an mbid"));
        live.length = Some(Duration::from_secs(1_380));

        assert!(told.surely_names(&song("ECHOES", "pink floyd", " Meddle ")));
        assert!(told.surely_names(&identified));
        assert!(identified.surely_names(&told));
        assert!(!identified.surely_names(&conflicting));
        assert!(!told.surely_names(&song("Echoes", "Pink Floyd", "Live at Pompeii")));
        assert!(!told.surely_names(&live));
    }

    #[test]
    fn a_deletion_naming_neither_album_length_nor_id_surely_names_nothing() {
        let bare = Recording {
            album: String::new(),
            length: None,
            ..song("Echoes", "Pink Floyd", "")
        };
        let timed = Recording {
            album: String::new(),
            ..song("Echoes", "Pink Floyd", "")
        };

        assert!(!bare.surely_names(&song("Echoes", "Pink Floyd", "")));
        assert!(!bare.surely_names(&song("Echoes", "Pink Floyd", "Meddle")));
        assert!(timed.surely_names(&song("Echoes", "Pink Floyd", "")));
        assert!(!timed.surely_names(&song("Echoes", "Pink Floyd", "Meddle")));
    }
}
