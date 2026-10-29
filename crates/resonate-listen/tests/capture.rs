use std::{f64::consts::TAU, thread, time::Duration};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_dsp::Sweep;
use resonate_listen::{Clip, Hearing, Listener, Listening};
use resonate_pipewire::{
    AudioSource, Error, LatencyRequest, MediaRole, PipeWire, SinkInfo, StreamRequest,
};

const DISCOVERY: Duration = Duration::from_secs(2);
const TONE_HZ: f64 = 997.0;
const ELSEWHERE_HZ: f64 = 3_100.0;
const HEARD_FOR: Duration = Duration::from_millis(1_500);
const SETTLES_FOR: Duration = Duration::from_millis(300);
const TONE_STANDS_OVER_DB: f64 = 30.0;

struct Tone {
    at: u64,
}

impl AudioSource for Tone {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        for frame in dst.as_chunks_mut::<8>().0 {
            let sample = (0.3 * (TAU * TONE_HZ * self.at as f64 / 48_000.0).sin()) as f32;
            for word in frame.as_chunks_mut::<4>().0 {
                *word = sample.to_le_bytes();
            }
            self.at += 1;
        }
        dst.len() / 8 * 8
    }
}

fn daemon() -> Option<(PipeWire, SinkInfo)> {
    let pipewire = match PipeWire::start("resonate-listen-tests") {
        Ok(pipewire) => pipewire,
        Err(error) => {
            eprintln!("skipped: no PipeWire daemon to talk to ({error})");
            return None;
        }
    };
    match pipewire.default_sink(DISCOVERY) {
        Ok(Some(sink)) => Some((pipewire, sink)),
        Err(Error::Disconnected) => {
            eprintln!("skipped: no PipeWire daemon to talk to");
            None
        }
        Ok(None) | Err(Error::NoSink) => {
            eprintln!("skipped: the daemon advertises no sink");
            None
        }
        Err(error) => panic!("the daemon would not enumerate its sinks: {error}"),
    }
}

fn level_at(clip: &Clip, hertz: f64) -> f64 {
    let mono = clip.mono();
    let step = TAU * hertz / f64::from(clip.rate.hz());
    let (mut real, mut imaginary) = (0.0, 0.0);
    for (n, sample) in mono.iter().enumerate() {
        real += f64::from(*sample) * (step * n as f64).cos();
        imaginary += f64::from(*sample) * (step * n as f64).sin();
    }
    20.0 * (real.hypot(imaginary) / mono.len().max(1) as f64)
        .max(1e-12)
        .log10()
}

#[test]
fn what_the_desktop_plays_is_heard_through_its_monitor() {
    let Some((pipewire, sink)) = daemon() else {
        return;
    };
    let spec = StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Stereo,
        SampleFormat::F32,
    );
    let stream = pipewire
        .open(
            &StreamRequest {
                target: Some(sink.id),
                spec,
                latency: LatencyRequest::Auto,
                role: MediaRole::Music,
                media_name: "resonate listen test".to_owned(),
                force_graph_rate: false,
                no_convert: false,
                realtime: true,
            },
            Box::new(Tone { at: 0 }),
        )
        .expect("the daemon accepts a tone");
    stream.set_active(true).expect("the tone plays");
    thread::sleep(SETTLES_FOR);

    let clip = Listener::new("resonate-listen-tests")
        .record(&Listening::Desktop, HEARD_FOR, &Hearing::new())
        .expect("the desktop is heard");
    drop(stream);
    let _ = pipewire.shutdown();

    assert!(!clip.is_silent(), "the monitor heard nothing");
    let tone = level_at(&clip, TONE_HZ);
    let elsewhere = level_at(&clip, ELSEWHERE_HZ);
    assert!(
        tone - elsewhere > TONE_STANDS_OVER_DB,
        "the tone reads {tone:.1} dB against {elsewhere:.1} dB elsewhere"
    );
    assert!(clip.length() >= HEARD_FOR - Duration::from_millis(50));
}

#[test]
fn a_sweep_played_to_the_desktop_is_heard_through_its_monitor_and_deconvolves_to_a_response() {
    let Some((pipewire, _)) = daemon() else {
        return;
    };
    let _ = pipewire.shutdown();
    let sweep = Sweep::new(SampleRate::HZ_48000, 20, 20_000, Duration::from_secs(3))
        .expect("a sweep that fits the rate");

    let clip = Listener::new("resonate-listen-tests")
        .play_and_record(
            sweep.samples(),
            None,
            &Listening::Desktop,
            sweep.length() + Duration::from_secs(1),
            &Hearing::new(),
        )
        .expect("the sweep is heard");

    let heard = sweep
        .heard_as(&clip.mono())
        .expect("the monitor heard the sweep");
    let response = heard.response().expect("a response");
    let at_a_kilohertz = response
        .iter()
        .min_by(|a, b| {
            (a.hertz - 1_000.0)
                .abs()
                .total_cmp(&(b.hertz - 1_000.0).abs())
        })
        .expect("a point near a kilohertz");
    assert!(
        (-12.0..6.0).contains(&at_a_kilohertz.decibels),
        "the monitor reads {:.1} dB at a kilohertz",
        at_a_kilohertz.decibels
    );
}
