use std::{
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_jellyfin::{
    Flavour, Id, Jellyfin, KeptSession, Named, Patience, PlaysInPlace as _, Server,
};
use resonate_providers::{Error, ProviderOp};

const AUDIO: &[u8] = b"fLaC and the rest of the file";
const TOKEN: &str = "token-1";
const USER_ID: &str = "0a1b2c3d";
const IMPATIENT: Duration = Duration::from_millis(400);

struct Canned {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Canned {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.as_bytes().to_vec(),
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn audio() -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "audio/flac".to_owned())],
            body: AUDIO.to_vec(),
        }
    }

    fn audio_from(from: usize) -> Self {
        Self {
            status: 206,
            headers: vec![
                ("Content-Type", "audio/flac".to_owned()),
                (
                    "Content-Range",
                    format!("bytes {from}-{}/{}", AUDIO.len() - 1, AUDIO.len()),
                ),
            ],
            body: AUDIO[from..].to_vec(),
        }
    }

    fn picture() -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "image/jpeg".to_owned())],
            body: b"\xff\xd8 a cover".to_vec(),
        }
    }
}

#[derive(Clone, Debug)]
struct Asked {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    authorization: Option<String>,
    emby_authorization: Option<String>,
    range: Option<usize>,
    body: String,
}

impl Asked {
    fn parameter(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn is(&self, method: &str, path: &str) -> bool {
        self.method == method && self.path == path
    }
}

fn unescaped(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(byte) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

type Answering = dyn Fn(&Asked, usize) -> Canned + Send + Sync;

struct Fake {
    url: String,
    heard: Arc<Mutex<Vec<Asked>>>,
}

impl Fake {
    fn serving(answering: impl Fn(&Asked, usize) -> Canned + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        let heard = Arc::new(Mutex::new(Vec::new()));
        let noted = Arc::clone(&heard);
        let answering: Arc<Answering> = Arc::new(answering);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                answer(stream, &noted, answering.as_ref());
            }
        });

        Self { url, heard }
    }

    fn jellyfin(&self) -> Jellyfin {
        Jellyfin::at(Server {
            url: self.url.clone(),
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
            kept: None,
        })
    }

    fn kept(&self) -> Jellyfin {
        Jellyfin::at(Server {
            url: self.url.clone(),
            user: "listener".to_owned(),
            password: String::new(),
            kept: Some(KeptSession {
                token: TOKEN.to_owned(),
                user: USER_ID.to_owned(),
                device: "kept-device".to_owned(),
            }),
        })
    }

    fn impatient(&self) -> Jellyfin {
        self.jellyfin().waiting(Patience {
            answered_within: IMPATIENT,
            broken_off_after: IMPATIENT,
            resumed_after: IMPATIENT / 8,
        })
    }

    fn heard(&self) -> Vec<Asked> {
        self.heard.lock().clone()
    }

    fn heard_at(&self, path: &str) -> Vec<Asked> {
        self.heard()
            .into_iter()
            .filter(|asked| asked.path == path)
            .collect()
    }
}

fn answer(stream: TcpStream, heard: &Mutex<Vec<Asked>>, answering: &Answering) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut asked = Asked {
        method: request_line
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_owned(),
        path: String::new(),
        query: Vec::new(),
        authorization: None,
        emby_authorization: None,
        range: None,
        body: String::new(),
    };
    let target = request_line.split(' ').nth(1).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    asked.path = path.to_owned();
    asked.query = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), unescaped(value)))
        .collect();
    let mut length = 0;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {
                let Some((name, value)) = header.split_once(':') else {
                    continue;
                };
                let value = value.trim().to_owned();
                match name.to_ascii_lowercase().as_str() {
                    "range" => {
                        asked.range = value
                            .strip_prefix("bytes=")
                            .and_then(|range| range.trim_end_matches('-').parse().ok());
                    }
                    "content-length" => length = value.parse().unwrap_or(0),
                    "authorization" => asked.authorization = Some(value),
                    "x-emby-authorization" => asked.emby_authorization = Some(value),
                    _ => {}
                }
            }
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    asked.body = String::from_utf8_lossy(&body).into_owned();
    let nth = heard.lock().len();
    let canned = answering(&asked, nth);
    heard.lock().push(asked);

    let mut written = format!(
        "HTTP/1.1 {} Canned\r\nContent-Length: {}\r\nConnection: close\r\n",
        canned.status,
        canned.body.len()
    );
    for (name, value) in &canned.headers {
        written.push_str(&format!("{name}: {value}\r\n"));
    }
    written.push_str("\r\n");
    let mut stream = reader.into_inner();
    let _ = stream.write_all(written.as_bytes());
    let _ = stream.write_all(&canned.body);
}

