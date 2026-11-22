use std::{collections::BTreeMap, fmt::Write as _, sync::Arc};

use resonate_core::{
    AppliedGain, ChannelLayout, Decibels, Gain, SampleFormat, SampleRate, StreamSpec, Volume,
    eq::Frequency,
};
use resonate_dsp::{Crossfeed, FilterPhase, Impulse, Quality, Remix, Restoration, Tuning};
use resonate_engine::{Decoded, EngineConfig, Equalisation, SinkFormats, Words};

use crate::{
    analysis::{
        fidelity::{
            self, BLOCK, Feed, LOUD_DBFS, MIDDLE_DBFS, QUIET_DBFS, amplitude_of, delay,
            heard_where_fed, held_back, response, smpte_imd, spread_db,
        },
        level::{decibels, null_db},
    },
    error::{Error, Result},
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        readings::{Measured, find},
        subject::{Subject, SubjectName, ran},
        suite::{Outcome, Suite},
    },
    report::markdown::{cell, header, row},
    signal,
    subjects::played::{Played, in_words, sink},
    suites::{convolve::decaying, eq},
};

const NULL_SECONDS: f64 = 1.0;
const MP3_WALL_HZ: f64 = 16_000.0;
const ROOM_SEED: u64 = 0x726F_6F6D;

