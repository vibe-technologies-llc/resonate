use resonate_core::eq::{
    Band, BandGain, BandKind, FITTED_AT, Frequency, MAX_BANDS, Preamp, Profile, Q, sweep,
};

use crate::{Error, Result, measurement::Response};

const GRID_POINTS: usize = 256;
const SMOOTHED_OVER_OCTAVES: f64 = 1.0 / 6.0;
const NORMALISED_FROM_HZ: f64 = 500.0;
const NORMALISED_TO_HZ: f64 = 2_000.0;
const WEIGHT_FALLS_FROM_HZ: f64 = 6_000.0;
const WEIGHT_AT_THE_TOP: f64 = 0.25;
const TOP_HZ: f64 = 20_000.0;
const ATOMS_FROM_HZ: f64 = 20.0;
const ATOMS_TO_HZ: f64 = 16_000.0;
const ATOMS_PER_OCTAVE: f64 = 6.0;
const LOW_SHELVES_UP_TO_HZ: f64 = 1_000.0;
const HIGH_SHELVES_FROM_HZ: f64 = 1_500.0;
const PEAKING_QS: [f64; 9] = [0.5, 0.7, 1.0, 1.4, 2.0, 2.8, 4.0, 5.6, 8.0];
const SHELF_QS: [f64; 3] = [0.5, 0.7, 1.0];
const UNIT_GAIN_DB: f64 = 4.0;
const WORTH_A_BAND_DB: f64 = 0.3;
const WORTH_A_BAND_OF_THE_ERROR: f64 = 0.01;
const REFINING_PASSES: usize = 3;
const REFINING_STEPS: usize = 2;
const NUDGES_OF_FREQUENCY: [i32; 5] = [-2, -1, 0, 1, 2];
const NUDGES_OF_Q: [f64; 3] = [0.7, 1.0, 1.4];
const NUDGE_OF_FREQUENCY_OCTAVES: f64 = 1.0 / 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitting {
    pub bands: usize,
    pub boost_at_most_db: f64,
    pub cut_at_most_db: f64,
    pub q_least: f64,
    pub q_most: f64,
}

