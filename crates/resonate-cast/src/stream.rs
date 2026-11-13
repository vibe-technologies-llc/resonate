use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Downstream, Error as SinkError, Operation, SinkResult, SinkStream, StreamClock,
    StreamCommand, StreamEvent, StreamRequest, StreamState, Words,
};
use resonate_core::{Frames, SampleFormat};
use resonate_upnp::{Feed, Server, Token};

use crate::{
    Asked, Device, Error, Result, device,
    json::Json,
    link::{ANSWERED_WITHIN, Link, Namespace, Told},
};

const STEERED_EVERY: Duration = Duration::from_millis(50);

const STATUS_ASKED_EVERY: Duration = Duration::from_millis(500);

const STATUS_FAILURES_AT_MOST: u32 = 6;

const HEARD_WITHIN: Duration = Duration::from_millis(250);

const LOADED_WITHIN: Duration = Duration::from_secs(20);

const MUSIC_TRACK: u32 = 3;

const NO_FAULT_CODE: i32 = -1;

enum Steer {
    Active(bool),
    Drain,
    Close,
}

struct Steering(Sender<Steer>);

impl Drop for Steering {
    fn drop(&mut self) {
        let _ = self.0.send(Steer::Close);
    }
}

const NOBODY: u64 = u64::MAX;

type Lane = Arc<AtomicU64>;

#[derive(Default)]
pub(crate) struct Lanes {
    lanes: Mutex<BTreeMap<SocketAddr, Lane>>,
    minted: AtomicU64,
}

impl Lanes {
    fn taken(&self, at: SocketAddr) -> Hold {
        let lane = Arc::clone(
            self.lanes
                .lock()
                .entry(at)
                .or_insert_with(|| Arc::new(AtomicU64::new(NOBODY))),
        );
        let mine = self.minted.fetch_add(1, Ordering::Relaxed);
        lane.store(mine, Ordering::Release);
        Hold { lane, mine }
    }

    pub(crate) fn idle(&self, at: SocketAddr) -> Option<Hold> {
        let lane = Arc::clone(
            self.lanes
                .lock()
                .entry(at)
                .or_insert_with(|| Arc::new(AtomicU64::new(NOBODY))),
        );
        let mine = self.minted.fetch_add(1, Ordering::Relaxed);
        lane.compare_exchange(NOBODY, mine, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(Hold { lane, mine })
    }
}

pub(crate) struct Hold {
    lane: Lane,
    mine: u64,
}

impl Hold {
    fn is_mine(&self) -> bool {
        self.lane.load(Ordering::Acquire) == self.mine
    }

    pub(crate) fn steer<T>(&self, call: impl FnOnce() -> T) -> Option<T> {
        self.is_mine().then(call)
    }

