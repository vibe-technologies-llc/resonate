use std::{mem, path::PathBuf, sync::Arc};

use gpui::{Context, Task};
use resonate_library::{FileTags, Layout, OrganiseOptions, RetagOptions};

use super::{
    LibraryModel, Notice, Pass, Planned, RootWaiting, SCAN_POLL, Work,
    told::{filed, folding_told, relocations, retagged},
};
use crate::{toast, words};

impl LibraryModel {
    pub fn stop_poll(&mut self, cx: &mut Context<Self>) {
        let Some(progress) = self.running.work.polling() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_import(&mut self, cx: &mut Context<Self>) {
        let Some((_, progress)) = self.running.work.importing() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_organise(&mut self, cx: &mut Context<Self>) {
        let Some((_, progress)) = self.running.work.organising() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_retag(&mut self, cx: &mut Context<Self>) {
        let Some((_, progress)) = self.running.work.tagging() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn retag(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.tag(
            RetagOptions {
                roots: Vec::new(),
                apply: pass.applies(),
                undo: false,
            },
            pass,
            cx,
        );
    }

    pub fn walk_the_tags_back(&mut self, cx: &mut Context<Self>) {
        self.tag(
            RetagOptions {
                roots: Vec::new(),
                apply: true,
                undo: true,
            },
            Pass::Apply,
            cx,
        );
    }

    pub(super) fn tag(&mut self, options: RetagOptions, pass: Pass, cx: &mut Context<Self>) {
        if self.running.work.is_busy() {
            toast::tell(
                Notice::Trouble(words::models::already_walking().to_owned()),
                cx,
            );
            cx.notify();
            return;
        }
        let read_at = self.library.plans_stamp();
        let handle = match self.library.retag(Arc::new(FileTags::default()), options) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the catalog could not be written back into the files");
                toast::tell(
                    toast::could_not(words::models::write_the_tags_into_the_files(), &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        self.running.work = Work::Tagging(pass, Arc::clone(handle.progress()));
        self.passes.tagged = None;
        self.passes.previewed_tags = Planned::Not;
        cx.notify();

        self.running._retag = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.choices.tags_walk_back = this.library.retag_walks_back().unwrap_or_default();
                this.take_up_what_waited(cx);
                match handle.join() {
                    Ok(summary) => {
                        this.passes.previewed_tags =
                            Planned::after(pass, summary.cancelled, read_at);
                        if pass.applies() {
                            toast::tell(retagged(&summary), cx);
                        }
                        this.passes.tagged = Some((pass, summary));
                    }
                    Err(error) => {
                        tracing::error!(%error, "the catalog was not written back into the files");
                        toast::tell(
                            toast::could_not(
                                words::models::write_the_tags_into_the_files(),
                                &error,
                            ),
                            cx,
                        );
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }

    pub fn organise(&mut self, layout: Layout, pass: Pass, cx: &mut Context<Self>) {
        self.file(
            OrganiseOptions {
                layout,
                apply: pass.applies(),
                ..OrganiseOptions::default()
            },
            pass,
            cx,
        );
    }

    pub fn walk_the_filing_back(&mut self, cx: &mut Context<Self>) {
        self.file(
            OrganiseOptions {
                apply: true,
                walk_back: true,
                ..OrganiseOptions::default()
            },
            Pass::Apply,
            cx,
        );
    }

    pub(super) fn file(&mut self, options: OrganiseOptions, pass: Pass, cx: &mut Context<Self>) {
        if self.running.work.is_busy() {
            toast::tell(
                Notice::Trouble(words::models::already_walking().to_owned()),
                cx,
            );
            cx.notify();
            return;
        }
        let read_at = self.library.plans_stamp();
        let handle = match self.library.organise(options) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the files could not be put in order");
                toast::tell(
                    toast::could_not(words::models::put_the_files_in_order(), &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        self.running.work = Work::Organising(pass, Arc::clone(handle.progress()));
        self.passes.organised = None;
        self.passes.previewed = Planned::Not;
        cx.notify();

        self.running._organise = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.choices.walks_back = this.library.walks_back().unwrap_or_default();
                this.take_up_what_waited(cx);
                match handle.join() {
                    Ok(summary) => {
                        this.passes.previewed = Planned::after(pass, summary.cancelled, read_at);
                        if pass.applies() {
                            toast::tell(filed(&summary), cx);
                            this.passes.relocated.extend(relocations(&summary));
                        }
                        this.passes.organised = Some((pass, summary));
                    }
                    Err(error) => {
                        tracing::error!(%error, "the files were not put in order");
                        toast::tell(
                            toast::could_not(words::models::put_the_files_in_order(), &error),
                            cx,
                        );
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }

    pub fn find_identical_songs(&mut self, cx: &mut Context<Self>) {
        if self.running.work.is_busy() {
            toast::tell(
                Notice::Trouble(words::models::already_walking().to_owned()),
                cx,
            );
            return;
        }
        let read_at = self.library.plans_stamp();
        let library = Arc::clone(&self.library);
        self.running.work = Work::Identical(Pass::Preview);
        self.passes.identical = None;
        self.passes.previewed_identical = Planned::Not;
        cx.notify();

        self.running._scan = cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { library.identical_copies() })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.take_up_what_waited(cx);
                match found {
                    Ok(copies) => {
                        this.passes.identical = Some(Arc::new(copies));
                        this.passes.previewed_identical =
                            Planned::after(Pass::Preview, false, read_at);
                    }
                    Err(error) => {
                        tracing::error!(%error, "the identical songs could not be found");
                        toast::tell(
                            toast::could_not(words::models::find_the_identical_songs(), &error),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub fn fold_identical_songs(&mut self, cx: &mut Context<Self>) {
        if self.running.work.is_busy() {
            toast::tell(
                Notice::Trouble(words::models::already_walking().to_owned()),
                cx,
            );
            return;
        }
        let Some(copies) = self
            .passes
            .identical
            .clone()
            .filter(|_| self.passes.previewed_identical.is_shown())
        else {
            return;
        };
        let library = Arc::clone(&self.library);
        let before = mem::replace(&mut self._edit, Task::ready(()));
        self.running.work = Work::Identical(Pass::Apply);
        self.passes.identical = None;
        self.passes.previewed_identical = Planned::Not;
        cx.notify();

        self.running._scan = cx.spawn(async move |this, cx| {
            before.await;
            let folded = cx
                .background_executor()
                .spawn(async move { library.fold_copies(&copies.identical) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.take_up_what_waited(cx);
                toast::tell(folding_told(folded), cx);
                this.reload(cx);
            });
            let _ = outcome;
        });
    }

    pub fn forget_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.running.work.is_busy() {
            let waiting = RootWaiting::ToForget(root);
            if !self.running.roots_waiting.contains(&waiting) {
                self.running.roots_waiting.push(waiting);
            }
            cx.notify();
            return;
        }
        self.running.work = Work::Forgetting;
        cx.notify();

        let library = Arc::clone(&self.library);
        self.running._scan = cx.spawn(async move |this, cx| {
            let dropped = cx
                .background_executor()
                .spawn(async move { library.remove_root(&root) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.take_up_what_waited(cx);
                if let Err(error) = dropped {
                    tracing::error!(%error, "a library folder could not be dropped");
                    toast::tell(
                        toast::could_not(words::models::drop_that_library_folder(), &error),
                        cx,
                    );
                }
                this.reload(cx);
            });
            let _ = outcome;
        });
    }
}
