use std::{
    io::{self, Cursor, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{AlbumId, ArtistId, PlaylistId, TrackId};
use resonate_upnp::{
    AudioKind, Catalog, Content, Entry, Field, Held, Kind, MediaServer, Offering, Opened, Page,
    Picture, Query, Searched, ServerName, Shelf, Shelved, Song, Test, Udn, Within,
};
use roxmltree::Document;

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

const HEARD_WITHIN: Duration = Duration::from_secs(5);

const MODERATED_AT_LEAST: Duration = Duration::from_millis(1_800);

const DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

const SERVER_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";

const COVER: &[u8] = b"\xff\xd8\xff\xe0 a picture";

type Listed = (PlaylistId, &'static str, Vec<TrackId>);

pub(crate) struct Shelves {
    songs: Mutex<Vec<Song>>,
    albums: Vec<(AlbumId, &'static str, ArtistId)>,
    artists: Vec<(ArtistId, &'static str)>,
    playlists: Mutex<Vec<Listed>>,
    revision: AtomicU32,
}

fn artist(raw: u64) -> ArtistId {
    ArtistId::new(raw).expect("an id")
}

fn album(raw: u64) -> AlbumId {
    AlbumId::new(raw).expect("an id")
}

pub(crate) fn track(raw: u64) -> TrackId {
    TrackId::new(raw).expect("an id")
}

pub(crate) fn bytes_of(id: TrackId) -> Vec<u8> {
    (0..1000_u32)
        .map(|at| (at as u64 * 7 + id.get()) as u8)
        .collect()
}

impl Shelves {
    fn new() -> Self {
        let song = |raw: u64, title: &str, on: u64, by: &str| Song {
            id: track(raw),
            title: title.to_owned(),
            artist: Some(by.to_owned()),
            album: Some(format!("Album {on}")),
            cover: Some(album(on)),
            genre: None,
            track_number: Some(raw as u32),
            duration: Some(Duration::from_secs(180)),
            kind: AudioKind::Flac,
            content: Content::Untouched,
            size: Some(1000),
            rate: 44_100,
            channels: 2,
            bits: Some(16),
        };
        Self {
            songs: Mutex::new(vec![
                song(1, "One", 1, "The Band"),
                song(2, "Two & Three", 1, "The Band"),
                song(3, "<Three>", 1, "The Band"),
                song(4, "Four", 2, "The Band"),
                song(5, "Five", 2, "The Band"),
                song(6, "Only One", 3, "Soloist"),
                Song {
                    album: None,
                    cover: None,
                    ..song(7, "Demo", 3, "Soloist")
                },
                Song {
                    artist: None,
                    album: None,
                    cover: None,
                    ..song(8, "Stray", 3, "")
                },
            ]),
            albums: vec![
                (album(1), "Album 1", artist(1)),
                (album(2), "Album 2", artist(1)),
                (album(3), "Solo", artist(2)),
            ],
            artists: vec![(artist(1), "The Band"), (artist(2), "Soloist")],
            playlists: Mutex::new(vec![(
                PlaylistId::new(1).expect("an id"),
                "Mixed",
                vec![track(3), track(6), track(1)],
            )]),
            revision: AtomicU32::new(7),
        }
    }

    fn songs(&self) -> Vec<Song> {
        self.songs.lock().clone()
    }

    fn playlists(&self) -> Vec<Listed> {
        self.playlists.lock().clone()
    }

    fn changed(&self, change: impl FnOnce(&Self)) {
        change(self);
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    fn songs_on(&self, on: AlbumId) -> Vec<Song> {
        self.songs()
            .into_iter()
            .filter(|song| song.cover == Some(on))
            .collect()
    }

    fn named(&self, artist: ArtistId) -> Option<&'static str> {
        self.artists
            .iter()
            .find(|(held, _)| *held == artist)
            .map(|(_, name)| *name)
    }

    fn songs_within(&self, within: Within) -> Vec<Song> {
        match within {
            Within::Catalog => self.songs(),
            Within::Artists => self
                .songs()
                .into_iter()
                .filter(|song| {
                    self.artists
                        .iter()
                        .any(|(_, name)| song.artist.as_deref() == Some(*name))
                })
                .collect(),
            Within::Albums => self
                .songs()
                .into_iter()
                .filter(|song| song.cover.is_some())
                .collect(),
            Within::Artist(artist) => self
                .songs()
                .into_iter()
                .filter(|song| song.artist.as_deref() == self.named(artist))
                .collect(),
            Within::Album(on) => self.songs_on(on),
            Within::Playlist(id) => self
                .playlists()
                .into_iter()
                .filter(|(held, ..)| *held == id)
                .flat_map(|(_, _, listed)| listed)
                .filter_map(|id| self.songs().into_iter().find(|song| song.id == id))
                .collect(),
        }
    }

    fn album_held(&self, id: AlbumId) -> Option<Held> {
        let (_, title, by) = self.albums.iter().find(|(held, ..)| *held == id)?;
        Some(Held {
            shelf: Shelf::Album(id),
            title: (*title).to_owned(),
            artist: self.named(*by).map(str::to_owned),
            children: self.songs_on(id).len() as u32,
            cover: Some(id),
        })
    }
}

type Fields<'a> = Vec<(Field, Option<&'a str>)>;

fn holds(query: &Query, fields: &[(Field, Option<&str>)]) -> Option<bool> {
    let field = |asked: Field| {
        fields
            .iter()
            .find(|(field, _)| *field == asked)
            .and_then(|(_, held)| *held)
    };
    Some(match query {
        Query::Everything => true,
        Query::All(all) => {
            for one in all {
                if !holds(one, fields)? {
                    return Some(false);
                }
            }
            true
        }
        Query::Any(any) => {
            for one in any {
                if holds(one, fields)? {
                    return Some(true);
                }
            }
            false
        }
        Query::Text {
            field: asked,
            test,
            text,
        } => {
            let held = field(*asked).map(str::to_lowercase);
            let text = text.to_lowercase();
            match (test, held) {
                (Test::Is, Some(held)) => held == text,
                (Test::Contains, Some(held)) => held.contains(&text),
                (Test::IsNot, Some(held)) => held != text,
                (Test::Lacks, Some(held)) => !held.contains(&text),
                (Test::Is | Test::Contains, None) => false,
                (Test::IsNot | Test::Lacks, None) => true,
            }
        }
        Query::Present { .. } => return None,
    })
}

fn paged(entries: Vec<Entry>, page: Page) -> Shelved {
    Shelved {
        total: entries.len() as u32,
        held: entries
            .into_iter()
            .skip(page.from as usize)
            .take(page.most as usize)
            .collect(),
    }
}

impl Catalog for Shelves {
    type Failure = io::Error;

    fn searched(
        &self,
        kind: Kind,
        within: Within,
        query: &Query,
        page: Page,
    ) -> io::Result<Searched> {
        let songs = self.songs_within(within);
        let candidates: Vec<(Entry, Fields<'_>)> = match (kind, within) {
            (Kind::Song, _) => songs
                .iter()
                .map(|song| {
                    (
                        Entry::Song(song.clone()),
                        vec![
                            (Field::Title, Some(song.title.as_str())),
                            (Field::Artist, song.artist.as_deref()),
                            (Field::Album, song.album.as_deref()),
                            (Field::Genre, song.genre.as_deref()),
                        ],
                    )
                })
                .collect(),
            (
                Kind::Album,
                Within::Catalog | Within::Artists | Within::Albums | Within::Artist(_),
            ) => self
                .albums
                .iter()
                .filter(|(_, _, by)| match within {
                    Within::Artist(artist) => *by == artist,
                    _ => true,
                })
                .filter_map(|(id, title, by)| {
                    let held = self.album_held(*id)?;
                    Some((
                        Entry::Shelf(held),
                        vec![
                            (Field::Title, Some(*title)),
                            (Field::Artist, self.named(*by)),
                        ],
                    ))
                })
                .collect(),
            (Kind::Artist, Within::Catalog | Within::Artists) => self
                .artists
                .iter()
                .filter_map(|(id, name)| {
                    let held = self.shelf(Shelf::Artist(*id)).ok().flatten()?;
                    Some((Entry::Shelf(held), vec![(Field::Title, Some(*name))]))
                })
                .collect(),
            (Kind::Playlist, Within::Catalog) => self
                .playlists()
                .into_iter()
                .filter_map(|(id, name, _)| {
                    let held = self.shelf(Shelf::Playlist(id)).ok().flatten()?;
                    Some((Entry::Shelf(held), vec![(Field::Title, Some(name))]))
                })
                .collect(),
            (Kind::Album | Kind::Artist | Kind::Playlist, _) => Vec::new(),
        };
        let mut found = Vec::new();

        for (entry, fields) in candidates {
            match holds(query, &fields) {
                Some(true) => found.push(entry),
                Some(false) => {}
                None => return Ok(Searched::Unanswered),
            }
        }
        Ok(Searched::Found(paged(found, page)))
    }

    fn shelved(&self, shelf: Shelf, page: Page) -> io::Result<Option<Shelved>> {
        let entries: Vec<Entry> = match shelf {
            Shelf::Artists => self
                .artists
                .iter()
                .filter_map(|(id, _)| self.shelf(Shelf::Artist(*id)).ok().flatten())
                .map(Entry::Shelf)
                .collect(),
            Shelf::Artist(id) => {
                if !self.artists.iter().any(|(held, _)| *held == id) {
                    return Ok(None);
                }
                self.albums
                    .iter()
                    .filter(|(_, _, by)| *by == id)
                    .filter_map(|(album, ..)| self.album_held(*album))
                    .map(Entry::Shelf)
                    .collect()
            }
            Shelf::Albums => self
                .albums
                .iter()
                .filter_map(|(album, ..)| self.album_held(*album))
                .map(Entry::Shelf)
                .collect(),
            Shelf::Album(id) => {
                if self.album_held(id).is_none() {
                    return Ok(None);
                }
                self.songs_on(id).into_iter().map(Entry::Song).collect()
            }
            Shelf::Songs => self.songs().into_iter().map(Entry::Song).collect(),
            Shelf::Playlists => self
                .playlists()
                .into_iter()
                .filter_map(|(id, ..)| self.shelf(Shelf::Playlist(id)).ok().flatten())
                .map(Entry::Shelf)
                .collect(),
            Shelf::Playlist(id) => {
                let Some((_, _, held)) = self
                    .playlists()
                    .into_iter()
                    .find(|(named, ..)| *named == id)
                else {
                    return Ok(None);
                };
                held.iter()
                    .filter_map(|id| self.song(*id).ok().flatten())
                    .map(Entry::Song)
                    .collect()
            }
        };
        Ok(Some(paged(entries, page)))
    }

    fn shelf(&self, shelf: Shelf) -> io::Result<Option<Held>> {
        Ok(match shelf {
            Shelf::Artist(id) => {
                self.artists
                    .iter()
                    .find(|(held, _)| *held == id)
                    .map(|(_, name)| Held {
                        shelf,
                        title: (*name).to_owned(),
                        artist: None,
                        children: self.albums.iter().filter(|(_, _, by)| *by == id).count() as u32,
                        cover: None,
                    })
            }
            Shelf::Album(id) => self.album_held(id),
            Shelf::Playlist(id) => self
                .playlists()
                .into_iter()
                .find(|(held, ..)| *held == id)
                .map(|(_, name, songs)| Held {
                    shelf,
                    title: name.to_owned(),
                    artist: None,
                    children: songs.len() as u32,
                    cover: None,
                }),
            Shelf::Artists | Shelf::Albums | Shelf::Songs | Shelf::Playlists => None,
        })
    }

    fn song(&self, id: TrackId) -> io::Result<Option<Song>> {
        Ok(self.songs().into_iter().find(|song| song.id == id))
    }

    fn bytes_of(&self, id: TrackId) -> io::Result<Option<Opened>> {
        Ok(self.song(id)?.map(|song| Opened {
            bytes: Box::new(Cursor::new(bytes_of(song.id))),
            kind: song.kind,
            content: song.content,
        }))
    }

    fn cover(&self, id: AlbumId) -> io::Result<Option<Picture>> {
        Ok(self.album_held(id).map(|_| Picture {
            bytes: COVER.to_vec(),
            mime: "image/jpeg",
        }))
    }

    fn revision(&self) -> u32 {
        self.revision.load(Ordering::Acquire)
    }
}

pub(crate) struct Started {
    pub(crate) server: MediaServer<Shelves>,
    told: UdpSocket,
    shelves: Arc<Shelves>,
}

fn started() -> Started {
    started_as(None)
}

pub(crate) fn started_as(name: Option<ServerName>) -> Started {
    let told = UdpSocket::bind(SocketAddr::new(LOOPBACK, 0)).expect("a socket told of the server");

    told.set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");

    let shelves = Arc::new(Shelves::new());
    let server = MediaServer::start(
        Arc::clone(&shelves),
        Udn::new("uuid:4c1b3a27-0000-4000-8000-000000000001"),
        Offering {
            heard_on: SocketAddr::new(LOOPBACK, 0),
            told: told.local_addr().expect("an address"),
            every: Duration::from_secs(60),
            serving_on: LOOPBACK,
            name,
        },
    )
    .expect("the server starts");
    Started {
        server,
        told,
        shelves,
    }
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (named, value) = line.split_once(':')?;
        named
            .trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

fn heard(socket: &UdpSocket, until: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + HEARD_WITHIN;
    let mut buffer = [0; 8192];

    while Instant::now() < deadline {
        let Ok(read) = socket.recv(&mut buffer) else {
            continue;
        };
        let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
        if until(&text) {
            return text;
        }
    }
    panic!("nothing was heard in time");
}

struct Answer {
    status: u16,
    head: String,
    body: Vec<u8>,
}

fn asked(address: SocketAddr, request: &str, body: &[u8]) -> Answer {
    let mut stream = TcpStream::connect(address).expect("connects");

    stream
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");
    stream.write_all(request.as_bytes()).expect("asks");
    stream.write_all(body).expect("sends the body");

    let mut answer = Vec::new();

    stream.read_to_end(&mut answer).expect("an answer");

    let ends = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a head")
        + 4;
    let head = String::from_utf8_lossy(&answer[..ends]).into_owned();
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .expect("a status");
    Answer {
        status,
        head,
        body: answer[ends..].to_vec(),
    }
}

fn fetched(address: SocketAddr, path: &str, range: Option<&str>) -> Answer {
    let range = range.map_or_else(String::new, |range| format!("Range: {range}\r\n"));
    asked(
        address,
        &format!("GET {path} HTTP/1.1\r\nHost: {address}\r\n{range}Connection: close\r\n\r\n"),
        &[],
    )
}

struct Browsed {
    status: u16,
    body: String,
}

impl Browsed {
    fn named(&self, name: &str) -> Option<String> {
        let document = Document::parse(&self.body).expect("an envelope");
        document
            .descendants()
            .find(|node| node.tag_name().name() == name)
            .map(|node| node.text().unwrap_or_default().to_owned())
    }

    fn didl(&self) -> String {
        self.named("Result").expect("a result")
    }

    fn ids(&self, tag: &str) -> Vec<String> {
        let didl = self.didl();
        let document = Document::parse(&didl).expect("a DIDL document");
        document
            .descendants()
            .filter(|node| node.tag_name().name() == tag)
            .filter_map(|node| node.attribute("id").map(str::to_owned))
            .collect()
    }

    fn counted(&self, name: &str) -> u32 {
        self.named(name)
            .and_then(|count| count.parse().ok())
            .expect("a count")
    }
}

fn browsed(address: SocketAddr, object: &str, flag: &str, from: u32, most: u32) -> Browsed {
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:Browse \
         xmlns:u=\"{DIRECTORY}\"><ObjectID>{object}</ObjectID><BrowseFlag>{flag}</BrowseFlag>\
         <Filter>*</Filter><StartingIndex>{from}</StartingIndex><RequestedCount>{most}\
         </RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"
    );
    let answer = asked(
        address,
        &format!(
            "POST /control/ContentDirectory HTTP/1.1\r\nHost: {address}\r\nContent-Type: \
             text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{DIRECTORY}#Browse\"\r\n\
             Content-Length: {}\r\n\r\n",
            body.len()
        ),
        body.as_bytes(),
    );
    Browsed {
        status: answer.status,
        body: String::from_utf8(answer.body).expect("text"),
    }
}

fn children(address: SocketAddr, object: &str, from: u32, most: u32) -> Browsed {
    browsed(address, object, "BrowseDirectChildren", from, most)
}

fn discovered(server: &MediaServer<Shelves>) -> SocketAddr {
    let searching = UdpSocket::bind(SocketAddr::new(LOOPBACK, 0)).expect("a search socket");

    searching
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");

    let search = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: \
         1\r\nST: {SERVER_TYPE}\r\n\r\n"
    );

    searching
        .send_to(search.as_bytes(), server.heard_at())
        .expect("searches");

    let answer = heard(&searching, |answer| answer.starts_with("HTTP/1.1 200"));
    let location = header(&answer, "LOCATION").expect("a location");

    assert_eq!(header(&answer, "ST"), Some(SERVER_TYPE));
    assert_eq!(
        header(&answer, "USN"),
        Some(format!("{}::{SERVER_TYPE}", server.udn()).as_str())
    );
    assert!(header(&answer, "SERVER").is_some_and(|said| said.starts_with("resonate/")));

    location
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.parse().ok())
        .expect("a location on loopback")
}

#[test]
fn a_control_point_finds_the_server_browses_its_shelves_page_by_page_and_fetches_a_song() {
    let Started { server, told, .. } = started();

    let alive = heard(&told, |told| told.contains("NTS: ssdp:alive"));

    assert!(alive.starts_with("NOTIFY * HTTP/1.1"));

    let at = discovered(&server);
    let described = fetched(at, "/description.xml", None);
    let description = String::from_utf8(described.body).expect("text");

    assert_eq!(described.status, 200);
    assert!(description.contains("<friendlyName>Resonate</friendlyName>"));
    assert!(description.contains(server.udn().as_str()));
    assert!(description.contains("<controlURL>/control/ContentDirectory</controlURL>"));
    assert_eq!(
        fetched(at, "/ContentDirectory.xml", None).status,
        200,
        "the service's description"
    );

    let root = children(at, "0", 0, 0);

    assert_eq!(root.status, 200);
    assert_eq!(
        root.ids("container"),
        ["artists", "albums", "songs", "playlists"]
    );
    assert_eq!(root.counted("TotalMatches"), 4);
    assert_eq!(root.counted("UpdateID"), 7);

    let artists = children(at, "artists", 0, 0);

    assert_eq!(artists.ids("container"), ["artist-1", "artist-2"]);

    let albums_of_the_artist = children(at, "artist-1", 0, 0);

    assert_eq!(
        albums_of_the_artist.ids("container"),
        ["album-1", "album-2"]
    );

    let on_the_album = children(at, "album-1", 0, 0);

    assert_eq!(on_the_album.ids("item"), ["song-1", "song-2", "song-3"]);
    assert!(on_the_album.didl().contains("&lt;Three&gt;"));

    let listed = children(at, "playlist-1", 0, 0);

    assert_eq!(listed.ids("item"), ["song-3", "song-6", "song-1"]);

    let mut paged = Vec::new();
    let mut from = 0;

    loop {
        let page = children(at, "songs", from, 2);

        assert_eq!(page.counted("TotalMatches"), 8);

        let returned = page.counted("NumberReturned");

        assert!(returned <= 2);

        paged.extend(page.ids("item"));
        from += returned;
        if returned == 0 || from >= 8 {
            break;
        }
    }

    assert_eq!(
        paged,
        [
            "song-1", "song-2", "song-3", "song-4", "song-5", "song-6", "song-7", "song-8"
        ]
    );

    let itself = browsed(at, "album-2", "BrowseMetadata", 0, 0);

    assert_eq!(itself.ids("container"), ["album-2"]);
    assert!(itself.didl().contains("parentID=\"albums\""));
    assert!(itself.didl().contains("childCount=\"2\""));

    let didl = on_the_album.didl();
    let song_at = didl
        .split("<res ")
        .nth(1)
        .and_then(|res| res.split('>').nth(1))
        .and_then(|address| address.split('<').next())
        .expect("a resource address");
    let path = song_at
        .strip_prefix(&format!("http://{at}"))
        .expect("an address on the server");

    let whole = fetched(at, path, None);

    assert_eq!(whole.status, 200);
    assert_eq!(whole.body, bytes_of(track(1)));
    assert_eq!(header(&whole.head, "Content-Type"), Some("audio/flac"));
    assert_eq!(header(&whole.head, "Accept-Ranges"), Some("bytes"));
    assert!(header(&whole.head, "contentFeatures.dlna.org").is_some());

    let part = fetched(at, path, Some("bytes=100-199"));

    assert_eq!(part.status, 206);
    assert_eq!(part.body, bytes_of(track(1))[100..200]);
    assert_eq!(
        header(&part.head, "Content-Range"),
        Some("bytes 100-199/1000")
    );
    assert_eq!(fetched(at, path, Some("bytes=1000-")).status, 416);

    let cover = fetched(at, "/cover/album-1", None);

    assert_eq!(cover.status, 200);
    assert_eq!(cover.body, COVER);

    drop(server);

    let gone = heard(&told, |told| told.contains("NTS: ssdp:byebye"));

    assert!(gone.contains("USN: uuid:4c1b3a27-0000-4000-8000-000000000001"));
}

fn friendly_name_of(name: Option<ServerName>) -> String {
    let Started { server, .. } = started_as(name);
    let described = fetched(discovered(&server), "/description.xml", None);
    let description = String::from_utf8(described.body).expect("text");
    let document = Document::parse(&description).expect("the description reads as XML");

    document
        .descendants()
        .find(|node| node.tag_name().name() == "friendlyName")
        .and_then(|node| node.text())
        .expect("a friendly name")
        .to_owned()
}

#[test]
fn the_description_carries_the_name_typed_and_the_catalogs_word_where_none_was() {
    assert_eq!(friendly_name_of(None), "Resonate");
    assert_eq!(friendly_name_of(ServerName::new("   ")), "Resonate");
    assert_eq!(friendly_name_of(ServerName::new("  Study ")), "Study");
}

#[test]
fn a_name_typed_is_escaped_in_the_description_and_cut_to_the_length_a_control_point_reads() {
    assert_eq!(
        friendly_name_of(ServerName::new("Kitchen <&> \"Two\"")),
        "Kitchen <&> \"Two\""
    );

    let long = "é".repeat(100);
    let cut = friendly_name_of(ServerName::new(&long));

    assert_eq!(cut.chars().count(), 63);
    assert!(cut.chars().all(|letter| letter == 'é'));
}

fn root_title_of(name: Option<ServerName>) -> String {
    let Started { server, .. } = started_as(name);
    let root = browsed(discovered(&server), "0", "BrowseMetadata", 0, 0);
    let didl = root.didl();
    let document = Document::parse(&didl).expect("a DIDL document");

    document
        .descendants()
        .find(|node| node.tag_name().name() == "title")
        .and_then(|node| node.text())
        .expect("a title")
        .to_owned()
}

#[test]
fn the_root_container_is_titled_by_the_name_typed_and_the_catalogs_word_where_none_was() {
    assert_eq!(root_title_of(None), "Resonate");
    assert_eq!(root_title_of(ServerName::new("   ")), "Resonate");
    assert_eq!(root_title_of(ServerName::new("  Study ")), "Study");
    assert_eq!(
        root_title_of(ServerName::new("Kitchen <&> \"Two\"")),
        "Kitchen <&> \"Two\""
    );
}

#[test]
fn an_object_or_a_path_the_server_never_minted_is_refused() {
    let Started {
        server,
        told: _told,
        ..
    } = started();
    let at = discovered(&server);

    for object in [
        "album-99",
        "song-99",
        "/etc/passwd",
        "../album-1",
        "album-01",
        "",
    ] {
        let refused = children(at, object, 0, 0);

        assert_eq!(refused.status, 500, "{object:?}");
        assert_eq!(
            refused.named("errorCode").as_deref(),
            Some("701"),
            "{object:?}"
        );
    }

    assert_eq!(
        children(at, "song-1", 0, 0).named("errorCode").as_deref(),
        Some("710")
    );

    for path in [
        "/resonate/song-99.flac",
        "/etc/passwd",
        "/resonate/../../etc/passwd",
        "/resonate/%2e%2e/etc/passwd",
        "/resonate//etc/passwd.flac",
        "/cover/album-99",
    ] {
        assert_eq!(fetched(at, path, None).status, 404, "{path}");
    }

    assert_eq!(
        asked(
            at,
            &format!("DELETE /resonate/song-1.flac HTTP/1.1\r\nHost: {at}\r\n\r\n"),
            &[]
        )
        .status,
        405
    );
    assert_eq!(fetched(at, "/control/ContentDirectory", None).status, 405);
}

#[test]
fn a_request_naming_another_host_is_refused_and_one_naming_the_servers_address_is_answered() {
    let Started {
        server,
        told: _told,
        ..
    } = started();
    let at = discovered(&server);
    let hosted = |path: &str, host: &str| {
        asked(
            at,
            &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
            &[],
        )
        .status
    };

    for path in [
        "/description.xml",
        "/resonate/song-1.flac",
        "/cover/album-1",
    ] {
        assert_eq!(hosted(path, "evil.example"), 421, "{path}");
        assert_eq!(
            hosted(path, &format!("evil.example:{}", at.port())),
            421,
            "{path}"
        );
        assert_eq!(
            hosted(path, &format!("127.0.0.2:{}", at.port())),
            421,
            "{path}"
        );
        assert_eq!(hosted(path, &at.to_string()), 200, "{path}");
        assert_eq!(hosted(path, &at.ip().to_string()), 200, "{path}");
    }

    let browse = asked(
        at,
        "POST /control/ContentDirectory HTTP/1.1\r\nHost: evil.example\r\nContent-Length: \
         0\r\n\r\n",
        &[],
    );
    let nameless = asked(
        at,
        "GET /description.xml HTTP/1.1\r\nConnection: close\r\n\r\n",
        &[],
    );

    assert_eq!(browse.status, 421);
    assert_eq!(nameless.status, 400);
}

fn xml_escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn called(address: SocketAddr, action: &str, arguments: &str) -> Browsed {
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} \
         xmlns:u=\"{DIRECTORY}\">{arguments}</u:{action}></s:Body></s:Envelope>"
    );
    let answer = asked(
        address,
        &format!(
            "POST /control/ContentDirectory HTTP/1.1\r\nHost: {address}\r\nContent-Type: \
             text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{DIRECTORY}#{action}\"\r\n\
             Content-Length: {}\r\n\r\n",
            body.len()
        ),
        body.as_bytes(),
    );
    Browsed {
        status: answer.status,
        body: String::from_utf8(answer.body).expect("text"),
    }
}

