#![cfg(unix)]

use std::fs;

use resonate_providers::Error;
use resonate_ytmusic::{Connection, Cookies, VideoId, YtMusic};

#[allow(dead_code)]
mod support;

use support::Fake;

const CHECKED_WITH: &str = "https://music.youtube.com/watch?v=9RfVp-GhKfs";
const LAPSED: &str =
    "echo 'WARNING: [youtube] The provided YouTube account cookies are no longer valid.' >&2";

fn cookies() -> Cookies {
    Cookies::read("/somewhere/cookies.txt")
}

fn told(fake: &Fake, cookies: Cookies) -> Connection {
    YtMusic::new(fake.tool(cookies)).check()
}

#[test]
fn a_connection_is_told_premium_from_signed_in_from_signed_out() {
    let premium = Fake::listing(&[("774", 1), ("251", 1)]);
    let aac_alone = Fake::listing(&[("141", 1)]);
    let free = Fake::listing(&[("251", 1)]);
    let lapsed = Fake::answering(&format!("{LAPSED}\necho '{{\"formats\":[]}}'"));

    assert_eq!(told(&premium, cookies()), Connection::Premium);
    assert_eq!(told(&aac_alone, cookies()), Connection::Premium);
    assert_eq!(told(&free, cookies()), Connection::SignedInWithoutPremium);
    assert_eq!(told(&free, Cookies::None), Connection::NotSignedIn);
    assert_eq!(told(&lapsed, cookies()), Connection::NotSignedIn);
}

#[test]
fn a_connection_is_checked_on_the_album_song_alone() {
    let fake = Fake::listing(&[("774", 1)]);

    told(&fake, cookies());

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].last().map(String::as_str), Some(CHECKED_WITH));
}

#[test]
fn a_check_asks_though_a_lapse_was_noted_and_a_good_answer_clears_it() {
    let fake = Fake::answering(&format!(
        "if [ -e \"$here/exported\" ]; then echo '{{\"formats\":[{{\"format_id\":\"774\"}}]}}'; else {LAPSED}; fi"
    ));
    let tool = fake.tool(cookies());
    let music = YtMusic::new(tool.clone());
    let song = VideoId::read("abc").expect("a video id");
    let lapsed = |asked: Result<_, Error>| matches!(asked, Err(Error::Unwelcome { .. }));

    assert!(lapsed(tool.formats(&song)));
    assert!(lapsed(tool.formats(&song)));
    assert_eq!(fake.calls().len(), 1, "a lapse noted spares the tool");

    assert_eq!(music.check(), Connection::NotSignedIn);
    assert_eq!(fake.calls().len(), 2, "a check asks whatever was noted");
    assert!(lapsed(tool.formats(&song)));
    assert_eq!(fake.calls().len(), 2, "a lapse heard again stands");

    fs::write(fake.folder().join("exported"), b"").expect("the cookies exported again");
    assert_eq!(music.check(), Connection::Premium);
    assert_eq!(fake.calls().len(), 3);

    assert!(tool.formats(&song).is_ok());
    assert_eq!(fake.calls().len(), 4, "a good answer clears the note");
}

#[test]
fn a_tool_that_will_not_run_is_told_apart_from_one_that_failed() {
    let missing = YtMusic::new(resonate_ytmusic::Tool::new(
        Some("/nowhere/at/all/yt-dlp".into()),
        cookies(),
    ));
    let failing = Fake::answering("exit 1");

    assert_eq!(missing.check(), Connection::ToolMissing);
    assert_eq!(told(&failing, cookies()), Connection::CouldNotCheck);
}

#[test]
fn a_check_that_could_not_be_made_is_told_apart_from_not_being_signed_in() {
    let network = Fake::answering(
        "echo 'ERROR: [youtube] 9RfVp-GhKfs: Unable to download API page: HTTP Error 503' >&2\nexit 1",
    );
    let bot_check = Fake::answering(
        "echo \"ERROR: [youtube] 9RfVp-GhKfs: Sign in to confirm you're not a bot\" >&2\nexit 1",
    );
    let nonsense = Fake::answering("echo 'not a listing'");

    assert_eq!(told(&network, cookies()), Connection::CouldNotCheck);
    assert_eq!(told(&bot_check, cookies()), Connection::CouldNotCheck);
    assert_eq!(told(&nonsense, cookies()), Connection::CouldNotCheck);
}

#[test]
fn the_check_song_taken_down_is_told_as_that_with_cookies_or_without() {
    let gone =
        Fake::answering("echo 'ERROR: [youtube] 9RfVp-GhKfs: Video unavailable' >&2\nexit 1");

    assert_eq!(told(&gone, cookies()), Connection::CheckSongGone);
    assert_eq!(told(&gone, Cookies::None), Connection::CheckSongGone);
}

#[test]
fn the_check_song_gone_leaves_a_lapse_standing() {
    let fake = Fake::answering(&format!(
        "if [ -e \"$here/taken-down\" ]; then echo 'ERROR: [youtube] 9RfVp-GhKfs: Video unavailable' >&2; exit 1; else {LAPSED}; fi"
    ));
    let tool = fake.tool(cookies());
    let music = YtMusic::new(tool.clone());
    let song = VideoId::read("abc").expect("a video id");
    assert!(matches!(tool.formats(&song), Err(Error::Unwelcome { .. })));

    fs::write(fake.folder().join("taken-down"), b"").expect("the check song taken down");
    assert_eq!(music.check(), Connection::CheckSongGone);

    assert!(matches!(tool.formats(&song), Err(Error::Unwelcome { .. })));
    assert_eq!(fake.calls().len(), 2, "the lapse still spares the tool");
}
