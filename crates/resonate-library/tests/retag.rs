#[cfg(all(unix, not(target_os = "macos")))]
use std::ffi::OsStr;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, TrackId};
use resonate_library::{
    AlbumQuery, CoverArt, Error, Favoured, FileTags, ImageFormat, Library, LookupOp, LyricText,
    Picturing, Popularity, Rated, Result, RetagOptions, RetagSummary, ScanOptions, SheetEdit,
    SheetField, SheetScope, TagEdit, TagField, TagSet, TagSink, TagSource, TrackQuery, Unwritten,
};

use crate::support::{
    ALBUM, ALBUM_ARTIST, ANSWERED, ARTIST, ASKED, CHANNELS, Canned, FRONT_COVER, Fake,
    MUSICBRAINZ_ALBUM, ORBITS_TITLES, RATE, RELEASE, RELEASE_GROUP, TITLE, TRACK, Tree, USER_TEXT,
    UTF8, Wav, allow_writing, answer_track, applied, beside, can_write, enrich, ffmpeg, fields_of,
    forbid_writing, frame, landings, mbid, meddle, moved, only_album, options, orbits,
    orbits_match, orbits_rows, png_art, retagged, scan, scanned_lone, settled, stored, synchsafe,
    words,
};

const MEDDLE_AIFF_SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.aiff" AIFF
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Echoes"
    INDEX 01 00:00:20
"#;

struct Aiff {
    frames: u32,
    id3: Vec<u8>,
}

impl Aiff {
    fn new() -> Self {
        Self {
            frames: 4_410,
            id3: Vec::new(),
        }
    }

    fn framed(mut self, id: &[u8; 4], body: &[u8]) -> Self {
        frame(&mut self.id3, id, body);
        self
    }

    fn frames(mut self, frames: u32) -> Self {
        self.frames = frames;
        self
    }

    fn text(mut self, id: &[u8; 4], value: &str) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, id, &body);
        self
    }

    fn described(mut self, description: &str, value: &str) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(description.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, USER_TEXT, &body);
        self
    }

    fn picture(mut self, bytes: &[u8]) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(b"image/png\0");
        body.push(FRONT_COVER);
        body.push(0);
        body.extend_from_slice(bytes);
        frame(&mut self.id3, b"APIC", &body);
        self
    }

    fn build(&self) -> Vec<u8> {
        let bits = 16_u16;
        let mut common = Vec::new();

        common.extend_from_slice(&CHANNELS.to_be_bytes());
        common.extend_from_slice(&self.frames.to_be_bytes());
        common.extend_from_slice(&bits.to_be_bytes());

        let leading = RATE.leading_zeros();
        let exponent = (16_383 + 31 - leading) as u16;

        common.extend_from_slice(&exponent.to_be_bytes());
        common.extend_from_slice(&(u64::from(RATE) << (leading + 32)).to_be_bytes());

        let stride = usize::from(CHANNELS) * usize::from(bits / 8);
        let mut sound = vec![0_u8; 8];

        sound.resize(sound.len() + self.frames as usize * stride, 0);

        let mut body = b"AIFF".to_vec();

        aiff_chunk(&mut body, b"COMM", &common);
        aiff_chunk(&mut body, b"SSND", &sound);
        if !self.id3.is_empty() {
            let mut tag = b"ID3\x04\x00\x00".to_vec();

            tag.extend_from_slice(&synchsafe(self.id3.len() as u32));
            tag.extend_from_slice(&self.id3);
            aiff_chunk(&mut body, b"ID3 ", &tag);
        }

        let mut file = b"FORM".to_vec();

        file.extend_from_slice(&(body.len() as u32).to_be_bytes());
        file.extend_from_slice(&body);
        file
    }
}

fn aiff_chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn tags_of(file: &Path) -> TagSet {
    FileTags::default()
        .read(&MediaLocation::local(file), Picturing::Whether)
        .expect("a file whose tags read back")
        .tags
}

fn cover_of(file: &Path) -> Option<CoverArt> {
    FileTags::default()
        .read(&MediaLocation::local(file), Picturing::Copied)
        .expect("a file whose picture reads back")
        .picture
        .into_copied()
}

fn covered(library: &Library, art: &CoverArt) -> Result<()> {
    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("one album was grouped");

    assert!(
        library.land_archive_cover(album.id, art)?,
        "the catalog took no cover to write back"
    );
    Ok(())
}

fn why_passed_over(summary: &RetagSummary) -> Vec<Unwritten> {
    summary
        .retagging
        .passed_over
        .iter()
        .map(|over| over.why)
        .collect()
}

#[test]
fn a_track_number_read_off_the_file_name_is_never_written_into_the_file() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "03 - Echos.aiff",
        &Aiff::new().text(ARTIST, "The Orbiters").build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    assert_eq!(
        library
            .track_at(&file, None)?
            .expect("the scanned row")
            .track_number,
        Some(3),
        "the file name numbered nothing"
    );

    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let preview = retagged(&library, false)?;

    for write in &preview.retagging.writes {
        assert!(
            !fields_of(write).contains(&TagField::TrackNumber),
            "a number guessed from the file name was offered"
        );
    }
    Ok(())
}

#[test]
fn only_what_a_lookup_answered_for_is_written_into_the_file() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let preview = retagged(&library, false)?;

    assert_eq!(preview.retagging.writes.len(), 1);
    assert_eq!(
        fields_of(&preview.retagging.writes[0]),
        vec![TagField::Title]
    );
    assert_eq!(preview.stats.written, 0, "a preview wrote something");
    assert_eq!(
        tags_of(&file).title.as_deref(),
        Some("Echos"),
        "a preview changed the file"
    );

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.written, 1);
    assert_eq!(applied.stats.fields, 1);
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));
    assert_eq!(
        tags_of(&file).artist.as_deref(),
        Some("The Orbiters"),
        "a name the file already carried was written again"
    );

    let again = retagged(&library, false)?;

    assert!(
        again.retagging.writes.is_empty(),
        "the write did not stick: {:?}",
        again.retagging.writes
    );
    assert_eq!(again.stats.unchanged, 1);
    Ok(())
}

const LEFT_BY_A_DEAD_WRITER: &str = "01 Echoes.aiff.resonate-staging";

struct TwoRoots {
    one: Tree,
    other: Tree,
    database: PathBuf,
    library: Library,
    in_one: PathBuf,
    in_other: PathBuf,
}

impl TwoRoots {
    fn answered() -> Result<Self> {
        let one = Tree::new();
        let other = Tree::new();
        let misspelt = |title: &str| {
            Aiff::new()
                .text(TITLE, title)
                .text(ARTIST, "The Orbiters")
                .build()
        };

        let in_one = one.write("1.aiff", &misspelt("Echos"));
        let in_other = other.write("2.aiff", &misspelt("Fearles"));
        let database = one.path().join("library.db");
        let library = Library::open(&database)?;

        scan(
            &library,
            &ScanOptions {
                roots: vec![one.path().to_path_buf(), other.path().to_path_buf()],
                ..options(&one)
            },
        )?;
        answer_track(&database, &in_one, "Echoes", "The Orbiters", "Orbits");
        answer_track(&database, &in_other, "Fearless", "The Orbiters", "Orbits");
        Ok(Self {
            one,
            other,
            database,
            library,
            in_one,
            in_other,
        })
    }

    fn retagged_under(&self, roots: Vec<PathBuf>, apply: bool) -> Result<RetagSummary> {
        self.library
            .retag(
                Arc::new(FileTags::default()),
                RetagOptions {
                    roots,
                    apply,
                    undo: false,
                    lyrics: Vec::new(),
                },
            )?
            .join()
    }

    fn bytes(&self) -> [Vec<u8>; 2] {
        [&self.in_one, &self.in_other].map(|file| fs::read(file).expect("a file to read"))
    }

    fn stamps(&self) -> [SystemTime; 2] {
        [&self.in_one, &self.in_other].map(|file| {
            fs::metadata(file)
                .and_then(|held| held.modified())
                .expect("a file's time")
        })
    }
}