fn searched(address: SocketAddr, container: &str, criteria: &str, from: u32, most: u32) -> Browsed {
    called(
        address,
        "Search",
        &format!(
            "<ContainerID>{container}</ContainerID><SearchCriteria>{}</SearchCriteria>\
             <Filter>*</Filter><StartingIndex>{from}</StartingIndex><RequestedCount>{most}\
             </RequestedCount><SortCriteria></SortCriteria>",
            xml_escaped(criteria)
        ),
    )
}

impl Browsed {
    fn every_id(&self) -> Vec<String> {
        let didl = self.didl();
        let document = Document::parse(&didl).expect("a DIDL document");
        document
            .descendants()
            .filter(|node| matches!(node.tag_name().name(), "container" | "item"))
            .filter_map(|node| node.attribute("id").map(str::to_owned))
            .collect()
    }
}

fn searched_page_by_page(at: SocketAddr, criteria: &str, most: u32) -> (Vec<String>, u32) {
    searched_within_page_by_page(at, "0", criteria, most)
}

fn searched_within_page_by_page(
    at: SocketAddr,
    container: &str,
    criteria: &str,
    most: u32,
) -> (Vec<String>, u32) {
    let mut ids = Vec::new();
    let mut from = 0;
    loop {
        let page = searched(at, container, criteria, from, most);

        assert_eq!(page.status, 200, "{container} {criteria}: {}", page.body);

        let total = page.counted("TotalMatches");
        let returned = page.counted("NumberReturned");

        assert!(returned <= most);
        assert_eq!(page.every_id().len() as u32, returned);

        ids.extend(page.every_id());
        from += returned;
        if returned == 0 || from >= total {
            return (ids, total);
        }
    }
}

