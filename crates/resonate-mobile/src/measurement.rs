use std::fmt;

use resonate_core::SampleRate;
use resonate_dsp::Sweep;
use resonate_eq::{BuiltIn, Fitting, LARGEST_MEASUREMENT, Response, fit, read_response};

use crate::{
    Error, Result,
    equaliser::{EqualiserImport, imported},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredFile {
    Measurement,
    Target,
    Calibration,
}

impl fmt::Display for MeasuredFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Measurement => "measurement",
            Self::Target => "target",
            Self::Calibration => "calibration",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BuiltInTarget {
    pub name: String,
    pub label: String,
    pub recommended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MeasurementOffers {
    pub targets: Vec<BuiltInTarget>,
    pub bytes_at_most: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementTarget {
    BuiltIn { name: String },
    Document { bytes: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementSweep {
    pub rate: u32,
    pub samples: Vec<f32>,
    pub recorded_frames: u64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementFit {
    pub imported: EqualiserImport,
    pub bands: u32,
    pub off_by_db_before: f32,
    pub off_by_db_after: f32,
}

const RECOMMENDED: BuiltIn = BuiltIn::HarmanOverEar2018;

impl MeasurementTarget {
    fn response(&self) -> Result<Response> {
        match self {
            Self::BuiltIn { name } => BuiltIn::parse(name)
                .ok_or_else(|| Error::UnknownTarget { name: name.clone() })?
                .response()
                .map_err(|source| Error::measurement(MeasuredFile::Target, source)),
            Self::Document { bytes } => read_response(bytes)
                .map(|reading| reading.response)
                .map_err(|source| Error::measurement(MeasuredFile::Target, source)),
        }
    }
}

fn out_of_reach(measured: &Response, source: &resonate_eq::Error) -> MeasuredFile {
    match source {
        resonate_eq::Error::ResponseMissesTheMidBand {
            lowest_hertz,
            highest_hertz,
        } if *lowest_hertz != measured.lowest_hertz()
            || *highest_hertz != measured.highest_hertz() =>
        {
            MeasuredFile::Target
        }
        _ => MeasuredFile::Measurement,
    }
}

#[uniffi::export]
pub fn measurement_offers() -> MeasurementOffers {
    MeasurementOffers {
        targets: BuiltIn::ALL
            .into_iter()
            .map(|target| BuiltInTarget {
                name: target.name().to_owned(),
                label: target.label().to_owned(),
                recommended: target == RECOMMENDED,
            })
            .collect(),
        bytes_at_most: LARGEST_MEASUREMENT as u64,
    }
}

const MEASURED_AT_RATES: [u32; 2] = [44_100, 48_000];
const MEASURED_AT_OTHERWISE: u32 = 48_000;

#[uniffi::export]
pub fn measurement_rate_for(output_rate: u32) -> u32 {
    if MEASURED_AT_RATES.contains(&output_rate) {
        output_rate
    } else {
        MEASURED_AT_OTHERWISE
    }
}

#[uniffi::export]
pub fn measurement_sweep(rate: u32) -> Result<MeasurementSweep> {
    let sweep = sweep_at(rate)?;
    let recorded_frames = (sweep.heard_for().as_secs_f64() * f64::from(rate)).round() as u64;

    Ok(MeasurementSweep {
        rate,
        samples: sweep.samples().to_vec(),
        recorded_frames,
    })
}

#[uniffi::export]
pub fn fit_measurement(
    measurement: Vec<u8>,
    target: MeasurementTarget,
    name: String,
) -> Result<MeasurementFit> {
    let reading = read_response(&measurement)
        .map_err(|source| Error::measurement(MeasuredFile::Measurement, source))?;

    fitted(&reading.response, reading.passed_over, &target, name)
}

#[uniffi::export]
pub fn fit_recorded_sweep(
    recorded: Vec<f32>,
    rate: u32,
    calibration: Option<Vec<u8>>,
    target: MeasurementTarget,
    name: String,
) -> Result<MeasurementFit> {
    let sweep = sweep_at(rate)?;
    let levels = sweep
        .heard_as(&recorded)
        .and_then(|heard| heard.response())
        .map_err(|source| Error::Sweep { source })?;
    let heard = Response::from_levels(
        levels
            .into_iter()
            .map(|level| (level.hertz, level.decibels)),
    )
    .map_err(|source| Error::measurement(MeasuredFile::Measurement, source))?;

    let measured = match calibration {
        None => heard,
        Some(bytes) => {
            let microphone = read_response(&bytes)
                .map_err(|source| Error::measurement(MeasuredFile::Calibration, source))?
                .response;
            heard
                .calibrated_by(&microphone)
                .map_err(|source| Error::measurement(MeasuredFile::Measurement, source))?
        }
    };

    fitted(&measured, 0, &target, name)
}

fn sweep_at(rate: u32) -> Result<Sweep> {
    Sweep::for_measuring(SampleRate::new(rate)?).map_err(|source| Error::Sweep { source })
}

fn fitted(
    measured: &Response,
    passed_over: usize,
    target: &MeasurementTarget,
    name: String,
) -> Result<MeasurementFit> {
    let goal = target.response()?;

    let fitted = fit(measured, &goal, &Fitting::default()).map_err(|source| {
        let file = out_of_reach(measured, &source);
        Error::measurement(file, source)
    })?;

    let mut loaded = imported(&fitted.profile, name);
    loaded.passed_over = u32::try_from(passed_over).unwrap_or(u32::MAX);

    Ok(MeasurementFit {
        bands: u32::try_from(loaded.curve.bands.len()).unwrap_or(u32::MAX),
        imported: loaded,
        off_by_db_before: fitted.off_by_db_before as f32,
        off_by_db_after: fitted.off_by_db_after as f32,
    })
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::MAX_BANDS;
    use resonate_eq::Store;

    use super::*;

    const HARMAN_OVER_EAR: &[u8] =
        include_bytes!("../../resonate-eq/targets/harman-over-ear-2018.csv");

    fn dipped_at_three_kilohertz() -> Vec<u8> {
        (0..120)
            .map(|step| {
                let hertz = 20.0 * 1.05_f64.powi(step);
                let octaves = (hertz / 3_000.0).log2();
                let level = -6.0 * (-octaves * octaves * 4.0).exp();
                format!("{hertz:.2},{level:.2}\n")
            })
            .fold(String::from("frequency,raw\n"), |text, line| text + &line)
            .into_bytes()
    }

    fn built_in(name: &str) -> MeasurementTarget {
        MeasurementTarget::BuiltIn { name: name.into() }
    }

    #[test]
    fn a_measurement_is_fitted_into_bands_the_phones_equaliser_takes() {
        let fit = fit_measurement(dipped_at_three_kilohertz(), built_in("flat"), "Mine".into())
            .expect("a fit");

        assert_eq!(fit.imported.curve.name, "Mine");
        assert_eq!(fit.bands as usize, fit.imported.curve.bands.len());
        assert!(fit.bands > 0 && fit.bands as usize <= MAX_BANDS);
        assert!(fit.off_by_db_after < fit.off_by_db_before);
        assert!(!fit.imported.converted);
        assert_eq!(fit.imported.clamped, 0);
        assert!(fit.imported.curve.profile().is_ok());
        assert!(
            fit.imported
                .curve
                .bands
                .iter()
                .any(|band| band.gain_db > 2.0 && (2_000.0..4_500.0).contains(&band.frequency_hz)),
            "the dip is lifted where it lies"
        );
    }

    #[test]
    fn the_phones_fit_is_the_desktops_fit_of_the_same_bytes() {
        let bytes = dipped_at_three_kilohertz();
        let measured = read_response(&bytes).expect("a response").response;
        let goal = Store::goal_named("harman-in-ear-2019").expect("a built-in");

        let desktop = fit(&measured, &goal, &Fitting::default()).expect("a fit");
        let phone =
            fit_measurement(bytes, built_in("Harman-In-Ear-2019"), String::new()).expect("a fit");

        assert_eq!(phone.imported, imported(&desktop.profile, String::new()));
        assert_eq!(phone.off_by_db_before, desktop.off_by_db_before as f32);
        assert_eq!(phone.off_by_db_after, desktop.off_by_db_after as f32);
    }

    #[test]
    fn a_target_document_fits_as_the_built_in_it_holds_would() {
        let bytes = dipped_at_three_kilohertz();

        let named = fit_measurement(
            bytes.clone(),
            built_in("harman-over-ear-2018"),
            String::new(),
        )
        .expect("a fit");
        let document = fit_measurement(
            bytes,
            MeasurementTarget::Document {
                bytes: HARMAN_OVER_EAR.to_vec(),
            },
            String::new(),
        )
        .expect("a fit");

        assert_eq!(named, document);
    }

    #[test]
    fn the_lines_of_a_measurement_that_are_no_point_are_counted() {
        let mut bytes = b"* Measurement data\n* Dated: today\nstray words here\n".to_vec();
        bytes.extend(dipped_at_three_kilohertz().split_off("frequency,raw\n".len()));

        let fit = fit_measurement(bytes, built_in("flat"), String::new()).expect("a fit");

        assert_eq!(fit.imported.passed_over, 1);
    }

    #[test]
    fn the_offers_name_the_three_built_ins_and_recommend_one_the_desktops_default() {
        let offers = measurement_offers();
        let names: Vec<&str> = offers
            .targets
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        let labels: Vec<&str> = offers
            .targets
            .iter()
            .map(|target| target.label.as_str())
            .collect();
        let recommended: Vec<&str> = offers
            .targets
            .iter()
            .filter(|target| target.recommended)
            .map(|target| target.name.as_str())
            .collect();

        assert_eq!(
            names,
            ["flat", "harman-over-ear-2018", "harman-in-ear-2019"]
        );
        assert_eq!(
            labels,
            ["Flat", "Harman over-ear 2018", "Harman in-ear 2019"]
        );
        assert_eq!(recommended, ["harman-over-ear-2018"]);
        assert_eq!(offers.bytes_at_most, LARGEST_MEASUREMENT as u64);
        for name in names {
            assert!(
                fit_measurement(dipped_at_three_kilohertz(), built_in(name), String::new()).is_ok(),
                "{name} fits"
            );
        }
    }

    #[test]
    fn a_target_nobody_built_in_is_called_is_refused_by_name() {
        let refused = fit_measurement(
            dipped_at_three_kilohertz(),
            built_in("a/file.csv"),
            String::new(),
        );

        match refused {
            Err(error @ Error::UnknownTarget { .. }) => {
                assert_eq!(error.to_string(), "No built-in target is called a/file.csv");
            }
            other => panic!("expected an unknown target, got {other:?}"),
        }
    }

    #[test]
    fn a_file_that_will_not_read_is_refused_in_a_sentence_naming_which_file() {
        let told = |measurement: Vec<u8>, target| {
            fit_measurement(measurement, target, String::new())
                .expect_err("a refusal")
                .to_string()
        };
        let document = |bytes: &[u8]| MeasurementTarget::Document {
            bytes: bytes.to_vec(),
        };
        let narrow: String = (0..20)
            .map(|step| format!("{},0\n", 5_000 + step * 100))
            .collect();

        assert_eq!(
            told(vec![b'1'; LARGEST_MEASUREMENT + 1], built_in("flat")),
            "The measurement file is larger than 4 MiB"
        );
        assert_eq!(
            told(b"nothing here\n".to_vec(), built_in("flat")),
            "No line of the measurement file reads as a frequency and a level"
        );
        assert_eq!(
            told(b"100 0\n200 1\n".to_vec(), built_in("flat")),
            "The measurement file holds 2 points and a response needs at least 8"
        );
        assert_eq!(
            told(dipped_at_three_kilohertz(), document(b"words\n")),
            "No line of the target file reads as a frequency and a level"
        );
        assert_eq!(
            told(dipped_at_three_kilohertz(), document(narrow.as_bytes())),
            "The target spans 5000 Hz to 6900 Hz and must reach 500 Hz to 2 kHz"
        );
        assert_eq!(
            told(narrow.clone().into_bytes(), built_in("flat")),
            "The measurement spans 5000 Hz to 6900 Hz and must reach 500 Hz to 2 kHz"
        );
    }

    #[test]
    fn a_measurement_already_on_target_says_there_is_nothing_to_correct() {
        let flat: String = (0..60)
            .map(|step| format!("{:.2} 0.00\n", 20.0 * 1.1_f64.powi(step)))
            .collect();

        let told = fit_measurement(flat.into_bytes(), built_in("flat"), String::new())
            .expect_err("nothing to correct")
            .to_string();

        assert_eq!(
            told,
            "The measurement is within 0.0 dB of the target, so there is nothing to correct"
        );
    }

    const SMOOTHING: f64 = 0.3;
    const DELAY_FRAMES: usize = 211;

    fn through_a_lowpass(rate: u32) -> (Vec<f32>, MeasurementSweep) {
        let sweep = measurement_sweep(rate).expect("a sweep");
        let total = sweep.recorded_frames as usize;
        let mut held = 0.0_f64;
        let mut heard = vec![0.0_f32; DELAY_FRAMES];
        for at in 0..total - DELAY_FRAMES {
            let sample = sweep.samples.get(at).copied().unwrap_or(0.0);
            held = SMOOTHING * f64::from(sample) + (1.0 - SMOOTHING) * held;
            heard.push(held as f32);
        }
        (heard, sweep)
    }

    fn lowpass_db(hertz: f64, rate: u32) -> f64 {
        let turn = 2.0 * std::f64::consts::PI * hertz / f64::from(rate);
        let real = 1.0 - (1.0 - SMOOTHING) * turn.cos();
        let imaginary = (1.0 - SMOOTHING) * turn.sin();
        20.0 * (SMOOTHING / real.hypot(imaginary)).log10()
    }

    fn the_filter_as_a_calibration(rate: u32) -> Vec<u8> {
        (0..150)
            .map(|step| {
                let hertz = 20.0 * 1.05_f64.powi(step);
                format!("{hertz:.2},{:.3}\n", lowpass_db(hertz, rate))
            })
            .fold(String::from("frequency,raw\n"), |text, line| text + &line)
            .into_bytes()
    }

    #[test]
    fn a_sweep_is_measured_at_the_outputs_rate_where_it_is_one_a_microphone_takes_and_at_48_khz_else()
     {
        assert_eq!(measurement_rate_for(44_100), 44_100);
        assert_eq!(measurement_rate_for(48_000), 48_000);
        assert_eq!(measurement_rate_for(96_000), 48_000);
        assert_eq!(measurement_rate_for(0), 48_000);
        assert!(measurement_sweep(measurement_rate_for(22_050)).is_ok());
    }

    #[test]
    fn a_sweep_is_handed_out_at_the_rate_asked_and_says_how_long_to_record() {
        for rate in [44_100, 48_000, 96_000] {
            let sweep = measurement_sweep(rate).expect("a sweep");

            assert_eq!(sweep.rate, rate);
            assert_eq!(sweep.samples.len() as u64, u64::from(rate) * 8);
            assert_eq!(sweep.recorded_frames, u64::from(rate) * 10);
            assert!(
                sweep
                    .samples
                    .iter()
                    .all(|sample| sample.abs() <= 0.1 + 1e-6)
            );
        }
    }

    #[test]
    fn a_sweep_through_a_known_filter_is_fitted_as_the_desktops_measurement_is() {
        for rate in [44_100, 48_000] {
            let (heard, sweep) = through_a_lowpass(rate);
            let played =
                Sweep::for_measuring(SampleRate::new(rate).expect("a rate")).expect("a sweep");
            assert_eq!(sweep.samples, played.samples());

            let phone =
                fit_recorded_sweep(heard.clone(), rate, None, built_in("flat"), "Room".into())
                    .expect("a fit");

            let levels = played
                .heard_as(&heard)
                .and_then(|heard| heard.response())
                .expect("a response");
            let measured = Response::from_levels(
                levels
                    .into_iter()
                    .map(|level| (level.hertz, level.decibels)),
            )
            .expect("a response");
            for hertz in [100.0, 1_000.0, 6_000.0, 10_000.0] {
                let off = (measured.at(hertz) - lowpass_db(hertz, rate)).abs();
                assert!(off < 0.5, "{hertz} Hz is {off:.2} dB off at {rate}");
            }
            let desktop = fit(
                &measured,
                &Store::goal_named("flat").expect("a built-in"),
                &Fitting::default(),
            )
            .expect("a fit");

            assert_eq!(phone.imported, imported(&desktop.profile, "Room".into()));
            assert_eq!(phone.off_by_db_before, desktop.off_by_db_before as f32);
            assert!(phone.off_by_db_after < phone.off_by_db_before * 0.5);
            assert!(
                phone
                    .imported
                    .curve
                    .bands
                    .iter()
                    .any(|band| band.gain_db > 2.0 && band.frequency_hz > 2_000.0),
                "the treble the filter took away is lifted"
            );
        }
    }

    #[test]
    fn a_microphone_calibration_is_taken_off_what_it_heard() {
        let (heard, _) = through_a_lowpass(48_000);

        let raw = fit_recorded_sweep(heard.clone(), 48_000, None, built_in("flat"), String::new())
            .expect("a fit");
        let calibrated = fit_recorded_sweep(
            heard,
            48_000,
            Some(the_filter_as_a_calibration(48_000)),
            built_in("flat"),
            String::new(),
        )
        .expect("a fit");

        assert!(raw.off_by_db_before > 1.0, "{}", raw.off_by_db_before);
        assert!(
            calibrated.off_by_db_before < 0.2,
            "{}",
            calibrated.off_by_db_before
        );
    }

    #[test]
    fn a_calibration_that_will_not_read_is_refused_naming_it() {
        let (heard, _) = through_a_lowpass(48_000);

        let told = fit_recorded_sweep(
            heard,
            48_000,
            Some(b"nothing here\n".to_vec()),
            built_in("flat"),
            String::new(),
        )
        .expect_err("a refusal")
        .to_string();

        assert_eq!(
            told,
            "No line of the calibration file reads as a frequency and a level"
        );
    }

    #[test]
    fn a_recording_that_heard_nothing_or_too_little_or_a_rate_too_slow_is_told_in_words() {
        let told = |recorded: Vec<f32>, rate: u32| {
            fit_recorded_sweep(recorded, rate, None, built_in("flat"), String::new())
                .expect_err("a refusal")
                .to_string()
        };

        assert_eq!(
            told(vec![0.0; 480_000], 48_000),
            "Nothing of the sweep was heard: raise the volume or bring the microphone nearer"
        );
        assert_eq!(
            told(vec![0.0; 1_000], 48_000),
            "The recording held 1000 frames and the sweep alone was 384000, so it was cut short"
        );
        assert!(told(vec![0.0; 480_000], 32_000).starts_with(
            "A sweep up to 20 kHz needs a rate above 40 kHz and this one runs at 32 kHz"
        ));
        assert!(measurement_sweep(32_000).is_err());
    }
}
