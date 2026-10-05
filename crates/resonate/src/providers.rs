use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use resonate_inbox::Inbox;
#[cfg(feature = "online")]
use resonate_monochrome::Monochrome;
use resonate_providers::Providers;
#[cfg(feature = "online")]
use resonate_subsonic::{Server, Subsonic};
#[cfg(feature = "online")]
use resonate_tidal::{Account, HifiApi, Tidal};

#[cfg(feature = "online")]
use crate::{config, error::ConfigKey, signals};
use crate::{
    config::Config,
    error::{Error, Result},
};

pub fn registered(config: &Config, settings: Option<PathBuf>) -> Providers {
    registry(
        config.inbox.as_deref(),
        &Accounts::of(config),
        &Made::new(settings),
    )
}

#[cfg(feature = "ui")]
pub fn sourced(settings: Option<PathBuf>) -> resonate_ui::Registering {
    let made = Made::new(settings);
    Arc::new(move |supplying: &resonate_ui::Supplying<'_>| {
        let every = registry(supplying.inbox, &Accounts::given(supplying.online), &made);
        match (supplying.asking, supplying.inbox) {
            (resonate_ui::Asking::TheInboxAlone, Some(folder)) => {
                every.only(resonate_providers::Provider::source(&Inbox::at(folder)))
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
    hifi: Option<Hosting>,
    monochrome: Option<Hosting>,
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
            hifi: Some(hosting(config.hifi_api.as_deref())),
            monochrome: Some(hosting(config.monochrome.as_deref())),
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
            hifi: Some(hosting(Some(&online.hifi_api))),
            monochrome: Some(hosting(Some(&online.monochrome))),
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
enum Hosting {
    Hosted,
    Custom(String),
}

#[cfg(feature = "online")]
fn hosting(server: Option<&str>) -> Hosting {
    given(server).map_or(Hosting::Hosted, Hosting::Custom)
}

#[cfg(feature = "online")]
struct Made {
    subsonic: Kept<Server, Subsonic>,
    tidal: Kept<Account, Tidal>,
    hifi: Kept<Hosting, HifiApi>,
    monochrome: Kept<Hosting, Monochrome>,
    renewed: Arc<Renewed>,
}

#[cfg(not(feature = "online"))]
struct Made;

#[cfg(not(feature = "online"))]
impl Made {
    fn new(_settings: Option<PathBuf>) -> Self {
        Self
    }
}

#[cfg(feature = "online")]
impl Made {
    fn new(settings: Option<PathBuf>) -> Self {
        Self {
            subsonic: Kept::default(),
            tidal: Kept::default(),
            hifi: Kept::default(),
            monochrome: Kept::default(),
            renewed: Arc::new(Renewed {
                settings,
                rotated: parking_lot::Mutex::new(None),
            }),
        }
    }

    fn signed_in(&self, account: &Account) -> Tidal {
        let renewed = Arc::clone(&self.renewed);
        let given = account.refresh_token.clone();
        Tidal::signed_in(self.renewed.standing_for(account))
            .telling(move |token| renewed.note(given.clone(), token))
    }
}

#[cfg(feature = "online")]
struct Renewed {
    settings: Option<PathBuf>,
    rotated: parking_lot::Mutex<Option<Rotation>>,
}

#[cfg(feature = "online")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rotation {
    given: String,
    standing: String,
}

#[cfg(feature = "online")]
impl Renewed {
    fn standing_for(&self, account: &Account) -> Account {
        let rotated = self.rotated.lock();
        match rotated.as_ref() {
            Some(rotation) if rotation.given == account.refresh_token => Account {
                refresh_token: rotation.standing.clone(),
                ..account.clone()
            },
            Some(_) | None => account.clone(),
        }
    }

    fn note(&self, given: String, token: resonate_providers::RefreshToken) {
        let standing = token.into_string();
        *self.rotated.lock() = Some(Rotation {
            given,
            standing: standing.clone(),
        });
        let Some(settings) = &self.settings else {
            return;
        };
        if let Err(error) = crate::config::store(
            settings,
            crate::error::ConfigKey::TidalRefreshToken,
            standing,
        ) {
            tracing::warn!(%error, "the refresh token TIDAL rotated could not be kept");
        }
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
                .made_for(account, |account| made.signed_in(account)),
        );
    }
    if let Some(hifi) = &accounts.hifi {
        providers = providers.and(made.hifi.made_for(hifi, |hifi| match hifi {
            Hosting::Hosted => HifiApi::hosted(),
            Hosting::Custom(server) => HifiApi::at(server),
        }));
    }
    if let Some(monochrome) = &accounts.monochrome {
        providers = providers.and(made.monochrome.made_for(
            monochrome,
            |monochrome| match monochrome {
                Hosting::Hosted => Monochrome::hosted(),
                Hosting::Custom(server) => Monochrome::at(server),
            },
        ));
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

#[cfg(feature = "online")]
pub fn sign_in_to_tidal(config: &Config, settings: &Path) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};

    use resonate_providers::{Client, SignsIn as _};

    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    let kept = |held: &Option<String>| {
        held.as_deref()
            .map(str::trim)
            .filter(|held| !held.is_empty())
            .map(str::to_owned)
    };
    let client = Client {
        id: kept(&config.tidal_client_id).ok_or(Error::NoTidalClient)?,
        secret: kept(&config.tidal_client_secret),
    };

    let signs_in = resonate_tidal::TidalSignIn::default();
    let authorizing = signs_in.authorizing(&client)?;
    said!(
        "open {} and enter {} to sign in to TIDAL",
        authorizing.verify_at,
        authorizing.user_code
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&cancelled);
    let _interrupting = signals::cancel_when_told(move || stopping.store(true, Ordering::Relaxed));
    let token =
        signs_in.authorized(&client, &authorizing, &|| cancelled.load(Ordering::Relaxed))?;

    let Some(token) = token else {
        return Err(Error::SignInCancelled);
    };
    config::store(settings, ConfigKey::TidalRefreshToken, token.into_string())?;
    said!(
        "signed in; the refresh token is kept in {}",
        settings.display()
    );
    Ok(())
}

#[cfg(not(feature = "online"))]
pub fn sign_in_to_tidal(_: &Config, _: &Path) -> Result<()> {
    Err(Error::NoSignIn)
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
        registered(config, None)
            .names()
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect()
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_tidal_sign_in_with_online_off_or_no_client_is_refused_before_anything_is_asked() {
        let nowhere = PathBuf::from("/nowhere/resonate/config.toml");

        let offline = Config {
            online: Some(false),
            tidal_client_id: Some("client".to_owned()),
            ..Config::default()
        };
        assert!(matches!(
            sign_in_to_tidal(&offline, &nowhere),
            Err(Error::OnlineOff)
        ));

        for blank in [None, Some("  ".to_owned())] {
            let unregistered = Config {
                tidal_client_id: blank,
                ..Config::default()
            };
            assert!(matches!(
                sign_in_to_tidal(&unregistered, &nowhere),
                Err(Error::NoTidalClient)
            ));
        }
    }

    #[test]
    fn an_inbox_is_registered_even_when_online_services_are_off() {
        let offline = Config {
            online: Some(false),
            ..Config::default()
        };
        assert!(!registered(&offline, None).has_a_source());

        let config = Config {
            inbox: Some(PathBuf::from("/music/inbox")),
            ..offline
        };
        let providers = registered(&config, None);
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
        assert_eq!(hosting(None), Hosting::Hosted);

        let whole = Config {
            hifi_api: Some("http://hifi.home.arpa:8000".to_owned()),
            ..Config::default()
        };
        assert!(named(&whole).contains(&"hifi-api".to_owned()));
        assert!(matches!(
            Accounts::of(&whole).hifi,
            Some(Hosting::Custom(_))
        ));
        assert_eq!(hosting(Some("  ")), Hosting::Hosted);

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"hifi-api".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn the_hosted_monochrome_is_registered_after_hifi_api_without_an_override_and_while_online() {
        let hosted = named(&Config::default());
        assert!(hosted.contains(&"monochrome".to_owned()));
        assert_eq!(
            hosted.iter().position(|name| name == "monochrome"),
            hosted
                .iter()
                .position(|name| name == "hifi-api")
                .map(|hifi| hifi + 1)
        );
        assert_eq!(
            Accounts::of(&Config::default()).monochrome,
            Some(Hosting::Hosted)
        );

        let whole = Config {
            monochrome: Some("https://tracks.home.arpa".to_owned()),
            ..Config::default()
        };
        assert!(named(&whole).contains(&"monochrome".to_owned()));
        assert_eq!(
            Accounts::of(&whole).monochrome,
            Some(Hosting::Custom("https://tracks.home.arpa".to_owned()))
        );

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"monochrome".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_refresh_token_tidal_rotated_is_kept_and_signed_in_with_from_then_on() {
        let folder = std::env::temp_dir().join(format!("resonate-renewed-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a writable temporary directory");
        let settings = folder.join("config.toml");
        std::fs::write(&settings, "tidal-refresh-token = \"first\"\n")
            .expect("a writable temporary directory");
        let made = Made::new(Some(settings.clone()));
        let given = Account {
            client_id: "client".to_owned(),
            client_secret: None,
            refresh_token: "first".to_owned(),
        };

        let before = made.renewed.standing_for(&given);
        made.renewed.note(
            "first".to_owned(),
            resonate_providers::RefreshToken::new("second".to_owned()),
        );
        let after = made.renewed.standing_for(&given);
        let elsewhere = made.renewed.standing_for(&Account {
            refresh_token: "another".to_owned(),
            ..given.clone()
        });
        let kept = std::fs::read_to_string(&settings).expect("the settings read back");
        std::fs::remove_dir_all(&folder).expect("the temporary directory removed");

        assert_eq!(before.refresh_token, "first");
        assert_eq!(after.refresh_token, "second");
        assert_eq!(elsewhere.refresh_token, "another");
        assert!(kept.contains("tidal-refresh-token = \"second\""), "{kept}");
    }

    #[cfg(all(feature = "online", feature = "ui"))]
    #[test]
    fn the_window_registers_what_its_settings_say_now_and_keeps_a_provider_its_settings_left_alone()
    {
        let register = sourced(None);
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
        assert!(named(&signed_in).contains(&"monochrome".to_owned()));

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

        let made = Made::new(None);
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