#[test]
fn a_control_point_searches_the_catalog_by_artist_and_by_title_page_by_page() {
    let Started {
        server,
        told: _told,
        ..
    } = started();
    let at = discovered(&server);

    let capabilities = called(at, "GetSearchCapabilities", "");
    let by_the_band = searched_page_by_page(
        at,
        "upnp:class derivedfrom \"object.item.audioItem\" and upnp:artist = \"the band\"",
        2,
    );
    let titled = searched(at, "0", "dc:title contains \"three\"", 0, 0);
    let named_album = searched(at, "0", "dc:title contains \"album\"", 0, 0);
    let among_the_songs = searched(at, "songs", "dc:title contains \"album\"", 0, 0);
    let everything = searched_page_by_page(at, "*", 3);
    let root = browsed(at, "0", "BrowseMetadata", 0, 0);

    assert!(
        capabilities
            .named("SearchCaps")
            .is_some_and(|caps| caps.contains("dc:title") && caps.contains("upnp:artist"))
    );
    assert_eq!(
        by_the_band,
        (
            vec!["song-1", "song-2", "song-3", "song-4", "song-5"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            5
        )
    );
    assert_eq!(titled.every_id(), ["song-2", "song-3"]);
    assert!(titled.didl().contains("parentID=\"songs\""));
    assert_eq!(named_album.every_id(), ["albums", "album-1", "album-2"]);
    assert_eq!(named_album.counted("TotalMatches"), 3);
    assert_eq!(among_the_songs.counted("TotalMatches"), 0);
    assert_eq!(everything.1, 18);
    assert_eq!(
        everything.0,
        [
            "artists",
            "albums",
            "songs",
            "playlists",
            "artist-1",
            "artist-2",
            "album-1",
            "album-2",
            "album-3",
            "playlist-1",
            "song-1",
            "song-2",
            "song-3",
            "song-4",
            "song-5",
            "song-6",
            "song-7",
            "song-8"
        ]
    );
    assert!(root.didl().contains("searchable=\"1\""));
}

#[test]
fn a_search_the_catalog_cannot_answer_is_refused_not_answered_wrongly() {
    let Started {
        server,
        told: _told,
        ..
    } = started();
    let at = discovered(&server);

    for criteria in [
        "upnp:genre exists true",
        "res@size > \"1\"",
        "dc:title < \"b\"",
        "dc:title contains",
        "@id = \"song-1\"",
        "dc:title contains \"a\" and (upnp:artist = \"b\"",
        "",
    ] {
        let refused = searched(at, "0", criteria, 0, 0);

        assert_eq!(refused.status, 500, "{criteria:?}");
        assert_eq!(
            refused.named("errorCode").as_deref(),
            Some("708"),
            "{criteria:?}"
        );
    }
    for unheld in ["artist-99", "album-99", "playlist-99", "song-99"] {
        assert_eq!(
            searched(at, unheld, "*", 0, 0)
                .named("errorCode")
                .as_deref(),
            Some("701"),
            "{unheld}"
        );
    }

    assert_eq!(
        searched(at, "song-1", "*", 0, 0)
            .named("errorCode")
            .as_deref(),
        Some("710")
    );
}

fn owned(ids: &[&str]) -> Vec<String> {
    ids.iter().copied().map(str::to_owned).collect()
}

#[test]
fn a_search_under_an_artist_an_album_a_playlist_or_a_shelf_answers_what_it_holds_page_by_page() {
    let Started {
        server,
        told: _told,
        ..
    } = started();
    let at = discovered(&server);

    let the_band = searched_within_page_by_page(at, "artist-1", "*", 2);
    let the_bands_with_an_o = searched(at, "artist-1", "dc:title contains \"o\"", 0, 0);
    let the_soloist = searched_within_page_by_page(at, "artist-2", "*", 1);
    let the_bands_songs = searched(
        at,
        "artist-1",
        "upnp:class derivedfrom \"object.item.audioItem\"",
        0,
        0,
    );
    let on_the_album =
        searched_within_page_by_page(at, "album-1", "dc:title contains \"three\"", 1);
    let second_on_the_album = searched(at, "album-1", "dc:title contains \"three\"", 1, 1);
    let no_album_on_the_album = searched(
        at,
        "album-1",
        "upnp:class derivedfrom \"object.container\"",
        0,
        0,
    );
    let in_the_playlist = searched_within_page_by_page(at, "playlist-1", "*", 2);
    let named_in_the_playlist = searched(at, "playlist-1", "dc:title contains \"one\"", 0, 0);
    let among_the_artists = searched(at, "artists", "dc:title contains \"solo\"", 0, 0);
    let under_the_artists = searched_within_page_by_page(at, "artists", "*", 5);
    let across_the_kinds_under_the_artists = searched(at, "artists", "*", 1, 3);
    let songs_under_the_artists = searched(
        at,
        "artists",
        "upnp:class derivedfrom \"object.item\"",
        0,
        0,
    );
    let among_the_albums = searched_within_page_by_page(at, "albums", "*", 2);
    let songs_among_the_albums =
        searched(at, "albums", "upnp:class derivedfrom \"object.item\"", 0, 0);
    let named_among_the_albums = searched(at, "albums", "dc:title contains \"o\"", 0, 0);
    let among_the_playlists = searched(at, "playlists", "dc:title contains \"mix\"", 0, 0);
    let among_the_songs = searched(at, "songs", "dc:title contains \"one\"", 0, 0);
    let an_artist = browsed(at, "artist-1", "BrowseMetadata", 0, 0);
    let a_playlist = children(at, "playlists", 0, 0);

    assert_eq!(
        the_band,
        (
            owned(&[
                "album-1", "album-2", "song-1", "song-2", "song-3", "song-4", "song-5"
            ]),
            7
        )
    );
    assert_eq!(
        the_bands_with_an_o.every_id(),
        ["song-1", "song-2", "song-4"]
    );
    assert_eq!(the_bands_with_an_o.counted("TotalMatches"), 3);
    assert_eq!(the_soloist, (owned(&["album-3", "song-6", "song-7"]), 3));
    assert_eq!(the_bands_songs.counted("TotalMatches"), 5);
    assert_eq!(on_the_album, (owned(&["song-2", "song-3"]), 2));
    assert_eq!(second_on_the_album.every_id(), ["song-3"]);
    assert_eq!(second_on_the_album.counted("TotalMatches"), 2);
    assert_eq!(no_album_on_the_album.counted("TotalMatches"), 0);
    assert_eq!(in_the_playlist, (owned(&["song-3", "song-6", "song-1"]), 3));
    assert_eq!(named_in_the_playlist.every_id(), ["song-6", "song-1"]);
    assert_eq!(among_the_artists.every_id(), ["artist-2", "album-3"]);
    assert_eq!(among_the_artists.counted("TotalMatches"), 2);
    assert_eq!(
        under_the_artists,
        (
            owned(&[
                "artist-1", "artist-2", "album-1", "album-2", "album-3", "song-1", "song-2",
                "song-3", "song-4", "song-5", "song-6", "song-7"
            ]),
            12
        )
    );
    assert_eq!(
        across_the_kinds_under_the_artists.every_id(),
        ["artist-2", "album-1", "album-2"]
    );
    assert_eq!(
        across_the_kinds_under_the_artists.counted("TotalMatches"),
        12
    );
    assert_eq!(songs_under_the_artists.counted("TotalMatches"), 7);
    assert!(
        !songs_under_the_artists
            .every_id()
            .contains(&"song-8".to_owned())
    );
    assert_eq!(
        among_the_albums,
        (
            owned(&[
                "album-1", "album-2", "album-3", "song-1", "song-2", "song-3", "song-4", "song-5",
                "song-6"
            ]),
            9
        )
    );
    assert_eq!(
        songs_among_the_albums.every_id(),
        ["song-1", "song-2", "song-3", "song-4", "song-5", "song-6"]
    );
    assert_eq!(songs_among_the_albums.counted("TotalMatches"), 6);
    assert_eq!(
        named_among_the_albums.every_id(),
        ["album-3", "song-1", "song-2", "song-4", "song-6"]
    );
    assert!(
        songs_among_the_albums
            .didl()
            .contains("id=\"song-1\" parentID=\"songs\"")
    );
    assert_eq!(among_the_playlists.every_id(), ["playlist-1"]);
    assert_eq!(among_the_songs.every_id(), ["song-1", "song-6"]);
    assert!(the_bands_songs.didl().contains("parentID=\"songs\""));
    assert!(
        searched(at, "artist-1", "*", 0, 1)
            .didl()
            .contains("id=\"album-1\" parentID=\"albums\"")
    );
    assert!(an_artist.didl().contains("searchable=\"1\""));
    assert!(a_playlist.didl().contains("searchable=\"1\""));
}

struct Notified {
    head: String,
    body: String,
}

fn notified(listener: &TcpListener) -> Notified {
    listener.set_nonblocking(true).expect("a listener");

    let deadline = Instant::now() + HEARD_WITHIN;
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "no event was told in time");

                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("no event could be heard: {error}"),
        }
    };

    stream.set_nonblocking(false).expect("a stream");
    stream
        .set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");

    let mut held = Vec::new();
    let mut buffer = [0; 4096];

    let (ends, length) = loop {
        let read = stream.read(&mut buffer).expect("an event");

        assert!(read > 0, "an event ended early");

        held.extend_from_slice(&buffer[..read]);
        if let Some(at) = held.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&held[..at]).into_owned();
            let length: usize = header(&head, "Content-Length")
                .and_then(|length| length.parse().ok())
                .expect("a length");
            if held.len() >= at + 4 + length {
                break (at + 4, length);
            }
        }
    };

    stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
        .expect("answers");
    Notified {
        head: String::from_utf8_lossy(&held[..ends]).into_owned(),
        body: String::from_utf8_lossy(&held[ends..ends + length]).into_owned(),
    }
}

