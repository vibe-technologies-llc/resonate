use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use parking_lot::RwLock;
use resonate_core::{AppId, FrameSpan, Mbid, MediaLocation, Pictured, Presence, Shown, TrackId};
use resonate_engine::{PlaybackState, Player, StreamDigest, TagSet};

use crate::{
    Error, Releases,
    activity::{Activity, Cover, Playing},
    socket::Session,
};

const TICK: Duration = Duration::from_secs(1);
const SENDS_APART: Duration = Duration::from_secs(4);
const DRIFT_ALLOWED: Duration = Duration::from_secs(2);
const RETRY_AFTER_AT_FIRST: Duration = Duration::from_secs(5);
const RETRY_AFTER_AT_MOST: Duration = Duration::from_secs(120);
const SENT_AGAIN_AFTER_A_REFUSAL: Duration = Duration::from_secs(15);
const SENT_AGAIN_AT_MOST: Duration = Duration::from_secs(10 * 60);
const COVER_REFRESH_AFTER: Duration = Duration::from_secs(15);
const FAREWELL: Duration = Duration::from_millis(500);

pub(crate) struct Running {
    nudges: Sender<()>,
    done: Receiver<()>,
}

impl Running {
    pub(crate) fn start(
        player: Arc<Player>,
        releases: Arc<dyn Releases>,
        presence: Arc<RwLock<Presence>>,
    ) -> Option<Self> {
        let (nudges, nudged) = bounded(1);
        let (finished, done) = bounded::<()>(0);
        let publisher = Publisher {
            player,
            releases,
            presence,
            showing: Showing::finding_with(Box::new(Session::find)),
            covered: None,
        };
        let spawned = thread::Builder::new()
            .name("resonate-discord".to_owned())
            .spawn(move || {
                publisher.run(&nudged);
                drop(finished);
            });

        match spawned {
            Ok(_) => Some(Self { nudges, done }),
            Err(error) => {
                tracing::warn!(%error, "what is playing cannot be shown on Discord");
                None
            }
        }
    }

    pub(crate) fn nudge(&self) {
        if self.nudges.try_send(()).is_err() {
            tracing::trace!("a change of presence arrived while one was already waiting");
        }
    }

    pub(crate) fn stop(self) {
        drop(self.nudges);
        if let Err(RecvTimeoutError::Timeout) = self.done.recv_timeout(FAREWELL) {
            tracing::debug!(
                "Discord did not see the presence cleared in time; leaving it to close"
            );
        }
    }
}

struct Sent {
    activity: Option<Activity>,
    at: Instant,
    refusals: u32,
}

struct CachedCover {
    track: TrackId,
    location: MediaLocation,
    span: Option<FrameSpan>,
    cover: Option<Cover>,
    checked: Instant,
}

enum Link {
    Closed {
        retry_at: Option<Instant>,
    },
    Open {
        session: Box<Session>,
        app: AppId,
        sent: Option<Sent>,
    },
    Unknown {
        app: AppId,
    },
}

type Find = Box<dyn Fn(AppId, Option<&Path>) -> Result<(Session, PathBuf), Error> + Send>;

struct Publisher {
    player: Arc<Player>,
    releases: Arc<dyn Releases>,
    presence: Arc<RwLock<Presence>>,
    showing: Showing,
    covered: Option<CachedCover>,
}

struct Showing {
    find: Find,
    link: Link,
    answered_last: Option<PathBuf>,
    waits: Duration,
}

