use std::time::Duration;

use resonate_core::SourceId;

use crate::{CoverArt, Isrc, Link, Mbid, Relation, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    pub joined_by: String,
    pub mbid: Option<Mbid>,
}

pub fn credited_as(credit: &[Credit]) -> String {
    credit
        .iter()
        .map(|credit| format!("{}{}", credit.name, credit.joined_by))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseTrack {
    pub position: u32,
    pub number: String,
    pub title: String,
    pub artist: Option<String>,
    pub recording: Option<Mbid>,
    pub track: Option<Mbid>,
    pub length: Option<Duration>,
    pub isrc: Option<String>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Medium {
    pub position: u32,
    pub format: Option<String>,
    pub title: Option<String>,
    pub tracks: Vec<ReleaseTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub id: Mbid,
    pub group: Option<Mbid>,
    pub title: String,
    pub credit: Vec<Credit>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub has_front_cover: bool,
    pub links: Vec<Link>,
    pub media: Vec<Medium>,
}

impl Release {
    pub fn track_count(&self) -> u32 {
        self.media
            .iter()
            .map(|medium| medium.tracks.len() as u32)
            .sum()
    }

    pub fn credited_as(&self) -> String {
        credited_as(&self.credit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wording {
    Phrase,
    Words,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAsked {
    pub title: String,
    pub artist: Option<String>,
    pub artist_mbid: Option<Mbid>,
    pub barcode: Option<String>,
    pub catalog_number: Option<String>,
    pub wording: Wording,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseMatch {
    pub release: Mbid,
    pub group: Option<Mbid>,
    pub score: u8,
    pub title: String,
    pub credit: Vec<Credit>,
    pub track_count: Option<u32>,
    pub date: Option<String>,
}

impl ReleaseMatch {
    pub fn credited_as(&self) -> String {
        credited_as(&self.credit)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingRelease {
    pub id: Mbid,
    pub title: String,
    pub date: Option<String>,
    pub disc: Option<u32>,
    pub position: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    pub id: Mbid,
    pub title: String,
    pub credit: Vec<Credit>,
    pub length: Option<Duration>,
    pub isrcs: Vec<Isrc>,
    pub releases: Vec<RecordingRelease>,
}

impl Recording {
    pub fn credited_as(&self) -> String {
        credited_as(&self.credit)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingAsked {
    pub title: String,
    pub artist: Option<String>,
    pub artist_mbid: Option<Mbid>,
    pub release: Option<String>,
    pub length: Option<Duration>,
    pub wording: Wording,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingMatch {
    pub recording: Mbid,
    pub score: u8,
    pub title: String,
    pub credit: Vec<Credit>,
    pub length: Option<Duration>,
    pub isrcs: Vec<Isrc>,
    pub releases: Vec<RecordingRelease>,
}

impl RecordingMatch {
    pub fn credited_as(&self) -> String {
        credited_as(&self.credit)
    }

    pub fn into_recording(self) -> Recording {
        Recording {
            id: self.recording,
            title: self.title,
            credit: self.credit,
            length: self.length,
            isrcs: self.isrcs,
            releases: self.releases,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupRelease {
    pub id: Mbid,
    pub title: String,
    pub date: Option<String>,
    pub country: Option<String>,
    pub track_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseGroup {
    pub id: Mbid,
    pub title: String,
    pub credit: Vec<Credit>,
    pub kind: Option<String>,
    pub first_released: Option<String>,
    pub disambiguation: Option<String>,
    pub links: Vec<Link>,
    pub releases: Vec<GroupRelease>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupAsked {
    pub title: String,
    pub artist: Option<String>,
    pub artist_mbid: Option<Mbid>,
    pub year: Option<i32>,
    pub wording: Wording,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMatch {
    pub group: Mbid,
    pub score: u8,
    pub title: String,
    pub credit: Vec<Credit>,
}

impl GroupMatch {
    pub fn credited_as(&self) -> String {
        credited_as(&self.credit)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LifeSpan {
    pub begin: Option<String>,
    pub end: Option<String>,
    pub ended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Genre {
    pub name: String,
    pub weight: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistProfile {
    pub mbid: Mbid,
    pub name: String,
    pub sort_name: Option<String>,
    pub kind: Option<String>,
    pub gender: Option<String>,
    pub country: Option<String>,
    pub area: Option<String>,
    pub began_in: Option<String>,
    pub span: LifeSpan,
    pub disambiguation: Option<String>,
    pub aliases: Vec<String>,
    pub genres: Vec<Genre>,
    pub links: Vec<Link>,
}

impl ArtistProfile {
    pub fn may_be_pictured(&self) -> bool {
        may_be_pictured(&self.links)
    }
}

pub fn portrait_urls(links: &[Link]) -> impl Iterator<Item = &str> {
    urls_of(links, Relation::Image)
}

pub fn wikidata_urls(links: &[Link]) -> impl Iterator<Item = &str> {
    urls_of(links, Relation::Wikidata)
}

pub fn may_be_pictured(links: &[Link]) -> bool {
    portrait_urls(links).next().is_some() || wikidata_urls(links).next().is_some()
}

fn urls_of(links: &[Link], relation: Relation) -> impl Iterator<Item = &str> {
    links
        .iter()
        .filter(move |link| link.relation == relation)
        .map(|link| link.url.as_str())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistRelease {
    pub mbid: Mbid,
    pub title: String,
    pub kind: Option<String>,
    pub secondary: Vec<String>,
    pub first_released: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistMatch {
    pub mbid: Mbid,
    pub name: String,
    pub score: u8,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LookupOp {
    Release,
    FindRelease,
    Recording,
    Isrc,
    FindRecording,
    ReleaseGroup,
    FindReleaseGroup,
    Artist,
    FindArtist,
    ReleaseGroupsOfArtist,
    Cover,
    Portrait,
    Lyrics,
    Devices,
    Correction,
    Recognise,
}

pub trait Reference: Send + Sync {
    fn source(&self) -> &SourceId;

    fn release(&self, id: &Mbid) -> Result<Option<Release>>;

    fn find_release(&self, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>>;

    fn recording(&self, id: &Mbid) -> Result<Option<Recording>>;

    fn recordings_of_isrc(&self, isrc: &Isrc) -> Result<Vec<Recording>>;

    fn find_recording(&self, asked: &RecordingAsked) -> Result<Vec<RecordingMatch>>;

    fn find_songs(&self, words: &str) -> Result<Vec<RecordingMatch>>;

    fn release_group(&self, id: &Mbid) -> Result<Option<ReleaseGroup>>;

    fn find_release_group(&self, asked: &GroupAsked) -> Result<Vec<GroupMatch>>;

    fn group_cover(&self, group: &Mbid) -> Result<Option<CoverArt>>;

    fn artist(&self, id: &Mbid) -> Result<Option<ArtistProfile>>;

    fn find_artist(&self, name: &str) -> Result<Vec<ArtistMatch>>;

    fn release_groups_of(&self, artist: &Mbid) -> Result<Vec<ArtistRelease>>;

    fn cover(&self, release: &Mbid, group: Option<&Mbid>) -> Result<Option<CoverArt>>;

    fn portrait(&self, links: &[Link]) -> Result<Option<CoverArt>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";

    #[test]
    fn a_release_is_credited_as_its_names_joined_the_way_musicbrainz_joins_them() {
        let release = Release {
            id: Mbid::new(ECHOES).expect("a well-formed mbid"),
            group: None,
            title: "Under the Covers".to_owned(),
            credit: vec![
                Credit {
                    name: "Matthew Sweet".to_owned(),
                    joined_by: " & ".to_owned(),
                    mbid: None,
                },
                Credit {
                    name: "Susanna Hoffs".to_owned(),
                    joined_by: String::new(),
                    mbid: None,
                },
            ],
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media: vec![
                Medium {
                    position: 1,
                    format: None,
                    title: None,
                    tracks: vec![track(1), track(2)],
                },
                Medium {
                    position: 2,
                    format: None,
                    title: None,
                    tracks: vec![track(1)],
                },
            ],
        };

        assert_eq!(release.credited_as(), "Matthew Sweet & Susanna Hoffs");
        assert_eq!(release.track_count(), 3);
    }

    fn track(position: u32) -> ReleaseTrack {
        ReleaseTrack {
            position,
            number: position.to_string(),
            title: format!("track {position}"),
            artist: None,
            recording: None,
            track: None,
            length: None,
            isrc: None,
            links: Vec::new(),
        }
    }
}
