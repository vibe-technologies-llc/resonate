use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::SourceId;

use crate::{Error, LyricProvider, Lyrics, Result, Wanted, lrc};

const SIDECAR: &str = "sidecar";
const BESIDE: [&str; 2] = ["lrc", "txt"];
const WITHIN: [&str; 3] = ["lyrics", "lyric", "lrc"];
const LARGEST_SIDECAR: u64 = lrc::LARGEST_SHEET as u64;
const FOLDERS_WALKED: usize = 32;
const TIMESTAMPS_SETTLE_IN: Duration = Duration::from_secs(1);
const NAMED_AFTER_THE_FILE: usize = 2;

pub struct Sidecar {
    source: SourceId,
    walked: Mutex<Walked>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self {
            source: SourceId::new(SIDECAR).unwrap_or_else(|_| SourceId::local()),
            walked: Mutex::new(Walked::default()),
        }
    }
}

impl Sidecar {
    fn text(&self, path: &Path) -> Result<Option<String>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(cause) => return Err(self.unreachable(cause)),
        };
        let found = file.metadata().map_err(|cause| self.unreachable(cause))?;
        if !found.is_file() {
            return Ok(None);
        }
        let mut head = Vec::new();
        file.take(LARGEST_SIDECAR.saturating_add(1))
            .read_to_end(&mut head)
            .map_err(|cause| self.unreachable(cause))?;

        Ok(Some(whole_lines_within_a_sheet(head)))
    }

    fn unreachable(&self, cause: io::Error) -> Error {
        Error::Unreachable {
            provider: self.source.clone(),
            cause,
        }
    }

    fn held(&self, candidate: &Candidate, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let lyrics = self.read(&candidate.path, wanted)?;
        Ok(match candidate.named {
            NamedAfter::TheFile => lyrics.and_then(|whole| wanted.cut_of_the_file(whole)),
            NamedAfter::TheTrack => lyrics,
        })
    }

    fn read(&self, candidate: &Path, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let Some(text) = self.text(candidate)? else {
            return Ok(None);
        };
        let sheet = lrc::read(self.source.clone(), &text)?;
        let Some(lyrics) = sheet.lyrics else {
            return Ok(None);
        };
        if sheet.declared.names_another_track(wanted) {
            tracing::debug!(
                sidecar = %candidate.display(),
                "a sidecar declares itself to be another track"
            );
            return Ok(None);
        }

        Ok(Some(lyrics))
    }

    fn beside(&self, wanted: &Wanted) -> Result<Vec<Candidate>> {
        let Some(path) = wanted.location.as_path() else {
            return Ok(Vec::new());
        };
        let Some(named) = Named::after(path, wanted) else {
            return Ok(Vec::new());
        };
        let folder = match path.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let held = self.walk(&folder)?;
        let mut ranked = named.ranked(&folder, &held, Place::Beside);

        for name in held.iter().filter(|name| a_lyric_folder(name)) {
            let within = folder.join(name);
            match self.walk(&within) {
                Ok(held) => ranked.extend(named.ranked(&within, &held, Place::Within)),
                Err(error) => tracing::debug!(
                    folder = %within.display(),
                    %error,
                    "a lyric folder beside the track could not be read"
                ),
            }
        }
        ranked.sort();

        Ok(ranked
            .into_iter()
            .map(|(rank, path)| Candidate {
                named: rank.named_after(),
                path,
            })
            .collect())
    }

    fn walk(&self, folder: &Path) -> Result<Arc<[PathBuf]>> {
        let written = written_at(folder);
        if let Some(held) = self.walked.lock().read(folder, written) {
            return Ok(held);
        }
        let names = self.names_in(folder)?;
        self.walked.lock().hold(folder, written, Arc::clone(&names));

        Ok(names)
    }

    fn names_in(&self, folder: &Path) -> Result<Arc<[PathBuf]>> {
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) if nothing_to_walk(&error) => return Ok(Vec::new().into()),
            Err(cause) => return Err(self.unreachable(cause)),
        };

        Ok(entries
            .flatten()
            .map(|entry| PathBuf::from(entry.file_name()))
            .collect())
    }
}

