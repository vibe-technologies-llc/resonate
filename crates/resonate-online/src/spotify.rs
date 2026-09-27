use resonate_codec::CoverArt;

use crate::{
    Client, Host, Result,
    shared::{on_host, page_of, picture_at, shared_picture},
};

const ARTIST_PAGE: &str = "/artist/";
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

    (id.len() == ID_LENGTH && id.chars().all(|glyph| glyph.is_ascii_alphanumeric())).then_some(id)
}

fn of_the_artist(picture: &str) -> bool {
    on_host(picture, PICTURES_SERVED_BY).is_some_and(|path| path.starts_with(AN_ARTIST_IMAGE))
}

#[cfg(test)]
mod tests {
    use super::*;

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
