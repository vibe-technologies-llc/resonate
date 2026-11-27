use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use resonate_core::{StreamSpec, TrackId};
use resonate_vault::Halt;

use crate::{
    Ceiling, Error, Library, Recompressed, Result,
    pass::{Cancelling, PassHandle, PassKind},
    recompress::{InPlace, recompress_in_place},
};

const LANDED_READ_AT_ONCE: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecompressCandidate {
    pub track: TrackId,
    pub path: PathBuf,
    pub root: PathBuf,
    pub bytes: u64,
    pub spec: StreamSpec,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecompressStats {
    pub candidates: u64,
    pub weighed: u64,
    pub smaller: u64,
    pub kept: u64,
    pub failed: u64,
    pub before: u64,
    pub after: u64,
}

impl RecompressStats {
    pub const fn saved(self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

#[derive(Debug, Default)]
pub struct RecompressProgress {
    candidates: AtomicU64,
    weighed: AtomicU64,
    smaller: AtomicU64,
    kept: AtomicU64,
    failed: AtomicU64,
    before: AtomicU64,
    after: AtomicU64,
    cancelled: AtomicBool,
}

impl RecompressProgress {
    pub fn snapshot(&self) -> RecompressStats {
        RecompressStats {
            candidates: self.candidates.load(Ordering::Relaxed),
            weighed: self.weighed.load(Ordering::Relaxed),
            smaller: self.smaller.load(Ordering::Relaxed),
            kept: self.kept.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            before: self.before.load(Ordering::Relaxed),
            after: self.after.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn heard(&self, outcome: &Recompressed) {
        self.weighed.fetch_add(1, Ordering::Relaxed);
        match outcome {
            Recompressed::Smaller(shrunk) => {
                self.smaller.fetch_add(1, Ordering::Relaxed);
                self.before.fetch_add(shrunk.before, Ordering::Relaxed);
                self.after.fetch_add(shrunk.after, Ordering::Relaxed);
            }
            Recompressed::Kept(_) => {
                self.kept.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Cancelling for RecompressProgress {
    fn cancel(&self) {
        RecompressProgress::cancel(self);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecompressedFile {
    pub track: TrackId,
    pub path: PathBuf,
    pub outcome: Recompressed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecompressSummary {
    pub stats: RecompressStats,
    pub files: Vec<RecompressedFile>,
    pub cancelled: bool,
}

pub type RecompressHandle = PassHandle<RecompressProgress, RecompressSummary>;

pub(crate) fn start(
    library: Library,
    candidates: Vec<RecompressCandidate>,
    ceiling: Ceiling,
) -> Result<RecompressHandle> {
    let walking = library.walk_the_tree()?;
    let progress = Arc::new(RecompressProgress::default());
    let owned = Arc::clone(&progress);

    walking.cancelled_by(Arc::clone(&progress) as Arc<dyn Cancelling>);
    progress
        .candidates
        .store(candidates.len() as u64, Ordering::Relaxed);

    let thread = thread::Builder::new()
        .name("resonate-recompress".to_owned())
        .spawn(move || {
            let mut files = Vec::with_capacity(candidates.len());
            let mut landed: Vec<RecompressCandidate> = Vec::new();

            for candidate in candidates {
                if progress.is_cancelled() {
                    break;
                }

                let outcome = recompress_in_place(InPlace {
                    path: &candidate.path,
                    ceiling,
                    halt: Halt::on(&progress.cancelled),
                });

                match outcome {
                    Ok(outcome) => {
                        progress.heard(&outcome);
                        if matches!(outcome, Recompressed::Smaller(_)) {
                            landed.push(candidate.clone());
                        }
                        files.push(RecompressedFile {
                            track: candidate.track,
                            path: candidate.path,
                            outcome,
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "a file could not be re-compressed");
                        progress.weighed.fetch_add(1, Ordering::Relaxed);
                        progress.failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if landed.len() >= LANDED_READ_AT_ONCE {
                    library.landed_after_recompressing(&walking, &landed);
                    landed.clear();
                }
            }
            library.landed_after_recompressing(&walking, &landed);
            drop(walking);
            Ok(RecompressSummary {
                stats: progress.snapshot(),
                files,
                cancelled: progress.is_cancelled(),
            })
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Recompress, owned, thread))
}
