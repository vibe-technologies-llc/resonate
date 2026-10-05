use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::Isrc;
use resonate_monochrome::{Monochrome, Patience};
use resonate_providers::{Delivery, Error, Identity, Obtained, Provider, ProviderOp};

const ECHOES_ISRC: &str = "GBN9Y1100065";
const ECHOES: &str = "154140652551016448";
const AUDIO: &[u8] = b"fLaC and the rest of a long and lossless file";
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
            headers: vec![("Content-Type", "application/octet-stream".to_owned())],
            body: AUDIO.to_vec(),
            sent: Sent::Whole,
        }
    }

    fn audio_from(from: usize) -> Self {
        Self {
            status: 206,
            headers: vec![
                ("Content-Type", "application/octet-stream".to_owned()),
                (
                    "Content-Range",
                    format!("bytes {from}-{}/{}", AUDIO.len() - 1, AUDIO.len()),
                ),
            ],
            body: AUDIO[from..].to_vec(),
            sent: Sent::Whole,
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            headers: vec![("Retry-After", "0".to_owned())],
            body: Vec::new(),
            sent: Sent::Whole,
        }
    }

    fn sent(self, sent: Sent) -> Self {
        Self { sent, ..self }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Search(String),
    Track { id: String, from: Option<usize> },
    Elsewhere(String),
}

fn decoded(query: &str) -> String {
    let mut decoded = Vec::new();
    let mut bytes = query.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().unwrap_or(b'0');
            let low = bytes.next().unwrap_or(b'0');
            let hex = [high, low];
            let text = std::str::from_utf8(&hex).unwrap_or("00");
            decoded.push(u8::from_str_radix(text, 16).unwrap_or(0));
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn asked_from(target: &str, range: Option<&str>) -> Asked {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path == "/search/tracks" {
        let words = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == "q")
            .map(|(_, value)| decoded(value))
            .unwrap_or_default();
        return Asked::Search(words);
    }
    if let Some(id) = path.strip_prefix("/track/") {
        let from = range
            .and_then(|range| range.trim().strip_prefix("bytes="))
            .and_then(|range| range.strip_suffix('-'))
            .and_then(|from| from.parse().ok());
        return Asked::Track {
            id: id.to_owned(),
            from,
        };
    }
    Asked::Elsewhere(target.to_owned())
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
                let noted = Arc::clone(&noted);
                let answering = Arc::clone(&answering);
                thread::spawn(move || answer(stream, &noted, answering.as_ref()));
            }
        });

        Self { url, heard }
    }

    fn monochrome(&self) -> Monochrome {
        Monochrome::at(&self.url)
    }

    fn impatient(&self) -> Monochrome {
        self.monochrome().waiting(Patience {
            answered_within: IMPATIENT,
            broken_off_after: IMPATIENT,
        })
    }

    fn heard(&self) -> Vec<Asked> {
        self.heard.lock().clone()
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
    let canned = {
        let mut heard = heard.lock();
        let canned = answering(&asked, heard.len());
        heard.push(asked);
        canned
    };

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
            let _ = stream.flush();
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

fn listing(id: &str, isrc: &str) -> String {
    format!(
        r#"{{"id":"{id}","trackId":"{id}","title":"Echoes","artistNames":["Pink Floyd"],"duration":1412451,"isrc":"{isrc}","playable":true}}"#
    )
}

fn found(listings: &[String]) -> Canned {
    Canned::json(format!(
        r#"{{"tracks":[{}],"releases":[],"artists":[],"topResults":[],"users":[],"playlists":[]}}"#,
        listings.join(",")
    ))
}

fn echoes() -> Identity {
    Identity {
        isrc: Some(Isrc::new(ECHOES_ISRC).expect("an isrc")),
        artist: Some("Pink Floyd".to_owned()),
        ..Identity::named("Echoes")
    }
}

fn searched(words: &str) -> Asked {
    Asked::Search(words.to_owned())
}

fn track(id: &str, from: Option<usize>) -> Asked {
    Asked::Track {
        id: id.to_owned(),
        from,
    }
}

fn streamed(obtained: Obtained) -> Option<(String, String, Vec<u8>)> {
    match obtained {
        Obtained::Found(Delivery::Stream {
            key,
            extension,
            mut reader,
        }) => {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).expect("the stream reads");
            Some((key.into_string(), extension.as_str().to_owned(), bytes))
        }
        Obtained::Found(Delivery::File(_)) | Obtained::Nothing => None,
    }
}

