use std::{any::Any, sync::Arc, thread::JoinHandle};

use crate::{
    EnrichProgress, EnrichSummary, Error, ImportProgress, ImportSummary, OrganiseProgress,
    OrganiseSummary, PollProgress, PollSummary, Result, RetagProgress, RetagSummary, ScanProgress,
    ScanSummary, TakeInProgress, TakeInSummary,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassKind {
    Scan,
    Enrich,
    Poll,
    Organise,
    Retag,
    Import,
    TakeIn,
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
        self.thread.join().unwrap_or_else(|panicked| {
            tracing::error!(
                ?pass,
                panic = what_it_said(panicked.as_ref()),
                "a pass fell over before it finished"
            );
            Err(Error::Stopped { pass })
        })
    }
}

fn what_it_said(panicked: &(dyn Any + Send)) -> &str {
    panicked
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panicked.downcast_ref::<String>().map(String::as_str))
        .unwrap_or_default()
}

pub type ScanHandle = PassHandle<ScanProgress, ScanSummary>;
pub type EnrichHandle = PassHandle<EnrichProgress, EnrichSummary>;
pub type PollHandle = PassHandle<PollProgress, PollSummary>;
pub type OrganiseHandle = PassHandle<OrganiseProgress, OrganiseSummary>;
pub type RetagHandle = PassHandle<RetagProgress, RetagSummary>;
pub type ImportHandle = PassHandle<ImportProgress, ImportSummary>;
pub type TakeInHandle = PassHandle<TakeInProgress, TakeInSummary>;

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn what_a_pass_said_as_it_fell_over_is_read_whichever_way_it_was_raised() {
        let fixed = thread::spawn(|| panic!("the walk lost its root"))
            .join()
            .expect_err("a panicking thread");
        let formatted = thread::spawn(|| panic!("row {} would not read", 7))
            .join()
            .expect_err("a panicking thread");
        let opaque: Box<dyn Any + Send> = Box::new(7_u8);

        assert_eq!(what_it_said(fixed.as_ref()), "the walk lost its root");
        assert_eq!(what_it_said(formatted.as_ref()), "row 7 would not read");
        assert_eq!(what_it_said(opaque.as_ref()), "");
    }
}
