use std::{
    f64::consts::TAU,
    iter,
    ops::{Add, Mul, Sub},
    time::Duration,
};

use resonate_core::{
    Frames, SampleRate,
    eq::{RESPONSE_FROM_HZ, RESPONSE_TO_HZ},
};

pub(crate) const FLOOR_DB: f32 = -78.0;
pub(crate) const CEILING_DB: f32 = 0.0;
pub(crate) const MARKED_EVERY_DB: f32 = 12.0;
const QUIETEST_DB: f32 = -160.0;
const ANALYSED_FOR: Duration = Duration::from_millis(85);
const FEWEST_POINTS: usize = 8;
const MOST_POINTS: usize = 32_768;
const BANDS_PER_OCTAVE: f64 = 6.0;
const PIVOT_HZ: f64 = 1_000.0;
const TILT_DB_PER_OCTAVE: f32 = 3.0;
const FALLS_DB_PER_SECOND: f32 = 40.0;
const PEAK_HELD_FOR: Duration = Duration::from_millis(700);
const PEAK_FALLS_DB_PER_SECOND: f32 = 20.0;
const LONGEST_STEP: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };
    const HALF_BACK_A_QUARTER_TURN: Self = Self { re: 0.0, im: -0.5 };

    #[expect(clippy::cast_possible_truncation, reason = "a unit phasor fits an f32")]
    fn turned_back(turns: f64) -> Self {
        let angle = -TAU * turns;
        Self {
            re: angle.cos() as f32,
            im: angle.sin() as f32,
        }
    }

    const fn conjugate(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }

    fn power(self) -> f32 {
        self.re.mul_add(self.re, self.im * self.im)
    }
}

impl Add for Complex {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }
}

impl Sub for Complex {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
}

impl Mul for Complex {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re.mul_add(other.re, -(self.im * other.im)),
            im: self.re.mul_add(other.im, self.im * other.re),
        }
    }
}

impl Mul<f32> for Complex {
    type Output = Self;

    fn mul(self, scale: f32) -> Self {
        Self {
            re: self.re * scale,
            im: self.im * scale,
        }
    }
}

pub(crate) struct Fft {
    points: usize,
    reversed: Box<[usize]>,
    turns: Box<[Complex]>,
    unzipped: Box<[Complex]>,
    window: Box<[f32]>,
    gain: f32,
    windowed: Vec<f32>,
    work: Vec<Complex>,
    bins: Vec<Complex>,
}

impl Fft {
    pub(crate) fn new(points: usize) -> Self {
        let points = points.clamp(FEWEST_POINTS, MOST_POINTS).next_power_of_two();
        let half = points / 2;
        let bits = half.trailing_zeros();
        let window: Box<[f32]> = (0..points).map(|at| hann(at, points)).collect();
        let gain = 2.0 / window.iter().sum::<f32>();

        Self {
            points,
            reversed: (0..half)
                .map(|at| at.reverse_bits() >> (usize::BITS - bits))
                .collect(),
            turns: (0..half / 2)
                .map(|at| Complex::turned_back(at as f64 / half as f64))
                .collect(),
            unzipped: (0..=half)
                .map(|at| Complex::turned_back(at as f64 / points as f64))
                .collect(),
            window,
            gain,
            windowed: Vec::with_capacity(points),
            work: vec![Complex::ZERO; half],
            bins: Vec::with_capacity(half + 1),
        }
    }

    pub(crate) const fn points(&self) -> usize {
        self.points
    }

    pub(crate) fn levels(&mut self, samples: &[f32], decibels: &mut Vec<f32>) {
        self.windowed.clear();
        self.windowed.extend(
            self.window
                .iter()
                .zip(samples.iter().chain(iter::repeat(&0.0)))
                .map(|(weight, sample)| weight * sample),
        );
        self.transform();

        let gain = self.gain * self.gain;
        decibels.clear();
        decibels.extend(
            self.bins
                .iter()
                .map(|bin| decibels_of_power(bin.power() * gain)),
        );
    }

