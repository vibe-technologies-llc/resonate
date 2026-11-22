use resonate_core::{ChannelLayout, SampleRate};

use crate::{
    analysis::{
        level::{self, power_decibels},
        spectrum::{AUDIBLE_FROM_HZ, AUDIBLE_TO_HZ, Spectrum},
    },
    error::Result,
    lab::{
        pair::Pair,
        subject::{Ran, Subject, ran},
    },
    signal::{self, on_the_bins},
};

pub const TONE_HZ: f64 = 997.0;
pub const LOUD_DBFS: f64 = -1.0;
pub const MIDDLE_DBFS: f64 = -20.0;
pub const QUIET_DBFS: f64 = -60.0;
const SETTLE_SECONDS: f64 = 0.75;
const TAIL_SECONDS: f64 = 0.25;
const FRAMES_AT_48_KHZ: usize = 65_536;
const FULL_SCALE_SINE_POWER: f64 = 0.5;
const RESPONSE_TONES: usize = 48;
const RESPONSE_SEED: u64 = 0x7265_7370_6F6E_7365;
const RESPONSE_REACH_OF_THE_BAND: f64 = 0.45;
const SMPTE_LOW_HZ: f64 = 60.0;
const SMPTE_HIGH_HZ: f64 = 7_000.0;
const SMPTE_SIDEBANDS: usize = 4;
const SMPTE_LOW_SHARE: f64 = 0.8;
const IMPULSE_AT_SECONDS: f64 = 0.5;
const IMPULSE_SECONDS: f64 = 1.5;
const IMPULSE_LEVEL: f64 = 0.5;
pub const BLOCK: usize = 1_024;

pub fn amplitude_of(dbfs: f64) -> f64 {
    10_f64.powf(dbfs / 20.0)
}

#[derive(Clone, Copy, Debug)]
pub struct Feed {
    pub layout: ChannelLayout,
    pub on: Option<usize>,
}

impl Feed {
    pub const fn every(layout: ChannelLayout) -> Self {
        Self { layout, on: None }
    }

    pub const fn only(layout: ChannelLayout, channel: usize) -> Self {
        Self {
            layout,
            on: Some(channel),
        }
    }

    pub fn channels(self) -> usize {
        usize::from(self.layout.count().get())
    }

    pub fn laid(self, mono: &[f64]) -> Vec<f64> {
        match self.on {
            Some(channel) => signal::on_one(mono, channel, self.channels()),
            None => signal::spread(mono, self.channels()),
        }
    }
}

pub fn analysed_frames(rate: SampleRate) -> usize {
    let wider = (f64::from(rate.hz()) / 48_000.0).ceil() as usize;

    FRAMES_AT_48_KHZ * wider.next_power_of_two()
}

pub struct Steady {
    pub channels: Vec<Vec<f64>>,
    pub rate: f64,
}

impl Steady {
    pub fn spectra(&self) -> Vec<Spectrum> {
        self.channels
            .iter()
            .map(|samples| Spectrum::of(samples, self.rate))
            .collect()
    }
}

pub fn steady(
    subject: &dyn Subject,
    pair: Pair,
    feed: Feed,
    signal: impl Fn(f64, usize) -> Vec<f64>,
) -> Result<Steady> {
    let frames = analysed_frames(pair.to);
    let input_frames = ((SETTLE_SECONDS + TAIL_SECONDS) * pair.input_hz()).ceil() as usize
        + (frames as f64 * pair.input_hz() / pair.output_hz()).ceil() as usize;
    let input = feed.laid(&signal(pair.input_hz(), input_frames));

    let heard = ran(subject, pair, feed.layout, &input, BLOCK)?;
    let skip = (SETTLE_SECONDS * pair.output_hz()) as usize;

    Ok(Steady {
        channels: (0..heard.channels)
            .map(|channel| {
                let mut samples = heard.channel(channel);

                samples.resize(skip + frames, 0.0);
                samples[skip..skip + frames].to_vec()
            })
            .collect(),
        rate: pair.output_hz(),
    })
}

