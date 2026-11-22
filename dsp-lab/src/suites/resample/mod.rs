mod measure;
mod summary;
pub mod tuning;

use std::{fmt::Write as _, path::Path};

use resonate_core::SampleRate;
use resonate_dsp::{FilterPhase, Quality};

use crate::{
    error::Result,
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        readings::{Curve, Measured, find},
        runner::Timed,
        subject::{Subject, SubjectName},
        suite::{Outcome, Suite},
    },
    report::{
        markdown::{self, DASH, header, named, row},
        plot::{Across, Frame, SERIES_CLASSES, Series, chart, written},
    },
    subjects::resampler::Resonate,
    suites::resample::{
        measure::{AUDIBLE_HZ, Tone},
        summary::{Summary, summarised},
    },
};

pub const PAIRS: [Pair; 7] = [
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_48000, SampleRate::HZ_44100),
    Pair::new(SampleRate::HZ_96000, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_96000, SampleRate::HZ_44100),
    Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_96000),
    Pair::new(SampleRate::HZ_48000, SampleRate::HZ_96000),
];

const BUILDS_SHOWN: [Pair; 2] = [
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000),
];

const LEVELS: [Resonate; 8] = [
    Resonate::at(Quality::LowLatency, FilterPhase::Linear),
    Resonate::at(Quality::Balanced, FilterPhase::Linear),
    Resonate::at(Quality::High, FilterPhase::Linear),
    Resonate::at(Quality::VeryHigh, FilterPhase::Linear),
    Resonate::at(Quality::High, FilterPhase::Intermediate),
    Resonate::at(Quality::High, FilterPhase::Minimum),
    Resonate::at(Quality::VeryHigh, FilterPhase::Intermediate),
    Resonate::at(Quality::VeryHigh, FilterPhase::Minimum),
];

const RIPPLE_SHOWN_TO_DB: f64 = 0.000_1;
const ECHO_SHOWN_TO_DB: f64 = -200.0;
const RESPONSE: &str = "response";

