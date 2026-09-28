use std::{path::Path, sync::Arc};

use resonate_inbox::Inbox;
use resonate_providers::Providers;
#[cfg(feature = "online")]
use resonate_subsonic::{Server, Subsonic};

use crate::config::Config;

pub type Registering = Arc<dyn Fn(Option<&Path>) -> Providers + Send + Sync>;

pub fn registered(config: &Config) -> Providers {
    sourced(config)(config.inbox.as_deref())
}

#[cfg(feature = "online")]
pub fn sourced(config: &Config) -> Registering {
    let server = subsonic(config);
    Arc::new(move |inbox| {
        let providers = with_inbox(inbox);
        match server.clone() {
            Some(server) => providers.and(Arc::new(Subsonic::at(server))),
            None => providers,
        }
    })
}

#[cfg(not(feature = "online"))]
pub fn sourced(_config: &Config) -> Registering {
    Arc::new(with_inbox)
}

#[cfg(feature = "online")]
fn subsonic(config: &Config) -> Option<Server> {
    if !config.online_enabled() {
        return None;
    }
    Some(Server {
        url: config.subsonic.clone()?,
        user: config.subsonic_user.clone()?,
        password: config.subsonic_password.clone()?,
    })
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

    #[test]
    fn nothing_is_registered_until_an_inbox_is_named() {
        assert!(!registered(&Config::default()).has_a_source());

        let config = Config {
            inbox: Some(PathBuf::from("/music/inbox")),
            ..Config::default()
        };
        let providers = registered(&config);
        assert!(providers.has_a_source());
        assert!(
            providers
                .names()
                .iter()
                .any(|name| name.as_str() == "inbox")
        );
    }

    #[cfg(feature = "online")]
    #[test]
    fn a_subsonic_server_is_registered_only_with_its_account_and_while_online() {
        let named = |config: &Config| -> Vec<String> {
            registered(config)
                .names()
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect()
        };
        let halfway = Config {
            subsonic: Some("http://music.local:4533".to_owned()),
            subsonic_user: Some("listener".to_owned()),
            ..Config::default()
        };
        assert!(!named(&halfway).contains(&"subsonic".to_owned()));

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
}