impl LyricProvider for Sidecar {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn lyrics(&self, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let mut refused = None;
        for candidate in self.beside(wanted)? {
            match self.held(&candidate, wanted) {
                Ok(Some(lyrics)) => return Ok(Some(lyrics)),
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(
                        sidecar = %candidate.path.display(),
                        %error,
                        "a sidecar beside the track could not be read"
                    );
                    refused = Some(error);
                }
            }
        }

        refused.map_or(Ok(None), Err)
    }
}

#[derive(Default)]
struct Walked {
    folders: Vec<Walk>,
}

struct Walk {
    folder: PathBuf,
    written: Option<SystemTime>,
    walked: SystemTime,
    names: Arc<[PathBuf]>,
}

impl Walk {
    fn still_says_what_is_there(&self, written: Option<SystemTime>) -> bool {
        if self.written != written {
            return false;
        }
        let Some(written) = written else {
            return true;
        };

        self.walked
            .duration_since(written)
            .is_ok_and(|since| since > TIMESTAMPS_SETTLE_IN)
    }
}

impl Walked {
    fn read(&mut self, folder: &Path, written: Option<SystemTime>) -> Option<Arc<[PathBuf]>> {
        let held = self.folders.iter().position(|walk| walk.folder == folder)?;
        if !self.folders[held].still_says_what_is_there(written) {
            self.folders.remove(held);
            return None;
        }
        let walk = self.folders.remove(held);
        let names = Arc::clone(&walk.names);
        self.folders.insert(0, walk);

        Some(names)
    }

