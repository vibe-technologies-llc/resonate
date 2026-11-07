use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use resonate_core::{Service, SourceId};
use resonate_library::{
    KeepStopped, KeepSummary, KeptList, KeptSong, Library, PlaylistToKeep, SongToKeep,
};
use resonate_providers::{Id, PlaysInPlace as _, Providers, Song, WhyAway};
use resonate_ytmusic::{Connection, VideoId, YtMusic, playlist_id};

use crate::{Error, Result, cli::YtmusicArgs, config::Config, counted, providers, signals};

const AFTER_THE_SCHEME: &str = "://";
const SHORT_LINKS_ARE_ON: &str = "youtu.be";
const A_SONGS_PATH: &str = "watch";
const A_PLAYLISTS_PATH: &str = "playlist";
const A_SONG_IS_NAMED_BY: &str = "v";
const A_PLAYLIST_IS_NAMED_BY: &str = "list";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keeping {
    Song(String),
    Playlist(String),
    Elsewhere,
}

enum Asked {
    Song(VideoId),
    Playlist(Id),
}

fn named_in<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        .filter(|value| !value.is_empty())
}

pub fn keep_of(link: &str) -> Keeping {
    let link = link.trim();
    if !matches!(
        Service::of_url(link),
        Service::YoutubeMusic | Service::Youtube
    ) {
        return Keeping::Elsewhere;
    }
    let Some((_, reached)) = link.split_once(AFTER_THE_SCHEME) else {
        return Keeping::Elsewhere;
    };
    let reached = reached.split('#').next().unwrap_or_default();
    let (address, query) = reached.split_once('?').unwrap_or((reached, ""));
    let (host, path) = address.split_once('/').unwrap_or((address, ""));
    let path = path.trim_end_matches('/');

    if host.eq_ignore_ascii_case(SHORT_LINKS_ARE_ON) {
        let names_a_video =
            VideoId::read(path).is_some() && ![A_SONGS_PATH, A_PLAYLISTS_PATH].contains(&path);
        return if names_a_video {
            Keeping::Song(path.to_owned())
        } else {
            Keeping::Elsewhere
        };
    }
    match path {
        A_SONGS_PATH => named_in(query, A_SONG_IS_NAMED_BY)
            .map_or(Keeping::Elsewhere, |video| Keeping::Song(video.to_owned())),
        A_PLAYLISTS_PATH => named_in(query, A_PLAYLIST_IS_NAMED_BY)
            .map_or(Keeping::Elsewhere, |list| {
                Keeping::Playlist(list.to_owned())
            }),
        _ => Keeping::Elsewhere,
    }
}

fn asked_of(link: &str) -> Result<Asked> {
    let refused = || Error::NotAYoutubeMusicLink {
        link: link.to_owned(),
    };
    match keep_of(link) {
        Keeping::Song(video) => VideoId::read(&video).map(Asked::Song).ok_or_else(refused),
        Keeping::Playlist(list) => playlist_id(&list).map(Asked::Playlist).ok_or_else(refused),
        Keeping::Elsewhere => Err(refused()),
    }
}

fn account(config: &Config, settings: Option<PathBuf>) -> Result<Arc<YtMusic>> {
    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    providers::youtube_music_named(config, settings).ok_or(Error::YoutubeMusicOff)
}

fn to_keep(source: &SourceId, song: Song) -> SongToKeep {
    SongToKeep {
        source: source.clone(),
        video: song.id.to_string(),
        title: song.title,
        artist: song.artist,
        album: song.album,
        length: song.length,
    }
}

fn queue_the_song(music: &YtMusic, video: &VideoId, library: &Library) -> Result<()> {
    let song = music.song(video)?.ok_or_else(|| Error::NoSuchVideo {
        video: video.as_str().to_owned(),
    })?;
    let title = song.title.clone();
    match library.keep_song(to_keep(music.scheme(), song))? {
        KeptSong::Queued => said!("{title} is queued"),
        KeptSong::AlreadyHeld(_) => said!("{title} is already held, so nothing was queued"),
    }
    Ok(())
}