impl Default for Fitting {
    fn default() -> Self {
        Self {
            bands: 10,
            boost_at_most_db: 6.0,
            cut_at_most_db: 12.0,
            q_least: 0.5,
            q_most: 8.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    pub profile: Profile,
    pub off_by_db_before: f64,
    pub off_by_db_after: f64,
}

struct Atom {
    kind: BandKind,
    frequency: Frequency,
    q: Q,
    shape: Vec<f64>,
}

struct Placed {
    band: Band,
    response: Vec<f64>,
}

fn band_at(kind: BandKind, frequency: Frequency, gain_db: f64, q: Q) -> Band {
    Band::new(
        kind,
        frequency,
        BandGain::from_decibels(gain_db).unwrap_or(BandGain::FLAT),
        q,
    )
}

fn response_of(band: Band, grid: &[f64]) -> Vec<f64> {
    let design = band.design(FITTED_AT);
    grid.iter()
        .map(|hertz| design.magnitude_db(*hertz, FITTED_AT))
        .collect()
}

fn weight_at(hertz: f64) -> f64 {
    if hertz <= WEIGHT_FALLS_FROM_HZ {
        return 1.0;
    }
    let along = (hertz / WEIGHT_FALLS_FROM_HZ).log2() / (TOP_HZ / WEIGHT_FALLS_FROM_HZ).log2();
    1.0 - (1.0 - WEIGHT_AT_THE_TOP) * along.min(1.0)
}

fn levels_on(response: &Response, grid: &[f64]) -> Vec<f64> {
    let raw: Vec<f64> = grid
        .iter()
        .map(|hertz| response.smoothed_at(*hertz, SMOOTHED_OVER_OCTAVES / 2.0))
        .collect();
    let mid: Vec<f64> = grid
        .iter()
        .zip(&raw)
        .filter(|(hertz, _)| (NORMALISED_FROM_HZ..=NORMALISED_TO_HZ).contains(*hertz))
        .map(|(_, level)| *level)
        .collect();
    let reference = mid.iter().sum::<f64>() / mid.len().max(1) as f64;
    raw.into_iter().map(|level| level - reference).collect()
}

fn must_reach_the_mid_band(response: &Response) -> Result<()> {
    if response.lowest_hertz() <= NORMALISED_FROM_HZ && response.highest_hertz() >= NORMALISED_TO_HZ
    {
        return Ok(());
    }
    Err(Error::ResponseMissesTheMidBand {
        lowest_hertz: response.lowest_hertz(),
        highest_hertz: response.highest_hertz(),
    })
}

fn atoms_for(fitting: &Fitting, grid: &[f64]) -> Vec<Atom> {
    let frequencies: Vec<Frequency> = (0..)
        .map(|step| ATOMS_FROM_HZ * 2.0_f64.powf(f64::from(step) / ATOMS_PER_OCTAVE))
        .take_while(|hertz| *hertz <= ATOMS_TO_HZ)
        .filter_map(|hertz| Frequency::from_hertz(hertz).ok())
        .collect();

    let mut atoms = Vec::new();
    for frequency in frequencies {
        let hertz = frequency.hertz();
        let kinds = [
            (BandKind::Peaking, &PEAKING_QS[..], true),
            (
                BandKind::LowShelf,
                &SHELF_QS[..],
                hertz <= LOW_SHELVES_UP_TO_HZ,
            ),
            (
                BandKind::HighShelf,
                &SHELF_QS[..],
                hertz >= HIGH_SHELVES_FROM_HZ,
            ),
        ];
        for (kind, qs, wanted) in kinds {
            if !wanted {
                continue;
            }
            for units in qs
                .iter()
                .filter(|units| (fitting.q_least..=fitting.q_most).contains(*units))
            {
                let Ok(q) = Q::from_units(*units) else {
                    continue;
                };
                let unit = band_at(kind, frequency, UNIT_GAIN_DB, q);
                atoms.push(Atom {
                    kind,
                    frequency,
                    q,
                    shape: response_of(unit, grid)
                        .into_iter()
                        .map(|level| level / UNIT_GAIN_DB)
                        .collect(),
                });
            }
        }
    }
    atoms
}

fn weighted_rms(error: &[f64], weights: &[f64]) -> f64 {
    let total: f64 = weights.iter().sum();
    let squared: f64 = error
        .iter()
        .zip(weights)
        .map(|(error, weight)| weight * error * error)
        .sum();
    (squared / total.max(f64::MIN_POSITIVE)).sqrt()
}

fn weighted_sum_of_squares(error: &[f64], weights: &[f64]) -> f64 {
    error
        .iter()
        .zip(weights)
        .map(|(error, weight)| weight * error * error)
        .sum()
}

fn inner(weights: &[f64], a: &[f64], b: &[f64]) -> f64 {
    weights
        .iter()
        .zip(a)
        .zip(b)
        .map(|((weight, a), b)| weight * a * b)
        .sum()
}

struct Taken {
    low_shelf: bool,
    high_shelf: bool,
    places: Vec<(BandKind, Frequency)>,
}

impl Taken {
    fn forbids(&self, atom: &Atom) -> bool {
        match atom.kind {
            BandKind::LowShelf if self.low_shelf => true,
            BandKind::HighShelf if self.high_shelf => true,
            kind => self.places.contains(&(kind, atom.frequency)),
        }
    }

    fn take(&mut self, atom: &Atom) {
        match atom.kind {
            BandKind::LowShelf => self.low_shelf = true,
            BandKind::HighShelf => self.high_shelf = true,
            _ => {}
        }
        self.places.push((atom.kind, atom.frequency));
    }
}

struct Pick<'a> {
    atom: &'a Atom,
    gain_db: f64,
    reduction: f64,
}

fn best_atom<'a>(
    atoms: &'a [Atom],
    residual: &[f64],
    weights: &[f64],
    taken: &Taken,
    fitting: &Fitting,
) -> Option<Pick<'a>> {
    atoms
        .iter()
        .filter(|atom| !taken.forbids(atom))
        .filter_map(|atom| {
            let along = inner(weights, residual, &atom.shape);
            let within = inner(weights, &atom.shape, &atom.shape);
            if within <= 0.0 {
                return None;
            }
            let gain_db = (along / within).clamp(-fitting.cut_at_most_db, fitting.boost_at_most_db);
            Some(Pick {
                atom,
                gain_db,
                reduction: gain_db * (2.0 * along - gain_db * within),
            })
        })
        .max_by(|a, b| a.reduction.total_cmp(&b.reduction))
}