fn signed_in() -> Canned {
    Canned::json(&format!(
        r#"{{"AccessToken":"{TOKEN}","User":{{"Id":"{USER_ID}","Name":"listener"}},"ServerId":"s"}}"#
    ))
}

fn items(entries: &str, total: usize) -> Canned {
    Canned::json(&format!(
        r#"{{"Items":[{entries}],"TotalRecordCount":{total},"StartIndex":0}}"#
    ))
}

fn id(id: &str) -> Id {
    Id::new(id).expect("an id")
}

fn library(asked: &Asked, _: usize) -> Canned {
    let user = format!("/Users/{USER_ID}/Items");
    if asked.is("POST", "/Users/AuthenticateByName") {
        return signed_in();
    }
    if asked.parameter("api_key") != Some(TOKEN) {
        return Canned::status(401);
    }
    match (asked.method.as_str(), asked.path.as_str()) {
        ("GET", "/Artists/AlbumArtists") => items(
            r#"{"Id":"ar-1","Name":"Pink Floyd","Type":"MusicArtist","AlbumCount":2,"ImageTags":{"Primary":"x"}}"#,
            1,
        ),
        ("GET", "/Artists") => items(
            r#"{"Id":"ar-1","Name":"Pink Floyd","Type":"MusicArtist"}"#,
            1,
        ),
        ("GET", path) if path == format!("{user}/ar-1") => Canned::json(
            r#"{"Id":"ar-1","Name":"Pink Floyd","Type":"MusicArtist","AlbumCount":2,"ImageTags":{"Primary":"x"}}"#,
        ),
        ("GET", path) if path == format!("{user}/al-1") => Canned::json(
            r#"{"Id":"al-1","Name":"Meddle","Type":"MusicAlbum","AlbumArtist":"Pink Floyd","ProductionYear":1971,"ChildCount":2,"ImageTags":{"Primary":"y"}}"#,
        ),
        ("GET", path) if path == format!("{user}/so-1") => Canned::json(
            r#"{"Id":"so-1","Name":"One of These Days","Type":"Audio","Artists":["Pink Floyd"],"Album":"Meddle","AlbumId":"al-1","AlbumPrimaryImageTag":"t","IndexNumber":1,"ParentIndexNumber":1,"RunTimeTicks":2860000000,"Container":"flac","UserData":{"IsFavorite":true}}"#,
        ),
        ("GET", path) if path == format!("{user}/so-9") => Canned::status(404),
        ("GET", path) if path == user => match asked.parameter("IncludeItemTypes") {
            Some("MusicAlbum") if asked.parameter("ArtistIds") == Some("ar-1") => items(
                r#"{"Id":"al-1","Name":"Meddle","Type":"MusicAlbum","AlbumArtist":"Pink Floyd","ProductionYear":1971,"ChildCount":2},
                   {"Id":"al-2","Name":"Obscured by Clouds","Type":"MusicAlbum","ProductionYear":1972}"#,
                2,
            ),
            Some("MusicAlbum") => items(r#"{"Id":"al-1","Name":"Meddle","Type":"MusicAlbum"}"#, 1),
            Some("Audio") if asked.parameter("ParentId") == Some("al-1") => items(
                r#"{"Id":"so-1","Name":"One of These Days","Type":"Audio","IndexNumber":1,"Container":"flac"},
                   {"Id":"so-2","Name":"A Pillow of Winds","Type":"Audio","IndexNumber":2,"Container":"mp3"}"#,
                2,
            ),
            Some("Audio") if asked.parameter("ParentId") == Some("al-2") => items(
                r#"{"Id":"so-3","Name":"Wot's... Uh the Deal","Type":"Audio","IndexNumber":1}"#,
                1,
            ),
            Some("Audio") => items(
                r#"{"Id":"so-2","Name":"A Pillow of Winds","Type":"Audio"}"#,
                1,
            ),
            _ => Canned::status(400),
        },
        ("GET", path) if path == format!("{user}/al-2") => {
            Canned::json(r#"{"Id":"al-2","Name":"Obscured by Clouds","Type":"MusicAlbum"}"#)
        }
        ("GET", "/Audio/so-1/stream") => match asked.range {
            Some(from) => Canned::audio_from(from),
            None => Canned::audio(),
        },
        ("GET", "/Items/al-1/Images/Primary") => Canned::picture(),
        ("POST", path) if path == format!("/Users/{USER_ID}/FavoriteItems/so-1") => {
            Canned::json(r#"{"IsFavorite":true}"#)
        }
        ("DELETE", path) if path == format!("/Users/{USER_ID}/FavoriteItems/so-1") => {
            Canned::json(r#"{"IsFavorite":false}"#)
        }
        _ => Canned::status(404),
    }
}

#[test]
fn a_sign_in_names_the_build_and_a_generated_device_and_nothing_of_the_listener() {
    let fake = Fake::serving(library);

    fake.jellyfin().artists().expect("the artists");

    let sign_in = &fake.heard_at("/Users/AuthenticateByName")[0];
    let authorization = sign_in.authorization.clone().expect("an authorization");
    let body: serde_json::Value = serde_json::from_str(&sign_in.body).expect("a json body");

    assert!(
        authorization
            .starts_with(r#"MediaBrowser Client="Resonate", Device="Resonate", DeviceId=""#)
    );
    assert!(authorization.ends_with(&format!(r#"Version="{}""#, env!("CARGO_PKG_VERSION"))));
    assert_eq!(sign_in.emby_authorization.as_ref(), Some(&authorization));
    assert!(!authorization.contains("listener"));
    assert_eq!(body["Username"], "listener");
    assert_eq!(body["Pw"], "sesame");
}

#[test]
fn one_sign_in_serves_every_request_and_each_carries_its_token_never_the_password() {
    let fake = Fake::serving(library);
    let jellyfin = fake.jellyfin();

    jellyfin.artists().expect("the artists");
    jellyfin.album(&id("al-1")).expect("an album");

    assert_eq!(fake.heard_at("/Users/AuthenticateByName").len(), 1);
    assert!(
        fake.heard()
            .iter()
            .filter(|asked| asked.path != "/Users/AuthenticateByName")
            .all(|asked| asked.parameter("api_key") == Some(TOKEN)
                && !asked.query.iter().any(|(_, value)| value == "sesame"))
    );
    assert_eq!(jellyfin.flavour(), Some(Flavour::Jellyfin));
}

#[test]
fn a_servers_artists_albums_and_songs_are_browsed_by_their_ids() {
    let fake = Fake::serving(library);
    let jellyfin = fake.jellyfin();

    let artists = jellyfin.artists().expect("the artists");
    let discography = jellyfin.artist(&id("ar-1")).expect("an artist");
    let album = jellyfin.album(&id("al-1")).expect("an album");
    let song = jellyfin.song(&id("so-1")).expect("a song");

    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].name, "Pink Floyd");
    assert_eq!(artists[0].albums, Some(2));
    assert_eq!(artists[0].cover, Some(id("ar-1")));
    assert_eq!(discography.artist.name, "Pink Floyd");
    assert_eq!(
        discography
            .albums
            .iter()
            .map(|album| (album.name.as_str(), album.year))
            .collect::<Vec<_>>(),
        [("Meddle", Some(1971)), ("Obscured by Clouds", Some(1972))]
    );
    assert_eq!(album.album.name, "Meddle");
    assert_eq!(album.album.artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(album.album.cover, Some(id("al-1")));
    assert_eq!(
        album
            .songs
            .iter()
            .map(|song| (song.title.as_str(), song.track, song.suffix.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("One of These Days", Some(1), Some("flac")),
            ("A Pillow of Winds", Some(2), Some("mp3"))
        ]
    );
    assert_eq!(song.length, Some(Duration::from_secs(286)));
    assert_eq!(song.cover, Some(id("al-1")));
    assert_eq!(song.favourite, Some(true));
}

#[test]
fn a_page_of_artists_is_followed_by_the_next_until_the_total_is_read() {
    let fake = Fake::serving(|asked, nth| {
        if asked.is("POST", "/Users/AuthenticateByName") {
            return signed_in();
        }
        let entries: Vec<String> = (0..500)
            .map(|each| format!(r#"{{"Id":"a-{nth}-{each}","Name":"Artist"}}"#))
            .collect();
        match asked.parameter("StartIndex") {
            Some("0") => items(&entries.join(","), 501),
            _ => items(r#"{"Id":"last","Name":"Last"}"#, 501),
        }
    });

    let artists = fake.jellyfin().artists().expect("the artists");

    assert_eq!(artists.len(), 501);
    assert_eq!(artists[500].id.as_str(), "last");
    assert_eq!(fake.heard_at("/Artists/AlbumArtists").len(), 2);
}

#[test]
fn a_search_asks_for_artists_albums_and_songs_by_the_words() {
    let fake = Fake::serving(library);

    let found = fake.jellyfin().search("pillow of winds").expect("a search");

    assert_eq!(found.artists.len(), 1);
    assert_eq!(found.albums.len(), 1);
    assert_eq!(found.songs[0].title, "A Pillow of Winds");
    assert!(
        fake.heard()
            .iter()
            .filter(|asked| asked.method == "GET")
            .all(|asked| asked.parameter("SearchTerm") == Some("pillow of winds"))
    );
}

#[test]
fn an_album_or_an_artist_named_by_its_location_is_the_songs_it_holds_in_order() {
    let fake = Fake::serving(library);
    let jellyfin = fake.jellyfin();
    let album = jellyfin.location_of(&Named::Album(id("al-1")));
    let artist = jellyfin.location_of(&Named::Artist(id("ar-1")));

    let from_album = jellyfin
        .songs_of(&jellyfin.named_by(&album).expect("a named album"))
        .expect("songs");
    let from_artist = jellyfin
        .songs_of(&jellyfin.named_by(&artist).expect("a named artist"))
        .expect("songs");

    assert_eq!(album.to_uri(), "jellyfin:album/al-1");
    assert_eq!(
        from_album
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["so-1", "so-2"]
    );
    assert_eq!(
        from_artist
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["so-1", "so-2", "so-3"]
    );
}

#[test]
fn a_song_the_server_does_not_hold_is_the_refusal_it_carries() {
    let fake = Fake::serving(library);

    assert!(matches!(
        fake.jellyfin().song(&id("so-9")),
        Err(Error::Refused {
            status: 404,
            op: ProviderOp::Browse,
            ..
        })
    ));
}

#[test]
fn an_id_that_would_leave_its_path_is_never_asked() {
    let fake = Fake::serving(library);

    assert!(matches!(
        fake.jellyfin().song(&id("../../System/Info")),
        Err(Error::Unreadable { .. })
    ));
    assert!(fake.heard().is_empty());
}

#[test]
fn a_wrong_password_is_the_listener_unwelcome_at_the_sign_in() {
    let fake = Fake::serving(|_, _| Canned::status(401));

    assert!(matches!(
        fake.jellyfin().artists(),
        Err(Error::Unwelcome {
            op: ProviderOp::SignIn,
            code: 401,
            ..
        })
    ));
    assert_eq!(fake.heard().len(), 1);
}

#[test]
fn a_token_the_server_turned_down_is_signed_in_for_again_once() {
    let signed = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&signed);
    let fake = Fake::serving(move |asked, _| {
        if asked.is("POST", "/Users/AuthenticateByName") {
            *counted.lock() += 1;
            return signed_in();
        }
        if *counted.lock() < 2 {
            return Canned::status(401);
        }
        library(asked, 0)
    });

    let artists = fake.jellyfin().artists().expect("the artists");

    assert_eq!(artists.len(), 1);
    assert_eq!(*signed.lock(), 2);
}

#[test]
fn a_kept_session_is_asked_with_its_token_and_never_signs_in() {
    let fake = Fake::serving(library);

    let artists = fake.kept().artists().expect("the artists");

    assert_eq!(artists.len(), 1);
    assert!(fake.heard_at("/Users/AuthenticateByName").is_empty());
    assert!(
        fake.heard()
            .iter()
            .all(|asked| asked.parameter("api_key") == Some(TOKEN))
    );
}

#[test]
fn a_kept_token_turned_away_is_the_account_turned_away_and_not_a_new_sign_in() {
    let fake = Fake::serving(|_, _| Canned::status(401));

    assert!(matches!(
        fake.kept().artists(),
        Err(Error::Unwelcome {
            op: ProviderOp::SignIn,
            code: 401,
            ..
        })
    ));
    assert!(fake.heard_at("/Users/AuthenticateByName").is_empty());
    assert_eq!(fake.heard().len(), 1);
}

#[test]
fn a_token_turned_down_twice_is_a_refusal_and_not_a_loop() {
    let fake = Fake::serving(|asked, _| {
        if asked.is("POST", "/Users/AuthenticateByName") {
            return signed_in();
        }
        Canned::status(401)
    });

    assert!(matches!(
        fake.jellyfin().artists(),
        Err(Error::Refused { status: 401, .. })
    ));
    assert_eq!(fake.heard_at("/Users/AuthenticateByName").len(), 2);
}

#[test]
fn an_emby_server_that_lacks_the_plain_route_is_signed_in_to_under_its_own() {
    let fake = Fake::serving(|asked, nth| match asked.path.strip_prefix("/emby") {
        Some(path) => {
            let mut under = asked.clone();
            under.path = path.to_owned();
            library(&under, nth)
        }
        None => Canned::status(404),
    });
    let jellyfin = fake.jellyfin();

    let artists = jellyfin.artists().expect("the artists");

    assert_eq!(artists.len(), 1);
    assert_eq!(jellyfin.flavour(), Some(Flavour::Emby));
    assert_eq!(
        fake.heard()
            .iter()
            .map(|asked| asked.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/Users/AuthenticateByName",
            "/emby/Users/AuthenticateByName",
            "/emby/Artists/AlbumArtists"
        ]
    );
    assert!(
        fake.heard_at("/emby/Users/AuthenticateByName")[0]
            .emby_authorization
            .is_some()
    );
}

#[test]
fn an_address_that_names_the_emby_route_is_signed_in_to_there_alone() {
    let fake = Fake::serving(|asked, nth| match asked.path.strip_prefix("/emby") {
        Some(path) => {
            let mut under = asked.clone();
            under.path = path.to_owned();
            library(&under, nth)
        }
        None => Canned::status(404),
    });
    let jellyfin = Jellyfin::at(Server {
        url: format!("{}/emby/", fake.url),
        user: "listener".to_owned(),
        password: "sesame".to_owned(),
        kept: None,
    });

    jellyfin.artists().expect("the artists");

    assert_eq!(fake.heard()[0].path, "/emby/Users/AuthenticateByName");
    assert_eq!(jellyfin.flavour(), Some(Flavour::Emby));
}

#[test]
fn a_page_where_the_servers_document_was_asked_is_not_the_service() {
    let fake = Fake::serving(|_, _| Canned {
        status: 200,
        headers: vec![("Content-Type", "text/html".to_owned())],
        body: b"<!DOCTYPE html><html>a router</html>".to_vec(),
    });

    assert!(matches!(
        fake.jellyfin().artists(),
        Err(Error::NotTheService {
            op: ProviderOp::SignIn,
            ..
        })
    ));
}

#[test]
fn a_server_asking_to_be_asked_later_is_asked_again_and_then_answers() {
    let fake = Fake::serving(|asked, nth| {
        if asked.is("POST", "/Users/AuthenticateByName") {
            return signed_in();
        }
        if nth < 3 {
            return Canned {
                status: 503,
                headers: vec![("Retry-After", "0".to_owned())],
                body: Vec::new(),
            };
        }
        library(asked, nth)
    });

    assert_eq!(fake.jellyfin().artists().expect("the artists").len(), 1);
}

#[test]
fn a_server_that_never_answers_is_given_up_within_the_patience() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let url = format!("http://{}", listener.local_addr().expect("an address"));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::sleep(Duration::from_secs(10));
            drop(stream);
        }
    });
    let jellyfin = Jellyfin::at(Server {
        url,
        user: "listener".to_owned(),
        password: "sesame".to_owned(),
        kept: None,
    })
    .waiting(Patience {
        answered_within: IMPATIENT,
        broken_off_after: IMPATIENT,
        resumed_after: IMPATIENT / 8,
    });
    let started = std::time::Instant::now();

    let refused = jellyfin.artists();

    assert!(matches!(refused, Err(Error::Io { .. })));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_song_is_streamed_as_stored_and_a_seek_back_asks_the_server_from_there() {
    let fake = Fake::serving(library);
    let mut streamed = fake.impatient().streamed(&id("so-1")).expect("a stream");
    let mut whole = Vec::new();

    streamed.read_to_end(&mut whole).expect("the whole file");
    streamed.seek(SeekFrom::Start(5)).expect("a seek");
    let mut rest = Vec::new();
    streamed.read_to_end(&mut rest).expect("the rest");

    assert_eq!(whole, AUDIO);
    assert_eq!(rest, &AUDIO[5..]);
    assert_eq!(streamed.length(), Some(AUDIO.len() as u64));
    assert_eq!(streamed.media_type.as_deref(), Some("audio/flac"));
    let asked = fake.heard_at("/Audio/so-1/stream");
    assert_eq!(asked[0].parameter("static"), Some("true"));
    assert_eq!(asked[0].parameter("api_key"), Some(TOKEN));
    assert_eq!(asked[1].range, Some(5));
}

#[test]
fn an_error_document_answering_a_stream_is_unreadable_rather_than_streamed() {
    let fake = Fake::serving(|asked, nth| {
        if asked.path.starts_with("/Audio/") {
            return Canned::json(r#"{"title":"Bad Request"}"#);
        }
        library(asked, nth)
    });

    assert!(matches!(
        fake.jellyfin().streamed(&id("so-1")),
        Err(Error::Unreadable {
            op: ProviderOp::Stream,
            ..
        })
    ));
}

#[test]
fn a_cover_is_asked_for_by_the_name_the_server_gives_it_at_a_size() {
    let fake = Fake::serving(library);
    let jellyfin = fake.jellyfin();

    let named = jellyfin
        .cover_of(&Named::Song(id("so-1")))
        .expect("a cover name");
    let bytes = jellyfin.cover(&named, 300).expect("a cover");

    assert_eq!(named, id("al-1"));
    assert_eq!(bytes, b"\xff\xd8 a cover");
    let asked = &fake.heard_at("/Items/al-1/Images/Primary")[0];
    assert_eq!(asked.parameter("maxWidth"), Some("300"));
    assert_eq!(asked.parameter("maxHeight"), Some("300"));
    assert_eq!(
        jellyfin.cover_of(&Named::Album(id("al-1"))).ok(),
        Some(id("al-1"))
    );
}

#[test]
fn a_cover_the_server_does_not_hold_is_the_refusal_it_carries() {
    let fake = Fake::serving(library);

    assert!(matches!(
        fake.jellyfin().cover(&id("al-9"), 300),
        Err(Error::Refused {
            status: 404,
            op: ProviderOp::Cover,
            ..
        })
    ));
}

#[test]
fn a_song_is_marked_a_favourite_and_taken_out_through_the_users_own_favourites() {
    let fake = Fake::serving(library);
    let jellyfin = fake.jellyfin();

    jellyfin.favour(&id("so-1"), true).expect("marked");
    jellyfin.favour(&id("so-1"), false).expect("unmarked");

    let path = format!("/Users/{USER_ID}/FavoriteItems/so-1");
    let asked = fake.heard_at(&path);
    assert_eq!(
        asked
            .iter()
            .map(|asked| asked.method.as_str())
            .collect::<Vec<_>>(),
        ["POST", "DELETE"]
    );
    assert!(jellyfin.keeps_favourites());
}

#[test]
fn a_song_the_server_will_not_mark_is_the_refusal_it_carries() {
    let fake = Fake::serving(library);

    assert!(matches!(
        fake.jellyfin().favour(&id("so-9"), true),
        Err(Error::Refused {
            status: 404,
            op: ProviderOp::Favourite,
            ..
        })
    ));
}

#[test]
fn the_first_page_is_the_servers_artists() {
    let fake = Fake::serving(library);

    let first = fake.jellyfin().first_page().expect("a first page");

    assert_eq!(first.artists.len(), 1);
    assert!(first.songs.is_empty());
}
