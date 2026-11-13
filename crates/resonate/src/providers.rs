#[cfg(feature = "online")]
use std::sync::OnceLock;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use resonate_inbox::Inbox;
#[cfg(feature = "online")]
use resonate_jellyfin::Jellyfin;
#[cfg(feature = "online")]
use resonate_library::{
    HostedServices, Hosting, JellyfinLogin, Key, Reachable, ServiceAccount, SubsonicLogin,
    TidalLogin, YoutubeMusicLogin, reachable,
};
#[cfg(feature = "online")]
use resonate_monochrome::Monochrome;
#[cfg(feature = "online")]
use resonate_providers::PlaysInPlace;
use resonate_providers::Providers;
#[cfg(feature = "online")]
use resonate_subsonic::{Server, Subsonic};
#[cfg(feature = "online")]
use resonate_tidal::{Account, HifiApi, Tidal};
#[cfg(feature = "online")]
use resonate_ytmusic::{Cookies, Tool, YtMusic};

#[cfg(feature = "online")]
use crate::{config, error::ConfigKey, signals};
use crate::{
    config::Config,
    error::{Error, Result},
};

#[cfg(any(feature = "mcp", test))]
pub fn registered(config: &Config, settings: Option<PathBuf>) -> Providers {
    registered_keeping(config, settings).0
}

pub fn registered_keeping(config: &Config, settings: Option<PathBuf>) -> (Providers, Keeping) {
    let made = Made::new(settings);
    let providers = registry(config.inbox.as_deref(), &Accounts::of(config), &made);
    (providers, made.keeping())
}

#[cfg(feature = "online")]
pub struct Keeping(Arc<Renewed>);

#[cfg(feature = "online")]
impl Keeping {
    pub fn unkept(&self) -> Option<Error> {
        self.0.unkept.lock().take()
    }
}

#[cfg(not(feature = "online"))]
pub struct Keeping;

#[cfg(not(feature = "online"))]
impl Keeping {
    pub const fn unkept(&self) -> Option<Error> {
        None
    }
}

#[cfg(all(feature = "ui", feature = "online"))]
pub fn sourced(settings: Option<PathBuf>) -> (resonate_ui::Registering, resonate_ui::Unkept) {
    sourced_over(made_once(settings))
}

#[cfg(all(feature = "ui", not(feature = "online")))]
pub fn sourced(settings: Option<PathBuf>) -> (resonate_ui::Registering, resonate_ui::Unkept) {
    sourced_over(Arc::new(Made::new(settings)))
}

#[cfg(all(feature = "online", not(test)))]
static MADE: OnceLock<Arc<Made>> = OnceLock::new();

#[cfg(all(feature = "online", not(test)))]
fn made_once(settings: Option<PathBuf>) -> Arc<Made> {
    Arc::clone(MADE.get_or_init(|| Arc::new(Made::new(settings))))
}

#[cfg(all(feature = "online", test))]
fn made_once(settings: Option<PathBuf>) -> Arc<Made> {
    thread_local! {
        static MADE: OnceLock<Arc<Made>> = const { OnceLock::new() };
    }
    MADE.with(|made| Arc::clone(made.get_or_init(|| Arc::new(Made::new(settings)))))
}

#[cfg(feature = "online")]
pub type InPlace = Vec<Arc<dyn PlaysInPlace>>;

#[cfg(feature = "online")]
pub fn in_place_named(config: &Config, settings: Option<PathBuf>) -> InPlace {
    in_place(&Accounts::of(config), &made_once(settings))
}

#[cfg(feature = "online")]
pub fn youtube_music_named(config: &Config, settings: Option<PathBuf>) -> Option<Arc<YtMusic>> {
    let accounts = Accounts::of(config);
    let login = accounts.youtube_music()?;
    Some(made_once(settings).youtube_music(login))
}

#[cfg(all(feature = "ui", feature = "online"))]
pub fn in_place_given(online: &resonate_ui::Online, settings: Option<PathBuf>) -> InPlace {
    in_place(&Accounts::given(online), &made_once(settings))
}

#[cfg(feature = "online")]
fn in_place(accounts: &Accounts, made: &Made) -> InPlace {
    accounts
        .0
        .iter()
        .map(|reachable| made.built(reachable).in_place())
        .collect()
}

#[cfg(feature = "online")]
enum Built {
    Subsonic(Arc<Subsonic>),
    Jellyfin(Arc<Jellyfin>),
    Tidal(Arc<Tidal>),
    HifiApi(Arc<HifiApi>),
    Monochrome(Arc<Monochrome>),
    YtMusic(Arc<YtMusic>),
}

