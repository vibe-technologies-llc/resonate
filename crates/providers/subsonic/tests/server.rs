use std::{
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{Isrc, Mbid};
use resonate_providers::{
    Delivery, Error, Identity, Obtained, Opened, Opening, Provider, ProviderOp,
};
use resonate_subsonic::{Id, Named, Patience, PlaysInPlace as _, Server, Subsonic};

const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const ECHOES_ISRC: &str = "GBN9Y1100089";
const AUDIO: &[u8] = b"fLaC and the rest of the file";
const A_PAGE: usize = 40;
const STALLED_FOR: Duration = Duration::from_secs(10);
const IMPATIENT: Duration = Duration::from_millis(400);
const GIVEN_UP_WELL_BEFORE: Duration = Duration::from_secs(5);

enum Sent {
    Whole,
    CutAfter(usize),
    StallingAfter(usize),
    Dripping { bytes: usize, apart: Duration },
}

struct Canned {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    sent: Sent,
}

impl Canned {
    fn json(body: String) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.into_bytes(),
            sent: Sent::Whole,
        }
    }

    fn audio() -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "audio/flac".to_owned())],
            body: AUDIO.to_vec(),
            sent: Sent::Whole,
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
            sent: Sent::Whole,
        }
    }

    fn unavailable() -> Self {
        Self {
            status: 503,
            headers: vec![("Retry-After", "0".to_owned())],
            body: Vec::new(),
            sent: Sent::Whole,
        }
    }

    fn sent(self, sent: Sent) -> Self {
        Self { sent, ..self }
    }
}

struct Asked {
    method: String,
    query: Option<String>,
    offset: Option<usize>,
    from: Option<usize>,
    id: Option<String>,
    format: Option<String>,
}

fn asked_from(target: &str, range: Option<&str>) -> Asked {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let parameter = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.replace("%20", " "))
    };

    Asked {
        method: path.rsplit('/').next().unwrap_or_default().to_owned(),
        query: parameter("query"),
        offset: parameter("songOffset").and_then(|offset| offset.parse().ok()),
        id: parameter("id"),
        format: parameter("format"),
        from: range
            .and_then(|range| range.strip_prefix("bytes="))
            .and_then(|range| range.trim_end_matches('-').parse().ok()),
    }
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

    fn subsonic(&self) -> Subsonic {
        Subsonic::at(Server {
            url: self.url.clone(),
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
        })
    }

    fn impatient(&self) -> Subsonic {
        self.subsonic().waiting(Patience {
            answered_within: IMPATIENT,
            broken_off_after: IMPATIENT,
            resumed_after: IMPATIENT / 8,
        })
    }

    fn heard(&self) -> Vec<(String, Option<String>, Option<usize>)> {
        self.heard
            .lock()
            .iter()
            .map(|asked| (asked.method.clone(), asked.query.clone(), asked.offset))
            .collect()
    }
}

fn answer(stream: TcpStream, heard: &Mutex<Vec<Asked>>, answering: &Answering) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut range = None;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("range")
                {
                    range = Some(value.trim().to_owned());
                }
            }
        }
    }
    let target = request_line.split(' ').nth(1).unwrap_or_default();
    let asked = asked_from(target, range.as_deref());
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
    match canned.sent {
        Sent::Whole => {
            let _ = stream.write_all(&canned.body);
        }
        Sent::CutAfter(bytes) => {
            let _ = stream.write_all(&canned.body[..bytes]);
        }
        Sent::StallingAfter(bytes) => {
            let _ = stream.write_all(&canned.body[..bytes]);
            let _ = stream.flush();
            thread::sleep(STALLED_FOR);
        }
        Sent::Dripping { bytes, apart } => {
            for drop in canned.body.chunks(bytes) {
                let _ = stream.write_all(drop);
                let _ = stream.flush();
                thread::sleep(apart);
            }
        }
    }
}

fn song(id: &str, recording: &str, isrc: &str) -> String {
    format!(
        r#"{{"id":"{id}","title":"Echoes","suffix":"flac","musicBrainzId":"{recording}","isrc":"{isrc}"}}"#
    )
}

