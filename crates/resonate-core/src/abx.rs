use crate::{
    Error, Result, SampleRate,
    eq::{Preamp, Profile, RESPONSE_POINTS, sweep},
};

pub const TRIALS_AT_MOST: usize = 100;
pub const TRIALS_BY_DEFAULT: usize = 16;
pub const LEVELS_MATCHED_WITHIN_DB: f64 = 0.1;
pub const SIGNIFICANT_AT: f64 = 0.05;
pub const MATCHED_FROM_HZ: f64 = 20.0;
pub const MATCHED_TO_HZ: f64 = 16_000.0;

const FIRST_STEP: u64 = 0x9E37_79B9_7F4A_7C15;
const SECOND_STEP: u64 = 0xBF58_476D_1CE4_E5B9;
const THIRD_STEP: u64 = 0x94D0_49BB_1331_11EB;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arm {
    A,
    B,
}

impl Arm {
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    Right,
    Wrong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Trial {
    pub x: Arm,
    pub answered: Arm,
}

impl Trial {
    pub fn verdict(self) -> Verdict {
        if self.x == self.answered {
            Verdict::Right
        } else {
            Verdict::Wrong
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Score {
    pub right: usize,
    pub answered: usize,
}

impl Score {
    pub fn of(verdicts: impl IntoIterator<Item = Verdict>) -> Self {
        verdicts
            .into_iter()
            .fold(Self::default(), |score, verdict| Self {
                right: score.right + usize::from(verdict == Verdict::Right),
                answered: score.answered + 1,
            })
    }

    pub fn p_value(self) -> f64 {
        binomial_tail(self.right, self.answered)
    }

    pub fn is_significant(self) -> bool {
        self.answered > 0 && self.p_value() < SIGNIFICANT_AT
    }
}

pub fn binomial_tail(at_least: usize, of: usize) -> f64 {
    if at_least == 0 {
        return 1.0;
    }
    if at_least > of {
        return 0.0;
    }
    let ln_factorial = ln_factorials(of);
    let ln_half_pow = of as f64 * std::f64::consts::LN_2;
    let tail: f64 = (at_least..=of)
        .map(|hits| {
            (ln_factorial[of] - ln_factorial[hits] - ln_factorial[of - hits] - ln_half_pow).exp()
        })
        .sum();
    tail.min(1.0)
}

fn ln_factorials(up_to: usize) -> Vec<f64> {
    let mut table = Vec::with_capacity(up_to + 1);
    let mut running = 0.0;
    table.push(running);
    for number in 1..=up_to {
        running += (number as f64).ln();
        table.push(running);
    }
    table
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    drawn: Vec<Arm>,
    trials: Vec<Trial>,
}

impl Session {
    pub fn new(planned: usize, seed: u64) -> Result<Self> {
        if planned == 0 || planned > TRIALS_AT_MOST {
            return Err(Error::AbxTrialsOutOfRange(planned));
        }
        let mut state = seed;
        let drawn = (0..planned)
            .map(|_| {
                if splitmix(&mut state) >> 63 == 0 {
                    Arm::A
                } else {
                    Arm::B
                }
            })
            .collect();
        Ok(Self {
            drawn,
            trials: Vec::with_capacity(planned),
        })
    }

    pub fn planned(&self) -> usize {
        self.drawn.len()
    }

    pub fn is_finished(&self) -> bool {
        self.trials.len() == self.drawn.len()
    }

    pub fn x(&self) -> Option<Arm> {
        self.drawn.get(self.trials.len()).copied()
    }

    pub fn answer(&mut self, answered: Arm) -> Result<Verdict> {
        let x = self.x().ok_or(Error::AbxFinished)?;
        let trial = Trial { x, answered };
        self.trials.push(trial);
        Ok(trial.verdict())
    }

    pub fn trials(&self) -> &[Trial] {
        &self.trials
    }

    pub fn verdicts(&self) -> impl Iterator<Item = Verdict> + '_ {
        self.trials.iter().map(|trial| trial.verdict())
    }

    pub fn score(&self) -> Score {
        Score::of(self.verdicts())
    }
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(FIRST_STEP);
    let mut mixed = *state;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(SECOND_STEP);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(THIRD_STEP);
    mixed ^ (mixed >> 31)
}

pub fn mean_gain_db(profile: &Profile, rate: SampleRate) -> f64 {
    let response = profile.response(rate, RESPONSE_POINTS);
    let (power, weighed) = sweep(RESPONSE_POINTS)
        .zip(response)
        .filter(|(hertz, _)| (MATCHED_FROM_HZ..=MATCHED_TO_HZ).contains(hertz))
        .fold((0.0, 0.0), |(power, weighed), (_, decibels)| {
            (power + 10.0_f64.powf(decibels / 10.0), weighed + 1.0)
        });
    if weighed == 0.0 {
        return 0.0;
    }
    10.0 * (power / weighed).log10()
}

fn levelling_preamp(decibels: f64) -> Result<Preamp> {
    Preamp::from_decibels(decibels).map_err(|_| Error::LevelsNotMatched {
        millibels: (decibels * 1_000.0).round() as i32,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arms {
    pub a: Profile,
    pub b: Profile,
    pub a_gain_db: f64,
    pub b_gain_db: f64,
}

impl Arms {
    pub fn matched(chain: &Profile, rate: SampleRate) -> Result<Self> {
        let louder_by = mean_gain_db(chain, rate);
        let lowered = louder_by.max(0.0);

        let mut a = chain.clone();
        a.set_preamp(levelling_preamp(chain.preamp().decibels() - lowered)?);

        let mut b = Profile::flat();
        b.set_preamp(levelling_preamp(louder_by.min(0.0))?);

        let a_gain_db = mean_gain_db(&a, rate);
        let b_gain_db = mean_gain_db(&b, rate);
        let apart = (a_gain_db - b_gain_db).abs();
        if apart > LEVELS_MATCHED_WITHIN_DB {
            return Err(Error::LevelsNotMatched {
                millibels: (apart * 1_000.0).round() as i32,
            });
        }
        Ok(Self {
            a,
            b,
            a_gain_db,
            b_gain_db,
        })
    }

    pub fn residual_db(&self) -> f64 {
        (self.a_gain_db - self.b_gain_db).abs()
    }

    pub fn profile(&self, arm: Arm) -> &Profile {
        match arm {
            Arm::A => &self.a,
            Arm::B => &self.b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::{Band, BandGain, BandKind, Frequency, Q};

    fn band(kind: BandKind, hertz: f64, decibels: f64) -> Band {
        Band::new(
            kind,
            Frequency::from_hertz(hertz).expect("in range"),
            BandGain::from_decibels(decibels).expect("in range"),
            Q::BUTTERWORTH,
        )
    }

    fn profile(preamp_db: f64, bands: Vec<Band>) -> Profile {
        Profile::new(Preamp::from_decibels(preamp_db).expect("in range"), bands).expect("bands")
    }

    #[test]
    fn the_binomial_tail_is_the_chance_of_doing_this_well_by_guessing() {
        assert!((binomial_tail(0, 10) - 1.0).abs() < 1e-12);
        assert!((binomial_tail(10, 10) - 1.0 / 1024.0).abs() < 1e-12);
        assert!((binomial_tail(8, 10) - 56.0 / 1024.0).abs() < 1e-12);
        assert!((binomial_tail(5, 10) - 638.0 / 1024.0).abs() < 1e-12);
        assert_eq!(binomial_tail(11, 10), 0.0);
    }

    #[test]
    fn twelve_right_of_sixteen_is_significant_and_ten_is_not() {
        let twelve = Score {
            right: 12,
            answered: 16,
        };
        let ten = Score {
            right: 10,
            answered: 16,
        };

        assert!(twelve.is_significant());
        assert!((twelve.p_value() - 0.0384).abs() < 0.0001);
        assert!(!ten.is_significant());
        assert!(!Score::default().is_significant());
    }

    #[test]
    fn the_same_seed_draws_the_same_sequence_and_draws_both_arms() {
        let first = Session::new(40, 7).expect("planned");
        let again = Session::new(40, 7).expect("planned");
        let other = Session::new(40, 8).expect("planned");

        assert_eq!(first, again);
        assert_ne!(first, other);
        assert!(first.drawn.contains(&Arm::A));
        assert!(first.drawn.contains(&Arm::B));
    }

    #[test]
    fn a_session_keeps_a_verdict_per_trial_and_refuses_an_answer_after_the_last() {
        let mut session = Session::new(3, 1).expect("planned");

        let first_x = session.x().expect("a trial is due");
        assert_eq!(session.answer(first_x), Ok(Verdict::Right));
        let second_x = session.x().expect("a trial is due");
        assert_eq!(session.answer(second_x.other()), Ok(Verdict::Wrong));
        assert!(!session.is_finished());
        let third_x = session.x().expect("a trial is due");
        session.answer(third_x).expect("answered");

        assert!(session.is_finished());
        assert_eq!(session.x(), None);
        assert_eq!(session.answer(Arm::A), Err(Error::AbxFinished));
        assert_eq!(
            session.score(),
            Score {
                right: 2,
                answered: 3
            }
        );
        assert_eq!(
            session.verdicts().collect::<Vec<_>>(),
            vec![Verdict::Right, Verdict::Wrong, Verdict::Right]
        );
    }

    #[test]
    fn a_session_of_no_trials_or_too_many_is_refused() {
        assert_eq!(Session::new(0, 1), Err(Error::AbxTrialsOutOfRange(0)));
        assert_eq!(
            Session::new(TRIALS_AT_MOST + 1, 1),
            Err(Error::AbxTrialsOutOfRange(TRIALS_AT_MOST + 1))
        );
    }

    #[test]
    fn a_flat_chain_matches_itself_exactly() {
        let arms = Arms::matched(&Profile::flat(), SampleRate::HZ_48000).expect("matched");

        assert!(arms.residual_db() < 1e-9);
        assert!(arms.a.is_transparent());
        assert!(arms.b.is_transparent());
    }

    #[test]
    fn a_boosting_chain_is_lowered_to_the_level_of_its_bypass() {
        let chain = profile(0.0, vec![band(BandKind::Peaking, 1_000.0, 9.0)]);
        let rate = SampleRate::HZ_48000;
        assert!(mean_gain_db(&chain, rate) > 0.3);

        let arms = Arms::matched(&chain, rate).expect("matched");

        assert!(arms.residual_db() <= LEVELS_MATCHED_WITHIN_DB);
        assert!(arms.a.preamp().decibels() < 0.0);
        assert!(arms.b.is_transparent());
        assert_eq!(arms.profile(Arm::A), &arms.a);
        assert_eq!(arms.profile(Arm::B), &arms.b);
    }

    #[test]
    fn a_cutting_chain_leaves_its_bypass_lowered_to_meet_it() {
        let chain = profile(-6.0, vec![band(BandKind::Peaking, 100.0, -4.0)]);
        let rate = SampleRate::HZ_48000;

        let arms = Arms::matched(&chain, rate).expect("matched");

        assert!(arms.residual_db() <= LEVELS_MATCHED_WITHIN_DB);
        assert_eq!(arms.a.preamp(), chain.preamp());
        assert!(arms.b.preamp().decibels() < -5.0);
    }

    #[test]
    fn a_chain_the_preamp_cannot_lower_enough_is_refused_not_played_unmatched() {
        let boosts = (0..20)
            .map(|step| band(BandKind::LowShelf, 10_000.0 + f64::from(step), 24.0))
            .collect();
        let chain = profile(24.0, boosts);

        assert!(matches!(
            Arms::matched(&chain, SampleRate::HZ_48000),
            Err(Error::LevelsNotMatched { .. })
        ));
    }

    #[test]
    fn matching_holds_across_rates_and_curves() {
        for rate in [
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            SampleRate::HZ_96000,
        ] {
            for gain in [-10.0, -3.0, 2.0, 7.5] {
                let chain = profile(
                    -1.0,
                    vec![
                        band(BandKind::LowShelf, 80.0, gain),
                        band(BandKind::Peaking, 3_000.0, -gain / 2.0),
                    ],
                );
                let arms = Arms::matched(&chain, rate).expect("matched");
                assert!(arms.residual_db() <= LEVELS_MATCHED_WITHIN_DB);
            }
        }
    }
}
