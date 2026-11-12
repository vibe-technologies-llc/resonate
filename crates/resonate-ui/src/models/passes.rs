use std::sync::Arc;

use gpui::Context;
use resonate_core::{MediaLocation, TrackId};
use resonate_library::{
    Copies, OrganiseStats, OrganiseSummary, RetagStats, RetagSummary, ScanStats,
};

use super::{LibraryModel, Pass, Planned};

impl LibraryModel {
    pub const fn is_busy(&self) -> bool {
        self.work.is_busy()
    }

    pub const fn is_scanning(&self) -> bool {
        self.work.scanning().is_some()
    }

    pub fn is_stopping(&self) -> bool {
        self.work
            .scanning()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn stats(&self) -> Option<ScanStats> {
        self.work.scanning().map(|progress| progress.snapshot())
    }

    pub const fn is_organising(&self) -> bool {
        self.work.organising().is_some()
    }

    pub const fn walks_back(&self) -> bool {
        self.walks_back
    }

    pub const fn tags_walk_back(&self) -> bool {
        self.tags_walk_back
    }

    pub fn is_moving(&self) -> bool {
        self.work
            .organising()
            .is_some_and(|(pass, _)| pass.applies())
    }

    pub fn is_stopping_organise(&self) -> bool {
        self.work
            .organising()
            .is_some_and(|(_, progress)| progress.is_cancelled())
    }

    pub fn organise_stats(&self) -> Option<OrganiseStats> {
        match self.work.organising() {
            Some((_, progress)) => Some(progress.snapshot()),
            None => self.organised.as_ref().map(|(_, summary)| summary.stats),
        }
    }

    pub fn take_arrival(&mut self) -> Option<TrackId> {
        self.arrived.take()
    }

    pub fn take_relocations(&mut self) -> Vec<(MediaLocation, MediaLocation)> {
        std::mem::take(&mut self.relocated)
    }

    pub fn organised(&self) -> Option<(Pass, &OrganiseSummary)> {
        self.organised
            .as_ref()
            .map(|(pass, summary)| (*pass, summary))
    }

    pub const fn previewed(&self) -> Planned {
        self.previewed
    }

    pub fn forget_the_preview(&mut self, cx: &mut Context<Self>) {
        if self.organised.is_none() && self.previewed == Planned::Not {
            return;
        }
        self.organised = None;
        self.previewed = Planned::Not;
        cx.notify();
    }

    pub(super) fn take_down_what_moved(&mut self) {
        let now = self.library.plans_stamp();
        if self.previewed.outdated_at(now) {
            self.organised = None;
            self.previewed = Planned::Outdated;
        }
        if self.previewed_tags.outdated_at(now) {
            self.tagged = None;
            self.previewed_tags = Planned::Outdated;
        }
        if self.previewed_import.outdated_at(now) {
            self.imported = None;
            self.previewed_import = Planned::Outdated;
        }
        if self.previewed_identical.outdated_at(now) {
            self.identical = None;
            self.previewed_identical = Planned::Outdated;
        }
    }

    pub const fn is_tagging(&self) -> bool {
        self.work.tagging().is_some()
    }

    pub fn is_writing_tags(&self) -> bool {
        self.work.tagging().is_some_and(|(pass, _)| pass.applies())
    }

    pub fn is_stopping_retag(&self) -> bool {
        self.work
            .tagging()
            .is_some_and(|(_, progress)| progress.is_cancelled())
    }

    pub fn retag_stats(&self) -> Option<RetagStats> {
        match self.work.tagging() {
            Some((_, progress)) => Some(progress.snapshot()),
            None => self.tagged.as_ref().map(|(_, summary)| summary.stats),
        }
    }

    pub fn tagged(&self) -> Option<(Pass, &RetagSummary)> {
        self.tagged.as_ref().map(|(pass, summary)| (*pass, summary))
    }

    pub fn identical_songs(&self) -> Option<Arc<Copies>> {
        self.identical.clone()
    }

    pub const fn previewed_identical(&self) -> Planned {
        self.previewed_identical
    }

    pub const fn is_finding_identical(&self) -> bool {
        matches!(self.work.identical(), Some(Pass::Preview))
    }

    pub const fn is_folding_identical(&self) -> bool {
        matches!(self.work.identical(), Some(Pass::Apply))
    }

    pub const fn previewed_tags(&self) -> Planned {
        self.previewed_tags
    }
}
