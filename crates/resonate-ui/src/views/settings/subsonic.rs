use gpui::{
    App, AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Window,
};

use crate::{
    Notice, ResonateApp, Setting,
    icons::Icon,
    settings::Online,
    views::{
        field::{Field, Submitted},
        kit,
        root::{Pane, RootView},
        settings::{action, note},
    },
};

const SERVED_NOTE: &str = "Opens the pane that browses what a service plays in place, and plays \
                           it from there. It is there while a service of yours is set up and \
                           Online is on.";

const NO_SERVICE_SERVED: &str = "No service that plays in place is set up. Give a Subsonic or \
                                 Jellyfin server, or a TIDAL, Monochrome or hifi-api account, \
                                 and turn Online on.";

const SUBSONIC_NOTE: &str = "A Subsonic server of your own — Navidrome, Airsonic, Gonic — is \
                             asked for every track marked wanted, by the recording's \
                             MusicBrainz id or its ISRC and never by a title, and what it holds \
                             is kept in the vault. Asked as soon as all three are given, and \
                             only while Online is on; the password is sent as a salted token, \
                             never as it was typed.";

const JELLYFIN_NOTE: &str = "A Jellyfin or Emby server of your own is browsed and played where \
                             it is, in the server pane, with nothing downloaded. Asked as soon \
                             as all three are given, and only while Online is on; the password \
                             is sent to sign in and the token it answers with is what every \
                             later request carries.";

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
pub(crate) enum Service {
    Subsonic,
    Jellyfin,
}

impl Service {
    pub(crate) const ALL: [Self; 2] = [Self::Subsonic, Self::Jellyfin];

    const fn name(self) -> &'static str {
        match self {
            Self::Subsonic => "Subsonic",
            Self::Jellyfin => "Jellyfin",
        }
    }

    const fn note(self) -> &'static str {
        match self {
            Self::Subsonic => SUBSONIC_NOTE,
            Self::Jellyfin => JELLYFIN_NOTE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Account {
    Server,
    User,
    Password,
}

impl Account {
    pub(crate) const ALL: [Self; 3] = [Self::Server, Self::User, Self::Password];

    const fn placeholder(self, service: Service) -> &'static str {
        match (service, self) {
            (Service::Subsonic, Self::Server) => {
                "The server's address, as http://music.local:4533, then press enter"
            }
            (Service::Jellyfin, Self::Server) => {
                "The server's address, as http://music.local:8096, then press enter"
            }
            (_, Self::User) => "The name you sign in with, then press enter",
            (_, Self::Password) => "Its password, then press enter",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::User => "User",
            Self::Password => "Password",
        }
    }

    const fn id(self, service: Service) -> &'static str {
        match (service, self) {
            (Service::Subsonic, Self::Server) => "subsonic-server",
            (Service::Subsonic, Self::User) => "subsonic-user",
            (Service::Subsonic, Self::Password) => "subsonic-password",
            (Service::Jellyfin, Self::Server) => "jellyfin-server",
            (Service::Jellyfin, Self::User) => "jellyfin-user",
            (Service::Jellyfin, Self::Password) => "jellyfin-password",
        }
    }

    const fn nth(self) -> usize {
        match self {
            Self::Server => 0,
            Self::User => 1,
            Self::Password => 2,
        }
    }

    fn held(self, service: Service, online: &Online) -> String {
        match (service, self) {
            (Service::Subsonic, Self::Server) => online.subsonic.clone(),
            (Service::Subsonic, Self::User) => online.subsonic_user.clone(),
            (Service::Subsonic, Self::Password) => online.subsonic_password.clone(),
            (Service::Jellyfin, Self::Server) => online.jellyfin.clone(),
            (Service::Jellyfin, Self::User) => online.jellyfin_user.clone(),
            (Service::Jellyfin, Self::Password) => online.jellyfin_password.clone(),
        }
    }

    fn hold(self, service: Service, online: &mut Online, given: String) {
        match (service, self) {
            (Service::Subsonic, Self::Server) => online.subsonic = given,
            (Service::Subsonic, Self::User) => online.subsonic_user = given,
            (Service::Subsonic, Self::Password) => online.subsonic_password = given,
            (Service::Jellyfin, Self::Server) => online.jellyfin = given,
            (Service::Jellyfin, Self::User) => online.jellyfin_user = given,
            (Service::Jellyfin, Self::Password) => online.jellyfin_password = given,
        }
    }

    fn setting(self, service: Service, given: String) -> Setting {
        match (service, self) {
            (Service::Subsonic, Self::Server) => Setting::Subsonic(given),
            (Service::Subsonic, Self::User) => Setting::SubsonicUser(given),
            (Service::Subsonic, Self::Password) => Setting::SubsonicPassword(given),
            (Service::Jellyfin, Self::Server) => Setting::Jellyfin(given),
            (Service::Jellyfin, Self::User) => Setting::JellyfinUser(given),
            (Service::Jellyfin, Self::Password) => Setting::JellyfinPassword(given),
        }
    }

    pub(crate) fn fields(
        service: Service,
        online: &Online,
        window: &mut Window,
        cx: &mut Context<RootView>,
    ) -> [Entity<Field>; 3] {
        Self::ALL.map(|account| {
            let field = cx.new(|cx| {
                let field = Field::new(account.placeholder(service), window, cx);
                let mut field = match account {
                    Self::Password => field.masked(),
                    Self::Server | Self::User => field,
                };
                field.hold(account.held(service, online), cx);
                field
            });
            cx.subscribe_in(&field, window, move |this, _, _: &Submitted, window, cx| {
                this.account_given(service, account, window, cx);
            })
            .detach();
            field
        })
    }
}