pub fn tone_hz(pair: Pair, wanted: f64) -> f64 {
    on_the_bins(wanted, pair.output_hz(), analysed_frames(pair.to))
}

#[derive(Clone, Copy, Debug)]
pub struct ToneHeard {
    pub gain_db: f64,
    pub thd_db: f64,
    pub thd_n_db: f64,
    pub noise_a_db: f64,
}

pub fn heard_tone(spectrum: &Spectrum, hz: f64, amplitude: f64) -> ToneHeard {
    let fundamental = spectrum.power_at(hz);
    let top = spectrum.audible_top();
    let harmonics: f64 = spectrum
        .harmonics_of(hz)
        .iter()
        .map(|&harmonic| spectrum.power_at(harmonic))
        .sum();
    let rest = spectrum.band_without(AUDIBLE_FROM_HZ, top, &[hz]);
    let weighted = spectrum.weighted_without(AUDIBLE_FROM_HZ, top, &[hz]);

    ToneHeard {
        gain_db: level::decibels(spectrum.amplitude_at(hz) / amplitude),
        thd_db: power_decibels(harmonics / fundamental),
        thd_n_db: power_decibels(rest / fundamental),
        noise_a_db: power_decibels(weighted / FULL_SCALE_SINE_POWER),
    }
}

pub fn tone(subject: &dyn Subject, pair: Pair, feed: Feed, dbfs: f64) -> Result<Vec<ToneHeard>> {
    let hz = tone_hz(pair, TONE_HZ);
    let amplitude = amplitude_of(dbfs);

    let steady = steady(subject, pair, feed, |rate, frames| {
        signal::sine(hz, amplitude, rate, frames)
    })?;

    Ok(steady
        .spectra()
        .iter()
        .map(|spectrum| heard_tone(spectrum, hz, amplitude))
        .collect())
}

pub fn worst(heard: &[ToneHeard], read: fn(&ToneHeard) -> f64) -> f64 {
    heard.iter().map(read).fold(f64::NEG_INFINITY, f64::max)
}

