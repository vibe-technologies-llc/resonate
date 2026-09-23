use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use resonate_analysis::signature_of;
use resonate_core::{Isrc, SourceId};
use resonate_library::LookupOp;
use resonate_listen::{Clip, Heard, Picture, PictureFormat, Recogniser};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Client, Host,
    client::{LARGEST_PICTURE, Posted},
};

const SHAZAM: &str = "shazam";
const JSON: &str = "application/json";
const TAGGED_AT: &str = "/discovery/v5/en/US/android/-/tag";
const ASKED_WITH: &str =
    "?sync=true&webv3=true&sampling=true&shazamapiversion=v3&shazhub=true&video=v3";
const TIMEZONE: &str = "UTC";
const ALBUM: &str = "Album";
const RELEASED: &str = "Released";
const SONG_SECTION: &str = "SONG";
const PICTURES_SERVED_BY: &str = ".mzstatic.com";
const SECURE: &str = "https://";
const YEAR_DIGITS: usize = 4;

#[derive(Serialize)]
struct Asked<'a> {
    geolocation: Somewhere,
    signature: Signed<'a>,
    timestamp: u64,
    timezone: &'static str,
}

#[derive(Serialize)]
struct Somewhere {
    altitude: f64,
    latitude: f64,
    longitude: f64,
}

#[derive(Serialize)]
struct Signed<'a> {
    samplems: u64,
    timestamp: u64,
    uri: &'a str,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    matches: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    track: Option<TrackDoc>,
}

#[derive(Deserialize)]
struct TrackDoc {
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    isrc: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    images: Option<ImagesDoc>,
    #[serde(default)]
    sections: Vec<SectionDoc>,
}

#[derive(Deserialize)]
struct ImagesDoc {
    #[serde(default)]
    coverarthq: Option<String>,
    #[serde(default)]
    coverart: Option<String>,
}

#[derive(Deserialize)]
struct SectionDoc {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    metadata: Vec<MetadatumDoc>,
}

#[derive(Deserialize)]
struct MetadatumDoc {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

pub struct Shazam {
    client: Arc<Client>,
    service: SourceId,
}

impl Shazam {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            service: SourceId::new(SHAZAM).unwrap_or_else(|_| SourceId::local()),
        }
    }

    fn picture(&self, url: &str) -> Option<Picture> {
        if !served_by_the_picture_host(url) {
            tracing::debug!(
                url,
                "a cover answered from a host this build does not fetch from"
            );
            return None;
        }
        let bytes = self
            .client
            .bytes(Host::AppleArtwork, LookupOp::Cover, url, LARGEST_PICTURE)
            .inspect_err(
                |error| tracing::debug!(%error, "the cover of what was heard is not to be had"),
            )
            .ok()??;
        Some(Picture {
            format: PictureFormat::sniffed(&bytes)?,
            bytes,
        })
    }
}

impl Recogniser for Shazam {
    fn service(&self) -> &SourceId {
        &self.service
    }

    fn recognise(&self, clip: &Clip) -> resonate_listen::Result<Option<Heard>> {
        let signature = signature_of(&clip.mono(), clip.rate);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let uri = signature.uri();
        let asked = Asked {
            geolocation: Somewhere {
                altitude: 0.0,
                latitude: 0.0,
                longitude: 0.0,
            },
            signature: Signed {
                samplems: signature.sample_ms(),
                timestamp: now,
                uri: &uri,
            },
            timestamp: now,
            timezone: TIMEZONE,
        };
        let body = serde_json::to_vec(&asked).map_err(|_| resonate_listen::Error::Unreadable {
            service: self.service.clone(),
        })?;
        let url = format!(
            "{}{TAGGED_AT}/{}/{}{ASKED_WITH}",
            Host::Shazam.base(),
            Uuid::new_v4().hyphenated().to_string().to_uppercase(),
            Uuid::new_v4().hyphenated()
        );
        let answer: Option<Answer> = self
            .client
            .posted(
                Host::Shazam,
                LookupOp::Recognise,
                &url,
                &Posted {
                    content_type: JSON.to_owned(),
                    bytes: body,
                },
            )
            .map_err(|error| error.into_listen_error(self.service.clone()))?;

        let Some((mut heard, cover)) = heard_in(answer, &self.service) else {
            return Ok(None);
        };
        heard.picture = cover.and_then(|url| self.picture(&url));
        Ok(Some(heard))
    }
}