    fn hold(&mut self, folder: &Path, written: Option<SystemTime>, names: Arc<[PathBuf]>) {
        self.folders.retain(|walk| walk.folder != folder);
        self.folders.insert(
            0,
            Walk {
                folder: folder.to_path_buf(),
                written,
                walked: SystemTime::now(),
                names,
            },
        );
        self.folders.truncate(FOLDERS_WALKED);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Beside,
    Within,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    extension: usize,
    name: usize,
    place: Place,
}

impl Rank {
    const fn named_after(&self) -> NamedAfter {
        if self.name < NAMED_AFTER_THE_FILE {
            NamedAfter::TheFile
        } else {
            NamedAfter::TheTrack
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NamedAfter {
    TheFile,
    TheTrack,
}

struct Candidate {
    path: PathBuf,
    named: NamedAfter,
}

struct Named {
    stem: String,
    whole: String,
    tagged: Vec<String>,
}

impl Named {
    fn after(path: &Path, wanted: &Wanted) -> Option<Self> {
        let named = Self {
            stem: lowered(path.file_stem())?,
            whole: lowered(path.file_name())?,
            tagged: tagged(wanted),
        };

        (!named.stem.is_empty()).then_some(named)
    }

    fn ranked(&self, folder: &Path, held: &[PathBuf], place: Place) -> Vec<(Rank, PathBuf)> {
        held.iter()
            .filter_map(|name| Some((self.rank(name, place)?, folder.join(name))))
            .collect()
    }

    fn rank(&self, name: &Path, place: Place) -> Option<Rank> {
        let extension = lowered(name.extension())?;
        let extension = BESIDE.iter().position(|beside| *beside == extension)?;
        let stem = lowered(name.file_stem())?;

        Some(Rank {
            extension,
            name: self.names(&stem)?,
            place,
        })
    }

    fn names(&self, stem: &str) -> Option<usize> {
        let after_the_file: [&String; NAMED_AFTER_THE_FILE] = [&self.stem, &self.whole];
        if let Some(named) = after_the_file.iter().position(|held| held.as_str() == stem) {
            return Some(named);
        }
        let folded = lrc::folded(stem);
        let named = self.tagged.iter().position(|held| *held == folded)?;

        Some(after_the_file.len() + named)
    }
}

fn tagged(wanted: &Wanted) -> Vec<String> {
    let Some(title) = folded_tag(wanted.title.as_deref()) else {
        return Vec::new();
    };
    let mut spellings = Vec::new();
    if let Some(artist) = folded_tag(wanted.artist.as_deref()) {
        spellings.push(format!("{artist}{title}"));
    }
    spellings.push(title);

    spellings
}

fn folded_tag(tag: Option<&str>) -> Option<String> {
    let folded = lrc::folded(tag?);

    (!folded.is_empty()).then_some(folded)
}

fn a_lyric_folder(name: &Path) -> bool {
    lowered(Some(name.as_os_str())).is_some_and(|name| WITHIN.contains(&name.as_str()))
}

fn written_at(folder: &Path) -> Option<SystemTime> {
    fs::metadata(folder).ok()?.modified().ok()
}

fn nothing_to_walk(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

fn lowered(name: Option<&OsStr>) -> Option<String> {
    Some(name?.to_string_lossy().to_lowercase())
}

fn whole_lines_within_a_sheet(mut head: Vec<u8>) -> String {
    if head.len() as u64 <= LARGEST_SIDECAR {
        return String::from_utf8_lossy(&head).into_owned();
    }
    head.truncate(LARGEST_SIDECAR as usize);
    let ended = head
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |last| last + 1);
    head.truncate(ended);

    String::from_utf8_lossy(&head).into_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        env, process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use resonate_core::MediaLocation;

    use super::*;
    use crate::Timing;

    const LRC: &str = "[00:01.00]all that you touch\n[00:05.00]all that you see";

    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "resonate-sidecar-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("a writable temporary directory");
            Self { root }
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.root.join(name);
            fs::write(&path, text).expect("a writable temporary file");
            path
        }

        fn within(&self, folder: &str, name: &str, text: &str) -> PathBuf {
            let folder = self.root.join(folder);
            fs::create_dir_all(&folder).expect("a writable temporary directory");
            let path = folder.join(name);
            fs::write(&path, text).expect("a writable temporary file");
            path
        }

        fn track(&self, name: &str) -> Wanted {
            Wanted::for_media(MediaLocation::local(self.root.join(name)))
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn found(wanted: &Wanted) -> Option<Lyrics> {
        Sidecar::default().lyrics(wanted).expect("nothing failed")
    }

    fn a_cut(wanted: Wanted, from_second: u64, to_second: Option<u64>) -> Wanted {
        let rate = resonate_core::SampleRate::HZ_44100;
        let at = |second: u64| resonate_core::Frames(second * u64::from(rate.hz()));
        Wanted {
            span: Some(match to_second {
                Some(to) => resonate_core::FrameSpan::between(at(from_second), at(to)),
                None => resonate_core::FrameSpan::starting(at(from_second)),
            }),
            rate: Some(rate),
            title: Some("A Pillow of Winds".to_owned()),
            ..wanted
        }
    }

    #[test]
    fn a_row_cut_out_of_a_file_is_handed_its_own_lines_on_its_own_clock() {
        let tree = Tree::new();
        tree.write(
            "Meddle.lrc",
            "[00:01.00]one of these days\n[00:20.50]a cloud of eiderdown\n\
             [00:30.00]draws me\n[00:40.00]overhead the albatross",
        );

        let lyrics = found(&a_cut(tree.track("Meddle.flac"), 20, Some(40)))
            .expect("the row's share of the file's lyrics");

        let lines: Vec<_> = lyrics
            .lines()
            .iter()
            .map(|line| (line.at, line.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (Some(Duration::from_millis(500)), "a cloud of eiderdown"),
                (Some(Duration::from_secs(10)), "draws me"),
            ]
        );
    }

    #[test]
    fn a_row_is_handed_a_sidecar_named_after_it_whole_and_an_unsynced_file_sheet_not_at_all() {
        let tree = Tree::new();
        tree.write("Meddle.txt", "a whole album of words");
        assert_eq!(found(&a_cut(tree.track("Meddle.flac"), 20, None)), None);

        tree.write("A Pillow of Winds.lrc", LRC);
        let own = found(&a_cut(tree.track("Meddle.flac"), 20, None)).expect("the row's own sheet");
        assert_eq!(
            own.lines().first().and_then(|line| line.at),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn a_track_with_an_lrc_beside_it_is_read_from_that_file() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", LRC);

        let lyrics = found(&tree.track("Echoes.flac")).expect("the file beside it");

        assert_eq!(lyrics.timing(), Timing::Synced);
        assert_eq!(lyrics.source().as_str(), SIDECAR);
        assert_eq!(lyrics.lines().len(), 2);
    }

    #[test]
    fn a_sidecar_named_after_the_whole_file_is_found_too() {
        let tree = Tree::new();
        tree.write("Echoes.flac.lrc", LRC);

        assert!(found(&tree.track("Echoes.flac")).is_some());
    }

    #[test]
    fn an_lrc_outranks_a_text_file_beside_the_same_track() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", LRC);
        tree.write("Echoes.txt", "all that you distrust");

        let lyrics = found(&tree.track("Echoes.flac")).expect("the file beside it");

        assert_eq!(lyrics.timing(), Timing::Synced);
    }

    #[test]
    fn a_text_file_beside_the_track_is_read_as_an_unsynced_set() {
        let tree = Tree::new();
        tree.write("Echoes.txt", "all that you touch\nall that you see");

        let lyrics = found(&tree.track("Echoes.flac")).expect("the file beside it");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
    }

    #[test]
    fn a_track_with_nothing_beside_it_answers_with_nothing_rather_than_refusing() {
        let tree = Tree::new();

        assert!(found(&tree.track("Echoes.flac")).is_none());
    }

    #[test]
    fn a_location_that_is_not_a_path_is_not_something_to_look_beside() {
        let remote = Wanted::for_media(MediaLocation::new(
            SourceId::new("subsonic").expect("a lowercase name"),
            "track/1",
        ));

        assert!(found(&remote).is_none());
    }

    #[test]
    fn a_sidecar_holding_more_lines_than_a_sheet_does_is_refused() {
        let tree = Tree::new();
        let bulk = "all that you touch\n".repeat(64 * 1024);
        tree.write("Echoes.lrc", &bulk);

        let refused = Sidecar::default().lyrics(&tree.track("Echoes.flac"));

        assert!(matches!(refused, Err(Error::Unreadable { .. })));
    }

    #[test]
    fn a_text_file_behind_a_sidecar_too_large_to_read_is_what_is_drawn() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &"all that you touch\n".repeat(64 * 1024));
        tree.write("Echoes.txt", "all that you touch");

        let lyrics = found(&tree.track("Echoes.flac")).expect("the text file behind it");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
    }

    #[test]
    fn the_words_of_a_sidecar_are_read_though_a_tail_too_long_to_hold_follows_them() {
        let tree = Tree::new();
        let tail = "-".repeat(LARGEST_SIDECAR as usize);
        tree.write("Echoes.lrc", &format!("{LRC}\n{tail}"));

        let lyrics = found(&tree.track("Echoes.flac")).expect("the words before the tail");

        assert_eq!(lyrics.timing(), Timing::Synced);
        assert_eq!(lyrics.lines().len(), 2);
    }

    #[test]
    fn a_sidecar_whose_first_line_outruns_a_sheet_is_read_as_holding_nothing() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &"-".repeat(LARGEST_SIDECAR as usize + 1));
        tree.write("Echoes.txt", "all that you touch");

