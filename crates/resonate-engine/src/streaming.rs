use std::{sync::Arc, thread};

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
}

impl Streaming {
    pub(crate) fn start(opener: Arc<dyn Opener>) -> Option<Self> {
        let (asks, asked) = bounded::<Asked>(1);
        let (answer, answers) = bounded(1);
        thread::Builder::new()
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
        while let Ok((_, opened)) = self.answers.try_recv() {
            if let Ok((stream, _)) = opened {
                close_unwanted(stream);
            }
        }
        self.in_flight = false;
    }
}

pub(crate) fn close_unwanted(stream: SinkStream) {
    let _ = stream.set_active(false);
    if let Err(error) = stream.close() {
        tracing::debug!(%error, "a stream nothing wanted any more did not close cleanly");
    }
}
