#![cfg(unix)]

use std::{env, io::Read, time::Duration};

use resonate_providers::Id;
use resonate_ytmusic::{Connection, Cookies, Tier, Tool, VideoId, YtMusic};

const GATE: &str = "RESONATE_YTMUSIC_TESTS";
const COOKIES: &str = "RESONATE_YTMUSIC_COOKIES";
const SONG: &str = "RESONATE_YTMUSIC_SONG";
const PLAYLIST: &str = "RESONATE_YTMUSIC_PLAYLIST";
const ALBUM_SONG: &str = "9RfVp-GhKfs";
const MATROSKA_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];
const MP4_BOX_NAME: &[u8; 4] = b"ftyp";
const MP4_BOX_NAME_AT: usize = 4;
const HEAD_BYTES: usize = 4096;
const PATIENCE: Duration = Duration::from_secs(120);

fn gated() -> bool {
    let open = env::var_os(GATE).is_some();
    if !open {
        eprintln!("skipped: set {GATE} to reach YouTube Music through yt-dlp");
    }
    open
}

fn cookies() -> Cookies {
    Cookies::read(&env::var(COOKIES).unwrap_or_default())
}

fn tool() -> Tool {
    Tool::new(None, cookies()).waiting_at_most(PATIENCE)
}

fn song() -> VideoId {
    let named = env::var(SONG).unwrap_or_else(|_| ALBUM_SONG.to_owned());
    VideoId::read(&named).expect("the song named is a video id")
}

fn head_of(tier: Tier, tool: &Tool, video: &VideoId) -> Vec<u8> {
    let resonate_providers::Opened::Reading { mut reader, .. } = tool
        .download(video, tier)
        .expect("yt-dlp starts the download")
    else {
        panic!("the song is gone from YouTube Music");
    };
    let mut head = vec![0; HEAD_BYTES];
    reader
        .read_exact(&mut head)
        .expect("the stream delivers its head");
    head
}

#[test]
fn a_real_album_song_is_listed_and_its_best_tier_arrives_as_the_stream_it_names() {
    if !gated() {
        return;
    }
    let tool = tool();
    let video = song();

    let formats = tool.formats(&video).expect("yt-dlp lists the song");
    let best = formats.best.expect("the song offers a tier");
    let head = head_of(best, &tool, &video);

    match best.extension() {
        "mka" => assert_eq!(head[..4], MATROSKA_MAGIC),
        "m4a" => assert_eq!(
            &head[MP4_BOX_NAME_AT..MP4_BOX_NAME_AT + 4],
            MP4_BOX_NAME,
            "an MP4 head opens with its ftyp box"
        ),
        other => panic!("a tier with an extension nothing reads: {other}"),
    }
    assert!(formats.length.is_some_and(|length| length > 0));
}

#[test]
fn the_account_is_told_as_it_is_and_the_song_reads_as_a_song() {
    if !gated() {
        return;
    }
    let music = YtMusic::new(tool());

    let connection = music.check();
    let song = music
        .song(&song())
        .expect("yt-dlp reads the song")
        .expect("the song is there");

    assert_ne!(connection, Connection::ToolMissing);
    assert!(!song.title.is_empty());
    assert!(song.artist.is_some());
}

#[test]
fn a_playlist_of_the_account_lists_its_songs() {
    if !gated() {
        return;
    }
    let Ok(named) = env::var(PLAYLIST) else {
        eprintln!("skipped: set {PLAYLIST} to a playlist id to list");
        return;
    };
    let playlist = tool()
        .playlist(&Id::new(&named).expect("the playlist is named by an id"))
        .expect("yt-dlp lists the playlist");

    assert!(!playlist.songs.is_empty());
    assert!(playlist.songs.iter().all(|song| !song.title.is_empty()));
}
