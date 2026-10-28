use std::sync::Arc;

use resonate_codec::Sources;
use resonate_core::{
    FrameSpan, MediaLocation, SampleRate, StreamSpec,
    eq::{Frequency, Profile, RESPONSE_FROM_HZ},
};

use crate::{
    Error, Result, Spectrum, Watching,
    analysis::{native_format, opened},
    conform::Walking,
    spectrum::{FLOOR_DB, Transforming},
    split::Splitting,
};

pub const BANDS: usize = 29;

const LOWEST_EDGE_HZ: f64 = 25.0;

const BANDS_AN_OCTAVE: f64 = 3.0;

const STEPS_A_DECIBEL: f64 = 2.0;

const SILENT: u8 = u8::MAX;

pub const BAND_PEAKS_BYTES: usize = BANDS * 2;

const PEAK_STEPS_A_DECIBEL: f64 = 16.0;

const PEAK_CODE_ZERO_DB: f64 = 200.0;

const SILENT_PEAK: u16 = 0;

const SKIRT_BANDS: f64 = 2.0;

const LOWEST_REACH_HZ: f64 = Frequency::LOWEST_CENTIHERTZ as f64 / 100.0;

const LEVELS_ALIKE_WITHIN: f64 = 1e-4;

fn band_of(hertz: f64) -> usize {
    if hertz <= LOWEST_EDGE_HZ {
        return 0;
    }
    let band = ((hertz / LOWEST_EDGE_HZ).log2() * BANDS_AN_OCTAVE).floor();
    (band as usize).min(BANDS - 1)
}

