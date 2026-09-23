use std::{path::Path, sync::Arc};

use resonate_inbox::Inbox;
use resonate_providers::Providers;

use crate::config::Config;

pub fn registered(config: &Config) -> Providers {
    with_inbox(config.inbox.as_deref())
}

pub fn with_inbox(inbox: Option<&Path>) -> Providers {
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
}
