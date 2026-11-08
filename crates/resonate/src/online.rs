use std::sync::{Arc, atomic::AtomicBool};
#[cfg(feature = "online")]
use std::{
    fs,
    path::PathBuf,
    sync::{OnceLock, atomic::Ordering},
    time::SystemTime,
};

#[cfg(feature = "online")]
use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_eq::Corrected;
#[cfg(feature = "online")]
use resonate_library::Scrobbler;
use resonate_library::{Fingerprinters, Library, Reference};
use resonate_listen::Recognisers;
#[cfg(feature = "ui")]
use resonate_lyrics::Lyricists;
#[cfg(all(feature = "online", feature = "ui"))]
use resonate_online::OnlineLyrics;
#[cfg(feature = "online")]
use resonate_online::{
    AcoustId, Application, Audd, AutoEq, ByEar, Client, Identity, Introduction, LastFm,
    ListenBrainz, Online, Session, Shazam, SpotifyAccount, SpotifyGrant, SpotifyGrants,
    SpotifySignIn,
};

#[cfg(feature = "online")]
use crate::metered;
use crate::{Error, Result, config::Config};
#[cfg(feature = "online")]
use crate::{
    config::{self, LastfmAccount},
    error::ConfigKey,
    signals,
};

#[cfg(feature = "online")]
static INTRODUCTION: OnceLock<Introduction> = OnceLock::new();

#[cfg(feature = "online")]
static CLIENT: OnceLock<Arc<Client>> = OnceLock::new();

#[cfg(feature = "online")]
static YIELDING: OnceLock<Arc<Client>> = OnceLock::new();

#[cfg(feature = "online")]
static SPOTIFY: OnceLock<Arc<SpotifyAccount>> = OnceLock::new();

#[cfg(feature = "online")]
static REACHING: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asking {
    ForTheListener,
    InTheBackground,
}

#[cfg(feature = "online")]
fn identity(contact: Option<String>) -> Identity {
    Identity {
        contact,
        ..Identity::of_this_build()
    }
}

#[cfg(feature = "online")]
fn client(config: &Config) -> Arc<Client> {
    let client = CLIENT.get_or_init(|| {
        Arc::new(
            Client::introduced(introduction(config).clone()).gated_by(metered::for_lookups(config)),
        )
    });
    client.reach(REACHING.load(Ordering::Relaxed));
    Arc::clone(client)
}

#[cfg(feature = "online")]
fn client_for(config: &Config, asking: Asking) -> Arc<Client> {
    match asking {
        Asking::ForTheListener => client(config),
        Asking::InTheBackground => {
            Arc::clone(YIELDING.get_or_init(|| Arc::new(client(config).yielding())))
        }
    }
}

#[cfg(feature = "online")]
fn introduction(config: &Config) -> &'static Introduction {
    INTRODUCTION.get_or_init(|| {
        let said = identity(config.contact.clone());
        match config.read_from.clone() {
            Some(path) => {
                let followed = Followed {
                    path,
                    seen: Mutex::new(None),
                };
                Introduction::following(&said, Arc::new(move || followed.moved()))
            }
            None => Introduction::as_(&said),
        }
    })
}

#[cfg(feature = "online")]
struct Followed {
    path: PathBuf,
    seen: Mutex<Option<SystemTime>>,
}

