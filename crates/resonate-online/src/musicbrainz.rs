use std::{fmt::Write, time::Duration};

use resonate_library::{
    ArtistMatch, ArtistProfile, ArtistRelease, Credit, Genre, GroupAsked, GroupMatch, GroupRelease,
    Isrc, LifeSpan, Link, LookupOp, Mbid, Medium, Recording, RecordingAsked, RecordingMatch,
    RecordingRelease, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch, ReleaseTrack, Wording,
};
use serde::Deserialize;

use crate::{
    Client, Error, Host, Result,
    lrclib::folded,
    query::{Params, lucene_quoted},
};

const FOUND_AT_MOST: u32 = 5;
const SONGS_FOUND_AT_MOST: u32 = 25;
const RELEASES_FOUND_AT_MOST: u32 = 10;
const BROWSE_PAGE: u32 = 100;
const GROUPS_AT_MOST: u32 = 1000;
const RELEASE_INCLUDES: &str =
    "recordings+artist-credits+media+release-groups+isrcs+labels+url-rels+recording-level-rels";
const ARTIST_INCLUDES: &str = "url-rels+tags+aliases";
const RECORDING_INCLUDES: &str = "artist-credits+releases+isrcs+media";
const RELEASE_GROUP_INCLUDES: &str = "artist-credits+releases+media+url-rels";
const LENGTH_MAY_DIFFER_BY_MS: u64 = 10_000;

#[derive(Deserialize)]
struct CreditDoc {
    #[serde(default)]
    name: String,
    #[serde(default)]
    joinphrase: Option<String>,
    #[serde(default)]
    artist: Option<ArtistRefDoc>,
}

#[derive(Deserialize)]
struct ArtistRefDoc {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct RelationDoc {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    url: Option<UrlDoc>,
}

#[derive(Deserialize)]
struct UrlDoc {
    #[serde(default)]
    resource: Option<String>,
}

#[derive(Deserialize)]
struct ReleaseGroupDoc {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "primary-type")]
    primary_type: Option<String>,
}

#[derive(Deserialize)]
struct LabelInfoDoc {
    #[serde(default, rename = "catalog-number")]
    catalog_number: Option<String>,
    #[serde(default)]
    label: Option<LabelDoc>,
}

#[derive(Deserialize)]
struct LabelDoc {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct CoverArtArchiveDoc {
    #[serde(default)]
    front: bool,
}

#[derive(Deserialize)]
struct RecordingDoc {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    length: Option<u64>,
    #[serde(default)]
    isrcs: Vec<String>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default)]
    relations: Vec<RelationDoc>,
}

#[derive(Deserialize)]
struct TrackDoc {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    position: u32,
    #[serde(default)]
    number: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    length: Option<u64>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default)]
    recording: Option<RecordingDoc>,
}

#[derive(Deserialize)]
struct MediumDoc {
    #[serde(default)]
    position: u32,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    tracks: Vec<TrackDoc>,
}

#[derive(Deserialize)]
struct ReleaseDoc {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    barcode: Option<String>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default, rename = "release-group")]
    release_group: Option<ReleaseGroupDoc>,
    #[serde(default, rename = "label-info")]
    label_info: Vec<LabelInfoDoc>,
    #[serde(default, rename = "cover-art-archive")]
    cover_art_archive: Option<CoverArtArchiveDoc>,
    #[serde(default)]
    relations: Vec<RelationDoc>,
    #[serde(default)]
    media: Vec<MediumDoc>,
}

#[derive(Deserialize)]
struct ReleaseFoundDoc {
    id: String,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default, rename = "release-group")]
    release_group: Option<ReleaseGroupDoc>,
    #[serde(default, rename = "track-count")]
    track_count: Option<u32>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Deserialize)]
struct ReleaseSearchDoc {
    #[serde(default)]
    releases: Vec<ReleaseFoundDoc>,
}

#[derive(Deserialize)]
struct TrackPlaceDoc {
    #[serde(default)]
    position: Option<u32>,
}

#[derive(Deserialize)]
struct TrackedMediumDoc {
    #[serde(default)]
    position: Option<u32>,
    #[serde(default, rename = "track-count")]
    track_count: Option<u32>,
    #[serde(default, rename = "track-offset")]
    track_offset: Option<u32>,
    #[serde(default, alias = "track")]
    tracks: Vec<TrackPlaceDoc>,
}

#[derive(Deserialize)]
struct ReleaseOfRecordingDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    media: Vec<TrackedMediumDoc>,
}

#[derive(Deserialize)]
struct FullRecordingDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    length: Option<u64>,
    #[serde(default)]
    isrcs: Vec<String>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default)]
    releases: Vec<ReleaseOfRecordingDoc>,
}

#[derive(Deserialize)]
struct IsrcDoc {
    #[serde(default)]
    recordings: Vec<FullRecordingDoc>,
}

#[derive(Deserialize)]
struct RecordingFoundDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    title: String,
    #[serde(default)]
    length: Option<u64>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default)]
    isrcs: Vec<String>,
    #[serde(default)]
    releases: Vec<ReleaseOfRecordingDoc>,
}

#[derive(Deserialize)]
struct RecordingSearchDoc {
    #[serde(default)]
    recordings: Vec<RecordingFoundDoc>,
}

#[derive(Deserialize)]
struct GroupReleaseDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    media: Vec<TrackedMediumDoc>,
}

#[derive(Deserialize)]
struct FullReleaseGroupDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(default, rename = "first-release-date")]
    first_release_date: Option<String>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
    #[serde(default)]
    relations: Vec<RelationDoc>,
    #[serde(default)]
    releases: Vec<GroupReleaseDoc>,
}

#[derive(Deserialize)]
struct GroupFoundDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<CreditDoc>,
}

#[derive(Deserialize)]
struct GroupSearchDoc {
    #[serde(default, rename = "release-groups")]
    release_groups: Vec<GroupFoundDoc>,
}

#[derive(Deserialize)]
struct BrowsedGroupDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(default, rename = "secondary-types")]
    secondary_types: Vec<String>,
    #[serde(default, rename = "first-release-date")]
    first_release_date: Option<String>,
}

#[derive(Deserialize)]
struct BrowseDoc {
    #[serde(default, rename = "release-group-count")]
    release_group_count: u32,
    #[serde(default, rename = "release-group-offset")]
    release_group_offset: u32,
    #[serde(default, rename = "release-groups")]
    release_groups: Vec<BrowsedGroupDoc>,
}

#[derive(Deserialize)]
struct AreaDoc {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct LifeSpanDoc {
    #[serde(default)]
    begin: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    ended: Option<bool>,
}

#[derive(Deserialize)]
struct TagDoc {
    #[serde(default)]
    name: String,
    #[serde(default)]
    count: i64,
}

#[derive(Deserialize)]
struct AliasDoc {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct ArtistDoc {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "sort-name")]
    sort_name: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    area: Option<AreaDoc>,
    #[serde(default, rename = "begin-area")]
    begin_area: Option<AreaDoc>,
    #[serde(default, rename = "life-span")]
    life_span: Option<LifeSpanDoc>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default)]
    aliases: Vec<AliasDoc>,
    #[serde(default)]
    tags: Vec<TagDoc>,
    #[serde(default)]
    relations: Vec<RelationDoc>,
}

