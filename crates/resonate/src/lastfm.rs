use std::path::Path;

#[cfg(feature = "online")]
use resonate_core::Switch;
use resonate_library::{Library, ListeningService};

use crate::{
    Result,
    config::{self, Config},
    error::ConfigKey,
    words,
};

pub fn sign_out(library: &Library, settings: &Path) -> Result<()> {
    config::clear(settings, ConfigKey::LastfmSession)?;

    if let Err(error) = library.stop_telling(ListeningService::LastFm) {
        tracing::warn!(%error, "Last.fm's mark in the history could not be let go of");
    }
    said!("{}", words::lastfm::forgotten());
    Ok(())
}

#[cfg(feature = "online")]
pub fn sign_in(config: &Config, settings: &Path) -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use resonate_online::Application;

    use crate::{Error, online, signals};

    if !config.is_on(Switch::Online) {
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

    said!("{}", words::lastfm::open_and_allow(approving.address()));
    opened_in_a_browser(approving.address());

    let cancelled = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&cancelled);
    let _interrupting = signals::cancel_when_told(move || stopping.store(true, Ordering::Relaxed));
    let Some(session) = approving.approved(&|| cancelled.load(Ordering::Relaxed))? else {
        return Err(Error::LastfmSignInEnded);
    };

    config::store(settings, ConfigKey::LastfmSession, session.key)?;
    said!(
        "{}",
        words::lastfm::SignedIn {
            name: session.name,
            settings: settings.display().to_string()
        }
        .said()
    );
    Ok(())
}

#[cfg(all(feature = "online", windows))]
const OPENS_A_PAGE: &str = "explorer";

#[cfg(all(feature = "online", target_os = "macos"))]
const OPENS_A_PAGE: &str = "open";

#[cfg(all(feature = "online", not(windows), not(target_os = "macos")))]
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
pub fn sign_in(_: &Config, _: &Path) -> Result<()> {
    Err(crate::Error::NoSignIn)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::fixtures::scratch;

    #[test]
    fn signing_out_of_lastfm_lets_go_of_its_mark_and_leaves_listenbrainzs() {
        let folder = scratch("lastfm-signed-out");
        let settings = folder.join("config.toml");

        fs::write(&settings, "lastfm-session = \"a session\"\n").expect("the settings");

        let library = Library::open_in_memory().expect("a catalog");

        for service in [ListeningService::ListenBrainz, ListeningService::LastFm] {
            library.tell_earlier_listens(service).expect("a mark");
        }

        sign_out(&library, &settings).expect("signed out");

        assert!(
            !fs::read_to_string(&settings)
                .expect("the settings")
                .contains("lastfm-session")
        );
        assert!(
            !library
                .earlier_listens_owed(ListeningService::LastFm)
                .expect("read")
        );
        assert!(
            library
                .earlier_listens_owed(ListeningService::ListenBrainz)
                .expect("read")
        );

        fs::remove_dir_all(folder).expect("cleaned");
    }
}
