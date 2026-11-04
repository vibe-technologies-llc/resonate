use std::{fmt::Write as _, fs, path::Path};

use resonate_core::SampleRate;
use resonate_dsp::{Quality, SincParams};
use soxr::params::QualityRecipe;

use crate::{
    contender::{Contender, Pair, SoxrPhase},
    cost::Cost,
    error::{Error, Result},
    summary::Measured,
};

const CD: Pair = Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000);
const DOWN_BY_TWO: Pair = Pair::new(SampleRate::HZ_96000, SampleRate::HZ_48000);
const DOWN_BY_FOUR: Pair = Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000);
const UP_BY_TWO: Pair = Pair::new(SampleRate::HZ_48000, SampleRate::HZ_96000);

fn level_named(word: &str) -> Option<Quality> {
    match word {
        "low-latency" => Some(Quality::LowLatency),
        "balanced" => Some(Quality::Balanced),
        "high" => Some(Quality::High),
        "very-high" => Some(Quality::VeryHigh),
        _ => None,
    }
}

fn parsed<T: std::str::FromStr>(word: Option<&str>, line: &str) -> Result<T> {
    word.and_then(|word| word.parse().ok())
        .ok_or_else(|| Error::UnreadableCandidate(line.to_owned()))
}

fn window_in(word: Option<&str>, line: &str) -> Result<&'static [f64]> {
    let named = word.ok_or_else(|| Error::UnreadableCandidate(line.to_owned()))?;
    if named == "-" {
        return Ok(&[]);
    }
    let text = fs::read_to_string(named).map_err(|source| Error::Candidates {
        file: named.into(),
        source,
    })?;
    let coefficients = text
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<Vec<f64>, _>>()
        .map_err(|_| Error::UnreadableCandidate(line.to_owned()))?;
    Ok(Vec::leak(coefficients))
}

fn soxr_named(word: &str) -> Option<Contender> {
    let (recipe, steep) = match word {
        "soxr-hq" => (QualityRecipe::Bits20, false),
        "soxr-24" => (QualityRecipe::Bits24, false),
        "soxr-vhq" => (QualityRecipe::Bits28, false),
        "soxr-32" => (QualityRecipe::Bits32, false),
        "soxr-steep" => (QualityRecipe::Bits28, true),
        _ => return None,
    };
    Some(Contender::Soxr {
        recipe,
        phase: SoxrPhase::Linear,
        steep,
    })
}

fn candidate(line: &str) -> Result<Contender> {
    if let Some(soxr) = soxr_named(line) {
        return Ok(soxr);
    }
    let mut words = line.split_whitespace();
    let quality = words
        .next()
        .and_then(level_named)
        .ok_or_else(|| Error::UnreadableCandidate(line.to_owned()))?;
    let half_taps: u16 = parsed(words.next(), line)?;
    let kaiser_beta: f32 = parsed(words.next(), line)?;
    let clear_taps: u16 = parsed(words.next(), line)?;
    let hop_taps: u16 = parsed(words.next(), line)?;
    let steep_hop_taps: u16 = parsed(words.next(), line)?;
    let window = window_in(words.next(), line)?;
    let phases = quality.params().phases;
    let closes_on_nyquist =
        1.0 - f64::from(kaiser_beta) / (std::f64::consts::PI * f64::from(half_taps));
    Ok(Contender::Tuned {
        quality,
        params: SincParams {
            half_taps,
            phases,
            cutoff: closes_on_nyquist as f32,
            kaiser_beta,
            rejects_db: 0.0,
            clear_taps,
            hop_taps,
            steep_hop_taps,
            window,
        },
    })
}

pub fn candidates(file: &Path) -> Result<Vec<Contender>> {
    let text = fs::read_to_string(file).map_err(|source| Error::Candidates {
        file: file.to_owned(),
        source,
    })?;
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(candidate)
        .collect()
}

fn cell(value: f64, digits: usize) -> String {
    if value.is_finite() {
        format!("{value:.digits$}")
    } else {
        "—".to_owned()
    }
}

pub fn verdicts(
    pairs: &[Pair],
    contenders: &[Contender],
    measured: &[Measured],
    costs: &[(Contender, Pair, Cost)],
) -> String {
    let mut text = String::from(
        "| Setting | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias, any pair (dB) | Worst alias by pair (dB) | Worst SNR, any pair (dB) | Null to 10 kHz (dB) | Ringing (ms) | Echo over 1 ms (dB) | Latency 44.1 → 48 (ms) | Latency 96 → 48 (ms) | CPU 44.1 → 48 | CPU 96 → 48 | CPU 192 → 48 | CPU 48 → 96 |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
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
        let latency_at = |pair: Pair| {
            everywhere
                .iter()
                .find(|measured| measured.pair == pair)
                .map_or(f64::NAN, |measured| measured.summary.latency_s * 1e3)
        };
        let summary = &cd.summary;
        let _ = writeln!(
            text,
            "| {contender} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(summary.slight_drop_hz.map_or(f64::NAN, |hz| hz / 1e3), 3),
            cell(summary.half_power_hz.map_or(f64::NAN, |hz| hz / 1e3), 3),
            cell(loudest(|measured| measured.summary.worst_rejection_db), 1),
            pairs
                .iter()
                .filter_map(|pair| everywhere.iter().find(|measured| measured.pair == *pair))
                .map(|measured| cell(measured.summary.worst_rejection_db, 0))
                .collect::<Vec<_>>()
                .join(" "),
            cell(-loudest(|measured| -measured.summary.worst_snr_db), 1),
            cell(summary.null_narrow_db, 1),
            cell(summary.impulse.after_peak_s * 1e3, 2),
            cell(summary.impulse.pre_echo_db, 2),
            cell(latency_at(CD), 2),
            cell(latency_at(DOWN_BY_TWO), 2),
            cell(cost_at(contender, CD), 3),
            cell(cost_at(contender, DOWN_BY_TWO), 3),
            cell(cost_at(contender, DOWN_BY_FOUR), 3),
            cell(cost_at(contender, UP_BY_TWO), 3),
        );
    }
    text
}
