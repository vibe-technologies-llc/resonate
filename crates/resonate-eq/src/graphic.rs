use resonate_core::eq::{BandGain, Frequency, Target, TargetPoint};

use crate::apo::read_number;

pub const LEADER: &str = "graphiceq:";

fn clamped_point(hertz: f64, decibels: f64) -> Option<TargetPoint> {
    let frequency = Frequency::from_hertz(hertz).ok()?;
    let widest = f64::from(BandGain::WIDEST_MILLIBELS) / 1_000.0;
    let gain = BandGain::from_decibels(decibels.clamp(-widest, widest)).ok()?;
    Some(TargetPoint { frequency, gain })
}

pub fn target_of(line: &str) -> Option<Target> {
    let (_, tail) = line.split_once(':')?;

    let mut points = Vec::new();
    for spelled in tail.split(';') {
        let spelled = spelled.trim();
        if spelled.is_empty() {
            continue;
        }
        let mut words = spelled.split_whitespace();
        let read = match (
            words.next().and_then(read_number),
            words.next().and_then(read_number),
        ) {
            (Some(hertz), Some(decibels)) => clamped_point(hertz, decibels),
            _ => None,
        };
        match read {
            Some(point) => points.push(point),
            None => tracing::debug!(spelled, "a graphic point this build could not read"),
        }
        if points.len() >= resonate_core::eq::TARGET_POINTS_AT_MOST {
            break;
        }
    }

    Target::new(points)
}

pub fn spelled(target: &Target) -> String {
    let points: Vec<String> = target
        .points()
        .iter()
        .map(|point| {
            format!(
                "{} {}",
                crate::apo::spelled_hertz(point.frequency),
                spelled_decibels(point.gain)
            )
        })
        .collect();
    format!("GraphicEQ: {}", points.join("; "))
}