fn found(songs: &[String]) -> Canned {
    Canned::json(format!(
        r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1","searchResult3":{{"song":[{}]}}}}}}"#,
        songs.join(",")
    ))
}

fn tributes(count: usize) -> Vec<String> {
    (0..count)
        .map(|nth| song(&format!("tribute-{nth}"), "", ""))
        .collect()
}

fn echoes() -> Identity {
    Identity {
        recording: Some(Mbid::new(ECHOES).expect("an mbid")),
        artist: Some("Pink Floyd".to_owned()),
        ..Identity::named("Echoes")
    }
}

fn opened(opening: Opening) -> Box<dyn Read + Send> {
    match opening.open().expect("the offer opens") {
        Opened::Reading { reader, .. } => reader,
        Opened::Gone => panic!("the offer was gone when opened"),
    }
}

fn streamed(obtained: Obtained) -> Option<(String, Vec<u8>)> {
    match obtained {
        Obtained::Found(Delivery::Stream { key, opening, .. }) => {
            let mut reader = opened(opening);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).expect("the stream reads");
            Some((key.into_string(), bytes))
        }
        Obtained::Found(Delivery::File(_)) | Obtained::Nothing => None,
    }
}

#[test]
fn a_search_is_answered_without_the_download_being_asked_for() {
    let fake = Fake::serving(|asked, _| match asked.method.as_str() {
        "search3" => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::audio(),
    });

    let Obtained::Found(Delivery::Stream { opening, .. }) =
        fake.subsonic().find(&echoes()).expect("an answer")
    else {
        panic!("nothing was offered");
    };
    let asked_before_opening = fake.heard();
    let mut bytes = Vec::new();
    opened(opening)
        .read_to_end(&mut bytes)
        .expect("the stream reads");

    assert!(
        asked_before_opening
            .iter()
            .all(|(method, _, _)| method == "search3"),
        "{asked_before_opening:?}"
    );
    assert_eq!(bytes, AUDIO);
}

#[test]
fn a_song_is_searched_for_by_title_and_artist_both_ways_round_and_then_by_title_page_after_page() {
    let fake = Fake::serving(
        |asked, _| match (asked.method.as_str(), asked.query.as_deref()) {
            ("search3", Some("Echoes Pink Floyd" | "Pink Floyd Echoes")) => found(&[]),
            ("search3", _) if asked.offset == Some(0) => found(&tributes(A_PAGE)),
            ("search3", _) => found(&[song("floyd", ECHOES, "")]),
            _ => Canned::audio(),
        },
    );

    let delivered = streamed(fake.subsonic().find(&echoes()).expect("an answer"));

    assert_eq!(delivered, Some(("floyd".to_owned(), AUDIO.to_vec())));
    assert_eq!(
        fake.heard(),
        vec![
            (
                "search3".to_owned(),
                Some("Echoes Pink Floyd".to_owned()),
                Some(0)
            ),
            (
                "search3".to_owned(),
                Some("Pink Floyd Echoes".to_owned()),
                Some(0)
            ),
            ("search3".to_owned(), Some("Echoes".to_owned()), Some(0)),
            (
                "search3".to_owned(),
                Some("Echoes".to_owned()),
                Some(A_PAGE)
            ),
            ("download".to_owned(), None, None),
        ]
    );
}

#[test]
fn a_short_page_ends_the_search_and_nothing_found_is_nothing_delivered() {
    let fake = Fake::serving(|_, _| found(&tributes(3)));

    let obtained = fake.subsonic().find(&echoes()).expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert_eq!(
        fake.heard().len(),
        3,
        "a page for each wording, none past the short one"
    );
}

#[test]
fn an_error_document_answering_a_download_is_a_refusal_when_the_offer_is_opened() {
    let fake = Fake::serving(|asked, _| {
        match asked.method.as_str() {
        "search3" => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::json(
            r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"not found"}}}"#
                .to_owned(),
        ),
    }
    });

    let Obtained::Found(Delivery::Stream { opening, .. }) =
        fake.subsonic().find(&echoes()).expect("an answer")
    else {
        panic!("nothing was offered");
    };

    assert!(matches!(
        opening.open(),
        Err(Error::TurnedAway {
            op: ProviderOp::Download,
            code: 70,
            ..
        })
    ));
}