pub fn heard_where_fed(feed: Feed, values: &[f64]) -> f64 {
    match feed.on {
        Some(_) => values.first().copied().unwrap_or(f64::NAN),
        None => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

pub fn response_tones(pair: Pair) -> Vec<f64> {
    let top = AUDIBLE_TO_HZ.min(RESPONSE_REACH_OF_THE_BAND * pair.input_hz().min(pair.output_hz()));
    let frames = analysed_frames(pair.to);
    let spread = (top / AUDIBLE_FROM_HZ).ln();

    let mut tones: Vec<f64> = (0..RESPONSE_TONES)
        .map(|index| {
            let along = index as f64 / (RESPONSE_TONES - 1) as f64;
            on_the_bins(
                AUDIBLE_FROM_HZ * (spread * along).exp(),
                pair.output_hz(),
                frames,
            )
        })
        .collect();

    tones.dedup();
    tones
}

pub fn response(
    subject: &dyn Subject,
    pair: Pair,
    feed: Feed,
    dbfs: f64,
) -> Result<Vec<Vec<(f64, f64)>>> {
    let hz = response_tones(pair);
    let amplitude = amplitude_of(dbfs) / hz.len() as f64;
    let mut noise = signal::Noise::seeded(RESPONSE_SEED);

    let tones: Vec<(f64, f64, f64)> = hz
        .iter()
        .map(|&hz| (hz, amplitude, std::f64::consts::TAU * noise.unit()))
        .collect();

    let steady = steady(subject, pair, feed, |rate, frames| {
        signal::phased(&tones, rate, frames)
    })?;

    Ok(steady
        .spectra()
        .iter()
        .map(|spectrum| {
            hz.iter()
                .map(|&hz| (hz, level::decibels(spectrum.amplitude_at(hz) / amplitude)))
                .collect()
        })
        .collect())
}

pub fn spread_db(points: &[(f64, f64)]) -> f64 {
    let loudest = points
        .iter()
        .map(|&(_, db)| db)
        .fold(f64::NEG_INFINITY, f64::max);
    let quietest = points
        .iter()
        .map(|&(_, db)| db)
        .fold(f64::INFINITY, f64::min);

    loudest - quietest
}

pub fn smpte_imd(subject: &dyn Subject, pair: Pair, feed: Feed) -> Result<Vec<f64>> {
    let low = tone_hz(pair, SMPTE_LOW_HZ);
    let high = tone_hz(pair, SMPTE_HIGH_HZ);
    let whole = amplitude_of(LOUD_DBFS);

    let tones = [
        (low, SMPTE_LOW_SHARE * whole),
        (high, (1.0 - SMPTE_LOW_SHARE) * whole),
    ];

    let steady = steady(subject, pair, feed, |rate, frames| {
        signal::summed(&tones, rate, frames)
    })?;

    Ok(steady
        .spectra()
        .iter()
        .map(|spectrum| {
            let sidebands: f64 = (1..=SMPTE_SIDEBANDS)
                .flat_map(|order| {
                    let apart = order as f64 * low;
                    [high - apart, high + apart]
                })
                .map(|hz| spectrum.power_at(hz))
                .sum();
            power_decibels(sidebands / spectrum.power_at(high))
        })
        .collect())
}

pub struct Delay {
    pub measured_frames: f64,
    pub claimed_frames: Option<f64>,
}

pub fn delay(subject: &dyn Subject, pair: Pair, feed: Feed, heard_on: usize) -> Result<Delay> {
    let frames = (IMPULSE_SECONDS * pair.input_hz()) as usize;
    let at = (IMPULSE_AT_SECONDS * pair.input_hz()) as usize;

    let impulse: Vec<f64> = signal::impulse(frames, at)
        .into_iter()
        .map(|sample| sample * IMPULSE_LEVEL)
        .collect();
    let input = feed.laid(&impulse);

    let heard: Ran = ran(subject, pair, feed.layout, &input, BLOCK)?;
    let channel = heard_on.min(heard.channels - 1);

    let peak = level::loudest(&heard.channel(channel));

    Ok(Delay {
        measured_frames: peak as f64 - at as f64 * pair.output_hz() / pair.input_hz(),
        claimed_frames: heard.claimed_latency_frames,
    })
}

pub fn held_back(subject: &dyn Subject, pair: Pair, feed: Feed) -> Result<usize> {
    Ok(delay(subject, pair, feed, feed.on.unwrap_or(0))?
        .measured_frames
        .round()
        .max(0.0) as usize)
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;
    use resonate_dsp::{GainConfig, GainStage};

    use super::*;
    use crate::subjects::stage::Stage;

    fn unity() -> Stage {
        Stage::new("Unity", |_, _| {
            Ok(Box::new(GainStage::new(GainConfig::default())))
        })
    }

    #[test]
    fn a_subject_that_changes_nothing_reads_as_ideal_everywhere() -> Result<()> {
        let pair = Pair::at(SampleRate::HZ_48000);
        let feed = Feed::every(ChannelLayout::Stereo);

        let tones = tone(&unity(), pair, feed, MIDDLE_DBFS)?;
        let response = response(&unity(), pair, feed, MIDDLE_DBFS)?;
        let imd = smpte_imd(&unity(), pair, feed)?;
        let delayed = delay(&unity(), pair, feed, 0)?;

        assert!(worst(&tones, |tone| tone.gain_db.abs()) < 1e-9);
        assert!(worst(&tones, |tone| tone.thd_n_db) < -250.0);
        assert!(response.iter().all(|points| spread_db(points) < 1e-9));
        assert!(imd.iter().all(|&db| db < -250.0));
        assert_eq!(delayed.measured_frames, 0.0);
        assert_eq!(delayed.claimed_frames, Some(0.0));
        Ok(())
    }

    #[test]
    fn a_tone_fed_to_one_channel_is_read_there_alone() {
        let feed = Feed::only(ChannelLayout::Stereo, 0);

        assert_eq!(heard_where_fed(feed, &[-120.0, 80.0]), -120.0);
        assert_eq!(
            heard_where_fed(Feed::every(ChannelLayout::Stereo), &[-120.0, 80.0]),
            80.0
        );
    }
}
