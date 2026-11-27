#![cfg(target_os = "linux")]

#[cfg(feature = "online")]
use std::net::TcpListener;
use std::{
    env,
    ffi::OsStr,
    fs::{self, DirBuilder, OpenOptions},
    io::{Seek as _, SeekFrom, Write as _},
    os::unix::fs::DirBuilderExt as _,
    path::{Path, PathBuf},
    process::Command,
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const UTF8: u8 = 3;
const MP3_HEADER: [u8; 4] = [0xff, 0xfb, 0x90, 0x00];
const MP3_FRAME_LENGTH: usize = 417;
const MP3_FRAMES: usize = 40;
const SOUNDING_STEP: u32 = 7;
const OWNER_ONLY: u32 = 0o700;
const FAILED: i32 = 1;
const REFUSED_BY_THE_GRAMMAR: i32 = 2;
const TITLE: &[u8; 4] = b"TIT2";
const ARTIST: &[u8; 4] = b"TPE1";
const ALBUM: &[u8; 4] = b"TALB";
const OFFLINE: &str = "language = \"en\"\nonline = false\n";
#[cfg(feature = "sync")]
const NEARBY_ONLY: &str = "language = \"en\"\nonline = false\nsync-nearby-only = true\n";
#[cfg(feature = "sync")]
const PAIRING_CODE: &str = "MPJTB-9D5MP-JTB9D-5MPJT-B9D5M-PJTB9-D53QJ";
#[cfg(feature = "online")]
const ONLINE_OFF: &str = "online is off in the settings";
const UNSETTLED_LOCALE: [&str; 6] = [
    "LC_ALL",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "LANG",
    "RESONATE_LOG",
];

struct Home(PathBuf);

impl Home {
    fn new(name: &str, settings: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-cli-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);

        for folder in ["config/resonate", "data", "cache", "state", "music"] {
            fs::create_dir_all(path.join(folder)).expect("a temporary home");
        }
        DirBuilder::new()
            .mode(OWNER_ONLY)
            .create(path.join("run"))
            .expect("a runtime folder");

        let home = Self(path.canonicalize().expect("a home named whole"));

        home.settled(settings);
        home
    }

    fn settled(&self, settings: &str) {
        fs::write(self.settings_file(), settings).expect("the settings written");
    }

    fn settings_file(&self) -> PathBuf {
        self.0.join("config/resonate/config.toml")
    }

    #[cfg(feature = "sync")]
    fn settings(&self) -> String {
        fs::read_to_string(self.settings_file()).expect("the settings read")
    }

    fn music(&self) -> PathBuf {
        self.0.join("music")
    }

    fn data(&self) -> PathBuf {
        self.0.join("data")
    }

    fn vault(&self) -> PathBuf {
        self.data().join("resonate/vault")
    }

    fn holding(&self, within_the_music: &str, bytes: &[u8]) -> PathBuf {
        let path = self.music().join(within_the_music);

        fs::create_dir_all(path.parent().expect("a folder")).expect("the folder made");
        fs::write(&path, bytes).expect("a song written");
        path
    }

    fn ran<I, S>(&self, arguments: I) -> Ran
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(env!("CARGO_BIN_EXE_resonate"));

        for variable in UNSETTLED_LOCALE {
            command.env_remove(variable);
        }

        let output = command
            .args(arguments)
            .current_dir(&self.0)
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_DATA_HOME", self.data())
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env("LANGUAGE", "en")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .output()
            .expect("the binary to run");

        Ran {
            code: output.status.code(),
            out: String::from_utf8_lossy(&output.stdout).into_owned(),
            err: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn scanned(&self) -> Ran {
        self.ran([OsStr::new("scan"), self.music().as_os_str()])
            .succeeded()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

impl Ran {
    #[track_caller]
    fn succeeded(self) -> Self {
        assert_eq!(
            self.code,
            Some(0),
            "the run failed\nstdout:\n{}\nstderr:\n{}",
            self.out,
            self.err
        );
        self
    }

    #[track_caller]
    fn failed(self) -> Self {
        assert_eq!(
            self.code,
            Some(FAILED),
            "the run did not fail as refused\nstdout:\n{}\nstderr:\n{}",
            self.out,
            self.err
        );
        self
    }

    #[track_caller]
    fn says(&self, line: &str) -> &Self {
        assert!(
            self.out.contains(line),
            "{line:?} was not said\nstdout:\n{}\nstderr:\n{}",
            self.out,
            self.err
        );
        self
    }

    #[track_caller]
    fn tells(&self, line: &str) -> &Self {
        assert!(
            self.err.contains(line),
            "{line:?} was not told\nstdout:\n{}\nstderr:\n{}",
            self.out,
            self.err
        );
        self
    }
}

fn id3_tag(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut frames = Vec::new();

    for (id, value) in tags {
        let mut body = vec![UTF8];

        body.extend_from_slice(value.as_bytes());
        frames.extend_from_slice(*id);
        frames.extend_from_slice(&synchsafe(body.len() as u32));
        frames.extend_from_slice(&[0, 0]);
        frames.extend_from_slice(&body);
    }

    let mut tag = b"ID3\x04\x00\x00".to_vec();

    tag.extend_from_slice(&synchsafe(frames.len() as u32));
    tag.extend_from_slice(&frames);
    tag
}

const fn synchsafe(value: u32) -> [u8; 4] {
    [
        (value >> 21) as u8 & 0x7f,
        (value >> 14) as u8 & 0x7f,
        (value >> 7) as u8 & 0x7f,
        value as u8 & 0x7f,
    ]
}

fn wave_holding(mut file: Vec<u8>, data: &[u8]) -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let mut format = Vec::new();

    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());

    let mut body = b"WAVE".to_vec();

    for (id, payload) in [(b"fmt ", format.as_slice()), (b"data", data)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
    }
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn frames_in(tenths: u32) -> u32 {
    RATE / 10 * tenths
}

fn silent_wave(tenths: u32) -> Vec<u8> {
    let bytes = frames_in(tenths) as usize * usize::from(CHANNELS * BITS / 8);

    wave_holding(Vec::new(), &vec![0; bytes])
}

fn sounding_wave(tenths: u32) -> Vec<u8> {
    let data: Vec<u8> = (0..frames_in(tenths) * u32::from(CHANNELS))
        .flat_map(|sample| (sample.wrapping_mul(SOUNDING_STEP) as i16).to_le_bytes())
        .collect();

    wave_holding(Vec::new(), &data)
}

fn tagged_wave(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let bytes = frames_in(2) as usize * usize::from(CHANNELS * BITS / 8);

    wave_holding(id3_tag(tags), &vec![0; bytes])
}

fn tagged_mp3(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut file = id3_tag(tags);

    for _ in 0..MP3_FRAMES {
        file.extend_from_slice(&MP3_HEADER);
        file.resize(file.len() + MP3_FRAME_LENGTH - MP3_HEADER.len(), 0);
    }
    file
}

fn files_under(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found = Vec::new();

    for entry in entries {
        let path = entry.expect("an entry").path();

        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn objects_in(vault: &Path) -> Vec<PathBuf> {
    files_under(&vault.join("audio"))
}

fn damaged_in_the_middle(file: &Path) {
    let length = fs::metadata(file).expect("the object").len();
    let mut object = OpenOptions::new()
        .write(true)
        .open(file)
        .expect("the object opened");

    object
        .seek(SeekFrom::Start(length / 2))
        .expect("the middle reached");
    object.write_all(b"XXXXXXXX").expect("the object damaged");
}

#[cfg(feature = "online")]
fn unreachable_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("its address");

    drop(listener);
    format!("http://{address}")
}

#[test]
fn a_scan_adds_what_a_root_holds_and_refuses_a_root_that_is_not_there() {
    let home = Home::new("scan", OFFLINE);

    home.holding("one.wav", &silent_wave(1));
    home.holding("two.wav", &silent_wave(2));

    let missing = home.ran(["scan", "nowhere"]).failed();
    let first = home.scanned();
    let again = home.scanned();
    let roots = home.ran(["roots"]).succeeded();
    let unopened = home
        .ran([
            OsStr::new("roots"),
            OsStr::new("--library"),
            home.music().as_os_str(),
        ])
        .failed();

    missing.tells("resonate: library root nowhere does not exist");
    first.says("discovered 2 | added 2 | updated 0");
    again.says("discovered 2 | added 0 | updated 0");

    assert_eq!(roots.out.trim(), home.music().display().to_string());

    unopened.tells("unable to open database file");

    assert!(home.data().join("resonate/library.db").is_file());
}

#[test]
fn forgetting_drops_the_tracks_gone_from_a_folder_then_the_root_and_says_when_it_names_nothing() {
    let home = Home::new("forget", OFFLINE);

    let gone = home.holding("retired/a.wav", &silent_wave(1));

    home.holding("kept/b.wav", &silent_wave(1));
    home.scanned();
    fs::remove_file(gone).expect("a track taken away");

    let bare = home.ran(["forget"]);
    let from_the_folder = home
        .ran([
            OsStr::new("forget"),
            home.music().join("retired").as_os_str(),
        ])
        .succeeded();
    let from_nowhere = home
        .ran([OsStr::new("forget"), home.music().join("never").as_os_str()])
        .succeeded();
    let the_root = home
        .ran([OsStr::new("forget"), home.music().as_os_str()])
        .succeeded();
    let roots = home.ran(["roots"]).succeeded();

    assert_eq!(bare.code, Some(REFUSED_BY_THE_GRAMMAR));

    from_the_folder.says("forgot 1 track gone from");
    from_nowhere.says("was neither a library root, a delivered track nor a folder");
    the_root.says(&format!("forgot {}", home.music().display()));

    assert!(roots.out.trim().is_empty(), "a root stayed: {}", roots.out);
}

#[test]
fn tagging_plans_a_favourites_rating_writes_it_with_apply_and_puts_it_back_with_undo() {
    let home = Home::new("tag", OFFLINE);

    let song = home.holding(
        "loose/evening.mp3",
        &tagged_mp3(&[(TITLE, "Evening"), (ARTIST, "Ada"), (ALBUM, "First Light")]),
    );

    fs::create_dir_all(home.music().join("elsewhere")).expect("a folder");
    home.scanned();
    home.ran(["favourites", "--tracks", "--add", "Evening"])
        .succeeded();

    let before = fs::read(&song).expect("the song");

    let unrooted = home
        .ran([
            OsStr::new("tag"),
            OsStr::new("--root"),
            home.music().join("elsewhere").as_os_str(),
        ])
        .failed();
    let planned = home.ran(["tag"]).succeeded();
    let untouched = fs::read(&song).expect("the song");
    let applied = home.ran(["tag", "--apply"]).succeeded();
    let written = fs::read(&song).expect("the song");
    let settled = home.ran(["tag"]).succeeded();
    let undone = home.ran(["tag", "--undo", "--apply"]).succeeded();
    let planned_again = home.ran(["tag"]).succeeded();

    unrooted.tells("is not a library root");
    planned
        .says("favourite · 0 plays")
        .says("would write 1 |")
        .says("nothing was written; pass --apply to do it");

    assert_eq!(untouched, before, "a preview wrote to the file");

    applied.says("written 1 |").says("ratings 1");

    assert_ne!(written, before, "the rating was not written");

    settled.says("nothing to tag");
    undone.says("not a favourite · 0 plays").says("written 1 |");
    planned_again.says("would write 1 |");
}

#[test]
fn organising_previews_moves_with_apply_and_walks_back_with_undo() {
    let home = Home::new("organise", OFFLINE);

    let loose = home.holding(
        "loose/x.wav",
        &tagged_wave(&[(TITLE, "Morning"), (ARTIST, "Ada"), (ALBUM, "First Light")]),
    );
    let filed = home.music().join("Ada/First Light/Morning.wav");

    fs::create_dir_all(home.music().join("elsewhere")).expect("a folder");
    home.scanned();

    let unread_layout = home.ran(["organise", "--as", "{nonsense}"]).failed();
    let unrooted = home
        .ran([
            OsStr::new("organise"),
            OsStr::new("--root"),
            home.music().join("elsewhere").as_os_str(),
        ])
        .failed();
    let previewed = home.ran(["organise"]).succeeded();
    let stayed = loose.is_file();
    let applied = home.ran(["organise", "--apply"]).succeeded();
    let moved = (loose.exists(), filed.is_file());
    let undone = home.ran(["organise", "--undo", "--apply"]).succeeded();

    unread_layout.tells("the naming layout asks for nonsense");
    unrooted.tells("is not a library root");
    previewed
        .says("loose/x.wav  Ada/First Light/Morning.wav")
        .says("would move 1 |")
        .says("nothing was moved; pass --apply to do it");

    assert!(stayed, "a preview moved the file");

    applied.says("moved 1 |");

    assert_eq!(moved, (false, true), "the file was not filed");

    undone.says("moved 1 |");

    assert!(loose.is_file(), "the undo did not put the file back");
    assert!(!filed.exists(), "the undo left the filed copy");
}

#[test]
fn deduplicating_with_a_trash_that_refuses_fails_and_keeps_the_copy_then_folds_it_once_it_takes() {
    let home = Home::new("dedupe", OFFLINE);

    let kept = home.holding("album/song.wav", &silent_wave(2));
    let copy = home.holding("album/song (1).wav", &silent_wave(2));
    let trash = home.data().join("Trash");

    home.scanned();

    let previewed = home.ran(["dedupe"]).succeeded();
    let after_the_preview = copy.is_file();

    fs::write(&trash, b"no folder").expect("a trash that cannot be made");

    let refused = home.ran(["dedupe", "--apply"]).failed();
    let after_the_refusal = copy.is_file();

    fs::remove_file(&trash).expect("the trash freed");

    let folded = home.ran(["dedupe", "--apply"]).succeeded();
    let nothing_left = home.ran(["dedupe"]).succeeded();

    previewed
        .says("copies")
        .says("1 copy to fold; --apply folds them");

    assert!(after_the_preview, "a preview removed the copy");

    refused
        .says("0 copies folded and sent to the trash")
        .says("1 the trash would not take, so it stays")
        .tells("1 files could not be moved to the trash");

    assert!(after_the_refusal, "the copy the trash refused is gone");

    folded.says("1 copy folded and sent to the trash");

    assert!(!copy.exists(), "the copy stayed");
    assert!(kept.is_file(), "the copy kept went too");
    assert!(trash.join("files/song (1).wav").is_file());
    assert!(trash.join("info/song (1).wav.trashinfo").is_file());

    nothing_left.says("0 copies to fold");
}

#[test]
fn the_vault_previews_an_import_keeps_it_with_apply_and_fails_a_verify_of_a_damaged_object() {
    let home = Home::new("vault", OFFLINE);

    home.holding("s.wav", &sounding_wave(2));
    fs::create_dir_all(home.music().join("elsewhere")).expect("a folder");
    home.scanned();

    let empty = home.ran(["vault"]).succeeded();
    let unrooted = home
        .ran([
            OsStr::new("vault"),
            OsStr::new("--import"),
            OsStr::new("--root"),
            home.music().join("elsewhere").as_os_str(),
        ])
        .failed();
    let previewed = home.ran(["vault", "--import"]).succeeded();
    let held_after_the_preview = objects_in(&home.vault());
    let imported = home.ran(["vault", "--import", "--apply"]).succeeded();
    let held = objects_in(&home.vault());
    let standing = home.ran(["vault"]).succeeded();
    let verified = home.ran(["vault", "--verify"]).succeeded();

    empty.says("the vault holds nothing yet");
    unrooted.tells("is not a library root");
    previewed
        .says("would keep 1 |")
        .says("nothing was written; pass --apply to do it");

    assert!(
        held_after_the_preview.is_empty(),
        "a preview kept an object"
    );

    imported.says("s.wav").says("kept 1 |");

    assert_eq!(held.len(), 1, "the vault holds {held:?}");

    standing.says("flac  1");
    verified.says("weighed 1 | held 1 | moved 0");

    damaged_in_the_middle(&held[0]);

    let unverified = home.ran(["vault", "--verify"]).failed();

    unverified
        .says("did not read back as what went in")
        .tells("1 vault objects did not read back as what went in");
}

#[test]
fn a_playlist_is_made_exported_renamed_and_discarded_and_refuses_what_it_cannot_hold() {
    let home = Home::new("playlist", OFFLINE);

    let song = home.holding("one.wav", &silent_wave(2));
    let sheet = home.0.join("mix.m3u8");
    let unnamed = home.0.join("mix.bogus");

    home.scanned();

    let a_folder = home
        .ran([
            OsStr::new("playlist"),
            OsStr::new("Mix"),
            OsStr::new("--add"),
            home.music().as_os_str(),
        ])
        .failed();
    let not_audio = home
        .ran([
            OsStr::new("playlist"),
            OsStr::new("Mix"),
            OsStr::new("--add"),
            home.settings_file().as_os_str(),
        ])
        .failed();
    let added = home
        .ran([
            OsStr::new("playlist"),
            OsStr::new("Mix"),
            OsStr::new("--add"),
            song.as_os_str(),
        ])
        .succeeded();
    let listed = home.ran(["playlists"]).succeeded();
    let refused_format = home
        .ran([
            OsStr::new("playlist"),
            OsStr::new("Mix"),
            OsStr::new("--export"),
            unnamed.as_os_str(),
        ])
        .failed();
    let no_such = home
        .ran([
            OsStr::new("playlist"),
            OsStr::new("Nope"),
            OsStr::new("--export"),
            sheet.as_os_str(),
        ])
        .failed();
    let unwritten = sheet.exists();
    let exported = home
        .ran([
            OsStr::new("playlist"),
            OsStr::new("Mix"),
            OsStr::new("--export"),
            sheet.as_os_str(),
        ])
        .succeeded();
    let renamed = home
        .ran(["playlist", "Mix", "--rename", "Other"])
        .succeeded();
    let listed_renamed = home.ran(["playlists"]).succeeded();
    let discarded = home.ran(["playlist", "Other", "--discard"]).succeeded();
    let listed_at_last = home.ran(["playlists"]).succeeded();

    a_folder.tells("is a folder, and a playlist holds songs");
    not_audio.tells("is not an audio file");
    added.says("added 1 to Mix, which now holds 1");
    listed.says("Mix       a list  1");
    refused_format.tells("does not end in .m3u, .m3u8, .pls or .xspf");

    assert!(!unnamed.exists(), "a refused export wrote a file");

    no_such.tells("no playlist named Nope");

    assert!(!unwritten, "an export of no playlist wrote a file");

    exported.says("wrote 1 row of Mix to").says("as M3U");

    let written = fs::read_to_string(&sheet).expect("the sheet");

    assert!(written.starts_with("#EXTM3U\n#PLAYLIST:Mix\n"), "{written}");
    assert!(written.contains("one.wav"), "{written}");

    renamed.says("Mix is called Other now");
    listed_renamed.says("Other");

    assert!(
        !listed_renamed.out.contains("Mix"),
        "{}",
        listed_renamed.out
    );

    discarded.says("discarded Other and the 1 row it held");

    assert!(
        !listed_at_last.out.contains("Other"),
        "{}",
        listed_at_last.out
    );
}

#[test]
fn importing_a_sheet_makes_its_playlist_and_refuses_a_missing_file_or_an_upkeep_only_a_link_takes()
{
    let home = Home::new("import", OFFLINE);

    let song = home.holding("one.wav", &silent_wave(2));
    let sheet = home.0.join("mix.m3u8");

    home.scanned();
    fs::write(&sheet, format!("#EXTM3U\n{}\n", song.display())).expect("a sheet");

    let missing = home
        .ran([OsStr::new("import"), home.0.join("none.m3u").as_os_str()])
        .failed();
    let in_step = home
        .ran([
            OsStr::new("import"),
            sheet.as_os_str(),
            OsStr::new("--upkeep"),
            OsStr::new("in-step"),
        ])
        .failed();
    let two_named_once = home
        .ran([
            OsStr::new("import"),
            sheet.as_os_str(),
            sheet.as_os_str(),
            OsStr::new("--as"),
            OsStr::new("Two"),
        ])
        .failed();
    let imported = home
        .ran([
            OsStr::new("import"),
            sheet.as_os_str(),
            OsStr::new("--as"),
            OsStr::new("Back"),
        ])
        .succeeded();
    let listed = home.ran(["playlists"]).succeeded();

    missing.tells("cannot read");
    in_step.tells("only a playlist read from a link is kept in step");
    two_named_once.tells("--as names one playlist, and 2 files were given");
    imported.says("Back: read M3U as UTF-8, added 1, which now holds 1");
    listed.says("Back");

    assert!(!listed.out.contains("Two"), "{}", listed.out);
}

#[test]
fn favourites_are_marked_by_name_and_a_name_or_kind_not_given_is_refused() {
    let home = Home::new("favourites", OFFLINE);

    home.holding(
        "morning.wav",
        &tagged_wave(&[(TITLE, "Morning"), (ARTIST, "Ada"), (ALBUM, "First Light")]),
    );
    home.scanned();

    let no_kind = home.ran(["favourites", "--add", "Morning"]).failed();
    let nobody = home
        .ran(["favourites", "--tracks", "--add", "Nothing"])
        .failed();
    let added = home
        .ran(["favourites", "--tracks", "--add", "Morning"])
        .succeeded();
    let again = home
        .ran(["favourites", "--tracks", "--add", "morning"])
        .succeeded();
    let listed = home.ran(["favourites"]).succeeded();
    let removed = home
        .ran(["favourites", "--tracks", "--remove", "Morning"])
        .succeeded();
    let emptied = home.ran(["favourites"]).succeeded();

    no_kind.tells("--add and --remove take one of --tracks, --albums or --artists");
    nobody.tells("the catalog holds no track named Nothing");
    added.says("favourited Morning by Ada");
    again.says("Morning by Ada is a favourite already");
    listed.says("Morning  Ada");
    removed.says("took Morning by Ada off the favourites");

    assert!(!emptied.out.contains("Morning"), "{}", emptied.out);
}

#[test]
fn the_history_is_brought_in_from_a_file_and_exported_whole_and_a_bad_line_or_target_is_refused() {
    let home = Home::new("history", OFFLINE);

    home.holding(
        "evening.wav",
        &tagged_wave(&[(TITLE, "Evening"), (ARTIST, "Ada"), (ALBUM, "First Light")]),
    );
    home.scanned();

    let listens = home.0.join("listens.jsonl");
    let unread = home.0.join("unread.jsonl");
    let exported_to = home.0.join("out.jsonl");
    let nowhere = home.0.join("nowhere/out.jsonl");

    fs::write(
        &listens,
        "{\"listened_at\":1700000000,\"track_metadata\":{\"artist_name\":\"Ada\",\"track_name\":\"Evening\",\"release_name\":\"First Light\"}}\n",
    )
    .expect("a listen");
    fs::write(&unread, "garbage\n").expect("a line that is no listen");

    let refused_line = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--import-file"),
            unread.as_os_str(),
        ])
        .failed();
    let brought = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--import-file"),
            listens.as_os_str(),
        ])
        .succeeded();
    let brought_again = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--import-file"),
            listens.as_os_str(),
        ])
        .succeeded();
    let unwritten = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--export"),
            nowhere.as_os_str(),
        ])
        .failed();
    let exported = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--export"),
            exported_to.as_os_str(),
        ])
        .succeeded();

    refused_line.tells("line 1 of the listens is refused");
    brought.says(&format!("brought in 1 from {}:", listens.display()));
    brought_again.says(&format!("brought in 0 from {}:", listens.display()));
    unwritten.tells("cannot write the listening history");

    assert!(!nowhere.exists());

    exported.says(&format!("wrote one listen to {}", exported_to.display()));

    let written = fs::read_to_string(&exported_to).expect("the export");

    assert_eq!(written.lines().count(), 1, "{written}");
    assert!(
        written.starts_with("{\"listened_at\":1700000000,"),
        "{written}"
    );
    assert!(written.contains("\"track_name\":\"Evening\""), "{written}");
}

