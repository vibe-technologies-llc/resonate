use std::{path::Path, sync::Arc};

use resonate_inbox::Inbox;
use resonate_providers::{Provider as _, Providers};
#[cfg(feature = "online")]
use resonate_subsonic::{Server, Subsonic};
#[cfg(feature = "online")]
use resonate_tidal::{Account, HifiApi, Tidal};

use crate::config::Config;

pub fn registered(config: &Config) -> Providers {
    registry(config.inbox.as_deref(), &Accounts::of(config), &Made::new())
}

#[cfg(feature = "ui")]
pub fn sourced() -> resonate_ui::Registering {
    let made = Made::new();
    Arc::new(move |supplying: &resonate_ui::Supplying<'_>| {
        let every = registry(supplying.inbox, &Accounts::given(supplying.online), &made);
        match (supplying.asking, supplying.inbox) {
            (resonate_ui::Asking::TheInboxAlone, Some(folder)) => {
                every.only(Inbox::at(folder).source())
            }
            (resonate_ui::Asking::TheInboxAlone, None) => Providers::none(),
            (resonate_ui::Asking::EveryProvider, _) => every,
        }
    })
}

#[cfg(feature = "online")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Accounts {
    subsonic: Option<Server>,
    tidal: Option<Account>,
    hifi: Option<HifiServer>,
}

#[cfg(not(feature = "online"))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Accounts;

#[cfg(feature = "online")]
impl Accounts {
    fn of(config: &Config) -> Self {
        if !config.online_enabled() {
            return Self::default();
        }
        Self {
            subsonic: subsonic(
                config.subsonic.as_deref(),
                config.subsonic_user.as_deref(),
                config.subsonic_password.as_deref(),
            ),
            tidal: tidal(
                config.tidal_client_id.as_deref(),
                config.tidal_client_secret.as_deref(),
                config.tidal_refresh_token.as_deref(),
            ),
            hifi: Some(hifi_api(config.hifi_api.as_deref())),
        }
    }

    #[cfg(feature = "ui")]
    fn given(online: &resonate_ui::Online) -> Self {
        if !online.enabled {
            return Self::default();
        }
        Self {
            subsonic: subsonic(
                Some(&online.subsonic),
                Some(&online.subsonic_user),
                Some(&online.subsonic_password),
            ),
            tidal: tidal(
                Some(&online.tidal_client_id),
                Some(&online.tidal_client_secret),
                Some(&online.tidal_refresh_token),
            ),
            hifi: Some(hifi_api(Some(&online.hifi_api))),
        }
    }
}

#[cfg(not(feature = "online"))]
impl Accounts {
    const fn of(_config: &Config) -> Self {
        Self
    }

    #[cfg(feature = "ui")]
    const fn given(_online: &resonate_ui::Online) -> Self {
        Self
    }
}

#[cfg(feature = "online")]
fn given(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(feature = "online")]
fn subsonic(url: Option<&str>, user: Option<&str>, password: Option<&str>) -> Option<Server> {
    Some(Server {
        url: given(url)?,
        user: given(user)?,
        password: given(password)?,
    })
}

#[cfg(feature = "online")]
fn tidal(
    client_id: Option<&str>,
    client_secret: Option<&str>,
    refresh_token: Option<&str>,
) -> Option<Account> {
    Some(Account {
        client_id: given(client_id)?,
        client_secret: given(client_secret),
        refresh_token: given(refresh_token)?,
    })
}

#[cfg(feature = "online")]
#[derive(Clone, Debug, PartialEq, Eq)]
enum HifiServer {
    Hosted,
    Custom(String),
}

#[cfg(feature = "online")]
fn hifi_api(server: Option<&str>) -> HifiServer {
    given(server).map_or(HifiServer::Hosted, HifiServer::Custom)
}

#[cfg(feature = "online")]
#[derive(Default)]
struct Made {
    subsonic: Kept<Server, Subsonic>,
    tidal: Kept<Account, Tidal>,
    hifi: Kept<HifiServer, HifiApi>,
}

#[cfg(not(feature = "online"))]
struct Made;

#[cfg(not(feature = "online"))]
impl Made {
    const fn new() -> Self {
        Self
    }
}

#[cfg(feature = "online")]
impl Made {
    fn new() -> Self {
        Self::default()
    }
}

#[cfg(feature = "online")]
struct Kept<Settings, Made> {
    held: parking_lot::Mutex<Option<(Settings, Arc<Made>)>>,
}

#[cfg(feature = "online")]
impl<Settings, Made> Default for Kept<Settings, Made> {
    fn default() -> Self {
        Self {
            held: parking_lot::Mutex::new(None),
        }
    }
}

#[cfg(feature = "online")]
impl<Settings: Clone + PartialEq, Made> Kept<Settings, Made> {
    fn made_for(&self, settings: &Settings, make: impl FnOnce(&Settings) -> Made) -> Arc<Made> {
        let mut held = self.held.lock();
        if let Some((standing, made)) = held.as_ref()
            && standing == settings
        {
            return Arc::clone(made);
        }
        let made = Arc::new(make(settings));
        *held = Some((settings.clone(), Arc::clone(&made)));
        made
    }
}

