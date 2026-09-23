use std::{fmt, num::NonZeroU32};

use resonate_core::{SampleFormat, SampleRate, Silence};

use crate::dsd::Dop;

pub const DOP_DECIMATION: u32 = 16;

const CD_DSD_RATE: u32 = 2_822_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DsdRate(NonZeroU32);

impl DsdRate {
    pub fn new(hz: u32) -> Option<Self> {
        if !hz.is_multiple_of(DOP_DECIMATION) {
            return None;
        }
        SampleRate::new(hz / DOP_DECIMATION).ok()?;
        NonZeroU32::new(hz).map(Self)
    }

    pub const fn hz(self) -> u32 {
        self.0.get()
    }

    pub fn carrier(self) -> SampleRate {
        SampleRate::new(self.hz() / DOP_DECIMATION)
            .expect("a DsdRate only exists where its carrier is representable")
    }

    pub const fn times_cd(self) -> Option<u32> {
        if self.hz().is_multiple_of(CD_DSD_RATE) {
            Some(self.hz() / CD_DSD_RATE * 64)
        } else {
            None
        }
    }
}

impl fmt::Display for DsdRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.times_cd() {
            Some(times) => write!(f, "DSD{times}"),
            None => write!(f, "{} Hz 1-bit", self.hz()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Packing {
    #[default]
    Samples,
    DopMarked(DsdRate),
}

impl Packing {
    pub fn silence(self, format: SampleFormat) -> Silence {
        match self {
            Self::Samples => Silence::Unmarked,
            Self::DopMarked(_) => Dop::silence(format),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dsd_rate_is_named_by_what_it_multiplies_the_cd_rate_by() {
        assert_eq!(DsdRate::new(2_822_400).expect("dsd64").to_string(), "DSD64");
        assert_eq!(
            DsdRate::new(5_644_800).expect("dsd128").to_string(),
            "DSD128"
        );
        assert_eq!(
            DsdRate::new(11_289_600).expect("dsd256").to_string(),
            "DSD256"
        );
    }

    #[test]
    fn the_carrier_is_the_rate_a_dop_stream_runs_at() {
        assert_eq!(
            DsdRate::new(2_822_400).expect("dsd64").carrier(),
            SampleRate::HZ_176400
        );
        assert_eq!(
            DsdRate::new(3_072_000)
                .expect("a 48 kHz family rate")
                .carrier(),
            SampleRate::HZ_192000
        );
    }

    #[test]
    fn only_a_marked_packing_has_a_silence_the_graph_cannot_supply_itself() {
        assert_eq!(
            Packing::Samples.silence(SampleFormat::S24),
            Silence::Unmarked
        );
        assert!(matches!(
            Packing::DopMarked(DsdRate::new(2_822_400).expect("dsd64")).silence(SampleFormat::S24),
            Silence::Marked { .. }
        ));
    }

    #[test]
    fn a_rate_whose_carrier_is_past_the_hardware_range_is_refused() {
        assert_eq!(DsdRate::new(22_579_200), None, "DSD512 has no carrier");
        assert_eq!(DsdRate::new(0), None);
        assert_eq!(
            DsdRate::new(2_822_401),
            None,
            "a rate that is not sixteen-aligned"
        );
    }
}
