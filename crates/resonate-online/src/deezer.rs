use std::time::Duration;

use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::{
    Barcode, Isrc, Link, LinkNames, LookupOp, Relation, Service, StreamAsked, folded_letters,
};
use serde::Deserialize;

use crate::{
    Client, Host, Result,
    client::{LARGEST_PICTURE, passed_over_when_refused},
    query::Params,
    shared::on_host,
};

const ARTIST: &str = "/artist/";
const ALBUM: &str = "/album/";
const TRACK: &str = "/track/";
const TRACK_BY_ISRC: &str = "/track/isrc:";
const SEARCH: &str = "/search";
const SEARCHED_AT_MOST: &str = "10";
const TRACK_PAGES: &str = "https://www.deezer.com/";
const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(3);
const SECURE: &str = "https://";
const PICTURES_SERVED_BY: &str = "dzcdn.net";
const NOTHING_HASHED: &str = "/d41d8cd98f00b204e9800998ecf8427e/";

pub(crate) fn portrait(client: &Client, url: &str) -> Result<Option<CoverArt>> {
    let Some(artist) = artist(url) else {
        tracing::debug!(url, "a Deezer link that names no artist is passed over");
        return Ok(None);
    };
    let asked = format!("{ARTIST}{artist}");
    let Some(held) = passed_over_when_refused(client.json::<ArtistDoc>(
        Host::Deezer,
        LookupOp::Portrait,
        &asked,
    ))?
    else {
        return Ok(None);
    };
    let Some(picture) = held.pictured() else {
        tracing::debug!(artist, "Deezer holds no picture of the artist it links");
        return Ok(None);
    };

    Ok(passed_over_when_refused(client.bytes(
        Host::DeezerPictures,
        LookupOp::Portrait,
        picture,
        LARGEST_PICTURE,
    ))?
    .and_then(|bytes| {
        let format = ImageFormat::sniff(&bytes)?;
        Some(CoverArt { format, bytes })
    }))
}

pub(crate) fn streamed(client: &Client, asked: &StreamAsked) -> Result<Option<Link>> {
    if let Some(isrc) = &asked.isrc {
        let by_code = format!("{TRACK_BY_ISRC}{}", isrc.as_str());
        let held = passed_over_when_refused(client.json::<TrackDoc>(
            Host::Deezer,
            LookupOp::StreamLink,
            &by_code,
        ))?;
        if let Some(link) = held.and_then(TrackDoc::linked) {
            return Ok(Some(link));
        }
    }

    let Some(artist) = asked
        .artist
        .as_deref()
        .filter(|artist| !artist.trim().is_empty())
    else {
        return Ok(None);
    };
    let words = format!("{artist} {}", asked.title);
    let searched = format!(
        "{SEARCH}{}",
        Params::new()
            .with("q", &words)
            .with("limit", SEARCHED_AT_MOST)
            .finish()
    );
    let found = passed_over_when_refused(client.json::<Found>(
        Host::Deezer,
        LookupOp::StreamLink,
        &searched,
    ))?;

    Ok(found.and_then(|found| {
        found
            .data
            .into_iter()
            .find(|track| track.answers(asked))
            .and_then(TrackDoc::linked)
    }))
}

pub(crate) fn track_named(client: &Client, track: u64) -> Result<Option<LinkNames>> {
    let asked = format!("{TRACK}{track}");
    let held = client.json::<TrackDoc>(Host::Deezer, LookupOp::FollowLink, &asked)?;

    Ok(held.and_then(TrackDoc::named))
}

pub(crate) fn album_named(client: &Client, album: u64) -> Result<Option<Barcode>> {
    let asked = format!("{ALBUM}{album}");
    let held = client.json::<AlbumDoc>(Host::Deezer, LookupOp::FollowLink, &asked)?;

    Ok(held.and_then(AlbumDoc::barcode))
}

pub(crate) fn artist(url: &str) -> Option<u64> {
    let rest = url
        .strip_prefix(SECURE)
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if host != "deezer.com" && !host.ends_with(".deezer.com") {
        return None;
    }
    let mut segments = path.split(['/', '?', '#']);
    segments.find(|segment| *segment == "artist")?;
    segments.next()?.parse().ok()
}

#[derive(Debug, Deserialize)]
struct Found {
    #[serde(default)]
    data: Vec<TrackDoc>,
}

