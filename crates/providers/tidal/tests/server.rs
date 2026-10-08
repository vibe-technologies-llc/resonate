use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use parking_lot::Mutex;
use resonate_core::{Isrc, Link};
use resonate_providers::{
    Client, Delivery, Error, Identity, Obtained, Opened, Opening, Provider, ProviderOp, SignsIn,
};
use resonate_tidal::{Account, Endpoints, HifiApi, MediaHosts, Tidal, TidalSignIn};

const TONE: &[u8] = include_bytes!("fixtures/tone.mp4");
const ECHOES_ISRC: &str = "GBN9Y1100089";
const HEROES_TONIGHT_ISRC: &str = "GB2LD0902006";
const TRACK: u64 = 55_391_743;
const BEARER: &str = "Bearer fresh-access";

const QUEUED: &str = "f1b5c0de";

struct Canned {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    cut_after: Option<usize>,
}

impl Canned {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.as_bytes().to_vec(),
            cut_after: None,
        }
    }

    fn refused(status: u16, body: &str) -> Self {
        Self {
            status,
            ..Self::json(body)
        }
    }

    fn media(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", "audio/mp4".to_owned())],
            body: body.to_vec(),
            cut_after: None,
        }
    }
}

#[derive(Clone, Debug)]
struct Asked {
    method: String,
    path: String,
    query: String,
    authorization: Option<String>,
    range: Option<String>,
    body: String,
}

type Answering = dyn Fn(&Asked, &[Asked], &str) -> Canned + Send + Sync;

struct Fake {
    url: String,
    heard: Arc<Mutex<Vec<Asked>>>,
}

impl Fake {
    fn serving(
        answering: impl Fn(&Asked, &[Asked], &str) -> Canned + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        let heard = Arc::new(Mutex::new(Vec::new()));
        let noted = Arc::clone(&heard);
        let answering: Arc<Answering> = Arc::new(answering);
        let own = url.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                answer(stream, &noted, answering.as_ref(), &own);
            }
        });

        Self { url, heard }
    }

    fn tidal(&self) -> Tidal {
        Tidal::at(
            Account {
                client_id: "client".to_owned(),
                client_secret: Some("hush".to_owned()),
                refresh_token: "sesame".to_owned(),
            },
            Endpoints {
                auth: format!("{}/auth/token", self.url),
                device: format!("{}/auth/device", self.url),
                api: format!("{}/api/", self.url),
                openapi: format!("{}/openapi/", self.url),
                media: MediaHosts {
                    scheme: "http".to_owned(),
                    domain: "127.0.0.1".to_owned(),
                },
            },
        )
    }

    fn hifi(&self) -> HifiApi {
        HifiApi::fetching_from(
            &format!("{}/", self.url),
            MediaHosts {
                scheme: "http".to_owned(),
                domain: "127.0.0.1".to_owned(),
            },
        )
    }

    fn signing_in(&self) -> TidalSignIn {
        TidalSignIn::at(Endpoints {
            auth: format!("{}/auth/token", self.url),
            device: format!("{}/auth/device", self.url),
            ..Endpoints::tidal()
        })
    }

    fn heard(&self) -> Vec<Asked> {
        self.heard.lock().clone()
    }

    fn paths(&self) -> Vec<String> {
        self.heard().into_iter().map(|asked| asked.path).collect()
    }
}

