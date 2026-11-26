use std::sync::Arc;

use resonate_core::{
    BitDepth, ChannelLayout, SampleFormat, SampleRate, StreamSpec,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};
use resonate_dsp::{
    Convolver, Crossfeed, Crossfeeder, Dither, DitherKind, Equaliser, FilterPhase, GainConfig,
    GainStage, Impulse, NoiseShaping, Overs, Processor, Quality, Remix, Resampler, ResamplerConfig,
    Restoration, Restore, RestoreConfig, TruePeak, Tuning,
};
use rustfft::{FftPlanner, num_complex::Complex};

pub const MAX_BLOCK: usize = 2_048;
pub const BLOCKS: [usize; 10] = [1, 7, 61, 509, 1_023, 3, 256, 2_048, 31, 997];
pub const RATES: [SampleRate; 9] = [
    SampleRate::HZ_8000,
    SampleRate::HZ_16000,
    SampleRate::HZ_22050,
    SampleRate::HZ_44100,
    SampleRate::HZ_48000,
    SampleRate::HZ_88200,
    SampleRate::HZ_96000,
    SampleRate::HZ_176400,
    SampleRate::HZ_192000,
];
pub const LAYOUTS: [ChannelLayout; 3] = [
    ChannelLayout::Mono,
    ChannelLayout::Stereo,
    ChannelLayout::Surround51,
];

const RESAMPLED_TO: [SampleRate; 4] = [
    SampleRate::HZ_8000,
    SampleRate::HZ_44100,
    SampleRate::HZ_48000,
    SampleRate::HZ_192000,
];
const RESAMPLER_QUALITIES: [Quality; 2] = [Quality::LowLatency, Quality::Balanced];
const NOISE_LEVEL: f64 = 0.25;
const CONVOLVED_WITH: [f64; 3] = [1.0, 0.0, 0.1];
const DITHERED_TO: BitDepth = BitDepth::Bits16;
const DITHER_SEED: u64 = 0x5EED;

pub struct Stage {
    pub name: String,
    pub input: StreamSpec,
    pub rings: bool,
    pub build: Box<dyn Fn() -> Box<dyn Processor>>,
}

impl Stage {
    pub fn prepared(&self) -> (Box<dyn Processor>, StreamSpec) {
        let mut stage = (self.build)();

        stage
            .prepare(self.input, MAX_BLOCK)
            .expect("the stage prepares");

        let output = stage.output_spec(self.input);

        (stage, output)
    }
}

fn boxed(stage: impl Processor + 'static) -> Box<dyn Processor> {
    Box::new(stage)
}

fn stage<F>(name: &str, rate: SampleRate, layout: ChannelLayout, build: F) -> Stage
where
    F: Fn() -> Box<dyn Processor> + 'static,
{
    Stage {
        name: format!("{name} {} Hz {layout:?}", rate.hz()),
        input: StreamSpec::new(rate, layout, SampleFormat::F32),
        rings: false,
        build: Box::new(build),
    }
}

fn profile(rate: SampleRate) -> Arc<Profile> {
    let highest = f64::from(rate.hz()) * 0.4;
    let bands = [
        (BandKind::LowShelf, 120.0_f64, 3.0, 0.7),
        (BandKind::Peaking, 1_000.0, -4.0, 1.2),
        (BandKind::Peaking, 3_000.0, 2.5, 2.0),
        (BandKind::HighShelf, 6_000.0, -2.0, 0.7),
    ]
    .into_iter()
    .map(|(kind, at, gain, q)| {
        Band::new(
            kind,
            Frequency::from_hertz(at.min(highest)).expect("in range"),
            BandGain::from_decibels(gain).expect("in range"),
            Q::from_units(q).expect("in range"),
        )
    })
    .collect();

    Arc::new(Profile::new(Preamp::NONE, bands).expect("a profile in range"))
}

fn impulse(rate: SampleRate) -> Arc<Impulse> {
    Arc::new(Impulse::new(rate, vec![CONVOLVED_WITH.to_vec()]).expect("an impulse"))
}

fn resampled(
    from: SampleRate,
    to: SampleRate,
    layout: ChannelLayout,
    quality: Quality,
) -> Box<dyn Processor> {
    boxed(
        Resampler::new(ResamplerConfig {
            input_rate: from,
            output_rate: to,
            channels: layout,
            quality,
            phase: FilterPhase::Linear,
            max_frames_in: MAX_BLOCK,
        })
        .expect("a resampler"),
    )
}

