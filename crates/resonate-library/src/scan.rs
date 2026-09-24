use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, Metadata},
    num::{NonZeroU8, NonZeroU32, NonZeroUsize},
    path::{MAIN_SEPARATOR, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet};
use crossbeam_channel::{Receiver, Sender, bounded};
use resonate_codec::{
    Codec, CueFile, CueSheet, MediaInfo, Picturing, Sources, TagSet, probe_pictured, read_cue,
};
use resonate_core::{MediaLocation, TrackId};
use rusqlite::params;

use crate::{
    Error, Result, StoreOp, alternatives,
    db::Inner,
    enriched::stripped_title,
    moves,
    pass::{Cancelling, PassHandle, PassKind, ScanHandle},
    stem,
    store::{self, Cache, TrackRecord},
};

const MAX_DEPTH: NonZeroU8 = match NonZeroU8::new(32) {
    Some(depth) => depth,
    None => panic!("the depth limit must be non-zero"),
};

const PAST_THE_SEPARATOR: char = match char::from_u32(MAIN_SEPARATOR as u32 + 1) {
    Some(next) => next,
    None => panic!("the path separator is followed by a character"),
};

const BATCH: usize = 1_000;
const QUEUE: usize = 1_024;

const SHEET_EXTENSION: &str = "cue";

const LARGEST_SHEET_ON_DISC: u64 = 1 << 20;

