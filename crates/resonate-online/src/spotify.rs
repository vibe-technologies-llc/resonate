use std::time::Duration;

use resonate_codec::CoverArt;
use resonate_library::{LinkNames, LinkedPlaylist, ListedSong, LookupOp};
use serde::Deserialize;

use crate::{
    Client, Host, Result,
    client::LARGEST_DOCUMENT,
    shared::{next_data, on_host, page_of, picture_at, shared_picture, stated},
};

const ARTIST_PAGE: &str = "/artist/";
const EMBEDDED_PLAYLIST: &str = "/embed/playlist/";
const A_PLAYLIST: &str = "playlist";
const A_TRACK: &str = "track";
const PICTURES_SERVED_BY: &str = "i.scdn.co";
const AN_ARTIST_IMAGE: &str = "image/ab676161";
const ID_LENGTH: usize = 22;

pub(crate) fn portrait(client: &Client, url: &str) -> Result<Option<CoverArt>> {
    let Some(artist) = artist(url) else {
        tracing::debug!(url, "a Spotify link that names no artist is passed over");
        return Ok(None);
    };
    let page = format!("{}{ARTIST_PAGE}{artist}", Host::Spotify.base());
    let Some(held) = page_of(client, Host::Spotify, &page)? else {
        return Ok(None);
    };
    let Some(picture) = shared_picture(&held).filter(|picture| of_the_artist(picture)) else {
        tracing::debug!(page, "Spotify shows no picture of the artist itself");
        return Ok(None);
    };

    picture_at(client, Host::SpotifyPictures, picture)
}

pub(crate) fn artist(url: &str) -> Option<&str> {
    let path = on_host(url, "open.spotify.com")?;
    let path = path.split(['?', '#']).next()?;
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let mut kind = segments.next()?;
    if kind.starts_with("intl-") {
        kind = segments.next()?;
    }
    if kind != "artist" {
        return None;
    }
    let id = segments.next()?;

    names_an_id(id).then_some(id)
}

pub(crate) fn playlist_named(client: &Client, playlist: &str) -> Result<Option<LinkedPlaylist>> {
    if !names_an_id(playlist) {
        return Ok(None);
    }
    let page = format!("{}{EMBEDDED_PLAYLIST}{playlist}", Host::Spotify.base());
    let held = client.bytes(Host::Spotify, LookupOp::FollowLink, &page, LARGEST_DOCUMENT)?;

    Ok(held.and_then(|held| embedded(&String::from_utf8_lossy(&held))))
}

fn embedded(page: &str) -> Option<LinkedPlaylist> {
    let document: Embedded = serde_json::from_str(next_data(page)?)
        .inspect_err(|error| tracing::debug!(%error, "Spotify's page data did not read"))
        .ok()?;
    let entity = document.props.page_props.state.data.entity;
    if entity.kind.as_deref() != Some(A_PLAYLIST) {
        return None;
    }
    let name = stated(entity.name.as_deref().or(entity.title.as_deref()))?;

    Some(LinkedPlaylist {
        name,
        songs: entity
            .track_list
            .into_iter()
            .filter(|track| {
                track
                    .entity_type
                    .as_deref()
                    .is_none_or(|kind| kind == A_TRACK)
            })
            .filter_map(Listed::named)
            .map(ListedSong::Named)
            .collect(),
    })
}

#[derive(Deserialize)]
struct Embedded {
    props: Props,
}

#[derive(Deserialize)]
struct Props {
    #[serde(rename = "pageProps")]
    page_props: PageProps,
}

#[derive(Deserialize)]
struct PageProps {
    state: State,
}

#[derive(Deserialize)]
struct State {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    entity: Entity,
}

#[derive(Deserialize)]
struct Entity {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
    title: Option<String>,
    #[serde(rename = "trackList", default)]
    track_list: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    title: Option<String>,
    subtitle: Option<String>,
    duration: Option<u64>,
    #[serde(rename = "entityType")]
    entity_type: Option<String>,
}

impl Listed {
    fn named(self) -> Option<LinkNames> {
        Some(LinkNames {
            isrcs: Vec::new(),
            length: self
                .duration
                .filter(|millis| *millis > 0)
                .map(Duration::from_millis),
            title: Some(stated(self.title.as_deref())?),
            artist: Some(stated(self.subtitle.as_deref())?),
        })
    }
}

fn names_an_id(id: &str) -> bool {
    id.len() == ID_LENGTH && id.chars().all(|glyph| glyph.is_ascii_alphanumeric())
}

fn of_the_artist(picture: &str) -> bool {
    on_host(picture, PICTURES_SERVED_BY).is_some_and(|path| path.starts_with(AN_ARTIST_IMAGE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spotify_playlist_names_its_title_and_each_song_by_its_title_artists_and_length() {
        let linked = embedded(include_str!(
            "../tests/fixtures/spotify_playlist_embed.html"
        ))
        .expect("a playlist read off the embed page");

        assert_eq!(linked.name, "Today’s Top Hits");
        assert_eq!(linked.songs.len(), 50);
        assert_eq!(
            linked.songs.first(),
            Some(&ListedSong::Named(LinkNames {
                isrcs: Vec::new(),
                length: Some(Duration::from_millis(225_868)),
                title: Some("Patient Zero".to_owned()),
                artist: Some("Taylor Swift".to_owned()),
            }))
        );
        assert_eq!(embedded("<html>no page data</html>"), None);
    }

    #[test]
    fn a_spotify_link_names_the_artist_it_points_at() {
        assert_eq!(
            artist("https://open.spotify.com/artist/4BNbYkwFLxXh7rpAzwEit4"),
            Some("4BNbYkwFLxXh7rpAzwEit4")
        );
        assert_eq!(
            artist("https://open.spotify.com/intl-de/artist/4BNbYkwFLxXh7rpAzwEit4?si=x"),
            Some("4BNbYkwFLxXh7rpAzwEit4")
        );
        assert_eq!(
            artist("https://open.spotify.com/album/4BNbYkwFLxXh7rpAzwEit4"),
            None
        );
        assert_eq!(artist("https://open.spotify.com/artist/short"), None);
        assert_eq!(
            artist("https://example.com/artist/4BNbYkwFLxXh7rpAzwEit4"),
            None
        );
    }

    #[test]
    fn only_an_artist_image_on_spotifys_own_host_is_a_portrait() {
        assert!(of_the_artist(
            "https://i.scdn.co/image/ab6761610000e5eb6f27fd5b6666098ebc543416"
        ));
        assert!(
            !of_the_artist("https://i.scdn.co/image/ab67616d0000b2736f27fd5b6666098ebc543416"),
            "an album's sleeve is not the artist"
        );
        assert!(!of_the_artist(
            "https://example.com/image/ab6761610000e5eb6f27fd5b6666098ebc543416"
        ));
    }
}
