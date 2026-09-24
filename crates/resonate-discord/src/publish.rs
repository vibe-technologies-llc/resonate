use std::{
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
const RETRY_AFTER: Duration = Duration::from_secs(15);
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
            link: Link::Closed { retry_at: None },
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
    refused: bool,
}

struct CachedCover {
    track: TrackId,
    location: MediaLocation,
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

struct Publisher {
    player: Arc<Player>,
    releases: Arc<dyn Releases>,
    presence: Arc<RwLock<Presence>>,
    link: Link,
    covered: Option<CachedCover>,
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
        self.farewell();
    }

    fn tick(&mut self) {
        let presence = self.presence.read().clone();
        let Some(app) = presence.app.filter(|_| presence.active()) else {
            self.farewell();
            return;
        };
        let wanted = self
            .playing(&presence)
            .and_then(|playing| Activity::of(&presence, &playing, SystemTime::now()));
        self.show(app, wanted);
    }

    fn show(&mut self, app: AppId, wanted: Option<Activity>) {
        let now = Instant::now();
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
            match Session::find(app) {
                Ok(session) => {
                    tracing::info!("showing what is playing on Discord");
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
                    tracing::debug!(%error, "Discord could not be reached; asking again later");
                    self.link = Link::Closed {
                        retry_at: Some(now + RETRY_AFTER),
                    };
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
            *sent = Some(Sent {
                activity: wanted,
                at: now,
                refused: false,
            });
            set
        });
        match outcome {
            Ok(()) => {}
            Err(Error::Refused { code }) => {
                tracing::warn!(code, "Discord refused what was playing");
                if let Some(sent) = sent.as_mut() {
                    sent.refused = true;
                }
            }
            Err(error) => {
                tracing::debug!(%error, "the connection to Discord was lost");
                self.link = Link::Closed {
                    retry_at: Some(now + RETRY_AFTER),
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
        && (held.cover.is_some()
            || now.saturating_duration_since(held.checked) < COVER_REFRESH_AFTER)
    {
        return held.cover.clone();
    }
    let cover = tagged(tags).or_else(|| releases.cover(location, span));
    *cached = Some(CachedCover {
        track,
        location: location.clone(),
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
        sent.refused && waited >= RETRY_AFTER
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use resonate_core::AppId;

    use super::*;

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
            refused: false,
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
            refused: false,
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
            refused: true,
        };
        let same = activity("Echoes", 1);

        assert!(!due(Some(&sent), Some(&same), now + SENDS_APART));
        assert!(due(Some(&sent), Some(&same), now + RETRY_AFTER));
        assert!(due(
            Some(&sent),
            Some(&activity("Time", 0)),
            now + SENDS_APART
        ));
    }

    #[test]
    fn a_seek_is_sent_as_a_change() {
        let now = Instant::now();
        let sent = Sent {
            activity: Some(activity("Echoes", 0)),
            at: now,
            refused: false,
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
