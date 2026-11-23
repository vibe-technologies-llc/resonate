use resonate_core::{ChannelLayout, SampleRate};

use crate::{
    analysis::level::{decibels, null_db, rms},
    error::Result,
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Family, Ran, Running, Subject, SubjectName, ran, ran_again_after_a_reset},
        suite::{Outcome, Suite},
    },
    signal::{self, Noise},
    suites,
};

const NAME: &str = "stress";
const SEED: u64 = 0x7374_7265_7373;
const SOUNDING_SECONDS: f64 = 0.25;
const SILENT_AFTER_SECONDS: f64 = 1.5;
const TAIL_SECONDS: f64 = 0.25;
const SILENCE_HEARD_AFTER_SECONDS: f64 = 0.1;
const LONG_SILENCE_SECONDS: f64 = 12.0;
const SQUARE_HZ: f64 = 1_000.0;
const SWEEP_FROM_HZ: f64 = 10.0;
const SWEEP_TO_NYQUIST: f64 = 0.99;
const DC_LEVEL: f64 = 0.5;
const IMPULSES_APART_SECONDS: f64 = 0.05;
const QUIET_TONE_HZ: f64 = 1_000.0;
const BELOW_EVERY_WORD: f64 = 1e-20;
const BLOCKS: [usize; 3] = [1, 37, 4_096];
const USUAL_BLOCK: usize = 1_024;
const UNALIKE_LENGTHS_DB: f64 = 0.0;
const POISONED_FOR_SECONDS: f64 = 3.0;
const NOT_A_NUMBER_AT_SECONDS: f64 = 0.1;
const INFINITY_AT_SECONDS: f64 = 0.2;
const RECOVERED_WITHIN_SECONDS: f64 = 1.5;
const POISONED_TONE_HZ: f64 = 1_000.0;
const POISONED_TONE_AMPLITUDE: f64 = 0.5;

pub const NON_FINITE: Metric = Metric::new(
    "non-finite-samples",
    "Non-finite samples out",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);
pub const POISONED: Metric = Metric::new(
    "non-finite-a-second-on",
    "Non-finite a second after a NaN and an infinity in",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);
pub const RECOVERED: Metric = Metric::new(
    "null-a-second-after-the-poison-db",
    "Null against a clean run a second on (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const PEAK: Metric = Metric::new(
    "loudest-peak-dbfs",
    "Loudest sample out (dBFS)",
    Better::Same,
    Tolerance::DECIBELS,
    Shown::Digits(2),
);
pub const SILENCE: Metric = Metric::new(
    "silence-peak-dbfs",
    "Silence in, peak out after 0.1 s (dBFS)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const TAIL: Metric = Metric::new(
    "tail-dbfs",
    "Left after 1.5 s of silence (dBFS)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const SUBNORMAL: Metric = Metric::new(
    "subnormal-samples",
    "Under f32's normal range in 12 s of silence",
    Better::Lower,
    Tolerance::EXACT,
    Shown::Count,
);
pub const BLOCK_NULL: Metric = Metric::new(
    "block-null-db",
    "Worst null across block sizes (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const RESET_NULL: Metric = Metric::new(
    "reset-null-db",
    "Null of a run after a flush and reset (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const LENGTH: Metric = Metric::new(
    "frames-beyond-the-input",
    "Frames out beyond the input's",
    Better::Same,
    Tolerance::EXACT,
    Shown::Count,
);

const METRICS: [&Metric; 10] = [
    &NON_FINITE,
    &POISONED,
    &RECOVERED,
    &PEAK,
    &SILENCE,
    &TAIL,
    &SUBNORMAL,
    &BLOCK_NULL,
    &RESET_NULL,
    &LENGTH,
];

const PAIRS: [Pair; 8] = [
    Pair::at(SampleRate::HZ_44100),
    Pair::at(SampleRate::HZ_48000),
    Pair::at(SampleRate::HZ_96000),
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_48000, SampleRate::HZ_44100),
    Pair::new(SampleRate::HZ_96000, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_192000, SampleRate::HZ_44100),
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_96000),
];

