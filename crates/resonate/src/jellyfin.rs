use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use resonate_core::Switch;
use resonate_jellyfin::{Jellyfin, KeptSession, QuickConnect, Server, SignedIn};

use crate::{
    Error, Result,
    config::{self, Config},
    error::ConfigKey,
    signals, words,
};

const KEPT: [ConfigKey; 3] = [
    ConfigKey::JellyfinToken,
    ConfigKey::JellyfinUserId,
    ConfigKey::JellyfinDevice,
];

pub fn sign_in(config: &Config, settings: &Path, forgetting: bool) -> Result<()> {
    if forgetting {
        log_out(config);
        forget(settings)?;
        said!("{}", words::jellyfin::forgotten());
        return Ok(());
    }
    if !config.is_on(Switch::Online) {
        return Err(Error::OnlineOff);
    }

    let server = config
        .jellyfin
        .as_deref()
        .map(str::trim)
        .filter(|server| !server.is_empty())
        .ok_or(Error::NoJellyfinServer)?;
    let asking = QuickConnect::at(server);

    if !asking.offered()? {
        return Err(Error::QuickConnectOff);
    }

    let approving = asking.begun()?;

    said!("{}", words::jellyfin::approve(approving.code.as_str()));

    let cancelled = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&cancelled);
    let _interrupting = signals::cancel_when_told(move || stopping.store(true, Ordering::Relaxed));
    let Some(signed) = asking.approved(&approving, &|| cancelled.load(Ordering::Relaxed))? else {
        return Err(Error::JellyfinSignInCancelled);
    };

    keep(settings, &signed)?;
    said!(
        "{}",
        words::jellyfin::signed_in(signed.name.as_str(), settings.display().to_string())
    );
    Ok(())
}

fn kept_session(config: &Config) -> Option<Server> {
    let given = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    Some(Server {
        url: given(&config.jellyfin)?,
        user: String::new(),
        password: String::new(),
        kept: Some(KeptSession {
            token: given(&config.jellyfin_token)?,
            user: given(&config.jellyfin_user_id)?,
            device: given(&config.jellyfin_device)?,
        }),
    })
}

fn log_out(config: &Config) {
    if !config.is_on(Switch::Online) {
        return;
    }

    let Some(server) = kept_session(config) else {
        return;
    };

    if let Err(error) = Jellyfin::at(server).log_out() {
        tracing::debug!(%error, "the Jellyfin server did not log the session out");
    }
}

pub fn keep(settings: &Path, signed: &SignedIn) -> Result<()> {
    config::edit(settings, |editing| {
        editing.store(ConfigKey::JellyfinToken, signed.session.token.as_str());
        editing.store(ConfigKey::JellyfinUserId, signed.session.user.as_str());
        editing.store(ConfigKey::JellyfinDevice, signed.session.device.as_str());
        if !signed.name.trim().is_empty() {
            editing.store(ConfigKey::JellyfinUser, signed.name.as_str());
        }
        editing.clear(ConfigKey::JellyfinPassword);
        Ok(())
    })
}

pub fn forget(settings: &Path) -> Result<()> {
    config::edit(settings, |editing| {
        for key in KEPT {
            editing.clear(key);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        process, thread,
    };

    use resonate_core::{Switch, Switches};
    use resonate_jellyfin::KeptSession;

    use super::*;

    fn a_settings_file(name: &str, text: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-jellyfin-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&folder);

        fs::create_dir_all(&folder).expect("a folder");

        let settings = folder.join("config.toml");

        fs::write(&settings, text).expect("the settings");
        settings
    }

    fn signed() -> SignedIn {
        SignedIn {
            session: KeptSession {
                token: "token-1".to_owned(),
                user: "0a1b".to_owned(),
                device: "device-1".to_owned(),
            },
            name: "listener".to_owned(),
        }
    }

    #[test]
    fn a_quick_connect_sign_in_keeps_the_session_and_lets_go_of_the_password() {
        let settings = a_settings_file(
            "kept",
            "jellyfin = \"http://music.local:8096\"\njellyfin-user = \"old\"\njellyfin-password = \"sesame\"\n",
        );

        keep(&settings, &signed()).expect("kept");

        let written = fs::read_to_string(&settings).expect("the settings");

        assert!(
            written.contains("jellyfin-token = \"token-1\""),
            "{written}"
        );
        assert!(written.contains("jellyfin-user-id = \"0a1b\""), "{written}");
        assert!(
            written.contains("jellyfin-device = \"device-1\""),
            "{written}"
        );
        assert!(
            written.contains("jellyfin-user = \"listener\""),
            "{written}"
        );
        assert!(!written.contains("sesame"), "{written}");
        assert!(
            written.contains("jellyfin = \"http://music.local:8096\""),
            "{written}"
        );

        forget(&settings).expect("forgotten");

        let forgotten = fs::read_to_string(&settings).expect("the settings");

        assert!(
            !forgotten.contains("token-1") && !forgotten.contains("device-1"),
            "{forgotten}"
        );
    }

    #[test]
    fn a_jellyfin_sign_in_with_online_off_or_no_server_is_refused_before_anything_is_asked() {
        let settings = a_settings_file("refused", "");
        let offline = Config {
            switches: Switches::UNSAID.with(Switch::Online, false),
            jellyfin: Some("http://127.0.0.1:9".to_owned()),
            ..Config::default()
        };

        assert!(matches!(
            sign_in(&offline, &settings, false),
            Err(Error::OnlineOff)
        ));
        assert!(matches!(
            sign_in(&Config::default(), &settings, false),
            Err(Error::NoJellyfinServer)
        ));
    }

    fn a_kept_config(server: &str) -> Config {
        Config {
            switches: Switches::UNSAID,
            jellyfin: Some(server.to_owned()),
            jellyfin_token: Some("token-1".to_owned()),
            jellyfin_user_id: Some("0a1b".to_owned()),
            jellyfin_device: Some("device-1".to_owned()),
            ..Config::default()
        }
    }

    const KEPT_TEXT: &str = "jellyfin-token = \"token-1\"\njellyfin-user-id = \"0a1b\"\njellyfin-device = \"device-1\"\n";

    #[test]
    fn forgetting_the_session_logs_it_out_with_its_token_and_forgets_it() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let server = format!("http://{}", listener.local_addr().expect("an address"));
        let serving = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a request");
            let mut heard = [0; 4096];

            let read = stream.read(&mut heard).expect("a request read");

            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .expect("an answer");
            String::from_utf8_lossy(&heard[..read]).into_owned()
        });
        let settings = a_settings_file("logout", KEPT_TEXT);

        sign_in(&a_kept_config(&server), &settings, true).expect("forgotten");

        let heard = serving.join().expect("the request");

        assert!(heard.starts_with("POST /Sessions/Logout "), "{heard}");
        assert!(heard.contains("Token=\"token-1\""), "{heard}");
        assert!(!heard.contains("api_key"), "{heard}");

        let written = fs::read_to_string(&settings).expect("the settings");

        assert!(!written.contains("token-1"), "{written}");
    }

    #[test]
    fn forgetting_goes_on_when_the_server_cannot_log_the_session_out() {
        let settings = a_settings_file("unreachable", KEPT_TEXT);

        sign_in(&a_kept_config("http://127.0.0.1:9"), &settings, true).expect("forgotten");

        let written = fs::read_to_string(&settings).expect("the settings");

        assert!(!written.contains("token-1"), "{written}");
    }
}
