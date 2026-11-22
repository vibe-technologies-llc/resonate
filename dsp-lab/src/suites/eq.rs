use std::{path::Path, sync::Arc};

use resonate_core::{
    ChannelLayout, SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};
use resonate_dsp::Equaliser;

use crate::{
    analysis::fidelity::{self, Feed, MIDDLE_DBFS, response},
    error::{Error, Result},
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        readings::{Curve, Measured, find},
        subject::{Subject, SubjectName, ran},
        suite::{Outcome, Suite},
    },
    report::plot::{Across, Frame, SERIES_CLASSES, Series, chart, written},
    signal,
    subjects::stage::Stage,
};

const MEASURED: &str = "measured";
const PLOTTED_AT: Pair = Pair::at(SampleRate::HZ_48000);
const PLOTTED: [&str; 2] = ["Ten octave bands", "Thirty-two bands"];
const OCTAVE_CENTRES_HZ: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
const SMILE_DB: [f64; 10] = [6.0, 4.0, 2.0, 0.0, -1.0, -2.0, 0.0, 2.0, 4.0, 5.0];
const OCTAVE_Q: f64 = std::f64::consts::SQRT_2;
const IMPULSE_FRAMES: usize = 4_096;

pub const WORST: Metric = Metric::new(
    "worst-deviation-db",
    "Worst off the design 20 Hz–20 kHz (dB)",
    Better::Lower,
    Tolerance::within(1e-3),
    Shown::Scientific,
);
pub const MEAN: Metric = Metric::new(
    "mean-deviation-db",
    "Mean off the design (dB)",
    Better::Lower,
    Tolerance::within(1e-3),
    Shown::Scientific,
);
pub const THD_N: Metric = Metric::new(
    "thd-n-20-dbfs-db",
    "THD+N at −20 dBFS (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const IMPULSE_NULL: Metric = Metric::new(
    "impulse-null-db",
    "Impulse against the design (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const LATENCY: Metric = Metric::new(
    "claimed-latency-frames",
    "Claimed latency (frames)",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);

const METRICS: [&Metric; 5] = [&WORST, &MEAN, &THD_N, &IMPULSE_NULL, &LATENCY];

fn band(kind: BandKind, hertz: f64, decibels: f64, q: f64) -> Result<Band> {
    Ok(Band::new(
        kind,
        Frequency::from_hertz(hertz).map_err(Error::Vocabulary)?,
        BandGain::from_decibels(decibels).map_err(Error::Vocabulary)?,
        Q::from_units(q).map_err(Error::Vocabulary)?,
    ))
}

fn profile(preamp_db: f64, bands: Vec<Band>) -> Result<Arc<Profile>> {
    Profile::new(
        Preamp::from_decibels(preamp_db).map_err(Error::Vocabulary)?,
        bands,
    )
    .map(Arc::new)
    .map_err(Error::Vocabulary)
}

fn alternating(count: usize) -> Result<Vec<Band>> {
    let lowest: f64 = 31.0;
    let highest: f64 = 16_000.0;

    let step = (highest / lowest).powf(1.0 / (count - 1) as f64);

    (0..count)
        .map(|index| {
            let decibels = if index % 2 == 0 { 4.5 } else { -3.0 };
            band(
                BandKind::Peaking,
                lowest * step.powi(index as i32),
                decibels,
                1.4,
            )
        })
        .collect()
}

pub fn profiles() -> Result<Vec<(&'static str, Arc<Profile>)>> {
    let single = |kind, hertz, decibels, q| -> Result<Arc<Profile>> {
        profile(0.0, vec![band(kind, hertz, decibels, q)?])
    };
    let smile = OCTAVE_CENTRES_HZ
        .iter()
        .zip(SMILE_DB)
        .map(|(&hertz, decibels)| band(BandKind::Peaking, hertz, decibels, OCTAVE_Q))
        .collect::<Result<Vec<_>>>()?;

    Ok(vec![
        (
            "Peaking, 1 kHz, +6 dB, Q 1",
            single(BandKind::Peaking, 1_000.0, 6.0, 1.0)?,
        ),
        (
            "Peaking, 60 Hz, −9 dB, Q 8",
            single(BandKind::Peaking, 60.0, -9.0, 8.0)?,
        ),
        (
            "Low shelf, 100 Hz, +6 dB",
            single(BandKind::LowShelf, 100.0, 6.0, 0.707)?,
        ),
        (
            "High shelf, 8 kHz, −6 dB",
            single(BandKind::HighShelf, 8_000.0, -6.0, 0.707)?,
        ),
        (
            "Low pass, 10 kHz",
            single(BandKind::LowPass, 10_000.0, 0.0, 0.707)?,
        ),
        (
            "High pass, 40 Hz",
            single(BandKind::HighPass, 40.0, 0.0, 0.707)?,
        ),
        (
            "Notch, 3 kHz, Q 4",
            single(BandKind::Notch, 3_000.0, 0.0, 4.0)?,
        ),
        (
            "Band pass, 1 kHz, Q 1",
            single(BandKind::BandPass, 1_000.0, 0.0, 1.0)?,
        ),
        (
            "All pass, 2 kHz, Q 0.7",
            single(BandKind::AllPass, 2_000.0, 0.0, 0.7)?,
        ),
        (PLOTTED[0], profile(-6.0, smile)?),
        (PLOTTED[1], profile(-4.5, alternating(32)?)?),
    ])
}

