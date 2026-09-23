use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_pipewire::{
    AudioSource, Error, LatencyRequest, MediaRole, PipeWire, SinkInfo, StreamEvent, StreamRequest,
    StreamState,
};

const DISCOVERY: Duration = Duration::from_secs(2);
const PATIENCE: Duration = Duration::from_secs(5);
const SAME_EVERY_RATE: Duration = Duration::from_millis(20);

#[derive(Default)]
struct Pulled {
    frames: AtomicU64,
    calls: AtomicU64,
    ragged: AtomicBool,
    underruns: AtomicU64,
}

struct Silence {
    bytes_per_frame: usize,
    pulled: Arc<Pulled>,
}

impl AudioSource for Silence {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        if !dst.len().is_multiple_of(self.bytes_per_frame) {
            self.pulled.ragged.store(true, Ordering::Relaxed);
        }
        dst.fill(0);

        self.pulled.calls.fetch_add(1, Ordering::Relaxed);
        self.pulled
            .frames
            .fetch_add((dst.len() / self.bytes_per_frame) as u64, Ordering::Relaxed);
        dst.len()
    }

    fn on_underrun(&mut self, _missing_bytes: usize) {
        self.pulled.underruns.fetch_add(1, Ordering::Relaxed);
    }
}

fn daemon() -> Option<(PipeWire, SinkInfo)> {
    let pipewire = match PipeWire::start("resonate-tests") {
        Ok(pipewire) => pipewire,
        Err(error) => {
            eprintln!("skipped: no PipeWire daemon to talk to ({error})");
            return None;
        }
    };

    match pipewire.default_sink(DISCOVERY) {
        Ok(Some(sink)) => Some((pipewire, sink)),
        Ok(None) | Err(Error::NoSink) => {
            eprintln!("skipped: the daemon advertises no sink");
            None
        }
        Err(error) => panic!("the daemon would not enumerate its sinks: {error}"),
    }
}

fn request(sink: &SinkInfo, spec: StreamSpec) -> StreamRequest {
    StreamRequest {
        target: Some(sink.id),
        spec,
        latency: LatencyRequest::Frames(1_024),
        role: MediaRole::Music,
        media_name: "resonate stream test".to_owned(),
        force_graph_rate: false,
        no_convert: false,
        exclusive: false,
        realtime: true,
    }
}

#[test]
fn a_source_with_no_engine_behind_it_is_pulled_whole_frames_at_a_time() {
    let Some((pipewire, sink)) = daemon() else {
        return;
    };

    let spec = StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Stereo,
        SampleFormat::F32,
    );
    let pulled = Arc::new(Pulled::default());
    let source = Silence {
        bytes_per_frame: spec.bytes_per_frame().get() as usize,
        pulled: Arc::clone(&pulled),
    };

    let stream = pipewire
        .open(&request(&sink, spec), Box::new(source))
        .expect("the daemon accepts a stereo float stream");
    stream.set_active(true).expect("the stream activates");

    let deadline = Instant::now() + PATIENCE;
    while pulled.frames.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    let frames = pulled.frames.load(Ordering::Relaxed);
    let announced: Vec<StreamEvent> = stream.events().try_iter().collect();
    let state = announced.iter().rev().find_map(|event| match event {
        StreamEvent::StateChanged { to, .. } => Some(*to),
        _ => None,
    });
    let negotiated = announced.iter().find_map(|event| match event {
        StreamEvent::FormatChanged(spec) => Some(*spec),
        _ => None,
    });
    stream.close().expect("the stream closes");
    pipewire.shutdown().expect("the loop shuts down");

    assert!(frames > 0, "the graph never pulled the source");
    assert!(
        pulled.calls.load(Ordering::Relaxed) > 0,
        "frames arrived without a single callback"
    );
    assert!(
        !pulled.ragged.load(Ordering::Relaxed),
        "the graph asked for a partial frame"
    );
    assert_eq!(state, Some(StreamState::Streaming));
    assert_eq!(
        negotiated.map(|spec| spec.channels),
        Some(ChannelLayout::Stereo),
        "the negotiated format was never announced"
    );
    assert_eq!(
        pulled.underruns.load(Ordering::Relaxed),
        0,
        "a source that filled every buffer was told it had underrun"
    );
}

