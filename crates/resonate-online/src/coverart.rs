use std::collections::BTreeMap;

use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::{LookupOp, Mbid};
use serde::Deserialize;

use crate::{Client, Error, Host, Result, client::LARGEST_PICTURE};

const WANTED_SIZE: &str = "500";
const LARGE: &str = "large";
const PLAIN: &str = "http://";
const ENCRYPTED: &str = "https://";

#[derive(Deserialize)]
struct ImageDoc {
    #[serde(default)]
    front: bool,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    thumbnails: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct IndexDoc {
    #[serde(default)]
    images: Vec<ImageDoc>,
}

impl IndexDoc {
    fn front_url(self) -> Option<String> {
        let front = self.images.into_iter().find(|image| image.front)?;
        let mut thumbnails = front.thumbnails;
        thumbnails
            .remove(WANTED_SIZE)
            .or_else(|| thumbnails.remove(LARGE))
            .or(front.image)
            .map(encrypted)
    }
}

pub(crate) fn cover(
    client: &Client,
    release: &Mbid,
    group: Option<&Mbid>,
) -> Result<Option<CoverArt>> {
    let op = LookupOp::Cover;
    let release_index = format!("/release/{release}");
    let mut front = front_of(client, op, &release_index)?;
    if front.is_none()
        && let Some(group) = group
    {
        let group_index = format!("/release-group/{group}");
        front = front_of(client, op, &group_index)?;
    }
    let Some(url) = front else {
        return Ok(None);
    };

    fetched(client, op, &url)
}

pub(crate) fn group_cover(client: &Client, group: &Mbid) -> Result<Option<CoverArt>> {
    let op = LookupOp::Cover;
    let index = format!("/release-group/{group}");
    let Some(url) = front_of(client, op, &index)? else {
        return Ok(None);
    };

    fetched(client, op, &url)
}

fn encrypted(url: String) -> String {
    match url.strip_prefix(PLAIN) {
        Some(rest) => format!("{ENCRYPTED}{rest}"),
        None => url,
    }
}

fn fetched(client: &Client, op: LookupOp, url: &str) -> Result<Option<CoverArt>> {
    client
        .bytes(Host::CoverArtArchive, op, url, LARGEST_PICTURE)?
        .map(|bytes| picture(Host::CoverArtArchive, op, bytes))
        .transpose()
}

fn front_of(client: &Client, op: LookupOp, index: &str) -> Result<Option<String>> {
    Ok(client
        .json::<IndexDoc>(Host::CoverArtArchive, op, index)?
        .and_then(IndexDoc::front_url))
}

pub(crate) fn picture(host: Host, op: LookupOp, bytes: Vec<u8>) -> Result<CoverArt> {
    ImageFormat::sniff(&bytes)
        .map(|format| CoverArt { format, bytes })
        .ok_or(Error::Unreadable { host, op })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = include_str!("../tests/fixtures/coverart.json");
    const GROUP_INDEX: &str = include_str!("../tests/fixtures/coverart_group.json");

    #[test]
    fn the_cover_archive_index_names_a_front_image() {
        let index: IndexDoc = serde_json::from_str(INDEX).expect("the fixture parses");
        assert_eq!(index.images.len(), 5);

        assert_eq!(
            index.front_url().as_deref(),
            Some(
                "https://coverartarchive.org/release/aadf62d6-d475-42e0-b622-e6da7a59fdf7/8941972886-500.jpg"
            )
        );
    }

    #[test]
    fn a_release_group_index_names_a_front_image_of_its_own() {
        let index: IndexDoc = serde_json::from_str(GROUP_INDEX).expect("the fixture parses");
        assert_eq!(index.images.len(), 1);

        assert_eq!(
            index.front_url().as_deref(),
            Some(
                "https://coverartarchive.org/release/a59a0e76-ae5c-4373-84de-644f55a45150/12699162449-500.jpg"
            )
        );
    }

    #[test]
    fn a_five_hundred_thumbnail_is_preferred_and_the_full_image_is_the_last_resort() {
        let read = |text: &str| {
            serde_json::from_str::<IndexDoc>(text)
                .expect("the document parses")
                .front_url()
        };

        assert_eq!(
            read(r#"{"images":[{"front":true,"image":"full","thumbnails":{"500":"five","large":"big"}}]}"#).as_deref(),
            Some("five")
        );
        assert_eq!(
            read(r#"{"images":[{"front":true,"image":"full","thumbnails":{}}]}"#).as_deref(),
            Some("full")
        );
        assert_eq!(read(r#"{"images":[{"front":false,"image":"back"}]}"#), None);
        assert_eq!(read(r#"{"images":[]}"#), None);
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
