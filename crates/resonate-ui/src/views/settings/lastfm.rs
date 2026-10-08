use gpui::{
    App, AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Task,
    Window,
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
        settings::{action, note},
    },
};

const SIGN_IN_NOTE: &str = "Sign in asks Last.fm for a session under the key and secret of an API \
                            account you made on last.fm, with the user name and password typed \
                            here. The password is sent once and never kept; the session key is \
                            kept in the settings file, and what is heard is scrobbled under it \
                            within half a minute, beside ListenBrainz where a token is given.";

const SIGNED_IN_NOTE: &str = "A Last.fm session is kept: what is heard is scrobbled to Last.fm \
                              within half a minute. Signing out forgets the session, not the \
                              key and secret.";

const OFFLINE: &str = "Last.fm is signed in to only while Online is on";

const NOTHING_TO_SIGN_IN_WITH: &str =
    "Give the API key and secret, the user name and the password first";

const REFUSED: &str =
    "Last.fm refused the sign-in: the user name, the password, or the API key and secret";

const UNREACHED: &str = "Last.fm could not be reached to sign in";

const SIGNED_OUT: &str = "Nothing heard is scrobbled to Last.fm from now on";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LastfmAccount {
    Key,
    Secret,
    User,
    Password,
}

#[derive(Default)]
pub(crate) struct LastfmSigning {
    asking: bool,
    task: Option<Task<()>>,
}

impl LastfmSigning {
    #[cfg(test)]
    pub(crate) const fn is_asking(&self) -> bool {
        self.asking
    }
}

impl LastfmAccount {
    pub(crate) const ALL: [Self; 4] = [Self::Key, Self::Secret, Self::User, Self::Password];
    const KEPT: [Self; 2] = [Self::Key, Self::Secret];
    const TYPED_TO_SIGN_IN: [Self; 2] = [Self::User, Self::Password];

