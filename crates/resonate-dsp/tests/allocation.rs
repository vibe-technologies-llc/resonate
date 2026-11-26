use std::sync::Arc;

use allocation_counter::measure;
use resonate_core::{
    AppliedGain, Decibels, Volume,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};
use resonate_dsp::{Easing, Processor};

use crate::stages::{BLOCKS, MAX_BLOCK, Stage, every_stage, walled_noise};

const SWEEPS_WARMED_UP: usize = 3;
const SEED: u64 = 0xA110_C000_0000_0001;
const PEAKING_AT_HZ: f64 = 2_000.0;
const PEAKING_GAIN_DB: f64 = 4.0;
const PEAKING_Q: f64 = 1.5;
const TRIMMED_BY_DB: f32 = -3.0;
const HALF_THE_VOLUME: f32 = 0.5;
const GAIN_SETTLES_AFTER_FRAMES: usize = 100;

struct Buffers {
    input: Vec<f64>,
    scratch: Vec<f64>,
    tail: Vec<f64>,
    from: usize,
}

struct Changes {
    volume: Volume,
    replay_gain: AppliedGain,
    profile: Arc<Profile>,
}

fn changes() -> Changes {
    let peaking = Band::new(
        BandKind::Peaking,
        Frequency::from_hertz(PEAKING_AT_HZ).expect("in range"),
        BandGain::from_decibels(PEAKING_GAIN_DB).expect("in range"),
        Q::from_units(PEAKING_Q).expect("in range"),
    );

    Changes {
        volume: Volume::new(HALF_THE_VOLUME).expect("in range"),
        replay_gain: AppliedGain {
            gain: Some(Decibels::new(TRIMMED_BY_DB).expect("finite")),
            peak: None,
        },
        profile: Arc::new(Profile::new(Preamp::NONE, vec![peaking]).expect("a profile in range")),
    }
}

fn sweep(stage: &mut dyn Processor, buffers: &mut Buffers) -> usize {
    let mut frames = 0;
    let mut remaining = buffers.input.as_slice();

    for block in BLOCKS {
        let (now, rest) = remaining.split_at(block * buffers.from);

        frames += stage.process(now, &mut buffers.scratch).frames_out;
        remaining = rest;
    }
    frames
}

fn allocations_of(stage: &Stage) -> u64 {
    let (mut processor, output) = stage.prepared();
    let changes = changes();
    let from = usize::from(stage.input.channel_count().get());
    let to = usize::from(output.channel_count().get());
    let sweep_frames: usize = BLOCKS.iter().sum();

    let mut buffers = Buffers {
        input: walled_noise(stage.input.rate, from, sweep_frames, SEED),
        scratch: vec![0.0; processor.max_output_frames(MAX_BLOCK) * to],
        tail: vec![0.0; processor.max_flush_frames().max(1) * to],
        from,
    };

    for _ in 0..SWEEPS_WARMED_UP {
        sweep(processor.as_mut(), &mut buffers);
    }
    processor.reset();
    processor.flush(&mut buffers.tail);
    sweep(processor.as_mut(), &mut buffers);

    let mut frames = 0;

    let info = measure(|| {
        frames += sweep(processor.as_mut(), &mut buffers);
        processor.reset();
        frames += sweep(processor.as_mut(), &mut buffers);
        processor.set_gain(changes.volume, changes.replay_gain);
        frames += sweep(processor.as_mut(), &mut buffers);
        processor.set_gain_after(
            changes.volume,
            changes.replay_gain,
            GAIN_SETTLES_AFTER_FRAMES,
        );
        frames += sweep(processor.as_mut(), &mut buffers);
        processor.set_equalisation(&changes.profile);
        processor.ease_equalisation(Easing::Entering);
        frames += sweep(processor.as_mut(), &mut buffers);
        processor.forget_the_source_heard();
        frames += sweep(processor.as_mut(), &mut buffers);
        frames += processor.flush(&mut buffers.tail);
    });

    assert!(frames > 0, "{} produced no frames", stage.name);
    info.count_total
}

fn allocates_nothing_in(prefix: &str) {
    let stages: Vec<Stage> = every_stage()
        .into_iter()
        .filter(|stage| stage.name.starts_with(prefix))
        .collect();

    assert!(!stages.is_empty(), "no stage is named {prefix}");

    let allocating: Vec<String> = stages
        .iter()
        .filter_map(|stage| {
            let count = allocations_of(stage);

            (count > 0).then(|| format!("{}: {count} allocations", stage.name))
        })
        .collect();

    assert!(
        allocating.is_empty(),
        "{} stages allocate:\n{}",
        allocating.len(),
        allocating.join("\n")
    );
}

#[test]
fn overs_allocates_nothing() {
    allocates_nothing_in("overs");
}

#[test]
fn gain_allocates_nothing() {
    allocates_nothing_in("gain");
}

#[test]
fn dither_allocates_nothing() {
    allocates_nothing_in("dither");
}

#[test]
fn the_equaliser_allocates_nothing() {
    allocates_nothing_in("equaliser");
}

#[test]
fn the_convolver_allocates_nothing() {
    allocates_nothing_in("convolver");
}

#[test]
fn the_true_peak_limiter_allocates_nothing() {
    allocates_nothing_in("true peak");
}

#[test]
fn the_crossfeed_allocates_nothing() {
    allocates_nothing_in("crossfeed");
}

#[test]
fn remix_allocates_nothing() {
    allocates_nothing_in("remix");
}

#[test]
fn restoration_allocates_nothing() {
    allocates_nothing_in("restore");
}

#[test]
fn the_low_latency_resampler_allocates_nothing() {
    allocates_nothing_in("resampler LowLatency");
}

#[test]
fn the_balanced_resampler_allocates_nothing() {
    allocates_nothing_in("resampler Balanced");
}