pub(crate) const AUDIO_EXTENSIONS: &[&str] = &[
    "aac", "aif", "aiff", "caf", "dff", "dsf", "flac", "m4a", "m4b", "mka", "mp3", "mp4", "oga",
    "ogg", "opus", "wav", "wave",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub incremental: bool,
    pub follow_symlinks: bool,
    pub extract_cover_art: bool,
    pub workers: NonZeroUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Failure {
    Unnamed,
    Misnamed,
    Undecodable,
    Unreadable,
}

impl Failure {
    pub const ALL: [Self; 4] = [
        Self::Unnamed,
        Self::Misnamed,
        Self::Undecodable,
        Self::Unreadable,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unnamed => "unnamed",
            Self::Misnamed => "misnamed",
            Self::Undecodable => "undecodable",
            Self::Unreadable => "unreadable",
        }
    }

    fn of(error: &Error) -> Self {
        match error {
            Error::Tags { source, .. } => Self::of_codec(source),
            _ => Self::Unreadable,
        }
    }

    fn of_codec(error: &resonate_codec::Error) -> Self {
        use resonate_codec::Error as Codec;

        match error {
            Codec::UnrecognisedContainer { .. } | Codec::NoAudioTrack { .. } => Self::Misnamed,
            Codec::NoDecoder { .. }
            | Codec::DsdCompressed { .. }
            | Codec::TrackPropertyMissing { .. }
            | Codec::RateNotRepresentable { .. }
            | Codec::LayoutNotRepresentable { .. }
            | Codec::SampleFormatNotRepresentable { .. }
            | Codec::DsdChunkMissing { .. }
            | Codec::DsdFieldNotUsable { .. } => Self::Undecodable,
            Codec::Io { .. }
            | Codec::Symphonia { .. }
            | Codec::SheetTooLarge { .. }
            | Codec::NoSuchSource { .. }
            | Codec::LocatorNotUsable { .. }
            | Codec::UnknownDuration { .. }
            | Codec::SeekOutOfRange { .. }
            | Codec::NotSeekable { .. }
            | Codec::SeekBackwardUnsupported { .. }
            | Codec::SeekInvalidTrack { .. }
            | Codec::ResetRequired { .. }
            | Codec::Unwritable { .. }
            | Codec::TagsUnread { .. }
            | Codec::TagsUnwritten { .. }
            | Codec::Domain(_) => Self::Unreadable,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Failures {
    pub unnamed: u64,
    pub misnamed: u64,
    pub undecodable: u64,
    pub unreadable: u64,
}

impl Failures {
    pub const fn total(self) -> u64 {
        self.unnamed + self.misnamed + self.undecodable + self.unreadable
    }

    pub const fn counted(self, failure: Failure) -> u64 {
        match failure {
            Failure::Unnamed => self.unnamed,
            Failure::Misnamed => self.misnamed,
            Failure::Undecodable => self.undecodable,
            Failure::Unreadable => self.unreadable,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub discovered: u64,
    pub processed: u64,
    pub added: u64,
    pub updated: u64,
    pub moved: u64,
    pub removed: u64,
    pub failed: Failures,
}

#[derive(Debug, Default)]
struct FailureCounts {
    unnamed: AtomicU64,
    misnamed: AtomicU64,
    undecodable: AtomicU64,
    unreadable: AtomicU64,
}

impl FailureCounts {
    fn add(&self, failure: Failure, rows: u64) {
        let counted = match failure {
            Failure::Unnamed => &self.unnamed,
            Failure::Misnamed => &self.misnamed,
            Failure::Undecodable => &self.undecodable,
            Failure::Unreadable => &self.unreadable,
        };
        counted.fetch_add(rows, Ordering::Relaxed);
    }

    fn snapshot(&self) -> Failures {
        Failures {
            unnamed: self.unnamed.load(Ordering::Relaxed),
            misnamed: self.misnamed.load(Ordering::Relaxed),
            undecodable: self.undecodable.load(Ordering::Relaxed),
            unreadable: self.unreadable.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default)]
pub struct ScanProgress {
    discovered: AtomicU64,
    processed: AtomicU64,
    added: AtomicU64,
    updated: AtomicU64,
    moved: AtomicU64,
    removed: AtomicU64,
    failed: FailureCounts,
    cancelled: AtomicBool,
}

impl ScanProgress {
    pub fn snapshot(&self) -> ScanStats {
        ScanStats {
            discovered: self.discovered.load(Ordering::Relaxed),
            processed: self.processed.load(Ordering::Relaxed),
            added: self.added.load(Ordering::Relaxed),
            updated: self.updated.load(Ordering::Relaxed),
            moved: self.moved.load(Ordering::Relaxed),
            removed: self.removed.load(Ordering::Relaxed),
            failed: self.failed.snapshot(),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Cancelling for ScanProgress {
    fn cancel(&self) {
        ScanProgress::cancel(self);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub stats: ScanStats,
    pub cancelled: bool,
}

pub(crate) fn start(inner: Arc<Inner>, options: ScanOptions) -> Result<ScanHandle> {
    let walking = inner.walk_the_tree()?;
    let progress = Arc::new(ScanProgress::default());
    let owned = Arc::clone(&progress);

    let thread = thread::Builder::new()
        .name("resonate-scan".to_owned())
        .spawn(move || {
            let outcome = run(&inner, &options, &progress);
            drop(walking);
            outcome
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Scan, owned, thread))
}

struct Root {
    id: i64,
    path: PathBuf,
}

struct Stored {
    id: TrackId,
    file_size: u64,
    modified: SystemTime,
    sheet_modified: Option<SystemTime>,
}

#[derive(Default)]
struct Known {
    rows: AHashMap<String, Vec<Stored>>,
}

impl Known {
    fn under(inner: &Inner, roots: &[Root]) -> Result<Self> {
        let mut known = Self::default();
        inner.read(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT path, id, file_size, modified, sheet_modified FROM tracks
                     WHERE path >= ?1 AND path < ?2 ORDER BY path, span_start",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;

            for root in roots {
                let (from, past) = walked_from(store::path_text(&root.path)?);
                let rows = statement
                    .query_map(params![from, past], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                        ))
                    })
                    .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                    .map_err(|source| Error::store(StoreOp::Query, source))?;

                for (path, id, file_size, modified, sheet_modified) in rows {
                    known.rows.entry(path).or_default().push(Stored {
                        id: TrackId::new(id as u64)?,
                        file_size: file_size as u64,
                        modified: store::from_nanos(modified),
                        sheet_modified: sheet_modified.map(store::from_nanos),
                    });
                }
            }
            Ok(())
        })?;

        Ok(known)
    }

    fn rows(&self, path: &str) -> &[Stored] {
        self.rows.get(path).map_or(&[], Vec::as_slice)
    }
}

pub(crate) fn forget_the_gone(inner: &Arc<Inner>, named: &[PathBuf]) -> Result<u64> {
    let _walking = inner.walk_the_tree()?;
    inner.write(|transaction| {
        let mut gone: BTreeMap<i64, Vec<PathBuf>> = BTreeMap::new();
        for path in named {
            let text = store::path_text(path)?;
            let (from, past) = walked_from(text);
            for (root, held) in store::rooted_paths_at(transaction, text, &from, &past)? {
                if !held.exists() {
                    gone.entry(root).or_default().push(held);
                }
            }
        }

        let mut forgotten = 0;
        for (root, paths) in &gone {
            forgotten += store::forget_paths(transaction, *root, paths)?;
        }
        if forgotten > 0 {
            store::sweep_orphans(transaction)?;
            alternatives::settle(transaction)?;
        }
        Ok(forgotten)
    })
}

fn walked_from(root: &str) -> (String, String) {
    let mut from = root.trim_end_matches(MAIN_SEPARATOR).to_owned();
    from.push(MAIN_SEPARATOR);

    let mut past = from.clone();
    past.pop();
    past.push(PAST_THE_SEPARATOR);
    (from, past)
}

struct Candidate {
    root_id: i64,
    path: PathBuf,
    sleeve: Option<PathBuf>,
    existing: Vec<Option<TrackId>>,
    file_size: u64,
    modified: SystemTime,
}

struct SheetCandidate {
    root_id: i64,
    sleeve: Option<PathBuf>,
    sheet: PathBuf,
    file: PathBuf,
    cut: CueFile,
    existing: Vec<Option<TrackId>>,
    file_size: u64,
    modified: SystemTime,
    sheet_modified: SystemTime,
}

enum Job {
    Known(TrackId),
    Probe(Box<Candidate>),
    Sheet(Box<SheetCandidate>),
}

enum Outcome {
    Seen(TrackId),
    Store(Box<TrackRecord>),
}

fn run(
    inner: &Arc<Inner>,
    options: &ScanOptions,
    progress: &Arc<ScanProgress>,
) -> Result<ScanSummary> {
    let generation = store::to_nanos(SystemTime::now());
    let roots = roots(inner, &options.roots)?;
    let ids: Vec<i64> = roots.iter().map(|root| root.id).collect();
    let known = Known::under(inner, &roots)?;

    let (work_tx, work_rx) = bounded::<Job>(QUEUE);
    let (done_tx, done_rx) = bounded::<Outcome>(QUEUE);

    let mut workers = Vec::with_capacity(options.workers.get());
    for index in 0..options.workers.get() {
        let work_rx = work_rx.clone();
        let done_tx = done_tx.clone();
        let progress = Arc::clone(progress);
        workers.push(
            thread::Builder::new()
                .name(format!("resonate-probe-{index}"))
                .spawn(move || probe_all(&work_rx, &done_tx, &progress))
                .map_err(|source| Error::ThreadSpawn { source })?,
        );
    }
    drop(work_rx);
    drop(done_tx);

    let walker = {
        let options = options.clone();
        let progress = Arc::clone(progress);
        thread::Builder::new()
            .name("resonate-walk".to_owned())
            .spawn(move || {
                let outcome = walk_all(&known, &roots, &options, &progress, &work_tx);
                drop(work_tx);
                outcome
            })
            .map_err(|source| Error::ThreadSpawn { source })?
    };

    let written = write_all(inner, options, &done_rx, progress, generation);
    drop(done_rx);
    let mut lost = false;
    for worker in workers {
        lost |= worker.join().is_err();
    }
    let walked = walker.join().unwrap_or(Err(Error::Stopped {
        pass: PassKind::Scan,
    }));
    written?;
    walked?;
    if lost {
        return Err(Error::Stopped {
            pass: PassKind::Scan,
        });
    }

    let cancelled = progress.is_cancelled();
    if !cancelled {
        let moved =
            inner.write(|transaction| moves::follow_the_moved(transaction, &ids, generation))?;
        progress.moved.store(moved, Ordering::Relaxed);
        progress.added.fetch_sub(moved, Ordering::Relaxed);
        let removed = inner.write(|transaction| store::prune(transaction, &ids, generation))?;
        let tidied = tidy_the_roots_beside(inner, &ids, progress)?;
        progress.removed.store(removed + tidied, Ordering::Relaxed);
    }
    inner.write(alternatives::settle)?;
    if !cancelled {
        inner.restate_the_statistics();
    }

    Ok(ScanSummary {
        stats: progress.snapshot(),
        cancelled,
    })
}

fn roots(inner: &Inner, wanted: &[PathBuf]) -> Result<Vec<Root>> {
    inner.write(|transaction| {
        if wanted.is_empty() {
            let mut statement = transaction
                .prepare("SELECT id, path FROM roots ORDER BY path")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let found = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            return Ok(found
                .into_iter()
                .map(|(id, path)| Root {
                    id,
                    path: PathBuf::from(path),
                })
                .filter(is_there)
                .collect());
        }

        let mut resolved: Vec<Root> = Vec::with_capacity(wanted.len());
        for root in wanted {
            if !root.is_dir() {
                return Err(Error::RootNotADirectory { path: root.clone() });
            }
            let path = root.canonicalize().map_err(|source| Error::Io {
                path: root.clone(),
                source,
            })?;
            let id = store::register_root(transaction, &path)?;

            resolved.retain(|held| !held.path.starts_with(&path));
            if !resolved.iter().any(|held| held.id == id) {
                resolved.push(Root { id, path });
            }
        }
        Ok(resolved)
    })
}

fn is_there(root: &Root) -> bool {
    if root.path.is_dir() {
        return true;
    }
    tracing::warn!(
        path = %root.path.display(),
        "leaving a root that is not there alone rather than emptying it"
    );
    false
}

fn roots_beside(inner: &Inner, walked: &[i64]) -> Result<Vec<Root>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare("SELECT id, path FROM roots ORDER BY path")
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let found = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        Ok(found
            .into_iter()
            .filter(|(id, _)| !walked.contains(id))
            .map(|(id, path)| Root {
                id,
                path: PathBuf::from(path),
            })
            .collect())
    })
}

fn tidy_the_roots_beside(
    inner: &Arc<Inner>,
    walked: &[i64],
    progress: &ScanProgress,
) -> Result<u64> {
    let mut tidied = 0;
    for root in roots_beside(inner, walked)? {
        if progress.is_cancelled() {
            break;
        }
        if !is_there(&root) {
            continue;
        }

        let gone: Vec<PathBuf> = inner
            .read(|connection| store::paths_under(connection, root.id))?
            .into_iter()
            .filter(|path| !path.exists())
            .collect();
        if gone.is_empty() {
            continue;
        }

        tracing::debug!(
            path = %root.path.display(),
            files = gone.len(),
            "tidying a root this scan did not walk"
        );
        tidied += inner.write(|transaction| store::forget_paths(transaction, root.id, &gone))?;
    }

    if tidied > 0 {
        inner.write(store::sweep_orphans)?;
    }
    Ok(tidied)
}

fn walk_all(
    known: &Known,
    roots: &[Root],
    options: &ScanOptions,
    progress: &ScanProgress,
    work: &Sender<Job>,
) -> Result<()> {
    let mut visited = AHashSet::new();
    for root in roots {
        walk(known, root, options, progress, work, &mut visited)?;
    }
    Ok(())
}

fn walk(
    known: &Known,
    root: &Root,
    options: &ScanOptions,
    progress: &ScanProgress,
    work: &Sender<Job>,
    visited: &mut AHashSet<PathBuf>,
) -> Result<()> {
    let walking = Walking {
        known,
        root,
        options,
        progress,
        work,
    };
    let mut stack = vec![(root.path.clone(), 1_u8)];

    while let Some((directory, depth)) = stack.pop() {
        if progress.is_cancelled() {
            return Ok(());
        }
        if depth > MAX_DEPTH.get() {
            tracing::warn!(
                path = %directory.display(),
                limit = MAX_DEPTH.get(),
                "skipping a directory deeper than the walk reads"
            );
            continue;
        }

        let mut audio = Vec::new();
        let mut sheets = Vec::new();

        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, path = %directory.display(), "skipping an unreadable directory");
                continue;
            }
        };

        for entry in entries.flatten() {
            if progress.is_cancelled() {
                return Ok(());
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };

            if kind.is_symlink() && !options.follow_symlinks {
                continue;
            }
            if kind.is_dir() {
                stack.push((path, depth.saturating_add(1)));
                continue;
            }
            if kind.is_file() && !is_a_sheet(&path) && !is_audio(&path) {
                continue;
            }

            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "skipping an unreadable entry");
                    continue;
                }
            };

            if kind.is_symlink() && metadata.is_dir() && !followed(&path, visited) {
                continue;
            }
            if metadata.is_dir() {
                stack.push((path, depth.saturating_add(1)));
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            if is_a_sheet(&path) {
                sheets.push((path, metadata));
            } else if is_audio(&path) {
                audio.push((path, metadata));
            }
        }

        if !directory_of(&walking, &sheets, &audio)? {
            return Ok(());
        }
    }
    Ok(())
}