    pub(crate) fn let_go<T>(&self, call: impl FnOnce() -> T) -> Option<T> {
        self.lane
            .compare_exchange(self.mine, NOBODY, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(call)
    }
}

pub(crate) fn open(
    device: Arc<Device>,
    server: &Arc<Server>,
    lanes: &Lanes,
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
) -> SinkResult<SinkStream> {
    let spec = request.spec;
    let carriage = device.carriage();
    if !carriage.carries(spec) {
        return Err(Error::FormatRefused(spec).into());
    }
    let link = Link::connect(device.at())?;

    let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
    let feed = Arc::new(Feed::new(
        spec,
        carriage.clone(),
        source,
        Arc::clone(&clock),
    ));
    let token = server.offer(Arc::clone(&feed));
    let address = match server.address_toward(device.at(), token, carriage.extension()) {
        Ok(address) => address,
        Err(error) => {
            server.withdraw(token);
            return Err(Error::served(error).into());
        }
    };
    let hold = lanes.taken(device.at());

    let (announce, events) = unbounded();
    let (steer, steered) = unbounded();
    let words = match spec.format {
        SampleFormat::S24 => Words::Packed,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Words::Whole,
    };
    let _ = announce.send(StreamEvent::FormatChanged { spec, words });

    let watched = Watch {
        device,
        link,
        server: Arc::clone(server),
        token,
        feed,
        hold,
        offered: Offered {
            address,
            kind: carriage.kind_of(spec),
            title: request.media_name.clone(),
        },
        announce,
    };
    thread::Builder::new()
        .name("resonate-cast-stream".to_owned())
        .spawn(move || watched.run(&steered))
        .map_err(|source| SinkError::Backend {
            op: Operation::Open,
            source,
        })?;

    let steering = Steering(steer);
    Ok(SinkStream::new(
        events,
        clock,
        Box::new(move |command| {
            let steer = match command {
                StreamCommand::SetActive(active) => Steer::Active(active),
                StreamCommand::Drain => Steer::Drain,
                StreamCommand::Close => Steer::Close,
            };
            steering.0.send(steer).map_err(|_| SinkError::LoopStopped)
        }),
    )
    .holding(Downstream::Seconds))
}

pub(crate) struct Offered {
    pub(crate) address: String,
    pub(crate) kind: String,
    pub(crate) title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Played {
    pub(crate) transport: String,
    pub(crate) session: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Idle {
    Finished,
    Errored,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) session: u64,
    pub(crate) playing: bool,
    pub(crate) idle: Option<Idle>,
    pub(crate) heard: Option<Duration>,
}

enum Reading {
    Heard,
    Drained,
    Gone,
}

struct Watch {
    device: Arc<Device>,
    link: Link,
    server: Arc<Server>,
    token: Token,
    feed: Arc<Feed>,
    hold: Hold,
    offered: Offered,
    announce: Sender<StreamEvent>,
}

impl Watch {
    fn run(self, steered: &Receiver<Steer>) {
        let mut state = StreamState::Paused;
        let mut played: Option<Played> = None;
        let mut drain_asked = false;
        let mut drained = false;
        let mut unanswered = 0;
        let mut asked_at = Instant::now();

        loop {
            match steered.recv_timeout(STEERED_EVERY) {
                Ok(Steer::Active(true)) => {
                    self.feed.set_active(true);
                    match self.hold.steer(|| self.play(&mut played)) {
                        Some(Ok(true)) => state = self.moved(state, StreamState::Streaming),
                        Some(Err(error)) => state = self.failed(state, &error),
                        Some(Ok(false)) | None => {}
                    }
                }
                Ok(Steer::Active(false)) => {
                    self.feed.set_active(false);
                    if let Some(played) = &played
                        && let Some(Err(error)) =
                            self.hold.steer(|| self.media(played, Asked::Pause))
                    {
                        tracing::debug!(%error, receiver = %self.device.id(), "a Cast receiver did not pause");
                    }
                    if state == StreamState::Streaming {
                        state = self.moved(state, StreamState::Paused);
                    }
                }
                Ok(Steer::Drain) => {
                    self.feed.drain();
                    drain_asked = true;
                }
                Ok(Steer::Close) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }

            if state == StreamState::Failed {
                continue;
            }
            let told: Vec<Told> = self.link.told().try_iter().collect();
            for told in told {
                let reading = played
                    .as_ref()
                    .and_then(|played| self.read_told(&told, played, drain_asked));
                match reading {
                    Some(Reading::Drained) if !drained => {
                        drained = true;
                        let _ = self.announce.send(StreamEvent::Drained);
                    }
                    Some(Reading::Gone) if self.hold.steer(|| ()).is_some() => {
                        state = self.failed(
                            state,
                            &Error::Closed {
                                asked: Asked::MediaStatus,
                            },
                        );
                    }
                    Some(Reading::Heard | Reading::Drained | Reading::Gone) | None => {}
                }
            }
            if state != StreamState::Failed
                && !self.link.is_alive()
                && self.hold.steer(|| ()).is_some()
            {
                state = self.failed(
                    state,
                    &Error::Closed {
                        asked: Asked::MediaStatus,
                    },
                );
            }

            if state != StreamState::Streaming || asked_at.elapsed() < STATUS_ASKED_EVERY {
                continue;
            }
            asked_at = Instant::now();
            let Some(played) = played.as_ref() else {
                continue;
            };
            let Some(answer) = self.hold.steer(|| self.media(played, Asked::MediaStatus)) else {
                continue;
            };
            let reading = answer.and_then(|answer| {
                self.read(&answer, played, drain_asked, true)
                    .ok_or(Error::Unreadable {
                        asked: Asked::MediaStatus,
                    })
            });
            match reading {
                Ok(reading) => {
                    unanswered = 0;
                    match reading {
                        Reading::Drained if !drained => {
                            drained = true;
                            let _ = self.announce.send(StreamEvent::Drained);
                        }
                        Reading::Gone => {
                            state = self.failed(
                                state,
                                &Error::Closed {
                                    asked: Asked::MediaStatus,
                                },
                            );
                        }
                        Reading::Heard | Reading::Drained => {}
                    }
                }
                Err(error) => {
                    unanswered += 1;
                    tracing::debug!(%error, receiver = %self.device.id(), "a Cast receiver did not say where it is");
                    if unanswered >= STATUS_FAILURES_AT_MOST {
                        state = self.failed(state, &error);
                    }
                }
            }
        }

        self.feed.close();
        self.server.withdraw(self.token);
        let stopped = self.hold.let_go(|| match &played {
            Some(played) => self.media(played, Asked::Stop).map(drop),
            None => Ok(()),
        });
        if let Some(Err(error)) = stopped {
            tracing::debug!(%error, receiver = %self.device.id(), "a Cast receiver did not stop");
        }
    }