impl Publisher {
    fn run(mut self, nudged: &Receiver<()>) {
        loop {
            self.tick();
            match nudged.recv_timeout(TICK) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.showing.farewell();
    }

    fn tick(&mut self) {
        let presence = self.presence.read().clone();
        let Some(app) = presence.app.filter(|_| presence.active()) else {
            self.showing.farewell();
            return;
        };
        let wanted = self
            .playing(&presence)
            .and_then(|playing| Activity::of(&presence, &playing, SystemTime::now()));
        self.showing.show(app, wanted, Instant::now());
    }

    fn playing(&mut self, presence: &Presence) -> Option<Playing> {
        let state = self.player.state();
        let current = state.current?;
        let paused = match state.playback {
            PlaybackState::Playing | PlaybackState::Buffering => false,
            PlaybackState::Paused => true,
            PlaybackState::Idle | PlaybackState::Stopped => return None,
        };
        let digest = self
            .player
            .digest()
            .filter(|digest| digest.track == current.id)?;
        let tags = &digest.info.tags;
        let pictures_a_cover =
            presence.pictured == Pictured::Cover && presence.shown != Shown::Application;
        let cover = if pictures_a_cover {
            self.cover(current.id, &digest)
        } else {
            None
        };
        let rate = current.source.rate;

        Some(Playing {
            title: named(tags.title.as_deref())
                .or_else(|| digest.location.stem().map(|stem| stem.into_owned()))
                .unwrap_or_default(),
            artist: named(tags.artist.as_deref()),
            album: named(tags.album.as_deref()),
            cover,
            position: current.position.to_duration(rate),
            duration: current.duration.map(|duration| duration.to_duration(rate)),
            paused,
        })
    }

    fn cover(&mut self, track: TrackId, digest: &StreamDigest) -> Option<Cover> {
        cover_for(
            self.releases.as_ref(),
            &mut self.covered,
            track,
            &digest.location,
            digest.span,
            &digest.info.tags,
            Instant::now(),
        )
    }
}

impl Showing {
    fn finding_with(find: Find) -> Self {
        Self {
            find,
            link: Link::Closed { retry_at: None },
            answered_last: None,
            waits: RETRY_AFTER_AT_FIRST,
        }
    }

    fn show(&mut self, app: AppId, wanted: Option<Activity>, now: Instant) {
        match self.link {
            Link::Open { app: open, .. } if open != app => self.farewell(),
            Link::Unknown { app: refused } if refused != app => {
                self.link = Link::Closed { retry_at: None };
            }
            Link::Unknown { .. } => return,
            Link::Open { .. } | Link::Closed { .. } => {}
        }

        if let Link::Closed { retry_at } = self.link {
            if wanted.is_none() || retry_at.is_some_and(|at| now < at) {
                return;
            }
            match (self.find)(app, self.answered_last.as_deref()) {
                Ok((session, path)) => {
                    tracing::info!("showing what is playing on Discord");
                    self.answered_last = Some(path);
                    self.waits = RETRY_AFTER_AT_FIRST;
                    self.link = Link::Open {
                        session: Box::new(session),
                        app,
                        sent: None,
                    };
                }
                Err(error) if error.names_no_application() => {
                    tracing::warn!(%error, %app, "Discord knows no application by that id, so nothing is shown until it changes");
                    self.link = Link::Unknown { app };
                    return;
                }
                Err(error) => {
                    tracing::debug!(%error, waits = ?self.waits, "Discord could not be reached; asking again later");
                    self.link = Link::Closed {
                        retry_at: Some(now + self.waits),
                    };
                    self.waits = self.waits.saturating_mul(2).min(RETRY_AFTER_AT_MOST);
                    return;
                }
            }
        }

        let Link::Open { session, sent, .. } = &mut self.link else {
            return;
        };
        let outcome = session.drain().and_then(|()| {
            if !due(sent.as_ref(), wanted.as_ref(), now) {
                return Ok(());
            }
            let set = session.set(wanted.as_ref());
            let refusals = refusals_carried(sent.as_ref(), wanted.as_ref());
            *sent = Some(Sent {
                activity: wanted,
                at: now,
                refusals,
            });
            set
        });
        match outcome {
            Ok(()) => {}
            Err(Error::Refused { code }) => {
                tracing::warn!(code, "Discord refused what was playing");
                if let Some(sent) = sent.as_mut() {
                    sent.refusals = sent.refusals.saturating_add(1);
                }
            }
            Err(error) => {
                tracing::debug!(%error, "the connection to Discord was lost");
                self.waits = RETRY_AFTER_AT_FIRST;
                self.link = Link::Closed {
                    retry_at: Some(now + self.waits),
                };
            }
        }
    }

