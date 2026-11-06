#![cfg(unix)]

use std::{
    io::{self, Write},
    sync::Arc,
};

use parking_lot::Mutex;
use resonate_ytmusic::{Cookies, Tool, VideoId};
use tracing::Level;

#[allow(dead_code)]
mod support;

use support::Fake;

#[derive(Clone, Default)]
struct Logged(Arc<Mutex<Vec<u8>>>);

impl Write for Logged {
    fn write(&mut self, said: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(said);
        Ok(said.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn listed_by(tool: &Tool) {
    let song = VideoId::read("abc").expect("a video id");
    tool.formats(&song).expect("an answer");
}

#[test]
fn a_cookie_value_a_tool_says_back_is_withheld_from_the_log() {
    let logged = Logged::default();
    let writer = logged.clone();
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .init();
    let fake = Fake::answering(
        "echo 'ERROR: could not find firefox cookies database in /home/a/zen' >&2\n\
         echo 'WARNING: firefox:/home/a/zen::work was read' >&2\n\
         printf 'cut across two reads /home/' >&2\n\
         sleep 0.1\n\
         printf 'a/zen and on\\n' >&2\n\
         printf 'never ended /home/a/zen' >&2\n\
         echo '{\"formats\":[]}'",
    );
    let file = Fake::answering(
        "echo 'ERROR: /home/a/cookies.txt could not be read' >&2\n\
         echo '{\"formats\":[]}'",
    );

    listed_by(&fake.tool(Cookies::read("firefox:/home/a/zen::work")));
    listed_by(&file.tool(Cookies::read("/home/a/cookies.txt")));

    let log = String::from_utf8_lossy(&logged.0.lock()).into_owned();
    assert!(
        log.contains("could not find firefox cookies database"),
        "{log}"
    );
    assert!(log.contains("cut across two reads"), "{log}");
    assert!(log.contains("never ended"), "{log}");
    assert!(log.contains("could not be read"), "{log}");
    assert!(!log.contains("/home/a"), "{log}");
}
