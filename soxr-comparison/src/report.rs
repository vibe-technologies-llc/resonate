use std::{fmt::Write as _, time::Duration};

use crate::{
    contender::{Contender, Pair},
    cost::Cost,
    summary::Measured,
};

const RIPPLE_SHOWN_TO_DB: f64 = 0.000_1;
const DASH: &str = "—";
const ECHO_SHOWN_TO_DB: f64 = -200.0;

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

fn kilohertz(value: Option<f64>) -> String {
    value.map_or_else(|| DASH.to_owned(), |hz| format!("{:.2}", hz / 1_000.0))
}

fn microseconds(seconds: f64) -> String {
    format!("{:.1}", seconds * 1e6)
}

fn milliseconds(seconds: f64) -> String {
    format!("{:.2}", seconds * 1e3)
}

fn duration(value: Duration) -> String {
    format!("{:.2}", value.as_secs_f64() * 1e3)
}

fn row(cells: &[String]) -> String {
    format!("| {} |\n", cells.join(" | "))
}

fn header(cells: &[&str]) -> String {
    let mut text = row(&cells
        .iter()
        .map(|cell| (*cell).to_owned())
        .collect::<Vec<_>>());
    text.push_str(&row(&cells
        .iter()
        .enumerate()
        .map(|(index, _)| if index == 0 { "---" } else { "---:" }.to_owned())
        .collect::<Vec<_>>()));
    text
}

fn named(contender: Contender) -> String {
    if contender.is_ours() {
        format!("**{contender}**")
    } else {
        contender.to_string()
    }
}

pub fn quality(contenders: &[Contender], pair: Pair, measured: &[Measured]) -> String {
    let rows: Vec<&Measured> = contenders
        .iter()
        .filter_map(|contender| {
            measured
                .iter()
                .find(|measured| measured.contender == *contender && measured.pair == pair)
        })
        .collect();
    let mut text = String::new();

    let _ = writeln!(text, "### Frequency domain, {pair}\n");
    text.push_str(&header(&[
        "Setting",
        "Ripple 20 Hz–10 kHz (dB)",
        "Deviation 20 Hz–20 kHz (dB)",
        "At 20 kHz (dB)",
        "−0.1 dB (kHz)",
        "−3 dB (kHz)",
        "Worst alias or image (dB)",
        "Worst landing ≤ 20 kHz (dB)",
        "SNR at 1 kHz (dB)",
        "Worst SNR 20 Hz–20 kHz (dB)",
        "Null 20 Hz–20 kHz (dB)",
        "Null 20 Hz–10 kHz (dB)",
    ]));
    for measured in &rows {
        let summary = &measured.summary;
        text.push_str(&row(&[
            named(measured.contender),
            ripple(summary.ripple_db),
            deviation(summary.deviation_db),
            decibels(summary.at_audible_db),
            kilohertz(summary.slight_drop_hz),
            kilohertz(summary.half_power_hz),
            decibels(summary.worst_rejection_db),
            decibels(summary.worst_audible_rejection_db),
            decibels(summary.snr_middle_db),
            decibels(summary.worst_snr_db),
            decibels(summary.null_wide_db),
            decibels(summary.null_narrow_db),
        ]));
    }

    let _ = writeln!(text, "\n### Time domain, {pair}\n");
    text.push_str(&header(&[
        "Setting",
        "Delay at 1 kHz (µs)",
        "Delay spread 20 Hz–20 kHz (µs)",
        "Impulse peak offset (µs)",
        "Ringing before peak (ms)",
        "Ringing after peak (ms)",
        "Energy over 1 ms before peak (dB)",
        "Streaming latency (ms)",
    ]));
    for measured in &rows {
        let summary = &measured.summary;
        text.push_str(&row(&[
            named(measured.contender),
            microseconds(summary.delay_middle_s),
            microseconds(summary.delay_spread_s),
            microseconds(summary.impulse.peak_offset_s),
            milliseconds(summary.impulse.before_peak_s),
            milliseconds(summary.impulse.after_peak_s),
            echo(summary.impulse.pre_echo_db),
            milliseconds(summary.latency_s),
        ]));
    }
    text.push('\n');
    text
}

pub fn costs(
    contenders: &[Contender],
    pairs: &[Pair],
    built: &[Pair],
    costs: &[(Contender, Pair, Cost)],
) -> String {
    let mut text =
        String::from("#### Throughput, percent of one core per second of stereo audio\n\n");
    let titles: Vec<String> = pairs.iter().map(ToString::to_string).collect();
    let mut columns = vec!["Setting"];
    columns.extend(titles.iter().map(String::as_str));
    text.push_str(&header(&columns));

    let found = |contender: Contender, pair: Pair| {
        costs
            .iter()
            .find(|(held, at, _)| *held == contender && *at == pair)
            .map(|(_, _, cost)| *cost)
    };

    for &contender in contenders {
        let mut cells = vec![named(contender)];
        cells.extend(pairs.iter().map(|&pair| {
            found(contender, pair).map_or_else(
                || DASH.to_owned(),
                |cost| format!("{:.3}", cost.share_of_a_core),
            )
        }));
        text.push_str(&row(&cells));
    }

    text.push_str("\n#### Construction, milliseconds\n\n");
    let mut columns = vec!["Setting".to_owned()];
    for pair in built {
        columns.push(format!("First, {pair}"));
        columns.push(format!("Again, {pair}"));
    }
    text.push_str(&header(
        &columns.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    for &contender in contenders {
        let mut cells = vec![named(contender)];
        for &pair in built {
            match found(contender, pair) {
                Some(cost) => {
                    cells.push(duration(cost.first_build));
                    cells.push(duration(cost.rebuilt));
                }
                None => cells.extend([DASH.to_owned(), DASH.to_owned()]),
            }
        }
        text.push_str(&row(&cells));
    }
    text.push('\n');
    text
}
