use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::{LookupOp, Mbid};

use crate::{Client, Error, Host, Result, client::LARGEST_PICTURE};

const FRONT_AT_THE_SIZE_DRAWN: &str = "front-500";
const FRONT_AS_UPLOADED: &str = "front";

pub(crate) fn cover(
    client: &Client,
    release: &Mbid,
    group: Option<&Mbid>,
) -> Result<Option<CoverArt>> {
    if let Some(art) = front(client, "release", release)? {
        return Ok(Some(art));
    }
    match group {
        Some(group) => group_cover(client, group),
        None => Ok(None),
    }
}

pub(crate) fn group_cover(client: &Client, group: &Mbid) -> Result<Option<CoverArt>> {
    front(client, "release-group", group)
}

fn front(client: &Client, kind: &str, id: &Mbid) -> Result<Option<CoverArt>> {
    for url in fronts_of(kind, id) {
        if let Some(art) = fetched(client, &url)? {
            return Ok(Some(art));
        }
    }
    Ok(None)
}

fn fronts_of(kind: &str, id: &Mbid) -> [String; 2] {
    [FRONT_AT_THE_SIZE_DRAWN, FRONT_AS_UPLOADED]
        .map(|size| format!("{}/{kind}/{id}/{size}", Host::CoverArtArchive.base()))
}

fn fetched(client: &Client, url: &str) -> Result<Option<CoverArt>> {
    let op = LookupOp::Cover;
    client
        .bytes(Host::CoverArtArchive, op, url, LARGEST_PICTURE)?
        .map(|bytes| picture(Host::CoverArtArchive, op, bytes))
        .transpose()
}

pub(crate) fn picture(host: Host, op: LookupOp, bytes: Vec<u8>) -> Result<CoverArt> {
    ImageFormat::sniff(&bytes)
        .map(|format| CoverArt { format, bytes })
        .ok_or(Error::Unreadable { host, op })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = "aadf62d6-d475-42e0-b622-e6da7a59fdf7";

    #[test]
    fn a_front_cover_is_asked_at_the_size_the_window_draws_then_as_uploaded_before_its_thumbnail_exists()
     {
        let id = Mbid::new(RELEASE).expect("an mbid");

        assert_eq!(
            fronts_of("release", &id),
            [
                format!("https://coverartarchive.org/release/{RELEASE}/front-500"),
                format!("https://coverartarchive.org/release/{RELEASE}/front"),
            ]
        );
        assert_eq!(
            fronts_of("release-group", &id),
            [
                format!("https://coverartarchive.org/release-group/{RELEASE}/front-500"),
                format!("https://coverartarchive.org/release-group/{RELEASE}/front"),
            ]
        );
    }

    #[test]
    fn bytes_that_are_no_picture_are_unreadable_rather_than_stored() {
        let png = b"\x89PNG\r\n\x1a\n rest".to_vec();
        let picture = picture(Host::CoverArtArchive, LookupOp::Cover, png).expect("a png");
        assert_eq!(picture.format, ImageFormat::Png);

        assert!(matches!(
            super::picture(Host::CoverArtArchive, LookupOp::Cover, b"<html>".to_vec()),
            Err(Error::Unreadable {
                host: Host::CoverArtArchive,
                op: LookupOp::Cover
            })
        ));
    }
}