fn subscribed(at: SocketAddr, method: &str, service: &str, headers: &str) -> Answer {
    asked(
        at,
        &format!("{method} /event/{service} HTTP/1.1\r\nHOST: {at}\r\n{headers}\r\n"),
        &[],
    )
}

#[test]
fn a_subscriber_is_told_of_every_change_to_the_catalog_until_it_unsubscribes() {
    let Started {
        server,
        told: _told,
        shelves,
    } = started();
    let at = discovered(&server);
    let listener = TcpListener::bind(SocketAddr::new(LOOPBACK, 0)).expect("a callback");
    let callback = format!(
        "http://{}/notify",
        listener.local_addr().expect("an address")
    );

    let subscription = subscribed(
        at,
        "SUBSCRIBE",
        "ContentDirectory",
        &format!("CALLBACK: <{callback}>\r\nNT: upnp:event\r\nTIMEOUT: Second-300\r\n"),
    );
    let sid = header(&subscription.head, "SID")
        .expect("a subscription id")
        .to_owned();
    let first = notified(&listener);

    shelves.revision.store(8, Ordering::Release);

    let changed = notified(&listener);
    let renewed = subscribed(
        at,
        "SUBSCRIBE",
        "ContentDirectory",
        &format!("SID: {sid}\r\nTIMEOUT: Second-60\r\n"),
    );
    let both = subscribed(
        at,
        "SUBSCRIBE",
        "ContentDirectory",
        &format!("SID: {sid}\r\nNT: upnp:event\r\n"),
    );
    let elsewhere = subscribed(
        at,
        "SUBSCRIBE",
        "ConnectionManager",
        &format!("SID: {sid}\r\n"),
    );
    let ended = subscribed(
        at,
        "UNSUBSCRIBE",
        "ContentDirectory",
        &format!("SID: {sid}\r\n"),
    );
    let ended_again = subscribed(
        at,
        "UNSUBSCRIBE",
        "ContentDirectory",
        &format!("SID: {sid}\r\n"),
    );
    let renewed_after = subscribed(
        at,
        "SUBSCRIBE",
        "ContentDirectory",
        &format!("SID: {sid}\r\n"),
    );
    let connections = subscribed(
        at,
        "SUBSCRIBE",
        "ConnectionManager",
        &format!("CALLBACK: <{callback}>\r\nNT: upnp:event\r\n"),
    );
    let connections_told = notified(&listener);

    assert_eq!(subscription.status, 200);
    assert!(sid.starts_with("uuid:"));
    assert_eq!(header(&subscription.head, "TIMEOUT"), Some("Second-300"));
    assert!(first.head.starts_with("NOTIFY /notify HTTP/1.1"));
    assert_eq!(header(&first.head, "NT"), Some("upnp:event"));
    assert_eq!(header(&first.head, "NTS"), Some("upnp:propchange"));
    assert_eq!(header(&first.head, "SID"), Some(sid.as_str()));
    assert_eq!(header(&first.head, "SEQ"), Some("0"));
    assert!(first.body.contains("<SystemUpdateID>7</SystemUpdateID>"));
    assert_eq!(header(&changed.head, "SEQ"), Some("1"));
    assert!(changed.body.contains("<SystemUpdateID>8</SystemUpdateID>"));
    assert_eq!(renewed.status, 200);
    assert_eq!(header(&renewed.head, "SID"), Some(sid.as_str()));
    assert_eq!(header(&renewed.head, "TIMEOUT"), Some("Second-60"));
    assert_eq!(both.status, 400);
    assert_eq!(elsewhere.status, 412);
    assert_eq!(ended.status, 200);
    assert_eq!(ended_again.status, 412);
    assert_eq!(renewed_after.status, 412);
    assert_eq!(connections.status, 200);
    assert_eq!(header(&connections.head, "TIMEOUT"), Some("Second-1800"));
    assert!(
        connections_told
            .body
            .contains("<CurrentConnectionIDs>0</CurrentConnectionIDs>")
    );
}

