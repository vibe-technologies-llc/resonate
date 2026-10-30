use std::{
    collections::BTreeMap,
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
    Asked, CastId, Device, Error, Result, device,
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

type Lane = Arc<Mutex<Option<u64>>>;

#[derive(Default)]
pub(crate) struct Lanes {
    lanes: Mutex<BTreeMap<CastId, Lane>>,
    minted: AtomicU64,
}

impl Lanes {
    fn taken(&self, id: &CastId) -> Hold {
        let lane = Arc::clone(self.lanes.lock().entry(id.clone()).or_default());
        let mine = self.minted.fetch_add(1, Ordering::Relaxed);
        *lane.lock() = Some(mine);
        Hold { lane, mine }
    }
}

struct Hold {
    lane: Lane,
    mine: u64,
}

impl Hold {
    fn steer<T>(&self, call: impl FnOnce() -> T) -> Option<T> {
        let held = self.lane.lock();
        (*held == Some(self.mine)).then(call)
    }

    fn let_go<T>(&self, call: impl FnOnce() -> T) -> Option<T> {
        let mut held = self.lane.lock();
        if *held != Some(self.mine) {
            return None;
        }
        *held = None;
        Some(call())
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
    let carriage = Device::carriage();
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
    let hold = lanes.taken(device.id());

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

struct Offered {
    address: String,
    kind: String,
    title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Played {
    transport: String,
    session: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Idle {
    Finished,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry {
    session: u64,
    idle: Option<Idle>,
    heard: Option<Duration>,
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
                        Some(Ok(())) => state = self.moved(state, StreamState::Streaming),
                        Some(Err(error)) => state = self.failed(state, &error),
                        None => {}
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

    fn play(&self, played: &mut Option<Played>) -> Result<()> {
        if let Some(played) = played {
            return self.media(played, Asked::Play).map(drop);
        }
        let session = device::launched(&self.link)?;
        self.link.tell(
            Namespace::Connection,
            &session.transport,
            Json::object([
                ("type", Json::text("CONNECT")),
                ("origin", Json::Object(Vec::new())),
            ]),
        );
        let loaded = self.link.ask(
            Namespace::Media,
            &session.transport,
            Asked::Load,
            Json::object([
                (
                    "media",
                    Json::object([
                        ("contentId", Json::text(self.offered.address.as_str())),
                        ("contentType", Json::text(self.offered.kind.as_str())),
                        ("streamType", Json::text("LIVE")),
                        (
                            "metadata",
                            Json::object([
                                ("metadataType", MUSIC_TRACK.into()),
                                ("title", Json::text(self.offered.title.as_str())),
                            ]),
                        ),
                    ]),
                ),
                ("autoplay", true.into()),
            ]),
            LOADED_WITHIN,
        )?;
        let entry = entries_in(&loaded)
            .and_then(|entries| entries.first().copied())
            .ok_or(Error::Unreadable { asked: Asked::Load })?;
        *played = Some(Played {
            transport: session.transport,
            session: entry.session,
        });
        Ok(())
    }

    fn media(&self, played: &Played, asked: Asked) -> Result<Json> {
        self.link.ask(
            Namespace::Media,
            &played.transport,
            asked,
            Json::object([("mediaSessionId", Json::Number(played.session as f64))]),
            ANSWERED_WITHIN,
        )
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
            Some(Idle::Finished | Idle::Ended) => Some(Reading::Gone),
            None => {
                let drained = drain_asked
                    && entry
                        .heard
                        .zip(self.feed.ended_at())
                        .is_some_and(|(heard, ended)| {
                            self.feed.frames_of(heard + HEARD_WITHIN) >= ended
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

fn entries_in(said: &Json) -> Option<Vec<Entry>> {
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
            let idle =
                (entry.get("playerState").and_then(Json::as_str) == Some("IDLE")).then(|| {
                    match entry.get("idleReason").and_then(Json::as_str) {
                        Some("FINISHED") => Idle::Finished,
                        _ => Idle::Ended,
                    }
                });
            let heard = entry
                .get("currentTime")
                .and_then(Json::as_number)
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(Duration::from_secs_f64);
            Some(Entry {
                session,
                idle,
                heard,
            })
        })
        .collect();
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_status_gives_each_session_its_place_and_whether_it_ended() {
        let said = Json::read(
            r#"{"type":"MEDIA_STATUS","requestId":4,"status":[
                {"mediaSessionId":2,"playerState":"PLAYING","currentTime":12.5},
                {"mediaSessionId":3,"playerState":"IDLE","idleReason":"FINISHED"},
                {"mediaSessionId":4,"playerState":"IDLE","idleReason":"ERROR"},
                {"playerState":"PLAYING"}]}"#,
        )
        .expect("JSON");

        assert_eq!(
            entries_in(&said).expect("a media status"),
            [
                Entry {
                    session: 2,
                    idle: None,
                    heard: Some(Duration::from_millis(12_500))
                },
                Entry {
                    session: 3,
                    idle: Some(Idle::Finished),
                    heard: None
                },
                Entry {
                    session: 4,
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
}