#[derive(Deserialize)]
struct ArtistFoundDoc {
    id: String,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default)]
    aliases: Vec<AliasDoc>,
}

#[derive(Deserialize)]
struct ArtistSearchDoc {
    #[serde(default)]
    artists: Vec<ArtistFoundDoc>,
}

pub(crate) fn release(client: &Client, id: &Mbid) -> Result<Option<Release>> {
    let op = LookupOp::Release;
    let path = format!("/release/{id}?inc={RELEASE_INCLUDES}&fmt=json");
    client
        .json::<ReleaseDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.into_release(op))
        .transpose()
}

pub(crate) fn find_release(client: &Client, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
    let op = LookupOp::FindRelease;
    let path = release_search(asked);
    let found = client
        .json::<ReleaseSearchDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.releases)
        .unwrap_or_default();

    Ok(found
        .into_iter()
        .filter_map(ReleaseFoundDoc::into_match)
        .collect())
}

pub(crate) fn artist(client: &Client, id: &Mbid) -> Result<Option<ArtistProfile>> {
    let op = LookupOp::Artist;
    let path = format!("/artist/{id}?inc={ARTIST_INCLUDES}&fmt=json");
    client
        .json::<ArtistDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.into_profile(op))
        .transpose()
}

pub(crate) fn find_artist(client: &Client, name: &str) -> Result<Vec<ArtistMatch>> {
    let op = LookupOp::FindArtist;
    let query = format!("artist:{}", lucene_quoted(name));
    let path = format!(
        "/artist/{}",
        Params::new()
            .with("query", &query)
            .with("fmt", "json")
            .with("limit", &FOUND_AT_MOST.to_string())
            .finish()
    );
    let found = client
        .json::<ArtistSearchDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.artists)
        .unwrap_or_default();

    Ok(found
        .into_iter()
        .filter_map(ArtistFoundDoc::into_match)
        .collect())
}

pub(crate) fn release_groups_of(client: &Client, artist: &Mbid) -> Result<Vec<ArtistRelease>> {
    let op = LookupOp::ReleaseGroupsOfArtist;
    let mut releases: Vec<ArtistRelease> = Vec::new();
    let mut offset: u32 = 0;
    loop {
        let path = format!(
            "/release-group{}",
            Params::new()
                .with("artist", artist.as_str())
                .with("limit", &BROWSE_PAGE.to_string())
                .with("offset", &offset.to_string())
                .with("fmt", "json")
                .finish()
        );
        let Some(page) = client.json::<BrowseDoc>(Host::MusicBrainz, op, &path)? else {
            break;
        };
        let read = u32::try_from(page.release_groups.len()).unwrap_or(u32::MAX);
        let next = page.release_group_offset.saturating_add(read);
        let held = page.release_group_count.min(GROUPS_AT_MOST);
        releases.extend(
            page.release_groups
                .into_iter()
                .filter_map(BrowsedGroupDoc::into_artist_release),
        );
        if read == 0 || next <= offset || next >= held {
            break;
        }
        offset = next;
    }

    Ok(releases)
}

pub(crate) fn recording(client: &Client, id: &Mbid) -> Result<Option<Recording>> {
    let op = LookupOp::Recording;
    let path = format!("/recording/{id}?inc={RECORDING_INCLUDES}&fmt=json");
    client
        .json::<FullRecordingDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.into_recording(op))
        .transpose()
}

pub(crate) fn recordings_of_isrc(client: &Client, isrc: &Isrc) -> Result<Vec<Recording>> {
    let op = LookupOp::Isrc;
    let path = format!("/isrc/{isrc}?inc={RECORDING_INCLUDES}&fmt=json");
    client
        .json::<IsrcDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.recordings)
        .unwrap_or_default()
        .into_iter()
        .map(|document| document.into_recording(op))
        .collect()
}

pub(crate) fn find_recording(
    client: &Client,
    asked: &RecordingAsked,
) -> Result<Vec<RecordingMatch>> {
    let op = LookupOp::FindRecording;
    let path = recording_search(asked);
    let found = client
        .json::<RecordingSearchDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.recordings)
        .unwrap_or_default();

    Ok(found
        .into_iter()
        .filter_map(RecordingFoundDoc::into_match)
        .collect())
}

pub(crate) fn find_songs(client: &Client, words: &str) -> Result<Vec<RecordingMatch>> {
    let op = LookupOp::FindRecording;
    let path = songs_search(words);
    let found = client
        .json::<RecordingSearchDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.recordings)
        .unwrap_or_default();

    Ok(found
        .into_iter()
        .filter_map(RecordingFoundDoc::into_match)
        .collect())
}

fn songs_search(words: &str) -> String {
    searched_in_words("/recording/", words, None, SONGS_FOUND_AT_MOST)
}

pub(crate) fn release_group(client: &Client, id: &Mbid) -> Result<Option<ReleaseGroup>> {
    let op = LookupOp::ReleaseGroup;
    let path = format!("/release-group/{id}?inc={RELEASE_GROUP_INCLUDES}&fmt=json");
    client
        .json::<FullReleaseGroupDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.into_group(op))
        .transpose()
}

pub(crate) fn find_release_group(client: &Client, asked: &GroupAsked) -> Result<Vec<GroupMatch>> {
    let op = LookupOp::FindReleaseGroup;
    let path = release_group_search(asked);
    let found = client
        .json::<GroupSearchDoc>(Host::MusicBrainz, op, &path)?
        .map(|document| document.release_groups)
        .unwrap_or_default();

    Ok(found
        .into_iter()
        .filter_map(GroupFoundDoc::into_match)
        .collect())
}

fn release_search(asked: &ReleaseAsked) -> String {
    match asked.wording {
        Wording::Phrase => searched("/release/", &release_query(asked), RELEASES_FOUND_AT_MOST),
        Wording::Words => searched_in_words(
            "/release/",
            &asked.title,
            asked.artist.as_deref(),
            RELEASES_FOUND_AT_MOST,
        ),
    }
}

fn recording_search(asked: &RecordingAsked) -> String {
    match asked.wording {
        Wording::Phrase => searched("/recording/", &recording_query(asked), FOUND_AT_MOST),
        Wording::Words => searched_in_words(
            "/recording/",
            &asked.title,
            asked.artist.as_deref().or(asked.release.as_deref()),
            FOUND_AT_MOST,
        ),
    }
}

fn release_group_search(asked: &GroupAsked) -> String {
    match asked.wording {
        Wording::Phrase => searched(
            "/release-group/",
            &release_group_query(asked),
            FOUND_AT_MOST,
        ),
        Wording::Words => searched_in_words(
            "/release-group/",
            &asked.title,
            asked.artist.as_deref(),
            FOUND_AT_MOST,
        ),
    }
}

fn searched(endpoint: &str, query: &str, limit: u32) -> String {
    format!(
        "{endpoint}{}",
        Params::new()
            .with("query", query)
            .with("fmt", "json")
            .with("limit", &limit.to_string())
            .finish()
    )
}