fn queue_the_playlist(music: &YtMusic, list: &Id, ordered: bool, library: &Library) -> Result<()> {
    let listed = music.mix(list)?;
    let name = listed.mix.name;
    let (songs, untitled): (Vec<_>, Vec<_>) = listed
        .songs
        .into_iter()
        .map(|song| to_keep(music.scheme(), song))
        .partition(|song| !song.title.trim().is_empty());
    let kept = library.keep_playlist(PlaylistToKeep {
        name: name.clone(),
        ordered,
        songs,
    })?;
    said!("{}", list_kept(&name, kept, untitled.len()));
    Ok(())
}

fn list_kept(name: &str, kept: KeptList, left_out: usize) -> String {
    let held = format!(
        "{name}: {} queued, {} already held",
        counted(kept.queued as u64, "song", "songs"),
        counted(kept.held as u64, "song", "songs"),
    );
    match left_out {
        0 => held,
        left_out => format!(
            "{held}, {} left out with no title",
            counted(left_out as u64, "song", "songs")
        ),
    }
}

fn queue(asked: &Asked, music: &YtMusic, ordered: bool, library: &Library) -> Result<()> {
    library.has_a_folder_to_keep_into()?;
    let _leaving = signals::leave_when_told();
    match asked {
        Asked::Song(video) => queue_the_song(music, video, library),
        Asked::Playlist(list) => queue_the_playlist(music, list, ordered, library),
    }
}

fn walked(library: &Library, music: Arc<YtMusic>, stopping: &AtomicBool) -> Result<KeepSummary> {
    let providers = Providers::none().and(music);
    let cancelled = || stopping.load(Ordering::Relaxed);
    library
        .keep_pending(&providers, &cancelled)
        .map_err(|error| match error {
            resonate_library::Error::AlreadyAsking { .. } => Error::KeepWaitsItsTurn,
            error => error.into(),
        })
}

fn walked_until_told(library: &Library, music: Arc<YtMusic>) -> Result<()> {
    let stopping = Arc::new(AtomicBool::new(false));
    let told = Arc::clone(&stopping);
    let _interrupting = signals::cancel_when_told(move || told.store(true, Ordering::Relaxed));
    let summary = walked(library, music, &stopping)?;
    said!(
        "landed {} | gone {} | unkept {}",
        summary.landed,
        summary.gone,
        summary.unkept
    );
    if stopping.load(Ordering::Relaxed) {
        return Err(Error::KeepCancelled);
    }
    match (summary.stopped_by, summary.stopped) {
        (Some(provider), Some(stopped)) => {
            Err(stopped_for(provider, stopped, library.keeps_pending()?))
        }
        _ => Ok(()),
    }
}

fn stopped_for(provider: SourceId, stopped: KeepStopped, left: u64) -> Error {
    let KeepStopped { why, song } = stopped;
    match why {
        WhyAway::SignInLapsed => Error::KeepSignInLapsed { song, left },
        WhyAway::SubscriptionLapsed => Error::KeepPremiumLost { song, left },
        WhyAway::ToolMissing => Error::KeepToolMissing { song, left },
        WhyAway::NotAnswering => Error::KeepStopped {
            provider,
            song,
            left,
        },
    }
}

fn keep(
    config: &Config,
    settings: Option<PathBuf>,
    link: &str,
    library: impl FnOnce() -> Result<Library>,
) -> Result<()> {
    let asked = asked_of(link)?;
    let music = account(config, settings)?;
    let library = library()?;
    queue(&asked, &music, config.keeps_youtubes_order(), &library)?;
    walked_until_told(&library, music)
}

const fn connection_said(connection: Connection) -> &'static str {
    match connection {
        Connection::Premium => "Premium formats available",
        Connection::SignedInWithoutPremium => "signed in, but this account has no Premium formats",
        Connection::NotSignedIn => "not signed in",
        Connection::ToolMissing => "yt-dlp could not be run; it is a separate install",
        Connection::CheckSongGone => {
            "the song this check asks about has left YouTube, so nothing could be told"
        }
        Connection::CouldNotCheck => "YouTube could not be asked just now; try again in a while",
    }
}

