use std::time::Duration;

use resonate_library::{Isrc, LinkNames, LookupOp, SongLink};
use serde::Deserialize;

use crate::{Client, Host, Result, client::LARGEST_DOCUMENT, deezer};

const PAGE_DATA_OPENS: &str = r#"<script id="__NEXT_DATA__" type="application/json">"#;
const PAGE_DATA_CLOSES: &str = "</script>";
const A_SONG: &str = "song";
const ON_DEEZER: &str = "deezer|song|";

pub(crate) fn named_at(client: &Client, link: &SongLink) -> Result<Option<LinkNames>> {
    match link {
        SongLink::MusicBrainz(_) => Ok(None),
        SongLink::Deezer(track) => deezer::track_named(client, *track),
        SongLink::Elsewhere(_) => {
            let Some(page) = link
                .page()
                .filter(|page| page.starts_with(Host::SongLink.base()))
            else {
                return Ok(None);
            };
            let Some(held) = client.bytes(
                Host::SongLink,
                LookupOp::FollowLink,
                &page,
                LARGEST_DOCUMENT,
            )?
            else {
                return Ok(None);
            };
            let Some(song) = Song::read(&String::from_utf8_lossy(&held)) else {
                tracing::debug!(page, "song.link names no song at the link");
                return Ok(None);
            };
            let twin = match song.on_deezer {
                Some(track) => deezer::track_named(client, track)?,
                None => None,
            };

            Ok(song.named_with(twin))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Song {
    isrc: Option<Isrc>,
    length: Option<Duration>,
    on_deezer: Option<u64>,
}

impl Song {
    fn read(page: &str) -> Option<Self> {
        let from = page.find(PAGE_DATA_OPENS)? + PAGE_DATA_OPENS.len();
        let rest = &page[from..];
        let data = &rest[..rest.find(PAGE_DATA_CLOSES)?];
        let document: NextData = serde_json::from_str(data)
            .inspect_err(|error| tracing::debug!(%error, "song.link's page data did not read"))
            .ok()?;
        let held = document.props.page_props.page_data?;
        let entity = held.entity_data?;
        if entity.kind.as_deref() != Some(A_SONG) {
            return None;
        }
        let on_deezer = held
            .sections
            .iter()
            .flat_map(|section| &section.links)
            .filter_map(|link| link.unique_id.as_deref()?.strip_prefix(ON_DEEZER))
            .find_map(|track| track.parse().ok());

        Some(Self {
            isrc: entity.isrc.as_deref().and_then(|code| Isrc::new(code).ok()),
            length: entity
                .duration
                .filter(|millis| *millis > 0)
                .map(Duration::from_millis),
            on_deezer,
        })
    }

    fn named_with(self, twin: Option<LinkNames>) -> Option<LinkNames> {
        let mut isrcs: Vec<Isrc> = self.isrc.into_iter().collect();
        let mut length = self.length;
        if let Some(twin) = twin {
            for isrc in twin.isrcs {
                if !isrcs.contains(&isrc) {
                    isrcs.push(isrc);
                }
            }
            length = length.or(twin.length);
        }

        (!isrcs.is_empty()).then_some(LinkNames { isrcs, length })
    }
}

#[derive(Debug, Deserialize)]
struct NextData {
    props: Props,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Props {
    page_props: PageProps,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageProps {
    page_data: Option<PageData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageData {
    entity_data: Option<Entity>,
    #[serde(default)]
    sections: Vec<Section>,
}

#[derive(Debug, Deserialize)]
struct Entity {
    #[serde(rename = "type")]
    kind: Option<String>,
    isrc: Option<String>,
    duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Section {
    #[serde(default)]
    links: Vec<Linked>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Linked {
    unique_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../tests/fixtures/song_link_page.html");

    fn isrc(code: &str) -> Isrc {
        Isrc::new(code).expect("a well-formed isrc")
    }

    #[test]
    fn a_song_link_page_names_the_isrc_the_length_and_the_deezer_twin() {
        assert_eq!(
            Song::read(PAGE),
            Some(Song {
                isrc: Some(isrc("GBARL9300135")),
                length: Some(Duration::from_millis(213_573)),
                on_deezer: Some(781_592_622),
            })
        );
    }

    #[test]
    fn a_page_holding_no_song_names_nothing() {
        assert_eq!(Song::read("<html><title>Not Found</title></html>"), None);
        assert_eq!(
            Song::read(&PAGE.replace(r#""type":"song""#, r#""type":"album""#)),
            None
        );
    }

    #[test]
    fn the_twin_on_deezer_adds_its_code_where_the_page_named_another_or_none() {
        let page = Song {
            isrc: Some(isrc("GB5KW2103369")),
            length: None,
            on_deezer: Some(781_592_622),
        };
        let twin = LinkNames {
            isrcs: vec![isrc("GBARL9300135")],
            length: Some(Duration::from_secs(213)),
        };

        assert_eq!(
            page.clone().named_with(Some(twin)),
            Some(LinkNames {
                isrcs: vec![isrc("GB5KW2103369"), isrc("GBARL9300135")],
                length: Some(Duration::from_secs(213)),
            })
        );
        assert_eq!(Song { isrc: None, ..page }.named_with(None), None);
    }
}
