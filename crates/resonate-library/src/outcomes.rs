use crate::EnrichStats;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Albums,
    Tracks,
    Artists,
    Pictures,
    Words,
    Studies,
    Trouble,
}

impl Section {
    pub const ALL: [Self; 7] = [
        Self::Albums,
        Self::Tracks,
        Self::Artists,
        Self::Pictures,
        Self::Words,
        Self::Studies,
        Self::Trouble,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Albums => "Albums",
            Self::Tracks => "Tracks",
            Self::Artists => "Artists",
            Self::Pictures => "Pictures",
            Self::Words => "Lyrics and songs",
            Self::Studies => "Studies",
            Self::Trouble => "Trouble",
        }
    }

    pub fn outcomes(self) -> impl Iterator<Item = Outcome> {
        Outcome::ALL
            .into_iter()
            .filter(move |outcome| outcome.section() == self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Albums,
    Releases,
    Groups,
    Matched,
    AlbumsUnmatched,
    AlbumsRejected,
    Tracks,
    Named,
    ByIsrc,
    ByRecording,
    BySearch,
    ByFingerprint,
    TracksUnmatched,
    Artists,
    Profiles,
    ArtistsUnmatched,
    ReleasesFound,
    Covers,
    CoversMissing,
    Portraits,
    PortraitsMissing,
    Lyrics,
    LyricsMissing,
    Songs,
    Studied,
    Fakes,
    Recognised,
    Misnamed,
    Refused,
}

impl Outcome {
    pub const ALL: [Self; 29] = [
        Self::Albums,
        Self::Releases,
        Self::Groups,
        Self::Matched,
        Self::AlbumsUnmatched,
        Self::AlbumsRejected,
        Self::Tracks,
        Self::Named,
        Self::ByIsrc,
        Self::ByRecording,
        Self::BySearch,
        Self::ByFingerprint,
        Self::TracksUnmatched,
        Self::Artists,
        Self::Profiles,
        Self::ArtistsUnmatched,
        Self::ReleasesFound,
        Self::Covers,
        Self::CoversMissing,
        Self::Portraits,
        Self::PortraitsMissing,
        Self::Lyrics,
        Self::LyricsMissing,
        Self::Songs,
        Self::Studied,
        Self::Fakes,
        Self::Recognised,
        Self::Misnamed,
        Self::Refused,
    ];

    pub const fn section(self) -> Section {
        match self {
            Self::Albums
            | Self::Releases
            | Self::Groups
            | Self::Matched
            | Self::AlbumsUnmatched
            | Self::AlbumsRejected => Section::Albums,
            Self::Tracks
            | Self::Named
            | Self::ByIsrc
            | Self::ByRecording
            | Self::BySearch
            | Self::ByFingerprint
            | Self::TracksUnmatched => Section::Tracks,
            Self::Artists | Self::Profiles | Self::ArtistsUnmatched | Self::ReleasesFound => {
                Section::Artists
            }
            Self::Covers | Self::CoversMissing | Self::Portraits | Self::PortraitsMissing => {
                Section::Pictures
            }
            Self::Lyrics | Self::LyricsMissing | Self::Songs => Section::Words,
            Self::Studied | Self::Fakes | Self::Recognised | Self::Misnamed => Section::Studies,
            Self::Refused => Section::Trouble,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Albums => "albums",
            Self::Releases => "releases",
            Self::Groups => "groups",
            Self::Matched => "matched",
            Self::AlbumsUnmatched => "albums_unmatched",
            Self::AlbumsRejected => "albums_rejected",
            Self::Tracks => "tracks",
            Self::Named => "named",
            Self::ByIsrc => "by_isrc",
            Self::ByRecording => "by_recording",
            Self::BySearch => "by_search",
            Self::ByFingerprint => "by_fingerprint",
            Self::TracksUnmatched => "tracks_unmatched",
            Self::Artists => "artists",
            Self::Profiles => "profiles",
            Self::ArtistsUnmatched => "artists_unmatched",
            Self::ReleasesFound => "releases_found",
            Self::Covers => "covers",
            Self::CoversMissing => "covers_missing",
            Self::Portraits => "portraits",
            Self::PortraitsMissing => "portraits_missing",
            Self::Lyrics => "lyrics",
            Self::LyricsMissing => "lyrics_missing",
            Self::Songs => "songs",
            Self::Studied => "studied",
            Self::Fakes => "fakes",
            Self::Recognised => "recognised",
            Self::Misnamed => "misnamed",
            Self::Refused => "refused",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Albums => "asked about",
            Self::Releases => "matched to a release",
            Self::Groups => "matched to a release group alone",
            Self::Matched => "tracks paired with a release's rows",
            Self::AlbumsUnmatched => "with no strict match",
            Self::AlbumsRejected => "left alone, the release being one you refused",
            Self::Tracks => "asked about",
            Self::Named => "renamed to what the reference calls them",
            Self::ByIsrc => "identified by their ISRC",
            Self::ByRecording => "identified by their recording id",
            Self::BySearch => "identified by a search",
            Self::ByFingerprint => "identified by their sound",
            Self::TracksUnmatched => "with no strict match",
            Self::Artists => "asked about",
            Self::Profiles => "answered",
            Self::ArtistsUnmatched => "with no exact match",
            Self::ReleasesFound => "releases found on their discographies",
            Self::Covers => "covers fetched",
            Self::CoversMissing => "albums the archive holds no cover for",
            Self::Portraits => "portraits fetched",
            Self::PortraitsMissing => "artists with no portrait to fetch",
            Self::Lyrics => "tracks given lyrics",
            Self::LyricsMissing => "tracks the lyric services hold none for",
            Self::Songs => "songs of albums not held learnt",
            Self::Studied => "tracks studied",
            Self::Fakes => "found fake lossless",
            Self::Recognised => "recognised by sound",
            Self::Misnamed => "recognised as another song than named",
            Self::Refused => "requests a service refused or answered unreadably",
        }
    }
}

impl EnrichStats {
    pub const fn of(&self, outcome: Outcome) -> u64 {
        match outcome {
            Outcome::Albums => self.albums,
            Outcome::Releases => self.releases,
            Outcome::Groups => self.groups,
            Outcome::Matched => self.matched,
            Outcome::AlbumsUnmatched => self.albums_unmatched,
            Outcome::AlbumsRejected => self.albums_rejected,
            Outcome::Tracks => self.tracks,
            Outcome::Named => self.named,
            Outcome::ByIsrc => self.by_isrc,
            Outcome::ByRecording => self.by_recording,
            Outcome::BySearch => self.by_search,
            Outcome::ByFingerprint => self.by_fingerprint,
            Outcome::TracksUnmatched => self.tracks_unmatched,
            Outcome::Artists => self.artists,
            Outcome::Profiles => self.profiles,
            Outcome::ArtistsUnmatched => self.artists_unmatched,
            Outcome::ReleasesFound => self.releases_found,
            Outcome::Covers => self.covers,
            Outcome::CoversMissing => self.covers_missing,
            Outcome::Portraits => self.portraits,
            Outcome::PortraitsMissing => self.portraits_missing,
            Outcome::Lyrics => self.lyrics,
            Outcome::LyricsMissing => self.lyrics_missing,
            Outcome::Songs => self.songs,
            Outcome::Studied => self.studied,
            Outcome::Fakes => self.fakes,
            Outcome::Recognised => self.recognised,
            Outcome::Misnamed => self.misnamed,
            Outcome::Refused => self.refused,
        }
    }

    pub fn is_blank(&self) -> bool {
        Outcome::ALL
            .into_iter()
            .all(|outcome| self.of(outcome) == 0)
    }

    pub fn told_by_section(&self) -> impl Iterator<Item = (Section, Vec<(Outcome, u64)>)> + '_ {
        Section::ALL.into_iter().filter_map(|section| {
            let counted: Vec<(Outcome, u64)> = section
                .outcomes()
                .map(|outcome| (outcome, self.of(outcome)))
                .filter(|(_, count)| *count > 0)
                .collect();
            (!counted.is_empty()).then_some((section, counted))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn distinct() -> EnrichStats {
        EnrichStats {
            albums: 1,
            releases: 2,
            matched: 3,
            covers: 4,
            tracks: 5,
            named: 6,
            artists: 7,
            portraits: 8,
            releases_found: 9,
            refused: 10,
            studied: 11,
            fakes: 12,
            recognised: 13,
            misnamed: 14,
            lyrics: 15,
            songs: 16,
            groups: 17,
            albums_unmatched: 18,
            albums_rejected: 19,
            by_isrc: 20,
            by_recording: 21,
            by_search: 22,
            by_fingerprint: 23,
            tracks_unmatched: 24,
            profiles: 25,
            artists_unmatched: 26,
            covers_missing: 27,
            portraits_missing: 28,
            lyrics_missing: 29,
        }
    }

    #[test]
    fn every_counter_is_one_outcome_and_every_outcome_one_counter() {
        let stats = distinct();

        let counts: HashSet<u64> = Outcome::ALL
            .into_iter()
            .map(|outcome| stats.of(outcome))
            .collect();
        let keys: HashSet<&str> = Outcome::ALL.into_iter().map(Outcome::key).collect();
        let labels: HashSet<(Section, &str)> = Outcome::ALL
            .into_iter()
            .map(|outcome| (outcome.section(), outcome.label()))
            .collect();

        assert_eq!(counts.len(), Outcome::ALL.len());
        assert_eq!(keys.len(), Outcome::ALL.len());
        assert_eq!(labels.len(), Outcome::ALL.len());
        assert_eq!(counts, (1..=29).collect());
    }

    #[test]
    fn every_section_holds_its_outcomes_and_every_outcome_is_in_one_section() {
        let held: usize = Section::ALL
            .into_iter()
            .map(|section| section.outcomes().count())
            .sum();

        assert_eq!(held, Outcome::ALL.len());
        assert!(
            Section::ALL
                .into_iter()
                .all(|section| section.outcomes().next().is_some())
        );
    }

    #[test]
    fn a_pass_that_counted_nothing_is_blank_and_tells_no_section() {
        let stats = EnrichStats::default();

        assert!(stats.is_blank());
        assert_eq!(stats.told_by_section().count(), 0);
    }

    #[test]
    fn only_the_sections_with_a_count_are_told_and_only_their_counted_outcomes() {
        let stats = EnrichStats {
            albums: 4,
            releases: 3,
            albums_unmatched: 1,
            refused: 2,
            ..EnrichStats::default()
        };

        let told: Vec<(Section, Vec<(Outcome, u64)>)> = stats.told_by_section().collect();

        assert_eq!(
            told,
            vec![
                (
                    Section::Albums,
                    vec![
                        (Outcome::Albums, 4),
                        (Outcome::Releases, 3),
                        (Outcome::AlbumsUnmatched, 1)
                    ]
                ),
                (Section::Trouble, vec![(Outcome::Refused, 2)]),
            ]
        );
    }
}
