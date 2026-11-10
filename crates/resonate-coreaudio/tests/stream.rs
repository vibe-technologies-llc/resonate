#![cfg(target_os = "macos")]

use std::{
    env,
    f32::consts::TAU,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_apple::{Component, Device, Direction, Format, Hear, Shape, Unit, Word};
use resonate_audio::{
    AudioSource, Backend, LatencyRequest, MediaRole, Moment, SinkInfo, SinkStream, Stamp,
    StreamEvent, StreamRequest, StreamState,
};
use resonate_core::{ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec};
use resonate_coreaudio::CoreAudio;

const WAIT: Duration = Duration::from_secs(10);
const A_LOOPBACK_DEVICE_IS_NAMED: &str = "BlackHole";
const REQUIRED_BY: &str = "RESONATE_COREAUDIO_TESTS";
const TONE_HZ: f32 = 997.0;
const FRAME_BYTES: usize = 8;
const RAMP_FRAMES: usize = 48_000;
const RAMP_COMPARED_FRAMES: usize = 4_096;
const TWENTY_FOUR_BITS: f32 = 8_388_608.0;

fn passed_over(why: &str) {
    assert!(
        env::var(REQUIRED_BY).as_deref() != Ok("required"),
        "{why}, and {REQUIRED_BY} asks for a device"
    );
    eprintln!("skipped: {why}");
}

fn wait_until(holds: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while !holds() {
        if started.elapsed() >= WAIT {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
    true
}

fn sinks(backend: &CoreAudio) -> Vec<SinkInfo> {
    backend
        .surveyor()
        .enumerate_sinks(WAIT)
        .expect("the devices are listed")
}

fn loops_back(sink: &SinkInfo) -> bool {
    sink.description.contains(A_LOOPBACK_DEVICE_IS_NAMED)
}

fn is_shared(sink: &SinkInfo) -> bool {
    sink.name.as_str().starts_with("coreaudio:")
}

fn shared_sink(sinks: &[SinkInfo]) -> Option<SinkInfo> {
    sinks
        .iter()
        .find(|sink| is_shared(sink) && loops_back(sink))
        .or_else(|| sinks.iter().find(|sink| sink.is_default))
        .cloned()
}

fn request(sink: &SinkInfo, spec: StreamSpec) -> StreamRequest {
    StreamRequest {
        target: Some(sink.id),
        spec,
        latency: LatencyRequest::Auto,
        role: MediaRole::Music,
        media_name: String::new(),
        force_graph_rate: false,
        no_convert: true,
        realtime: true,
    }
}

fn heard(stream: &SinkStream, wanted: impl Fn(&StreamEvent) -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < WAIT {
        match stream.events().recv_timeout(Duration::from_millis(50)) {
            Ok(event) if wanted(&event) => return true,
            Ok(StreamEvent::StateChanged {
                to: StreamState::Failed,
                ..
            }) => return false,
            _ => {}
        }
    }
    false
}

struct Tone {
    phase: f32,
    step: f32,
    left: Arc<AtomicUsize>,
    stamps: Arc<Mutex<Vec<Stamp>>>,
}

impl AudioSource for Tone {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let frames = (dst.len() / FRAME_BYTES).min(self.left.load(Ordering::Acquire));
        for frame in dst.as_chunks_mut::<FRAME_BYTES>().0.iter_mut().take(frames) {
            let sample = (self.phase.sin() * 0.25).to_ne_bytes();
            frame[..4].copy_from_slice(&sample);
            frame[4..].copy_from_slice(&sample);
            self.phase = (self.phase + self.step) % TAU;
        }
        self.left.fetch_sub(frames, Ordering::AcqRel);
        frames * FRAME_BYTES
    }

    fn fill_stamped(&mut self, dst: &mut [u8], stamp: Stamp) -> usize {
        if let Some(mut stamps) = self.stamps.try_lock() {
            stamps.push(stamp);
        }
        self.fill(dst)
    }
}

#[test]
fn a_tone_plays_through_the_shared_sink_stamped_in_order_and_drains() {
    let backend = CoreAudio::start("resonate-tests").expect("a backend");
    let Some(sink) = shared_sink(&sinks(&backend)) else {
        return passed_over("this machine has no output device");
    };
    let Some(rate) = sink.current_rate else {
        return passed_over("the output device names no rate");
    };
    let layout = sink.formats[0].channels[0];
    if layout != ChannelLayout::Stereo {
        return passed_over("the output device is not stereo");
    }
    let left = Arc::new(AtomicUsize::new(rate.hz() as usize));
    let stamps = Arc::new(Mutex::new(Vec::new()));
    let tone = Tone {
        phase: 0.0,
        step: TAU * TONE_HZ / rate.hz() as f32,
        left: Arc::clone(&left),
        stamps: Arc::clone(&stamps),
    };
    let spec = StreamSpec::new(rate, layout, SampleFormat::F32);

    let stream = backend
        .opener()
        .open(&request(&sink, spec), Box::new(tone))
        .expect("a stream");

    assert!(heard(&stream, |event| matches!(
        event,
        StreamEvent::FormatChanged { spec: settled, .. } if *settled == spec
    )));
    stream.set_active(true).expect("started");
    assert!(heard(&stream, |event| matches!(
        event,
        StreamEvent::StateChanged {
            to: StreamState::Streaming,
            ..
        }
    )));
    if !wait_until(|| left.load(Ordering::Acquire) == 0) {
        stream.close().expect("closed");
        return passed_over("the output device never asked for the tone");
    }
    assert!(wait_until(|| stream.latency() > Frames::ZERO));
    stream.drain().expect("asked to drain");
    assert!(heard(&stream, |event| matches!(
        event,
        StreamEvent::Drained
    )));
    stream.close().expect("closed");

    let stamps = stamps.lock();
    let moments: Vec<Moment> = stamps.iter().map(|stamp| stamp.at).collect();
    assert!(moments.len() > 1);
    assert!(moments.windows(2).all(|pair| pair[0] <= pair[1]));
    Box::new(backend).shutdown().expect("shut down");
}

struct Ramp {
    next: i32,
    left: usize,
}

impl AudioSource for Ramp {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let frames = (dst.len() / FRAME_BYTES).min(self.left);
        for frame in dst.as_chunks_mut::<FRAME_BYTES>().0.iter_mut().take(frames) {
            frame[..4].copy_from_slice(&self.next.to_ne_bytes());
            frame[4..].copy_from_slice(&(-self.next).to_ne_bytes());
            self.next += 1;
        }
        self.left -= frames;
        frames * FRAME_BYTES
    }
}

struct Captured {
    words: Arc<Mutex<Vec<i32>>>,
    listening: Arc<AtomicBool>,
}

impl Hear for Captured {
    fn hear(&mut self, bytes: &[u8]) {
        if !self.listening.load(Ordering::Acquire) {
            return;
        }
        let Some(mut words) = self.words.try_lock() else {
            return;
        };
        for frame in bytes.as_chunks::<FRAME_BYTES>().0 {
            let left = f32::from_ne_bytes([frame[0], frame[1], frame[2], frame[3]]);
            words.push((left * TWENTY_FOUR_BITS).round() as i32);
        }
    }
}

fn loopback_device() -> Option<Device> {
    Device::all().ok()?.into_iter().find(|device| {
        device
            .name()
            .is_ok_and(|name| name.contains(A_LOOPBACK_DEVICE_IS_NAMED))
            && device
                .streams(Direction::Input)
                .is_ok_and(|streams| !streams.is_empty())
    })
}

fn another_rate(sink: &SinkInfo, than: Option<SampleRate>) -> Option<SampleRate> {
    sink.formats
        .iter()
        .filter(|offered| offered.format == SampleFormat::S24)
        .flat_map(|offered| offered.rates.iter().copied())
        .find(|rate| Some(*rate) != than)
}

#[test]
fn the_hogged_sink_moves_the_device_to_the_songs_rate_carries_24_bits_exactly_and_puts_it_back() {
    let backend = CoreAudio::start("resonate-tests").expect("a backend");
    let listed = sinks(&backend);
    let hogged = listed
        .iter()
        .find(|sink| !is_shared(sink) && loops_back(sink));
    let shared = listed
        .iter()
        .find(|sink| is_shared(sink) && loops_back(sink));
    let (Some(hogged), Some(shared), Some(device)) = (hogged, shared, loopback_device()) else {
        return passed_over("this machine has no loopback device to hog");
    };
    let before = shared.current_rate;
    let Some(rate) = another_rate(hogged, before) else {
        return passed_over("the loopback device offers 24 bits at no other rate");
    };
    let spec = StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::S24);
    let ramp = Ramp {
        next: 1,
        left: RAMP_FRAMES,
    };

    let stream = backend
        .opener()
        .open(&request(hogged, spec), Box::new(ramp))
        .expect("a hogged stream");

    let rate_of_the_shared_sink = || {
        sinks(&backend)
            .into_iter()
            .find(|sink| sink.id == shared.id)
            .and_then(|sink| sink.current_rate)
    };
    assert_eq!(rate_of_the_shared_sink(), Some(rate));

    let words = Arc::new(Mutex::new(Vec::new()));
    let listening = Arc::new(AtomicBool::new(true));
    let capture = Unit::hearing(
        Shape {
            component: Component::HalOutput,
            device: Some(device.id()),
            format: Format::new(
                f64::from(rate.hz()),
                NonZeroU32::MIN.saturating_add(1),
                Word::Float32,
            ),
        },
        Box::new(Captured {
            words: Arc::clone(&words),
            listening: Arc::clone(&listening),
        }),
        NonZeroU32::MIN.saturating_add(8_191),
    )
    .expect("a capture of the loopback");
    capture.start().expect("the capture started");
    stream.set_active(true).expect("started");
    stream.drain().expect("asked to drain");
    let drained = heard(&stream, |event| matches!(event, StreamEvent::Drained));
    listening.store(false, Ordering::Release);
    capture.stop().expect("the capture stopped");
    drop(capture);
    stream.close().expect("closed");

    assert!(wait_until(|| rate_of_the_shared_sink() == before));
    if !drained {
        return passed_over("the loopback device never asked for the ramp");
    }
    let words = words.lock();
    let Some(first) = words.iter().position(|word| *word != 0) else {
        return passed_over("the loopback device handed nothing back");
    };
    let run = &words[first..];
    assert!(run.len() >= RAMP_COMPARED_FRAMES);
    assert!(
        run[..RAMP_COMPARED_FRAMES]
            .windows(2)
            .all(|pair| pair[1] == pair[0] + 1)
    );
    Box::new(backend).shutdown().expect("shut down");
}
