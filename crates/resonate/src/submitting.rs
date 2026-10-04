use std::sync::Arc;
#[cfg(feature = "online")]
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime},
};

#[cfg(feature = "online")]
use crossbeam_channel::{RecvTimeoutError, Sender, bounded};
#[cfg(feature = "online")]
use resonate_core::{FrameSpan, Frames, MediaLocation};
#[cfg(feature = "online")]
use resonate_engine::PlaybackState;
use resonate_engine::Player;
use resonate_library::Library;
#[cfg(feature = "online")]
use resonate_library::{Error as LibraryError, LookupOp, Scrobbler};

use crate::config::Config;
#[cfg(feature = "online")]
use crate::{config, online};

#[cfg(feature = "online")]
const FIRST_AFTER: Duration = Duration::from_secs(5);
#[cfg(feature = "online")]
const SUBMITTED_EVERY: Duration = Duration::from_secs(30);
#[cfg(feature = "online")]
const PLAYING_LOOKED_AT_EVERY: Duration = Duration::from_secs(2);
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
pub(crate) fn start(config: &Config, library: &Arc<Library>, player: &Arc<Player>) -> Submitting {
    let (stop, stopped) = bounded(1);
    let config = config.clone();
    let library = Arc::clone(library);
    let player = Arc::clone(player);
    let started = thread::Builder::new()
        .name("resonate-submit".to_owned())
        .spawn(move || {
            let mut token = Token::of(&config);
            let mut listens = Pace::after(FIRST_AFTER);
            let mut loves = Pace::after(FIRST_AFTER);
            let mut refused: Option<String> = None;
            let mut told_playing: Option<Row> = None;
            let mut scrobbling: Option<(String, Arc<dyn Scrobbler>)> = None;
            loop {
                match stopped.recv_timeout(PLAYING_LOOKED_AT_EVERY) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
                let Some(held) = token.current() else {
                    continue;
                };
                if refused.as_deref() == Some(held) {
                    continue;
                }
                let held = held.to_owned();
                let scrobbler = match &scrobbling {
                    Some((kept, scrobbler)) if *kept == held => Arc::clone(scrobbler),
                    _ => {
                        let made = online::listenbrainz(&config, held.clone());
                        scrobbling = Some((held.clone(), Arc::clone(&made)));
                        made
                    }
                };

                let playing = playing_row(&player);
                if lapses(told_playing.as_ref(), playing.as_ref())
                    && let Some(row) = playing.as_ref()
                {
                    tell_what_is_playing(&library, &*scrobbler, row);
                }
                told_playing = playing;

                let outcomes = [
                    listens.settle(|| told_listens(&library, &*scrobbler)),
                    loves.settle(|| told_loves(&library, &*scrobbler)),
                ];
                if outcomes.contains(&Outcome::TokenRefused) {
                    tracing::warn!(
                        "ListenBrainz refused the token; nothing is submitted until it changes"
                    );
                    refused = Some(held);
                }
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no thread could be started to submit what is heard");
    }

    Submitting { stop }
}

#[cfg(feature = "online")]
fn told_listens(library: &Library, scrobbler: &dyn Scrobbler) -> resonate_library::Result<()> {
    let submitted = library.submit_listens(scrobbler)?;
    if submitted.submitted + submitted.refused > 0 {
        tracing::debug!(
            submitted = submitted.submitted,
            refused = submitted.refused,
            unnamed = submitted.unnamed,
            "ListenBrainz was told what was heard"
        );
    }
    Ok(())
}

#[cfg(feature = "online")]
fn told_loves(library: &Library, scrobbler: &dyn Scrobbler) -> resonate_library::Result<()> {
    let loves = library.tell_loves(scrobbler)?;
    if loves.loved + loves.taken_back + loves.refused > 0 {
        tracing::debug!(
            loved = loves.loved,
            taken_back = loves.taken_back,
            refused = loves.refused,
            "ListenBrainz was told what is a favourite"
        );
    }
    Ok(())
}

#[cfg(feature = "online")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Waiting,
    Told,
    TokenRefused,
    Failed,
}

#[cfg(feature = "online")]
struct Pace {
    due: Instant,
    failed: u32,
}

#[cfg(feature = "online")]
impl Pace {
    fn after(wait: Duration) -> Self {
        Self {
            due: Instant::now() + wait,
            failed: 0,
        }
    }

    fn settle(&mut self, tell: impl FnOnce() -> resonate_library::Result<()>) -> Outcome {
        if Instant::now() < self.due {
            return Outcome::Waiting;
        }
        self.due = Instant::now() + SUBMITTED_EVERY;
        match tell() {
            Ok(()) => {
                self.failed = 0;
                Outcome::Told
            }
            Err(LibraryError::Refused {
                op: LookupOp::Submit | LookupOp::Love,
                status,
            }) if TOKEN_REFUSED.contains(&status) => Outcome::TokenRefused,
            Err(error) => {
                self.failed += 1;
                self.due = Instant::now() + backed_off(self.failed);
                tracing::warn!(%error, "what was heard was not submitted; it is kept and tried again");
                Outcome::Failed
            }
        }
    }
}

#[cfg(feature = "online")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    location: MediaLocation,
    span: Option<FrameSpan>,
    position: Frames,
}

