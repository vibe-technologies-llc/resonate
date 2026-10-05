use std::{sync::Arc, thread};

use crossbeam_channel::{Receiver, SendError, Sender, TryRecvError, bounded};
use resonate_pipewire::{AudioSource, SinkStream, StreamRequest};

use crate::{SinkResult, backend::Opener};

pub(crate) type Streamed = (u64, SinkResult<SinkStream>);

pub(crate) struct Asked {
    pub(crate) made: u64,
    pub(crate) request: StreamRequest,
    pub(crate) source: Box<dyn AudioSource>,
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
                for Asked {
                    made,
                    request,
                    source,
                } in asked
                {
                    let opened = opener.open(&request, source);
                    if let Err(SendError((_, Ok(stream)))) = answer.send((made, opened)) {
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
            if let Ok(stream) = opened {
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