fn settled(
    band: Band,
    residual_without_it: &[f64],
    weights: &[f64],
    grid: &[f64],
    fitting: &Fitting,
) -> Placed {
    let mut band = band;
    let mut response = response_of(band, grid);
    for _ in 0..REFINING_STEPS {
        let along = inner(weights, residual_without_it, &response);
        let within = inner(weights, &response, &response);
        if within <= 0.0 || band.gain.decibels() == 0.0 {
            break;
        }
        let scaled = band.gain.decibels() * (along / within);
        band = band_at(
            band.kind,
            band.frequency,
            scaled.clamp(-fitting.cut_at_most_db, fitting.boost_at_most_db),
            band.q,
        );
        response = response_of(band, grid);
    }
    Placed { band, response }
}

fn nudged_around(band: Band, fitting: &Fitting) -> Vec<Band> {
    let mut around = Vec::new();
    for steps in NUDGES_OF_FREQUENCY {
        let hertz =
            band.frequency.hertz() * 2.0_f64.powf(f64::from(steps) * NUDGE_OF_FREQUENCY_OCTAVES);
        let Ok(frequency) = Frequency::from_hertz(hertz) else {
            continue;
        };
        for scale in NUDGES_OF_Q {
            let units = (band.q.units() * scale).clamp(fitting.q_least, fitting.q_most);
            let Ok(q) = Q::from_units(units) else {
                continue;
            };
            around.push(band_at(band.kind, frequency, band.gain.decibels(), q));
        }
    }
    around
}

fn left_over(residual: &[f64], response: &[f64], weights: &[f64]) -> f64 {
    residual
        .iter()
        .zip(response)
        .zip(weights)
        .map(|((residual, response), weight)| weight * (residual - response).powi(2))
        .sum()
}

fn polished(
    band: Band,
    residual_without_it: &[f64],
    weights: &[f64],
    grid: &[f64],
    fitting: &Fitting,
) -> Placed {
    nudged_around(band, fitting)
        .into_iter()
        .map(|nudged| settled(nudged, residual_without_it, weights, grid, fitting))
        .min_by(|a, b| {
            left_over(residual_without_it, &a.response, weights).total_cmp(&left_over(
                residual_without_it,
                &b.response,
                weights,
            ))
        })
        .unwrap_or_else(|| settled(band, residual_without_it, weights, grid, fitting))
}

fn subtract(residual: &mut [f64], response: &[f64]) {
    for (residual, response) in residual.iter_mut().zip(response) {
        *residual -= response;
    }
}

fn add(residual: &mut [f64], response: &[f64]) {
    for (residual, response) in residual.iter_mut().zip(response) {
        *residual += response;
    }
}

