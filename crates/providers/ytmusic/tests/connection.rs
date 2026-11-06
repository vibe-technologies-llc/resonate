#![cfg(unix)]

use resonate_ytmusic::{Connection, Cookies, YtMusic};

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
fn a_tool_that_will_not_run_is_told_apart_from_one_that_failed() {
    let missing = YtMusic::new(resonate_ytmusic::Tool::new(
        Some("/nowhere/at/all/yt-dlp".into()),
        cookies(),
    ));
    let failing = Fake::answering("exit 1");

    assert_eq!(missing.check(), Connection::ToolMissing);
    assert_eq!(told(&failing, cookies()), Connection::NotSignedIn);
}
