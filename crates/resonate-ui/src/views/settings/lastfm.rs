use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    App, AppContext as _, BorrowAppContext as _, Context, Entity, ParentElement as _, Task, Window,
};
use resonate_library::{Error, LastfmSession, LastfmSignIn};

use crate::{
    Notice, ResonateApp, Setting,
    icons::Icon,
    settings::Online,
    views::{
        field::{Field, Submitted},
        kit,
        root::RootView,
        settings::{Rows, action},
    },
    words,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LastfmAccount {
    Key,
    Secret,
}

#[derive(Default)]
pub(crate) struct LastfmSigning {
    asking: bool,
    waiting: Option<String>,
    stopped: Arc<AtomicBool>,
    task: Option<Task<()>>,
}

impl LastfmSigning {
    #[cfg(test)]
    pub(crate) const fn is_asking(&self) -> bool {
        self.asking
    }
}

impl LastfmAccount {
    pub(crate) const ALL: [Self; 2] = [Self::Key, Self::Secret];

    fn placeholder(self) -> &'static str {
        match self {
            Self::Key => words::settings_lastfm::the_api_key_of_your_last(),
            Self::Secret => words::settings_lastfm::its_shared_secret_then_press_enter(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Key => words::settings_lastfm::api_key(),
            Self::Secret => words::settings_lastfm::shared_secret(),
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Key => "lastfm-key",
            Self::Secret => "lastfm-secret",
        }
    }

    const fn nth(self) -> usize {
        match self {
            Self::Key => 0,
            Self::Secret => 1,
        }
    }

    fn held(self, online: &Online) -> String {
        match self {
            Self::Key => online.lastfm_key.clone(),
            Self::Secret => online.lastfm_secret.clone(),
        }
    }

    fn hold(self, online: &mut Online, given: String) {
        match self {
            Self::Key => online.lastfm_key = given,
            Self::Secret => online.lastfm_secret = given,
        }
    }

    fn setting(self, given: String) -> Setting {
        match self {
            Self::Key => Setting::LastfmKey(given),
            Self::Secret => Setting::LastfmSecret(given),
        }
    }

    pub(crate) fn fields(
        online: &Online,
        window: &mut Window,
        cx: &mut Context<RootView>,
    ) -> [Entity<Field>; 2] {
        Self::ALL.map(|account| {
            let field = cx.new(|cx| {
                let mut field = Field::new(account.placeholder(), window, cx).masked();
                field.hold(account.held(online), cx);
                field
            });
            cx.subscribe_in(&field, window, move |this, _, _: &Submitted, window, cx| {
                this.lastfm_given(account, window, cx);
            })
            .detach();
            field
        })
    }
}

impl RootView {
    fn lastfm_field(&self, account: LastfmAccount) -> &Entity<Field> {
        &self.lastfm[account.nth()]
    }

    fn lastfm_given(
        &mut self,
        account: LastfmAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keep_the_typed_application(cx);
        let said = if self.lastfm_field(account).read(cx).text().trim().is_empty() {
            words::settings_lastfm::last_fm_is_not_scrobbled_to()
        } else {
            words::settings_lastfm::kept_for_signing_in_to_last()
        };
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn keep_the_typed_application(&mut self, cx: &mut Context<Self>) {
        for account in LastfmAccount::ALL {
            let given = self.lastfm_field(account).read(cx).text().trim().to_owned();
            if given == account.held(&cx.global::<ResonateApp>().online) {
                continue;
            }
            self.lastfm_field(account)
                .clone()
                .update(cx, |field, cx| field.hold(given.clone(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                account.hold(&mut global.online, given.clone());
            });
            self.store(&account.setting(given), cx);
        }
    }

    pub(super) fn give_what_is_typed_of_the_lastfm_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for account in LastfmAccount::ALL {
            let field = self.lastfm_field(account).read(cx);
            let typed_apart = field.is_focused(window)
                && field.text().trim() != account.held(&cx.global::<ResonateApp>().online);
            if typed_apart {
                self.lastfm_given(account, window, cx);
                return true;
            }
        }
        false
    }

    pub(super) fn put_back_the_lastfm_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for account in LastfmAccount::ALL {
            let stored = account.held(&cx.global::<ResonateApp>().online);
            let field = self.lastfm_field(account).clone();
            if field.read(cx).is_focused(window) {
                field.update(cx, |field, cx| field.hold(stored, cx));
            }
        }
    }

    pub(crate) fn forget_the_lastfm_account(&mut self, cx: &mut Context<Self>) {
        for account in LastfmAccount::ALL {
            self.lastfm_field(account)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
        }
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.lastfm_key = String::new();
            global.online.lastfm_secret = String::new();
            global.online.lastfm_session = String::new();
        });
        self.store(&Setting::LastfmKey(String::new()), cx);
        self.store(&Setting::LastfmSecret(String::new()), cx);
        self.store(&Setting::LastfmSession(String::new()), cx);
    }

    fn lastfm_sign_in(&self, cx: &App) -> Option<LastfmSignIn> {
        let typed = |account| {
            let typed = self.lastfm_field(account).read(cx).text().trim().to_owned();
            (!typed.is_empty()).then_some(typed)
        };
        Some(LastfmSignIn {
            key: typed(LastfmAccount::Key)?,
            secret: typed(LastfmAccount::Secret)?,
        })
    }

    fn sign_in_to_lastfm(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(scrobblers) = app.scrobblers.clone().filter(|_| app.online.enabled) else {
            self.report(
                Notice::Trouble(words::settings_lastfm::offline().to_owned()),
                cx,
            );
            return;
        };
        let Some(asked) = self.lastfm_sign_in(cx) else {
            self.report(
                Notice::Trouble(words::settings_lastfm::nothing_to_sign_in_with().to_owned()),
                cx,
            );
            return;
        };
        if self.lastfm_signing.asking {
            return;
        }

        self.keep_the_typed_application(cx);
        let stopped = Arc::new(AtomicBool::new(false));
        self.lastfm_signing.stopped = Arc::clone(&stopped);
        self.lastfm_signing.asking = true;
        cx.notify();
        self.lastfm_signing.task = Some(cx.spawn(async move |this, cx| {
            let asked_for = cx
                .background_executor()
                .spawn(async move { scrobblers.lastfm_approval(&asked) })
                .await;
            let approval = match asked_for {
                Ok(approval) => approval,
                Err(error) => {
                    let told = this.update(cx, |this, cx| this.lastfm_signed_in(Err(error), cx));
                    let _ = told;
                    return;
                }
            };
            let address = approval.address().to_owned();
            let shown = this.update(cx, |this, cx| {
                this.lastfm_signing.waiting = Some(address.clone());
                cx.open_url(&address);
                cx.notify();
            });
            if shown.is_err() {
                return;
            }
            let answered = cx
                .background_executor()
                .spawn(async move { approval.approved(&|| stopped.load(Ordering::SeqCst)) })
                .await;
            let told = this.update(cx, |this, cx| this.lastfm_signed_in(answered, cx));
            let _ = told;
        }));
    }

    fn stop_signing_in_to_lastfm(&mut self, cx: &mut Context<Self>) {
        self.lastfm_signing.stopped.store(true, Ordering::SeqCst);
        self.lastfm_signing.asking = false;
        self.lastfm_signing.waiting = None;
        self.lastfm_signing.task = None;
        cx.notify();
    }

    fn lastfm_signed_in(
        &mut self,
        answered: resonate_library::Result<Option<LastfmSession>>,
        cx: &mut Context<Self>,
    ) {
        self.lastfm_signing.asking = false;
        self.lastfm_signing.waiting = None;
        let notice = match answered {
            Ok(Some(LastfmSession { name, key })) => {
                cx.update_global::<ResonateApp, _>(|global, _| {
                    global.online.lastfm_session = key.clone();
                });
                self.store(&Setting::LastfmSession(key), cx);
                Notice::Done(words::settings_lastfm::signed_in_as(&name))
            }
            Ok(None) if self.lastfm_signing.stopped.load(Ordering::SeqCst) => {
                cx.notify();
                return;
            }
            Ok(None) => Notice::Noted(words::settings_lastfm::ended().to_owned()),
            Err(Error::Refused { .. }) => {
                Notice::Trouble(words::settings_lastfm::refused().to_owned())
            }
            Err(error) => {
                tracing::warn!(%error, "the Last.fm sign-in did not finish");
                Notice::Trouble(words::settings_lastfm::unreached().to_owned())
            }
        };
        self.report(notice, cx);
        cx.notify();
    }

    fn sign_out_of_lastfm(&mut self, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.lastfm_session = String::new();
        });
        self.store(&Setting::LastfmSession(String::new()), cx);
        self.report(
            Notice::Done(words::settings_lastfm::signed_out().to_owned()),
            cx,
        );
        cx.notify();
    }

    pub(super) fn lastfm_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let app = cx.global::<ResonateApp>();
        let signed_in = !app.online.lastfm_session.trim().is_empty();
        let reachable = app.scrobblers.is_some() && app.online.enabled;
        let asking = self.lastfm_signing.asking;

        let mut body = kit::rows();
        for account in LastfmAccount::ALL {
            body = body.child(kit::field(
                account.label(),
                self.text_field(
                    account.id(),
                    self.lastfm_field(account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }

        if signed_in {
            return body
                .child(kit::action_row().child(action(
                    "lastfm-sign-out",
                    words::settings_lastfm::sign_out_of_last_fm(),
                    Icon::Close,
                    false,
                    |this, _, cx| this.sign_out_of_lastfm(cx),
                    self,
                    cx,
                )))
                .note(words::settings_lastfm::signed_in_note());
        }
        if let Some(address) = self.lastfm_signing.waiting.clone() {
            return body
                .child(
                    kit::action_row()
                        .child(action(
                            "lastfm-open-again",
                            words::settings_lastfm::open_the_page_again(),
                            Icon::Link,
                            false,
                            move |_, _, cx| cx.open_url(&address),
                            self,
                            cx,
                        ))
                        .child(action(
                            "lastfm-stop",
                            words::settings_lastfm::stop(),
                            Icon::Close,
                            false,
                            |this, _, cx| this.stop_signing_in_to_lastfm(cx),
                            self,
                            cx,
                        )),
                )
                .note(words::settings_lastfm::waiting_note());
        }
        let held_back = !reachable || asking || self.lastfm_sign_in(cx).is_none();
        body.child(kit::action_row().child(action(
            "lastfm-sign-in",
            if asking {
                words::settings_lastfm::asking_last_fm()
            } else {
                words::settings_lastfm::sign_in_to_last_fm()
            },
            Icon::Link,
            held_back,
            |this, _, cx| this.sign_in_to_lastfm(cx),
            self,
            cx,
        )))
        .note(words::settings_lastfm::sign_in_note())
    }
}