fn searched_in_words(endpoint: &str, title: &str, artist: Option<&str>, limit: u32) -> String {
    let words = match artist {
        Some(name) => format!("{title} {name}"),
        None => title.to_owned(),
    };
    format!(
        "{endpoint}{}",
        Params::new()
            .with("query", &words)
            .with("dismax", "true")
            .with("fmt", "json")
            .with("limit", &limit.to_string())
            .finish()
    )
}

fn release_query(asked: &ReleaseAsked) -> String {
    let mut query = format!("release:{}", lucene_quoted(&asked.title));
    credited_to(
        &mut query,
        asked.artist_mbid.as_ref(),
        asked.artist.as_deref(),
    );
    if let Some(barcode) = &asked.barcode {
        let _ = write!(query, " AND barcode:{}", lucene_quoted(barcode));
    }
    if let Some(catalog_number) = &asked.catalog_number {
        let _ = write!(query, " AND catno:{}", lucene_quoted(catalog_number));
    }

    query
}

fn recording_query(asked: &RecordingAsked) -> String {
    let mut query = format!("recording:{}", lucene_quoted(&asked.title));
    credited_to(
        &mut query,
        asked.artist_mbid.as_ref(),
        asked.artist.as_deref(),
    );
    if let Some(release) = &asked.release {
        let _ = write!(query, " AND release:{}", lucene_quoted(release));
    }
    if let Some(length) = asked.length {
        let millis = u64::try_from(length.as_millis()).unwrap_or(u64::MAX);
        let shortest = millis.saturating_sub(LENGTH_MAY_DIFFER_BY_MS);
        let longest = millis.saturating_add(LENGTH_MAY_DIFFER_BY_MS);
        let _ = write!(query, " AND dur:[{shortest} TO {longest}]");
    }

    query
}

fn release_group_query(asked: &GroupAsked) -> String {
    let mut query = format!("releasegroup:{}", lucene_quoted(&asked.title));
    credited_to(
        &mut query,
        asked.artist_mbid.as_ref(),
        asked.artist.as_deref(),
    );
    if let Some(year) = asked.year {
        let _ = write!(query, " AND firstreleasedate:{year}*");
    }

    query
}

fn credited_to(query: &mut String, artist_mbid: Option<&Mbid>, artist: Option<&str>) {
    match (artist_mbid, artist) {
        (Some(mbid), _) => {
            let _ = write!(query, " AND arid:{mbid}");
        }
        (None, Some(name)) => {
            let _ = write!(query, " AND artist:{}", lucene_quoted(name));
        }
        (None, None) => {}
    }
}

fn mbid(text: Option<&str>) -> Option<Mbid> {
    text.and_then(|text| Mbid::new(text).ok())
}

