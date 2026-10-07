use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use ahash::{AHashMap, AHashSet};
use resonate_codec::renamed_cue;
use resonate_core::{names_a_picture, names_audio, naming};

use crate::{
    Error, Result,
    pass::{Cancelling, PassHandle, PassKind, TakeInHandle},
};

const SHEET_EXTENSION: &str = "cue";

const LYRIC_ENDINGS: [&str; 3] = [".lrc", ".lyricsfile.yaml", ".lyricsfile.yml"];
const LYRIC_FOLDERS: [&str; 3] = ["lyrics", "lyric", "lrc"];

const LARGEST_SHEET_REWRITTEN: u64 = 1 << 20;

const DEEPEST_FOLDER: usize = 32;

const COMPARED_AT_ONCE: usize = 256 * 1024;

const CANDIDATE_NAMES: u32 = 999;

const STAGED_SUFFIX: &str = "resonate-part";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Looks {
    Audio,
    Sheet,
    Companion,
    Folder,
    Other,
    Gone,
}

impl Looks {
    pub const fn is_taken(self) -> bool {
        matches!(
            self,
            Self::Audio | Self::Sheet | Self::Companion | Self::Folder
        )
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

fn is_a_lyric_sheet(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            LYRIC_ENDINGS.iter().any(|ending| name.ends_with(ending))
        })
}

fn is_a_lyric_folder(folder: &Path) -> bool {
    folder
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            LYRIC_FOLDERS
                .iter()
                .any(|folder| name.eq_ignore_ascii_case(folder))
        })
}

fn is_named_after(path: &Path, audio: &Path) -> bool {
    let (Some(name), Some(stem)) = (path.file_name(), audio.file_stem()) else {
        return false;
    };
    let (name, stem) = (name.as_encoded_bytes(), stem.as_encoded_bytes());
    name.len() > stem.len() && name.starts_with(stem) && name[stem.len()] == b'.'
}

fn accompanies(path: &Path, audio: &[&Path]) -> bool {
    let beside: Vec<&Path> = audio
        .iter()
        .copied()
        .filter(|audio| audio.parent() == path.parent())
        .collect();

    !beside.is_empty()
        && (names_a_picture(path)
            || is_a_lyric_sheet(path)
            || beside.iter().any(|audio| is_named_after(path, audio)))
}

fn looks_of(path: &Path) -> Looks {
    match fs::metadata(path) {
        Err(_) => Looks::Gone,
        Ok(metadata) if metadata.is_dir() => Looks::Folder,
        Ok(_) if names_audio(path) => Looks::Audio,
        Ok(_) if is_a_sheet(path) => Looks::Sheet,
        Ok(_) => Looks::Other,
    }
}

