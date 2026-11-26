use std::sync::Arc;
#[cfg(feature = "online")]
use std::{
    fs, io,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

#[cfg(feature = "online")]
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
#[cfg(feature = "online")]
use resonate_core::{FrameSpan, Frames, MediaLocation};
#[cfg(feature = "online")]
use resonate_engine::PlaybackState;
use resonate_engine::Player;
use resonate_library::Library;
#[cfg(feature = "online")]
use resonate_library::{Error as LibraryError, ListeningService, LookupOp, Scrobbler};

use crate::config::Config;
#[cfg(feature = "online")]
use crate::{
    config::{self, Accounts, LastfmAccount},
    online,
};

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
    _stop: Sender<()>,
}

impl Submitting {
    pub(crate) fn leave(self) {}
}

#[cfg(feature = "online")]
const LANES: [ListeningService; 2] = [ListeningService::ListenBrainz, ListeningService::LastFm];

#[cfg(feature = "online")]
#[derive(Clone, PartialEq, Eq)]
enum Account {
    ListenBrainz(String),
    LastFm(LastfmAccount),
}

#[cfg(feature = "online")]
impl Account {
    fn of(service: ListeningService, accounts: &Accounts) -> Option<Self> {
        match service {
            ListeningService::ListenBrainz => accounts.listenbrainz.clone().map(Self::ListenBrainz),
            ListeningService::LastFm => accounts.lastfm.clone().map(Self::LastFm),
        }
    }

    fn scrobbler(&self, config: &Config) -> Arc<dyn Scrobbler> {
        match self {
            Self::ListenBrainz(token) => online::listenbrainz(config, token.clone()),
            Self::LastFm(account) => online::lastfm(config, account.clone()),
        }
    }
}

#[cfg(feature = "online")]
#[derive(Clone, Copy)]
struct Paced {
    first_after: Duration,
    looked_at_every: Duration,
}

#[cfg(feature = "online")]
impl Paced {
    const USUAL: Self = Self {
        first_after: FIRST_AFTER,
        looked_at_every: PLAYING_LOOKED_AT_EVERY,
    };
}

#[cfg(feature = "online")]
struct Telling {
    listens: Pace,
    loves: Pace,
    playing: Pace,
    refused: Option<Account>,
    told_playing: Option<Row>,
    scrobbling: Option<(Account, Arc<dyn Scrobbler>)>,
}

#[cfg(feature = "online")]
impl Telling {
    fn new(first_after: Duration) -> Self {
        Self {
            listens: Pace::after(first_after),
            loves: Pace::after(first_after),
            playing: Pace::every(Duration::ZERO),
            refused: None,
            told_playing: None,
            scrobbling: None,
        }
    }

    fn tell(
        &mut self,
        account: Account,
        scrobbler_of: &dyn Fn(&Account) -> Arc<dyn Scrobbler>,
        library: &Library,
        playing: &dyn Fn() -> Option<Row>,
    ) {
        if self.refused.as_ref() == Some(&account) {
            return;
        }

        let scrobbler = match &self.scrobbling {
            Some((kept, scrobbler)) if *kept == account => Arc::clone(scrobbler),
            _ => {
                let made = scrobbler_of(&account);

                self.scrobbling = Some((account.clone(), Arc::clone(&made)));
                made
            }
        };

        let playing = playing();

        if lapses(self.told_playing.as_ref(), playing.as_ref())
            && let Some(row) = playing.as_ref()
        {
            self.playing
                .settle(|| tell_what_is_playing(library, &*scrobbler, row));
        }
        self.told_playing = playing;

        let outcomes = [
            self.listens.settle(|| told_listens(library, &*scrobbler)),
            self.loves.settle(|| told_loves(library, &*scrobbler)),
        ];
        if outcomes.contains(&Outcome::TokenRefused) {
            tracing::warn!(
                service = scrobbler.service().name(),
                "the service refused what it was signed in with; nothing is submitted until it changes"
            );
            self.refused = Some(account);
        }
    }
}

#[cfg(feature = "online")]
pub(crate) fn start(config: &Config, library: &Arc<Library>, player: &Arc<Player>) -> Submitting {
    let (stop, stopped) = bounded(0);

    for service in LANES {
        let config = config.clone();
        let player = Arc::clone(player);
        let mut token = Token::of(&config);

        let started = lane(
            stopped.clone(),
            Arc::clone(library),
            Paced::USUAL,
            Box::new(move || Account::of(service, token.current())),
            Box::new(move |account| account.scrobbler(&config)),
            Box::new(move || playing_row(&player)),
        );
        if let Err(error) = started {
            tracing::warn!(
                %error,
                service = service.name(),
                "no thread could be started to submit what is heard"
            );
        }
    }

    Submitting { _stop: stop }
}

#[cfg(feature = "online")]
type Following = Box<dyn FnMut() -> Option<Account> + Send>;

#[cfg(feature = "online")]
type Making = Box<dyn Fn(&Account) -> Arc<dyn Scrobbler> + Send>;