#[cfg(feature = "online")]
impl Followed {
    fn moved(&self) -> Option<Identity> {
        let stamp = fs::metadata(&self.path)
            .and_then(|held| held.modified())
            .ok();
        {
            let mut seen = self.seen.lock();
            if *seen == stamp {
                return None;
            }
            *seen = stamp;
        }

        match config::contact_in(&self.path) {
            Ok(contact) => Some(identity(contact)),
            Err(error) => {
                tracing::warn!(%error, "the contact in the settings went unread, so what was said goes on being said");
                None
            }
        }
    }
}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn introduce(contact: Option<&str>) {
    if let Some(introduction) = INTRODUCTION.get() {
        introduction.change_to(&identity(contact.map(str::to_owned)));
    }
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn introduce(_contact: Option<&str>) {}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn reach(on: bool) {
    REACHING.store(on, Ordering::Relaxed);
    if let Some(client) = CLIENT.get() {
        client.reach(on);
    }
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn reach(_on: bool) {}

#[cfg(feature = "online")]
pub fn reference(config: &Config, asking: Asking) -> Option<Arc<dyn Reference>> {
    let online = config.online_enabled().then(|| {
        Arc::new(Online::with_client(client_for(config, asking)).with_spotify(spotify(config)))
            as Arc<dyn Reference>
    });
    crate::musicbrainz::layered(config, online)
}

#[cfg(feature = "online")]
fn spotify(config: &Config) -> Arc<SpotifyAccount> {
    Arc::clone(
        SPOTIFY
            .get_or_init(|| Arc::new(SpotifyAccount::new(Arc::new(KeptInTheSettings::of(config))))),
    )
}

#[cfg(feature = "online")]
struct KeptInTheSettings {
    settings: Option<PathBuf>,
    read: Mutex<(Option<SystemTime>, Option<SpotifyGrant>)>,
}

#[cfg(feature = "online")]
impl KeptInTheSettings {
    fn of(config: &Config) -> Self {
        let given = config.spotify_signed_in().then(|| SpotifyGrant {
            client_id: config.spotify_client_id.clone().unwrap_or_default(),
            refresh_token: config.spotify_refresh_token.clone().unwrap_or_default(),
        });
        Self {
            settings: config
                .read_from
                .clone()
                .or_else(|| config::config_path().ok()),
            read: Mutex::new((None, given)),
        }
    }
}

#[cfg(feature = "online")]
impl SpotifyGrants for KeptInTheSettings {
    fn grant(&self) -> Option<SpotifyGrant> {
        let mut read = self.read.lock();
        let Some(settings) = &self.settings else {
            return read.1.clone();
        };
        let moved = fs::metadata(settings).and_then(|file| file.modified()).ok();
        if moved != read.0 {
            match config::spotify_in(settings) {
                Ok(given) => {
                    *read = (
                        moved,
                        given.map(|(client_id, refresh_token)| SpotifyGrant {
                            client_id,
                            refresh_token,
                        }),
                    );
                }
                Err(error) => tracing::warn!(%error, "the Spotify account could not be read"),
            }
        }
        read.1.clone()
    }

    fn renewed(&self, _grant: &SpotifyGrant, token: &str) {
        let Some(settings) = &self.settings else {
            tracing::error!("Spotify rotated the refresh token and no settings file can keep it");
            return;
        };
        if let Err(error) = config::store(settings, ConfigKey::SpotifyRefreshToken, token) {
            tracing::error!(%error, "the refresh token Spotify rotated could not be kept");
        }
    }
}

#[cfg(all(feature = "online", feature = "ui"))]
struct SignInForTheWindow {
    signs_in: SpotifySignIn,
}

#[cfg(all(feature = "online", feature = "ui"))]
struct ApprovalForTheWindow {
    signs_in: SpotifySignIn,
    authorizing: resonate_online::SpotifyAuthorizing,
}

#[cfg(all(feature = "online", feature = "ui"))]
impl resonate_library::SignsInToSpotify for SignInForTheWindow {
    fn authorizing(
        &self,
        client_id: &str,
    ) -> std::result::Result<
        Box<dyn resonate_library::SpotifyApproval>,
        resonate_library::SpotifySignInFailure,
    > {
        Ok(Box::new(ApprovalForTheWindow {
            signs_in: self.signs_in.clone(),
            authorizing: self.signs_in.authorizing(client_id)?,
        }))
    }
}

#[cfg(all(feature = "online", feature = "ui"))]
impl resonate_library::SpotifyApproval for ApprovalForTheWindow {
    fn address(&self) -> &str {
        &self.authorizing.authorize_at
    }

    fn approved(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<Option<String>, resonate_library::SpotifySignInFailure> {
        self.signs_in
            .authorized(&self.authorizing, cancelled)
            .map(|signed_in| signed_in.map(|signed_in| signed_in.refresh_token))
            .map_err(Into::into)
    }
}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn signs_in_to_spotify(config: &Config) -> Option<Arc<dyn resonate_library::SignsInToSpotify>> {
    Some(Arc::new(SignInForTheWindow {
        signs_in: SpotifySignIn::new(client_for(config, Asking::ForTheListener)),
    }))
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn signs_in_to_spotify(
    _config: &Config,
) -> Option<Arc<dyn resonate_library::SignsInToSpotify>> {
    None
}

#[cfg(feature = "online")]
pub fn signed_in_to_spotify(config: &Config) -> Result<String> {
    use std::sync::atomic::{AtomicBool, Ordering};

    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    let client_id = config
        .spotify_client_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or(Error::NoSpotifyClient)?;

    let signs_in = SpotifySignIn::new(client_for(config, Asking::ForTheListener));
    let authorizing = signs_in.authorizing(client_id)?;
    said!(
        "open this address to sign in to Spotify, with {} listed among the app's redirect URIs:",
        resonate_online::SPOTIFY_REDIRECT
    );
    said!("{}", authorizing.authorize_at);

    let cancelled = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&cancelled);
    let _interrupting = signals::cancel_when_told(move || stopping.store(true, Ordering::Relaxed));
    signs_in
        .authorized(&authorizing, &|| cancelled.load(Ordering::Relaxed))?
        .map(|signed_in| signed_in.refresh_token)
        .ok_or(Error::SpotifySignInCancelled)
}

#[cfg(not(feature = "online"))]
pub fn signed_in_to_spotify(_config: &Config) -> Result<String> {
    Err(Error::NoSignIn)
}

#[cfg(feature = "online")]
pub fn reference_asked_for(config: &Config, asking: Asking) -> Result<Arc<dyn Reference>> {
    reference(config, asking).ok_or(Error::OnlineOff)
}

pub fn by_sound(config: &Config) -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(config.identifies_by_sound()))
}

#[cfg(feature = "ui")]
pub fn lyrics_by_the_locale(config: &Config) -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(config.chooses_lyrics_by_the_locale()))
}