#[cfg(feature = "online")]
impl Built {
    fn in_place(self) -> Arc<dyn PlaysInPlace> {
        match self {
            Self::Subsonic(service) => service,
            Self::Jellyfin(service) => service,
            Self::Tidal(service) => service,
            Self::HifiApi(service) => service,
            Self::Monochrome(service) => service,
            Self::YtMusic(service) => service,
        }
    }

    fn provider(self) -> Option<Arc<dyn resonate_providers::Provider>> {
        match self {
            Self::Subsonic(service) => Some(service),
            Self::Tidal(service) => Some(service),
            Self::HifiApi(service) => Some(service),
            Self::Monochrome(service) => Some(service),
            Self::YtMusic(service) => Some(service),
            Self::Jellyfin(_) => None,
        }
    }
}

#[cfg(feature = "online")]
fn the_tool_of(login: &YoutubeMusicLogin) -> (Option<PathBuf>, Cookies) {
    (
        login.program.as_deref().and_then(config::a_path),
        Cookies::read(&login.cookies),
    )
}

#[cfg(all(feature = "ui", feature = "online"))]
pub fn checks_youtube_music(settings: Option<PathBuf>) -> Option<resonate_ui::ChecksYoutubeMusic> {
    use resonate_ytmusic::Connection;

    let made = made_once(settings);
    Some(Arc::new(move |online: &resonate_ui::Online| {
        let accounts = Accounts::given(online);
        let login = accounts.youtube_music()?;
        Some(match made.youtube_music(login).check() {
            Connection::Premium => resonate_ui::YoutubeMusicConnection::Premium,
            Connection::SignedInWithoutPremium => {
                resonate_ui::YoutubeMusicConnection::SignedInWithoutPremium
            }
            Connection::NotSignedIn => resonate_ui::YoutubeMusicConnection::NotSignedIn,
            Connection::ToolMissing => resonate_ui::YoutubeMusicConnection::ToolMissing,
            Connection::CheckSongGone => resonate_ui::YoutubeMusicConnection::CheckSongGone,
            Connection::CouldNotCheck => resonate_ui::YoutubeMusicConnection::CouldNotCheck,
        })
    }))
}

#[cfg(all(feature = "ui", feature = "online"))]
#[derive(Default)]
struct QuickConnects {
    waiting: parking_lot::Mutex<std::collections::HashMap<String, Waiting>>,
}

#[cfg(all(feature = "ui", feature = "online"))]
struct Waiting {
    asking: resonate_jellyfin::QuickConnect,
    approving: resonate_jellyfin::Approving,
    began: std::time::Instant,
}

#[cfg(all(feature = "ui", feature = "online"))]
impl resonate_ui::SignsInToJellyfin for QuickConnects {
    fn begun(&self, address: &str) -> resonate_ui::JellyfinBegun {
        use resonate_ui::JellyfinBegun;

        let asking = resonate_jellyfin::QuickConnect::at(address);
        match asking.offered() {
            Ok(true) => {}
            Ok(false) => return JellyfinBegun::Off,
            Err(error) => {
                tracing::warn!(%error, "the Jellyfin server could not be asked about Quick Connect");
                return JellyfinBegun::Failed;
            }
        }
        match asking.begun() {
            Ok(approving) => {
                let code = approving.code.clone();
                let mut waiting = self.waiting.lock();
                waiting.retain(|_, held| {
                    held.began.elapsed() < resonate_jellyfin::A_QUICK_CONNECT_LAPSES_AFTER
                });
                waiting.insert(
                    code.clone(),
                    Waiting {
                        asking,
                        approving,
                        began: std::time::Instant::now(),
                    },
                );
                JellyfinBegun::Code(code)
            }
            Err(error) => {
                tracing::warn!(%error, "the Jellyfin server gave no Quick Connect code");
                JellyfinBegun::Failed
            }
        }
    }