#[test]
fn deactivating_a_stream_stops_the_graph_pulling_its_source() {
    let Some((pipewire, sink)) = daemon() else {
        return;
    };

    let spec = StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Stereo,
        SampleFormat::F32,
    );
    let pulled = Arc::new(Pulled::default());
    let source = Silence {
        bytes_per_frame: spec.bytes_per_frame().get() as usize,
        pulled: Arc::clone(&pulled),
    };

    let stream = pipewire
        .open(&request(&sink, spec), Box::new(source))
        .expect("the daemon accepts a stereo float stream");
    stream.set_active(true).expect("the stream activates");

    let deadline = Instant::now() + PATIENCE;
    while pulled.calls.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    stream.set_active(false).expect("the stream deactivates");
    thread::sleep(Duration::from_millis(200));

    let settled = pulled.calls.load(Ordering::Relaxed);
    thread::sleep(Duration::from_millis(200));
    let after = pulled.calls.load(Ordering::Relaxed);

    stream.close().expect("the stream closes");
    pipewire.shutdown().expect("the loop shuts down");

    assert!(settled > 0, "the graph never pulled the source");
    assert_eq!(after, settled, "a deactivated stream was still pulled");
}

#[test]
fn a_stream_asked_to_drain_reports_when_the_graph_has_played_it_out() {
    let Some((pipewire, sink)) = daemon() else {
        return;
    };

    let spec = StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Stereo,
        SampleFormat::F32,
    );
    let pulled = Arc::new(Pulled::default());
    let source = Silence {
        bytes_per_frame: spec.bytes_per_frame().get() as usize,
        pulled: Arc::clone(&pulled),
    };

    let stream = pipewire
        .open(&request(&sink, spec), Box::new(source))
        .expect("the daemon accepts a stereo float stream");
    stream.set_active(true).expect("the stream activates");

    let deadline = Instant::now() + PATIENCE;
    while pulled.calls.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    stream.drain().expect("the daemon takes a drain request");

    let mut drained = false;
    let deadline = Instant::now() + PATIENCE;
    while !drained && Instant::now() < deadline {
        drained = stream
            .events()
            .try_iter()
            .any(|event| event == StreamEvent::Drained);
        thread::sleep(Duration::from_millis(10));
    }

    stream.close().expect("the stream closes");
    pipewire.shutdown().expect("the loop shuts down");

    assert!(drained, "the graph never said it had played the stream out");
}

fn measured_latency(pipewire: &PipeWire, sink: &SinkInfo, rate: SampleRate) -> Option<u64> {
    let spec = StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32);
    let pulled = Arc::new(Pulled::default());
    let source = Silence {
        bytes_per_frame: spec.bytes_per_frame().get() as usize,
        pulled: Arc::clone(&pulled),
    };
    let asked = StreamRequest {
        latency: LatencyRequest::Duration(SAME_EVERY_RATE),
        ..request(sink, spec)
    };

    let stream = pipewire
        .open(&asked, Box::new(source))
        .expect("the daemon accepts a stereo float stream");
    stream.set_active(true).expect("the stream activates");

    let deadline = Instant::now() + PATIENCE;
    let mut ahead = 0;
    while ahead == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
        ahead = stream.latency().get();
    }

    stream.close().expect("the stream closes");
    (ahead > 0).then_some(ahead)
}

#[test]
fn the_latency_a_stream_reports_is_counted_in_its_own_frames_not_the_graphs() {
    let Some((pipewire, sink)) = daemon() else {
        return;
    };

    let doubled = measured_latency(&pipewire, &sink, SampleRate::HZ_96000);
    let base = measured_latency(&pipewire, &sink, SampleRate::HZ_48000);
    pipewire.shutdown().expect("the loop shuts down");

    let (Some(doubled), Some(base)) = (doubled, base) else {
        eprintln!("skipped: the daemon reports no delay to a device");
        return;
    };

    assert!(
        doubled > base * 3 / 2,
        "a 96 kHz stream reported {doubled} frames of latency against a 48 kHz stream's {base}, \
         which is the graph's own tick count rather than either stream's frames"
    );
}
