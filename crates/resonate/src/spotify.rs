use std::path::Path;

use resonate_library::{Library, ProfileLink, Upkeep};

use crate::{Error, Result, config, config::Config, error::ConfigKey, followed, online, words};

pub fn sign_in(config: &Config, settings: &Path) -> Result<()> {
    let token = online::signed_in_to_spotify(config)?;
    config::store(settings, ConfigKey::SpotifyRefreshToken, token)?;
    said!(
        "{}",
        words::spotify::signed_in(settings.display().to_string())
    );
    said!("{}", words::spotify::follow_or_move());
    Ok(())
}

pub fn follow(library: &Library, config: &Config, upkeep: Upkeep) -> Result<()> {
    if !config.spotify_signed_in() {
        return Err(Error::NotSignedInToSpotify);
    }
    followed::follow_profile(library, config, &ProfileLink::SpotifyAccount, upkeep)
}

pub fn sign_out(library: &Library, settings: &Path) -> Result<()> {
    config::clear(settings, ConfigKey::SpotifyRefreshToken)?;
    let let_go = library.let_go_of_the_profile(&ProfileLink::SpotifyAccount)?;
    said!("{}", words::spotify::signed_out(let_go));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "online")]
    use std::path::PathBuf;

    #[cfg(feature = "online")]
    use resonate_core::{Switch, Switches};

    use super::*;

    #[cfg(feature = "online")]
    #[test]
    fn a_sign_in_with_online_off_or_no_client_is_refused_before_anything_is_asked() {
        let nowhere = PathBuf::from("/nowhere/resonate/config.toml");
        let offline = Config {
            switches: Switches::UNSAID.with(Switch::Online, false),
            spotify_client_id: Some("registered".to_owned()),
            ..Config::default()
        };

        assert!(matches!(sign_in(&offline, &nowhere), Err(Error::OnlineOff)));
        for blank in [None, Some("  ".to_owned())] {
            let unregistered = Config {
                spotify_client_id: blank,
                ..Config::default()
            };
            assert!(matches!(
                sign_in(&unregistered, &nowhere),
                Err(Error::NoSpotifyClient)
            ));
        }
    }

    #[test]
    fn an_account_not_signed_in_is_refused_before_anything_is_asked() {
        let unregistered = Config::default();
        let library = Library::open_in_memory().expect("a catalog");

        assert!(matches!(
            follow(&library, &unregistered, Upkeep::InStep),
            Err(Error::NotSignedInToSpotify)
        ));

        let client_alone = Config {
            spotify_client_id: Some("registered".to_owned()),
            ..Config::default()
        };
        assert!(matches!(
            follow(&library, &client_alone, Upkeep::Once),
            Err(Error::NotSignedInToSpotify)
        ));
    }
}
