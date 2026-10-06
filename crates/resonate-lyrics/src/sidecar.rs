use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{SourceId, text};

use crate::{
    Error, LARGEST_LYRICSFILE, LyricOp, LyricProvider, Lyrics, Result, Wanted, lrc, read_lyricsfile,
};

const SIDECAR: &str = "sidecar";
const BESIDE: [(&str, Written); 4] = [
    (".lyricsfile.yaml", Written::Lyricsfile),
    (".lyricsfile.yml", Written::Lyricsfile),
    (".lrc", Written::Lrc),
    (".txt", Written::Lrc),
];
const WITHIN: [&str; 3] = ["lyrics", "lyric", "lrc"];
const LARGEST_SIDECAR: u64 = lrc::LARGEST_SHEET as u64;
const LANGUAGE_CODE_LETTERS: usize = 2;
const FOLDERS_WALKED: usize = 32;
const TIMESTAMPS_SETTLE_IN: Duration = Duration::from_secs(1);
const SPELLED_AFTER_THE_FILE: usize = 2;
const SPELLED_IN_A_LANGUAGE: usize = SPELLED_AFTER_THE_FILE;
const NAMED_AFTER_THE_FILE: usize = SPELLED_IN_A_LANGUAGE + 1;
const LANGUAGES_LISTED_IN: &str = "LANGUAGE";
const LOCALE_NAMED_IN: [&str; 3] = ["LC_ALL", "LC_MESSAGES", "LANG"];
const THE_PLAIN_LOCALES: [&str; 2] = ["c", "posix"];

pub struct Sidecar {
    source: SourceId,
    walked: Mutex<Walked>,
    read_in: Vec<String>,
    by_the_locale: Arc<AtomicBool>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self::choosing_by_the_locale(Arc::new(AtomicBool::new(false)))
    }
}

