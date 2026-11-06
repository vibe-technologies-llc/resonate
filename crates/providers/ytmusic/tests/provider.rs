#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    num::NonZeroUsize,
    panic,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::Link;
use resonate_providers::{
    Delivery, Error, Extension, Identity, Obtained, Opened, Opening, Provider, ProviderOp,
};
use resonate_ytmusic::{Cookies, YtMusic};
use serde_json::{Value, json};

#[allow(dead_code)]
mod support;

use support::Fake;

const SONGS: &str = include_str!("fixtures/songs.json");
const CLIENT_DATA: &str = ")]}'\n[[\"yt.sw.adr\",null,[[[\"lt\",\"LT\",null,\"78.57.30.54\",\"resonate/0.5.0,gzip(gfe)\",67,\"1.20261104.00.00\",null]]]]]";
const CREEP: &str = "9RfVp-GhKfs";
const ANOTHER: &str = "QQnc-hM80UQ";
const CHECKED_WITH: &str = "9RfVp-GhKfs";
const AUDIO: &str = "the served stream";
const PACED_APART: Duration = Duration::from_millis(40);
const A_SHORT_WAIT_FOR_A_TURN: Duration = Duration::from_millis(100);
const THE_TURN_IS_HELD_FOR_AT_LEAST: Duration = Duration::from_millis(300);
const A_TURN_FREED_IS_TAKEN_WITHIN: Duration = Duration::from_secs(5);
const LAPSED: &str = "echo 'WARNING: [youtube] The provided YouTube account cookies are no longer valid. They have likely been rotated in the browser as a security measure.' >&2";

#[derive(Clone, Debug)]
struct Asked {
    line: String,
    headers: Vec<String>,
    body: String,
}

impl Asked {
    fn is_a_search(&self) -> bool {
        self.line
            .starts_with("POST /youtubei/v1/search?prettyPrint=false ")
    }

    fn header(&self, named: &str) -> Option<&str> {
        self.headers.iter().find_map(|header| {
            let (name, value) = header.split_once(':')?;
            name.eq_ignore_ascii_case(named).then_some(value.trim())
        })
    }
}

struct Answers {
    client_data: String,
    songs: String,
}

struct Music {
    address: String,
    asked: Arc<Mutex<Vec<Asked>>>,
}

fn request_of(stream: &TcpStream) -> Option<Asked> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end().to_owned();
        if header.is_empty() {
            break;
        }
        headers.push(header);
    }
    let mut asked = Asked {
        line: line.trim_end().to_owned(),
        headers,
        body: String::new(),
    };
    let length = asked
        .header("content-length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    asked.body = String::from_utf8_lossy(&body).into_owned();
    Some(asked)
}

fn answered(mut stream: TcpStream, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

impl Music {
    fn answering(songs: &str) -> Self {
        Self::serving(Answers {
            client_data: CLIENT_DATA.to_owned(),
            songs: songs.to_owned(),
        })
    }

    fn serving(answers: Answers) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = format!("http://{}", listener.local_addr().expect("an address"));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let noted = Arc::clone(&asked);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let Some(request) = request_of(&stream) else {
                    continue;
                };
                let body = if request.is_a_search() {
                    &answers.songs
                } else {
                    &answers.client_data
                };
                noted.lock().push(request);
                answered(stream, body);
            }
        });
        Self { address, asked }
    }

    fn provider(&self, fake: &Fake, cookies: Cookies) -> YtMusic {
        YtMusic::at(fake.tool(cookies), &self.address).asked_apart(Duration::ZERO)
    }

    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().clone()
    }

    fn searches(&self) -> Vec<Asked> {
        self.asked()
            .into_iter()
            .filter(Asked::is_a_search)
            .collect()
    }
}

fn no_server() -> Music {
    Music::answering("")
}

fn listed(formats: &[(&str, u64)]) -> String {
    let listed: Vec<Value> = formats
        .iter()
        .map(|(id, size)| json!({ "format_id": id, "filesize": size }))
        .collect();
    json!({ "formats": listed }).to_string()
}

