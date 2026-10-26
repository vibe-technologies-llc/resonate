use resonate_library::{Isrc, LinkNames, LinkedPlaylist, ListedSong, LookupOp};
use serde_json::Value;

use crate::{
    Client, Error, Host, Result,
    client::LARGEST_DOCUMENT,
    shared::{stated, unescaped},
    youtube::clock,
};

const EMBEDDED_PLAYLIST: &str = "/playlists/";
const A_TRACK_PAGE: &str = "/track/";
const A_PRODUCT_ID: &str = "product-id=\"";
const A_QUOTE: char = '"';
const STRUCTURED_DATA_OPENS: &str = "<script type=\"application/ld+json\">";
const STRUCTURED_DATA_CLOSES: &str = "</script>";
const A_RECORDING: &str = "MusicRecording";
const THE_CODE: &str = "isrcCode";
const THE_TYPE: &str = "@type";
const THE_NAME: &str = "class=\"media-album\"";
const THE_NAME_CLOSES: &str = "</h1>";
const AN_ITEM: &str = "<list-item ";
const AN_ITEM_CLOSES: &str = "</list-item>";
const A_TRACK: &str = "product-type=\"track\"";
const A_TITLE: &str = "<span slot=\"title\">";
const AN_ARTIST: &str = "<span slot=\"artist\">";
const A_SPAN_CLOSES: &str = "</span>";
const A_LENGTH: &str = "<time slot=\"duration\">";
const A_TIME_CLOSES: &str = "</time>";
const AN_ANCHOR: &str = "<a ";
const AN_ANCHOR_CLOSES: &str = "</a>";
const ARTISTS_APART: &str = ", ";

pub(crate) fn playlist_named(client: &Client, playlist: &str) -> Result<Option<LinkedPlaylist>> {
    let page = format!("{}{EMBEDDED_PLAYLIST}{playlist}", Host::TidalEmbed.base());
    let held = client.bytes(
        Host::TidalEmbed,
        LookupOp::FollowLink,
        &page,
        LARGEST_DOCUMENT,
    )?;

    let Some(embed) = held.and_then(|held| embedded(&String::from_utf8_lossy(&held))) else {
        return Ok(None);
    };

    Ok(Some(embed.coded_by(client)?))
}

#[cfg(fuzzing)]
pub(crate) fn read_embedded(page: &str) -> Option<LinkedPlaylist> {
    embedded(page).map(Embed::linked)
}

#[derive(Debug, PartialEq, Eq)]
struct Embed {
    name: String,
    songs: Vec<Listed>,
}

#[derive(Debug, PartialEq, Eq)]
struct Listed {
    track: Option<String>,
    names: LinkNames,
}

impl Embed {
    fn linked(self) -> LinkedPlaylist {
        LinkedPlaylist {
            name: self.name,
            songs: self
                .songs
                .into_iter()
                .map(|song| ListedSong::Named(song.names))
                .collect(),
        }
    }

    fn coded_by(mut self, client: &Client) -> Result<LinkedPlaylist> {
        let mut answering = true;
        for song in &mut self.songs {
            let Some(track) = song.track.as_deref().filter(|_| answering) else {
                continue;
            };
            match isrc_of(client, track) {
                Ok(Some(isrc)) => song.names.isrcs.push(isrc),
                Ok(None) => {}
                Err(error @ Error::Offline { .. }) => return Err(error),
                Err(error) => {
                    tracing::debug!(%error, "TIDAL's song pages are not asked further; the strict search names the rest");
                    answering = false;
                }
            }
        }

        Ok(self.linked())
    }
}

fn isrc_of(client: &Client, track: &str) -> Result<Option<Isrc>> {
    let page = format!("{}{A_TRACK_PAGE}{track}", Host::Tidal.base());
    let held = client.bytes(Host::Tidal, LookupOp::FollowLink, &page, LARGEST_DOCUMENT)?;

    Ok(held.and_then(|held| isrc_in(&String::from_utf8_lossy(&held))))
}