#[test]
fn a_retag_naming_one_root_writes_into_that_root_and_leaves_the_other_byte_for_byte() -> Result<()>
{
    let roots = TwoRoots::answered()?;
    let before = roots.bytes();
    let stamped = roots.stamps();

    let named = roots.retagged_under(vec![roots.one.path().to_path_buf()], true)?;

    assert_eq!(named.stats.written, 1);
    assert_eq!(named.retagging.writes.len(), 1);
    assert_eq!(named.retagging.writes[0].path, roots.in_one);
    assert_eq!(tags_of(&roots.in_one).title.as_deref(), Some("Echoes"));
    assert_eq!(
        roots.bytes()[1],
        before[1],
        "a root the run was not given was written into"
    );
    assert_eq!(roots.stamps()[1], stamped[1]);
    assert_ne!(roots.bytes()[0], before[0]);

    let rest = roots.retagged_under(vec![roots.other.path().to_path_buf()], true)?;

    assert_eq!(rest.stats.written, 1);
    assert_eq!(tags_of(&roots.in_other).title.as_deref(), Some("Fearless"));
    Ok(())
}

#[test]
fn a_retag_naming_a_root_the_catalog_does_not_hold_is_refused_and_writes_nothing() -> Result<()> {
    let roots = TwoRoots::answered()?;
    let before = roots.bytes();
    let elsewhere = env::temp_dir();

    let refused = roots.retagged_under(
        vec![roots.one.path().to_path_buf(), elsewhere.clone()],
        true,
    );

    assert!(
        matches!(refused, Err(Error::NotARoot { ref path }) if *path == elsewhere),
        "a root the catalog does not hold was not refused: {refused:?}"
    );
    assert_eq!(roots.bytes(), before, "a refused run wrote a file");
    assert!(
        !roots.library.retag_walks_back()?,
        "a refused run noted something to put back"
    );
    Ok(())
}

#[test]
fn a_retag_preview_leaves_every_file_and_staging_file_alone_and_only_an_apply_sweeps_the_stage()
-> Result<()> {
    let roots = TwoRoots::answered()?;
    let staged = roots.one.write_stamped(
        LEFT_BY_A_DEAD_WRITER,
        b"half a file",
        Duration::from_secs(1),
    );

    beside(&roots.database)
        .execute(
            "INSERT INTO staged_writes (path, pid) VALUES (?1, ?2)",
            rusqlite::params![
                staged.to_str().expect("a path the catalog can hold"),
                u32::MAX
            ],
        )
        .expect("the stage is noted as a run's that is gone");

    let before = roots.bytes();
    let stamped = roots.stamps();
    let noted_stages = || -> i64 {
        beside(&roots.database)
            .query_row("SELECT count(*) FROM staged_writes", [], |row| row.get(0))
            .expect("the stages are counted")
    };

    for named in [
        Vec::new(),
        vec![roots.one.path().to_path_buf()],
        vec![roots.other.path().to_path_buf()],
    ] {
        let preview = roots.retagged_under(named, false)?;

        assert_eq!(preview.stats.written, 0, "a preview wrote something");
        assert!(!preview.retagging.writes.is_empty());
        assert_eq!(roots.bytes(), before, "a preview changed a file");
        assert_eq!(roots.stamps(), stamped);
        assert!(staged.exists(), "a preview swept a staging file");
        assert_eq!(noted_stages(), 1, "a preview forgot a staging file");
        assert!(
            !roots.library.retag_walks_back()?,
            "a preview noted something to put back"
        );
    }

    let applied = roots.retagged_under(Vec::new(), true)?;

    assert_eq!(applied.stats.written, 2);
    assert!(
        !staged.exists(),
        "an apply left a dead writer's staging file"
    );
    assert_eq!(noted_stages(), 0);
    Ok(())
}

