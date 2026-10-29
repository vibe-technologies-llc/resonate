use std::{
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use parking_lot::Mutex;
use resonate_providers::{Error, Id, Named, PlaysInPlace, ProviderOp};
use resonate_tidal::{Account, Endpoints, HifiApi, Hosting, MediaHosts, Tidal};

const SONG_BYTES: usize = 600_000;
const FAR_AHEAD: u64 = 500_000;
const COVER: &[u8] = b"\xff\xd8\xff\xe0 a cover";
const SEARCHED: &str = r#"{"artists":{"items":[{"id":3,"name":"Pink Floyd","picture":"ee-ff"}]},
    "albums":{"items":[{"id":7,"title":"Meddle","releaseDate":"1971-10-30","numberOfTracks":2,"artist":{"name":"Pink Floyd"},"cover":"aa-bb"}]},
    "tracks":{"items":[{"id":11,"title":"One of These Days","duration":357,"trackNumber":1,"artist":{"name":"Pink Floyd"},"album":{"title":"Meddle","cover":"aa-bb"}}]}}"#;
const ALBUM_TRACKS: &str = r#"{"items":[
    {"id":11,"title":"One of These Days","duration":357,"trackNumber":1,"volumeNumber":1,"artist":{"name":"Pink Floyd"},"album":{"title":"Meddle","cover":"aa-bb"}},
    {"id":12,"title":"A Pillow of Winds","version":"Remastered","duration":310,"trackNumber":2,"volumeNumber":1,"artist":{"name":"Pink Floyd"},"album":{"title":"Meddle","cover":"aa-bb"}}]}"#;
const MIXES: &str = r#"{"items":[
    {"dateAdded":"2024-02-01","id":"0a1b2c","title":"My Mix 1","subTitle":"Pink Floyd, Camel and more","mixType":"TRACK_MIX"},
    {"item":{"id":"3d4e5f","title":"Daily Discovery","subTitle":""}},
    {"id":"../x","title":"Out of the path"},
    {"title":"Nameless"}]}"#;
const MIX_ITEMS: &str = r#"{"items":[
    {"item":{"id":11,"title":"One of These Days","duration":357,"artist":{"name":"Pink Floyd"},"album":{"title":"Meddle","cover":"aa-bb"}},"type":"track"},
    {"item":null,"type":"video"},
    {"item":{"id":12,"title":"A Pillow of Winds","duration":310,"artist":{"name":"Pink Floyd"}},"type":"track"}]}"#;
const ARTIST_ALBUMS: &str = r#"{"items":[{"id":7,"title":"Meddle","artist":{"name":"Pink Floyd"},"cover":"aa-bb"},{"id":"x"},{"id":8,"title":"Animals","artist":{"name":"Pink Floyd"}}]}"#;

fn song() -> Vec<u8> {
    let mut song = b"fLaC".to_vec();
    song.extend((0..SONG_BYTES).map(|at| u8::try_from(at * 7 % 251).expect("a byte")));
    song
}

#[derive(Clone, Debug)]
struct Asked {
    method: String,
    body: String,
    path: String,
    query: String,
    authorization: Option<String>,
    range: Option<u64>,
}

struct Canned {
    status: u16,
    media_type: &'static str,
    body: Vec<u8>,
    ranged: bool,
}