fn keeping(formats: &[(&str, u64)]) -> Fake {
    Fake::answering(&format!(
        "case \" $* \" in *\" -J \"*) echo '{}' ;; *) printf '{AUDIO}' ;; esac",
        listed(formats)
    ))
}

fn checked_as(fake_song: &[(&str, u64)], checked: &[(&str, u64)]) -> Fake {
    Fake::answering(&format!(
        "case \"$*\" in *v={CHECKED_WITH}*) echo '{}' ;; *) echo '{}' ;; esac",
        listed(checked),
        listed(fake_song),
    ))
}

fn row(video: &str, title: &str, byline: &str, explicit: bool) -> Value {
    let badges = if explicit {
        json!([{ "musicInlineBadgeRenderer": { "icon": { "iconType": "MUSIC_EXPLICIT_BADGE" } } }])
    } else {
        json!([])
    };
    let column = |text: &str| json!({ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": text }] } } });
    json!({
        "musicResponsiveListItemRenderer": {
            "flexColumns": [column(title), column(byline)],
            "badges": badges,
            "playlistItemData": { "videoId": video }
        }
    })
}

fn answer_of(rows: &[Value]) -> String {
    json!({
        "contents": { "tabbedSearchResultsRenderer": { "tabs": [{ "tabRenderer": { "content": {
            "sectionListRenderer": { "contents": [{ "musicShelfRenderer": { "contents": rows } }] }
        } } }] } }
    })
    .to_string()
}

fn creep() -> Identity {
    Identity {
        artist: Some("Radiohead".to_owned()),
        length: Some(Duration::from_secs(239)),
        ..Identity::named("Creep")
    }
}

fn linked(url: &str) -> Identity {
    Identity {
        links: vec![Link::new("streaming", url.to_owned())],
        ..creep()
    }
}

fn song_at(video: &str) -> Identity {
    linked(&format!("https://music.youtube.com/watch?v={video}"))
}

fn offered(obtained: Obtained) -> (Box<str>, Extension, Opening) {
    match obtained {
        Obtained::Found(Delivery::Stream {
            key,
            extension,
            opening,
        }) => (key, extension, opening),
        other => panic!("no stream offered: {other:?}"),
    }
}

fn reader_of(opening: Opening) -> Box<dyn Read + Send> {
    match opening.open() {
        Ok(Opened::Reading { reader, .. }) => reader,
        Ok(Opened::Gone) => panic!("the song was gone"),
        Err(error) => panic!("the song did not open: {error}"),
    }
}

fn key_of(obtained: Obtained) -> Box<str> {
    offered(obtained).0
}

#[test]
fn the_provider_is_named_ytmusic_a_last_resort_and_holds_no_lane_from_the_others() {
    let fake = keeping(&[]);
    let music = no_server();
    let provider = music.provider(&fake, Cookies::None);

    assert_eq!(provider.source().as_str(), "ytmusic");
    assert!(provider.last_resort());
    assert_eq!(provider.downloads_at_once(), NonZeroUsize::MAX);
}

#[test]
fn a_want_that_may_not_be_listed_is_not_searched_for_and_the_tool_is_not_run() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&Identity::named("Creep"))
        .expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert!(music.asked().is_empty());
    assert!(fake.calls().is_empty());
}

