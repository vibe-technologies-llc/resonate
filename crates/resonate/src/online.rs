use std::sync::Arc;
#[cfg(feature = "online")]
use std::sync::OnceLock;

use resonate_eq::Corrected;
use resonate_library::{Fingerprinters, Library, Reference};
use resonate_listen::Recognisers;
#[cfg(feature = "ui")]
use resonate_lyrics::Lyricists;
#[cfg(all(feature = "online", feature = "ui"))]
use resonate_online::Lrclib;
#[cfg(feature = "online")]
use resonate_online::{AcoustId, Audd, AutoEq, Client, Identity, Introduction, Online, Shazam};

use crate::{Error, Result, config::Config};

#[cfg(feature = "online")]
static INTRODUCTION: OnceLock<Introduction> = OnceLock::new();

#[cfg(feature = "online")]
fn identity(contact: Option<String>) -> Identity {
    Identity {
        contact,
        ..Identity::of_this_build()
    }
}

#[cfg(feature = "online")]
fn client(config: &Config) -> Arc<Client> {
    let introduction =
        INTRODUCTION.get_or_init(|| Introduction::as_(&identity(config.contact.clone())));
    Arc::new(Client::introduced(introduction.clone()))
}

#[cfg(all(feature = "online", feature = "ui"))]
pub fn introduce(contact: Option<&str>) {
    if let Some(introduction) = INTRODUCTION.get() {
        introduction.change_to(&identity(contact.map(str::to_owned)));
    }
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn introduce(_contact: Option<&str>) {}

#[cfg(feature = "online")]
pub fn reference(config: &Config) -> Option<Arc<dyn Reference>> {
    config
        .online_enabled()
        .then(|| Arc::new(Online::with_client(client(config))) as Arc<dyn Reference>)
}

#[cfg(feature = "online")]
pub fn reference_asked_for(config: &Config) -> Result<Arc<dyn Reference>> {
    reference(config).ok_or(Error::OnlineOff)
}

#[cfg(feature = "online")]
pub fn fingerprinters(config: &Config) -> Fingerprinters {
    let local = Fingerprinters::none();
    if !config.online_enabled() {
        return local;
    }
    match config.acoustid_key.clone() {
        Some(key) => local.and(Arc::new(AcoustId::new(client(config), key))),
        None => local,
    }
}

#[cfg(not(feature = "online"))]
pub fn fingerprinters(_config: &Config) -> Fingerprinters {
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
        recognisers = recognisers.and(Arc::new(Audd::new(Arc::clone(&client), token)));
    }
    if let Some(key) = config.acoustid_key.clone() {
        recognisers = recognisers.and(Arc::new(AcoustId::new(client, key)));
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
pub fn lyricists(config: &Config, library: Option<Arc<Library>>) -> Lyricists {
    let local = Lyricists::local();
    if !config.online_enabled() {
        return local;
    }
    local.and(Arc::new(Lrclib::new(client(config), library)))
}

#[cfg(not(feature = "online"))]
pub fn reference(_config: &Config) -> Option<Arc<dyn Reference>> {
    None
}

#[cfg(not(feature = "online"))]
pub fn reference_asked_for(_config: &Config) -> Result<Arc<dyn Reference>> {
    Err(Error::NoReference)
}

#[cfg(all(not(feature = "online"), feature = "ui"))]
pub fn lyricists(_config: &Config, _library: Option<Arc<Library>>) -> Lyricists {
    Lyricists::local()
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
