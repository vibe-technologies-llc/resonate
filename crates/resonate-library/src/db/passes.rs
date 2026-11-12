use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use resonate_codec::Sources;
use resonate_core::TrackId;
use resonate_providers::Providers;
use rusqlite::types::Value;

use super::{
    Asking, Library, ReleaseTotals, Source, Walk, asking_across_processes, counted_by_album,
    counted_by_disc, paged_under_roots, rooted,
    rows::{RawToTag, rows},
    statements::{RELEASE_DISC_TRACKS, RELEASE_MEDIA, TRACKS_TO_TAG},
};
use crate::{
    CoverArt, EnrichHandle, EnrichOptions, Error, Fingerprinters, ImportHandle, ImportOptions,
    Move, OrganiseHandle, OrganiseOptions, PassKind, PollHandle, PollOptions, Reference, Result,
    RetagHandle, RetagOptions, ScanHandle, ScanOptions, StoreOp, TagSink, enrich, import,
    organise::{self},
    retag::{self, Followed, TrackToTag},
    scan, sheeted, store, supply,
};

impl Library {
    pub fn wind_down(&self, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        let mut held = self.inner.walking_pass.lock();
        if let Some(pass) = held.as_ref() {
            pass.cancel();
        }
        while self.inner.walking.load(Ordering::Acquire) {
            if self
                .inner
                .walk_ended
                .wait_until(&mut held, deadline)
                .timed_out()
            {
                return !self.inner.walking.load(Ordering::Acquire);
            }
        }
        true
    }

    pub(crate) fn begins_keeping(&self) {
        self.inner
            .a_poll_waits_on_a_keep
            .store(false, Ordering::Release);
        self.inner.keeping.store(true, Ordering::Release);
    }

    pub(crate) fn ends_keeping(&self) {
        self.inner.keeping.store(false, Ordering::Release);
    }

    pub(crate) fn poll_refused(&self) {
        if self.inner.keeping.load(Ordering::Acquire) {
            self.inner
                .a_poll_waits_on_a_keep
                .store(true, Ordering::Release);
        }
    }

    pub(crate) fn a_poll_waits_on_a_keep(&self) -> bool {
        self.inner.a_poll_waits_on_a_keep.load(Ordering::Acquire)
    }

    pub(crate) fn asking_alone(&self, pass: PassKind) -> Result<Asking> {
        let _across_processes = match &self.inner.source {
            Source::File(catalog) => Some(asking_across_processes(catalog, pass)?),
            Source::Memory => None,
        };
        Ok(Asking { _across_processes })
    }

    pub fn enrich(
        &self,
        reference: Arc<dyn Reference>,
        fingerprinters: Arc<Fingerprinters>,
        options: EnrichOptions,
    ) -> Result<EnrichHandle> {
        enrich::start(self.shared(), reference, fingerprinters, options)
    }

    pub fn poll(&self, providers: Arc<Providers>, options: PollOptions) -> Result<PollHandle> {
        supply::start(self.shared(), providers, options)
    }

    pub fn scan(&self, options: ScanOptions) -> Result<ScanHandle> {
        scan::start(Arc::clone(&self.inner), options)
    }

    pub fn scan_what_is_held(&self, options: ScanOptions) -> Result<Option<ScanHandle>> {
        scan::start_over_held(Arc::clone(&self.inner), options)
    }

    pub fn forget_the_gone(&self, gone: &[PathBuf]) -> Result<u64> {
        scan::forget_the_gone(&self.inner, gone)
    }

    pub fn retire(&self, folder: &Path) -> Result<u64> {
        scan::retire(&self.inner, folder)
    }

    pub fn retag(&self, tags: Arc<dyn TagSink>, options: RetagOptions) -> Result<RetagHandle> {
        retag::start(self.shared(), tags, options)
    }

    pub fn organise(&self, options: OrganiseOptions) -> Result<OrganiseHandle> {
        organise::start(self.shared(), options)
    }

    pub(crate) fn walk_the_tree(&self) -> Result<Walk> {
        self.inner.walk_the_tree()
    }

    pub(crate) fn restate_the_statistics(&self) {
        self.inner.restate_the_statistics();
    }

    pub(crate) fn files_moved(&self, landed: &[Move], noting: organise::Noting) -> Result<()> {
        self.inner.write(|transaction| {
            organise::files_moved(transaction, landed)?;
            organise::noted(transaction, landed, noting)
        })
    }

    pub(crate) fn staging(&self, staged: &Path) -> Result<()> {
        self.inner
            .write(|transaction| organise::staging(transaction, staged))
    }

    pub(crate) fn staged_away(&self, staged: &Path) -> Result<()> {
        self.inner
            .write(|transaction| organise::staged_away(transaction, staged))
    }