    fn play(&self, played: &mut Option<Played>) -> Result<bool> {
        if let Some(played) = played {
            return self.media(played, Asked::Play).map(|_| true);
        }
        let session = device::launched(&self.link)?;
        if !self.hold.is_mine() {
            return Ok(false);
        }
        let entry = loaded(&self.link, &session.transport, &self.offered)?;
        *played = Some(Played {
            transport: session.transport,
            session: entry.session,
        });
        Ok(true)
    }

    fn media(&self, played: &Played, asked: Asked) -> Result<Json> {
        media(&self.link, played, asked)
    }

    fn read_told(&self, told: &Told, played: &Played, drain_asked: bool) -> Option<Reading> {
        match told.namespace {
            Namespace::Connection
                if told.from == played.transport && told.said.kind() == Some("CLOSE") =>
            {
                Some(Reading::Gone)
            }
            Namespace::Media => self.read(&told.said, played, drain_asked, false),
            Namespace::Connection | Namespace::Heartbeat | Namespace::Receiver => None,
        }
    }

    fn read(
        &self,
        said: &Json,
        played: &Played,
        drain_asked: bool,
        asked: bool,
    ) -> Option<Reading> {
        let entries = entries_in(said)?;
        let Some(entry) = entries.iter().find(|entry| entry.session == played.session) else {
            let ended = drain_asked && self.feed.ended_at().is_some();
            return match (ended, asked) {
                (true, _) => Some(Reading::Drained),
                (false, true) => Some(Reading::Gone),
                (false, false) => Some(Reading::Heard),
            };
        };
        if let Some(heard) = entry.heard {
            self.feed.heard_through(heard);
        }
        match entry.idle {
            Some(Idle::Finished) if drain_asked => Some(Reading::Drained),
            Some(Idle::Finished | Idle::Errored | Idle::Ended) => Some(Reading::Gone),
            None => {
                let drained = drain_asked
                    && entry
                        .heard
                        .zip(self.feed.ended_at())
                        .is_some_and(|(heard, ended)| {
                            self.feed.frames_of(heard.saturating_add(HEARD_WITHIN)) >= ended
                        });
                Some(if drained {
                    Reading::Drained
                } else {
                    Reading::Heard
                })
            }
        }
    }

    fn moved(&self, from: StreamState, to: StreamState) -> StreamState {
        if from != to {
            let _ = self.announce.send(StreamEvent::StateChanged { from, to });
        }
        to
    }

    fn failed(&self, from: StreamState, error: &Error) -> StreamState {
        tracing::warn!(%error, receiver = %self.device.id(), "a Cast receiver stopped taking the stream");
        let event = match error {
            Error::Refused { asked, .. } => StreamEvent::Refused {
                from,
                op: match asked {
                    Asked::Launch | Asked::Load | Asked::Play => Operation::Start,
                    Asked::Status
                    | Asked::Pause
                    | Asked::Stop
                    | Asked::MediaStatus
                    | Asked::SetVolume
                    | Asked::SetMute => Operation::Connect,
                },
                status: NO_FAULT_CODE,
            },
            _ => StreamEvent::StateChanged {
                from,
                to: StreamState::Failed,
            },
        };
        let _ = self.announce.send(event);
        StreamState::Failed
    }
}

pub(crate) fn loaded(link: &Link, transport: &str, offered: &Offered) -> Result<Entry> {
    link.tell(
        Namespace::Connection,
        transport,
        Json::object([
            ("type", Json::text("CONNECT")),
            ("origin", Json::Object(Vec::new())),
        ]),
    );
    let loaded = link.ask(
        Namespace::Media,
        transport,
        Asked::Load,
        Json::object([
            (
                "media",
                Json::object([
                    ("contentId", Json::text(offered.address.as_str())),
                    ("contentType", Json::text(offered.kind.as_str())),
                    ("streamType", Json::text("LIVE")),
                    (
                        "metadata",
                        Json::object([
                            ("metadataType", MUSIC_TRACK.into()),
                            ("title", Json::text(offered.title.as_str())),
                        ]),
                    ),
                ]),
            ),
            ("autoplay", true.into()),
        ]),
        LOADED_WITHIN,
    )?;
    entries_in(&loaded)
        .and_then(|entries| entries.first().copied())
        .ok_or(Error::Unreadable { asked: Asked::Load })
}

pub(crate) fn media(link: &Link, played: &Played, asked: Asked) -> Result<Json> {
    link.ask(
        Namespace::Media,
        &played.transport,
        asked,
        Json::object([("mediaSessionId", Json::Number(played.session as f64))]),
        ANSWERED_WITHIN,
    )
}

pub(crate) fn entries_in(said: &Json) -> Option<Vec<Entry>> {
    if said.kind() != Some("MEDIA_STATUS") {
        return None;
    }
    let entries = said
        .get("status")
        .and_then(Json::as_list)?
        .iter()
        .filter_map(|entry| {
            let session = entry
                .get("mediaSessionId")
                .and_then(Json::as_number)
                .filter(|session| *session >= 0.0)? as u64;
            let state = entry.get("playerState").and_then(Json::as_str);
            let idle = (state == Some("IDLE")).then(|| {
                match entry.get("idleReason").and_then(Json::as_str) {
                    Some("FINISHED") => Idle::Finished,
                    Some("ERROR") => Idle::Errored,
                    _ => Idle::Ended,
                }
            });
            let heard = entry
                .get("currentTime")
                .and_then(Json::as_number)
                .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok());
            Some(Entry {
                session,
                playing: state == Some("PLAYING"),
                idle,
                heard,
            })
        })
        .collect();
    Some(entries)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use crossbeam_channel::bounded;

