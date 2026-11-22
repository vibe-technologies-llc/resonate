use resonate_core::{AppliedGain, ChannelLayout, Decibels, Gain, SampleRate, Volume};
use resonate_dsp::{GainConfig, GainStage};

use crate::{
    analysis::{
        fidelity::{self, BLOCK, Feed, MIDDLE_DBFS, amplitude_of},
        level::{decibels, null_db},
    },
    error::{Error, Result},
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Subject, SubjectName, ran},
        suite::{Outcome, Suite},
    },
    signal,
    subjects::stage::Stage,
};

const HOT_DBFS: f64 = 3.0;
const HEARD_SECONDS: f64 = 1.0;
const VOLUMES_DB: [f64; 5] = [0.0, -0.1, -6.0, -20.0, -60.0];
const VOLUME_CUBED: f64 = 60.0;

pub const ASKED: Metric = Metric::new(
    "asked-db",
    "Gain asked (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(3),
);
pub const HEARD: Metric = Metric::new(
    "heard-db",
    "Gain heard at 1 kHz (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(3),
);
pub const ERROR: Metric = Metric::new(
    "error-db",
    "Off the gain asked (dB)",
    Better::Lower,
    Tolerance::within(1e-3),
    Shown::Scientific,
);
pub const NULL: Metric = Metric::new(
    "null-db",
    "Null against the input scaled (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const OVERS: Metric = Metric::new(
    "overs",
    "Samples past full scale, +3 dBFS in",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);

const METRICS: [&Metric; 5] = [&ASKED, &HEARD, &ERROR, &NULL, &OVERS];

fn volume_at(db: f64) -> Result<Volume> {
    Volume::new(10_f64.powf(db / VOLUME_CUBED) as f32).map_err(Error::Vocabulary)
}

fn replay_gain(db: f32, peak: Option<f32>) -> Result<AppliedGain> {
    Ok(AppliedGain {
        gain: Some(Decibels::new(db).map_err(Error::Vocabulary)?),
        peak: peak
            .map(|peak| Gain::new(peak).map_err(Error::Vocabulary))
            .transpose()?,
    })
}

fn settings() -> Result<Vec<(String, GainConfig)>> {
    let mut settings = VOLUMES_DB
        .iter()
        .map(|&db| {
            Ok((
                format!("Volume {db:+} dB"),
                GainConfig {
                    volume: volume_at(db)?,
                    ..GainConfig::default()
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    settings.extend([
        (
            "ReplayGain −6 dB".to_owned(),
            GainConfig {
                replay_gain: replay_gain(-6.0, Some(0.98))?,
                ..GainConfig::default()
            },
        ),
        (
            "ReplayGain +6 dB, peak 0.9".to_owned(),
            GainConfig {
                replay_gain: replay_gain(6.0, Some(0.9))?,
                ..GainConfig::default()
            },
        ),
        (
            "ReplayGain +6 dB, no peak".to_owned(),
            GainConfig {
                replay_gain: replay_gain(6.0, None)?,
                ..GainConfig::default()
            },
        ),
        (
            "ReplayGain +6 dB, no peak, unprotected".to_owned(),
            GainConfig {
                replay_gain: replay_gain(6.0, None)?,
                prevent_clipping: false,
                ..GainConfig::default()
            },
        ),
    ]);
    Ok(settings)
}

fn asked_of(subject: &SubjectName) -> Result<Option<f64>> {
    Ok(settings()?
        .into_iter()
        .find(|(name, _)| *name == subject.0)
        .map(|(_, config)| f64::from(config.amplitude())))
}

pub struct Gaining;

impl Suite for Gaining {
    fn name(&self) -> &'static str {
        "gain"
    }

    fn title(&self) -> &'static str {
        "Gain"
    }

    fn about(&self) -> &'static str {
        "the volume and ReplayGain stage: the gain heard against the gain asked, and what a hot input does"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(settings()?
            .into_iter()
            .map(|(name, config)| {
                Box::new(Stage::new(name, move |_, _| {
                    Ok(Box::new(GainStage::new(config)))
                })) as Box<dyn Subject>
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
        let Some(asked) = asked_of(&subject.name())? else {
            return Ok(Outcome::of(Vec::new()));
        };
        let feed = Feed::every(ChannelLayout::Stereo);

        let tone = fidelity::tone(subject, pair, feed, MIDDLE_DBFS)?;
        let heard = fidelity::worst(&tone, |heard| heard.gain_db);
        let asked_db = decibels(asked);

        let frames = (HEARD_SECONDS * pair.input_hz()) as usize;
        let input = signal::spread(
            &signal::sine(
                fidelity::TONE_HZ,
                amplitude_of(MIDDLE_DBFS),
                pair.input_hz(),
                frames,
            ),
            feed.channels(),
        );
        let scaled: Vec<f64> = input.iter().map(|sample| sample * asked).collect();

        let null = null_db(
            &ran(subject, pair, feed.layout, &input, BLOCK)?.output,
            &scaled,
        );

        let hot = signal::spread(
            &signal::sine(
                fidelity::TONE_HZ,
                amplitude_of(HOT_DBFS),
                pair.input_hz(),
                frames,
            ),
            feed.channels(),
        );
        let overs = ran(subject, pair, feed.layout, &hot, BLOCK)?
            .output
            .iter()
            .filter(|sample| sample.abs() > 1.0)
            .count();

        Ok(Outcome::of(vec![
            Reading::of(&ASKED, asked_db),
            Reading::of(&HEARD, heard),
            Reading::of(&ERROR, (heard - asked_db).abs()),
            Reading::of(&NULL, null),
            Reading::of(&OVERS, overs as f64),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    #[test]
    fn a_volume_is_heard_at_the_gain_it_asks() {
        let heard = measured_as(&Gaining, "Volume -6 dB", Pair::at(SampleRate::HZ_48000));

        assert!((heard.value(&ASKED) + 6.0).abs() < 1e-3);
        assert!(heard.value(&ERROR) < 1e-9);
        assert_eq!(heard.value(&NULL), f64::NEG_INFINITY);
    }

    #[test]
    fn a_boost_with_no_peak_to_heed_is_held_under_full_scale() {
        let heard = measured_as(
            &Gaining,
            "ReplayGain +6 dB, no peak",
            Pair::at(SampleRate::HZ_48000),
        );
        let unprotected = measured_as(
            &Gaining,
            "ReplayGain +6 dB, no peak, unprotected",
            Pair::at(SampleRate::HZ_48000),
        );

        assert_eq!(heard.value(&OVERS), 0.0);
        assert!(unprotected.value(&OVERS) > 0.0);
    }
}