    pub(crate) fn note_organised(&self, landed: &[Move]) -> Result<()> {
        self.inner
            .write(|transaction| organise::note_organised(transaction, landed))
    }

    pub(crate) fn last_organised(&self) -> Result<Vec<Move>> {
        self.inner.read(organise::last_organised)
    }

    pub fn walks_back(&self) -> Result<bool> {
        self.inner.read(|connection| {
            connection
                .query_row("SELECT EXISTS (SELECT 1 FROM organised)", [], |row| {
                    row.get::<_, bool>(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub(crate) fn staged_writes(&self) -> Result<Vec<organise::StagedWrite>> {
        self.inner.read(organise::staged_writes)
    }

    pub(crate) fn albums_re_keyed(&self, landed: &[Move]) -> Result<()> {
        self.inner
            .write(|transaction| organise::re_key_the_sleeves(transaction, landed))
    }

    pub(crate) fn release_totals(&self) -> Result<ReleaseTotals> {
        self.inner.read(|connection| {
            Ok(ReleaseTotals {
                tracks: counted_by_disc(connection, RELEASE_DISC_TRACKS)?,
                discs: counted_by_album(connection, RELEASE_MEDIA)?,
            })
        })
    }

    pub(crate) fn tracks_to_tag_after(
        &self,
        roots: &[PathBuf],
        after: Option<&Path>,
        at_most: usize,
        totals: &ReleaseTotals,
    ) -> Result<Vec<TrackToTag>> {
        let mut binds = rooted(roots)?;
        let sql = paged_under_roots(TRACKS_TO_TAG, binds.len(), after.is_some());
        if let Some(after) = after {
            binds.push(Value::Text(store::path_text(after).into_owned()));
        }
        binds.push(Value::Integer(i64::try_from(at_most).unwrap_or(i64::MAX)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds.clone(), |row| {
                RawToTag::read(row).map(Ok)
            })?
            .into_iter()
            .map(|raw| raw.into_tagged(&totals.tracks, &totals.discs))
            .collect()
        })
    }

    pub(crate) fn retag_to_be_written(
        &self,
        undoing: &[retag::Undoing],
        sheets: &[sheeted::SheetWrite],
        begins: bool,
        pictures: &mut retag::KeptPictures,
    ) -> Result<()> {
        self.inner.write(|transaction| {
            retag::note_what_was_there(transaction, undoing, sheets, begins, pictures)
        })
    }

    pub(crate) fn sheets_retagged(
        &self,
        followed: &[sheeted::SheetFollowed],
        unwritten: &[PathBuf],
        noted_again: Option<&[sheeted::KeptSheet]>,
    ) -> Result<()> {
        self.inner.write(|transaction| {
            sheeted::sheets_retagged(transaction, followed, unwritten, noted_again)
        })
    }

    pub(crate) fn last_resheet(&self) -> Result<Vec<sheeted::KeptSheet>> {
        self.inner.read(sheeted::last_resheet)
    }

    pub(crate) fn rows_cut_from(&self, file: &Path) -> Result<Vec<(TrackId, u64, u32)>> {
        self.inner
            .read(|connection| sheeted::rows_cut_from(connection, file))
    }

    pub(crate) fn retagged_picture(&self, id: i64) -> Result<Option<CoverArt>> {
        self.inner
            .read(|connection| retag::retagged_picture(connection, id))
    }

    pub(crate) fn files_retagged(
        &self,
        followed: &[Followed],
        unlanded: &retag::Unlanded<'_>,
        noted_again: Option<&[retag::KeptRetag]>,
    ) -> Result<()> {
        self.inner.write(|transaction| {
            retag::files_retagged(transaction, followed, unlanded, noted_again)
        })
    }

    pub(crate) fn retag_walked_back(
        &self,
        written: &[PathBuf],
        sheets: &[PathBuf],
        finished: bool,
    ) -> Result<()> {
        self.inner
            .write(|transaction| retag::walked_back(transaction, written, sheets, finished))
    }

    pub(crate) fn last_retag(&self) -> Result<Vec<retag::KeptRetag>> {
        self.inner.read(retag::last_retag)
    }

    pub fn retag_walks_back(&self) -> Result<bool> {
        self.inner.read(|connection| {
            connection
                .query_row("SELECT EXISTS (SELECT 1 FROM retagged) OR EXISTS (SELECT 1 FROM retagged_sheets)", [], |row| {
                    row.get(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn import(&self, sources: Arc<Sources>, options: ImportOptions) -> Result<ImportHandle> {
        let Some(vault) = self.inner.vault.clone() else {
            return Err(Error::NoVault);
        };
        import::start(self.shared(), vault, sources, options)
    }
}