#[test]
fn a_want_named_alike_is_offered_and_opened_only_when_taken() {
    let fake = keeping(&[("774", 7_800_000), ("251", 3)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");

    let (key, extension, opening) = offered(obtained);
    assert_eq!(&*key, "track/9RfVp-GhKfs");
    assert_eq!(extension.as_str(), "mka");
    assert!(!opening.is_resumable());
    assert_eq!(fake.calls().len(), 1);
    assert!(fake.calls()[0].iter().any(|argument| argument == "-J"));

    let Ok(Opened::Reading { mut reader, length }) = opening.open() else {
        panic!("the offer did not open");
    };
    let mut read = String::new();
    reader.read_to_string(&mut read).expect("the whole song");

    assert_eq!(read, AUDIO);
    assert_eq!(length, Some(7_800_000));
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn a_tier_of_aac_lands_under_the_name_it_was_served_with() {
    let fake = keeping(&[("141", 9)]);
    let music = Music::answering(SONGS);

    let (_, extension, _) = offered(
        music
            .provider(&fake, Cookies::None)
            .find(&creep())
            .expect("an answer"),
    );

    assert_eq!(extension.as_str(), "m4a");
}

#[test]
fn a_search_is_signed_out_of_the_songs_filter_as_the_web_client_read_from_the_service() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);

    music
        .provider(&fake, Cookies::read("firefox"))
        .find(&creep())
        .expect("an answer");

    let searches = music.searches();
    let body: Value = serde_json::from_str(&searches[0].body).expect("a document");
    assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
    assert_eq!(
        body["context"]["client"]["clientVersion"],
        "1.20261104.00.00"
    );
    assert_eq!(body["context"]["client"]["hl"], "en");
    assert_eq!(body["query"], "Creep Radiohead");
    assert_eq!(body["params"], "EgWKAQIIAWoKEAkQBRAKEAMQBA==");
    assert_eq!(
        searches[0].header("origin"),
        Some("https://music.youtube.com")
    );
    for asked in music.asked() {
        assert_eq!(asked.header("cookie"), None);
        assert!(
            asked
                .header("user-agent")
                .is_some_and(|agent| agent.starts_with("resonate/"))
        );
    }
}

#[test]
fn the_client_version_is_read_once_and_kept() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);
    let provider = music.provider(&fake, Cookies::None);

    provider.find(&creep()).expect("an answer");
    provider.find(&creep()).expect("an answer");

    let asked = music.asked();
    assert_eq!(asked.len(), 3);
    assert!(asked[0].line.starts_with("GET /sw.js_data "));
    assert_eq!(music.searches().len(), 2);
}

#[test]
fn a_service_that_names_no_client_version_is_the_provider_away() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::serving(Answers {
        client_data: "<html>your browser is out of date</html>".to_owned(),
        songs: SONGS.to_owned(),
    });

    let refused = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect_err("no version to ask with");

    assert!(refused.is_the_provider_away());
    assert!(matches!(
        refused,
        Error::NotTheService {
            op: ProviderOp::Search,
            ..
        }
    ));
    assert!(music.searches().is_empty());
}

#[test]
fn a_search_answered_with_a_page_is_the_provider_away_and_an_odd_document_the_wants_alone() {
    let fake = keeping(&[("774", 1)]);
    let a_page = Music::answering("<html>sorry</html>");
    let odd = Music::answering(r#"{"contents": {}}"#);

    let page_refused = a_page
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect_err("a page");
    let odd_refused = odd
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect_err("an odd document");

    assert!(matches!(page_refused, Error::NotTheService { .. }));
    assert!(page_refused.is_the_provider_away());
    assert!(matches!(odd_refused, Error::Unreadable { .. }));
    assert!(!odd_refused.is_the_provider_away());
}

#[test]
fn every_wording_is_asked_until_one_holds_the_song_and_nothing_found_is_nothing() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(&answer_of(&[]));

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert_eq!(music.searches().len(), creep().wordings().len());
    assert!(fake.calls().is_empty());

    let found = Music::answering(SONGS);
    found
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");
    assert_eq!(found.searches().len(), 1);
}

#[test]
fn every_request_waits_its_turn_apart_from_the_last() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(&answer_of(&[]));
    let provider = music
        .provider(&fake, Cookies::None)
        .asked_apart(PACED_APART);
    let began = Instant::now();

    provider.find(&creep()).expect("an answer");

    let requests = u32::try_from(music.asked().len()).expect("a few requests");
    assert!(began.elapsed() >= PACED_APART * (requests - 1));
}

#[test]
fn a_song_of_another_length_or_by_another_artist_is_not_the_song() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);
    let provider = music.provider(&fake, Cookies::None);

    let longer = provider
        .find(&Identity {
            length: Some(Duration::from_secs(250)),
            ..creep()
        })
        .expect("an answer");
    let another = provider
        .find(&Identity {
            artist: Some("Lena Hall".to_owned()),
            ..creep()
        })
        .expect("an answer");

    assert!(matches!(longer, Obtained::Nothing));
    assert!(matches!(another, Obtained::Nothing));
}

