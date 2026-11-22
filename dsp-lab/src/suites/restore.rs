use resonate_core::{ChannelLayout, SampleRate, eq::Frequency};
use resonate_dsp::{Restoration, Restore, RestoreConfig, Tuning};

use crate::{
    analysis::{
        fidelity::{BLOCK, Feed, analysed_frames, held_back},
        level::power_decibels,
        spectrum::{AUDIBLE_FROM_HZ, Spectrum},
    },
    error::{Error, Result},
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Subject, ran},
        suite::{Outcome, Suite},
    },
    signal::{self, Noise},
    subjects::stage::Stage,
};

const WALL_HZ: f64 = 16_000.0;
const SETTLED_SECONDS: f64 = 4.5;
const BELOW_THE_WALL_SHARE: f64 = 0.9;
const BAND_BESIDE_HZ: f64 = 3_000.0;
const EDGE_SHARE: f64 = 0.02;
const SEED: u64 = 0x0072_6573_746F_7265;
const TUNINGS: [Tuning; 3] = [Tuning::Mp3, Tuning::Aac, Tuning::Vorbis];

pub const NULL_BELOW: Metric = Metric::new(
    "null-below-the-wall-db",
    "Null below the wall (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const ADDED_ABOVE: Metric = Metric::new(
    "added-above-the-wall-db",
    "Above the wall against under it (dB)",
    Better::Same,
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

const METRICS: [&Metric; 4] = [&NULL_BELOW, &ADDED_ABOVE, &CLAIMED, &MEASURED];

fn tuning_named(tuning: Tuning) -> &'static str {
    match tuning {
        Tuning::Mp3 => "MP3",
        Tuning::Aac => "AAC",
        Tuning::Vorbis => "Vorbis",
    }
}

fn settings() -> Vec<(String, Restoration, Tuning)> {
    let mut settings = Vec::new();

    for restoration in [Restoration::Repair, Restoration::Extend] {
        for tuning in TUNINGS {
            settings.push((
                format!("{} {}", restoration.as_str(), tuning_named(tuning)),
                restoration,
                tuning,
            ));
        }
    }
    settings
}

pub struct Restoring;

impl Suite for Restoring {
    fn name(&self) -> &'static str {
        "restore"
    }

    fn title(&self) -> &'static str {
        "Lossy restoration"
    }

    fn about(&self) -> &'static str {
        "repair and extension under each codec's tuning, on noise walled at 16 kHz: what changes under the wall and what is added above it"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        let wall = Frequency::from_hertz(WALL_HZ).map_err(Error::Vocabulary)?;

        Ok(settings()
            .into_iter()
            .map(|(name, restoration, tuning)| {
                let config = RestoreConfig {
                    restoration,
                    tuning,
                    wall: Some(wall),
                };

                Box::new(
                    Stage::new(name, move |_, _| Ok(Box::new(Restore::new(config))))
                        .taking(ChannelLayout::Stereo),
                ) as Box<dyn Subject>
            })
            .collect())
    }

    fn pairs(&self) -> Vec<Pair> {
        vec![
            Pair::at(SampleRate::HZ_44100),
            Pair::at(SampleRate::HZ_48000),
        ]
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let rate = pair.input_hz();
        let analysed = analysed_frames(pair.from);
        let settled = (SETTLED_SECONDS * rate) as usize;

        let frames = settled + analysed;
        let mut noise = Noise::seeded(SEED ^ u64::from(pair.from.hz()));

        let left = signal::walled(&noise.samples(frames), rate, WALL_HZ);
        let right = signal::walled(&noise.samples(frames), rate, WALL_HZ);

        let input: Vec<f64> = left
            .iter()
            .zip(&right)
            .flat_map(|(&l, &r)| [l, r])
            .collect();

        let heard = ran(subject, pair, ChannelLayout::Stereo, &input, BLOCK)?;
        let claimed = heard.claimed_latency_frames.unwrap_or(0.0);
        let held_back = held_back(subject, pair, Feed::every(ChannelLayout::Stereo))?;
        let mut out = heard.channel(0);

        out.resize(held_back + frames, 0.0);

        let window = |samples: &[f64]| samples[settled..settled + analysed].to_vec();
        let wanted = window(&left);
        let got = window(&out[held_back..]);

        let missed: Vec<f64> = got.iter().zip(&wanted).map(|(a, b)| a - b).collect();

        let below = BELOW_THE_WALL_SHARE * WALL_HZ;
        let wanted_spectrum = Spectrum::hann_windowed(&wanted, rate);
        let missed_spectrum = Spectrum::hann_windowed(&missed, rate);
        let got_spectrum = Spectrum::hann_windowed(&got, rate);

        let null_below = power_decibels(
            missed_spectrum.band(AUDIBLE_FROM_HZ, below)
                / wanted_spectrum.band(AUDIBLE_FROM_HZ, below),
        );
        let above = got_spectrum.band(
            WALL_HZ * (1.0 + EDGE_SHARE),
            (WALL_HZ + BAND_BESIDE_HZ).min(got_spectrum.nyquist()),
        );
        let under = got_spectrum.band(WALL_HZ - BAND_BESIDE_HZ, WALL_HZ * (1.0 - EDGE_SHARE));

        Ok(Outcome::of(vec![
            Reading::of(&NULL_BELOW, null_below),
            Reading::of(&ADDED_ABOVE, power_decibels(above / under)),
            Reading::of(&CLAIMED, claimed),
            Reading::of(&MEASURED, held_back as f64),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    #[test]
    fn extension_adds_above_the_wall_and_leaves_what_is_under_it() {
        let pair = Pair::at(SampleRate::HZ_48000);
        let repaired = measured_as(&Restoring, "repair MP3", pair);
        let extended = measured_as(&Restoring, "extend MP3", pair);

        assert!(extended.value(&ADDED_ABOVE) > repaired.value(&ADDED_ABOVE) + 40.0);
        assert!(extended.value(&NULL_BELOW) < -60.0);
    }
}
