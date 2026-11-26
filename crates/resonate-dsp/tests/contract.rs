use resonate_core::StreamSpec;
use resonate_dsp::Processor;
use rustfft::{FftPlanner, num_complex::Complex};

use crate::stages::{BLOCKS, MAX_BLOCK, Stage, every_stage, tones, walled_noise};

const LONG_FRAMES: usize = 32_768;
const SHORT_FRAMES: [usize; 5] = [1, 10, 300, 1_000, 4_000];
const ABANDONED_AFTER_FRAMES: usize = 5_003;
const SEED: u64 = 0xC0FF_EE00_1234_5678;
const SEARCHED_BEYOND_LATENCY: usize = 256;
const FOLLOWS_AT_LEAST: f64 = 0.9;
const BETTER_THAN_ANOTHER_CHANNEL_BY: f64 = 0.05;
const LONG_ENOUGH_TO_FOLLOW: usize = 200;
const ROUNDING_OF_A_RATIO: f64 = 2.0;

#[derive(Clone, Copy)]
struct Tally {
    frames_in: usize,
    frames_out: usize,
    held: usize,
    short: usize,
    past: usize,
}

struct Fed {
    heard: Vec<f64>,
    tallies: Vec<Tally>,
    frames_in: usize,
    frames_out_before_the_flush: usize,
    flushed: usize,
}

fn feed(stage: &mut dyn Processor, input: &[f64], from: usize, to: usize) -> Fed {
    let mut scratch = vec![0.0; stage.max_output_frames(MAX_BLOCK) * to];
    let mut heard = Vec::new();
    let mut tallies = Vec::new();
    let mut frames_in = 0;
    let mut frames_out = 0;
    let mut remaining = input;

    for block in BLOCKS.iter().cycle() {
        if remaining.is_empty() {
            break;
        }

        let (now, rest) = remaining.split_at((block * from).min(remaining.len()));
        let count = stage.process(now, &mut scratch);

        remaining = rest;
        frames_in += count.frames_in;
        frames_out += count.frames_out;
        heard.extend_from_slice(&scratch[..count.frames_out * to]);
        tallies.push(Tally {
            frames_in,
            frames_out,
            held: stage.frames_held(),
            short: stage.frames_short_of_its_latency(),
            past: stage.frames_past_its_latency(),
        });
    }

    let frames_out_before_the_flush = frames_out;
    let mut tail = vec![0.0; stage.max_flush_frames().max(1) * to];

    let flushed = stage.flush(&mut tail);

    heard.extend_from_slice(&tail[..flushed * to]);

    Fed {
        heard,
        tallies,
        frames_in,
        frames_out_before_the_flush,
        flushed,
    }
}

fn abandoned_midway(stage: &mut dyn Processor, input: &[f64], from: usize, to: usize) {
    let mut scratch = vec![0.0; stage.max_output_frames(MAX_BLOCK) * to];
    let midway = &input[..ABANDONED_AFTER_FRAMES * from];

    for block in midway.chunks(BLOCKS[3] * from) {
        stage.process(block, &mut scratch);
    }
}

fn channel(samples: &[f64], of: usize, count: usize) -> Vec<f64> {
    samples.iter().skip(of).step_by(count).copied().collect()
}

fn norm(samples: &[f64]) -> f64 {
    samples
        .iter()
        .map(|sample| sample * sample)
        .sum::<f64>()
        .sqrt()
}

fn together_at(output: &[f64], input: &[f64], lag: isize) -> f64 {
    let (from, to) = if lag >= 0 {
        (input, output.get(lag as usize..).unwrap_or_default())
    } else {
        (input.get(lag.unsigned_abs()..).unwrap_or_default(), output)
    };

    let length = from.len().min(to.len());
    let (from, to) = (&from[..length], &to[..length]);

    let product: f64 = from.iter().zip(to).map(|(a, b)| a * b).sum();
    let size = norm(from) * norm(to);

    if size > 0.0 { product / size } else { 0.0 }
}

