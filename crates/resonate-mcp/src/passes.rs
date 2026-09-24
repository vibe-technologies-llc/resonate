use std::{
    cell::RefCell,
    fmt,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};

use resonate_library::{
    Cancelling, EnrichOptions, EnrichProgress, EnrichStats, EnrichSummary, Fingerprinters, Library,
    PassHandle, PollOptions, PollProgress, PollStats, PollSummary, Reference, ScanOptions,
    ScanProgress, ScanStats, ScanSummary,
};
use resonate_providers::Providers;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Error, Result, error::said};

pub struct Lookups {
    pub reference: Option<Arc<dyn Reference>>,
    pub fingerprinters: Arc<Fingerprinters>,
    pub providers: Arc<Providers>,
    pub studies: bool,
}

impl Lookups {
    pub fn none() -> Self {
        Self {
            reference: None,
            fingerprinters: Arc::new(Fingerprinters::none()),
            providers: Arc::new(Providers::none()),
            studies: EnrichOptions::default().studies,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pass {
    Scan,
    Lookup,
    Poll,
}

impl Pass {
    pub const ALL: [Self; 3] = [Self::Scan, Self::Lookup, Self::Poll];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Lookup => "lookup",
            Self::Poll => "poll",
        }
    }
}

impl fmt::Display for Pass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

trait Told {
    fn told(&self) -> Value;
}

impl Told for ScanStats {
    fn told(&self) -> Value {
        json!({
            "discovered": self.discovered,
            "processed": self.processed,
            "added": self.added,
            "updated": self.updated,
            "moved": self.moved,
            "removed": self.removed,
            "failed": self.failed.total(),
        })
    }
}

impl Told for EnrichStats {
    fn told(&self) -> Value {
        json!({
            "albums": self.albums,
            "releases": self.releases,
            "matched": self.matched,
            "covers": self.covers,
            "tracks": self.tracks,
            "named": self.named,
            "artists": self.artists,
            "portraits": self.portraits,
            "releases_found": self.releases_found,
            "refused": self.refused,
            "studied": self.studied,
            "fakes": self.fakes,
            "recognised": self.recognised,
            "misnamed": self.misnamed,
        })
    }
}

impl Told for PollStats {
    fn told(&self) -> Value {
        json!({
            "asked": self.asked,
            "offered": self.offered,
            "kept": self.kept,
            "unkept": self.unkept,
            "nothing": self.nothing,
            "refused": self.refused,
            "late": self.late,
        })
    }
}

trait Watched: Cancelling + 'static {
    type Stats: Told;

    fn now(&self) -> Self::Stats;
}

impl Watched for ScanProgress {
    type Stats = ScanStats;

    fn now(&self) -> ScanStats {
        self.snapshot()
    }
}

impl Watched for EnrichProgress {
    type Stats = EnrichStats;

    fn now(&self) -> EnrichStats {
        self.snapshot()
    }
}

impl Watched for PollProgress {
    type Stats = PollStats;

    fn now(&self) -> PollStats {
        self.snapshot()
    }
}

trait Ended {
    fn told(&self) -> Value;
}

impl Ended for ScanSummary {
    fn told(&self) -> Value {
        json!({ "stats": self.stats.told(), "cancelled": self.cancelled })
    }
}

impl Ended for EnrichSummary {
    fn told(&self) -> Value {
        let mut told = json!({ "stats": self.stats.told(), "cancelled": self.cancelled });
        if let Some(op) = self.stopped_by {
            told["stopped_asking_for"] = json!(format!("{op:?}"));
        }
        told
    }
}

impl Ended for PollSummary {
    fn told(&self) -> Value {
        json!({ "stats": self.stats.told(), "cancelled": self.cancelled })
    }
}

enum Slot<Progress, Summary> {
    Idle,
    Running(PassHandle<Progress, Summary>),
    Finished(Value),
}

impl<Progress: Watched, Summary: Ended> Slot<Progress, Summary> {
    fn settled(&mut self) {
        let finished = matches!(self, Self::Running(handle) if handle.is_finished());
        if finished {
            self.joined();
        }
    }

    fn joined(&mut self) {
        let Self::Running(handle) = std::mem::replace(self, Self::Idle) else {
            return;
        };
        *self = Self::Finished(match handle.join() {
            Ok(summary) => {
                let mut told = summary.told();
                told["state"] = json!("finished");
                told
            }
            Err(error) => json!({ "state": "failed", "error": said(&error) }),
        });
    }

    fn state(&mut self) -> Value {
        self.settled();
        match self {
            Self::Idle => json!({ "state": "idle" }),
            Self::Running(handle) => json!({
                "state": "running",
                "progress": handle.progress().now().told(),
            }),
            Self::Finished(told) => told.clone(),
        }
    }

