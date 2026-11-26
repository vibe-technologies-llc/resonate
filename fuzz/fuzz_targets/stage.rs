use std::sync::Arc;

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec, eq::Profile};
use resonate_dsp::{Equaliser, Processor};

pub const RATES: [SampleRate; 3] = [
    SampleRate::HZ_44100,
    SampleRate::HZ_96000,
    SampleRate::HZ_192000,
];
pub const IMPULSE_FRAMES: usize = 1_024;
const ESTIMATE_SLACK: f64 = 2.0;
const LOWEST_HZ: f64 = 1.0;
const POINTS: usize = 4_096;

pub fn impulse_through(profile: &Profile, rate: SampleRate) -> Vec<f64> {
    let spec = StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32);
    let mut stage = Equaliser::new(Arc::new(profile.clone()), rate);

    stage
        .prepare(spec, IMPULSE_FRAMES)
        .expect("the stage takes the rate it was built for");

    let mut input = vec![0.0; IMPULSE_FRAMES * 2];

    input[0] = 1.0;
    input[1] = 1.0;

    let mut output = vec![0.0; IMPULSE_FRAMES * 2];

    stage.process(&input, &mut output);
    output
}

fn loudest_db_to_nyquist(profile: &Profile, rate: SampleRate) -> f64 {
    let nyquist = f64::from(rate.hz()) / 2.0;
    let ratio = (nyquist / LOWEST_HZ).powf(1.0 / POINTS as f64);
    let centres = profile.bands().iter().map(|band| band.frequency.hertz());

    let hertz: Vec<f64> = (0..=POINTS)
        .map(|point| LOWEST_HZ * ratio.powi(point as i32))
        .chain(centres.filter(|hertz| *hertz <= nyquist))
        .collect();

    profile
        .heard_at(&hertz, rate)
        .iter()
        .flatten()
        .map(|(real, imaginary)| 20.0 * real.hypot(*imaginary).log10())
        .fold(f64::NEG_INFINITY, f64::max)
}

fn peak_at_most(profile: &Profile, fitted: &Profile, rate: SampleRate) -> f64 {
    let unheld_db = (loudest_db_to_nyquist(profile, rate) + fitted.preamp().decibels()).max(0.0);
    ESTIMATE_SLACK * 10.0_f64.powf(unheld_db / 20.0)
}

pub fn holds_under_full_scale(profile: &Profile) {
    for rate in RATES {
        let mut fitted = profile.clone();

        fitted.set_preamp(profile.fitted_preamp(rate));

        let output = impulse_through(&fitted, rate);
        let peak = output
            .iter()
            .fold(0.0_f64, |peak, sample| peak.max(sample.abs()));

        assert!(
            output.iter().all(|sample| sample.is_finite()),
            "{rate:?} {fitted:?}"
        );
        assert!(
            peak <= peak_at_most(profile, &fitted, rate),
            "{rate:?} {peak} {fitted:?}"
        );
    }
}