fn peak_lag(output: &[f64], input: &[f64], reach: usize) -> (isize, f64) {
    let length = (output.len() + input.len()).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();

    let forward = planner.plan_fft_forward(length);
    let inverse = planner.plan_fft_inverse(length);

    let spectrum = |samples: &[f64]| {
        let mut bins: Vec<Complex<f64>> = samples
            .iter()
            .map(|sample| Complex::new(*sample, 0.0))
            .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
            .take(length)
            .collect();

        forward.process(&mut bins);
        bins
    };

    let mut product: Vec<Complex<f64>> = spectrum(output)
        .iter()
        .zip(spectrum(input))
        .map(|(a, b)| a * b.conj())
        .collect();

    inverse.process(&mut product);

    let reach = reach.min(length / 2 - 1) as isize;
    let best = (-reach..=reach)
        .max_by(|a, b| {
            let at = |lag: isize| product[lag.rem_euclid(length as isize) as usize].re;
            at(*a).total_cmp(&at(*b))
        })
        .unwrap_or(0);

    (best, together_at(output, input, best))
}

struct Verdicts<'a> {
    stage: &'a Stage,
    failures: Vec<String>,
}

impl Verdicts<'_> {
    fn expect(&mut self, holds: bool, what: impl FnOnce() -> String) {
        if !holds {
            self.failures
                .push(format!("{}: {}", self.stage.name, what()));
        }
    }
}

fn frames_through(frames: usize, ratio: f64) -> f64 {
    frames as f64 * ratio
}

fn input_for(spec: StreamSpec, ratio: f64, frames: usize) -> Vec<f64> {
    let channels = usize::from(spec.channel_count().get());

    if ratio == 1.0 {
        walled_noise(spec.rate, channels, frames, SEED)
    } else {
        tones(spec.rate, channels, frames)
    }
}

fn reference_for(
    output: StreamSpec,
    input: &[f64],
    from: usize,
    ratio: f64,
    frames: usize,
) -> Option<Vec<f64>> {
    let to = usize::from(output.channel_count().get());

    if ratio != 1.0 {
        return Some(tones(
            output.rate,
            to,
            frames_through(frames, ratio).ceil() as usize,
        ));
    }
    (from == to).then(|| input.to_vec())
}

struct Shape {
    ratio: f64,
    to: usize,
}

fn check_counts(
    verdicts: &mut Verdicts,
    stage: &dyn Processor,
    fed: &Fed,
    shape: &Shape,
    lag: isize,
) {
    let rings = verdicts.stage.rings;
    let expected = frames_through(fed.frames_in, shape.ratio);
    let delivered = fed.heard.len() / shape.to;

    verdicts.expect(fed.flushed <= stage.max_flush_frames(), || {
        format!(
            "a flush gave {} frames, more than max_flush_frames {}",
            fed.flushed,
            stage.max_flush_frames()
        )
    });

    let counted = if rings {
        delivered as f64 >= expected
    } else {
        (delivered as f64 - expected).abs() <= shape.ratio.max(1.0) + ROUNDING_OF_A_RATIO
    };

    verdicts.expect(counted, || {
        format!(
            "{} frames in came out as {delivered}, expected {expected}",
            fed.frames_in
        )
    });

    for tally in &fed.tallies {
        let claimed =
            stage.latency_frames().round() as isize + tally.past as isize - tally.short as isize;

        if shape.ratio == 1.0 {
            verdicts.expect(tally.frames_in - tally.frames_out == tally.held, || {
                format!(
                    "after {} frames in and {} out it reports holding {}",
                    tally.frames_in, tally.frames_out, tally.held
                )
            });
            verdicts.expect(
                claimed == tally.held as isize + lag,
                || format!(
                    "after {} frames in it claims a latency of {claimed} frames, yet holds {} behind a lag of {lag}",
                    tally.frames_in, tally.held
                ),
            );
        }
    }
    if shape.ratio != 1.0
        && let Some(last) = fed.tallies.last()
        && frames_through(last.frames_in, shape.ratio) > stage.latency_frames() * 2.0
    {
        let in_flight =
            frames_through(last.frames_in, shape.ratio) - fed.frames_out_before_the_flush as f64;
        let claimed = stage.latency_frames() + last.past as f64 - last.short as f64;

        verdicts.expect(
            (in_flight + lag as f64 - claimed).abs() <= shape.ratio.max(1.0) + ROUNDING_OF_A_RATIO,
            || format!(
                "it claims a latency of {claimed} frames, yet holds {in_flight:.1} behind a lag of {lag}"
            ),
        );
    }
}

