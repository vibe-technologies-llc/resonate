use std::{
    io,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    Backend, Error, Opener, Operation, SinkChange, SinkId, SinkInfo, SinkResult, Surveyor,
};
use resonate_core::Gain;

use crate::{
    changes::changes_between,
    device::enumerate,
    stream::{Known, Opens},
};

const WATCHED_EVERY: Duration = Duration::from_secs(2);

pub struct Wasapi {
    known: Known,
    changes: Receiver<SinkChange>,
    stopping: Sender<()>,
    watcher: Option<JoinHandle<()>>,
}

struct Survey {
    known: Known,
}

impl Wasapi {
    pub fn start(_app_name: &str) -> SinkResult<Self> {
        let known: Known = Arc::new(Mutex::new(Vec::new()));
        let (told, changes) = unbounded();
        let (stopping, stopped) = bounded(1);
        let first = enumerate().unwrap_or_default();
        remember(&known, &first);

        let watching = Arc::clone(&known);
        let watcher = thread::Builder::new()
            .name("resonate-wasapi-watch".into())
            .spawn(move || watch(first, &watching, &told, &stopped))
            .map_err(|source| Error::Backend {
                op: Operation::Connect,
                source,
            })?;
        Ok(Self {
            known,
            changes,
            stopping,
            watcher: Some(watcher),
        })
    }
}

fn remember(known: &Known, sinks: &[SinkInfo]) {
    *known.lock() = sinks
        .iter()
        .map(|sink| (sink.id, sink.name.clone()))
        .collect();
}

fn watch(
    mut before: Vec<SinkInfo>,
    known: &Known,
    told: &Sender<SinkChange>,
    stopped: &Receiver<()>,
) {
    loop {
        match stopped.recv_timeout(WATCHED_EVERY) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
        let Ok(now) = enumerate() else {
            continue;
        };
        remember(known, &now);
        for change in changes_between(&before, &now) {
            let _ = told.send(change);
        }
        before = now;
    }
}

impl Surveyor for Survey {
    fn enumerate_sinks(&self, _: Duration) -> SinkResult<Vec<SinkInfo>> {
        let sinks = enumerate()?;
        remember(&self.known, &sinks);
        Ok(sinks)
    }
}

impl Backend for Wasapi {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Survey {
            known: Arc::clone(&self.known),
        })
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Opens {
            known: Arc::clone(&self.known),
        })
    }

    fn set_device_volume(&self, _: SinkId, _: Gain) -> SinkResult<()> {
        Err(Error::Backend {
            op: Operation::Volume,
            source: io::ErrorKind::Unsupported.into(),
        })
    }

    fn set_device_mute(&self, _: SinkId, _: bool) -> SinkResult<()> {
        Err(Error::Backend {
            op: Operation::Volume,
            source: io::ErrorKind::Unsupported.into(),
        })
    }

    fn shutdown(mut self: Box<Self>) -> SinkResult<()> {
        let _ = self.stopping.send(());
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        Ok(())
    }
}