fn evented(told: &Notified, name: &str) -> Option<String> {
    let document = Document::parse(&told.body).expect("a property set");
    document
        .descendants()
        .find(|node| node.tag_name().name() == name)
        .map(|node| node.text().unwrap_or_default().to_owned())
}

fn containers_named(told: &Notified) -> Option<Vec<String>> {
    let named = evented(told, "ContainerUpdateIDs")?;
    let figures: Vec<&str> = named.split(',').filter(|part| !part.is_empty()).collect();
    let mut pairs: Vec<String> = figures.chunks(2).map(|pair| pair.join(",")).collect();

    pairs.sort();
    Some(pairs)
}

#[test]
fn a_subscriber_is_told_which_containers_it_browsed_changed_at_most_every_two_seconds() {
    let Started {
        server,
        told: _told,
        shelves,
    } = started();
    let at = discovered(&server);

    for container in ["0", "playlist-1", "album-2", "artist-2"] {
        assert_eq!(children(at, container, 0, 0).status, 200, "{container}");
    }

    let listener = TcpListener::bind(SocketAddr::new(LOOPBACK, 0)).expect("a callback");
    let callback = format!(
        "http://{}/notify",
        listener.local_addr().expect("an address")
    );

    let subscription = subscribed(
        at,
        "SUBSCRIBE",
        "ContentDirectory",
        &format!("CALLBACK: <{callback}>\r\nNT: upnp:event\r\n"),
    );
    let first = notified(&listener);
    let first_at = Instant::now();

    shelves.changed(|shelves| shelves.playlists.lock()[0].2.reverse());

    let reordered = notified(&listener);
    let reordered_at = Instant::now();

    shelves.changed(|shelves| shelves.songs.lock().retain(|song| song.id != track(5)));

    let removed = notified(&listener);
    let removed_at = Instant::now();

    assert_eq!(subscription.status, 200);
    assert_eq!(evented(&first, "SystemUpdateID").as_deref(), Some("7"));
    assert_eq!(containers_named(&first), Some(Vec::new()));
    assert_eq!(evented(&reordered, "SystemUpdateID").as_deref(), Some("8"));
    assert_eq!(containers_named(&reordered), Some(owned(&["playlist-1,8"])));
    assert_eq!(evented(&removed, "SystemUpdateID").as_deref(), Some("9"));
    assert_eq!(
        containers_named(&removed),
        Some(owned(&["0,9", "album-2,9"]))
    );
    assert!(reordered_at - first_at >= MODERATED_AT_LEAST);
    assert!(removed_at - reordered_at >= MODERATED_AT_LEAST);
}