fn followed(path: &Path, visited: &mut AHashSet<PathBuf>) -> bool {
    let Ok(target) = path.canonicalize() else {
        return false;
    };
    if visited.insert(target) {
        return true;
    }

    tracing::debug!(
        path = %path.display(),
        "stepping past a link to a directory this walk has already been down"
    );
    false
}

struct Walking<'a> {
    known: &'a Known,
    root: &'a Root,
    options: &'a ScanOptions,
    progress: &'a ScanProgress,
    work: &'a Sender<Job>,
}

fn directory_of(
    walking: &Walking<'_>,
    sheets: &[(PathBuf, Metadata)],
    audio: &[(PathBuf, Metadata)],
) -> Result<bool> {
    let mut claimed = AHashSet::new();
    for (path, metadata) in sheets {
        let Some(sheet) = read_sheet(path) else {
            continue;
        };
        let touched = metadata.modified().unwrap_or(UNIX_EPOCH);

        for cut in &sheet.files {
            let Some(file) = beside(path, &cut.named) else {
                continue;
            };
            let Some(held) = audio.iter().find(|(named, _)| *named == file) else {
                tracing::debug!(
                    sheet = %path.display(),
                    file = %file.display(),
                    "a cue sheet names a file that is not beside it"
                );
                continue;
            };

            claimed.insert(file.clone());
            if !sheet_job(walking, cut, path, held, touched)? {
                return Ok(false);
            }
        }
    }

    for (path, metadata) in audio {
        if claimed.contains(path) {
            continue;
        }
        if !whole_file_job(walking, path, metadata)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn sheet_job(
    walking: &Walking<'_>,
    cut: &CueFile,
    sheet: &Path,
    held: &(PathBuf, Metadata),
    touched: SystemTime,
) -> Result<bool> {
    let Walking {
        known,
        root,
        options,
        progress,
        work,
    } = walking;
    let (file, metadata) = held;
    let tracks = cut.audio_tracks().count();
    if tracks == 0 {
        return Ok(true);
    }

    progress
        .discovered
        .fetch_add(tracks as u64, Ordering::Relaxed);
    let Some(text) = file.to_str() else {
        tracing::warn!(path = %file.display(), "skipping a path that is not UTF-8");
        progress.failed.add(Failure::Unnamed, tracks as u64);
        return Ok(true);
    };

    let file_size = metadata.len();
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);

    let rows = known.rows(text);
    let unchanged = rows.len() == tracks
        && rows.iter().all(|held| {
            held.file_size == file_size
                && held.modified == modified
                && held.sheet_modified == Some(touched)
        });

    if options.incremental && unchanged {
        for held in rows {
            if work.send(Job::Known(held.id)).is_err() {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    let mut existing: Vec<Option<TrackId>> = rows.iter().map(|held| Some(held.id)).collect();
    existing.resize(tracks, None);

    let job = Job::Sheet(Box::new(SheetCandidate {
        root_id: root.id,
        sleeve: sleeve(sheet, &root.path),
        sheet: sheet.to_path_buf(),
        file: file.clone(),
        cut: cut.clone(),
        existing,
        file_size,
        modified,
        sheet_modified: touched,
    }));
    Ok(work.send(job).is_ok())
}

fn whole_file_job(walking: &Walking<'_>, path: &Path, metadata: &Metadata) -> Result<bool> {
    let Walking {
        known,
        root,
        options,
        progress,
        work,
    } = walking;
    progress.discovered.fetch_add(1, Ordering::Relaxed);
    let Some(text) = path.to_str() else {
        tracing::warn!(path = %path.display(), "skipping a path that is not UTF-8");
        progress.failed.add(Failure::Unnamed, 1);
        return Ok(true);
    };

    let file_size = metadata.len();
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);

    let rows = known.rows(text);
    let unchanged = !rows.is_empty()
        && rows.iter().all(|held| {
            held.file_size == file_size
                && held.modified == modified
                && held.sheet_modified.is_none()
        });

    if options.incremental && unchanged {
        rows_past_the_first(progress, rows.len());
        for held in rows {
            if work.send(Job::Known(held.id)).is_err() {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    let job = Job::Probe(Box::new(Candidate {
        root_id: root.id,
        path: path.to_path_buf(),
        sleeve: sleeve(path, &root.path),
        existing: rows.iter().map(|held| Some(held.id)).collect(),
        file_size,
        modified,
    }));
    Ok(work.send(job).is_ok())
}

fn rows_past_the_first(progress: &ScanProgress, rows: usize) {
    progress
        .discovered
        .fetch_add(rows.saturating_sub(1) as u64, Ordering::Relaxed);
}

pub(crate) fn sleeve(path: &Path, root: &Path) -> Option<PathBuf> {
    let folder = path.parent()?;
    if folder == root {
        return None;
    }
    let one_disc = folder
        .file_name()
        .and_then(OsStr::to_str)
        .and_then(disc_in_folder)
        .is_some();

    if one_disc {
        return folder
            .parent()
            .filter(|up| *up != root)
            .map(Path::to_path_buf);
    }

    Some(folder.to_path_buf())
}

const SPELLINGS: [&str; 10] = [
    "cd", "disc", "disco", "disk", "disque", "dysk", "platte", "schijf", "skiva", "диск",
];

const ELSEWHERE: [(&str, u32); 68] = [
    ("un", 1),
    ("une", 1),
    ("uno", 1),
    ("eins", 1),
    ("een", 1),
    ("um", 1),
    ("uma", 1),
    ("deux", 2),
    ("dos", 2),
    ("due", 2),
    ("zwei", 2),
    ("twee", 2),
    ("dois", 2),
    ("duas", 2),
    ("trois", 3),
    ("tres", 3),
    ("três", 3),
    ("tre", 3),
    ("drei", 3),
    ("drie", 3),
    ("quatre", 4),
    ("cuatro", 4),
    ("quattro", 4),
    ("quatro", 4),
    ("vier", 4),
    ("cinq", 5),
    ("cinco", 5),
    ("cinque", 5),
    ("fünf", 5),
    ("funf", 5),
    ("vijf", 5),
    ("seis", 6),
    ("sei", 6),
    ("sechs", 6),
    ("zes", 6),
    ("sept", 7),
    ("siete", 7),
    ("sette", 7),
    ("sete", 7),
    ("sieben", 7),
    ("zeven", 7),
    ("huit", 8),
    ("ocho", 8),
    ("otto", 8),
    ("oito", 8),
    ("acht", 8),
    ("neuf", 9),
    ("nueve", 9),
    ("nove", 9),
    ("neun", 9),
    ("negen", 9),
    ("dix", 10),
    ("diez", 10),
    ("dieci", 10),
    ("dez", 10),
    ("zehn", 10),
    ("tien", 10),
    ("onze", 11),
    ("once", 11),
    ("undici", 11),
    ("elf", 11),
    ("douze", 12),
    ("doce", 12),
    ("dodici", 12),
    ("doze", 12),
    ("zwölf", 12),
    ("zwolf", 12),
    ("twaalf", 12),
];
const ORDINALS_ELSEWHERE: [(&str, u32); 113] = [
    ("premier", 1),
    ("première", 1),
    ("premiere", 1),
    ("deuxième", 2),
    ("deuxieme", 2),
    ("second", 2),
    ("seconde", 2),
    ("troisième", 3),
    ("troisieme", 3),
    ("quatrième", 4),
    ("quatrieme", 4),
    ("cinquième", 5),
    ("cinquieme", 5),
    ("sixième", 6),
    ("sixieme", 6),
    ("septième", 7),
    ("septieme", 7),
    ("huitième", 8),
    ("huitieme", 8),
    ("neuvième", 9),
    ("neuvieme", 9),
    ("dixième", 10),
    ("dixieme", 10),
    ("onzième", 11),
    ("onzieme", 11),
    ("douzième", 12),
    ("douzieme", 12),
    ("primer", 1),
    ("primero", 1),
    ("primera", 1),
    ("segundo", 2),
    ("segunda", 2),
    ("tercer", 3),
    ("tercero", 3),
    ("tercera", 3),
    ("cuarto", 4),
    ("cuarta", 4),
    ("quinto", 5),
    ("quinta", 5),
    ("sexto", 6),
    ("sexta", 6),
    ("séptimo", 7),
    ("septimo", 7),
    ("séptima", 7),
    ("septima", 7),
    ("octavo", 8),
    ("octava", 8),
    ("noveno", 9),
    ("novena", 9),
    ("décimo", 10),
    ("decimo", 10),
    ("décima", 10),
    ("decima", 10),
    ("undécimo", 11),
    ("undecimo", 11),
    ("duodécimo", 12),
    ("duodecimo", 12),
    ("primo", 1),
    ("prima", 1),
    ("secondo", 2),
    ("seconda", 2),
    ("terzo", 3),
    ("terza", 3),
    ("quarto", 4),
    ("quarta", 4),
    ("sesto", 6),
    ("sesta", 6),
    ("settimo", 7),
    ("settima", 7),
    ("ottavo", 8),
    ("ottava", 8),
    ("nono", 9),
    ("nona", 9),
    ("undicesimo", 11),
    ("undicesima", 11),
    ("dodicesimo", 12),
    ("dodicesima", 12),
    ("erste", 1),
    ("zweite", 2),
    ("dritte", 3),
    ("vierte", 4),
    ("fünfte", 5),
    ("funfte", 5),
    ("sechste", 6),
    ("siebte", 7),
    ("achte", 8),
    ("neunte", 9),
    ("zehnte", 10),
    ("elfte", 11),
    ("zwölfte", 12),
    ("zwolfte", 12),
    ("eerste", 1),
    ("tweede", 2),
    ("derde", 3),
    ("vierde", 4),
    ("vijfde", 5),
    ("zesde", 6),
    ("zevende", 7),
    ("achtste", 8),
    ("negende", 9),
    ("tiende", 10),
    ("elfde", 11),
    ("twaalfde", 12),
    ("primeiro", 1),
    ("primeira", 1),
    ("terceiro", 3),
    ("terceira", 3),
    ("sétimo", 7),
    ("setimo", 7),
    ("sétima", 7),
    ("setima", 7),
    ("oitavo", 8),
    ("oitava", 8),
];
const SEPARATORS: [char; 4] = [' ', '-', '_', '.'];
const ONES: [(&str, &str); 9] = [
    ("one", "first"),
    ("two", "second"),
    ("three", "third"),
    ("four", "fourth"),
    ("five", "fifth"),
    ("six", "sixth"),
    ("seven", "seventh"),
    ("eight", "eighth"),
    ("nine", "ninth"),
];
const TEENS: [(&str, &str); 10] = [
    ("ten", "tenth"),
    ("eleven", "eleventh"),
    ("twelve", "twelfth"),
    ("thirteen", "thirteenth"),
    ("fourteen", "fourteenth"),
    ("fifteen", "fifteenth"),
    ("sixteen", "sixteenth"),
    ("seventeen", "seventeenth"),
    ("eighteen", "eighteenth"),
    ("nineteen", "nineteenth"),
];
const TENS: [(&str, &str); 8] = [
    ("twenty", "twentieth"),
    ("thirty", "thirtieth"),
    ("forty", "fortieth"),
    ("fifty", "fiftieth"),
    ("sixty", "sixtieth"),
    ("seventy", "seventieth"),
    ("eighty", "eightieth"),
    ("ninety", "ninetieth"),
];

pub(crate) fn disc_in_folder(name: &str) -> Option<NonZeroU32> {
    let folded = name.trim().to_lowercase();
    numbered_after_the_word(&folded).or_else(|| named_before_the_word(&folded))
}

fn numbered_after_the_word(folded: &str) -> Option<NonZeroU32> {
    let beyond = SPELLINGS
        .iter()
        .filter_map(|spelling| folded.strip_prefix(spelling))
        .min_by_key(|beyond| beyond.len())?;
    let numbered = beyond.trim_start_matches(SEPARATORS);

    if numbered.is_empty() {
        return None;
    }
    if numbered.bytes().all(|digit| digit.is_ascii_digit()) {
        return numbered.parse().ok();
    }
    if numbered.len() == beyond.len() {
        return None;
    }

    counted(numbered).or_else(|| numbered_elsewhere(numbered))
}

fn numbered_elsewhere(word: &str) -> Option<NonZeroU32> {
    ELSEWHERE
        .iter()
        .find_map(|(spelling, count)| (*spelling == word).then(|| numbering(*count)))
}

fn named_before_the_word(folded: &str) -> Option<NonZeroU32> {
    [
        ordinal_at_the_front(folded),
        ordinal_elsewhere_at_the_front(folded),
    ]
    .into_iter()
    .flatten()
    .find_map(|(number, beyond)| {
        let spelling = beyond.trim_start_matches(SEPARATORS);
        let separated = spelling.len() != beyond.len();
        (separated && SPELLINGS.contains(&spelling)).then_some(number)
    })
}

fn counted(word: &str) -> Option<NonZeroU32> {
    if let Some(number) = counted_whole(word) {
        return Some(number);
    }

    let (tens, ones) = word.split_once(SEPARATORS)?;
    let tens = cardinal_of(&TENS, tens, tens_number)?;
    let ones = cardinal_of(&ONES, ones.trim_start_matches(SEPARATORS), ones_number)?;

    Some(numbering(tens.get() + ones.get()))
}

fn counted_whole(word: &str) -> Option<NonZeroU32> {
    cardinal_of(&ONES, word, ones_number)
        .or_else(|| cardinal_of(&TEENS, word, teens_number))
        .or_else(|| cardinal_of(&TENS, word, tens_number))
}

fn ordinal_at_the_front(folded: &str) -> Option<(NonZeroU32, &str)> {
    tens_and_ones_at_the_front(folded)
        .or_else(|| ordinal_at(&TENS, folded, tens_number))
        .or_else(|| ordinal_at(&TEENS, folded, teens_number))
        .or_else(|| ordinal_at(&ONES, folded, ones_number))
}

fn ordinal_elsewhere_at_the_front(folded: &str) -> Option<(NonZeroU32, &str)> {
    ORDINALS_ELSEWHERE
        .iter()
        .filter_map(|(spelling, count)| Some((numbering(*count), folded.strip_prefix(spelling)?)))
        .min_by_key(|(_, beyond)| beyond.len())
}

fn tens_and_ones_at_the_front(folded: &str) -> Option<(NonZeroU32, &str)> {
    let (tens, beyond) = cardinal_at(&TENS, folded, tens_number)?;
    let joined = beyond.trim_start_matches(SEPARATORS);

    if joined.len() == beyond.len() {
        return None;
    }

    let (ones, beyond) = ordinal_at(&ONES, joined, ones_number)?;
    Some((numbering(tens.get() + ones.get()), beyond))
}

fn cardinal_of(
    table: &[(&str, &str)],
    word: &str,
    number: fn(usize) -> NonZeroU32,
) -> Option<NonZeroU32> {
    table
        .iter()
        .position(|(cardinal, _)| *cardinal == word)
        .map(number)
}

fn cardinal_at<'a>(
    table: &[(&str, &str)],
    folded: &'a str,
    number: fn(usize) -> NonZeroU32,
) -> Option<(NonZeroU32, &'a str)> {
    table
        .iter()
        .enumerate()
        .find_map(|(place, (cardinal, _))| Some((number(place), folded.strip_prefix(cardinal)?)))
}

fn ordinal_at<'a>(
    table: &[(&str, &str)],
    folded: &'a str,
    number: fn(usize) -> NonZeroU32,
) -> Option<(NonZeroU32, &'a str)> {
    table
        .iter()
        .enumerate()
        .find_map(|(place, (_, ordinal))| Some((number(place), folded.strip_prefix(ordinal)?)))
}

fn ones_number(place: usize) -> NonZeroU32 {
    numbering(place as u32 + 1)
}

fn teens_number(place: usize) -> NonZeroU32 {
    numbering(place as u32 + TEENS_BEGIN)
}

fn tens_number(place: usize) -> NonZeroU32 {
    numbering((place as u32 + TENS_BEGIN) * TEN)
}

fn numbering(count: u32) -> NonZeroU32 {
    NonZeroU32::MIN.saturating_add(count.saturating_sub(1))
}

const TEN: u32 = 10;
const TEENS_BEGIN: u32 = 10;
const TENS_BEGIN: u32 = 2;

pub(crate) fn is_a_sheet(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SHEET_EXTENSION))
}

pub(crate) fn read_sheet(path: &Path) -> Option<CueSheet> {
    let held = fs::metadata(path).ok()?;
    if held.len() > LARGEST_SHEET_ON_DISC {
        tracing::warn!(
            path = %path.display(),
            bytes = held.len(),
            "skipping a cue sheet larger than one is read as being"
        );
        return None;
    }

    let bytes = fs::read(path).ok()?;
    let sheet = read_cue(&bytes);
    (!sheet.is_empty()).then_some(sheet)
}

pub(crate) fn beside(sheet: &Path, named: &str) -> Option<PathBuf> {
    let named = Path::new(named);
    if named.components().count() != 1 {
        tracing::warn!(
            sheet = %sheet.display(),
            file = %named.display(),
            "a cue sheet naming a file outside its own folder is not followed"
        );
        return None;
    }
    Some(sheet.parent()?.join(named))
}

pub(crate) fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let lowered = extension.to_ascii_lowercase();
            AUDIO_EXTENSIONS.contains(&lowered.as_str())
        })
}

