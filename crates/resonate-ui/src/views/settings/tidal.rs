use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, SharedString,
    Stateful, Styled as _, Task, Window, div,
};
use resonate_core::RESONATE_HOST;
use resonate_providers::{Authorizing, Client, Error, RefreshToken};

use crate::{
    Notice, ResonateApp, Setting, TidalSignIn,
    icons::Icon,
    settings::Online,
    views::{
        field::{Field, Submitted},
        kit,
        root::RootView,
        settings::{
            Rows, action, note,
            subsonic::{not_a_server, reads_as_a_server},
            switch_row,
        },
    },
    words,
};

#[derive(Clone, Debug, Default)]
pub(crate) enum TidalSigning {
    #[default]
    Idle,
    Asking,
    Waiting(Authorizing),
}

pub(crate) struct SigningIn {
    pub(crate) shown: TidalSigning,
    stopped: Arc<AtomicBool>,
    task: Task<()>,
}

impl Default for SigningIn {
    fn default() -> Self {
        Self {
            shown: TidalSigning::Idle,
            stopped: Arc::new(AtomicBool::new(false)),
            task: Task::ready(()),
        }
    }
}

fn turned_away(error: &Error) -> &'static str {
    match error {
        Error::AuthorizationLapsed { .. } => {
            words::settings_tidal::the_tidal_code_lapsed_before_it()
        }
        Error::AuthorizationDenied { .. } => words::settings_tidal::tidal_was_told_not_to_sign(),
        Error::Unwelcome { .. } => words::settings_tidal::tidal_refused_the_client_id(),
        Error::Io { .. } | Error::Unaddressable { .. } | Error::Busy { .. } => {
            words::settings_tidal::tidal_could_not_be_reached()
        }
        Error::Refused { .. } => words::settings_tidal::tidal_refused_the_sign_in(),
        Error::Untrusted { .. } => words::settings_tidal::tidals_certificate_is_not_one_this(),
        Error::StillArriving { .. }
        | Error::Unreadable { .. }
        | Error::TurnedAway { .. }
        | Error::OffItsHosts { .. }
        | Error::StillQueued { .. }
        | Error::NotTheService { .. }
        | Error::NotOffered { .. }
        | Error::ToolFailed { .. }
        | Error::ToolNotRun { .. }
        | Error::NotAnExtension => {
            words::settings_tidal::tidal_answered_something_this_build_cannot()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TidalAccount {
    ClientId,
    ClientSecret,
    RefreshToken,
    HifiApi,
    HifiSecret,
    Monochrome,
}

impl TidalAccount {
    pub(crate) const ALL: [Self; 6] = [
        Self::ClientId,
        Self::ClientSecret,
        Self::RefreshToken,
        Self::HifiApi,
        Self::HifiSecret,
        Self::Monochrome,
    ];

    pub(crate) const fn is_tidals(self) -> bool {
        !matches!(self, Self::HifiSecret)
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::ClientId => words::settings_tidal::the_client_id_the_token_was(),
            Self::ClientSecret => words::settings_tidal::its_client_secret_if_it_has(),
            Self::RefreshToken => words::settings_tidal::a_refresh_token_for_your_account(),
            Self::HifiApi => words::settings_tidal::custom_hifi_api_server_address_blank(),
            Self::HifiSecret => words::settings_tidal::a_key_your_resonate_server_lists(),
            Self::Monochrome => {
                words::settings_tidal::custom_monochrome_server_address_blank_uses()
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ClientId => words::settings_tidal::client_id(),
            Self::ClientSecret => words::settings_tidal::client_secret(),
            Self::RefreshToken => words::settings_tidal::refresh_token(),
            Self::HifiApi => words::settings_tidal::hifi_api_server(),
            Self::HifiSecret => words::settings_tidal::key(),
            Self::Monochrome => words::settings_tidal::monochrome_server(),
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::ClientId => "tidal-client-id",
            Self::ClientSecret => "tidal-client-secret",
            Self::RefreshToken => "tidal-refresh-token",
            Self::HifiApi => "hifi-api",
            Self::HifiSecret => "hifi-secret",
            Self::Monochrome => "monochrome",
        }
    }

    pub(crate) const fn nth(self) -> usize {
        match self {
            Self::ClientId => 0,
            Self::ClientSecret => 1,
            Self::RefreshToken => 2,
            Self::HifiApi => 3,
            Self::HifiSecret => 4,
            Self::Monochrome => 5,
        }
    }

    fn held(self, online: &Online) -> String {
        match self {
            Self::ClientId => online.tidal_client_id.clone(),
            Self::ClientSecret => online.tidal_client_secret.clone(),
            Self::RefreshToken => online.tidal_refresh_token.clone(),
            Self::HifiApi => online.hifi_api.clone(),
            Self::HifiSecret => online.hifi_secret.clone(),
            Self::Monochrome => online.monochrome.clone(),
        }
    }

    fn hold(self, online: &mut Online, given: String) {
        match self {
            Self::ClientId => online.tidal_client_id = given,
            Self::ClientSecret => online.tidal_client_secret = given,
            Self::RefreshToken => online.tidal_refresh_token = given,
            Self::HifiApi => online.hifi_api = given,
            Self::HifiSecret => online.hifi_secret = given,
            Self::Monochrome => online.monochrome = given,
        }
    }

    fn setting(self, given: String) -> Setting {
        match self {
            Self::ClientId => Setting::TidalClientId(given),
            Self::ClientSecret => Setting::TidalClientSecret(given),
            Self::RefreshToken => Setting::TidalRefreshToken(given),
            Self::HifiApi => Setting::HifiApi(given),
            Self::HifiSecret => Setting::HifiSecret(given),
            Self::Monochrome => Setting::Monochrome(given),
        }
    }

    pub(crate) fn fields(
        online: &Online,
        window: &mut Window,
        cx: &mut Context<RootView>,
    ) -> [Entity<Field>; 6] {
        Self::ALL.map(|account| {
            let field = cx.new(|cx| {
                let field = Field::new(account.placeholder(), window, cx);
                let mut field = match account {
                    Self::ClientSecret | Self::RefreshToken | Self::HifiSecret => field.masked(),
                    Self::ClientId | Self::HifiApi | Self::Monochrome => field,
                };
                field.hold(account.held(online), cx);
                field
            });
            cx.subscribe_in(&field, window, move |this, _, _: &Submitted, window, cx| {
                this.tidal_given(account, window, cx);
            })
            .detach();
            field
        })
    }
}

impl RootView {
    fn tidal_field(&self, account: TidalAccount) -> &Entity<Field> {
        &self.tidal[account.nth()]
    }

    fn tidal_given(&mut self, account: TidalAccount, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.tidal_field(account).read(cx).text().trim().to_owned();
        let names_a_server = matches!(account, TidalAccount::HifiApi | TidalAccount::Monochrome);
        if names_a_server && !given.is_empty() && !reads_as_a_server(&given) {
            self.report(not_a_server(), cx);
            return;
        }
        self.tidal_field(account)
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            account.hold(&mut global.online, given.clone())
        });

        let said = match (account, given.is_empty()) {
            (TidalAccount::HifiApi, true) => {
                words::settings_tidal::the_hosted_hifi_api_service_is()
            }
            (TidalAccount::HifiApi, false) => {
                words::settings_tidal::the_custom_hifi_api_server_is()
            }
            (TidalAccount::HifiSecret, true) => {
                words::settings_tidal::the_resonate_server_is_no_longer()
            }
            (TidalAccount::HifiSecret, false) => {
                words::settings_tidal::the_resonate_server_is_asked_with()
            }
            (TidalAccount::Monochrome, true) => {
                words::settings_tidal::the_hosted_monochrome_service_is_asked()
            }
            (TidalAccount::Monochrome, false) => {
                words::settings_tidal::the_custom_monochrome_server_is_asked()
            }
            (_, true) => words::settings_tidal::tidal_is_no_longer_asked(),
            (_, false) => words::settings_tidal::tidal_is_asked_from_now_on(),
        };
        self.store(&account.setting(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        self.the_sources_moved(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn follow_the_tidal_sign_in_made_elsewhere(
        &mut self,
        signed_in: TidalSignIn,
        cx: &mut Context<Self>,
    ) {
        let TidalSignIn {
            client_id,
            client_secret,
            refresh_token,
        } = signed_in;
        for (account, given) in [
            (TidalAccount::ClientId, client_id),
            (TidalAccount::ClientSecret, client_secret),
            (TidalAccount::RefreshToken, refresh_token),
        ] {
            self.tidal_field(account)
                .clone()
                .update(cx, |field, cx| field.hold(given.clone(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| account.hold(&mut global.online, given));
        }
        self.the_sources_moved(cx);
        cx.notify();
    }

    pub(super) fn give_what_is_typed_of_the_tidal_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for account in TidalAccount::ALL {
            let field = self.tidal_field(account).read(cx);
            let typed_apart = field.is_focused(window)
                && field.text().trim() != account.held(&cx.global::<ResonateApp>().online);
            if typed_apart {
                self.tidal_given(account, window, cx);
                return true;
            }
        }
        false
    }

    pub(super) fn put_back_the_tidal_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for account in TidalAccount::ALL {
            let stored = account.held(&cx.global::<ResonateApp>().online);
            let field = self.tidal_field(account).clone();
            if field.read(cx).is_focused(window) {
                field.update(cx, |field, cx| field.hold(stored, cx));
            }
        }
    }

    pub(crate) fn forget_the_resonate_key(&mut self, cx: &mut Context<Self>) {
        let account = TidalAccount::HifiSecret;
        self.tidal_field(account)
            .clone()
            .update(cx, |field, cx| field.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            account.hold(&mut global.online, String::new());
        });
        self.store(&account.setting(String::new()), cx);
        self.the_sources_moved(cx);
    }

    pub(crate) fn forget_the_tidal_account(&mut self, cx: &mut Context<Self>) {
        for account in TidalAccount::ALL
            .into_iter()
            .filter(|account| account.is_tidals())
        {
            self.tidal_field(account)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                account.hold(&mut global.online, String::new());
            });
            self.store(&account.setting(String::new()), cx);
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.online.hosted_services = false);
        self.store(&Setting::HostedServices(false), cx);
    }

    fn client_for_tidal(&self, cx: &Context<Self>) -> Option<Client> {
        let online = &cx.global::<ResonateApp>().online;
        let id = online.tidal_client_id.trim();
        let secret = online.tidal_client_secret.trim();
        (!id.is_empty()).then(|| Client {
            id: id.to_owned(),
            secret: (!secret.is_empty()).then(|| secret.to_owned()),
        })
    }

    fn sign_in_to_tidal(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(signs_in) = app.signs_in.clone().filter(|_| app.online.enabled) else {
            self.report(
                Notice::Trouble(words::settings_tidal::offline().to_owned()),
                cx,
            );
            return;
        };
        let Some(client) = self.client_for_tidal(cx) else {
            self.report(
                Notice::Trouble(words::settings_tidal::no_client().to_owned()),
                cx,
            );
            return;
        };

        self.signing_in.stopped.store(true, Ordering::Relaxed);
        let stopped = Arc::new(AtomicBool::new(false));
        self.signing_in.stopped = Arc::clone(&stopped);
        self.signing_in.shown = TidalSigning::Asking;
        cx.notify();

        self.signing_in.task = cx.spawn(async move |this, cx| {
            let asking = Arc::clone(&signs_in);
            let asked_for = client.clone();
            let authorizing = cx
                .background_executor()
                .spawn(async move { asking.authorizing(&asked_for) })
                .await;
            let authorizing = match authorizing {
                Ok(authorizing) => authorizing,
                Err(error) => {
                    let told = this.update(cx, |this, cx| this.tidal_signed_in(Err(error), cx));
                    let _ = told;
                    return;
                }
            };

            let shown = authorizing.clone();
            let waiting = this.update(cx, |this, cx| {
                this.signing_in.shown = TidalSigning::Waiting(shown);
                cx.notify();
            });
            if waiting.is_err() {
                return;
            }
            let answered = cx
                .background_executor()
                .spawn(async move {
                    let stopping = move || stopped.load(Ordering::Relaxed);
                    signs_in.authorized(&client, &authorizing, &stopping)
                })
                .await;
            let told = this.update(cx, |this, cx| this.tidal_signed_in(answered, cx));
            let _ = told;
        });
    }

    fn tidal_signed_in(
        &mut self,
        answered: resonate_providers::Result<Option<RefreshToken>>,
        cx: &mut Context<Self>,
    ) {
        self.signing_in.shown = TidalSigning::Idle;
        match answered {
            Ok(Some(token)) => {
                let token = token.into_string();
                let account = TidalAccount::RefreshToken;
                self.tidal_field(account)
                    .clone()
                    .update(cx, |field, cx| field.hold(token.clone(), cx));
                cx.update_global::<ResonateApp, _>(|global, _| {
                    account.hold(&mut global.online, token.clone());
                });
                self.store(&account.setting(token), cx);
                self.report(
                    Notice::Done(words::settings_tidal::signed_in().to_owned()),
                    cx,
                );
                self.the_sources_moved(cx);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "the TIDAL sign-in did not finish");
                self.report(Notice::Trouble(turned_away(&error).to_owned()), cx);
            }
        }
        cx.notify();
    }

    fn stop_signing_in_to_tidal(&mut self, cx: &mut Context<Self>) {
        self.signing_in.stopped.store(true, Ordering::Relaxed);
        self.signing_in.shown = TidalSigning::Idle;
        self.signing_in.task = Task::ready(());
        cx.notify();
    }

    fn signing_in_to_tidal(&self, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        let reachable = app.signs_in.is_some() && app.online.enabled;
        let held_back = !reachable || self.client_for_tidal(cx).is_none();

        match self.signing_in.shown.clone() {
            TidalSigning::Idle => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(kit::action_row().child(self.sign_in_button(
                    words::settings_tidal::sign_in_to_tidal(),
                    held_back,
                    cx,
                )))
                .child(note(words::settings_tidal::sign_in_note())),
            TidalSigning::Asking => kit::action_row().child(self.sign_in_button(
                words::settings_tidal::asking_tidal(),
                true,
                cx,
            )),
            TidalSigning::Waiting(authorizing) => {
                let opened = authorizing.verify_at.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(kit::field(
                        words::settings_tidal::code(),
                        kit::figure(authorizing.user_code.clone()),
                    ))
                    .child(
                        kit::action_row()
                            .child(self.open_the_tidal_page(opened, cx))
                            .child(self.stop_signing_in(cx)),
                    )
                    .child(note(words::settings_tidal::approve_the_code(
                        authorizing.user_code.to_string(),
                        authorizing.lasts.as_secs().div_ceil(60),
                        authorizing.verify_at.to_string(),
                    )))
            }
        }
    }

    fn sign_in_button(
        &self,
        label: &'static str,
        held_back: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "tidal-sign-in",
            label,
            Icon::Link,
            held_back,
            |this, _, cx| this.sign_in_to_tidal(cx),
            self,
            cx,
        )
    }

    fn open_the_tidal_page(&self, page: String, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "tidal-open-page",
            words::settings_tidal::open_the_page(),
            Icon::Globe,
            false,
            move |_, _, cx| cx.open_url(&page),
            self,
            cx,
        )
    }

    fn stop_signing_in(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "tidal-stop-sign-in",
            words::settings_tidal::stop(),
            Icon::Stop,
            false,
            |this, _, cx| this.stop_signing_in_to_tidal(cx),
            self,
            cx,
        )
    }

    pub(super) fn tidal_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let mut body = kit::rows();
        for account in TidalAccount::ALL
            .into_iter()
            .filter(|account| account.is_tidals())
        {
            body = body.child(kit::field(
                account.label(),
                self.text_field(
                    account.id(),
                    self.tidal_field(account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }
        let allowed = cx.global::<ResonateApp>().online.hosted_services;
        body.child(self.in_the_ring(
            "hosted-services",
            switch_row(
                words::settings_tidal::ask_the_hosted_services(),
                words::settings_tidal::off_hifi_api_and_monochrome_are(),
                allowed,
                "hosted-services",
            ),
            move |this, _, cx| this.set_hosted_services(!allowed, cx),
            cx,
        ))
        .child(self.signing_in_to_tidal(cx))
        .note(words::settings_tidal::tidal_note())
    }

    pub(super) fn resonate_key_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let account = TidalAccount::HifiSecret;
        kit::rows()
            .child(kit::field(
                account.label(),
                self.text_field(
                    account.id(),
                    self.tidal_field(account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ))
            .note(SharedString::from(
                words::settings_tidal::resonate_key_note(RESONATE_HOST),
            ))
    }

    fn set_hosted_services(&mut self, allowed: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.hosted_services = allowed);
        self.store(&Setting::HostedServices(allowed), cx);
        self.the_sources_moved(cx);
        cx.notify();
    }
}
