use std::{mem, sync::Arc, time::Instant};

use gpui::Context;
use resonate_library::{EnrichOptions, EnrichStats, EnrichSummary, Pace};

use super::{
    A_QUIET_LOOKUP_SHOWS_AFTER, LibraryModel, LookupAsked, POLLS_PER_RELOAD, Prompted, SCAN_POLL,
    told::looked_up,
};
use crate::{toast, words};

impl LibraryModel {
    pub fn is_enriching(&self) -> bool {
        self.enriching.is_some()
    }

    pub fn shows_the_lookup(&self) -> bool {
        self.enriching.is_some()
            && self.lookup_asked.is_some_and(|asked| match asked.prompted {
                Prompted::ByHand | Prompted::ByTheInbox => true,
                Prompted::OnItsOwn => asked.began.elapsed() >= A_QUIET_LOOKUP_SHOWS_AFTER,
            })
    }

    pub fn enrich_stats(&self) -> Option<EnrichStats> {
        match &self.enriching {
            Some(progress) => Some(progress.snapshot()),
            None => self.looked_up.map(|summary| summary.stats),
        }
    }

    pub fn looked_up(&self) -> Option<EnrichSummary> {
        self.looked_up
    }

    pub fn is_stopping_enrich(&self) -> bool {
        self.enriching
            .as_ref()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn enrich(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let asked = EnrichOptions {
            refresh,
            at_most: None,
            sought: Arc::clone(&self.sought),
            studies: self.studies,
            lyrics: self.lyrics,
            pace: Pace::Background,
        };
        self.enrich_with(asked, Prompted::ByHand, cx);
    }

    fn enrich_on_its_own(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let asked = EnrichOptions {
            refresh,
            at_most: None,
            sought: Arc::clone(&self.sought),
            studies: self.studies,
            lyrics: self.lyrics,
            pace: Pace::Background,
        };
        self.enrich_with(asked, Prompted::OnItsOwn, cx);
    }

    pub fn hurry_the_lookup(&mut self, cx: &mut Context<Self>) {
        match &self.enriching {
            Some(progress) => {
                progress.hurry();
                cx.notify();
            }
            None => {
                let asked = EnrichOptions {
                    refresh: false,
                    at_most: None,
                    sought: Arc::clone(&self.sought),
                    studies: self.studies,
                    lyrics: self.lyrics,
                    pace: Pace::Hurried,
                };
                self.enrich_with(asked, Prompted::ByHand, cx);
            }
        }
    }

    pub fn is_hurried(&self) -> bool {
        self.enriching
            .as_ref()
            .is_some_and(|progress| progress.is_hurried())
    }

    pub(super) fn enrich_new_songs(&mut self, cx: &mut Context<Self>) {
        if !self.after_scan || !self.can_enrich() {
            return;
        }
        if self.enriching.is_some() {
            self.enrich_owed = true;
        } else {
            self.enrich_on_its_own(false, cx);
        }
    }

    pub(super) fn only_the_covers(&self) -> EnrichOptions {
        EnrichOptions {
            refresh: false,
            at_most: None,
            sought: Arc::clone(&self.sought),
            studies: false,
            lyrics: false,
            pace: Pace::Background,
        }
    }

    pub(super) fn enrich_with(
        &mut self,
        asked: EnrichOptions,
        prompted: Prompted,
        cx: &mut Context<Self>,
    ) {
        if self.enriching.is_some() || !self.online {
            return;
        }
        let Some(reference) = self.for_the_pass.reference.clone() else {
            return;
        };
        let handle = match self.library.enrich(
            reference,
            Arc::clone(&self.for_the_pass.fingerprinters),
            asked,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the lookup could not be started");
                toast::tell(
                    toast::could_not(words::models::start_the_lookup(), &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        self.enriching = Some(Arc::clone(handle.progress()));
        self.lookup_asked = Some(LookupAsked {
            prompted,
            began: Instant::now(),
        });
        self.enrich_owed = false;
        self.looked_up = None;
        cx.notify();

        self._enrich = cx.spawn(async move |this, cx| {
            let mut polled = 0;
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                polled += 1;

                let shown = this.update(cx, |this, cx| {
                    if polled % POLLS_PER_RELOAD == 0 {
                        this.reload(cx);
                    } else {
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.enriching = None;
                this.lookup_asked = None;
                let owed = mem::take(&mut this.enrich_owed);
                let mut finished = false;
                match handle.join() {
                    Ok(summary) => {
                        finished = !summary.cancelled && summary.stopped_by.is_none();
                        this.looked_up = Some(summary);
                        if let Some(notice) = looked_up(&summary, prompted) {
                            toast::tell(notice, cx);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "the lookup failed");
                        toast::tell(
                            toast::could_not(words::models::finish_the_lookup(), &error),
                            cx,
                        );
                    }
                }
                this.reload(cx);
                if owed && finished {
                    this.enrich_new_songs(cx);
                }
            });
            let _ = finished;
        });
    }

    pub(super) fn carry_on_enriching(&mut self, cx: &mut Context<Self>) {
        if !self.can_enrich() {
            return;
        }
        let left = match self.library.unfinished_enrichment() {
            Ok(left) => left,
            Err(error) => {
                tracing::warn!(%error, "what a lookup left unfinished could not be read back");
                return;
            }
        };
        let Some(left) = left else {
            self.enrich_new_songs(cx);
            return;
        };

        tracing::info!(
            began = ?left.began,
            refresh = left.refresh,
            "carrying on a lookup the last run left unfinished"
        );
        self.enrich_on_its_own(left.refresh, cx);
    }

    pub fn stop_enrich(&mut self, cx: &mut Context<Self>) {
        self.enrich_owed = false;
        let Some(progress) = self.enriching.as_ref() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_scan(&mut self, cx: &mut Context<Self>) {
        let Some(progress) = self.work.scanning() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }
}