fn probe_all(work: &Receiver<Job>, done: &Sender<Outcome>, progress: &ScanProgress) {
    let sources = Sources::local();
    for job in work {
        if progress.is_cancelled() {
            return;
        }

        let outcomes = match job {
            Job::Known(id) => vec![Outcome::Seen(id)],
            Job::Probe(candidate) => match read_candidate(&sources, &candidate) {
                Ok(records) => {
                    rows_past_the_first(progress, records.len());
                    records
                        .into_iter()
                        .map(|record| Outcome::Store(Box::new(record)))
                        .collect()
                }
                Err(error) => {
                    let failure = Failure::of(&error);
                    tracing::debug!(%error, ?failure, path = %candidate.path.display(), "skipping a file that would not probe");
                    progress.failed.add(failure, 1);
                    progress.processed.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            },
            Job::Sheet(candidate) => match read_cut(&sources, &candidate) {
                Ok(records) => records
                    .into_iter()
                    .map(|record| Outcome::Store(Box::new(record)))
                    .collect(),
                Err(error) => {
                    let tracks = candidate.cut.audio_tracks().count() as u64;
                    let failure = Failure::of(&error);
                    tracing::debug!(%error, ?failure, sheet = %candidate.sheet.display(), "skipping a cue sheet whose file would not probe");
                    progress.failed.add(failure, tracks);
                    progress.processed.fetch_add(tracks, Ordering::Relaxed);
                    continue;
                }
            },
        };

        for outcome in outcomes {
            if done.send(outcome).is_err() {
                return;
            }
        }
    }
}

struct Cutting<'a> {
    root_id: i64,
    path: &'a Path,
    sleeve: &'a Option<PathBuf>,
    existing: &'a [Option<TrackId>],
    file_size: u64,
    modified: SystemTime,
    sheet_modified: Option<SystemTime>,
}

