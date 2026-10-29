use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_core::{
    Error as CoreError,
    abx::{Arm, Arms, Session, TRIALS_BY_DEFAULT, Verdict},
    eq::FITTED_AT,
};
use resonate_engine::{Command, Equalisation};

use crate::{Error, MobilePlayer, Result, equaliser::EqualiserCurve};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TestArm {
    A,
    B,
}

impl From<TestArm> for Arm {
    fn from(arm: TestArm) -> Self {
        match arm {
            TestArm::A => Self::A,
            TestArm::B => Self::B,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TestHear {
    A,
    B,
    X,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TestStanding {
    pub planned: u32,
    pub answered: u32,
    pub right: u32,
    pub finished: bool,
    pub chance_percent: f64,
    pub significant: bool,
    pub verdicts: Vec<bool>,
    pub residual_db: f64,
    pub heard: Option<TestArm>,
}

struct Running {
    session: Session,
    heard: Option<TestArm>,
}

#[derive(uniffi::Object)]
pub struct ListeningTest {
    running: Mutex<Running>,
    arms: Arms,
}

impl ListeningTest {
    fn begin(
        trials: u32,
        seed: u64,
        chain: &resonate_core::eq::Profile,
        rate: resonate_core::SampleRate,
    ) -> Result<Self> {
        let session = Session::new(trials as usize, seed).map_err(refused)?;
        let arms = Arms::matched(chain, rate).map_err(refused)?;
        Ok(Self {
            running: Mutex::new(Running {
                session,
                heard: None,
            }),
            arms,
        })
    }

    fn equalisation_of(&self, hear: TestHear) -> Result<Option<Equalisation>> {
        let mut running = self.running.lock();
        let arm = match hear {
            TestHear::A => Arm::A,
            TestHear::B => Arm::B,
            TestHear::X => match running.session.x() {
                Some(x) => x,
                None => return Ok(None),
            },
        };
        running.heard = match hear {
            TestHear::A => Some(TestArm::A),
            TestHear::B => Some(TestArm::B),
            TestHear::X => None,
        };
        Ok(Some(Equalisation {
            enabled: true,
            fallback: Some(Arc::new(self.arms.profile(arm).clone())),
            ..Equalisation::default()
        }))
    }
}

fn refused(error: CoreError) -> Error {
    match error {
        CoreError::AbxTrialsOutOfRange(planned) => Error::ListeningTestTrials {
            trials: u32::try_from(planned).unwrap_or(u32::MAX),
        },
        CoreError::LevelsNotMatched { millibels } => Error::ListeningTestLevels { millibels },
        CoreError::AbxFinished => Error::ListeningTestFinished,
        other => Error::Domain(other),
    }
}

fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64)
}

#[uniffi::export]
impl ListeningTest {
    pub fn standing(&self) -> TestStanding {
        let running = self.running.lock();
        let score = running.session.score();
        TestStanding {
            planned: running.session.planned() as u32,
            answered: score.answered as u32,
            right: score.right as u32,
            finished: running.session.is_finished(),
            chance_percent: score.p_value() * 100.0,
            significant: score.is_significant(),
            verdicts: running
                .session
                .verdicts()
                .map(|verdict| verdict == Verdict::Right)
                .collect(),
            residual_db: self.arms.residual_db(),
            heard: running.heard,
        }
    }

    pub fn answer(&self, said: TestArm) -> Result<bool> {
        let mut running = self.running.lock();
        let verdict = running.session.answer(said.into()).map_err(refused)?;
        running.heard = None;
        Ok(verdict == Verdict::Right)
    }
}

#[uniffi::export]
pub fn listening_test_lengths() -> Vec<u32> {
    let usual = TRIALS_BY_DEFAULT as u32;
    vec![usual / 2, usual, usual * 2]
}

#[uniffi::export]
impl MobilePlayer {
    pub fn begin_listening_test(
        &self,
        curve: EqualiserCurve,
        trials: u32,
    ) -> Result<Arc<ListeningTest>> {
        let profile = Arc::new(curve.profile()?);
        if profile.is_transparent() {
            return Err(Error::ListeningTestNothingToTest);
        }
        let rate = {
            let held = self.player.lock();
            let player = held.as_ref().ok_or(Error::Closed)?;
            player
                .state()
                .output
                .map_or(FITTED_AT, |output| output.negotiated.rate)
        };

        Ok(Arc::new(ListeningTest::begin(
            trials,
            clock_seed(),
            &profile.at_rate(rate),
            rate,
        )?))
    }

    pub fn hear_in_listening_test(&self, test: Arc<ListeningTest>, hear: TestHear) -> Result<()> {
        let Some(equalisation) = test.equalisation_of(hear)? else {
            return Ok(());
        };
        self.command(Command::SetEqualisation(Arc::new(equalisation)))
    }

    pub fn end_listening_test(&self) -> Result<()> {
        let preferences = self.preferences.lock().clone();
        let equalisation = preferences.equalisation(preferences.equaliser.profile()?);
        self.command(Command::SetEqualisation(Arc::new(equalisation)))
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Instant};

    use resonate_core::eq::{ChannelSet, Profile};

    use super::*;
    use crate::{
        equaliser::{EqualiserBand, EqualiserFilter},
        harness::{Harness, LOOKED_AGAIN_AFTER, SETTLED_WITHIN},
    };

    fn peaking(frequency_hz: f32, gain_db: f32) -> EqualiserBand {
        EqualiserBand {
            filter: EqualiserFilter::Peaking,
            frequency_hz,
            gain_db,
            q: 1.0,
            channels: ChannelSet::EVERY.bits(),
        }
    }

    fn boosting() -> EqualiserCurve {
        EqualiserCurve {
            name: String::new(),
            preamp_db: 0.0,
            bands: vec![peaking(1_000.0, 9.0)],
        }
    }

    fn engine_equalisation(harness: &Harness) -> Arc<Equalisation> {
        let held = harness.player.player.lock();
        held.as_ref()
            .expect("an open player")
            .output_settings()
            .equaliser
            .clone()
    }

    fn engine_holds(harness: &Harness, wanted: &Equalisation) {
        let began = Instant::now();
        while *engine_equalisation(harness) != *wanted {
            assert!(
                began.elapsed() < SETTLED_WITHIN,
                "the engine never took the equalisation"
            );
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
    }

    fn drawn(test: &ListeningTest) -> Arm {
        test.running.lock().session.x().expect("a trial is due")
    }

    fn named(arm: Arm) -> TestArm {
        match arm {
            Arm::A => TestArm::A,
            Arm::B => TestArm::B,
        }
    }

    fn sent(profile: &Profile) -> Equalisation {
        Equalisation {
            enabled: true,
            fallback: Some(Arc::new(profile.clone())),
            ..Equalisation::default()
        }
    }

    #[test]
    fn a_test_begins_level_matched_with_every_trial_unanswered() {
        let harness = Harness::new();

        let test = harness
            .player
            .begin_listening_test(boosting(), 16)
            .expect("a curve that levels");
        let standing = test.standing();

        assert_eq!(standing.planned, 16);
        assert_eq!(standing.answered, 0);
        assert!(!standing.finished);
        assert!(standing.residual_db <= 0.1);
        assert_eq!(standing.heard, None);
        assert!(standing.verdicts.is_empty());
    }

    #[test]
    fn a_curve_with_nothing_in_it_or_a_test_of_no_trials_is_refused_typed() {
        let harness = Harness::new();

        assert!(matches!(
            harness
                .player
                .begin_listening_test(EqualiserCurve::default(), 16),
            Err(Error::ListeningTestNothingToTest)
        ));
        assert!(matches!(
            harness.player.begin_listening_test(boosting(), 0),
            Err(Error::ListeningTestTrials { trials: 0 })
        ));
        assert!(matches!(
            harness.player.begin_listening_test(boosting(), 101),
            Err(Error::ListeningTestTrials { trials: 101 })
        ));
    }

    #[test]
    fn hearing_an_arm_sends_its_matched_profile_and_ending_puts_the_curve_back() {
        let harness = Harness::new();
        let test = harness
            .player
            .begin_listening_test(boosting(), 4)
            .expect("a curve that levels");

        harness
            .player
            .hear_in_listening_test(Arc::clone(&test), TestHear::A)
            .expect("heard");
        engine_holds(&harness, &sent(&test.arms.a));
        assert_eq!(test.standing().heard, Some(TestArm::A));

        harness
            .player
            .hear_in_listening_test(Arc::clone(&test), TestHear::B)
            .expect("heard");
        engine_holds(&harness, &sent(&test.arms.b));
        assert_eq!(test.standing().heard, Some(TestArm::B));

        let x = drawn(&test);
        harness
            .player
            .hear_in_listening_test(Arc::clone(&test), TestHear::X)
            .expect("heard");
        engine_holds(&harness, &sent(test.arms.profile(x)));
        assert_eq!(test.standing().heard, None);

        harness.player.end_listening_test().expect("put back");
        let preferences = harness.player.playback_preferences();
        let standing = preferences.equalisation(preferences.equaliser.profile().expect("a curve"));
        engine_holds(&harness, &standing);
    }

    #[test]
    fn answering_keeps_a_verdict_a_trial_and_refuses_one_past_the_last() {
        let harness = Harness::new();
        let test = harness
            .player
            .begin_listening_test(boosting(), 8)
            .expect("a curve that levels");

        while !test.standing().finished {
            assert!(test.answer(named(drawn(&test))).expect("answered"));
        }
        let standing = test.standing();

        assert_eq!(standing.answered, 8);
        assert_eq!(standing.right, 8);
        assert_eq!(standing.verdicts, vec![true; 8]);
        assert!(standing.significant);
        assert!(standing.chance_percent < 1.0);
        assert!(matches!(
            test.answer(TestArm::A),
            Err(Error::ListeningTestFinished)
        ));
        assert!(
            harness
                .player
                .hear_in_listening_test(Arc::clone(&test), TestHear::X)
                .is_ok()
        );
    }

    #[test]
    fn a_wrong_answer_is_told_and_a_guess_is_not_significant() {
        let harness = Harness::new();
        let test = harness
            .player
            .begin_listening_test(boosting(), 2)
            .expect("a curve that levels");
        let other = named(drawn(&test).other());

        assert!(!test.answer(other).expect("answered"));

        let standing = test.standing();
        assert_eq!(standing.verdicts, vec![false]);
        assert_eq!(standing.right, 0);
        assert!(!standing.significant);
    }

    #[test]
    fn the_lengths_offered_are_the_desktops_eight_sixteen_and_thirty_two() {
        assert_eq!(listening_test_lengths(), vec![8, 16, 32]);
    }

    #[test]
    fn the_arms_are_the_desktops_for_the_same_curve() {
        let harness = Harness::new();
        let test = harness
            .player
            .begin_listening_test(boosting(), 4)
            .expect("a curve that levels");
        let profile = Arc::new(boosting().profile().expect("a valid curve"));

        let desktop = Arms::matched(&profile.at_rate(FITTED_AT), FITTED_AT).expect("matched");

        assert_eq!(test.arms, desktop);
    }
}