fn answer(stream: TcpStream, heard: &Mutex<Vec<Asked>>, answering: &Answering, own: &str) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut authorization = None;
    let mut range = None;
    let mut length = 0;
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
        let value = value.trim().to_owned();
        match name.to_ascii_lowercase().as_str() {
            "authorization" => authorization = Some(value),
            "range" => range = Some(value),
            "content-length" => length = value.parse().unwrap_or_default(),
            _ => {}
        }
    }
    let mut body = vec![0_u8; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let asked = Asked {
        method,
        path: path.to_owned(),
        query: query.to_owned(),
        authorization,
        range,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let before = heard.lock().clone();
    let canned = answering(&asked, &before, own);
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
    let sent = canned.cut_after.unwrap_or(canned.body.len());
    let _ = stream.write_all(&canned.body[..sent]);
}

fn boxes(file: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 8 <= file.len() {
        let size = u32::from_be_bytes(file[at..at + 4].try_into().expect("four bytes")) as usize;
        found.push((&file[at + 4..at + 8], &file[at..at + size]));
        at += size;
    }
    found
}

fn segmented() -> Vec<Vec<u8>> {
    let mut segments = vec![Vec::new()];
    let mut opened = false;
    for (kind, whole) in boxes(TONE) {
        if matches!(kind, b"styp" | b"sidx" | b"moof") && !opened {
            segments.push(Vec::new());
            opened = true;
        }
        if kind == b"mdat" {
            opened = false;
        }
        segments
            .last_mut()
            .expect("a segment")
            .extend_from_slice(whole);
    }
    segments
}

fn frames() -> Vec<u8> {
    boxes(TONE)
        .into_iter()
        .filter(|(kind, _)| *kind == b"mdat")
        .flat_map(|(_, whole)| whole[8..].to_vec())
        .collect()
}

fn dash(media: &str, segments: usize) -> String {
    let mpd = format!(
        r#"<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static"><Period><AdaptationSet mimeType="audio/mp4"><Representation id="FLAC,8000,16" codecs="flac" bandwidth="1"><SegmentTemplate timescale="8000" initialization="{media}/0.mp4" media="{media}/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="2000" r="{}"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#,
        segments - 1
    );
    STANDARD.encode(mpd)
}

fn playback(presentation: &str, manifest: &str) -> Canned {
    Canned::json(&format!(
        r#"{{"trackId":{TRACK},"assetPresentation":"{presentation}","audioMode":"STEREO","audioQuality":"LOSSLESS","manifestMimeType":"application/dash+xml","manifest":"{manifest}"}}"#
    ))
}

fn granted() -> Canned {
    Canned::json(
        r#"{"access_token":"fresh-access","token_type":"Bearer","expires_in":604800,"user":{"countryCode":"GB"}}"#,
    )
}

fn listed(isrc: &str) -> Canned {
    Canned::json(&format!(
        r#"{{"data":[{{"id":"{TRACK}","type":"tracks","attributes":{{"title":"Echoes","isrc":"{isrc}"}}}}]}}"#
    ))
}

fn segment(path: &str) -> Option<usize> {
    path.strip_prefix("/media/")?
        .strip_suffix(".mp4")?
        .parse()
        .ok()
}

type Serving = dyn Fn(&Asked, usize, &[Asked]) -> Canned + Send + Sync;

fn whole_segment(_asked: &Asked, nth: usize, _before: &[Asked]) -> Canned {
    Canned::media(&segmented()[nth])
}

fn tidal_server(
    presentation: &'static str,
    media_at: impl Fn(&str) -> String + Send + Sync + 'static,
    media: impl Fn(&Asked, usize, &[Asked]) -> Canned + Send + Sync + 'static,
) -> Fake {
    let count = segmented().len() - 1;
    let media: Arc<Serving> = Arc::new(media);
    Fake::serving(move |asked, before, own| {
        if let Some(nth) = segment(&asked.path) {
            return media(asked, nth, before);
        }
        match asked.path.as_str() {
            "/auth/token" => granted(),
            "/openapi/tracks" => listed(ECHOES_ISRC),
            "/api/tracks/55391743/playbackinfopostpaywall" => {
                playback(presentation, &dash(&media_at(own), count))
            }
            _ => Canned::refused(404, "{}"),
        }
    })
}

fn on_itself(own: &str) -> String {
    format!("{own}/media")
}

fn by_isrc() -> Identity {
    Identity {
        isrc: Some(Isrc::new(ECHOES_ISRC).expect("an isrc")),
        artist: Some("Pink Floyd".to_owned()),
        ..Identity::named("Echoes")
    }
}

fn ncs_song() -> Identity {
    Identity {
        isrc: Some(Isrc::new(HEROES_TONIGHT_ISRC).expect("an isrc")),
        artist: Some("Janji".to_owned()),
        ..Identity::named("Heroes Tonight")
    }
}

fn opened(opening: Opening) -> Box<dyn Read + Send> {
    match opening.open().expect("the offer opens") {
        Opened::Reading(reader) => reader,
        Opened::Gone => panic!("the offer was gone when opened"),
    }
}

fn streamed(obtained: Obtained) -> Option<(String, String, Vec<u8>)> {
    match obtained {
        Obtained::Found(Delivery::Stream {
            key,
            extension,
            opening,
        }) => {
            let mut reader = opened(opening);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).expect("the stream reads");
            Some((key.into_string(), extension.to_string(), bytes))
        }
        Obtained::Found(Delivery::File(_)) | Obtained::Nothing => None,
    }
}