    fn approved(
        &self,
        code: &str,
        stopped: &std::sync::atomic::AtomicBool,
    ) -> resonate_ui::JellyfinSignIn {
        use std::sync::atomic::Ordering;

        use resonate_providers::Error as Refused;
        use resonate_ui::{JellyfinSession, JellyfinSignIn};

        let Some(Waiting {
            asking, approving, ..
        }) = self.waiting.lock().remove(code)
        else {
            return JellyfinSignIn::Failed;
        };
        match asking.approved(&approving, &|| stopped.load(Ordering::SeqCst)) {
            Ok(Some(signed)) => JellyfinSignIn::SignedIn(JellyfinSession {
                token: signed.session.token,
                user_id: signed.session.user,
                device: signed.session.device,
                name: signed.name,
            }),
            Ok(None) => JellyfinSignIn::Stopped,
            Err(Refused::AuthorizationLapsed { .. }) => JellyfinSignIn::Lapsed,
            Err(Refused::AuthorizationDenied { .. }) => JellyfinSignIn::Denied,
            Err(error) => {
                tracing::warn!(%error, "the Jellyfin Quick Connect did not end signed in");
                JellyfinSignIn::Failed
            }
        }
    }
}

#[cfg(all(feature = "ui", feature = "online"))]
pub fn signs_in_to_jellyfin() -> Option<Arc<dyn resonate_ui::SignsInToJellyfin>> {
    Some(Arc::new(QuickConnects::default()))
}

#[cfg(all(feature = "ui", not(feature = "online")))]
pub fn signs_in_to_jellyfin() -> Option<Arc<dyn resonate_ui::SignsInToJellyfin>> {
    None
}

#[cfg(all(feature = "ui", feature = "online"))]
pub fn signs_in_to_youtube_music() -> Option<Arc<dyn resonate_ui::SignsInToYoutubeMusic>> {
    Some(Arc::new(crate::signing_in::FromTheWindow::here()))
}

#[cfg(all(feature = "ui", not(feature = "online")))]
pub fn signs_in_to_youtube_music() -> Option<Arc<dyn resonate_ui::SignsInToYoutubeMusic>> {
    None
}

#[cfg(all(feature = "ui", not(feature = "online")))]
pub fn checks_youtube_music(_settings: Option<PathBuf>) -> Option<resonate_ui::ChecksYoutubeMusic> {
    None
}

#[cfg(feature = "online")]
pub fn stop_the_tools() {
    resonate_ytmusic::stop_the_tools();
    crate::signing_in::stop_signing_in();
}

#[cfg(not(feature = "online"))]
pub const fn stop_the_tools() {}

#[cfg(feature = "ui")]
fn sourced_over(made: Arc<Made>) -> (resonate_ui::Registering, resonate_ui::Unkept) {
    let keeping = made.keeping();
    let register: resonate_ui::Registering =
        Arc::new(move |supplying: &resonate_ui::Supplying<'_>| {
            let every = registry(supplying.inbox, &Accounts::given(supplying.online), &made);
            match (supplying.asking, supplying.inbox) {
                (resonate_ui::Asking::TheInboxAlone, Some(folder)) => {
                    every.only(resonate_providers::Provider::source(&Inbox::at(folder)))
                }
                (resonate_ui::Asking::TheInboxAlone, None) => Providers::none(),
                (resonate_ui::Asking::EveryProvider, _) => every,
            }
        });
    let unkept: resonate_ui::Unkept = Arc::new(move || keeping.unkept().is_some());
    (register, unkept)
}

#[cfg(feature = "online")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Accounts(Vec<Reachable>);

#[cfg(not(feature = "online"))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Accounts;

