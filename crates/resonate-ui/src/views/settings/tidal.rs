use gpui::{
    AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Window,
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

const TIDAL_NOTE: &str = "A TIDAL subscription of your own is asked for every track marked \
                          wanted, by the TIDAL track MusicBrainz links the recording to or by \
                          its ISRC and never by a title. Only the whole track in lossless FLAC \
                          is taken — never a preview, a lossy stream or an encrypted one — and \
                          it is downloaded, repacked as a FLAC file and kept in the vault. The \
                          client id and secret are those of the application the refresh token \
                          was issued to. Used from the next start, and only while Online is on.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TidalAccount {
    ClientId,
    ClientSecret,
    RefreshToken,
}

impl TidalAccount {
    pub(crate) const ALL: [Self; 3] = [Self::ClientId, Self::ClientSecret, Self::RefreshToken];

    const fn placeholder(self) -> &'static str {
        match self {
            Self::ClientId => "The client id the token was issued to, then press enter",
            Self::ClientSecret => "Its client secret, if it has one, then press enter",
            Self::RefreshToken => "A refresh token for your account, then press enter",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ClientId => "Client id",
            Self::ClientSecret => "Client secret",
            Self::RefreshToken => "Refresh token",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::ClientId => "tidal-client-id",
            Self::ClientSecret => "tidal-client-secret",
            Self::RefreshToken => "tidal-refresh-token",
        }
    }

    const fn nth(self) -> usize {
        match self {
            Self::ClientId => 0,
            Self::ClientSecret => 1,
            Self::RefreshToken => 2,
        }
    }

    fn held(self, online: &Online) -> String {
        match self {
            Self::ClientId => online.tidal_client_id.clone(),
            Self::ClientSecret => online.tidal_client_secret.clone(),
            Self::RefreshToken => online.tidal_refresh_token.clone(),
        }
    }

    fn hold(self, online: &mut Online, given: String) {
        match self {
            Self::ClientId => online.tidal_client_id = given,
            Self::ClientSecret => online.tidal_client_secret = given,
            Self::RefreshToken => online.tidal_refresh_token = given,
        }
    }

    fn setting(self, given: String) -> Setting {
        match self {
            Self::ClientId => Setting::TidalClientId(given),
            Self::ClientSecret => Setting::TidalClientSecret(given),
            Self::RefreshToken => Setting::TidalRefreshToken(given),
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
                    Self::ClientSecret | Self::RefreshToken => field.masked(),
                    Self::ClientId => field,
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
        self.tidal_field(account)
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            account.hold(&mut global.online, given.clone())
        });

        let said = if given.is_empty() {
            "TIDAL is not asked from the next start"
        } else {
            "TIDAL is asked from the next start"
        };
        self.store(&account.setting(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn forget_the_tidal_account(&mut self, cx: &mut Context<Self>) {
        for account in TidalAccount::ALL {
            self.tidal_field(account)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                account.hold(&mut global.online, String::new());
            });
            self.store(&account.setting(String::new()), cx);
        }
    }

    pub(super) fn tidal_group(&mut self, cx: &mut Context<Self>) -> Div {
        let mut body = kit::section_body();
        for account in TidalAccount::ALL {
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
        body.child(note(TIDAL_NOTE))
    }
}
