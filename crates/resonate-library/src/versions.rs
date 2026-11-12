use std::{cmp::Reverse, time::Duration};

use resonate_core::{
    SampleFormat, folded_letters,
    titles::{self, Rating},
};

use crate::Track;

const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionSought {
    Better,
    Rated(Rating),
}

impl VersionSought {
    pub fn offered_for(rating: Rating) -> Vec<Self> {
        let rated = match rating {
            Rating::Explicit => vec![Self::Rated(Rating::Clean)],
            Rating::Clean => vec![Self::Rated(Rating::Explicit)],
            Rating::Unmarked => vec![Self::Rated(Rating::Clean), Self::Rated(Rating::Explicit)],
        };
        let mut offered = vec![Self::Better];
        offered.extend(rated);
        offered
    }
}

pub fn rating_of(track: &Track) -> Rating {
    Rating::of(Rating::flagged(track.explicit), &track.title, None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quality {
    lossless: bool,
    bits: u8,
    rate: u32,
}

impl Quality {
    pub const fn new(lossless: bool, bits: u8, rate: u32) -> Self {
        Self {
            lossless,
            bits,
            rate,
        }
    }

    pub fn of(track: &Track) -> Self {
        Self::new(
            track.codec.is_lossless(),
            significant_bits(track.spec.format),
            track.spec.rate.hz(),
        )
    }

    pub fn is_above(self, held: Self) -> bool {
        let at_least =
            self.lossless >= held.lossless && self.bits >= held.bits && self.rate >= held.rate;
        at_least && self != held
    }

    pub fn best_first(self) -> Reverse<(bool, u8, u32)> {
        Reverse((self.lossless, self.bits, self.rate))
    }
}

const fn significant_bits(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::S16 => 16,
        SampleFormat::S24 | SampleFormat::F32 => 24,
        SampleFormat::S32 => 32,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SongHeard<'a> {
    pub title: &'a str,
    pub artist: Option<&'a str>,
    pub length: Option<Duration>,
    pub lossless: bool,
    pub rating: Rating,
}

impl<'a> SongHeard<'a> {
    pub fn of(track: &'a Track) -> Self {
        Self {
            title: &track.title,
            artist: track.artist.as_deref(),
            length: length_of(track),
            lossless: track.codec.is_lossless(),
            rating: rating_of(track),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TheSong {
    title: String,
    artist: Option<String>,
    length: Option<Duration>,
    lossless: bool,
    rating: Rating,
}

impl TheSong {
    pub fn new(heard: SongHeard<'_>) -> Self {
        Self {
            title: folded_letters(titles::bare(heard.title)),
            artist: heard
                .artist
                .map(folded_letters)
                .filter(|artist| !artist.is_empty()),
            length: heard.length,
            lossless: heard.lossless,
            rating: heard.rating,
        }
    }

    pub fn of(track: &Track) -> Self {
        Self::new(SongHeard::of(track))
    }

    pub const fn is_lossless(&self) -> bool {
        self.lossless
    }

    pub fn words(title: &str, artist: Option<&str>) -> String {
        let title = titles::bare(title);
        match artist.filter(|artist| !artist.trim().is_empty()) {
            Some(artist) => format!("{title} {artist}"),
            None => title.to_owned(),
        }
    }

    pub fn is_bettered_by(&self, heard: SongHeard<'_>) -> bool {
        !self.lossless && heard.lossless && self.names(heard)
    }

    pub fn is_rated_as(&self, wanted: Rating, rating: Rating) -> bool {
        match wanted {
            Rating::Explicit => rating == Rating::Explicit,
            Rating::Clean | Rating::Unmarked => {
                rating == Rating::Clean
                    || (rating == Rating::Unmarked && self.rating == Rating::Explicit)
            }
        }
    }

    pub fn served_as(&self, sought: VersionSought, heard: SongHeard<'_>) -> bool {
        match sought {
            VersionSought::Better => self.is_bettered_by(heard),
            VersionSought::Rated(wanted) => self.held_as(wanted, heard),
        }
    }

    pub fn held_as(&self, wanted: Rating, heard: SongHeard<'_>) -> bool {
        self.is_rated_as(wanted, heard.rating) && self.names(heard)
    }

    pub fn names(&self, heard: SongHeard<'_>) -> bool {
        let same_title = folded_letters(titles::bare(heard.title)) == self.title;
        let same_artist = match (&self.artist, heard.artist.map(folded_letters)) {
            (Some(held), Some(served)) => {
                served.contains(held.as_str()) || held.contains(served.as_str())
            }
            (None, _) => true,
            (Some(_), None) => false,
        };
        let same_length = match (self.length, heard.length) {
            (Some(held), Some(served)) => held.abs_diff(served) <= LENGTHS_AGREE_WITHIN,
            _ => true,
        };

        same_title && same_artist && same_length
    }
}

fn length_of(track: &Track) -> Option<Duration> {
    track
        .duration
        .map(|frames| frames.to_duration(track.spec.rate))
}

pub fn better_held(track: &Track, copies: Vec<Track>) -> Vec<Track> {
    let held = Quality::of(track);
    let mut better: Vec<Track> = copies
        .into_iter()
        .filter(|copy| Quality::of(copy).is_above(held))
        .collect();
    better.sort_by_key(|copy| Quality::of(copy).best_first());
    better
}

pub fn rated_held(track: &Track, wanted: Rating, found: Vec<Track>) -> Vec<Track> {
    let song = TheSong::of(track);
    found
        .into_iter()
        .filter(|copy| copy.id != track.id && song.held_as(wanted, SongHeard::of(copy)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{ChannelLayout, Frames, MediaLocation, SampleRate, StreamSpec, TrackId};

    use super::*;
    use crate::Codec;

    fn track(codec: Codec, format: SampleFormat, rate: SampleRate) -> Track {
        Track {
            explicit: None,
            id: TrackId::new(1).expect("a valid id"),
            location: MediaLocation::local("/music/Echoes.flac"),
            title: "Echoes (2011 Remaster)".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_id: None,
            album_id: None,
            track_number: None,
            disc_number: None,
            duration: Some(Frames::from_duration(Duration::from_secs(1_412), rate)),
            spec: StreamSpec::new(rate, ChannelLayout::Stereo, format),
            codec,
            replay_gain: Default::default(),
            file_size: 0,
            modified: SystemTime::UNIX_EPOCH,
            added: SystemTime::UNIX_EPOCH,
            plays: 0,
            played: None,
            span: None,
            favourite: None,
            genre: None,
            hidden: false,
            alternatives: 0,
            delivered: false,
        }
    }

    fn rated(id: u64, explicit: Option<bool>, title: &str) -> Track {
        Track {
            id: TrackId::new(id).expect("a valid id"),
            explicit,
            title: title.to_owned(),
            location: MediaLocation::local(format!("/music/{id}.flac")),
            ..track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100)
        }
    }

    fn served<'a>(title: &'a str, artist: &'a str, seconds: u64, lossless: bool) -> SongHeard<'a> {
        SongHeard {
            title,
            artist: Some(artist),
            length: Some(Duration::from_secs(seconds)),
            lossless,
            rating: Rating::Unmarked,
        }
    }

    #[test]
    fn a_copy_is_higher_quality_only_where_it_is_no_lower_on_any_count_and_higher_on_one() {
        let cd = Quality::of(&track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100));
        let mp3 = Quality::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100));
        let hi_res = Quality::of(&track(Codec::Flac, SampleFormat::S24, SampleRate::HZ_96000));
        let lossy_hi_rate =
            Quality::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_48000));

        assert!(cd.is_above(mp3));
        assert!(hi_res.is_above(cd));
        assert!(hi_res.is_above(mp3));

        assert!(!cd.is_above(cd));
        assert!(!mp3.is_above(cd));
        assert!(!lossy_hi_rate.is_above(cd));
        assert!(!cd.is_above(lossy_hi_rate));
    }

    #[test]
    fn a_served_song_betters_a_lossy_track_only_where_it_is_lossless_and_the_same_song() {
        let song = TheSong::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100));
        let lossless = TheSong::of(&track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100));

        assert!(song.is_bettered_by(served("Echoes", "Pink Floyd", 1_413, true)));
        assert!(song.is_bettered_by(served("ECHOES", "pink floyd", 1_410, true)));

        assert!(!song.is_bettered_by(served("Echoes", "Pink Floyd", 1_413, false)));
        assert!(!song.is_bettered_by(served("Echoes", "Pink Floyd", 1_300, true)));
        assert!(!song.is_bettered_by(served("Time", "Pink Floyd", 1_413, true)));
        assert!(!song.is_bettered_by(served("Echoes", "Someone Else", 1_413, true)));
        assert!(!lossless.is_bettered_by(served("Echoes", "Pink Floyd", 1_413, true)));
    }

    #[test]
    fn the_better_copies_held_are_offered_best_first_and_none_worse_than_the_track() {
        let held = track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100);
        let copies = vec![
            track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100),
            track(Codec::Flac, SampleFormat::S24, SampleRate::HZ_48000),
            track(Codec::Flac, SampleFormat::S24, SampleRate::HZ_96000),
        ];

        let better = better_held(&held, copies);

        assert_eq!(
            better.iter().map(|copy| copy.spec.rate).collect::<Vec<_>>(),
            [SampleRate::HZ_96000, SampleRate::HZ_48000]
        );
    }

    #[test]
    fn a_rating_is_offered_only_where_the_track_is_not_already_it() {
        let explicit = rated(1, Some(true), "Echoes");
        let clean = rated(2, None, "Echoes (Clean)");
        let unmarked = rated(3, None, "Echoes");

        assert_eq!(
            VersionSought::offered_for(rating_of(&explicit)),
            [VersionSought::Better, VersionSought::Rated(Rating::Clean)]
        );
        assert_eq!(
            VersionSought::offered_for(rating_of(&clean)),
            [
                VersionSought::Better,
                VersionSought::Rated(Rating::Explicit)
            ]
        );
        assert_eq!(
            VersionSought::offered_for(rating_of(&unmarked)),
            [
                VersionSought::Better,
                VersionSought::Rated(Rating::Clean),
                VersionSought::Rated(Rating::Explicit)
            ]
        );
    }

    #[test]
    fn a_clean_version_of_an_explicit_song_may_be_unmarked_and_an_explicit_one_must_be_marked() {
        let explicit = rated(1, Some(true), "Echoes");
        let found = vec![
            rated(1, Some(true), "Echoes"),
            rated(2, None, "Echoes"),
            rated(3, None, "Echoes (Clean)"),
            rated(4, Some(true), "Echoes"),
            rated(5, None, "Time"),
        ];

        let clean: Vec<u64> = rated_held(&explicit, Rating::Clean, found.clone())
            .iter()
            .map(|copy| copy.id.get())
            .collect();
        let unmarked = rated(9, None, "Echoes");
        let marked: Vec<u64> = rated_held(&unmarked, Rating::Explicit, found.clone())
            .iter()
            .map(|copy| copy.id.get())
            .collect();
        let strictly_clean: Vec<u64> = rated_held(&unmarked, Rating::Clean, found)
            .iter()
            .map(|copy| copy.id.get())
            .collect();

        assert_eq!(clean, [2, 3]);
        assert_eq!(marked, [1, 4]);
        assert_eq!(strictly_clean, [3]);
    }

    #[test]
    fn the_words_asked_are_the_bare_title_and_the_artist_where_there_is_one() {
        assert_eq!(
            TheSong::words("Echoes (2011 Remaster)", Some("Pink Floyd")),
            "Echoes Pink Floyd"
        );
        assert_eq!(TheSong::words("Echoes", Some("  ")), "Echoes");
        assert_eq!(TheSong::words("Echoes", None), "Echoes");
    }
}