#[cfg(feature = "online")]
pub fn fingerprinters(
    config: &Config,
    sources: Arc<Sources>,
    by_sound: &Arc<AtomicBool>,
    asking: Asking,
) -> Fingerprinters {
    let local = Fingerprinters::none();
    if !config.online_enabled() {
        return local;
    }
    let printed = match config.acoustid_key.clone() {
        Some(key) => local.and(Arc::new(AcoustId::new(client_for(config, asking), key))),
        None => local,
    };
    printed.and(Arc::new(ByEar::new(
        client_for(config, asking),
        sources,
        Arc::clone(by_sound),
    )))
}

#[cfg(feature = "online")]
pub fn listenbrainz(config: &Config, token: String) -> Arc<dyn Scrobbler> {
    Arc::new(ListenBrainz::new(client(config), token))
}

#[cfg(feature = "online")]
pub fn lastfm(config: &Config, account: LastfmAccount) -> Arc<dyn Scrobbler> {
    let LastfmAccount {
        key,
        secret,
        session,
    } = account;
    Arc::new(LastFm::new(
        client(config),
        Application { key, secret },
        session,
    ))
}

#[cfg(feature = "online")]
pub fn lastfm_signed_in(
    config: &Config,
    application: &Application,
    user: &str,
    password: &str,
) -> resonate_library::Result<Session> {
    resonate_online::signed_in(&client(config), application, user, password)
}

