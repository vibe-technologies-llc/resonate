use std::{
    io::{self, Cursor, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
    sync::Arc,
    time::{Duration, Instant},
};

use resonate_core::{AlbumId, ArtistId, PlaylistId, TrackId};
use resonate_upnp::{
    AudioKind, Catalog, Entry, Held, MediaServer, Offering, Opened, Page, Picture, Shelf, Shelved,
    Song, Udn,
};
use roxmltree::Document;

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

const HEARD_WITHIN: Duration = Duration::from_secs(5);

const DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

const SERVER_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";

const COVER: &[u8] = b"\xff\xd8\xff\xe0 a picture";

struct Shelves {
    songs: Vec<Song>,
    albums: Vec<(AlbumId, &'static str, ArtistId)>,
    artists: Vec<(ArtistId, &'static str)>,
    playlists: Vec<(PlaylistId, &'static str, Vec<TrackId>)>,
}

fn artist(raw: u64) -> ArtistId {
    ArtistId::new(raw).expect("an id")
}

fn album(raw: u64) -> AlbumId {
    AlbumId::new(raw).expect("an id")
}

fn track(raw: u64) -> TrackId {
    TrackId::new(raw).expect("an id")
}

fn bytes_of(id: TrackId) -> Vec<u8> {
    (0..1000_u32)
        .map(|at| (at as u64 * 7 + id.get()) as u8)
        .collect()
}

impl Shelves {
    fn new() -> Self {
        let song = |raw: u64, title: &str, on: u64| Song {
            id: track(raw),
            title: title.to_owned(),
            artist: Some("The Band".to_owned()),
            album: Some(format!("Album {on}")),
            cover: Some(album(on)),
            genre: None,
            track_number: Some(raw as u32),
            duration: Some(Duration::from_secs(180)),
            kind: AudioKind::Flac,
            size: 1000,
            rate: 44_100,
            channels: 2,
            bits: Some(16),
        };
        Self {
            songs: vec![
                song(1, "One", 1),
                song(2, "Two & Three", 1),
                song(3, "<Three>", 1),
                song(4, "Four", 2),
                song(5, "Five", 2),
            ],
            albums: vec![
                (album(1), "Album 1", artist(1)),
                (album(2), "Album 2", artist(1)),
            ],
            artists: vec![(artist(1), "The Band")],
            playlists: vec![(
                PlaylistId::new(1).expect("an id"),
                "Mixed",
                vec![track(3), track(1)],
            )],
        }
    }

    fn songs_on(&self, on: AlbumId) -> Vec<Song> {
        self.songs
            .iter()
            .filter(|song| song.cover == Some(on))
            .cloned()
            .collect()
    }

    fn album_held(&self, id: AlbumId) -> Option<Held> {
        let (_, title, _) = self.albums.iter().find(|(held, ..)| *held == id)?;
        Some(Held {
            shelf: Shelf::Album(id),
            title: (*title).to_owned(),
            artist: Some("The Band".to_owned()),
            children: self.songs_on(id).len() as u32,
            cover: Some(id),
        })
    }
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
            Shelf::Songs => self.songs.iter().cloned().map(Entry::Song).collect(),
            Shelf::Playlists => self
                .playlists
                .iter()
                .filter_map(|(id, ..)| self.shelf(Shelf::Playlist(*id)).ok().flatten())
                .map(Entry::Shelf)
                .collect(),
            Shelf::Playlist(id) => {
                let Some((_, _, held)) = self.playlists.iter().find(|(named, ..)| *named == id)
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
            Shelf::Playlist(id) => {
                self.playlists
                    .iter()
                    .find(|(held, ..)| *held == id)
                    .map(|(_, name, songs)| Held {
                        shelf,
                        title: (*name).to_owned(),
                        artist: None,
                        children: songs.len() as u32,
                        cover: None,
                    })
            }
            Shelf::Artists | Shelf::Albums | Shelf::Songs | Shelf::Playlists => None,
        })
    }

    fn song(&self, id: TrackId) -> io::Result<Option<Song>> {
        Ok(self.songs.iter().find(|song| song.id == id).cloned())
    }

    fn bytes_of(&self, id: TrackId) -> io::Result<Option<Opened>> {
        Ok(self.song(id)?.map(|song| Opened {
            bytes: Box::new(Cursor::new(bytes_of(song.id))),
            kind: song.kind,
        }))
    }

    fn cover(&self, id: AlbumId) -> io::Result<Option<Picture>> {
        Ok(self.album_held(id).map(|_| Picture {
            bytes: COVER.to_vec(),
            mime: "image/jpeg",
        }))
    }

    fn revision(&self) -> u32 {
        7
    }
}

struct Started {
    server: MediaServer<Shelves>,
    told: UdpSocket,
}

fn started() -> Started {
    let told = UdpSocket::bind(SocketAddr::new(LOOPBACK, 0)).expect("a socket told of the server");
    told.set_read_timeout(Some(HEARD_WITHIN))
        .expect("a read timeout");
    let server = MediaServer::start(
        Arc::new(Shelves::new()),
        Udn::new("uuid:4c1b3a27-0000-4000-8000-000000000001"),
        Offering {
            heard_on: SocketAddr::new(LOOPBACK, 0),
            told: told.local_addr().expect("an address"),
            every: Duration::from_secs(60),
            serving_on: LOOPBACK,
        },
    )
    .expect("the server starts");
    Started { server, told }
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
    let Started { server, told } = started();

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
    assert_eq!(artists.ids("container"), ["artist-1"]);
    let albums_of_the_artist = children(at, "artist-1", 0, 0);
    assert_eq!(
        albums_of_the_artist.ids("container"),
        ["album-1", "album-2"]
    );
    let on_the_album = children(at, "album-1", 0, 0);
    assert_eq!(on_the_album.ids("item"), ["song-1", "song-2", "song-3"]);
    assert!(on_the_album.didl().contains("&lt;Three&gt;"));
    let listed = children(at, "playlist-1", 0, 0);
    assert_eq!(listed.ids("item"), ["song-3", "song-1"]);

    let mut paged = Vec::new();
    let mut from = 0;
    loop {
        let page = children(at, "songs", from, 2);
        assert_eq!(page.counted("TotalMatches"), 5);
        let returned = page.counted("NumberReturned");
        assert!(returned <= 2);
        paged.extend(page.ids("item"));
        from += returned;
        if returned == 0 || from >= 5 {
            break;
        }
    }
    assert_eq!(paged, ["song-1", "song-2", "song-3", "song-4", "song-5"]);

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

#[test]
fn an_object_or_a_path_the_server_never_minted_is_refused() {
    let Started {
        server,
        told: _told,
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
