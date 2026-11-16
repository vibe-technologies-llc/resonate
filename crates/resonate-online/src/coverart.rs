use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::{LookupOp, Mbid};

use crate::{Client, Error, Host, Result, client::LARGEST_PICTURE};

const FRONT_AT_THE_LARGEST_THUMBNAIL: &str = "front-1200";
const FRONT_AS_UPLOADED: &str = "front";
const FRONT_UNDER_THE_PICTURE_CAP: &str = "front-500";

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
    front_among(&fronts_of(kind, id), |url| fetched(client, url))
}

fn front_among(
    [largest_thumbnail, uploaded, under_the_cap]: &[String; 3],
    mut fetch: impl FnMut(&str) -> Result<Option<CoverArt>>,
) -> Result<Option<CoverArt>> {
    if let Some(art) = too_large_passed_over(fetch(largest_thumbnail))? {
        return Ok(Some(art));
    }
    match fetch(uploaded) {
        Err(Error::TooLarge { limit, .. }) => {
            tracing::debug!(
                limit,
                "the front as uploaded is past the cap, so its smaller thumbnail is asked"
            );
            fetch(under_the_cap)
        }
        answered => answered,
    }
}

fn too_large_passed_over(fetched: Result<Option<CoverArt>>) -> Result<Option<CoverArt>> {
    match fetched {
        Err(Error::TooLarge { limit, .. }) => {
            tracing::debug!(
                limit,
                "a front thumbnail is past the cap; the next is asked"
            );
            Ok(None)
        }
        fetched => fetched,
    }
}

fn fronts_of(kind: &str, id: &Mbid) -> [String; 3] {
    [
        FRONT_AT_THE_LARGEST_THUMBNAIL,
        FRONT_AS_UPLOADED,
        FRONT_UNDER_THE_PICTURE_CAP,
    ]
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
    fn a_front_cover_is_asked_at_the_largest_thumbnail_then_as_uploaded_then_under_the_cap() {
        let id = Mbid::new(RELEASE).expect("an mbid");

        assert_eq!(
            fronts_of("release", &id),
            [
                format!("https://coverartarchive.org/release/{RELEASE}/front-1200"),
                format!("https://coverartarchive.org/release/{RELEASE}/front"),
                format!("https://coverartarchive.org/release/{RELEASE}/front-500"),
            ]
        );
        assert_eq!(
            fronts_of("release-group", &id),
            [
                format!("https://coverartarchive.org/release-group/{RELEASE}/front-1200"),
                format!("https://coverartarchive.org/release-group/{RELEASE}/front"),
                format!("https://coverartarchive.org/release-group/{RELEASE}/front-500"),
            ]
        );
    }

    fn png() -> CoverArt {
        CoverArt {
            format: ImageFormat::Png,
            bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
        }
    }

    fn too_large() -> Error {
        Error::TooLarge {
            host: Host::CoverArtArchive,
            op: LookupOp::Cover,
            limit: LARGEST_PICTURE,
        }
    }

    fn walked(
        answer: impl Fn(&str) -> Result<Option<CoverArt>>,
    ) -> (Result<Option<CoverArt>>, Vec<String>) {
        let fronts = ["1200", "uploaded", "500"].map(str::to_owned);
        let mut asked = Vec::new();
        let found = front_among(&fronts, |url| {
            asked.push(url.to_owned());
            answer(url)
        });
        (found, asked)
    }

    #[test]
    fn a_largest_thumbnail_held_is_the_one_request() {
        let (found, asked) = walked(|_| Ok(Some(png())));

        assert_eq!(found.expect("a front"), Some(png()));
        assert_eq!(asked, ["1200"]);
    }

    #[test]
    fn a_front_with_no_thumbnail_yet_is_taken_as_uploaded() {
        let (found, asked) = walked(|url| Ok((url == "uploaded").then(png)));

        assert_eq!(found.expect("a front"), Some(png()));
        assert_eq!(asked, ["1200", "uploaded"]);
    }

    #[test]
    fn a_release_with_no_front_costs_two_requests_and_never_a_third() {
        let (found, asked) = walked(|_| Ok(None));

        assert_eq!(found.expect("a miss"), None);
        assert_eq!(asked, ["1200", "uploaded"]);
    }

    #[test]
    fn an_upload_past_the_cap_falls_to_the_thumbnail_under_it() {
        let (found, asked) = walked(|url| match url {
            "uploaded" => Err(too_large()),
            "500" => Ok(Some(png())),
            _ => Ok(None),
        });

        assert_eq!(found.expect("a front"), Some(png()));
        assert_eq!(asked, ["1200", "uploaded", "500"]);
    }

    #[test]
    fn a_failure_other_than_size_ends_the_walk() {
        let (found, asked) = walked(|_| {
            Err(Error::Refused {
                host: Host::CoverArtArchive,
                op: LookupOp::Cover,
                status: 503,
            })
        });

        assert!(matches!(found, Err(Error::Refused { status: 503, .. })));
        assert_eq!(asked, ["1200"]);
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
