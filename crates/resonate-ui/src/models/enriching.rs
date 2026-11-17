use std::{mem, sync::Arc, time::Instant};

use gpui::Context;
use resonate_core::{TrackId, gaming};
use resonate_library::{Breadth, EnrichOptions, EnrichStats, EnrichSummary, Pace};

use super::{
    A_QUIET_LOOKUP_SHOWS_AFTER, LibraryModel, LookupAsked, Notice, POLLS_PER_RELOAD, Prompted,
    SCAN_POLL, SongAsked,
    told::{looked_up, song_looked_up},
};
use crate::{toast, words};

impl LibraryModel {
    pub fn is_enriching(&self) -> bool {
        self.enrichment.running.is_some()
    }

    pub fn shows_the_lookup(&self) -> bool {
        self.enrichment.running.is_some()
            && self.enrichment.song.is_none()
            && self
                .enrichment
                .asked
                .is_some_and(|asked| match asked.prompted {
                    Prompted::ByHand | Prompted::ByTheInbox => true,
                    Prompted::OnItsOwn => asked.began.elapsed() >= A_QUIET_LOOKUP_SHOWS_AFTER,
                })
    }

    pub fn enrich_stats(&self) -> Option<EnrichStats> {
        match &self.enrichment.running {
            Some(progress) => Some(progress.snapshot()),
            None => self.enrichment.looked_up.map(|summary| summary.stats),
        }
    }

    pub fn looked_up(&self) -> Option<EnrichSummary> {
        self.enrichment.looked_up
    }

    pub fn is_stopping_enrich(&self) -> bool {
        self.enrichment
            .running
            .as_ref()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn enrich(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let asked = EnrichOptions {
            refresh,
            at_most: None,
            sought: Arc::clone(&self.enrichment.sought),
            studies: self.choices.studies,
            lyrics: self.choices.lyrics,
            pace: Pace::Background,
            breadth: Breadth::TheLibrary,
        };
        self.enrich_with(asked, Prompted::ByHand, cx);
    }

    fn enrich_on_its_own(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let asked = EnrichOptions {
            refresh,
            at_most: None,
            sought: Arc::clone(&self.enrichment.sought),
            studies: self.choices.studies,
            lyrics: self.choices.lyrics,
            pace: Pace::Background,
            breadth: Breadth::TheLibrary,
        };
        self.enrich_with(asked, Prompted::OnItsOwn, cx);
    }

    pub fn hurry_the_lookup(&mut self, cx: &mut Context<Self>) {
        match &self.enrichment.running {
            Some(progress) => {
                progress.hurry();
                cx.notify();
            }
            None => {
                let asked = EnrichOptions {
                    refresh: false,
                    at_most: None,
                    sought: Arc::clone(&self.enrichment.sought),
                    studies: self.choices.studies,
                    lyrics: self.choices.lyrics,
                    pace: Pace::Hurried,
                    breadth: Breadth::TheLibrary,
                };
                self.enrich_with(asked, Prompted::ByHand, cx);
            }
        }
    }

    pub fn is_hurried(&self) -> bool {
        self.enrichment
            .running
            .as_ref()
            .is_some_and(|progress| progress.is_hurried())
    }

    pub(super) fn enrich_new_songs(&mut self, cx: &mut Context<Self>) {
        if !self.choices.after_scan || !self.can_enrich() {
            return;
        }
        if self.enrichment.running.is_some() {
            self.enrichment.owed = true;
        } else {
            self.enrich_on_its_own(false, cx);
        }
    }

    pub(super) fn only_the_covers(&self) -> EnrichOptions {
        EnrichOptions {
            refresh: false,
            at_most: None,
            sought: Arc::clone(&self.enrichment.sought),
            studies: false,
            lyrics: false,
            pace: Pace::Background,
            breadth: Breadth::TheLibrary,
        }
    }

    pub fn look_up_the_song(&mut self, track: TrackId, title: String, cx: &mut Context<Self>) {
        let song = SongAsked { track, title };
        if !self.can_enrich() {
            toast::tell(
                Notice::Noted(words::models::song_not_looked_up_offline(&song.title)),
                cx,
            );
            return;
        }
        if self.for_the_pass.lookups.is_shut() {
            toast::tell(
                Notice::Noted(words::models::song_not_looked_up_metered(&song.title)),
                cx,
            );
            return;
        }
        if self.enrichment.running.is_some() {
            toast::tell(
                Notice::Noted(words::models::song_looked_up_next(&song.title)),
                cx,
            );
            self.enrichment.song_owed = Some(song);
            return;
        }

        let said = match gaming::is_active() {
            true => words::models::song_looked_up_after_the_game(&song.title),
            false => words::models::looking_up_the_song(&song.title),
        };
        let asked = EnrichOptions {
            refresh: false,
            at_most: None,
            sought: Arc::clone(&self.enrichment.sought),
            studies: self.choices.studies,
            lyrics: self.choices.lyrics,
            pace: Pace::Hurried,
            breadth: Breadth::TheSong(song.track),
        };
        toast::tell(Notice::Noted(said), cx);
        self.enrichment.song = Some(song);
        self.enrich_with(asked, Prompted::ByHand, cx);
    }

    pub(super) fn enrich_with(
        &mut self,
        asked: EnrichOptions,
        prompted: Prompted,
        cx: &mut Context<Self>,
    ) {
        if self.enrichment.running.is_some() || !self.choices.online {
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
                self.enrichment.song = None;
                tracing::error!(%error, "the lookup could not be started");
                toast::tell(
                    toast::could_not(words::models::start_the_lookup(), &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        self.enrichment.running = Some(Arc::clone(handle.progress()));
        self.enrichment.asked = Some(LookupAsked {
            prompted,
            began: Instant::now(),
        });
        if self.enrichment.song.is_none() {
            self.enrichment.owed = false;
            self.enrichment.looked_up = None;
        }
        cx.notify();

        self.enrichment._task = cx.spawn(async move |this, cx| {
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
                this.enrichment.running = None;
                this.enrichment.asked = None;
                let owed = mem::take(&mut this.enrichment.owed);
                let song = this.enrichment.song.take();
                let mut finished = false;
                match (handle.join(), &song) {
                    (Ok(summary), Some(song)) => {
                        finished = !summary.cancelled && summary.stopped_by.is_none();
                        toast::tell(song_looked_up(&summary, &song.title), cx);
                    }
                    (Ok(summary), None) => {
                        finished = !summary.cancelled && summary.stopped_by.is_none();
                        this.enrichment.looked_up = Some(summary);
                        if let Some(notice) = looked_up(&summary, prompted) {
                            toast::tell(notice, cx);
                        }
                    }
                    (Err(error), Some(_)) => {
                        tracing::error!(%error, "the song could not be looked up");
                        toast::tell(
                            toast::could_not(words::models::look_up_that_song(), &error),
                            cx,
                        );
                    }
                    (Err(error), None) => {
                        tracing::error!(%error, "the lookup failed");
                        toast::tell(
                            toast::could_not(words::models::finish_the_lookup(), &error),
                            cx,
                        );
                    }
                }
                this.reload(cx);
                if let Some(owed) = this.enrichment.song_owed.take() {
                    this.look_up_the_song(owed.track, owed.title, cx);
                }
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
        self.enrichment.owed = false;
        self.enrichment.song_owed = None;
        let Some(progress) = self.enrichment.running.as_ref() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_scan(&mut self, cx: &mut Context<Self>) {
        let Some(progress) = self.running.work.scanning() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }
}
