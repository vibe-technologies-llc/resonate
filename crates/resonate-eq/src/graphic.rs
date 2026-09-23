use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};

use crate::{
    EqOp, Error, Result,
    apo::{LARGEST_PROFILE, read_number},
};

const LEADER: &str = "graphiceq:";
const POINTS_AT_LEAST: usize = 2;
const POINTS_AT_MOST: usize = 1_024;
const FITTING_PASSES: usize = 12;
const WORTH_A_BAND_MILLIBELS: i32 = 200;
const FITTED_AT: SampleRate = SampleRate::HZ_48000;

pub const THIRD_OCTAVE_CENTRES: [u32; 31] = [
    2_000, 2_500, 3_150, 4_000, 5_000, 6_300, 8_000, 10_000, 12_500, 16_000, 20_000, 25_000,
    31_500, 40_000, 50_000, 63_000, 80_000, 100_000, 125_000, 160_000, 200_000, 250_000, 315_000,
    400_000, 500_000, 630_000, 800_000, 1_000_000, 1_250_000, 1_600_000, 2_000_000,
];

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    points: Vec<(f64, f64)>,
}

impl Curve {
    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }

    pub fn at(&self, hertz: f64) -> f64 {
        let Some(first) = self.points.first() else {
            return 0.0;
        };
        let last = self.points.last().unwrap_or(first);
        if hertz <= first.0 {
            return first.1;
        }
        if hertz >= last.0 {
            return last.1;
        }

        for pair in self.points.windows(2) {
            let (below, above) = (pair[0], pair[1]);
            if hertz >= below.0 && hertz <= above.0 {
                if above.0 <= below.0 {
                    return above.1;
                }
                let along = (hertz / below.0).ln() / (above.0 / below.0).ln();
                return below.1 + along * (above.1 - below.1);
            }
        }
        last.1
    }
}

pub fn read(text: &str) -> Result<Option<Curve>> {
    if text.len() > LARGEST_PROFILE {
        return Err(Error::TooLarge {
            op: EqOp::Parse,
            limit: LARGEST_PROFILE,
        });
    }

    let Some(line) = text
        .lines()
        .map(|line| line.trim_start_matches('\u{feff}').trim())
        .find(|line| line.to_ascii_lowercase().starts_with(LEADER))
    else {
        return Ok(None);
    };
    let Some((_, tail)) = line.split_once(':') else {
        return Ok(None);
    };

    let mut points = Vec::new();
    for spelled in tail.split(';') {
        let spelled = spelled.trim();
        if spelled.is_empty() {
            continue;
        }
        let mut words = spelled.split_whitespace();
        match (
            words.next().and_then(read_number),
            words.next().and_then(read_number),
        ) {
            (Some(hertz), Some(decibels)) if hertz > 0.0 => points.push((hertz, decibels)),
            _ => tracing::debug!(spelled, "a graphic point this build could not read"),
        }
        if points.len() >= POINTS_AT_MOST {
            break;
        }
    }

    points.sort_by(|one, other| one.0.total_cmp(&other.0));
    points.dedup_by(|one, other| one.0 == other.0);

    if points.len() < POINTS_AT_LEAST {
        return Ok(None);
    }
    Ok(Some(Curve { points }))
}

pub fn read_curve(text: &str) -> Result<Option<Curve>> {
    read(text)
}

fn centres() -> Vec<Frequency> {
    THIRD_OCTAVE_CENTRES
        .into_iter()
        .filter_map(|centihertz| Frequency::from_centihertz(centihertz).ok())
        .collect()
}

fn bank(centres: &[Frequency], gains: &[f64]) -> Profile {
    let bands = centres
        .iter()
        .zip(gains)
        .map(|(centre, gain)| {
            Band::new(
                BandKind::Peaking,
                *centre,
                BandGain::from_decibels(*gain).unwrap_or(BandGain::FLAT),
                Q::THIRD_OCTAVE,
            )
        })
        .collect();
    Profile::new(Preamp::NONE, bands).unwrap_or_else(|_| Profile::flat())
}

fn fitted(centres: &[Frequency], wanted: &[f64]) -> Vec<f64> {
    let mut gains = wanted.to_vec();
    for _ in 0..FITTING_PASSES {
        let realised = bank(centres, &gains);
        for (at, centre) in centres.iter().enumerate() {
            let Some(gain) = gains.get_mut(at) else {
                continue;
            };
            let target = wanted.get(at).copied().unwrap_or_default();
            *gain -= realised.magnitude_db(centre.hertz(), FITTED_AT) - target;
        }
    }
    gains
}