fn walked_back(library: &Library, apply: bool) -> Result<RetagSummary> {
    let summary = library
        .retag(
            Arc::new(FileTags::default()),
            RetagOptions {
                roots: Vec::new(),
                apply,
                undo: true,
                lyrics: Vec::new(),
            },
        )?
        .join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

fn lyrics_tagged(library: &Library, tracks: Vec<TrackId>, apply: bool) -> Result<RetagSummary> {
    let summary = library
        .retag(
            Arc::new(FileTags::default()),
            RetagOptions {
                apply,
                lyrics: tracks,
                ..RetagOptions::default()
            },
        )?
        .join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

#[test]
fn fetched_lyrics_go_into_a_songs_tags_only_when_asked_and_the_walk_back_takes_them_out()
-> Result<()> {
    const SHEET: &str =
        "[00:01.00]Overhead the albatross\n[00:05.00]Hangs motionless upon the air\n";

    let tree = Tree::new();
    let sung = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    let unsung = tree.write(
        "2.aiff",
        &Aiff::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    library.keep_lyrics(
        &MediaLocation::local(&sung),
        None,
        Some(&LyricText {
            text: SHEET.to_owned(),
            synced: true,
            lyricsfile: None,
        }),
    )?;

    let asked = [&sung, &unsung]
        .into_iter()
        .map(|file| Ok(library.track_at(file, None)?.expect("a scanned song").id))
        .collect::<Result<Vec<_>>>()?;

    let everything = retagged(&library, true)?;
    let planned = lyrics_tagged(&library, asked.clone(), false)?;
    let untouched = tags_of(&sung).lyrics;
    let written = lyrics_tagged(&library, asked.clone(), true)?;
    let tagged = tags_of(&sung).lyrics;
    let again = lyrics_tagged(&library, asked, true)?;
    let put_back = walked_back(&library, true)?;

    assert!(
        everything
            .retagging
            .writes
            .iter()
            .flat_map(|write| &write.edits)
            .all(|edit| edit.field != TagField::Lyrics),
        "a run nobody asked lyrics of wrote them"
    );
    assert_eq!(planned.retagging.writes.len(), 1);
    assert_eq!(
        planned.retagging.writes[0].edits,
        [TagEdit {
            field: TagField::Lyrics,
            value: SHEET.trim_end().to_owned(),
        }]
    );
    assert_eq!(
        planned
            .retagging
            .passed_over
            .iter()
            .map(|over| (over.path.clone(), over.why))
            .collect::<Vec<_>>(),
        [(unsung.clone(), Unwritten::NoLyricsKept)]
    );
    assert_eq!(untouched, None);
    assert_eq!(written.stats.written, 1);
    assert_eq!(tagged.as_deref(), Some(SHEET.trim_end()));
    assert_eq!(again.stats.written, 0);
    assert_eq!(again.stats.unchanged, 1);
    assert_eq!(put_back.stats.written, 1);
    assert_eq!(tags_of(&sung).lyrics, None);
    assert_eq!(tags_of(&sung).title.as_deref(), Some("Echoes"));
    Ok(())
}

const TIMED_LYRICS: &[u8; 4] = b"SYLT";

const PRIVATE: &[u8; 4] = b"PRIV";

const HELD_PRIVATELY: &[u8] = b"another tagger\0its own bytes";

const UNTIMED_LYRICS: &[u8; 4] = b"USLT";

const STAMPED_IN_MILLISECONDS: u8 = 2;

const CONTENT_OF_LYRICS: u8 = 1;

fn timed_lyrics(lines: &[(&str, u32)]) -> Vec<u8> {
    let mut body = vec![UTF8];

    body.extend_from_slice(b"eng");
    body.extend_from_slice(&[STAMPED_IN_MILLISECONDS, CONTENT_OF_LYRICS, 0]);
    for (text, at) in lines {
        body.extend_from_slice(text.as_bytes());
        body.push(0);
        body.extend_from_slice(&at.to_be_bytes());
    }
    body
}

fn untimed_lyrics(text: &str) -> Vec<u8> {
    let mut body = vec![UTF8];

    body.extend_from_slice(b"eng");
    body.push(0);
    body.extend_from_slice(text.as_bytes());
    body
}

fn id3_frames_of(file: &Path) -> Vec<([u8; 4], Vec<u8>)> {
    let bytes = fs::read(file).expect("a file to read");
    let at = bytes
        .windows(4)
        .position(|window| window == b"ID3 ")
        .expect("an ID3 chunk");

    let tag = &bytes[at + 8..];
    let synchsafe = |held: &[u8]| {
        held[..4].iter().fold(0_usize, |size, byte| {
            (size << 7) | usize::from(*byte & 0x7f)
        })
    };

    let whole = synchsafe(&tag[6..10]);
    let mut frames = Vec::new();
    let mut rest = &tag[10..10 + whole];

    while rest.len() >= 10 && rest[0] != 0 {
        let id: [u8; 4] = rest[..4].try_into().expect("a frame id");
        let size = synchsafe(&rest[4..8]);

        frames.push((id, rest[10..10 + size].to_vec()));
        rest = &rest[10 + size..];
    }
    frames
}

fn lyric_frames_of(file: &Path) -> Vec<([u8; 4], Vec<u8>)> {
    id3_frames_of(file)
        .into_iter()
        .filter(|(id, _)| id == TIMED_LYRICS || id == UNTIMED_LYRICS)
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn the_walk_back_puts_back_the_tags_of_a_file_whose_name_is_not_text() -> Result<()> {
    const SHEET: &str = "[00:01.00]Overhead the albatross\n";

    let tree = Tree::new();
    let sung = tree
        .path()
        .join(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(
            b"caf\xe9.aiff",
        ));

    fs::write(
        &sung,
        Aiff::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .build(),
    )
    .expect("a file with a name that is not text");

    let library = Library::open(&tree.path().join("library.db"))?;

    scan(&library, &options(&tree))?;
    library.keep_lyrics(
        &MediaLocation::local(&sung),
        None,
        Some(&LyricText {
            text: SHEET.to_owned(),
            synced: true,
            lyricsfile: None,
        }),
    )?;

    let asked = vec![library.track_at(&sung, None)?.expect("a scanned song").id];

    let written = lyrics_tagged(&library, asked, true)?;
    let tagged = tags_of(&sung).lyrics;
    let put_back = walked_back(&library, true)?;

    assert_eq!(written.stats.written, 1);
    assert_eq!(tagged.as_deref(), Some(SHEET.trim_end()));
    assert!(
        put_back.retagging.passed_over.is_empty(),
        "the walk back passed over {:?}",
        why_passed_over(&put_back)
    );
    assert_eq!(put_back.stats.written, 1);
    assert_eq!(tags_of(&sung).lyrics, None);
    Ok(())
}

#[test]
fn lyrics_written_over_a_timed_frame_read_back_and_the_walk_back_puts_the_timed_frame_back()
-> Result<()> {
    const FETCHED: &str = "[00:02.00]Strangers passing in the street\n";
    const TIMED_AS: &str = "[00:01.000]Overhead the albatross\n[00:05.000]Hangs motionless\n";

    let tree = Tree::new();
    let sung = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .framed(
                TIMED_LYRICS,
                &timed_lyrics(&[
                    ("Overhead the albatross", 1_000),
                    ("Hangs motionless", 5_000),
                ]),
            )
            .framed(
                UNTIMED_LYRICS,
                &untimed_lyrics("Overhead the albatross\nHangs motionless"),
            )
            .framed(PRIVATE, HELD_PRIVATELY)
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    library.keep_lyrics(
        &MediaLocation::local(&sung),
        None,
        Some(&LyricText {
            text: FETCHED.to_owned(),
            synced: true,
            lyricsfile: None,
        }),
    )?;

    let asked = vec![library.track_at(&sung, None)?.expect("a scanned song").id];
    let before = tags_of(&sung).lyrics;
    let framed_before = lyric_frames_of(&sung);

    let written = lyrics_tagged(&library, asked, true)?;
    let tagged = tags_of(&sung).lyrics;
    let framed_written = lyric_frames_of(&sung);
    let put_back = walked_back(&library, true)?;
    let framed_put_back = lyric_frames_of(&sung);

    assert_eq!(before.as_deref(), Some(TIMED_AS));
    assert_eq!(written.stats.written, 1);
    assert_eq!(tagged.as_deref(), Some(FETCHED.trim_end()));
    assert!(
        framed_written.iter().all(|(id, _)| id != TIMED_LYRICS),
        "the timed frame the read prefers stayed beside the words written"
    );
    assert_eq!(put_back.stats.written, 1);
    assert_eq!(tags_of(&sung).lyrics.as_deref(), Some(TIMED_AS));
    assert_eq!(framed_put_back, framed_before);
    assert!(
        id3_frames_of(&sung)
            .iter()
            .any(|(id, body)| id == PRIVATE && body == HELD_PRIVATELY),
        "a frame nobody asked to change was lost"
    );
    assert_eq!(tags_of(&sung).title.as_deref(), Some("Echoes"));
    Ok(())
}

#[test]
fn walking_a_tag_run_back_leaves_a_field_another_tagger_changed_since() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");
    retagged(&library, true)?;

    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));

    fs::write(
        &file,
        Aiff::new()
            .text(TITLE, "Echoes (Live)")
            .text(ARTIST, "The Orbiters")
            .build(),
    )
    .expect("another tagger rewrote the file");
    walked_back(&library, true)?;

    assert_eq!(
        tags_of(&file).title.as_deref(),
        Some("Echoes (Live)"),
        "the walk back wrote over a title edited since the run"
    );
    Ok(())
}

#[test]
fn an_applied_tag_run_is_put_back_field_for_field_and_putting_it_back_again_writes_it_again()
-> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let location = MediaLocation::local(&file);
    let track = library
        .track_at(&file, None)?
        .expect("the scanned track")
        .id;

    library.favour(Favoured::Track(track), true)?;

    assert!(
        !library.retag_walks_back()?,
        "a run was kept before any applied"
    );

    retagged(&library, true)?;

    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));
    assert!(library.retag_walks_back()?);

    let preview = walked_back(&library, false)?;

    assert_eq!(preview.retagging.writes.len(), 1);
    assert_eq!(preview.stats.written, 0);
    assert_eq!(
        tags_of(&file).title.as_deref(),
        Some("Echoes"),
        "a preview wrote"
    );

    let undone = walked_back(&library, true)?;

    assert_eq!(undone.stats.written, 1);
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echos"));
    assert_eq!(tags_of(&file).artist.as_deref(), Some("The Orbiters"));
    assert_eq!(
        FileTags::default()
            .rated(&location)
            .expect("a rating that reads back"),
        Rated::Unrated { plays: Some(0) }
    );
    assert_eq!(
        stored(&database, &file).tagged_title.as_deref(),
        Some("Echos"),
        "the catalog did not follow the file put back"
    );

    walked_back(&library, true)?;

    assert_eq!(
        tags_of(&file).title.as_deref(),
        Some("Echoes"),
        "putting the walk back did not write the run again"
    );
    assert_eq!(
        FileTags::default()
            .rated(&location)
            .expect("a rating that reads back"),
        Rated::Favourite { plays: Some(0) }
    );
    Ok(())
}

#[test]
fn a_tag_run_keeps_the_study_of_every_file_it_wrote() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let location = MediaLocation::local(&file);
    let studied = library
        .study_of(&location, None)?
        .expect("the enrichment studied the file");

    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.written, 1);

    assert_eq!(
        library.study_of(&location, None)?,
        Some(studied),
        "writing the tags threw away a study of audio it did not touch"
    );
    Ok(())
}

#[test]
fn a_tag_run_keeps_the_words_kept_for_every_file_it_wrote_and_asks_again_after_a_new_name()
-> Result<()> {
    let tree = Tree::new();
    let written = |name: &str| {
        tree.write(
            name,
            &Aiff::new()
                .text(TITLE, "Echos")
                .text(ARTIST, "The Orbiters")
                .build(),
        )
    };
    let found = written("found/1.aiff");
    let missed = written("missed/2.aiff");
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let found_at = MediaLocation::local(&found);
    let missed_at = MediaLocation::local(&missed);

    library.keep_lyrics(&found_at, None, Some(&words("Overhead the albatross")))?;
    library.keep_lyrics(&missed_at, None, None)?;
    beside(&database)
        .execute(
            "INSERT INTO lyrics_refused (path, span_start, refused, refusals) VALUES (?1, 0, 1, 1)",
            [found.to_str().expect("a path")],
        )
        .expect("a refusal is written");

    answer_track(&database, &found, "Echoes", "The Orbiters", "Orbits");
    answer_track(&database, &missed, "Echoes", "The Orbiters", "Orbits");

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.written, 2);

    assert_eq!(
        library
            .kept_lyrics(&found_at, None)?
            .and_then(|kept| kept.sung)
            .map(|sung| sung.text),
        Some("Overhead the albatross".to_owned()),
        "writing the tags threw away the words of a song it did not touch"
    );
    assert!(
        library.kept_lyrics(&missed_at, None)?.is_none(),
        "a miss under the old name was kept past the new one"
    );

    let refusals: i64 = beside(&database)
        .query_row("SELECT count(*) FROM lyrics_refused", [], |row| row.get(0))
        .expect("the refusals count");

    assert_eq!(refusals, 0, "a refusal under the old name was kept");
    Ok(())
}