pub const THD_LOUD: Metric = Metric::new(
    "thd-1-dbfs-db",
    "THD at −1 dBFS (dB)",
    Better::Lower,
    Tolerance::AMONG_DITHER,
    Shown::Decibels,
);
pub const THD_N_LOUD: Metric = Metric::new(
    "thd-n-1-dbfs-db",
    "THD+N at −1 dBFS (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const THD_N_MIDDLE: Metric = Metric::new(
    "thd-n-20-dbfs-db",
    "THD+N at −20 dBFS (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const DYNAMIC_RANGE: Metric = Metric::new(
    "dynamic-range-db",
    "Dynamic range, A-weighted (dB)",
    Better::Higher,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const IMD: Metric = Metric::new(
    "smpte-imd-db",
    "SMPTE IMD (dB)",
    Better::Lower,
    Tolerance::AMONG_DITHER,
    Shown::Decibels,
);
pub const SPREAD: Metric = Metric::new(
    "response-spread-db",
    "Response spread 20 Hz–20 kHz (dB)",
    Better::Lower,
    Tolerance::within(0.05),
    Shown::Digits(3),
);
pub const ASKED: Metric = Metric::new(
    "gain-asked-db",
    "Gain the plan asks (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const HEARD: Metric = Metric::new(
    "gain-heard-db",
    "Gain heard at 1 kHz (dB)",
    Better::Same,
    Tolerance::FINE_DECIBELS,
    Shown::Digits(2),
);
pub const CLAIMED: Metric = Metric::new(
    "claimed-latency-frames",
    "Claimed latency (frames)",
    Better::Lower,
    Tolerance::within(1.0),
    Shown::Digits(1),
);
pub const MEASURED: Metric = Metric::new(
    "measured-latency-frames",
    "Measured latency (frames)",
    Better::Lower,
    Tolerance::within(1.0),
    Shown::Digits(1),
);
pub const NULL: Metric = Metric::new(
    "null-against-the-input-db",
    "Null against the input (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);

const METRICS: [&Metric; 11] = [
    &THD_LOUD,
    &THD_N_LOUD,
    &THD_N_MIDDLE,
    &DYNAMIC_RANGE,
    &IMD,
    &SPREAD,
    &ASKED,
    &HEARD,
    &CLAIMED,
    &MEASURED,
    &NULL,
];

fn spec(rate: SampleRate, channels: ChannelLayout, format: SampleFormat) -> StreamSpec {
    StreamSpec::new(rate, channels, format)
}

fn offering(format: SampleFormat, rate: SampleRate, channels: ChannelLayout) -> Vec<SinkFormats> {
    vec![SinkFormats {
        format,
        words: Words::Whole,
        rates: vec![rate],
        channels: vec![channels],
    }]
}

struct Scenario {
    name: &'static str,
    source: Decoded,
    sink: Vec<SinkFormats>,
    config: EngineConfig,
    gain: AppliedGain,
}

impl Scenario {
    fn new(name: &'static str, source: StreamSpec, sink: Vec<SinkFormats>) -> Self {
        Self {
            name,
            source: Decoded::samples(source),
            sink,
            config: EngineConfig::default(),
            gain: AppliedGain::default(),
        }
    }

    fn with(mut self, change: impl FnOnce(&mut EngineConfig)) -> Self {
        change(&mut self.config);
        self
    }

    fn played(self) -> Played {
        Played::new(
            self.name,
            self.source,
            &sink(self.sink),
            self.config,
            self.gain,
        )
    }
}

fn scenarios() -> Result<Vec<Played>> {
    use ChannelLayout::{Stereo, Surround51};
    use SampleFormat::{F32, S16, S24};
    use SampleRate as Hz;

    let ten_bands = eq::profiles()?
        .into_iter()
        .find(|(name, _)| *name == "Ten octave bands")
        .map(|(_, profile)| profile);
    let room = Impulse::new(Hz::HZ_48000, vec![decaying(1.0, ROOM_SEED, 0.5, 0)])
        .ok_or_else(|| Error::ResponseRefused {
            subject: SubjectName("Room correction, 1 s".to_owned()),
        })?
        .with_headroom();
    let half = Volume::heard_at(Gain::new(0.5).map_err(Error::Vocabulary)?);
    let raised = AppliedGain {
        gain: Some(Decibels::new(6.0).map_err(Error::Vocabulary)?),
        peak: None,
    };
    let mut restored = Scenario::new(
        "MP3 extended past its wall, 44.1 on 48/24",
        spec(Hz::HZ_44100, Stereo, F32),
        offering(S24, Hz::HZ_48000, Stereo),
    )
    .with(|config| config.restoration = Restoration::Extend);

    restored.source.tuning = Some(Tuning::Mp3);
    restored.source.lowpass = Some(Frequency::from_hertz(MP3_WALL_HZ).map_err(Error::Vocabulary)?);

    let mut guarded = Scenario::new(
        "Half volume, ReplayGain +6 dB, guarded, 44.1/16 on 44.1/24",
        spec(Hz::HZ_44100, Stereo, S16),
        offering(S24, Hz::HZ_44100, Stereo),
    )
    .with(|config| config.volume = half);

    guarded.gain = raised;

    Ok(vec![
        Scenario::new(
            "CD on a CD sink",
            spec(Hz::HZ_44100, Stereo, S16),
            offering(S16, Hz::HZ_44100, Stereo),
        )
        .played(),
        Scenario::new(
            "CD on 48/24",
            spec(Hz::HZ_44100, Stereo, S16),
            offering(S24, Hz::HZ_48000, Stereo),
        )
        .played(),
        Scenario::new(
            "CD on 48/16",
            spec(Hz::HZ_44100, Stereo, S16),
            offering(S16, Hz::HZ_48000, Stereo),
        )
        .played(),
        Scenario::new(
            "96/24 on 48/16, very high",
            spec(Hz::HZ_96000, Stereo, S24),
            offering(S16, Hz::HZ_48000, Stereo),
        )
        .with(|config| config.quality = Quality::VeryHigh)
        .played(),
        Scenario::new(
            "192/24 on 44.1/24, low latency",
            spec(Hz::HZ_192000, Stereo, S24),
            offering(S24, Hz::HZ_44100, Stereo),
        )
        .with(|config| config.quality = Quality::LowLatency)
        .played(),
        Scenario::new(
            "CD on 96 kHz float, minimum phase",
            spec(Hz::HZ_44100, Stereo, S16),
            offering(F32, Hz::HZ_96000, Stereo),
        )
        .with(|config| config.filter_phase = FilterPhase::Minimum)
        .played(),
        Scenario::new(
            "5.1 at 48/24 on a stereo sink",
            spec(Hz::HZ_48000, Surround51, S24),
            offering(S24, Hz::HZ_48000, Stereo),
        )
        .played(),
        Scenario::new(
            "Ten bands and moderate crossfeed, 48/24",
            spec(Hz::HZ_48000, Stereo, S24),
            offering(S24, Hz::HZ_48000, Stereo),
        )
        .with(|config| {
            config.equaliser = Arc::new(Equalisation {
                enabled: true,
                bound: BTreeMap::new(),
                fallback: ten_bands,
            });
            config.crossfeed = Crossfeed::Moderate;
        })
        .played(),
        guarded.played(),
        restored.played(),
        Scenario::new(
            "Room correction of 1 s, 48/24",
            spec(Hz::HZ_48000, Stereo, S24),
            offering(S24, Hz::HZ_48000, Stereo),
        )
        .with(|config| config.convolution = Some(Arc::new(room)))
        .played(),
    ])
}

fn feed_of(played: &Played) -> Feed {
    let layout = played.source().channels;

    if layout == ChannelLayout::Stereo || layout == ChannelLayout::Mono {
        Feed::every(layout)
    } else {
        Feed::only(layout, 0)
    }
}

fn remixed_gain(played: &Played) -> f64 {
    played
        .plan()
        .remix
        .and_then(|(from, to)| Remix::new(from, to).gain(0, 0))
        .unwrap_or(1.0)
}

pub struct Chains;

impl Suite for Chains {
    fn name(&self) -> &'static str {
        "chain"
    }

    fn title(&self) -> &'static str {
        "Whole chains, as the engine plans them"
    }

    fn about(&self) -> &'static str {
        "a source, a sink and settings planned by the engine and built as it builds them, words in and words out: distortion, noise, response, gain and latency"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        Ok(scenarios()?
            .into_iter()
            .map(|played| Box::new(played) as Box<dyn Subject>)
            .collect())
    }

    fn pairs(&self) -> Vec<Pair> {
        let mut pairs: Vec<Pair> = Vec::new();

        for played in scenarios().unwrap_or_default() {
            if !pairs.contains(&played.pair()) {
                pairs.push(played.pair());
            }
        }
        pairs
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let Some(played) = scenarios()?
            .into_iter()
            .find(|played| played.name() == subject.name())
        else {
            return Ok(Outcome::of(Vec::new()));
        };
        let feed = feed_of(&played);

        let loud = fidelity::tone(subject, pair, feed, LOUD_DBFS)?;
        let middle = fidelity::tone(subject, pair, feed, MIDDLE_DBFS)?;
        let quiet = fidelity::tone(subject, pair, feed, QUIET_DBFS)?;
        let imd = heard_where_fed(feed, &smpte_imd(subject, pair, feed)?);
        let read = |tones: &[fidelity::ToneHeard], reading: fn(&fidelity::ToneHeard) -> f64| {
            heard_where_fed(feed, &tones.iter().map(reading).collect::<Vec<_>>())
        };
        let spread = response(subject, pair, feed, MIDDLE_DBFS)?
            .first()
            .map_or(f64::NAN, |points| spread_db(points));
        let delayed = delay(subject, pair, feed, 0)?;
        let asked = decibels(f64::from(played.plan().amplitude()) * remixed_gain(&played));
        let heard = middle.first().map_or(f64::NAN, |tone| tone.gain_db);

        Ok(Outcome::of(vec![
            Reading::of(&THD_LOUD, read(&loud, |tone| tone.thd_db)),
            Reading::of(&THD_N_LOUD, read(&loud, |tone| tone.thd_n_db)),
            Reading::of(&THD_N_MIDDLE, read(&middle, |tone| tone.thd_n_db)),
            Reading::of(&DYNAMIC_RANGE, -read(&quiet, |tone| tone.noise_a_db)),
            Reading::of(&IMD, imd),
            Reading::of(&SPREAD, spread),
            Reading::of(&ASKED, asked),
            Reading::of(&HEARD, heard),
            Reading::of(&CLAIMED, delayed.claimed_frames.unwrap_or(f64::NAN)),
            Reading::of(&MEASURED, delayed.measured_frames),
            Reading::of(&NULL, null_against_the_input(subject, &played, pair)?),
        ]))
    }

    fn report(&self, subjects: &[Box<dyn Subject>], measured: &[Measured]) -> String {
        let scenarios = scenarios().unwrap_or_default();
        let mut text = String::from("### What each chain is\n\n");

        text.push_str(&header(&["Chain", "From", "To", "Mode", "Stages"]));
        for played in scenarios.iter().filter(|played| {
            subjects
                .iter()
                .any(|subject| subject.name() == played.name())
        }) {
            let source = played.source();
            let stream = played.plan().stream;

            text.push_str(&row(&[
                played.name().to_string(),
                format!(
                    "{} {:?} {:?}",
                    source.rate.kilohertz(),
                    source.channels,
                    source.format
                ),
                format!(
                    "{} {:?} {:?}",
                    stream.rate.kilohertz(),
                    stream.channels,
                    stream.format
                ),
                format!("{:?}", played.plan().mode),
                played.stages(),
            ]));
        }

        let mut columns = vec!["Chain"];

        columns.extend(METRICS.iter().map(|metric| metric.title));

        let _ = writeln!(text, "\n### What each chain does\n");

        text.push_str(&header(&columns));
        for played in &scenarios {
            let Some(case) = find(measured, &played.name(), played.pair()) else {
                continue;
            };
            let mut cells = vec![played.name().to_string()];

            cells.extend(
                METRICS
                    .iter()
                    .map(|metric| cell(metric.shown, case.value(metric))),
            );
            text.push_str(&row(&cells));
        }
        text.push('\n');
        text
    }
}

