use std::{
    io::{Read, Seek, SeekFrom},
    time::Duration,
};

use resonate_upnp::{Error, MediaServers, ObjectId, PageAsked, ServerName, Udn};

use crate::offered::{bytes_of, started_as, track};

const ANSWERED_WITHIN: Duration = Duration::from_millis(800);

fn object(id: &str) -> ObjectId {
    ObjectId::new(id).expect("an object id")
}

#[test]
fn a_media_server_answering_the_search_is_listed_by_its_name_and_browsed_page_by_page() {
    let started = started_as(ServerName::new("Den shelf"));
    let servers = MediaServers::searching(started.server.heard_at(), ANSWERED_WITHIN);

    let found = servers.found().expect("the search is answered");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name(), "Den shelf");
    assert_eq!(found[0].udn(), started.server.udn());

    let server = found[0].clone().paged_by(3);
    let top = server
        .children(&ObjectId::root())
        .expect("the root is browsed");
    let titles: Vec<&str> = top
        .containers
        .iter()
        .map(|container| container.title.as_str())
        .collect();
    let page = server
        .page_of_children(&object("songs"), PageAsked { from: 2, most: 3 })
        .expect("a page of songs");
    let songs = server.children(&object("songs")).expect("every song");

    assert_eq!(titles, ["Artists", "Albums", "Songs", "Playlists"]);
    assert!(top.items.is_empty());
    assert_eq!(page.returned, 3);
    assert_eq!(page.total, Some(8));
    assert_eq!(page.listing.items.len(), 3);
    assert_eq!(songs.items.len(), 8);
    assert_eq!(
        songs
            .items
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        [
            "One",
            "Two & Three",
            "<Three>",
            "Four",
            "Five",
            "Only One",
            "Demo",
            "Stray"
        ]
    );
    assert_eq!(songs.items[0].album.as_deref(), Some("Album 1"));
    assert_eq!(songs.items[0].artist.as_deref(), Some("The Band"));
    assert_eq!(songs.items[0].track, Some(1));
}

#[test]
fn a_song_found_on_a_server_is_fetched_as_served_and_seeks_by_range() {
    let started = started_as(None);
    let servers = MediaServers::searching(started.server.heard_at(), ANSWERED_WITHIN);

    let server = servers
        .server(started.server.udn())
        .expect("the server is found");

    let described = server
        .metadata(&object("song-2"))
        .expect("a song's metadata");
    let song = &described.items[0];
    let played = song.played_from().expect("a resource");
    let mut fetched = server.fetched(&played.at).expect("the song is fetched");
    let mut whole = Vec::new();

    fetched.read_to_end(&mut whole).expect("read whole");

    let landed = fetched.seek(SeekFrom::Start(600)).expect("a seek");
    let mut after = Vec::new();

    fetched.read_to_end(&mut after).expect("read from there");

    assert_eq!(song.title, "Two & Three");
    assert_eq!(played.mime.as_deref(), Some("audio/flac"));
    assert_eq!(played.size, Some(1000));
    assert_eq!(fetched.length(), Some(1000));
    assert_eq!(whole, bytes_of(track(2)));
    assert_eq!(landed, 600);
    assert_eq!(after, bytes_of(track(2))[600..]);
}

#[test]
fn a_search_on_a_server_finds_the_songs_named() {
    let started = started_as(None);
    let servers = MediaServers::searching(started.server.heard_at(), ANSWERED_WITHIN);

    let server = servers
        .server(started.server.udn())
        .expect("the server is found");

    let found = server.songs_named("three").expect("the server searches");

    assert_eq!(
        found
            .items
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["Two & Three", "<Three>"]
    );
}

#[test]
fn a_server_that_leaves_is_gone_after_two_searches_miss_it_and_then_is_said_not_to_answer() {
    let started = started_as(None);
    let udn = started.server.udn().clone();
    let servers = MediaServers::searching(started.server.heard_at(), ANSWERED_WITHIN);
    let found = servers.found().expect("the search is answered");

    drop(started);

    let once = servers.found().expect("a search hearing nothing");
    let twice = servers.found().expect("a search hearing nothing");
    let asked = servers.server(&udn);

    assert_eq!(found.len(), 1);
    assert_eq!(once.len(), 1);
    assert!(twice.is_empty());
    assert!(matches!(asked, Err(Error::NoSuchServer(gone)) if gone == udn));
    assert!(matches!(
        servers.server(&Udn::new("uuid:never")),
        Err(Error::NoSuchServer(_))
    ));
}
