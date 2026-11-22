use std::sync::Arc;

use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::{Convolver, Impulse};

use crate::{
    analysis::{
        fidelity::BLOCK,
        level::{loudest, null_db},
    },
    error::{Error, Result},
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Subject, SubjectName, ran},
        suite::{Outcome, Suite},
    },
    signal::{self, Noise},
    subjects::stage::Stage,
};

const MEASURED_AT: SampleRate = SampleRate::HZ_48000;
const HEARD_SECONDS: f64 = 0.5;
const IMPULSE_AT_SECONDS: f64 = 0.25;
const SEED: u64 = 0x636F_6E76_6F6C_7665;
const DECAY_SECONDS: f64 = 0.15;

pub const NULL: Metric = Metric::new(
    "null-db",
    "Null against direct convolution (dB)",
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
pub const UNFLUSHED: Metric = Metric::new(
    "unflushed-frames",
    "Tail frames never flushed",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);

const METRICS: [&Metric; 4] = [&NULL, &CLAIMED, &MEASURED, &UNFLUSHED];

pub fn decaying(seconds: f64, seed: u64, level: f64, delay_frames: usize) -> Vec<f64> {
    let frames = (seconds * f64::from(MEASURED_AT.hz())) as usize;
    let mut noise = Noise::seeded(seed);
    let mut taps = vec![0.0; delay_frames];

    taps.extend((0..frames).map(|index| {
        let time = index as f64 / f64::from(MEASURED_AT.hz());
        level * (2.0 * noise.unit() - 1.0) * (-time / DECAY_SECONDS).exp()
    }));
    if let Some(first) = taps.get_mut(delay_frames) {
        *first = level;
    }
    taps
}

fn responses() -> Vec<(&'static str, Option<Impulse>)> {
    vec![
        ("Unit impulse", Impulse::new(MEASURED_AT, vec![vec![1.0]])),
        (
            "Decaying noise, 1 s",
            Impulse::new(MEASURED_AT, vec![decaying(1.0, SEED, 0.5, 0)]),
        ),
        (
            "Stereo pair, 0.1 s",
            Impulse::new(
                MEASURED_AT,
                vec![decaying(0.1, SEED, 0.5, 0), decaying(0.1, SEED + 1, 0.5, 3)],
            ),
        ),
        (
            "Two-by-two, 0.25 s",
            Impulse::crossed(
                MEASURED_AT,
                vec![
                    decaying(0.25, SEED, 0.6, 0),
                    decaying(0.25, SEED + 1, 0.2, 13),
                    decaying(0.25, SEED + 2, 0.2, 13),
                    decaying(0.25, SEED + 3, 0.6, 0),
                ],
            ),
        ),
    ]
}

fn response_of(subject: &SubjectName) -> Option<Impulse> {
    responses()
        .into_iter()
        .find(|(name, _)| *name == subject.0)
        .and_then(|(_, impulse)| impulse)
}

fn convolved(input: &[f64], taps: &[f64]) -> Vec<f64> {
    let mut output = vec![0.0; input.len() + taps.len() - 1];

    for (at, &sample) in input.iter().enumerate() {
        if sample == 0.0 {
            continue;
        }
        for (slot, tap) in output[at..].iter_mut().zip(taps) {
            *slot += sample * tap;
        }
    }
    output
}

fn directly(input: &[Vec<f64>], taps: &[Vec<f64>], crossed: bool) -> Vec<Vec<f64>> {
    let channels = input.len();

    (0..channels)
        .map(|output| {
            if crossed && channels == 2 {
                let heard = |from: usize| convolved(&input[from], &taps[from * 2 + output]);
                heard(0).iter().zip(heard(1)).map(|(a, b)| a + b).collect()
            } else {
                convolved(&input[output], &taps[output % taps.len()])
            }
        })
        .collect()
}

fn interleaved(channels: &[Vec<f64>]) -> Vec<f64> {
    let frames = channels.iter().map(Vec::len).max().unwrap_or(0);

    (0..frames)
        .flat_map(|frame| {
            channels
                .iter()
                .map(move |channel| channel.get(frame).copied().unwrap_or(0.0))
        })
        .collect()
}

pub struct Convolving;

impl Suite for Convolving {
    fn name(&self) -> &'static str {
        "convolve"
    }

    fn title(&self) -> &'static str {
        "Convolver"
    }

    fn about(&self) -> &'static str {
        "responses from a unit impulse to a two-by-two set, at their own rate and redrawn: against direct convolution, its latency and its tail"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        responses()
            .into_iter()
            .map(|(name, impulse)| {
                let impulse = Arc::new(impulse.ok_or_else(|| Error::ResponseRefused {
                    subject: SubjectName(name.to_owned()),
                })?);

                Ok(Box::new(
                    Stage::new(name, move |_, _| {
                        Ok(Box::new(Convolver::new(Arc::clone(&impulse))))
                    })
                    .taking(ChannelLayout::Stereo),
                ) as Box<dyn Subject>)
            })
            .collect()
    }

    fn pairs(&self) -> Vec<Pair> {
        vec![Pair::at(MEASURED_AT), Pair::at(SampleRate::HZ_44100)]
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let Some(impulse) = response_of(&subject.name()) else {
            return Ok(Outcome::of(Vec::new()));
        };
        let refused = |source| Error::Ours {
            subject: subject.name(),
            pair,
            source,
        };

        let taps = impulse.at(pair.from).map_err(refused)?;
        let layout = ChannelLayout::Stereo;
        let frames = (HEARD_SECONDS * pair.input_hz()) as usize;
        let mut noise = Noise::seeded(SEED ^ u64::from(pair.from.hz()));

        let input = vec![noise.samples(frames), noise.samples(frames)];

        let heard = ran(subject, pair, layout, &interleaved(&input), BLOCK)?;
        let claimed = heard.claimed_latency_frames.unwrap_or(0.0);
        let held_back = claimed.round() as usize;
        let wanted = directly(&input, &taps, impulse.is_crossed());
        let wanted_frames = wanted.first().map_or(0, Vec::len);

        let null = null_db(
            heard.output.get(held_back * 2..).unwrap_or_default(),
            &interleaved(&wanted),
        );
        let unflushed = wanted_frames.saturating_sub(heard.frames().saturating_sub(held_back));

        let at = (IMPULSE_AT_SECONDS * pair.input_hz()) as usize;
        let click = signal::impulse(frames, at);
        let clicked = ran(
            subject,
            pair,
            layout,
            &interleaved(&[click.clone(), vec![0.0; frames]]),
            BLOCK,
        )?;
        let expected = directly(&[click, vec![0.0; frames]], &taps, impulse.is_crossed());

        let measured = loudest(&clicked.channel(0)) as f64
            - expected.first().map_or(0, |channel| loudest(channel)) as f64;

        Ok(Outcome::of(vec![
            Reading::of(&NULL, null),
            Reading::of(&CLAIMED, claimed),
            Reading::of(&MEASURED, measured),
            Reading::of(&UNFLUSHED, unflushed as f64),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    #[test]
    fn direct_convolution_is_the_sum_of_shifted_taps() {
        assert_eq!(convolved(&[1.0, 2.0], &[0.5, 0.25]), vec![0.5, 1.25, 0.5]);
    }

    #[test]
    fn a_unit_impulse_hands_the_input_back_late_by_its_claim_and_whole() {
        let heard = measured_as(&Convolving, "Unit impulse", Pair::at(MEASURED_AT));

        assert!(heard.value(&NULL) < -250.0);
        assert_eq!(heard.value(&MEASURED), heard.value(&CLAIMED));
        assert_eq!(heard.value(&UNFLUSHED), 0.0);
    }

    #[test]
    fn a_crossed_set_redrawn_at_another_rate_still_convolves_as_its_taps_say() {
        let heard = measured_as(
            &Convolving,
            "Two-by-two, 0.25 s",
            Pair::at(SampleRate::HZ_44100),
        );

        assert!(heard.value(&NULL) < -200.0);
        assert_eq!(heard.value(&UNFLUSHED), 0.0);
    }
}
