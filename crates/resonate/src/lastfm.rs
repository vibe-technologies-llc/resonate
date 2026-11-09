use std::path::Path;

use crate::{Result, config::Config};

#[cfg(feature = "online")]
pub fn sign_in(config: &Config, settings: &Path, forget: bool) -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use resonate_online::Application;

    use crate::{Error, config, error::ConfigKey, online, signals};

    if forget {
        config::clear(settings, ConfigKey::LastfmSession)?;
        said!("nothing more is scrobbled to Last.fm");
        return Ok(());
    }
    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    let given = |held: &Option<String>| {
        held.as_deref()
            .map(str::trim)
            .filter(|held| !held.is_empty())
            .map(str::to_owned)
    };
    let application = Application {
        key: given(&config.lastfm_key).ok_or(Error::NoLastfmApplication)?,
        secret: given(&config.lastfm_secret).ok_or(Error::NoLastfmApplication)?,
    };

    let approving = match online::lastfm_approval(config, application) {
        Ok(approving) => approving,
        Err(resonate_library::Error::Refused { .. }) => return Err(Error::LastfmRefused),
        Err(error) => return Err(error.into()),
    };
    said!(
        "open {} and allow Resonate there; this waits until Last.fm says so",
        approving.address()
    );
    opened_in_a_browser(approving.address());

    let cancelled = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&cancelled);
    let _interrupting = signals::cancel_when_told(move || stopping.store(true, Ordering::Relaxed));
    let Some(session) = approving.approved(&|| cancelled.load(Ordering::Relaxed))? else {
        return Err(Error::LastfmSignInEnded);
    };
    config::store(settings, ConfigKey::LastfmSession, session.key)?;
    said!(
        "signed in to Last.fm as {}; the session key is kept in {}",
        session.name,
        settings.display()
    );
    Ok(())
}

#[cfg(all(feature = "online", windows))]
const OPENS_A_PAGE: &str = "explorer";

#[cfg(all(feature = "online", not(windows)))]
const OPENS_A_PAGE: &str = "xdg-open";

#[cfg(feature = "online")]
fn opened_in_a_browser(address: &str) {
    use std::process::{Command, Stdio};

    let opened = Command::new(OPENS_A_PAGE)
        .arg(address)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(error) = opened {
        tracing::debug!(%error, "no browser could be opened on the Last.fm page");
    }
}

#[cfg(not(feature = "online"))]
pub fn sign_in(_: &Config, _: &Path, _: bool) -> Result<()> {
    Err(crate::Error::NoSignIn)
}