const A_GATE_HOLDS_FOR: Duration = Duration::from_secs(60);

struct Gate {
    at: PathBuf,
    reached: mpsc::Sender<()>,
    released: Mutex<mpsc::Receiver<()>>,
}

struct Interrupting {
    tags: FileTags,
    refused: Option<PathBuf>,
    gate: Option<Gate>,
}

impl Interrupting {
    fn refusing(file: &Path) -> Self {
        Self {
            tags: FileTags::default(),
            refused: Some(file.to_path_buf()),
            gate: None,
        }
    }

    fn holding_after(file: &Path) -> (Self, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (reached, arrival) = mpsc::channel();
        let (release, released) = mpsc::channel();

        let sink = Self {
            tags: FileTags::default(),
            refused: None,
            gate: Some(Gate {
                at: file.to_path_buf(),
                reached,
                released: Mutex::new(released),
            }),
        };

        (sink, arrival, release)
    }
}

impl TagSource for Interrupting {
    fn read(
        &self,
        location: &MediaLocation,
        picturing: Picturing,
    ) -> resonate_codec::Result<resonate_library::Tagged> {
        self.tags.read(location, picturing)
    }
}

impl TagSink for Interrupting {
    fn writes(&self, location: &MediaLocation) -> bool {
        self.tags.writes(location)
    }

    fn write(
        &self,
        location: &MediaLocation,
        writing: resonate_codec::Writing<'_>,
    ) -> resonate_codec::Result<()> {
        let path = location.as_path().expect("a local file");

        if self.refused.as_deref() == Some(path) {
            return Err(resonate_codec::Error::Unwritable {
                location: location.clone(),
            });
        }
        self.tags.write(location, writing)?;
        if let Some(gate) = self.gate.as_ref().filter(|gate| gate.at == path) {
            gate.reached.send(()).expect("the test waits at the gate");
            gate.released
                .lock()
                .recv_timeout(A_GATE_HOLDS_FOR)
                .expect("the test lets the write go");
        }
        Ok(())
    }

    fn rated(&self, location: &MediaLocation) -> resonate_codec::Result<Rated> {
        self.tags.rated(location)
    }
}

const MISSPELT_AND_RIGHT: [(&str, &str); 3] = [
    ("Echos", "Echoes"),
    ("Fearles", "Fearless"),
    ("San Trope", "San Tropez"),
];

fn titles_where(put_back: [bool; 3]) -> Vec<Option<String>> {
    put_back
        .into_iter()
        .zip(MISSPELT_AND_RIGHT)
        .map(|(back, (misspelt, right))| Some(if back { misspelt } else { right }.to_owned()))
        .collect()
}

struct ThreeFilesRetagged {
    _tree: Tree,
    database: PathBuf,
    library: Library,
    files: Vec<PathBuf>,
}

impl ThreeFilesRetagged {
    fn written() -> Result<Self> {
        let tree = Tree::new();
        let files: Vec<PathBuf> = MISSPELT_AND_RIGHT
            .iter()
            .enumerate()
            .map(|(at, (misspelt, _))| {
                tree.write(
                    &format!("{}.aiff", at + 1),
                    &Aiff::new()
                        .text(TITLE, misspelt)
                        .text(ARTIST, "The Orbiters")
                        .build(),
                )
            })
            .collect();
        let database = tree.path().join("library.db");
        let library = Library::open(&database)?;

        scan(&library, &options(&tree))?;
        for (file, (_, right)) in files.iter().zip(MISSPELT_AND_RIGHT) {
            answer_track(&database, file, right, "The Orbiters", "Orbits");
        }

        let run = retagged(&library, true)?;

        assert_eq!(run.stats.written, 3);
        Ok(Self {
            _tree: tree,
            database,
            library,
            files,
        })
    }

    fn titles(&self) -> Vec<Option<String>> {
        self.files.iter().map(|file| tags_of(file).title).collect()
    }

    fn noted(&self) -> Vec<String> {
        let reader = beside(&self.database);
        let mut statement = reader
            .prepare("SELECT path FROM retagged ORDER BY path")
            .expect("the notes are readable");

        statement
            .query_map([], |row| row.get::<_, String>(0))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .expect("the notes answer their paths")
    }

    fn title_kept_for(&self, file: &Path) -> Option<String> {
        beside(&self.database)
            .query_row(
                "SELECT was FROM retagged_fields WHERE path = ?1 AND field = ?2",
                rusqlite::params![
                    file.to_str().expect("a path the catalog can hold"),
                    TagField::Title.as_str()
                ],
                |row| row.get(0),
            )
            .expect("a note of the title")
    }

    fn named(&self, files: &[usize]) -> Vec<String> {
        files
            .iter()
            .map(|at| {
                self.files[*at]
                    .to_str()
                    .expect("a path the catalog can hold")
                    .to_owned()
            })
            .collect()
    }

    fn walked_back_by(&self, sink: Arc<dyn TagSink>) -> Result<resonate_library::RetagHandle> {
        self.library.retag(
            sink,
            RetagOptions {
                roots: Vec::new(),
                apply: true,
                undo: true,
                lyrics: Vec::new(),
            },
        )
    }

    fn assert_the_unfinished_are_kept(&self, unfinished: usize) {
        assert!(self.library.retag_walks_back().expect("the notes read"));
        assert_eq!(
            self.noted(),
            self.named(&[unfinished]),
            "the notes kept are not the files left unfinished"
        );
        assert_eq!(
            self.title_kept_for(&self.files[unfinished]).as_deref(),
            Some(MISSPELT_AND_RIGHT[unfinished].0),
            "the original title was not kept for the file left unfinished"
        );
    }

    fn assert_a_later_walk_back_finishes(&self, unfinished: usize) -> Result<()> {
        let rest = walked_back(&self.library, true)?;

        assert_eq!(rest.stats.written, 1, "{:?}", rest.retagging.passed_over);
        assert!(rest.retagging.passed_over.is_empty());
        assert_eq!(self.titles(), titles_where([true; 3]));
        assert_eq!(
            tags_of(&self.files[unfinished]).title.as_deref(),
            Some(MISSPELT_AND_RIGHT[unfinished].0)
        );
        Ok(())
    }
}

#[test]
fn a_tag_walk_back_cancelled_between_files_keeps_the_notes_of_those_it_did_not_reach() -> Result<()>
{
    let run = ThreeFilesRetagged::written()?;
    let (gated, arrived, release) = Interrupting::holding_after(&run.files[1]);

    let handle = run.walked_back_by(Arc::new(gated))?;

    arrived
        .recv_timeout(A_GATE_HOLDS_FOR)
        .expect("the walk back reaches the second file");
    handle.cancel();
    release.send(()).expect("the write is waiting");

    let cut_short = handle.join()?;

    assert!(cut_short.cancelled);
    assert_eq!(cut_short.stats.written, 2);
    assert_eq!(run.titles(), titles_where([true, true, false]));

    run.assert_the_unfinished_are_kept(2);
    run.assert_a_later_walk_back_finishes(2)
}

#[test]
fn a_tag_walk_back_that_a_file_refuses_keeps_that_files_notes_for_the_next_one() -> Result<()> {
    let run = ThreeFilesRetagged::written()?;
    let refused = run
        .walked_back_by(Arc::new(Interrupting::refusing(&run.files[1])))?
        .join()?;

    assert!(!refused.cancelled);
    assert_eq!(refused.stats.written, 2);
    assert_eq!(why_passed_over(&refused), [Unwritten::Refused]);
    assert_eq!(run.titles(), titles_where([true, false, true]));

    run.assert_the_unfinished_are_kept(1);
    run.assert_a_later_walk_back_finishes(1)
}

