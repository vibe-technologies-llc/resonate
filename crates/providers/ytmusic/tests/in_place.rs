#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

use resonate_providers::{Error, Id, Named, PlaysInPlace, ProviderOp, Rating};
use resonate_ytmusic::{Cookies, Tool, VideoId, YtMusic};
use serde_json::{Value, json};

#[allow(dead_code)]
mod support;

use support::Fake;

const CLIENT_DATA: &str = "[\"1.20261104.00.00\"]";
const SONGS: &str = include_str!("fixtures/songs.json");
const GIVEN_UP_WELL_BEFORE: Duration = Duration::from_secs(4);
const THE_FEED: &str = "https://www.youtube.com/feed/playlists";
const LAPSED: &str =
    "echo 'WARNING: [youtube] The provided YouTube account cookies are no longer valid.' >&2";

fn cookies() -> Cookies {
    Cookies::read("/somewhere/cookies.txt")
}

fn drained(stream: &TcpStream) -> Option<bool> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        if header.trim_end().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok()?;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(line.starts_with("POST"))
}

fn serving(songs: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = format!("http://{}", listener.local_addr().expect("an address"));
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let Some(searched) = drained(&stream) else {
                continue;
            };
            let body = if searched { &songs } else { CLIENT_DATA };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    address
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

fn searching(songs: String) -> YtMusic {
    let tool = Tool::new(Some("/nowhere/at/all/yt-dlp".into()), Cookies::None);
    YtMusic::at(tool, &serving(songs)).asked_apart(Duration::ZERO)
}

fn answering(document: &Value) -> Fake {
    Fake::answering(&format!("echo '{document}'"))
}

fn music(fake: &Fake, cookies: Cookies) -> YtMusic {
    YtMusic::new(fake.tool(cookies))
}

fn an_id(id: &str) -> Id {
    Id::new(id).expect("an id")
}

fn a_video(id: &str) -> VideoId {
    VideoId::read(id).expect("a video id")
}

fn the_accounts_playlists() -> Value {
    json!({ "_type": "playlist", "entries": [
        { "_type": "url", "ie_key": "YoutubeTab", "id": "LL", "title": "Liked videos" },
        { "_type": "url", "ie_key": "YoutubeTab", "id": "WL", "title": "Watch later" },
        { "_type": "url", "ie_key": "YoutubeTab", "id": "PLabcdefghijklmnopqrstuvwxyz012345", "title": "XD" },
        { "_type": "url", "ie_key": "YoutubeTab", "id": "PLa_b", "title": "Road" },
        null,
        { "id": "no title" },
        { "id": "not an id", "title": "Elsewhere" },
    ] })
}

#[test]
fn a_search_lists_songs_alone_explicit_first_and_offers_no_clean_take() {
    let music = searching(answer_of(&[
        row("first", "Other", "Ada • One • 2:00", false),
        row("unmarked", "Bad Guy", "Ada • One • 3:00", false),
        row("explicit", "Bad Guy", "Ada • One • 3:00", true),
        row("clean", "Bad Guy (Clean)", "Ada • One • 3:00", false),
    ]));

    let found = music.search("bad guy").expect("an answer");

    let songs: Vec<&str> = found.songs.iter().map(|song| song.id.as_str()).collect();
    assert_eq!(songs, ["first", "explicit"]);
    assert_eq!(found.songs[1].rating, Rating::Explicit);
    assert_eq!(found.songs[1].artist.as_deref(), Some("Ada"));
    assert_eq!(found.songs[1].album.as_deref(), Some("One"));
    assert_eq!(found.songs[1].length, Some(Duration::from_secs(180)));
    assert!(found.albums.is_empty() && found.artists.is_empty() && found.mixes.is_empty());
}

#[test]
fn the_first_page_is_the_accounts_playlists_and_liked_songs() {
    let fake = answering(&the_accounts_playlists());

    let found = music(&fake, cookies()).first_page().expect("an answer");

    assert_eq!(found.mixes[0].name, "Liked songs");
    assert_eq!(found.mixes[0].id.as_str(), "LM");
    let rest: Vec<(&str, &str)> = found.mixes[1..]
        .iter()
        .map(|mix| (mix.id.as_str(), mix.name.as_str()))
        .collect();
    assert_eq!(
        rest,
        [
            ("PLabcdefghijklmnopqrstuvwxyz012345", "XD"),
            ("PLa_b", "Road")
        ]
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].last().map(String::as_str), Some(THE_FEED));
    for asked in ["--ignore-config", "--cookies", "-J", "--flat-playlist"] {
        assert!(calls[0].iter().any(|argument| argument == asked), "{asked}");
    }
    assert!(!calls[0].iter().any(|argument| argument == "--no-playlist"));
}

#[test]
fn with_no_cookies_the_first_page_is_empty() {
    let fake = answering(&the_accounts_playlists());

    let found = music(&fake, Cookies::None).first_page().expect("an answer");

    assert!(found.mixes.is_empty());
    assert!(fake.calls().is_empty());
}

