use std::{fmt::Write as _, fs, path::Path};

use resonate_core::SampleRate;
use resonate_dsp::{Quality, SincParams};

use crate::{
    error::{Error, Result},
    lab::{
        pair::Pair,
        readings::Measured,
        runner::{Timed, cost_of},
        subject::Subject,
    },
    subjects::resampler::Resonate,
    suites::resample::{
        AFTER_PEAK, HALF_POWER, LATENCY, NULL_NARROW, PRE_ECHO, SLIGHT_DROP, WORST_REJECTION,
        WORST_SNR,
    },
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

struct Line<'a> {
    file: &'a Path,
    number: usize,
}

impl Line<'_> {
    fn unreadable(&self) -> Error {
        Error::UnreadableCandidate {
            file: self.file.to_owned(),
            line: self.number,
        }
    }

    fn parsed<T: std::str::FromStr>(&self, word: Option<&str>) -> Result<T> {
        word.and_then(|word| word.parse().ok())
            .ok_or_else(|| self.unreadable())
    }

    fn window_in(&self, word: Option<&str>) -> Result<&'static [f64]> {
        let named = word.ok_or_else(|| self.unreadable())?;

        if named == "-" {
            return Ok(&[]);
        }

        let text = fs::read_to_string(named).map_err(|source| Error::Read {
            path: named.into(),
            source,
        })?;
        let coefficients = text
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<Vec<f64>, _>>()
            .map_err(|_| self.unreadable())?;
        Ok(Vec::leak(coefficients))
    }
}

#[cfg(feature = "soxr")]
fn recipe_named(word: &str) -> Option<Box<dyn Subject>> {
    crate::subjects::soxr::named(word).map(|recipe| Box::new(recipe) as Box<dyn Subject>)
}

#[cfg(not(feature = "soxr"))]
fn recipe_named(_word: &str) -> Option<Box<dyn Subject>> {
    None
}

fn candidate(text: &str, line: &Line<'_>) -> Result<Box<dyn Subject>> {
    if let Some(recipe) = recipe_named(text) {
        return Ok(recipe);
    }

    let mut words = text.split_whitespace();

    let quality = words
        .next()
        .and_then(level_named)
        .ok_or_else(|| line.unreadable())?;
    let half_taps: u16 = line.parsed(words.next())?;
    let kaiser_beta: f32 = line.parsed(words.next())?;
    let clear_taps: u16 = line.parsed(words.next())?;
    let hop_taps: u16 = line.parsed(words.next())?;
    let steep_hop_taps: u16 = line.parsed(words.next())?;
    let window = line.window_in(words.next())?;
    let phases = quality.params().phases;

    let closes_on_nyquist =
        1.0 - f64::from(kaiser_beta) / (std::f64::consts::PI * f64::from(half_taps));
    Ok(Box::new(Resonate::Tuned {
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
    }))
}

pub fn candidates(file: &Path) -> Result<Vec<Box<dyn Subject>>> {
    let text = fs::read_to_string(file).map_err(|source| Error::Read {
        path: file.to_owned(),
        source,
    })?;
    text.lines()
        .enumerate()
        .map(|(index, text)| (index + 1, text.trim()))
        .filter(|(_, text)| !text.is_empty() && !text.starts_with('#'))
        .map(|(number, text)| candidate(text, &Line { file, number }))
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
    subjects: &[Box<dyn Subject>],
    measured: &[Measured],
    costs: &[Timed],
) -> String {
    let mut text = String::from(
        "| Setting | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias, any pair (dB) | Worst alias by pair (dB) | Worst SNR, any pair (dB) | Null to 10 kHz (dB) | Ringing by pair (ms) | Echo over 1 ms by pair (dB) | Latency by pair (ms) | CPU 44.1 → 48 | CPU 96 → 48 | CPU 192 → 48 | CPU 48 → 96 |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );

    for subject in subjects {
        let name = subject.name();
        let cost_at =
            |pair: Pair| cost_of(costs, &name, pair).map_or(f64::NAN, |cost| cost.share_of_a_core);
        let everywhere: Vec<&Measured> = measured
            .iter()
            .filter(|measured| measured.subject == name)
            .collect();
        let Some(cd) = everywhere.iter().find(|measured| measured.pair == CD) else {
            continue;
        };
        let loudest = |read: &dyn Fn(&Measured) -> f64| {
            everywhere
                .iter()
                .map(|measured| read(measured))
                .filter(|value| value.is_finite())
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let by_pair = |read: &dyn Fn(&Measured) -> f64, digits: usize| {
            pairs
                .iter()
                .filter_map(|pair| everywhere.iter().find(|measured| measured.pair == *pair))
                .map(|measured| cell(read(measured), digits))
                .collect::<Vec<_>>()
                .join(" ")
        };

        let _ = writeln!(
            text,
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(cd.value(&SLIGHT_DROP) / 1e3, 3),
            cell(cd.value(&HALF_POWER) / 1e3, 3),
            cell(loudest(&|measured| measured.value(&WORST_REJECTION)), 1),
            by_pair(&|measured| measured.value(&WORST_REJECTION), 0),
            cell(-loudest(&|measured| -measured.value(&WORST_SNR)), 1),
            cell(cd.value(&NULL_NARROW), 1),
            by_pair(&|measured| measured.value(&AFTER_PEAK) * 1e3, 2),
            by_pair(&|measured| measured.value(&PRE_ECHO), 2),
            by_pair(&|measured| measured.value(&LATENCY) * 1e3, 2),
            cell(cost_at(CD), 4),
            cell(cost_at(DOWN_BY_TWO), 4),
            cell(cost_at(DOWN_BY_FOUR), 4),
            cell(cost_at(UP_BY_TWO), 4),
        );
    }
    text
}
