use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use resonate_core::{AlbumId, ArtistId, MediaLocation, TrackId};
use resonate_dlna::Dlna;
use resonate_providers::{Id, Named, PlaysInPlace};
use resonate_upnp::{
    AudioKind, Catalog, Content, Entry, Held, Kind, MediaServer, Offering, Opened, Page, Picture,
    Query, Searched, ServerName, Shelf, Shelved, Song, Test, Udn, Within,
};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

const ANSWERED_WITHIN: Duration = Duration::from_millis(800);

const COVER: &[u8] = b"\xff\xd8\xff\xe0 a cover";

const SERVER_UDN: &str = "uuid:7a1d0c55-0000-4000-8000-00000000d1a7";

struct Shelf3;

fn album() -> AlbumId {
    AlbumId::new(1).expect("an id")
}

fn artist() -> ArtistId {
    ArtistId::new(1).expect("an id")
}

fn bytes_of(id: TrackId) -> Vec<u8> {
    (0..1000_u32)
        .map(|at| (u64::from(at) * 13 + id.get()) as u8)
        .collect()
}

fn songs() -> Vec<Song> {
    ["One", "Two", "Three"]
        .into_iter()
        .zip(1..)
        .map(|(title, raw)| Song {
            id: TrackId::new(raw).expect("an id"),
            title: title.to_owned(),
            artist: Some("The Band".to_owned()),
            album: Some("First".to_owned()),
            cover: Some(album()),
            genre: None,
            track_number: Some(raw as u32),
            duration: Some(Duration::from_secs(200)),
            kind: AudioKind::Flac,
            content: Content::Untouched,
            size: Some(1000),
            rate: 44_100,
            channels: 2,
            bits: Some(16),
        })
        .collect()
}

fn the_album() -> Held {
    Held {
        shelf: Shelf::Album(album()),
        title: "First".to_owned(),
        artist: Some("The Band".to_owned()),
        children: 3,
        cover: Some(album()),
    }
}

fn paged(held: Vec<Entry>, page: Page) -> Shelved {
    Shelved {
        total: held.len() as u32,
        held: held
            .into_iter()
            .skip(page.from as usize)
            .take(page.most as usize)
            .collect(),
    }
}

impl Catalog for Shelf3 {
    type Failure = io::Error;

    fn searched(&self, kind: Kind, _: Within, query: &Query, page: Page) -> io::Result<Searched> {
        let words = |query: &Query| -> Vec<String> {
            let mut held = Vec::new();
            let mut stack = vec![query.clone()];
            while let Some(one) = stack.pop() {
                match one {
                    Query::All(all) | Query::Any(all) => stack.extend(all),
                    Query::Text {
                        test: Test::Contains,
                        text,
                        ..
                    } => held.push(text.to_lowercase()),
                    _ => {}
                }
            }
            held
        };
        if kind != Kind::Song {
            return Ok(Searched::Found(paged(Vec::new(), page)));
        }
        let wanted = words(query);
        let found = songs()
            .into_iter()
            .filter(|song| {
                wanted
                    .iter()
                    .any(|word| song.title.to_lowercase().contains(word))
            })
            .map(Entry::Song)
            .collect();
        Ok(Searched::Found(paged(found, page)))
    }

    fn shelved(&self, shelf: Shelf, page: Page) -> io::Result<Option<Shelved>> {
        let held = match shelf {
            Shelf::Artists => vec![Entry::Shelf(Held {
                shelf: Shelf::Artist(artist()),
                title: "The Band".to_owned(),
                artist: None,
                children: 1,
                cover: None,
            })],
            Shelf::Artist(_) | Shelf::Albums => vec![Entry::Shelf(the_album())],
            Shelf::Album(_) | Shelf::Songs => songs().into_iter().map(Entry::Song).collect(),
            Shelf::Playlists => Vec::new(),
            Shelf::Playlist(_) => return Ok(None),
        };
        Ok(Some(paged(held, page)))
    }

    fn shelf(&self, shelf: Shelf) -> io::Result<Option<Held>> {
        Ok(match shelf {
            Shelf::Album(_) => Some(the_album()),
            _ => None,
        })
    }

    fn song(&self, id: TrackId) -> io::Result<Option<Song>> {
        Ok(songs().into_iter().find(|song| song.id == id))
    }

    fn bytes_of(&self, id: TrackId) -> io::Result<Option<Opened>> {
        Ok(self.song(id)?.map(|song| Opened {
            bytes: Box::new(Cursor::new(bytes_of(song.id))),
            kind: song.kind,
            content: song.content,
        }))
    }

    fn cover(&self, _: AlbumId) -> io::Result<Option<Picture>> {
        Ok(Some(Picture {
            bytes: COVER.to_vec(),
            mime: "image/jpeg",
        }))
    }

    fn revision(&self) -> u32 {
        1
    }
}