fn isrc_in(page: &str) -> Option<Isrc> {
    page.split(STRUCTURED_DATA_OPENS)
        .skip(1)
        .filter_map(|block| block.split(STRUCTURED_DATA_CLOSES).next())
        .filter_map(|block| serde_json::from_str::<Value>(block).ok())
        .filter(|data| data.get(THE_TYPE).and_then(Value::as_str) == Some(A_RECORDING))
        .find_map(|data| {
            data.get(THE_CODE)
                .and_then(Value::as_str)
                .and_then(|code| Isrc::new(code).ok())
        })
}

fn embedded(page: &str) -> Option<Embed> {
    let name = named(page)?;
    let songs = page
        .split(AN_ITEM)
        .skip(1)
        .filter_map(|item| item.split(AN_ITEM_CLOSES).next().and_then(listed))
        .collect();

    Some(Embed { name, songs })
}

fn named(page: &str) -> Option<String> {
    let heading = page.split(THE_NAME).nth(1)?;
    let heading = heading.split(THE_NAME_CLOSES).next()?;
    let text = texts_of_anchors(heading).into_iter().next()?;

    stated(Some(&unescaped(&text)))
}

fn track_of(head: &str) -> Option<String> {
    let id = head.split_once(A_PRODUCT_ID)?.1.split(A_QUOTE).next()?;

    (!id.is_empty() && id.bytes().all(|digit| digit.is_ascii_digit())).then(|| id.to_owned())
}

fn listed(item: &str) -> Option<Listed> {
    let (head, body) = item.split_once('>')?;
    if !head.contains(A_TRACK) {
        return None;
    }
    let title = stated(Some(&unescaped(between(body, A_TITLE, A_SPAN_CLOSES)?)))?;
    let artists: Vec<String> = texts_of_anchors(between(body, AN_ARTIST, A_SPAN_CLOSES)?)
        .iter()
        .filter_map(|artist| stated(Some(&unescaped(artist))))
        .collect();
    if artists.is_empty() {
        return None;
    }

    Some(Listed {
        track: track_of(head),
        names: LinkNames {
            isrcs: Vec::new(),
            length: between(body, A_LENGTH, A_TIME_CLOSES).and_then(clock),
            title: Some(title),
            artist: Some(artists.join(ARTISTS_APART)),
        },
    })
}

fn between<'a>(text: &'a str, opens: &str, closes: &str) -> Option<&'a str> {
    text.split_once(opens)?.1.split(closes).next()
}