fn assert_native_flac(bytes: &[u8]) {
    let frames = frames();

    assert_eq!(&bytes[..4], b"fLaC");
    assert!(bytes.ends_with(&frames));
    assert_eq!(&frames[..2], b"\xff\xf8");
}

#[test]
fn a_wanted_track_is_found_by_its_isrc_and_delivered_as_native_flac() {
    let fake = tidal_server("FULL", on_itself, whole_segment);

    let (key, extension, bytes) =
        streamed(fake.tidal().find(&by_isrc()).expect("an answer")).expect("a delivery");

    assert_eq!(key, format!("track/{TRACK}"));
    assert_eq!(extension, "flac");
    assert_native_flac(&bytes);

    let heard = fake.heard();
    let signed_in = &heard[0];
    assert_eq!(signed_in.method, "POST");
    assert!(signed_in.body.contains("grant_type=refresh_token"));
    assert!(signed_in.body.contains("refresh_token=sesame"));
    assert!(signed_in.body.contains("client_secret=hush"));
    let searched = &heard[1];
    assert_eq!(searched.path, "/openapi/tracks");
    assert!(searched.query.contains("countryCode=GB"));
    assert!(
        searched
            .query
            .contains(&format!("filter%5Bisrc%5D={ECHOES_ISRC}"))
    );
    assert!(
        heard[1..3]
            .iter()
            .all(|asked| asked.authorization.as_deref() == Some(BEARER))
    );
    assert!(heard[3..].iter().all(|asked| asked.authorization.is_none()));
}

#[test]
fn a_search_is_answered_without_the_download_being_asked_for() {
    let fake = tidal_server("FULL", on_itself, whole_segment);

    let Obtained::Found(Delivery::Stream { opening, .. }) =
        fake.tidal().find(&by_isrc()).expect("an answer")
    else {
        panic!("nothing was offered");
    };
    let fetched_before_opening = fake.paths().iter().any(|path| segment(path).is_some());
    let mut bytes = Vec::new();
    opened(opening)
        .read_to_end(&mut bytes)
        .expect("the stream reads");

    assert!(!fetched_before_opening, "{:?}", fake.paths());
    assert_native_flac(&bytes);
}

#[test]
fn a_track_musicbrainz_links_to_tidal_is_taken_without_a_search() {
    let fake = tidal_server("FULL", on_itself, whole_segment);
    let linked = Identity {
        links: vec![Link::new(
            "streaming",
            format!("https://tidal.com/track/{TRACK}"),
        )],
        ..Identity::named("Echoes")
    };

    let delivered = streamed(fake.tidal().find(&linked).expect("an answer"));

    assert_native_flac(&delivered.expect("a delivery").2);
    assert!(!fake.paths().iter().any(|path| path == "/openapi/tracks"));
}

