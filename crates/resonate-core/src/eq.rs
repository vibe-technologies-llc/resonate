use std::fmt;

use crate::{Error, Result, SampleRate};

pub const MAX_BANDS: usize = 32;

const CENTIHERTZ_PER_HERTZ: f64 = 100.0;
const MILLIBELS_PER_DECIBEL: f64 = 1_000.0;
const MILLI_PER_UNIT: f64 = 1_000.0;

pub const RESPONSE_FROM_HZ: f64 = 20.0;
pub const RESPONSE_TO_HZ: f64 = 20_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frequency(u32);

impl Frequency {
    pub const LOWEST_CENTIHERTZ: u32 = 100;
    pub const HIGHEST_CENTIHERTZ: u32 = 4_000_000;

    pub const LOWEST: Self = Self(Self::LOWEST_CENTIHERTZ);
    pub const HIGHEST: Self = Self(Self::HIGHEST_CENTIHERTZ);

    pub const fn from_centihertz(centihertz: u32) -> Result<Self> {
        if centihertz < Self::LOWEST_CENTIHERTZ || centihertz > Self::HIGHEST_CENTIHERTZ {
            return Err(Error::BandFrequencyOutOfRange(centihertz));
        }
        Ok(Self(centihertz))
    }

    pub fn from_hertz(hertz: f64) -> Result<Self> {
        if !hertz.is_finite() || hertz < 0.0 {
            return Err(Error::BandFrequencyOutOfRange(0));
        }
        let centihertz = (hertz * CENTIHERTZ_PER_HERTZ).round();
        if centihertz > f64::from(Self::HIGHEST_CENTIHERTZ) {
            return Err(Error::BandFrequencyOutOfRange(Self::HIGHEST_CENTIHERTZ + 1));
        }
        Self::from_centihertz(centihertz as u32)
    }

    pub const fn centihertz(self) -> u32 {
        self.0
    }

    pub fn hertz(self) -> f64 {
        f64::from(self.0) / CENTIHERTZ_PER_HERTZ
    }

    pub fn is_under_nyquist(self, rate: SampleRate) -> bool {
        self.hertz() * 2.0 < f64::from(rate.hz())
    }