fn spelled_decibels(gain: BandGain) -> String {
    let written = format!("{:.3}", gain.decibels());
    let trimmed = written.trim_end_matches('0').trim_end_matches('.');
    match trimmed {
        "-0" | "" => "0".to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{
        SampleRate,
        eq::{FITTED_AT, Profile, Q, THIRD_OCTAVE_CENTRES, sweep},
    };

    use super::*;

    const AUTOEQ: &str = include_str!("../tests/fixtures/autoeq_graphic.txt");
    const FITS_WITHIN_DB: f64 = 0.3;
    const EVERY_RATE_PLAYED: [SampleRate; 8] = [
        SampleRate::HZ_44100,
        SampleRate::HZ_48000,
        SampleRate::HZ_88200,
        SampleRate::HZ_96000,
        SampleRate::HZ_176400,
        SampleRate::HZ_192000,
        SampleRate::HZ_352800,
        SampleRate::HZ_384000,
    ];

    fn line_of(text: &str) -> &str {
        text.lines()
            .find(|line| line.to_ascii_lowercase().starts_with(LEADER))
            .expect("a graphic line")
    }

    fn target(text: &str) -> Target {
        target_of(line_of(text)).expect("a curve")
    }

    fn worst_centre_db(profile: &Profile, target: &Target, rate: SampleRate) -> f64 {
        THIRD_OCTAVE_CENTRES
            .iter()
            .map(|centihertz| f64::from(*centihertz) / 100.0)
            .map(|centre| {
                let realised = profile.magnitude_db(centre, rate) - profile.preamp().decibels();
                (realised - target.at(centre)).abs()
            })
            .fold(0.0, f64::max)
    }

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
        let curve = target(AUTOEQ);

        assert_eq!(curve.points().len(), 127);
        let first = curve.points().first().copied().expect("a first point");
        assert_eq!(first.frequency.centihertz(), 2_000);
        assert!((curve.at(20.0) - first.gain.decibels()).abs() < 1e-9);
    }

    #[test]
    fn a_point_between_two_is_read_along_the_log_of_the_frequency() {
        let curve = target("GraphicEQ: 100 0; 400 12");

        assert!((curve.at(200.0) - 6.0).abs() < 1e-9);
        assert!((curve.at(100.0) - 0.0).abs() < 1e-9);
        assert!((curve.at(400.0) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn past_either_end_the_nearest_point_is_held_rather_than_extrapolated() {
        let curve = target("GraphicEQ: 100 -3; 400 12");

        assert!((curve.at(1.0) + 3.0).abs() < 1e-9);
        assert!((curve.at(40_000.0) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_measured_curve_fits_onto_third_octave_bands_within_a_stated_tolerance() {
        let curve = target(AUTOEQ);
        let fitted = Profile::fitted_to(curve.clone());

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
    fn a_curve_fitted_again_at_the_rate_it_plays_at_holds_its_shape_at_every_rate() {
        let curve = target(AUTOEQ);
        let kept = std::sync::Arc::new(Profile::fitted_to(curve.clone()));

        for rate in EVERY_RATE_PLAYED {
            let played = kept.at_rate(rate);
            let worst = worst_centre_db(&played, &curve, rate);
            assert!(worst < FITS_WITHIN_DB, "{worst} dB off at {rate}");
        }

        let drifted = worst_centre_db(&kept, &curve, SampleRate::HZ_96000);
        assert!(
            drifted > 1.0,
            "the 48 kHz fit played at 96 kHz drifts only {drifted} dB, so refitting proves nothing"
        );
    }

    #[test]
    fn a_preamp_moved_by_hand_rides_along_when_the_curve_is_fitted_again() {
        let mut moved = Profile::fitted_to(target(AUTOEQ));
        let fitted = moved.preamp();
        moved.set_preamp(
            resonate_core::eq::Preamp::from_millibels(fitted.millibels() - 3_000)
                .expect("in range"),
        );
        let moved = std::sync::Arc::new(moved);

        for rate in EVERY_RATE_PLAYED {
            let played = moved.at_rate(rate);
            assert_eq!(
                played.preamp().millibels(),
                played.fitted_preamp(rate).millibels() - 3_000,
                "{rate}"
            );
            assert!(played.target().is_some());
        }
    }

    #[test]
    fn a_fitted_curve_is_held_at_or_below_full_scale_by_its_own_preamp() {
        let curve = target("GraphicEQ: 20 12; 200 12; 2000 0; 20000 0");
        let fitted = std::sync::Arc::new(Profile::fitted_to(curve));

        assert!(fitted.preamp().decibels() <= -11.0, "{}", fitted.preamp());
        for rate in EVERY_RATE_PLAYED {
            let played = fitted.at_rate(rate);
            for point in sweep(128) {
                assert!(
                    played.magnitude_db(point, rate) <= 0.05,
                    "{point} Hz at {rate}"
                );
            }
        }
    }

    #[test]
    fn a_flat_curve_spends_no_bands_on_nothing() {
        let fitted = Profile::fitted_to(target("GraphicEQ: 20 0; 1000 0; 20000 0"));

        assert!(fitted.bands().is_empty());
        assert!(fitted.is_transparent());
    }

    #[test]
    fn a_shaped_band_lets_go_of_the_curve_it_was_fitted_to() {
        let mut edited = Profile::fitted_to(target(AUTOEQ));
        assert!(edited.target().is_some());

        if let Some(band) = edited.band_mut(0) {
            band.on = false;
        }
        assert_eq!(edited.target(), None);
    }

    #[test]
    fn a_line_that_is_not_a_graphic_equaliser_answers_with_nothing() {
        assert_eq!(target_of("GraphicEQ: 100 0"), None);
        assert_eq!(target_of("GraphicEQ:"), None);
        assert_eq!(target_of(""), None);
    }

    #[test]
    fn a_point_the_grammar_cannot_read_is_passed_over_rather_than_failing() {
        let curve = target("GraphicEQ: 100 0; wat; 400 12; 500");

        assert_eq!(curve.points().len(), 2);
    }

    #[test]
    fn a_curve_is_written_as_the_line_it_was_read_from() {
        let curve = target(AUTOEQ);

        assert_eq!(target_of(&spelled(&curve)), Some(curve));
        assert_eq!(
            spelled(&target("GraphicEQ: 20 -0.0; 105.5 3.25; 1000 12")),
            "GraphicEQ: 20 0; 105.50 3.25; 1000 12"
        );
    }
}