#[test]
fn a_track_whose_isrc_is_not_the_wanted_one_is_never_taken() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/token" => granted(),
        "/openapi/tracks" => listed("USUM71703861"),
        _ => Canned::refused(500, "{}"),
    });

    assert!(matches!(
        fake.tidal().find(&by_isrc()),
        Ok(Obtained::Nothing)
    ));
    assert!(
        !fake
            .paths()
            .iter()
            .any(|path| path.contains("playbackinfo"))
    );
}

#[test]
fn a_preview_is_never_delivered_for_the_track() {
    let fake = tidal_server("PREVIEW", on_itself, whole_segment);

    assert!(matches!(
        fake.tidal().find(&by_isrc()),
        Ok(Obtained::Nothing)
    ));
    assert!(fake.paths().iter().all(|path| segment(path).is_none()));
}

#[test]
fn media_named_off_the_audio_hosts_is_never_fetched() {
    let fake = tidal_server(
        "FULL",
        |own| format!("{}/media", own.replace("127.0.0.1", "localhost")),
        whole_segment,
    );

    assert!(matches!(
        fake.tidal().find(&by_isrc()),
        Err(Error::OffItsHosts {
            op: ProviderOp::Playback,
            ..
        })
    ));
    assert!(fake.paths().iter().all(|path| segment(path).is_none()));
}

#[test]
fn a_dash_stream_is_fetched_segments_ahead_and_read_in_order() {
    let fake = tidal_server("FULL", on_itself, whole_segment);
    let last = segmented().len() - 1;
    let ahead = last.min(3);

    let Obtained::Found(Delivery::Stream { opening, .. }) =
        fake.tidal().find(&by_isrc()).expect("an answer")
    else {
        panic!("nothing was offered");
    };
    let mut reader = opened(opening);
    let began = std::time::Instant::now();
    while !(1..=ahead).all(|nth| fake.paths().iter().any(|path| segment(path) == Some(nth))) {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the segments ahead of the reader were never asked for: {:?}",
            fake.paths()
        );
        thread::sleep(Duration::from_millis(5));
    }
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).expect("the stream reads");

    assert_native_flac(&bytes);
    let mut asked: Vec<usize> = fake
        .paths()
        .iter()
        .filter_map(|path| segment(path))
        .collect();
    asked.sort_unstable();
    assert_eq!(asked, (0..=last).collect::<Vec<_>>());
}

#[test]
fn a_segment_that_breaks_off_is_asked_for_again_from_where_it_stopped() {
    let fake = tidal_server("FULL", on_itself, |asked, nth, before| {
        let whole = &segmented()[nth];
        let half = whole.len() / 2;
        let asked_before = before.iter().any(|earlier| earlier.path == asked.path);
        match (&asked.range, asked_before, nth) {
            (None, false, 2) => Canned {
                cut_after: Some(half),
                ..Canned::media(whole)
            },
            (Some(range), true, 2) => {
                let from: usize = range
                    .trim_start_matches("bytes=")
                    .trim_end_matches('-')
                    .parse()
                    .expect("a range start");
                Canned {
                    status: 206,
                    headers: vec![(
                        "Content-Range",
                        format!("bytes {from}-{}/{}", whole.len() - 1, whole.len()),
                    )],
                    ..Canned::media(&whole[from..])
                }
            }
            _ => Canned::media(whole),
        }
    });

    let delivered = streamed(fake.tidal().find(&by_isrc()).expect("an answer"));

    assert_native_flac(&delivered.expect("a delivery").2);
    let resumed = fake
        .heard()
        .into_iter()
        .filter(|asked| segment(&asked.path) == Some(2))
        .collect::<Vec<_>>();
    assert_eq!(resumed.len(), 2);
    assert_eq!(
        resumed[1].range.as_deref(),
        Some(format!("bytes={}-", segmented()[2].len() / 2).as_str())
    );
}

