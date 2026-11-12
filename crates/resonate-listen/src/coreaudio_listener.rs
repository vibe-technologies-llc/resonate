use std::{sync::Arc, time::Duration};

use resonate_apple::{DesktopTap, Device};
use resonate_audio::{Error as AudioError, NodeName, Operation};

use crate::{
    CaptureOp, Clip, Error, Hearing, Listening, Result,
    coreaudio_capture::{Capture, backend_error, hears, microphone_of},
    coreaudio_names::{Source, capture_node_of, output_of, source_of},
    coreaudio_playback::Playback,
    microphone::Microphone,
    recording::Recorder,
    session::{Played, Sitting, finished},
};

#[derive(Clone, Debug)]
pub struct Listener;

fn untapped(refused: resonate_apple::Error) -> Error {
    match refused {
        resonate_apple::Error::NoProcessTap => Error::DesktopUnheard,
        refused => Error::capture(CaptureOp::Open, backend_error(Operation::Open, refused)),
    }
}

fn tapped_where_asked(source: Source<'_>) -> Result<Option<DesktopTap>> {
    match source {
        Source::TheDesktop => DesktopTap::open().map(Some).map_err(untapped),
        Source::Microphone(_) => Ok(None),
    }
}

fn device_heard(
    source: Source<'_>,
    tap: Option<&DesktopTap>,
) -> std::result::Result<Device, AudioError> {
    match (source, tap) {
        (_, Some(tap)) => Ok(tap.device()),
        (Source::Microphone(uid), None) => microphone_of(uid),
        (Source::TheDesktop, None) => Err(AudioError::NoSink),
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
        let source =
            source_of(from).map_err(|name| Error::NoSuchMicrophone { name: name.clone() })?;
        let tap = tapped_where_asked(source)?;
        let sitting = Sitting::begin(length, hearing);

        let opened = device_heard(source, tap.as_ref())
            .and_then(|device| {
                Capture::open(device, Recorder::admitting(Arc::clone(&sitting.recording)))
            })
            .map_err(|source| Error::capture(CaptureOp::Open, source))?;

        let accompanying = match accompanied_by() {
            Ok(playback) => playback,
            Err(error) => {
                let _ = opened.stop();
                return Err(error);
            }
        };

        let left = sitting.until_full(opened, length, hearing, |recorder| {
            device_heard(source, tap.as_ref())
                .and_then(|device| Capture::open(device, recorder))
                .inspect_err(|error| tracing::debug!(%error, "the capture cannot open again yet"))
                .ok()
        });
        let stopped = left
            .map(Capture::stop)
            .transpose()
            .map_err(|source| Error::capture(CaptureOp::Stop, source));
        drop(accompanying);
        drop(tap);

        finished(
            stopped,
            hearing.stopped(),
            sitting.recording.taken(),
            sitting.spec,
        )
    }
}
