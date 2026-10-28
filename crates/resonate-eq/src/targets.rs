use crate::{
    Result,
    measurement::{Response, read_response},
};

const HARMAN_OVER_EAR_2018: &[u8] = include_bytes!("../targets/harman-over-ear-2018.csv");
const HARMAN_IN_EAR_2019: &[u8] = include_bytes!("../targets/harman-in-ear-2019.csv");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    Flat,
    HarmanOverEar2018,
    HarmanInEar2019,
}

impl BuiltIn {
    pub const ALL: [Self; 3] = [Self::Flat, Self::HarmanOverEar2018, Self::HarmanInEar2019];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::HarmanOverEar2018 => "harman-over-ear-2018",
            Self::HarmanInEar2019 => "harman-in-ear-2019",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::HarmanOverEar2018 => "Harman over-ear 2018",
            Self::HarmanInEar2019 => "Harman in-ear 2019",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let wanted = text.trim();
        Self::ALL
            .into_iter()
            .find(|target| target.name().eq_ignore_ascii_case(wanted))
    }

    pub fn response(self) -> Result<Response> {
        let bytes = match self {
            Self::Flat => return Ok(Response::flat()),
            Self::HarmanOverEar2018 => HARMAN_OVER_EAR_2018,
            Self::HarmanInEar2019 => HARMAN_IN_EAR_2019,
        };
        Ok(read_response(bytes)?.response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_target_reads_and_is_named_by_the_word_that_asks_for_it() {
        for target in BuiltIn::ALL {
            let response = target.response().expect("a readable target");

            assert!(response.points().len() >= 2, "{target:?}");
            assert_eq!(BuiltIn::parse(target.name()), Some(target));
            assert_eq!(BuiltIn::parse(&target.name().to_uppercase()), Some(target));
        }
        assert_eq!(BuiltIn::parse("a/file.csv"), None);
    }

    #[test]
    fn the_flat_target_is_nought_decibels_everywhere() {
        let flat = BuiltIn::Flat.response().expect("flat");

        for hertz in [5.0, 20.0, 1_000.0, 20_000.0, 90_000.0] {
            assert_eq!(flat.at(hertz), 0.0);
        }
    }

    #[test]
    fn the_harman_targets_sit_at_nought_decibels_at_one_kilohertz_and_rise_to_the_ear_gain() {
        for (target, ear_gain_db, bass_db) in [
            (BuiltIn::HarmanOverEar2018, 8.3, 3.9),
            (BuiltIn::HarmanInEar2019, 9.3, 7.6),
        ] {
            let response = target.response().expect("a readable target");

            assert!(response.at(1_000.0).abs() < 0.05, "{target:?}");
            assert!(
                (response.at(3_000.0) - ear_gain_db).abs() < 0.2,
                "{target:?}"
            );
            assert!((response.at(20.0) - bass_db).abs() < 0.2, "{target:?}");
            assert!(response.at(20_000.0) < -15.0, "{target:?}");
        }
    }

    #[test]
    fn the_two_harman_targets_differ_where_over_ear_and_in_ear_listening_differ() {
        let over = BuiltIn::HarmanOverEar2018.response().expect("target");
        let inside = BuiltIn::HarmanInEar2019.response().expect("target");

        assert!(inside.at(20.0) > over.at(20.0) + 3.0);
        assert!(inside.at(3_000.0) > over.at(3_000.0) + 0.5);
    }
}