fn offered() -> MediaServer<Shelf3> {
    MediaServer::start(
        Arc::new(Shelf3),
        Udn::new(SERVER_UDN),
        Offering {
            heard_on: SocketAddr::new(LOOPBACK, 0),
            told: SocketAddr::new(LOOPBACK, 9),
            every: Duration::from_secs(60),
            serving_on: LOOPBACK,
            name: ServerName::new("Shelf"),
        },
    )
    .expect("the server starts")
}

fn id(text: &str) -> Id {
    Id::new(text).expect("an id")
}

#[test]
fn a_media_server_on_the_network_is_listed_by_name_and_browsed_down_to_its_songs() {
    let server = offered();
    let dlna = Dlna::searching(server.heard_at(), ANSWERED_WITHIN);

    let servers = dlna.artists().expect("the network is searched");
    let top = dlna.artist(&servers[0].id).expect("the server's top");
    let albums_shelf = top
        .albums
        .iter()
        .find(|shelf| shelf.name == "Albums")
        .expect("the albums shelf");
    let albums = dlna
        .albums_within(&albums_shelf.id)
        .expect("the shelf is browsed");
    let held = dlna.album(&albums[0].id).expect("the album is browsed");

    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "Shelf");
    assert_eq!(servers[0].id.as_str(), SERVER_UDN);
    assert_eq!(
        top.albums
            .iter()
            .map(|shelf| shelf.name.as_str())
            .collect::<Vec<_>>(),
        ["Artists", "Albums", "Songs", "Playlists"]
    );
    assert!(
        dlna.album(&albums_shelf.id)
            .expect("a shelf")
            .songs
            .is_empty()
    );
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "First");
    assert_eq!(albums[0].artist.as_deref(), Some("The Band"));
    assert_eq!(albums[0].songs, Some(3));
    assert_eq!(held.album.name, "First");
    assert_eq!(
        held.songs
            .iter()
            .map(|song| (song.title.as_str(), song.track))
            .collect::<Vec<_>>(),
        [("One", Some(1)), ("Two", Some(2)), ("Three", Some(3))]
    );
    assert_eq!(held.songs[1].artist.as_deref(), Some("The Band"));
    assert_eq!(held.songs[1].album.as_deref(), Some("First"));
    assert_eq!(held.songs[1].suffix.as_deref(), Some("flac"));
    assert_eq!(held.songs[1].length, Some(Duration::from_secs(200)));
}

#[test]
fn a_song_on_a_media_server_is_named_by_a_location_and_streamed_as_served_and_seeks() {
    let server = offered();
    let dlna = Dlna::searching(server.heard_at(), ANSWERED_WITHIN);
    let song = id(&format!("{SERVER_UDN}/song-2"));
    let location = dlna.location_of(&Named::Song(song.clone()));

    let read = MediaLocation::from_uri(&location.to_uri()).expect("the uri reads");
    let mut streamed = dlna.streamed(&song).expect("the song is streamed");
    let mut whole = Vec::new();
    streamed.read_to_end(&mut whole).expect("read whole");
    streamed.seek(SeekFrom::Start(900)).expect("a seek");
    let mut tail = Vec::new();
    streamed.read_to_end(&mut tail).expect("read on");

    assert_eq!(dlna.named_by(&read), Some(Named::Song(song)));
    assert!(streamed.is_seekable());
    assert_eq!(streamed.media_type.as_deref(), Some("audio/flac"));
    assert_eq!(whole, bytes_of(TrackId::new(2).expect("an id")));
    assert_eq!(tail, whole[900..]);
}

#[test]
fn a_search_asks_every_server_and_a_cover_is_the_servers_own_picture() {
    let server = offered();
    let dlna = Dlna::searching(server.heard_at(), ANSWERED_WITHIN);

    let found = dlna.search("tw").expect("the servers are searched");
    let cover = dlna
        .cover_of(&Named::Song(found.songs[0].id.clone()))
        .expect("the song names a cover");
    let picture = dlna.cover(&cover, 300).expect("the cover is fetched");

    assert_eq!(
        found
            .songs
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["Two"]
    );
    assert_eq!(found.songs[0].cover.as_ref(), Some(&cover));
    assert_eq!(picture, COVER);
}

#[test]
fn a_server_that_has_left_the_network_is_said_to_be_away_and_is_no_longer_listed() {
    let server = offered();
    let dlna = Dlna::searching(server.heard_at(), ANSWERED_WITHIN);
    let listed = dlna.artists().expect("the network is searched");
    drop(server);

    let streamed = dlna.streamed(&id(&format!("{SERVER_UDN}/song-1")));
    let _missed_once = dlna.artists();
    let listed_after = dlna.artists().expect("the network is searched");
    let asked_after = dlna.album(&id(&format!("{SERVER_UDN}/album-1")));

    assert_eq!(listed.len(), 1);
    assert!(streamed.is_err_and(|error| error.is_the_provider_away()));
    assert!(listed_after.is_empty());
    assert!(asked_after.is_err_and(|error| error.is_the_provider_away()));
}
