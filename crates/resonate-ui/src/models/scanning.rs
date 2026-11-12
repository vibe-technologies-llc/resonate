use std::{path::PathBuf, sync::Arc};

use gpui::Context;
use resonate_library::{Layout, OrganiseOptions};

use super::{
    Change, Edited, LibraryModel, POLLS_PER_RELOAD, Pass, Prompted, Reading, RootWaiting,
    SCAN_POLL, ToFile, Wanted, Work, told::scanned, watched::walk,
};
use crate::{toast, words};

impl LibraryModel {
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.scan(Vec::new(), cx);
    }

    pub fn read_everything_again(&mut self, cx: &mut Context<Self>) {
        self.start_scan(Vec::new(), Reading::Everything, Prompted::ByHand, cx);
    }

    pub fn lyrics_ahead(&self, wanted: &resonate_lyrics::Wanted) -> resonate_library::LyricsAhead {
        self.library
            .lyrics_ahead(&wanted.location, wanted.span)
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "how far the words run ahead could not be read");
                resonate_library::LyricsAhead::ZERO
            })
    }

    pub fn hold_lyrics_ahead(
        &self,
        wanted: &resonate_lyrics::Wanted,
        ahead: resonate_library::LyricsAhead,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self
            .library
            .hold_lyrics_ahead(&wanted.location, wanted.span, ahead)
        {
            tracing::error!(%error, "how far the words run ahead could not be kept");
            toast::tell(
                toast::could_not(words::models::keep_the_lyrics_timing(), &error),
                cx,
            );
        }
    }

    pub fn tell_earlier_listens(&mut self, cx: &mut Context<Self>) -> bool {
        match self
            .library
            .tell_earlier_listens(resonate_library::ListeningService::ListenBrainz)
        {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "the earlier plays could not be asked to be sent");
                toast::tell(
                    toast::could_not(words::models::send_the_earlier_plays(), &error),
                    cx,
                );
                cx.notify();
                false
            }
        }
    }

    pub fn look_for_missing_covers(&mut self, cx: &mut Context<Self>) {
        if self.enriching.is_some() || !self.can_enrich() {
            return;
        }
        self.edited_then(
            Wanted::ThePage,
            Change::LookForMissingCovers,
            |library| library.ask_again_for_covers().map(|_| None),
            |this, edited, cx| {
                if edited == Edited::Landed {
                    this.enrich_with(this.only_the_covers(), Prompted::ByHand, cx);
                }
            },
            cx,
        );
    }

    pub fn add_roots(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        if roots.is_empty() {
            return;
        }
        if self.work.is_busy() {
            for root in roots {
                let waiting = RootWaiting::ToAdd(root);
                if !self.roots_waiting.contains(&waiting) {
                    self.roots_waiting.push(waiting);
                }
            }
            cx.notify();
            return;
        }
        self.scan(roots, cx);
    }

    pub fn roots_waiting(&self) -> &[RootWaiting] {
        &self.roots_waiting
    }

    pub fn file_once_scanned(
        &mut self,
        layout: Layout,
        files: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if files.is_empty() {
            return;
        }
        match &mut self.to_file {
            Some(waiting) => {
                waiting.layout = layout;
                waiting.files.extend(files);
            }
            None => self.to_file = Some(ToFile { layout, files }),
        }
        cx.notify();
    }

    pub(super) fn take_up_what_waited(&mut self, cx: &mut Context<Self>) {
        if self.work.is_busy() {
            return;
        }
        if self.roots_waiting.is_empty() {
            if let Some(ToFile { layout, files }) = self.to_file.take() {
                self.file(
                    OrganiseOptions {
                        layout,
                        only: files,
                        apply: true,
                        ..OrganiseOptions::default()
                    },
                    Pass::Apply,
                    cx,
                );
                return;
            }
            if let Some(options) = self.poll_owed.take() {
                self.fetch_what_was_wanted(options, cx);
            }
            return;
        }
        let forgotten = self
            .roots_waiting
            .iter()
            .position(|waiting| matches!(waiting, RootWaiting::ToForget(_)));
        if let Some(at) = forgotten {
            let RootWaiting::ToForget(root) = self.roots_waiting.remove(at) else {
                return;
            };
            self.forget_root(root, cx);
            return;
        }
        let added: Vec<PathBuf> = self
            .roots_waiting
            .drain(..)
            .filter_map(|waiting| match waiting {
                RootWaiting::ToAdd(root) => Some(root),
                RootWaiting::ToForget(_) => None,
            })
            .collect();
        self.scan(added, cx);
    }

    pub(super) fn scan(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.start_scan(roots, Reading::WhatChanged, Prompted::ByHand, cx);
    }

    pub(super) fn start_scan(
        &mut self,
        roots: Vec<PathBuf>,
        reading: Reading,
        prompted: Prompted,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.work.is_busy() {
            return false;
        }
        let handle = match walk(&self.library, roots, reading, prompted) {
            Ok(Some(handle)) => handle,
            Ok(None) => return true,
            Err(error) if prompted == Prompted::OnItsOwn => {
                tracing::debug!(%error, "a scan for files changed under the roots could not start yet");
                return false;
            }
            Err(error) => {
                tracing::error!(%error, "the scan could not be started");
                toast::tell(
                    toast::could_not(words::models::start_the_scan(), &error),
                    cx,
                );
                cx.notify();
                return false;
            }
        };

        self.work = Work::Scanning(Arc::clone(handle.progress()));
        cx.notify();

        self._scan = cx.spawn(async move |this, cx| {
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
                this.work = Work::Nothing;
                this.take_up_what_waited(cx);
                match handle.join() {
                    Ok(summary) => {
                        if let Some(notice) = scanned(&summary, prompted) {
                            toast::tell(notice, cx);
                        }
                        let brought = summary.stats.added + summary.stats.updated > 0;
                        let worth_asking = prompted == Prompted::ByHand || brought;
                        if !summary.cancelled && worth_asking {
                            this.enrich_new_songs(cx);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "the scan failed");
                        toast::tell(
                            toast::could_not(words::models::finish_the_scan(), &error),
                            cx,
                        );
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
        true
    }
}
