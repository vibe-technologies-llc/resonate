use std::{
    collections::BTreeMap,
    io,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_apple::{
    Device, Listening, Mute, Scalar, Told, Volume, notify_on_the_hals_own_thread,
    told_of_the_system,
};
use resonate_audio::{
    Backend, Error, NodeName, Opener, Operation, Pace, SinkChange, SinkId, SinkInfo, SinkResult,
    Surveyor, scalar_of, watch,
};
use resonate_core::Gain;

use crate::{
    device::{backend_error, device_of_uid, enumerate},
    opening::{Known, Opens},
    remembered::Remembered,
    ways::device_of,
};

const TOLD_OF_THE_SYSTEM: [Told; 3] = [Told::Devices, Told::DefaultOutput, Told::ServiceRestarted];
const TOLD_OF_A_DEVICE: [Told; 5] = [Told::Rate, Told::Alive, Told::Hog, Told::Volume, Told::Mute];

pub struct CoreAudio {
    known: Known,
    remembered: Arc<Mutex<Remembered>>,
    changes: Receiver<SinkChange>,
    stopping: Sender<()>,
    watcher: Option<JoinHandle<()>>,
}

fn surveyed(known: &Known, remembered: &Mutex<Remembered>) -> SinkResult<Vec<SinkInfo>> {
    let sinks = enumerate(remembered)?;
    *known.lock() = sinks
        .iter()
        .map(|sink| (sink.id, sink.name.clone()))
        .collect();
    Ok(sinks)
}

#[derive(Default)]
struct Watched {
    devices: BTreeMap<Device, Vec<Listening>>,
}

impl Watched {
    fn follow(&mut self, nudge: &Sender<()>) {
        let Ok(present) = Device::all() else {
            return;
        };
        self.devices.retain(|device, _| present.contains(device));
        for device in present {
            self.devices.entry(device).or_insert_with(|| {
                TOLD_OF_A_DEVICE
                    .into_iter()
                    .filter_map(|what| {
                        let nudge = nudge.clone();
                        device
                            .told_of(what, move || {
                                let _ = nudge.try_send(());
                            })
                            .ok()
                    })
                    .collect()
            });
        }
    }
}

fn watching(
    first: Vec<SinkInfo>,
    known: &Known,
    remembered: &Mutex<Remembered>,
    told: &Sender<SinkChange>,
    stopped: &Receiver<()>,
) {
    if let Err(error) = notify_on_the_hals_own_thread() {
        tracing::debug!(%error, "Core Audio keeps its notifications on the main run loop");
    }
    let (nudge, nudged) = bounded(1);
    let _system: Vec<Listening> = TOLD_OF_THE_SYSTEM
        .into_iter()
        .filter_map(|what| {
            let nudge = nudge.clone();
            told_of_the_system(what, move || {
                let _ = nudge.try_send(());
            })
            .ok()
        })
        .collect();
    let mut watched = Watched::default();
    watched.follow(&nudge);
    let survey = || {
        let sinks = surveyed(known, remembered).ok()?;
        watched.follow(&nudge);
        Some(sinks)
    };
    watch(first, survey, told, &nudged, stopped, Pace::ENDPOINTS);
}

impl CoreAudio {
    pub fn start(_app_name: &str) -> SinkResult<Self> {
        let known = Known::default();
        let remembered = Arc::new(Mutex::new(Remembered::default()));
        let first = surveyed(&known, &remembered)?;
        let (told, changes) = unbounded();
        let (stopping, stopped) = bounded(1);
        let watcher = thread::Builder::new()
            .name("resonate-coreaudio-watch".into())
            .spawn({
                let known = Arc::clone(&known);
                let remembered = Arc::clone(&remembered);
                move || watching(first, &known, &remembered, &told, &stopped)
            })
            .map_err(|source| Error::Backend {
                op: Operation::Connect,
                source,
            })?;
        Ok(Self {
            known,
            remembered,
            changes,
            stopping,
            watcher: Some(watcher),
        })
    }

    fn device_of(&self, sink: SinkId) -> SinkResult<Device> {
        let node: Option<NodeName> = self
            .known
            .lock()
            .iter()
            .find(|(held, _)| *held == sink)
            .map(|(_, node)| node.clone());
        node.as_ref()
            .and_then(device_of)
            .and_then(|(_, uid)| device_of_uid(uid))
            .ok_or(Error::SinkGone { node: sink })
    }
}

fn no_control() -> Error {
    Error::Backend {
        op: Operation::Volume,
        source: io::ErrorKind::Unsupported.into(),
    }
}

struct Survey {
    known: Known,
    remembered: Arc<Mutex<Remembered>>,
}

impl Surveyor for Survey {
    fn enumerate_sinks(&self, _: Duration) -> SinkResult<Vec<SinkInfo>> {
        surveyed(&self.known, &self.remembered)
    }
}

impl Backend for CoreAudio {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Survey {
            known: Arc::clone(&self.known),
            remembered: Arc::clone(&self.remembered),
        })
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Opens {
            known: Arc::clone(&self.known),
            remembered: Arc::clone(&self.remembered),
        })
    }

    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        let volume = Volume::of(self.device_of(sink)?).ok_or_else(no_control)?;
        volume
            .set_level(Scalar::new(scalar_of(gain)))
            .map_err(|error| backend_error(Operation::Volume, error))
    }

    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        let mute = Mute::of(self.device_of(sink)?).ok_or_else(no_control)?;
        mute.set_muted(muted)
            .map_err(|error| backend_error(Operation::Volume, error))
    }

    fn shutdown(mut self: Box<Self>) -> SinkResult<()> {
        let _ = self.stopping.send(());
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        Ok(())
    }
}