#[test]
fn a_callback_elsewhere_is_refused_and_a_subscriber_that_does_not_answer_is_dropped() {
    let Started {
        server,
        told: _told,
        ..
    } = started();
    let at = discovered(&server);
    let gone_to = {
        let listener = TcpListener::bind(SocketAddr::new(LOOPBACK, 0)).expect("a callback");
        listener.local_addr().expect("an address")
    };

    for headers in [
        "CALLBACK: <http://8.8.8.8:80/notify>\r\nNT: upnp:event\r\n".to_owned(),
        format!(
            "CALLBACK: <http://127.0.0.2:{}/notify>\r\nNT: upnp:event\r\n",
            gone_to.port()
        ),
        format!("CALLBACK: <http://{gone_to}/notify>\r\n"),
        format!("CALLBACK: <http://{gone_to}/notify>\r\nNT: upnp:other\r\n"),
        "NT: upnp:event\r\n".to_owned(),
    ] {
        assert_eq!(
            subscribed(at, "SUBSCRIBE", "ContentDirectory", &headers).status,
            412,
            "{headers:?}"
        );
    }

    let dropped = subscribed(
        at,
        "SUBSCRIBE",
        "ContentDirectory",
        &format!("CALLBACK: <http://{gone_to}/notify>\r\nNT: upnp:event\r\n"),
    );
    let sid = header(&dropped.head, "SID")
        .expect("a subscription id")
        .to_owned();
    let deadline = Instant::now() + HEARD_WITHIN;
    let renewed = loop {
        let renewed = subscribed(
            at,
            "SUBSCRIBE",
            "ContentDirectory",
            &format!("SID: {sid}\r\n"),
        );

        if renewed.status != 200 || Instant::now() >= deadline {
            break renewed.status;
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    assert_eq!(dropped.status, 200);
    assert_eq!(renewed, 412);
}
