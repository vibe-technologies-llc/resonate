use std::path::Path;

use crate::{Result, config::Config};

#[cfg(feature = "online")]
pub fn sign_in(config: &Config, settings: &Path, user: Option<&str>, forget: bool) -> Result<()> {
    use resonate_online::Application;

    use crate::{Error, config, error::ConfigKey, input, online, signals, words};

    if forget {
        config::clear(settings, ConfigKey::LastfmSession)?;
        said!("{}", words::lastfm::forgotten());
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
    let Some(user) = user.map(str::trim).filter(|user| !user.is_empty()) else {
        return Err(Error::NoLastfmUser);
    };

    told!("{}", words::lastfm::type_the_password(user));
    let leaving = signals::leave_when_told();
    let password = input::a_line_unechoed().map_err(|source| Error::ReadPassword { source })?;
    drop(leaving);
    let session = match online::lastfm_signed_in(config, &application, user, &password) {
        Ok(session) => session,
        Err(resonate_library::Error::Refused { .. }) => return Err(Error::LastfmRefused),
        Err(error) => return Err(error.into()),
    };
    config::store(settings, ConfigKey::LastfmSession, session.key)?;
    said!(
        "{}",
        words::lastfm::signed_in(session.name, settings.display().to_string())
    );
    Ok(())
}

#[cfg(not(feature = "online"))]
pub fn sign_in(_: &Config, _: &Path, _: Option<&str>, _: bool) -> Result<()> {
    Err(crate::Error::NoSignIn)
}