    fn transform(&mut self) {
        let half = self.points / 2;
        for (at, reversed) in self.reversed.iter().enumerate() {
            let pair = Complex {
                re: self.windowed.get(2 * at).copied().unwrap_or(0.0),
                im: self.windowed.get(2 * at + 1).copied().unwrap_or(0.0),
            };
            if let Some(slot) = self.work.get_mut(*reversed) {
                *slot = pair;
            }
        }

        let mut span = 1;
        while span < half {
            let stride = half / (span * 2);
            for block in self.work.chunks_exact_mut(span * 2) {
                let (lower, upper) = block.split_at_mut(span);
                let turns = self.turns.iter().step_by(stride);
                for ((low, high), turn) in lower.iter_mut().zip(upper.iter_mut()).zip(turns) {
                    let turned = *high * *turn;
                    *high = *low - turned;
                    *low = *low + turned;
                }
            }
            span *= 2;
        }

        self.bins.clear();
        for (at, unzipped) in self.unzipped.iter().enumerate() {
            let ahead = self.work.get(at % half).copied().unwrap_or_default();
            let mirrored = self
                .work
                .get((half - at % half) % half)
                .copied()
                .unwrap_or_default()
                .conjugate();
            let even = (ahead + mirrored) * 0.5;
            let odd = (ahead - mirrored) * Complex::HALF_BACK_A_QUARTER_TURN;
            self.bins.push(even + *unzipped * odd);
        }
    }
}

fn hann(at: usize, points: usize) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a window weight fits an f32"
    )]
    let weight = (0.5 - 0.5 * (TAU * at as f64 / points as f64).cos()) as f32;
    weight
}

fn decibels_of_power(power: f32) -> f32 {
    (10.0 * power.log10()).max(QUIETEST_DB)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Reach {
    Bins { first: usize, last: usize },
    Between { below: usize, towards: f32 },
    Beyond,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    low: f64,
    centre: f64,
    high: f64,
    reach: Reach,
    tilt: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bar {
    level: f32,
    peak: f32,
    held: Duration,
}

impl Bar {
    const RESTING: Self = Self {
        level: FLOOR_DB,
        peak: FLOOR_DB,
        held: Duration::ZERO,
    };

    fn fold(&mut self, reading: f32, step: Duration) {
        let seconds = step.as_secs_f32();
        self.level = if reading >= self.level {
            reading
        } else {
            FALLS_DB_PER_SECOND
                .mul_add(-seconds, self.level)
                .max(reading)
        };

        if self.level >= self.peak {
            self.peak = self.level;
            self.held = Duration::ZERO;
            return;
        }
        self.held = self.held.saturating_add(step);
        if self.held > PEAK_HELD_FOR {
            self.peak = PEAK_FALLS_DB_PER_SECOND
                .mul_add(-seconds, self.peak)
                .max(self.level);
        }
    }

    fn is_at_rest(self) -> bool {
        self.level <= FLOOR_DB && self.peak <= FLOOR_DB
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Column {
    pub(crate) low_hz: f64,
    pub(crate) high_hz: f64,
    pub(crate) level: f32,
    pub(crate) peak: f32,
}

pub(crate) struct Spectrum {
    rate: SampleRate,
    fft: Fft,
    bands: Box<[Band]>,
    bins: Vec<f32>,
    bars: Vec<Bar>,
}

impl Spectrum {
    pub(crate) fn new(rate: SampleRate) -> Self {
        let wanted =
            usize::try_from(Frames::from_duration(ANALYSED_FOR, rate).get()).unwrap_or(MOST_POINTS);
        let fft = Fft::new(wanted);
        let bands = bands_for(rate, fft.points());

        Self {
            rate,
            bins: Vec::with_capacity(fft.points() / 2 + 1),
            bars: vec![Bar::RESTING; bands.len()],
            fft,
            bands,
        }
    }

    pub(crate) const fn rate(&self) -> SampleRate {
        self.rate
    }

    pub(crate) const fn points(&self) -> usize {
        self.fft.points()
    }

    pub(crate) fn take(&mut self, samples: &[f32], step: Duration) {
        self.fft.levels(samples, &mut self.bins);
        let step = step.min(LONGEST_STEP);
        for (bar, band) in self.bars.iter_mut().zip(self.bands.iter()) {
            bar.fold(reading(&self.bins, *band), step);
        }
    }

    pub(crate) fn is_at_rest(&self) -> bool {
        self.bars.iter().all(|bar| bar.is_at_rest())
    }

    pub(crate) fn columns(&self) -> impl Iterator<Item = Column> + '_ {
        self.bands
            .iter()
            .zip(self.bars.iter())
            .map(|(band, bar)| Column {
                low_hz: band.low,
                high_hz: band.high,
                level: height_of(bar.level),
                peak: height_of(bar.peak),
            })
    }
}

pub(crate) fn height_of(decibels: f32) -> f32 {
    ((decibels - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

fn reading(bins: &[f32], band: Band) -> f32 {
    let raw = match band.reach {
        Reach::Bins { first, last } => bins.get(first..=last).map_or(QUIETEST_DB, |run| {
            run.iter().copied().fold(QUIETEST_DB, f32::max)
        }),
        Reach::Between { below, towards } => {
            let under = bins.get(below).copied().unwrap_or(QUIETEST_DB);
            let over = bins.get(below + 1).copied().unwrap_or(under);
            (over - under).mul_add(towards, under)
        }
        Reach::Beyond => return FLOOR_DB,
    };

    (raw + band.tilt).clamp(FLOOR_DB, CEILING_DB)
}

fn bands_for(rate: SampleRate, points: usize) -> Box<[Band]> {
    let bin_hz = f64::from(rate.hz()) / points as f64;
    let nyquist = f64::from(rate.hz()) / 2.0;
    let last_bin = points / 2;
    let half_a_band = 2.0_f64.powf(0.5 / BANDS_PER_OCTAVE);

    let steps = |hertz: f64| (hertz / PIVOT_HZ).log2() * BANDS_PER_OCTAVE;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a band count across the audible range fits an i32"
    )]
    let first = steps(RESPONSE_FROM_HZ * half_a_band).ceil() as i32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a band count across the audible range fits an i32"
    )]
    let last = steps(RESPONSE_TO_HZ / half_a_band).floor() as i32;

    (first..=last)
        .map(|step| {
            let centre = PIVOT_HZ * 2.0_f64.powf(f64::from(step) / BANDS_PER_OCTAVE);
            let low = centre / half_a_band;
            let high = centre * half_a_band;

            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a bin index below the Nyquist bin fits a usize"
            )]
            let reach = if low >= nyquist {
                Reach::Beyond
            } else {
                let first = (low / bin_hz).ceil() as usize;
                let last = ((high.min(nyquist) / bin_hz).floor() as usize).min(last_bin);
                if first <= last {
                    Reach::Bins { first, last }
                } else {
                    let at = centre / bin_hz;
                    Reach::Between {
                        below: (at.floor() as usize).min(last_bin.saturating_sub(1)),
                        towards: at.fract() as f32,
                    }
                }
            };

            #[expect(
                clippy::cast_possible_truncation,
                reason = "a tilt in decibels fits an f32"
            )]
            let tilt = TILT_DB_PER_OCTAVE * (centre / PIVOT_HZ).log2() as f32;

            Band {
                low,
                centre,
                high,
                reach,
                tilt,
            }
        })
        .collect()
}