    const fn placeholder(self) -> &'static str {
        match self {
            Self::Key => "The API key of your Last.fm API account, then press enter",
            Self::Secret => "Its shared secret, then press enter",
            Self::User => "Your Last.fm user name",
            Self::Password => "Your Last.fm password, then press enter to sign in",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Key => "API key",
            Self::Secret => "Shared secret",
            Self::User => "User name",
            Self::Password => "Password",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Key => "lastfm-key",
            Self::Secret => "lastfm-secret",
            Self::User => "lastfm-user",
            Self::Password => "lastfm-password",
        }
    }

    const fn nth(self) -> usize {
        match self {
            Self::Key => 0,
            Self::Secret => 1,
            Self::User => 2,
            Self::Password => 3,
        }
    }

    fn held(self, online: &Online) -> String {
        match self {
            Self::Key => online.lastfm_key.clone(),
            Self::Secret => online.lastfm_secret.clone(),
            Self::User | Self::Password => String::new(),
        }
    }

    fn hold(self, online: &mut Online, given: String) {
        match self {
            Self::Key => online.lastfm_key = given,
            Self::Secret => online.lastfm_secret = given,
            Self::User | Self::Password => {}
        }
    }

    fn setting(self, given: String) -> Option<Setting> {
        match self {
            Self::Key => Some(Setting::LastfmKey(given)),
            Self::Secret => Some(Setting::LastfmSecret(given)),
            Self::User | Self::Password => None,
        }
    }

    pub(crate) fn fields(
        online: &Online,
        window: &mut Window,
        cx: &mut Context<RootView>,
    ) -> [Entity<Field>; 4] {
        Self::ALL.map(|account| {
            let field = cx.new(|cx| {
                let field = Field::new(account.placeholder(), window, cx);
                let mut field = match account {
                    Self::Key | Self::Secret | Self::Password => field.masked(),
                    Self::User => field,
                };
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
        if LastfmAccount::TYPED_TO_SIGN_IN.contains(&account) {
            self.sign_in_to_lastfm(cx);
            window.focus(&self.focus);
            return;
        }
        self.keep_the_typed_application(cx);
        let said = if self.lastfm_field(account).read(cx).text().trim().is_empty() {
            "Last.fm is not scrobbled to without its API key and secret"
        } else {
            "Kept for signing in to Last.fm"
        };
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
        cx.notify();
    }

    fn keep_the_typed_application(&mut self, cx: &mut Context<Self>) {
        for account in LastfmAccount::KEPT {
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
            if let Some(setting) = account.setting(given) {
                self.store(&setting, cx);
            }
        }
    }

    pub(super) fn give_what_is_typed_of_the_lastfm_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for account in LastfmAccount::KEPT {
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
            user: typed(LastfmAccount::User)?,
            password: typed(LastfmAccount::Password)?,
        })
    }

    fn sign_in_to_lastfm(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(scrobblers) = app.scrobblers.clone().filter(|_| app.online.enabled) else {
            self.report(Notice::Trouble(OFFLINE.to_owned()), cx);
            return;
        };
        let Some(asked) = self.lastfm_sign_in(cx) else {
            self.report(Notice::Trouble(NOTHING_TO_SIGN_IN_WITH.to_owned()), cx);
            return;
        };
        if self.lastfm_signing.asking {
            return;
        }

        self.keep_the_typed_application(cx);
        self.lastfm_signing.asking = true;
        cx.notify();
        self.lastfm_signing.task = Some(cx.spawn(async move |this, cx| {
            let answered = cx
                .background_executor()
                .spawn(async move { scrobblers.signed_in_to_lastfm(&asked) })
                .await;
            let told = this.update(cx, |this, cx| this.lastfm_signed_in(answered, cx));
            let _ = told;
        }));
    }

    fn lastfm_signed_in(
        &mut self,
        answered: resonate_library::Result<LastfmSession>,
        cx: &mut Context<Self>,
    ) {
        self.lastfm_signing.asking = false;
        self.lastfm_field(LastfmAccount::Password)
            .clone()
            .update(cx, |field, cx| field.hold(String::new(), cx));
        let notice = match answered {
            Ok(LastfmSession { name, key }) => {
                cx.update_global::<ResonateApp, _>(|global, _| {
                    global.online.lastfm_session = key.clone();
                });
                self.store(&Setting::LastfmSession(key), cx);
                Notice::Done(format!(
                    "Signed in to Last.fm as {name} — what is heard from now on is scrobbled there"
                ))
            }
            Err(Error::Refused { .. }) => Notice::Trouble(REFUSED.to_owned()),
            Err(error) => {
                tracing::warn!(%error, "the Last.fm sign-in did not finish");
                Notice::Trouble(UNREACHED.to_owned())
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
        self.report(Notice::Done(SIGNED_OUT.to_owned()), cx);
        cx.notify();
    }

    pub(super) fn lastfm_group(&mut self, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        let signed_in = !app.online.lastfm_session.trim().is_empty();
        let reachable = app.scrobblers.is_some() && app.online.enabled;
        let asking = self.lastfm_signing.asking;

        let shown: &[LastfmAccount] = if signed_in {
            &LastfmAccount::KEPT
        } else {
            &LastfmAccount::ALL
        };
        let mut body = kit::section_body();
        for account in shown {
            body = body.child(kit::field(
                account.label(),
                self.key_field(
                    account.id(),
                    self.lastfm_field(*account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }

        if signed_in {
            return body
                .child(action(
                    "lastfm-sign-out",
                    "Sign out of Last.fm",
                    Icon::Close,
                    false,
                    |this, _, cx| this.sign_out_of_lastfm(cx),
                    self,
                    cx,
                ))
                .child(note(SIGNED_IN_NOTE));
        }
        let held_back = !reachable || asking || self.lastfm_sign_in(cx).is_none();
        body.child(action(
            "lastfm-sign-in",
            if asking {
                "Asking Last.fm…"
            } else {
                "Sign in to Last.fm"
            },
            Icon::Link,
            held_back,
            |this, _, cx| this.sign_in_to_lastfm(cx),
            self,
            cx,
        ))
        .child(note(SIGN_IN_NOTE))
    }
}
