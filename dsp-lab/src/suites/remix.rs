use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::Remix;

use crate::{
    analysis::fidelity::{self, Feed, MIDDLE_DBFS, amplitude_of, steady, tone_hz},
    error::Result,
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Subject, SubjectName},
        suite::{Outcome, Suite},
    },
    signal,
    subjects::stage::Stage,
};

const SILENT_CELL: f64 = 1e-12;

const DOWNMIXES: [(ChannelLayout, ChannelLayout); 8] = [
    (ChannelLayout::Mono, ChannelLayout::Stereo),
    (ChannelLayout::Stereo, ChannelLayout::Mono),
    (ChannelLayout::Quad, ChannelLayout::Stereo),
    (ChannelLayout::Surround50, ChannelLayout::Stereo),
    (ChannelLayout::Surround51, ChannelLayout::Stereo),
    (ChannelLayout::Surround61, ChannelLayout::Stereo),
    (ChannelLayout::Surround71, ChannelLayout::Stereo),
    (ChannelLayout::Surround71, ChannelLayout::Surround51),
];

pub const WORST_CELL: Metric = Metric::new(
    "worst-cell-off-db",
    "Worst cell off the matrix (dB)",
    Better::Lower,
    Tolerance::within(1e-3),
    Shown::Scientific,
);
pub const LEAK: Metric = Metric::new(
    "leak-db",
    "Loudest where the matrix asks nothing (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const ROW_SUM: Metric = Metric::new(
    "loudest-row-sum",
    "Loudest output's summed gains",
    Better::Lower,
    Tolerance::within(1e-6),
    Shown::Digits(4),
);
pub const THD_N: Metric = Metric::new(
    "thd-n-db",
    "Worst THD+N (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);

const METRICS: [&Metric; 4] = [&WORST_CELL, &LEAK, &ROW_SUM, &THD_N];

fn spoken(layout: ChannelLayout) -> &'static str {
    match layout {
        ChannelLayout::Mono => "mono",
        ChannelLayout::Stereo => "stereo",
        ChannelLayout::Quad => "quad",
        ChannelLayout::Surround50 => "5.0",
        ChannelLayout::Surround51 => "5.1",
        ChannelLayout::Surround61 => "6.1",
        ChannelLayout::Surround71 => "7.1",
        _ => "other",
    }
}

fn named(from: ChannelLayout, to: ChannelLayout) -> String {
    format!("Remix {} to {}", spoken(from), spoken(to))
}

fn layouts_of(subject: &SubjectName) -> Option<(ChannelLayout, ChannelLayout)> {
    DOWNMIXES
        .into_iter()
        .find(|&(from, to)| named(from, to) == subject.0)
}

pub struct Remixing;

impl Suite for Remixing {
    fn name(&self) -> &'static str {
        "remix"
    }

    fn title(&self) -> &'static str {
        "Remix"
    }

    fn about(&self) -> &'static str {
        "every layout a downmix or upmix meets: the matrix heard tone by tone against the matrix the stage holds"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(DOWNMIXES
            .into_iter()
            .map(|(from, to)| {
                Box::new(
                    Stage::new(named(from, to), move |_, _| {
                        Ok(Box::new(Remix::new(from, to)))
                    })
                    .taking(from),
                ) as Box<dyn Subject>
            })
            .collect())
    }

    fn pairs(&self) -> Vec<Pair> {
        vec![Pair::at(SampleRate::HZ_48000)]
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let Some((from, to)) = layouts_of(&subject.name()) else {
            return Ok(Outcome::of(Vec::new()));
        };

        let remix = Remix::new(from, to);
        let inputs = usize::from(from.count().get());
        let outputs = usize::from(to.count().get());
        let hz = tone_hz(pair, fidelity::TONE_HZ);
        let amplitude = amplitude_of(MIDDLE_DBFS);

        let mut worst_cell: f64 = 0.0;
        let mut leak = f64::NEG_INFINITY;
        let mut sums = vec![0.0; outputs];
        let mut thd_n = f64::NEG_INFINITY;

        for input in 0..inputs {
            let heard = steady(subject, pair, Feed::only(from, input), |rate, frames| {
                signal::sine(hz, amplitude, rate, frames)
            })?;

            for (output, spectrum) in heard.spectra().iter().enumerate() {
                let measured = spectrum.amplitude_at(hz) / amplitude;
                let asked = remix.gain(output, input).unwrap_or(0.0);

                if let Some(sum) = sums.get_mut(output) {
                    *sum += measured;
                }
                if asked.abs() > SILENT_CELL {
                    let off = 20.0 * (measured / asked.abs()).log10();

                    worst_cell = worst_cell.max(off.abs());
                    thd_n = thd_n
                        .max(fidelity::heard_tone(spectrum, hz, amplitude * asked.abs()).thd_n_db);
                } else {
                    leak = leak.max(if measured == 0.0 {
                        f64::NEG_INFINITY
                    } else {
                        crate::analysis::level::decibels(measured)
                    });
                }
            }
        }

        Ok(Outcome::of(vec![
            Reading::of(&WORST_CELL, worst_cell),
            Reading::of(&LEAK, leak),
            Reading::of(&ROW_SUM, sums.iter().copied().fold(0.0, f64::max)),
            Reading::of(&THD_N, thd_n),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    #[test]
    fn every_matrix_is_heard_as_the_stage_holds_it_and_no_output_passes_unity() {
        for (from, to) in DOWNMIXES {
            let heard = measured_as(&Remixing, &named(from, to), Pair::at(SampleRate::HZ_48000));

            assert!(heard.value(&WORST_CELL) < 1e-9, "{}", named(from, to));
            assert!(heard.value(&ROW_SUM) <= 1.0 + 1e-12, "{}", named(from, to));
        }
    }
}