fn cut_into_rows(
    cutting: &Cutting<'_>,
    cut: &CueFile,
    info: &MediaInfo,
    embeds_a_picture: bool,
) -> Vec<TrackRecord> {
    let rate = info.spec.rate;
    let codec = Codec::from_id(info.codec);
    let mut records = Vec::new();

    for (held, (index, track)) in cut.audio_tracks().enumerate() {
        let Some(span) = cut.span_of(index, rate, info.duration) else {
            continue;
        };
        records.push(TrackRecord {
            root_id: cutting.root_id,
            path: cutting.path.to_path_buf(),
            sleeve: cutting.sleeve.clone(),
            existing: cutting.existing.get(held).copied().flatten(),
            file_size: cutting.file_size,
            modified: cutting.modified,
            sheet_modified: cutting.sheet_modified,
            spec: info.spec,
            codec,
            duration: span.frames(),
            span: Some(span),
            tags: track.titled(),
            embeds_a_picture,
        });
    }

    records
}

fn probed(sources: &Sources, path: &Path) -> resonate_codec::Result<(MediaInfo, bool)> {
    probe_pictured(sources, &MediaLocation::local(path), Picturing::Whether)
        .map(|(info, pictured)| (info, pictured.carries_one()))
}

fn read_cut(sources: &Sources, candidate: &SheetCandidate) -> Result<Vec<TrackRecord>> {
    let (info, embeds_a_picture) =
        probed(sources, &candidate.file).map_err(|source| Error::Tags {
            path: candidate.file.clone(),
            source: Box::new(source),
        })?;

    Ok(cut_into_rows(
        &Cutting {
            root_id: candidate.root_id,
            path: &candidate.file,
            sleeve: &candidate.sleeve,
            existing: &candidate.existing,
            file_size: candidate.file_size,
            modified: candidate.modified,
            sheet_modified: Some(candidate.sheet_modified),
        },
        &candidate.cut,
        &info,
        embeds_a_picture,
    ))
}