pub const RIPPLE: Metric = Metric::new(
    "ripple-db",
    "Ripple 20 Hz–10 kHz (dB)",
    Better::Lower,
    Tolerance::SHARE,
    Shown::Scientific,
);
pub const DEVIATION: Metric = Metric::new(
    "deviation-db",
    "Deviation 20 Hz–20 kHz (dB)",
    Better::Lower,
    Tolerance::SHARE,
    Shown::Digits(4),
);
pub const AT_AUDIBLE: Metric = Metric::new(
    "at-20-khz-db",
    "At 20 kHz (dB)",
    Better::Higher,
    Tolerance::FINE_DECIBELS,
    Shown::Decibels,
);
pub const SLIGHT_DROP: Metric = Metric::new(
    "slight-drop-hz",
    "−0.1 dB (kHz)",
    Better::Higher,
    Tolerance::HERTZ,
    Shown::Kilohertz,
);
pub const HALF_POWER: Metric = Metric::new(
    "half-power-hz",
    "−3 dB (kHz)",
    Better::Higher,
    Tolerance::HERTZ,
    Shown::Kilohertz,
);
pub const WORST_REJECTION: Metric = Metric::new(
    "worst-rejection-db",
    "Worst alias or image (dB)",
    Better::Lower,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const WORST_AUDIBLE_REJECTION: Metric = Metric::new(
    "worst-audible-rejection-db",
    "Worst landing ≤ 20 kHz (dB)",
    Better::Lower,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const SNR_MIDDLE: Metric = Metric::new(
    "snr-1-khz-db",
    "SNR at 1 kHz (dB)",
    Better::Higher,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const WORST_SNR: Metric = Metric::new(
    "worst-snr-db",
    "Worst SNR 20 Hz–20 kHz (dB)",
    Better::Higher,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const NULL_WIDE: Metric = Metric::new(
    "null-20-khz-db",
    "Null 20 Hz–20 kHz (dB)",
    Better::Lower,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const NULL_NARROW: Metric = Metric::new(
    "null-10-khz-db",
    "Null 20 Hz–10 kHz (dB)",
    Better::Lower,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const DELAY_MIDDLE: Metric = Metric::new(
    "delay-1-khz-s",
    "Delay at 1 kHz (µs)",
    Better::Same,
    Tolerance::SECONDS,
    Shown::Microseconds,
);
pub const DELAY_SPREAD: Metric = Metric::new(
    "delay-spread-s",
    "Delay spread 20 Hz–20 kHz (µs)",
    Better::Lower,
    Tolerance::SECONDS,
    Shown::Microseconds,
);
pub const PEAK_OFFSET: Metric = Metric::new(
    "peak-offset-s",
    "Impulse peak offset (µs)",
    Better::Same,
    Tolerance::SECONDS,
    Shown::Microseconds,
);
pub const BEFORE_PEAK: Metric = Metric::new(
    "ringing-before-s",
    "Ringing before peak (ms)",
    Better::Lower,
    Tolerance::SECONDS,
    Shown::Milliseconds,
);
pub const AFTER_PEAK: Metric = Metric::new(
    "ringing-after-s",
    "Ringing after peak (ms)",
    Better::Lower,
    Tolerance::SECONDS,
    Shown::Milliseconds,
);
pub const PRE_ECHO: Metric = Metric::new(
    "pre-echo-db",
    "Energy over 1 ms before peak (dB)",
    Better::Lower,
    Tolerance::LEVEL,
    Shown::Decibels,
);
pub const LATENCY: Metric = Metric::new(
    "latency-s",
    "Streaming latency (ms)",
    Better::Lower,
    Tolerance::SECONDS,
    Shown::Milliseconds,
);

const METRICS: [&Metric; 18] = [
    &RIPPLE,
    &DEVIATION,
    &AT_AUDIBLE,
    &SLIGHT_DROP,
    &HALF_POWER,
    &WORST_REJECTION,
    &WORST_AUDIBLE_REJECTION,
    &SNR_MIDDLE,
    &WORST_SNR,
    &NULL_WIDE,
    &NULL_NARROW,
    &DELAY_MIDDLE,
    &DELAY_SPREAD,
    &PEAK_OFFSET,
    &BEFORE_PEAK,
    &AFTER_PEAK,
    &PRE_ECHO,
    &LATENCY,
];

fn readings(summary: &Summary) -> Vec<Reading> {
    vec![
        Reading::of(&RIPPLE, summary.ripple_db),
        Reading::of(&DEVIATION, summary.deviation_db),
        Reading::of(&AT_AUDIBLE, summary.at_audible_db),
        Reading::of(&SLIGHT_DROP, summary.slight_drop_hz.unwrap_or(f64::NAN)),
        Reading::of(&HALF_POWER, summary.half_power_hz.unwrap_or(f64::NAN)),
        Reading::of(&WORST_REJECTION, summary.worst_rejection_db),
        Reading::of(&WORST_AUDIBLE_REJECTION, summary.worst_audible_rejection_db),
        Reading::of(&SNR_MIDDLE, summary.snr_middle_db),
        Reading::of(&WORST_SNR, summary.worst_snr_db),
        Reading::of(&NULL_WIDE, summary.null_wide_db),
        Reading::of(&NULL_NARROW, summary.null_narrow_db),
        Reading::of(&DELAY_MIDDLE, summary.delay_middle_s),
        Reading::of(&DELAY_SPREAD, summary.delay_spread_s),
        Reading::of(&PEAK_OFFSET, summary.impulse.peak_offset_s),
        Reading::of(&BEFORE_PEAK, summary.impulse.before_peak_s),
        Reading::of(&AFTER_PEAK, summary.impulse.after_peak_s),
        Reading::of(&PRE_ECHO, summary.impulse.pre_echo_db),
        Reading::of(&LATENCY, summary.latency_s),
    ]
}

fn response(tones: &[Tone]) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = tones
        .iter()
        .flat_map(|tone| {
            let heard = tone.heard.map(|heard| (tone.hz, heard.gain_db));
            let leak = tone.leak.map(|leak| (tone.hz, leak.level_db));
            let image = tone.image.map(|image| (image.at_hz, image.level_db));
            [heard, leak, image].into_iter().flatten()
        })
        .collect();

    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points
}

pub fn everyone() -> Vec<Box<dyn Subject>> {
    LEVELS
        .iter()
        .map(|&level| Box::new(level) as Box<dyn Subject>)
        .chain(crate::subjects::references())
        .collect()
}

fn plotted() -> Vec<SubjectName> {
    [
        Resonate::at(Quality::High, FilterPhase::Linear).name(),
        Resonate::at(Quality::VeryHigh, FilterPhase::Linear).name(),
    ]
    .into_iter()
    .chain(crate::subjects::plotted_references())
    .collect()
}

pub struct Resample;

impl Suite for Resample {
    fn name(&self) -> &'static str {
        "resample"
    }

    fn title(&self) -> &'static str {
        "Resampling"
    }

    fn about(&self) -> &'static str {
        "the resampler's levels and phases against SoXR's recipes and rubato, tone by tone"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(everyone())
    }

    fn pairs(&self) -> Vec<Pair> {
        PAIRS.to_vec()
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let summarised = summarised(subject, pair)?;

        Ok(Outcome {
            readings: readings(&summarised.summary),
            curves: vec![Curve {
                name: RESPONSE,
                points: response(&summarised.tones),
            }],
        })
    }

    fn report(&self, subjects: &[Box<dyn Subject>], measured: &[Measured]) -> String {
        PAIRS
            .iter()
            .map(|&pair| quality(subjects, pair, measured))
            .collect()
    }

    fn plotted(&self, directory: &Path, measured: &[Measured]) -> Result<()> {
        for pair in PAIRS {
            plotted_at(directory, pair, measured)?;
        }
        Ok(())
    }

    fn built_pairs(&self) -> Vec<Pair> {
        BUILDS_SHOWN.to_vec()
    }

    fn costs(&self, subjects: &[Box<dyn Subject>], costs: &[Timed]) -> String {
        markdown::costs(
            "percent of one core per second of stereo audio",
            subjects,
            &PAIRS,
            &BUILDS_SHOWN,
            costs,
        )
    }
}

fn decibels(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.1}")
    } else {
        DASH.to_owned()
    }
}

