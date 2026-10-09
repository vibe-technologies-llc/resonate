use std::{
    io::{BufRead, BufReader, Read, Write},
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
use resonate_subsonic::{Patience, Server, Subsonic};

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
        Opened::Reading(reader) => reader,
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
fn a_song_is_searched_for_by_title_and_artist_and_then_by_title_page_after_page() {
    let fake = Fake::serving(
        |asked, _| match (asked.method.as_str(), asked.query.as_deref()) {
            ("search3", Some("Echoes Pink Floyd")) => found(&[]),
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
    assert_eq!(fake.heard().len(), 2);
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
        isrc: Some(Isrc::new(ECHOES_ISRC).expect("an isrc")),
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
