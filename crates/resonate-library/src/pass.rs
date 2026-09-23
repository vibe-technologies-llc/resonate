use std::{sync::Arc, thread::JoinHandle};

use crate::{
    EnrichProgress, EnrichSummary, Error, ImportProgress, ImportSummary, OrganiseProgress,
    OrganiseSummary, PollProgress, PollSummary, Result, RetagProgress, RetagSummary, ScanProgress,
    ScanSummary,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassKind {
    Scan,
    Enrich,
    Poll,
    Organise,
    Retag,
    Import,
}

pub trait Cancelling: Send + Sync {
    fn cancel(&self);
}

pub struct PassHandle<Progress, Summary> {
    pass: PassKind,
    progress: Arc<Progress>,
    thread: JoinHandle<Result<Summary>>,
}

impl<Progress: Cancelling, Summary> PassHandle<Progress, Summary> {
    pub(crate) const fn of(
        pass: PassKind,
        progress: Arc<Progress>,
        thread: JoinHandle<Result<Summary>>,
    ) -> Self {
        Self {
            pass,
            progress,
            thread,
        }
    }

    pub const fn pass(&self) -> PassKind {
        self.pass
    }

    pub const fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub fn join(self) -> Result<Summary> {
        let pass = self.pass;
        self.thread.join().unwrap_or(Err(Error::Stopped { pass }))
    }
}

pub type ScanHandle = PassHandle<ScanProgress, ScanSummary>;
pub type EnrichHandle = PassHandle<EnrichProgress, EnrichSummary>;
pub type PollHandle = PassHandle<PollProgress, PollSummary>;
pub type OrganiseHandle = PassHandle<OrganiseProgress, OrganiseSummary>;
pub type RetagHandle = PassHandle<RetagProgress, RetagSummary>;
pub type ImportHandle = PassHandle<ImportProgress, ImportSummary>;