#[test]
fn a_tag_walk_back_that_finds_a_file_missing_keeps_its_notes_until_the_file_is_back() -> Result<()>
{
    let run = ThreeFilesRetagged::written()?;
    let away = run.files[1].with_extension("away");

    moved(&run.files[1], &away);

    let absent = walked_back(&run.library, true);

    moved(&away, &run.files[1]);

    let absent = absent?;

    assert_eq!(absent.stats.written, 2);
    assert_eq!(why_passed_over(&absent), [Unwritten::Unreadable]);
    assert_eq!(run.titles(), titles_where([true, false, true]));

    run.assert_the_unfinished_are_kept(1);
    run.assert_a_later_walk_back_finishes(1)
}

#[test]
fn a_tag_run_is_put_back_after_organise_moved_the_files_it_wrote() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("loose/echoes.wav", &meddle("Echos", "2"));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "Pink Floyd", "Meddle");
    retagged(&library, true)?;

    let organised = applied(&library)?;
    let landed = landings(&organised)
        .into_iter()
        .map(|(_, to)| to)
        .next()
        .expect("the tagged file was filed");

    assert!(!file.exists(), "organise left the file where it stood");
    assert_eq!(tags_of(&landed).title.as_deref(), Some("Echoes"));

    let undone = walked_back(&library, true)?;

    assert_eq!(
        undone.stats.written, 1,
        "{:?}",
        undone.retagging.passed_over
    );
    assert!(undone.retagging.passed_over.is_empty());
    assert_eq!(tags_of(&landed).title.as_deref(), Some("Echos"));
    Ok(())
}

#[test]
fn a_tag_run_is_put_back_after_a_hand_move_the_scan_followed() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("loose/echoes.wav", &meddle("Echos", "2"));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "Pink Floyd", "Meddle");
    retagged(&library, true)?;

    let after = tree.path().join("filed/Pink Floyd/02 Echoes.wav");

    moved(&file, &after);

    let stats = scan(&library, &options(&tree))?;
    let undone = walked_back(&library, true)?;

    assert_eq!(stats.moved, 1);
    assert_eq!(
        undone.stats.written, 1,
        "{:?}",
        undone.retagging.passed_over
    );
    assert_eq!(tags_of(&after).title.as_deref(), Some("Echos"));
    Ok(())
}

struct Misheard(FileTags);

impl TagSource for Misheard {
    fn read(
        &self,
        location: &MediaLocation,
        picturing: Picturing,
    ) -> resonate_codec::Result<resonate_library::Tagged> {
        self.0.read(location, picturing)
    }
}

impl TagSink for Misheard {
    fn writes(&self, location: &MediaLocation) -> bool {
        self.0.writes(location)
    }

    fn write(
        &self,
        location: &MediaLocation,
        writing: resonate_codec::Writing<'_>,
    ) -> resonate_codec::Result<()> {
        let misheard: Vec<TagEdit> = writing
            .edits
            .iter()
            .map(|edit| TagEdit {
                field: edit.field,
                value: format!("{} misheard", edit.value),
            })
            .collect();
        self.0.write(
            location,
            resonate_codec::Writing {
                edits: &misheard,
                ..writing
            },
        )
    }

    fn rated(&self, location: &MediaLocation) -> resonate_codec::Result<Rated> {
        self.0.rated(location)
    }
}

#[test]
fn a_write_that_lands_but_reads_back_otherwise_can_still_be_put_back() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let misheard = library
        .retag(
            Arc::new(Misheard(FileTags::default())),
            RetagOptions {
                roots: Vec::new(),
                apply: true,
                undo: false,
                lyrics: Vec::new(),
            },
        )?
        .join()?;

    assert_eq!(misheard.stats.written, 0);
    assert_eq!(misheard.retagging.passed_over.len(), 1);
    assert_eq!(
        misheard.retagging.passed_over[0].why,
        Unwritten::Unconfirmed
    );
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes misheard"));
    assert!(
        library.retag_walks_back()?,
        "a write that landed was dropped from what can be put back"
    );

    walked_back(&library, true)?;

    assert_eq!(tags_of(&file).title.as_deref(), Some("Echos"));
    Ok(())
}

struct EditedDuringTheReadBack {
    tags: FileTags,
    written: AtomicBool,
}

impl EditedDuringTheReadBack {
    fn edited_by_another_program(&self, location: &MediaLocation) -> resonate_codec::Result<()> {
        let path = location.as_path().expect("a local file");
        let genre = [TagEdit {
            field: TagField::Genre,
            value: "Ambient".to_owned(),
        }];

        self.tags.write(
            location,
            resonate_codec::Writing {
                edits: &genre,
                taken: &[],
                picture: None,
                unpictured: false,
                popularity: None,
            },
        )?;

        let touched = fs::metadata(path).and_then(|held| held.modified());
        let later = touched.expect("a written file's time") + Duration::from_secs(3_600);

        fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(later))
            .expect("another program's edit lands later");
        Ok(())
    }
}

impl TagSource for EditedDuringTheReadBack {
    fn read(
        &self,
        location: &MediaLocation,
        picturing: Picturing,
    ) -> resonate_codec::Result<resonate_library::Tagged> {
        if self.written.swap(false, Ordering::SeqCst) {
            self.edited_by_another_program(location)?;
        }
        self.tags.read(location, picturing)
    }
}

impl TagSink for EditedDuringTheReadBack {
    fn writes(&self, location: &MediaLocation) -> bool {
        self.tags.writes(location)
    }

    fn write(
        &self,
        location: &MediaLocation,
        writing: resonate_codec::Writing<'_>,
    ) -> resonate_codec::Result<()> {
        self.tags.write(location, writing)?;
        self.written.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn rated(&self, location: &MediaLocation) -> resonate_codec::Result<Rated> {
        self.tags.rated(location)
    }
}

#[test]
fn an_edit_another_program_lands_while_a_write_is_read_back_is_read_by_the_next_scan() -> Result<()>
{
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let retagged = library
        .retag(
            Arc::new(EditedDuringTheReadBack {
                tags: FileTags::default(),
                written: AtomicBool::new(false),
            }),
            RetagOptions {
                roots: Vec::new(),
                apply: true,
                undo: false,
                lyrics: Vec::new(),
            },
        )?
        .join()?;

    scan(&library, &options(&tree))?;

    let tracks = library.tracks(&TrackQuery::default())?;

    assert_eq!(retagged.stats.written, 1);
    assert_eq!(tags_of(&file).genre.as_deref(), Some("Ambient"));
    assert_eq!(tracks.len(), 1);
    assert_eq!(
        tracks[0].genre.as_deref(),
        Some("Ambient"),
        "an edit landing between the write and its read-back was taken for the write's own"
    );
    Ok(())
}

#[test]
fn a_tag_run_the_catalog_cannot_note_writes_no_file() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");
    beside(&database)
        .execute_batch(
            "CREATE TRIGGER refused BEFORE INSERT ON retagged
             BEGIN SELECT RAISE(ABORT, 'the catalog refuses the note'); END;",
        )
        .expect("the trigger is laid down");

    assert!(
        retagged(&library, true).is_err(),
        "a run the catalog could not note went on"
    );
    assert_eq!(
        tags_of(&file).title.as_deref(),
        Some("Echos"),
        "a file was written ahead of the note that would put it back"
    );
    Ok(())
}

#[test]
fn a_write_that_fails_is_not_noted_as_one_to_put_back() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "shut/1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    forbid_writing(&file);
    if can_write(&file) {
        allow_writing(&file);
        eprintln!("skipping: this user writes a file whatever its mode");
        return Ok(());
    }

    let summary = retagged(&library, true);

    allow_writing(&file);

    assert_eq!(summary?.stats.written, 0);
    assert!(
        !library.retag_walks_back()?,
        "a write that never landed was noted as one to put back"
    );
    Ok(())
}

fn drawn_png(side: u32) -> Vec<u8> {
    let mut written = std::io::Cursor::new(Vec::new());

    image::RgbImage::from_pixel(side, side, image::Rgb([200, 40, 40]))
        .write_to(&mut written, image::ImageFormat::Png)
        .expect("a picture drawn in memory");
    written.into_inner()
}

