pub mod chain;
pub mod convolve;
pub mod crossfeed;
pub mod dither;
pub mod eq;
pub mod gain;
pub mod peak;
pub mod remix;
pub mod resample;
pub mod restore;

use resonate_core::BitDepth;

use crate::lab::suite::Suite;

pub fn all() -> Vec<&'static dyn Suite> {
    vec![
        &resample::Resample,
        &dither::Dithering {
            depth: BitDepth::Bits16,
        },
        &dither::Dithering {
            depth: BitDepth::Bits24,
        },
        &eq::Equalising,
        &gain::Gaining,
        &peak::Guarding,
        &remix::Remixing,
        &crossfeed::Crossfeeding,
        &convolve::Convolving,
        &restore::Restoring,
        &chain::Chains,
    ]
}

pub fn named(name: &str) -> Option<&'static dyn Suite> {
    all().into_iter().find(|suite| suite.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_suite_is_named_once_and_found_by_its_name() {
        let names: Vec<&str> = all().iter().map(|suite| suite.name()).collect();

        for name in &names {
            assert_eq!(names.iter().filter(|other| *other == name).count(), 1);
            assert!(named(name).is_some());
        }
    }

    #[test]
    fn every_suite_offers_its_subjects() {
        for suite in all() {
            assert!(
                suite.subjects().is_ok_and(|subjects| !subjects.is_empty()),
                "{}",
                suite.name()
            );
        }
    }
}
