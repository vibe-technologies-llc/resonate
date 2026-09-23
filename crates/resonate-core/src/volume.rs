use std::fmt;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Decibels(f32);

impl Decibels {
    pub const SILENCE: Self = Self(f32::NEG_INFINITY);
    pub const UNITY: Self = Self(0.0);

    pub fn new(db: f32) -> Result<Self> {
        if db.is_nan() || db == f32::INFINITY {
            return Err(Error::DecibelsNotFinite(db));
        }
        Ok(Self(db))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub fn to_gain(self) -> Gain {
        if self.0 == f32::NEG_INFINITY {
            return Gain::SILENT;
        }
        Gain(10.0_f32.powf(self.0 / 20.0))
    }
}

const MILLIBELS_PER_DECIBEL: f32 = 100.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Trim(i32);

impl Trim {
    pub const NONE: Self = Self(0);
    pub const WIDEST_MILLIBELS: i32 = 2_000;

    pub const fn from_millibels(millibels: i32) -> Result<Self> {
        if millibels < -Self::WIDEST_MILLIBELS || millibels > Self::WIDEST_MILLIBELS {
            return Err(Error::TrimOutOfRange(millibels));
        }
        Ok(Self(millibels))
    }

    pub fn from_decibels(decibels: f64) -> Result<Self> {
        let millibels = (decibels * f64::from(MILLIBELS_PER_DECIBEL)).round();
        if !millibels.is_finite() {
            return Err(Error::TrimOutOfRange(i32::MAX));
        }
        Self::from_millibels(millibels.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
    }

    pub const fn millibels(self) -> i32 {
        self.0
    }

    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    pub fn decibels(self) -> Decibels {
        Decibels(self.0 as f32 / MILLIBELS_PER_DECIBEL)
    }

    pub fn raised(self, level: Decibels) -> Decibels {
        Decibels(level.0 + self.decibels().0)
    }
}

impl fmt::Display for Trim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+.1} dB", self.decibels().0)
    }
}

impl fmt::Display for Decibels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} dB", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Gain(f32);

impl Gain {
    pub const SILENT: Self = Self(0.0);
    pub const UNITY: Self = Self(1.0);

    pub fn new(linear: f32) -> Result<Self> {
        if !linear.is_finite() || linear < 0.0 {
            return Err(Error::GainNotFinite(linear));
        }
        Ok(Self(linear))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub const fn is_unity(self) -> bool {
        self.0 == 1.0
    }

    pub fn to_decibels(self) -> Decibels {
        if self.0 == 0.0 {
            return Decibels::SILENCE;
        }
        Decibels(20.0 * self.0.log10())
    }
}

impl fmt::Display for Gain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppliedGain {
    pub gain: Option<Decibels>,
    pub peak: Option<Gain>,
}

impl AppliedGain {
    pub fn requested(self) -> Gain {
        self.gain.map_or(Gain::UNITY, Decibels::to_gain)
    }

    pub fn ceiling(self) -> Option<Gain> {
        let peak = self.peak.filter(|peak| peak.get() > 0.0)?;
        Gain::new(1.0 / peak.get()).ok()
    }

    pub fn applied(self) -> Gain {
        let requested = self.requested();
        match self.ceiling() {
            Some(ceiling) if ceiling < requested => ceiling,
            _ => requested,
        }
    }

    pub fn is_capped(self) -> bool {
        self.applied() < self.requested()
    }

    pub fn adjusts(self) -> bool {
        !self.applied().is_unity()
    }

    pub fn heeding(self, true_peak: Option<Gain>) -> Self {
        let peak = match (self.peak, true_peak) {
            (Some(declared), Some(measured)) if measured > declared => Some(measured),
            (None, measured) => measured,
            (declared, _) => declared,
        };
        Self { peak, ..self }
    }