pub fn fit(measured: &Response, goal: &Response, fitting: &Fitting) -> Result<Fitted> {
    must_reach_the_mid_band(measured)?;
    must_reach_the_mid_band(goal)?;

    let grid: Vec<f64> = sweep(GRID_POINTS).collect();
    let weights: Vec<f64> = grid.iter().map(|hertz| weight_at(*hertz)).collect();
    let measured = levels_on(measured, &grid);
    let goal = levels_on(goal, &grid);
    let wanted: Vec<f64> = goal
        .iter()
        .zip(&measured)
        .map(|(goal, measured)| goal - measured)
        .collect();

    let off_by_db_before = weighted_rms(&wanted, &weights);
    let first_sum = weighted_sum_of_squares(&wanted, &weights);

    let atoms = atoms_for(fitting, &grid);
    let mut residual = wanted.clone();
    let mut placed: Vec<Placed> = Vec::new();
    let mut taken = Taken {
        low_shelf: false,
        high_shelf: false,
        places: Vec::new(),
    };

    while placed.len() < fitting.bands.min(MAX_BANDS) {
        let Some(pick) = best_atom(&atoms, &residual, &weights, &taken, fitting) else {
            break;
        };
        if pick.gain_db.abs() < WORTH_A_BAND_DB
            || pick.reduction < WORTH_A_BAND_OF_THE_ERROR * first_sum
        {
            break;
        }

        let band = band_at(
            pick.atom.kind,
            pick.atom.frequency,
            pick.gain_db,
            pick.atom.q,
        );
        let one = settled(band, &residual, &weights, &grid, fitting);
        subtract(&mut residual, &one.response);
        taken.take(pick.atom);
        placed.push(one);
    }

    for _ in 0..REFINING_PASSES {
        for one in &mut placed {
            add(&mut residual, &one.response);
            *one = polished(one.band, &residual, &weights, &grid, fitting);
            subtract(&mut residual, &one.response);
        }
    }

    placed.retain(|one| one.band.gain.decibels().abs() >= WORTH_A_BAND_DB);

    let mut remaining = wanted;
    for one in &placed {
        subtract(&mut remaining, &one.response);
    }
    let off_by_db_after = weighted_rms(&remaining, &weights);

    let mut bands: Vec<Band> = placed.into_iter().map(|one| one.band).collect();
    bands.sort_by_key(|band| band.frequency.centihertz());
    if bands.is_empty() {
        return Err(Error::NothingToCorrect {
            off_by_db: off_by_db_before,
        });
    }

    let mut profile = Profile::new(Preamp::NONE, bands)?;
    profile.set_preamp(profile.fitted_preamp(FITTED_AT));
    Ok(Fitted {
        profile,
        off_by_db_before,
        off_by_db_after,
    })
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::Profile;

    use super::*;
    use crate::{BuiltIn, measurement::read_response};

    const WITHIN_DB: f64 = 0.5;
    const CHECKED_AT_HZ: [f64; 15] = [
        40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1_000.0, 1_600.0, 2_500.0, 3_150.0, 4_000.0,
        6_300.0, 8_000.0, 10_000.0,
    ];

    fn known_error() -> Profile {
        let bands = vec![
            band_at(
                BandKind::LowShelf,
                Frequency::from_hertz(120.0).expect("in range"),
                4.0,
                Q::from_units(0.7).expect("in range"),
            ),
            band_at(
                BandKind::Peaking,
                Frequency::from_hertz(250.0).expect("in range"),
                -4.0,
                Q::from_units(1.4).expect("in range"),
            ),
            band_at(
                BandKind::Peaking,
                Frequency::from_hertz(3_150.0).expect("in range"),
                5.0,
                Q::from_units(2.0).expect("in range"),
            ),
            band_at(
                BandKind::HighShelf,
                Frequency::from_hertz(8_000.0).expect("in range"),
                -4.0,
                Q::from_units(0.7).expect("in range"),
            ),
        ];
        Profile::new(Preamp::NONE, bands).expect("few bands")
    }

    fn mid_band_mean(error: &Profile) -> f64 {
        let mid: Vec<f64> = sweep(512)
            .filter(|hertz| (NORMALISED_FROM_HZ..=NORMALISED_TO_HZ).contains(hertz))
            .map(|hertz| error.magnitude_db(hertz, FITTED_AT))
            .collect();
        mid.iter().sum::<f64>() / mid.len() as f64
    }

    fn measured_text(goal: &Response, error: &Profile, level_offset_db: f64) -> String {
        let mut text = String::from("* measured\n");
        let mut hertz = 10.0_f64;
        while hertz < 30_000.0 {
            text.push_str(&format!(
                "{hertz:.3}\t{:.4}\t0\n",
                goal.at(hertz) - error.magnitude_db(hertz, FITTED_AT) + level_offset_db
            ));
            hertz *= 2.0_f64.powf(1.0 / 12.0);
        }
        text
    }

    fn worst_miss_db(fitted: &Fitted, error: &Profile) -> f64 {
        let offset = mid_band_mean(error);
        CHECKED_AT_HZ
            .iter()
            .map(|hertz| {
                let realised = fitted.profile.magnitude_db(*hertz, FITTED_AT)
                    - fitted.profile.preamp().decibels();
                let wanted = error.magnitude_db(*hertz, FITTED_AT) - offset;
                (realised - wanted).abs()
            })
            .fold(0.0, f64::max)
    }

    fn fitted_against(goal: &Response, error: &Profile, fitting: &Fitting) -> Fitted {
        let measured = read_response(measured_text(goal, error, 17.0).as_bytes())
            .expect("a readable measurement")
            .response;
        fit(&measured, goal, fitting).expect("a fit")
    }

    #[test]
    fn a_known_error_in_a_measurement_of_a_flat_target_is_fitted_away_within_half_a_decibel() {
        let error = known_error();
        let fitted = fitted_against(&Response::flat(), &error, &Fitting::default());

        let miss = worst_miss_db(&fitted, &error);
        assert!(miss < WITHIN_DB, "{miss} dB left");
        assert!(fitted.off_by_db_after < fitted.off_by_db_before / 4.0);
        assert!(fitted.off_by_db_after < 0.3, "{}", fitted.off_by_db_after);
    }

    #[test]
    fn a_known_error_against_the_harman_over_ear_curve_is_fitted_away_the_same_way() {
        let error = known_error();
        let goal = BuiltIn::HarmanOverEar2018.response().expect("a built-in");
        let fitted = fitted_against(&goal, &error, &Fitting::default());

        let miss = worst_miss_db(&fitted, &error);
        assert!(miss < WITHIN_DB, "{miss} dB left");
    }

    #[test]
    fn a_fit_is_held_under_full_scale_by_the_preamp_it_comes_with() {
        let error = known_error();
        let fitted = fitted_against(&Response::flat(), &error, &Fitting::default());

        assert!(!fitted.profile.preamp().is_none());
        assert!(
            fitted.profile.peak_db(FITTED_AT) + fitted.profile.preamp().decibels() <= 0.05,
            "{}",
            fitted.profile.preamp()
        );
        assert_eq!(
            fitted.profile.preamp(),
            fitted.profile.fitted_preamp(FITTED_AT)
        );
    }

    #[test]
    fn a_fit_stays_within_the_bands_the_boost_and_the_cut_it_was_allowed() {
        let error = known_error();
        let fitting = Fitting {
            bands: 3,
            boost_at_most_db: 2.0,
            cut_at_most_db: 3.0,
            ..Fitting::default()
        };
        let fitted = fitted_against(&Response::flat(), &error, &fitting);

        assert!(fitted.profile.bands().len() <= 3);
        for band in fitted.profile.bands() {
            let gain = band.gain.decibels();
            assert!((-3.0..=2.0).contains(&gain), "{gain} dB");
        }
    }

    #[test]
    fn a_fit_uses_each_shelf_once_and_keeps_its_q_inside_the_range() {
        let error = known_error();
        let fitting = Fitting {
            q_least: 0.7,
            q_most: 2.0,
            ..Fitting::default()
        };
        let fitted = fitted_against(&Response::flat(), &error, &fitting);

        for kind in [BandKind::LowShelf, BandKind::HighShelf] {
            let held = fitted
                .profile
                .bands()
                .iter()
                .filter(|band| band.kind == kind)
                .count();
            assert!(held <= 1, "{kind} held {held} times");
        }
        for band in fitted.profile.bands() {
            assert!((0.7..=2.0).contains(&band.q.units()), "{}", band.q);
        }
    }

    #[test]
    fn a_measurement_already_on_its_target_has_nothing_to_correct() {
        let measured =
            read_response(measured_text(&Response::flat(), &Profile::flat(), 3.0).as_bytes())
                .expect("a readable measurement")
                .response;

        assert!(matches!(
            fit(&measured, &Response::flat(), &Fitting::default()),
            Err(Error::NothingToCorrect { .. })
        ));
    }

    #[test]
    fn a_response_that_stops_short_of_the_mid_band_cannot_be_set_against_another() {
        let low_only =
            read_response(b"20 0\n30 0\n40 0\n60 0\n80 0\n100 0\n150 0\n200 0\n300 0\n400 0\n")
                .expect("readable")
                .response;

        assert!(matches!(
            fit(&low_only, &Response::flat(), &Fitting::default()),
            Err(Error::ResponseMissesTheMidBand { .. })
        ));
        assert!(matches!(
            fit(&Response::flat(), &low_only, &Fitting::default()),
            Err(Error::ResponseMissesTheMidBand { .. })
        ));
    }

    #[test]
    fn the_level_a_measurement_was_taken_at_does_not_change_the_fit() {
        let error = known_error();
        let goal = Response::flat();
        let quiet = read_response(measured_text(&goal, &error, -30.0).as_bytes())
            .expect("readable")
            .response;
        let loud = read_response(measured_text(&goal, &error, 40.0).as_bytes())
            .expect("readable")
            .response;

        let quiet = fit(&quiet, &goal, &Fitting::default()).expect("a fit");
        let loud = fit(&loud, &goal, &Fitting::default()).expect("a fit");

        assert_eq!(quiet.profile, loud.profile);
    }
}