fn names_the_same_title(tagged: Option<&str>, parsed: &str) -> bool {
    match tagged {
        None => true,
        Some(tagged) => stripped_title(tagged) == stripped_title(parsed),
    }
}

fn name_from_stem(path: &Path, tags: &mut TagSet) {
    if tags.title.is_some() && tags.artist.is_some() {
        return;
    }
    let Some(stem) = path.file_stem() else {
        return;
    };
    let Some(named) = stem::read(&stem.to_string_lossy()) else {
        return;
    };

    if tags.artist.is_none() && names_the_same_title(tags.title.as_deref(), &named.title) {
        tags.artist = named.artist;
    }
    if tags.track_number.is_none() {
        tags.track_number = named.track_number;
    }
    if tags.title.is_none() {
        tags.title = Some(named.title);
    }
}

fn read_candidate(sources: &Sources, candidate: &Candidate) -> Result<Vec<TrackRecord>> {
    let (mut info, embeds_a_picture) =
        probed(sources, &candidate.path).map_err(|source| Error::Tags {
            path: candidate.path.clone(),
            source: Box::new(source),
        })?;

    let cutting = Cutting {
        root_id: candidate.root_id,
        path: &candidate.path,
        sleeve: &candidate.sleeve,
        existing: &candidate.existing,
        file_size: candidate.file_size,
        modified: candidate.modified,
        sheet_modified: None,
    };
    if let Some(cut) = info
        .cue
        .as_ref()
        .filter(|cut| cut.audio_tracks().next().is_some())
    {
        return Ok(cut_into_rows(&cutting, cut, &info, embeds_a_picture));
    }
    name_from_stem(&candidate.path, &mut info.tags);

    Ok(vec![TrackRecord {
        root_id: candidate.root_id,
        path: candidate.path.clone(),
        sleeve: candidate.sleeve.clone(),
        existing: candidate.existing.first().copied().flatten(),
        file_size: candidate.file_size,
        modified: candidate.modified,
        sheet_modified: None,
        spec: info.spec,
        codec: Codec::from_id(info.codec),
        duration: info.duration,
        span: None,
        tags: info.tags,
        embeds_a_picture,
    }])
}