#[test]
fn a_history_import_says_one_held_listen_and_one_unmatched_in_the_singular_and_more_in_the_plural()
{
    let home = Home::new("history-singular", OFFLINE);

    home.holding(
        "evening.wav",
        &tagged_wave(&[(TITLE, "Evening"), (ARTIST, "Ada"), (ALBUM, "First Light")]),
    );
    home.scanned();

    let once = home.0.join("once.jsonl");
    let twice = home.0.join("twice.jsonl");

    let evening = |second: u64| {
        format!(
            "{{\"listened_at\":{second},\"track_metadata\":{{\"artist_name\":\"Ada\",\"track_name\":\"Evening\",\"release_name\":\"First Light\"}}}}\n"
        )
    };
    let elsewhere = |second: u64| {
        format!(
            "{{\"listened_at\":{second},\"track_metadata\":{{\"artist_name\":\"Bo\",\"track_name\":\"Nowhere\",\"release_name\":\"Lost\"}}}}\n"
        )
    };

    fs::write(
        &once,
        [evening(1_700_000_000), elsewhere(1_700_000_100)].concat(),
    )
    .expect("two listens");
    fs::write(
        &twice,
        [
            evening(1_700_000_000),
            evening(1_700_000_200),
            elsewhere(1_700_000_100),
            elsewhere(1_700_000_300),
        ]
        .concat(),
    )
    .expect("four listens");

    let first = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--import-file"),
            once.as_os_str(),
        ])
        .succeeded();
    let second = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--import-file"),
            once.as_os_str(),
        ])
        .succeeded();
    let more = home
        .ran([
            OsStr::new("history"),
            OsStr::new("--import-file"),
            twice.as_os_str(),
        ])
        .succeeded();

    first.says("0 were held already, and 1 names no song held here");
    second.says(&format!(
        "brought in 0 from {}: 1 was held already, and 1 names no song held here",
        once.display()
    ));
    more.says(&format!(
        "brought in 1 from {}: 1 was held already, and 2 name no song held here",
        twice.display()
    ));

    assert!(!second.out.contains("1 were held"), "{}", second.out);
}