#[cfg(all(feature = "online", feature = "ui"))]
struct ByToken {
    config: Config,
}

#[cfg(all(feature = "online", feature = "ui"))]
impl resonate_library::Scrobblers for ByToken {
    fn under(&self, token: String) -> Arc<dyn Scrobbler> {
        listenbrainz(&self.config, token)
    }

    fn signed_in_to_lastfm(
        &self,
        asked: &resonate_library::LastfmSignIn,
    ) -> resonate_library::Result<resonate_library::LastfmSession> {
        let application = Application {
            key: asked.key.clone(),
            secret: asked.secret.clone(),
        };
        let Session { name, key } =
            lastfm_signed_in(&self.config, &application, &asked.user, &asked.password)?;
        Ok(resonate_library::LastfmSession { name, key })
    }
}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn scrobblers(config: &Config) -> Option<Arc<dyn resonate_library::Scrobblers>> {
    Some(Arc::new(ByToken {
        config: config.clone(),
    }))
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn scrobblers(_config: &Config) -> Option<Arc<dyn resonate_library::Scrobblers>> {
    None
}

#[cfg(not(feature = "online"))]
pub fn fingerprinters(
    _config: &Config,
    _sources: Arc<Sources>,
    _by_sound: &Arc<AtomicBool>,
    _asking: Asking,
) -> Fingerprinters {
    Fingerprinters::none()
}

#[cfg(feature = "online")]
pub fn recognisers(config: &Config) -> Recognisers {
    if !config.online_enabled() {
        return Recognisers::none();
    }
    let client = client(config);
    let mut recognisers = Recognisers::none().and(Arc::new(Shazam::new(Arc::clone(&client))));
    if let Some(token) = config.audd_token.clone() {
        recognisers = recognisers.and(Arc::new(Audd::new(client, token)));
    }
    recognisers
}

#[cfg(not(feature = "online"))]
pub fn recognisers(_config: &Config) -> Recognisers {
    Recognisers::none()
}

#[cfg(feature = "online")]
pub fn corrections(config: &Config, library: Option<Arc<Library>>) -> Corrected {
    let local = Corrected::uncorrected();
    if !config.online_enabled() {
        return local;
    }
    local.and(Arc::new(AutoEq::new(client(config), library)))
}

#[cfg(feature = "online")]
pub fn corrections_asked_for(config: &Config, library: Option<Arc<Library>>) -> Result<Corrected> {
    let corrections = corrections(config, library);
    corrections
        .has_a_source()
        .then_some(corrections)
        .ok_or(Error::OnlineOff)
}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn lyricists(
    config: &Config,
    library: Option<Arc<Library>>,
    by_the_locale: &Arc<AtomicBool>,
) -> Lyricists {
    let local = Lyricists::local_choosing_by_the_locale(Arc::clone(by_the_locale));
    if !config.online_enabled() {
        return local;
    }
    local.and(Arc::new(OnlineLyrics::new(client(config), library)))
}

#[cfg(not(feature = "online"))]
pub fn reference(_config: &Config, _asking: Asking) -> Option<Arc<dyn Reference>> {
    None
}

#[cfg(not(feature = "online"))]
pub fn reference_asked_for(_config: &Config, _asking: Asking) -> Result<Arc<dyn Reference>> {
    Err(Error::NoReference)
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn lyricists(
    _config: &Config,
    _library: Option<Arc<Library>>,
    by_the_locale: &Arc<AtomicBool>,
) -> Lyricists {
    Lyricists::local_choosing_by_the_locale(Arc::clone(by_the_locale))
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn corrections(_config: &Config, _library: Option<Arc<Library>>) -> Corrected {
    Corrected::uncorrected()
}

#[cfg(not(feature = "online"))]
pub fn corrections_asked_for(
    _config: &Config,
    _library: Option<Arc<Library>>,
) -> Result<Corrected> {
    Err(Error::NoReference)
}