        let lyrics = found(&tree.track("Echoes.flac")).expect("the text file behind it");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
    }

    #[test]
    fn a_sidecar_holding_nothing_worth_drawing_is_passed_over() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", "[ti:Echoes]\n");
        tree.write("Echoes.txt", "all that you touch");

        let lyrics = found(&tree.track("Echoes.flac")).expect("the text file behind it");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
    }

    #[test]
    fn a_sidecar_under_any_casing_is_found_rather_than_only_the_four_it_used_to_be_named() {
        let tree = Tree::new();
        tree.write("ECHOES.Lrc", LRC);

        assert!(found(&tree.track("Echoes.flac")).is_some());
    }

    #[test]
    fn a_sidecar_declaring_the_track_it_sits_beside_is_read() {
        let tree = Tree::new();
        tree.write(
            "Echoes.lrc",
            &format!("[ti:Echoes]\n[ar:Pink Floyd]\n{LRC}"),
        );

        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            ..tree.track("Echoes.flac")
        };

        assert!(found(&wanted).is_some());
    }

    #[test]
    fn a_sidecar_declaring_another_song_is_passed_over_rather_than_drawn() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &format!("[ti:Time]\n[ar:Pink Floyd]\n{LRC}"));

        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            ..tree.track("Echoes.flac")
        };

        assert!(found(&wanted).is_none());
    }

    #[test]
    fn a_text_file_behind_a_sidecar_that_names_another_song_is_what_is_drawn() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &format!("[ti:Time]\n{LRC}"));
        tree.write("Echoes.txt", "all that you touch");

        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            ..tree.track("Echoes.flac")
        };

        let lyrics = found(&wanted).expect("the text file behind it");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
    }

    #[test]
    fn a_sidecar_is_read_where_nothing_has_named_the_track_yet() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &format!("[ti:Time]\n{LRC}"));

        assert!(found(&tree.track("Echoes.flac")).is_some());
    }

    #[test]
    fn a_directory_named_like_a_sidecar_is_not_one() {
        let tree = Tree::new();
        fs::create_dir_all(tree.root.join("Echoes.lrc")).expect("a writable temporary directory");
        tree.write("Echoes.txt", "all that you touch");

        assert!(found(&tree.track("Echoes.flac")).is_some());
    }

    #[test]
    fn a_sheet_filed_in_a_lyrics_folder_beside_the_track_is_found() {
        let tree = Tree::new();
        tree.within("Lyrics", "Echoes.lrc", LRC);

        let lyrics = found(&tree.track("Echoes.flac")).expect("the folder beside it");

        assert_eq!(lyrics.timing(), Timing::Synced);
    }

    #[test]
    fn every_spelling_of_a_lyrics_folder_is_looked_in() {
        for folder in ["LYRICS", "Lyric", "lrc"] {
            let tree = Tree::new();
            tree.within(folder, "Echoes.lrc", LRC);

            assert!(found(&tree.track("Echoes.flac")).is_some(), "{folder}");
        }
    }

    #[test]
    fn a_folder_beside_the_track_that_is_not_a_lyric_folder_is_left_alone() {
        let tree = Tree::new();
        tree.within("Scans", "Echoes.lrc", LRC);

        assert!(found(&tree.track("Echoes.flac")).is_none());
    }

    #[test]
    fn a_sheet_beside_the_track_outranks_the_same_name_filed_in_the_lyrics_folder() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", LRC);
        tree.within("Lyrics", "Echoes.lrc", "[00:01.00]all that you distrust");

        let lyrics = found(&tree.track("Echoes.flac")).expect("the file beside it");

        assert_eq!(lyrics.lines().len(), 2);
    }

    #[test]
    fn a_sheet_named_after_the_tags_rather_than_the_file_is_found() {
        let tree = Tree::new();
        tree.write("Pink Floyd - Echoes.lrc", LRC);
        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            ..tree.track("01 Meddle side two.flac")
        };

        assert!(found(&wanted).is_some());
    }

    #[test]
    fn a_sheet_named_after_the_title_alone_is_found_too() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", LRC);
        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            ..tree.track("01 Meddle side two.flac")
        };

        assert!(found(&wanted).is_some());
    }

    #[test]
    fn punctuation_and_case_in_a_tag_named_sheet_still_name_the_track() {
        let tree = Tree::new();
        tree.write("pink_floyd echoes.LRC", LRC);
        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            ..tree.track("01 Meddle side two.flac")
        };

        assert!(found(&wanted).is_some());
    }

    #[test]
    fn a_sheet_named_after_the_file_outranks_one_named_after_the_tags() {
        let tree = Tree::new();
        tree.write("01 Meddle side two.lrc", LRC);
        tree.write("Pink Floyd - Echoes.lrc", "[00:01.00]all that you distrust");
        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            ..tree.track("01 Meddle side two.flac")
        };

        let lyrics = found(&wanted).expect("the sheet named after the file");

        assert_eq!(lyrics.lines().len(), 2);
    }

    #[test]
    fn a_sheet_named_after_the_tags_is_passed_over_where_nothing_has_named_the_track() {
        let tree = Tree::new();
        tree.write("Pink Floyd - Echoes.lrc", LRC);

        assert!(found(&tree.track("01 Meddle side two.flac")).is_none());
    }

    #[test]
    fn a_sidecar_declaring_another_length_is_passed_over_rather_than_drawn() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &format!("[length:03:20]\n{LRC}"));
        let wanted = Wanted {
            duration: Some(Duration::from_secs(23 * 60 + 31)),
            ..tree.track("Echoes.flac")
        };

        assert!(found(&wanted).is_none());
    }

    #[test]
    fn a_sidecar_declaring_the_length_it_sits_beside_is_read() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &format!("[length:23:31]\n{LRC}"));
        let wanted = Wanted {
            duration: Some(Duration::from_secs(23 * 60 + 33)),
            ..tree.track("Echoes.flac")
        };

        assert!(found(&wanted).is_some());
    }

    #[test]
    fn a_sidecar_is_read_where_the_track_has_no_length_to_weigh_it_against() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", &format!("[length:03:20]\n{LRC}"));

        assert!(found(&tree.track("Echoes.flac")).is_some());
    }

    #[test]
    fn a_sheet_dropped_in_beside_a_playing_track_is_found_though_the_folder_was_walked() {
        let tree = Tree::new();
        let sidecar = Sidecar::default();
        let wanted = tree.track("Echoes.flac");
        assert!(sidecar.lyrics(&wanted).expect("nothing failed").is_none());

        tree.write("Echoes.lrc", LRC);

        assert!(sidecar.lyrics(&wanted).expect("nothing failed").is_some());
    }

    #[test]
    fn a_folder_is_remembered_once_however_many_tracks_are_played_out_of_it() {
        let tree = Tree::new();
        let sidecar = Sidecar::default();
        tree.write("Echoes.lrc", LRC);
        for track in ["Echoes.flac", "Time.flac", "Money.flac"] {
            let _ = sidecar.lyrics(&tree.track(track)).expect("nothing failed");
        }

        assert_eq!(sidecar.walked.lock().folders.len(), 1);
    }

    fn held(names: &[&str]) -> Arc<[PathBuf]> {
        names.iter().map(PathBuf::from).collect()
    }

    fn folder(at: usize) -> PathBuf {
        PathBuf::from(format!("/music/{at}"))
    }

    fn long_ago() -> Option<SystemTime> {
        Some(SystemTime::UNIX_EPOCH)
    }

    #[test]
    fn a_folder_walked_once_is_read_back_rather_than_walked_again() {
        let mut walked = Walked::default();
        walked.hold(&folder(0), long_ago(), held(&["Echoes.lrc"]));

        assert_eq!(
            walked
                .read(&folder(0), long_ago())
                .expect("a folder held")
                .len(),
            1
        );
    }

    #[test]
    fn a_folder_nothing_has_walked_is_not_something_to_read_back() {
        let mut walked = Walked::default();
        walked.hold(&folder(0), long_ago(), held(&["Echoes.lrc"]));

        assert!(walked.read(&folder(1), long_ago()).is_none());
    }

    #[test]
    fn a_folder_written_to_since_it_was_walked_is_walked_again() {
        let mut walked = Walked::default();
        walked.hold(&folder(0), long_ago(), held(&["Echoes.lrc"]));

        assert!(walked.read(&folder(0), Some(SystemTime::now())).is_none());
    }

    #[test]
    fn a_folder_written_to_in_the_moment_it_was_walked_is_walked_again() {
        let mut walked = Walked::default();
        let written = Some(SystemTime::now());
        walked.hold(&folder(0), written, held(&["Echoes.lrc"]));

        assert!(walked.read(&folder(0), written).is_none());
    }

    #[test]
    fn the_walk_remembers_no_more_folders_than_it_says_it_does() {
        let mut walked = Walked::default();
        for at in 0..FOLDERS_WALKED + 8 {
            walked.hold(&folder(at), long_ago(), held(&[]));
        }

        assert_eq!(walked.folders.len(), FOLDERS_WALKED);
        assert!(walked.read(&folder(0), long_ago()).is_none());
        assert!(
            walked
                .read(&folder(FOLDERS_WALKED + 7), long_ago())
                .is_some()
        );
    }

    #[test]
    fn the_folder_nothing_has_looked_at_is_the_one_the_walk_forgets() {
        let mut walked = Walked::default();
        for at in 0..FOLDERS_WALKED {
            walked.hold(&folder(at), long_ago(), held(&[]));
        }
        assert!(walked.read(&folder(0), long_ago()).is_some());

        walked.hold(&folder(FOLDERS_WALKED), long_ago(), held(&[]));

        assert!(walked.read(&folder(0), long_ago()).is_some());
        assert!(walked.read(&folder(1), long_ago()).is_none());
    }
}