#[test]
fn an_isrc_written_with_dashes_is_the_same_code() {
    let fake = Fake::serving(|asked, _| match asked.method.as_str() {
        "search3" => found(&[song("floyd", "", "gb-n9y-11-00089")]),
        _ => Canned::audio(),
    });
    let by_isrc = Identity {
        isrcs: vec![Isrc::new(ECHOES_ISRC).expect("an isrc")],
        ..Identity::named("Echoes")
    };

    let delivered = streamed(fake.subsonic().find(&by_isrc).expect("an answer"));

    assert_eq!(delivered.map(|(key, _)| key), Some("floyd".to_owned()));
}

#[test]
fn a_server_asking_to_be_asked_later_is_asked_again_and_then_answers() {
    let fake = Fake::serving(|asked, nth| match (asked.method.as_str(), nth) {
        (_, 0) => Canned::unavailable(),
        ("search3", _) => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::audio(),
    });

    let delivered = streamed(fake.subsonic().find(&echoes()).expect("an answer"));

    assert_eq!(delivered.map(|(key, _)| key), Some("floyd".to_owned()));
    assert_eq!(fake.heard().len(), 3);
}

#[test]
fn a_server_that_stays_unavailable_is_a_refusal_after_a_few_tries() {
    let fake = Fake::serving(|_, _| Canned::unavailable());

    assert!(matches!(
        fake.subsonic().find(&echoes()),
        Err(Error::Refused {
            op: ProviderOp::Search,
            status: 503,
            ..
        })
    ));
    assert_eq!(fake.heard().len(), 4);
}

#[test]
fn an_answer_that_stalls_part_way_is_given_up_within_its_deadline() {
    let fake =
        Fake::serving(|_, _| found(&[song("floyd", ECHOES, "")]).sent(Sent::StallingAfter(8)));
    let started = Instant::now();

    let refused = fake.impatient().find(&echoes());

    assert!(started.elapsed() < GIVEN_UP_WELL_BEFORE);
    assert!(matches!(
        refused,
        Err(Error::Io {
            op: ProviderOp::Search,
            ..
        })
    ));
}

#[test]
fn a_download_that_stalls_part_way_is_broken_off_rather_than_held() {
    let fake = Fake::serving(|asked, _| match asked.method.as_str() {
        "search3" => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::audio().sent(Sent::StallingAfter(AUDIO.len() / 2)),
    });
    let started = Instant::now();

    let Obtained::Found(Delivery::Stream { opening, .. }) =
        fake.impatient().find(&echoes()).expect("an answer")
    else {
        panic!("no stream was delivered");
    };
    let mut reader = opened(opening);
    let mut bytes = Vec::new();
    let broken_off = reader.read_to_end(&mut bytes);

    assert!(broken_off.is_err());
    assert_eq!(bytes, AUDIO[..AUDIO.len() / 2]);
    assert!(started.elapsed() < GIVEN_UP_WELL_BEFORE);
}

#[test]
fn a_download_cut_off_part_way_is_asked_for_again_from_where_it_stopped() {
    let half = AUDIO.len() / 2;
    let fake = Fake::serving(move |asked, _| match (asked.method.as_str(), asked.from) {
        ("search3", _) => found(&[song("floyd", ECHOES, "")]),
        (_, Some(from)) => Canned::audio_from(from),
        _ => Canned::audio().sent(Sent::CutAfter(half)),
    });

    let delivered = streamed(fake.impatient().find(&echoes()).expect("an answer"));
    let downloads: Vec<_> = fake
        .heard
        .lock()
        .iter()
        .filter(|asked| asked.method == "download")
        .map(|asked| asked.from)
        .collect();

    assert_eq!(delivered, Some(("floyd".to_owned(), AUDIO.to_vec())));
    assert_eq!(downloads, [None, Some(half)]);
}