    fn farewell(&mut self) {
        if let Link::Open { session, sent, .. } = &mut self.link
            && sent.as_ref().is_some_and(|sent| sent.activity.is_some())
            && let Err(error) = session.set(None)
        {
            tracing::debug!(%error, "the presence could not be cleared; Discord clears it as the socket closes");
        }
        self.link = Link::Closed { retry_at: None };
    }
}

fn cover_for(
    releases: &dyn Releases,
    cached: &mut Option<CachedCover>,
    track: TrackId,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    tags: &TagSet,
    now: Instant,
) -> Option<Cover> {
    if let Some(held) = cached.as_ref()
        && held.track == track
        && held.location == *location
        && held.span == span
        && (held.cover.is_some()
            || now.saturating_duration_since(held.checked) < COVER_REFRESH_AFTER)
    {
        return held.cover.clone();
    }
    let cover = tagged(tags).or_else(|| releases.cover(location, span));
    *cached = Some(CachedCover {
        track,
        location: location.clone(),
        span,
        cover: cover.clone(),
        checked: now,
    });
    cover
}

fn named(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn tagged(tags: &TagSet) -> Option<Cover> {
    let mbid = |text: Option<&str>| text.and_then(|text| Mbid::new(text.trim()).ok());
    mbid(tags.musicbrainz_album_id.as_deref())
        .map(Cover::Release)
        .or_else(|| mbid(tags.musicbrainz_release_group_id.as_deref()).map(Cover::Group))
}

fn due(sent: Option<&Sent>, wanted: Option<&Activity>, now: Instant) -> bool {
    let Some(sent) = sent else {
        return wanted.is_some();
    };
    let changed = match (sent.activity.as_ref(), wanted) {
        (None, None) => false,
        (Some(was), Some(is)) => !was.alike(is) || was.drifted(is, DRIFT_ALLOWED),
        (Some(_), None) | (None, Some(_)) => true,
    };
    let waited = now.duration_since(sent.at);
    if changed {
        waited >= SENDS_APART
    } else {
        sent.refusals > 0 && waited >= wait_after_refusals(sent.refusals)
    }
}

fn wait_after_refusals(refusals: u32) -> Duration {
    let doublings = refusals.saturating_sub(1).min(u32::BITS - 1);
    SENT_AGAIN_AFTER_A_REFUSAL
        .saturating_mul(1 << doublings)
        .min(SENT_AGAIN_AT_MOST)
}

fn refusals_carried(sent: Option<&Sent>, wanted: Option<&Activity>) -> u32 {
    match (sent, wanted) {
        (Some(sent), Some(wanted)) => sent
            .activity
            .as_ref()
            .filter(|was| was.alike(wanted))
            .map_or(0, |_| sent.refusals),
        (Some(_) | None, None | Some(_)) => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        net::Shutdown,
        os::unix::net::{UnixListener, UnixStream},
        process,
        sync::{
            atomic::{AtomicU32, AtomicUsize, Ordering},
            mpsc,
        },
    };

    use parking_lot::Mutex;
    use resonate_core::{AppId, Frames};
    use serde_json::{Value, json};

    use super::*;
    use crate::frame::{Opcode, read_frame, write_frame};

    const FIRST_APP: &str = "1234567890123456789";
    const SECOND_APP: &str = "9876543210987654321";
    const HEARD_WITHIN: Duration = Duration::from_secs(5);
    static FOLDERS: AtomicU32 = AtomicU32::new(0);

    #[derive(Debug, PartialEq, Eq)]
    enum Said {
        Handshake {
            connection: usize,
            client_id: String,
        },
        Activity {
            connection: usize,
            details: Option<String>,
        },
    }

    struct FakeDiscord {
        folder: PathBuf,
        heard: mpsc::Receiver<Said>,
        streams: Arc<Mutex<Vec<UnixStream>>>,
    }

    impl FakeDiscord {
        fn socket_in(folder: &Path) -> PathBuf {
            folder.join("discord-ipc-0")
        }

        fn folder() -> PathBuf {
            let folder = env::temp_dir().join(format!(
                "resonate-publish-{}-{}",
                process::id(),
                FOLDERS.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&folder).expect("a folder");
            folder
        }

        fn listening_in(folder: PathBuf) -> Self {
            let listener = UnixListener::bind(Self::socket_in(&folder)).expect("bound");
            let (said, heard) = mpsc::channel();
            let streams = Arc::new(Mutex::new(Vec::new()));
            let kept = Arc::clone(&streams);
            thread::spawn(move || {
                for (connection, stream) in listener.incoming().enumerate() {
                    let Ok(mut stream) = stream else {
                        break;
                    };
                    kept.lock()
                        .push(stream.try_clone().expect("a second handle"));
                    let said = said.clone();
                    thread::spawn(move || serve(connection, &mut stream, &said));
                }
            });
            Self {
                folder,
                heard,
                streams,
            }
        }

        fn hear(&self) -> Said {
            self.heard
                .recv_timeout(HEARD_WITHIN)
                .expect("the fake Discord heard something")
        }

        fn hang_up(&self) {
            for stream in self.streams.lock().iter() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }

    impl Drop for FakeDiscord {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    fn serve(connection: usize, stream: &mut UnixStream, said: &mpsc::Sender<Said>) {
        let Ok(handshake) = read_frame(stream) else {
            return;
        };
        let body: Value = serde_json::from_slice(&handshake.body).expect("json");
        let client_id = body["client_id"].as_str().unwrap_or_default().to_owned();
        let _ = said.send(Said::Handshake {
            connection,
            client_id,
        });
        let ready = json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1 } });
        write_frame(
            stream,
            Opcode::Frame,
            &serde_json::to_vec(&ready).expect("json"),
        )
        .expect("ready");

        while let Ok(frame) = read_frame(stream) {
            let body: Value = serde_json::from_slice(&frame.body).expect("json");
            let details = body["args"]["activity"]["details"]
                .as_str()
                .map(str::to_owned);
            let _ = said.send(Said::Activity {
                connection,
                details,
            });
        }
    }

    fn found_at(path: PathBuf, finds: &Arc<AtomicUsize>) -> Find {
        let finds = Arc::clone(finds);
        Box::new(move |app, _| {
            finds.fetch_add(1, Ordering::Relaxed);
            Session::open(&path, app).map(|session| (session, path.clone()))
        })
    }

    fn app(digits: &str) -> AppId {
        AppId::parse(digits).expect("a snowflake")
    }

    fn retry_at(showing: &Showing) -> Option<Instant> {
        match showing.link {
            Link::Closed { retry_at } => retry_at,
            Link::Open { .. } | Link::Unknown { .. } => None,
        }
    }

    fn is_open(showing: &Showing) -> bool {
        matches!(showing.link, Link::Open { .. })
    }

    #[test]
    fn a_discord_nobody_answers_is_asked_again_ever_later_up_to_a_ceiling() {
        let folder = FakeDiscord::folder();
        let finds = Arc::new(AtomicUsize::new(0));
        let mut showing = Showing::finding_with(found_at(FakeDiscord::socket_in(&folder), &finds));
        let wanted = || Some(activity("Echoes", 0));
        let started = Instant::now();

        showing.show(app(FIRST_APP), wanted(), started);
        assert_eq!(finds.load(Ordering::Relaxed), 1);
        assert_eq!(retry_at(&showing), Some(started + RETRY_AFTER_AT_FIRST));
        assert_eq!(showing.waits, RETRY_AFTER_AT_FIRST * 2);

        showing.show(app(FIRST_APP), wanted(), started + Duration::from_secs(1));
        showing.show(app(FIRST_APP), None, started + RETRY_AFTER_AT_FIRST);
        assert_eq!(
            finds.load(Ordering::Relaxed),
            1,
            "Discord was asked before its wait ran out, or with nothing to show"
        );

        let mut asked_again = started;
        for _ in 0..8 {
            asked_again = retry_at(&showing).expect("a time to ask again");
            showing.show(app(FIRST_APP), wanted(), asked_again);
        }
        assert_eq!(finds.load(Ordering::Relaxed), 9);
        assert_eq!(showing.waits, RETRY_AFTER_AT_MOST);
        assert_eq!(retry_at(&showing), Some(asked_again + RETRY_AFTER_AT_MOST));
        fs::remove_dir_all(&folder).expect("the folder removed");
    }

    #[test]
    fn a_discord_that_appears_is_reached_and_what_is_wanted_shown_at_once() {
        let folder = FakeDiscord::folder();
        let finds = Arc::new(AtomicUsize::new(0));
        let mut showing = Showing::finding_with(found_at(FakeDiscord::socket_in(&folder), &finds));
        let started = Instant::now();

        showing.show(app(FIRST_APP), Some(activity("Echoes", 0)), started);
        let discord = FakeDiscord::listening_in(folder);
        showing.show(
            app(FIRST_APP),
            Some(activity("Echoes", 0)),
            started + RETRY_AFTER_AT_FIRST,
        );

        assert!(is_open(&showing));
        assert_eq!(showing.waits, RETRY_AFTER_AT_FIRST);
        assert_eq!(
            discord.hear(),
            Said::Handshake {
                connection: 0,
                client_id: FIRST_APP.to_owned()
            }
        );
        assert_eq!(
            discord.hear(),
            Said::Activity {
                connection: 0,
                details: Some("Echoes".to_owned())
            }
        );
    }

    #[test]
    fn a_changed_application_clears_the_old_one_and_connects_again_under_the_new_id() {
        let discord = FakeDiscord::listening_in(FakeDiscord::folder());
        let finds = Arc::new(AtomicUsize::new(0));
        let mut showing =
            Showing::finding_with(found_at(FakeDiscord::socket_in(&discord.folder), &finds));
        let started = Instant::now();

        showing.show(app(FIRST_APP), Some(activity("Echoes", 0)), started);
        showing.show(
            app(SECOND_APP),
            Some(activity("Time", 0)),
            started + SENDS_APART,
        );

        let mut heard: Vec<Said> = (0..5).map(|_| discord.hear()).collect();
        heard.sort_by_key(|said| match said {
            Said::Handshake { connection, .. } => (*connection, 0),
            Said::Activity {
                connection,
                details,
            } => (*connection, 1 + usize::from(details.is_none())),
        });
        assert_eq!(finds.load(Ordering::Relaxed), 2);
        assert!(is_open(&showing));
        assert_eq!(
            heard,
            [
                Said::Handshake {
                    connection: 0,
                    client_id: FIRST_APP.to_owned()
                },
                Said::Activity {
                    connection: 0,
                    details: Some("Echoes".to_owned())
                },
                Said::Activity {
                    connection: 0,
                    details: None
                },
                Said::Handshake {
                    connection: 1,
                    client_id: SECOND_APP.to_owned()
                },
                Said::Activity {
                    connection: 1,
                    details: Some("Time".to_owned())
                },
            ]
        );
    }

    #[test]
    fn an_application_discord_does_not_know_is_waited_out_until_the_id_changes() {
        let discord = FakeDiscord::listening_in(FakeDiscord::folder());
        let finds = Arc::new(AtomicUsize::new(0));
        let reaching = found_at(FakeDiscord::socket_in(&discord.folder), &finds);
        let unknown = app(FIRST_APP);
        let find: Find = Box::new(move |asked, answered| {
            if asked == unknown {
                return Err(Error::Closed { code: 4000 });
            }
            reaching(asked, answered)
        });
        let mut showing = Showing::finding_with(find);
        let started = Instant::now();

        showing.show(app(FIRST_APP), Some(activity("Echoes", 0)), started);
        assert!(matches!(showing.link, Link::Unknown { .. }));
        showing.show(
            app(FIRST_APP),
            Some(activity("Echoes", 0)),
            started + RETRY_AFTER_AT_MOST * 5,
        );
        assert!(matches!(showing.link, Link::Unknown { .. }));
        assert_eq!(finds.load(Ordering::Relaxed), 0);

        showing.show(
            app(SECOND_APP),
            Some(activity("Echoes", 0)),
            started + RETRY_AFTER_AT_MOST * 6,
        );

        assert!(is_open(&showing));
        assert_eq!(
            discord.hear(),
            Said::Handshake {
                connection: 0,
                client_id: SECOND_APP.to_owned()
            }
        );
    }

    #[test]
    fn a_lost_connection_is_made_again_after_the_first_wait_and_what_is_wanted_sent_afresh() {
        let discord = FakeDiscord::listening_in(FakeDiscord::folder());
        let finds = Arc::new(AtomicUsize::new(0));
        let mut showing =
            Showing::finding_with(found_at(FakeDiscord::socket_in(&discord.folder), &finds));
        let started = Instant::now();
        let wanted = || Some(activity("Echoes", 0));

        showing.show(app(FIRST_APP), wanted(), started);
        discord.hear();
        discord.hear();
        discord.hang_up();
        let lost_at = started + SENDS_APART;
        showing.show(app(FIRST_APP), wanted(), lost_at);

        assert!(!is_open(&showing));
        assert_eq!(retry_at(&showing), Some(lost_at + RETRY_AFTER_AT_FIRST));
        showing.show(app(FIRST_APP), wanted(), lost_at + RETRY_AFTER_AT_FIRST / 2);
        assert_eq!(finds.load(Ordering::Relaxed), 1);

        showing.show(app(FIRST_APP), wanted(), lost_at + RETRY_AFTER_AT_FIRST);

        assert!(is_open(&showing));
        assert_eq!(finds.load(Ordering::Relaxed), 2);
        assert_eq!(
            discord.hear(),
            Said::Handshake {
                connection: 1,
                client_id: FIRST_APP.to_owned()
            }
        );
        assert_eq!(
            discord.hear(),
            Said::Activity {
                connection: 1,
                details: Some("Echoes".to_owned())
            }
        );
    }

    const RELEASE: &str = "0A7F4B1C-2D3E-4F50-8A6B-7C8D9E0F1A2B";
    const GROUP: &str = "11111111-2222-3333-4444-555555555555";

    struct ChangingReleases {
        cover: RwLock<Option<Cover>>,
        reads: AtomicUsize,
    }

    impl Releases for ChangingReleases {
        fn cover(&self, _location: &MediaLocation, _span: Option<FrameSpan>) -> Option<Cover> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.cover.read().clone()
        }
    }

    #[test]
    fn a_cover_discovered_during_playback_replaces_a_cached_miss() {
        let releases = ChangingReleases {
            cover: RwLock::new(None),
            reads: AtomicUsize::new(0),
        };
        let mut cached = None;
        let track = TrackId::new(1).expect("a track id");
        let location = MediaLocation::local(Path::new("song.wav"));
        let now = Instant::now();
        let read = |cached: &mut Option<CachedCover>, at| {
            cover_for(
                &releases,
                cached,
                track,
                &location,
                None,
                &TagSet::default(),
                at,
            )
        };

        assert_eq!(read(&mut cached, now), None);
        *releases.cover.write() = Some(Cover::Group(Mbid::new(GROUP).expect("a group id")));
        assert_eq!(read(&mut cached, now + COVER_REFRESH_AFTER / 2), None);
        assert_eq!(releases.reads.load(Ordering::Relaxed), 1);
        assert_eq!(
            read(&mut cached, now + COVER_REFRESH_AFTER),
            Some(Cover::Group(Mbid::new(GROUP).expect("a group id")))
        );
        assert_eq!(releases.reads.load(Ordering::Relaxed), 2);
    }

    struct CoverPerCut;

    impl Releases for CoverPerCut {
        fn cover(&self, _location: &MediaLocation, span: Option<FrameSpan>) -> Option<Cover> {
            let release = match span.map(FrameSpan::start) {
                Some(Frames(0)) => RELEASE,
                _ => GROUP,
            };
            Some(Cover::Release(Mbid::new(release).expect("a release id")))
        }
    }

    #[test]
    fn another_cut_of_the_same_file_under_the_same_id_is_asked_about_again() {
        let mut cached = None;
        let track = TrackId::MAX;
        let location = MediaLocation::local(Path::new("album.flac"));
        let now = Instant::now();
        let first = FrameSpan::between(Frames(0), Frames(44_100));
        let second = FrameSpan::starting(Frames(44_100));
        let read = |cached: &mut Option<CachedCover>, span| {
            cover_for(
                &CoverPerCut,
                cached,
                track,
                &location,
                Some(span),
                &TagSet::default(),
                now,
            )
        };

        assert_eq!(
            read(&mut cached, first),
            Some(Cover::Release(Mbid::new(RELEASE).expect("a release id")))
        );
        assert_eq!(
            read(&mut cached, second),
            Some(Cover::Release(Mbid::new(GROUP).expect("a release id")))
        );
    }

    fn activity(title: &str, position: u64) -> Activity {
        let presence = Presence {
            enabled: true,
            app: AppId::parse("1234567890123456789"),
            ..Presence::OFF
        };
        let playing = Playing {
            title: title.to_owned(),
            artist: None,
            album: None,
            cover: None,
            position: Duration::from_secs(position),
            duration: Some(Duration::from_secs(600)),
            paused: false,
        };
        Activity::of(
            &presence,
            &playing,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10_000),
        )
        .expect("an activity")
    }