    use super::*;

    fn receiver() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8009)
    }

    #[test]
    fn a_stream_taking_a_receiver_over_does_not_wait_for_a_call_the_old_one_is_making() {
        let lanes = Lanes::default();
        let old = lanes.taken(receiver());
        let (started, running) = bounded(0);
        let (finish, finished) = bounded::<()>(0);

        thread::scope(|scope| {
            let call = scope.spawn(|| {
                old.steer(|| {
                    started.send(()).expect("the call is heard");
                    finished.recv().expect("the call is let finish");
                })
            });
            running.recv().expect("the call began");

            let new = lanes.taken(receiver());

            assert!(new.is_mine());
            assert!(!old.is_mine());
            finish.send(()).expect("the call finishes");
            assert_eq!(call.join().expect("the call ends"), Some(()));
            assert_eq!(old.steer(|| ()), None);
            assert_eq!(old.let_go(|| ()), None);
            assert_eq!(new.steer(|| ()), Some(()));
        });
    }

    #[test]
    fn a_stream_lets_go_of_its_receiver_once_and_only_while_it_holds_it() {
        let lanes = Lanes::default();
        let hold = lanes.taken(receiver());
        let elsewhere = lanes.taken(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8010));

        assert_eq!(hold.let_go(|| ()), Some(()));
        assert_eq!(hold.let_go(|| ()), None);
        assert_eq!(hold.steer(|| ()), None);
        assert!(elsewhere.is_mine());
    }

    #[test]
    fn a_media_status_gives_each_session_its_place_and_whether_it_ended() {
        let said = Json::read(
            r#"{"type":"MEDIA_STATUS","requestId":4,"status":[
                {"mediaSessionId":2,"playerState":"PLAYING","currentTime":12.5},
                {"mediaSessionId":3,"playerState":"IDLE","idleReason":"FINISHED"},
                {"mediaSessionId":4,"playerState":"IDLE","idleReason":"ERROR"},
                {"mediaSessionId":5,"playerState":"IDLE","idleReason":"CANCELLED"},
                {"playerState":"PLAYING"}]}"#,
        )
        .expect("JSON");

        assert_eq!(
            entries_in(&said).expect("a media status"),
            [
                Entry {
                    session: 2,
                    playing: true,
                    idle: None,
                    heard: Some(Duration::from_millis(12_500))
                },
                Entry {
                    session: 3,
                    playing: false,
                    idle: Some(Idle::Finished),
                    heard: None
                },
                Entry {
                    session: 4,
                    playing: false,
                    idle: Some(Idle::Errored),
                    heard: None
                },
                Entry {
                    session: 5,
                    playing: false,
                    idle: Some(Idle::Ended),
                    heard: None
                },
            ]
        );
        assert!(
            entries_in(&Json::read(r#"{"type":"RECEIVER_STATUS","status":[]}"#).expect("JSON"))
                .is_none()
        );
    }

    #[test]
    fn a_place_no_duration_can_hold_is_no_place_rather_than_a_panic() {
        let said = Json::read(
            r#"{"type":"MEDIA_STATUS","requestId":4,"status":[
                {"mediaSessionId":2,"playerState":"PLAYING","currentTime":1e30}]}"#,
        )
        .expect("JSON");

        assert_eq!(
            entries_in(&said).expect("a media status"),
            [Entry {
                session: 2,
                playing: true,
                idle: None,
                heard: None
            }]
        );
    }
}