#[derive(Debug, Deserialize)]
struct TrackDoc {
    link: Option<String>,
    title: Option<String>,
    title_short: Option<String>,
    duration: Option<u64>,
    artist: Option<Credited>,
    isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Credited {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AlbumDoc {
    upc: Option<String>,
}

impl AlbumDoc {
    fn barcode(self) -> Option<Barcode> {
        Barcode::new(self.upc.as_deref()?)
    }
}

fn named(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

impl TrackDoc {
    fn named(self) -> Option<LinkNames> {
        let isrcs: Vec<Isrc> = self
            .isrc
            .as_deref()
            .and_then(|code| Isrc::new(code).ok())
            .into_iter()
            .collect();
        let title = named(self.title.as_deref());
        let artist = named(self.artist.as_ref().map(|credited| credited.name.as_str()));
        if isrcs.is_empty() && (title.is_none() || artist.is_none()) {
            return None;
        }

        Some(LinkNames {
            isrcs,
            length: self
                .duration
                .filter(|seconds| *seconds > 0)
                .map(Duration::from_secs),
            title,
            artist,
        })
    }

    fn linked(self) -> Option<Link> {
        let url = self.link.filter(|url| url.starts_with(TRACK_PAGES))?;

        Some(Link {
            relation: Relation::Streaming,
            service: Service::Deezer,
            url,
        })
    }

    fn answers(&self, asked: &StreamAsked) -> bool {
        let title = stripped(&asked.title);
        let titled = [&self.title, &self.title_short]
            .into_iter()
            .flatten()
            .any(|named| stripped(named) == title);
        let credited = self
            .artist
            .as_ref()
            .zip(asked.artist.as_deref())
            .is_some_and(|(credited, artist)| stripped(&credited.name) == stripped(artist));
        let as_long = match (self.duration, asked.length) {
            (Some(seconds), Some(length)) if seconds > 0 => {
                Duration::from_secs(seconds).abs_diff(length) <= LENGTHS_AGREE_WITHIN
            }
            _ => true,
        };

        !title.is_empty() && titled && credited && as_long
    }
}

fn stripped(name: &str) -> String {
    folded_letters(name)
        .chars()
        .filter(|glyph| glyph.is_alphanumeric())
        .collect()
}

#[derive(Debug, Deserialize)]
struct ArtistDoc {
    picture_big: Option<String>,
}

impl ArtistDoc {
    fn pictured(&self) -> Option<&str> {
        self.picture_big
            .as_deref()
            .filter(|picture| served_by_the_picture_host(picture))
            .filter(|picture| !picture.contains(NOTHING_HASHED))
    }
}

fn served_by_the_picture_host(url: &str) -> bool {
    on_host(url, PICTURES_SERVED_BY).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(document: &str) -> ArtistDoc {
        serde_json::from_str(document).expect("the captured answer reads back")
    }

    fn echoes(length: Option<u64>) -> StreamAsked {
        StreamAsked {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            isrc: None,
            length: length.map(Duration::from_secs),
        }
    }

    fn searched() -> Found {
        serde_json::from_str(include_str!("../tests/fixtures/deezer_search.json"))
            .expect("the captured search reads back")
    }

    fn first_answering(asked: &StreamAsked) -> Option<Link> {
        searched()
            .data
            .into_iter()
            .find(|track| track.answers(asked))
            .and_then(TrackDoc::linked)
    }

    #[test]
    fn a_track_deezer_holds_under_an_isrc_is_the_track_page_it_links() {
        let held: TrackDoc =
            serde_json::from_str(include_str!("../tests/fixtures/deezer_track_isrc.json"))
                .expect("the captured answer reads back");

        assert_eq!(
            held.linked(),
            Some(Link {
                relation: Relation::Streaming,
                service: Service::Deezer,
                url: "https://www.deezer.com/track/677241".to_owned(),
            })
        );
    }

    #[test]
    fn an_isrc_deezer_does_not_hold_links_nothing() {
        let held: TrackDoc =
            serde_json::from_str(include_str!("../tests/fixtures/deezer_no_data.json"))
                .expect("the captured answer reads back");

        assert_eq!(held.linked(), None);
    }

    #[test]
    fn a_search_is_taken_only_where_the_title_the_artist_and_the_length_agree() {
        assert_eq!(
            first_answering(&echoes(Some(1_413))).map(|link| link.url),
            Some("https://www.deezer.com/track/116913994".to_owned())
        );
        assert_eq!(
            first_answering(&echoes(None)).map(|link| link.url),
            Some("https://www.deezer.com/track/116913994".to_owned())
        );
        assert_eq!(first_answering(&echoes(Some(600))), None);

        let elsewhere = StreamAsked {
            artist: Some("The Orbiters".to_owned()),
            ..echoes(None)
        };
        assert_eq!(first_answering(&elsewhere), None);
    }

    #[test]
    fn a_title_is_weighed_whatever_its_case_marks_and_punctuation() {
        let marked = StreamAsked {
            title: "ÉCHOES!".to_owned(),
            artist: Some("pink floyd".to_owned()),
            ..echoes(Some(1_412))
        };

        assert!(first_answering(&marked).is_some());
    }

    #[test]
    fn a_link_off_deezers_own_pages_is_not_taken() {
        let elsewhere = TrackDoc {
            link: Some("https://example.com/track/1".to_owned()),
            title: Some("Echoes".to_owned()),
            title_short: None,
            duration: None,
            artist: None,
            isrc: None,
        };

        assert_eq!(elsewhere.linked(), None);
    }

    #[test]
    fn a_deezer_track_names_its_code_and_length() {
        let track: TrackDoc =
            serde_json::from_str(include_str!("../tests/fixtures/deezer_track.json"))
                .expect("the captured answer reads back");
        let unknown: TrackDoc =
            serde_json::from_str(include_str!("../tests/fixtures/deezer_no_data.json"))
                .expect("the captured answer reads back");

        assert_eq!(
            track.named(),
            Some(LinkNames {
                isrcs: vec![Isrc::new("GBARL9300135").expect("an isrc")],
                length: Some(Duration::from_secs(213)),
                title: Some("Never Gonna Give You Up".to_owned()),
                artist: Some("Rick Astley".to_owned()),
            })
        );
        assert_eq!(unknown.named(), None);
    }

    #[test]
    fn a_deezer_album_names_its_barcode() {
        let album: AlbumDoc =
            serde_json::from_str(include_str!("../tests/fixtures/deezer_album.json"))
                .expect("the captured answer reads back");
        let unknown: AlbumDoc =
            serde_json::from_str(include_str!("../tests/fixtures/deezer_no_data.json"))
                .expect("the captured answer reads back");

        assert_eq!(album.barcode(), Barcode::new("859381157694"));
        assert_eq!(unknown.barcode(), None);
    }

    #[test]
    fn a_deezer_link_names_the_artist_it_points_at() {
        assert_eq!(
            artist("https://www.deezer.com/artist/7307038"),
            Some(7_307_038)
        );
        assert_eq!(
            artist("https://www.deezer.com/en/artist/72216032"),
            Some(72_216_032)
        );
        assert_eq!(artist("http://deezer.com/artist/12?utm=x"), Some(12));

        assert_eq!(artist("https://www.deezer.com/album/302127"), None);
        assert_eq!(artist("https://www.deezer.com/artist/"), None);
        assert_eq!(artist("https://notdeezer.com/artist/12"), None);
        assert_eq!(artist("not a url"), None);
    }

    #[test]
    fn an_artist_deezer_pictured_names_the_picture_on_its_own_host() {
        let held = read(include_str!("../tests/fixtures/deezer_artist.json"));
        assert_eq!(
            held.pictured(),
            Some(
                "https://cdn-images.dzcdn.net/images/artist/1ecaaa8df3152eef5188f53c7354889f/500x500-000000-80-0-0.jpg"
            )
        );
    }

    #[test]
    fn deezers_placeholder_and_an_answer_naming_nothing_are_no_picture() {
        assert_eq!(
            read(include_str!("../tests/fixtures/deezer_unpictured.json")).pictured(),
            None
        );
        assert_eq!(
            read(include_str!("../tests/fixtures/deezer_no_data.json")).pictured(),
            None
        );
        let elsewhere = ArtistDoc {
            picture_big: Some("https://example.com/picture.jpg".to_owned()),
        };
        assert_eq!(elsewhere.pictured(), None);
    }
}
