use std::{
    env,
    f64::consts::TAU,
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use resonate_core::{
    AudioBuffer, BitDepth, ChannelLayout, SampleFormat, SampleRate, StreamSpec, Volume,
    eq::{Band, BandGain, BandKind, Frequency, MAX_BANDS, Preamp, Profile, Q},
};
use resonate_dsp::{
    Chain, Dither, DitherKind, Equaliser, FilterPhase, GainConfig, GainStage, NoiseShaping,
    Processor, Quality, Remix, Resampler, ResamplerConfig, Restoration, Restore, RestoreConfig,
    TruePeak, Tuning,
};

const BLOCK: usize = 1_024;
const AUDIO_SECONDS: u32 = 4;
const RUNS: usize = 5;
const CONSTRUCTIONS: usize = 9;
const SEED: u64 = 0x5265_736F_6E61_7465;
const PUSHED_OVER: f64 = 1.6;

const SHAPED_PHASES: [(FilterPhase, &str); 2] = [
    (FilterPhase::Minimum, "minimum"),
    (FilterPhase::Intermediate, "intermediate"),
];

const QUALITIES: [(Quality, &str); 4] = [
    (Quality::Fast, "fast"),
    (Quality::Balanced, "balanced"),
    (Quality::High, "high"),
    (Quality::VeryHigh, "very high"),
];

const RATE_PAIRS: [(SampleRate, SampleRate); 6] = [
    (SampleRate::HZ_44100, SampleRate::HZ_48000),
    (SampleRate::HZ_48000, SampleRate::HZ_44100),
    (SampleRate::HZ_96000, SampleRate::HZ_48000),
    (SampleRate::HZ_192000, SampleRate::HZ_48000),
    (SampleRate::HZ_176400, SampleRate::HZ_48000),
    (SampleRate::HZ_44100, SampleRate::HZ_96000),
];

struct Bench {
    wanted: Vec<String>,
}

impl Bench {
    fn from_arguments() -> Self {
        Self {
            wanted: env::args()
                .skip(1)
                .filter(|argument| !argument.starts_with("--"))
                .collect(),
        }
    }

    fn runs(&self, name: &str) -> bool {
        self.wanted.is_empty() || self.wanted.iter().any(|word| name.contains(word.as_str()))
    }

    fn stage(&self, name: &str, spec: StreamSpec, build: impl FnOnce() -> Box<dyn Processor>) {
        if self.runs(name) {
            report(name, through_stage(build(), spec, &signal(spec)));
        }
    }

    fn hot_stage(&self, name: &str, spec: StreamSpec, build: impl FnOnce() -> Box<dyn Processor>) {
        if self.runs(name) {
            report(name, through_stage(build(), spec, &hot(signal(spec))));
        }
    }

    fn chain(&self, name: &str, spec: StreamSpec, build: impl FnOnce() -> Chain) {
        if self.runs(name) {
            report(name, through_chain(build(), spec));
        }
    }

    fn conversion(&self, name: &str, format: SampleFormat) {
        if self.runs(name) {
            report(name, converting_into(format));
        }
    }

    fn construction(&self, name: &str, mut build: impl FnMut()) {
        if !self.runs(name) {
            return;
        }
        let fastest = (0..CONSTRUCTIONS)
            .map(|_| {
                let started = Instant::now();
                build();
                started.elapsed()
            })
            .min()
            .unwrap_or_default();
        println!(
            "{name:<52} {:>12.1} µs to build",
            fastest.as_secs_f64() * 1e6
        );
    }

    fn construction_after(&self, name: &str, mut first: impl FnMut(), mut build: impl FnMut()) {
        if !self.runs(name) {
            return;
        }
        let fastest = (0..CONSTRUCTIONS)
            .map(|_| {
                first();
                let started = Instant::now();
                build();
                started.elapsed()
            })
            .min()
            .unwrap_or_default();
        println!(
            "{name:<52} {:>12.1} µs to build",
            fastest.as_secs_f64() * 1e6
        );
    }

    fn first_construction(&self, name: &str, build: impl FnOnce()) {
        if !self.runs(name) {
            return;
        }
        let started = Instant::now();
        build();
        println!(
            "{name:<52} {:>12.1} ms the first time",
            started.elapsed().as_secs_f64() * 1e3
        );
    }
}

fn built_for() -> &'static str {
    if cfg!(target_feature = "avx512f") {
        "avx512f"
    } else if cfg!(target_feature = "avx2") {
        "avx2"
    } else if cfg!(target_feature = "sse4.1") {
        "sse4.1"
    } else {
        "sse2"
    }
}

