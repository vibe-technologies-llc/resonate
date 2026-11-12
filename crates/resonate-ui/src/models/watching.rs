use std::{
    mem,
    path::PathBuf,
    slice,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use gpui::{Context, Task};
use resonate_library::RootsWatch;

use super::{
    ASKED_EVERY, INBOX_LOOKED_AT_EVERY, INBOX_QUIET_FOR, LibraryModel, Prompted,
    ROOTS_LOOKED_AT_EVERY, Reading, WATCHED_EVERY,
    pictures::landed_since,
    told::retried_after,
    watched::{Watching, roots_looked_at_now},
};

impl LibraryModel {
    pub(super) fn watch_the_inbox(&mut self, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self.watchers._inbox = cx.spawn(async move |this, cx| {
            let mut watched: Option<(PathBuf, Option<RootsWatch>)> = None;
            let mut owed = false;
            loop {
                cx.background_executor().timer(INBOX_LOOKED_AT_EVERY).await;
                let Ok(inbox) = this.update(cx, |this, _| this.sourcing.inbox.clone()) else {
                    return;
                };
                if watched.as_ref().map(|(folder, _)| folder) != inbox.as_ref() {
                    owed = false;
                    watched = match inbox {
                        Some(folder) => {
                            let over = folder.clone();
                            let asked = Arc::clone(&library);
                            let (watch, landed) = cx
                                .background_executor()
                                .spawn(async move {
                                    let landed = asked
                                        .last_tried()
                                        .ok()
                                        .flatten()
                                        .is_some_and(|tried| landed_since(&over, tried));
                                    (RootsWatch::over(slice::from_ref(&over)), landed)
                                })
                                .await;
                            owed = landed;
                            Some((folder, watch))
                        }
                        None => None,
                    };
                }

                if let Some(watch) = watched.as_ref().and_then(|(_, watch)| watch.as_ref()) {
                    let _ = watch.taken_away(Duration::ZERO);
                    owed |= !watch.settled(INBOX_QUIET_FOR).is_empty();
                }
                if !owed {
                    continue;
                }
                let Ok(answered) =
                    this.update(cx, |this, cx| this.poll_as(Prompted::ByTheInbox, cx))
                else {
                    return;
                };
                owed = !answered;
            }
        });
    }

    pub(super) fn watch_the_roots(&mut self, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self.watchers._roots = cx.spawn(async move |this, cx| {
            let mut watching = Watching::default();
            let mut owed: Vec<PathBuf> = Vec::new();
            let mut changed_while_closed = true;
            let mut looked_at = None;
            loop {
                cx.background_executor().timer(ROOTS_LOOKED_AT_EVERY).await;
                let Ok((busy, seen)) = this.update(cx, |this, _| {
                    (this.running.work.is_busy(), this.window_seen)
                }) else {
                    return;
                };
                if !roots_looked_at_now(seen, looked_at) {
                    continue;
                }
                looked_at = Some(Instant::now());

                let reading = Arc::clone(&library);
                let (followed, settled, forgotten) = cx
                    .background_executor()
                    .spawn(async move {
                        let mut followed = if watching.is_due_to_read_the_roots() {
                            watching.following(&reading)
                        } else {
                            watching
                        };
                        let forgotten = followed.forget_what_went(&reading);
                        let mut settled = followed.came_back();
                        if !busy {
                            settled.extend(followed.settled());
                        }
                        (followed, settled, forgotten)
                    })
                    .await;
                watching = followed;

                if forgotten > 0 && this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    return;
                }

                for root in settled {
                    if !owed.contains(&root) {
                        owed.push(root);
                    }
                }
                if busy {
                    continue;
                }

                if changed_while_closed && watching.has_read_the_roots() {
                    if !watching.finds_a_root_there() {
                        changed_while_closed = false;
                        continue;
                    }
                    let Ok(started) = this.update(cx, |this, cx| {
                        this.start_scan(Vec::new(), Reading::WhatChanged, Prompted::OnItsOwn, cx)
                    }) else {
                        return;
                    };
                    if started {
                        changed_while_closed = false;
                        owed.clear();
                    }
                    continue;
                }
                if owed.is_empty() {
                    continue;
                }

                let asked = owed.clone();
                let Ok(started) = this.update(cx, |this, cx| {
                    this.start_scan(asked, Reading::WhatChanged, Prompted::OnItsOwn, cx)
                }) else {
                    return;
                };
                if started {
                    owed.clear();
                }
            }
        });
    }

    pub(crate) fn window_seen(&mut self, seen: bool) {
        self.window_seen = seen;
    }

    pub(crate) fn take_what_a_model_offered(&mut self, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        let before = mem::replace(&mut self.watchers._offers, Task::ready(()));

        self.watchers._offers = cx.spawn(async move |this, cx| {
            before.await;
            let taken = cx
                .background_executor()
                .spawn(async move { library.take_what_a_model_offered() })
                .await;
            match taken {
                Ok(0) => {}
                Ok(_) => {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
                Err(error) => {
                    tracing::warn!(%error, "the edits a model made were not taken onto Undo");
                }
            }
        });
    }

    pub(super) fn watch_for_writes_elsewhere(&mut self, cx: &mut Context<Self>) {
        self.take_what_a_model_offered(cx);
        let library = Arc::clone(&self.library);
        self.watchers._writes = cx.spawn(async move |this, cx| {
            let mut loaded = library.written_elsewhere();
            let mut seen = loaded;
            loop {
                cx.background_executor().timer(WATCHED_EVERY).await;
                let reading = Arc::clone(&library);
                let now = cx
                    .background_executor()
                    .spawn(async move { reading.written_elsewhere() })
                    .await;
                let Some(now) = now else {
                    continue;
                };
                let settled = seen == Some(now) && loaded != Some(now);
                seen = Some(now);
                if !settled {
                    continue;
                }
                loaded = Some(now);
                let followed = this.update(cx, |this, cx| {
                    this.take_what_a_model_offered(cx);
                    this.reload(cx);
                });
                if followed.is_err() {
                    return;
                }
            }
        });
    }

    pub(super) fn ask_when_due(&mut self, cx: &mut Context<Self>) {
        let at = self.next_asked_at();
        if self
            .outreach
            .asking_at
            .is_some_and(|scheduled| scheduled <= at && scheduled > Instant::now())
        {
            return;
        }
        self.outreach.asking_at = Some(at);

        self.outreach._task = cx.spawn(async move |this, cx| {
            let mut at = at;
            loop {
                cx.background_executor()
                    .timer(at.saturating_duration_since(Instant::now()))
                    .await;
                let Ok(next) = this.update(cx, |this, cx| {
                    this.poll_as(Prompted::OnItsOwn, cx);
                    let next = this.next_asked_at();
                    this.outreach.asking_at = Some(next);
                    next
                }) else {
                    return;
                };
                at = next;
            }
        });
    }

    fn next_asked_at(&self) -> Instant {
        let now = Instant::now();
        let latest = now + ASKED_EVERY;
        let due = self.outreach.next_try.map_or(latest, |at| {
            now + at.duration_since(SystemTime::now()).unwrap_or_default()
        });

        due.max(self.outreach.asks_from)
            .max(now + retried_after(self.outreach.unheard_in_a_row))
            .min(latest)
    }
}