fn check_following(
    verdicts: &mut Verdicts,
    fed: &Fed,
    reference: &[f64],
    shape: &Shape,
    lag: isize,
) {
    for of in 0..shape.to {
        let output = channel(&fed.heard, of, shape.to);
        let wanted = channel(reference, of, shape.to);
        let beside = channel(reference, (of + 1) % shape.to, shape.to);

        let own = together_at(&output, &wanted, lag);
        let other = together_at(&output, &beside, lag);

        verdicts.expect(own >= FOLLOWS_AT_LEAST, || {
            format!("channel {of} follows its input by {own:.3} at lag {lag}")
        });
        if shape.to > 1 {
            verdicts.expect(
                own >= other + BETTER_THAN_ANOTHER_CHANNEL_BY,
                || format!("channel {of} follows its input by {own:.3} and the next channel's by {other:.3}"),
            );
        }
    }
}

fn contract_of(stage: &Stage) -> Vec<String> {
    let (mut processor, output) = stage.prepared();
    let from = usize::from(stage.input.channel_count().get());
    let to = usize::from(output.channel_count().get());
    let ratio = f64::from(output.rate.hz()) / f64::from(stage.input.rate.hz());

    let shape = Shape { ratio, to };
    let mut verdicts = Verdicts {
        stage,
        failures: Vec::new(),
    };

    let input = input_for(stage.input, ratio, LONG_FRAMES);
    let fed = feed(processor.as_mut(), &input, from, to);

    let reach = processor.latency_frames().ceil() as usize + SEARCHED_BEYOND_LATENCY;
    let reference = reference_for(output, &input, from, ratio, LONG_FRAMES);
    let lags: Vec<isize> = reference
        .iter()
        .flat_map(|reference| {
            (0..to).map(|of| {
                peak_lag(
                    &channel(&fed.heard, of, to),
                    &channel(reference, of, to),
                    reach,
                )
                .0
            })
        })
        .collect();
    let lag = lags.first().copied().unwrap_or(0);

    verdicts.expect(lags.iter().all(|other| *other == lag), || {
        format!("its channels lag the input by different frames: {lags:?}")
    });
    check_counts(&mut verdicts, processor.as_ref(), &fed, &shape, lag);
    if let Some(reference) = &reference {
        check_following(&mut verdicts, &fed, reference, &shape, lag);
    }

    abandoned_midway(processor.as_mut(), &input, from, to);
    processor.reset();

    let again = feed(processor.as_mut(), &input, from, to);

    check_counts(&mut verdicts, processor.as_ref(), &again, &shape, lag);
    if let Some(reference) = &reference {
        check_following(&mut verdicts, &again, reference, &shape, lag);
    }

    for frames in SHORT_FRAMES {
        processor.reset();

        let input = input_for(stage.input, ratio, frames);
        let fed = feed(processor.as_mut(), &input, from, to);

        check_counts(&mut verdicts, processor.as_ref(), &fed, &shape, lag);
        if frames > lag.unsigned_abs() + LONG_ENOUGH_TO_FOLLOW
            && let Some(reference) = reference_for(output, &input, from, ratio, frames)
        {
            check_following(&mut verdicts, &fed, &reference, &shape, lag);
        }
    }
    verdicts.failures
}

fn holds_for(prefix: &str) {
    let stages: Vec<Stage> = every_stage()
        .into_iter()
        .filter(|stage| stage.name.starts_with(prefix))
        .collect();

    assert!(!stages.is_empty(), "no stage is named {prefix}");

    let failures: Vec<String> = stages.iter().flat_map(contract_of).collect();

    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn overs_holds_what_it_claims() {
    holds_for("overs");
}

#[test]
fn gain_holds_what_it_claims() {
    holds_for("gain");
}

#[test]
fn dither_holds_what_it_claims() {
    holds_for("dither");
}

#[test]
fn the_equaliser_holds_what_it_claims() {
    holds_for("equaliser");
}

#[test]
fn the_convolver_holds_what_it_claims() {
    holds_for("convolver");
}

#[test]
fn the_true_peak_limiter_holds_what_it_claims() {
    holds_for("true peak");
}

#[test]
fn the_crossfeed_holds_what_it_claims() {
    holds_for("crossfeed");
}

#[test]
fn remix_holds_what_it_claims() {
    holds_for("remix");
}

#[test]
fn restoration_off_holds_what_it_claims() {
    holds_for("restore Off");
}

#[test]
fn restoration_repair_holds_what_it_claims() {
    holds_for("restore Repair");
}

#[test]
fn restoration_extend_holds_what_it_claims() {
    holds_for("restore Extend");
}

#[test]
fn the_low_latency_resampler_holds_what_it_claims() {
    holds_for("resampler LowLatency");
}

#[test]
fn the_balanced_resampler_holds_what_it_claims() {
    holds_for("resampler Balanced");
}