fn front_cover_side(file: &Path) -> Option<u32> {
    FileTags::default()
        .read(&MediaLocation::local(file), Picturing::Copied)
        .expect("a file whose tags read back")
        .picture
        .into_copied()
        .and_then(|picture| picture.shorter_side())
}

#[test]
fn a_thumbnail_a_ripper_embedded_gives_way_to_a_cover_twice_its_size() -> Result<()> {
    let tree = Tree::new();
    let thumbnail = drawn_png(120);
    let mut files = Vec::new();

    for (file, title, number) in [
        ("1.wav", "One of These Days", "1"),
        ("2.wav", "A Pillow of Winds", "2"),
        ("3.wav", "Fearless!", "3"),
    ] {
        files.push(
            tree.write(
                file,
                &Wav::new()
                    .text(TITLE, title)
                    .text(ARTIST, "The Orbiters")
                    .text(ALBUM, "Orbits")
                    .text(TRACK, number)
                    .picture(&thumbnail)
                    .id3_in_a_chunk()
                    .build(),
            ),
        );
    }

    let library = Library::open(&tree.path().join("library.db"))?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    assert_eq!(
        library
            .cover_art(album.id)?
            .and_then(|art| art.shorter_side()),
        Some(120)
    );

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        covers: vec![(
            mbid(RELEASE),
            CoverArt {
                format: ImageFormat::Png,
                bytes: drawn_png(600),
            },
        )],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(
        library
            .cover_art(album.id)?
            .and_then(|art| art.shorter_side()),
        Some(600),
        "the archive's cover did not take the thumbnail's place"
    );

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    assert_eq!(
        library
            .cover_art(album.id)?
            .and_then(|art| art.shorter_side()),
        Some(600),
        "a rescan put the thumbnail back over the better cover"
    );

    let planned = retagged(&library, false)?;

    assert!(
        planned
            .retagging
            .writes
            .iter()
            .all(|write| write.picture.is_some()),
        "a file's thumbnail was not offered the better cover"
    );

    let applied = retagged(&library, true)?;

    assert_eq!(front_cover_side(&files[0]), Some(600));
    assert!(
        applied
            .retagging
            .writes
            .iter()
            .all(|write| write.was.picture.is_none()),
        "a replaced picture was held past its page"
    );

    let kept: i64 = beside(&tree.path().join("library.db"))
        .query_row("SELECT count(*) FROM retagged_pictures", [], |row| {
            row.get(0)
        })
        .expect("the kept pictures count");

    assert_eq!(kept, 1, "one thumbnail three files held was kept thrice");

    walked_back(&library, true)?;

    assert_eq!(
        front_cover_side(&files[0]),
        Some(120),
        "putting the run back did not put the thumbnail back"
    );
    Ok(())
}

#[test]
fn a_cover_under_the_small_size_held_before_the_step_is_asked_once_for_a_larger_one() -> Result<()>
{
    let tree = Tree::new();
    let held = drawn_png(500);

    for (file, title, number) in [
        ("1.wav", "One of These Days", "1"),
        ("2.wav", "A Pillow of Winds", "2"),
        ("3.wav", "Fearless!", "3"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(TRACK, number)
                .picture(&held)
                .id3_in_a_chunk()
                .build(),
        );
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;
    let answers = || Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    };
    let larger = CoverArt {
        format: ImageFormat::Png,
        bytes: drawn_png(1200),
    };

    enrich(&library, &Arc::new(Fake::new(answers())), false)?;
    beside(&database)
        .execute_batch("CREATE TABLE larger_covers_wanted (since INTEGER) STRICT;")
        .expect("the step owes the larger covers again");

    let archive = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), larger)],
        ..answers()
    }));

    enrich(&library, &archive, false)?;

    let asked_once = archive.called(LookupOp::Cover);

    enrich(&library, &archive, false)?;

    assert_eq!(
        asked_once, 1,
        "the album owed a larger cover was not asked for one"
    );
    assert_eq!(
        archive.called(LookupOp::Cover),
        1,
        "a cover the step owed was asked for again once the walk had passed"
    );
    assert_eq!(
        library
            .cover_art(album.id)?
            .and_then(|art| art.shorter_side()),
        Some(1200),
        "the archive's larger cover did not take the small one's place"
    );

    let owed: bool = beside(&database)
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'larger_covers_wanted')",
            [],
            |row| row.get(0),
        )
        .expect("the schema reads back");

    assert!(!owed, "the walk left the larger covers owed");
    Ok(())
}

#[test]
fn a_favourite_and_its_plays_are_written_into_the_file_and_taken_away_again() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let library = Library::open(&tree.path().join("library.db"))?;

    scan(&library, &options(&tree))?;

    let location = MediaLocation::local(&file);
    let track = library
        .track_at(&file, None)?
        .expect("the scanned track")
        .id;

    library.favour(Favoured::Track(track), true)?;
    library.track_played(&location, None, Duration::ZERO)?;
    library.track_played(&location, None, Duration::ZERO)?;

    let preview = retagged(&library, false)?;
    let wanted = Popularity {
        favourite: true,
        plays: 2,
    };

    assert_eq!(
        preview
            .retagging
            .writes
            .iter()
            .map(|write| write.popularity)
            .collect::<Vec<_>>(),
        vec![Some(wanted)]
    );

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.ratings, 1);
    assert_eq!(
        FileTags::default()
            .rated(&location)
            .expect("a rating that reads back"),
        Rated::Favourite { plays: Some(2) }
    );

    let again = retagged(&library, false)?;

    assert!(
        again.retagging.writes.is_empty(),
        "the rating did not stick: {:?}",
        again.retagging.writes
    );

    library.favour(Favoured::Track(track), false)?;
    retagged(&library, true)?;

    assert_eq!(
        FileTags::default()
            .rated(&location)
            .expect("a rating that reads back"),
        Rated::Unrated { plays: Some(2) }
    );
    Ok(())
}

#[test]
fn a_play_count_is_written_into_a_file_nobody_marked_a_favourite() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let library = Library::open(&tree.path().join("library.db"))?;

    scan(&library, &options(&tree))?;

    let location = MediaLocation::local(&file);

    for _ in 0..3 {
        library.track_played(&location, None, Duration::ZERO)?;
    }

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.ratings, 1);
    assert_eq!(
        FileTags::default()
            .rated(&location)
            .expect("a count that reads back"),
        Rated::Unrated { plays: Some(3) }
    );
    assert!(
        retagged(&library, false)?.retagging.writes.is_empty(),
        "the count did not stick"
    );
    Ok(())
}

#[test]
fn a_rescan_reads_this_builds_own_write_as_the_names_it_already_knew() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");
    retagged(&library, true)?;

    let held = stored(&database, &file);

    assert_eq!(
        held.tagged_title.as_deref(),
        Some("Echoes"),
        "the catalog did not follow the file it had just written"
    );
    assert!(held.answered.is_some());

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    let after = stored(&database, &file);

    assert_eq!(after.title, "Echoes");
    assert!(
        after.answered.is_some(),
        "a rescan read this build's own write as a retagging and asked again"
    );
    Ok(())
}

#[test]
fn a_release_that_landed_is_written_into_every_file_it_named() -> Result<()> {
    let tree = Tree::new();

    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;

        tree.write(
            &format!("Orbits/{number}.aiff"),
            &Aiff::new()
                .text(TITLE, title)
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(TRACK, &number.to_string())
                .described(MUSICBRAINZ_ALBUM, RELEASE)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let summary = retagged(&library, true)?;

    assert_eq!(summary.stats.written, 3, "{:?}", why_passed_over(&summary));

    let held = tags_of(&tree.path().join("Orbits/1.aiff"));

    assert_eq!(held.album.as_deref(), Some("Orbits"));
    assert_eq!(held.date.as_deref(), Some("1998-03-02"));
    assert_eq!(held.label.as_deref(), Some("Harvest"));
    assert_eq!(held.catalog_number.as_deref(), Some("SHVL 795"));
    assert_eq!(held.barcode.as_deref(), Some("724382920021"));
    assert_eq!(held.musicbrainz_album_id.as_deref(), Some(RELEASE));
    assert_eq!(
        held.musicbrainz_release_group_id.as_deref(),
        Some(RELEASE_GROUP)
    );
    assert_eq!(held.track_total, Some(3));
    assert_eq!(held.disc_total, Some(1));
    assert_eq!(
        held.album_artist, None,
        "an album artist no lookup named was written"
    );
    Ok(())
}

#[test]
fn a_cover_the_catalog_holds_is_written_into_a_file_that_carries_none() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let art = png_art(512);

    covered(&library, &art)?;

    let preview = retagged(&library, false)?;

    assert_eq!(preview.retagging.writes.len(), 1);
    assert_eq!(preview.retagging.writes[0].picture.as_deref(), Some(&art));
    assert_eq!(cover_of(&file), None, "a preview wrote a picture");

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.pictures, 1, "{:?}", why_passed_over(&applied));
    assert_eq!(cover_of(&file), Some(art));

    let again = retagged(&library, false)?;

    assert!(
        again.retagging.writes.is_empty(),
        "the picture did not stick: {:?}",
        again.retagging.writes
    );
    Ok(())
}