fn check(config: &Config, settings: Option<PathBuf>) -> Result<()> {
    let music = account(config, settings)?;
    let _leaving = signals::leave_when_told();
    said!("{}", connection_said(music.check()));
    Ok(())
}

pub fn run(
    config: &Config,
    settings: Option<PathBuf>,
    wanted: &YtmusicArgs,
    library: impl FnOnce() -> Result<Library>,
) -> Result<()> {
    match wanted {
        YtmusicArgs::Keep { url } => keep(config, settings, url, library),
        YtmusicArgs::Check => check(config, settings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_SONG: &str = "https://music.youtube.com/watch?v=abc123";

    fn turned_on() -> Config {
        Config {
            youtube_music: Some(true),
            ..Config::default()
        }
    }

    fn never_opened() -> Result<Library> {
        panic!("the catalog was opened for a keep refused before it")
    }

    #[test]
    fn a_pass_stopped_tells_the_listener_what_to_do_about_it() {
        let provider = SourceId::new("ytmusic").expect("a nameable source");
        let told = |why| {
            let stopped = KeepStopped {
                why,
                song: "Creep".to_owned(),
            };
            stopped_for(provider.clone(), stopped, 3).to_string()
        };

        for (why, words) in [
            (WhyAway::SignInLapsed, "sign in to YouTube again"),
            (WhyAway::SubscriptionLapsed, "lost Premium"),
            (WhyAway::ToolMissing, "install it"),
            (WhyAway::NotAnswering, "stopped answering"),
        ] {
            let said = told(why);
            assert!(said.contains(words), "{said}");
            assert!(said.contains("Creep"), "{said}");
            assert!(said.contains('3'), "{said}");
        }
    }

    #[test]
    fn a_keep_is_read_from_a_song_link_and_a_playlist_link() {
        assert_eq!(keep_of(A_SONG), Keeping::Song("abc123".into()));
        assert_eq!(
            keep_of("https://music.youtube.com/playlist?list=PLx"),
            Keeping::Playlist("PLx".into())
        );
    }

    #[test]
    fn a_song_in_a_list_is_the_song_and_other_spellings_of_youtube_read_alike() {
        for link in [
            "https://music.youtube.com/watch?list=PLx&v=abc123&si=shared",
            "https://www.youtube.com/watch?v=abc123#t=4",
            "https://youtu.be/abc123?si=shared",
            "  https://music.youtube.com/watch?v=abc123  ",
        ] {
            assert_eq!(keep_of(link), Keeping::Song("abc123".into()), "{link}");
        }
    }

    #[test]
    fn a_keep_with_youtube_music_off_says_so_and_asks_nothing() {
        let off = Config {
            yt_dlp: Some("/nowhere/yt-dlp".to_owned()),
            ..Config::default()
        };

        assert!(matches!(
            keep(&off, None, A_SONG, never_opened),
            Err(Error::YoutubeMusicOff)
        ));
    }

    #[test]
    fn a_keep_with_online_off_says_so_and_asks_nothing() {
        let offline = Config {
            online: Some(false),
            ..turned_on()
        };

        assert!(matches!(
            keep(&offline, None, A_SONG, never_opened),
            Err(Error::OnlineOff)
        ));
    }

    #[test]
    fn a_link_to_anywhere_else_is_refused() {
        for link in [
            "https://example.com/watch?v=abc123",
            "https://music.youtube.com.example.com/watch?v=abc123",
            "https://music.youtube.com/channel/UCY2qt3dw2TQJxvBrXwstEPQ",
            "https://music.youtube.com/watch",
            "https://music.youtube.com/watch?v=",
            "music.youtube.com/watch?v=abc123",
            "not a link",
        ] {
            assert_eq!(keep_of(link), Keeping::Elsewhere, "{link}");
            assert!(
                matches!(
                    keep(&turned_on(), None, link, never_opened),
                    Err(Error::NotAYoutubeMusicLink { .. })
                ),
                "{link}"
            );
        }
    }

    #[test]
    fn a_link_naming_what_no_video_is_called_is_refused() {
        assert!(matches!(
            keep(
                &turned_on(),
                None,
                "https://music.youtube.com/watch?v=a%20b",
                never_opened
            ),
            Err(Error::NotAYoutubeMusicLink { .. })
        ));
    }

    #[test]
    fn a_link_naming_what_no_playlist_is_called_is_refused_before_the_catalog_opens() {
        assert!(matches!(
            keep(
                &turned_on(),
                None,
                "https://music.youtube.com/playlist?list=a%20b",
                never_opened
            ),
            Err(Error::NotAYoutubeMusicLink { .. })
        ));
    }

    #[test]
    fn a_short_link_is_a_song_only_where_its_path_reads_as_a_video() {
        for link in [
            "https://youtu.be/playlist?list=PLx",
            "https://youtu.be/watch?v=abc123",
            "https://youtu.be/a%20b",
            "https://youtu.be/a/b",
            "https://youtu.be/",
        ] {
            assert_eq!(keep_of(link), Keeping::Elsewhere, "{link}");
        }
    }

    #[test]
    fn a_list_says_how_many_of_its_songs_were_left_out_for_want_of_a_title() {
        let kept = KeptList { queued: 2, held: 1 };

        assert_eq!(
            list_kept("Road trip", kept, 0),
            "Road trip: 2 songs queued, 1 song already held"
        );
        assert_eq!(
            list_kept("Road trip", kept, 1),
            "Road trip: 2 songs queued, 1 song already held, 1 song left out with no title"
        );
    }

    #[test]
    fn every_standing_of_the_account_is_said_in_words_of_its_own() {
        let said = [
            Connection::Premium,
            Connection::SignedInWithoutPremium,
            Connection::NotSignedIn,
            Connection::ToolMissing,
            Connection::CheckSongGone,
            Connection::CouldNotCheck,
        ]
        .map(connection_said);

        for (at, words) in said.iter().enumerate() {
            assert!(!said[..at].contains(words), "{words}");
        }
    }

    #[cfg(unix)]
    mod with_the_tool {
        use std::{
            env, fs,
            io::ErrorKind,
            os::unix::fs::PermissionsExt as _,
            path::{Path, PathBuf},
            process::{self, Command},
            sync::atomic::AtomicU32,
            thread,
            time::{Duration, Instant},
        };

        use resonate_library::{DeliveryFolder, Layout};
        use resonate_ytmusic::{Cookies, Tool};

        use super::*;

        const EXECUTABLE: u32 = 0o755;
        const FORKS_HOLDING_THE_SCRIPT_OPEN_END_WITHIN: Duration = Duration::from_secs(5);
        const ASKED_IF_IT_RUNS: &str = "--does-it-run";
        const SAMPLES_A_SECOND: u32 = 8_000;
        const THE_TOOL_IS_REACHED_WITHIN: Duration = Duration::from_secs(10);
        const LOOKED_AT_EVERY: Duration = Duration::from_millis(10);

        static MADE: AtomicU32 = AtomicU32::new(0);

        fn runs_once_no_fork_holds_it_open(program: &Path) {
            let began = Instant::now();
            loop {
                match Command::new(program).arg(ASKED_IF_IT_RUNS).status() {
                    Err(error) if error.kind() == ErrorKind::ExecutableFileBusy => {
                        assert!(began.elapsed() < FORKS_HOLDING_THE_SCRIPT_OPEN_END_WITHIN);
                        thread::yield_now();
                    }
                    answered => {
                        answered.expect("the fake tool runs");
                        return;
                    }
                }
            }
        }

        fn a_second_of_silence() -> Vec<u8> {
            let data = SAMPLES_A_SECOND * 2;
            let mut wave = Vec::new();
            wave.extend_from_slice(b"RIFF");
            wave.extend_from_slice(&(36 + data).to_le_bytes());
            wave.extend_from_slice(b"WAVEfmt ");
            wave.extend_from_slice(&16_u32.to_le_bytes());
            wave.extend_from_slice(&1_u16.to_le_bytes());
            wave.extend_from_slice(&1_u16.to_le_bytes());
            wave.extend_from_slice(&SAMPLES_A_SECOND.to_le_bytes());
            wave.extend_from_slice(&(SAMPLES_A_SECOND * 2).to_le_bytes());
            wave.extend_from_slice(&2_u16.to_le_bytes());
            wave.extend_from_slice(&16_u16.to_le_bytes());
            wave.extend_from_slice(b"data");
            wave.extend_from_slice(&data.to_le_bytes());
            wave.resize(wave.len() + data as usize, 0);
            wave
        }

        struct Folder(PathBuf);

        impl Folder {
            fn holding_a_tool(body: &str) -> Self {
                let folder = env::temp_dir().join(format!(
                    "resonate-keep-{}-{}",
                    process::id(),
                    MADE.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir_all(folder.join("music")).expect("a folder");
                let program = folder.join("yt-dlp");
                fs::write(
                    &program,
                    format!(
                        "#!/bin/sh\n[ \"$1\" = {ASKED_IF_IT_RUNS} ] && exit 0\necho \"$@\" >> \"$(dirname \"$0\")/asked\"\n{body}\n"
                    ),
                )
                .expect("the fake tool");
                fs::set_permissions(&program, fs::Permissions::from_mode(EXECUTABLE))
                    .expect("the fake tool made executable");
                runs_once_no_fork_holds_it_open(&program);
                Self(folder)
            }

            fn path(&self) -> &Path {
                &self.0
            }

            fn music(&self) -> Arc<YtMusic> {
                let tool = Tool::new(Some(self.0.join("yt-dlp")), Cookies::None);
                Arc::new(YtMusic::new(tool))
            }

            fn asked(&self) -> Vec<String> {
                fs::read_to_string(self.0.join("asked"))
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect()
            }
        }

        impl Drop for Folder {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn delivering_into(library: &Library, folder: &Folder) {
            library.deliver_into(Some(DeliveryFolder {
                path: folder.path().join("music"),
                layout: Layout::default(),
            }));
        }

        #[test]
        fn a_song_link_is_named_by_the_tool_queued_and_asked_for_by_the_pass() {
            let folder = Folder::holding_a_tool(
                r#"echo '{"track":"Creep","artist":"Radiohead","duration":238,"formats":[]}'"#,
            );
            let library = Library::open_in_memory().expect("a catalog");
            delivering_into(&library, &folder);
            let music = folder.music();
            let asked = asked_of(A_SONG).expect("a song");

            queue(&asked, &music, true, &library).expect("the song queued");
            assert_eq!(library.keeps_pending().expect("a count"), 1);
            assert_eq!(folder.asked().len(), 1);

            let summary =
                walked(&library, music, &AtomicBool::new(false)).expect("the pass answered");

            assert_eq!(summary.unkept, 1);
            assert_eq!(library.keeps_pending().expect("a count"), 0);
            assert_eq!(folder.asked().len(), 2);
        }

        #[test]
        fn a_link_to_a_video_gone_is_said_to_be_gone_and_nothing_is_queued() {
            let folder = Folder::holding_a_tool("echo 'ERROR: Video unavailable' >&2\nexit 1");
            let library = Library::open_in_memory().expect("a catalog");
            delivering_into(&library, &folder);
            let asked = asked_of(A_SONG).expect("a song");

            assert!(matches!(
                queue(&asked, &folder.music(), true, &library),
                Err(Error::NoSuchVideo { .. })
            ));
            assert_eq!(library.keeps_pending().expect("a count"), 0);
        }

        #[test]
        fn a_keep_with_no_music_folder_is_refused_as_the_library_says() {
            let folder = Folder::holding_a_tool(r#"echo '{"track":"Creep","artist":"Radiohead"}'"#);
            let library = Library::open_in_memory().expect("a catalog");
            let asked = asked_of(A_SONG).expect("a song");

            assert!(matches!(
                queue(&asked, &folder.music(), true, &library),
                Err(Error::Library(resonate_library::Error::NoDeliveryFolder))
            ));
            assert!(folder.asked().is_empty(), "yt-dlp ran with nowhere to file");
        }

        #[test]
        fn a_playlist_link_keeps_the_list_under_its_own_title() {
            let folder = Folder::holding_a_tool(
                r#"echo '{"title":"Road trip","entries":[{"id":"one","title":"One","channel":"Ada"},{"id":"blank","title":" "}]}'"#,
            );
            let library = Library::open_in_memory().expect("a catalog");
            delivering_into(&library, &folder);
            let asked = asked_of("https://music.youtube.com/playlist?list=PLx").expect("a list");

            queue(&asked, &folder.music(), true, &library).expect("the list queued");

            assert_eq!(library.keeps_pending().expect("a count"), 1);
        }

        #[test]
        fn a_pass_met_by_another_says_so_and_leaves_the_song_queued() {
            let folder = Folder::holding_a_tool("exec sleep 30");
            let catalog = folder.path().join("library.db");
            let walking = Library::open(&catalog).expect("a catalog");
            let waiting = Library::open(&catalog).expect("the catalog again");
            delivering_into(&walking, &folder);
            delivering_into(&waiting, &folder);
            walking
                .keep_song(SongToKeep {
                    source: SourceId::new("ytmusic").expect("a source"),
                    video: "abc123".to_owned(),
                    title: "Creep".to_owned(),
                    artist: None,
                    album: None,
                    length: None,
                })
                .expect("the song queued");
            let stopping = Arc::new(AtomicBool::new(false));

            thread::scope(|scope| {
                let told = Arc::clone(&stopping);
                let music = folder.music();
                let first = scope.spawn(move || walked(&walking, music, &told));
                let began = Instant::now();
                while folder.asked().is_empty() && began.elapsed() < THE_TOOL_IS_REACHED_WITHIN {
                    thread::sleep(LOOKED_AT_EVERY);
                }

                let second = walked(&waiting, folder.music(), &AtomicBool::new(false));

                stopping.store(true, Ordering::Relaxed);
                assert!(matches!(second, Err(Error::KeepWaitsItsTurn)));
                first.join().expect("the first pass").expect("its summary");
            });
            assert_eq!(waiting.keeps_pending().expect("a count"), 1);
        }

        #[test]
        fn a_video_yt_dlp_refuses_for_its_own_reason_gives_the_keep_up_and_the_pass_goes_on() {
            let folder = Folder::holding_a_tool(
                r#"case "$*" in
    *v=refused*) echo 'ERROR: [youtube] refused: Sign in to confirm your age' >&2; exit 1 ;;
    *-J*) echo '{"formats":[{"format_id":"251"}]}' ;;
    *) cat "$(dirname "$0")/song" ;;
esac"#,
            );
            fs::write(folder.path().join("song"), a_second_of_silence()).expect("the song");
            let library = Library::open_in_memory().expect("a catalog");
            library.deliver_into(Some(DeliveryFolder {
                path: folder.path().join("music"),
                layout: Layout::default(),
            }));
            for video in ["refused", "willing"] {
                let song = SongToKeep {
                    source: SourceId::new("ytmusic").expect("a nameable source"),
                    video: video.to_owned(),
                    title: video.to_owned(),
                    artist: Some("Someone".to_owned()),
                    album: None,
                    length: None,
                };
                library.keep_song(song).expect("queued");
            }

            let summary = library
                .keep_pending(&Providers::none().and(folder.music()), &|| false)
                .expect("a pass");

            assert_eq!(summary.stopped_by, None);
            assert_eq!(summary.landed, 1);
            assert_eq!(summary.unkept, 1);
            assert_eq!(library.keeps_pending().expect("a count"), 0);
        }
    }
}
