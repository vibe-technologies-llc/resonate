use crate::{Decibels, Gain, eq::Frequency};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeasuredGain {
    pub track: Option<Decibels>,
    pub album: Option<Decibels>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackHints {
    pub true_peak: Option<Gain>,
    pub lowpass: Option<Frequency>,
    pub measured: MeasuredGain,
}