#[cfg(feature = "sync")]
#[test]
fn a_shared_name_is_kept_where_sync_runs_on_the_network_alone_and_sharing_needs_sync_on() {
    let home = Home::new("shared", OFFLINE);

    home.holding("one.wav", &silent_wave(1));
    home.scanned();
    home.ran([
        OsStr::new("playlist"),
        OsStr::new("Mix"),
        OsStr::new("--add"),
        home.music().join("one.wav").as_os_str(),
    ])
    .succeeded();

    let off = home.ran(["shared"]).failed();

    home.settled(NEARBY_ONLY);

    let named = home.ran(["shared", "--name", "Bea"]).succeeded();
    let settings = home.settings();
    let serverless = home.ran(["shared", "--share", "Mix"]).failed();

    off.tells("device sync is off");
    named
        .says("Others will see you as Bea")
        .says("No playlist is shared with anyone");

    assert!(settings.contains("shared-name = \"Bea\""), "{settings}");

    serverless.tells("sharing a playlist needs a server, and this device is set to use none");
}

#[cfg(feature = "sync")]
#[test]
fn a_pairing_code_is_kept_and_let_go_of_and_sync_off_is_said_as_off() {
    let home = Home::new("sync", OFFLINE);

    let off = home.ran(["sync"]).failed();
    let unread = home.ran(["sync", "--join", "ABC"]).failed();
    let joined = home.ran(["sync", "--join", PAIRING_CODE]).succeeded();
    let kept = home.settings();
    let left = home.ran(["sync", "--leave"]).succeeded();
    let let_go = home.settings();

    off.tells("device sync is off");
    unread.tells("the pairing code does not read");
    joined.says(&format!("This device joins the devices of {PAIRING_CODE}"));

    assert!(
        kept.contains(&format!("sync-key = \"{PAIRING_CODE}\"")),
        "{kept}"
    );

    left.says("This device left its devices");

    assert!(!let_go.contains("sync-key"), "{let_go}");
}

