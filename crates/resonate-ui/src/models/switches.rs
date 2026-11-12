use std::{sync::Arc, time::Duration};

use gpui::{Context, Task};
use resonate_core::{KeptZone, ZoneId};
use resonate_engine::Keep;
use resonate_library::{HistoryKept, Library, Reference};

use super::LibraryModel;
use crate::ResonateApp;

impl LibraryModel {
    pub const fn is_online(&self) -> bool {
        self.online
    }

    pub fn set_online(&mut self, online: bool, cx: &mut Context<Self>) {
        if self.online == online {
            return;
        }
        self.online = online;
        self.unreached_for = None;
        self.ask_elsewhere_after(Duration::ZERO, cx);
        self.restate_the_listing();
        if online {
            self.sources_moved(cx);
            self.enrich_new_songs(cx);
        }
        cx.notify();
    }

    pub const fn resumes(&self) -> bool {
        self.resume
    }

    pub fn set_resume(&mut self, resume: bool, cx: &mut Context<Self>) {
        if self.resume == resume {
            return;
        }
        self.resume = resume;
        if !resume {
            let library = Arc::clone(&self.library);
            self.keep_after_what_was_kept(cx, move || {
                if let Err(error) = library.forget_resumption() {
                    tracing::warn!(%error, "what was kept of a queue could not be discarded");
                }
            });
        }
        cx.notify();
    }

    pub fn age_the_history(&mut self, kept: HistoryKept, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self._aged = cx.background_executor().spawn(async move {
            if let Err(error) = library.age_the_history(kept) {
                tracing::warn!(%error, "the history could not be aged");
            }
        });
        cx.notify();
    }

    pub fn keep_the_queue(&mut self, zone: ZoneId, keep: Keep, cx: &Context<Self>) {
        let following = cx
            .global::<ResonateApp>()
            .together
            .as_ref()
            .is_some_and(|together| together.following(zone));
        if following {
            return;
        }

        let library = Arc::clone(&self.library);

        self.keep_after_what_was_kept(cx, move || {
            let kept = match keep {
                Keep::Queue(resumption) => library.keep_resumption(zone, &resumption),
                Keep::Order(reordered) => library.keep_order(zone, &reordered),
                Keep::Place { row, at } => library.keep_place(zone, row, at),
            };
            if let Err(error) = kept {
                tracing::warn!(%error, "the queue was not kept for the next run");
            }
        });
    }

    pub fn keep_the_zone(&mut self, zone: KeptZone, cx: &Context<Self>) {
        let library = Arc::clone(&self.library);

        self.keep_after_what_was_kept(cx, move || {
            if let Err(error) = library.keep_zone(&zone) {
                tracing::warn!(%error, zone = %zone.id, "the zone was not kept for the next run");
            }
        });
    }

    pub fn forget_the_zone(&mut self, zone: ZoneId, cx: &Context<Self>) {
        let library = Arc::clone(&self.library);

        self.keep_after_what_was_kept(cx, move || {
            if let Err(error) = library.forget_zone(zone) {
                tracing::warn!(%error, %zone, "what was kept of a zone could not be discarded");
            }
        });
    }

    fn keep_after_what_was_kept(
        &mut self,
        cx: &Context<Self>,
        keep: impl FnOnce() + Send + 'static,
    ) {
        let before = std::mem::replace(&mut self._kept, Task::ready(()));
        self._kept = cx.background_executor().spawn(async move {
            before.await;
            keep();
        });
    }

    pub const fn enriches_after_scan(&self) -> bool {
        self.after_scan
    }

    pub const fn studies(&self) -> bool {
        self.studies
    }

    pub fn set_studies(&mut self, studies: bool, cx: &mut Context<Self>) {
        if self.studies == studies {
            return;
        }
        self.studies = studies;
        cx.notify();
    }

    pub const fn fetches_lyrics(&self) -> bool {
        self.lyrics
    }

    pub fn set_lyrics(&mut self, lyrics: bool, cx: &mut Context<Self>) {
        if self.lyrics == lyrics {
            return;
        }
        self.lyrics = lyrics;
        cx.notify();
    }

    pub fn set_after_scan(&mut self, after_scan: bool, cx: &mut Context<Self>) {
        if self.after_scan == after_scan {
            return;
        }
        self.after_scan = after_scan;
        cx.notify();
    }

    pub fn has_reference(&self) -> bool {
        self.reference.is_some()
    }

    pub fn can_enrich(&self) -> bool {
        self.online && self.for_the_pass.reference.is_some()
    }

    pub(crate) fn follows_links(&self) -> Option<(Arc<Library>, Arc<dyn Reference>)> {
        let reference = self.reference.clone().filter(|_| self.online)?;

        Some((Arc::clone(&self.library), reference))
    }
}