fn report(name: &str, elapsed: Duration) {
    let per_second = elapsed.as_secs_f64() / f64::from(AUDIO_SECONDS);
    println!(
        "{name:<52} {:>12.1} µs per s {:>9.3} % of a core",
        per_second * 1e6,
        per_second * 100.0
    );
}

fn fastest(mut run: impl FnMut() -> Duration) -> Duration {
    (0..RUNS).map(|_| run()).min().unwrap_or_default()
}

fn signal(spec: StreamSpec) -> Vec<f64> {
    let channels = usize::from(spec.channel_count().get());
    let rate = f64::from(spec.rate.hz());
    let frames = spec.rate.hz() as usize * AUDIO_SECONDS as usize;
    let mut noise = SEED;

    (0..frames * channels)
        .map(|index| {
            let frame = (index / channels) as f64;
            let channel = (index % channels) as f64;
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            let hiss = (noise >> 40) as f64 / 16_777_216.0 - 0.5;
            let low = (TAU * 440.0 * frame / rate + channel).sin();
            let high = (TAU * 3_100.0 * frame / rate + 2.0 * channel).sin();
            0.4 * low + 0.3 * high + 0.1 * hiss
        })
        .collect()
}

fn hot(signal: Vec<f64>) -> Vec<f64> {
    signal
        .into_iter()
        .map(|sample| sample * PUSHED_OVER)
        .collect()
}

fn through_stage(mut stage: Box<dyn Processor>, spec: StreamSpec, input: &[f64]) -> Duration {
    let channels = usize::from(spec.channel_count().get());
    let widened = usize::from(stage.output_spec(spec).channel_count().get());
    let most = stage
        .prepare(spec, BLOCK)
        .expect("every benchmarked stage prepares");
    let mut output = vec![0.0; most.max(BLOCK) * widened];

    fastest(|| {
        stage.reset();
        let started = Instant::now();
        for block in input.chunks(BLOCK * channels) {
            black_box(stage.process(black_box(block), &mut output));
        }
        started.elapsed()
    })
}

fn through_chain(mut chain: Chain, spec: StreamSpec) -> Duration {
    let channels = usize::from(spec.channel_count().get());
    let input = signal(spec);
    let mut output = vec![0.0; chain.max_output_frames() * channels];

    fastest(|| {
        chain.reset();
        let started = Instant::now();
        for block in input.chunks(BLOCK * channels) {
            black_box(chain.process(black_box(block), &mut output));
        }
        started.elapsed()
    })
}

fn converting_into(format: SampleFormat) -> Duration {
    let spec = stereo(SampleRate::HZ_48000);
    let carrier: Vec<f64> = signal(spec)
        .into_iter()
        .take(BLOCK * usize::from(spec.channel_count().get()))
        .collect();
    let mut staged = AudioBuffer::silence(StreamSpec::new(spec.rate, spec.channels, format), BLOCK);
    let blocks = spec.rate.hz() as usize * AUDIO_SECONDS as usize / BLOCK;

    fastest(|| {
        let started = Instant::now();
        for _ in 0..blocks {
            staged.data_mut().write_f64(black_box(&carrier));
            black_box(&staged);
        }
        started.elapsed()
    })
}

fn stereo(rate: SampleRate) -> StreamSpec {
    StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32)
}

fn surround(rate: SampleRate) -> StreamSpec {
    StreamSpec::new(rate, ChannelLayout::Surround51, SampleFormat::F32)
}

fn resampler(from: SampleRate, to: SampleRate, quality: Quality) -> ResamplerConfig {
    ResamplerConfig {
        input_rate: from,
        output_rate: to,
        channels: ChannelLayout::Stereo,
        quality,
        phase: FilterPhase::Linear,
        max_frames_in: BLOCK,
    }
}