#[test]
fn a_refresh_token_turned_away_is_the_account_and_not_the_want() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/token" => Canned::refused(
            400,
            r#"{"status":400,"error":"invalid_grant","sub_status":11101}"#,
        ),
        _ => Canned::refused(500, "{}"),
    });

    let refused = fake.tidal().find(&by_isrc());

    let Err(error) = refused else {
        panic!("a refused sign-in delivered");
    };
    assert!(matches!(
        error,
        Error::Unwelcome {
            op: ProviderOp::SignIn,
            code: 400,
            ..
        }
    ));
    assert!(error.is_the_provider_away());
    assert_eq!(fake.paths(), vec!["/auth/token".to_owned()]);
}

#[test]
fn a_session_said_to_last_past_any_clock_is_refused_as_unreadable() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/token" => Canned::json(&format!(
            r#"{{"access_token":"fresh-access","token_type":"Bearer","expires_in":{},"user":{{"countryCode":"GB"}}}}"#,
            u64::MAX
        )),
        _ => Canned::refused(500, "{}"),
    });

    let refused = fake.tidal().find(&by_isrc());

    assert!(
        matches!(
            refused,
            Err(Error::Unreadable {
                op: ProviderOp::SignIn,
                ..
            })
        ),
        "{refused:?}"
    );
}

#[test]
fn a_session_that_lapsed_signs_in_again_once() {
    let fake = Fake::serving(|asked, before, _| match asked.path.as_str() {
        "/auth/token" => granted(),
        "/openapi/tracks"
            if before
                .iter()
                .filter(|earlier| earlier.path == "/auth/token")
                .count()
                < 2 =>
        {
            Canned::refused(
                401,
                r#"{"status":401,"subStatus":11002,"userMessage":"expired"}"#,
            )
        }
        "/openapi/tracks" => Canned::json(r#"{"data":[]}"#),
        _ => Canned::refused(500, "{}"),
    });

    assert!(matches!(
        fake.tidal().find(&by_isrc()),
        Ok(Obtained::Nothing)
    ));
    assert_eq!(
        fake.paths(),
        vec![
            "/auth/token".to_owned(),
            "/openapi/tracks".to_owned(),
            "/auth/token".to_owned(),
            "/openapi/tracks".to_owned(),
        ]
    );
}

#[test]
fn a_session_turned_away_after_signing_in_again_is_unwelcome() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/token" => granted(),
        _ => Canned::refused(401, r#"{"status":401,"subStatus":11003}"#),
    });

    assert!(matches!(
        fake.tidal().find(&by_isrc()),
        Err(Error::Unwelcome {
            op: ProviderOp::Search,
            code: 11003,
            ..
        })
    ));
}

#[test]
fn a_refresh_token_tidal_rotates_is_handed_back_once_and_signed_in_with_from_then_on() {
    let fake = Fake::serving(|asked, before, _| match asked.path.as_str() {
        "/auth/token" => Canned::json(
            r#"{"access_token":"fresh-access","refresh_token":"rotated","expires_in":604800,"user":{"countryCode":"GB"}}"#,
        ),
        "/openapi/tracks"
            if before
                .iter()
                .filter(|earlier| earlier.path == "/auth/token")
                .count()
                < 2 =>
        {
            Canned::refused(401, r#"{"status":401,"subStatus":11002}"#)
        }
        "/openapi/tracks" => Canned::json(r#"{"data":[]}"#),
        _ => Canned::refused(500, "{}"),
    });
    let told = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&told);
    let tidal = fake
        .tidal()
        .telling(move |renewed| noted.lock().push(renewed.into_string()));

    assert!(matches!(tidal.find(&by_isrc()), Ok(Obtained::Nothing)));

    let signed_in = fake
        .heard()
        .into_iter()
        .filter(|asked| asked.path == "/auth/token")
        .collect::<Vec<_>>();
    assert_eq!(signed_in.len(), 2);
    assert!(signed_in[0].body.contains("refresh_token=sesame"));
    assert!(signed_in[1].body.contains("refresh_token=rotated"));
    assert_eq!(*told.lock(), vec!["rotated".to_owned()]);
}

#[test]
fn a_refresh_token_tidal_keeps_is_never_handed_back() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/token" => Canned::json(
            r#"{"access_token":"fresh-access","refresh_token":"sesame","expires_in":604800,"user":{"countryCode":"GB"}}"#,
        ),
        "/openapi/tracks" => Canned::json(r#"{"data":[]}"#),
        _ => Canned::refused(500, "{}"),
    });
    let told = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&told);
    let tidal = fake
        .tidal()
        .telling(move |renewed| noted.lock().push(renewed.into_string()));

    assert!(matches!(tidal.find(&by_isrc()), Ok(Obtained::Nothing)));
    assert!(told.lock().is_empty());
}

