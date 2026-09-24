use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::LookupOp;

use crate::{
    Client, Host, Result,
    client::{LARGEST_DOCUMENT, LARGEST_PICTURE, passed_over_when_refused},
};

const SECURE: &str = "https://";
const PICTURES_SERVED_BY: &str = ".mzstatic.com";
const THUMBNAIL: &str = "/image/thumb/";
const SHARED_AS: &str = "property=\"og:image\" content=\"";
const SQUARE: &str = "600x600cc.jpg";
const ARTIST_BUCKETS: [&str; 2] = ["AMCArtistImages", "Features"];
const PRESS_PICTURE: &str = "pr_source";

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

fn shared_picture(page: &str) -> Option<&str> {
    let from = page.find(SHARED_AS)? + SHARED_AS.len();
    let rest = &page[from..];
    Some(&rest[..rest.find('"')?])
}

fn squared(picture: &str) -> Option<String> {
    let rest = picture.strip_prefix(SECURE)?;
    let (host, _) = rest.split_once('/')?;
    if !host.ends_with(PICTURES_SERVED_BY) {
        return None;
    }
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