fn echoes_found() -> Canned {
    found(&[
        listing("176742690942394368", "USSM12409270"),
        listing(ECHOES, ECHOES_ISRC),
    ])
}

#[test]
fn the_listing_holding_the_wants_isrc_is_delivered_whole_as_flac() {
    let fake = Fake::serving(|asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        _ => Canned::audio(),
    });

    let delivered = streamed(fake.monochrome().obtain(&echoes()).expect("an answer"));

    assert_eq!(
        delivered,
        Some((format!("track/{ECHOES}"), "flac".to_owned(), AUDIO.to_vec()))
    );
    assert_eq!(
        fake.heard(),
        vec![searched("Echoes Pink Floyd"), track(ECHOES, None)]
    );
}

#[test]
fn a_track_is_searched_for_by_title_and_artist_and_then_by_title_alone() {
    let fake = Fake::serving(|asked, _| match asked {
        Asked::Search(words) if words == "Echoes Pink Floyd" => found(&[]),
        Asked::Search(_) => echoes_found(),
        _ => Canned::audio(),
    });

    let delivered = streamed(fake.monochrome().obtain(&echoes()).expect("an answer"));

    assert!(delivered.is_some());
    assert_eq!(
        fake.heard(),
        vec![
            searched("Echoes Pink Floyd"),
            searched("Echoes"),
            track(ECHOES, None)
        ]
    );
}

#[test]
fn a_same_titled_listing_under_another_isrc_is_never_delivered() {
    let fake = Fake::serving(|_, _| found(&[listing("176742690942394368", "USSM12409270")]));

    let obtained = fake.monochrome().obtain(&echoes()).expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert_eq!(
        fake.heard(),
        vec![searched("Echoes Pink Floyd"), searched("Echoes")]
    );
}

#[test]
fn a_want_with_no_isrc_asks_the_server_nothing() {
    let fake = Fake::serving(|_, _| echoes_found());

    let obtained = fake
        .monochrome()
        .obtain(&Identity {
            artist: Some("Pink Floyd".to_owned()),
            ..Identity::named("Echoes")
        })
        .expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert!(fake.heard().is_empty());
}

#[test]
fn a_track_id_that_is_not_digits_is_never_asked_for() {
    let fake = Fake::serving(|asked, _| match asked {
        Asked::Search(_) => found(&[listing("../admin", ECHOES_ISRC)]),
        _ => Canned::audio(),
    });

    let obtained = fake.monochrome().obtain(&echoes()).expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert!(
        fake.heard()
            .iter()
            .all(|asked| matches!(asked, Asked::Search(_)))
    );
}

#[test]
fn a_server_asking_to_be_asked_later_is_asked_again_and_then_answers() {
    let fake = Fake::serving(|asked, nth| match (asked, nth) {
        (_, 0) => Canned::status(429),
        (Asked::Search(_), _) => echoes_found(),
        _ => Canned::audio(),
    });

    let delivered = streamed(fake.monochrome().obtain(&echoes()).expect("an answer"));

    assert!(delivered.is_some());
    assert_eq!(fake.heard().len(), 3);
}

#[test]
fn a_server_that_stays_unavailable_is_a_refusal_after_a_few_tries_and_the_provider_away() {
    let fake = Fake::serving(|_, _| Canned::status(503));

    let refused = fake.monochrome().obtain(&echoes());

    assert!(matches!(
        &refused,
        Err(Error::Refused {
            op: ProviderOp::Search,
            status: 503,
            ..
        })
    ));
    assert!(refused.is_err_and(|error| error.is_the_provider_away()));
    assert_eq!(fake.heard().len(), 4);
}

