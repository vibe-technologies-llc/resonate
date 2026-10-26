use std::sync::Arc;

use resonate_codec::Sources;
use resonate_core::{
    FrameSpan, MediaLocation, SampleRate, StreamSpec,
    eq::{Profile, RESPONSE_FROM_HZ},
};

use crate::{
    Error, Result, Spectrum, Watching,
    analysis::{native_format, opened},
    conform::Walking,
    spectrum::{FLOOR_DB, Transforming},
};

pub const BANDS: usize = 29;

const LOWEST_EDGE_HZ: f64 = 25.0;

const BANDS_AN_OCTAVE: f64 = 3.0;

const STEPS_A_DECIBEL: f64 = 2.0;

const SILENT: u8 = u8::MAX;

const DECIBELS_A_STEP: f64 = 1.0 / STEPS_A_DECIBEL;

fn band_of(hertz: f64) -> usize {
    if hertz <= LOWEST_EDGE_HZ {
        return 0;
    }
    let band = ((hertz / LOWEST_EDGE_HZ).log2() * BANDS_AN_OCTAVE).floor();
    (band as usize).min(BANDS - 1)
}

fn edge_of(band: usize) -> f64 {
    LOWEST_EDGE_HZ * 2.0_f64.powf(band as f64 / BANDS_AN_OCTAVE)
}

