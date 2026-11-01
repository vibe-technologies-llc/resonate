use std::{
    collections::BTreeMap,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    Backend, Error, Opener, Operation, SinkChange, SinkId, SinkInfo, SinkResult, Surveyor,
};
use resonate_core::Gain;
use resonate_win32::{EndpointVolume, Level, VolumeWatch};
use wasapi::{DeviceEnumerator, DeviceEventCallbacks, DeviceEventRegistration};

use crate::{
    device::{enter_the_apartment, enumerate, level_error},
    level::scalar_of,
    stream::{Known, Opens},
    watching::{Pace, watch},
    ways::endpoint_of,
};

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
            .spawn(move || watched(first, &watching, &told, &stopped))
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

    fn endpoint_of(&self, sink: SinkId) -> SinkResult<EndpointVolume> {
        let node = self
            .known
            .lock()
            .iter()
            .find(|(held, _)| *held == sink)
            .map(|(_, node)| node.clone())
            .ok_or(Error::SinkGone { node: sink })?;
        let (_, endpoint) = endpoint_of(&node).ok_or(Error::SinkGone { node: sink })?;
        enter_the_apartment();
        EndpointVolume::of(endpoint).map_err(level_error)
    }
}

fn remember(known: &Known, sinks: &[SinkInfo]) {
    *known.lock() = sinks
        .iter()
        .map(|sink| (sink.id, sink.name.clone()))
        .collect();
}

fn watched(first: Vec<SinkInfo>, known: &Known, told: &Sender<SinkChange>, stopped: &Receiver<()>) {
    enter_the_apartment();
    let (nudge, nudged) = bounded(1);
    let _registration = told_of_endpoints(nudge.clone());
    let mut volumes = VolumesWatched::default();
    volumes.follow(&first, &nudge);
    let survey = || {
        let now = enumerate().ok()?;
        remember(known, &now);
        volumes.follow(&now, &nudge);
        Some(now)
    };
    watch(first, survey, told, &nudged, stopped, Pace::ENDPOINTS);
}

#[derive(Default)]
struct VolumesWatched {
    by_endpoint: BTreeMap<String, VolumeWatch>,
}

impl VolumesWatched {
    fn follow(&mut self, sinks: &[SinkInfo], nudge: &Sender<()>) {
        let endpoints: Vec<&str> = sinks
            .iter()
            .filter_map(|sink| endpoint_of(&sink.name).map(|(_, endpoint)| endpoint))
            .collect();
        self.by_endpoint
            .retain(|endpoint, _| endpoints.contains(&endpoint.as_str()));
        for endpoint in endpoints {
            if self.by_endpoint.contains_key(endpoint) {
                continue;
            }
            let nudge = nudge.clone();
            let watched = EndpointVolume::of(endpoint).and_then(|volume| {
                volume.watched(move || {
                    let _ = nudge.try_send(());
                })
            });
            match watched {
                Ok(watch) => {
                    self.by_endpoint.insert(endpoint.to_owned(), watch);
                }
                Err(error) => {
                    tracing::debug!(%error, endpoint, "the endpoint's volume is followed by the poll alone");
                }
            }
        }
    }
}

fn told_of_endpoints(nudge: Sender<()>) -> Option<DeviceEventRegistration> {
    let mut callbacks = DeviceEventCallbacks::new();
    let nudged = move || {
        let _ = nudge.try_send(());
    };
    let added = nudged.clone();
    let removed = nudged.clone();
    let moved = nudged.clone();
    let defaulted = nudged.clone();
    callbacks.set_device_added_callback(move |_| added());
    callbacks.set_device_removed_callback(move |_| removed());
    callbacks.set_device_state_callback(move |_, _| moved());
    callbacks.set_default_device_callback(move |_, _, _| defaulted());
    callbacks.set_property_value_callback(move |_, _| nudged());
    let registered = DeviceEnumerator::new()
        .and_then(|enumerator| enumerator.register_notification_callback(callbacks));
    match registered {
        Ok(registration) => Some(registration),
        Err(error) => {
            tracing::warn!(%error, "endpoint notifications refused; devices are found by the poll alone");
            None
        }
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

    fn set_device_volume(&self, sink: SinkId, gain: Gain) -> SinkResult<()> {
        self.endpoint_of(sink)?
            .set_level(Level::new(scalar_of(gain)))
            .map_err(level_error)
    }

    fn set_device_mute(&self, sink: SinkId, muted: bool) -> SinkResult<()> {
        self.endpoint_of(sink)?
            .set_muted(muted)
            .map_err(level_error)
    }

    fn shutdown(mut self: Box<Self>) -> SinkResult<()> {
        let _ = self.stopping.send(());
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        Ok(())
    }
}