struct Borrowed {
    name: SubjectName,
    pairs: Vec<Pair>,
    inner: Box<dyn Subject>,
}

impl Subject for Borrowed {
    fn name(&self) -> SubjectName {
        self.name.clone()
    }

    fn family(&self) -> Family {
        self.inner.family()
    }

    fn supports(&self, pair: Pair) -> bool {
        self.pairs.contains(&pair) && self.inner.supports(pair)
    }

    fn layout(&self) -> Option<ChannelLayout> {
        self.inner.layout()
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, block: usize) -> Result<Box<dyn Running>> {
        self.inner.open(pair, layout, block)
    }
}

fn borrowed() -> Result<Vec<Box<dyn Subject>>> {
    let mut subjects: Vec<Box<dyn Subject>> = Vec::new();

    for suite in suites::all()
        .into_iter()
        .filter(|suite| suite.name() != NAME)
    {
        let pairs: Vec<Pair> = suite
            .pairs()
            .into_iter()
            .filter(|pair| PAIRS.contains(pair))
            .collect();

        for inner in suite
            .subjects()?
            .into_iter()
            .filter(|inner| inner.is_ours())
        {
            subjects.push(Box::new(Borrowed {
                name: SubjectName(format!("{}: {}", suite.name(), inner.name())),
                pairs: pairs.clone(),
                inner,
            }));
        }
    }
    Ok(subjects)
}

enum Generated {
    Noise,
    Sounding(Vec<f64>),
}

fn frames_of(seconds: f64, rate: f64) -> usize {
    (seconds * rate) as usize
}

fn square(rate: f64, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|index| {
            if signal::phase_at(SQUARE_HZ, index, rate) < std::f64::consts::PI {
                1.0
            } else {
                -1.0
            }
        })
        .collect()
}

fn alternating(frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
        .collect()
}

fn swept(rate: f64, frames: usize) -> Vec<f64> {
    let to = SWEEP_TO_NYQUIST * rate / 2.0;
    let seconds = frames as f64 / rate;
    let spread = (to / SWEEP_FROM_HZ).ln();

    (0..frames)
        .map(|index| {
            let along = index as f64 / frames as f64;
            let phase = std::f64::consts::TAU * SWEEP_FROM_HZ * seconds / spread
                * ((along * spread).exp() - 1.0);

            phase.sin()
        })
        .collect()
}

fn impulses(rate: f64, frames: usize) -> Vec<f64> {
    let apart = frames_of(IMPULSES_APART_SECONDS, rate).max(1);

    (0..frames)
        .map(|index| if index % apart == 0 { 1.0 } else { 0.0 })
        .collect()
}

fn battery(rate: f64) -> Vec<Generated> {
    let frames = frames_of(SOUNDING_SECONDS, rate);

    vec![
        Generated::Noise,
        Generated::Sounding(square(rate, frames)),
        Generated::Sounding(alternating(frames)),
        Generated::Sounding(vec![DC_LEVEL; frames]),
        Generated::Sounding(impulses(rate, frames)),
        Generated::Sounding(swept(rate, frames)),
        Generated::Sounding(signal::sine(QUIET_TONE_HZ, BELOW_EVERY_WORD, rate, frames)),
    ]
}

fn laid(generated: &Generated, channels: usize, rate: f64, silent_after: usize) -> Vec<f64> {
    let sounding = frames_of(SOUNDING_SECONDS, rate);
    let mut whole = vec![0.0; (sounding + silent_after) * channels];

    match generated {
        Generated::Noise => {
            let mut noise = Noise::seeded(SEED ^ rate as u64);

            for sample in whole.iter_mut().take(sounding * channels) {
                *sample = 2.0 * noise.unit() - 1.0;
            }
        }
        Generated::Sounding(samples) => {
            for (frame, &sample) in whole.chunks_exact_mut(channels).zip(samples) {
                frame.fill(sample);
            }
        }
    }
    whole
}

