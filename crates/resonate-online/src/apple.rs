use std::time::Duration;

use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::{LinkNames, LinkedPlaylist, ListedSong, LookupOp};
use serde::Deserialize;

use crate::{
    Client, Host, Result,
    client::{LARGEST_DOCUMENT, LARGEST_PICTURE, passed_over_when_refused},
    shared::{on_host, shared_picture, stated},
};

const SECURE: &str = "https://";
const PICTURES_SERVED_BY: &str = "mzstatic.com";
const THUMBNAIL: &str = "/image/thumb/";
const SQUARE: &str = "600x600cc.jpg";
const ARTIST_BUCKETS: [&str; 2] = ["AMCArtistImages", "Features"];
const PRESS_PICTURE: &str = "pr_source";
const SERVER_DATA_OPENS: &str = r#"<script type="application/json" id="serialized-server-data">"#;
const SERVER_DATA_CLOSES: &str = "</script>";
const HEADER: &str = "containerDetailHeaderLockup";
const TRACK_LIST: &str = "trackLockup";
const A_SONG: &str = "song";
const ARTISTS_APART: &str = ", ";

pub(crate) fn playlist_named(
    client: &Client,
    storefront: &str,
    playlist: &str,
) -> Result<Option<LinkedPlaylist>> {
    let page = format!(
        "{}/{storefront}/playlist/{playlist}",
        Host::AppleMusic.base()
    );
    let held = client.bytes(
        Host::AppleMusic,
        LookupOp::FollowLink,
        &page,
        LARGEST_DOCUMENT,
    )?;

    Ok(held.and_then(|held| served(&String::from_utf8_lossy(&held))))
}

fn served(page: &str) -> Option<LinkedPlaylist> {
    let from = page.find(SERVER_DATA_OPENS)? + SERVER_DATA_OPENS.len();
    let rest = page.get(from..)?;
    let data = rest.get(..rest.find(SERVER_DATA_CLOSES)?)?;
    let document: Served = serde_json::from_str(data)
        .inspect_err(|error| tracing::debug!(%error, "Apple Music's page data did not read"))
        .ok()?;
    let sections = document.data.into_iter().next()?.data.sections;
    let name = sections
        .iter()
        .filter(|section| section.item_kind == HEADER)
        .flat_map(|section| &section.items)
        .find_map(|item| stated(item.title.as_deref()))?;

    Some(LinkedPlaylist {
        name,
        songs: sections
            .into_iter()
            .filter(|section| section.item_kind == TRACK_LIST)
            .flat_map(|section| section.items)
            .filter_map(Item::named)
            .map(ListedSong::Named)
            .collect(),
    })
}

#[derive(Deserialize)]
struct Served {
    data: Vec<Page>,
}

#[derive(Deserialize)]
struct Page {
    data: Sections,
}

#[derive(Deserialize)]
struct Sections {
    #[serde(default)]
    sections: Vec<Section>,
}

#[derive(Deserialize)]
struct Section {
    #[serde(rename = "itemKind", default)]
    item_kind: String,
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    title: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    #[serde(rename = "subtitleLinks", default)]
    subtitle_links: Vec<Titled>,
    duration: Option<u64>,
    #[serde(rename = "contentDescriptor")]
    content_descriptor: Option<Descriptor>,
}

#[derive(Deserialize)]
struct Titled {
    title: Option<String>,
}

#[derive(Deserialize)]
struct Descriptor {
    kind: Option<String>,
}

impl Item {
    fn named(self) -> Option<LinkNames> {
        let kind = self
            .content_descriptor
            .as_ref()
            .and_then(|descriptor| descriptor.kind.as_deref());
        if kind.is_some_and(|kind| kind != A_SONG) {
            return None;
        }
        let artist = stated(self.artist_name.as_deref()).or_else(|| {
            let credited: Vec<String> = self
                .subtitle_links
                .iter()
                .filter_map(|link| stated(link.title.as_deref()))
                .collect();
            (!credited.is_empty()).then(|| credited.join(ARTISTS_APART))
        })?;

        Some(LinkNames {
            isrcs: Vec::new(),
            length: self
                .duration
                .filter(|millis| *millis > 0)
                .map(Duration::from_millis),
            title: Some(stated(self.title.as_deref())?),
            artist: Some(artist),
        })
    }
}

pub(crate) fn portrait(client: &Client, url: &str) -> Result<Option<CoverArt>> {
    let Some(page) = artist_page(url) else {
        tracing::debug!(
            url,
            "an Apple Music link that names no artist is passed over"
        );
        return Ok(None);
    };
    let Some(held) = passed_over_when_refused(client.bytes(
        Host::AppleMusic,
        LookupOp::Portrait,
        &page,
        LARGEST_DOCUMENT,
    ))?
    else {
        return Ok(None);
    };
    let Some(picture) = shared_picture(&String::from_utf8_lossy(&held)).and_then(squared) else {
        tracing::debug!(page, "Apple Music shows no picture of the artist itself");
        return Ok(None);
    };

    Ok(passed_over_when_refused(client.bytes(
        Host::AppleArtwork,
        LookupOp::Portrait,
        &picture,
        LARGEST_PICTURE,
    ))?
    .and_then(|bytes| {
        let format = ImageFormat::sniff(&bytes)?;
        Some(CoverArt { format, bytes })
    }))
}