fn matroska_named(tree: &Tree, name: &str, tags: &[&str]) -> Option<PathBuf> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a {name}");
        return None;
    }

    let held = Tree::new();
    let source = held.write("tone.wav", &Wav::new().frames(88_200).build());
    let target = tree.path().join(name);
    let mut command = Command::new("ffmpeg");

    command
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .args(["-c:a", "flac"]);
    for tag in tags {
        command.args(["-metadata", tag]);
    }

    let encoded = command
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if !encoded.is_ok_and(|status| status.success()) || !target.exists() {
        eprintln!("skipped: ffmpeg would not encode {name}");
        return None;
    }
    Some(target)
}

#[test]
fn a_matroska_under_an_album_with_a_cover_takes_its_names_and_is_offered_no_picture() -> Result<()>
{
    let tree = Tree::new();
    let Some(file) = matroska_named(
        &tree,
        "1.mka",
        &["title=Echos", "artist=The Orbiters", "album=Orbits"],
    ) else {
        return Ok(());
    };
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    covered(&library, &png_art(512))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let preview = retagged(&library, false)?;

    assert_eq!(preview.retagging.writes.len(), 1);
    assert_eq!(preview.retagging.writes[0].picture, None);

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.written, 1, "{:?}", why_passed_over(&applied));
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));

    let again = retagged(&library, true)?;

    assert_eq!(why_passed_over(&again), Vec::new());
    assert_eq!(again.stats.written, 0);
    Ok(())
}

#[test]
fn a_file_that_carries_a_picture_keeps_the_one_it_has() -> Result<()> {
    let held = png_art(64);
    let tree = Tree::new();

    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .picture(&held.bytes)
            .build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let summary = retagged(&library, true)?;

    assert!(
        summary.retagging.writes.is_empty(),
        "a file with a picture of its own was offered another: {:?}",
        summary.retagging.writes
    );
    assert_eq!(summary.stats.pictures, 0);
    assert_eq!(cover_of(&file), Some(held));
    Ok(())
}

#[test]
fn a_track_under_no_album_is_offered_no_picture() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("1.aiff", &Aiff::new().text(TITLE, "Echoes").build());
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let summary = retagged(&library, true)?;

    assert!(summary.retagging.writes.is_empty());
    assert_eq!(summary.stats.pictures, 0);
    assert_eq!(cover_of(&file), None);
    Ok(())
}

#[test]
fn a_wave_file_is_written_into_and_read_back_as_what_was_written() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .id3_in_a_chunk(),
    )?;

    assert_eq!(
        stored(&database, &file).tagged_title.as_deref(),
        Some("Echos"),
        "the scan did not read the ID3 chunk"
    );

    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let applied = retagged(&library, true)?;

    assert_eq!(applied.stats.written, 1, "{:?}", why_passed_over(&applied));
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));
    assert_eq!(tags_of(&file).artist.as_deref(), Some("The Orbiters"));

    let again = retagged(&library, false)?;

    assert!(
        again.retagging.writes.is_empty(),
        "the write did not stick: {:?}",
        again.retagging.writes
    );
    Ok(())
}

#[test]
fn a_wave_file_tagged_ahead_of_its_riff_header_has_the_tag_moved_into_a_chunk_and_written()
-> Result<()> {
    let (_tree, library, database, file) =
        scanned_lone(Wav::new().text(TITLE, "Echos").text(ARTIST, "The Orbiters"))?;

    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let before = decoded_samples(&file);

    let summary = retagged(&library, true)?;

    assert!(why_passed_over(&summary).is_empty(), "{summary:?}");

    let written = fs::read(&file).expect("the file is still there");

    assert!(
        written.starts_with(b"RIFF"),
        "a tag still stands ahead of the header"
    );
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));
    assert_eq!(
        tags_of(&file).artist.as_deref(),
        Some("The Orbiters"),
        "what the tag carried beside the title was lost as it moved"
    );
    assert_eq!(
        decoded_samples(&file),
        before,
        "the audio did not survive the move"
    );
    Ok(())
}

fn decoded_samples(file: &Path) -> Vec<u8> {
    let bytes = fs::read(file).expect("the file is there");
    let at = bytes
        .windows(4)
        .position(|window| window == b"data")
        .expect("a data chunk");

    let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("a length")) as usize;
    bytes[at + 8..at + 8 + length].to_vec()
}

#[test]
fn a_format_this_build_would_not_read_its_own_write_back_from_is_passed_over() -> Result<()> {
    let tree = Tree::new();

    tree.write("1.caf", &Wav::new().text(TITLE, "Echoes").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let summary = retagged(&library, true)?;

    assert!(summary.retagging.writes.is_empty());
    assert_eq!(why_passed_over(&summary), vec![Unwritten::Unsupported]);
    assert_eq!(summary.stats.passed_over, 1);
    Ok(())
}

#[test]
fn a_row_cut_out_of_a_file_it_shares_never_has_its_names_written_into_the_file() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());
    let sheet = tree.write("Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(
        library.tracks(&TrackQuery::default())?.len(),
        2,
        "the sheet did not cut the file into rows"
    );

    let held = fs::read(&file).expect("the cut file is there");
    let summary = retagged(&library, true)?;

    assert!(summary.retagging.writes.is_empty());
    assert!(
        summary.retagging.sheets.is_empty(),
        "a guess was written into the sheet"
    );
    assert!(why_passed_over(&summary).is_empty(), "{summary:?}");
    assert_eq!(
        fs::read(&file).expect("the cut file is still there"),
        held,
        "a file a sheet cuts was written to"
    );
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is still there"),
        MEDDLE_AIFF_SHEET
    );
    Ok(())
}

fn answer_the_cut(database: &Path, file: &Path, second: bool, title: &str, artist: &str) {
    let written = beside(database)
        .execute(
            "UPDATE tracks
                SET title = ?1, artist = ?2, asked = ?3, answered = ?4
              WHERE path = ?5 AND (span_start > 0) = ?6",
            rusqlite::params![title, artist, ASKED, ANSWERED, settled(file), second],
        )
        .expect("the row the enrichment answers for is writable");

    assert_eq!(written, 1, "one cut row was expected to be answered for");
}

fn titles_held(library: &Library) -> Result<Vec<String>> {
    let mut titles: Vec<String> = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .map(|track| track.title)
        .collect();

    titles.sort();
    Ok(titles)
}