#[cfg(feature = "online")]
fn lapses(told: Option<&Row>, now: Option<&Row>) -> bool {
    match (told, now) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(told), Some(now)) => {
            told.location != now.location || told.span != now.span || now.position < told.position
        }
    }
}

#[cfg(feature = "online")]
fn playing_row(player: &Player) -> Option<Row> {
    let state = player.state();
    if state.playback != PlaybackState::Playing {
        return None;
    }
    let current = state.current.as_ref()?;
    let queue = player.queue();
    let item = state
        .queue_position
        .and_then(|position| queue.get(position))
        .filter(|item| item.id == current.id)?;
    Some(Row {
        location: item.location.clone(),
        span: item.span,
        position: current.position,
    })
}

#[cfg(feature = "online")]
fn tell_what_is_playing(library: &Library, scrobbler: &dyn Scrobbler, row: &Row) {
    let billed = match library.billed_as(&row.location, row.span) {
        Ok(Some(billed)) => billed,
        Ok(None) => return,
        Err(error) => {
            tracing::debug!(%error, "what is playing could not be read to tell ListenBrainz");
            return;
        }
    };
    if let Err(error) = scrobbler.playing_now(&billed) {
        tracing::debug!(%error, "ListenBrainz was not told what is playing");
    }
}

#[cfg(not(feature = "online"))]
pub(crate) fn start(config: &Config, _library: &Arc<Library>, _player: &Arc<Player>) -> Submitting {
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
    fn a_failing_pace_backs_off_alone_and_a_told_one_goes_back_to_the_usual_wait() {
        let mut loves = Pace::after(Duration::ZERO);
        let mut listens = Pace::after(Duration::ZERO);

        let failing = loves.settle(|| {
            Err(LibraryError::Refused {
                op: LookupOp::Love,
                status: 429,
            })
        });
        let fine = listens.settle(|| Ok(()));

        assert_eq!(failing, Outcome::Failed);
        assert_eq!(fine, Outcome::Told);
        assert_eq!(loves.failed, 1);
        assert_eq!(listens.failed, 0);
        assert!(loves.due > listens.due);
        assert_eq!(loves.settle(|| Ok(())), Outcome::Waiting);
    }

    #[test]
    fn a_refused_token_is_told_apart_from_a_failure() {
        let mut pace = Pace::after(Duration::ZERO);

        let outcome = pace.settle(|| {
            Err(LibraryError::Refused {
                op: LookupOp::Love,
                status: 401,
            })
        });

        assert_eq!(outcome, Outcome::TokenRefused);
        assert_eq!(pace.failed, 0);
    }

    fn row(name: &str, seconds: u64) -> Row {
        Row {
            location: MediaLocation::local(name),
            span: None,
            position: Frames(seconds * 44_100),
        }
    }

    #[test]
    fn what_is_playing_is_told_again_when_it_resumes_or_comes_round_and_not_while_it_plays() {
        let begun = row("/music/a.flac", 0);
        let later = row("/music/a.flac", 30);
        let again = row("/music/a.flac", 1);
        let other = row("/music/b.flac", 31);

        assert!(lapses(None, Some(&begun)));
        assert!(!lapses(Some(&begun), Some(&later)));
        assert!(lapses(Some(&later), Some(&again)));
        assert!(lapses(Some(&later), Some(&other)));
        assert!(!lapses(Some(&later), None));
        assert!(!lapses(None, None));
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