#[cfg(feature = "online")]
struct Typed<'a> {
    subsonic: [&'a str; 3],
    jellyfin: [&'a str; 6],
    tidal: [&'a str; 3],
    hifi_api: [&'a str; 2],
    monochrome: &'a str,
    youtube_music: Option<[&'a str; 2]>,
    hosted_services: bool,
}

#[cfg(feature = "online")]
impl Typed<'_> {
    fn accounts(self) -> Accounts {
        let owned = |text: &str| text.to_owned();
        let [server, user, password] = self.subsonic.map(owned);
        let subsonic = ServiceAccount::Subsonic {
            server,
            user,
            password,
        };
        let [server, user, password, token, user_id, device] = self.jellyfin.map(owned);
        let jellyfin = ServiceAccount::Jellyfin {
            server,
            user,
            password,
            token,
            user_id,
            device,
        };
        let [client_id, client_secret, refresh_token] = self.tidal.map(owned);
        let tidal = ServiceAccount::Tidal {
            client_id,
            client_secret,
            refresh_token,
        };
        let [server, key] = self.hifi_api.map(owned);
        let hifi_api = ServiceAccount::HifiApi { server, key };
        let monochrome = ServiceAccount::Monochrome {
            server: owned(self.monochrome),
        };
        let youtube_music =
            self.youtube_music
                .map(|[cookies, program]| ServiceAccount::YoutubeMusic {
                    cookies: owned(cookies),
                    program: owned(program),
                });

        Accounts(reachable(
            [subsonic, jellyfin, tidal, hifi_api, monochrome]
                .into_iter()
                .chain(youtube_music),
            HostedServices::allowed(self.hosted_services),
        ))
    }
}

#[cfg(feature = "online")]
fn text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or_default()
}

#[cfg(feature = "online")]
impl Accounts {
    fn of(config: &Config) -> Self {
        if !config.online_enabled() {
            return Self::default();
        }
        Typed {
            subsonic: [
                text(&config.subsonic),
                text(&config.subsonic_user),
                text(&config.subsonic_password),
            ],
            jellyfin: [
                text(&config.jellyfin),
                text(&config.jellyfin_user),
                text(&config.jellyfin_password),
                text(&config.jellyfin_token),
                text(&config.jellyfin_user_id),
                text(&config.jellyfin_device),
            ],
            tidal: [
                text(&config.tidal_client_id),
                text(&config.tidal_client_secret),
                text(&config.tidal_refresh_token),
            ],
            hifi_api: [text(&config.hifi_api), text(&config.hifi_secret)],
            monochrome: text(&config.monochrome),
            youtube_music: config
                .turns_youtube_music_on()
                .then(|| [text(&config.youtube_music_cookies), text(&config.yt_dlp)]),
            hosted_services: config.hosted_services_allowed(),
        }
        .accounts()
    }

    #[cfg(feature = "ui")]
    fn given(online: &resonate_ui::Online) -> Self {
        if !online.enabled {
            return Self::default();
        }
        Typed {
            subsonic: [
                &online.subsonic,
                &online.subsonic_user,
                &online.subsonic_password,
            ],
            jellyfin: [
                &online.jellyfin,
                &online.jellyfin_user,
                &online.jellyfin_password,
                &online.jellyfin_token,
                &online.jellyfin_user_id,
                &online.jellyfin_device,
            ],
            tidal: [
                &online.tidal_client_id,
                &online.tidal_client_secret,
                &online.tidal_refresh_token,
            ],
            hifi_api: [&online.hifi_api, &online.hifi_secret],
            monochrome: &online.monochrome,
            youtube_music: online.youtube_music.then_some([
                online.youtube_music_cookies.as_str(),
                online.yt_dlp.as_str(),
            ]),
            hosted_services: online.hosted_services,
        }
        .accounts()
    }

    fn youtube_music(&self) -> Option<&YoutubeMusicLogin> {
        self.0.iter().find_map(|reachable| match reachable {
            Reachable::YoutubeMusic(login) => Some(login),
            _ => None,
        })
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
struct Made {
    subsonic: Kept<SubsonicLogin, Subsonic>,
    jellyfin: Kept<JellyfinLogin, Jellyfin>,
    tidal: Kept<TidalLogin, Tidal>,
    resonate: Kept<Key, HifiApi>,
    hifi: Kept<Hosting, HifiApi>,
    monochrome: Kept<Hosting, Monochrome>,
    ytmusic: Kept<YoutubeMusicLogin, YtMusic>,
    renewed: Arc<Renewed>,
}

#[cfg(not(feature = "online"))]
struct Made;

#[cfg(not(feature = "online"))]
impl Made {
    fn new(_settings: Option<PathBuf>) -> Self {
        Self
    }

    const fn keeping(&self) -> Keeping {
        Keeping
    }
}

#[cfg(feature = "online")]
impl Made {
    fn new(settings: Option<PathBuf>) -> Self {
        Self {
            subsonic: Kept::default(),
            jellyfin: Kept::default(),
            tidal: Kept::default(),
            resonate: Kept::default(),
            hifi: Kept::default(),
            monochrome: Kept::default(),
            ytmusic: Kept::default(),
            renewed: Arc::new(Renewed {
                settings,
                rotated: parking_lot::Mutex::new(None),
                unkept: parking_lot::Mutex::new(None),
            }),
        }
    }

    fn keeping(&self) -> Keeping {
        Keeping(Arc::clone(&self.renewed))
    }

    fn signed_in(&self, login: &TidalLogin) -> Tidal {
        let account = Account {
            client_id: login.client_id.clone(),
            client_secret: login.client_secret.clone(),
            refresh_token: login.refresh_token.clone(),
        };
        let renewed = Arc::clone(&self.renewed);
        let given = account.refresh_token.clone();
        Tidal::signed_in(self.renewed.standing_for(&account))
            .telling(move |token| renewed.note(given.clone(), token))
    }

    fn youtube_music(&self, login: &YoutubeMusicLogin) -> Arc<YtMusic> {
        self.ytmusic.made_for(login, |login| {
            let (program, cookies) = the_tool_of(login);
            YtMusic::new(Tool::new(program, cookies))
        })
    }

    fn built(&self, reachable: &Reachable) -> Built {
        match reachable {
            Reachable::Subsonic(login) => Built::Subsonic(self.subsonic.made_for(login, |login| {
                Subsonic::at(Server {
                    url: login.url.clone(),
                    user: login.user.clone(),
                    password: login.password.clone(),
                })
            })),
            Reachable::Jellyfin(login) => Built::Jellyfin(self.jellyfin.made_for(login, |login| {
                Jellyfin::at(resonate_jellyfin::Server {
                    url: login.url.clone(),
                    user: login.user.clone(),
                    password: login.password.clone(),
                    kept: login
                        .session
                        .as_ref()
                        .map(|session| resonate_jellyfin::KeptSession {
                            token: session.token.clone(),
                            user: session.user.clone(),
                            device: session.device.clone(),
                        }),
                })
            })),
            Reachable::Tidal(login) => {
                Built::Tidal(self.tidal.made_for(login, |login| self.signed_in(login)))
            }
            Reachable::Resonate(key) => Built::HifiApi(self.resonate.made_for(key, |key| {
                HifiApi::at(resonate_tidal::OWN_SERVER)
                    .with_secret(key.as_str())
                    .known_as_resonate()
            })),
            Reachable::HifiApi(hosting) => {
                Built::HifiApi(self.hifi.made_for(hosting, |hosting| match hosting {
                    Hosting::Hosted => HifiApi::hosted(),
                    Hosting::Custom(server) => HifiApi::at(server),
                }))
            }
            Reachable::Monochrome(hosting) => {
                Built::Monochrome(self.monochrome.made_for(hosting, |hosting| match hosting {
                    Hosting::Hosted => Monochrome::hosted(),
                    Hosting::Custom(server) => Monochrome::at(server),
                }))
            }
            Reachable::YoutubeMusic(login) => Built::YtMusic(self.youtube_music(login)),
        }
    }
}

#[cfg(feature = "online")]
struct Renewed {
    settings: Option<PathBuf>,
    rotated: parking_lot::Mutex<Option<Rotation>>,
    unkept: parking_lot::Mutex<Option<Error>>,
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
        let failure = match &self.settings {
            None => Some(Error::TidalTokenHasNoSettings),
            Some(settings) => config::store(settings, ConfigKey::TidalRefreshToken, standing)
                .err()
                .map(|source| Error::TidalTokenNotKept {
                    path: settings.clone(),
                    source: Box::new(source),
                }),
        };
        if let Some(failure) = &failure {
            tracing::error!(%failure, "the refresh token TIDAL rotated could not be kept");
        }
        *self.unkept.lock() = failure;
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
    accounts
        .0
        .iter()
        .filter(|reachable| reachable.fetches_for_wants())
        .filter_map(|reachable| made.built(reachable).provider())
        .fold(with_inbox(inbox), Providers::and)
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
        "{}",
        crate::words::providers::open_to_sign_in(
            authorizing.verify_at.to_string(),
            authorizing.user_code.to_string(),
        )
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
        "{}",
        crate::words::providers::signed_in(settings.display().to_string())
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

    #[cfg(feature = "online")]
    use resonate_core::SourceId;

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
    fn a_jellyfin_server_is_played_in_place_only_with_its_account_and_while_online_and_is_no_provider()
     {
        let schemes = |config: &Config| -> Vec<String> {
            in_place(&Accounts::of(config), &Made::new(None))
                .iter()
                .map(|served| served.scheme().as_str().to_owned())
                .collect()
        };
        let halfway = Config {
            jellyfin: Some("http://music.local:8096".to_owned()),
            jellyfin_user: Some("listener".to_owned()),
            ..Config::default()
        };
        assert!(!schemes(&halfway).contains(&"jellyfin".to_owned()));

        let whole = Config {
            jellyfin_password: Some("sesame".to_owned()),
            ..halfway
        };
        assert!(schemes(&whole).contains(&"jellyfin".to_owned()));
        assert!(!named(&whole).contains(&"jellyfin".to_owned()));

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!schemes(&offline).contains(&"jellyfin".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_jellyfin_account_is_ready_with_a_password_or_a_whole_kept_session() {
        let jellyfin_of = |config: &Config| {
            Accounts::of(config)
                .0
                .into_iter()
                .find_map(|reachable| match reachable {
                    Reachable::Jellyfin(login) => Some(login),
                    _ => None,
                })
        };
        let ready = |config: &Config| jellyfin_of(config).is_some();
        let address = Config {
            jellyfin: Some("http://music.local:8096".to_owned()),
            ..Config::default()
        };
        let kept = Config {
            jellyfin_token: Some("token".to_owned()),
            jellyfin_user_id: Some("0a1b".to_owned()),
            jellyfin_device: Some("device".to_owned()),
            ..address.clone()
        };
        let half_kept = Config {
            jellyfin_device: None,
            ..kept.clone()
        };

        assert!(!ready(&address));
        assert!(ready(&kept));
        assert!(!ready(&half_kept));
        assert!(jellyfin_of(&kept).is_some_and(|login| login.session.is_some()));
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
    fn a_key_registers_the_resonate_server_beside_whatever_hifi_api_is_named() {
        let keyed = |server: Option<&str>, hosted: bool| Config {
            hifi_api: server.map(str::to_owned),
            hifi_secret: Some(" key ".to_owned()),
            hosted_services: Some(hosted),
            ..Config::default()
        };

        let alone = keyed(None, false);
        assert!(Accounts::of(&alone).0.iter().any(
            |reachable| matches!(reachable, Reachable::Resonate(key) if key.as_str() == "key")
        ));
        assert!(named(&alone).contains(&"resonate".to_owned()));
        assert!(!named(&alone).contains(&"hifi-api".to_owned()));

        let beside = keyed(Some("https://hifi.home.arpa"), true);
        assert!(
            Accounts::of(&beside)
                .0
                .contains(&Reachable::HifiApi(Hosting::Custom(
                    "https://hifi.home.arpa".to_owned()
                )))
        );
        let order = named(&beside);
        let place = |name: &str| order.iter().position(|each| each == name);
        assert!(place("resonate").is_some());
        assert!(place("resonate") < place("hifi-api"));
        assert!(place("hifi-api") < place("monochrome"));

        assert!(
            Accounts::of(&Config {
                hifi_secret: Some("  ".to_owned()),
                hosted_services: Some(false),
                ..Config::default()
            })
            .0
            .is_empty()
        );
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_key_is_registered_and_never_prints_its_secret() {
        let config = Config {
            hifi_secret: Some("hush-secret".to_owned()),
            hosted_services: Some(false),
            ..Config::default()
        };

        assert!(named(&config).contains(&"resonate".to_owned()));
        assert!(!format!("{:?}", Accounts::of(&config)).contains("hush-secret"));
    }

    #[cfg(feature = "online")]
    #[test]
    fn the_hosted_hifi_api_is_registered_only_when_the_listener_allowed_hosted_services() {
        let unasked = Config {
            hosted_services: Some(false),
            ..Config::default()
        };
        assert!(!named(&unasked).contains(&"hifi-api".to_owned()));

        let allowed = Config {
            hosted_services: Some(true),
            ..Config::default()
        };
        assert!(named(&allowed).contains(&"hifi-api".to_owned()));
        let blank = Config {
            hifi_api: Some("  ".to_owned()),
            ..allowed.clone()
        };
        assert!(
            Accounts::of(&blank)
                .0
                .contains(&Reachable::HifiApi(Hosting::Hosted))
        );

        let offline = Config {
            online: Some(false),
            ..allowed
        };
        assert!(!named(&offline).contains(&"hifi-api".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_hifi_api_server_the_listener_named_is_registered_without_hosted_services() {
        let whole = Config {
            hifi_api: Some("http://hifi.home.arpa:8000".to_owned()),
            ..Config::default()
        };

        assert!(named(&whole).contains(&"hifi-api".to_owned()));
        assert!(
            Accounts::of(&whole)
                .0
                .iter()
                .any(|reachable| matches!(reachable, Reachable::HifiApi(Hosting::Custom(_))))
        );

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"hifi-api".to_owned()));
    }

    #[cfg(feature = "online")]
    fn monochrome_of(config: &Config) -> bool {
        Accounts::of(config)
            .0
            .iter()
            .any(|reachable| matches!(reachable, Reachable::Monochrome(_)))
    }

    #[cfg(feature = "online")]
    #[test]
    fn the_hosted_monochrome_is_registered_after_hifi_api_only_when_hosted_services_are_allowed() {
        let unasked = Config {
            hosted_services: Some(false),
            ..Config::default()
        };
        assert!(!named(&unasked).contains(&"monochrome".to_owned()));
        assert!(!monochrome_of(&unasked));

        let allowed = Config {
            hosted_services: Some(true),
            ..Config::default()
        };
        let hosted = named(&allowed);
        assert!(hosted.contains(&"monochrome".to_owned()));
        assert_eq!(
            hosted.iter().position(|name| name == "monochrome"),
            hosted
                .iter()
                .position(|name| name == "hifi-api")
                .map(|hifi| hifi + 1)
        );
        assert!(
            Accounts::of(&allowed)
                .0
                .contains(&Reachable::Monochrome(Hosting::Hosted))
        );

        let whole = Config {
            monochrome: Some("https://tracks.home.arpa".to_owned()),
            ..Config::default()
        };
        assert!(named(&whole).contains(&"monochrome".to_owned()));
        assert!(
            Accounts::of(&whole)
                .0
                .contains(&Reachable::Monochrome(Hosting::Custom(
                    "https://tracks.home.arpa".to_owned()
                )))
        );

        let offline = Config {
            online: Some(false),
            ..whole
        };
        assert!(!named(&offline).contains(&"monochrome".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn youtube_music_is_registered_only_where_it_is_turned_on_and_online_is_on() {
        let unasked = Config::default();
        assert!(!named(&unasked).contains(&"ytmusic".to_owned()));
        assert_eq!(Accounts::of(&unasked).youtube_music(), None);

        let turned_off = Config {
            youtube_music: Some(false),
            youtube_music_cookies: Some("firefox".to_owned()),
            ..Config::default()
        };
        assert!(!named(&turned_off).contains(&"ytmusic".to_owned()));

        let turned_on = Config {
            youtube_music: Some(true),
            ..Config::default()
        };
        assert!(named(&turned_on).contains(&"ytmusic".to_owned()));

        let offline = Config {
            online: Some(false),
            ..turned_on
        };
        assert!(!named(&offline).contains(&"ytmusic".to_owned()));
    }

    #[cfg(feature = "online")]
    #[test]
    fn youtube_music_is_registered_last() {
        let config = Config {
            youtube_music: Some(true),
            subsonic: Some("http://music.local:4533".to_owned()),
            subsonic_user: Some("listener".to_owned()),
            subsonic_password: Some("sesame".to_owned()),
            hosted_services: Some(true),
            ..Config::default()
        };

        let providers = registered(&config, None);

        assert_eq!(
            providers.names().last().map(SourceId::as_str),
            Some("ytmusic")
        );
    }

    #[cfg(feature = "online")]
    #[test]
    fn youtube_music_is_served_after_monochrome_only_where_it_is_turned_on() {
        let schemes = |config: &Config| -> Vec<String> {
            in_place(&Accounts::of(config), &Made::new(None))
                .iter()
                .map(|served| served.scheme().as_str().to_owned())
                .collect()
        };
        let off = Config {
            hosted_services: Some(true),
            ..Config::default()
        };
        let on = Config {
            youtube_music: Some(true),
            ..off.clone()
        };

        assert!(!schemes(&off).contains(&"ytmusic".to_owned()));
        assert_eq!(
            schemes(&on).last().map(String::as_str),
            Some("ytmusic"),
            "{:?}",
            schemes(&on)
        );
        assert_eq!(
            schemes(&on).iter().rev().nth(1).map(String::as_str),
            Some("monochrome")
        );
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_youtube_music_account_is_named_by_its_cookies_and_program_and_prints_neither_cookie() {
        let config = Config {
            youtube_music: Some(true),
            youtube_music_cookies: Some(" firefox:/home/a/zen ".to_owned()),
            yt_dlp: Some(" /opt/yt-dlp ".to_owned()),
            ..Config::default()
        };

        let accounts = Accounts::of(&config);
        let login = accounts.youtube_music().expect("an account");

        assert_eq!(
            the_tool_of(login),
            (
                Some(PathBuf::from("/opt/yt-dlp")),
                Cookies::Browser("firefox:/home/a/zen".to_owned()),
            )
        );
        assert!(!format!("{:?}", Accounts::of(&config)).contains("/home/a/zen"));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_youtube_music_account_with_nothing_given_asks_the_path_and_no_cookies() {
        let accounts = Accounts::of(&Config {
            youtube_music: Some(true),
            youtube_music_cookies: Some("  ".to_owned()),
            yt_dlp: Some("  ".to_owned()),
            ..Config::default()
        });
        let login = accounts.youtube_music().expect("an account");

        assert_eq!(the_tool_of(login), (None, Cookies::None));
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_rotated_token_that_could_not_be_written_is_told_to_the_listener_once() {
        let rotate = |made: &Made| {
            made.renewed.note(
                "first".to_owned(),
                resonate_providers::RefreshToken::new("second".to_owned()),
            );
            made.keeping().unkept()
        };

        let nowhere = Made::new(None);
        assert!(matches!(
            rotate(&nowhere),
            Some(Error::TidalTokenHasNoSettings)
        ));
        assert!(nowhere.keeping().unkept().is_none());

        let folder = std::env::temp_dir().join(format!("resonate-unkept-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a writable temporary directory");
        let file = folder.join("file");
        std::fs::write(&file, "").expect("a writable temporary directory");
        let beneath_a_file = file.join("config.toml");
        let unwritable = Made::new(Some(beneath_a_file.clone()));
        let failed = rotate(&unwritable);
        let kept = Made::new(Some(folder.join("config.toml")));
        let succeeded = rotate(&kept);
        std::fs::remove_dir_all(&folder).expect("the temporary directory removed");

        assert!(
            matches!(failed, Some(Error::TidalTokenNotKept { path, .. }) if path == beneath_a_file)
        );
        assert!(succeeded.is_none());
    }

    #[cfg(all(feature = "online", feature = "ui"))]
    #[test]
    fn the_window_registers_youtube_music_only_while_its_switch_and_online_are_on() {
        let on = resonate_ui::Online {
            enabled: true,
            youtube_music: true,
            youtube_music_cookies: "firefox".to_owned(),
            yt_dlp: "/opt/yt-dlp".to_owned(),
            ..resonate_ui::Online::default()
        };
        let off = resonate_ui::Online {
            youtube_music: false,
            ..on.clone()
        };
        let offline = resonate_ui::Online {
            enabled: false,
            ..on.clone()
        };

        let accounts = Accounts::given(&on);
        assert_eq!(
            accounts.youtube_music().map(the_tool_of),
            Some((
                Some(PathBuf::from("/opt/yt-dlp")),
                Cookies::Browser("firefox".to_owned()),
            ))
        );
        assert_eq!(Accounts::given(&off).youtube_music(), None);
        assert_eq!(Accounts::given(&offline).youtube_music(), None);
    }

    #[cfg(all(feature = "online", feature = "ui"))]
    #[test]
    fn a_check_is_answered_only_where_the_settings_turn_youtube_music_on() {
        let checks = checks_youtube_music(None).expect("a build that can check");
        let missing = resonate_ui::Online {
            enabled: true,
            youtube_music: true,
            yt_dlp: "/nowhere/at/all/yt-dlp".to_owned(),
            ..resonate_ui::Online::default()
        };
        let off = resonate_ui::Online {
            youtube_music: false,
            ..missing.clone()
        };

        assert_eq!(
            checks(&missing),
            Some(resonate_ui::YoutubeMusicConnection::ToolMissing)
        );
        assert_eq!(checks(&off), None);
    }

    #[cfg(all(feature = "online", feature = "ui"))]
    #[test]
    fn the_window_is_told_of_a_rotated_token_that_could_not_be_written_once() {
        let made = Arc::new(Made::new(None));
        let (_, unkept) = sourced_over(Arc::clone(&made));

        assert!(!unkept());

        made.renewed.note(
            "first".to_owned(),
            resonate_providers::RefreshToken::new("second".to_owned()),
        );

        assert!(unkept());
        assert!(!unkept());
    }

    #[cfg(all(feature = "online", feature = "ui"))]
    #[test]
    fn the_window_registers_what_its_settings_say_now_and_keeps_a_provider_its_settings_left_alone()
    {
        let (register, _) = sourced(None);
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
        assert!(!named(&signed_in).contains(&"hifi-api".to_owned()));
        assert!(!named(&signed_in).contains(&"monochrome".to_owned()));

        let hosted = resonate_ui::Online {
            hosted_services: true,
            ..signed_in.clone()
        };
        assert!(named(&hosted).contains(&"hifi-api".to_owned()));
        assert!(named(&hosted).contains(&"monochrome".to_owned()));

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
        let tidal = |accounts: &Accounts| match accounts
            .0
            .iter()
            .find(|reachable| matches!(reachable, Reachable::Tidal(_)))
            .map(|reachable| made.built(reachable))
        {
            Some(Built::Tidal(tidal)) => tidal,
            _ => panic!("a TIDAL account"),
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