    fn angle_at(self, rate: SampleRate) -> f64 {
        (std::f64::consts::TAU * self.hertz() / f64::from(rate.hz())).min(std::f64::consts::PI)
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_multiple_of(100) {
            write!(f, "{} Hz", self.0 / 100)
        } else {
            write!(f, "{:.2} Hz", self.hertz())
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BandGain(i32);

impl BandGain {
    pub const FLAT: Self = Self(0);
    pub const WIDEST_MILLIBELS: i32 = 40_000;

    pub const fn from_millibels(millibels: i32) -> Result<Self> {
        if millibels < -Self::WIDEST_MILLIBELS || millibels > Self::WIDEST_MILLIBELS {
            return Err(Error::BandGainOutOfRange(millibels));
        }
        Ok(Self(millibels))
    }

    pub fn from_decibels(decibels: f64) -> Result<Self> {
        if !decibels.is_finite() {
            return Err(Error::BandGainOutOfRange(Self::WIDEST_MILLIBELS + 1));
        }
        let millibels = (decibels * MILLIBELS_PER_DECIBEL).round();
        if millibels.abs() > f64::from(Self::WIDEST_MILLIBELS) {
            return Err(Error::BandGainOutOfRange(Self::WIDEST_MILLIBELS + 1));
        }
        Self::from_millibels(millibels as i32)
    }

    pub const fn millibels(self) -> i32 {
        self.0
    }

    pub fn decibels(self) -> f64 {
        f64::from(self.0) / MILLIBELS_PER_DECIBEL
    }

    pub const fn is_flat(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for BandGain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+.1} dB", self.decibels())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Q(u32);

impl Q {
    pub const NARROWEST_MILLI: u32 = 40_000;
    pub const WIDEST_MILLI: u32 = 100;

    pub const BUTTERWORTH: Self = Self(707);
    pub const THIRD_OCTAVE: Self = Self(4_318);

    pub const fn from_milli(milli: u32) -> Result<Self> {
        if milli < Self::WIDEST_MILLI || milli > Self::NARROWEST_MILLI {
            return Err(Error::BandQOutOfRange(milli));
        }
        Ok(Self(milli))
    }

    pub fn from_units(units: f64) -> Result<Self> {
        if !units.is_finite() || units < 0.0 {
            return Err(Error::BandQOutOfRange(0));
        }
        let milli = (units * MILLI_PER_UNIT).round();
        if milli > f64::from(Self::NARROWEST_MILLI) {
            return Err(Error::BandQOutOfRange(Self::NARROWEST_MILLI + 1));
        }
        Self::from_milli(milli as u32)
    }

    pub const fn milli(self) -> u32 {
        self.0
    }

    pub fn units(self) -> f64 {
        f64::from(self.0) / MILLI_PER_UNIT
    }
}

impl Default for Q {
    fn default() -> Self {
        Self::BUTTERWORTH
    }
}

impl fmt::Display for Q {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}", self.units())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Preamp(i32);

impl Preamp {
    pub const NONE: Self = Self(0);

    pub const fn from_millibels(millibels: i32) -> Result<Self> {
        if millibels < -BandGain::WIDEST_MILLIBELS || millibels > BandGain::WIDEST_MILLIBELS {
            return Err(Error::PreampOutOfRange(millibels));
        }
        Ok(Self(millibels))
    }

    pub fn from_decibels(decibels: f64) -> Result<Self> {
        Self::from_millibels(BandGain::from_decibels(decibels)?.millibels())
    }

    pub const fn millibels(self) -> i32 {
        self.0
    }

    pub fn decibels(self) -> f64 {
        f64::from(self.0) / MILLIBELS_PER_DECIBEL
    }

    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    pub fn amplitude(self) -> f64 {
        10.0_f64.powf(self.decibels() / 20.0)
    }
}

impl fmt::Display for Preamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+.1} dB", self.decibels())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BandKind {
    #[default]
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    Notch,
    BandPass,
    AllPass,
}

impl BandKind {
    pub const ALL: [Self; 8] = [
        Self::Peaking,
        Self::LowShelf,
        Self::HighShelf,
        Self::LowPass,
        Self::HighPass,
        Self::Notch,
        Self::BandPass,
        Self::AllPass,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Peaking => "peaking",
            Self::LowShelf => "low-shelf",
            Self::HighShelf => "high-shelf",
            Self::LowPass => "low-pass",
            Self::HighPass => "high-pass",
            Self::Notch => "notch",
            Self::BandPass => "band-pass",
            Self::AllPass => "all-pass",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Peaking => "Peak",
            Self::LowShelf => "Low shelf",
            Self::HighShelf => "High shelf",
            Self::LowPass => "Low pass",
            Self::HighPass => "High pass",
            Self::Notch => "Notch",
            Self::BandPass => "Band pass",
            Self::AllPass => "All pass",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    pub const fn uses_gain(self) -> bool {
        matches!(self, Self::Peaking | Self::LowShelf | Self::HighShelf)
    }
}

impl fmt::Display for BandKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelSet(u8);

impl ChannelSet {
    pub const EVERY: Self = Self(u8::MAX);
    pub const NAMED_AT_MOST: usize = 8;

    pub const fn of(channels: &[usize]) -> Option<Self> {
        let mut held = 0_u8;
        let mut at = 0;
        while at < channels.len() {
            let channel = channels[at];
            if channel >= Self::NAMED_AT_MOST {
                return None;
            }
            held |= 1 << channel;
            at += 1;
        }
        if held == 0 { None } else { Some(Self(held)) }
    }

    pub const fn is_every(self) -> bool {
        self.0 == u8::MAX
    }

    pub const fn holds(self, channel: usize) -> bool {
        if channel >= Self::NAMED_AT_MOST {
            return self.is_every();
        }
        self.0 & (1 << channel) != 0
    }

    pub fn held(self) -> impl Iterator<Item = usize> {
        (0..Self::NAMED_AT_MOST).filter(move |channel| self.holds(*channel))
    }
}

impl Default for ChannelSet {
    fn default() -> Self {
        Self::EVERY
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Band {
    pub kind: BandKind,
    pub frequency: Frequency,
    pub gain: BandGain,
    pub q: Q,
    pub on: bool,
    pub channels: ChannelSet,
}

impl Band {
    pub const fn new(kind: BandKind, frequency: Frequency, gain: BandGain, q: Q) -> Self {
        Self {
            kind,
            frequency,
            gain: if kind.uses_gain() {
                gain
            } else {
                BandGain::FLAT
            },
            q,
            on: true,
            channels: ChannelSet::EVERY,
        }
    }

    pub const fn peaking(frequency: Frequency, gain: BandGain, q: Q) -> Self {
        Self::new(BandKind::Peaking, frequency, gain, q)
    }

    pub fn shapes(self) -> bool {
        self.on && !(self.kind.uses_gain() && self.gain.is_flat())
    }

    pub fn applies_at(self, rate: SampleRate) -> bool {
        self.shapes() && self.frequency.is_under_nyquist(rate)
    }

    pub fn design_for(self, rate: SampleRate, channel: usize) -> Biquad {
        if !self.channels.holds(channel) {
            return Biquad::IDENTITY;
        }
        self.design(rate)
    }

    pub fn design(self, rate: SampleRate) -> Biquad {
        if !self.applies_at(rate) {
            return Biquad::IDENTITY;
        }
        Biquad::of(self, rate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

struct Shaping {
    cosine: f64,
    alpha: f64,
    amplitude: f64,
}

impl Shaping {
    fn of(band: Band, rate: SampleRate) -> Self {
        let angle = band.frequency.angle_at(rate);
        Self {
            cosine: angle.cos(),
            alpha: angle.sin() / (2.0 * band.q.units()),
            amplitude: 10.0_f64.powf(band.gain.decibels() / 40.0),
        }
    }
}

impl Biquad {
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn of(band: Band, rate: SampleRate) -> Self {
        let shaping = Shaping::of(band, rate);
        match band.kind {
            BandKind::Peaking => Self::peaking(&shaping),
            BandKind::LowShelf => Self::low_shelf(&shaping),
            BandKind::HighShelf => Self::high_shelf(&shaping),
            BandKind::LowPass => Self::low_pass(&shaping),
            BandKind::HighPass => Self::high_pass(&shaping),
            BandKind::Notch => Self::notch(&shaping),
            BandKind::BandPass => Self::band_pass(&shaping),
            BandKind::AllPass => Self::all_pass(&shaping),
        }
    }

    fn normalised(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        if a0 == 0.0 || !a0.is_finite() {
            return Self::IDENTITY;
        }
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn peaking(shaping: &Shaping) -> Self {
        let Shaping {
            cosine,
            alpha,
            amplitude,
        } = *shaping;
        Self::normalised(
            1.0 + alpha * amplitude,
            -2.0 * cosine,
            1.0 - alpha * amplitude,
            1.0 + alpha / amplitude,
            -2.0 * cosine,
            1.0 - alpha / amplitude,
        )
    }

    fn low_shelf(shaping: &Shaping) -> Self {
        let Shaping {
            cosine,
            alpha,
            amplitude,
        } = *shaping;
        let skirt = 2.0 * amplitude.sqrt() * alpha;
        let (up, down) = (amplitude + 1.0, amplitude - 1.0);
        Self::normalised(
            amplitude * (up - down * cosine + skirt),
            2.0 * amplitude * (down - up * cosine),
            amplitude * (up - down * cosine - skirt),
            up + down * cosine + skirt,
            -2.0 * (down + up * cosine),
            up + down * cosine - skirt,
        )
    }

    fn high_shelf(shaping: &Shaping) -> Self {
        let Shaping {
            cosine,
            alpha,
            amplitude,
        } = *shaping;
        let skirt = 2.0 * amplitude.sqrt() * alpha;
        let (up, down) = (amplitude + 1.0, amplitude - 1.0);
        Self::normalised(
            amplitude * (up + down * cosine + skirt),
            -2.0 * amplitude * (down + up * cosine),
            amplitude * (up + down * cosine - skirt),
            up - down * cosine + skirt,
            2.0 * (down - up * cosine),
            up - down * cosine - skirt,
        )
    }

    fn low_pass(shaping: &Shaping) -> Self {
        let Shaping { cosine, alpha, .. } = *shaping;
        let shoulder = 1.0 - cosine;
        Self::normalised(
            shoulder / 2.0,
            shoulder,
            shoulder / 2.0,
            1.0 + alpha,
            -2.0 * cosine,
            1.0 - alpha,
        )
    }

    fn high_pass(shaping: &Shaping) -> Self {
        let Shaping { cosine, alpha, .. } = *shaping;
        let shoulder = 1.0 + cosine;
        Self::normalised(
            shoulder / 2.0,
            -shoulder,
            shoulder / 2.0,
            1.0 + alpha,
            -2.0 * cosine,
            1.0 - alpha,
        )
    }

    fn notch(shaping: &Shaping) -> Self {
        let Shaping { cosine, alpha, .. } = *shaping;
        Self::normalised(
            1.0,
            -2.0 * cosine,
            1.0,
            1.0 + alpha,
            -2.0 * cosine,
            1.0 - alpha,
        )
    }

    fn band_pass(shaping: &Shaping) -> Self {
        let Shaping { cosine, alpha, .. } = *shaping;
        Self::normalised(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cosine, 1.0 - alpha)
    }

    fn all_pass(shaping: &Shaping) -> Self {
        let Shaping { cosine, alpha, .. } = *shaping;
        Self::normalised(
            1.0 - alpha,
            -2.0 * cosine,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * cosine,
            1.0 - alpha,
        )
    }

    pub fn magnitude_db(self, hertz: f64, rate: SampleRate) -> f64 {
        let angle = std::f64::consts::TAU * hertz / f64::from(rate.hz());
        let above = squared_modulus(self.b0, self.b1, self.b2, angle);
        let below = squared_modulus(1.0, self.a1, self.a2, angle);
        if above <= 0.0 || below <= 0.0 {
            return SILENT_DB;
        }
        10.0 * (above / below).log10()
    }
}

const SILENT_DB: f64 = -400.0;

fn squared_modulus(zeroth: f64, first: f64, second: f64, angle: f64) -> f64 {
    let real = zeroth + first * angle.cos() + second * (2.0 * angle).cos();
    let imaginary = first * angle.sin() + second * (2.0 * angle).sin();
    real * real + imaginary * imaginary
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Profile {
    preamp: Preamp,
    bands: Vec<Band>,
}

impl Profile {
    pub fn new(preamp: Preamp, bands: Vec<Band>) -> Result<Self> {
        if bands.len() > MAX_BANDS {
            return Err(Error::TooManyBands(bands.len()));
        }
        Ok(Self { preamp, bands })
    }

    pub fn flat() -> Self {
        Self::default()
    }

    pub const fn preamp(&self) -> Preamp {
        self.preamp
    }

    pub const fn set_preamp(&mut self, preamp: Preamp) {
        self.preamp = preamp;
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    pub fn band_mut(&mut self, at: usize) -> Option<&mut Band> {
        self.bands.get_mut(at)
    }

    pub fn push(&mut self, band: Band) -> Result<()> {
        if self.bands.len() >= MAX_BANDS {
            return Err(Error::TooManyBands(self.bands.len() + 1));
        }
        self.bands.push(band);
        Ok(())
    }

    pub fn remove(&mut self, at: usize) -> bool {
        if at >= self.bands.len() {
            return false;
        }
        self.bands.remove(at);
        true
    }

    pub fn is_transparent(&self) -> bool {
        self.bands.is_empty() && self.preamp.is_none()
    }

    pub fn designed(&self, rate: SampleRate) -> impl Iterator<Item = Biquad> + '_ {
        self.bands.iter().map(move |band| band.design(rate))
    }

    pub fn applied(&self, rate: SampleRate) -> usize {
        self.bands
            .iter()
            .filter(|band| band.applies_at(rate))
            .count()
    }

    pub fn passed_over(&self, rate: SampleRate) -> usize {
        self.bands
            .iter()
            .filter(|band| band.shapes() && !band.frequency.is_under_nyquist(rate))
            .count()
    }

    pub fn magnitude_db(&self, hertz: f64, rate: SampleRate) -> f64 {
        self.preamp.decibels()
            + self
                .designed(rate)
                .map(|section| section.magnitude_db(hertz, rate))
                .sum::<f64>()
    }

    pub fn response(&self, rate: SampleRate, points: usize) -> Vec<f64> {
        sweep(points)
            .map(|hertz| self.magnitude_db(hertz, rate))
            .collect()
    }

    pub fn peak_db(&self, rate: SampleRate) -> f64 {
        sweep(RESPONSE_POINTS).fold(f64::NEG_INFINITY, |highest, hertz| {
            highest.max(self.magnitude_db(hertz, rate) - self.preamp.decibels())
        })
    }

    pub fn fitted_preamp(&self, rate: SampleRate) -> Preamp {
        let peak = self.peak_db(rate);
        if !peak.is_finite() || peak <= 0.0 {
            return Preamp::NONE;
        }
        Preamp::from_decibels(-peak).unwrap_or(Preamp::NONE)
    }
}

pub const RESPONSE_POINTS: usize = 256;

pub fn sweep(points: usize) -> impl Iterator<Item = f64> {
    let last = points.saturating_sub(1).max(1) as f64;
    let decades = (RESPONSE_TO_HZ / RESPONSE_FROM_HZ).log10();
    (0..points).map(move |point| RESPONSE_FROM_HZ * 10.0_f64.powf(decades * point as f64 / last))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [SampleRate; 4] = [
        SampleRate::HZ_44100,
        SampleRate::HZ_48000,
        SampleRate::HZ_96000,
        SampleRate::HZ_192000,
    ];

    fn hertz(hertz: f64) -> Frequency {
        Frequency::from_hertz(hertz).expect("a frequency in range")
    }

    fn decibels(decibels: f64) -> BandGain {
        BandGain::from_decibels(decibels).expect("a gain in range")
    }

    fn quality(units: f64) -> Q {
        Q::from_units(units).expect("a Q in range")
    }

    fn band(kind: BandKind, at: f64, gain: f64, q: f64) -> Band {
        Band::new(kind, hertz(at), decibels(gain), quality(q))
    }

    #[test]
    fn a_band_with_no_gain_designs_to_the_identity_filter() {
        for rate in RATES {
            for kind in [BandKind::Peaking, BandKind::LowShelf, BandKind::HighShelf] {
                let designed = band(kind, 1_000.0, 0.0, 1.0).design(rate);
                assert_eq!(designed, Biquad::IDENTITY, "{kind} at {rate}");
            }
        }
    }

    #[test]
    fn a_peaking_band_lifts_its_own_centre_by_exactly_its_gain() {
        for rate in RATES {
            for centre in [30.0, 250.0, 1_000.0, 8_800.0] {
                for gain in [-12.0, -3.0, 0.7, 6.4, 18.0] {
                    for q in [0.5, 1.41, 3.9, 10.0] {
                        let designed = band(BandKind::Peaking, centre, gain, q).design(rate);
                        let measured = designed.magnitude_db(centre, rate);
                        assert!(
                            (measured - gain).abs() < 1e-9,
                            "{gain} dB at {centre} Hz, Q {q}, {rate} measured {measured}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_shelf_reaches_its_gain_at_one_end_and_half_of_it_at_its_corner() {
        let rate = SampleRate::HZ_48000;

        let low = band(BandKind::LowShelf, 105.0, 6.4, 0.7).design(rate);
        assert!((low.magnitude_db(0.001, rate) - 6.4).abs() < 1e-9);
        assert!((low.magnitude_db(105.0, rate) - 3.2).abs() < 1e-9);
        assert!(low.magnitude_db(10_000.0, rate).abs() < 1e-3);

        let high = band(BandKind::HighShelf, 10_000.0, -2.1, 0.7).design(rate);
        assert!(high.magnitude_db(0.001, rate).abs() < 1e-9);
        assert!((high.magnitude_db(10_000.0, rate) + 1.05).abs() < 1e-9);
        assert!((high.magnitude_db(20_000.0, rate) + 2.1).abs() < 0.02);
    }

    #[test]
    fn an_all_pass_holds_unity_at_every_frequency_and_a_notch_is_silent_at_its_centre() {
        let rate = SampleRate::HZ_48000;

        let all_pass = band(BandKind::AllPass, 1_000.0, 0.0, 1.0).design(rate);
        for point in sweep(64) {
            assert!(
                all_pass.magnitude_db(point, rate).abs() < 1e-9,
                "an all-pass moved the level at {point} Hz"
            );
        }

        let notch = band(BandKind::Notch, 1_000.0, 0.0, 4.0).design(rate);
        assert!(notch.magnitude_db(1_000.0, rate) <= SILENT_DB);

        let band_pass = band(BandKind::BandPass, 1_000.0, 0.0, 4.0).design(rate);
        assert!(band_pass.magnitude_db(1_000.0, rate).abs() < 1e-9);
    }

    #[test]
    fn a_low_pass_is_three_decibels_down_at_its_corner() {
        let rate = SampleRate::HZ_48000;
        let designed = band(BandKind::LowPass, 1_000.0, 0.0, 0.707).design(rate);
        let measured = designed.magnitude_db(1_000.0, rate);
        assert!((measured + 3.0103).abs() < 0.01, "got {measured}");
    }

    #[test]
    fn a_band_at_or_above_nyquist_is_passed_over_rather_than_folded_back_into_the_band() {
        let rate = SampleRate::HZ_44100;
        let above = band(BandKind::Peaking, 30_000.0, 12.0, 1.0);

        assert!(!above.applies_at(rate));
        assert_eq!(above.design(rate), Biquad::IDENTITY);
        assert!(above.applies_at(SampleRate::HZ_96000));

        let folded_to = 44_100.0 - 30_000.0;
        assert!(
            above.design(rate).magnitude_db(folded_to, rate).abs() < 1e-12,
            "a band above Nyquist reached {folded_to} Hz"
        );
    }

    #[test]
    fn a_kind_that_reads_no_gain_is_never_built_carrying_one() {
        let carried = band(BandKind::Notch, 1_000.0, 9.0, 4.0);
        assert_eq!(carried.gain, BandGain::FLAT);
        assert!(carried.shapes());

        let peaking = band(BandKind::Peaking, 1_000.0, 9.0, 4.0);
        assert_eq!(peaking.gain.millibels(), 9_000);
    }

    #[test]
    fn a_disabled_band_is_still_a_band_but_shapes_nothing() {
        let rate = SampleRate::HZ_48000;
        let mut quiet = band(BandKind::Peaking, 1_000.0, 6.0, 1.0);
        quiet.on = false;

        assert!(!quiet.shapes());
        assert_eq!(quiet.design(rate), Biquad::IDENTITY);

        let profile = Profile::new(Preamp::NONE, vec![quiet]).expect("one band");
        assert!(!profile.is_transparent());
    }

    #[test]
    fn an_empty_profile_is_transparent_and_a_profile_of_flat_bands_is_not() {
        assert!(Profile::flat().is_transparent());

        let flat = Profile::new(
            Preamp::NONE,
            vec![band(BandKind::Peaking, 1_000.0, 0.0, 1.0)],
        )
        .expect("one band");
        assert!(
            !flat.is_transparent(),
            "a profile holding a band must keep its stage so a change reaches it"
        );
        assert!(flat.bands().iter().all(|band| !band.shapes()));
    }

    #[test]
    fn the_values_a_band_stores_round_trip_the_text_it_is_written_in() {
        for tenth in -400..=400 {
            let written = f64::from(tenth) / 10.0;
            let read = decibels(written).decibels();
            assert_eq!(format!("{read:.1}"), format!("{written:.1}"));
        }
        for hundredth in 100..=4_000 {
            let written = f64::from(hundredth) / 100.0;
            let read = quality(written).units();
            assert_eq!(format!("{read:.2}"), format!("{written:.2}"));
        }
        for whole in [1_u32, 20, 105, 1_227, 8_800, 20_000] {
            let read = hertz(f64::from(whole)).hertz();
            assert_eq!(format!("{read:.0}"), format!("{whole}"));
        }
    }

    #[test]
    fn a_band_outside_what_the_vocabulary_allows_is_refused() {
        assert!(Frequency::from_hertz(0.5).is_err());
        assert!(Frequency::from_hertz(48_000.0).is_err());
        assert!(BandGain::from_decibels(41.0).is_err());
        assert!(BandGain::from_decibels(f64::NAN).is_err());
        assert!(Q::from_units(0.05).is_err());
        assert!(Q::from_units(41.0).is_err());
        assert!(Preamp::from_decibels(-41.0).is_err());

        let too_many = vec![band(BandKind::Peaking, 1_000.0, 1.0, 1.0); MAX_BANDS + 1];
        assert!(matches!(
            Profile::new(Preamp::NONE, too_many),
            Err(Error::TooManyBands(_))
        ));
    }

    fn sennheiser_hd_650() -> Profile {
        let bands = vec![
            band(BandKind::LowShelf, 105.0, 6.4, 0.70),
            band(BandKind::Peaking, 8_800.0, 5.1, 1.42),
            band(BandKind::Peaking, 118.0, -3.1, 0.50),
            band(BandKind::Peaking, 37.0, 0.7, 3.96),
            band(BandKind::Peaking, 3_169.0, -1.7, 3.89),
            band(BandKind::HighShelf, 10_000.0, -2.1, 0.70),
            band(BandKind::Peaking, 1_227.0, -1.2, 2.53),
            band(BandKind::Peaking, 2_055.0, 1.2, 3.23),
            band(BandKind::Peaking, 587.0, 0.4, 1.19),
            band(BandKind::Peaking, 5_332.0, -1.1, 5.75),
        ];
        Profile::new(Preamp::from_decibels(-6.1).expect("in range"), bands).expect("ten bands")
    }

    #[test]
    fn the_preamp_a_measurement_declares_is_the_peak_this_design_computes() {
        let profile = sennheiser_hd_650();
        let rate = SampleRate::HZ_48000;

        let peak = profile.peak_db(rate);
        let declared = -profile.preamp().decibels();
        assert!(
            (peak - declared).abs() < 0.1,
            "AutoEq declared a preamp of {declared} dB against a computed peak of {peak} dB"
        );
        assert_eq!(profile.applied(rate), 10);
        assert_eq!(profile.passed_over(rate), 0);
    }

    #[test]
    fn a_suggested_preamp_holds_the_response_at_or_below_full_scale() {
        let rate = SampleRate::HZ_48000;
        let boosting = Profile::new(
            Preamp::NONE,
            vec![
                band(BandKind::LowShelf, 105.0, 9.0, 0.7),
                band(BandKind::Peaking, 120.0, 6.0, 1.0),
            ],
        )
        .expect("two bands");

        let fitted = boosting.fitted_preamp(rate);
        assert!(fitted.decibels() < -9.0, "{fitted}");

        let held = Profile::new(fitted, boosting.bands().to_vec()).expect("two bands");
        for point in sweep(RESPONSE_POINTS) {
            let level = held.magnitude_db(point, rate);
            assert!(level <= 0.01, "{level} dB at {point} Hz");
        }

        assert_eq!(Profile::flat().fitted_preamp(rate), Preamp::NONE);
    }

    #[test]
    fn a_response_runs_from_the_bottom_of_the_band_to_the_top_of_it() {
        let points: Vec<f64> = sweep(RESPONSE_POINTS).collect();
        assert_eq!(points.len(), RESPONSE_POINTS);
        assert!((points[0] - RESPONSE_FROM_HZ).abs() < 1e-9);
        assert!((points[RESPONSE_POINTS - 1] - RESPONSE_TO_HZ).abs() < 1e-6);
        assert!(points.windows(2).all(|pair| pair[1] > pair[0]));

        let drawn = sennheiser_hd_650().response(SampleRate::HZ_48000, RESPONSE_POINTS);
        assert_eq!(drawn.len(), RESPONSE_POINTS);
        assert!(drawn.iter().all(|level| level.is_finite()));
    }

    #[test]
    fn two_profiles_a_hundredth_of_a_decibel_apart_are_not_equal() {
        let one = Profile::new(
            Preamp::NONE,
            vec![band(BandKind::Peaking, 1_000.0, 3.0, 1.0)],
        )
        .expect("one band");
        let other = Profile::new(
            Preamp::NONE,
            vec![band(BandKind::Peaking, 1_000.0, 3.01, 1.0)],
        )
        .expect("one band");

        assert_ne!(one, other);
        assert_eq!(one, one.clone());
    }

    #[test]
    fn every_kind_the_vocabulary_names_reads_back_as_itself() {
        for kind in BandKind::ALL {
            assert_eq!(BandKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(BandKind::parse("shelving"), None);
    }
}