    fn is_running(&mut self) -> bool {
        self.settled();
        matches!(self, Self::Running(_))
    }

    fn stop(&self) {
        if let Self::Running(handle) = self {
            handle.cancel();
        }
    }

    fn drained(&mut self) {
        self.stop();
        self.joined();
    }
}

struct Running {
    scan: Slot<ScanProgress, ScanSummary>,
    lookup: Slot<EnrichProgress, EnrichSummary>,
    poll: Slot<PollProgress, PollSummary>,
}

impl Running {
    fn state(&mut self, pass: Pass) -> Value {
        match pass {
            Pass::Scan => self.scan.state(),
            Pass::Lookup => self.lookup.state(),
            Pass::Poll => self.poll.state(),
        }
    }

    fn is_running(&mut self, pass: Pass) -> bool {
        match pass {
            Pass::Scan => self.scan.is_running(),
            Pass::Lookup => self.lookup.is_running(),
            Pass::Poll => self.poll.is_running(),
        }
    }

    fn stop(&self, pass: Pass) {
        match pass {
            Pass::Scan => self.scan.stop(),
            Pass::Lookup => self.lookup.stop(),
            Pass::Poll => self.poll.stop(),
        }
    }
}

pub struct Passes {
    lookups: Lookups,
    running: RefCell<Running>,
}

impl Passes {
    pub fn over(lookups: Lookups) -> Self {
        Self {
            lookups,
            running: RefCell::new(Running {
                scan: Slot::Idle,
                lookup: Slot::Idle,
                poll: Slot::Idle,
            }),
        }
    }

    fn free(&self, pass: Pass) -> Result<()> {
        if self.running.borrow_mut().is_running(pass) {
            return Err(Error::AlreadyRunning { pass });
        }
        Ok(())
    }

    pub(crate) fn scan(&self, library: &Library, roots: &[PathBuf]) -> Result<Value> {
        self.free(Pass::Scan)?;
        for root in roots {
            if !root.is_dir() {
                return Err(Error::NoSuchFolder { path: root.clone() });
            }
            library.add_root(root)?;
        }

        let handle = library.scan(ScanOptions {
            roots: roots.to_vec(),
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: true,
            workers: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
        })?;
        self.running.borrow_mut().scan = Slot::Running(handle);

        let walked: Vec<PathBuf> = if roots.is_empty() {
            library.roots()?
        } else {
            roots.to_vec()
        };
        Ok(json!({
            "started": Pass::Scan.name(),
            "roots": walked.iter().map(|root| spoken(root)).collect::<Vec<_>>(),
        }))
    }

    pub(crate) fn look_up(&self, library: &Library, refresh: bool) -> Result<Value> {
        self.free(Pass::Lookup)?;
        let reference = self.lookups.reference.clone().ok_or(Error::NoReference)?;

        let left = if refresh {
            None
        } else {
            library.unfinished_enrichment()?
        };
        let refreshing = left.as_ref().map_or(refresh, |left| left.refresh);
        let handle = library.enrich(
            reference,
            Arc::clone(&self.lookups.fingerprinters),
            EnrichOptions {
                refresh: refreshing,
                studies: self.lookups.studies,
                ..EnrichOptions::default()
            },
        )?;
        self.running.borrow_mut().lookup = Slot::Running(handle);

        Ok(json!({
            "started": Pass::Lookup.name(),
            "refresh": refreshing,
            "carrying_on": left.is_some(),
        }))
    }

    pub(crate) fn poll(&self, library: &Library, again: bool) -> Result<Value> {
        self.free(Pass::Poll)?;
        let options = if again {
            PollOptions::ASKING_EVERY_WANT
        } else {
            PollOptions::default()
        };
        let handle = library.poll(Arc::clone(&self.lookups.providers), options)?;
        self.running.borrow_mut().poll = Slot::Running(handle);

        Ok(json!({
            "started": Pass::Poll.name(),
            "a_provider_is_registered": self.lookups.providers.has_a_source(),
        }))
    }

    pub(crate) fn states(&self) -> Value {
        let mut running = self.running.borrow_mut();
        let mut states = serde_json::Map::new();
        for pass in Pass::ALL {
            states.insert(pass.name().to_owned(), running.state(pass));
        }
        Value::Object(states)
    }

    pub(crate) fn stop(&self, pass: Pass) -> Value {
        let mut running = self.running.borrow_mut();
        running.stop(pass);
        running.state(pass)
    }

    pub fn drain(&self) {
        let mut running = self.running.borrow_mut();
        running.scan.drained();
        running.lookup.drained();
        running.poll.drained();
    }
}

impl Drop for Passes {
    fn drop(&mut self) {
        self.drain();
    }
}

fn spoken(path: &Path) -> String {
    path.display().to_string()
}
