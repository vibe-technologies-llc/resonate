use std::{
    cmp::Reverse,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
};

use ahash::AHashSet;
use parking_lot::Mutex;
use resonate_codec::{Codec, Sources};
use resonate_core::{AlbumId, TrackId};
use resonate_vault::{Form, Keeping, Refusal, Taking, Vault};

use crate::{
    Error, Library, Result,
    db::TrackToVault,
    pass::{Cancelling, ImportHandle, PassHandle, PassKind},
};

#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub roots: Vec<PathBuf>,
    pub apply: bool,
    pub at_most: Option<NonZeroUsize>,
    pub workers: NonZeroUsize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            apply: false,
            at_most: None,
            workers: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub track: TrackId,
    pub from: PathBuf,
    pub form: Form,
    pub bytes: u64,
    pub codec: Codec,
    pub renewing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vaulted {
    pub wanted: Wanted,
    pub to: PathBuf,
    pub bytes: u64,
    pub deduped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Passing {
    SourceGone,
    Unreadable,
    Refused(Refusal),
}

impl Passing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceGone => "is not there any more",
            Self::Unreadable => "could not be read",
            Self::Refused(refusal) => refusal.as_str(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Passed {
    pub from: PathBuf,
    pub why: Passing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportPlan {
    pub wanted: Vec<Wanted>,
    pub vaulted: Vec<Vaulted>,
    pub passed: Vec<Passed>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub walked: u64,
    pub vaulted: u64,
    pub deduped: u64,
    pub covers: u64,
    pub covers_passed: u64,
    pub passed: u64,
    pub was_bytes: u64,
    pub bytes: u64,
}

impl ImportStats {
    pub const fn saved(&self) -> u64 {
        self.was_bytes.saturating_sub(self.bytes)
    }
}

#[derive(Debug, Default)]
pub struct ImportProgress {
    walked: AtomicU64,
    vaulted: AtomicU64,
    deduped: AtomicU64,
    covers: AtomicU64,
    covers_passed: AtomicU64,
    passed: AtomicU64,
    was_bytes: AtomicU64,
    bytes: AtomicU64,
    cancelled: AtomicBool,
}

impl ImportProgress {
    pub fn snapshot(&self) -> ImportStats {
        ImportStats {
            walked: self.walked.load(Ordering::Relaxed),
            vaulted: self.vaulted.load(Ordering::Relaxed),
            deduped: self.deduped.load(Ordering::Relaxed),
            covers: self.covers.load(Ordering::Relaxed),
            covers_passed: self.covers_passed.load(Ordering::Relaxed),
            passed: self.passed.load(Ordering::Relaxed),
            was_bytes: self.was_bytes.load(Ordering::Relaxed),
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

impl Cancelling for ImportProgress {
    fn cancel(&self) {
        ImportProgress::cancel(self);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub stats: ImportStats,
    pub plan: ImportPlan,
    pub cancelled: bool,
}

pub(crate) fn start(
    library: Library,
    vault: Arc<Vault>,
    sources: Arc<Sources>,
    options: ImportOptions,
) -> Result<ImportHandle> {
    let walking = library.walk_the_tree()?;
    let progress = Arc::new(ImportProgress::default());
    let owned = Arc::clone(&progress);

    let thread = thread::Builder::new()
        .name("resonate-import".to_owned())
        .spawn(move || {
            let outcome = run(&library, &vault, &sources, &options, &progress);
            drop(walking);
            outcome
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Import, owned, thread))
}

fn run(
    library: &Library,
    vault: &Vault,
    sources: &Sources,
    options: &ImportOptions,
    progress: &ImportProgress,
) -> Result<ImportSummary> {
    let known = library.roots()?;
    for root in &options.roots {
        if !known.contains(root) {
            return Err(Error::NotARoot { path: root.clone() });
        }
    }

    let mut rows = library.tracks_to_vault(&options.roots)?;
    rows.retain(worth_weighing);
    if let Some(cap) = options.at_most {
        rows.truncate(cap.get());
    }
    progress.walked.store(rows.len() as u64, Ordering::Relaxed);

    let mut plan = ImportPlan::default();
    if !options.apply {
        plan.wanted = rows.iter().map(wanted).collect();
        return Ok(ImportSummary {
            stats: progress.snapshot(),
            plan,
            cancelled: false,
        });
    }

    let shared = Shared {
        library,
        vault,
        sources,
        progress,
        rows: &rows,
        claimed_in: costliest_first(&rows),
        next: AtomicUsize::new(0),
        failed: AtomicBool::new(false),
        covered: Mutex::new(AHashSet::new()),
    };
    let workers = options
        .workers
        .min(NonZeroUsize::new(rows.len()).unwrap_or(NonZeroUsize::MIN));
    let mut settled = thread::scope(|scope| shared.worked_through(scope, workers))?;
    settled.sort_by_key(|(at, _)| *at);
    for (_, outcome) in settled {
        match outcome {
            Outcome::Vaulted(vaulted) => plan.vaulted.push(vaulted),
            Outcome::Passed(passed) => plan.passed.push(passed),
        }
    }

    Ok(ImportSummary {
        stats: progress.snapshot(),
        plan,
        cancelled: progress.is_cancelled(),
    })
}

enum Outcome {
    Vaulted(Vaulted),
    Passed(Passed),
}

struct Shared<'a> {
    library: &'a Library,
    vault: &'a Vault,
    sources: &'a Sources,
    progress: &'a ImportProgress,
    rows: &'a [TrackToVault],
    claimed_in: Vec<usize>,
    next: AtomicUsize,
    failed: AtomicBool,
    covered: Mutex<AHashSet<AlbumId>>,
}

impl<'a> Shared<'a> {
    fn worked_through<'scope>(
        &'scope self,
        scope: &'scope thread::Scope<'scope, '_>,
        workers: NonZeroUsize,
    ) -> Result<Vec<(usize, Outcome)>> {
        let mut spawned = Vec::with_capacity(workers.get());
        for index in 0..workers.get() {
            match thread::Builder::new()
                .name(format!("resonate-import-{index}"))
                .spawn_scoped(scope, || self.work())
            {
                Ok(worker) => spawned.push(worker),
                Err(source) => {
                    self.failed.store(true, Ordering::Relaxed);
                    return Err(Error::ThreadSpawn { source });
                }
            }
        }

        let mut settled = Vec::with_capacity(self.rows.len());
        let mut first_error = None;
        for worker in spawned {
            match worker.join() {
                Ok(Ok(done)) => settled.extend(done),
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(_) => {
                    first_error.get_or_insert(Error::Stopped {
                        pass: PassKind::Import,
                    });
                }
            }
        }
        first_error.map_or(Ok(settled), Err)
    }

    fn work(&self) -> Result<Vec<(usize, Outcome)>> {
        let mut done = Vec::new();
        while !self.progress.is_cancelled() && !self.failed.load(Ordering::Relaxed) {
            let claimed = self.next.fetch_add(1, Ordering::Relaxed);
            let Some(&at) = self.claimed_in.get(claimed) else {
                break;
            };
            let row = &self.rows[at];
            let kept = keep_one(self.library, self.vault, self.sources, row, self.progress)
                .and_then(|outcome| self.cover_of(row).map(|()| outcome));
            match kept {
                Ok(outcome) => done.push((at, outcome)),
                Err(error) => {
                    self.failed.store(true, Ordering::Relaxed);
                    return Err(error);
                }
            }
        }
        Ok(done)
    }

    fn cover_of(&self, row: &TrackToVault) -> Result<()> {
        let Some(album) = row.album_id else {
            return Ok(());
        };
        if !self.covered.lock().insert(album) {
            return Ok(());
        }
        cover_of(self.library, self.vault, album, self.progress)
    }
}

fn keep_one(
    library: &Library,
    vault: &Vault,
    sources: &Sources,
    row: &TrackToVault,
    progress: &ImportProgress,
) -> Result<Outcome> {
    let asked = wanted(row);
    if !row.path.is_file() {
        return Ok(passed(progress, &asked, Passing::SourceGone));
    }

    let location = row.location();
    let taking = Taking {
        sources,
        location: &location,
        span: row.span,
        renewing: row.renewing,
    };

    match vault.keep(&taking) {
        Err(source) => {
            tracing::warn!(path = %row.path.display(), %source, "the vault could not keep a track");
            Ok(passed(progress, &asked, Passing::Unreadable))
        }
        Ok(Keeping::Refused(refusal)) => Ok(passed(progress, &asked, Passing::Refused(refusal))),
        Ok(Keeping::Kept(kept)) => {
            let asked = Wanted {
                form: kept.form,
                bytes: kept.was,
                ..asked
            };
            library.note_vaulted(row, &kept)?;
            progress.vaulted.fetch_add(1, Ordering::Relaxed);
            progress.was_bytes.fetch_add(asked.bytes, Ordering::Relaxed);
            if kept.deduped {
                progress.deduped.fetch_add(1, Ordering::Relaxed);
            } else {
                progress.bytes.fetch_add(kept.bytes, Ordering::Relaxed);
            }
            Ok(Outcome::Vaulted(Vaulted {
                wanted: asked,
                to: kept.path,
                bytes: kept.bytes,
                deduped: kept.deduped,
            }))
        }
    }
}

fn cover_of(
    library: &Library,
    vault: &Vault,
    album: AlbumId,
    progress: &ImportProgress,
) -> Result<()> {
    let Some(art) = library.cover_the_vault_lacks(album)? else {
        return Ok(());
    };

    let kept = match vault.keep_cover(&art) {
        Ok(kept) => kept,
        Err(source) => {
            tracing::warn!(album = album.get(), %source, "the vault could not keep an album's cover");
            progress.covers_passed.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    };
    if library.note_vaulted_cover(album, &kept)? {
        progress.covers.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn passed(progress: &ImportProgress, asked: &Wanted, why: Passing) -> Outcome {
    progress.passed.fetch_add(1, Ordering::Relaxed);
    Outcome::Passed(Passed {
        from: asked.from.clone(),
        why,
    })
}

fn costliest_first(rows: &[TrackToVault]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&at| Reverse(cost_of(&rows[at])));
    order
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Effort {
    Copied,
    Encoded,
    Packed,
}

fn cost_of(row: &TrackToVault) -> (Effort, u64) {
    let effort = match form_of(row) {
        Form::Kept => Effort::Copied,
        Form::Flac => Effort::Encoded,
        Form::Wave => Effort::Packed,
    };
    (effort, row.file_size)
}

fn worth_weighing(row: &TrackToVault) -> bool {
    !row.renewing || (form_of(row) != Form::Kept && row.path.is_file())
}

fn form_of(row: &TrackToVault) -> Form {
    Form::of(row.codec, row.spec, row.spec.format.valid_bits())
}

fn wanted(row: &TrackToVault) -> Wanted {
    Wanted {
        track: row.id,
        from: row.path.clone(),
        form: form_of(row),
        bytes: row.file_size,
        codec: row.codec,
        renewing: row.renewing,
    }
}
