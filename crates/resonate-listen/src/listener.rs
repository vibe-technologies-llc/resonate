use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::Receiver;
use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_pipewire::{
    CaptureRequest, CaptureStream, Capturing, Microphone, NodeName, PipeWire, StreamEvent,
    StreamState,
};

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
            .capture(
                &request,
                Box::new(Recorder::admitting(Arc::clone(&recording))),
            )
            .map_err(|source| Error::capture(CaptureOp::Open, source))?;

        let capturing = Capture {
            pipewire: &pipewire,
            request: &request,
            recording: &recording,
            channels,
        };
        let left = capturing.until_full(opened, length, hearing);
        let stopped = left
            .map(CaptureStream::stop)
            .transpose()
            .map_err(|source| Error::capture(CaptureOp::Stop, source));
        let _ = pipewire.shutdown();

        finished(stopped, hearing.stopped(), recording.taken(), spec)
    }
}

fn finished(
    stopped: Result<Option<()>>,
    asked_to_stop: bool,
    samples: Vec<f32>,
    spec: StreamSpec,
) -> Result<Clip> {
    if asked_to_stop {
        return Err(Error::Stopped);
    }
    if samples.is_empty() {
        stopped?;
        return Err(Error::NothingHeard);
    }
    if let Err(error) = stopped {
        tracing::warn!(%error, "the capture would not stop cleanly; the clip heard is kept");
    }
    Ok(Clip {
        rate: spec.rate,
        channels: u16::from(spec.channel_count().get()),
        samples,
    })
}

struct Capture<'a> {
    pipewire: &'a PipeWire,
    request: &'a CaptureRequest,
    recording: &'a Arc<Recording>,
    channels: usize,
}

impl Capture<'_> {
    fn until_full(
        &self,
        opened: CaptureStream,
        length: Duration,
        hearing: &Hearing,
    ) -> Option<CaptureStream> {
        let started = Instant::now();
        let deadline = length + GIVES_UP_AFTER_MORE_THAN_ASKED;
        let mut stream = Some(opened);
        let mut lost_since = None;
        let mut filled_at_reopen = None;
        while !self.recording.is_full() && !hearing.stopped() && started.elapsed() < deadline {
            thread::sleep(LOOKS_EVERY);
            hearing.heard.store(
                (self.recording.filled() / self.channels) as u64,
                Ordering::Relaxed,
            );

            if stream
                .as_ref()
                .is_some_and(|stream| was_lost(stream.events()))
            {
                if filled_at_reopen.take() == Some(self.recording.filled()) {
                    tracing::debug!("the capture reopened was lost before it heard anything");
                } else {
                    tracing::warn!("the capture was lost; opening it again");
                }
                stream = None;
                lost_since.get_or_insert_with(Instant::now);
            }
            if stream.is_none() {
                let since = lost_since.unwrap_or_else(Instant::now);
                stream = self.again(since);
                if stream.is_some() {
                    lost_since = None;
                    filled_at_reopen = Some(self.recording.filled());
                }
            }
        }
        stream
    }

    fn again(&self, lost_since: Instant) -> Option<CaptureStream> {
        let (recorder, admitting) = Recorder::held_back(Arc::clone(self.recording));
        let stream = self
            .pipewire
            .capture(self.request, Box::new(recorder))
            .inspect_err(|error| tracing::debug!(%error, "the capture cannot open again yet"))
            .ok()?;
        self.recording
            .silence_for(frames_in(lost_since.elapsed()), self.channels);
        admitting.store(true, Ordering::Release);
        tracing::info!("the capture carries on after a gap of silence");
        Some(stream)
    }
}

fn frames_in(elapsed: Duration) -> usize {
    (elapsed.as_secs_f64() * f64::from(HEARD_AT.hz())).round() as usize
}

fn was_lost(events: &Receiver<StreamEvent>) -> bool {
    let mut failed = false;
    loop {
        match events.try_recv() {
            Ok(StreamEvent::StateChanged {
                to: StreamState::Failed,
                ..
            }) => failed = true,
            Ok(_) => {}
            Err(error) => return failed || error.is_disconnected(),
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

    fn events_after(
        events: &[StreamEvent],
    ) -> (
        Receiver<StreamEvent>,
        crossbeam_channel::Sender<StreamEvent>,
    ) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        for event in events {
            sender.send(event.clone()).expect("an open channel");
        }
        (receiver, sender)
    }

    fn changed(from: StreamState, to: StreamState) -> StreamEvent {
        StreamEvent::StateChanged { from, to }
    }

    #[test]
    fn a_stream_that_failed_is_lost_though_its_channel_is_open() {
        let (events, _kept_open) = events_after(&[
            changed(StreamState::Connecting, StreamState::Streaming),
            changed(StreamState::Streaming, StreamState::Failed),
            StreamEvent::Drained,
        ]);

        assert!(was_lost(&events));
    }

    #[test]
    fn a_stream_that_streams_and_pauses_is_not_lost() {
        let (events, _kept_open) = events_after(&[
            changed(StreamState::Connecting, StreamState::Streaming),
            changed(StreamState::Streaming, StreamState::Paused),
        ]);

        assert!(!was_lost(&events));
    }

    #[test]
    fn a_stream_whose_channel_closed_is_lost() {
        let (events, kept_open) = events_after(&[]);
        assert!(!was_lost(&events));

        drop(kept_open);
        assert!(was_lost(&events));
    }

    fn stereo() -> StreamSpec {
        StreamSpec::new(HEARD_AT, HEARD_IN, SampleFormat::F32)
    }

    #[test]
    fn a_clip_survives_a_stop_that_failed() {
        let failed = Err(Error::capture(
            CaptureOp::Stop,
            resonate_pipewire::Error::Disconnected,
        ));

        let clip = finished(failed, false, vec![0.5, -0.5], stereo()).expect("the clip heard");
        assert_eq!(clip.samples, vec![0.5, -0.5]);
        assert_eq!(clip.channels, 2);
    }

    #[test]
    fn a_stop_that_failed_with_nothing_heard_is_the_error_it_was() {
        let failed = Err(Error::capture(
            CaptureOp::Stop,
            resonate_pipewire::Error::Disconnected,
        ));

        assert!(matches!(
            finished(failed, false, Vec::new(), stereo()),
            Err(Error::Capture {
                op: CaptureOp::Stop,
                ..
            })
        ));
        assert!(matches!(
            finished(Ok(Some(())), false, Vec::new(), stereo()),
            Err(Error::NothingHeard)
        ));
    }

    #[test]
    fn a_listener_who_stopped_it_hears_no_clip() {
        assert!(matches!(
            finished(Ok(Some(())), true, vec![0.5, 0.5], stereo()),
            Err(Error::Stopped)
        ));
    }

    #[test]
    fn a_gap_counts_the_frames_of_the_time_that_passed() {
        assert_eq!(frames_in(Duration::from_millis(500)), 24_000);
        assert_eq!(frames_in(Duration::ZERO), 0);
    }
}