impl Canned {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            media_type: "application/json",
            body: body.as_bytes().to_vec(),
            ranged: false,
        }
    }

    fn wrapped(body: &str) -> Self {
        Self::json(&format!(r#"{{"version":"2.2","data":{body}}}"#))
    }

    fn song() -> Self {
        Self {
            status: 200,
            media_type: "audio/flac",
            body: song(),
            ranged: true,
        }
    }

    fn cover() -> Self {
        Self {
            status: 200,
            media_type: "image/jpeg",
            body: COVER.to_vec(),
            ranged: false,
        }
    }

    fn missing() -> Self {
        Self {
            status: 404,
            ..Self::json("{}")
        }
    }
}

struct Fake {
    url: String,
    heard: Arc<Mutex<Vec<Asked>>>,
}

impl Fake {
    fn serving(answering: impl Fn(&Asked, &str) -> Canned + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        let heard = Arc::new(Mutex::new(Vec::new()));
        let noted = Arc::clone(&heard);
        let own = url.clone();
        let answering = Arc::new(answering);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let noted = Arc::clone(&noted);
                let answering = Arc::clone(&answering);
                let own = own.clone();
                thread::spawn(move || answer(stream, &noted, answering.as_ref(), &own));
            }
        });
        Self { url, heard }
    }

    fn heard(&self) -> Vec<Asked> {
        self.heard.lock().clone()
    }

    fn tidal(&self) -> Tidal {
        Tidal::at(
            Account {
                client_id: "client".to_owned(),
                client_secret: None,
                refresh_token: "sesame".to_owned(),
            },
            Endpoints {
                auth: format!("{}/auth/token", self.url),
                device: format!("{}/auth/device", self.url),
                api: format!("{}/api/", self.url),
                openapi: format!("{}/openapi/", self.url),
                images: format!("{}/images/", self.url),
                media: on_loopback(),
            },
        )
    }

    fn hifi(&self) -> HifiApi {
        HifiApi::fetching_from(&self.url, on_loopback())
            .pictured_from(&format!("{}/images/", self.url))
    }

    fn hosted(&self) -> HifiApi {
        HifiApi::hosted_through(
            Hosting {
                server: self.url.clone(),
                hifi_token: format!("{}/hifi/token", self.url),
                tidal_token: format!("{}/tidal/token", self.url),
                tidal_api: format!("{}/v1/", self.url),
                images: format!("{}/images/", self.url),
                manifests: on_loopback(),
            },
            on_loopback(),
        )
    }
}

fn on_loopback() -> MediaHosts {
    MediaHosts {
        scheme: "http".to_owned(),
        domain: "127.0.0.1".to_owned(),
    }
}

fn answer(
    stream: TcpStream,
    heard: &Mutex<Vec<Asked>>,
    answering: &(dyn Fn(&Asked, &str) -> Canned + Send + Sync),
    own: &str,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut authorization = None;
    let mut range = None;
    let mut body_length = 0_usize;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {}
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "authorization" => authorization = Some(value.to_owned()),
            "content-length" => body_length = value.parse().unwrap_or_default(),
            "range" => {
                range = value
                    .strip_prefix("bytes=")
                    .and_then(|from| from.trim_end_matches('-').parse().ok());
            }
            _ => {}
        }
    }
    let mut sent = vec![0; body_length];
    if reader.read_exact(&mut sent).is_err() {
        return;
    }
    let method = line.split(' ').next().unwrap_or_default().to_owned();
    let target = line.split(' ').nth(1).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let asked = Asked {
        method,
        body: String::from_utf8_lossy(&sent).into_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        authorization,
        range,
    };
    let canned = answering(&asked, own);
    heard.lock().push(asked.clone());

    let total = canned.body.len();
    let (status, body, ranged) = match asked.range {
        Some(from) if canned.ranged && canned.status == 200 => {
            let from = usize::try_from(from).expect("a small offset");
            (
                206,
                canned.body[from..].to_vec(),
                format!("Content-Range: bytes {from}-{}/{total}\r\n", total - 1),
            )
        }
        _ => (canned.status, canned.body, String::new()),
    };
    let mut stream = reader.into_inner();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Canned\r\nContent-Type: {}\r\nContent-Length: {}\r\n{ranged}Connection: close\r\n\r\n",
        canned.media_type,
        body.len()
    );
    let _ = stream.write_all(&body);
}

fn one_flac_manifest(own: &str) -> String {
    STANDARD.encode(format!(
        r#"{{"mimeType":"audio/flac","codecs":"flac","encryptionType":"NONE","urls":["{own}/media/whole.flac"]}}"#
    ))
}

fn playback(own: &str) -> String {
    format!(
        r#"{{"trackId":11,"assetPresentation":"FULL","audioMode":"STEREO","audioQuality":"LOSSLESS","manifestMimeType":"application/vnd.tidal.bts","manifest":"{}"}}"#,
        one_flac_manifest(own)
    )
}

