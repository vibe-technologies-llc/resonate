use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_pipewire::{CaptureRequest, CaptureStream, Capturing, Microphone, NodeName, PipeWire};

use crate::{
    CaptureOp, Clip, Error, Result,
    recording::{Recorder, Recording},
};

const HEARD_AT: SampleRate = SampleRate::HZ_48000;
const HEARD_IN: ChannelLayout = ChannelLayout::Stereo;
const LOOKS_EVERY: Duration = Duration::from_millis(40);
const GIVES_UP_AFTER_MORE_THAN_ASKED: Duration = Duration::from_secs(4);
const ENUMERATES_WITHIN: Duration = Duration::from_secs(2);
const STREAM_NAME: &str = "Listening";
pub const CLIP_BY_DEFAULT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Listening {
    #[default]
    Desktop,
    Microphone(Option<NodeName>),
}

const DESKTOP: &str = "desktop";
const ANY_MICROPHONE: &str = "microphone";

impl Listening {
    pub fn named(text: &str) -> Self {
        match text.trim() {
            "" | DESKTOP => Self::Desktop,
            ANY_MICROPHONE => Self::Microphone(None),
            node => Self::Microphone(Some(NodeName::new(node.to_owned()))),
        }
    }

    pub fn written(&self) -> String {
        match self {
            Self::Desktop => DESKTOP.to_owned(),
            Self::Microphone(None) => ANY_MICROPHONE.to_owned(),
            Self::Microphone(Some(node)) => node.to_string(),
        }
    }

    pub const fn is_a_microphone(&self) -> bool {
        matches!(self, Self::Microphone(_))
    }
}

#[derive(Debug, Default)]
pub struct Hearing {
    heard: AtomicU64,
    wanted: AtomicU64,
    stopped: AtomicBool,
}

impl Hearing {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    pub fn share(&self) -> f32 {
        let wanted = self.wanted.load(Ordering::Relaxed);
        if wanted == 0 {
            return 0.0;
        }
        (self.heard.load(Ordering::Relaxed) as f64 / wanted as f64).min(1.0) as f32
    }
}

#[derive(Clone, Debug)]
pub struct Listener {
    app_name: String,
}

impl Listener {
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_owned(),
        }
    }

    pub fn microphones(&self) -> Result<Vec<Microphone>> {
        let pipewire = PipeWire::start(&self.app_name)
            .map_err(|source| Error::capture(CaptureOp::Connect, source))?;
        let found = pipewire
            .microphones(ENUMERATES_WITHIN)
            .map_err(|source| Error::capture(CaptureOp::Connect, source));
        let _ = pipewire.shutdown();
        found
    }

    pub fn record(&self, from: &Listening, length: Duration, hearing: &Hearing) -> Result<Clip> {
        let spec = StreamSpec::new(HEARD_AT, HEARD_IN, SampleFormat::F32);
        let channels = usize::from(spec.channel_count().get());
        let frames = (length.as_secs_f64() * f64::from(HEARD_AT.hz())).round() as usize;
        hearing.wanted.store(frames as u64, Ordering::Relaxed);
        hearing.heard.store(0, Ordering::Relaxed);

        let pipewire = PipeWire::start(&self.app_name)
            .map_err(|source| Error::capture(CaptureOp::Connect, source))?;
        let recording = Recording::holding(frames * channels);
        let request = CaptureRequest {
            from: match from {
                Listening::Desktop => Capturing::Desktop { sink: None },
                Listening::Microphone(source) => Capturing::Microphone {
                    source: source.clone(),
                },
            },
            spec,
            media_name: STREAM_NAME.to_owned(),
        };
        let opened = pipewire
            .capture(&request, Box::new(Recorder(Arc::clone(&recording))))
            .map_err(|source| Error::capture(CaptureOp::Open, source))?;

        let capturing = Capture {
            pipewire: &pipewire,
            request: &request,
            recording: &recording,
        };
        let left = capturing.until_full(opened, length, channels, hearing);
        let stopped = left
            .map(CaptureStream::stop)
            .transpose()
            .map_err(|source| Error::capture(CaptureOp::Stop, source));
        let _ = pipewire.shutdown();
        stopped?;

        if hearing.stopped() {
            return Err(Error::Stopped);
        }
        let samples = recording.taken();
        if samples.is_empty() {
            return Err(Error::NothingHeard);
        }
        Ok(Clip {
            rate: HEARD_AT,
            channels: u16::from(spec.channel_count().get()),
            samples,
        })
    }
}

struct Capture<'a> {
    pipewire: &'a PipeWire,
    request: &'a CaptureRequest,
    recording: &'a Arc<Recording>,
}

impl Capture<'_> {
    fn until_full(
        &self,
        opened: CaptureStream,
        length: Duration,
        channels: usize,
        hearing: &Hearing,
    ) -> Option<CaptureStream> {
        let started = Instant::now();
        let mut deadline = length + GIVES_UP_AFTER_MORE_THAN_ASKED;
        let mut stream = Some(opened);
        let mut lost_since = None;
        while !self.recording.is_full() && !hearing.stopped() && started.elapsed() < deadline {
            thread::sleep(LOOKS_EVERY);
            hearing.heard.store(
                (self.recording.filled() / channels) as u64,
                Ordering::Relaxed,
            );

            if stream.as_ref().is_some_and(was_lost) {
                tracing::warn!("the capture went with the PipeWire daemon; opening it again");
                stream = None;
                lost_since = Some(Instant::now());
            }
            if stream.is_none() {
                stream = self.again();
                if stream.is_some()
                    && let Some(since) = lost_since.take()
                {
                    deadline += since.elapsed();
                }
            }
        }
        stream
    }

    fn again(&self) -> Option<CaptureStream> {
        self.pipewire
            .capture(self.request, Box::new(Recorder(Arc::clone(self.recording))))
            .inspect(|_| tracing::info!("the capture carries on where it was lost"))
            .inspect_err(|error| tracing::debug!(%error, "the capture cannot open again yet"))
            .ok()
    }
}

fn was_lost(stream: &CaptureStream) -> bool {
    loop {
        match stream.events().try_recv() {
            Ok(_) => {}
            Err(error) => return error.is_disconnected(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_to_listen_reads_back_what_it_was_written_as() {
        for listening in [
            Listening::Desktop,
            Listening::Microphone(None),
            Listening::Microphone(Some(NodeName::new("alsa_input.usb-mono".to_owned()))),
        ] {
            assert_eq!(Listening::named(&listening.written()), listening);
        }
        assert_eq!(Listening::named(""), Listening::Desktop);
    }
}
