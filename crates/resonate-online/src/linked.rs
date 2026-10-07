use std::time::Duration;

use resonate_library::{AlbumLink, AlbumNames, Barcode, Isrc, LinkNames, LookupOp, SongLink};
use serde::Deserialize;

use crate::{Client, Host, Result, client::LARGEST_DOCUMENT, deezer, shared::next_data};

const A_SONG: &str = "song";
const AN_ALBUM: &str = "album";
const SONG_ON_DEEZER: &str = "deezer|song|";
const ALBUM_ON_DEEZER: &str = "deezer|album|";
const ALBUM_PAGES: &str = "https://album.link/";

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
            let Some(held) = page_at(client, &page)? else {
                return Ok(None);
            };
            let Some(song) = Song::read(&held) else {
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

pub(crate) fn album_named_at(client: &Client, link: &AlbumLink) -> Result<Option<AlbumNames>> {
    match link {
        AlbumLink::Release(_) | AlbumLink::Group(_) => Ok(None),
        AlbumLink::Deezer(album) => Ok(Album::named_with(
            None,
            deezer::album_named(client, *album)?,
        )),
        AlbumLink::Elsewhere(_) => {
            let Some(page) = link.page().filter(|page| page.starts_with(ALBUM_PAGES)) else {
                return Ok(None);
            };
            let Some(held) = page_at(client, &page)? else {
                return Ok(None);
            };
            let Some(album) = Album::read(&held) else {
                tracing::debug!(page, "album.link names no album at the link");
                return Ok(None);
            };
            let twin = match album.on_deezer {
                Some(album) => deezer::album_named(client, album)?,
                None => None,
            };

            Ok(Album::named_with(album.barcode, twin))
        }
    }
}

fn page_at(client: &Client, page: &str) -> Result<Option<String>> {
    Ok(client
        .bytes(Host::SongLink, LookupOp::FollowLink, page, LARGEST_DOCUMENT)?
        .map(|held| String::from_utf8_lossy(&held).into_owned()))
}

fn page_data(page: &str) -> Option<PageData> {
    let document: NextData = serde_json::from_str(next_data(page)?)
        .inspect_err(|error| tracing::debug!(%error, "song.link's page data did not read"))
        .ok()?;

    document.props.page_props.page_data
}

impl PageData {
    fn entity_of(&self, kind: &str) -> Option<&Entity> {
        self.entity_data
            .as_ref()
            .filter(|entity| entity.kind.as_deref() == Some(kind))
    }

    fn on_deezer(&self, prefix: &str) -> Option<u64> {
        self.sections
            .iter()
            .flat_map(|section| &section.links)
            .filter_map(|link| link.unique_id.as_deref()?.strip_prefix(prefix))
            .find_map(|id| id.parse().ok())
    }
}

fn stated(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Song {
    isrc: Option<Isrc>,
    length: Option<Duration>,
    title: Option<String>,
    artist: Option<String>,
    on_deezer: Option<u64>,
}

impl Song {
    fn read(page: &str) -> Option<Self> {
        let held = page_data(page)?;
        let entity = held.entity_of(A_SONG)?;

        Some(Self {
            isrc: entity.isrc.as_deref().and_then(|code| Isrc::new(code).ok()),
            length: entity
                .duration
                .filter(|millis| *millis > 0)
                .map(Duration::from_millis),
            title: stated(entity.title.as_deref()),
            artist: stated(entity.artist_name.as_deref()),
            on_deezer: held.on_deezer(SONG_ON_DEEZER),
        })
    }

    fn named_with(self, twin: Option<LinkNames>) -> Option<LinkNames> {
        let mut isrcs: Vec<Isrc> = self.isrc.into_iter().collect();
        let mut length = self.length;
        let mut title = self.title;
        let mut artist = self.artist;
        if let Some(twin) = twin {
            for isrc in twin.isrcs {
                if !isrcs.contains(&isrc) {
                    isrcs.push(isrc);
                }
            }
            length = length.or(twin.length);
            title = title.or(twin.title);
            artist = artist.or(twin.artist);
        }
        let titled = title.is_some() && artist.is_some();

        (!isrcs.is_empty() || titled).then_some(LinkNames {
            isrcs,
            length,
            title,
            artist,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Album {
    barcode: Option<Barcode>,
    on_deezer: Option<u64>,
}

impl Album {
    fn read(page: &str) -> Option<Self> {
        let held = page_data(page)?;
        let entity = held.entity_of(AN_ALBUM)?;

        Some(Self {
            barcode: entity.upc.as_deref().and_then(Barcode::new),
            on_deezer: held.on_deezer(ALBUM_ON_DEEZER),
        })
    }

    fn named_with(barcode: Option<Barcode>, twin: Option<Barcode>) -> Option<AlbumNames> {
        let mut barcodes: Vec<Barcode> = barcode.into_iter().collect();
        if let Some(twin) = twin
            && !barcodes.iter().any(|held| held.names(twin.as_str()))
        {
            barcodes.push(twin);
        }

        (!barcodes.is_empty()).then_some(AlbumNames { barcodes })
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
#[serde(rename_all = "camelCase")]
struct Entity {
    #[serde(rename = "type")]
    kind: Option<String>,
    isrc: Option<String>,
    upc: Option<String>,
    duration: Option<u64>,
    title: Option<String>,
    artist_name: Option<String>,
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
    const YOUTUBE_PAGE: &str = include_str!("../tests/fixtures/song_link_youtube_page.html");
    const ALBUM_PAGE: &str = include_str!("../tests/fixtures/album_link_page.html");

    fn isrc(code: &str) -> Isrc {
        Isrc::new(code).expect("a well-formed isrc")
    }

    fn barcode(code: &str) -> Barcode {
        Barcode::new(code).expect("a well-formed barcode")
    }

    #[test]
    fn a_song_link_page_names_the_isrc_the_length_the_names_and_the_deezer_twin() {
        assert_eq!(
            Song::read(PAGE),
            Some(Song {
                isrc: Some(isrc("GBARL9300135")),
                length: Some(Duration::from_millis(213_573)),
                title: Some("Never Gonna Give You Up".to_owned()),
                artist: Some("Rick Astley".to_owned()),
                on_deezer: Some(781_592_622),
            })
        );
    }

    #[test]
    fn a_page_of_a_video_names_no_isrc_but_the_title_and_artist_it_is_billed_under() {
        let video = Song::read(YOUTUBE_PAGE).expect("the page names a song");

        assert_eq!(
            video,
            Song {
                isrc: None,
                length: Some(Duration::from_millis(214_000)),
                title: Some("Never Gonna Give You Up".to_owned()),
                artist: Some("Rick Astley".to_owned()),
                on_deezer: Some(781_592_622),
            }
        );
        assert_eq!(
            Song {
                on_deezer: None,
                ..video
            }
            .named_with(None),
            Some(LinkNames {
                isrcs: Vec::new(),
                length: Some(Duration::from_millis(214_000)),
                title: Some("Never Gonna Give You Up".to_owned()),
                artist: Some("Rick Astley".to_owned()),
            }),
            "a song with no code is still named by its title and artist"
        );
    }

    #[test]
    fn a_page_holding_no_song_names_nothing() {
        assert_eq!(Song::read("<html><title>Not Found</title></html>"), None);
        assert_eq!(
            Song::read(&PAGE.replace(r#""type":"song""#, r#""type":"album""#)),
            None
        );
        assert_eq!(Song::read(ALBUM_PAGE), None);
    }

    #[test]
    fn the_twin_on_deezer_adds_its_code_where_the_page_named_another_or_none() {
        let page = Song {
            isrc: Some(isrc("GB5KW2103369")),
            length: None,
            title: None,
            artist: None,
            on_deezer: Some(781_592_622),
        };
        let twin = LinkNames {
            isrcs: vec![isrc("GBARL9300135")],
            length: Some(Duration::from_secs(213)),
            title: Some("Never Gonna Give You Up".to_owned()),
            artist: Some("Rick Astley".to_owned()),
        };

        assert_eq!(
            page.clone().named_with(Some(twin.clone())),
            Some(LinkNames {
                isrcs: vec![isrc("GB5KW2103369"), isrc("GBARL9300135")],
                ..twin
            })
        );
        assert_eq!(
            Song {
                isrc: None,
                ..page.clone()
            }
            .named_with(None),
            None
        );
        assert_eq!(
            Song {
                isrc: None,
                title: Some("Never Gonna Give You Up".to_owned()),
                ..page
            }
            .named_with(None),
            None,
            "a title with no artist names nothing"
        );
    }

    #[test]
    fn an_album_link_page_names_the_albums_barcode_and_its_deezer_twin() {
        assert_eq!(
            Album::read(ALBUM_PAGE),
            Some(Album {
                barcode: Some(barcode("035627515026")),
                on_deezer: Some(214_959_662),
            })
        );
        assert_eq!(Album::read(PAGE), None);
    }

    #[test]
    fn the_twins_barcode_is_added_after_the_pages_unless_it_is_the_same_code() {
        let on_the_page = barcode("035627515026");

        assert_eq!(
            Album::named_with(Some(on_the_page.clone()), Some(barcode("859381157694"))),
            Some(AlbumNames {
                barcodes: vec![on_the_page.clone(), barcode("859381157694")],
            })
        );
        assert_eq!(
            Album::named_with(Some(on_the_page.clone()), Some(barcode("0035627515026"))),
            Some(AlbumNames {
                barcodes: vec![on_the_page],
            })
        );
        assert_eq!(Album::named_with(None, None), None);
    }
}
