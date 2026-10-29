use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_core::{
    Error as CoreError,
    abx::{Arm, Arms, Session, TRIALS_BY_DEFAULT, Verdict},
    eq::FITTED_AT,
};
use resonate_engine::{
    Audition, Command, Contest, DitherKind, Dithering, Equalisation, FilterPhase, Lowered,
    NoiseShaping, Quality, Resampling, Restoration,
};

use crate::{
    Error, MobilePlayer, Result,
    equaliser::EqualiserCurve,
    playback::{LossyRestoration, PlaybackPreferences, ResamplingPhase, ResamplingQuality},
};

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
    pub levels: TestLevels,
    pub heard: Option<TestArm>,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum TestLevels {
    Matched { residual_db: f64 },
    Alike,
    MeasuredAlike,
    Lowered { arm: TestArm, db: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SettingKind {
    Restoration,
    Resampling,
    Dither,
}

const DITHER_AS_PLAYED: TestedDither = TestedDither::Shaped;

fn arm_named(arm: Arm) -> TestArm {
    match arm {
        Arm::A => TestArm::A,
        Arm::B => TestArm::B,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TestedDither {
    Off,
    Flat,
    Shaped,
}

impl From<TestedDither> for Dithering {
    fn from(dither: TestedDither) -> Self {
        match dither {
            TestedDither::Off => Self {
                kind: DitherKind::None,
                shaping: NoiseShaping::None,
            },
            TestedDither::Flat => Self {
                kind: DitherKind::Triangular,
                shaping: NoiseShaping::None,
            },
            TestedDither::Shaped => Self {
                kind: DitherKind::Triangular,
                shaping: NoiseShaping::Threshold,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TestedSetting {
    Restoration {
        a: LossyRestoration,
        b: LossyRestoration,
    },
    Resampling {
        a_quality: ResamplingQuality,
        a_phase: ResamplingPhase,
        b_quality: ResamplingQuality,
        b_phase: ResamplingPhase,
    },
    Dither {
        a: TestedDither,
        b: TestedDither,
    },
}

const fn restoration_of(restoration: LossyRestoration) -> Restoration {
    match restoration {
        LossyRestoration::Off => Restoration::Off,
        LossyRestoration::Repair => Restoration::Repair,
        LossyRestoration::Extend => Restoration::Extend,
    }
}

const fn resampling_of(quality: ResamplingQuality, phase: ResamplingPhase) -> Resampling {
    Resampling {
        quality: match quality {
            ResamplingQuality::Fast => Quality::Fast,
            ResamplingQuality::Balanced => Quality::Balanced,
            ResamplingQuality::High => Quality::High,
            ResamplingQuality::VeryHigh => Quality::VeryHigh,
        },
        phase: match phase {
            ResamplingPhase::Linear => FilterPhase::Linear,
            ResamplingPhase::Intermediate => FilterPhase::Intermediate,
            ResamplingPhase::Minimum => FilterPhase::Minimum,
        },
    }
}

impl From<TestedSetting> for Contest {
    fn from(setting: TestedSetting) -> Self {
        match setting {
            TestedSetting::Restoration { a, b } => Self::Restoration {
                a: restoration_of(a),
                b: restoration_of(b),
            },
            TestedSetting::Resampling {
                a_quality,
                a_phase,
                b_quality,
                b_phase,
            } => Self::Resampling {
                a: resampling_of(a_quality, a_phase),
                b: resampling_of(b_quality, b_phase),
            },
            TestedSetting::Dither { a, b } => Self::Dithering {
                a: a.into(),
                b: b.into(),
            },
        }
    }
}

struct Running {
    session: Session,
    heard: Option<TestArm>,
}

#[derive(Clone, Debug, PartialEq)]
enum Compared {
    Equaliser(Arms),
    Setting { contest: Contest, lowered: Lowered },
}

#[derive(uniffi::Object)]
pub struct ListeningTest {
    running: Mutex<Running>,
    compared: Compared,
}

impl ListeningTest {
    fn begin(
        trials: u32,
        seed: u64,
        chain: &resonate_core::eq::Profile,
        rate: resonate_core::SampleRate,
    ) -> Result<Self> {
        let arms = Arms::matched(chain, rate).map_err(refused)?;
        Self::comparing(trials, seed, Compared::Equaliser(arms))
    }

    fn comparing(trials: u32, seed: u64, compared: Compared) -> Result<Self> {
        let session = Session::new(trials as usize, seed).map_err(refused)?;
        Ok(Self {
            running: Mutex::new(Running {
                session,
                heard: None,
            }),
            compared,
        })
    }

    fn command_for(&self, hear: TestHear) -> Result<Option<Command>> {
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
        Ok(Some(match &self.compared {
            Compared::Equaliser(arms) => Command::SetEqualisation(Arc::new(Equalisation {
                enabled: true,
                fallback: Some(Arc::new(arms.profile(arm).clone())),
                ..Equalisation::default()
            })),
            Compared::Setting { contest, lowered } => Command::Audition(Some(Audition {
                contest: *contest,
                heard: arm,
                lowered: *lowered,
            })),
        }))
    }

    fn levels(&self) -> TestLevels {
        match &self.compared {
            Compared::Equaliser(arms) => TestLevels::Matched {
                residual_db: arms.residual_db(),
            },
            Compared::Setting { contest, .. } if contest.is_flat_in_level() => TestLevels::Alike,
            Compared::Setting { lowered, .. } if lowered.by.is_none() => TestLevels::MeasuredAlike,
            Compared::Setting { lowered, .. } => TestLevels::Lowered {
                arm: arm_named(lowered.arm),
                db: f64::from(lowered.by.millibels().abs()) / 100.0,
            },
        }
    }
}

const SETTING_TEST_BEGUN_WITHIN: Duration = Duration::from_secs(10);

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
            levels: self.levels(),
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
pub fn setting_test_by_default(
    kind: SettingKind,
    preferences: PlaybackPreferences,
) -> TestedSetting {
    match kind {
        SettingKind::Restoration => TestedSetting::Restoration {
            a: preferences.restoration,
            b: if preferences.restoration == LossyRestoration::Off {
                LossyRestoration::Extend
            } else {
                LossyRestoration::Off
            },
        },
        SettingKind::Resampling => TestedSetting::Resampling {
            a_quality: preferences.quality,
            a_phase: preferences.phase,
            b_quality: if preferences.quality == ResamplingQuality::Fast {
                ResamplingQuality::VeryHigh
            } else {
                ResamplingQuality::Fast
            },
            b_phase: preferences.phase,
        },
        SettingKind::Dither => TestedSetting::Dither {
            a: DITHER_AS_PLAYED,
            b: TestedDither::Off,
        },
    }
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
            self.playing
                .player()?
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

    pub fn begin_setting_test(
        &self,
        setting: TestedSetting,
        trials: u32,
    ) -> Result<Arc<ListeningTest>> {
        let contest = Contest::from(setting);
        let player = self.playing.player()?;
        let lowered = if contest.is_flat_in_level() {
            Lowered::NOTHING
        } else {
            player
                .levels_of_the_contest(contest, &AtomicBool::new(false))?
                .lowered()?
        };
        let test =
            ListeningTest::comparing(trials, clock_seed(), Compared::Setting { contest, lowered })?;
        self.playing
            .player()?
            .request(Command::Audition(Some(Audition {
                contest,
                heard: Arm::A,
                lowered,
            })))?
            .wait_for(SETTING_TEST_BEGUN_WITHIN)?;
        Ok(Arc::new(test))
    }

    pub fn hear_in_listening_test(&self, test: Arc<ListeningTest>, hear: TestHear) -> Result<()> {
        let Some(command) = test.command_for(hear)? else {
            return Ok(());
        };
        self.command(command)
    }

    pub fn end_listening_test(&self) -> Result<()> {
        let preferences = self.preferences.lock().clone();
        let equalisation = preferences.equalisation(preferences.equaliser.profile()?);
        self.command(Command::Audition(None))?;
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
        harness
            .player
            .playing
            .player()
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

    fn arms_of(test: &ListeningTest) -> &Arms {
        match &test.compared {
            Compared::Equaliser(arms) => arms,
            Compared::Setting { .. } => panic!("a test of the equaliser"),
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
        assert!(
            matches!(standing.levels, TestLevels::Matched { residual_db } if residual_db <= 0.1)
        );
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
        engine_holds(&harness, &sent(&arms_of(&test).a));
        assert_eq!(test.standing().heard, Some(TestArm::A));

        harness
            .player
            .hear_in_listening_test(Arc::clone(&test), TestHear::B)
            .expect("heard");
        engine_holds(&harness, &sent(&arms_of(&test).b));
        assert_eq!(test.standing().heard, Some(TestArm::B));

        let x = drawn(&test);
        harness
            .player
            .hear_in_listening_test(Arc::clone(&test), TestHear::X)
            .expect("heard");
        engine_holds(&harness, &sent(arms_of(&test).profile(x)));
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

        assert_eq!(*arms_of(&test), desktop);
    }

    #[test]
    fn a_setting_test_is_refused_while_nothing_plays() {
        let harness = Harness::new();

        let begun = harness.player.begin_setting_test(
            TestedSetting::Dither {
                a: TestedDither::Shaped,
                b: TestedDither::Off,
            },
            4,
        );

        assert!(matches!(
            begun,
            Err(Error::Player(resonate_engine::Error::NothingToAudition))
        ));
    }

    #[test]
    fn each_setting_a_phone_names_is_the_contest_the_engine_runs() {
        assert_eq!(
            Contest::from(TestedSetting::Restoration {
                a: LossyRestoration::Off,
                b: LossyRestoration::Extend,
            }),
            Contest::Restoration {
                a: Restoration::Off,
                b: Restoration::Extend,
            }
        );
        assert_eq!(
            Contest::from(TestedSetting::Resampling {
                a_quality: ResamplingQuality::Fast,
                a_phase: ResamplingPhase::Minimum,
                b_quality: ResamplingQuality::VeryHigh,
                b_phase: ResamplingPhase::Linear,
            }),
            Contest::Resampling {
                a: Resampling {
                    quality: Quality::Fast,
                    phase: FilterPhase::Minimum,
                },
                b: Resampling {
                    quality: Quality::VeryHigh,
                    phase: FilterPhase::Linear,
                },
            }
        );
        assert_eq!(
            Contest::from(TestedSetting::Dither {
                a: TestedDither::Shaped,
                b: TestedDither::Off,
            }),
            Contest::Dithering {
                a: Dithering {
                    kind: DitherKind::Triangular,
                    shaping: NoiseShaping::Threshold,
                },
                b: Dithering {
                    kind: DitherKind::None,
                    shaping: NoiseShaping::None,
                },
            }
        );
    }

    fn setting_test_of(contest: Contest, lowered: Lowered) -> ListeningTest {
        ListeningTest::comparing(4, 7, Compared::Setting { contest, lowered }).expect("a session")
    }

    #[test]
    fn a_setting_test_says_how_its_levels_were_matched() {
        let flat = Contest::Dithering {
            a: Dithering {
                kind: DitherKind::Triangular,
                shaping: NoiseShaping::Threshold,
            },
            b: Dithering {
                kind: DitherKind::None,
                shaping: NoiseShaping::None,
            },
        };
        let measured = Contest::Restoration {
            a: Restoration::Off,
            b: Restoration::Extend,
        };
        let by = resonate_core::Trim::from_millibels(12).expect("a trim");

        assert_eq!(
            setting_test_of(flat, Lowered::NOTHING).standing().levels,
            TestLevels::Alike
        );
        assert_eq!(
            setting_test_of(measured, Lowered::NOTHING)
                .standing()
                .levels,
            TestLevels::MeasuredAlike
        );
        assert_eq!(
            setting_test_of(measured, Lowered { arm: Arm::B, by })
                .standing()
                .levels,
            TestLevels::Lowered {
                arm: TestArm::B,
                db: 0.12
            }
        );
    }

    #[test]
    fn a_setting_test_begins_against_the_opposite_of_what_the_phone_has_set() {
        let preferences = PlaybackPreferences {
            restoration: LossyRestoration::Off,
            quality: ResamplingQuality::Fast,
            phase: ResamplingPhase::Minimum,
            ..PlaybackPreferences::default()
        };
        let extended = PlaybackPreferences {
            restoration: LossyRestoration::Repair,
            quality: ResamplingQuality::High,
            ..preferences.clone()
        };

        assert_eq!(
            setting_test_by_default(SettingKind::Restoration, preferences.clone()),
            TestedSetting::Restoration {
                a: LossyRestoration::Off,
                b: LossyRestoration::Extend,
            }
        );
        assert_eq!(
            setting_test_by_default(SettingKind::Restoration, extended.clone()),
            TestedSetting::Restoration {
                a: LossyRestoration::Repair,
                b: LossyRestoration::Off,
            }
        );
        assert_eq!(
            setting_test_by_default(SettingKind::Resampling, preferences),
            TestedSetting::Resampling {
                a_quality: ResamplingQuality::Fast,
                a_phase: ResamplingPhase::Minimum,
                b_quality: ResamplingQuality::VeryHigh,
                b_phase: ResamplingPhase::Minimum,
            }
        );
        assert_eq!(
            setting_test_by_default(SettingKind::Resampling, extended),
            TestedSetting::Resampling {
                a_quality: ResamplingQuality::High,
                a_phase: ResamplingPhase::Minimum,
                b_quality: ResamplingQuality::Fast,
                b_phase: ResamplingPhase::Minimum,
            }
        );
        assert_eq!(
            setting_test_by_default(SettingKind::Dither, PlaybackPreferences::default()),
            TestedSetting::Dither {
                a: TestedDither::Shaped,
                b: TestedDither::Off,
            }
        );
    }

    #[test]
    fn the_engines_refusals_of_a_test_are_told_in_sentences_a_phone_shows() {
        for error in [
            resonate_engine::Error::NothingToAudition,
            resonate_engine::Error::ArmsAlike,
            resonate_engine::Error::LevelsUnmeasured,
            resonate_engine::Error::LevelsTooFarApart { millibels: 5000 },
        ] {
            let said = Error::Player(error).to_string();

            assert!(!said.ends_with('.'), "{said} reads as a sentence");
            assert!(
                !said.contains("millibel"),
                "{said} speaks the engine's language"
            );
        }
    }
}
