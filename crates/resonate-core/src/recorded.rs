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
        RecordingKey::named(&self.artist, &self.title, &self.album)
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
        let album = title_words(&self.album);
        let lengths_agree = match (self.length, held.length) {
            (Some(mine), Some(theirs)) => {
                Some(mine.abs_diff(theirs) <= SURELY_THE_SAME_LENGTH_WITHIN)
            }
            _ => None,
        };

        let named_alike = !words.is_empty()
            && words == title_words(&held.title)
            && title_words(&self.artist) == title_words(&held.artist)
            && album == title_words(&held.album);
        let told_apart = !album.is_empty() || lengths_agree.is_some();

        named_alike && told_apart && lengths_agree != Some(false)
    }
}

impl RecordingKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn refolded(stored: &str) -> Self {
        let parts = stored
            .strip_prefix(NAMED_PREFIX)
            .map(|named| named.split(KEY_SEPARATOR).collect::<Vec<_>>());

        match parts.as_deref() {
            Some([artist, title, album]) => Self::named(artist, title, album),
            _ => RecordingKey(stored.into()),
        }
    }

    fn named(artist: &str, title: &str, album: &str) -> Self {
        let named = format!(
            "{NAMED_PREFIX}{}{KEY_SEPARATOR}{}{KEY_SEPARATOR}{}",
            title_words(artist),
            title_words(title),
            title_words(album)
        );
        RecordingKey(named.into_boxed_str())
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

    const SLOW_WATER: &str = "83d91898-7763-47d7-b03b-b92132375c47";

    const NAMES_FOLDED_BOTH_WAYS: [&str; 24] = [
        "Rum N' Raisin",
        "  Rum  N Raisin ",
        "AB/CD",
        "Silvo Mór",
        "Sölvi",
        "Nöble Crüe",
        "Céleste",
        "Ænima",
        "Die Straße",
        "Œuvres complètes",
        "İstanbul",
        "ǅemal",
        "ﬁfty ﬁfty",
        "坂本龍一",
        "Сплин",
        "Ἀθῆναι",
        "हिन्दी गाना",
        "தமிழ் பாடல்",
        "ข้าวเหนียว",
        "한국 노래",
        "Café del Mar (Vol. 1)",
        "Ⅻ Monkeys ½",
        "",
        "!!!",
    ];

    fn kept_under_the_earlier_fold(recording: &Recording) -> String {
        format!(
            "{NAMED_PREFIX}{}{KEY_SEPARATOR}{}{KEY_SEPARATOR}{}",
            folded_letters(recording.artist.trim()),
            title_words(&recording.title),
            folded_letters(recording.album.trim())
        )
    }

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
        let mut first = song("Slow Water", "Example Band", "Low Tide");
        let mut second = song("slow water (live)", "Band", "Harbour");

        first.mbid = Some(Mbid::new(SLOW_WATER).expect("an mbid"));
        second.mbid = first.mbid.clone();

        assert_eq!(first.key(), second.key());
        assert_eq!(first.key().as_str(), format!("mbid:{SLOW_WATER}"));
    }

    #[test]
    fn a_recording_without_an_mbid_is_keyed_by_its_folded_names() {
        let typed = song("Faról", "Another  Band", "Farol");
        let shouted = song("FAROL!", "another  band", " FAROL ");

        assert_eq!(typed.key(), shouted.key());
        assert_ne!(typed.key(), song("Farol", "Another Band", "Tide").key());
    }

    #[test]
    fn a_recording_is_keyed_alike_whatever_punctuation_and_spacing_its_names_carry() {
        let quoted = song("Harbour City", "Rum N' Raisin", "Hunger for Reasons");
        let plain = song("Harbour City", "Rum N Raisin", "Hunger for Reasons!");
        let spaced = song("Harbour City", "  rum  n'  raisin ", "Hunger  for Reasons");

        assert_eq!(quoted.named_key(), plain.named_key());
        assert_eq!(quoted.named_key(), spaced.named_key());
        assert_eq!(
            song("Lightning Rod", "AB/CD", "").key(),
            song("Lightning Rod", "AB CD", "").key()
        );
        assert_ne!(
            quoted.named_key(),
            song("Harbour City", "Rum N Raisin", "Lies").named_key()
        );
    }

    #[test]
    fn a_key_kept_under_the_earlier_fold_is_refolded_to_the_key_its_names_make_now() {
        for artist in NAMES_FOLDED_BOTH_WAYS {
            for album in NAMES_FOLDED_BOTH_WAYS {
                let recording = song("Faról!", artist, album);

                let kept = kept_under_the_earlier_fold(&recording);

                assert_eq!(
                    RecordingKey::refolded(&kept),
                    recording.named_key(),
                    "{artist:?} on {album:?}"
                );
                assert_eq!(
                    RecordingKey::refolded(recording.named_key().as_str()),
                    recording.named_key()
                );
            }
        }
    }

    #[test]
    fn a_key_that_names_no_song_by_its_names_is_left_as_it_was_kept() {
        let identified = format!("mbid:{SLOW_WATER}");
        let torn = format!("{NAMED_PREFIX}a{KEY_SEPARATOR}b");
        let overfull = format!("{NAMED_PREFIX}a{KEY_SEPARATOR}b{KEY_SEPARATOR}c{KEY_SEPARATOR}d");

        assert_eq!(RecordingKey::refolded(&identified).as_str(), identified);
        assert_eq!(RecordingKey::refolded(&torn).as_str(), torn);
        assert_eq!(RecordingKey::refolded(&overfull).as_str(), overfull);
        assert_eq!(RecordingKey::refolded("").as_str(), "");
    }

    #[test]
    fn one_side_knowing_the_mbid_still_matches_by_names() {
        let named = song("Slow Water", "Example Band", "Low Tide");
        let mut identified = named.clone();

        identified.mbid = Some(Mbid::new(SLOW_WATER).expect("an mbid"));

        assert!(named.names_the_same_song_as(&identified));
        assert_ne!(named.key(), identified.key());
    }

    #[test]
    fn a_deletion_surely_names_only_the_recording_its_id_or_its_album_and_length_tell() {
        let told = song("Slow Water", "Example Band", "Low Tide");
        let mut identified = told.clone();
        let mut conflicting = told.clone();
        let mut live = told.clone();

        identified.mbid = Some(Mbid::new(SLOW_WATER).expect("an mbid"));
        conflicting.mbid =
            Some(Mbid::new("0bd1c7a3-9f2e-4c6b-8a1d-5e7f3b2c4d6a").expect("an mbid"));
        live.length = Some(Duration::from_secs(1_380));

        assert!(told.surely_names(&song("SLOW WATER", "example band", " Low Tide ")));
        assert!(told.surely_names(&identified));
        assert!(identified.surely_names(&told));
        assert!(!identified.surely_names(&conflicting));
        assert!(!told.surely_names(&song("Slow Water", "Example Band", "Live at Harbour")));
        assert!(!told.surely_names(&live));
    }

    #[test]
    fn a_deletion_surely_names_a_copy_whose_artist_and_album_differ_only_in_punctuation() {
        let told = song("Harbour City", "Rum N' Raisin", "Hunger for Reasons");

        assert!(told.surely_names(&song("Harbour City", "Rum N Raisin", "Hunger For Reasons!")));
        assert!(!told.surely_names(&song("Harbour City", "Rum N Raisin", "Lies")));
    }

    #[test]
    fn a_deletion_naming_neither_album_length_nor_id_surely_names_nothing() {
        let bare = Recording {
            album: String::new(),
            length: None,
            ..song("Slow Water", "Example Band", "")
        };
        let timed = Recording {
            album: String::new(),
            ..song("Slow Water", "Example Band", "")
        };

        assert!(!bare.surely_names(&song("Slow Water", "Example Band", "")));
        assert!(!bare.surely_names(&song("Slow Water", "Example Band", "Low Tide")));
        assert!(timed.surely_names(&song("Slow Water", "Example Band", "")));
        assert!(!timed.surely_names(&song("Slow Water", "Example Band", "Low Tide")));
    }
}