pub fn span_of_band(band: usize) -> (f64, f64) {
    let from = if band == 0 {
        RESPONSE_FROM_HZ
    } else {
        edge_of(band)
    };
    let to = if band + 1 >= BANDS {
        f64::INFINITY
    } else {
        edge_of(band + 1)
    };
    (from, to)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandLevels {
    steps: [u8; BANDS],
}

impl BandLevels {
    pub const SILENT: Self = Self {
        steps: [SILENT; BANDS],
    };

    pub fn of(spectrum: &Spectrum) -> Self {
        let mut powers = [0.0_f64; BANDS];
        let bin_hz = f64::from(spectrum.bin_hz());
        for (bin, level) in spectrum.levels().iter().enumerate().skip(1) {
            if *level > FLOOR_DB {
                powers[band_of(bin as f64 * bin_hz)] += 10_f64.powf(f64::from(*level) / 10.0);
            }
        }

        let loudest = powers.iter().copied().fold(0.0, f64::max);
        if loudest <= 0.0 {
            return Self::SILENT;
        }
        Self {
            steps: powers.map(|power| steps_below(loudest, power)),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        <[u8; BANDS]>::try_from(bytes)
            .ok()
            .map(|steps| Self { steps })
    }

    pub const fn as_bytes(&self) -> &[u8] {
        &self.steps
    }

    pub fn is_silent(&self) -> bool {
        self.steps.iter().all(|step| *step == SILENT)
    }

    fn amplitudes(&self) -> impl Iterator<Item = f64> + '_ {
        self.steps.iter().map(|step| {
            if *step == SILENT {
                0.0
            } else {
                10_f64.powf(-f64::from(*step) * DECIBELS_A_STEP / 20.0)
            }
        })
    }
}

fn steps_below(loudest: f64, power: f64) -> u8 {
    if power <= 0.0 {
        return SILENT;
    }
    let steps = (10.0 * (loudest / power).log10() * STEPS_A_DECIBEL).round();
    if steps >= f64::from(SILENT) {
        SILENT
    } else {
        steps as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandGains {
    gains: [f64; BANDS],
}

impl BandGains {
    pub fn of(profile: &Arc<Profile>, rate: SampleRate) -> Self {
        let at_the_rate = profile.at_rate(rate);
        let gains = std::array::from_fn(|band| {
            let (from, to) = span_of_band(band);
            let decibels = at_the_rate.peak_db_between(from, to, rate);
            if decibels.is_finite() {
                10_f64.powf(decibels / 20.0)
            } else {
                1.0
            }
        });
        Self { gains }
    }

    pub fn peak(&self, levels: &BandLevels, true_peak: f32) -> f64 {
        let (shaped, heard) = levels
            .amplitudes()
            .zip(self.gains)
            .fold((0.0, 0.0), |(shaped, heard), (amplitude, gain)| {
                (shaped + amplitude * gain, heard + amplitude)
            });
        if heard <= 0.0 || !true_peak.is_finite() {
            return 0.0;
        }
        f64::from(true_peak.max(0.0)) * shaped / heard
    }
}

pub fn band_levels(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    watch: &dyn Watching,
) -> Result<BandLevels> {
    let (mut decoder, info) = opened(sources, location, span)?;
    let codec = resonate_codec::Codec::from_id(info.codec);
    let format = native_format(&info, codec);
    decoder.set_output_format(format);
    decoder.follow_format_changes();

    let rate = info.spec.rate;
    let placements = info.speakers.placements(info.spec.channels.count().get());
    let mut transforming = Transforming::new(rate.hz(), &placements);
    let mut walking = Walking::new(StreamSpec::new(rate, info.spec.channels, format));
    loop {
        if watch.stopped() {
            return Err(Error::Stopped);
        }
        let Some(heard) = walking.step(&mut decoder)? else {
            break;
        };
        transforming.note(heard.samples, |_| {});
    }

    Ok(BandLevels::of(&transforming.finished()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Preamp, Q};

    use super::*;

    const BIN_HZ: f32 = 10.0;

    fn spectrum_sounding_at(hertz: f32, level_db: f32) -> Spectrum {
        let mut levels = vec![FLOOR_DB; 2_049];
        levels[(hertz / BIN_HZ) as usize] = level_db;
        Spectrum::new(BIN_HZ, levels, Duration::from_secs(10))
    }

    fn boosting(hertz: f64, decibels: f64) -> Arc<Profile> {
        let band = Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(hertz).expect("in range"),
            BandGain::from_decibels(decibels).expect("in range"),
            Q::from_units(2.0).expect("in range"),
        );
        Arc::new(Profile::new(Preamp::NONE, vec![band]).expect("a profile in range"))
    }

    #[test]
    fn every_frequency_falls_in_the_band_whose_span_holds_it() {
        for band in 0..BANDS {
            let (from, to) = span_of_band(band);
            let inside = (from * to.min(from * 2.0)).sqrt();
            assert_eq!(band_of(inside), band, "{from} to {to} Hz");
        }
        assert_eq!(band_of(0.0), 0);
        assert_eq!(band_of(96_000.0), BANDS - 1);
    }

    #[test]
    fn a_band_the_music_sounds_in_is_the_loudest_and_the_rest_are_steps_below_it() {
        let levels = BandLevels::of(&spectrum_sounding_at(1_000.0, -6.0));
        let sounding = band_of(1_000.0);

        let amplitudes: Vec<f64> = levels.amplitudes().collect();

        assert_eq!(amplitudes[sounding], 1.0);
        assert!(
            amplitudes
                .iter()
                .enumerate()
                .all(|(band, amplitude)| band == sounding || *amplitude == 0.0)
        );
        assert!(!levels.is_silent());
    }

    #[test]
    fn music_that_never_rose_above_the_floor_is_silent_in_every_band() {
        let levels = BandLevels::of(&Spectrum::new(
            BIN_HZ,
            vec![FLOOR_DB; 2_049],
            Duration::ZERO,
        ));

        assert!(levels.is_silent());
        assert_eq!(levels, BandLevels::SILENT);
        assert_eq!(
            BandGains::of(&boosting(100.0, 6.0), SampleRate::HZ_44100).peak(&levels, 0.9),
            0.0
        );
    }

    #[test]
    fn levels_read_back_from_their_bytes_and_a_wrong_length_reads_as_nothing() {
        let levels = BandLevels::of(&spectrum_sounding_at(440.0, -3.0));

        assert_eq!(BandLevels::from_bytes(levels.as_bytes()), Some(levels));
        assert_eq!(BandLevels::from_bytes(&levels.as_bytes()[1..]), None);
        assert_eq!(BandLevels::from_bytes(&[]), None);
    }

    #[test]
    fn music_on_a_boost_is_raised_by_it_and_music_far_from_it_is_not() {
        let reach = BandGains::of(&boosting(100.0, 6.0), SampleRate::HZ_44100);
        let on_the_boost = BandLevels::of(&spectrum_sounding_at(100.0, -6.0));
        let far_from_it = BandLevels::of(&spectrum_sounding_at(5_000.0, -6.0));

        let raised = reach.peak(&on_the_boost, 0.5);
        let left = reach.peak(&far_from_it, 0.5);

        assert!(
            (20.0 * (raised / 0.5).log10() - 6.0).abs() < 0.1,
            "{raised}"
        );
        assert!((left - 0.5).abs() < 0.01, "{left}");
    }

    #[test]
    fn music_in_two_bands_is_raised_by_the_boost_in_proportion_to_what_it_has_there() {
        let mut levels = vec![FLOOR_DB; 2_049];
        levels[10] = -6.0;
        levels[500] = -6.0;
        let both = BandLevels::of(&Spectrum::new(BIN_HZ, levels, Duration::from_secs(10)));
        let reach = BandGains::of(&boosting(100.0, 6.0), SampleRate::HZ_44100);

        let raised = reach.peak(&both, 1.0);

        let alone = 10_f64.powf(6.0 / 20.0);
        assert!(raised > 1.0 && raised < alone, "{raised}");
    }

    #[test]
    fn a_cut_lowers_what_the_music_is_thought_to_reach_and_a_flat_curve_leaves_it() {
        let levels = BandLevels::of(&spectrum_sounding_at(100.0, -6.0));
        let flat = BandGains::of(&Arc::new(Profile::flat()), SampleRate::HZ_44100);
        let cutting = BandGains::of(&boosting(100.0, -6.0), SampleRate::HZ_44100);

        assert!((flat.peak(&levels, 0.8) - 0.8).abs() < 1e-6);
        assert!(cutting.peak(&levels, 0.8) < 0.8 * 0.9);
    }
}
