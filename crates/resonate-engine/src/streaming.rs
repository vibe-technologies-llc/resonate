use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, SendError, Sender, TryRecvError, bounded};
use resonate_audio::{AudioSource, Reshaped, SinkStream, StreamEvent, StreamRequest, StreamState};

use crate::{Opener, SinkResult};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Came {
    Opened,
    Reshaped { heard_while_kept: Vec<StreamEvent> },
}

pub(crate) type Streamed = (u64, SinkResult<(SinkStream, Came)>);

pub(crate) struct Asked {
    pub(crate) made: u64,
    pub(crate) request: StreamRequest,
    pub(crate) source: Box<dyn AudioSource>,
    pub(crate) kept: Option<Box<SinkStream>>,
}

impl Asked {
    pub(crate) fn answered_by(self, opener: &dyn Opener) -> Streamed {
        let Self {
            made,
            request,
            source,
            kept,
        } = self;
        let source = match kept {
            Some(stream) => match reshaped(*stream, &request, source) {
                Ok((stream, heard_while_kept)) => {
                    return (made, Ok((stream, Came::Reshaped { heard_while_kept })));
                }
                Err(Unkept::Refused(source)) => source,
                Err(Unkept::Lost(error)) => return (made, Err(error)),
            },
            None => source,
        };
        let opened = opener.open(&request, source);
        (made, opened.map(|stream| (stream, Came::Opened)))
    }
}

enum Unkept {
    Refused(Box<dyn AudioSource>),
    Lost(resonate_audio::Error),
}

fn reshaped(
    stream: SinkStream,
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
) -> Result<(SinkStream, Vec<StreamEvent>), Unkept> {
    let Some(heard_while_kept) = heard_while_kept(&stream) else {
        tracing::debug!("a stream that failed while it was kept is closed rather than reshaped");
        close_unwanted(stream);
        return Err(Unkept::Refused(source));
    };
    match stream.reshape(request, source) {
        Reshaped::Kept => Ok((stream, heard_while_kept)),
        Reshaped::Refused { source, because } => {
            tracing::debug!(
                ?because,
                "the backend would not reshape the stream; it is reopened"
            );
            close_unwanted(stream);
            Err(Unkept::Refused(source))
        }
        Reshaped::Lost(error) => {
            close_unwanted(stream);
            Err(Unkept::Lost(error))
        }
    }
}

fn heard_while_kept(stream: &SinkStream) -> Option<Vec<StreamEvent>> {
    let heard: Vec<StreamEvent> = stream.events().try_iter().collect();
    let failed = heard.iter().any(|event| {
        matches!(
            event,
            StreamEvent::StateChanged {
                to: StreamState::Failed,
                ..
            } | StreamEvent::Refused { .. }
        )
    });
    (!failed).then_some(heard)
}

pub(crate) struct Streaming {
    asks: Option<Sender<Asked>>,
    answers: Receiver<Streamed>,
    in_flight: bool,
    thread: Option<JoinHandle<()>>,
}

impl Streaming {
    pub(crate) fn start(opener: Arc<dyn Opener>) -> Option<Self> {
        let (asks, asked) = bounded::<Asked>(1);
        let (answer, answers) = bounded(1);
        let thread = thread::Builder::new()
            .name("resonate-stream-open".to_owned())
            .spawn(move || {
                for asked in asked {
                    let streamed = asked.answered_by(opener.as_ref());
                    if let Err(SendError((_, Ok((stream, _))))) = answer.send(streamed) {
                        close_unwanted(stream);
                        return;
                    }
                }
            })
            .inspect_err(|error| {
                tracing::warn!(%error, "no thread could be started to open streams on; they open in line");
            })
            .ok()?;

        Some(Self {
            asks: Some(asks),
            answers,
            in_flight: false,
            thread: Some(thread),
        })
    }

    pub(crate) const fn answers(&self) -> &Receiver<Streamed> {
        &self.answers
    }

    pub(crate) const fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    pub(crate) fn ask(&mut self, asked: Asked) -> Result<(), Asked> {
        let Some(asks) = self.asks.as_ref() else {
            return Err(asked);
        };
        asks.try_send(asked)
            .map_err(|refused| refused.into_inner())?;
        self.in_flight = true;
        Ok(())
    }