fn present(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

fn credits(documents: Vec<CreditDoc>) -> Vec<Credit> {
    documents
        .into_iter()
        .map(|document| Credit {
            name: document.name,
            joined_by: document.joinphrase.unwrap_or_default(),
            mbid: mbid(document.artist.and_then(|artist| artist.id).as_deref()),
        })
        .collect()
}

fn credited_as(credit: &[Credit]) -> String {
    credit
        .iter()
        .map(|credit| format!("{}{}", credit.name, credit.joined_by))
        .collect()
}

fn links(relations: Vec<RelationDoc>) -> Vec<Link> {
    relations
        .into_iter()
        .filter_map(|relation| {
            let resource = relation.url?.resource?;
            Some(Link::new(
                relation.kind.as_deref().unwrap_or_default(),
                resource,
            ))
        })
        .collect()
}

fn millis(length: Option<u64>) -> Option<Duration> {
    length.map(Duration::from_millis)
}

impl ReleaseDoc {
    fn into_release(self, op: LookupOp) -> Result<Release> {
        let id = Mbid::new(&self.id).map_err(|_| Error::Unreadable {
            host: Host::MusicBrainz,
            op,
        })?;
        let credit = credits(self.artist_credit);
        let credited = credited_as(&credit);
        let (group, kind) = self
            .release_group
            .map(|group| (mbid(group.id.as_deref()), present(group.primary_type)))
            .unwrap_or_default();
        let (label, catalog_number) = self
            .label_info
            .into_iter()
            .next()
            .map(|info| {
                (
                    present(info.label.and_then(|label| label.name)),
                    present(info.catalog_number),
                )
            })
            .unwrap_or_default();

        let mut media: Vec<Medium> = self
            .media
            .into_iter()
            .map(|medium| medium.into_medium(&credited))
            .collect();
        media.sort_by_key(|medium| medium.position);

        Ok(Release {
            id,
            group,
            title: self.title,
            credit,
            date: present(self.date),
            country: present(self.country),
            label,
            catalog_number,
            barcode: present(self.barcode),
            kind,
            disambiguation: present(self.disambiguation),
            has_front_cover: self.cover_art_archive.is_some_and(|archive| archive.front),
            links: links(self.relations),
            media,
        })
    }
}

impl MediumDoc {
    fn into_medium(self, release_credited: &str) -> Medium {
        let mut tracks: Vec<ReleaseTrack> = self
            .tracks
            .into_iter()
            .map(|track| track.into_track(release_credited))
            .collect();
        tracks.sort_by_key(|track| track.position);

        Medium {
            position: self.position,
            format: present(self.format),
            title: present(self.title),
            tracks,
        }
    }
}

impl TrackDoc {
    fn into_track(self, release_credited: &str) -> ReleaseTrack {
        let (recording, recording_length, isrc, recording_credit, relations) = self
            .recording
            .map(|recording| {
                (
                    mbid(recording.id.as_deref()),
                    recording.length,
                    recording.isrcs.into_iter().next(),
                    recording.artist_credit,
                    recording.relations,
                )
            })
            .unwrap_or_default();
        let own_credit = if self.artist_credit.is_empty() {
            recording_credit
        } else {
            self.artist_credit
        };
        let artist = present(Some(credited_as(&credits(own_credit))))
            .filter(|credited| credited != release_credited);

        ReleaseTrack {
            position: self.position,
            number: self.number.unwrap_or_else(|| self.position.to_string()),
            title: self.title,
            artist,
            recording,
            track: mbid(self.id.as_deref()),
            length: millis(self.length.or(recording_length)),
            isrc: present(isrc),
            links: links(relations),
        }
    }
}

impl ReleaseFoundDoc {
    fn into_match(self) -> Option<ReleaseMatch> {
        let release = mbid(Some(&self.id))?;

        Some(ReleaseMatch {
            release,
            group: self
                .release_group
                .and_then(|group| mbid(group.id.as_deref())),
            score: self.score,
            title: self.title,
            credit: credits(self.artist_credit),
            track_count: self.track_count,
            date: present(self.date),
        })
    }
}

fn counted(media: &[TrackedMediumDoc]) -> Option<u32> {
    media
        .iter()
        .filter_map(|medium| medium.track_count)
        .reduce(u32::saturating_add)
}

#[derive(Clone, Copy, Debug, Default)]
struct Placing {
    disc: Option<u32>,
    position: Option<u32>,
}

fn placed(media: &[TrackedMediumDoc]) -> Placing {
    media
        .iter()
        .find_map(|medium| {
            let track = medium.tracks.first()?;
            Some(Placing {
                disc: medium.position,
                position: track
                    .position
                    .or_else(|| medium.track_offset.map(|before| before + 1)),
            })
        })
        .unwrap_or_default()
}

fn codes(isrcs: Vec<String>) -> Vec<Isrc> {
    isrcs
        .into_iter()
        .filter_map(|text| Isrc::new(&text).ok())
        .collect()
}

impl FullRecordingDoc {
    fn into_recording(self, op: LookupOp) -> Result<Recording> {
        let id = Mbid::new(&self.id).map_err(|_| Error::Unreadable {
            host: Host::MusicBrainz,
            op,
        })?;

        Ok(Recording {
            id,
            title: self.title,
            credit: credits(self.artist_credit),
            length: millis(self.length),
            isrcs: codes(self.isrcs),
            releases: self
                .releases
                .into_iter()
                .filter_map(ReleaseOfRecordingDoc::into_release_of)
                .collect(),
        })
    }
}

impl ReleaseOfRecordingDoc {
    fn into_release_of(self) -> Option<RecordingRelease> {
        let id = mbid(Some(&self.id))?;
        let placing = placed(&self.media);

        Some(RecordingRelease {
            id,
            title: self.title,
            date: present(self.date),
            disc: placing.disc,
            position: placing.position,
        })
    }
}

impl RecordingFoundDoc {
    fn into_match(self) -> Option<RecordingMatch> {
        let recording = mbid(Some(&self.id))?;

        Some(RecordingMatch {
            recording,
            score: self.score,
            title: self.title,
            credit: credits(self.artist_credit),
            length: millis(self.length),
            isrcs: codes(self.isrcs),
            releases: self
                .releases
                .into_iter()
                .filter_map(ReleaseOfRecordingDoc::into_release_of)
                .collect(),
        })
    }
}

impl FullReleaseGroupDoc {
    fn into_group(self, op: LookupOp) -> Result<ReleaseGroup> {
        let id = Mbid::new(&self.id).map_err(|_| Error::Unreadable {
            host: Host::MusicBrainz,
            op,
        })?;

        Ok(ReleaseGroup {
            id,
            title: self.title,
            credit: credits(self.artist_credit),
            kind: present(self.primary_type),
            first_released: present(self.first_release_date),
            disambiguation: present(self.disambiguation),
            links: links(self.relations),
            releases: self
                .releases
                .into_iter()
                .filter_map(GroupReleaseDoc::into_release_of_group)
                .collect(),
        })
    }
}

impl GroupReleaseDoc {
    fn into_release_of_group(self) -> Option<GroupRelease> {
        let id = mbid(Some(&self.id))?;
        let track_count = counted(&self.media);

        Some(GroupRelease {
            id,
            title: self.title,
            date: present(self.date),
            country: present(self.country),
            track_count,
        })
    }
}

impl BrowsedGroupDoc {
    fn into_artist_release(self) -> Option<ArtistRelease> {
        let Some(mbid) = mbid(Some(&self.id)) else {
            tracing::debug!(
                id = self.id,
                title = self.title,
                "a browsed release group names no mbid"
            );
            return None;
        };

        Some(ArtistRelease {
            mbid,
            title: self.title,
            kind: present(self.primary_type),
            secondary: self.secondary_types,
            first_released: present(self.first_release_date),
        })
    }
}

impl GroupFoundDoc {
    fn into_match(self) -> Option<GroupMatch> {
        let group = mbid(Some(&self.id))?;

        Some(GroupMatch {
            group,
            score: self.score,
            title: self.title,
            credit: credits(self.artist_credit),
        })
    }
}

impl ArtistDoc {
    fn into_profile(self, op: LookupOp) -> Result<ArtistProfile> {
        let mbid = Mbid::new(&self.id).map_err(|_| Error::Unreadable {
            host: Host::MusicBrainz,
            op,
        })?;
        let span = self
            .life_span
            .map_or_else(LifeSpan::default, |span| LifeSpan {
                begin: present(span.begin),
                end: present(span.end),
                ended: span.ended.unwrap_or(false),
            });

        let known_as = aliases(self.aliases, &self.name);

        Ok(ArtistProfile {
            mbid,
            name: self.name,
            sort_name: present(self.sort_name),
            kind: present(self.kind),
            gender: present(self.gender),
            country: present(self.country),
            area: present(self.area.and_then(|area| area.name)),
            began_in: present(self.begin_area.and_then(|area| area.name)),
            span,
            disambiguation: present(self.disambiguation),
            aliases: known_as,
            genres: genres(self.tags),
            links: links(self.relations),
        })
    }
}

fn aliases(documents: Vec<AliasDoc>, billed: &str) -> Vec<String> {
    let billed = folded(billed);
    let mut known_as: Vec<String> = Vec::new();
    let mut folds: Vec<String> = Vec::new();
    for document in documents {
        let Some(name) = present(Some(document.name)) else {
            continue;
        };
        let fold = folded(&name);
        if fold == billed || folds.contains(&fold) {
            continue;
        }
        folds.push(fold);
        known_as.push(name);
    }

    known_as
}

fn genres(tags: Vec<TagDoc>) -> Vec<Genre> {
    let mut genres: Vec<Genre> = tags
        .into_iter()
        .filter_map(|tag| {
            let weight = u32::try_from(tag.count).ok().filter(|count| *count > 0)?;
            present(Some(tag.name)).map(|name| Genre { name, weight })
        })
        .collect();
    genres.sort_by(|one, other| {
        other
            .weight
            .cmp(&one.weight)
            .then_with(|| one.name.cmp(&other.name))
    });

    genres
}

impl ArtistFoundDoc {
    fn into_match(self) -> Option<ArtistMatch> {
        let mbid = mbid(Some(&self.id))?;
        let known_as = aliases(self.aliases, &self.name);

        Some(ArtistMatch {
            mbid,
            name: self.name,
            score: self.score,
            kind: present(self.kind),
            disambiguation: present(self.disambiguation),
            aliases: known_as,
        })
    }
}

#[cfg(test)]
mod tests {
    use resonate_library::{Relation, Service};

    use super::*;

    const RELEASE: &str = include_str!("../tests/fixtures/release.json");
    const RELEASE_LINKED: &str = include_str!("../tests/fixtures/release_linked.json");
    const RELEASE_SEARCH: &str = include_str!("../tests/fixtures/release_search.json");
    const RECORDING: &str = include_str!("../tests/fixtures/recording.json");
    const ISRC: &str = include_str!("../tests/fixtures/isrc.json");
    const RECORDING_SEARCH: &str = include_str!("../tests/fixtures/recording_search.json");
    const RELEASE_GROUP: &str = include_str!("../tests/fixtures/release_group.json");
    const RELEASE_GROUP_SEARCH: &str = include_str!("../tests/fixtures/release_group_search.json");
    const RELEASE_GROUP_BROWSE: &str = include_str!("../tests/fixtures/release_group_browse.json");
    const ARTIST: &str = include_str!("../tests/fixtures/artist.json");
    const ARTIST_SEARCH: &str = include_str!("../tests/fixtures/artist_search.json");

    const MEDDLE: &str = "aadf62d6-d475-42e0-b622-e6da7a59fdf7";
    const MEDDLE_GROUP: &str = "4e98c9b4-92f6-3049-b9da-a1088b623672";
    const PINK_FLOYD: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const ONE_OF_THESE_DAYS: &str = "8a313f74-75fe-40ff-a1a9-0b000216c555";

    fn read_release(text: &str) -> Release {
        serde_json::from_str::<ReleaseDoc>(text)
            .expect("the fixture parses")
            .into_release(LookupOp::Release)
            .expect("the fixture maps")
    }

    #[test]
    fn the_release_lookup_maps_every_track_its_recording_and_its_links() {
        let release = read_release(RELEASE);

        assert_eq!(release.id.as_str(), MEDDLE);
        assert_eq!(release.group.as_ref().map(Mbid::as_str), Some(MEDDLE_GROUP));
        assert_eq!(release.title, "Meddle");
        assert_eq!(release.credited_as(), "Pink Floyd");
        assert_eq!(
            release.credit[0].mbid.as_ref().map(Mbid::as_str),
            Some(PINK_FLOYD)
        );
        assert_eq!(release.date.as_deref(), Some("1986"));
        assert_eq!(release.country.as_deref(), Some("GB"));
        assert_eq!(release.label.as_deref(), Some("Harvest"));
        assert_eq!(release.catalog_number.as_deref(), Some("CDP 7 46034 2"));
        assert_eq!(release.barcode, None);
        assert_eq!(release.kind.as_deref(), Some("Album"));
        assert_eq!(release.disambiguation.as_deref(), Some("made in Japan"));
        assert!(release.has_front_cover);
        assert_eq!(release.links.len(), 1);
        assert_eq!(release.links[0].relation, Relation::Discogs);
        assert_eq!(release.links[0].service, Service::Discogs);

        assert_eq!(release.media.len(), 1);
        assert_eq!(release.media[0].position, 1);
        assert_eq!(release.media[0].format.as_deref(), Some("CD"));
        assert_eq!(release.media[0].title, None);
        assert_eq!(release.track_count(), 6);

        let first = &release.media[0].tracks[0];
        assert_eq!(first.position, 1);
        assert_eq!(first.number, "1");
        assert_eq!(first.title, "One of These Days");
        assert_eq!(first.artist, None);
        assert_eq!(
            first.recording.as_ref().map(Mbid::as_str),
            Some("8a313f74-75fe-40ff-a1a9-0b000216c555")
        );
        assert_eq!(
            first.track.as_ref().map(Mbid::as_str),
            Some("21262c35-e7ab-4f4e-b1d2-8b84d89ea69e")
        );
        assert_eq!(first.length, Some(Duration::from_millis(357_400)));
        assert_eq!(first.isrc.as_deref(), Some("GBAYE7100389"));
        assert!(first.links.iter().any(
            |link| link.service == Service::Spotify && link.relation == Relation::FreeStreaming
        ));

        let positions: Vec<u32> = release.media[0]
            .tracks
            .iter()
            .map(|track| track.position)
            .collect();
        assert_eq!(positions, vec![1, 2, 3, 4, 5, 6]);
        assert!(
            release.media[0]
                .tracks
                .iter()
                .all(|track| track.recording.is_some() && track.length.is_some())
        );
    }

    #[test]
    fn a_release_captured_without_credits_or_labels_still_maps_with_its_links() {
        let release = read_release(RELEASE_LINKED);

        assert_eq!(release.title, "Random Access Memories");
        assert_eq!(release.credit, Vec::new());
        assert_eq!(release.credited_as(), "");
        assert_eq!(release.group, None);
        assert_eq!(release.kind, None);
        assert_eq!(release.label, None);
        assert_eq!(release.barcode.as_deref(), Some("886443919266"));
        assert_eq!(
            release.disambiguation.as_deref(),
            Some("apple digital master")
        );
        assert!(release.has_front_cover);
        assert_eq!(release.links.len(), 4);
        assert!(release.links.iter().any(
            |link| link.service == Service::AppleMusic && link.relation == Relation::Streaming
        ));
        assert_eq!(release.track_count(), 13);
        assert!(
            release.media[0]
                .tracks
                .iter()
                .all(|track| track.artist.is_none()
                    && track.isrc.is_none()
                    && !track.links.is_empty())
        );
    }

    #[test]
    fn a_track_credited_differently_from_the_release_carries_its_own_artist() {
        let document: ReleaseDoc = serde_json::from_str(
            r#"{
                "id": "aadf62d6-d475-42e0-b622-e6da7a59fdf7",
                "title": "Under the Covers",
                "artist-credit": [
                    {"name": "Matthew Sweet", "joinphrase": " & "},
                    {"name": "Susanna Hoffs"}
                ],
                "media": [{
                    "position": 2,
                    "tracks": [
                        {"position": 2, "number": "B", "title": "later",
                         "artist-credit": [{"name": "Matthew Sweet", "joinphrase": " & "}, {"name": "Susanna Hoffs"}]},
                        {"position": 1, "number": "A", "title": "first",
                         "artist-credit": [{"name": "Susanna Hoffs"}],
                         "recording": {"length": 1000}}
                    ]
                }, {"position": 1, "tracks": []}]
            }"#,
        )
        .expect("the document parses");
        let release = document
            .into_release(LookupOp::Release)
            .expect("the document maps");

        assert_eq!(release.credited_as(), "Matthew Sweet & Susanna Hoffs");
        assert_eq!(release.media[0].position, 1);
        let tracks = &release.media[1].tracks;
        assert_eq!(tracks[0].number, "A");
        assert_eq!(tracks[0].artist.as_deref(), Some("Susanna Hoffs"));
        assert_eq!(tracks[0].length, Some(Duration::from_secs(1)));
        assert_eq!(tracks[1].number, "B");
        assert_eq!(tracks[1].artist, None);
        assert_eq!(tracks[1].length, None);
    }

    #[test]
    fn a_release_search_answers_scored_matches_with_their_track_counts() {
        let found: Vec<ReleaseMatch> = serde_json::from_str::<ReleaseSearchDoc>(RELEASE_SEARCH)
            .expect("the fixture parses")
            .releases
            .into_iter()
            .filter_map(ReleaseFoundDoc::into_match)
            .collect();

        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|found| found.score == 100));
        assert!(found.iter().all(|found| found.title == "Meddle"));
        assert!(found.iter().all(|found| found.track_count == Some(6)));
        assert!(
            found
                .iter()
                .all(|found| found.credited_as() == "Pink Floyd")
        );
        assert_eq!(found[1].release.as_str(), MEDDLE);
        assert_eq!(found[1].date.as_deref(), Some("1986"));
        assert_eq!(found[0].date.as_deref(), Some("2016-01-15"));
        assert_eq!(
            found[0].group.as_ref().map(Mbid::as_str),
            Some(MEDDLE_GROUP)
        );
        assert_eq!(
            found[0]
                .credit
                .first()
                .and_then(|credit| credit.mbid.as_ref())
                .map(Mbid::as_str),
            Some(PINK_FLOYD)
        );
    }

    #[test]
    fn a_release_query_names_only_what_was_asked() {
        let asked = ReleaseAsked {
            title: "Meddle".to_owned(),
            artist: None,
            artist_mbid: None,
            barcode: None,
            catalog_number: None,
            wording: Wording::Phrase,
        };
        assert_eq!(release_query(&asked), "release:\"Meddle\"");

        let asked = ReleaseAsked {
            artist: Some("Pink \"the\" Floyd".to_owned()),
            barcode: Some("5099968083755".to_owned()),
            catalog_number: Some("CDP 7 46034 2".to_owned()),
            ..asked
        };
        assert_eq!(
            release_query(&asked),
            "release:\"Meddle\" AND artist:\"Pink \\\"the\\\" Floyd\" AND barcode:\"5099968083755\" AND catno:\"CDP 7 46034 2\""
        );
    }

    #[test]
    fn a_release_query_asks_by_the_artist_id_over_the_name_where_it_has_one() {
        let asked = ReleaseAsked {
            title: "Meddle".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_mbid: Some(Mbid::new(PINK_FLOYD).expect("a well-formed mbid")),
            barcode: None,
            catalog_number: None,
            wording: Wording::Phrase,
        };
        assert_eq!(
            release_query(&asked),
            "release:\"Meddle\" AND arid:83d91898-7763-47d7-b03b-b92132375c47"
        );
    }

    #[test]
    fn a_release_search_is_a_lucene_phrase_or_the_words_under_dismax() {
        let asked = ReleaseAsked {
            title: "When We All Fall Asleep, Where Do We Go? LP".to_owned(),
            artist: Some("Billie Eilish".to_owned()),
            artist_mbid: Some(Mbid::new(PINK_FLOYD).expect("a well-formed mbid")),
            barcode: Some("602577427664".to_owned()),
            catalog_number: Some("B0029901-02".to_owned()),
            wording: Wording::Phrase,
        };
        assert_eq!(
            release_search(&asked),
            "/release/?query=release%3A%22When%20We%20All%20Fall%20Asleep%2C%20Where%20Do%20We%20Go%3F%20LP%22%20AND%20arid%3A83d91898-7763-47d7-b03b-b92132375c47%20AND%20barcode%3A%22602577427664%22%20AND%20catno%3A%22B0029901-02%22&fmt=json&limit=10"
        );

        let asked = ReleaseAsked {
            wording: Wording::Words,
            ..asked
        };
        assert_eq!(
            release_search(&asked),
            "/release/?query=When%20We%20All%20Fall%20Asleep%2C%20Where%20Do%20We%20Go%3F%20LP%20Billie%20Eilish&dismax=true&fmt=json&limit=10"
        );

        let asked = ReleaseAsked {
            artist: None,
            ..asked
        };
        assert_eq!(
            release_search(&asked),
            "/release/?query=When%20We%20All%20Fall%20Asleep%2C%20Where%20Do%20We%20Go%3F%20LP&dismax=true&fmt=json&limit=10"
        );
    }

    #[test]
    fn a_recording_lookup_maps_its_credit_its_isrcs_and_every_release_it_sits_on() {
        let recording = serde_json::from_str::<FullRecordingDoc>(RECORDING)
            .expect("the fixture parses")
            .into_recording(LookupOp::Recording)
            .expect("the fixture maps");

        assert_eq!(recording.id.as_str(), ONE_OF_THESE_DAYS);
        assert_eq!(recording.title, "One of These Days");
        assert_eq!(recording.length, Some(Duration::from_millis(356_000)));
        assert_eq!(recording.credited_as(), "Pink Floyd");
        assert_eq!(
            recording.credit[0].mbid.as_ref().map(Mbid::as_str),
            Some(PINK_FLOYD)
        );
        assert_eq!(
            recording
                .isrcs
                .iter()
                .map(Isrc::as_str)
                .collect::<Vec<&str>>(),
            ["GBAYE7100389", "GBAYE9200443", "GBN9Y1100060"]
        );

        assert_eq!(recording.releases.len(), 25);
        let first = &recording.releases[0];
        assert_eq!(first.id.as_str(), "09ed5877-bb22-4a26-8ec7-c95e5ee70fd3");
        assert_eq!(first.title, "Meddle");
        assert_eq!(first.date.as_deref(), Some("1971-10-30"));
        assert_eq!(first.disc, Some(1));
        assert_eq!(first.position, Some(1));

        let boxed = recording
            .releases
            .iter()
            .find(|release| release.title == "The Pink Floyd Collection")
            .expect("the box set is one of them");
        assert_eq!(boxed.disc, Some(7));
        assert_eq!(boxed.position, Some(1));

        assert!(
            recording
                .releases
                .iter()
                .all(|release| release.position == Some(1))
        );
    }

    #[test]
    fn an_isrc_lookup_answers_the_recordings_it_names() {
        let recordings: Vec<Recording> = serde_json::from_str::<IsrcDoc>(ISRC)
            .expect("the fixture parses")
            .recordings
            .into_iter()
            .map(|document| document.into_recording(LookupOp::Isrc))
            .collect::<Result<Vec<Recording>>>()
            .expect("the fixture maps");

        assert_eq!(recordings.len(), 2);
        assert!(
            recordings
                .iter()
                .all(|recording| recording.title == "One of These Days"
                    && recording.credited_as() == "Pink Floyd"
                    && recording.releases.is_empty())
        );
        assert_eq!(
            recordings[0].id.as_str(),
            "02c40b07-7550-4a7f-b6ac-3e3f9780648c"
        );
        assert_eq!(recordings[0].length, Some(Duration::from_millis(349_800)));
        assert_eq!(recordings[1].id.as_str(), ONE_OF_THESE_DAYS);
        assert_eq!(recordings[1].length, Some(Duration::from_millis(356_000)));

        let asked = Isrc::new("GBAYE7100389").expect("a well-formed isrc");
        assert!(
            recordings
                .iter()
                .all(|recording| recording.isrcs.contains(&asked))
        );
    }

    #[test]
    fn a_recording_search_answers_scored_matches_with_their_lengths() {
        let found: Vec<RecordingMatch> =
            serde_json::from_str::<RecordingSearchDoc>(RECORDING_SEARCH)
                .expect("the fixture parses")
                .recordings
                .into_iter()
                .filter_map(RecordingFoundDoc::into_match)
                .collect();

        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|found| found.score == 100));
        assert!(
            found
                .iter()
                .all(|found| found.credited_as() == "Pink Floyd")
        );
        assert_eq!(found[1].releases.len(), 1);
        assert_eq!(found[1].releases[0].title, "Re-Actor");
        assert_eq!(found[1].releases[0].date.as_deref(), Some("1995"));
        assert_eq!(found[1].releases[0].disc, Some(1));
        assert_eq!(
            found[1].releases[0].position,
            Some(10),
            "a search answer places a recording by the offset it names, having no position"
        );
        assert_eq!(
            found[0].recording.as_str(),
            "35e93af1-36d7-4493-ae4b-e7cdfbe38fce"
        );
        assert_eq!(found[0].title, "One of These Days");
        assert_eq!(found[0].length, Some(Duration::from_millis(478_693)));
        assert_eq!(found[1].length, Some(Duration::from_millis(410_000)));
        assert_eq!(found[2].length, None);
        assert_eq!(found[3].title, "One Of These Days");
    }

    #[test]
    fn a_search_answer_carries_the_codes_its_recording_is_registered_under() {
        let found: Vec<RecordingMatch> =
            serde_json::from_str::<RecordingSearchDoc>(RECORDING_SEARCH)
                .expect("the fixture parses")
                .recordings
                .into_iter()
                .filter_map(RecordingFoundDoc::into_match)
                .collect();

        assert_eq!(
            found[0].isrcs,
            vec![Isrc::new("GBN9Y2300077").expect("a well-formed isrc")]
        );
        assert!(
            found[1..].iter().all(|found| found.isrcs.is_empty()),
            "a take registered under no code answers with none rather than with an empty one"
        );
    }

    #[test]
    fn a_song_is_searched_for_in_the_words_it_was_typed_in_under_dismax() {
        let path = songs_search("echoes pink floyd");

        assert!(path.starts_with("/recording/?"), "{path}");
        assert!(path.contains("query=echoes%20pink%20floyd"), "{path}");
        assert!(path.contains("dismax=true"), "{path}");
        assert!(
            path.contains(&format!("limit={SONGS_FOUND_AT_MOST}")),
            "{path}"
        );
    }

    #[test]
    fn a_recording_query_names_only_what_was_asked() {
        let asked = RecordingAsked {
            title: "One of These Days".to_owned(),
            artist: None,
            artist_mbid: None,
            release: None,
            length: None,
            wording: Wording::Phrase,
        };
        assert_eq!(recording_query(&asked), "recording:\"One of These Days\"");

        let asked = RecordingAsked {
            artist: Some("Pink \"the\" Floyd".to_owned()),
            release: Some("Meddle".to_owned()),
            length: Some(Duration::from_millis(356_000)),
            ..asked
        };
        assert_eq!(
            recording_query(&asked),
            "recording:\"One of These Days\" AND artist:\"Pink \\\"the\\\" Floyd\" AND release:\"Meddle\" AND dur:[346000 TO 366000]"
        );

        let asked = RecordingAsked {
            length: Some(Duration::from_millis(1_000)),
            ..asked
        };
        assert!(recording_query(&asked).ends_with("AND dur:[0 TO 11000]"));

        let asked = RecordingAsked {
            artist_mbid: Some(Mbid::new(PINK_FLOYD).expect("a well-formed mbid")),
            release: None,
            length: None,
            ..asked
        };
        assert_eq!(
            recording_query(&asked),
            "recording:\"One of These Days\" AND arid:83d91898-7763-47d7-b03b-b92132375c47"
        );
    }

    #[test]
    fn a_recording_search_is_a_lucene_phrase_or_the_words_under_dismax() {
        let asked = RecordingAsked {
            title: "One of These Days".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_mbid: None,
            release: None,
            length: None,
            wording: Wording::Phrase,
        };
        assert_eq!(
            recording_search(&asked),
            "/recording/?query=recording%3A%22One%20of%20These%20Days%22%20AND%20artist%3A%22Pink%20Floyd%22&fmt=json&limit=5"
        );

        let asked = RecordingAsked {
            wording: Wording::Words,
            ..asked
        };
        assert_eq!(
            recording_search(&asked),
            "/recording/?query=One%20of%20These%20Days%20Pink%20Floyd&dismax=true&fmt=json&limit=5"
        );

        let asked = RecordingAsked {
            artist: None,
            release: Some("Meddle".to_owned()),
            ..asked
        };
        assert_eq!(
            recording_search(&asked),
            "/recording/?query=One%20of%20These%20Days%20Meddle&dismax=true&fmt=json&limit=5"
        );

        let asked = RecordingAsked {
            release: None,
            ..asked
        };
        assert_eq!(
            recording_search(&asked),
            "/recording/?query=One%20of%20These%20Days&dismax=true&fmt=json&limit=5"
        );
    }

    #[test]
    fn a_release_group_lookup_maps_its_releases_its_links_and_their_track_counts() {
        let group = serde_json::from_str::<FullReleaseGroupDoc>(RELEASE_GROUP)
            .expect("the fixture parses")
            .into_group(LookupOp::ReleaseGroup)
            .expect("the fixture maps");

        assert_eq!(group.id.as_str(), MEDDLE_GROUP);
        assert_eq!(group.title, "Meddle");
        assert_eq!(group.kind.as_deref(), Some("Album"));
        assert_eq!(group.first_released.as_deref(), Some("1971-10-30"));
        assert_eq!(group.disambiguation, None);
        assert_eq!(
            group.credit[0].mbid.as_ref().map(Mbid::as_str),
            Some(PINK_FLOYD)
        );

        assert_eq!(group.links.len(), 11);
        assert!(
            group.links.iter().any(
                |link| link.relation == Relation::AllMusic && link.service == Service::AllMusic
            )
        );
        assert!(
            group.links.iter().any(
                |link| link.relation == Relation::Wikidata && link.service == Service::Wikidata
            )
        );
        assert!(
            group
                .links
                .iter()
                .any(|link| link.relation == Relation::Lyrics && link.service == Service::Other)
        );

        assert_eq!(group.releases.len(), 25);
        assert!(
            group
                .releases
                .iter()
                .all(|release| release.track_count == Some(6))
        );
        let first = &group.releases[0];
        assert_eq!(first.id.as_str(), "09ed5877-bb22-4a26-8ec7-c95e5ee70fd3");
        assert_eq!(first.title, "Meddle");
        assert_eq!(first.date.as_deref(), Some("1971-10-30"));
        assert_eq!(first.country.as_deref(), Some("US"));
        assert!(
            group
                .releases
                .iter()
                .any(|release| release.id.as_str() == MEDDLE)
        );
    }

    #[test]
    fn a_release_group_search_answers_scored_matches_with_the_names_they_are_billed_under() {
        let found: Vec<GroupMatch> = serde_json::from_str::<GroupSearchDoc>(RELEASE_GROUP_SEARCH)
            .expect("the fixture parses")
            .release_groups
            .into_iter()
            .filter_map(GroupFoundDoc::into_match)
            .collect();

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].group.as_str(), MEDDLE_GROUP);
        assert_eq!(found[0].score, 100);
        assert_eq!(found[0].title, "Meddle");
        assert_eq!(found[0].credited_as(), "Pink Floyd");

        assert_eq!(found[1].score, 79);
        assert_eq!(found[1].title, "Meddle: Limited Edition Trance Remix");
    }

    #[test]
    fn a_release_group_query_names_only_what_was_asked() {
        let asked = GroupAsked {
            title: "Meddle".to_owned(),
            artist: None,
            artist_mbid: None,
            year: None,
            wording: Wording::Phrase,
        };
        assert_eq!(release_group_query(&asked), "releasegroup:\"Meddle\"");

        let asked = GroupAsked {
            artist: Some("Pink \"the\" Floyd".to_owned()),
            year: Some(1971),
            ..asked
        };
        assert_eq!(
            release_group_query(&asked),
            "releasegroup:\"Meddle\" AND artist:\"Pink \\\"the\\\" Floyd\" AND firstreleasedate:1971*"
        );

        let asked = GroupAsked {
            artist_mbid: Some(Mbid::new(PINK_FLOYD).expect("a well-formed mbid")),
            ..asked
        };
        assert_eq!(
            release_group_query(&asked),
            "releasegroup:\"Meddle\" AND arid:83d91898-7763-47d7-b03b-b92132375c47 AND firstreleasedate:1971*"
        );
    }

    #[test]
    fn a_release_group_search_is_a_lucene_phrase_or_the_words_under_dismax() {
        let asked = GroupAsked {
            title: "From Zero (Deluxe Edition)".to_owned(),
            artist: Some("Linkin Park".to_owned()),
            artist_mbid: Some(Mbid::new(PINK_FLOYD).expect("a well-formed mbid")),
            year: Some(2024),
            wording: Wording::Phrase,
        };
        assert_eq!(
            release_group_search(&asked),
            "/release-group/?query=releasegroup%3A%22From%20Zero%20%28Deluxe%20Edition%29%22%20AND%20arid%3A83d91898-7763-47d7-b03b-b92132375c47%20AND%20firstreleasedate%3A2024%2A&fmt=json&limit=5"
        );

        let asked = GroupAsked {
            wording: Wording::Words,
            ..asked
        };
        assert_eq!(
            release_group_search(&asked),
            "/release-group/?query=From%20Zero%20%28Deluxe%20Edition%29%20Linkin%20Park&dismax=true&fmt=json&limit=5"
        );

        let asked = GroupAsked {
            artist: None,
            ..asked
        };
        assert_eq!(
            release_group_search(&asked),
            "/release-group/?query=From%20Zero%20%28Deluxe%20Edition%29&dismax=true&fmt=json&limit=5"
        );
    }

    #[test]
    fn a_release_group_browse_answers_one_page_of_an_artists_groups_with_their_types() {
        let page: BrowseDoc =
            serde_json::from_str(RELEASE_GROUP_BROWSE).expect("the fixture parses");

        assert_eq!(page.release_group_count, 651);
        assert_eq!(page.release_group_offset, 0);
        assert_eq!(page.release_groups.len(), 100);

        let groups: Vec<ArtistRelease> = page
            .release_groups
            .into_iter()
            .filter_map(BrowsedGroupDoc::into_artist_release)
            .collect();
        assert_eq!(groups.len(), 100);

        let first = &groups[0];
        assert_eq!(first.mbid.as_str(), "06bc66bb-b9f1-3748-98a9-2acc878a940a");
        assert_eq!(first.title, "Works");
        assert_eq!(first.kind.as_deref(), Some("Album"));
        assert_eq!(first.secondary, ["Compilation"]);
        assert_eq!(first.first_released.as_deref(), Some("1983"));
        assert!(groups.iter().any(|group| group.title == "Meddle"
            && group.kind.as_deref() == Some("Album")
            && group.secondary.is_empty()));
    }

    #[test]
    fn an_artist_lookup_maps_the_profile_its_genres_and_every_provider_link() {
        let profile = serde_json::from_str::<ArtistDoc>(ARTIST)
            .expect("the fixture parses")
            .into_profile(LookupOp::Artist)
            .expect("the fixture maps");

        assert_eq!(profile.mbid.as_str(), PINK_FLOYD);
        assert_eq!(profile.name, "Pink Floyd");
        assert_eq!(profile.sort_name.as_deref(), Some("Pink Floyd"));
        assert_eq!(profile.kind.as_deref(), Some("Group"));
        assert_eq!(profile.gender, None);
        assert_eq!(profile.country.as_deref(), Some("GB"));
        assert_eq!(profile.area.as_deref(), Some("England"));
        assert_eq!(profile.began_in.as_deref(), Some("London"));
        assert_eq!(
            profile.span,
            LifeSpan {
                begin: Some("1965".to_owned()),
                end: Some("2014".to_owned()),
                ended: true,
            }
        );
        assert_eq!(profile.disambiguation, None);

        assert_eq!(profile.genres[0].name, "progressive rock");
        assert_eq!(profile.genres[0].weight, 58);
        assert_eq!(profile.genres[1].name, "psychedelic rock");
        assert!(
            profile
                .genres
                .windows(2)
                .all(|pair| pair[0].weight >= pair[1].weight)
        );
        assert!(profile.genres.iter().all(|genre| genre.weight > 0));

        let services: Vec<Service> = profile.links.iter().map(|link| link.service).collect();
        for service in [
            Service::Spotify,
            Service::Deezer,
            Service::AppleMusic,
            Service::Tidal,
            Service::Qobuz,
            Service::AmazonMusic,
            Service::Discogs,
            Service::Wikidata,
            Service::LastFm,
            Service::WikimediaCommons,
            Service::Youtube,
            Service::YoutubeMusic,
        ] {
            assert!(services.contains(&service), "{service:?} is missing");
        }
        assert_eq!(
            resonate_library::portrait_urls(&profile.links).collect::<Vec<&str>>(),
            vec!["https://commons.wikimedia.org/wiki/File:PinkFloyd1973_retouched.jpg"]
        );
        assert!(
            profile
                .links
                .iter()
                .any(|link| link.relation == Relation::Homepage)
        );
    }

    #[test]
    fn an_artist_lookup_maps_every_alias_but_the_name_it_is_billed_under() {
        let profile = serde_json::from_str::<ArtistDoc>(ARTIST)
            .expect("the fixture parses")
            .into_profile(LookupOp::Artist)
            .expect("the fixture maps");

        assert_eq!(
            profile.aliases,
            [
                "Floyd",
                "Pink Floid",
                "The Pink Floyd",
                "ピンク・フロイド",
                "핑크 플로이드",
            ]
        );
    }

    #[test]
    fn an_alias_is_kept_once_and_never_under_the_name_it_is_billed_under() {
        let documents = |names: &[&str]| {
            names
                .iter()
                .map(|name| AliasDoc {
                    name: (*name).to_owned(),
                })
                .collect::<Vec<AliasDoc>>()
        };

        assert_eq!(
            aliases(
                documents(&["Björk Guðmundsdóttir", "björk!", "Bjork", "   ", "Bjork"]),
                "Björk"
            ),
            ["Björk Guðmundsdóttir", "Bjork"]
        );
    }

    #[test]
    fn an_artist_search_answers_scored_matches() {
        let found: Vec<ArtistMatch> = serde_json::from_str::<ArtistSearchDoc>(ARTIST_SEARCH)
            .expect("the fixture parses")
            .artists
            .into_iter()
            .filter_map(ArtistFoundDoc::into_match)
            .collect();

        assert_eq!(found.len(), 3);
        assert_eq!(found[0].mbid.as_str(), PINK_FLOYD);
        assert_eq!(found[0].name, "Pink Floyd");
        assert_eq!(found[0].score, 100);
        assert_eq!(found[0].kind.as_deref(), Some("Group"));
        assert_eq!(found[0].disambiguation, None);
        assert_eq!(found[1].score, 61);
        assert_eq!(
            found[1].disambiguation.as_deref(),
            Some("online collective")
        );
        assert_eq!(found[2].name, "Celtic Pink Floyd");
    }
}
