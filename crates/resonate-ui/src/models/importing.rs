use std::{path::PathBuf, sync::Arc};

use gpui::Context;
use resonate_library::{ImportOptions, ImportStats, ImportSummary, Sources};

use super::{
    LibraryModel, Notice, Pass, Planned, SCAN_POLL, Work, told::vaulted, watched::workers_beside,
};
use crate::{toast, words};

impl LibraryModel {
    pub fn has_a_vault(&self) -> bool {
        self.library.vault().is_some()
    }

    pub fn vault_root(&self) -> Option<PathBuf> {
        self.library.vault().map(|vault| vault.root().to_path_buf())
    }

    pub fn is_importing(&self) -> bool {
        self.work.importing().is_some()
    }

    pub fn is_keeping(&self) -> bool {
        self.work
            .importing()
            .is_some_and(|(pass, _)| pass.applies())
    }

    pub fn is_stopping_import(&self) -> bool {
        self.work
            .importing()
            .is_some_and(|(_, progress)| progress.is_cancelled())
    }

    pub fn import_stats(&self) -> Option<ImportStats> {
        match self.work.importing() {
            Some((_, progress)) => Some(progress.snapshot()),
            None => self.imported.as_ref().map(|(_, summary)| summary.stats),
        }
    }

    pub fn imported(&self) -> Option<(Pass, &ImportSummary)> {
        self.imported
            .as_ref()
            .map(|(pass, summary)| (*pass, summary))
    }

    pub const fn previewed_import(&self) -> Planned {
        self.previewed_import
    }

    pub(crate) fn heard_playing(&mut self, playing: bool) {
        if self.playing == playing {
            return;
        }
        self.playing = playing;
        if let Work::Importing(_, progress) = &self.work {
            progress.hold_to(workers_beside(playing));
        }
    }

    pub fn import(&mut self, pass: Pass, cx: &mut Context<Self>) {
        if self.work.is_busy() {
            toast::tell(
                Notice::Trouble(words::models::already_walking().to_owned()),
                cx,
            );
            cx.notify();
            return;
        }
        let read_at = self.library.plans_stamp();
        let handle = match self.library.import(
            Arc::new(Sources::local()),
            ImportOptions {
                apply: pass.applies(),
                ..ImportOptions::default()
            },
        ) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the tracks could not be kept in the vault");
                toast::tell(
                    toast::could_not(words::models::keep_the_tracks_in_the_vault(), &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        handle.progress().hold_to(workers_beside(self.playing));
        self.work = Work::Importing(pass, Arc::clone(handle.progress()));
        self.imported = None;
        self.previewed_import = Planned::Not;
        cx.notify();

        self._import = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                this.take_up_what_waited(cx);
                match handle.join() {
                    Ok(summary) => {
                        this.previewed_import = Planned::after(pass, summary.cancelled, read_at);
                        if pass.applies() {
                            toast::tell(vaulted(&summary), cx);
                        }
                        this.imported = Some((pass, summary));
                    }
                    Err(error) => {
                        tracing::error!(%error, "the tracks were not kept in the vault");
                        toast::tell(
                            toast::could_not(words::models::keep_the_tracks_in_the_vault(), &error),
                            cx,
                        );
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }
}
