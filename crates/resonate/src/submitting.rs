use std::sync::Arc;
#[cfg(feature = "online")]
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, SystemTime},
};

#[cfg(feature = "online")]
use crossbeam_channel::{RecvTimeoutError, Sender, bounded};
use resonate_library::Library;
#[cfg(feature = "online")]
use resonate_library::{Error as LibraryError, LookupOp};

use crate::config::Config;
#[cfg(feature = "online")]
use crate::{config, online};

#[cfg(feature = "online")]
const FIRST_AFTER: Duration = Duration::from_secs(5);
#[cfg(feature = "online")]
const SUBMITTED_EVERY: Duration = Duration::from_secs(30);
#[cfg(feature = "online")]
const WAITED_AT_MOST: Duration = Duration::from_secs(60 * 60);
#[cfg(feature = "online")]
const TOKEN_REFUSED: [u16; 2] = [401, 403];

pub(crate) struct Submitting {
    #[cfg(feature = "online")]
    stop: Sender<()>,
}

impl Submitting {
    pub(crate) fn leave(self) {}
}

#[cfg(feature = "online")]
impl Drop for Submitting {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
    }
}

#[cfg(feature = "online")]
pub(crate) fn start(config: &Config, library: &Arc<Library>) -> Submitting {
    let (stop, stopped) = bounded(1);
    let config = config.clone();
    let library = Arc::clone(library);
    let started = thread::Builder::new()
        .name("resonate-submit".to_owned())
        .spawn(move || {
            let mut token = Token::of(&config);
            let mut waiting = FIRST_AFTER;
            let mut refused: Option<String> = None;
            let mut failed = 0;
            loop {
                match stopped.recv_timeout(waiting) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
                waiting = SUBMITTED_EVERY;
                let Some(held) = token.current() else {
                    continue;
                };
                if refused.as_deref() == Some(held) {
                    continue;
                }
                let scrobbler = online::listenbrainz(&config, held.to_owned());
                match library.submit_listens(&*scrobbler) {
                    Ok(submitted) => {
                        failed = 0;
                        if submitted.submitted + submitted.refused > 0 {
                            tracing::debug!(
                                submitted = submitted.submitted,
                                refused = submitted.refused,
                                unnamed = submitted.unnamed,
                                "ListenBrainz was told what was heard"
                            );
                        }
                    }
                    Err(LibraryError::Refused {
                        op: LookupOp::Submit,
                        status,
                    }) if TOKEN_REFUSED.contains(&status) => {
                        tracing::warn!(
                            status,
                            "ListenBrainz refused the token; nothing is submitted until it changes"
                        );
                        refused = Some(held.to_owned());
                    }
                    Err(error) => {
                        failed += 1;
                        waiting = backed_off(failed);
                        tracing::warn!(%error, ?waiting, "what was heard was not submitted; it is kept and tried again");
                    }
                }
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no thread could be started to submit what is heard");
    }

    Submitting { stop }
}

#[cfg(not(feature = "online"))]
pub(crate) fn start(config: &Config, _library: &Arc<Library>) -> Submitting {
    if config.listenbrainz_token.is_some() {
        tracing::warn!("this build reaches no network, so nothing heard is submitted");
    }
    Submitting {}
}

#[cfg(feature = "online")]
fn backed_off(failed: u32) -> Duration {
    SUBMITTED_EVERY
        .saturating_mul(1 << failed.min(7))
        .min(WAITED_AT_MOST)
}

#[cfg(feature = "online")]
struct Token {
    path: Option<PathBuf>,
    seen: Option<SystemTime>,
    held: Option<String>,
}

#[cfg(feature = "online")]
impl Token {
    fn of(config: &Config) -> Self {
        let path = config.read_from.clone();
        Self {
            seen: path.as_deref().and_then(modified),
            path,
            held: config.submits_to().map(str::to_owned),
        }
    }

    fn current(&mut self) -> Option<&str> {
        if let Some(path) = self.path.as_deref() {
            let stamp = modified(path);
            if stamp != self.seen {
                self.seen = stamp;
                match config::submitting_in(path) {
                    Ok(token) => self.held = token,
                    Err(error) => tracing::warn!(
                        %error,
                        "the ListenBrainz token in the settings went unread, so the one held is kept"
                    ),
                }
            }
        }
        self.held.as_deref()
    }
}

#[cfg(feature = "online")]
fn modified(path: &std::path::Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|held| held.modified()).ok()
}

#[cfg(all(test, feature = "online"))]
mod tests {
    use super::*;

    #[test]
    fn a_failed_submission_waits_twice_as_long_each_time_up_to_an_hour() {
        assert_eq!(backed_off(1), Duration::from_secs(60));
        assert_eq!(backed_off(2), Duration::from_secs(120));
        assert_eq!(backed_off(6), Duration::from_secs(1_920));
        assert_eq!(backed_off(7), WAITED_AT_MOST);
        assert_eq!(backed_off(40), WAITED_AT_MOST);
    }

    #[test]
    fn a_token_written_into_the_settings_is_followed_and_online_off_holds_it_back() {
        let folder =
            std::env::temp_dir().join(format!("resonate-submitting-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("a writable temporary directory");
        let path = folder.join("config.toml");
        fs::write(&path, "").expect("a writable settings file");
        let mut token = Token {
            path: Some(path.clone()),
            seen: None,
            held: None,
        };
        assert_eq!(token.current(), None);

        fs::write(&path, "listenbrainz-token = \"a token\"\n").expect("a writable settings file");
        token.seen = None;
        assert_eq!(token.current(), Some("a token"));

        fs::write(&path, "online = false\nlistenbrainz-token = \"a token\"\n")
            .expect("a writable settings file");
        token.seen = None;
        assert_eq!(token.current(), None);

        let _ = fs::remove_dir_all(&folder);
    }
}