pub(crate) fn artist_page(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix(SECURE)
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if host != "music.apple.com" && host != "itunes.apple.com" {
        return None;
    }
    let path = path.split(['?', '#']).next()?;
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let store = segments.next()?;
    if store.len() != 2 || !store.chars().all(|letter| letter.is_ascii_lowercase()) {
        return None;
    }
    if segments.next()? != "artist" {
        return None;
    }
    let last = segments.next_back()?;
    let id = last.strip_prefix("id").unwrap_or(last);
    if id.is_empty() || !id.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }

    Some(format!("{}/{store}/artist/{id}", Host::AppleMusic.base()))
}

fn squared(picture: &str) -> Option<String> {
    on_host(picture, PICTURES_SERVED_BY)?;
    let thumbnail = &picture[picture.find(THUMBNAIL)? + THUMBNAIL.len()..];
    let bucket = thumbnail.split('/').next()?;
    let (named, _sized) = picture.rsplit_once('/')?;
    let file = named.rsplit('/').next()?;
    let of_the_artist = ARTIST_BUCKETS
        .iter()
        .any(|artists| bucket.starts_with(artists))
        || file.starts_with(PRESS_PICTURE);

    of_the_artist.then(|| format!("{named}/{SQUARE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apple_music_playlist_names_its_title_and_each_song_by_its_title_artists_and_length() {
        let linked = served(include_str!("../tests/fixtures/apple_music_playlist.html"))
            .expect("a playlist read off the page");

        assert_eq!(linked.name, "Today’s Hits");
        assert_eq!(linked.songs.len(), 3);
        assert_eq!(
            linked.songs.first(),
            Some(&ListedSong::Named(LinkNames {
                isrcs: Vec::new(),
                length: Some(Duration::from_millis(218_389)),
                title: Some("Solar Eclipse".to_owned()),
                artist: Some("Drake & Don Toliver".to_owned()),
            }))
        );
        assert_eq!(served("<html>no page data</html>"), None);
    }

    #[test]
    fn a_song_naming_no_joined_artist_is_credited_to_every_artist_it_links() {
        let item = Item {
            title: Some("Nicole Kidman".to_owned()),
            artist_name: None,
            subtitle_links: vec![
                Titled {
                    title: Some("ADÉLA".to_owned()),
                },
                Titled {
                    title: Some("Someone Else".to_owned()),
                },
            ],
            duration: None,
            content_descriptor: Some(Descriptor {
                kind: Some(A_SONG.to_owned()),
            }),
        };
        let video = Item {
            title: Some("A Video".to_owned()),
            artist_name: Some("ADÉLA".to_owned()),
            subtitle_links: Vec::new(),
            duration: None,
            content_descriptor: Some(Descriptor {
                kind: Some("musicVideo".to_owned()),
            }),
        };

        assert_eq!(
            item.named().and_then(|names| names.artist).as_deref(),
            Some("ADÉLA, Someone Else")
        );
        assert_eq!(video.named(), None);
    }

    #[test]
    fn an_apple_music_link_names_the_artist_page_it_points_at() {
        assert_eq!(
            artist_page("https://music.apple.com/gb/artist/408212594").as_deref(),
            Some("https://music.apple.com/gb/artist/408212594")
        );
        assert_eq!(
            artist_page("https://itunes.apple.com/nz/artist/id408212594").as_deref(),
            Some("https://music.apple.com/nz/artist/408212594")
        );
        assert_eq!(
            artist_page("https://music.apple.com/us/artist/mikolai-stroinski/949986605?l=en")
                .as_deref(),
            Some("https://music.apple.com/us/artist/949986605")
        );

        assert_eq!(
            artist_page("https://music.apple.com/us/album/1440857781"),
            None
        );
        assert_eq!(artist_page("https://music.apple.com/us/artist/"), None);
        assert_eq!(artist_page("https://example.com/us/artist/408212594"), None);
    }

    #[test]
    fn the_picture_a_page_shares_is_read_off_its_open_graph_tag() {
        let page = r#"<meta name="x"><meta property="og:image" content="https://is1-ssl.mzstatic.com/image/thumb/Features115/v4/34/pr_source.png/1200x630cw.png"><meta>"#;
        assert_eq!(
            shared_picture(page),
            Some(
                "https://is1-ssl.mzstatic.com/image/thumb/Features115/v4/34/pr_source.png/1200x630cw.png"
            )
        );
        assert_eq!(shared_picture("<html></html>"), None);
    }

    #[test]
    fn an_artists_own_picture_is_asked_for_square_and_a_record_sleeve_is_not_a_portrait() {
        assert_eq!(
            squared(
                "https://is1-ssl.mzstatic.com/image/thumb/Features115/v4/34/3d/21/343d219b/pr_source.png/1200x630cw.png"
            )
            .as_deref(),
            Some(
                "https://is1-ssl.mzstatic.com/image/thumb/Features115/v4/34/3d/21/343d219b/pr_source.png/600x600cc.jpg"
            )
        );
        assert!(squared(
            "https://is1-ssl.mzstatic.com/image/thumb/AMCArtistImages211/v4/51/07/a0/5107a0fc/file_cropped.png/1200x630cw.png"
        )
        .is_some());
        assert!(squared(
            "https://is1-ssl.mzstatic.com/image/thumb/Music112/v4/05/ae/5d/05ae5da5/pr_source.png/1200x630cw.png"
        )
        .is_some());

        assert_eq!(
            squared(
                "https://is1-ssl.mzstatic.com/image/thumb/Music221/v4/38/3a/77/383a7719/663918314692.jpg/1200x630cw.png"
            ),
            None
        );
        assert_eq!(
            squared("https://music.apple.com/assets/meta/apple-music.png"),
            None
        );
    }
}
