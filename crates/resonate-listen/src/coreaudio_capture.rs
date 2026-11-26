use std::{io, num::NonZeroU32};

use resonate_apple::{Component, Device, Direction, Format, Hear, Shape, Unit, Word};
use resonate_audio::{Error as AudioError, Operation};
use resonate_core::SampleRate;

use crate::{
    coreaudio_carry::{Capture, Heard, Opened},
    recording::Recorder,
};

const FRAMES_A_SLICE_AT_MOST: NonZeroU32 = NonZeroU32::MIN.saturating_add(8_191);

pub(crate) fn backend_error(op: Operation, source: resonate_apple::Error) -> AudioError {
    AudioError::Backend {
        op,
        source: io::Error::other(source),
    }
}

pub(crate) fn hears(device: Device) -> bool {
    device
        .streams(Direction::Input)
        .is_ok_and(|streams| !streams.is_empty())
}

pub(crate) fn microphone_of(uid: Option<&str>) -> Result<Device, AudioError> {
    let connect = |error| backend_error(Operation::Connect, error);
    match uid {
        None => Device::default_input()
            .map_err(connect)?
            .ok_or(AudioError::NoSink),
        Some(uid) => Device::all()
            .map_err(connect)?
            .into_iter()
            .find(|device| hears(*device) && device.uid().is_ok_and(|held| held == uid))
            .ok_or(AudioError::NoSink),
    }
}

fn heard_by(device: Device) -> Result<Heard, AudioError> {
    let open = |error| backend_error(Operation::Open, error);
    let stream = device
        .streams(Direction::Input)
        .map_err(open)?
        .into_iter()
        .next()
        .ok_or(AudioError::NoSink)?;

    let channels = stream.physical_format().map_err(open)?.channels();
    let hz = device.nominal_rate().map_err(open)?;
    let rate = (hz.is_finite() && hz >= 1.0)
        .then(|| SampleRate::new(hz.round() as u32).ok())
        .flatten();

    Ok(Heard::kept_of(
        rate.ok_or(AudioError::FormatRefused)?,
        NonZeroU32::new(channels).ok_or(AudioError::FormatRefused)?,
    ))
}

struct Raw(Recorder);

impl Hear for Raw {
    fn hear(&mut self, bytes: &[u8]) {
        self.0.admit(bytes);
    }

    fn dropped(&mut self, frames: u32) {
        self.0.note_dropped(u64::from(frames));
    }
}

struct Running {
    unit: Unit,
    device: Device,
}

impl Opened for Running {
    fn is_alive(&self) -> bool {
        self.device.is_alive()
    }

    fn stop(&self) {
        let _ = self.unit.stop();
    }
}

impl Capture {
    pub(crate) fn open(device: Device, recorder: Recorder) -> Result<Self, AudioError> {
        let heard = heard_by(device)?;
        let shape = Shape {
            component: Component::HalOutput,
            device: Some(device.id()),
            format: Format::new(f64::from(heard.rate.hz()), heard.channels, Word::Float32),
        };

        Self::spawn(heard, recorder, move |raw| {
            let open = |error| backend_error(Operation::Open, error);
            let unit =
                Unit::hearing(shape, Box::new(Raw(raw)), FRAMES_A_SLICE_AT_MOST).map_err(open)?;

            unit.start().map_err(|status| AudioError::Backend {
                op: Operation::Start,
                source: io::Error::other(status),
            })?;
            Ok(Running { unit, device })
        })
    }
}
