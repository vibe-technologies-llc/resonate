use std::{
    io,
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use resonate_apple::{Component, Device, Direction, Format, Hear, Shape, Unit, Word};
use resonate_audio::{Error as AudioError, Operation};
use resonate_core::SampleRate;

use crate::{
    coreaudio_conform::Conforming,
    recording::{Recorder, Recording},
    session::{HEARD_AT, HEARD_IN_CHANNELS, Held},
};

const OPENED_WITHIN: Duration = Duration::from_secs(5);
const CONFORMED_EVERY: Duration = Duration::from_millis(40);
const FRAMES_A_SLICE_AT_MOST: NonZeroU32 = NonZeroU32::MIN.saturating_add(8_191);
const HEARD_FOR_LONGER_THAN_ASKED_SECONDS: usize = 2;

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

struct Heard {
    device: Device,
    rate: SampleRate,
    channels: NonZeroU32,
}

impl Heard {
    fn from(device: Device) -> Result<Self, AudioError> {
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
        Ok(Self {
            device,
            rate: rate.ok_or(AudioError::FormatRefused)?,
            channels: NonZeroU32::new(channels).ok_or(AudioError::FormatRefused)?,
        })
    }

    fn channels(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.channels.get() as usize).unwrap_or(NonZeroUsize::MIN)
    }

    fn raw_samples_for(&self, clip_samples: usize) -> usize {
        let clip_frames = clip_samples / HEARD_IN_CHANNELS;
        let device_hz = self.rate.hz() as usize;
        let frames = clip_frames.saturating_mul(device_hz) / HEARD_AT.hz() as usize;
        frames
            .saturating_add(device_hz.saturating_mul(HEARD_FOR_LONGER_THAN_ASKED_SECONDS))
            .saturating_mul(self.channels().get())
    }
}

struct Raw(Recorder);

impl Hear for Raw {
    fn hear(&mut self, bytes: &[u8]) {
        self.0.admit(bytes);
    }
}

pub(crate) struct Capture {
    stopping: Sender<()>,
    lost: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Carrying {
    raw: Arc<Recording>,
    carried: usize,
    conforming: Conforming,
    recorder: Recorder,
}

impl Carrying {
    fn carry(&mut self) {
        let heard = self.raw.heard_since(self.carried);
        self.carried += heard.len();
        if !heard.is_empty() {
            self.recorder.admit(&self.conforming.conformed(&heard));
        }
    }
}

fn hearing(
    heard: &Heard,
    recorder: Recorder,
    ready: &Sender<Result<(), AudioError>>,
    stopped: &Receiver<()>,
    lost: &AtomicBool,
) {
    let open = |error| backend_error(Operation::Open, error);
    let raw = Recording::holding(heard.raw_samples_for(recorder.room()));
    let conforming = match Conforming::of(heard.rate, heard.channels()) {
        Ok(conforming) => conforming,
        Err(_) => {
            let _ = ready.send(Err(AudioError::FormatRefused));
            return;
        }
    };
    let shape = Shape {
        component: Component::HalOutput,
        device: Some(heard.device.id()),
        format: Format::new(f64::from(heard.rate.hz()), heard.channels, Word::Float32),
    };
    let unit = Unit::hearing(
        shape,
        Box::new(Raw(Recorder::admitting(Arc::clone(&raw)))),
        FRAMES_A_SLICE_AT_MOST,
    )
    .map_err(open)
    .and_then(|unit| {
        unit.start()
            .map_err(|status| AudioError::Backend {
                op: Operation::Start,
                source: io::Error::other(status),
            })
            .map(|()| unit)
    });
    let unit = match unit {
        Ok(unit) => unit,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut carrying = Carrying {
        raw,
        carried: 0,
        conforming,
        recorder,
    };
    while stopped.recv_timeout(CONFORMED_EVERY) == Err(RecvTimeoutError::Timeout) {
        carrying.carry();
        if !heard.device.is_alive() {
            lost.store(true, Ordering::Release);
        }
    }
    let _ = unit.stop();
    carrying.carry();
}

impl Capture {
    pub(crate) fn open(device: Device, recorder: Recorder) -> Result<Self, AudioError> {
        let heard = Heard::from(device)?;
        let (stopping, stopped) = mpsc::channel();
        let (ready, opened) = mpsc::channel();
        let lost = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("resonate-listen-capture".into())
            .spawn({
                let lost = Arc::clone(&lost);
                move || hearing(&heard, recorder, &ready, &stopped, &lost)
            })
            .map_err(|source| AudioError::Backend {
                op: Operation::Open,
                source,
            })?;
        match opened.recv_timeout(OPENED_WITHIN) {
            Ok(Ok(())) => Ok(Self {
                stopping,
                lost,
                thread: Some(thread),
            }),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AudioError::OpenUnanswered),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::LoopStopped),
        }
    }

    pub(crate) fn stop(mut self) -> Result<(), AudioError> {
        let _ = self.stopping.send(());
        match self.thread.take().map(JoinHandle::join) {
            Some(Err(_)) => Err(AudioError::LoopStopped),
            _ => Ok(()),
        }
    }
}

impl Held for Capture {
    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}