fn client() -> Client {
    Client {
        id: "client".to_owned(),
        secret: None,
    }
}

fn device_code() -> Canned {
    Canned::json(
        r#"{"deviceCode":"device-1","userCode":"ABCDE","verificationUri":"link.tidal.com","verificationUriComplete":"link.tidal.com/ABCDE","expiresIn":300,"interval":1}"#,
    )
}

fn never() -> bool {
    false
}

#[test]
fn a_device_sign_in_waits_while_it_is_pending_and_answers_the_refresh_token() {
    let fake = Fake::serving(|asked, before, _| match asked.path.as_str() {
        "/auth/device" => device_code(),
        "/auth/token"
            if before
                .iter()
                .filter(|earlier| earlier.path == "/auth/token")
                .count()
                < 2 =>
        {
            Canned::refused(
                400,
                r#"{"status":400,"error":"authorization_pending","sub_status":1002}"#,
            )
        }
        "/auth/token" => Canned::json(
            r#"{"access_token":"fresh-access","refresh_token":"fresh-refresh","expires_in":604800}"#,
        ),
        _ => Canned::refused(404, "{}"),
    });
    let signing_in = fake.signing_in();

    let authorizing = signing_in.authorizing(&client()).expect("a device code");
    let token = signing_in
        .authorized(&client(), &authorizing, &never)
        .expect("an answer")
        .expect("not cancelled");

    assert_eq!(authorizing.user_code, "ABCDE");
    assert_eq!(authorizing.verify_at, "https://link.tidal.com/ABCDE");
    assert_eq!(token.into_string(), "fresh-refresh");
    let heard = fake.heard();
    assert!(heard[0].body.contains("client_id=client"));
    assert!(heard[0].body.contains("scope=r_usr"));
    assert_eq!(heard.len(), 4);
    assert!(heard[1..].iter().all(|asked| {
        asked.body.contains("device_code=device-1")
            && asked
                .body
                .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
    }));
}

#[test]
fn a_device_sign_in_asks_again_through_a_server_that_did_not_answer_this_time() {
    let fake = Fake::serving(|asked, before, _| {
        let asked_already = before
            .iter()
            .filter(|earlier| earlier.path == "/auth/token")
            .count();
        match (asked.path.as_str(), asked_already) {
            ("/auth/device", _) => device_code(),
            ("/auth/token", 0) => Canned::refused(503, "{}"),
            ("/auth/token", 1) => Canned::refused(502, "<html>bad gateway</html>"),
            ("/auth/token", _) => Canned::json(
                r#"{"access_token":"fresh-access","refresh_token":"fresh-refresh","expires_in":604800}"#,
            ),
            _ => Canned::refused(404, "{}"),
        }
    });
    let signing_in = fake.signing_in();
    let authorizing = signing_in.authorizing(&client()).expect("a device code");

    let token = signing_in
        .authorized(&client(), &authorizing, &never)
        .expect("an answer through the servers' bad moments")
        .expect("not cancelled");

    assert_eq!(token.into_string(), "fresh-refresh");
}

