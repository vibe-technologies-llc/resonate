use std::{fmt, time::Duration};

use crate::{Mbid, folded_letters};

const KEY_SEPARATOR: char = '\u{1f}';
const MBID_PREFIX: &str = "mbid:";
const NAMED_PREFIX: &str = "named:";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Recording {
    pub mbid: Option<Mbid>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
}
