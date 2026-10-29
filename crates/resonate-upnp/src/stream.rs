use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use resonate_audio::{
    AudioSource, Error as SinkError, Operation, SinkResult, SinkStream, StreamClock, StreamCommand,
    StreamEvent, StreamRequest, StreamState, Words,
};
use resonate_core::{Frames, SampleFormat};

use crate::{
    Action, Asked, Error, Renderer, Result,
    feed::Feed,
    serve::{Server, Token},
};

const POSITION_ASKED_EVERY: Duration = Duration::from_millis(500);

const POSITION_FAILURES_AT_MOST: u32 = 6;

const HEARD_TO_THE_SECOND: Duration = Duration::from_secs(1);

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

pub(crate) fn open(
    renderer: Arc<Renderer>,
    server: &Arc<Server>,
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
) -> SinkResult<SinkStream> {
    let spec = request.spec;
    if !renderer.carriage().carries(spec) {
        return Err(Error::FormatRefused(spec).into());
    }

    let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
    let feed = Arc::new(Feed::new(
        spec,
        renderer.carriage().clone(),
        source,
        Arc::clone(&clock),
    ));
    let token = server.offer(Arc::clone(&feed));
    if let Err(error) = offered(&renderer, server, token, request) {
        server.withdraw(token);
        return Err(error.into());
    }

    let (announce, events) = unbounded();
    let (steer, steered) = unbounded();
    let words = match spec.format {
        SampleFormat::S24 => Words::Packed,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Words::Whole,
    };
    let _ = announce.send(StreamEvent::FormatChanged { spec, words });

    let watched = Watch {
        renderer,
        server: Arc::clone(server),
        token,
        feed,
        announce,
    };
    thread::Builder::new()
        .name("resonate-upnp-stream".to_owned())
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
    ))
}

fn offered(
    renderer: &Renderer,
    server: &Server,
    token: Token,
    request: &StreamRequest,
) -> Result<()> {
    let reached = renderer
        .transport_reached_at()
        .reached_at()
        .map_err(|source| Error::Network {
            asked: Asked::Action(Action::SetAvTransportUri),
            source,
        })?;
    let address = server.address_toward(reached, token, renderer.carriage().extension())?;
    renderer.set_uri(&address, &request.media_name, request.spec)
}

struct Watch {
    renderer: Arc<Renderer>,
    server: Arc<Server>,
    token: Token,
    feed: Arc<Feed>,
    announce: Sender<StreamEvent>,
}

impl Watch {
    fn run(self, steered: &Receiver<Steer>) {
        let mut state = StreamState::Paused;
        let mut playing_asked = false;
        let mut drain_asked = false;
        let mut drained = false;
        let mut unanswered = 0;
        let mut asked_at = Instant::now();

        loop {
            match steered.recv_timeout(POSITION_ASKED_EVERY) {
                Ok(Steer::Active(true)) => {
                    self.feed.set_active(true);
                    match self.renderer.play() {
                        Ok(()) => {
                            playing_asked = true;
                            state = self.moved(state, StreamState::Streaming);
                        }
                        Err(error) => state = self.failed(state, &error),
                    }
                }
                Ok(Steer::Active(false)) => {
                    self.feed.set_active(false);
                    if playing_asked && let Err(error) = self.renderer.pause() {
                        tracing::debug!(%error, renderer = %self.renderer.udn(), "a renderer did not pause");
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

            if state != StreamState::Streaming || asked_at.elapsed() < POSITION_ASKED_EVERY {
                continue;
            }
            asked_at = Instant::now();
            match self.renderer.heard() {
                Ok(Some(heard)) => {
                    unanswered = 0;
                    self.feed.heard_through(heard);
                    if drain_asked
                        && !drained
                        && let Some(ended) = self.feed.ended_at()
                        && self.feed.frames_of(heard + HEARD_TO_THE_SECOND) >= ended
                    {
                        drained = true;
                        let _ = self.announce.send(StreamEvent::Drained);
                    }
                }
                Ok(None) => unanswered = 0,
                Err(error) => {
                    unanswered += 1;
                    tracing::debug!(%error, renderer = %self.renderer.udn(), "a renderer did not say where it is");
                    if unanswered >= POSITION_FAILURES_AT_MOST {
                        state = self.failed(state, &error);
                    }
                }
            }
        }

        self.feed.close();
        self.server.withdraw(self.token);
        if playing_asked && let Err(error) = self.renderer.stop() {
            tracing::debug!(%error, renderer = %self.renderer.udn(), "a renderer did not stop");
        }
    }

    fn moved(&self, from: StreamState, to: StreamState) -> StreamState {
        if from != to {
            let _ = self.announce.send(StreamEvent::StateChanged { from, to });
        }
        to
    }

    fn failed(&self, from: StreamState, error: &Error) -> StreamState {
        tracing::warn!(%error, renderer = %self.renderer.udn(), "a renderer stopped taking the stream");
        let event = match error {
            Error::Fault { action, code } => StreamEvent::Refused {
                from,
                op: match action {
                    Action::Play => Operation::Start,
                    _ => Operation::Connect,
                },
                status: code
                    .and_then(|code| i32::try_from(code).ok())
                    .unwrap_or(NO_FAULT_CODE),
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
