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
            action, note,
            subsonic::{not_a_server, reads_as_a_server},
            switch_row,
        },
    },
};

const SIGN_IN_NOTE: &str = "Sign in to TIDAL asks TIDAL for a code to approve in a browser \
                            with the client id above, then keeps the refresh token it hands \
                            back — nothing is typed into this window but the client.";

const NO_CLIENT: &str = "Give the client id above first";

const OFFLINE: &str = "TIDAL is signed in to only while Online is on";

const SIGNED_IN: &str = "Signed in to TIDAL — it is asked from now on";

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
        Error::AuthorizationLapsed { .. } => "The TIDAL code lapsed before it was approved",
        Error::AuthorizationDenied { .. } => "TIDAL was told not to sign in",
        Error::Unwelcome { .. } => "TIDAL refused the client id",
        Error::Io { .. } | Error::Unaddressable { .. } | Error::Busy { .. } => {
            "TIDAL could not be reached"
        }
        Error::Refused { .. } => "TIDAL refused the sign-in",
        Error::Untrusted { .. } => "TIDAL's certificate is not one this build trusts",
        Error::StillArriving { .. }
        | Error::Unreadable { .. }
        | Error::TurnedAway { .. }
        | Error::OffItsHosts { .. }
        | Error::StillQueued { .. }
        | Error::NotTheService { .. }
        | Error::NotAnExtension => "TIDAL answered something this build cannot read",
    }
}

const TIDAL_NOTE: &str = "Where a TIDAL client id and refresh token are set, your own \
                          subscription is asked for every track marked wanted, by the TIDAL \
                          track MusicBrainz links the recording to or by its ISRC and never by a \
                          title. Only the whole track in lossless FLAC is taken — never a \
                          preview, a lossy stream or an encrypted one — and it is downloaded, \
                          repacked as a FLAC file and kept in the vault, or the music folder \
                          where no vault is open. The client id and secret are those of the \
                          application the refresh token was issued to. The hosted hifi-api \
                          service is used where no custom server is given; a hifi-api server \
                          you run can replace it. Both use the same ISRC and whole-track checks. \
                          Monochrome's hosted service is asked too, by ISRC alone, for the whole \
                          FLAC; a Monochrome server you name can replace it. The hosted services \
                          are asked unless Ask the hosted services is turned off; a server you name \
                          is asked either way. Asked as soon as they are given, and only while \
                          Online is on.";

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

    const fn placeholder(self) -> &'static str {
        match self {
            Self::ClientId => "The client id the token was issued to, then press enter",
            Self::ClientSecret => "Its client secret, if it has one, then press enter",
            Self::RefreshToken => "A refresh token for your account, then press enter",
            Self::HifiApi => "Custom hifi-api server address; blank uses the hosted service",
            Self::HifiSecret => "A key your Resonate server lists, then press enter",
            Self::Monochrome => "Custom Monochrome server address; blank uses tracks.monochrome.st",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ClientId => "Client id",
            Self::ClientSecret => "Client secret",
            Self::RefreshToken => "Refresh token",
            Self::HifiApi => "hifi-api server",
            Self::HifiSecret => "Key",
            Self::Monochrome => "Monochrome server",
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
            (TidalAccount::HifiApi, true) => "The hosted hifi-api service is asked from now on",
            (TidalAccount::HifiApi, false) => "The custom hifi-api server is asked from now on",
            (TidalAccount::HifiSecret, true) => "The Resonate server is no longer asked",
            (TidalAccount::HifiSecret, false) => "The Resonate server is asked with the key",
            (TidalAccount::Monochrome, true) => {
                "The hosted Monochrome service is asked from now on"
            }
            (TidalAccount::Monochrome, false) => {
                "The custom Monochrome server is asked from now on"
            }
            (_, true) => "TIDAL is no longer asked",
            (_, false) => "TIDAL is asked from now on",
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
            self.report(Notice::Trouble(OFFLINE.to_owned()), cx);
            return;
        };
        let Some(client) = self.client_for_tidal(cx) else {
            self.report(Notice::Trouble(NO_CLIENT.to_owned()), cx);
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
                self.report(Notice::Done(SIGNED_IN.to_owned()), cx);
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
                .child(self.sign_in_button("Sign in to TIDAL", held_back, cx))
                .child(note(SIGN_IN_NOTE)),
            TidalSigning::Asking => div().child(self.sign_in_button("Asking TIDAL…", true, cx)),
            TidalSigning::Waiting(authorizing) => {
                let opened = authorizing.verify_at.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(kit::field(
                        "Code",
                        kit::figure(authorizing.user_code.clone()),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(self.open_the_tidal_page(opened, cx))
                            .child(self.stop_signing_in(cx)),
                    )
                    .child(note(format!(
                        "Open {} and approve the code {}; this waits until TIDAL answers, for \
                         {} minutes at most.",
                        authorizing.verify_at,
                        authorizing.user_code,
                        authorizing.lasts.as_secs().div_ceil(60),
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
            "Open the page",
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
            "Stop",
            Icon::Stop,
            false,
            |this, _, cx| this.stop_signing_in_to_tidal(cx),
            self,
            cx,
        )
    }

    pub(super) fn tidal_group(&mut self, cx: &mut Context<Self>) -> Div {
        let mut body = kit::section_body();
        for account in TidalAccount::ALL
            .into_iter()
            .filter(|account| account.is_tidals())
        {
            body = body.child(kit::field(
                account.label(),
                self.key_field(
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
                "Ask the hosted services",
                "Off, hifi-api and Monochrome are asked only at a server named above",
                allowed,
                "hosted-services",
            ),
            move |this, _, cx| this.set_hosted_services(!allowed, cx),
            cx,
        ))
        .child(self.signing_in_to_tidal(cx))
        .child(note(TIDAL_NOTE))
    }

    pub(super) fn resonate_key_group(&mut self, cx: &mut Context<Self>) -> Div {
        let account = TidalAccount::HifiSecret;
        kit::section_body()
            .child(kit::field(
                account.label(),
                self.key_field(
                    account.id(),
                    self.tidal_field(account),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ))
            .child(note(SharedString::from(format!(
                "Wanted songs are asked of {RESONATE_HOST} with this key, as a bearer token over \
                 https, and nowhere else. Asked as soon as it is given, and only while Online \
                 is on."
            ))))
    }

    fn set_hosted_services(&mut self, allowed: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.hosted_services = allowed);
        self.store(&Setting::HostedServices(allowed), cx);
        self.the_sources_moved(cx);
        cx.notify();
    }
}