#[cfg(feature = "online")]
#[test]
fn what_asks_the_network_is_refused_while_online_is_off() {
    let home = Home::new("offline", OFFLINE);

    let asked: [&[&str]; 10] = [
        &["served"],
        &["subsonic"],
        &["update"],
        &["enrich"],
        &["tidal"],
        &["jellyfin"],
        &["lastfm"],
        &["spotify"],
        &["ytmusic", "check"],
        &["history", "--import", "listenbrainz"],
    ];

    for arguments in asked {
        home.ran(arguments).failed().tells(ONLINE_OFF);
    }
}

#[cfg(feature = "online")]
#[test]
fn a_service_played_in_place_that_cannot_be_reached_fails_the_browse() {
    let home = Home::new("served", OFFLINE);

    home.settled(&format!(
        "language = \"en\"\nonline = true\nsync = false\nsubsonic = \"{}\"\nsubsonic-user = \"listener\"\nsubsonic-password = \"secret\"\n",
        unreachable_address()
    ));

    let unnamed = home.ran(["served", "--service", "jellyfin"]).failed();
    let browsed = home.ran(["served", "--service", "subsonic"]).failed();
    let subsonic = home.ran(["subsonic"]).failed();

    unnamed.tells("jellyfin is not a service the settings set to play in place");
    browsed.tells("Browse failed in the subsonic provider");
    subsonic.tells("Browse failed in the subsonic provider");
}
