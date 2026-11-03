use std::{fmt::Write as _, thread};

use resonate_core::SampleRate;
use resonate_dsp::{FilterPhase, Quality, SincParams};
use soxr::params::QualityRecipe;

use crate::{
    contender::{Contender, Pair, SoxrPhase},
    cost::Cost,
    error::Result,
    measure::{self, Tone},
    summary::Measured,
};

const KAISER_BETA_PER_DECIBEL: f64 = 0.1102;
const KAISER_DECIBELS_AT_NO_BETA: f64 = 8.7;
const ABOVE_ITS_STOPBAND_AT_MOST_DB: f64 = 1.0;
const CUTOFF_STEP: f32 = 0.0025;
const LOWEST_CUTOFF: f32 = 0.70;
const LONGEST_BY: u16 = 4;
const TUNED: [Quality; 2] = [Quality::Fast, Quality::Balanced];
const COMPARED: [QualityRecipe; 4] = [
    QualityRecipe::Low,
    QualityRecipe::Medium,
    QualityRecipe::Bits16,
    QualityRecipe::Bits20,
];
const CD: Pair = Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000);
const DOWN_BY_FOUR: Pair = Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000);

fn stopband_db(params: SincParams) -> f64 {
    -(f64::from(params.kaiser_beta) / KAISER_BETA_PER_DECIBEL + KAISER_DECIBELS_AT_NO_BETA)
}

fn loudest_past_nyquist(tones: &[Tone]) -> f64 {
    tones
        .iter()
        .flat_map(|tone| {
            [
                tone.leak.map(|leak| leak.level_db),
                tone.image.map(|image| image.level_db),
            ]
        })
        .flatten()
        .fold(f64::NEG_INFINITY, f64::max)
}

fn worst_past_nyquist(contender: Contender, pairs: &[Pair]) -> Result<f64> {
    let per_pair: Vec<Result<f64>> = thread::scope(|scope| {
        let handles: Vec<_> = pairs
            .iter()
            .map(|&pair| {
                scope.spawn(move || {
                    let tones = measure::sweep(pair)
                        .into_iter()
                        .map(|hz| measure::tone(contender, pair, hz))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(loudest_past_nyquist(&tones))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(Ok(f64::INFINITY)))
            .collect()
    });
    let mut worst = f64::NEG_INFINITY;
    for level in per_pair {
        worst = worst.max(level?);
    }
    Ok(worst)
}

fn holds(quality: Quality, params: SincParams, pairs: &[Pair]) -> Result<bool> {
    let worst = worst_past_nyquist(Contender::Tuned { quality, params }, pairs)?;
    let allowed = stopband_db(params) + ABOVE_ITS_STOPBAND_AT_MOST_DB;
    eprintln!(
        "{quality:?}: {} taps a side, cutoff {:.4}: worst past Nyquist {worst:.1} dB against {allowed:.1} dB",
        params.half_taps, params.cutoff
    );
    Ok(worst <= allowed)
}

fn narrowed(quality: Quality, pairs: &[Pair]) -> Result<SincParams> {
    let mut params = quality.params();
    while params.cutoff > LOWEST_CUTOFF && !holds(quality, params, pairs)? {
        params.cutoff -= CUTOFF_STEP;
    }
    Ok(params)
}

fn lengthened(quality: Quality, pairs: &[Pair]) -> Result<SincParams> {
    let mut params = quality.params();
    let longest = params.half_taps * LONGEST_BY;
    while params.half_taps < longest && !holds(quality, params, pairs)? {
        params.half_taps += 1;
    }
    Ok(params)
}

pub fn contenders(pairs: &[Pair]) -> Result<Vec<Contender>> {
    let mut contenders = Vec::new();
    for quality in TUNED {
        contenders.push(Contender::Ours {
            quality,
            phase: FilterPhase::Linear,
        });
        contenders.push(Contender::Tuned {
            quality,
            params: narrowed(quality, pairs)?,
        });
        contenders.push(Contender::Tuned {
            quality,
            params: lengthened(quality, pairs)?,
        });
    }
    contenders.extend(COMPARED.map(|recipe| Contender::Soxr {
        recipe,
        phase: SoxrPhase::Linear,
        steep: false,
    }));
    Ok(contenders)
}

fn cell(value: f64, digits: usize) -> String {
    if value.is_finite() {
        format!("{value:.digits$}")
    } else {
        "—".to_owned()
    }
}

pub fn verdicts(
    contenders: &[Contender],
    measured: &[Measured],
    costs: &[(Contender, Pair, Cost)],
) -> String {
    let mut text = String::from(
        "### Fast and Balanced: leave, narrow or lengthen\n\n| Setting | Worst alias or image, any pair (dB) | Worst landing ≤ 20 kHz, any pair (dB) | −0.1 dB (kHz) | −3 dB (kHz) | At 20 kHz (dB) | Worst SNR, any pair (dB) | Null 20 Hz–10 kHz (dB) | Latency (ms) | CPU 44.1 → 48 (%) | CPU 192 → 48 (%) |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    let cost_at = |contender: Contender, pair: Pair| {
        costs
            .iter()
            .find(|(held, at, _)| *held == contender && *at == pair)
            .map_or(f64::NAN, |(_, _, cost)| cost.share_of_a_core)
    };

    for &contender in contenders {
        let everywhere: Vec<&Measured> = measured
            .iter()
            .filter(|measured| measured.contender == contender)
            .collect();
        let Some(cd) = everywhere.iter().find(|measured| measured.pair == CD) else {
            continue;
        };
        let loudest = |read: fn(&Measured) -> f64| {
            everywhere
                .iter()
                .map(|measured| read(measured))
                .filter(|value| value.is_finite())
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let summary = &cd.summary;
        let name = if contender.is_ours() {
            format!("**{contender}**")
        } else {
            contender.to_string()
        };
        let _ = writeln!(
            text,
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(loudest(|measured| measured.summary.worst_rejection_db), 1),
            cell(
                loudest(|measured| measured.summary.worst_audible_rejection_db),
                1
            ),
            cell(summary.slight_drop_hz.map_or(f64::NAN, |hz| hz / 1e3), 2),
            cell(summary.half_power_hz.map_or(f64::NAN, |hz| hz / 1e3), 2),
            cell(summary.at_audible_db, 2),
            cell(-loudest(|measured| -measured.summary.worst_snr_db), 1),
            cell(summary.null_narrow_db, 1),
            cell(summary.latency_s * 1e3, 2),
            cell(cost_at(contender, CD), 3),
            cell(cost_at(contender, DOWN_BY_FOUR), 3),
        );
    }
    text.push('\n');
    text
}