fn write_all(
    inner: &Inner,
    options: &ScanOptions,
    done: &Receiver<Outcome>,
    progress: &ScanProgress,
    generation: i64,
) -> Result<()> {
    let mut cache = Cache::default();
    let mut batch = Vec::with_capacity(BATCH);

    for outcome in done {
        batch.push(outcome);
        if batch.len() >= BATCH {
            commit(inner, options, &mut cache, &mut batch, progress, generation)?;
        }
    }
    commit(inner, options, &mut cache, &mut batch, progress, generation)
}

fn commit(
    inner: &Inner,
    options: &ScanOptions,
    cache: &mut Cache,
    batch: &mut Vec<Outcome>,
    progress: &ScanProgress,
    generation: i64,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let mut added = 0;
    let mut updated = 0;

    inner.write(|transaction| {
        for outcome in batch.iter() {
            match outcome {
                Outcome::Seen(id) => store::touch(transaction, *id, generation)?,
                Outcome::Store(record) => {
                    store::apply(
                        transaction,
                        cache,
                        record,
                        generation,
                        options.extract_cover_art,
                    )?;
                    if record.existing.is_some() {
                        updated += 1;
                    } else {
                        added += 1;
                    }
                }
            }
        }
        Ok(())
    })?;

    progress
        .processed
        .fetch_add(batch.len() as u64, Ordering::Relaxed);
    progress.added.fetch_add(added, Ordering::Relaxed);
    progress.updated.fetch_add(updated, Ordering::Relaxed);
    batch.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_spelling_one_disc_of_a_set_answers_with_the_number_it_spells() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("CD1"), Some(1));
        assert_eq!(spelt("Disc 2"), Some(2));
        assert_eq!(spelt("disk-03"), Some(3));
        assert_eq!(spelt("  cd_11  "), Some(11));
        assert_eq!(spelt("DISC.4"), Some(4));

        assert_eq!(spelt("The Wall"), None);
        assert_eq!(spelt("cd"), None);
        assert_eq!(spelt("Discography"), None);
    }

    #[test]
    fn a_disc_numbered_in_words_is_read_either_side_of_the_word_it_numbers() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("CD Two"), Some(2));
        assert_eq!(spelt("Disc One"), Some(1));
        assert_eq!(spelt("disk_three"), Some(3));
        assert_eq!(spelt("DISC-TWELVE"), Some(12));
        assert_eq!(spelt("cd twenty"), Some(20));

        assert_eq!(spelt("Second Disc"), Some(2));
        assert_eq!(spelt("First CD"), Some(1));
        assert_eq!(spelt("  third-disk  "), Some(3));
        assert_eq!(spelt("TWELFTH.DISC"), Some(12));
    }

    #[test]
    fn a_disc_past_twenty_is_read_from_the_two_words_that_number_it() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("Disc Twenty One"), Some(21));
        assert_eq!(spelt("CD twenty-one"), Some(21));
        assert_eq!(spelt("disk_thirty_two"), Some(32));
        assert_eq!(spelt("DISC.FORTY.FIVE"), Some(45));
        assert_eq!(spelt("cd ninety nine"), Some(99));
        assert_eq!(spelt("cd sixty"), Some(60));

        assert_eq!(spelt("Twenty-First Disc"), Some(21));
        assert_eq!(spelt("FORTY-SECOND.CD"), Some(42));
        assert_eq!(spelt("Fiftieth Disc"), Some(50));
        assert_eq!(spelt("ninety ninth disk"), Some(99));

        assert_eq!(spelt("Disc Twenty Bonus"), None);
        assert_eq!(spelt("Disc One Hundred"), None);
        assert_eq!(spelt("Twenty Disc"), None);
        assert_eq!(spelt("Twentyfirst Disc"), None);
    }

    #[test]
    fn a_disc_is_read_by_the_word_every_language_files_it_under() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("Disque 2"), Some(2));
        assert_eq!(spelt("Disco 3"), Some(3));
        assert_eq!(spelt("Platte 4"), Some(4));
        assert_eq!(spelt("Schijf 5"), Some(5));
        assert_eq!(spelt("Skiva-6"), Some(6));
        assert_eq!(spelt("dysk_7"), Some(7));
        assert_eq!(spelt("Диск 8"), Some(8));
    }

    #[test]
    fn a_disc_numbered_in_another_language_is_the_same_disc() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("Disc Un"), Some(1));
        assert_eq!(spelt("CD Dos"), Some(2));
        assert_eq!(spelt("Disque Deux"), Some(2));
        assert_eq!(spelt("Disco Tre"), Some(3));
        assert_eq!(spelt("CD Vier"), Some(4));
        assert_eq!(spelt("Platte Fünf"), Some(5));
        assert_eq!(spelt("Schijf Twaalf"), Some(12));
        assert_eq!(spelt("Disco Zwölf"), Some(12));
    }

    #[test]
    fn a_disc_named_by_an_ordinal_in_another_language_is_the_same_disc() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("Zweite CD"), Some(2));
        assert_eq!(spelt("Erste Platte"), Some(1));
        assert_eq!(spelt("Deuxième disque"), Some(2));
        assert_eq!(spelt("Premier Disque"), Some(1));
        assert_eq!(spelt("Primer Disco"), Some(1));
        assert_eq!(spelt("Primera-CD"), Some(1));
        assert_eq!(spelt("Secondo Disco"), Some(2));
        assert_eq!(spelt("Tweede Schijf"), Some(2));
        assert_eq!(spelt("Terceiro Disco"), Some(3));
        assert_eq!(spelt("Zwölfte CD"), Some(12));
        assert_eq!(spelt("Zweitedisc"), None);
        assert_eq!(spelt("Zweite CD Bonus"), None);
        assert_eq!(spelt("Prima Donna"), None);
    }

    #[test]
    fn a_disc_noun_is_read_at_its_longest_so_a_longer_one_is_not_cut_short() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(
            spelt("Disco 2"),
            Some(2),
            "the shorter noun was stripped and left a letter before the number"
        );
        assert_eq!(spelt("Discovery"), None);
        assert_eq!(spelt("Disconnected"), None);
        assert_eq!(spelt("Discography"), None);
    }

    #[test]
    fn a_word_run_together_with_the_one_beside_it_names_no_disc() {
        let spelt = |name: &str| disc_in_folder(name).map(NonZeroU32::get);

        assert_eq!(spelt("Discone"), None);
        assert_eq!(spelt("seconddisc"), None);
        assert_eq!(spelt("Discovery"), None);
        assert_eq!(spelt("Disconnected"), None);
        assert_eq!(spelt("Second Disc Bonus"), None);
        assert_eq!(spelt("CD One Bonus"), None);
        assert_eq!(spelt("One"), None);
        assert_eq!(spelt("First"), None);
    }
}