#[test]
fn a_song_that_does_not_read_is_passed_over_and_a_numeric_id_is_read() {
    let fake = Fake::serving(|asked, _| match asked.method.as_str() {
        "search3" => Canned::json(format!(
            r#"{{"subsonic-response":{{"status":"ok","searchResult3":{{"song":[{{"id":["odd"]}},{{"id":42,"title":"Echoes","suffix":"flac","musicBrainzId":"{ECHOES}","isrc":[null,"{ECHOES_ISRC}"]}}]}}}}}}"#
        )),
        _ => Canned::audio(),
    });

    let delivered = streamed(fake.subsonic().find(&echoes()).expect("an answer"));

    assert_eq!(delivered, Some(("42".to_owned(), AUDIO.to_vec())));
}

#[test]
fn a_download_that_keeps_coming_is_read_however_long_it_takes_in_all() {
    let fake = Fake::serving(|asked, _| match asked.method.as_str() {
        "search3" => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::audio().sent(Sent::Dripping {
            bytes: 4,
            apart: IMPATIENT / 4,
        }),
    });
    let started = Instant::now();

    let delivered = streamed(fake.impatient().find(&echoes()).expect("an answer"));

    assert!(started.elapsed() > IMPATIENT);
    assert_eq!(delivered, Some(("floyd".to_owned(), AUDIO.to_vec())));
}

fn ok(body: &str) -> Canned {
    Canned::json(format!(
        r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#
    ))
}

fn not_held() -> Canned {
    Canned::json(
        r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"not found"}}}"#
            .to_owned(),
    )
}

fn id(id: &str) -> Id {
    Id::new(id).expect("an id")
}