    pub fn headroom(self) -> Option<Decibels> {
        let peak = self.peak.filter(|peak| peak.get() > 0.0)?;
        let after = Gain::new(peak.get() * self.applied().get()).ok()?;
        Decibels::new(-after.to_decibels().get()).ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Volume(f32);

impl Volume {
    pub const MUTE: Self = Self(0.0);
    pub const MAX: Self = Self(1.0);

    pub fn new(position: f32) -> Result<Self> {
        if !position.is_finite() || !(0.0..=1.0).contains(&position) {
            return Err(Error::VolumeOutOfRange(position));
        }
        Ok(Self(position))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub fn to_gain(self) -> Gain {
        Gain(self.0 * self.0 * self.0)
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::MAX
    }
}

impl fmt::Display for Volume {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0}%", self.0 * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_is_zero_decibels_in_both_directions() {
        assert_eq!(Decibels::UNITY.to_gain(), Gain::UNITY);
        assert_eq!(Gain::UNITY.to_decibels(), Decibels::UNITY);
    }

    #[test]
    fn minus_six_decibels_halves_amplitude() {
        let gain = Decibels::new(-6.0206).expect("finite").to_gain();
        assert!((gain.get() - 0.5).abs() < 1e-4, "got {gain}");
    }

    #[test]
    fn silence_maps_to_zero_gain_and_back() {
        assert_eq!(Decibels::SILENCE.to_gain(), Gain::SILENT);
        assert_eq!(Gain::SILENT.to_decibels(), Decibels::SILENCE);
    }

    #[test]
    fn decibel_gain_round_trip_is_stable() {
        for db in [-60.0, -24.0, -6.0, -0.5, 0.0, 3.0, 12.0] {
            let round_tripped = Decibels::new(db).expect("finite").to_gain().to_decibels();
            assert!(
                (round_tripped.get() - db).abs() < 1e-3,
                "{db} dB round-tripped to {round_tripped}"
            );
        }
    }

    #[test]
    fn nan_and_out_of_range_values_never_reach_the_audio_path() {
        assert!(Decibels::new(f32::NAN).is_err());
        assert!(Decibels::new(f32::INFINITY).is_err());
        assert!(Gain::new(-0.1).is_err());
        assert!(Gain::new(f32::NAN).is_err());
        assert!(Volume::new(1.1).is_err());
        assert!(Volume::new(-0.0001).is_err());
    }

    fn decibels(db: f32) -> Decibels {
        Decibels::new(db).expect("finite")
    }

    fn gain(linear: f32) -> Gain {
        Gain::new(linear).expect("in range")
    }

    #[test]
    fn a_boost_is_capped_at_the_point_the_declared_peak_would_clip() {
        let applied = AppliedGain {
            gain: Some(decibels(12.0)),
            peak: Some(gain(0.5)),
        };

        assert!((applied.requested().get() - 3.981).abs() < 1e-3);
        assert_eq!(applied.applied(), Gain::new(2.0).expect("in range"));
        assert!(applied.is_capped());
    }

    #[test]
    fn a_boost_that_fits_under_the_peak_is_left_alone() {
        let applied = AppliedGain {
            gain: Some(decibels(3.0)),
            peak: Some(gain(0.5)),
        };

        assert_eq!(applied.applied(), applied.requested());
        assert!(!applied.is_capped());
    }

    #[test]
    fn a_boost_a_full_scale_peak_caps_at_unity_leaves_the_samples_alone() {
        let applied = AppliedGain {
            gain: Some(decibels(6.0)),
            peak: Some(gain(1.0)),
        };

        assert!(applied.is_capped());
        assert_eq!(applied.applied(), Gain::UNITY);
        assert!(
            !applied.adjusts(),
            "a boost capped at unity was said to change the samples"
        );
    }

    #[test]
    fn a_boost_with_room_under_its_peak_changes_the_samples() {
        let applied = AppliedGain {
            gain: Some(decibels(6.0)),
            peak: Some(gain(0.5)),
        };

        assert!(!applied.applied().is_unity());
        assert!(applied.adjusts());
    }

    #[test]
    fn a_source_already_past_full_scale_is_pulled_below_it() {
        let applied = AppliedGain {
            gain: Some(Decibels::UNITY),
            peak: Some(gain(1.25)),
        };

        assert_eq!(applied.applied(), Gain::new(0.8).expect("in range"));
        assert!(
            applied.adjusts(),
            "a unity gain that clips was left transparent"
        );
    }

    #[test]
    fn an_undeclared_peak_leaves_the_gain_and_the_headroom_unknown() {
        let applied = AppliedGain {
            gain: Some(decibels(12.0)),
            peak: None,
        };

        assert_eq!(applied.applied(), applied.requested());
        assert_eq!(applied.headroom(), None);
        assert_eq!(AppliedGain::default().applied(), Gain::UNITY);
    }

    #[test]
    fn headroom_is_what_is_left_below_full_scale_once_the_gain_is_applied() {
        let halved = AppliedGain {
            gain: None,
            peak: Some(gain(0.5)),
        };
        assert!(
            (halved.headroom().expect("a peak").get() - 6.0206).abs() < 1e-3,
            "{:?}",
            halved.headroom()
        );

        let capped = AppliedGain {
            gain: Some(decibels(12.0)),
            peak: Some(gain(0.5)),
        };
        assert!(capped.headroom().expect("a peak").get().abs() < 1e-3);
    }

    #[test]
    fn a_measured_true_peak_over_full_scale_attenuates_a_track_nothing_tagged() {
        let untagged = AppliedGain::default().heeding(Some(gain(1.25)));
        assert!(
            untagged.adjusts(),
            "an over the file carries was left to clip"
        );
        assert!((untagged.applied().get() - 0.8).abs() < 1e-6);

        let under = AppliedGain::default().heeding(Some(gain(0.9)));
        assert!(!under.adjusts(), "a track under full scale was turned down");

        let tagged = AppliedGain {
            gain: Some(decibels(6.0)),
            peak: Some(gain(0.4)),
        };
        assert_eq!(tagged.heeding(Some(gain(0.3))).peak, Some(gain(0.4)));
        assert_eq!(tagged.heeding(Some(gain(0.6))).peak, Some(gain(0.6)));
        assert_eq!(tagged.heeding(None), tagged);
    }

    #[test]
    fn full_volume_is_unity_gain() {
        assert_eq!(Volume::MAX.to_gain(), Gain::UNITY);
        assert_eq!(Volume::MUTE.to_gain(), Gain::SILENT);
    }
}