pub(crate) fn edge_of(band: usize) -> f64 {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandPeaks {
    codes: [u16; BANDS],
}

impl BandPeaks {
    pub const SILENT: Self = Self {
        codes: [SILENT_PEAK; BANDS],
    };

    pub fn of(peaks: impl IntoIterator<Item = f64>) -> Self {
        let mut codes = [SILENT_PEAK; BANDS];
        for (code, peak) in codes.iter_mut().zip(peaks) {
            *code = code_above(peak);
        }
        Self { codes }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (pairs, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() || pairs.len() != BANDS {
            return None;
        }
        Some(Self {
            codes: std::array::from_fn(|band| u16::from_le_bytes(pairs[band])),
        })
    }

    pub fn to_bytes(&self) -> [u8; BAND_PEAKS_BYTES] {
        let mut bytes = [0; BAND_PEAKS_BYTES];
        for (slot, code) in bytes.as_chunks_mut::<2>().0.iter_mut().zip(self.codes) {
            *slot = code.to_le_bytes();
        }
        bytes
    }

    pub fn is_silent(&self) -> bool {
        self.codes.iter().all(|code| *code == SILENT_PEAK)
    }

    pub(crate) fn amplitudes(&self) -> [f64; BANDS] {
        self.codes.map(amplitude_of)
    }
}

fn code_above(peak: f64) -> u16 {
    if !peak.is_finite() || peak <= 0.0 {
        return SILENT_PEAK;
    }
    let steps = ((20.0 * peak.log10() + PEAK_CODE_ZERO_DB) * PEAK_STEPS_A_DECIBEL).ceil();
    steps.clamp(1.0, f64::from(u16::MAX)) as u16
}

pub(crate) fn amplitude_of(code: u16) -> f64 {
    if code == SILENT_PEAK {
        return 0.0;
    }
    10_f64.powf((f64::from(code) / PEAK_STEPS_A_DECIBEL - PEAK_CODE_ZERO_DB) / 20.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandFigures {
    pub levels: BandLevels,
    pub peaks: BandPeaks,
}

#[derive(Clone, Debug, PartialEq)]
struct Anchor {
    level: f64,
    reach: [f64; BANDS],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandGains {
    anchors: Vec<Anchor>,
}

impl BandGains {
    pub fn of(profile: &Arc<Profile>, rate: SampleRate) -> Self {
        let at_the_rate = profile.at_rate(rate);

        let mut levels = vec![0.0, 1.0];
        levels.extend((0..BANDS).map(|band| {
            let (from, to) = span_of_band(band);
            let decibels = at_the_rate.peak_db_between(from, to, rate);
            if decibels.is_finite() {
                10_f64.powf(decibels / 20.0)
            } else {
                1.0
            }
        }));
        levels.sort_by(f64::total_cmp);
        levels.dedup_by(|later, earlier| (*later - *earlier).abs() < LEVELS_ALIKE_WITHIN);

        let mut anchors: Vec<Anchor> = levels
            .iter()
            .map(|level| Anchor {
                level: *level,
                reach: [0.0; BANDS],
            })
            .collect();
        for band in 0..BANDS {
            let (from, to) = reach_of_band(band);
            let reaches = at_the_rate.farthest_from_between(&levels, from, to, rate);
            for (anchor, reach) in anchors.iter_mut().zip(reaches) {
                anchor.reach[band] = reach;
            }
        }
        Self { anchors }
    }

    pub fn bound(&self, peaks: &BandPeaks, true_peak: f32) -> f64 {
        if peaks.is_silent() || !true_peak.is_finite() {
            return 0.0;
        }
        let whole = f64::from(true_peak.max(0.0));
        let amplitudes = peaks.amplitudes();
        self.anchors
            .iter()
            .map(|anchor| {
                let apart: f64 = anchor
                    .reach
                    .iter()
                    .zip(amplitudes)
                    .map(|(reach, amplitude)| reach * amplitude)
                    .sum();
                anchor.level.abs() * whole + apart
            })
            .fold(f64::INFINITY, f64::min)
    }
}

fn reach_of_band(band: usize) -> (f64, f64) {
    let skirt = 2.0_f64.powf(SKIRT_BANDS / BANDS_AN_OCTAVE);
    let from = if band == 0 {
        LOWEST_REACH_HZ
    } else {
        edge_of(band) / skirt
    };
    let to = if band + 1 >= BANDS {
        f64::INFINITY
    } else {
        edge_of(band + 1) * skirt
    };
    (from, to)
}

pub fn band_figures(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    watch: &dyn Watching,
) -> Result<BandFigures> {
    let (mut decoder, info) = opened(sources, location, span)?;
    let codec = resonate_codec::Codec::from_id(info.codec);
    let format = native_format(&info, codec);
    decoder.set_output_format(format);
    decoder.follow_format_changes();

    let rate = info.spec.rate;
    let placements = info.speakers.placements(info.spec.channels.count().get());
    let mut transforming = Transforming::new(rate.hz(), &placements);
    let mut splitting = Splitting::new(rate, usize::from(info.spec.channels.count().get()));
    let mut walking = Walking::new(StreamSpec::new(rate, info.spec.channels, format));
    loop {
        if watch.stopped() {
            return Err(Error::Stopped);
        }
        let Some(heard) = walking.step(&mut decoder)? else {
            break;
        };
        transforming.note(heard.samples, |_| {});
        splitting.note(heard.samples);
    }

    Ok(BandFigures {
        levels: BandLevels::of(&transforming.finished()),
        peaks: splitting.finished(),
    })
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

    fn bell(hertz: f64, decibels: f64, q: f64) -> Arc<Profile> {
        let band = Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(hertz).expect("in range"),
            BandGain::from_decibels(decibels).expect("in range"),
            Q::from_units(q).expect("in range"),
        );
        Arc::new(Profile::new(Preamp::NONE, vec![band]).expect("a profile in range"))
    }

    fn boosting(hertz: f64, decibels: f64) -> Arc<Profile> {
        bell(hertz, decibels, 2.0)
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

        let steps = levels.as_bytes();

        assert_eq!(steps[sounding], 0);
        assert!(
            steps
                .iter()
                .enumerate()
                .all(|(band, step)| band == sounding || *step == SILENT)
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
            BandGains::of(&boosting(100.0, 6.0), SampleRate::HZ_44100)
                .bound(&BandPeaks::SILENT, 0.9),
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

    fn peaks_in(sounding: &[(f64, f64)]) -> BandPeaks {
        let mut peaks = [0.0; BANDS];
        for (hertz, peak) in sounding {
            peaks[band_of(*hertz)] = *peak;
        }
        BandPeaks::of(peaks)
    }

    fn decibels_between(bound: f64, peak: f64) -> f64 {
        20.0 * (bound / peak).log10()
    }

    #[test]
    fn peaks_read_back_from_their_bytes_and_a_wrong_length_reads_as_nothing() {
        let peaks = peaks_in(&[(110.0, 0.8), (3_000.0, 0.05)]);

        assert_eq!(BandPeaks::from_bytes(&peaks.to_bytes()), Some(peaks));
        assert_eq!(BandPeaks::from_bytes(&peaks.to_bytes()[1..]), None);
        assert_eq!(BandPeaks::from_bytes(&[]), None);
        assert!(BandPeaks::SILENT.is_silent());
        assert!(!peaks.is_silent());
    }

    #[test]
    fn a_peak_is_kept_at_or_above_what_it_was_by_less_than_a_tenth_of_a_decibel() {
        for peak in [1e-9, 0.003, 0.25, 0.8, 1.0, 3.7, 1_000.0] {
            let kept = amplitude_of(code_above(peak));

            assert!(kept >= peak, "{kept} under {peak}");
            assert!(decibels_between(kept, peak) < 0.1, "{kept} over {peak}");
        }
        assert_eq!(code_above(0.0), SILENT_PEAK);
        assert_eq!(code_above(f64::NAN), SILENT_PEAK);
        assert_eq!(amplitude_of(SILENT_PEAK), 0.0);
    }

    #[test]
    fn music_peaking_on_a_boost_is_raised_by_it_and_music_far_from_it_is_not() {
        let reach = BandGains::of(&boosting(110.0, 6.0), SampleRate::HZ_44100);
        let on_the_boost = peaks_in(&[(110.0, 0.5)]);
        let far_from_it = peaks_in(&[(5_000.0, 0.5)]);

        let raised = reach.bound(&on_the_boost, 0.5);
        let left = reach.bound(&far_from_it, 0.5);

        assert!(
            (decibels_between(raised, 0.5) - 6.0).abs() < 0.3,
            "{raised}"
        );
        assert!((left - 0.5).abs() < 0.01, "{left}");
    }

    #[test]
    fn a_song_peaking_in_two_bands_is_bounded_between_the_flat_peak_and_the_boost_on_all_of_it() {
        let both = peaks_in(&[(110.0, 0.4), (5_000.0, 0.4)]);
        let reach = BandGains::of(&boosting(110.0, 6.0), SampleRate::HZ_44100);

        let bound = reach.bound(&both, 0.5);

        let all_boosted = 0.5 * 10_f64.powf(6.0 / 20.0);
        assert!(bound > 0.5 && bound < all_boosted + 0.01, "{bound}");
    }

    #[test]
    fn a_cut_lowers_the_bound_and_a_flat_curve_leaves_the_peak_as_it_was() {
        let peaks = peaks_in(&[(110.0, 0.8)]);
        let flat = BandGains::of(&Arc::new(Profile::flat()), SampleRate::HZ_44100);
        let cutting = BandGains::of(&bell(110.0, -6.0, 0.2), SampleRate::HZ_44100);

        assert!((flat.bound(&peaks, 0.8) - 0.8).abs() < 0.8 * 0.01);
        assert!(cutting.bound(&peaks, 0.8) < 0.8 * 0.7);
    }

    #[test]
    fn peaks_gathered_in_one_boosted_band_are_bounded_by_the_boost_though_the_rest_is_loud() {
        let reach = BandGains::of(&boosting(110.0, 6.0), SampleRate::HZ_44100);
        let gathered = peaks_in(&[(110.0, 0.7), (4_000.0, 0.2), (9_000.0, 0.2)]);

        let bound = reach.bound(&gathered, 0.7);

        let boosted_peak = 0.7 * 10_f64.powf(6.0 / 20.0);
        assert!(bound >= boosted_peak, "{bound} under {boosted_peak}");
    }
}