#[cfg(feature = "online")]
type Looking = Box<dyn Fn() -> Option<Row> + Send>;

#[cfg(feature = "online")]
fn lane(
    stopped: Receiver<()>,
    library: Arc<Library>,
    paced: Paced,
    mut account: Following,
    scrobbler_of: Making,
    playing: Looking,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("resonate-submit".to_owned())
        .spawn(move || {
            let mut telling = Telling::new(paced.first_after);
            loop {
                match stopped.recv_timeout(paced.looked_at_every) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }

                if let Some(account) = account() {
                    telling.tell(account, &*scrobbler_of, &library, &*playing);
                }
            }
        })
}

#[cfg(feature = "online")]
fn told_listens(library: &Library, scrobbler: &dyn Scrobbler) -> resonate_library::Result<()> {
    let submitted = library.submit_listens(scrobbler)?;

    if submitted.submitted + submitted.refused > 0 {
        tracing::debug!(
            submitted = submitted.submitted,
            refused = submitted.refused,
            unnamed = submitted.unnamed,
            service = scrobbler.service().name(),
            "the service was told what was heard"
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
            service = scrobbler.service().name(),
            "the service was told what is a favourite"
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
    every: Duration,
    failed: u32,
}

#[cfg(feature = "online")]
impl Pace {
    fn after(wait: Duration) -> Self {
        Self {
            due: Instant::now() + wait,
            every: SUBMITTED_EVERY,
            failed: 0,
        }
    }

    fn every(every: Duration) -> Self {
        Self {
            due: Instant::now(),
            every,
            failed: 0,
        }
    }

    fn settle(&mut self, tell: impl FnOnce() -> resonate_library::Result<()>) -> Outcome {
        if Instant::now() < self.due {
            return Outcome::Waiting;
        }
        self.due = Instant::now() + self.every;
        match tell() {
            Ok(()) => {
                self.failed = 0;
                Outcome::Told
            }
            Err(LibraryError::Refused {
                op: LookupOp::Submit | LookupOp::Love,
                status,
            }) if TOKEN_REFUSED.contains(&status) => Outcome::TokenRefused,
            Err(LibraryError::AlreadySubmitting { service }) => {
                tracing::debug!(
                    service = service.name(),
                    "another process is telling the service what was heard, so this one waits its turn"
                );
                Outcome::Waiting
            }
            Err(error) => {
                self.failed += 1;
                self.due = Instant::now() + backed_off(self.failed);
                tracing::warn!(%error, "the service was not told; what it is owed is kept and tried again");
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
fn tell_what_is_playing(
    library: &Library,
    scrobbler: &dyn Scrobbler,
    row: &Row,
) -> resonate_library::Result<()> {
    match library.billed_as(&row.location, row.span)? {
        Some(billed) => scrobbler.playing_now(&billed),
        None => Ok(()),
    }
}

#[cfg(not(feature = "online"))]
pub(crate) fn start(config: &Config, _library: &Arc<Library>, _player: &Arc<Player>) -> Submitting {
    if config.listenbrainz_token.is_some() || config.lastfm_session.is_some() {
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
    held: Accounts,
}

#[cfg(feature = "online")]
impl Token {
    fn of(config: &Config) -> Self {
        let path = config.read_from.clone();
        Self {
            seen: path.as_deref().and_then(modified),
            path,
            held: config.submits_to(),
        }
    }

    fn current(&mut self) -> &Accounts {
        if let Some(path) = self.path.as_deref() {
            let stamp = modified(path);
            if stamp != self.seen {
                self.seen = stamp;
                match config::submitting_in(path) {
                    Ok(accounts) => self.held = accounts,
                    Err(error) => tracing::warn!(
                        %error,
                        "the accounts in the settings went unread, so the ones held are kept"
                    ),
                }
            }
        }
        &self.held
    }
}

#[cfg(feature = "online")]
fn modified(path: &std::path::Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|held| held.modified()).ok()
}

#[cfg(all(test, feature = "online"))]
mod tests {
    use std::sync::Barrier;

    use parking_lot::Mutex;
    use resonate_library::{Billed, Love, Loved, Scrobble, TokenHeld};

    use super::*;
    use crate::fixtures::{ARTIST, TITLE, scanned, scratch, tagged_wave};

    const PROMPTLY: Paced = Paced {
        first_after: Duration::ZERO,
        looked_at_every: Duration::from_millis(10),
    };
    const WAITED_FOR_AT_MOST: Duration = Duration::from_secs(30);

    struct Heard {
        service: ListeningService,
        told: Mutex<Vec<String>>,
        stalled: Option<Barrier>,
    }

    impl Heard {
        fn by(service: ListeningService, stalled: Option<Barrier>) -> Arc<Self> {
            Arc::new(Self {
                service,
                told: Mutex::new(Vec::new()),
                stalled,
            })
        }
    }

    impl Scrobbler for Heard {
        fn service(&self) -> ListeningService {
            self.service
        }

        fn submit(&self, listens: &[Scrobble]) -> resonate_library::Result<()> {
            if let Some(stalled) = &self.stalled {
                stalled.wait();
            }
            self.told
                .lock()
                .extend(listens.iter().map(|listen| listen.billed.title.clone()));
            Ok(())
        }

        fn playing_now(&self, _: &Billed) -> resonate_library::Result<()> {
            Ok(())
        }

        fn love(&self, _: &Loved, _: Love) -> resonate_library::Result<()> {
            Ok(())
        }

        fn token_held(&self) -> resonate_library::Result<TokenHeld> {
            Ok(TokenHeld::Unknown)
        }
    }

    fn lane_to(
        heard: &Arc<Heard>,
        account: Account,
        library: &Arc<Library>,
        stopped: &Receiver<()>,
    ) -> JoinHandle<()> {
        let made = Arc::clone(heard) as Arc<dyn Scrobbler>;

        lane(
            stopped.clone(),
            Arc::clone(library),
            PROMPTLY,
            Box::new(move || Some(account.clone())),
            Box::new(move |_| Arc::clone(&made)),
            Box::new(|| None),
        )
        .expect("a lane")
    }

    #[test]
    fn a_stalled_lastfm_leaves_listenbrainz_told_what_was_heard() {
        let root = scratch("submitting-lanes");

        for (title, artist) in [("Words", "Low"), ("Echoes", "Pink Floyd")] {
            fs::write(
                root.join(format!("{title}.wav")),
                tagged_wave(&[(TITLE, title), (ARTIST, artist)]),
            )
            .expect("a song");
        }

        let library = Arc::new(scanned(&root));
        let listenbrainz = Heard::by(ListeningService::ListenBrainz, None);
        let lastfm = Heard::by(ListeningService::LastFm, Some(Barrier::new(2)));

        for heard in [&listenbrainz, &lastfm] {
            assert!(library.submit_listens(&**heard).expect("marked").started);
        }
        for title in ["Words", "Echoes"] {
            library
                .track_played(
                    &MediaLocation::local(root.join(format!("{title}.wav"))),
                    None,
                    Duration::ZERO,
                )
                .expect("a listen");
        }

        let (stop, stopped) = bounded(0);

        let stalled = lane_to(
            &lastfm,
            Account::LastFm(LastfmAccount {
                key: "key".to_owned(),
                secret: "secret".to_owned(),
                session: "session".to_owned(),
            }),
            &library,
            &stopped,
        );
        let telling = lane_to(
            &listenbrainz,
            Account::ListenBrainz("token".to_owned()),
            &library,
            &stopped,
        );

        let waited_from = Instant::now();

        while listenbrainz.told.lock().len() < 2 && waited_from.elapsed() < WAITED_FOR_AT_MOST {
            thread::sleep(PROMPTLY.looked_at_every);
        }

        let told_while_lastfm_stalled = listenbrainz.told.lock().clone();
        let lastfm_told_while_stalled = lastfm.told.lock().len();

        if let Some(stalled) = &lastfm.stalled {
            stalled.wait();
        }
        drop(stop);
        stalled.join().expect("the Last.fm lane");
        telling.join().expect("the ListenBrainz lane");

        assert_eq!(told_while_lastfm_stalled, ["Words", "Echoes"]);
        assert_eq!(lastfm_told_while_stalled, 0);
        assert_eq!(*lastfm.told.lock(), ["Words", "Echoes"]);

        fs::remove_dir_all(root).expect("cleaned");
    }

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
    fn a_round_another_process_is_submitting_waits_its_turn_without_backing_off() {
        let mut pace = Pace::after(Duration::ZERO);

        let outcome = pace.settle(|| {
            Err(LibraryError::AlreadySubmitting {
                service: ListeningService::ListenBrainz,
            })
        });

        assert_eq!(outcome, Outcome::Waiting);
        assert_eq!(pace.failed, 0);
        assert!(pace.due <= Instant::now() + SUBMITTED_EVERY);
    }

    #[test]
    fn what_is_playing_is_told_at_once_until_the_service_fails_and_then_backs_off() {
        let mut playing = Pace::every(Duration::ZERO);

        let told = playing.settle(|| Ok(()));
        let again = playing.settle(|| Ok(()));
        let failed = playing.settle(|| {
            Err(LibraryError::Refused {
                op: LookupOp::Submit,
                status: 503,
            })
        });
        let held_back = playing.settle(|| Ok(()));

        assert_eq!(
            [told, again, failed],
            [Outcome::Told, Outcome::Told, Outcome::Failed]
        );
        assert_eq!(held_back, Outcome::Waiting);
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
            held: Accounts::default(),
        };

        assert_eq!(token.current(), &Accounts::default());

        fs::write(&path, "listenbrainz-token = \"a token\"\n").expect("a writable settings file");
        token.seen = None;

        assert_eq!(token.current().listenbrainz.as_deref(), Some("a token"));

        fs::write(&path, "online = false\nlistenbrainz-token = \"a token\"\n")
            .expect("a writable settings file");
        token.seen = None;

        assert_eq!(token.current(), &Accounts::default());

        let _ = fs::remove_dir_all(&folder);
    }
}