pub(crate) fn rising_edge(levels: &[f32], within: usize) -> usize {
    levels
        .windows(2)
        .take(within)
        .position(|pair| matches!(pair, [before, after] if *before < 0.0 && *after >= 0.0))
        .map_or(0, |at| at + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = SampleRate::HZ_48000;

    fn tone(points: usize, cycles: f64, amplitude: f32) -> Vec<f32> {
        (0..points)
            .map(|at| amplitude * (TAU * cycles * at as f64 / points as f64).sin() as f32)
            .collect()
    }

    fn tone_at(rate: SampleRate, points: usize, hertz: f64, amplitude: f32) -> Vec<f32> {
        tone(
            points,
            hertz * points as f64 / f64::from(rate.hz()),
            amplitude,
        )
    }

    fn noise(points: usize) -> Vec<f32> {
        let mut state = 0x2545_f491_u32;
        (0..points)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as f32 / u32::MAX as f32 * 2.0 - 1.0
            })
            .collect()
    }

    fn loudest(decibels: &[f32]) -> usize {
        decibels
            .iter()
            .enumerate()
            .fold((0, f32::MIN), |(held, most), (at, level)| {
                if *level > most {
                    (at, *level)
                } else {
                    (held, most)
                }
            })
            .0
    }

    fn band_level(spectrum: &Spectrum, hertz: f64) -> f32 {
        spectrum
            .bands
            .iter()
            .zip(spectrum.bars.iter())
            .find(|(band, _)| band.low <= hertz && hertz < band.high)
            .map(|(_, bar)| bar.level)
            .expect("a band holding the frequency")
    }

    #[test]
    fn the_transform_is_the_discrete_fourier_transform_of_what_it_is_handed() {
        for points in [8, 64, 1_024] {
            let mut fft = Fft::new(points);
            let input = noise(points);
            fft.windowed = input.clone();
            fft.transform();

            for (at, bin) in fft.bins.iter().enumerate() {
                let (re, im) =
                    input
                        .iter()
                        .enumerate()
                        .fold((0.0_f64, 0.0_f64), |(re, im), (n, sample)| {
                            let angle = -TAU * (at * n) as f64 / points as f64;
                            (
                                re + f64::from(*sample) * angle.cos(),
                                im + f64::from(*sample) * angle.sin(),
                            )
                        });
                let tolerance = 1e-4 * points as f64;
                assert!(
                    (f64::from(bin.re) - re).abs() < tolerance
                        && (f64::from(bin.im) - im).abs() < tolerance,
                    "bin {at} of {points} read {bin:?} where the sum reads {re} {im}"
                );
            }
        }
    }

    #[test]
    fn a_full_scale_sine_on_a_bin_reads_full_scale_in_that_bin() {
        let mut fft = Fft::new(4_096);
        let mut decibels = Vec::new();
        fft.levels(&tone(4_096, 100.0, 1.0), &mut decibels);

        assert_eq!(loudest(&decibels), 100);
        assert!(decibels[100].abs() < 0.01, "read {} dB", decibels[100]);
        assert_eq!(decibels.len(), 2_049);
    }

    #[test]
    fn a_sine_between_two_bins_lands_on_the_nearer_one_within_the_windows_scalloping() {
        let mut fft = Fft::new(4_096);
        let mut decibels = Vec::new();
        fft.levels(&tone(4_096, 200.3, 0.5), &mut decibels);

        assert_eq!(loudest(&decibels), 200);
        let half_scale = 20.0 * 0.5_f32.log10();
        assert!(
            (decibels[200] - half_scale).abs() < 1.5,
            "read {} dB against {half_scale}",
            decibels[200]
        );
    }

    #[test]
    fn the_window_keeps_a_tone_from_leaking_across_the_spectrum() {
        let mut fft = Fft::new(4_096);
        let mut decibels = Vec::new();
        fft.levels(&tone(4_096, 100.5, 1.0), &mut decibels);

        let unwindowed_leak = 20.0 * (1.0 / (std::f32::consts::PI * 20.0)).log10();
        assert!(
            decibels[120] < -70.0 && decibels[80] < -70.0,
            "twenty bins away read {} and {} dB, where no window leaks {unwindowed_leak} dB",
            decibels[80],
            decibels[120]
        );
    }

    #[test]
    fn silence_reads_as_the_quietest_level_in_every_bin() {
        let mut fft = Fft::new(1_024);
        let mut decibels = Vec::new();
        fft.levels(&[0.0; 1_024], &mut decibels);

        assert!(decibels.iter().all(|level| *level == QUIETEST_DB));
    }

    #[test]
    fn a_short_read_is_padded_with_silence_and_a_size_is_a_power_of_two() {
        let mut fft = Fft::new(3_000);
        assert_eq!(fft.points(), 4_096);
        assert_eq!(Fft::new(1).points(), FEWEST_POINTS);
        assert_eq!(Fft::new(usize::MAX).points(), MOST_POINTS);

        let mut decibels = Vec::new();
        fft.levels(&[], &mut decibels);
        assert!(decibels.iter().all(|level| *level == QUIETEST_DB));
    }

    #[test]
    fn the_bands_are_sixth_octaves_across_the_audible_range_meeting_at_their_edges() {
        let bands = bands_for(RATE, 4_096);

        assert_eq!(bands.len(), 59);
        assert!(
            bands
                .first()
                .is_some_and(|band| band.low >= RESPONSE_FROM_HZ)
        );
        assert!(bands.last().is_some_and(|band| band.high <= RESPONSE_TO_HZ));
        assert!(
            bands
                .iter()
                .any(|band| (band.centre - PIVOT_HZ).abs() < 1e-9)
        );
        for pair in bands.windows(2) {
            assert!((pair[0].high - pair[1].low).abs() < 1e-9);
        }
    }

    #[test]
    fn a_tone_lights_its_own_band_and_leaves_one_two_octaves_away_at_the_floor() {
        let mut spectrum = Spectrum::new(RATE);
        let points = spectrum.points();
        spectrum.take(&tone_at(RATE, points, 1_000.0, 1.0), Duration::ZERO);

        let lit = band_level(&spectrum, 1_000.0);
        assert!(
            lit > -2.0 && lit <= CEILING_DB,
            "the tone's band read {lit} dB"
        );
        assert_eq!(band_level(&spectrum, 4_000.0), FLOOR_DB);
        assert_eq!(band_level(&spectrum, 250.0), FLOOR_DB);
    }

    #[test]
    fn the_treble_is_tilted_up_three_decibels_an_octave_about_the_pivot() {
        let rate = SampleRate::HZ_44100;
        let mut spectrum = Spectrum::new(rate);
        let points = spectrum.points();
        let near_the_pivot = 93.0;
        let tones: Vec<f32> = tone(points, near_the_pivot, 0.05)
            .into_iter()
            .zip(tone(points, near_the_pivot * 2.0, 0.05))
            .map(|(one, other)| one + other)
            .collect();
        spectrum.take(&tones, Duration::ZERO);

        let bin_hz = f64::from(rate.hz()) / points as f64;
        let tone_level = 20.0 * 0.05_f32.log10();
        let at_the_pivot = band_level(&spectrum, near_the_pivot * bin_hz);
        let an_octave_up = band_level(&spectrum, 2.0 * near_the_pivot * bin_hz);
        assert!((at_the_pivot - tone_level).abs() < 0.05, "{at_the_pivot}");
        assert!(
            (an_octave_up - tone_level - TILT_DB_PER_OCTAVE).abs() < 0.05,
            "{an_octave_up}"
        );
    }

    #[test]
    fn a_band_above_what_the_rate_can_carry_stays_at_the_floor() {
        let rate = SampleRate::HZ_22050;
        let mut spectrum = Spectrum::new(rate);
        let points = spectrum.points();
        spectrum.take(&noise(points), Duration::ZERO);

        assert!(band_level(&spectrum, 5_000.0) > FLOOR_DB);
        assert_eq!(band_level(&spectrum, 15_000.0), FLOOR_DB);
    }

    #[test]
    fn silence_leaves_every_bar_resting_on_the_floor() {
        let mut spectrum = Spectrum::new(RATE);
        spectrum.take(&[], Duration::from_millis(16));

        assert!(spectrum.is_at_rest());
        assert!(
            spectrum
                .columns()
                .all(|column| column.level == 0.0 && column.peak == 0.0)
        );
    }

    #[test]
    fn a_bar_falls_at_its_rate_while_its_peak_holds_and_then_follows() {
        let mut bar = Bar::RESTING;
        bar.fold(-20.0, Duration::ZERO);
        assert_eq!((bar.level, bar.peak), (-20.0, -20.0));

        bar.fold(FLOOR_DB, Duration::from_millis(100));
        assert!((bar.level - (-24.0)).abs() < 1e-4, "{}", bar.level);
        assert_eq!(bar.peak, -20.0);

        for _ in 0..6 {
            bar.fold(FLOOR_DB, Duration::from_millis(100));
        }
        assert_eq!(
            bar.peak, -20.0,
            "the peak let go before it was held its time"
        );

        bar.fold(FLOOR_DB, Duration::from_millis(100));
        assert!((bar.peak - (-22.0)).abs() < 1e-4, "{}", bar.peak);

        for _ in 0..100 {
            bar.fold(FLOOR_DB, Duration::from_millis(100));
        }
        assert!(bar.is_at_rest());
    }

    #[test]
    fn a_louder_reading_lifts_a_bar_at_once() {
        let mut bar = Bar::RESTING;
        bar.fold(-40.0, Duration::from_millis(16));
        bar.fold(-6.0, Duration::from_millis(16));

        assert_eq!((bar.level, bar.peak), (-6.0, -6.0));
    }

    #[test]
    fn a_trace_starts_on_the_first_rising_zero_crossing_it_can_hold_a_whole_span_after() {
        let wave: Vec<f32> = (30..430)
            .map(|at| (TAU * (f64::from(at) + 0.5) / 100.0).sin() as f32)
            .collect();

        let at = rising_edge(&wave, 200);
        assert_eq!(at, 70);
        assert!(wave[at] > 0.0 && wave[at - 1] < 0.0);
        assert_eq!(
            rising_edge(&wave, 60),
            0,
            "a crossing past the reach was taken"
        );
        assert_eq!(rising_edge(&[0.5; 64], 32), 0);
    }
}
