use std::{
    f64::consts::{PI, SQRT_2, TAU},
    iter, slice,
};

use crate::{
    contender::{Channels, Contender, Pair, Stream, resampled},
    error::Result,
    fit::Window,
    signal::{self, Multitone, TONE_AMPLITUDE, folded, on_the_grid},
};

const INPUT_SECONDS: f64 = 1.2;
const SETTLED_SECONDS: f64 = 0.3;
const IMPULSE_SECONDS: f64 = 1.0;
const STREAMED_SECONDS: f64 = 0.6;
const RING_FLOOR: f64 = 1e-6;
const PRE_ECHO_SECONDS: f64 = 1e-3;
const IMAGE_APART_HZ: f64 = 25.0;
const MEASURING_BLOCK: usize = 4_096;
const NULL_TONES: usize = 48;
const NULL_SEED: u64 = 0x5265_736F_6E61_7465;
const QUIETEST_AMPLITUDE: f64 = 1e-30;
const LOG_SWEEP_POINTS: usize = 64;
const EDGE_SWEEP_POINTS: usize = 48;
const PAST_NYQUIST_NEAR_POINTS: usize = 24;
const PAST_NYQUIST_FAR_POINTS: usize = 40;
const LOG_SWEEP_UNTIL_EDGE: f64 = 0.85;
const EDGE_SWEEP_UNTIL: f64 = 0.999;
const PAST_NYQUIST_FROM: f64 = 1.001;
const PAST_NYQUIST_NEAR_UNTIL: f64 = 1.2;
const PAST_NYQUIST_FAR_UNTIL: f64 = 0.995;
pub const LOWEST_HZ: f64 = 20.0;
pub const AUDIBLE_HZ: f64 = 20_000.0;
pub const MIDDLE_HZ: f64 = 1_000.0;

pub fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.max(QUIETEST_AMPLITUDE).log10()
}