fn the_profile(subject: &SubjectName) -> Result<Option<Arc<Profile>>> {
    Ok(profiles()?
        .into_iter()
        .find(|(name, _)| *name == subject.0)
        .map(|(_, profile)| profile))
}

fn designed_impulse(profile: &Profile, rate: SampleRate) -> Vec<f64> {
    let preamp = 10_f64.powf(profile.preamp().decibels() / 20.0);

    profile
        .filtered(&signal::impulse(IMPULSE_FRAMES, 0), rate)
        .pop()
        .unwrap_or_default()
        .iter()
        .map(|sample| sample * preamp)
        .collect()
}

pub struct Equalising;

impl Suite for Equalising {
    fn name(&self) -> &'static str {
        "eq"
    }

    fn title(&self) -> &'static str {
        "Equaliser"
    }

    fn about(&self) -> &'static str {
        "single bands of every kind and whole curves: the measured response against the design, its noise and its impulse"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(profiles()?
            .into_iter()
            .map(|(name, profile)| {
                Box::new(Stage::new(name, move |pair, _| {
                    Ok(Box::new(Equaliser::new(Arc::clone(&profile), pair.from)))
                })) as Box<dyn Subject>
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
        let Some(profile) = the_profile(&subject.name())? else {
            return Ok(Outcome::of(Vec::new()));
        };
        let feed = Feed::every(ChannelLayout::Stereo);

        let heard = response(subject, pair, feed, MIDDLE_DBFS)?.remove(0);
        let offs: Vec<f64> = heard
            .iter()
            .map(|&(hz, db)| (db - profile.magnitude_db_on(hz, pair.from, 0)).abs())
            .collect();
        let worst = offs.iter().copied().fold(0.0, f64::max);
        let mean = offs.iter().sum::<f64>() / offs.len() as f64;

        let tone = fidelity::tone(subject, pair, feed, MIDDLE_DBFS)?;
        let thd_n = fidelity::worst(&tone, |heard| heard.thd_n_db);

        let impulse = ran(
            subject,
            pair,
            ChannelLayout::Mono,
            &signal::impulse(IMPULSE_FRAMES, 0),
            fidelity::BLOCK,
        )?;
        let wanted = designed_impulse(&profile, pair.from);

        let impulse_null = crate::analysis::level::null_db(&impulse.output, &wanted);

        Ok(Outcome {
            readings: vec![
                Reading::of(&WORST, worst),
                Reading::of(&MEAN, mean),
                Reading::of(&THD_N, thd_n),
                Reading::of(&IMPULSE_NULL, impulse_null),
                Reading::of(&LATENCY, impulse.claimed_latency_frames.unwrap_or(f64::NAN)),
            ],
            curves: vec![Curve {
                name: MEASURED,
                points: heard,
            }],
        })
    }

    fn plotted(&self, directory: &Path, measured: &[Measured]) -> Result<()> {
        let profiles = profiles()?;
        let mut series = Vec::new();
        let mut classes = SERIES_CLASSES.iter();

        for name in PLOTTED {
            let Some(case) = find(measured, &SubjectName(name.to_owned()), PLOTTED_AT) else {
                continue;
            };
            let Some(curve) = case.curve(MEASURED) else {
                continue;
            };
            let Some((_, profile)) = profiles.iter().find(|(held, _)| *held == name) else {
                continue;
            };

            let designed = curve
                .points
                .iter()
                .map(|&(hz, _)| (hz, profile.magnitude_db_on(hz, PLOTTED_AT.from, 0)))
                .collect();

            if let (Some(&measured_class), Some(&designed_class)) = (classes.next(), classes.next())
            {
                series.push(Series {
                    name: format!("{name}, designed"),
                    class: designed_class,
                    points: designed,
                });
                series.push(Series {
                    name: format!("{name}, measured"),
                    class: measured_class,
                    points: curve.points.clone(),
                });
            }
        }

        let frame = Frame {
            from_hz: 20.0,
            to_hz: 20_000.0,
            floor_db: -15.0,
            ceiling_db: 5.0,
            across: Across::Octaves,
        };
        written(
            &directory.join("eq-curves-48.svg"),
            &chart(
                &format!("Equaliser curves, designed and measured, {PLOTTED_AT}"),
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
    fn a_band_is_heard_as_it_was_designed() {
        let heard = measured_as(
            &Equalising,
            "Peaking, 1 kHz, +6 dB, Q 1",
            Pair::at(SampleRate::HZ_48000),
        );

        assert!(heard.value(&WORST) < 1e-9);
        assert!(heard.value(&IMPULSE_NULL) < -200.0);
        assert_eq!(heard.value(&LATENCY), 0.0);
    }
}