fn languages_the_listener_reads() -> Vec<String> {
    let named = |variable: &str| env::var(variable).ok().filter(|value| !value.is_empty());
    let locale = LOCALE_NAMED_IN.into_iter().find_map(named);
    let plain = locale
        .as_deref()
        .and_then(language_of)
        .is_none_or(|language| THE_PLAIN_LOCALES.contains(&language.as_str()));
    let listed = named(LANGUAGES_LISTED_IN).filter(|_| !plain);

    let mut languages: Vec<String> = Vec::new();
    for language in listed
        .iter()
        .flat_map(|listed| listed.split(':'))
        .chain(locale.as_deref())
        .filter_map(language_of)
    {
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    languages
}

fn language_of(locale: &str) -> Option<String> {
    let language = locale
        .split(['_', '-', '.', '@'])
        .next()?
        .to_ascii_lowercase();

    (!language.is_empty()).then_some(language)
}

impl Sidecar {
    pub fn choosing_by_the_locale(by_the_locale: Arc<AtomicBool>) -> Self {
        Self::read_in(languages_the_listener_reads(), by_the_locale)
    }

    fn read_in(languages: Vec<String>, by_the_locale: Arc<AtomicBool>) -> Self {
        Self {
            source: SourceId::new(SIDECAR).unwrap_or_else(|_| SourceId::local()),
            walked: Mutex::new(Walked::default()),
            read_in: languages,
            by_the_locale,
        }
    }

    fn languages_chosen_by(&self) -> &[String] {
        match self.by_the_locale.load(Ordering::Acquire) {
            true => &self.read_in,
            false => &[],
        }
    }

    fn head(&self, path: &Path, bound: u64) -> Result<Option<Vec<u8>>> {
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
        file.take(bound.saturating_add(1))
            .read_to_end(&mut head)
            .map_err(|cause| self.unreachable(cause))?;

        Ok(Some(head))
    }

    fn unparsed(&self) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op: LyricOp::Parse,
        }
    }

    fn unreachable(&self, cause: io::Error) -> Error {
        Error::Unreachable {
            provider: self.source.clone(),
            cause,
        }
    }

    fn held(&self, candidate: &Candidate, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let lyrics = self.read(candidate, wanted)?;
        Ok(match candidate.named {
            NamedAfter::TheFile => lyrics.and_then(|whole| wanted.cut_of_the_file(whole)),
            NamedAfter::TheTrack => lyrics,
        })
    }

    fn read(&self, candidate: &Candidate, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let (declared, lyrics) = match candidate.written {
            Written::Lrc => {
                let Some(head) = self.head(&candidate.path, LARGEST_SIDECAR)? else {
                    return Ok(None);
                };
                let sheet = lrc::read(self.source.clone(), &whole_lines_within_a_sheet(head))?;
                (sheet.declared, sheet.lyrics)
            }
            Written::Lyricsfile => {
                let Some(head) = self.head(&candidate.path, LARGEST_LYRICSFILE as u64)? else {
                    return Ok(None);
                };
                let text = String::from_utf8(head).map_err(|_| self.unparsed())?;
                let document = read_lyricsfile(self.source.clone(), &text).map_err(|unread| {
                    tracing::debug!(
                        sidecar = %candidate.path.display(),
                        ?unread,
                        "a lyricsfile beside the track could not be read"
                    );
                    self.unparsed()
                })?;
                (document.declared, document.lyrics)
            }
        };
        let Some(lyrics) = lyrics else {
            return Ok(None);
        };
        if declared.names_another_track(wanted) {
            tracing::debug!(
                sidecar = %candidate.path.display(),
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
        let Some(named) = Named::after(path, wanted, self.languages_chosen_by()) else {
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
                written: BESIDE[rank.extension].1,
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
    understood: usize,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Written {
    Lyricsfile,
    Lrc,
}

struct Candidate {
    path: PathBuf,
    named: NamedAfter,
    written: Written,
}

struct Named<'a> {
    stem: String,
    whole: String,
    tagged: Vec<String>,
    read_in: &'a [String],
}

impl<'a> Named<'a> {
    fn after(path: &Path, wanted: &Wanted, read_in: &'a [String]) -> Option<Self> {
        let named = Self {
            stem: lowered(path.file_stem())?,
            whole: lowered(path.file_name())?,
            tagged: tagged(wanted),
            read_in,
        };

        (!named.stem.is_empty()).then_some(named)
    }

    fn ranked(&self, folder: &Path, held: &[PathBuf], place: Place) -> Vec<(Rank, PathBuf)> {
        held.iter()
            .filter_map(|name| Some((self.rank(name, place)?, folder.join(name))))
            .collect()
    }

    fn rank(&self, name: &Path, place: Place) -> Option<Rank> {
        let name = lowered(Some(name.as_os_str()))?;
        let (extension, stem) = BESIDE.iter().enumerate().find_map(|(at, (ending, _))| {
            let stem = name.strip_suffix(ending)?;
            (!stem.is_empty()).then_some((at, stem))
        })?;

        Some(Rank {
            extension,
            name: self.names(stem)?,
            understood: self.understood(stem),
            place,
        })
    }

    fn understood(&self, stem: &str) -> usize {
        let Some(language) = self.language_of(stem) else {
            return 0;
        };

        self.read_in
            .iter()
            .position(|read| *read == language)
            .unwrap_or(self.read_in.len())
    }

    fn language_of(&self, stem: &str) -> Option<String> {
        let tag = stem
            .strip_prefix(self.stem.as_str())?
            .strip_prefix('.')
            .filter(|tag| a_language_tag(tag))?;

        language_of(tag)
    }

    fn names(&self, stem: &str) -> Option<usize> {
        let after_the_file: [&String; SPELLED_AFTER_THE_FILE] = [&self.stem, &self.whole];
        if let Some(named) = after_the_file.iter().position(|held| held.as_str() == stem) {
            return Some(named);
        }
        if self.in_a_language(stem) {
            return Some(SPELLED_IN_A_LANGUAGE);
        }
        let folded = lrc::folded(stem);
        let named = self.tagged.iter().position(|held| *held == folded)?;

        Some(NAMED_AFTER_THE_FILE + named)
    }

    fn in_a_language(&self, stem: &str) -> bool {
        self.language_of(stem).is_some()
    }
}

fn a_language_tag(tag: &str) -> bool {
    let mut subtags = tag.split(['-', '_']);
    let language = subtags.next().unwrap_or_default();

    language.len() == LANGUAGE_CODE_LETTERS
        && language.chars().all(|letter| letter.is_ascii_lowercase())
        && subtags.all(|subtag| {
            (2..=4).contains(&subtag.len()) && subtag.chars().all(|ch| ch.is_ascii_alphanumeric())
        })
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

fn whole_lines_within_a_sheet(head: Vec<u8>) -> String {
    let cut_short = head.len() as u64 > LARGEST_SIDECAR;
    let mut sheet = text::decoded(&head).0;
    if !cut_short && sheet.len() as u64 <= LARGEST_SIDECAR {
        return sheet;
    }

    let mut within = sheet.len().min(LARGEST_SIDECAR as usize);
    while !sheet.is_char_boundary(within) {
        within -= 1;
    }
    sheet.truncate(within);
    let ended = sheet.rfind(['\n', '\r']).map_or(0, |last| last + 1);
    sheet.truncate(ended);
    sheet
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
    fn an_lrc_written_in_a_legacy_code_page_is_read_in_it() {
        let tree = Tree::new();
        let sung = "[00:01.00]Группа крови на рукаве\n[00:05.00]Мой порядковый номер на рукаве";
        let written = text::encoded(
            sung,
            resonate_core::TextEncoding::Legacy(resonate_core::LegacyEncoding::WINDOWS_1251),
        )
        .expect("Cyrillic letters");
        fs::write(tree.root.join("Кино.lrc"), written).expect("a writable temporary file");

        let lyrics = found(&tree.track("Кино.flac")).expect("the file beside it");

        assert_eq!(
            lyrics.lines().first().map(|line| line.text.as_str()),
            Some("Группа крови на рукаве")
        );
    }

    const WORDED: &str = include_str!("../tests/fixtures/lyricsfile_worded.yaml");

    #[test]
    fn a_lyricsfile_beside_the_track_is_read_word_by_word_ahead_of_an_lrc() {
        let tree = Tree::new();
        tree.write("Small Hours.lrc", LRC);
        tree.write("Small Hours.lyricsfile.yaml", WORDED);

        let lyrics = found(&tree.track("Small Hours.flac")).expect("the file beside it");

        assert_eq!(lyrics.detail(), crate::Detail::Words);
        assert_eq!(lyrics.lines()[0].text, "Stay until the morning");
    }

    #[test]
    fn a_lyricsfile_declaring_another_song_gives_way_to_the_lrc_beside_it() {
        let tree = Tree::new();
        tree.write("Small Hours.lrc", LRC);
        tree.write("Small Hours.lyricsfile.yml", WORDED);

        let wanted = Wanted {
            title: Some("Large Hours".to_owned()),
            ..tree.track("Small Hours.flac")
        };
        let lyrics = found(&wanted).expect("the lrc beside it");

        assert_eq!(lyrics.detail(), crate::Detail::Lines);
    }

    #[test]
    fn a_lyricsfile_that_does_not_parse_gives_way_to_the_lrc_beside_it() {
        let tree = Tree::new();
        tree.write("Echoes.lrc", LRC);
        tree.write("Echoes.lyricsfile.yaml", "version: '9.0'\n");

        assert!(found(&tree.track("Echoes.flac")).is_some());
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
    fn a_sheet_named_after_the_file_and_a_language_is_found() {
        let tree = Tree::new();
        tree.write("Echoes.pt-BR.lrc", LRC);

        assert!(found(&tree.track("Echoes.flac")).is_some());
    }

    #[test]
    fn a_sheet_named_after_the_file_outranks_one_in_a_language() {
        let tree = Tree::new();
        tree.write("Echoes.en.lrc", "[00:01.00]all that you distrust");
        tree.write("Echoes.lrc", LRC);

        let lyrics = found(&tree.track("Echoes.flac")).expect("the sheet named after the file");

        assert_eq!(lyrics.lines().len(), 2);
    }

    #[test]
    fn of_two_sheets_in_a_language_the_one_the_listener_reads_answers() {
        let tree = Tree::new();
        tree.write("Song.en.lrc", "[00:01.00]all that you distrust");
        tree.write("Song.ja.lrc", LRC);
        let wanted = tree.track("Song.flac");
        let lines_read_in = |languages: &[&str], chosen: bool| {
            Sidecar::read_in(
                languages.iter().map(|read| (*read).to_owned()).collect(),
                Arc::new(AtomicBool::new(chosen)),
            )
            .lyrics(&wanted)
            .expect("nothing failed")
            .map(|lyrics| lyrics.lines().len())
        };

        assert_eq!(lines_read_in(&["ja", "en"], true), Some(2));
        assert_eq!(lines_read_in(&["en", "ja"], true), Some(1));
        assert_eq!(
            lines_read_in(&["fr"], true),
            Some(1),
            "a listener reading neither is given the first in name order"
        );
        assert_eq!(
            lines_read_in(&["ja", "en"], false),
            Some(1),
            "the locale chose though the listener said not to"
        );
    }

    #[test]
    fn a_locale_is_read_as_the_language_it_names() {
        assert_eq!(language_of("ja_JP.UTF-8"), Some("ja".to_owned()));
        assert_eq!(language_of("pt-BR"), Some("pt".to_owned()));
        assert_eq!(language_of("de_DE@euro"), Some("de".to_owned()));
        assert_eq!(language_of("C.UTF-8"), Some("c".to_owned()));
        assert_eq!(language_of(""), None);
    }

    #[test]
    fn a_sheet_named_after_the_file_and_a_word_that_is_no_language_is_passed_over() {
        let tree = Tree::new();
        tree.write("Echoes.live.lrc", LRC);
        tree.write("Echoes.1.lrc", LRC);

        assert!(found(&tree.track("Echoes.flac")).is_none());
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