fn kilohertz(rate: SampleRate) -> String {
    let hz = rate.hz();
    if hz.is_multiple_of(1_000) {
        format!("{}k", hz / 1_000)
    } else {
        format!("{:.1}k", f64::from(hz) / 1_000.0)
    }
}

fn band(hertz: f64, decibels: f64, q: f64) -> Band {
    Band::new(
        BandKind::Peaking,
        Frequency::from_hertz(hertz).expect("a benchmarked centre"),
        BandGain::from_decibels(decibels).expect("a benchmarked gain"),
        Q::from_units(q).expect("a benchmarked Q"),
    )
}

fn profile(bands: usize) -> Arc<Profile> {
    let lowest: f64 = 31.0;
    let highest: f64 = 16_000.0;
    let step = (highest / lowest).powf(1.0 / (bands.max(2) - 1) as f64);
    let shaped = (0..bands)
        .map(|index| {
            let decibels = if index % 2 == 0 { 4.5 } else { -3.0 };
            band(lowest * step.powi(index as i32), decibels, 1.4)
        })
        .collect();
    Arc::new(
        Profile::new(
            Preamp::from_decibels(-4.5).expect("a benchmarked preamp"),
            shaped,
        )
        .expect("a benchmarked profile"),
    )
}

fn half_volume() -> GainConfig {
    GainConfig {
        volume: Volume::new(0.5).expect("half volume is in range"),
        ramp: Duration::ZERO,
        ..GainConfig::default()
    }
}

fn dither(shaping: NoiseShaping) -> Box<dyn Processor> {
    Box::new(Dither::new(
        BitDepth::Bits16,
        DitherKind::Triangular,
        shaping,
        SEED,
    ))
}

