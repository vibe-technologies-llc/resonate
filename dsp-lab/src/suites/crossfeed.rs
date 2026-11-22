use std::path::Path;

use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::{Crossfeed, Crossfeeder};

use crate::{
    analysis::fidelity::{Feed, MIDDLE_DBFS, delay, response, spread_db},
    error::Result,
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        readings::{Curve, Measured, find},
        subject::{Subject, SubjectName},
        suite::{Outcome, Suite},
    },
    report::plot::{Across, Frame, SERIES_CLASSES, Series, chart, written},
    subjects::stage::Stage,
};

const LEVELS: [Crossfeed; 3] = [Crossfeed::Light, Crossfeed::Moderate, Crossfeed::Strong];
const LOW_HZ: f64 = 100.0;
const HIGH_HZ: f64 = 10_000.0;
const CROSSED: &str = "crossed";
const PLOTTED_AT: Pair = Pair::at(SampleRate::HZ_48000);

pub const ILD_LOW: Metric = Metric::new(
    "ild-100-hz-db",
    "Direct over crossed at 100 Hz (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const ILD_HIGH: Metric = Metric::new(
    "ild-10-khz-db",
    "Direct over crossed at 10 kHz (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const DIRECT_SPREAD: Metric = Metric::new(
    "direct-spread-db",
    "Direct path's spread 20 Hz–20 kHz (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const CENTRE_SPREAD: Metric = Metric::new(
    "centre-spread-db",
    "A centred source's spread (dB)",
    Better::Lower,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const DELAY: Metric = Metric::new(
    "crossed-delay-s",
    "Crossed path's delay (µs)",
    Better::Same,
    Tolerance::SECONDS,
    Shown::Microseconds,
);

const METRICS: [&Metric; 5] = [&ILD_LOW, &ILD_HIGH, &DIRECT_SPREAD, &CENTRE_SPREAD, &DELAY];

fn level_named(level: Crossfeed) -> String {
    format!("Crossfeed {}", level.as_str())
}

fn nearest(points: &[(f64, f64)], hz: f64) -> f64 {
    points
        .iter()
        .min_by(|a, b| (a.0 / hz).ln().abs().total_cmp(&(b.0 / hz).ln().abs()))
        .map_or(f64::NAN, |&(_, db)| db)
}

pub struct Crossfeeding;

impl Suite for Crossfeeding {
    fn name(&self) -> &'static str {
        "crossfeed"
    }

    fn title(&self) -> &'static str {
        "Crossfeed"
    }

    fn about(&self) -> &'static str {
        "each crossfeed level: the direct and crossed paths, their difference low and high, the crossed delay and a centred source's flatness"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(LEVELS
            .into_iter()
            .map(|level| {
                Box::new(
                    Stage::new(level_named(level), move |_, _| {
                        Ok(Box::new(Crossfeeder::new(level)))
                    })
                    .taking(ChannelLayout::Stereo),
                ) as Box<dyn Subject>
            })
            .collect())
    }

    fn pairs(&self) -> Vec<Pair> {
        [
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            SampleRate::HZ_96000,
            SampleRate::HZ_192000,
        ]
        .map(Pair::at)
        .to_vec()
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let left_only = Feed::only(ChannelLayout::Stereo, 0);

        let mut heard = response(subject, pair, left_only, MIDDLE_DBFS)?;

        let crossed = heard.pop().unwrap_or_default();
        let direct = heard.pop().unwrap_or_default();
        let centred = response(
            subject,
            pair,
            Feed::every(ChannelLayout::Stereo),
            MIDDLE_DBFS,
        )?
        .remove(0);
        let delayed = delay(subject, pair, left_only, 1)?;

        Ok(Outcome {
            readings: vec![
                Reading::of(
                    &ILD_LOW,
                    nearest(&direct, LOW_HZ) - nearest(&crossed, LOW_HZ),
                ),
                Reading::of(
                    &ILD_HIGH,
                    nearest(&direct, HIGH_HZ) - nearest(&crossed, HIGH_HZ),
                ),
                Reading::of(&DIRECT_SPREAD, spread_db(&direct)),
                Reading::of(&CENTRE_SPREAD, spread_db(&centred)),
                Reading::of(&DELAY, delayed.measured_frames / pair.output_hz()),
            ],
            curves: vec![Curve {
                name: CROSSED,
                points: crossed,
            }],
        })
    }

    fn plotted(&self, directory: &Path, measured: &[Measured]) -> Result<()> {
        let series: Vec<Series> = LEVELS
            .into_iter()
            .zip(SERIES_CLASSES)
            .filter_map(|(level, class)| {
                let name = level_named(level);

                find(measured, &SubjectName(name.clone()), PLOTTED_AT)
                    .and_then(|case| case.curve(CROSSED))
                    .map(|curve| Series {
                        name,
                        class,
                        points: curve.points.clone(),
                    })
            })
            .collect();
        let frame = Frame {
            from_hz: 20.0,
            to_hz: 20_000.0,
            floor_db: -40.0,
            ceiling_db: 0.0,
            across: Across::Octaves,
        };

        written(
            &directory.join("crossfeed-crossed-48.svg"),
            &chart(
                &format!("What crosses to the other ear, {PLOTTED_AT}"),
                &frame,
                &[],
                &series,
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    #[test]
    fn the_crossed_path_is_late_by_its_delay_and_the_direct_path_is_flat() {
        let pair = Pair::at(SampleRate::HZ_48000);
        let heard = measured_as(&Crossfeeding, "Crossfeed moderate", pair);
        let a_frame = 1.0 / pair.output_hz();

        assert!((heard.value(&DELAY) - 300e-6).abs() <= a_frame);
        assert!(heard.value(&DIRECT_SPREAD) < 1e-6);
        assert!(heard.value(&ILD_HIGH) > heard.value(&ILD_LOW));
    }
}