fn probe(channels: usize, rate: f64) -> Vec<f64> {
    let sounding = frames_of(SOUNDING_SECONDS, rate);
    let mut probe = laid(&Generated::Noise, channels, rate, sounding);

    probe[sounding * channels..(sounding + 1) * channels].fill(1.0);
    probe
}

fn clean(channels: usize, rate: f64) -> Vec<f64> {
    signal::spread(
        &signal::sine(
            POISONED_TONE_HZ,
            POISONED_TONE_AMPLITUDE,
            rate,
            frames_of(POISONED_FOR_SECONDS, rate),
        ),
        channels,
    )
}

fn poisoned(channels: usize, rate: f64) -> Vec<f64> {
    let mut music = clean(channels, rate);

    music[frames_of(NOT_A_NUMBER_AT_SECONDS, rate) * channels] = f64::NAN;
    music[frames_of(INFINITY_AT_SECONDS, rate) * channels + channels - 1] = f64::INFINITY;
    music
}

fn non_finite(output: &[f64]) -> usize {
    output.iter().filter(|sample| !sample.is_finite()).count()
}

fn subnormal(output: &[f64]) -> usize {
    output
        .iter()
        .filter(|sample| {
            let magnitude = sample.abs();

            magnitude > 0.0 && magnitude < f64::from(f32::MIN_POSITIVE)
        })
        .count()
}

fn peak(output: &[f64]) -> f64 {
    output
        .iter()
        .filter(|sample| sample.is_finite())
        .fold(0.0, |loudest: f64, sample| loudest.max(sample.abs()))
}

fn after(heard: &Ran, seconds: f64, output_hz: f64) -> &[f64] {
    heard
        .output
        .get(frames_of(seconds, output_hz) * heard.channels..)
        .unwrap_or_default()
}

fn tail(heard: &Ran, output_hz: f64) -> f64 {
    let frames = frames_of(TAIL_SECONDS, output_hz);
    let start = heard.frames().saturating_sub(frames) * heard.channels;

    decibels(rms(heard.output.get(start..).unwrap_or_default()))
}

fn matching_null(heard: &[f64], wanted: &[f64]) -> f64 {
    if heard.len() == wanted.len() {
        null_db(heard, wanted)
    } else {
        UNALIKE_LENGTHS_DB
    }
}

pub struct Stressing;