fn main() {
    let bench = Bench::from_arguments();
    println!(
        "resonate-dsp stages over {AUDIO_SECONDS} s of audio, fastest of {RUNS}, built for {}",
        built_for()
    );

    for (quality, named) in QUALITIES {
        for (from, to) in RATE_PAIRS {
            let name = format!("resample {named} {} to {}", kilohertz(from), kilohertz(to));
            bench.stage(&name, stereo(from), || {
                Box::new(
                    Resampler::new(resampler(from, to, quality))
                        .expect("a benchmarked configuration"),
                )
            });
        }
    }

    for (phase, named) in SHAPED_PHASES {
        let config = ResamplerConfig {
            phase,
            ..resampler(
                SampleRate::HZ_44100,
                SampleRate::HZ_48000,
                Quality::VeryHigh,
            )
        };
        bench.first_construction(&format!("design very high {named} phase"), || {
            black_box(Resampler::new(black_box(config)).expect("a benchmarked configuration"));
        });
        for (from, to) in [
            (SampleRate::HZ_44100, SampleRate::HZ_48000),
            (SampleRate::HZ_96000, SampleRate::HZ_48000),
        ] {
            let name = format!(
                "resample very high {named} {} to {}",
                kilohertz(from),
                kilohertz(to)
            );
            let config = ResamplerConfig {
                phase,
                ..resampler(from, to, Quality::VeryHigh)
            };
            bench.stage(&name, stereo(from), || {
                Box::new(Resampler::new(config).expect("a benchmarked configuration"))
            });
        }
    }

    let elsewhere = resampler(SampleRate::HZ_88200, SampleRate::HZ_96000, Quality::Fast);
    for (quality, named) in QUALITIES {
        for (from, to) in RATE_PAIRS {
            let pair = format!("{} to {}", kilohertz(from), kilohertz(to));
            let config = resampler(from, to, quality);
            bench.construction_after(
                &format!("build {named} {pair}"),
                || {
                    black_box(Resampler::new(elsewhere).expect("a benchmarked configuration"));
                },
                || {
                    black_box(
                        Resampler::new(black_box(config)).expect("a benchmarked configuration"),
                    );
                },
            );
            bench.construction(&format!("build {named} {pair} again"), || {
                black_box(Resampler::new(black_box(config)).expect("a benchmarked configuration"));
            });
        }
    }

    let rate = SampleRate::HZ_48000;
    for at in [rate, SampleRate::HZ_192000] {
        for bands in [10, MAX_BANDS] {
            let name = format!("equalise {bands} bands at {}", kilohertz(at));
            bench.stage(&name, stereo(at), || {
                Box::new(Equaliser::new(profile(bands), at))
            });
        }
    }

    for at in [rate, SampleRate::HZ_192000] {
        let named = kilohertz(at);
        bench.stage(&format!("true peak guard at {named}"), stereo(at), || {
            Box::new(TruePeak::new())
        });
        bench.hot_stage(
            &format!("true peak guard limiting at {named}"),
            stereo(at),
            || Box::new(TruePeak::new()),
        );
        bench.chain(
            &format!("true peak guard at half volume at {named}"),
            stereo(at),
            || {
                Chain::builder(stereo(at))
                    .max_frames_in(BLOCK)
                    .push(Box::new(GainStage::new(half_volume())))
                    .push(Box::new(TruePeak::new()))
                    .build()
                    .expect("a benchmarked chain")
            },
        );
    }
    bench.stage(
        "restore a lossy source",
        stereo(SampleRate::HZ_44100),
        || {
            Box::new(Restore::new(RestoreConfig {
                restoration: Restoration::Extend,
                tuning: Tuning::Mp3,
                wall: Frequency::from_hertz(16_000.0).ok(),
            }))
        },
    );

    bench.stage("dither flat to 16 bits", stereo(rate), || {
        dither(NoiseShaping::None)
    });
    bench.stage("dither lipshitz to 16 bits", stereo(rate), || {
        dither(NoiseShaping::Lipshitz)
    });
    bench.stage("dither threshold to 16 bits", stereo(rate), || {
        dither(NoiseShaping::Threshold)
    });
    bench.stage(
        "dither threshold to 16 bits at 192k",
        stereo(SampleRate::HZ_192000),
        || dither(NoiseShaping::Threshold),
    );
    for designed_at in [SampleRate::HZ_44100, SampleRate::HZ_192000] {
        let name = format!("prepare threshold dither at {}", kilohertz(designed_at));
        bench.construction(&name, || {
            let mut stage = dither(black_box(NoiseShaping::Threshold));
            black_box(
                stage
                    .prepare(black_box(stereo(designed_at)), BLOCK)
                    .expect("dither always prepares"),
            );
            black_box(stage);
        });
    }
    bench.stage("gain at half volume", stereo(rate), || {
        Box::new(GainStage::new(half_volume()))
    });
    bench.stage("remix 5.1 to stereo", surround(rate), || {
        Box::new(Remix::new(ChannelLayout::Surround51, ChannelLayout::Stereo))
    });

    bench.chain(
        "chain 5.1 44.1k to stereo 48k, eq, gain, dither",
        surround(SampleRate::HZ_44100),
        || {
            Chain::builder(surround(SampleRate::HZ_44100))
                .max_frames_in(BLOCK)
                .push(Box::new(Remix::new(
                    ChannelLayout::Surround51,
                    ChannelLayout::Stereo,
                )))
                .push(Box::new(
                    Resampler::new(resampler(
                        SampleRate::HZ_44100,
                        SampleRate::HZ_48000,
                        Quality::High,
                    ))
                    .expect("a benchmarked configuration"),
                ))
                .push(Box::new(Equaliser::new(profile(10), rate)))
                .push(Box::new(GainStage::new(half_volume())))
                .push(dither(NoiseShaping::Threshold))
                .build()
                .expect("a benchmarked chain")
        },
    );

    bench.chain(
        "chain stereo 44.1k to 48k, gain, guard, dither",
        stereo(SampleRate::HZ_44100),
        || {
            Chain::builder(stereo(SampleRate::HZ_44100))
                .max_frames_in(BLOCK)
                .push(Box::new(
                    Resampler::new(resampler(
                        SampleRate::HZ_44100,
                        SampleRate::HZ_48000,
                        Quality::High,
                    ))
                    .expect("a benchmarked configuration"),
                ))
                .push(Box::new(GainStage::new(GainConfig::default())))
                .push(Box::new(TruePeak::new()))
                .push(dither(NoiseShaping::Threshold))
                .build()
                .expect("a benchmarked chain")
        },
    );

    bench.conversion("narrow f64 to s16", SampleFormat::S16);
    bench.conversion("narrow f64 to s24", SampleFormat::S24);
    bench.conversion("narrow f64 to s32", SampleFormat::S32);
}
