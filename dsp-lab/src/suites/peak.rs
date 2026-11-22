use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::{Overs, TruePeak};

use crate::{
    analysis::{
        fidelity::{self, BLOCK, Feed, MIDDLE_DBFS, amplitude_of, delay},
        level::{decibels, null_db, rms, true_peak},
    },
    error::Result,
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Subject, ran},
        suite::{Outcome, Suite},
    },
    signal,
    subjects::stage::Stage,
};

const HEARD_SECONDS: f64 = 2.0;
const SETTLED_SECONDS: f64 = 0.5;
const HOT_TONE_DBFS: f64 = 1.0;

pub const OVER_PEAK: Metric = Metric::new(
    "inter-sample-over-dbtp",
    "Inter-sample over out (dBTP)",
    Better::Lower,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const HOT_PEAK: Metric = Metric::new(
    "hot-tone-dbtp",
    "+1 dBFS tone out (dBTP)",
    Better::Lower,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const REDUCTION: Metric = Metric::new(
    "reduction-db",
    "Reduction of the hot tone (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const LIMITED_THD_N: Metric = Metric::new(
    "limited-thd-n-db",
    "THD+N while limiting (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const UNDER_NULL: Metric = Metric::new(
    "under-the-ceiling-null-db",
    "Null under the ceiling (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const CLAIMED: Metric = Metric::new(
    "claimed-latency-frames",
    "Claimed latency (frames)",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);
pub const MEASURED: Metric = Metric::new(
    "measured-latency-frames",
    "Measured latency (frames)",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);

const METRICS: [&Metric; 7] = [
    &OVER_PEAK,
    &HOT_PEAK,
    &REDUCTION,
    &LIMITED_THD_N,
    &UNDER_NULL,
    &CLAIMED,
    &MEASURED,
];

fn inter_sample_over(rate: f64, frames: usize) -> Vec<f64> {
    signal::phased(
        &[(
            rate / 4.0,
            std::f64::consts::SQRT_2,
            std::f64::consts::FRAC_PI_4,
        )],
        rate,
        frames,
    )
}

fn settled(heard: &[f64], channels: usize, rate: f64) -> &[f64] {
    let skip = (SETTLED_SECONDS * rate) as usize * channels;

    heard.get(skip..).unwrap_or_default()
}

pub struct Guarding;

impl Suite for Guarding {
    fn name(&self) -> &'static str {
        "peak"
    }

    fn title(&self) -> &'static str {
        "True-peak guard"
    }

    fn about(&self) -> &'static str {
        "the guard against a counter that touches nothing: inter-sample overs, a hot tone, what limiting costs and what passes untouched"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(vec![
            Box::new(Stage::new("True-peak guard", |_, _| {
                Ok(Box::new(TruePeak::new()))
            })),
            Box::new(Stage::new("No guard", |_, _| Ok(Box::new(Overs::new())))),
        ])
    }

    fn pairs(&self) -> Vec<Pair> {
        [
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            SampleRate::HZ_96000,
        ]
        .map(Pair::at)
        .to_vec()
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let rate = pair.input_hz();
        let frames = (HEARD_SECONDS * rate) as usize;
        let feed = Feed::every(ChannelLayout::Stereo);
        let channels = feed.channels();

        let peak_out = |heard: &[f64]| decibels(true_peak(heard, channels, pair.to));

        let over = signal::spread(&inter_sample_over(rate, frames), channels);
        let over_out = ran(subject, pair, feed.layout, &over, BLOCK)?;

        let hot = signal::spread(
            &signal::sine(fidelity::TONE_HZ, amplitude_of(HOT_TONE_DBFS), rate, frames),
            channels,
        );
        let hot_out = ran(subject, pair, feed.layout, &hot, BLOCK)?;

        let reduction = decibels(
            rms(settled(&hot_out.output, channels, rate)) / rms(settled(&hot, channels, rate)),
        );
        let limited = fidelity::tone(subject, pair, feed, HOT_TONE_DBFS)?;

        let under = signal::spread(
            &signal::sine(fidelity::TONE_HZ, amplitude_of(MIDDLE_DBFS), rate, frames),
            channels,
        );
        let under_out = ran(subject, pair, feed.layout, &under, BLOCK)?;
        let delayed = delay(subject, pair, feed, 0)?;
        let held_back = delayed.measured_frames.round().max(0.0) as usize * channels;

        let under_null = null_db(
            under_out.output.get(held_back..).unwrap_or_default(),
            &under,
        );

        Ok(Outcome::of(vec![
            Reading::of(&OVER_PEAK, peak_out(&over_out.output)),
            Reading::of(&HOT_PEAK, peak_out(&hot_out.output)),
            Reading::of(&REDUCTION, reduction),
            Reading::of(
                &LIMITED_THD_N,
                fidelity::worst(&limited, |heard| heard.thd_n_db),
            ),
            Reading::of(&UNDER_NULL, under_null),
            Reading::of(&CLAIMED, delayed.claimed_frames.unwrap_or(f64::NAN)),
            Reading::of(&MEASURED, delayed.measured_frames),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    const AT: Pair = Pair::at(SampleRate::HZ_48000);
    const CEILING_DBTP: f64 = -0.1;
    const TURNING_GAIN_WITHIN_DB: f64 = 1e-4;

    #[test]
    fn the_guard_holds_an_over_the_counter_lets_through() {
        let guarded = measured_as(&Guarding, "True-peak guard", AT);
        let unguarded = measured_as(&Guarding, "No guard", AT);

        assert!(guarded.value(&OVER_PEAK) <= CEILING_DBTP + TURNING_GAIN_WITHIN_DB);
        assert!(unguarded.value(&OVER_PEAK) > 3.0);
        assert!(guarded.value(&HOT_PEAK) <= CEILING_DBTP + TURNING_GAIN_WITHIN_DB);
    }

    #[test]
    fn music_under_the_ceiling_passes_the_guard_untouched() {
        let guarded = measured_as(&Guarding, "True-peak guard", AT);

        assert_eq!(guarded.value(&UNDER_NULL), f64::NEG_INFINITY);
    }
}
