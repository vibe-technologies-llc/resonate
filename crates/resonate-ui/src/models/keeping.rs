use std::{
    mem,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use gpui::Context;
use resonate_library::{KeepSummary, KeptSong, Library, PlaylistToKeep, PollOptions};
use resonate_providers::Providers;

use super::{
    KEEP_THREAD, KEEPS_LOOKED_AT_EVERY, KeepPass, LibraryModel, Notice, OWED_ASKED_AGAIN_AFTER,
    Wanted, delivery_folder, keep_refused, kept, list_kept,
};
use crate::{
    ResonateApp,
    serving::{Row, song_to_keep},
    toast, words,
};

impl LibraryModel {
    pub(crate) fn keep(&mut self, row: &Row, cx: &mut Context<Self>) {
        let Some(song) = row.song().and_then(song_to_keep) else {
            return;
        };
        self.keep_what_was_pressed(
            move |library| {
                Ok(match library.keep_song(song)? {
                    KeptSong::Queued => None,
                    KeptSong::AlreadyHeld(_) => Some(words::models::already_held().to_owned()),
                })
            },
            cx,
        );
    }

    pub(crate) fn keep_mix(&mut self, name: String, rows: &[Row], cx: &mut Context<Self>) {
        let (songs, untitled): (Vec<_>, Vec<_>) = rows
            .iter()
            .filter_map(Row::song)
            .filter_map(song_to_keep)
            .partition(|song| !song.title.trim().is_empty());
        let left_out = untitled.len();
        let ordered = cx
            .try_global::<ResonateApp>()
            .is_some_and(|app| app.online.youtube_music_keep_order);
        self.keep_what_was_pressed(
            move |library| {
                let kept = library.keep_playlist(PlaylistToKeep {
                    name: name.clone(),
                    ordered,
                    songs,
                })?;
                Ok(Some(list_kept(&name, kept, left_out)))
            },
            cx,
        );
    }

    fn keep_what_was_pressed(
        &mut self,
        press: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.library.deliver_into(delivery_folder(cx));
        let library = Arc::clone(&self.library);
        cx.spawn(async move |this, cx| {
            let pressed = cx
                .background_executor()
                .spawn(async move { press(&library) })
                .await;
            let _ = this.update(cx, |this, cx| match pressed {
                Ok(said) => {
                    if let Some(said) = said {
                        toast::tell(Notice::Done(said), cx);
                    }
                    this.keep_what_is_queued(cx);
                }
                Err(error) => this.tell_of_a_keep_refused(&error, cx),
            });
        })
        .detach();
    }

    fn tell_of_a_keep_refused(&mut self, error: &resonate_library::Error, cx: &mut Context<Self>) {
        tracing::error!(%error, "a song could not be kept");
        toast::tell(keep_refused(error), cx);
        cx.notify();
    }

    pub(super) fn keep_what_was_left_queued(&mut self, cx: &mut Context<Self>) {
        let turned_on = cx
            .try_global::<ResonateApp>()
            .is_some_and(|app| app.online.enabled && app.online.youtube_music);
        if !turned_on {
            return;
        }
        let library = Arc::clone(&self.library);
        self.keeps._running = cx.spawn(async move |this, cx| {
            let queued = cx
                .background_executor()
                .spawn(async move { library.keeps_pending() })
                .await;
            match queued {
                Ok(0) => {}
                Ok(_) => {
                    let _ = this.update(cx, |this, cx| this.keep_what_is_queued(cx));
                }
                Err(error) => tracing::warn!(%error, "what was left to keep could not be read"),
            }
        });
    }

    pub(super) fn keep_what_is_owed(&mut self, cx: &mut Context<Self>) {
        if mem::take(&mut self.keeps.owed) {
            self.keep_what_is_queued(cx);
        }
    }

    pub(super) fn keep_what_is_queued(&mut self, cx: &mut Context<Self>) {
        if self.keeps.pass.is_some() || self.running.work.polling().is_some() {
            self.keeps.owed = true;
            return;
        }
        self.library.deliver_into(delivery_folder(cx));
        let providers = self.providers(cx);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let library = Arc::clone(&self.library);
        let began = thread::Builder::new()
            .name(KEEP_THREAD.to_owned())
            .spawn(move || {
                let cancelled = || stopped.load(Ordering::Relaxed);
                library.keep_pending(&providers, &cancelled)
            });
        let handle = match began {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the keep pass could not start");
                toast::tell(
                    Notice::Trouble(words::models::the_songs_could_not_be_kept().to_owned()),
                    cx,
                );
                return;
            }
        };
        self.keeps.pass = Some(KeepPass { stop, left: 0 });
        cx.notify();
        self.follow_the_keep(handle, cx);
    }

    fn follow_the_keep(
        &mut self,
        handle: JoinHandle<resonate_library::Result<KeepSummary>>,
        cx: &mut Context<Self>,
    ) {
        let library = Arc::clone(&self.library);
        self.keeps._running = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(KEEPS_LOOKED_AT_EVERY).await;
                let reading = Arc::clone(&library);
                let left = cx
                    .background_executor()
                    .spawn(async move { reading.keeps_queued() })
                    .await;
                let ticked = this.update(cx, |this, cx| {
                    this.keep_only_while_turned_on(cx);
                    this.keeps_left_moved(left, cx);
                });
                if ticked.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                let ended = handle.join();
                this.keep_pass_ended(ended, cx);
            });
        });
    }

    fn keep_only_while_turned_on(&mut self, cx: &mut Context<Self>) {
        let turned_on = cx
            .try_global::<ResonateApp>()
            .is_some_and(|app| app.online.enabled && app.online.youtube_music);
        if !turned_on {
            self.stop_keeping(cx);
        }
    }

    fn keeps_left_moved(&mut self, left: resonate_library::Result<u64>, cx: &mut Context<Self>) {
        let left = match left {
            Ok(left) => left,
            Err(error) => {
                tracing::warn!(%error, "what is left to keep could not be read");
                return;
            }
        };
        let Some(pass) = self.keeps.pass.as_mut() else {
            return;
        };
        let settled_one = left < pass.left;
        if left != pass.left {
            pass.left = left;
            cx.notify();
        }
        if settled_one {
            self.read(Wanted::Everything, cx);
        }
    }

    pub(super) fn keep_pass_ended(
        &mut self,
        ended: thread::Result<resonate_library::Result<KeepSummary>>,
        cx: &mut Context<Self>,
    ) {
        let stopped = self
            .keeps
            .pass
            .take()
            .is_some_and(|pass| pass.stop.load(Ordering::Relaxed));
        match ended {
            Ok(Ok(summary)) => {
                self.keep_pass_answered(
                    &KeepSummary {
                        yielded: summary.yielded && !stopped,
                        ..summary
                    },
                    cx,
                );
            }
            Ok(Err(resonate_library::Error::AlreadyAsking { .. })) => {
                tracing::debug!("another process is asking the providers; the keep waits for it");
                if stopped {
                    self.tell_what_was_kept(cx);
                } else {
                    self.keeps.owed = true;
                    self.keep_again_after(OWED_ASKED_AGAIN_AFTER, cx);
                }
                self.take_up_what_waited(cx);
                cx.notify();
                return;
            }
            Ok(Err(error)) => {
                self.tell_what_was_kept(cx);
                self.tell_of_a_keep_refused(&error, cx);
            }
            Err(_) => {
                tracing::error!("the keep pass stopped");
                self.tell_what_was_kept(cx);
                toast::tell(
                    Notice::Trouble(words::models::the_songs_could_not_be_kept().to_owned()),
                    cx,
                );
            }
        }
        self.take_up_what_waited(cx);
        self.keep_what_is_owed(cx);
        self.reload(cx);
    }

    pub(super) fn keep_pass_answered(&mut self, summary: &KeepSummary, cx: &mut Context<Self>) {
        if summary.landed > 0 {
            self.enrich_new_songs(cx);
        }
        self.keeps.tally.landed += summary.landed;
        self.keeps.tally.gone += summary.gone;
        self.keeps.tally.unkept += summary.unkept;
        self.keeps.tally.stopped_by.clone_from(&summary.stopped_by);
        self.keeps.tally.stopped.clone_from(&summary.stopped);
        if summary.yielded {
            self.keeps.owed = true;
            self.running.poll_owed.get_or_insert(PollOptions::default());
        } else {
            self.tell_what_was_kept(cx);
        }
    }

    fn tell_what_was_kept(&mut self, cx: &mut Context<Self>) {
        if let Some(notice) = kept(&mem::take(&mut self.keeps.tally)) {
            toast::tell(notice, cx);
        }
    }

    fn keep_again_after(&mut self, waiting: Duration, cx: &mut Context<Self>) {
        self.keeps._again = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(waiting).await;
            let _ = this.update(cx, |this, cx| this.keep_what_is_owed(cx));
        });
    }

    pub fn keeps_left(&self) -> Option<u64> {
        self.keeps.pass.as_ref().map(|pass| pass.left)
    }

    pub fn stop_keeping(&mut self, cx: &mut Context<Self>) {
        let Some(pass) = &self.keeps.pass else {
            return;
        };
        pass.stop.store(true, Ordering::Relaxed);
        self.keeps.owed = false;
        cx.notify();
    }

    pub(super) fn ask_what_is_owed_after(&mut self, waiting: Duration, cx: &mut Context<Self>) {
        self.running._owed = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(waiting).await;
            let _ = this.update(cx, |this, cx| {
                if !this.running.work.is_busy() {
                    this.take_up_what_waited(cx);
                }
            });
        });
    }

    pub(super) fn worth_asking_on_its_own(
        &self,
        providers: &Providers,
        options: PollOptions,
    ) -> bool {
        if !providers.has_a_source() {
            return false;
        }
        match self.library.is_a_want_due(options) {
            Ok(due) => due,
            Err(error) => {
                tracing::warn!(%error, "the wants could not be read");
                false
            }
        }
    }
}
