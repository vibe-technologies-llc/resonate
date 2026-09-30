use std::{
    ffi::OsString,
    fs::{self, File, Metadata},
    io::{self, Read},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use ahash::AHashSet;
use resonate_core::names_audio;

use crate::{
    Error, Result,
    pass::{Cancelling, PassHandle, PassKind, TakeInHandle},
};

const SHEET_EXTENSION: &str = "cue";

const DEEPEST_FOLDER: usize = 32;

const COMPARED_AT_ONCE: usize = 256 * 1024;

const CANDIDATE_NAMES: u32 = 999;

const STAGED_SUFFIX: &str = "resonate-part";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Looks {
    Audio,
    Sheet,
    Folder,
    Other,
    Gone,
}

impl Looks {
    pub const fn is_taken(self) -> bool {
        matches!(self, Self::Audio | Self::Sheet | Self::Folder)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub path: PathBuf,
    pub looks: Looks,
}

fn is_a_sheet(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SHEET_EXTENSION))
}

fn is_taken(path: &Path) -> bool {
    names_audio(path) || is_a_sheet(path)
}

pub fn weigh(paths: &[PathBuf]) -> Vec<Dropped> {
    paths
        .iter()
        .map(|path| Dropped {
            path: path.clone(),
            looks: match fs::metadata(path) {
                Err(_) => Looks::Gone,
                Ok(metadata) if metadata.is_dir() => Looks::Folder,
                Ok(_) if names_audio(path) => Looks::Audio,
                Ok(_) if is_a_sheet(path) => Looks::Sheet,
                Ok(_) => Looks::Other,
            },
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeInOptions {
    pub paths: Vec<PathBuf>,
    pub into: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Passing {
    SourceGone,
    Unreadable,
    NotAudio,
    NothingInside,
    AlreadyThere,
    AlreadyHeld,
    Unwritable,
    Unverified,
}

impl Passing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceGone => "is not there any more",
            Self::Unreadable => "could not be read",
            Self::NotAudio => "is not a format the library reads",
            Self::NothingInside => "holds no audio",
            Self::AlreadyThere => "is already in the folder",
            Self::AlreadyHeld => "is already in the folder, byte for byte",
            Self::Unwritable => "could not be written to the folder",
            Self::Unverified => "did not read back the same as it was sent",
        }
    }

    pub const fn is_a_refusal(self) -> bool {
        !matches!(self, Self::AlreadyThere | Self::AlreadyHeld)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Passed {
    pub from: PathBuf,
    pub why: Passing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landed {
    pub from: PathBuf,
    pub to: PathBuf,
    pub bytes: u64,
    pub renamed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TakeInStats {
    pub found: u64,
    pub copied: u64,
    pub held: u64,
    pub passed: u64,
    pub bytes_found: u64,
    pub bytes: u64,
}

impl TakeInStats {
    pub const fn seen_to(&self) -> u64 {
        self.copied + self.held + self.passed
    }
}

#[derive(Debug, Default)]
pub struct TakeInProgress {
    found: AtomicU64,
    copied: AtomicU64,
    held: AtomicU64,
    passed: AtomicU64,
    bytes_found: AtomicU64,
    bytes: AtomicU64,
    cancelled: AtomicBool,
}

impl TakeInProgress {
    pub fn snapshot(&self) -> TakeInStats {
        TakeInStats {
            found: self.found.load(Ordering::Relaxed),
            copied: self.copied.load(Ordering::Relaxed),
            held: self.held.load(Ordering::Relaxed),
            passed: self.passed.load(Ordering::Relaxed),
            bytes_found: self.bytes_found.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Cancelling for TakeInProgress {
    fn cancel(&self) {
        TakeInProgress::cancel(self);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TakeInSummary {
    pub stats: TakeInStats,
    pub landed: Vec<Landed>,
    pub passed: Vec<Passed>,
    pub cancelled: bool,
}

pub fn take_in(options: TakeInOptions) -> Result<TakeInHandle> {
    if !options.into.is_dir() {
        return Err(Error::DestinationNotADirectory { path: options.into });
    }

    let progress = Arc::new(TakeInProgress::default());
    let owned = Arc::clone(&progress);

    let thread = thread::Builder::new()
        .name("resonate-take-in".to_owned())
        .spawn(move || Ok(run(&options, &progress)))
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::TakeIn, owned, thread))
}

struct Item {
    from: PathBuf,
    relative: PathBuf,
    bytes: u64,
}

fn run(options: &TakeInOptions, progress: &TakeInProgress) -> TakeInSummary {
    let mut summary = TakeInSummary::default();
    let into = options
        .into
        .canonicalize()
        .unwrap_or_else(|_| options.into.clone());

    let items = gather(&options.paths, &mut summary.passed);
    progress.found.store(items.len() as u64, Ordering::Relaxed);
    progress
        .bytes_found
        .store(items.iter().map(|item| item.bytes).sum(), Ordering::Relaxed);
    progress
        .passed
        .store(summary.passed.len() as u64, Ordering::Relaxed);

    for item in items {
        if progress.is_cancelled() {
            summary.cancelled = true;
            break;
        }
        match land(&item, &into) {
            Ok(landed) => {
                progress.copied.fetch_add(1, Ordering::Relaxed);
                progress.bytes.fetch_add(landed.bytes, Ordering::Relaxed);
                summary.landed.push(landed);
            }
            Err(why) => {
                match why.is_a_refusal() {
                    true => progress.passed.fetch_add(1, Ordering::Relaxed),
                    false => progress.held.fetch_add(1, Ordering::Relaxed),
                };
                summary.passed.push(Passed {
                    from: item.from,
                    why,
                });
            }
        }
    }

    summary.stats = progress.snapshot();
    summary
}

fn gather(paths: &[PathBuf], passed: &mut Vec<Passed>) -> Vec<Item> {
    let mut items = Vec::new();
    let mut seen = AHashSet::new();

    for path in paths {
        let Ok(metadata) = fs::metadata(path) else {
            passed.push(Passed {
                from: path.clone(),
                why: Passing::SourceGone,
            });
            continue;
        };

        let name = path.file_name().map(PathBuf::from);
        let Some(name) = name else {
            passed.push(Passed {
                from: path.clone(),
                why: Passing::Unreadable,
            });
            continue;
        };

        if metadata.is_dir() {
            let before = items.len();
            walk(path, &name, 0, &mut items, &mut seen);
            if items.len() == before {
                passed.push(Passed {
                    from: path.clone(),
                    why: Passing::NothingInside,
                });
            }
        } else if is_taken(path) {
            keep(path, name, &metadata, &mut items, &mut seen);
        } else {
            passed.push(Passed {
                from: path.clone(),
                why: Passing::NotAudio,
            });
        }
    }

    items
}

fn keep(
    from: &Path,
    relative: PathBuf,
    metadata: &Metadata,
    items: &mut Vec<Item>,
    seen: &mut AHashSet<PathBuf>,
) {
    let identity = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    if seen.insert(identity) {
        items.push(Item {
            from: from.to_path_buf(),
            relative,
            bytes: metadata.len(),
        });
    }
}

fn walk(
    folder: &Path,
    relative: &Path,
    depth: usize,
    items: &mut Vec<Item>,
    seen: &mut AHashSet<PathBuf>,
) {
    if depth >= DEEPEST_FOLDER {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let below = relative.join(&name);

        if kind.is_dir() {
            walk(&path, &below, depth + 1, items, seen);
        } else if kind.is_file()
            && is_taken(&path)
            && let Ok(metadata) = entry.metadata()
        {
            keep(&path, below, &metadata, items, seen);
        }
    }
}

fn land(item: &Item, into: &Path) -> std::result::Result<Landed, Passing> {
    let source = item.from.canonicalize().map_err(|_| Passing::SourceGone)?;
    if source.starts_with(into) {
        return Err(Passing::AlreadyThere);
    }

    let whole = into.join(&item.relative);
    let folder = whole.parent().unwrap_or(into);
    fs::create_dir_all(folder).map_err(|error| unwritable(&error))?;

    let staged = staged_beside(&whole);
    fs::copy(&source, &staged).map_err(|error| refused_copy(&error))?;

    let outcome = verified(&source, &staged)
        .and_then(|()| File::open(&staged).and_then(|file| file.sync_all()))
        .map_err(|_| Passing::Unverified)
        .and_then(|()| place(&staged, &source, &whole));
    let _ = fs::remove_file(&staged);

    outcome.map(|(to, renamed)| Landed {
        from: item.from.clone(),
        to,
        bytes: item.bytes,
        renamed,
    })
}

fn unwritable(error: &io::Error) -> Passing {
    tracing::warn!(%error, "a folder for a dropped file could not be made");
    Passing::Unwritable
}

fn refused_copy(error: &io::Error) -> Passing {
    tracing::warn!(%error, "a dropped file could not be copied");
    match error.kind() {
        io::ErrorKind::NotFound => Passing::SourceGone,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => Passing::Unwritable,
        _ => Passing::Unreadable,
    }
}

fn staged_beside(whole: &Path) -> PathBuf {
    let name = whole.file_name().map(OsString::from).unwrap_or_default();
    let mut staged = OsString::from(".");
    staged.push(name);
    staged.push(format!(".{}.{STAGED_SUFFIX}", process::id()));
    whole.with_file_name(staged)
}

fn candidates(whole: &Path) -> impl Iterator<Item = PathBuf> {
    let stem = whole.file_stem().map(OsString::from).unwrap_or_default();
    let extension = whole.extension().map(OsString::from);
    let original = whole.to_path_buf();

    std::iter::once(original.clone()).chain((2..=CANDIDATE_NAMES).map(move |count| {
        let mut name = stem.clone();
        name.push(format!(" ({count})"));
        if let Some(extension) = &extension {
            name.push(".");
            name.push(extension);
        }
        original.with_file_name(name)
    }))
}

fn place(
    staged: &Path,
    source: &Path,
    whole: &Path,
) -> std::result::Result<(PathBuf, bool), Passing> {
    for (count, candidate) in candidates(whole).enumerate() {
        if let Ok(standing) = fs::metadata(&candidate) {
            let same = standing.len() == fs::metadata(source).map_or(u64::MAX, |held| held.len())
                && verified(source, &candidate).is_ok();
            if same {
                return Err(Passing::AlreadyHeld);
            }
            continue;
        }
        match fs::hard_link(staged, &candidate) {
            Ok(()) => return Ok((candidate, count > 0)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                if candidate.exists() {
                    continue;
                }
                fs::rename(staged, &candidate).map_err(|error| unwritable(&error))?;
                return Ok((candidate, count > 0));
            }
        }
    }
    Err(Passing::Unwritable)
}

fn verified(one: &Path, other: &Path) -> io::Result<()> {
    let mut one = File::open(one)?;
    let mut other = File::open(other)?;
    let mut held = vec![0_u8; COMPARED_AT_ONCE];
    let mut copied = vec![0_u8; COMPARED_AT_ONCE];

    loop {
        let read = read_full(&mut one, &mut held)?;
        let again = read_full(&mut other, &mut copied)?;
        if read != again || held[..read] != copied[..again] {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        if read == 0 {
            return Ok(());
        }
    }
}

fn read_full(file: &mut File, into: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < into.len() {
        match file.read(&mut into[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("resonate-take-in-{name}-{}", process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("a writable temporary directory");
            Self(root)
        }

        fn folder(&self, name: &str) -> PathBuf {
            let folder = self.0.join(name);
            fs::create_dir_all(&folder).expect("a writable temporary directory");
            folder
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn written(folder: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        fs::write(&path, bytes).expect("a writable temporary directory");
        path
    }

    fn taken_in(paths: Vec<PathBuf>, into: &Path) -> TakeInSummary {
        take_in(TakeInOptions {
            paths,
            into: into.to_path_buf(),
        })
        .expect("a folder to copy into")
        .join()
        .expect("the pass finishes")
    }

    #[test]
    fn a_file_is_copied_whole_and_the_original_is_left_where_it_was() {
        let scratch = Scratch::new("whole");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        let song = written(&from, "song.flac", &[7; 700_000]);

        let summary = taken_in(vec![song.clone()], &into);

        assert_eq!(summary.stats.copied, 1);
        assert_eq!(summary.stats.bytes, 700_000);
        assert_eq!(
            summary.landed[0].to,
            into.canonicalize().expect("there").join("song.flac")
        );
        assert_eq!(
            fs::read(into.join("song.flac")).expect("copied"),
            vec![7; 700_000]
        );
        assert!(song.exists());
    }

    #[test]
    fn a_folder_keeps_its_name_and_shape_and_only_what_the_library_reads() {
        let scratch = Scratch::new("folder");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        written(&from, "Album/01.flac", b"one");
        written(&from, "Album/Disc 2/02.mp3", b"two");
        written(&from, "Album/Album.cue", b"sheet");
        written(&from, "Album/notes.txt", b"words");
        written(&from, "Album/.hidden.flac", b"hidden");

        let summary = taken_in(vec![from.join("Album")], &into);

        assert_eq!(summary.stats.copied, 3);
        assert!(into.join("Album/01.flac").is_file());
        assert!(into.join("Album/Disc 2/02.mp3").is_file());
        assert!(into.join("Album/Album.cue").is_file());
        assert!(!into.join("Album/notes.txt").exists());
        assert!(!into.join("Album/.hidden.flac").exists());
    }

    #[test]
    fn a_byte_for_byte_copy_already_there_is_held_and_a_different_one_is_kept_beside_it() {
        let scratch = Scratch::new("collide");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        let song = written(&from, "song.flac", b"this one");
        written(&into, "song.flac", b"this one");

        let held = taken_in(vec![song.clone()], &into);

        assert_eq!(held.stats.copied, 0);
        assert_eq!(held.stats.held, 1);
        assert_eq!(held.passed[0].why, Passing::AlreadyHeld);

        fs::write(into.join("song.flac"), b"another song").expect("writable");
        let beside = taken_in(vec![song], &into);

        assert_eq!(beside.stats.copied, 1);
        assert!(beside.landed[0].renamed);
        assert!(into.join("song (2).flac").is_file());
        assert_eq!(
            fs::read(into.join("song.flac")).expect("untouched"),
            b"another song"
        );
    }

    #[test]
    fn a_file_already_inside_the_folder_is_not_copied_onto_itself() {
        let scratch = Scratch::new("inside");
        let into = scratch.folder("music");
        let song = written(&into, "Artist/song.flac", b"here");

        let summary = taken_in(vec![song], &into);

        assert_eq!(summary.stats.copied, 0);
        assert_eq!(summary.passed[0].why, Passing::AlreadyThere);
        assert!(!into.join("Artist/song (2).flac").exists());
    }

    #[test]
    fn what_is_not_audio_or_not_there_is_passed_over_with_its_reason() {
        let scratch = Scratch::new("passed");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        let text = written(&from, "notes.txt", b"words");
        let empty = scratch.folder("from/empty");

        let summary = taken_in(vec![text, empty, from.join("gone.flac")], &into);

        let why: Vec<_> = summary.passed.iter().map(|passed| passed.why).collect();
        assert_eq!(
            why,
            [
                Passing::NotAudio,
                Passing::NothingInside,
                Passing::SourceGone
            ]
        );
        assert_eq!(summary.stats.copied, 0);
        assert!(fs::read_dir(&into).expect("there").next().is_none());
    }

    #[test]
    fn nothing_is_left_staged_and_a_missing_folder_is_refused_before_a_thread_starts() {
        let scratch = Scratch::new("staged");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        let song = written(&from, "song.wav", b"riff");

        taken_in(vec![song.clone()], &into);
        let names: Vec<_> = fs::read_dir(&into)
            .expect("there")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        let refused = take_in(TakeInOptions {
            paths: vec![song],
            into: scratch.0.join("nowhere"),
        });

        assert_eq!(names, [OsString::from("song.wav")]);
        assert!(matches!(
            refused,
            Err(Error::DestinationNotADirectory { .. })
        ));
    }

    #[test]
    fn a_drag_is_weighed_by_what_each_path_looks_like() {
        let scratch = Scratch::new("weigh");
        let from = scratch.folder("from");
        let looks: Vec<_> = weigh(&[
            written(&from, "a.FLAC", b"x"),
            written(&from, "b.cue", b"x"),
            written(&from, "c.jpg", b"x"),
            scratch.folder("from/dir"),
            from.join("gone.mp3"),
        ])
        .into_iter()
        .map(|dropped| dropped.looks)
        .collect();

        assert_eq!(
            looks,
            [
                Looks::Audio,
                Looks::Sheet,
                Looks::Other,
                Looks::Folder,
                Looks::Gone
            ]
        );
    }
}