fn tidal_server() -> Fake {
    Fake::serving(|asked, own| match asked.path.as_str() {
        "/auth/token" => Canned::json(
            r#"{"access_token":"fresh","expires_in":3600,"user":{"countryCode":"GB"}}"#,
        ),
        "/api/search" => Canned::json(SEARCHED),
        "/api/sessions" => Canned::json(r#"{"countryCode":"GB","userId":"42"}"#),
        "/api/users/42/favorites/artists" => Canned::json(
            r#"{"items":[{"created":"2024-01-01","item":{"id":3,"name":"Pink Floyd","picture":"ee-ff"}},{"item":{"id":null}}]}"#,
        ),
        "/api/users/42/favorites/albums" => Canned::json(
            r#"{"items":[{"item":{"id":7,"title":"Meddle","artist":{"name":"Pink Floyd"},"cover":"aa-bb"}}]}"#,
        ),
        "/api/users/42/favorites/tracks" if asked.method == "POST" => {
            if asked.body.starts_with("trackIds=11&") {
                Canned::json(r#"{"lastModifiedAt":"now"}"#)
            } else {
                Canned::missing()
            }
        }
        "/api/users/42/favorites/tracks/11" if asked.method == "DELETE" => Canned::json("{}"),
        "/api/users/42/favorites/tracks" => Canned::json(
            r#"{"items":[{"item":{"id":12,"title":"A Pillow of Winds","duration":310,"artist":{"name":"Pink Floyd"},"album":{"title":"Meddle","cover":"aa-bb"}}}]}"#,
        ),
        "/api/users/42/favorites/mixes" => Canned::json(MIXES),
        "/api/mixes/0a1b2c/items" => Canned::json(MIX_ITEMS),
        "/api/albums/7/tracks" => Canned::json(ALBUM_TRACKS),
        "/api/artists/3/albums" => Canned::json(ARTIST_ALBUMS),
        "/api/tracks/11" => Canned::json(
            r#"{"id":11,"title":"One of These Days","album":{"title":"Meddle","cover":"aa-bb"}}"#,
        ),
        "/api/tracks/11/playbackinfopostpaywall" => Canned::json(&playback(own)),
        "/media/whole.flac" => Canned::song(),
        "/images/aa/bb/320x320.jpg" => Canned::cover(),
        _ => Canned::missing(),
    })
}

fn read_some(streamed: &mut impl Read, bytes: usize) -> Vec<u8> {
    let mut read = vec![0; bytes];
    streamed.read_exact(&mut read).expect("the stream reads");
    read
}

fn song_id() -> Id {
    Id::new("11").expect("an id")
}

#[test]
fn a_tidal_account_is_searched_and_its_albums_and_artists_opened_by_their_ids() {
    let fake = tidal_server();
    let tidal = fake.tidal();

    let found = PlaysInPlace::search(&tidal, "meddle").expect("a search");
    let album = PlaysInPlace::album(&tidal, &Id::new("7").expect("an id")).expect("an album");
    let artist = PlaysInPlace::artist(&tidal, &Id::new("3").expect("an id")).expect("an artist");
    let listed = PlaysInPlace::artists(&tidal).expect("nothing listed");

    assert_eq!(found.artists[0].name, "Pink Floyd");
    assert_eq!(found.albums[0].year, Some(1971));
    assert_eq!(found.songs[0].title, "One of These Days");
    assert_eq!(
        album
            .songs
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["One of These Days", "A Pillow of Winds (Remastered)"]
    );
    assert_eq!(album.album.name, "Meddle");
    assert_eq!(album.songs[1].track, Some(2));
    assert_eq!(
        artist
            .albums
            .iter()
            .map(|album| album.name.as_str())
            .collect::<Vec<_>>(),
        ["Meddle", "Animals"]
    );
    assert!(listed.is_empty());
    assert!(
        fake.heard()
            .iter()
            .filter(|asked| asked.path.starts_with("/api/"))
            .all(|asked| asked.query.contains("countryCode=GB")
                && asked.authorization.as_deref() == Some("Bearer fresh"))
    );
}

#[test]
fn a_tidal_accounts_first_page_is_its_favourite_artists_albums_and_songs() {
    let fake = tidal_server();
    let tidal = fake.tidal();

    let first = tidal.first_page().expect("the first page");

    assert_eq!(
        first
            .artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        ["Pink Floyd"]
    );
    assert_eq!(first.albums[0].name, "Meddle");
    assert_eq!(first.songs[0].title, "A Pillow of Winds");
    assert_eq!(
        first.songs[0].cover.as_ref().map(Id::as_str),
        Some("album:aa-bb")
    );
    assert_eq!(
        fake.heard()
            .iter()
            .filter(|asked| asked.path.starts_with("/api/users/42/favorites/")
                && asked.query.contains("countryCode=GB")
                && asked.query.contains("order=DATE"))
            .count(),
        4
    );
}

#[test]
fn a_tidal_accounts_favourite_mixes_are_on_its_first_page_and_a_mix_is_browsed_and_expanded_to_its_songs()
 {
    let fake = tidal_server();
    let tidal = fake.tidal();

    let first = tidal.first_page().expect("the first page");
    let mix = Named::Mix(first.mixes[0].id.clone());
    let location = tidal.location_of(&mix);
    let browsed = tidal.mix(&first.mixes[0].id).expect("the mix");
    let songs = tidal.songs_of(&mix).expect("the mix's songs");

    assert_eq!(
        first
            .mixes
            .iter()
            .map(|mix| (mix.id.as_str(), mix.name.as_str(), mix.subtitle.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("0a1b2c", "My Mix 1", Some("Pink Floyd, Camel and more")),
            ("3d4e5f", "Daily Discovery", None)
        ]
    );
    assert_eq!(location.to_uri(), "tidal:mix/0a1b2c");
    assert_eq!(tidal.named_by(&location), Some(mix));
    assert_eq!(browsed.songs, songs);
    assert_eq!(
        songs
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["One of These Days", "A Pillow of Winds"]
    );
    assert_eq!(
        tidal
            .location_of(&Named::Song(songs[1].id.clone()))
            .to_uri(),
        "tidal:track/12"
    );
    assert!(
        fake.heard()
            .iter()
            .filter(|asked| asked.path == "/api/mixes/0a1b2c/items")
            .all(|asked| asked.query.contains("countryCode=GB")
                && asked.authorization.as_deref() == Some("Bearer fresh"))
    );
}

#[test]
fn a_mix_is_refused_by_a_service_that_names_none() {
    let fake = hifi_server();
    let hifi = fake.hifi();
    let mix = Named::Mix(Id::new("0a1b2c").expect("an id"));

    let refused = hifi.songs_of(&mix);
    let hidden = hifi.mix(&Id::new("../x").expect("an id"));

    assert!(matches!(
        refused,
        Err(Error::NotOffered {
            op: ProviderOp::Browse,
            ..
        })
    ));
    assert!(matches!(hidden, Err(Error::NotOffered { .. })));
}

#[test]
fn a_mix_id_that_would_leave_the_path_is_never_asked() {
    let fake = tidal_server();
    let tidal = fake.tidal();

    let refused = tidal.mix(&Id::new("../users/42").expect("an id"));

    assert!(matches!(refused, Err(Error::Unreadable { .. })));
    assert!(
        fake.heard()
            .iter()
            .all(|asked| !asked.path.contains("users/42"))
    );
}

#[test]
fn a_hifi_api_server_has_no_favourites_to_offer_and_lists_nothing_first() {
    let fake = hifi_server();

    let first = fake.hifi().first_page().expect("the first page");

    assert_eq!(first, resonate_providers::Found::default());
}

#[test]
fn a_tidal_song_is_located_as_its_own_and_an_album_expands_to_its_songs() {
    let fake = tidal_server();
    let tidal = fake.tidal();
    let album = Named::Album(Id::new("7").expect("an id"));

    let location = tidal.location_of(&album);
    let songs = tidal.songs_of(&album).expect("the album's songs");

    assert_eq!(location.to_uri(), "tidal:album/7");
    assert_eq!(tidal.named_by(&location), Some(album));
    assert_eq!(songs.len(), 2);
    assert_eq!(
        tidal
            .location_of(&Named::Song(songs[0].id.clone()))
            .to_uri(),
        "tidal:track/11"
    );
}

#[test]
fn a_tidal_cover_is_the_album_picture_at_the_least_side_that_covers_the_one_drawn() {
    let fake = tidal_server();
    let tidal = fake.tidal();

    let cover = tidal
        .cover_of(&Named::Song(song_id()))
        .expect("the song's cover");
    let bytes = PlaysInPlace::cover(&tidal, &cover, 300).expect("the picture");

    assert_eq!(cover.as_str(), "album:aa-bb");
    assert_eq!(bytes, COVER);
}

#[test]
fn a_tidal_song_in_one_flac_file_is_streamed_and_a_seek_far_ahead_asks_from_there() {
    let fake = tidal_server();
    let tidal = fake.tidal();
    let whole = song();

    let mut streamed = PlaysInPlace::streamed(&tidal, &song_id()).expect("a stream");
    let head = read_some(&mut streamed, 4);
    let landed = streamed
        .seek(SeekFrom::Start(FAR_AHEAD))
        .expect("a seek ahead");
    let after = read_some(&mut streamed, 16);
    streamed.seek(SeekFrom::Start(8)).expect("a seek back");
    let back = read_some(&mut streamed, 16);

    assert!(streamed.is_seekable());
    assert_eq!(streamed.length(), Some(whole.len() as u64));
    assert_eq!(streamed.media_type.as_deref(), Some("audio/flac"));
    assert_eq!(head, b"fLaC");
    assert_eq!(landed, FAR_AHEAD);
    assert_eq!(after, whole[500_000..500_016]);
    assert_eq!(back, whole[8..24]);
    let ranges: Vec<_> = fake
        .heard()
        .into_iter()
        .filter(|asked| asked.path == "/media/whole.flac")
        .map(|asked| asked.range)
        .collect();
    assert_eq!(ranges, [None, Some(FAR_AHEAD), Some(8)]);
}

#[test]
fn a_tidal_song_offered_only_in_part_is_not_streamed() {
    let fake = Fake::serving(|asked, own| match asked.path.as_str() {
        "/auth/token" => Canned::json(r#"{"access_token":"fresh","user":{"countryCode":"GB"}}"#),
        "/api/tracks/11/playbackinfopostpaywall" => {
            Canned::json(&playback(own).replace("\"FULL\"", "\"PREVIEW\""))
        }
        _ => Canned::missing(),
    });

    let refused = PlaysInPlace::streamed(&fake.tidal(), &song_id()).map(|_| ());

    assert!(matches!(
        refused,
        Err(Error::NotOffered {
            op: ProviderOp::Stream,
            ..
        })
    ));
}

fn hifi_server() -> Fake {
    Fake::serving(
        |asked, own| match (asked.path.as_str(), asked.query.as_str()) {
            ("/search/", query) if query.starts_with("s=") => {
                Canned::wrapped(&ALBUM_TRACKS.replace("\"items\"", "\"limit\":20,\"items\""))
            }
            ("/search/", query) if query.starts_with("a=") => Canned::wrapped(SEARCHED),
            ("/search/", query) if query.starts_with("al=") => Canned::wrapped(
                r#"{"albums":{"items":[{"id":7,"title":"Meddle","cover":"aa-bb"}]}}"#,
            ),
            ("/album/", "id=7") => Canned::wrapped(
                r#"{"id":7,"title":"Meddle","cover":"aa-bb","releaseDate":"1971-10-30","items":[
                {"item":{"id":11,"title":"One of These Days","trackNumber":1},"type":"track"},
                {"item":{"id":12,"title":"A Pillow of Winds","trackNumber":2},"type":"track"}]}"#,
            ),
            ("/artist/", "f=3") => {
                Canned::json(&format!(r#"{{"albums":{ARTIST_ALBUMS},"tracks":[]}}"#))
            }
            ("/info/", "id=11") => Canned::wrapped(
                r#"{"id":11,"title":"One of These Days","album":{"title":"Meddle","cover":"aa-bb"}}"#,
            ),
            ("/track/", query) if query.starts_with("id=11") => Canned::wrapped(&playback(own)),
            ("/file/", "id=11") if asked.authorization.as_deref() == Some("Bearer hush") => {
                Canned::song()
            }
            ("/media/whole.flac", _) => Canned::song(),
            ("/images/aa/bb/160x160.jpg", _) => Canned::cover(),
            _ => Canned::missing(),
        },
    )
}

#[test]
fn a_hifi_api_server_is_searched_by_song_artist_and_album_and_each_is_opened() {
    let fake = hifi_server();
    let hifi = fake.hifi();

    let found = PlaysInPlace::search(&hifi, "meddle").expect("a search");
    let album = PlaysInPlace::album(&hifi, &Id::new("7").expect("an id")).expect("an album");
    let artist = PlaysInPlace::artist(&hifi, &Id::new("3").expect("an id")).expect("an artist");
    let cover = hifi
        .cover_of(&Named::Song(song_id()))
        .expect("the song's cover");

    assert_eq!(found.songs.len(), 2);
    assert_eq!(found.artists[0].name, "Pink Floyd");
    assert_eq!(found.albums.len(), 2);
    assert_eq!(album.album.name, "Meddle");
    assert_eq!(album.album.year, Some(1971));
    assert_eq!(album.songs.len(), 2);
    assert_eq!(artist.albums.len(), 2);
    assert_eq!(
        PlaysInPlace::cover(&hifi, &cover, 100).expect("the picture"),
        COVER
    );
    assert_eq!(PlaysInPlace::title(&hifi), "hifi-api");
    assert_eq!(
        hifi.location_of(&Named::Song(song_id())).to_uri(),
        "hifi-api:track/11"
    );
}

#[test]
fn a_hifi_api_song_is_streamed_from_the_file_its_manifest_names_and_seeks() {
    let fake = hifi_server();
    let whole = song();

    let mut streamed = PlaysInPlace::streamed(&fake.hifi(), &song_id()).expect("a stream");
    streamed
        .seek(SeekFrom::Start(FAR_AHEAD))
        .expect("a seek ahead");
    let after = read_some(&mut streamed, 16);

    assert!(streamed.is_seekable());
    assert_eq!(after, whole[500_000..500_016]);
}

#[test]
fn a_keyed_server_streams_its_file_with_the_key_on_every_ask_and_browses_by_search_alone() {
    let fake = hifi_server();
    let keyed = HifiApi::at(&fake.url).with_secret("hush");
    let whole = song();

    let found = PlaysInPlace::search(&keyed, "meddle").expect("a search");
    let album = PlaysInPlace::album(&keyed, &Id::new("7").expect("an id")).map(|_| ());
    let mut streamed = PlaysInPlace::streamed(&keyed, &song_id()).expect("a stream");
    streamed
        .seek(SeekFrom::Start(FAR_AHEAD))
        .expect("a seek ahead");
    let after = read_some(&mut streamed, 16);

    assert_eq!(found.songs.len(), 2);
    assert!(found.artists.is_empty() && found.albums.is_empty());
    assert!(matches!(album, Err(Error::NotOffered { .. })));
    assert_eq!(after, whole[500_000..500_016]);
    let asked: Vec<_> = fake
        .heard()
        .into_iter()
        .filter(|asked| asked.path == "/file/")
        .collect();
    assert_eq!(
        asked.iter().map(|asked| asked.range).collect::<Vec<_>>(),
        [None, Some(FAR_AHEAD)]
    );
    assert!(
        asked
            .iter()
            .all(|asked| asked.authorization.as_deref() == Some("Bearer hush"))
    );
}

#[test]
fn the_hosted_hifi_service_is_browsed_through_tidals_web_api_and_streamed_through_its_manifest() {
    let fake = Fake::serving(|asked, own| match asked.path.as_str() {
        "/tidal/token" => Canned::json(r#"{"access_token":"tidal-granted","expires_in":3600}"#),
        "/hifi/token" => Canned::json(r#"{"access_token":"hifi-granted","expires_in":3600}"#),
        "/v1/search" if asked.authorization.as_deref() == Some("Bearer tidal-granted") => {
            Canned::json(SEARCHED)
        }
        "/v1/albums/7/tracks" => Canned::json(ALBUM_TRACKS),
        "/manifests" if asked.authorization.as_deref() == Some("Bearer hifi-granted") => {
            Canned::json(&format!(
                r#"{{"data":{{"attributes":{{"uri":"{own}/manifest/document","trackPresentation":"FULL"}}}}}}"#
            ))
        }
        "/manifest/document" => Canned {
            media_type: "application/vnd.tidal.bts",
            body: STANDARD.decode(one_flac_manifest(own)).expect("a manifest"),
            ..Canned::json("")
        },
        "/media/whole.flac" => Canned::song(),
        _ => Canned::missing(),
    });
    let hosted = fake.hosted();
    let whole = song();

    let found = PlaysInPlace::search(&hosted, "meddle").expect("a search");
    let album = PlaysInPlace::album(&hosted, &Id::new("7").expect("an id")).expect("an album");
    let mut streamed = PlaysInPlace::streamed(&hosted, &song_id()).expect("a stream");
    streamed
        .seek(SeekFrom::End(-16))
        .expect("a seek to the end");
    let tail = read_some(&mut streamed, 16);

    assert_eq!(found.albums[0].name, "Meddle");
    assert_eq!(album.songs.len(), 2);
    assert_eq!(tail, whole[whole.len() - 16..]);
    assert!(
        fake.heard()
            .iter()
            .filter(|asked| asked.path.starts_with("/v1/"))
            .all(|asked| asked.query.contains("countryCode=US"))
    );
}

#[test]
fn a_tidal_song_is_marked_a_favourite_and_taken_out_through_the_accounts_own_favourites() {
    let fake = tidal_server();
    let tidal = fake.tidal();

    tidal.favour(&song_id(), true).expect("a favourite kept");
    tidal
        .favour(&song_id(), false)
        .expect("a favourite taken out");
    let elsewhere = tidal.favour(&Id::new("99").expect("an id"), false);

    let writes: Vec<_> = fake
        .heard()
        .into_iter()
        .filter(|asked| asked.method != "GET" && asked.path.starts_with("/api/users/"))
        .collect();
    assert!(tidal.keeps_favourites());
    assert!(elsewhere.is_ok());
    assert_eq!(writes.len(), 3);
    assert_eq!(
        (writes[0].method.as_str(), writes[0].path.as_str()),
        ("POST", "/api/users/42/favorites/tracks")
    );
    assert_eq!(writes[0].body, "trackIds=11&onArtifactNotFound=FAIL");
    assert_eq!(
        (writes[1].method.as_str(), writes[1].path.as_str()),
        ("DELETE", "/api/users/42/favorites/tracks/11")
    );
    assert!(
        writes
            .iter()
            .all(|asked| asked.query.contains("countryCode=GB")
                && asked.authorization.as_deref() == Some("Bearer fresh"))
    );
}

#[test]
fn a_song_tidal_will_not_add_is_the_refusal_it_carries_and_a_service_with_no_account_keeps_none() {
    let fake = tidal_server();
    let tidal = fake.tidal();
    let hifi_fake = hifi_server();

    let refused = tidal.favour(&Id::new("99").expect("an id"), true);
    let kept = hifi_fake.hifi().favour(&song_id(), true);

    assert!(matches!(
        refused,
        Err(Error::TurnedAway {
            op: ProviderOp::Favourite,
            code: 404,
            ..
        })
    ));
    assert!(!hifi_fake.hifi().keeps_favourites());
    assert!(matches!(
        kept,
        Err(Error::NotOffered {
            op: ProviderOp::Favourite,
            ..
        })
    ));
}

#[test]
fn a_favourite_song_is_known_to_be_one_on_the_first_page_and_unknown_elsewhere() {
    let fake = tidal_server();
    let tidal = fake.tidal();

    let first = tidal.first_page().expect("the first page");
    let album = PlaysInPlace::album(&tidal, &Id::new("7").expect("an id")).expect("an album");

    assert_eq!(first.songs[0].favourite, Some(true));
    assert_eq!(album.songs[0].favourite, None);
}