#[test]
fn any_artist_a_row_credits_may_be_the_one_wanted() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&Identity {
            artist: Some("Josy".to_owned()),
            length: Some(Duration::from_secs(131)),
            ..Identity::named("Creep (Aus the Voice Kids) (Live)")
        })
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/8RwMsvi9_Xs");
}

#[test]
fn a_want_with_a_youtube_music_link_takes_that_video_with_no_search() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&linked(
            "https://music.youtube.com/watch?list=LM&v=QQnc-hM80UQ&si=a#t",
        ))
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/QQnc-hM80UQ");
    assert!(music.asked().is_empty());
    assert_eq!(
        fake.calls()[0].last().map(String::as_str),
        Some("https://music.youtube.com/watch?v=QQnc-hM80UQ")
    );
}

#[test]
fn a_link_is_taken_though_the_want_names_no_length_or_artist_and_no_search_is_sent() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&Identity {
            links: vec![Link::new(
                "streaming",
                format!("https://music.youtube.com/watch?v={ANOTHER}"),
            )],
            ..Identity::named("You And Whose Army?")
        })
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/QQnc-hM80UQ");
    assert!(music.asked().is_empty());
}

#[test]
fn a_want_with_a_plain_youtube_link_is_searched_for() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&linked("https://www.youtube.com/watch?v=QQnc-hM80UQ"))
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/9RfVp-GhKfs");
    assert_eq!(music.searches().len(), 1);
}

#[test]
fn a_link_naming_no_video_that_can_be_asked_is_searched_for() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(SONGS);
    let provider = music.provider(&fake, Cookies::None);

    for link in [
        "https://music.youtube.com/watch?v=../../etc",
        "https://music.youtube.com/browse/MPREb_TgQPwAzodvg",
    ] {
        let obtained = provider.find(&linked(link)).expect("an answer");

        assert_eq!(&*key_of(obtained), "track/9RfVp-GhKfs");
    }
    assert_eq!(music.searches().len(), 2);
}

#[test]
fn a_linked_song_found_gone_is_searched_for_and_a_listed_one_is_passed_over() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(&answer_of(&[
        row(CREEP, "Creep", "Radiohead • Creep • 3:59", true),
        row(ANOTHER, "Creep", "Radiohead • Pablo Honey • 3:58", false),
    ]));
    let provider = music.provider(&fake, Cookies::None);
    let gone = resonate_core::MediaLocation::new(
        provider.source().clone(),
        format!("track/{CREEP}").into_boxed_str(),
    );

    let obtained = provider
        .find(&Identity {
            gone: vec![gone],
            ..song_at(CREEP)
        })
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/QQnc-hM80UQ");
}

#[test]
fn a_song_with_no_wanted_tier_is_nothing() {
    let fake = keeping(&[("140", 1), ("18", 1)]);
    let music = Music::answering(SONGS);

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
}

#[test]
fn an_explicit_row_is_taken_over_one_read_as_clean_and_over_one_unmarked() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(&answer_of(&[
        row(
            "cleanTake",
            "Creep (Clean)",
            "Radiohead • Creep • 3:59",
            false,
        ),
        row("unmarked", "Creep", "Radiohead • Creep • 3:59", false),
        row("explicit", "Creep", "Radiohead • Creep • 3:59", true),
    ]));

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/explicit");
}

#[test]
fn a_row_read_as_clean_is_not_taken_though_nothing_else_is_listed() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(&answer_of(&[row(
        "cleanTake",
        "Creep (Clean)",
        "Radiohead • Creep • 3:59",
        false,
    )]));

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");

    assert!(matches!(obtained, Obtained::Nothing));
    assert!(fake.calls().is_empty());
}

