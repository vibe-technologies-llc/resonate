use std::{sync::Arc, thread, time::Duration};

use resonate_audio::{Error as AudioError, NodeName, Operation};
use wasapi::{DeviceEnumerator, Direction};

use crate::{
    CaptureOp, Clip, Error, Hearing, Listening, Result,
    endpoints::{Endpoint, Microphone, Tap, microphones_of, output_of, tap_of},
    recording::Recorder,
    session::{Played, Sitting, finished},
    wasapi_apartment::{Apartment, backend_error},
    wasapi_capture::Capture,
    wasapi_playback::Playback,
};

#[derive(Clone, Debug)]
pub struct Listener;

impl Listener {
    pub fn new(_: &str) -> Self {
        Self
    }

    pub fn microphones(&self) -> Result<Vec<Microphone>> {
        let surveying = thread::Builder::new()
            .name("resonate-listen-survey".into())
            .spawn(surveyed)
            .map_err(|source| {
                Error::capture(
                    CaptureOp::Connect,
                    AudioError::Backend {
                        op: Operation::Connect,
                        source,
                    },
                )
            })?;
        surveying
            .join()
            .unwrap_or_else(|_| Err(Error::capture(CaptureOp::Connect, AudioError::LoopStopped)))
    }

    pub fn record(&self, from: &Listening, length: Duration, hearing: &Hearing) -> Result<Clip> {
        self.recorded(from, length, hearing, || Ok(None))
    }

    pub fn play_and_record(
        &self,
        played: &[f32],
        through: Option<&NodeName>,
        from: &Listening,
        length: Duration,
        hearing: &Hearing,
    ) -> Result<Clip> {
        let played = Played::of(played);
        self.recorded(from, length, hearing, || {
            let output =
                output_of(through).map_err(|name| Error::NoSuchSink { name: name.clone() })?;
            Playback::open(output, played).map(Some)
        })
    }

    fn recorded(
        &self,
        from: &Listening,
        length: Duration,
        hearing: &Hearing,
        accompanied_by: impl FnOnce() -> Result<Option<Playback>>,
    ) -> Result<Clip> {
        let tap = tap_of(from).map_err(|name| Error::NoSuchMicrophone { name: name.clone() })?;
        let sitting = Sitting::begin(length, hearing);

        let opened = Capture::open(&tap, Recorder::admitting(Arc::clone(&sitting.recording)))
            .map_err(|source| Error::capture(CaptureOp::Open, source))?;

        let accompanying = match accompanied_by() {
            Ok(playback) => playback,
            Err(error) => {
                let _ = opened.stop();
                return Err(error);
            }
        };

        let left = sitting.until_full(opened, length, hearing, |recorder| reopened(&tap, recorder));
        let stopped = left
            .map(Capture::stop)
            .transpose()
            .map_err(|source| Error::capture(CaptureOp::Stop, source));
        drop(accompanying);

        finished(
            stopped,
            hearing.stopped(),
            sitting.recording.taken(),
            sitting.spec,
        )
    }
}

fn reopened(tap: &Tap, recorder: Recorder) -> Option<Capture> {
    Capture::open(tap, recorder)
        .inspect_err(|error| tracing::debug!(%error, "the capture cannot open again yet"))
        .ok()
}

fn surveyed() -> Result<Vec<Microphone>> {
    let connect =
        |error| Error::capture(CaptureOp::Connect, backend_error(Operation::Connect, error));
    let _apartment = Apartment::enter();
    let enumerator = DeviceEnumerator::new().map_err(connect)?;
    let default = enumerator
        .get_default_device(Direction::Capture)
        .and_then(|device| device.get_id())
        .ok();
    let collection = enumerator
        .get_device_collection(Direction::Capture)
        .map_err(connect)?;

    let mut endpoints = Vec::new();
    for device in &collection {
        let Ok(device) = device else {
            continue;
        };
        let Ok(id) = device.get_id() else {
            continue;
        };
        endpoints.push(Endpoint {
            friendly_name: device.get_friendlyname().ok(),
            id,
        });
    }
    Ok(microphones_of(endpoints, default.as_deref()))
}