fn echo(value: f64) -> String {
    if value < ECHO_SHOWN_TO_DB {
        format!("< {ECHO_SHOWN_TO_DB}")
    } else {
        format!("{value:.1}")
    }
}

fn deviation(value: f64) -> String {
    if value < RIPPLE_SHOWN_TO_DB {
        format!("< {RIPPLE_SHOWN_TO_DB}")
    } else {
        format!("{value:.4}")
    }
}

fn ripple(value: f64) -> String {
    format!("{value:.1e}")
}

fn kilohertz(value: f64) -> String {
    if value.is_nan() {
        DASH.to_owned()
    } else {
        format!("{:.2}", value / 1_000.0)
    }
}

fn microseconds(seconds: f64) -> String {
    format!("{:.1}", seconds * 1e6)
}

fn milliseconds(seconds: f64) -> String {
    format!("{:.2}", seconds * 1e3)
}

fn quality(subjects: &[Box<dyn Subject>], pair: Pair, measured: &[Measured]) -> String {
    let rows: Vec<&Measured> = subjects
        .iter()
        .filter_map(|subject| find(measured, &subject.name(), pair))
        .collect();
    let mut text = String::new();

    let _ = writeln!(text, "### Frequency domain, {pair}\n");

    text.push_str(&header(&[
        "Setting",
        RIPPLE.title,
        DEVIATION.title,
        AT_AUDIBLE.title,
        SLIGHT_DROP.title,
        HALF_POWER.title,
        WORST_REJECTION.title,
        WORST_AUDIBLE_REJECTION.title,
        SNR_MIDDLE.title,
        WORST_SNR.title,
        NULL_WIDE.title,
        NULL_NARROW.title,
    ]));
    for case in &rows {
        text.push_str(&row(&[
            named(&case.subject, case.is_ours()),
            ripple(case.value(&RIPPLE)),
            deviation(case.value(&DEVIATION)),
            decibels(case.value(&AT_AUDIBLE)),
            kilohertz(case.value(&SLIGHT_DROP)),
            kilohertz(case.value(&HALF_POWER)),
            decibels(case.value(&WORST_REJECTION)),
            decibels(case.value(&WORST_AUDIBLE_REJECTION)),
            decibels(case.value(&SNR_MIDDLE)),
            decibels(case.value(&WORST_SNR)),
            decibels(case.value(&NULL_WIDE)),
            decibels(case.value(&NULL_NARROW)),
        ]));
    }

    let _ = writeln!(text, "\n### Time domain, {pair}\n");

    text.push_str(&header(&[
        "Setting",
        DELAY_MIDDLE.title,
        DELAY_SPREAD.title,
        PEAK_OFFSET.title,
        BEFORE_PEAK.title,
        AFTER_PEAK.title,
        PRE_ECHO.title,
        LATENCY.title,
    ]));
    for case in &rows {
        text.push_str(&row(&[
            named(&case.subject, case.is_ours()),
            microseconds(case.value(&DELAY_MIDDLE)),
            microseconds(case.value(&DELAY_SPREAD)),
            microseconds(case.value(&PEAK_OFFSET)),
            milliseconds(case.value(&BEFORE_PEAK)),
            milliseconds(case.value(&AFTER_PEAK)),
            echo(case.value(&PRE_ECHO)),
            milliseconds(case.value(&LATENCY)),
        ]));
    }
    text.push('\n');
    text
}