fn served_by_the_picture_host(url: &str) -> bool {
    url.strip_prefix(SECURE)
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| host.ends_with(PICTURES_SERVED_BY))
}

fn heard_in(answer: Option<Answer>, service: &SourceId) -> Option<(Heard, Option<String>)> {
    let answer = answer?;
    if answer.matches.is_empty() {
        return None;
    }
    let track = answer.track?;
    let told = |wanted: &str| {
        track
            .sections
            .iter()
            .filter(|section| section.kind.as_deref() == Some(SONG_SECTION))
            .flat_map(|section| &section.metadata)
            .find(|metadatum| metadatum.title.as_deref() == Some(wanted))
            .and_then(|metadatum| metadatum.text.clone())
    };
    let cover = track.images.as_ref().and_then(|images| {
        images
            .coverarthq
            .clone()
            .or_else(|| images.coverart.clone())
    });
    let heard = Heard {
        title: track.title.clone(),
        artist: track.subtitle.clone().filter(|artist| !artist.is_empty()),
        album: told(ALBUM),
        year: told(RELEASED).and_then(|released| {
            released
                .get(..YEAR_DIGITS)
                .and_then(|year| year.parse().ok())
        }),
        isrc: track.isrc.as_deref().and_then(|isrc| Isrc::new(isrc).ok()),
        recording: None,
        picture: None,
        link: track.url.clone(),
        by: service.clone(),
    };
    Some((heard, cover))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATCHED: &str = include_str!("../tests/fixtures/shazam_match.json");
    const NOTHING: &str = include_str!("../tests/fixtures/shazam_nothing.json");

    fn service() -> SourceId {
        SourceId::new(SHAZAM).expect("a service name")
    }

    fn answered(text: &str) -> Option<(Heard, Option<String>)> {
        heard_in(
            Some(serde_json::from_str(text).expect("a shazam answer")),
            &service(),
        )
    }

    #[test]
    fn a_match_names_the_song_its_album_its_year_and_where_its_cover_is() {
        let (heard, cover) = answered(MATCHED).expect("a match");
        assert_eq!(heard.title, "Hymn for the Weekend");
        assert_eq!(heard.artist.as_deref(), Some("Coldplay"));
        assert_eq!(heard.album.as_deref(), Some("A Head Full of Dreams"));
        assert_eq!(heard.year, Some(2015));
        assert!(heard.isrc.is_some());
        assert_eq!(
            heard.link.as_deref(),
            Some("https://www.shazam.com/track/300945736/hymn-for-the-weekend")
        );
        assert_eq!(heard.by, service());
        let cover = cover.expect("a cover");
        assert!(served_by_the_picture_host(&cover), "{cover}");
    }

    #[test]
    fn an_answer_with_no_matches_hears_nothing() {
        assert!(answered(NOTHING).is_none());
        assert!(heard_in(None, &service()).is_none());
    }

    #[test]
    fn a_cover_is_fetched_only_from_the_host_that_serves_them() {
        assert!(served_by_the_picture_host(
            "https://is3-ssl.mzstatic.com/image/thumb/a.jpg/400x400cc.jpg"
        ));
        assert!(!served_by_the_picture_host(
            "https://example.org/mzstatic.com/a.jpg"
        ));
        assert!(!served_by_the_picture_host(
            "http://is1-ssl.mzstatic.com/a.jpg"
        ));
        assert!(!served_by_the_picture_host(
            "https://mzstatic.com.example.org/a.jpg"
        ));
    }
}