#[test]
fn an_unmarked_row_with_no_badge_may_be_taken() {
    let fake = keeping(&[("774", 1)]);
    let music = Music::answering(&answer_of(&[row(
        "unmarked",
        "Creep",
        "Radiohead • Creep • 3:59",
        false,
    )]));

    let obtained = music
        .provider(&fake, Cookies::None)
        .find(&creep())
        .expect("an answer");

    assert_eq!(&*key_of(obtained), "track/unmarked");
}

#[test]
fn lapsed_cookies_stop_the_poll_and_leave_the_want_due() {
    let fake = Fake::answering(&format!("{LAPSED}\necho '{}'", listed(&[("251", 1)])));
    let music = Music::answering(SONGS);

    let error = music
        .provider(&fake, Cookies::read("firefox"))
        .find(&creep())
        .expect_err("lapsed cookies");

    assert!(error.is_the_provider_away());
    assert!(matches!(
        error,
        Error::Unwelcome {
            op: ProviderOp::Search,
            ..
        }
    ));
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn a_song_short_of_premium_is_taken_where_the_account_still_has_it() {
    let fake = checked_as(&[("251", 1)], &[("774", 1)]);
    let music = no_server();

    let (_, extension, _) = offered(
        music
            .provider(&fake, Cookies::read("firefox"))
            .find(&song_at(ANOTHER))
            .expect("an answer"),
    );

    assert_eq!(extension.as_str(), "mka");
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn a_song_short_of_premium_is_refused_where_the_account_has_lost_it() {
    let fake = checked_as(&[("251", 1)], &[("251", 1)]);
    let music = no_server();

    let error = music
        .provider(&fake, Cookies::read("firefox"))
        .find(&song_at(ANOTHER))
        .expect_err("an account without premium");

    assert!(error.is_the_provider_away());
    assert!(matches!(
        error,
        Error::Unwelcome {
            op: ProviderOp::Search,
            ..
        }
    ));
}

#[test]
fn a_song_short_of_premium_is_taken_with_no_question_where_no_account_was_given() {
    let fake = checked_as(&[("251", 1)], &[("251", 1)]);
    let music = no_server();

    let (_, extension, _) = offered(
        music
            .provider(&fake, Cookies::None)
            .find(&song_at(ANOTHER))
            .expect("an answer"),
    );

    assert_eq!(extension.as_str(), "mka");
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn a_check_that_could_not_be_made_is_no_verdict_and_is_asked_again() {
    let fake = Fake::answering(&format!(
        "case \"$*\" in *v={CHECKED_WITH}*) exit 7 ;; *) echo '{}' ;; esac",
        listed(&[("251", 1)])
    ));
    let music = no_server();
    let provider = music.provider(&fake, Cookies::read("firefox"));

    for _ in 0..2 {
        assert!(matches!(
            provider.find(&song_at(ANOTHER)),
            Err(Error::ToolFailed { code: Some(7), .. })
        ));
    }

    assert_eq!(fake.calls().len(), 4);
}

#[test]
fn the_account_is_asked_about_once_a_poll_however_many_songs_lack_premium() {
    let fake = checked_as(&[("251", 1)], &[("774", 1)]);
    let music = no_server();
    let provider = music.provider(&fake, Cookies::read("firefox"));

    for video in ["first", "second", "third"] {
        provider.find(&song_at(video)).expect("an answer");
    }

    let asked_about_the_account = fake
        .calls()
        .iter()
        .filter(|call| call.iter().any(|argument| argument.ends_with(CHECKED_WITH)))
        .count();
    assert_eq!(asked_about_the_account, 1);
    assert_eq!(fake.calls().len(), 4);
}

#[test]
fn a_lapse_heard_in_a_download_outranks_an_account_checked_premium() {
    let fake = Fake::answering(&format!(
        "case \" $* \" in *\" -J \"*v={CHECKED_WITH}*) echo '{}' ;; *\" -J \"*) echo '{}' ;; *) {LAPSED}; printf '{AUDIO}' ;; esac",
        listed(&[("774", 1)]),
        listed(&[("251", 1)]),
    ));
    let music = no_server();
    let provider = music.provider(&fake, Cookies::read("/home/a/cookies.txt"));
    let (_, _, opening) = offered(provider.find(&song_at(ANOTHER)).expect("an answer"));

    reader_of(opening)
        .read_to_end(&mut Vec::new())
        .expect("the bytes asked for");
    let refused = provider
        .find(&song_at("second"))
        .expect_err("cookies heard lapsed");

    assert!(refused.is_the_provider_away());
    assert!(matches!(refused, Error::Unwelcome { .. }));
}

#[test]
fn one_download_runs_at_a_time_for_as_long_as_its_reader_lives() {
    let fake = keeping(&[("774", 1)]);
    let music = no_server();
    let provider = music.provider(&fake, Cookies::None);
    let (_, _, first) = offered(provider.find(&song_at(CREEP)).expect("an answer"));
    let (_, _, second) = offered(provider.find(&song_at(ANOTHER)).expect("an answer"));
    let reader = reader_of(first);
    let (opened, took_its_turn) = mpsc::channel();

    thread::spawn(move || opened.send(reader_of(second)));

    assert!(
        took_its_turn
            .recv_timeout(THE_TURN_IS_HELD_FOR_AT_LEAST)
            .is_err()
    );
    drop(reader);
    took_its_turn
        .recv_timeout(A_TURN_FREED_IS_TAKEN_WITHIN)
        .expect("the second download");
}

#[test]
fn a_turn_not_had_within_its_limit_is_busy_and_starts_no_tool() {
    let fake = keeping(&[("774", 1)]);
    let music = no_server();
    let provider = music
        .provider(&fake, Cookies::None)
        .waiting_for_a_turn_at_most(A_SHORT_WAIT_FOR_A_TURN);
    let (_, _, first) = offered(provider.find(&song_at(CREEP)).expect("an answer"));
    let (_, _, second) = offered(provider.find(&song_at(ANOTHER)).expect("an answer"));
    let _reader = reader_of(first);
    let started = fake.calls().len();

    let Err(refused) = second.open() else {
        panic!("a second download opened beside the first");
    };

    assert!(!refused.is_the_provider_away());
    assert!(matches!(
        refused,
        Error::Busy {
            op: ProviderOp::Download,
            ..
        }
    ));
    assert_eq!(fake.calls().len(), started);
}

#[test]
fn a_turn_is_handed_on_though_the_reader_holding_it_went_down_in_a_panic() {
    let fake = keeping(&[("774", 1)]);
    let music = no_server();
    let provider = music.provider(&fake, Cookies::None);
    let (_, _, first) = offered(provider.find(&song_at(CREEP)).expect("an answer"));
    let (_, _, second) = offered(provider.find(&song_at(ANOTHER)).expect("an answer"));

    let panicked = thread::spawn(move || {
        let _reader = reader_of(first);
        panic::resume_unwind(Box::new("the reader's thread went down"));
    })
    .join();
    let (opened, took_its_turn) = mpsc::channel();
    thread::spawn(move || opened.send(reader_of(second)));

    assert!(panicked.is_err());
    took_its_turn
        .recv_timeout(A_TURN_FREED_IS_TAKEN_WITHIN)
        .expect("the second download");
}

#[test]
fn a_song_found_gone_when_opened_hands_its_turn_on() {
    let fake = Fake::answering(&format!(
        "case \" $* \" in *\" -J \"*) echo '{}' ;; *v={ANOTHER}*) printf '{AUDIO}' ;; *) echo 'ERROR: Video unavailable' >&2; exit 1 ;; esac",
        listed(&[("774", 1)]),
    ));
    let music = no_server();
    let provider = music.provider(&fake, Cookies::None);
    let (_, _, gone) = offered(provider.find(&song_at(CREEP)).expect("an answer"));
    let (_, _, next) = offered(provider.find(&song_at(ANOTHER)).expect("an answer"));

    assert!(matches!(gone.open(), Ok(Opened::Gone)));
    let (opened, took_its_turn) = mpsc::channel();
    thread::spawn(move || opened.send(reader_of(next)));

    took_its_turn
        .recv_timeout(A_TURN_FREED_IS_TAKEN_WITHIN)
        .expect("the next download");
}
