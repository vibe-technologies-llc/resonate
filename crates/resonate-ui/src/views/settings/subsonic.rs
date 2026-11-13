use gpui::{
    App, AppContext as _, BorrowAppContext as _, Context, Entity, ParentElement as _, Window,
};

use crate::{
    Notice, ResonateApp, Setting,
    settings::Online,
    views::{
        field::Field,
        kit,
        root::RootView,
        settings::{Entered, Rows, SettingsFields},
    },
    words,
};

pub(super) fn reads_as_a_server(given: &str) -> bool {
    let lowered = given.to_ascii_lowercase();
    ["http://", "https://"].iter().any(|scheme| {
        lowered
            .strip_prefix(scheme)
            .is_some_and(|rest| rest.chars().next().is_some_and(|first| first != '/'))
    })
}

pub(super) fn not_a_server() -> Notice {
    Notice::Trouble(words::settings_subsonic::not_a_server().to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Service {
    Subsonic,
    Jellyfin,
}

impl Service {
    pub(crate) const ALL: [Self; 2] = [Self::Subsonic, Self::Jellyfin];

    fn name(self) -> &'static str {
        match self {
            Self::Subsonic => "Subsonic",
            Self::Jellyfin => "Jellyfin",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Subsonic => words::settings_subsonic::subsonic_note(),
            Self::Jellyfin => words::settings_subsonic::jellyfin_note(),
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

    fn placeholder(self, service: Service) -> &'static str {
        match (service, self) {
            (Service::Subsonic, Self::Server) => {
                words::settings_subsonic::the_servers_address_as_http_music()
            }
            (Service::Jellyfin, Self::Server) => {
                words::settings_subsonic::the_servers_address_as_http_music_2()
            }
            (_, Self::User) => words::settings_subsonic::the_name_you_sign_in_with(),
            (_, Self::Password) => words::settings_subsonic::its_password_then_press_enter(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Server => words::settings_subsonic::server(),
            Self::User => words::settings_subsonic::user(),
            Self::Password => words::settings_subsonic::password(),
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
        cx: &mut Context<SettingsFields>,
    ) -> [Entity<Field>; 3] {
        Self::ALL.map(|account| {
            let field = cx.new(|cx| {
                let field = Field::new(move || account.placeholder(service), window, cx);
                let mut field = match account {
                    Self::Password => field.masked(),
                    Self::Server | Self::User => field,
                };
                field.hold(account.held(service, online), cx);
                field
            });
            SettingsFields::says_when_entered(&field, Entered::Account(service, account), cx);
            field
        })
    }
}

impl RootView {
    fn account_field(&self, service: Service, account: Account, cx: &App) -> Entity<Field> {
        let fields = self.settings_fields.read(cx);
        match service {
            Service::Subsonic => fields.subsonic[account.nth()].clone(),
            Service::Jellyfin => fields.jellyfin[account.nth()].clone(),
        }
    }

    pub(crate) fn account_given(
        &mut self,
        service: Service,
        account: Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let given = self
            .account_field(service, account, cx)
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if account == Account::Server && !given.is_empty() && !reads_as_a_server(&given) {
            self.report(not_a_server(), cx);
            return;
        }
        self.account_field(service, account, cx)
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            account.hold(service, &mut global.online, given.clone())
        });

        let said = if given.is_empty() {
            words::settings_subsonic::server_no_longer_asked(service.name())
        } else {
            words::settings_subsonic::server_asked_from_now_on(service.name())
        };
        let lets_the_session_go = service == Service::Jellyfin
            && (account == Account::Server || (account == Account::Password && !given.is_empty()));
        self.store(&account.setting(service, given), cx);
        if lets_the_session_go {
            self.let_go_of_the_jellyfin_session(cx);
        }
        if service == Service::Jellyfin && account == Account::Server {
            self.stop_quick_connect(cx);
        }
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
            let field = self.account_field(service, account, cx).clone();
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
            let field = self.account_field(service, account, cx).read(cx);
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
        let fields = self.settings_fields.read(cx);
        fields
            .subsonic
            .iter()
            .chain(&fields.jellyfin)
            .chain(&fields.tidal)
            .chain(&fields.lastfm)
            .chain(&fields.youtube_music)
            .chain(std::iter::once(&fields.spotify_client))
            .any(|field| field.read(cx).is_focused(window))
    }

    fn every_account() -> impl Iterator<Item = (Service, Account)> {
        Service::ALL
            .into_iter()
            .flat_map(|service| Account::ALL.map(|account| (service, account)))
    }

    pub(crate) fn forget_the_account(&mut self, service: Service, cx: &mut Context<Self>) {
        for account in Account::ALL {
            self.account_field(service, account, cx)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                account.hold(service, &mut global.online, String::new());
            });
            self.store(&account.setting(service, String::new()), cx);
        }
        if service == Service::Jellyfin {
            self.let_go_of_the_jellyfin_session(cx);
        }
    }

    pub(super) fn account_group(&mut self, service: Service, cx: &mut Context<Self>) -> Rows {
        let mut body = kit::rows();
        for account in Account::ALL {
            body = body.child(kit::field(
                account.label(),
                self.text_field(
                    account.id(service),
                    &self.account_field(service, account, cx),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }
        if service == Service::Jellyfin {
            body = body.child(self.quick_connect_row(cx));
        }
        body.note(service.note())
    }
}
