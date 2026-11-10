use std::{sync::Arc, time::Duration};

use resonate_apple::Device;
use resonate_audio::{NodeName, Operation};

use crate::{
    CaptureOp, Clip, Error, Hearing, Listening, Result,
    coreaudio_capture::{Capture, backend_error, hears},
    coreaudio_names::{Unheard, capture_node_of, microphone_of, output_of},
    coreaudio_playback::Playback,
    microphone::Microphone,
    recording::Recorder,
    session::{Played, Sitting, finished},
};

#[derive(Clone, Debug)]
pub struct Listener;

fn unheard(refused: Unheard<'_>) -> Error {
    match refused {
        Unheard::TheDesktop => Error::DesktopUnheard,
        Unheard::NoSuchMicrophone(name) => Error::NoSuchMicrophone { name: name.clone() },
    }
}

impl Listener {
    pub fn new(_: &str) -> Self {
        Self
    }

    pub fn microphones(&self) -> Result<Vec<Microphone>> {
        let connect =
            |error| Error::capture(CaptureOp::Connect, backend_error(Operation::Connect, error));
        let default = Device::default_input()
            .ok()
            .flatten()
            .and_then(|device| device.uid().ok());
        Ok(Device::all()
            .map_err(connect)?
            .into_iter()
            .filter(|device| hears(*device))
            .filter_map(|device| {
                let uid = device.uid().ok()?;
                Some(Microphone {
                    name: capture_node_of(&uid),
                    is_default: default.as_deref() == Some(uid.as_str()),
                    description: device.name().unwrap_or(uid),
                })
            })
            .collect())
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
            Playback::open(output, through, played).map(Some)
        })
    }

    fn recorded(
        &self,
        from: &Listening,
        length: Duration,
        hearing: &Hearing,
        accompanied_by: impl FnOnce() -> Result<Option<Playback>>,
    ) -> Result<Clip> {
        let microphone = microphone_of(from).map_err(unheard)?;
        let sitting = Sitting::begin(length, hearing);

        let opened = Capture::open(
            microphone,
            Recorder::admitting(Arc::clone(&sitting.recording)),
        )
        .map_err(|source| Error::capture(CaptureOp::Open, source))?;

        let accompanying = match accompanied_by() {
            Ok(playback) => playback,
            Err(error) => {
                let _ = opened.stop();
                return Err(error);
            }
        };

        let left = sitting.until_full(opened, length, hearing, |recorder| {
            Capture::open(microphone, recorder)
                .inspect_err(|error| tracing::debug!(%error, "the capture cannot open again yet"))
                .ok()
        });
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
