use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
};

use parking_lot::Mutex;
use resonate_core::{Isrc, Mbid};
use resonate_providers::{Delivery, Error, Identity, Obtained, Provider, ProviderOp};
use resonate_subsonic::{Server, Subsonic};

const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const ECHOES_ISRC: &str = "GBN9Y1100089";
const AUDIO: &[u8] = b"fLaC and the rest of the file";
const A_PAGE: usize = 40;

struct Canned {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Canned {
    fn json(body: String) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.into_bytes(),
        }
    }

    fn audio() -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "audio/flac".to_owned())],
            body: AUDIO.to_vec(),
        }
    }

    fn unavailable() -> Self {
        Self {
            status: 503,
            headers: vec![("Retry-After", "0".to_owned())],
            body: Vec::new(),
        }
    }
}

struct Asked {
    method: String,
    query: Option<String>,
    offset: Option<usize>,
}

fn asked_from(target: &str) -> Asked {
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
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {}
        }
    }
    let target = request_line.split(' ').nth(1).unwrap_or_default();
    let asked = asked_from(target);
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

fn streamed(obtained: Obtained) -> Option<(String, Vec<u8>)> {
    match obtained {
        Obtained::Found(Delivery::Stream {
            key, mut reader, ..
        }) => {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).expect("the stream reads");
            Some((key.into_string(), bytes))
        }
        Obtained::Found(Delivery::File(_)) | Obtained::Nothing => None,
    }
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

    let delivered = streamed(fake.subsonic().obtain(&echoes()).expect("an answer"));

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

    let obtained = fake.subsonic().obtain(&echoes()).expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert_eq!(fake.heard().len(), 2);
}

#[test]
fn an_error_document_answering_a_download_is_a_refusal_and_never_a_song() {
    let fake = Fake::serving(|asked, _| {
        match asked.method.as_str() {
        "search3" => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::json(
            r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"not found"}}}"#
                .to_owned(),
        ),
    }
    });

    assert!(matches!(
        fake.subsonic().obtain(&echoes()),
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

    let delivered = streamed(fake.subsonic().obtain(&by_isrc).expect("an answer"));

    assert_eq!(delivered.map(|(key, _)| key), Some("floyd".to_owned()));
}

#[test]
fn a_server_asking_to_be_asked_later_is_asked_again_and_then_answers() {
    let fake = Fake::serving(|asked, nth| match (asked.method.as_str(), nth) {
        (_, 0) => Canned::unavailable(),
        ("search3", _) => found(&[song("floyd", ECHOES, "")]),
        _ => Canned::audio(),
    });

    let delivered = streamed(fake.subsonic().obtain(&echoes()).expect("an answer"));

    assert_eq!(delivered.map(|(key, _)| key), Some("floyd".to_owned()));
    assert_eq!(fake.heard().len(), 3);
}

#[test]
fn a_server_that_stays_unavailable_is_a_refusal_after_a_few_tries() {
    let fake = Fake::serving(|_, _| Canned::unavailable());

    assert!(matches!(
        fake.subsonic().obtain(&echoes()),
        Err(Error::Refused {
            op: ProviderOp::Search,
            status: 503,
            ..
        })
    ));
    assert_eq!(fake.heard().len(), 4);
}
