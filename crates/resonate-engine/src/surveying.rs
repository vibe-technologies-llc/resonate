use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded};
use resonate_pipewire::SinkInfo;

use crate::{SinkResult, backend::Surveyor};

pub(crate) type Surveyed = SinkResult<Vec<SinkInfo>>;

pub(crate) struct Surveying {
    asks: Option<Sender<()>>,
    answers: Receiver<Surveyed>,
    asked: bool,
    thread: Option<JoinHandle<()>>,
}

impl Surveying {
    pub(crate) fn start(surveyor: Arc<dyn Surveyor>, within: Duration) -> Option<Self> {
        let (asks, asked) = bounded::<()>(1);
        let (answer, answers) = bounded(1);
        let thread = thread::Builder::new()
            .name("resonate-sinks".to_owned())
            .spawn(move || {
                for () in asked {
                    if answer.send(surveyor.enumerate_sinks(within)).is_err() {
                        return;
                    }
                }
            })
            .inspect_err(|error| {
                tracing::warn!(%error, "no thread could be started to follow the sinks");
            })
            .ok()?;

        Some(Self {
            asks: Some(asks),
            answers,
            asked: false,
            thread: Some(thread),
        })
    }

    pub(crate) const fn answers(&self) -> &Receiver<Surveyed> {
        &self.answers
    }

    pub(crate) fn ask(&mut self) -> bool {
        if self.asked {
            return false;
        }
        let Some(asks) = self.asks.as_ref() else {
            return false;
        };
        self.asked = asks.try_send(()).is_ok();
        self.asked
    }

    pub(crate) fn answered(&mut self) -> Option<Surveyed> {
        match self.answers.try_recv() {
            Ok(found) => {
                self.asked = false;
                Some(found)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    pub(crate) fn stop(&mut self) {
        self.asks = None;
        while self.answers.try_recv().is_ok() {}
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Surveying {
    fn drop(&mut self) {
        self.stop();
    }
}