impl RootView {
    fn account_field(&self, service: Service, account: Account) -> &Entity<Field> {
        match service {
            Service::Subsonic => &self.subsonic[account.nth()],
            Service::Jellyfin => &self.jellyfin[account.nth()],
        }
    }

    fn account_given(
        &mut self,
        service: Service,
        account: Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let given = self
            .account_field(service, account)
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if account == Account::Server && !given.is_empty() && !reads_as_a_server(&given) {
            self.report(not_a_server(), cx);
            return;
        }
        self.account_field(service, account)
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            account.hold(service, &mut global.online, given.clone())
        });

        let said = if given.is_empty() {
            format!("The {} server is no longer asked", service.name())
        } else {
            format!("The {} server is asked from now on", service.name())
        };
        self.store(&account.setting(service, given), cx);
        self.report(Notice::Done(said), cx);
        self.the_sources_moved(cx);
        window.focus(&self.focus, cx);
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
        if self.give_what_is_typed_of_the_account(window, cx) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_the_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (service, account) in Self::every_account() {
            let stored = account.held(service, &cx.global::<ResonateApp>().online);
            let field = self.account_field(service, account).clone();
            if field.read(cx).is_focused(window) {
                field.update(cx, |field, cx| field.hold(stored, cx));
            }
        }
        self.put_back_the_tidal_account(window, cx);
        self.put_back_the_lastfm_account(window, cx);
        self.put_back_the_spotify_client(window, cx);
        self.put_back_youtube_music(window, cx);
        self.leave_the_account(window, cx);
    }

    fn give_what_is_typed_of_the_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for (service, account) in Self::every_account() {
            let field = self.account_field(service, account).read(cx);
            let typed_apart = field.is_focused(window)
                && field.text().trim() != account.held(service, &cx.global::<ResonateApp>().online);
            if typed_apart {
                self.account_given(service, account, window, cx);
                return true;
            }
        }
        self.give_what_is_typed_of_the_tidal_account(window, cx)
            || self.give_what_is_typed_of_the_lastfm_account(window, cx)
            || self.give_what_is_typed_of_the_spotify_client(window, cx)
            || self.give_what_is_typed_of_youtube_music(window, cx)
    }

    pub(crate) fn an_account_field_is_focused(&self, window: &Window, cx: &App) -> bool {
        self.subsonic
            .iter()
            .chain(&self.jellyfin)
            .chain(&self.tidal)
            .chain(&self.lastfm)
            .chain(&self.youtube_music)
            .chain(std::iter::once(&self.spotify_client))
            .any(|field| field.read(cx).is_focused(window))
    }

    fn every_account() -> impl Iterator<Item = (Service, Account)> {
        Service::ALL
            .into_iter()
            .flat_map(|service| Account::ALL.map(|account| (service, account)))
    }

    pub(crate) fn forget_the_account(&mut self, service: Service, cx: &mut Context<Self>) {
        for account in Account::ALL {
            self.account_field(service, account)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                account.hold(service, &mut global.online, String::new());
            });
            self.store(&account.setting(service, String::new()), cx);
        }
    }

    pub(super) fn account_group(&mut self, service: Service, cx: &mut Context<Self>) -> Div {
        let mut body = kit::section_body();
        for account in Account::ALL {
            body = body.child(kit::field(
                account.label(),
                self.key_field(
                    account.id(service),
                    self.account_field(service, account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }
        body.child(note(service.note()))
    }

    pub(super) fn served_group(&mut self, cx: &mut Context<Self>) -> Div {
        let serving = self.server.read(cx).is_open();
        let body = kit::section_body();

        if !serving {
            return body.child(note(NO_SERVICE_SERVED));
        }

        body.child(action(
            "open-server",
            "Open the server",
            Icon::Server,
            false,
            |this, _, cx| this.choose_pane(Pane::Server, cx),
            self,
            cx,
        ))
        .child(note(SERVED_NOTE))
    }
}
