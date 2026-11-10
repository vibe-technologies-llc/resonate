use std::{
    io,
    num::NonZeroU32,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

use resonate_apple::{Component, Device, Format, HostTime, Render, Shape, Unit, Word};
use resonate_audio::{AudioSource, Error as AudioError, NodeName, Operation};

use crate::{
    Error, Result,
    coreaudio_capture::backend_error,
    session::{HEARD_AT, Played},
};

const OPENED_WITHIN: Duration = Duration::from_secs(5);
const STEREO: NonZeroU32 = NonZeroU32::MIN.saturating_add(1);

struct Sounding(Played);

impl Render for Sounding {
    fn render(&mut self, bytes: &mut [u8], _: Option<HostTime>) {
        let filled = self.0.fill(bytes);
        if let Some(rest) = bytes.get_mut(filled..) {
            rest.fill(0);
        }
    }
}

pub(crate) struct Playback {
    stopping: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

fn output_named(uid: Option<&str>) -> Option<Device> {
    match uid {
        None => Device::default_output().ok().flatten(),
        Some(uid) => Device::all()
            .ok()?
            .into_iter()
            .find(|device| device.uid().is_ok_and(|held| held == uid)),
    }
}

fn sounding(
    device: Device,
    played: Played,
    ready: &Sender<std::result::Result<(), AudioError>>,
    stopped: &Receiver<()>,
) {
    let shape = Shape {
        component: Component::HalOutput,
        device: Some(device.id()),
        format: Format::new(f64::from(HEARD_AT.hz()), STEREO, Word::Float32),
    };
    let unit = Unit::playing(shape, Box::new(Sounding(played)))
        .map_err(|error| backend_error(Operation::Open, error))
        .and_then(|unit| match unit.start() {
            Ok(()) => Ok(unit),
            Err(status) => Err(AudioError::Backend {
                op: Operation::Start,
                source: io::Error::other(status),
            }),
        });
    match unit {
        Ok(unit) => {
            let _ = ready.send(Ok(()));
            let _ = stopped.recv();
            let _ = unit.stop();
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
}

impl Playback {
    pub(crate) fn open(
        through: Option<&str>,
        named: Option<&NodeName>,
        played: Played,
    ) -> Result<Self> {
        let device = output_named(through).ok_or_else(|| match named {
            Some(name) => Error::NoSuchSink { name: name.clone() },
            None => Error::NoOutput,
        })?;
        let output = |source: AudioError| Error::Output {
            source: Box::new(source),
        };
        let (stopping, stopped) = mpsc::channel();
        let (ready, opened) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("resonate-listen-playback".into())
            .spawn(move || sounding(device, played, &ready, &stopped))
            .map_err(|source| {
                output(AudioError::Backend {
                    op: Operation::Open,
                    source,
                })
            })?;
        match opened.recv_timeout(OPENED_WITHIN) {
            Ok(Ok(())) => Ok(Self {
                stopping,
                thread: Some(thread),
            }),
            Ok(Err(error)) => Err(output(error)),
            Err(RecvTimeoutError::Timeout) => Err(output(AudioError::OpenUnanswered)),
            Err(RecvTimeoutError::Disconnected) => Err(output(AudioError::LoopStopped)),
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.stopping.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