    #[test]
    fn a_fresh_session_shows_whatever_is_wanted_at_once() {
        let now = Instant::now();

        assert!(due(None, Some(&activity("Echoes", 0)), now));
        assert!(!due(None, None, now));
    }

    #[test]
    fn nothing_is_sent_again_while_it_says_the_same() {
        let now = Instant::now();
        let sent = Sent {
            activity: Some(activity("Echoes", 0)),
            at: now,
            refusals: 0,
        };

        assert!(!due(
            Some(&sent),
            Some(&activity("Echoes", 1)),
            now + SENDS_APART * 3
        ));
    }

    #[test]
    fn a_change_waits_out_the_spacing_discord_asks_for() {
        let now = Instant::now();
        let sent = Sent {
            activity: Some(activity("Echoes", 0)),
            at: now,
            refusals: 0,
        };
        let next = activity("Time", 0);

        assert!(!due(Some(&sent), Some(&next), now + Duration::from_secs(1)));
        assert!(due(Some(&sent), Some(&next), now + SENDS_APART));
        assert!(due(Some(&sent), None, now + SENDS_APART));
    }

    #[test]
    fn a_refused_activity_is_offered_again_on_the_same_session_after_a_wait() {
        let now = Instant::now();
        let sent = Sent {
            activity: Some(activity("Echoes", 0)),
            at: now,
            refusals: 1,
        };
        let same = activity("Echoes", 1);

        assert!(!due(Some(&sent), Some(&same), now + SENDS_APART));
        assert!(due(
            Some(&sent),
            Some(&same),
            now + SENT_AGAIN_AFTER_A_REFUSAL
        ));
        assert!(due(
            Some(&sent),
            Some(&activity("Time", 0)),
            now + SENDS_APART
        ));
    }