#[test]
fn a_device_sign_in_turned_down_or_left_to_lapse_says_which() {
    let denied = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/device" => device_code(),
        _ => Canned::refused(400, r#"{"status":400,"error":"access_denied"}"#),
    });
    let lapsed = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/device" => device_code(),
        _ => Canned::refused(400, r#"{"status":400,"error":"expired_token"}"#),
    });

    for (fake, lapses) in [(denied, false), (lapsed, true)] {
        let signing_in = fake.signing_in();
        let authorizing = signing_in.authorizing(&client()).expect("a device code");
        let answer = signing_in.authorized(&client(), &authorizing, &never);
        match answer {
            Err(Error::AuthorizationLapsed { .. }) => assert!(lapses),
            Err(Error::AuthorizationDenied { .. }) => assert!(!lapses),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_device_sign_in_cancelled_while_waiting_stops_asking() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/auth/device" => device_code(),
        _ => Canned::refused(400, r#"{"status":400,"error":"authorization_pending"}"#),
    });
    let signing_in = fake.signing_in();
    let authorizing = signing_in.authorizing(&client()).expect("a device code");

    let answer = signing_in.authorized(&client(), &authorizing, &|| true);

    assert!(matches!(answer, Ok(None)));
    assert_eq!(fake.paths(), vec!["/auth/device".to_owned()]);
}

