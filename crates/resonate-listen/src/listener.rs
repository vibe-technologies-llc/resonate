use std::{sync::Arc, time::Duration};

use crossbeam_channel::Receiver;
use resonate_core::{SampleFormat, StreamSpec};
use resonate_pipewire::{
    CaptureRequest, CaptureStream, Capturing, LatencyRequest, MediaRole, Microphone, NodeName,
    PipeWire, SinkStream, StreamEvent, StreamRequest, StreamState,
};

use crate::{
    CaptureOp, Clip, Error, Hearing, Listening, Result,
    recording::Recorder,
    session::{HEARD_AT, HEARD_IN, Held, Played, Sitting, finished},
};

const ENUMERATES_WITHIN: Duration = Duration::from_secs(2);
const STREAM_NAME: &str = "Listening";
const PLAYING_NAME: &str = "Measuring";

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
        self.recorded(from, length, hearing, |_| Ok(None))
    }

    pub fn play_and_record(
        &self,
        played: &[f32],
        through: Option<&NodeName>,
        from: &Listening,
        length: Duration,
        hearing: &Hearing,
    ) -> Result<Clip> {
        let playing = Played::of(played);
        self.recorded(from, length, hearing, |pipewire| {
            let sink = match through {
                Some(name) => pipewire
                    .enumerate_sinks(ENUMERATES_WITHIN)
                    .map_err(|source| Error::capture(CaptureOp::Connect, source))?
                    .into_iter()
                    .find(|sink| &sink.name == name)
                    .map(|sink| sink.id)
                    .ok_or_else(|| Error::NoSuchSink { name: name.clone() })?,
                None => pipewire
                    .default_sink(ENUMERATES_WITHIN)
                    .map_err(|source| Error::capture(CaptureOp::Connect, source))?
                    .map(|sink| sink.id)
                    .ok_or(Error::NoOutput)?,
            };
            let request = StreamRequest {
                target: Some(sink),
                spec: StreamSpec::new(HEARD_AT, HEARD_IN, SampleFormat::F32),
                latency: LatencyRequest::Auto,
                role: MediaRole::Music,
                media_name: PLAYING_NAME.to_owned(),
                force_graph_rate: false,
                no_convert: false,
                realtime: true,
            };
            let stream = pipewire
                .open(&request, Box::new(playing))
                .map_err(|source| Error::capture(CaptureOp::Play, source))?;
            stream.set_active(true).map_err(|source| Error::Output {
                source: Box::new(source),
            })?;
            Ok(Some(stream))
        })
    }

    fn recorded(
        &self,
        from: &Listening,
        length: Duration,
        hearing: &Hearing,
        accompanied_by: impl FnOnce(&PipeWire) -> Result<Option<SinkStream>>,
    ) -> Result<Clip> {
        let sitting = Sitting::begin(length, hearing);

        let pipewire = PipeWire::start(&self.app_name)
            .map_err(|source| Error::capture(CaptureOp::Connect, source))?;
        let request = CaptureRequest {
            from: match from {
                Listening::Desktop => Capturing::Desktop { sink: None },
                Listening::Microphone(source) => Capturing::Microphone {
                    source: source.clone(),
                },
            },
            spec: sitting.spec,
            media_name: STREAM_NAME.to_owned(),
        };
        let opened = pipewire
            .capture(
                &request,
                Box::new(Recorder::admitting(Arc::clone(&sitting.recording))),
            )
            .map_err(|source| Error::capture(CaptureOp::Open, source))?;

        let accompanying = match accompanied_by(&pipewire) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = opened.stop();
                let _ = pipewire.shutdown();
                return Err(error);
            }
        };

        let left = sitting.until_full(opened, length, hearing, |recorder| {
            pipewire
                .capture(&request, Box::new(recorder))
                .inspect_err(|error| tracing::debug!(%error, "the capture cannot open again yet"))
                .ok()
        });
        let stopped = left
            .map(CaptureStream::stop)
            .transpose()
            .map_err(|source| Error::capture(CaptureOp::Stop, source));
        drop(accompanying);
        let _ = pipewire.shutdown();

        finished(
            stopped,
            hearing.stopped(),
            sitting.recording.taken(),
            sitting.spec,
        )
    }
}

impl Held for CaptureStream {
    fn is_lost(&self) -> bool {
        was_lost(self.events())
    }
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
}
