use std::{
    collections::BTreeMap,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    Backend, Error, Opener, Operation, Pace, SinkChange, SinkId, SinkInfo, SinkResult, Surveyor,
    scalar_of, watch,
};
use resonate_core::Gain;
use resonate_win32::{EndpointVolume, Level, VolumeWatch};
use wasapi::{DeviceEnumerator, DeviceEventCallbacks, DeviceEventRegistration};

use crate::{
    device::{Devices, enter_the_apartment, level_error},
    stream::{Known, Opens},
    survey::Probed,
    surveying::{Surveys, joined_within},
    ways::endpoint_of,
};

const WATCHED_WITHIN: Duration = Duration::from_secs(2);
const WATCHER_LET_GO_WITHIN: Duration = Duration::from_millis(500);

pub struct Wasapi {
    known: Known,
    surveys: Arc<Surveys>,
    changes: Receiver<SinkChange>,
    stopping: Sender<()>,
    ended: Receiver<()>,
    watcher: Option<JoinHandle<()>>,
}

struct Survey {
    surveys: Arc<Surveys>,
}

impl Wasapi {
    pub fn start(_app_name: &str) -> SinkResult<Self> {
        let known: Known = Arc::new(Mutex::new(Vec::new()));
        let surveys = Arc::new(surveys_into(Arc::clone(&known))?);
        let (told, changes) = unbounded();
        let (stopping, stopped) = bounded(1);
        let (ending, ended) = bounded::<()>(1);

        let watching = Arc::clone(&surveys);
        let watcher = thread::Builder::new()
            .name("resonate-wasapi-watch".into())
            .spawn(move || {
                let _ending = ending;

                watched(&watching, &told, &stopped);
            })
            .map_err(|source| Error::Backend {
                op: Operation::Connect,
                source,
            })?;
        Ok(Self {
            known,
            surveys,
            changes,
            stopping,
            ended,
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

fn surveys_into(known: Known) -> SinkResult<Surveys> {
    let mut probed = Probed::default();

    Surveys::start(move || {
        let sinks = probed.survey(&mut Devices)?;

        *known.lock() = sinks
            .iter()
            .map(|sink| (sink.id, sink.name.clone()))
            .collect();
        Ok(sinks)
    })
}

fn watched(surveys: &Surveys, told: &Sender<SinkChange>, stopped: &Receiver<()>) {
    enter_the_apartment();

    let (nudge, nudged) = bounded(1);
    let _registration = told_of_endpoints(nudge.clone());
    let Some(first) = surveys.baseline(
        stopped,
        Pace::ENDPOINTS.surveyed_at_least_every,
        WATCHED_WITHIN,
    ) else {
        return;
    };
    let mut volumes = VolumesWatched::default();

    volumes.follow(&first, &nudge);

    let survey = || {
        let now = surveys.within(WATCHED_WITHIN)?.ok()?;

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
    fn enumerate_sinks(&self, timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        self.surveys.answered_within(timeout)
    }
}

impl Backend for Wasapi {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Survey {
            surveys: Arc::clone(&self.surveys),
        })
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Opens {
            known: Arc::clone(&self.known),
            surveys: Arc::clone(&self.surveys),
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

        if let Some(watcher) = self.watcher.take()
            && !joined_within(watcher, &self.ended, WATCHER_LET_GO_WITHIN)
        {
            tracing::debug!("the endpoint watcher is still in a survey; it ends once that returns");
        }
        Ok(())
    }
}
