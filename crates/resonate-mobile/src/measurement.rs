use std::fmt;

use resonate_eq::{BuiltIn, Fitting, LARGEST_MEASUREMENT, Response, fit, read_response};

use crate::{
    Error, Result,
    equaliser::{EqualiserImport, imported},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredFile {
    Measurement,
    Target,
}

impl fmt::Display for MeasuredFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Measurement => "measurement",
            Self::Target => "target",
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

#[uniffi::export]
pub fn fit_measurement(
    measurement: Vec<u8>,
    target: MeasurementTarget,
    name: String,
) -> Result<MeasurementFit> {
    let reading = read_response(&measurement)
        .map_err(|source| Error::measurement(MeasuredFile::Measurement, source))?;
    let goal = target.response()?;

    let fitted = fit(&reading.response, &goal, &Fitting::default()).map_err(|source| {
        let file = out_of_reach(&reading.response, &source);
        Error::measurement(file, source)
    })?;

    let mut loaded = imported(&fitted.profile, name);
    loaded.passed_over = u32::try_from(reading.passed_over).unwrap_or(u32::MAX);

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
}
