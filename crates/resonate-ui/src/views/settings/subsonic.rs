use gpui::{
    App, AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Window,
};

use crate::{
    Notice, ResonateApp, Setting,
    settings::Online,
    views::{
        field::{Field, Submitted},
        kit,
        root::RootView,
        settings::note,
    },
};

const SUBSONIC_NOTE: &str = "A Subsonic server of your own — Navidrome, Airsonic, Gonic — is \
                             asked for every track marked wanted, by the recording's \
                             MusicBrainz id or its ISRC and never by a title, and what it holds \
                             is kept in the vault. Asked as soon as all three are given, and \
                             only while Online is on; the password is sent as a salted token, \
                             never as it was typed.";

const NOT_A_SERVER: &str = "A server's address starts with http:// or https://, as \
                            http://music.local:4533";

pub(super) fn reads_as_a_server(given: &str) -> bool {
    let lowered = given.to_ascii_lowercase();
    ["http://", "https://"].iter().any(|scheme| {
        lowered
            .strip_prefix(scheme)
            .is_some_and(|rest| rest.chars().next().is_some_and(|first| first != '/'))
    })
}

pub(super) fn not_a_server() -> Notice {
    Notice::Trouble(NOT_A_SERVER.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Account {
    Server,
    User,
    Password,
}

impl Account {
    pub(crate) const ALL: [Self; 3] = [Self::Server, Self::User, Self::Password];

    const fn placeholder(self) -> &'static str {
        match self {
            Self::Server => "The server's address, as http://music.local:4533, then press enter",
            Self::User => "The name you sign in with, then press enter",
            Self::Password => "Its password, then press enter",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::User => "User",
            Self::Password => "Password",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Server => "subsonic-server",
            Self::User => "subsonic-user",
            Self::Password => "subsonic-password",
        }
    }

    const fn nth(self) -> usize {
        match self {
            Self::Server => 0,
            Self::User => 1,
            Self::Password => 2,
        }
    }

    fn held(self, online: &Online) -> String {
        match self {
            Self::Server => online.subsonic.clone(),
            Self::User => online.subsonic_user.clone(),
            Self::Password => online.subsonic_password.clone(),
        }
    }

    fn hold(self, online: &mut Online, given: String) {
        match self {
            Self::Server => online.subsonic = given,
            Self::User => online.subsonic_user = given,
            Self::Password => online.subsonic_password = given,
        }
    }

    fn setting(self, given: String) -> Setting {
        match self {
            Self::Server => Setting::Subsonic(given),
            Self::User => Setting::SubsonicUser(given),
            Self::Password => Setting::SubsonicPassword(given),
        }
    }

    pub(crate) fn fields(
        online: &Online,
        window: &mut Window,
        cx: &mut Context<RootView>,
    ) -> [Entity<Field>; 3] {
        Self::ALL.map(|account| {
            let field = cx.new(|cx| {
                let field = Field::new(account.placeholder(), window, cx);
                let mut field = match account {
                    Self::Password => field.masked(),
                    Self::Server | Self::User => field,
                };
                field.hold(account.held(online), cx);
                field
            });
            cx.subscribe_in(&field, window, move |this, _, _: &Submitted, window, cx| {
                this.account_given(account, window, cx);
            })
            .detach();
            field
        })
    }
}

impl RootView {
    fn account_field(&self, account: Account) -> &Entity<Field> {
        &self.subsonic[account.nth()]
    }

    fn account_given(&mut self, account: Account, window: &mut Window, cx: &mut Context<Self>) {
        let given = self
            .account_field(account)
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if account == Account::Server && !given.is_empty() && !reads_as_a_server(&given) {
            self.report(not_a_server(), cx);
            return;
        }
        self.account_field(account)
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            account.hold(&mut global.online, given.clone())
        });

        let said = if given.is_empty() {
            "The Subsonic server is no longer asked"
        } else {
            "The Subsonic server is asked from now on"
        };
        self.store(&account.setting(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        self.the_sources_moved(cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn the_sources_moved(&mut self, cx: &mut Context<Self>) {
        self.library
            .update(cx, |library, cx| library.sources_moved(cx));
    }

    pub(crate) fn leave_the_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.an_account_field_is_focused(window, cx) {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn an_account_field_is_focused(&self, window: &Window, cx: &App) -> bool {
        self.subsonic
            .iter()
            .chain(&self.tidal)
            .any(|field| field.read(cx).is_focused(window))
    }

    pub(crate) fn forget_the_account(&mut self, cx: &mut Context<Self>) {
        for account in Account::ALL {
            self.account_field(account)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                account.hold(&mut global.online, String::new());
            });
            self.store(&account.setting(String::new()), cx);
        }
    }

    pub(super) fn subsonic_group(&mut self, cx: &mut Context<Self>) -> Div {
        let mut body = kit::section_body();
        for account in Account::ALL {
            body = body.child(kit::field(
                account.label(),
                self.key_field(
                    account.id(),
                    self.account_field(account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }
        body.child(note(SUBSONIC_NOTE))
    }
}