#[test]
fn a_track_gone_from_the_server_is_nothing_delivered() {
    let fake = Fake::serving(|asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        _ => Canned::status(404),
    });

    let obtained = fake.monochrome().obtain(&echoes()).expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
}

#[test]
fn a_document_answering_a_download_is_unreadable_and_never_a_song() {
    let fake = Fake::serving(|asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        _ => Canned::json(r#"{"error":"busy"}"#.to_owned()),
    });

    assert!(matches!(
        fake.monochrome().obtain(&echoes()),
        Err(Error::Unreadable {
            op: ProviderOp::Download,
            ..
        })
    ));
}

#[test]
fn a_download_cut_off_part_way_is_asked_for_again_from_where_it_stopped() {
    let half = AUDIO.len() / 2;
    let fake = Fake::serving(move |asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        Asked::Track {
            from: Some(from), ..
        } => Canned::audio_from(*from),
        _ => Canned::audio().sent(Sent::CutAfter(half)),
    });

    let delivered = streamed(fake.monochrome().obtain(&echoes()).expect("an answer"));

    assert_eq!(delivered.map(|(_, _, bytes)| bytes), Some(AUDIO.to_vec()));
    assert_eq!(
        fake.heard(),
        vec![
            searched("Echoes Pink Floyd"),
            track(ECHOES, None),
            track(ECHOES, Some(half))
        ]
    );
}

#[test]
fn a_server_resending_the_whole_file_to_a_resume_is_read_past_what_was_held() {
    let half = AUDIO.len() / 2;
    let fake = Fake::serving(move |asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        Asked::Track { from: Some(_), .. } => Canned::audio(),
        _ => Canned::audio().sent(Sent::CutAfter(half)),
    });

    let delivered = streamed(fake.monochrome().obtain(&echoes()).expect("an answer"));

    assert_eq!(delivered.map(|(_, _, bytes)| bytes), Some(AUDIO.to_vec()));
}

#[test]
fn a_download_that_stalls_and_stays_stalled_is_broken_off_rather_than_held() {
    let half = AUDIO.len() / 2;
    let fake = Fake::serving(move |asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        Asked::Track { from: Some(_), .. } => Canned::audio().sent(Sent::StallingAfter(0)),
        _ => Canned::audio().sent(Sent::StallingAfter(half)),
    });
    let started = Instant::now();

    let Obtained::Found(Delivery::Stream { mut reader, .. }) =
        fake.impatient().obtain(&echoes()).expect("an answer")
    else {
        panic!("no stream was delivered");
    };
    let mut bytes = Vec::new();
    let broken_off = reader.read_to_end(&mut bytes);

    assert!(broken_off.is_err());
    assert_eq!(bytes, AUDIO[..half]);
    assert!(started.elapsed() < GIVEN_UP_WELL_BEFORE);
}

#[test]
fn a_download_that_keeps_coming_is_read_however_long_it_takes_in_all() {
    let fake = Fake::serving(|asked, _| match asked {
        Asked::Search(_) => echoes_found(),
        _ => Canned::audio().sent(Sent::Dripping {
            bytes: 4,
            apart: IMPATIENT / 4,
        }),
    });
    let started = Instant::now();

    let delivered = streamed(fake.impatient().obtain(&echoes()).expect("an answer"));

    assert!(started.elapsed() > IMPATIENT);
    assert_eq!(delivered.map(|(_, _, bytes)| bytes), Some(AUDIO.to_vec()));
}

#[test]
fn an_unreachable_server_is_the_provider_away() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let url = format!("http://{}", listener.local_addr().expect("a bound address"));
    drop(listener);

    let refused = Monochrome::at(&url).obtain(&echoes());

    assert!(matches!(
        &refused,
        Err(Error::Io {
            op: ProviderOp::Search,
            ..
        })
    ));
    assert!(refused.is_err_and(|error| error.is_the_provider_away()));
}