    #[test]
    fn a_refusal_that_never_clears_is_offered_again_ever_less_often_up_to_a_ceiling() {
        let now = Instant::now();
        let same = activity("Echoes", 1);
        let sent = |refusals| Sent {
            activity: Some(activity("Echoes", 0)),
            at: now,
            refusals,
        };

        assert!(due(
            Some(&sent(1)),
            Some(&same),
            now + Duration::from_secs(15)
        ));
        assert!(!due(
            Some(&sent(2)),
            Some(&same),
            now + Duration::from_secs(15)
        ));
        assert!(due(
            Some(&sent(2)),
            Some(&same),
            now + Duration::from_secs(30)
        ));
        assert!(due(
            Some(&sent(3)),
            Some(&same),
            now + Duration::from_secs(60)
        ));
        assert_eq!(wait_after_refusals(u32::MAX), SENT_AGAIN_AT_MOST);
        assert_eq!(wait_after_refusals(40), SENT_AGAIN_AT_MOST);
    }

    #[test]
    fn refusals_are_carried_to_the_same_activity_and_to_nothing_else() {
        let was = Sent {
            activity: Some(activity("Echoes", 0)),
            at: Instant::now(),
            refusals: 3,
        };

        assert_eq!(
            refusals_carried(Some(&was), Some(&activity("Echoes", 1))),
            3
        );
        assert_eq!(refusals_carried(Some(&was), Some(&activity("Time", 0))), 0);
        assert_eq!(refusals_carried(Some(&was), None), 0);
        assert_eq!(refusals_carried(None, Some(&activity("Echoes", 0))), 0);
    }