#[test]
fn an_answer_that_holds_no_entries_is_liked_songs_alone() {
    for document in ["echo '{\"id\":\"x\"}'", "echo '{\"entries\":null}'", "true"] {
        let fake = Fake::answering(document);

        let found = music(&fake, cookies()).first_page().expect("an answer");

        assert_eq!(found.mixes.len(), 1, "{document}");
    }
}

#[test]
fn lapsed_cookies_on_the_first_page_are_the_provider_away() {
    let fake = Fake::answering(LAPSED);

    let tool = fake.tool(cookies());
    let music = YtMusic::new(tool.clone());

    let error = music.first_page().expect_err("lapsed");
    let noted = tool.formats(&VideoId::read("abc").expect("a video id"));

    assert!(error.is_the_provider_away());
    assert!(matches!(noted, Err(Error::Unwelcome { .. })));
    assert_eq!(
        fake.calls().len(),
        1,
        "the lapse is noted for what asks next"
    );
}

#[test]
fn a_playlist_lists_its_songs_in_youtubes_order() {
    let fake = answering(&json!({ "entries": [
        { "id": "second-song", "title": "Луна", "duration": 201, "channel": "Кошка - Topic",
          "uploader": "Кошка - Topic",
          "thumbnails": [
              { "url": "https://i.ytimg.com/vi/second-song/small.jpg" },
              { "url": "https://i.ytimg.com/vi/second-song/large.jpg" }
          ] },
        { "id": "first-song", "title": "Upload", "duration": 92.5, "channel": "noone",
          "uploader": "noone" },
        { "id": "uploader", "title": "Named by its uploader", "duration": 60,
          "uploader": "Ada - Topic" },
        { "id": "not an id", "title": "Passed over", "duration": 1, "channel": "Ada" },
        null,
    ] }));

    let songs = music(&fake, cookies())
        .mix(&an_id("PLa_b-1"))
        .expect("an answer")
        .songs;

    let read: Vec<(&str, &str, Option<&str>)> = songs
        .iter()
        .map(|song| {
            (
                song.id.as_str(),
                song.title.as_str(),
                song.artist.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("second-song", "Луна", Some("Кошка")),
            ("first-song", "Upload", Some("noone")),
            ("uploader", "Named by its uploader", Some("Ada")),
        ]
    );
    assert_eq!(songs[0].length, Some(Duration::from_secs(201)));
    assert_eq!(songs[1].length, Some(Duration::from_millis(92_500)));
    assert_eq!(
        songs[0].cover.as_ref().map(Id::as_str),
        Some("https://i.ytimg.com/vi/second-song/large.jpg")
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 1, "a listing whole needs no call per song");
    assert_eq!(
        calls[0].last().map(String::as_str),
        Some("https://music.youtube.com/playlist?list=PLa_b-1")
    );
    assert!(
        calls[0]
            .iter()
            .any(|argument| argument == "--ignore-config")
    );
    assert!(!calls[0].iter().any(|argument| argument == "--no-playlist"));
}

#[test]
fn a_song_short_of_a_field_shows_what_it_has_and_costs_no_call_of_its_own() {
    let fake = answering(&json!({ "entries": [
        { "id": "short", "title": "Short", "duration": null },
        { "id": "gone", "title": "[Deleted video]", "channel": "Ada" },
    ] }));

    let songs = fake
        .tool(cookies())
        .playlist(&an_id("PLa"))
        .expect("an answer")
        .songs;

    assert_eq!(songs[0].title, "Short");
    assert_eq!(songs[0].artist, None);
    assert_eq!(songs[0].length, None);
    assert_eq!(songs[1].artist.as_deref(), Some("Ada"));
    assert_eq!(songs[1].length, None);
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn a_playlist_is_named_by_its_own_title_and_liked_songs_by_their_name_whatever_it_says() {
    let fake = answering(&json!({ "title": " Road trip ", "entries": [] }));
    let music = music(&fake, cookies());

    assert_eq!(
        music.mix(&an_id("PLa")).expect("an answer").mix.name,
        "Road trip"
    );
    assert_eq!(
        music.mix(&an_id("LM")).expect("an answer").mix.name,
        "Liked songs"
    );
}

#[test]
fn a_playlist_that_gives_no_title_is_named_by_its_id() {
    let fake = answering(&json!({ "entries": [] }));

    let mix = music(&fake, cookies())
        .mix(&an_id("PLa"))
        .expect("an answer");

    assert_eq!(mix.mix.name, "PLa");
}

#[test]
fn a_song_is_named_from_one_listing_by_its_track_artist_and_album() {
    let fake = answering(&json!({
        "track": "Creep", "title": "Creep (Official)", "artist": "Radiohead",
        "channel": "Radiohead - Topic", "album": "Pablo Honey", "duration": 238.6,
    }));

    let song = music(&fake, cookies())
        .song(&a_video("9RfVp-GhKfs"))
        .expect("an answer")
        .expect("a song");

    assert_eq!(song.id.as_str(), "9RfVp-GhKfs");
    assert_eq!(song.title, "Creep");
    assert_eq!(song.artist.as_deref(), Some("Radiohead"));
    assert_eq!(song.album.as_deref(), Some("Pablo Honey"));
    assert_eq!(song.length, Some(Duration::from_millis(238_600)));
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    for asked in ["--ignore-config", "--no-playlist", "--cookies", "-J"] {
        assert!(calls[0].iter().any(|argument| argument == asked), "{asked}");
    }
    assert_eq!(
        calls[0].last().map(String::as_str),
        Some("https://music.youtube.com/watch?v=9RfVp-GhKfs")
    );
}

#[test]
fn a_video_with_no_track_is_named_by_its_title_and_its_channel_short_of_the_topic() {
    let fake = answering(&json!({ "title": "Луна", "channel": "Кошка - Topic", "duration": 201 }));

    let song = music(&fake, cookies())
        .song(&a_video("abc"))
        .expect("an answer")
        .expect("a song");

    assert_eq!(song.title, "Луна");
    assert_eq!(song.artist.as_deref(), Some("Кошка"));
    assert_eq!(song.album, None);
    assert_eq!(song.length, Some(Duration::from_secs(201)));
}

#[test]
fn a_video_gone_names_no_song_and_a_listing_naming_nothing_is_unreadable() {
    let gone = Fake::answering("echo 'ERROR: Video unavailable' >&2\nexit 1");
    let blank = answering(&json!({ "title": "  ", "channel": "Ada" }));

    assert!(matches!(
        music(&gone, cookies()).song(&a_video("abc")),
        Ok(None)
    ));
    assert!(matches!(
        music(&blank, cookies()).song(&a_video("abc")),
        Err(Error::Unreadable {
            op: ProviderOp::Search,
            ..
        })
    ));
}

#[test]
fn a_listing_that_hangs_is_given_up_at_its_deadline() {
    let fake = Fake::answering("exec sleep 30");
    let tool = fake
        .tool(cookies())
        .waiting_at_most(Duration::from_millis(100));
    let began = Instant::now();

    let playlist = tool.playlist(&an_id("PLa"));
    let playlists = tool.playlists();

    assert!(began.elapsed() < GIVEN_UP_WELL_BEFORE);
    assert!(matches!(playlist, Err(Error::Io { .. })));
    assert!(matches!(playlists, Err(Error::Io { .. })));
}

#[test]
fn a_playlist_id_that_would_leave_the_address_is_never_asked() {
    let fake = answering(&json!({ "entries": [] }));
    let music = music(&fake, cookies());

    for refused in ["a&b=c", "../a", "a b", "a?list=LM", "é", &"a".repeat(129)] {
        assert!(
            matches!(
                music.mix(&an_id(refused)),
                Err(Error::Unreadable {
                    op: ProviderOp::Browse,
                    ..
                })
            ),
            "{refused}"
        );
    }
    assert!(fake.calls().is_empty());
}

#[test]
fn a_song_is_not_offered_for_playing_in_place() {
    let fake = Fake::answering("exit 1");
    let music = music(&fake, cookies());
    let id = an_id("9RfVp-GhKfs");

    assert!(!music.plays_songs());
    assert!(music.keeps_songs());
    assert!(matches!(music.streamed(&id), Err(Error::NotOffered { .. })));
    assert!(fake.calls().is_empty());
}

#[test]
fn no_artist_or_album_is_browsed() {
    let fake = Fake::answering("exit 1");
    let music = music(&fake, cookies());
    let id = an_id("a");

    assert!(music.artists().expect("an answer").is_empty());
    assert!(matches!(
        music.artist(&id),
        Err(Error::NotOffered {
            op: ProviderOp::Browse,
            ..
        })
    ));
    assert!(matches!(
        music.album(&id),
        Err(Error::NotOffered {
            op: ProviderOp::Browse,
            ..
        })
    ));
}

#[test]
fn a_cover_is_the_rows_own_thumbnail_and_comes_only_from_youtubes_image_hosts() {
    let music = searching(SONGS.to_owned());

    let found = music.search("creep").expect("an answer");

    let cover = found.songs[0].cover.as_ref().expect("the row's thumbnail");
    assert!(
        cover
            .as_str()
            .starts_with("https://yt3.googleusercontent.com/"),
        "{cover}"
    );
    for refused in [
        "http://i.ytimg.com/a.jpg",
        "https://elsewhere.example/a.jpg",
        "https://i.ytimg.com.elsewhere.example/a.jpg",
    ] {
        assert!(matches!(
            music.cover(&an_id(refused), 120),
            Err(Error::OffItsHosts {
                op: ProviderOp::Cover,
                ..
            })
        ));
    }
    assert!(matches!(
        music.cover_of(&Named::Song(an_id("a"))),
        Err(Error::NotOffered {
            op: ProviderOp::Cover,
            ..
        })
    ));
}