fn a_served_library(asked: &Asked) -> Canned {
    match (asked.method.as_str(), asked.id.as_deref()) {
        ("getArtists", _) => ok(
            r#""artists":{"index":[{"name":"P","artist":[{"id":"ar-1","name":"Pink Floyd","albumCount":1},{"id":["odd"]}]}]}"#,
        ),
        ("getArtist", Some("ar-1")) => ok(
            r#""artist":{"id":"ar-1","name":"Pink Floyd","album":[{"id":"al-1","name":"Meddle","artist":"Pink Floyd","year":1971,"songCount":2}]}"#,
        ),
        ("getAlbum", Some("al-1")) => ok(
            r#""album":{"id":"al-1","name":"Meddle","artist":"Pink Floyd","coverArt":"al-1","song":[{"id":"so-1","title":"One of These Days","track":1,"discNumber":1,"duration":357,"suffix":"flac"},{"id":6,"title":"Echoes","track":6,"duration":1412,"suffix":"flac","starred":"2024-05-01T10:00:00Z"}]}"#,
        ),
        ("getSong", Some("so-1")) => ok(
            r#""song":{"id":"so-1","title":"One of These Days","album":"Meddle","suffix":"flac","coverArt":"mf-so-1"}"#,
        ),
        ("star" | "unstar", Some("so-1" | "6")) => {
            Canned::json(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#.to_owned())
        }
        ("getCoverArt", Some("al-1")) => Canned {
            status: 200,
            headers: vec![("Content-Type", "image/jpeg".to_owned())],
            body: b"\xFF\xD8 a picture".to_vec(),
            sent: Sent::Whole,
        },
        ("getCoverArt", _) => not_held(),
        ("search3", _) => ok(
            r#""searchResult3":{"artist":[{"id":"ar-1","name":"Pink Floyd"}],"album":[{"id":"al-1","name":"Meddle"}],"song":[{"id":6,"title":"Echoes"}]}"#,
        ),
        _ => not_held(),
    }
}

#[test]
fn a_servers_artists_albums_and_songs_are_browsed_by_their_ids() {
    let fake = Fake::serving(|asked, _| a_served_library(asked));
    let subsonic = fake.subsonic();

    let artists = subsonic.artists().expect("the artists");
    let albums = subsonic.artist(&artists[0].id).expect("an artist");
    let songs = subsonic.album(&albums.albums[0].id).expect("an album");
    let found = subsonic.search("Meddle").expect("a search");

    assert_eq!(
        artists.len(),
        1,
        "a listing that does not read is passed over"
    );
    assert_eq!(artists[0].name, "Pink Floyd");
    assert_eq!(artists[0].albums, Some(1));
    assert_eq!(albums.artist.name, "Pink Floyd");
    assert_eq!(albums.albums[0].name, "Meddle");
    assert_eq!(albums.albums[0].year, Some(1971));
    assert_eq!(albums.albums[0].songs, Some(2));
    assert_eq!(songs.album.name, "Meddle");
    assert_eq!(
        songs
            .songs
            .iter()
            .map(|song| (song.id.as_str(), song.title.as_str(), song.track))
            .collect::<Vec<_>>(),
        [
            ("so-1", "One of These Days", Some(1)),
            ("6", "Echoes", Some(6))
        ]
    );
    assert_eq!(songs.songs[1].length, Some(Duration::from_secs(1412)));
    assert_eq!(found.artists[0].id, id("ar-1"));
    assert_eq!(found.albums[0].id, id("al-1"));
    assert_eq!(found.songs[0].id, id("6"));
}

#[test]
fn an_album_or_an_artist_named_by_its_location_is_the_songs_it_holds_in_order() {
    let fake = Fake::serving(|asked, _| a_served_library(asked));
    let subsonic = fake.subsonic();
    let named = |uri: &str| {
        subsonic
            .named_by(&resonate_core::MediaLocation::from_uri(uri).expect("a uri"))
            .expect("a location the server names")
    };
    let ids = |named: &Named| {
        subsonic
            .songs_of(named)
            .expect("the songs")
            .into_iter()
            .map(|song| song.id)
            .collect::<Vec<_>>()
    };

    assert_eq!(ids(&named("subsonic:album/al-1")), [id("so-1"), id("6")]);
    assert_eq!(ids(&named("subsonic:artist/ar-1")), [id("so-1"), id("6")]);
    assert_eq!(ids(&named("subsonic:track/so-1")), [id("so-1")]);
    assert_eq!(
        subsonic.location_of(&Named::Song(id("6"))).to_uri(),
        "subsonic:track/6"
    );
}

#[test]
fn an_album_the_server_does_not_hold_is_turned_away_by_its_code() {
    let fake = Fake::serving(|asked, _| a_served_library(asked));

    assert!(matches!(
        fake.subsonic().album(&id("al-404")),
        Err(Error::TurnedAway {
            op: ProviderOp::Browse,
            code: 70,
            ..
        })
    ));
}

#[test]
fn a_song_is_streamed_as_stored_and_a_seek_back_asks_the_server_from_there() {
    let fake = Fake::serving(|asked, _| match asked.from {
        Some(from) => Canned::audio_from(from),
        None => Canned::audio(),
    });

    let mut streamed = fake.subsonic().streamed(&id("so-6")).expect("a stream");
    let mut head = [0_u8; 4];
    streamed.read_exact(&mut head).expect("the head reads");
    let landed = streamed.seek(SeekFrom::Start(1)).expect("a seek back");
    let mut rest = Vec::new();
    streamed.read_to_end(&mut rest).expect("the rest reads");
    let asked: Vec<_> = fake
        .heard
        .lock()
        .iter()
        .map(|asked| {
            (
                asked.method.clone(),
                asked.id.clone(),
                asked.format.clone(),
                asked.from,
            )
        })
        .collect();

    assert_eq!(streamed.length(), Some(AUDIO.len() as u64));
    assert_eq!(streamed.media_type.as_deref(), Some("audio/flac"));
    assert_eq!(&head, b"fLaC");
    assert_eq!(landed, 1);
    assert_eq!(rest, AUDIO[1..]);
    let stream = |from| {
        (
            "stream".to_owned(),
            Some("so-6".to_owned()),
            Some("raw".to_owned()),
            from,
        )
    };
    assert_eq!(asked, [stream(None), stream(Some(1))]);
}

#[test]
fn a_seek_a_little_ahead_reads_on_rather_than_asking_again_and_one_past_the_end_reads_nothing() {
    let fake = Fake::serving(|_, _| Canned::audio());

    let mut streamed = fake.subsonic().streamed(&id("so-6")).expect("a stream");
    streamed.seek(SeekFrom::Current(4)).expect("a seek ahead");
    let mut word = [0_u8; 3];
    streamed.read_exact(&mut word).expect("it reads on");
    streamed.seek(SeekFrom::End(0)).expect("a seek to the end");
    let mut past = Vec::new();
    streamed.read_to_end(&mut past).expect("nothing more");

    assert_eq!(&word, &AUDIO[4..7]);
    assert!(past.is_empty());
    assert_eq!(fake.heard().len(), 1);
}

#[test]
fn an_error_document_answering_a_stream_is_the_refusal_it_carries() {
    let fake = Fake::serving(|_, _| not_held());

    assert!(matches!(
        fake.subsonic().streamed(&id("so-404")),
        Err(Error::TurnedAway {
            op: ProviderOp::Stream,
            code: 70,
            ..
        })
    ));
}

#[test]
fn a_cover_is_asked_for_by_the_name_the_server_gives_it_and_a_song_is_asked_what_that_is() {
    let fake = Fake::serving(|asked, _| a_served_library(asked));
    let subsonic = fake.subsonic();

    let album = subsonic.album(&id("al-1")).expect("an album");
    let picture = subsonic
        .cover(album.album.cover.as_ref().expect("a cover"), 300)
        .expect("a picture");
    let song = subsonic
        .cover_of(&Named::Song(id("so-1")))
        .expect("a song's cover");
    let album_by_id = subsonic
        .cover_of(&Named::Album(id("al-1")))
        .expect("an album's cover");
    let asked = fake
        .heard()
        .into_iter()
        .filter(|(method, _, _)| method == "getCoverArt")
        .count();

    assert_eq!(picture, b"\xFF\xD8 a picture");
    assert_eq!(song, id("mf-so-1"));
    assert_eq!(album_by_id, id("al-1"));
    assert_eq!(asked, 1);
}

#[test]
fn a_cover_the_server_does_not_hold_is_the_refusal_it_carries() {
    let fake = Fake::serving(|asked, _| a_served_library(asked));

    assert!(matches!(
        fake.subsonic().cover(&id("al-404"), 300),
        Err(Error::TurnedAway {
            op: ProviderOp::Cover,
            code: 70,
            ..
        })
    ));
}

#[test]
fn a_song_the_server_has_starred_is_known_to_be_a_favourite_and_one_it_has_not_is_known_not_to_be()
{
    let fake = Fake::serving(|asked, _| a_served_library(asked));
    let subsonic = fake.subsonic();

    let album = subsonic.album(&id("al-1")).expect("an album");

    assert_eq!(
        album
            .songs
            .iter()
            .map(|song| song.favourite)
            .collect::<Vec<_>>(),
        [Some(false), Some(true)]
    );
}

#[test]
fn a_song_is_starred_and_unstarred_by_the_servers_own_calls_and_a_song_it_lacks_is_the_refusal_it_carries()
 {
    let fake = Fake::serving(|asked, _| a_served_library(asked));
    let subsonic = fake.subsonic();

    subsonic.favour(&id("so-1"), true).expect("a song starred");
    subsonic
        .favour(&id("so-1"), false)
        .expect("a song unstarred");
    let lacking = subsonic.favour(&id("nope"), true);

    let calls: Vec<_> = fake
        .heard
        .lock()
        .iter()
        .filter(|asked| asked.method.ends_with("star"))
        .map(|asked| (asked.method.clone(), asked.id.clone()))
        .collect();
    assert!(subsonic.keeps_favourites());
    assert_eq!(
        calls,
        [
            ("star".to_owned(), Some("so-1".to_owned())),
            ("unstar".to_owned(), Some("so-1".to_owned())),
            ("star".to_owned(), Some("nope".to_owned())),
        ]
    );
    assert!(matches!(
        lacking,
        Err(Error::TurnedAway {
            op: ProviderOp::Favourite,
            code: 70,
            ..
        })
    ));
}