    #[test]
    fn a_seek_is_sent_as_a_change() {
        let now = Instant::now();
        let sent = Sent {
            activity: Some(activity("Echoes", 0)),
            at: now,
            refusals: 0,
        };

        assert!(due(
            Some(&sent),
            Some(&activity("Echoes", 300)),
            now + SENDS_APART
        ));
    }

    #[test]
    fn a_release_in_the_tags_is_the_cover_and_a_release_group_stands_in_for_one() {
        let released = TagSet {
            musicbrainz_album_id: Some(RELEASE.to_owned()),
            musicbrainz_release_group_id: Some(GROUP.to_owned()),
            ..TagSet::default()
        };
        assert_eq!(
            tagged(&released),
            Some(Cover::Release(Mbid::new(RELEASE).expect("an mbid")))
        );

        let grouped = TagSet {
            musicbrainz_album_id: Some("not an id".to_owned()),
            musicbrainz_release_group_id: Some(GROUP.to_owned()),
            ..TagSet::default()
        };
        assert_eq!(
            tagged(&grouped),
            Some(Cover::Group(Mbid::new(GROUP).expect("an mbid")))
        );

        assert_eq!(tagged(&TagSet::default()), None);
    }

    #[test]
    fn a_blank_tag_names_nothing() {
        assert_eq!(named(Some("  ")), None);
        assert_eq!(named(Some(" Meddle ")), Some("Meddle".to_owned()));
        assert_eq!(named(None), None);
    }
}