fn plotted_at(directory: &Path, pair: Pair, measured: &[Measured]) -> Result<()> {
    let series: Vec<Series> = plotted()
        .iter()
        .zip(SERIES_CLASSES)
        .filter_map(|(name, class)| {
            find(measured, name, pair)
                .and_then(|case| case.curve(RESPONSE))
                .map(|curve| Series {
                    name: name.to_string(),
                    class,
                    points: curve.points.clone(),
                })
        })
        .collect();

    let reach = series
        .iter()
        .flat_map(|line| line.points.iter().map(|&(hz, _)| hz))
        .fold(pair.band_edge(), f64::max);
    let whole = Frame {
        from_hz: 0.0,
        to_hz: (reach / 1_000.0).ceil() * 1_000.0,
        floor_db: -340.0,
        ceiling_db: 10.0,
        across: Across::Kilohertz,
    };
    let markers = [
        (AUDIBLE_HZ, "20 kHz"),
        (pair.output_nyquist(), "output Nyquist"),
        (pair.input_nyquist(), "input Nyquist"),
    ];

    written(
        &directory.join(format!("response-{}.svg", pair.slug())),
        &chart(
            &format!("Magnitude response and rejection, {pair}"),
            &whole,
            &markers,
            &series,
        ),
    )?;

    let edge = pair.band_edge();
    let passband = Frame {
        from_hz: 0.6 * edge,
        to_hz: edge,
        floor_db: -3.5,
        ceiling_db: 0.5,
        across: Across::Kilohertz,
    };
    written(
        &directory.join(format!("passband-{}.svg", pair.slug())),
        &chart(
            &format!("Top of the passband, {pair}"),
            &passband,
            &markers,
            &series,
        ),
    )
}