#[test]
fn a_name_answered_for_a_cue_cut_row_is_written_into_its_track_of_the_sheet() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());
    let sheet = tree.write("Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_the_cut(&database, &file, true, "Echoes (Live)", "Pink Floyd");

    let held = fs::read(&file).expect("the cut file is there");

    let preview = retagged(&library, false)?;

    assert!(preview.retagging.writes.is_empty(), "{preview:?}");
    assert_eq!(preview.retagging.sheets.len(), 1, "{preview:?}");
    assert_eq!(
        preview.retagging.sheets[0].edits,
        vec![SheetEdit {
            scope: SheetScope::Track(2),
            field: SheetField::Title,
            value: Some("Echoes (Live)".to_owned()),
        }],
        "the artist the sheet already bills by its performer was offered as well"
    );
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is there"),
        MEDDLE_AIFF_SHEET,
        "a preview changed the sheet"
    );

    let applied = retagged(&library, true)?;

    assert!(why_passed_over(&applied).is_empty(), "{applied:?}");
    assert_eq!(applied.stats.written, 1);
    assert_eq!(applied.stats.fields, 1);
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is still there"),
        MEDDLE_AIFF_SHEET.replace("TITLE \"Echoes\"", "TITLE \"Echoes (Live)\""),
        "more of the sheet moved than the one title"
    );
    assert_eq!(
        fs::read(&file).expect("the cut file is still there"),
        held,
        "the audio the sheet cuts was written to"
    );

    let left: Vec<String> = fs::read_dir(tree.path())
        .expect("the folder lists")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".resonate-"))
        .collect();

    assert!(
        left.is_empty(),
        "the staged sheet was left behind: {left:?}"
    );

    let again = retagged(&library, false)?;

    assert!(again.retagging.sheets.is_empty(), "{again:?}");

    scan(&library, &options(&tree))?;

    assert_eq!(
        titles_held(&library)?,
        vec!["Echoes (Live)".to_owned(), "One of These Days".to_owned()],
        "the rescan did not read the sheet as the catalog had followed it"
    );

    let unasked: i64 = beside(&database)
        .query_row(
            "SELECT count(*) FROM tracks WHERE answered IS NULL AND span_start > 0",
            [],
            |row| row.get(0),
        )
        .expect("the rows are readable");

    assert_eq!(
        unasked, 0,
        "the rescan read the written sheet as retagged by hand"
    );
    Ok(())
}

#[test]
fn a_sheet_edited_since_the_scan_read_it_is_not_written_over() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());
    let sheet = tree.write("Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_the_cut(&database, &file, true, "Echoes (Live)", "Pink Floyd");

    let edited = MEDDLE_AIFF_SHEET.replace("Meddle\"\nFILE", "Meddle (Remastered)\"\nFILE");

    fs::write(&sheet, &edited).expect("the sheet is writable");
    fs::File::options()
        .write(true)
        .open(&sheet)
        .and_then(|held| held.set_modified(SystemTime::now() + Duration::from_secs(5)))
        .expect("the sheet takes a later stamp");

    let summary = retagged(&library, true)?;

    assert_eq!(why_passed_over(&summary), vec![Unwritten::SheetChanged]);
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is there"),
        edited,
        "an edit the scan never read was written over"
    );
    Ok(())
}

#[test]
fn a_tag_run_written_into_a_sheet_is_put_back_byte_for_byte() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());
    let sheet = tree.write("Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_the_cut(&database, &file, false, "One of These Days", "Ada");
    answer_the_cut(&database, &file, true, "Echoes (Live)", "Ada");

    let applied = retagged(&library, true)?;

    assert!(why_passed_over(&applied).is_empty(), "{applied:?}");

    let written = fs::read_to_string(&sheet).expect("the sheet is there");

    assert!(
        written
            .contains("    TITLE \"Echoes (Live)\"\n    PERFORMER \"Ada\"\n    INDEX 01 00:00:20"),
        "{written}"
    );
    assert!(
        library.retag_walks_back()?,
        "the run kept no note of the sheet"
    );

    let preview = walked_back(&library, false)?;

    assert_eq!(preview.retagging.sheets.len(), 1, "{preview:?}");
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is there"),
        written,
        "a preview of the walk back changed the sheet"
    );

    let put_back = walked_back(&library, true)?;

    assert!(why_passed_over(&put_back).is_empty(), "{put_back:?}");
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is there"),
        MEDDLE_AIFF_SHEET,
        "the walk back did not give the sheet back as it was"
    );

    let again = walked_back(&library, true)?;

    assert!(why_passed_over(&again).is_empty(), "{again:?}");
    assert_eq!(
        fs::read_to_string(&sheet).expect("the sheet is there"),
        written,
        "walking the walk back back did not write the run again"
    );
    Ok(())
}

#[test]
fn a_tag_run_written_into_a_sheet_is_put_back_after_organise_moved_the_sheet_and_its_file()
-> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());

    tree.write("Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_the_cut(&database, &file, false, "One of These Days", "Ada");
    answer_the_cut(&database, &file, true, "Echoes (Live)", "Ada");

    let written = retagged(&library, true)?;

    assert!(why_passed_over(&written).is_empty(), "{written:?}");

    let organised = applied(&library)?;
    let landed = landings(&organised)
        .into_iter()
        .map(|(_, to)| to)
        .next()
        .expect("the file a sheet cuts was filed");
    let sheet = landed.with_file_name("Meddle.cue");

    assert!(sheet.exists(), "the sheet did not travel with the file");
    assert!(!tree.path().join("Meddle.cue").exists());

    let put_back = walked_back(&library, true)?;

    assert!(why_passed_over(&put_back).is_empty(), "{put_back:?}");
    assert_eq!(put_back.retagging.sheets.len(), 1, "{put_back:?}");

    let read = fs::read_to_string(&sheet).expect("the sheet is where organise put it");

    assert!(read.contains("    TITLE \"Echoes\"\n"), "{read}");
    assert!(!read.contains("Echoes (Live)"), "{read}");
    Ok(())
}

#[test]
fn a_tag_run_written_into_a_sheet_is_put_back_after_a_hand_move_the_scan_followed() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("rip/Meddle.aiff", &Aiff::new().frames(44_100).build());

    tree.write("rip/Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    answer_the_cut(&database, &file, false, "One of These Days", "Ada");
    answer_the_cut(&database, &file, true, "Echoes (Live)", "Ada");

    let written = retagged(&library, true)?;

    assert!(why_passed_over(&written).is_empty(), "{written:?}");

    fs::create_dir_all(tree.path().join("filed")).expect("a writable temporary directory");
    fs::rename(tree.path().join("rip"), tree.path().join("filed/Meddle"))
        .expect("the rip is moved by hand");

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 2);

    let put_back = walked_back(&library, true)?;

    assert!(why_passed_over(&put_back).is_empty(), "{put_back:?}");
    assert_eq!(put_back.retagging.sheets.len(), 1, "{put_back:?}");

    let read = fs::read_to_string(tree.path().join("filed/Meddle/Meddle.cue"))
        .expect("the sheet is where the hand put it");

    assert!(read.contains("    TITLE \"Echoes\"\n"), "{read}");
    assert!(!read.contains("Echoes (Live)"), "{read}");
    Ok(())
}

#[test]
fn a_sheet_in_a_code_page_is_written_in_it_and_a_name_it_cannot_spell_is_refused() -> Result<()> {
    let latin = |text: &str| {
        text.chars()
            .map(|letter| u8::try_from(u32::from(letter)).expect("a Latin-1 letter"))
            .collect::<Vec<u8>>()
    };
    let sheet_text = "PERFORMER \"Ad\u{e9}la\u{ef}de\"\nTITLE \"Caf\u{e9} cr\u{e8}me\"\nFILE \"Meddle.aiff\" AIFF\n  TRACK 01 AUDIO\n    TITLE \"Entr\u{e9}e\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Fl\u{e2}nerie\"\n    INDEX 01 00:00:20\n";
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());

    let sheet = tree.write("Meddle.cue", &latin(sheet_text));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    assert!(
        titles_held(&library)?.contains(&"Fl\u{e2}nerie".to_owned()),
        "the sheet was not read in its code page"
    );

    answer_the_cut(
        &database,
        &file,
        true,
        "Fl\u{e2}nerie \u{e0} l'aube",
        "Ad\u{e9}la\u{ef}de",
    );

    let applied = retagged(&library, true)?;

    assert!(why_passed_over(&applied).is_empty(), "{applied:?}");
    assert_eq!(
        fs::read(&sheet).expect("the sheet is there"),
        latin(&sheet_text.replace("Fl\u{e2}nerie", "Fl\u{e2}nerie \u{e0} l'aube")),
        "the sheet was not written back in the code page it was read in"
    );

    let standing = fs::read(&sheet).expect("the sheet is there");

    answer_the_cut(
        &database,
        &file,
        false,
        "\u{6771}\u{4eac}",
        "Ad\u{e9}la\u{ef}de",
    );

    let refused = retagged(&library, true)?;

    assert_eq!(why_passed_over(&refused), vec![Unwritten::Refused]);
    assert_eq!(
        fs::read(&sheet).expect("the sheet is there"),
        standing,
        "a sheet whose code page lacks a letter was written to"
    );
    Ok(())
}
