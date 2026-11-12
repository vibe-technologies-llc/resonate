use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{App, Context};
use resonate_library::{PollOptions, PollStats};
use resonate_providers::Providers;

use super::{
    Followed, LibraryModel, Notice, OWED_ASKED_AGAIN_AFTER, Prompted, SCAN_POLL, Work,
    delivery_folder, told::polled,
};
use crate::{
    ResonateApp,
    downloads::Fetcher,
    settings::{Asking, Online},
    toast, words,
};

impl LibraryModel {
    pub fn inbox(&self) -> Option<&Path> {
        self.sourcing.inbox.as_deref()
    }

    pub fn set_inbox(&mut self, inbox: Option<PathBuf>, cx: &mut Context<Self>) {
        self.sourcing.inbox = inbox;
        cx.notify();
    }

    pub fn can_poll(&self, cx: &App) -> bool {
        self.providers(cx).has_a_source()
    }

    pub(super) fn providers(&self, cx: &App) -> Providers {
        self.providers_asking(Asking::EveryProvider, cx)
    }

    fn providers_asking(&self, asking: Asking, cx: &App) -> Providers {
        match cx.try_global::<ResonateApp>() {
            Some(global) => self.sourcing.providers(&global.online, asking),
            None => self.sourcing.providers(&Online::default(), asking),
        }
    }

    pub(super) fn fetched_by(&self, cx: &App) -> Fetcher {
        if self.providers(cx).has_a_source() {
            Fetcher::AProvider
        } else {
            Fetcher::Nobody
        }
    }

    pub fn is_polling(&self) -> bool {
        self.work.polling().is_some()
    }

    pub fn is_stopping_poll(&self) -> bool {
        self.work
            .polling()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn is_waiting_on_the_meter(&self) -> bool {
        self.work
            .polling()
            .is_some_and(|progress| progress.is_waiting_on_the_meter())
    }

    pub fn poll_stats(&self) -> Option<PollStats> {
        self.work.polling().map(|progress| progress.snapshot())
    }

    pub fn poll(&mut self, cx: &mut Context<Self>) {
        self.poll_as(Prompted::ByHand, cx);
    }

    pub(super) fn fetch_or_say_nobody_can(&mut self, cx: &mut Context<Self>) {
        if self.providers(cx).has_a_source() {
            self.fetch_what_was_wanted(PollOptions::default(), cx);
        } else {
            toast::tell(Notice::Trouble(words::models::no_provider().to_owned()), cx);
        }
    }

    pub(super) fn fetch_what_was_wanted(&mut self, options: PollOptions, cx: &mut Context<Self>) {
        if !self.poll_with(Prompted::OnItsOwn, options, cx) {
            self.owe_a_poll(options);
        }
    }

    fn owe_a_poll(&mut self, options: PollOptions) {
        self.poll_owed = Some(match self.poll_owed {
            Some(owed) if owed.every_want => owed,
            Some(_) | None => options,
        });
    }

    pub(super) fn poll_as(&mut self, prompted: Prompted, cx: &mut Context<Self>) -> bool {
        let options = match prompted {
            Prompted::ByHand | Prompted::ByTheInbox => PollOptions::ASKING_EVERY_WANT,
            Prompted::OnItsOwn => PollOptions::default(),
        };
        self.poll_with(prompted, options, cx)
    }

    fn poll_with(
        &mut self,
        prompted: Prompted,
        options: PollOptions,
        cx: &mut Context<Self>,
    ) -> bool {
        if prompted == Prompted::OnItsOwn
            && !options.every_want
            && self.polling_every_provider
            && self
                .work
                .polling()
                .is_some_and(|progress| progress.wants_changed())
        {
            return true;
        }
        if self.work.is_busy() {
            if prompted == Prompted::ByHand {
                toast::tell(
                    Notice::Trouble(words::models::already_walking().to_owned()),
                    cx,
                );
                cx.notify();
            }
            return false;
        }
        let asking = match prompted {
            Prompted::ByTheInbox => Asking::TheInboxAlone,
            Prompted::ByHand | Prompted::OnItsOwn => Asking::EveryProvider,
        };
        let providers = Arc::new(self.providers_asking(asking, cx));
        if prompted != Prompted::ByHand && !self.worth_asking_on_its_own(&providers, options) {
            return true;
        }
        self.library.deliver_into(delivery_folder(cx));
        let handle = match self.library.poll(providers, options) {
            Ok(handle) => handle,
            Err(resonate_library::Error::AlreadyAsking { .. }) if self.keeping.is_some() => {
                tracing::debug!("the keep pass gives way, and this poll follows it");
                self.owe_a_poll(options);
                return false;
            }
            Err(resonate_library::Error::AlreadyAsking { .. }) => {
                tracing::debug!("another process is asking the providers; this poll waits for it");
                if prompted == Prompted::ByHand {
                    toast::tell(
                        Notice::Trouble(words::models::already_walking().to_owned()),
                        cx,
                    );
                    cx.notify();
                }
                self.ask_what_is_owed_after(OWED_ASKED_AGAIN_AFTER, cx);
                return false;
            }
            Err(error) => {
                tracing::error!(%error, "the providers could not be asked");
                toast::tell(
                    toast::could_not(words::models::ask_the_providers(), &error),
                    cx,
                );
                cx.notify();
                return true;
            }
        };

        self.work = Work::Polling(Arc::clone(handle.progress()));
        self.polling_every_provider = asking == Asking::EveryProvider;
        cx.notify();

        self._poll = cx.spawn(async move |this, cx| {
            let mut followed = Followed::default();
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                let reread = followed.reread(handle.progress().snapshot());
                let ticked = this.update(cx, |this, cx| {
                    if let Some(wanted) = reread {
                        this.read(wanted, cx);
                    }
                    cx.notify();
                });
                if ticked.is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                if handle.progress().owes_a_poll() {
                    this.poll_owed.get_or_insert(PollOptions::default());
                }
                this.take_up_what_waited(cx);
                this.keep_what_is_owed(cx);
                let joined = handle.join();
                if let Some(notice) = this.sourcing.unkept_notice() {
                    toast::tell(notice, cx);
                }
                match joined {
                    Ok(summary) => {
                        if summary.stats.kept > 0 {
                            this.enrich_new_songs(cx);
                        }
                        if !summary.cancelled {
                            this.providers_unheard =
                                summary.stats.refused > 0 || summary.stats.late > 0;
                            this.unheard_in_a_row =
                                if this.providers_unheard && summary.stats.kept == 0 {
                                    this.unheard_in_a_row.saturating_add(1)
                                } else {
                                    0
                                };
                        }
                        let told_by_the_downloads =
                            prompted == Prompted::OnItsOwn && !this.downloads.is_empty();
                        if let Some(notice) =
                            polled(&summary, prompted).filter(|_| !told_by_the_downloads)
                        {
                            toast::tell(notice, cx);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "the providers were not asked");
                        toast::tell(
                            toast::could_not(words::models::ask_the_providers(), &error),
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