    pub(crate) fn answered(&mut self) -> Option<Streamed> {
        match self.answers.try_recv() {
            Ok(streamed) => {
                self.in_flight = false;
                Some(streamed)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.in_flight = false;
                None
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        self.asks = None;
        for (_, opened) in &self.answers {
            if let Ok((stream, _)) = opened {
                close_unwanted(stream);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.in_flight = false;
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn close_unwanted(stream: SinkStream) {
    let _ = stream.set_active(false);
    if let Err(error) = stream.close() {
        tracing::debug!(%error, "a stream nothing wanted any more did not close cleanly");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crossbeam_channel::unbounded;
    use parking_lot::Mutex;
    use resonate_audio::{LatencyRequest, MediaRole, StreamClock, StreamCommand, Unreshapable};
    use resonate_core::{ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec};

    use super::*;

    struct Silent;

    impl AudioSource for Silent {
        fn fill(&mut self, _dst: &mut [u8]) -> usize {
            0
        }
    }

    #[derive(Default)]
    struct Commands {
        heard: Mutex<Vec<StreamCommand>>,
    }

    impl Commands {
        fn closes(&self) -> usize {
            self.heard
                .lock()
                .iter()
                .filter(|command| **command == StreamCommand::Close)
                .count()
        }
    }

    fn request() -> StreamRequest {
        StreamRequest {
            target: None,
            spec: StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: "a test".to_owned(),
            force_graph_rate: false,
            no_convert: false,
            realtime: false,
        }
    }

    fn stream(commands: &Arc<Commands>) -> (SinkStream, Sender<StreamEvent>) {
        let (events, heard) = unbounded();
        let commands = Arc::clone(commands);
        let stream = SinkStream::new(
            heard,
            Arc::new(StreamClock::behind_by(Frames(0))),
            Box::new(move |command| {
                commands.heard.lock().push(command);
                Ok(())
            }),
        );
        (stream, events)
    }

    fn asked(kept: Option<SinkStream>) -> Asked {
        Asked {
            made: 7,
            request: request(),
            source: Box::new(Silent),
            kept: kept.map(Box::new),
        }
    }

    struct Held {
        commands: Arc<Commands>,
        entered: Sender<()>,
        released: Receiver<()>,
        opens: AtomicUsize,
    }

    struct HeldOpen(Arc<Held>);

    impl Opener for HeldOpen {
        fn open(
            &self,
            _request: &StreamRequest,
            _source: Box<dyn AudioSource>,
        ) -> SinkResult<SinkStream> {
            self.0.opens.fetch_add(1, Ordering::Relaxed);
            let _ = self.0.entered.send(());
            let _ = self.0.released.recv();
            Ok(stream(&self.0.commands).0)
        }
    }

    struct Immediate {
        commands: Arc<Commands>,
        opens: AtomicUsize,
    }

    impl Opener for Immediate {
        fn open(
            &self,
            _request: &StreamRequest,
            _source: Box<dyn AudioSource>,
        ) -> SinkResult<SinkStream> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Ok(stream(&self.commands).0)
        }
    }

    #[test]
    fn a_stream_that_opens_after_stop_began_is_closed_and_the_worker_is_gone_when_stop_returns() {
        let commands = Arc::new(Commands::default());
        let (entered, entering) = unbounded();
        let (release, released) = unbounded();
        let opener = Arc::new(HeldOpen(Arc::new(Held {
            commands: Arc::clone(&commands),
            entered,
            released,
            opens: AtomicUsize::new(0),
        })));
        let mut streaming = Streaming::start(opener).expect("a worker");

        assert!(streaming.ask(asked(None)).is_ok());
        entering.recv().expect("the open began");
        let releasing = thread::spawn(move || {
            let _ = release.send(());
        });
        streaming.stop();
        releasing.join().expect("released");

        assert!(!streaming.is_in_flight());
        assert!(streaming.thread.is_none());
        assert_eq!(commands.closes(), 1);
        assert!(streaming.ask(asked(None)).is_err());
    }

    #[test]
    fn a_stream_answered_but_never_taken_is_closed_by_stop() {
        let commands = Arc::new(Commands::default());
        let opener = Arc::new(Immediate {
            commands: Arc::clone(&commands),
            opens: AtomicUsize::new(0),
        });
        let mut streaming =
            Streaming::start(Arc::clone(&opener) as Arc<dyn Opener>).expect("a worker");

        assert!(streaming.ask(asked(None)).is_ok());
        streaming.stop();

        assert_eq!(opener.opens.load(Ordering::Relaxed), 1);
        assert_eq!(commands.closes(), 1);
    }

    #[test]
    fn a_stream_still_in_the_channel_when_the_streaming_is_dropped_is_closed() {
        let commands = Arc::new(Commands::default());
        let opener = Arc::new(Immediate {
            commands: Arc::clone(&commands),
            opens: AtomicUsize::new(0),
        });
        let mut streaming = Streaming::start(opener).expect("a worker");

        assert!(streaming.ask(asked(None)).is_ok());
        drop(streaming);

        assert_eq!(commands.closes(), 1);
    }

    #[test]
    fn an_answer_taken_is_the_one_asked_for_and_clears_what_was_in_flight() {
        let commands = Arc::new(Commands::default());
        let opener = Arc::new(Immediate {
            commands: Arc::clone(&commands),
            opens: AtomicUsize::new(0),
        });
        let mut streaming = Streaming::start(opener).expect("a worker");

        assert!(streaming.ask(asked(None)).is_ok());
        let in_flight = streaming.is_in_flight();
        let (made, opened) = loop {
            if let Some(streamed) = streaming.answered() {
                break streamed;
            }
            thread::yield_now();
        };
        let (landed, came) = opened.expect("the stream opened");

        assert!(in_flight);
        assert!(!streaming.is_in_flight());
        assert_eq!(made, 7);
        assert_eq!(came, Came::Opened);
        close_unwanted(landed);
        assert_eq!(commands.closes(), 1);
    }

    #[test]
    fn a_kept_stream_that_failed_while_kept_is_closed_and_a_new_one_is_opened() {
        let commands = Arc::new(Commands::default());
        let opener = Immediate {
            commands: Arc::clone(&commands),
            opens: AtomicUsize::new(0),
        };
        let kept_commands = Arc::new(Commands::default());
        let (kept, events) = stream(&kept_commands);
        events
            .send(StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Failed,
            })
            .expect("the event is held");

        let (made, answered) = asked(Some(kept)).answered_by(&opener);
        let (_, came) = answered.expect("a stream opened");

        assert_eq!(made, 7);
        assert_eq!(came, Came::Opened);
        assert_eq!(opener.opens.load(Ordering::Relaxed), 1);
        assert_eq!(kept_commands.closes(), 1);
        assert_eq!(commands.closes(), 0);
    }

    #[test]
    fn a_kept_stream_the_backend_reshapes_is_kept_with_what_it_heard_and_none_is_opened() {
        let opener = Immediate {
            commands: Arc::new(Commands::default()),
            opens: AtomicUsize::new(0),
        };
        let kept_commands = Arc::new(Commands::default());
        let (kept, events) = stream(&kept_commands);
        let kept = kept.reshaped_by(Box::new(|_, _| Reshaped::Kept));
        events
            .send(StreamEvent::Drained)
            .expect("the event is held");

        let (_, answered) = asked(Some(kept)).answered_by(&opener);
        let (_, came) = answered.expect("the stream was kept");

        assert_eq!(
            came,
            Came::Reshaped {
                heard_while_kept: vec![StreamEvent::Drained]
            }
        );
        assert_eq!(opener.opens.load(Ordering::Relaxed), 0);
        assert_eq!(kept_commands.closes(), 0);
    }

    #[test]
    fn a_kept_stream_the_backend_refuses_to_reshape_is_closed_and_reopened_with_the_same_source() {
        let opener = Immediate {
            commands: Arc::new(Commands::default()),
            opens: AtomicUsize::new(0),
        };
        let kept_commands = Arc::new(Commands::default());
        let (kept, _events) = stream(&kept_commands);
        let kept = kept.reshaped_by(Box::new(|_, source| Reshaped::Refused {
            source,
            because: Unreshapable::AnotherFormat,
        }));

        let (_, answered) = asked(Some(kept)).answered_by(&opener);
        let (_, came) = answered.expect("a stream opened");

        assert_eq!(came, Came::Opened);
        assert_eq!(opener.opens.load(Ordering::Relaxed), 1);
        assert_eq!(kept_commands.closes(), 1);
    }

    #[test]
    fn a_kept_stream_lost_while_reshaping_is_closed_and_the_loss_is_answered() {
        let opener = Immediate {
            commands: Arc::new(Commands::default()),
            opens: AtomicUsize::new(0),
        };
        let kept_commands = Arc::new(Commands::default());
        let (kept, _events) = stream(&kept_commands);
        let kept = kept.reshaped_by(Box::new(|_, _| {
            Reshaped::Lost(resonate_audio::Error::Disconnected)
        }));

        let (made, answered) = asked(Some(kept)).answered_by(&opener);

        assert_eq!(made, 7);
        assert!(matches!(answered, Err(resonate_audio::Error::Disconnected)));
        assert_eq!(opener.opens.load(Ordering::Relaxed), 0);
        assert_eq!(kept_commands.closes(), 1);
    }
}