#[cfg(feature = "online")]
fn registry(inbox: Option<&Path>, accounts: &Accounts, made: &Made) -> Providers {
    let mut providers = with_inbox(inbox);
    if let Some(server) = &accounts.subsonic {
        providers = providers.and(
            made.subsonic
                .made_for(server, |server| Subsonic::at(server.clone())),
        );
    }
    if let Some(account) = &accounts.tidal {
        providers = providers.and(
            made.tidal
                .made_for(account, |account| Tidal::signed_in(account.clone())),
        );
    }
    if let Some(hifi) = &accounts.hifi {
        providers = providers.and(made.hifi.made_for(hifi, |hifi| match hifi {
            HifiServer::Hosted => HifiApi::hosted(),
            HifiServer::Custom(server) => HifiApi::at(server),
        }));
    }
    providers
}

#[cfg(not(feature = "online"))]
fn registry(inbox: Option<&Path>, _accounts: &Accounts, _made: &Made) -> Providers {
    with_inbox(inbox)
}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn signs_in() -> Option<Arc<dyn resonate_providers::SignsIn>> {
    Some(Arc::new(resonate_tidal::TidalSignIn::default()))
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn signs_in() -> Option<Arc<dyn resonate_providers::SignsIn>> {
    None
}

fn with_inbox(inbox: Option<&Path>) -> Providers {
    let mut providers = Providers::none();
    if let Some(folder) = inbox {
        providers = providers.and(Arc::new(Inbox::at(folder)));
    }
    providers
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn named(config: &Config) -> Vec<String> {
        registered(config)
            .names()
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect()
    }

    #[test]
    fn an_inbox_is_registered_even_when_online_services_are_off() {
        let offline = Config {
            online: Some(false),
            ..Config::default()
        };
        assert!(!registered(&offline).has_a_source());

        let config = Config {
            inbox: Some(PathBuf::from("/music/inbox")),
            ..offline
        };
        let providers = registered(&config);
        assert!(providers.has_a_source());
        assert!(named(&config).contains(&"inbox".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_subsonic_server_is_registered_only_with_its_account_and_while_online() {
        let halfway = Config {
            subsonic: Some("http://music.local:4533".to_owned()),
            subsonic_user: Some("listener".to_owned()),
            ..Config::default()
        };
        assert!(!named(&halfway).contains(&"subsonic".to_owned()));

        let blank = Config {
            subsonic_password: Some("  ".to_owned()),
            ..halfway.clone()
        };
        assert!(!named(&blank).contains(&"subsonic".to_owned()));

        let whole = Config {
            subsonic_password: Some("sesame".to_owned()),
            ..halfway
        };
        assert!(named(&whole).contains(&"subsonic".to_owned()));

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"subsonic".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_tidal_account_is_registered_only_with_its_client_and_token_and_while_online() {
        let halfway = Config {
            tidal_client_id: Some("client".to_owned()),
            ..Config::default()
        };
        assert!(!named(&halfway).contains(&"tidal".to_owned()));

        let whole = Config {
            tidal_refresh_token: Some("token".to_owned()),
            ..halfway
        };
        assert!(named(&whole).contains(&"tidal".to_owned()));

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"tidal".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn the_hosted_hifi_api_is_registered_without_an_override_and_while_online() {
        assert!(named(&Config::default()).contains(&"hifi-api".to_owned()));
        assert_eq!(hifi_api(None), HifiServer::Hosted);

        let whole = Config {
            hifi_api: Some("http://hifi.home.arpa:8000".to_owned()),
            ..Config::default()
        };
        assert!(named(&whole).contains(&"hifi-api".to_owned()));
        assert!(matches!(
            Accounts::of(&whole).hifi,
            Some(HifiServer::Custom(_))
        ));
        assert_eq!(hifi_api(Some("  ")), HifiServer::Hosted);

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"hifi-api".to_owned()));
    }

    #[cfg(all(feature = "online", feature = "ui"))]
    #[test]
    fn the_window_registers_what_its_settings_say_now_and_keeps_a_provider_its_settings_left_alone()
    {
        let register = sourced();
        let named = |online: &resonate_ui::Online| -> Vec<String> {
            register(&resonate_ui::Supplying {
                inbox: None,
                online,
                asking: resonate_ui::Asking::EveryProvider,
            })
            .names()
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect()
        };
        let offline = resonate_ui::Online::default();
        assert!(!named(&offline).contains(&"tidal".to_owned()));

        let signed_in = resonate_ui::Online {
            enabled: true,
            tidal_client_id: "client".to_owned(),
            tidal_refresh_token: "token".to_owned(),
            ..resonate_ui::Online::default()
        };
        assert!(named(&signed_in).contains(&"tidal".to_owned()));
        assert!(named(&signed_in).contains(&"hifi-api".to_owned()));

        let alone = register(&resonate_ui::Supplying {
            inbox: Some(Path::new("/music/inbox")),
            online: &signed_in,
            asking: resonate_ui::Asking::TheInboxAlone,
        });
        assert_eq!(
            alone
                .names()
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect::<Vec<_>>(),
            ["unprovided", "inbox"]
        );

        let made = Made::new();
        let accounts = Accounts::given(&signed_in);
        let tidal = |accounts: &Accounts| {
            made.tidal
                .made_for(accounts.tidal.as_ref().expect("an account"), |account| {
                    Tidal::signed_in(account.clone())
                })
        };
        let first = tidal(&accounts);
        assert!(Arc::ptr_eq(&first, &tidal(&accounts)));

        let renewed = Accounts::given(&resonate_ui::Online {
            tidal_refresh_token: "another".to_owned(),
            ..signed_in
        });
        assert!(!Arc::ptr_eq(&first, &tidal(&renewed)));
    }
}
