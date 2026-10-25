use resonate_library::{LinkNames, LinkedPlaylist, ListedSong, LookupOp};

use crate::{
    Client, Host, Result,
    client::LARGEST_DOCUMENT,
    shared::{stated, unescaped},
    youtube::clock,
};

const EMBEDDED_PLAYLIST: &str = "/playlists/";
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

    Ok(held.and_then(|held| embedded(&String::from_utf8_lossy(&held))))
}

#[cfg(fuzzing)]
pub(crate) fn read_embedded(page: &str) -> Option<LinkedPlaylist> {
    embedded(page)
}

fn embedded(page: &str) -> Option<LinkedPlaylist> {
    let name = named(page)?;
    let songs = page
        .split(AN_ITEM)
        .skip(1)
        .filter_map(|item| item.split(AN_ITEM_CLOSES).next().and_then(listed))
        .map(ListedSong::Named)
        .collect();

    Some(LinkedPlaylist { name, songs })
}

fn named(page: &str) -> Option<String> {
    let heading = page.split(THE_NAME).nth(1)?;
    let heading = heading.split(THE_NAME_CLOSES).next()?;
    let text = texts_of_anchors(heading).into_iter().next()?;

    stated(Some(&unescaped(&text)))
}

fn listed(item: &str) -> Option<LinkNames> {
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

    Some(LinkNames {
        isrcs: Vec::new(),
        length: between(body, A_LENGTH, A_TIME_CLOSES).and_then(clock),
        title: Some(title),
        artist: Some(artists.join(ARTISTS_APART)),
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

        let linked = embedded(page).expect("a playlist");

        assert_eq!(linked.name, "Mix & Match");
        assert_eq!(linked.songs.len(), 2);
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
}
