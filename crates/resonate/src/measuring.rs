use std::time::Duration;

use resonate_core::SampleRate;
use resonate_dsp::Sweep;
use resonate_eq::{Error as EqError, Point, Response};
use resonate_listen::{Hearing, Listener, Listening};
use resonate_pipewire::NodeName;

use crate::{Error, Result};

pub const MEASURED_AT: SampleRate = SampleRate::HZ_48000;
pub const SWEEP_FROM_HZ: u32 = 20;
pub const SWEEP_TO_HZ: u32 = 20_000;
pub const SWEEP_FOR: Duration = Duration::from_secs(8);
pub const HEARD_AFTER_THE_SWEEP: Duration = Duration::from_secs(2);

const APP_NAME: &str = "Resonate";
const CSV_HEADER: &str = "frequency,raw";

pub trait Rig {
    fn sound(&self, sweep: &Sweep, heard_for: Duration) -> Result<Vec<f32>>;
}

pub struct Room<'a> {
    pub through: Option<&'a NodeName>,
    pub from: Listening,
}

impl Rig for Room<'_> {
    fn sound(&self, sweep: &Sweep, heard_for: Duration) -> Result<Vec<f32>> {
        let clip = Listener::new(APP_NAME).play_and_record(
            sweep.samples(),
            self.through,
            &self.from,
            heard_for,
            &Hearing::new(),
        )?;
        Ok(clip.mono())
    }
}

pub fn measured_by(rig: &dyn Rig) -> Result<Response> {
    let sweep = Sweep::new(MEASURED_AT, SWEEP_FROM_HZ, SWEEP_TO_HZ, SWEEP_FOR)?;
    let heard_for = sweep.length() + HEARD_AFTER_THE_SWEEP;

    let recorded = rig.sound(&sweep, heard_for)?;
    let levels = sweep.heard_as(&recorded)?.response()?;

    let points = levels
        .into_iter()
        .map(|level| Point {
            hertz: level.hertz,
            decibels: level.decibels,
        })
        .collect();
    Response::from_points(points).ok_or(Error::Eq(EqError::NoMeasuredPoints))
}

pub fn calibrated(heard: &Response, microphone: &Response) -> Result<Response> {
    let points = heard
        .points()
        .iter()
        .map(|point| Point {
            hertz: point.hertz,
            decibels: point.decibels - microphone.at(point.hertz),
        })
        .collect();
    Response::from_points(points).ok_or(Error::Eq(EqError::NoMeasuredPoints))
}

pub fn as_csv(response: &Response) -> String {
    let rows: String = response
        .points()
        .iter()
        .map(|point| format!("{:.3},{:.3}\n", point.hertz, point.decibels))
        .collect();
    format!("{CSV_HEADER}\n{rows}")
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use resonate_eq::{Fitting, Layout, POINTS_AT_LEAST, fit, read_response};

    use super::*;

    const SMOOTHING: f64 = 0.3;
    const DELAY_FRAMES: usize = 211;

    struct Lowpassed;

    impl Rig for Lowpassed {
        fn sound(&self, sweep: &Sweep, heard_for: Duration) -> Result<Vec<f32>> {
            let total = (heard_for.as_secs_f64() * f64::from(sweep.rate().hz())) as usize;
            let mut held = 0.0_f64;
            let mut heard = vec![0.0_f32; DELAY_FRAMES];
            for at in 0..total - DELAY_FRAMES {
                let sample = sweep.samples().get(at).copied().unwrap_or(0.0);
                held = SMOOTHING * f64::from(sample) + (1.0 - SMOOTHING) * held;
                heard.push(held as f32);
            }
            Ok(heard)
        }
    }

    struct Silent;

    impl Rig for Silent {
        fn sound(&self, _sweep: &Sweep, heard_for: Duration) -> Result<Vec<f32>> {
            let total = (heard_for.as_secs_f64() * f64::from(MEASURED_AT.hz())) as usize;
            Ok(vec![0.0; total])
        }
    }

    fn lowpass_db(hertz: f64) -> f64 {
        let turn = 2.0 * PI * hertz / f64::from(MEASURED_AT.hz());
        let real = 1.0 - (1.0 - SMOOTHING) * turn.cos();
        let imaginary = (1.0 - SMOOTHING) * turn.sin();
        20.0 * (SMOOTHING / real.hypot(imaginary)).log10()
    }

    #[test]
    fn a_sweep_through_a_fake_output_and_microphone_measures_the_filter_between_them() {
        let measured = measured_by(&Lowpassed).expect("the sweep is measured");

        for hertz in [100.0, 250.0, 1_000.0, 3_000.0, 6_000.0, 10_000.0] {
            let off = (measured.at(hertz) - lowpass_db(hertz)).abs();
            assert!(off < 0.5, "{hertz} Hz is {off:.2} dB off the filter's");
        }
        assert!(measured.lowest_hertz() <= 21.0 && measured.highest_hertz() >= 19_900.0);
    }

    #[test]
    fn what_is_measured_live_goes_through_the_fit_a_file_goes_through() {
        let measured = measured_by(&Lowpassed).expect("the sweep is measured");
        let flat = Response::flat();

        let fitted = fit(&measured, &flat, &Fitting::default()).expect("a fit");

        assert!(!fitted.profile.bands().is_empty());
        assert!(
            fitted.off_by_db_after < fitted.off_by_db_before * 0.5,
            "{:.2} dB off before and {:.2} after",
            fitted.off_by_db_before,
            fitted.off_by_db_after
        );
    }

    #[test]
    fn what_is_measured_is_kept_in_the_headed_layout_a_measurement_file_reads_back_from() {
        let measured = measured_by(&Lowpassed).expect("the sweep is measured");

        let read = read_response(as_csv(&measured).as_bytes()).expect("the csv reads");

        assert_eq!(read.layout, Layout::Headed);
        assert_eq!(read.passed_over, 0);
        assert!(read.response.points().len() >= POINTS_AT_LEAST);
        assert!((read.response.at(1_000.0) - measured.at(1_000.0)).abs() < 0.01);
    }

    #[test]
    fn a_microphones_own_response_is_taken_off_what_it_heard() {
        let heard = measured_by(&Lowpassed).expect("the sweep is measured");
        let microphone_is_the_filter = Response::from_points(
            heard
                .points()
                .iter()
                .map(|point| Point {
                    hertz: point.hertz,
                    decibels: lowpass_db(point.hertz),
                })
                .collect(),
        )
        .expect("a response");

        let calibrated = calibrated(&heard, &microphone_is_the_filter).expect("a response");

        for hertz in [100.0, 1_000.0, 6_000.0, 10_000.0] {
            let off = calibrated.at(hertz).abs();
            assert!(off < 0.5, "{hertz} Hz is {off:.2} dB off flat");
        }
    }

    #[test]
    fn a_microphone_that_heard_nothing_is_refused_not_fitted() {
        assert!(matches!(
            measured_by(&Silent),
            Err(Error::Sweep(resonate_dsp::Error::NothingHeard))
        ));
    }
}