fn texts_of_anchors(markup: &str) -> Vec<String> {
    markup
        .split(AN_ANCHOR)
        .skip(1)
        .filter_map(|anchor| {
            let (_, text) = anchor.split_once('>')?;
            Some(text.split(AN_ANCHOR_CLOSES).next()?.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn named_song(length: u64, title: &str, artist: &str) -> ListedSong {
        ListedSong::Named(LinkNames {
            isrcs: Vec::new(),
            length: Some(Duration::from_secs(length)),
            title: Some(title.to_owned()),
            artist: Some(artist.to_owned()),
        })
    }

    #[test]
    fn a_tidal_playlist_names_its_title_and_each_song_by_its_title_artists_and_length() {
        let linked = embedded(include_str!("../tests/fixtures/tidal_playlist_embed.html"))
            .map(Embed::linked)
            .expect("a playlist read off the embed page");

        assert_eq!(linked.name, "Pop Hits");
        assert_eq!(linked.songs.len(), 50);
        assert_eq!(
            linked.songs.first(),
            Some(&named_song(209, "Housewife", "Tate McRae"))
        );
        assert!(linked.songs.iter().all(|song| matches!(
            song,
            ListedSong::Named(names) if names.length.is_some()
        )));
    }

    #[test]
    fn a_song_billing_several_artists_names_them_all_and_its_escapes_read_as_letters() {
        let linked = embedded(include_str!("../tests/fixtures/tidal_playlist_embed.html"))
            .map(Embed::linked)
            .expect("a playlist read off the embed page");
        let titles: Vec<&str> = linked
            .songs
            .iter()
            .filter_map(|song| match song {
                ListedSong::Named(names) => names.title.as_deref(),
                ListedSong::Recording(_) | ListedSong::Uploaded(_) => None,
            })
            .collect();
        let artists: Vec<&str> = linked
            .songs
            .iter()
            .filter_map(|song| match song {
                ListedSong::Named(names) => names.artist.as_deref(),
                ListedSong::Recording(_) | ListedSong::Uploaded(_) => None,
            })
            .collect();

        assert!(titles.contains(&"Choosin' Texas"));
        assert!(
            titles.contains(&"I Knew It, I Knew You (Acoustic Version (From \"Toy Story 5\"))")
        );
        assert!(artists.contains(&"Tyla, Zara Larsson"));
        assert!(artists.contains(&"KAROL G, Judeline, rusowsky"));
        assert!(titles.iter().all(|title| !title.contains('&')));
    }

    #[test]
    fn only_a_track_with_a_title_and_an_artist_is_a_song_and_a_page_without_a_name_is_nothing() {
        let page = concat!(
            "<h1 class=\"media-album\" title=\"Album: Mix &amp; Match\"><a href=\"x\">Mix &amp; Match</a></h1>",
            "<list-item product-id=\"1\" product-type=\"track\" >",
            "<span slot=\"title\">One</span>",
            "<span slot=\"artist\"><a href=\"a\">Ann</a></span>",
            "<time slot=\"duration\">1:05</time></list-item>",
            "<list-item product-id=\"2\" product-type=\"video\" >",
            "<span slot=\"title\">Clip</span>",
            "<span slot=\"artist\"><a href=\"a\">Ann</a></span></list-item>",
            "<list-item product-id=\"3\" product-type=\"track\" >",
            "<span slot=\"title\">Nobody</span>",
            "<span slot=\"artist\"></span></list-item>",
            "<list-item product-id=\"4\" product-type=\"track\" >",
            "<span slot=\"artist\"><a href=\"a\">Ann</a></span></list-item>",
            "<list-item product-id=\"5\" product-type=\"track\" >",
            "<span slot=\"title\">Timeless</span>",
            "<span slot=\"artist\"><a href=\"a\">Ann</a></span></list-item>",
        );

        let embed = embedded(page).expect("a playlist");
        let tracks: Vec<Option<String>> =
            embed.songs.iter().map(|song| song.track.clone()).collect();
        let linked = embed.linked();

        assert_eq!(linked.name, "Mix & Match");
        assert_eq!(linked.songs.len(), 2);
        assert_eq!(tracks, [Some("1".to_owned()), Some("5".to_owned())]);
        assert_eq!(linked.songs[0], named_song(65, "One", "Ann"));
        assert_eq!(
            linked.songs[1],
            ListedSong::Named(LinkNames {
                isrcs: Vec::new(),
                length: None,
                title: Some("Timeless".to_owned()),
                artist: Some("Ann".to_owned()),
            })
        );
        assert_eq!(embedded("<html>no playlist</html>"), None);
        assert_eq!(
            embedded("<h1 class=\"media-album\"><a href=\"x\"> </a></h1>"),
            None
        );
    }

    #[test]
    fn each_song_of_the_embed_page_keeps_the_track_it_is() {
        let embed = embedded(include_str!("../tests/fixtures/tidal_playlist_embed.html"))
            .expect("a playlist read off the embed page");

        assert_eq!(embed.songs.len(), 50);
        assert_eq!(embed.songs[0].track.as_deref(), Some("570268437"));
        assert!(embed.songs.iter().all(|song| song.track.is_some()));
        assert_eq!(
            track_of("tabindex=\"1\" product-id=\"12x\" product-type=\"track\""),
            None
        );
        assert_eq!(track_of("tabindex=\"1\" product-type=\"track\""), None);
    }

    #[test]
    fn a_track_page_names_its_isrc_in_its_structured_data_and_nothing_else_does() {
        let page = include_str!("../tests/fixtures/tidal_track_page.html");

        assert_eq!(
            isrc_in(page),
            Some(Isrc::new("USRC12601916").expect("a code"))
        );
        assert_eq!(isrc_in("<html>no structured data</html>"), None);
        assert_eq!(
            isrc_in(
                "<script type=\"application/ld+json\">{\"@type\":\"MusicAlbum\",\"isrcCode\":\"USRC12601916\"}</script>"
            ),
            None
        );
        assert_eq!(
            isrc_in(
                "<script type=\"application/ld+json\">{\"@type\":\"MusicRecording\",\"isrcCode\":\"not a code\"}</script>"
            ),
            None
        );
    }
}