pub fn weigh(paths: &[PathBuf]) -> Vec<Dropped> {
    let looks: Vec<Looks> = paths.iter().map(|path| looks_of(path)).collect();
    let audio: Vec<&Path> = paths
        .iter()
        .zip(&looks)
        .filter(|(_, looks)| **looks == Looks::Audio)
        .map(|(path, _)| path.as_path())
        .collect();

    paths
        .iter()
        .zip(looks)
        .map(|(path, looks)| Dropped {
            path: path.clone(),
            looks: match looks {
                Looks::Other if accompanies(path, &audio) => Looks::Companion,
                looks => looks,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Audio,
    Sheet,
    Companion,
}

struct Item {
    from: PathBuf,
    relative: PathBuf,
    bytes: u64,
    role: Role,
}

#[derive(Clone)]
struct Renamed {
    from: OsString,
    to: OsString,
}

#[derive(Default)]
struct Renames(AHashMap<PathBuf, Vec<Renamed>>);

impl Renames {
    fn note(&mut self, from: &Path, to: &Path) {
        let (Some(folder), Some(was), Some(is)) = (from.parent(), from.file_name(), to.file_name())
        else {
            return;
        };
        if was != is {
            self.0
                .entry(folder.to_path_buf())
                .or_default()
                .push(Renamed {
                    from: was.to_os_string(),
                    to: is.to_os_string(),
                });
        }
    }

    fn in_folder(&self, folder: Option<&Path>) -> impl Iterator<Item = &Renamed> {
        folder
            .and_then(|folder| self.0.get(folder))
            .into_iter()
            .flatten()
    }

    fn beside(&self, item: &Item) -> Vec<Renamed> {
        let folder = item.from.parent();
        let above = folder
            .filter(|folder| is_a_lyric_folder(folder) && is_a_lyric_sheet(&item.from))
            .and_then(Path::parent);

        self.in_folder(folder)
            .chain(self.in_folder(above))
            .cloned()
            .collect()
    }
}

enum Content {
    Whole(PathBuf),
    Rewritten(Vec<u8>),
}

impl Content {
    fn is_at(&self, candidate: &Path) -> bool {
        let Ok(standing) = fs::metadata(candidate) else {
            return false;
        };
        match self {
            Self::Whole(source) => {
                fs::metadata(source).is_ok_and(|held| held.len() == standing.len())
                    && verified(source, candidate).is_ok()
            }
            Self::Rewritten(bytes) => {
                bytes.len() as u64 == standing.len()
                    && fs::read(candidate).is_ok_and(|read| read == *bytes)
            }
        }
    }

    fn staged_at(&self, staged: &Path) -> std::result::Result<u64, Passing> {
        match self {
            Self::Whole(source) => {
                let bytes = fs::copy(source, staged).map_err(|error| refused_copy(&error))?;
                verified(source, staged).map_err(|_| Passing::Unverified)?;
                Ok(bytes)
            }
            Self::Rewritten(bytes) => {
                fs::write(staged, bytes).map_err(|error| unwritable(&error))?;
                if !self.is_at(staged) {
                    return Err(Passing::Unverified);
                }
                Ok(bytes.len() as u64)
            }
        }
    }
}

enum Stood {
    Landed(Landed),
    Held(PathBuf),
}

fn run(options: &TakeInOptions, progress: &TakeInProgress) -> TakeInSummary {
    let mut summary = TakeInSummary::default();
    let into = options
        .into
        .canonicalize()
        .unwrap_or_else(|_| options.into.clone());

    let mut items = gather(&options.paths, &mut summary.passed);
    items.sort_by_key(|item| item.role != Role::Audio);
    progress.found.store(items.len() as u64, Ordering::Relaxed);
    progress
        .bytes_found
        .store(items.iter().map(|item| item.bytes).sum(), Ordering::Relaxed);
    progress
        .passed
        .store(summary.passed.len() as u64, Ordering::Relaxed);

    let mut renames = Renames::default();
    for item in items {
        if progress.is_cancelled() {
            summary.cancelled = true;
            break;
        }

        let (relative, content) = aimed(&item, &renames.beside(&item));
        match land(&item, &relative, &content, &into) {
            Ok(Stood::Landed(landed)) => {
                if item.role == Role::Audio {
                    renames.note(&item.from, &landed.to);
                }
                progress.copied.fetch_add(1, Ordering::Relaxed);
                progress.bytes.fetch_add(landed.bytes, Ordering::Relaxed);
                summary.landed.push(landed);
            }
            Ok(Stood::Held(at)) => {
                if item.role == Role::Audio {
                    renames.note(&item.from, &at);
                }
                progress.held.fetch_add(1, Ordering::Relaxed);
                summary.passed.push(Passed {
                    from: item.from,
                    why: Passing::AlreadyHeld,
                });
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

fn aimed(item: &Item, renamed: &[Renamed]) -> (PathBuf, Content) {
    let whole = Content::Whole(item.from.clone());
    if item.role == Role::Audio || renamed.is_empty() {
        return (item.relative.clone(), whole);
    }

    let relative = following_its_audio(&item.relative, renamed);
    let content = match item.role {
        Role::Sheet if item.bytes <= LARGEST_SHEET_REWRITTEN => {
            sheet_naming(&item.from, renamed).map_or(whole, Content::Rewritten)
        }
        _ => whole,
    };
    (relative, content)
}

fn following_its_audio(relative: &Path, renamed: &[Renamed]) -> PathBuf {
    let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
        return relative.to_path_buf();
    };

    let followed = renamed
        .iter()
        .filter_map(|renamed| {
            let was = Path::new(&renamed.from).file_stem()?.to_str()?;
            let is = Path::new(&renamed.to).file_stem()?.to_str()?;
            let rest = name.strip_prefix(was)?;
            rest.starts_with('.').then_some((was.len(), is, rest))
        })
        .max_by_key(|(stem, ..)| *stem);

    followed.map_or_else(
        || relative.to_path_buf(),
        |(_, is, rest)| relative.with_file_name(format!("{is}{rest}")),
    )
}

fn sheet_naming(sheet: &Path, renamed: &[Renamed]) -> Option<Vec<u8>> {
    let held = fs::read(sheet).ok()?;
    let mut rewritten = None;
    for renamed in renamed {
        let (Some(from), Some(to)) = (renamed.from.to_str(), renamed.to.to_str()) else {
            continue;
        };
        let current = rewritten.as_ref().unwrap_or(&held);
        if let Some(named) = renamed_cue(current, from, to) {
            rewritten = Some(named);
        }
    }
    rewritten
}

fn gather(paths: &[PathBuf], passed: &mut Vec<Passed>) -> Vec<Item> {
    let mut items = Vec::new();
    let mut seen = AHashSet::new();

    for Dropped { path, looks } in weigh(paths) {
        let why = match looks {
            Looks::Gone => Some(Passing::SourceGone),
            Looks::Other => Some(Passing::NotAudio),
            _ => None,
        };
        if let Some(why) = why {
            passed.push(Passed { from: path, why });
            continue;
        }

        let Some(name) = path.file_name().map(PathBuf::from) else {
            passed.push(Passed {
                from: path,
                why: Passing::Unreadable,
            });
            continue;
        };

        let role = match looks {
            Looks::Audio => Role::Audio,
            Looks::Sheet => Role::Sheet,
            Looks::Companion => Role::Companion,
            _ => {
                if !walked_whole(&path, &name, &mut items, &mut seen) {
                    passed.push(Passed {
                        from: path,
                        why: Passing::NothingInside,
                    });
                }
                continue;
            }
        };
        match fs::metadata(&path) {
            Ok(metadata) => keep(
                Item {
                    from: path,
                    relative: name,
                    bytes: metadata.len(),
                    role,
                },
                &mut items,
                &mut seen,
            ),
            Err(_) => passed.push(Passed {
                from: path,
                why: Passing::SourceGone,
            }),
        }
    }

    items
}

fn keep(item: Item, items: &mut Vec<Item>, seen: &mut AHashSet<PathBuf>) {
    let identity = item
        .from
        .canonicalize()
        .unwrap_or_else(|_| item.from.clone());
    if seen.insert(identity) {
        items.push(item);
    }
}

fn walked_whole(
    folder: &Path,
    name: &Path,
    items: &mut Vec<Item>,
    seen: &mut AHashSet<PathBuf>,
) -> bool {
    let mut found = Vec::new();
    walk(folder, name, 0, &mut found);
    if !found.iter().any(|item| item.role != Role::Companion) {
        return false;
    }

    let before = items.len();
    for item in found {
        keep(item, items, seen);
    }
    items.len() > before
}

fn walk(folder: &Path, relative: &Path, depth: usize, found: &mut Vec<Item>) {
    if depth >= DEEPEST_FOLDER {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);

    let mut files = Vec::new();
    for entry in entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let below = relative.join(&name);

        if kind.is_dir() {
            walk(&entry.path(), &below, depth + 1, found);
        } else if kind.is_file()
            && let Ok(metadata) = entry.metadata()
        {
            files.push((entry.path(), below, metadata.len()));
        }
    }

    let audio: Vec<PathBuf> = files
        .iter()
        .filter(|(path, ..)| names_audio(path))
        .map(|(path, ..)| path.clone())
        .collect();
    for (path, relative, bytes) in files {
        let role = if names_audio(&path) {
            Role::Audio
        } else if is_a_sheet(&path) {
            Role::Sheet
        } else if names_a_picture(&path)
            || is_a_lyric_sheet(&path)
            || audio.iter().any(|audio| is_named_after(&path, audio))
        {
            Role::Companion
        } else {
            continue;
        };
        found.push(Item {
            from: path,
            relative,
            bytes,
            role,
        });
    }
}

fn land(
    item: &Item,
    relative: &Path,
    content: &Content,
    into: &Path,
) -> std::result::Result<Stood, Passing> {
    let source = item.from.canonicalize().map_err(|_| Passing::SourceGone)?;
    if source.starts_with(into) {
        return Err(Passing::AlreadyThere);
    }
    let content = match content {
        Content::Whole(_) => &Content::Whole(source),
        rewritten @ Content::Rewritten(_) => rewritten,
    };

    let whole = into.join(relative);
    let folder = whole.parent().unwrap_or(into);
    fs::create_dir_all(folder).map_err(|error| unwritable(&error))?;

    let staged = staged_beside(&whole);
    let outcome = content.staged_at(&staged).and_then(|bytes| {
        File::open(&staged)
            .and_then(|file| file.sync_all())
            .map_err(|_| Passing::Unverified)?;
        place(&staged, content, &whole).map(|stood| (stood, bytes))
    });
    let _ = fs::remove_file(&staged);

    outcome.map(|(stood, bytes)| match stood {
        Placed::Named { to, renamed } => Stood::Landed(Landed {
            from: item.from.clone(),
            to,
            bytes,
            renamed,
        }),
        Placed::Held(at) => Stood::Held(at),
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
    whole.with_file_name(naming::named_within(
        ".",
        whole.file_name().unwrap_or_default(),
        format!(".{}.{STAGED_SUFFIX}", process::id()),
    ))
}

pub(crate) fn candidates(whole: &Path) -> impl Iterator<Item = PathBuf> {
    let stem = whole.file_stem().map(OsString::from).unwrap_or_default();
    let extension = whole.extension().map(OsString::from);
    let original = whole.to_path_buf();

    std::iter::once(original.clone()).chain((2..=CANDIDATE_NAMES).map(move |count| {
        let mut after = OsString::from(format!(" ({count})"));
        if let Some(extension) = &extension {
            after.push(".");
            after.push(extension);
        }
        original.with_file_name(naming::named_within("", &stem, after))
    }))
}

enum Placed {
    Named { to: PathBuf, renamed: bool },
    Held(PathBuf),
}

fn place(staged: &Path, content: &Content, whole: &Path) -> std::result::Result<Placed, Passing> {
    for (count, candidate) in candidates(whole).enumerate() {
        let renamed = count > 0;
        if fs::symlink_metadata(&candidate).is_ok() {
            if content.is_at(&candidate) {
                return Ok(Placed::Held(candidate));
            }
            continue;
        }
        match fs::hard_link(staged, &candidate) {
            Ok(()) => {
                return Ok(Placed::Named {
                    to: candidate,
                    renamed,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                if candidate.exists() {
                    continue;
                }
                fs::rename(staged, &candidate).map_err(|error| unwritable(&error))?;
                return Ok(Placed::Named {
                    to: candidate,
                    renamed,
                });
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
    fn a_name_at_the_limit_is_staged_and_kept_beside_another_under_a_name_that_fits() {
        let scratch = Scratch::new("longest");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        let name = format!("{}.flac", "ü".repeat(125));
        assert_eq!(name.len(), 255);
        let song = written(&from, &name, b"this one");
        written(&into, &name, b"another song");

        let summary = taken_in(vec![song], &into);

        assert_eq!(summary.stats.copied, 1, "{:?}", summary.passed);
        let landed = &summary.landed[0].to;
        let landed_name = landed.file_name().expect("a named file");
        assert!(landed_name.len() <= 255);
        assert!(
            landed_name
                .to_str()
                .is_some_and(|named| named.ends_with(" (2).flac")),
            "{landed_name:?}"
        );
        assert_eq!(fs::read(landed).expect("copied"), b"this one");
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
    fn a_folders_covers_lyrics_and_named_sidecars_come_with_its_audio() {
        let scratch = Scratch::new("companions");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        written(&from, "Album/CD1/01.flac", b"one");
        written(&from, "Album/CD1/01.lrc", b"[00:01.00]words");
        written(&from, "Album/CD1/01.txt", b"words");
        written(&from, "Album/cover.jpg", b"picture");
        written(&from, "Album/Scans/back.PNG", b"picture");
        written(&from, "Album/Album.log", b"a rip log naming no audio");
        written(&from, "Album/notes.txt", b"words");
        written(&from, "Pictures/front.jpg", b"picture");

        let summary = taken_in(vec![from.join("Album"), from.join("Pictures")], &into);

        assert_eq!(summary.stats.copied, 5);
        for landed in [
            "Album/CD1/01.flac",
            "Album/CD1/01.lrc",
            "Album/CD1/01.txt",
            "Album/cover.jpg",
            "Album/Scans/back.PNG",
        ] {
            assert!(into.join(landed).is_file(), "{landed} was left behind");
        }
        assert!(!into.join("Album/Album.log").exists());
        assert!(!into.join("Album/notes.txt").exists());
        assert!(!into.join("Pictures").exists());
        assert_eq!(summary.passed[0].why, Passing::NothingInside);
    }

    #[test]
    fn what_travels_with_audio_that_landed_under_a_new_name_follows_that_name() {
        let scratch = Scratch::new("follow");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        written(&from, "Album/Album.flac", b"this rip");
        written(
            &from,
            "Album/Album.cue",
            b"FILE \"Album.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    INDEX 01 00:00:00\r\n",
        );
        written(&from, "Album/Album.lrc", b"[00:01.00]words");
        written(&from, "Album/cover.jpg", b"picture");
        written(&into, "Album/Album.flac", b"another rip");
        written(&into, "Album/cover.jpg", b"picture");

        let summary = taken_in(vec![from.join("Album")], &into);

        assert_eq!(summary.stats.copied, 3);
        assert_eq!(summary.stats.held, 1);
        assert_eq!(
            fs::read(into.join("Album/Album (2).flac")).expect("copied beside"),
            b"this rip"
        );
        assert_eq!(
            fs::read(into.join("Album/Album (2).cue")).expect("the sheet followed"),
            b"FILE \"Album (2).flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    INDEX 01 00:00:00\r\n"
        );
        assert!(into.join("Album/Album (2).lrc").is_file());
        assert!(!into.join("Album/Album.cue").exists());
        assert!(!into.join("Album/cover (2).jpg").exists());
        assert_eq!(
            fs::read(from.join("Album/Album.cue")).expect("untouched"),
            b"FILE \"Album.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    INDEX 01 00:00:00\r\n"
        );
    }

    #[test]
    fn a_lyric_in_a_lyrics_folder_follows_the_name_its_track_landed_under() {
        let scratch = Scratch::new("lyrics-folder");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        written(&from, "Album/01 Song.flac", b"this song");
        written(&from, "Album/lyrics/01 Song.lrc", b"[00:01.00]words");
        written(&from, "Album/other/01 Song.lrc", b"[00:01.00]elsewhere");
        written(&into, "Album/01 Song.flac", b"another song");

        let summary = taken_in(vec![from.join("Album")], &into);

        assert_eq!(
            fs::read(into.join("Album/01 Song (2).flac")).expect("copied beside"),
            b"this song"
        );
        assert!(into.join("Album/lyrics/01 Song (2).lrc").is_file());
        assert!(!into.join("Album/lyrics/01 Song.lrc").exists());
        assert!(
            into.join("Album/other/01 Song.lrc").is_file(),
            "a folder not named for lyrics was followed all the same"
        );
        assert_eq!(summary.stats.copied, 3);
    }

    #[test]
    fn a_sheet_naming_audio_already_held_under_a_new_name_is_written_naming_that_name() {
        let scratch = Scratch::new("held-follow");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        written(&from, "Album/CDImage.wav", b"this rip");
        written(&from, "Album/Rip.cue", b"FILE \"CDImage.wav\" WAVE\n");
        written(&into, "Album/CDImage.wav", b"another rip");
        written(&into, "Album/CDImage (2).wav", b"this rip");

        let summary = taken_in(vec![from.join("Album")], &into);

        assert_eq!(summary.stats.held, 1);
        assert_eq!(
            fs::read(into.join("Album/Rip.cue")).expect("the sheet landed"),
            b"FILE \"CDImage (2).wav\" WAVE\n"
        );
    }

    #[test]
    fn a_loose_cover_or_lyric_dropped_beside_its_song_is_taken_and_alone_is_not() {
        let scratch = Scratch::new("loose");
        let (from, into) = (scratch.folder("from"), scratch.folder("music"));
        let song = written(&from, "song.flac", b"audio");
        let words = written(&from, "song.lrc", b"[00:01.00]words");
        let cover = written(&from, "cover.jpg", b"picture");
        let elsewhere = written(&scratch.folder("else"), "cover.jpg", b"picture");

        let looks: Vec<_> = weigh(&[
            song.clone(),
            words.clone(),
            cover.clone(),
            elsewhere.clone(),
        ])
        .into_iter()
        .map(|dropped| dropped.looks)
        .collect();
        let alone = weigh(std::slice::from_ref(&cover));
        let summary = taken_in(vec![song, words, cover, elsewhere], &into);

        assert_eq!(
            looks,
            [
                Looks::Audio,
                Looks::Companion,
                Looks::Companion,
                Looks::Other
            ]
        );
        assert_eq!(alone[0].looks, Looks::Other);
        assert_eq!(summary.stats.copied, 3);
        assert!(into.join("song.lrc").is_file());
        assert!(into.join("cover.jpg").is_file());
        assert_eq!(summary.passed[0].why, Passing::NotAudio);
    }

    #[test]
    fn a_drag_is_weighed_by_what_each_path_looks_like() {
        let scratch = Scratch::new("weigh");
        let from = scratch.folder("from");
        let looks: Vec<_> = weigh(&[
            written(&from, "a.FLAC", b"x"),
            written(&from, "b.cue", b"x"),
            written(&from, "c.txt", b"x"),
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