#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub gain_db: f64,
    pub phase: f64,
    pub spurious_db: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Leak {
    pub level_db: f64,
    pub lands_hz: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Image {
    pub level_db: f64,
    pub at_hz: f64,
    pub lands_hz: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Tone {
    pub hz: f64,
    pub heard: Option<Heard>,
    pub leak: Option<Leak>,
    pub image: Option<Image>,
}

#[derive(Clone, Copy, Debug)]
pub struct Impulse {
    pub peak_offset_s: f64,
    pub before_peak_s: f64,
    pub after_peak_s: f64,
    pub pre_echo_db: f64,
}

fn spaced(from: f64, until: f64, points: usize, logarithmic: bool) -> impl Iterator<Item = f64> {
    (0..points).map(move |index| {
        let along = index as f64 / (points - 1) as f64;
        if logarithmic {
            from * (until / from).powf(along)
        } else {
            from + (until - from) * along
        }
    })
}

pub fn sweep(pair: Pair) -> Vec<f64> {
    let edge = pair.band_edge();
    let nyquist = pair.output_nyquist();
    let top = pair.input_nyquist() * PAST_NYQUIST_FAR_UNTIL;
    let mut hz: Vec<f64> = spaced(
        LOWEST_HZ,
        LOG_SWEEP_UNTIL_EDGE * edge,
        LOG_SWEEP_POINTS,
        true,
    )
    .chain(spaced(
        LOG_SWEEP_UNTIL_EDGE * edge,
        EDGE_SWEEP_UNTIL * edge,
        EDGE_SWEEP_POINTS,
        false,
    ))
    .chain([MIDDLE_HZ, AUDIBLE_HZ])
    .collect();

    if pair.is_down() {
        let near = (PAST_NYQUIST_NEAR_UNTIL * nyquist).min(top);
        hz.extend(spaced(
            PAST_NYQUIST_FROM * nyquist,
            near,
            PAST_NYQUIST_NEAR_POINTS,
            false,
        ));
        if near < top {
            hz.extend(spaced(near, top, PAST_NYQUIST_FAR_POINTS, false));
        }
    }

    let mut hz: Vec<f64> = hz.into_iter().map(on_the_grid).collect();
    hz.sort_by(f64::total_cmp);
    hz.dedup();
    hz
}

fn settled(output: &[f64], pair: Pair, seconds: f64) -> Window<'_> {
    let rate = pair.output_hz();
    let settle = (SETTLED_SECONDS * rate) as usize;
    let last = ((seconds * rate) as usize).min(output.len()) - settle;

    Window {
        samples: &output[settle..last],
        first: settle,
        rate,
    }
}

pub fn tone(contender: Contender, pair: Pair, hz: f64) -> Result<Tone> {
    let frames = (INPUT_SECONDS * pair.input_hz()) as usize;
    let input = signal::tone(hz, pair.input_hz(), frames);
    let output = resampled(contender, pair, &input, MEASURING_BLOCK)?;
    let window = settled(&output, pair, INPUT_SECONDS);
    let reference = TONE_AMPLITUDE / SQRT_2;

    if hz >= pair.output_nyquist() {
        return Ok(Tone {
            hz,
            heard: None,
            leak: Some(Leak {
                level_db: decibels(window.rms() / reference),
                lands_hz: folded(hz, pair.output_hz()),
            }),
            image: None,
        });
    }

    let image_at = Some(pair.input_hz() - hz);
    let lands = image_at
        .map(|at| folded(at, pair.output_hz()))
        .filter(|&lands| {
            (lands - hz).abs() > IMAGE_APART_HZ
                && lands > IMAGE_APART_HZ
                && lands < pair.output_nyquist() - IMAGE_APART_HZ
        });
    let modelled: Vec<f64> = iter::once(hz).chain(lands).collect();
    let fitted = window.fitted(&modelled);
    let spurious = window.rms_without(&modelled[..1], &fitted[..1]);

    Ok(Tone {
        hz,
        heard: Some(Heard {
            gain_db: decibels(fitted[0].amplitude() / TONE_AMPLITUDE),
            phase: fitted[0].phase(),
            spurious_db: decibels(spurious / reference),
        }),
        leak: None,
        image: image_at.zip(lands).map(|(at_hz, lands_hz)| Image {
            level_db: decibels(fitted[1].amplitude() / TONE_AMPLITUDE),
            at_hz,
            lands_hz,
        }),
    })
}

fn loudest(samples: &[f64]) -> usize {
    samples
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map_or(0, |(index, _)| index)
}

pub fn impulse(contender: Contender, pair: Pair) -> Result<Impulse> {
    let frames = (IMPULSE_SECONDS * pair.input_hz()) as usize;
    let at = frames / 2;
    let mut input = vec![0.0; frames];
    input[at] = 1.0;
    let output = resampled(contender, pair, &input, MEASURING_BLOCK)?;

    let peak = loudest(&output);
    let floor = output[peak].abs() * RING_FLOOR;
    let first = output
        .iter()
        .position(|sample| sample.abs() > floor)
        .unwrap_or(peak);
    let last = output
        .iter()
        .rposition(|sample| sample.abs() > floor)
        .unwrap_or(peak);
    let total: f64 = output.iter().map(|sample| sample * sample).sum();
    let rate = pair.output_hz();
    let echo_ends = peak.saturating_sub((PRE_ECHO_SECONDS * rate) as usize);
    let echo: f64 = output[..echo_ends]
        .iter()
        .map(|sample| sample * sample)
        .sum();

    Ok(Impulse {
        peak_offset_s: peak as f64 / rate - at as f64 / pair.input_hz(),
        before_peak_s: (peak - first) as f64 / rate,
        after_peak_s: (last - peak) as f64 / rate,
        pre_echo_db: 10.0 * (echo / total).max(QUIETEST_AMPLITUDE).log10(),
    })
}

pub fn latency(contender: Contender, pair: Pair) -> Result<f64> {
    let frames = (STREAMED_SECONDS * pair.input_hz()) as usize;
    let at = frames / 2;
    let mut input = vec![0.0; frames];
    input[at] = 1.0;
    let mut stream = Stream::open(contender, pair, Channels::Mono, 1)?;
    let mut produced = Vec::new();
    let mut fed_when = Vec::new();

    for (index, sample) in input.iter().enumerate() {
        stream.push(slice::from_ref(sample), &mut produced)?;
        fed_when.resize(produced.len(), index + 1);
    }
    stream.finish(&mut produced)?;
    fed_when.resize(produced.len(), frames);

    let peak = loudest(&produced);
    Ok((fed_when[peak] - (at + 1)) as f64 / pair.input_hz())
}

pub fn null(contender: Contender, pair: Pair, highest_hz: f64, delay_s: f64) -> Result<f64> {
    let multitone = Multitone::log_spaced(LOWEST_HZ, highest_hz, NULL_TONES, NULL_SEED);
    let frames = (INPUT_SECONDS * pair.input_hz()) as usize;
    let input = multitone.rendered(pair.input_hz(), frames);
    let output = resampled(contender, pair, &input, MEASURING_BLOCK)?;
    let window = settled(&output, pair, INPUT_SECONDS);

    let (error, ideal) =
        window
            .samples
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(error, ideal), (offset, &sample)| {
                let wanted = multitone.at(window.first + offset, window.rate, delay_s);
                (error + (sample - wanted).powi(2), ideal + wanted * wanted)
            });

    Ok(decibels((error / ideal).sqrt()))
}

pub fn phase_delays(tones: &[Tone], coarse_s: f64) -> Vec<(f64, f64)> {
    let mut previous: Option<f64> = None;
    tones
        .iter()
        .filter(|tone| (LOWEST_HZ..=AUDIBLE_HZ).contains(&tone.hz))
        .filter_map(|tone| tone.heard.map(|heard| (tone.hz, heard.phase)))
        .map(|(hz, phase)| {
            let angular = TAU * hz;
            let residual = phase + angular * coarse_s;
            let unwrapped = match previous {
                None => (residual + PI).rem_euclid(TAU) - PI,
                Some(before) => residual - TAU * ((residual - before) / TAU).round(),
            };
            previous = Some(unwrapped);
            (hz, coarse_s - unwrapped / angular)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sweep_is_sorted_on_the_grid_and_reaches_past_nyquist_only_going_down() {
        let up = sweep(Pair::new(
            resonate_core::SampleRate::HZ_44100,
            resonate_core::SampleRate::HZ_48000,
        ));
        let down = sweep(Pair::new(
            resonate_core::SampleRate::HZ_96000,
            resonate_core::SampleRate::HZ_48000,
        ));

        assert!(up.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(up.contains(&MIDDLE_HZ) && up.contains(&AUDIBLE_HZ));
        assert!(up.iter().all(|&hz| hz < 22_050.0));
        assert!(down.iter().any(|&hz| hz > 40_000.0));
        assert!(down.iter().all(|&hz| (hz * 64.0).fract() == 0.0));
    }
}