impl Suite for Stressing {
    fn name(&self) -> &'static str {
        NAME
    }

    fn title(&self) -> &'static str {
        "Generated inputs, every stage and chain"
    }

    fn about(&self) -> &'static str {
        "every subject of ours the other suites hold, fed full-scale noise, a square, Nyquist, a DC step, impulses, a sweep, a tone under every word, a NaN and an infinity and silence, in blocks of 1 to 4 096 and again after a reset"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        borrowed()
    }

    fn pairs(&self) -> Vec<Pair> {
        PAIRS.to_vec()
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let rate = pair.input_hz();
        let output_hz = pair.output_hz();
        let layout = subject.layout_or(ChannelLayout::Stereo);
        let channels = usize::from(layout.count().get());
        let silent_after = frames_of(SILENT_AFTER_SECONDS, rate);

        let mut non_finite_count = 0;
        let mut loudest: f64 = 0.0;
        let mut tail_db = f64::NEG_INFINITY;
        let mut beyond = f64::NAN;

        for generated in battery(rate) {
            let input = laid(&generated, channels, rate, silent_after);
            let heard = ran(subject, pair, layout, &input, USUAL_BLOCK)?;

            non_finite_count += non_finite(&heard.output);
            loudest = loudest.max(peak(&heard.output));
            tail_db = tail_db.max(tail(&heard, output_hz));
            if matches!(generated, Generated::Noise) {
                let frames_in = input.len() / channels;

                beyond = heard.frames() as f64 - (frames_in as f64 * output_hz / rate).round();
            }
        }

        let silence = ran(
            subject,
            pair,
            layout,
            &vec![0.0; silent_after * channels],
            USUAL_BLOCK,
        )?;
        let decaying = ran(
            subject,
            pair,
            layout,
            &laid(
                &Generated::Noise,
                channels,
                rate,
                frames_of(LONG_SILENCE_SECONDS, rate),
            ),
            USUAL_BLOCK,
        )?;

        non_finite_count += non_finite(&silence.output) + non_finite(&decaying.output);

        let after_the_poison = ran(
            subject,
            pair,
            layout,
            &poisoned(channels, rate),
            USUAL_BLOCK,
        )?;
        let clean_run = ran(subject, pair, layout, &clean(channels, rate), USUAL_BLOCK)?;
        let poisoned_after = after(&after_the_poison, RECOVERED_WITHIN_SECONDS, output_hz);

        let recovered = matching_null(
            poisoned_after,
            after(&clean_run, RECOVERED_WITHIN_SECONDS, output_hz),
        );

        let probe = probe(channels, rate);
        let usual = ran(subject, pair, layout, &probe, USUAL_BLOCK)?.output;
        let mut worst_null = f64::NEG_INFINITY;

        for block in BLOCKS {
            let other = ran(subject, pair, layout, &probe, block)?.output;

            worst_null = worst_null.max(matching_null(&other, &usual));
        }

        let reset_null = ran_again_after_a_reset(subject, pair, layout, &probe, USUAL_BLOCK)?
            .map_or(f64::NAN, |(first, again)| matching_null(&again, &first));

        Ok(Outcome::of(vec![
            Reading::of(&NON_FINITE, non_finite_count as f64),
            Reading::of(&POISONED, non_finite(poisoned_after) as f64),
            Reading::of(&RECOVERED, recovered),
            Reading::of(&PEAK, decibels(loudest)),
            Reading::of(
                &SILENCE,
                decibels(peak(after(
                    &silence,
                    SILENCE_HEARD_AFTER_SECONDS,
                    output_hz,
                ))),
            ),
            Reading::of(&TAIL, tail_db),
            Reading::of(&SUBNORMAL, subnormal(&decaying.output) as f64),
            Reading::of(&BLOCK_NULL, worst_null),
            Reading::of(&RESET_NULL, reset_null),
            Reading::of(&LENGTH, beyond),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    const UP: Pair = Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000);
    const AT: Pair = Pair::at(SampleRate::HZ_48000);
    const NOTHING_DB: f64 = -500.0;

    #[test]
    fn a_resampler_hands_on_the_same_track_in_blocks_shorter_than_its_filter() {
        let heard = measured_as(&Stressing, "resample: Resonate Low latency", UP);

        assert_eq!(heard.value(&BLOCK_NULL), f64::NEG_INFINITY);
    }

    #[test]
    fn a_convolver_reset_after_its_flush_flushes_the_same_tail_again() {
        let heard = measured_as(&Stressing, "convolve: Unit impulse", AT);

        assert_eq!(heard.value(&RESET_NULL), f64::NEG_INFINITY);
    }

    #[test]
    fn dither_falls_silent_in_silence_and_an_equaliser_lets_go_of_a_nan() {
        let dithered = measured_as(&Stressing, "dither-16: Dither triangular, threshold", AT);
        let equalised = measured_as(&Stressing, "eq: Ten octave bands", AT);

        assert!(dithered.value(&SILENCE) < NOTHING_DB);
        assert_eq!(equalised.value(&POISONED), 0.0);
        assert_eq!(equalised.value(&RECOVERED), f64::NEG_INFINITY);
    }

    #[test]
    fn every_input_is_at_full_scale_or_under_and_the_sweep_starts_low() {
        let rate = 48_000.0;
        let frames = frames_of(SOUNDING_SECONDS, rate);

        for samples in [
            square(rate, frames),
            alternating(frames),
            swept(rate, frames),
            impulses(rate, frames),
        ] {
            assert!(peak(&samples) <= 1.0);
            assert!(peak(&samples) > 0.99);
        }

        let sweep = swept(rate, frames);
        let first_turn = sweep.windows(2).position(|pair| pair[1] < pair[0]);

        assert!(first_turn.is_some_and(|turn| turn > frames_of(0.01, rate)));
    }
}