fn null_against_the_input(subject: &dyn Subject, played: &Played, pair: Pair) -> Result<f64> {
    let source = played.source();
    let stream = played.plan().stream;

    if pair.converts() || source.channels != stream.channels {
        return Ok(f64::NAN);
    }

    let channels = usize::from(source.channels.count().get());
    let frames = (NULL_SECONDS * pair.input_hz()) as usize;

    let input = signal::spread(
        &signal::sine(
            fidelity::TONE_HZ,
            amplitude_of(MIDDLE_DBFS),
            pair.input_hz(),
            frames,
        ),
        channels,
    );
    let heard = ran(subject, pair, source.channels, &input, BLOCK)?;
    let held_back = held_back(subject, pair, Feed::every(source.channels))? * channels;

    Ok(null_db(
        heard.output.get(held_back..).unwrap_or_default(),
        &in_words(&input, source.format),
    ))
}

#[cfg(test)]
mod tests {
    use resonate_engine::OutputMode;

    use super::*;
    use crate::lab::suite::measured_as;

    fn scenario(name: &str) -> Played {
        let Some(played) = scenarios()
            .unwrap_or_default()
            .into_iter()
            .find(|played| played.name().0 == name)
        else {
            panic!("no chain called {name}");
        };
        played
    }

    #[test]
    fn a_cd_on_a_cd_sink_is_played_bit_for_bit() {
        let played = scenario("CD on a CD sink");
        let heard = measured_as(&Chains, "CD on a CD sink", played.pair());

        assert_eq!(played.plan().mode, OutputMode::BitPerfect);
        assert_eq!(heard.value(&NULL), f64::NEG_INFINITY);
    }

    #[test]
    fn every_chain_is_planned_onto_what_its_sink_offers() {
        let played = scenario("96/24 on 48/16, very high");

        assert_eq!(played.plan().stream.rate, SampleRate::HZ_48000);
        assert_eq!(played.plan().stream.format, SampleFormat::S16);
        assert!(played.plan().resample.is_some());
    }

    #[test]
    fn a_downmix_is_read_on_the_channel_it_was_fed() {
        let played = scenario("5.1 at 48/24 on a stereo sink");
        let heard = measured_as(&Chains, "5.1 at 48/24 on a stereo sink", played.pair());

        assert!(heard.value(&THD_N_LOUD) < -100.0);
        assert!((heard.value(&HEARD) - heard.value(&ASKED)).abs() < 0.01);
    }
}
