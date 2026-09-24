use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::LookupOp;
use serde::Deserialize;

use crate::{
    Client, Host, Result,
    client::{LARGEST_PICTURE, passed_over_when_refused},
};

const ARTIST: &str = "/artist/";
const SECURE: &str = "https://";
const PICTURES_SERVED_BY: &str = ".dzcdn.net";
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
    url.strip_prefix(SECURE)
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| host.ends_with(PICTURES_SERVED_BY))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(document: &str) -> ArtistDoc {
        serde_json::from_str(document).expect("the captured answer reads back")
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