fn wrapped(canned: Canned) -> Canned {
    let body = String::from_utf8(canned.body).expect("a json body");
    Canned {
        body: format!(r#"{{"version":"2.10","data":{body}}}"#).into_bytes(),
        ..canned
    }
}

fn hifi_listed(isrc: &str) -> Canned {
    Canned::json(&format!(
        r#"{{"version":"2.10","data":{{"limit":25,"offset":0,"totalNumberOfItems":1,"items":[{{"id":{TRACK},"title":"Echoes","isrc":"{isrc}"}}]}}}}"#
    ))
}

fn queued() -> Canned {
    Canned {
        status: 202,
        headers: vec![
            ("Content-Type", "application/json".to_owned()),
            ("Retry-After", "1".to_owned()),
        ],
        body: format!(
            r#"{{"status":"pending","requestId":"{QUEUED}","queuePosition":1,"statusUrl":"/playback/requests/{QUEUED}"}}"#
        )
        .into_bytes(),
        cut_after: None,
    }
}

fn hifi_server(track: impl Fn(&Asked, &[Asked], &str) -> Canned + Send + Sync + 'static) -> Fake {
    Fake::serving(move |asked, before, own| {
        if let Some(nth) = segment(&asked.path) {
            return Canned::media(&segmented()[nth]);
        }
        match asked.path.as_str() {
            "/search/" => hifi_listed(ECHOES_ISRC),
            _ => track(asked, before, own),
        }
    })
}

fn whole_track(own: &str) -> Canned {
    wrapped(playback(
        "FULL",
        &dash(&on_itself(own), segmented().len() - 1),
    ))
}

#[test]
fn a_hifi_api_server_is_asked_by_the_isrc_and_its_track_delivered_as_native_flac() {
    let fake = hifi_server(|asked, _, own| match asked.path.as_str() {
        "/track/" => whole_track(own),
        _ => Canned::refused(404, "{}"),
    });

    let (key, extension, bytes) =
        streamed(fake.hifi().find(&by_isrc()).expect("an answer")).expect("a delivery");

    assert_eq!(key, format!("track/{TRACK}"));
    assert_eq!(extension, "flac");
    assert_native_flac(&bytes);

    let heard = fake.heard();
    assert_eq!(heard[0].path, "/search/");
    assert_eq!(heard[0].query, format!("i={ECHOES_ISRC}&limit=25"));
    assert_eq!(heard[1].path, "/track/");
    assert_eq!(
        heard[1].query,
        format!("id={TRACK}&quality=HI_RES_LOSSLESS")
    );
    assert!(heard.iter().all(|asked| asked.authorization.is_none()));
}

#[test]
fn a_hifi_api_track_whose_isrc_is_not_the_wanted_one_is_never_asked_for() {
    let fake = Fake::serving(|asked, _, _| match asked.path.as_str() {
        "/search/" => hifi_listed("USUM71703861"),
        _ => Canned::refused(500, "{}"),
    });

    assert!(matches!(
        fake.hifi().find(&by_isrc()),
        Ok(Obtained::Nothing)
    ));
    assert_eq!(fake.paths(), vec!["/search/".to_owned()]);
}

#[test]
fn a_hifi_api_request_held_in_its_queue_is_waited_for() {
    let fake = hifi_server(|asked, before, own| match asked.path.as_str() {
        "/track/" => queued(),
        path if path == format!("/playback/requests/{QUEUED}") => {
            let looked = before
                .iter()
                .filter(|earlier| earlier.path == asked.path)
                .count();
            if looked == 0 {
                queued()
            } else {
                whole_track(own)
            }
        }
        _ => Canned::refused(404, "{}"),
    });

    let delivered = streamed(fake.hifi().find(&by_isrc()).expect("an answer"));

    assert_native_flac(&delivered.expect("a delivery").2);
    let looked: Vec<_> = fake
        .heard()
        .into_iter()
        .filter(|asked| asked.path.starts_with("/playback/requests/"))
        .map(|asked| asked.method)
        .collect();
    assert_eq!(looked, vec!["GET".to_owned(), "GET".to_owned()]);
}

#[test]
fn a_hifi_api_request_queued_past_its_patience_is_withdrawn_and_the_server_counted_away() {
    let fake = hifi_server(|asked, _, _| match asked.path.as_str() {
        "/track/" => queued(),
        _ => Canned::json("{}"),
    });

    let answer = fake
        .hifi()
        .queued_for_at_most(Duration::ZERO)
        .find(&by_isrc());

    let Err(error) = answer else {
        panic!("a queue never reached is an error");
    };
    assert!(matches!(
        error,
        Error::StillQueued {
            op: ProviderOp::Playback,
            ..
        }
    ));
    assert!(error.is_the_provider_away());
    assert!(
        fake.heard().iter().any(|asked| asked.method == "DELETE"
            && asked.path == format!("/playback/requests/{QUEUED}"))
    );
}

#[test]
fn a_hifi_api_track_the_server_cannot_play_is_nothing_and_a_refused_server_is_unwelcome() {
    let missing = hifi_server(|_, _, _| Canned::refused(404, r#"{"detail":"Upstream API error"}"#));
    assert!(matches!(
        missing.hifi().find(&by_isrc()),
        Ok(Obtained::Nothing)
    ));

    let refused = hifi_server(|_, _, _| Canned::refused(401, r#"{"detail":"Upstream API error"}"#));
    let Err(error) = refused.hifi().find(&by_isrc()) else {
        panic!("a server turned away is an error");
    };
    assert!(matches!(
        error,
        Error::Unwelcome {
            code: 401,
            op: ProviderOp::Playback,
            ..
        }
    ));
    assert!(error.is_the_provider_away());
}

#[test]
fn the_hosted_hifi_service_downloads_ncs_heroes_tonight_by_isrc() {
    const GATE: &str = "RESONATE_HIFI_LIVE_TESTS";
    if env::var_os(GATE).is_none() {
        eprintln!("skipped: set {GATE} to reach the hosted hifi service");
        return;
    }

    let (key, extension, bytes) = streamed(
        HifiApi::hosted()
            .find(&ncs_song())
            .expect("the hosted hifi service answers"),
    )
    .expect("the NCS track is delivered");

    assert!(
        key.strip_prefix("track/")
            .is_some_and(|id| id.parse::<u64>().is_ok())
    );
    assert_eq!(extension, "flac");
    assert!(bytes.starts_with(b"fLaC"));
    assert!(bytes.len() > 1_000_000);
}
