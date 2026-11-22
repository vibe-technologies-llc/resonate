use std::{fmt::Write as _, time::Duration};

use crate::lab::{
    metric::{Metric, Shown},
    pair::Pair,
    readings::{Measured, find},
    runner::{Timed, cost_of},
    subject::{Subject, SubjectName},
};

pub const DASH: &str = "—";

pub fn row(cells: &[String]) -> String {
    format!("| {} |\n", cells.join(" | "))
}

pub fn header(cells: &[&str]) -> String {
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

pub fn named(subject: &SubjectName, ours: bool) -> String {
    if ours {
        format!("**{subject}**")
    } else {
        subject.to_string()
    }
}

pub fn cell(shown: Shown, value: f64) -> String {
    if value.is_nan() {
        return DASH.to_owned();
    }
    match shown {
        Shown::Decibels if value.is_infinite() => if value < 0.0 { "−∞" } else { "∞" }.to_owned(),
        Shown::Decibels => format!("{value:.1}"),
        Shown::Digits(digits) => format!("{value:.digits$}"),
        Shown::Scientific => format!("{value:.1e}"),
        Shown::Kilohertz => format!("{:.2}", value / 1_000.0),
        Shown::Milliseconds => format!("{:.2}", value * 1e3),
        Shown::Microseconds => format!("{:.1}", value * 1e6),
        Shown::Count => format!("{value:.0}"),
        Shown::YesOrNo => if value != 0.0 { "yes" } else { "no" }.to_owned(),
    }
}

pub fn by_pair(
    title: &str,
    metrics: &[&'static Metric],
    subjects: &[Box<dyn Subject>],
    pairs: &[Pair],
    measured: &[Measured],
) -> String {
    let mut text = String::new();
    let mut columns = vec!["Setting"];

    columns.extend(metrics.iter().map(|metric| metric.title));

    for &pair in pairs {
        let rows: Vec<&Measured> = subjects
            .iter()
            .filter_map(|subject| find(measured, &subject.name(), pair))
            .collect();

        if rows.is_empty() {
            continue;
        }

        let _ = writeln!(text, "### {title}, {pair}\n");

        text.push_str(&header(&columns));
        for case in rows {
            let mut cells = vec![named(&case.subject, case.is_ours())];

            cells.extend(
                metrics
                    .iter()
                    .map(|metric| cell(metric.shown, case.value(metric))),
            );
            text.push_str(&row(&cells));
        }
        text.push('\n');
    }
    text
}

fn duration(value: Duration) -> String {
    format!("{:.2}", value.as_secs_f64() * 1e3)
}

pub fn costs(
    heading: &str,
    subjects: &[Box<dyn Subject>],
    pairs: &[Pair],
    built: &[Pair],
    costs: &[Timed],
) -> String {
    let mut text = format!("#### Throughput, {heading}\n\n");
    let titles: Vec<String> = pairs.iter().map(ToString::to_string).collect();
    let mut columns = vec!["Setting"];

    columns.extend(titles.iter().map(String::as_str));
    text.push_str(&header(&columns));

    for subject in subjects {
        let name = subject.name();
        let mut cells = vec![named(&name, subject.is_ours())];

        cells.extend(pairs.iter().map(|&pair| {
            cost_of(costs, &name, pair).map_or_else(
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
    for subject in subjects {
        let name = subject.name();
        let mut cells = vec![named(&name, subject.is_ours())];

        for &pair in built {
            match cost_of(costs, &name, pair) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_is_written_as_its_metric_shows_it() {
        assert_eq!(cell(Shown::Decibels, f64::NEG_INFINITY), "−∞");
        assert_eq!(cell(Shown::Decibels, -120.04), "-120.0");
        assert_eq!(cell(Shown::Kilohertz, 20_610.0), "20.61");
        assert_eq!(cell(Shown::Microseconds, 300e-6), "300.0");
        assert_eq!(cell(Shown::YesOrNo, 1.0), "yes");
        assert_eq!(cell(Shown::Count, f64::NAN), DASH);
    }
}