pub fn bands_fitted_to(curve: &Curve) -> Profile {
    let mut centres = centres();
    let mut wanted: Vec<f64> = centres.iter().map(|at| curve.at(at.hertz())).collect();
    let mut gains = fitted(&centres, &wanted);

    let worth_it: Vec<bool> = gains
        .iter()
        .map(|gain| {
            BandGain::from_decibels(*gain)
                .is_ok_and(|held| held.millibels().abs() >= WORTH_A_BAND_MILLIBELS)
        })
        .collect();

    if worth_it.iter().any(|kept| !kept) {
        let mut at = 0;
        centres.retain(|_| {
            let kept = worth_it.get(at).copied().unwrap_or_default();
            at += 1;
            kept
        });
        wanted = centres.iter().map(|at| curve.at(at.hertz())).collect();
        gains = fitted(&centres, &wanted);
    }

    let mut profile = bank(&centres, &gains);
    profile.set_preamp(profile.fitted_preamp(FITTED_AT));
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTOEQ: &str = include_str!("../tests/fixtures/autoeq_graphic.txt");
    const FITS_WITHIN_DB: f64 = 0.3;

    #[test]
    fn the_third_octave_centres_are_the_iso_ones_from_twenty_hertz_up() {
        assert_eq!(THIRD_OCTAVE_CENTRES.len(), 31);
        assert_eq!(THIRD_OCTAVE_CENTRES.first(), Some(&2_000));
        assert_eq!(THIRD_OCTAVE_CENTRES.last(), Some(&2_000_000));
        assert!(
            THIRD_OCTAVE_CENTRES
                .windows(2)
                .all(|pair| pair[1] > pair[0])
        );
        assert!((Q::THIRD_OCTAVE.units() - 4.3185).abs() < 0.001);
    }

    #[test]
    fn a_measured_curve_reads_back_as_the_points_it_names() {
        let curve = read(AUTOEQ)
            .expect("a well formed curve")
            .expect("the line is there");

        assert_eq!(curve.points().len(), 127);
        let first = curve.points().first().copied().expect("a first point");
        assert!((first.0 - 20.0).abs() < 1e-9);
        assert!((curve.at(20.0) - first.1).abs() < 1e-9);
    }

    #[test]
    fn a_point_between_two_is_read_along_the_log_of_the_frequency() {
        let curve = read("GraphicEQ: 100 0; 400 12")
            .expect("it reads")
            .expect("a curve");

        assert!((curve.at(200.0) - 6.0).abs() < 1e-9);
        assert!((curve.at(100.0) - 0.0).abs() < 1e-9);
        assert!((curve.at(400.0) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn past_either_end_the_nearest_point_is_held_rather_than_extrapolated() {
        let curve = read("GraphicEQ: 100 -3; 400 12")
            .expect("it reads")
            .expect("a curve");

        assert!((curve.at(1.0) + 3.0).abs() < 1e-9);
        assert!((curve.at(40_000.0) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_measured_curve_fits_onto_third_octave_bands_within_a_stated_tolerance() {
        let curve = read(AUTOEQ)
            .expect("a well formed curve")
            .expect("the line is there");
        let fitted = bands_fitted_to(&curve);

        assert!(!fitted.bands().is_empty());
        assert!(fitted.bands().len() <= THIRD_OCTAVE_CENTRES.len());

        for centre in fitted.bands().iter().map(|band| band.frequency.hertz()) {
            let wanted = curve.at(centre);
            let realised = fitted.magnitude_db(centre, FITTED_AT) - fitted.preamp().decibels();
            assert!(
                (realised - wanted).abs() < FITS_WITHIN_DB,
                "{centre} Hz wanted {wanted} dB and realised {realised} dB"
            );
        }
    }

    #[test]
    fn a_fitted_curve_is_held_at_or_below_full_scale_by_its_own_preamp() {
        let curve = read("GraphicEQ: 20 12; 200 12; 2000 0; 20000 0")
            .expect("it reads")
            .expect("a curve");
        let fitted = bands_fitted_to(&curve);

        assert!(fitted.preamp().decibels() <= -11.0, "{}", fitted.preamp());
        for point in resonate_core::eq::sweep(128) {
            assert!(fitted.magnitude_db(point, FITTED_AT) <= 0.05);
        }
    }

    #[test]
    fn a_flat_curve_spends_no_bands_on_nothing() {
        let curve = read("GraphicEQ: 20 0; 1000 0; 20000 0")
            .expect("it reads")
            .expect("a curve");

        assert!(bands_fitted_to(&curve).bands().is_empty());
    }

    #[test]
    fn a_line_that_is_not_a_graphic_equaliser_answers_with_nothing() {
        assert_eq!(read("Preamp: -6.1 dB").expect("it reads"), None);
        assert_eq!(read("GraphicEQ: 100 0").expect("it reads"), None);
        assert_eq!(read("").expect("it reads"), None);
    }

    #[test]
    fn a_point_the_grammar_cannot_read_is_passed_over_rather_than_failing() {
        let curve = read("GraphicEQ: 100 0; wat; 400 12; 500")
            .expect("it reads")
            .expect("a curve");

        assert_eq!(curve.points().len(), 2);
    }
}