fn stages_at(rate: SampleRate, layout: ChannelLayout) -> Vec<Stage> {
    let mut stages = vec![
        stage("overs", rate, layout, || boxed(Overs::new())),
        stage("gain", rate, layout, || {
            boxed(GainStage::new(GainConfig::default()))
        }),
        stage("dither", rate, layout, || {
            boxed(Dither::new(
                DITHERED_TO,
                DitherKind::Triangular,
                NoiseShaping::None,
                DITHER_SEED,
            ))
        }),
        stage("equaliser", rate, layout, move || {
            boxed(Equaliser::new(profile(rate), rate))
        }),
        Stage {
            rings: true,
            ..stage("convolver", rate, layout, move || {
                boxed(Convolver::new(impulse(rate)))
            })
        },
        stage("true peak", rate, layout, || boxed(TruePeak::new())),
    ];

    stages.push(stage("restore Off Mp3", rate, layout, || {
        boxed(Restore::new(RestoreConfig {
            restoration: Restoration::Off,
            tuning: Tuning::Mp3,
            wall: None,
        }))
    }));
    for restoration in [Restoration::Repair, Restoration::Extend] {
        for tuning in [Tuning::Mp3, Tuning::Aac, Tuning::Vorbis] {
            stages.push(stage(
                &format!("restore {restoration:?} {tuning:?}"),
                rate,
                layout,
                move || {
                    boxed(Restore::new(RestoreConfig {
                        restoration,
                        tuning,
                        wall: None,
                    }))
                },
            ));
        }
    }
    for quality in RESAMPLER_QUALITIES {
        for to in RESAMPLED_TO.into_iter().filter(|to| *to != rate) {
            stages.push(stage(
                &format!("resampler {quality:?} to {} Hz", to.hz()),
                rate,
                layout,
                move || resampled(rate, to, layout, quality),
            ));
        }
    }
    if layout == ChannelLayout::Stereo {
        for crossfeed in [Crossfeed::Light, Crossfeed::Strong] {
            stages.push(stage(
                &format!("crossfeed {crossfeed:?}"),
                rate,
                layout,
                move || boxed(Crossfeeder::new(crossfeed)),
            ));
        }
        stages.push(stage("remix to mono", rate, layout, || {
            boxed(Remix::new(ChannelLayout::Stereo, ChannelLayout::Mono))
        }));
    }
    if layout != ChannelLayout::Stereo {
        stages.push(stage("remix to stereo", rate, layout, move || {
            boxed(Remix::new(layout, ChannelLayout::Stereo))
        }));
    }
    stages
}

pub fn every_stage() -> Vec<Stage> {
    RATES
        .into_iter()
        .flat_map(|rate| {
            LAYOUTS
                .into_iter()
                .flat_map(move |layout| stages_at(rate, layout))
        })
        .collect()
}

const TONES_HZ: [f64; 5] = [337.0, 761.0, 1_283.0, 1_877.0, 2_641.0];

pub fn tones(rate: SampleRate, channels: usize, frames: usize) -> Vec<f64> {
    let rate = f64::from(rate.hz());
    let level = NOISE_LEVEL / TONES_HZ.len() as f64;

    (0..frames)
        .flat_map(|frame| {
            (0..channels).map(move |channel| {
                TONES_HZ
                    .iter()
                    .enumerate()
                    .map(|(index, hz)| {
                        let phase = (channel * TONES_HZ.len() + index) as f64 * 0.7;
                        level * (std::f64::consts::TAU * hz * frame as f64 / rate + phase).sin()
                    })
                    .sum::<f64>()
            })
        })
        .collect()
}

const WALL_AT_NYQUISTS: f64 = 0.75;
const WALL_AT_MOST_HZ: f64 = 16_000.0;

pub fn walled_noise(rate: SampleRate, channels: usize, frames: usize, seed: u64) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();

    let forward = planner.plan_fft_forward(frames.max(1));
    let inverse = planner.plan_fft_inverse(frames.max(1));
    let raw = noise(channels, frames, seed);
    let nyquist = f64::from(rate.hz()) / 2.0;
    let wall = (nyquist * WALL_AT_NYQUISTS).min(WALL_AT_MOST_HZ);

    let kept = (wall / nyquist * (frames / 2) as f64) as usize;

    let filtered: Vec<Vec<f64>> = (0..channels)
        .map(|of| {
            let mut bins: Vec<Complex<f64>> = raw
                .iter()
                .skip(of)
                .step_by(channels)
                .map(|sample| Complex::new(*sample, 0.0))
                .collect();

            forward.process(&mut bins);
            for (index, bin) in bins.iter_mut().enumerate() {
                let folded = index.min(frames - index);

                if folded > kept {
                    *bin = Complex::new(0.0, 0.0);
                }
            }
            inverse.process(&mut bins);
            bins.iter().map(|bin| bin.re / frames as f64).collect()
        })
        .collect();

    (0..frames)
        .flat_map(|frame| filtered.iter().map(move |channel| channel[frame]))
        .collect()
}

pub fn noise(channels: usize, frames: usize, seed: u64) -> Vec<f64> {
    let mut states: Vec<u64> = (0..channels)
        .map(|channel| seed ^ ((channel as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)))
        .collect();

    (0..frames)
        .flat_map(|_| {
            states
                .iter_mut()
                .map(|state| {
                    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);

                    let mut mixed = *state;

                    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                    mixed ^= mixed >> 31;
                    ((mixed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5) * 2.0 * NOISE_LEVEL
                })
                .collect::<Vec<f64>>()
        })
        .collect()
}
