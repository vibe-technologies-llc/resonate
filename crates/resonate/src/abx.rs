use std::{
    ffi::OsString,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{bounded, never, select};
use resonate_core::{
    SampleRate,
    abx::{Arm, Arms, Score, Session, Verdict},
    eq::Profile,
};
use resonate_engine::{
    Audition, Command, Contest, DitherKind, Dithering, EngineConfig, Equalisation, Event, Lowered,
    NodeName, OutputStatus, Player, Quality, RepeatMode, Resampling, Restoration,
};

use crate::{
    Cli, Error, Result, SINK_TIMEOUT,
    cli::{AbxArgs, AbxOfArg},
    config::Config,
    input,
    readout::billing,
    signals,
};

const OUTPUT_WITHIN: Duration = Duration::from_secs(15);
const AUDITION_BEGUN_WITHIN: Duration = Duration::from_secs(10);

const HELP: &str = "\
  a  hear A      b  hear B      x  hear X
  1  X is A      2  X is B      q  quit        ?  this help";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hear {
    A,
    B,
    X,
}

impl Hear {
    const fn spoken(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Hear(Hear),
    Say(Arm),
    Help,
    Quit,
}

const fn key_of(stroke: char) -> Option<Key> {
    match stroke {
        'a' => Some(Key::Hear(Hear::A)),
        'b' => Some(Key::Hear(Hear::B)),
        'x' => Some(Key::Hear(Hear::X)),
        '1' => Some(Key::Say(Arm::A)),
        '2' => Some(Key::Say(Arm::B)),
        '?' | 'h' => Some(Key::Help),
        'q' => Some(Key::Quit),
        _ => None,
    }
}

fn contest_of(asked: AbxArgs, set: &EngineConfig) -> Result<Option<Contest>> {
    let elsewhere = Error::AbxAgainstElsewhere { testing: asked.of };
    let restoration = asked.against_restoration.is_some();
    let resampler = asked.against_quality.is_some() || asked.against_filter_phase.is_some();
    let dither = asked.against_dither.is_some() || asked.against_noise_shaping.is_some();
    match asked.of {
        AbxOfArg::Equaliser if restoration || resampler || dither => Err(elsewhere),
        AbxOfArg::Equaliser => Ok(None),
        AbxOfArg::Restoration if resampler || dither => Err(elsewhere),
        AbxOfArg::Restoration => {
            let a = set.restoration;
            let b = asked.against_restoration.map_or(
                if a == Restoration::Off {
                    Restoration::Extend
                } else {
                    Restoration::Off
                },
                Into::into,
            );
            Ok(Some(Contest::Restoration { a, b }))
        }
        AbxOfArg::Resampler if restoration || dither => Err(elsewhere),
        AbxOfArg::Resampler => {
            let a = Resampling {
                quality: set.quality,
                phase: set.filter_phase,
            };
            let b = Resampling {
                quality: asked.against_quality.map_or(
                    if a.quality == Quality::Fast {
                        Quality::VeryHigh
                    } else {
                        Quality::Fast
                    },
                    Into::into,
                ),
                phase: asked.against_filter_phase.map_or(a.phase, Into::into),
            };
            Ok(Some(Contest::Resampling { a, b }))
        }
        AbxOfArg::Dither if restoration || resampler => Err(elsewhere),
        AbxOfArg::Dither => {
            let a = Dithering {
                kind: set.dither,
                shaping: set.noise_shaping,
            };
            let b = Dithering {
                kind: asked.against_dither.map_or(
                    if a.kind == DitherKind::None {
                        DitherKind::Triangular
                    } else {
                        DitherKind::None
                    },
                    Into::into,
                ),
                shaping: asked.against_noise_shaping.map_or(a.shaping, Into::into),
            };
            Ok(Some(Contest::Dithering { a, b }))
        }
    }
}

enum Subject {
    Equaliser { arms: Arms, sink: NodeName },
    Setting { contest: Contest, lowered: Lowered },
}

struct Test {
    session: Session,
    subject: Subject,
}

impl Test {
    fn begin(
        trials: usize,
        seed: u64,
        chain: &Arc<Profile>,
        rate: SampleRate,
        sink: NodeName,
    ) -> Result<Self> {
        let session = Session::new(trials, seed)?;
        let arms = Arms::matched(&chain.at_rate(rate), rate)?;
        Ok(Self {
            session,
            subject: Subject::Equaliser { arms, sink },
        })
    }

    fn of_a_setting(trials: usize, seed: u64, contest: Contest, lowered: Lowered) -> Result<Self> {
        Ok(Self {
            session: Session::new(trials, seed)?,
            subject: Subject::Setting { contest, lowered },
        })
    }

    fn hearing(&self, hear: Hear) -> Option<Command> {
        let arm = match hear {
            Hear::A => Arm::A,
            Hear::B => Arm::B,
            Hear::X => self.session.x()?,
        };
        Some(match &self.subject {
            Subject::Equaliser { arms, sink } => Command::SetEqualisation(Arc::new(
                Equalisation::auditioning(sink.clone(), Arc::new(arms.profile(arm).clone())),
            )),
            Subject::Setting { contest, lowered } => Command::Audition(Some(Audition {
                contest: *contest,
                heard: arm,
                lowered: *lowered,
            })),
        })
    }

    fn putting_back(&self, resolved: &Arc<Equalisation>) -> Command {
        match self.subject {
            Subject::Equaliser { .. } => Command::SetEqualisation(Arc::clone(resolved)),
            Subject::Setting { .. } => Command::Audition(None),
        }
    }

    fn answered(&mut self, said: Arm) -> Option<Verdict> {
        self.session.answer(said).ok()
    }

    fn is_finished(&self) -> bool {
        self.session.is_finished()
    }

    fn trial(&self) -> String {
        format!(
            "trial {} of {}",
            self.session.score().answered + 1,
            self.session.planned()
        )
    }

    fn opening(&self) -> String {
        match &self.subject {
            Subject::Equaliser { arms, .. } => {
                format!(
                    "levels matched to {:.3} dB; {}",
                    arms.residual_db(),
                    self.trial()
                )
            }
            Subject::Setting { contest, .. } if contest.is_flat_in_level() => {
                format!("A and B play at one level; {}", self.trial())
            }
            Subject::Setting { lowered, .. } if lowered.by.is_none() => {
                format!("A and B measured at one level; {}", self.trial())
            }
            Subject::Setting { lowered, .. } => format!(
                "the louder arm is lowered by {:.2} dB to match the other; {}",
                f64::from(lowered.by.millibels()) / 100.0,
                self.trial()
            ),
        }
    }

    fn after(&self, verdict: Verdict) -> String {
        let said = match verdict {
            Verdict::Right => "right",
            Verdict::Wrong => "wrong",
        };
        if self.is_finished() {
            return said.to_owned();
        }
        format!("{said}; {}; {}", scored(self.session.score()), self.trial())
    }

    fn conclusion(&self) -> Vec<String> {
        let score = self.session.score();
        if score.answered == 0 {
            return vec!["no trial was answered".to_owned()];
        }

        let mut lines = vec![scored(score)];
        if !self.is_finished() {
            lines.push(format!(
                "stopped after {} of {} trials",
                score.answered,
                self.session.planned()
            ));
        }
        lines.push(
            if score.is_significant() {
                "that is unlikely to be luck: you can hear this"
            } else {
                "that could well be luck: no difference was shown"
            }
            .to_owned(),
        );
        lines
    }
}

fn scored(score: Score) -> String {
    format!(
        "{} of {} right, chance of that by guessing {:.1}% (p = {:.4})",
        score.right,
        score.answered,
        score.p_value() * 100.0,
        score.p_value()
    )
}

fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64)
}

fn output_opened(player: &Player) -> Result<OutputStatus> {
    let events = player.events().clone();
    loop {
        match events.recv_timeout(OUTPUT_WITHIN) {
            Ok(Event::OutputChanged(status)) => return Ok(status),
            Ok(Event::Failed { .. } | Event::QueueFinished) => {
                return Err(Error::NothingPlayed { failed: 1 });
            }
            Ok(_) => {}
            Err(_) => return Err(Error::AbxNoOutput),
        }
    }
}

fn the_test(
    player: &Player,
    equalisation: &Equalisation,
    status: &OutputStatus,
    trials: usize,
) -> Result<Test> {
    let sink = player
        .sinks_once_surveyed(SINK_TIMEOUT)
        .iter()
        .find(|sink| sink.id == status.sink)
        .map(|sink| sink.name.clone())
        .ok_or(Error::AbxNoOutput)?;
    let chain = equalisation
        .for_sink(&sink)
        .ok_or_else(|| Error::AbxNothingBound { sink: sink.clone() })?;

    Test::begin(trials, clock_seed(), chain, status.negotiated.rate, sink)
}

fn the_setting_test(player: &Player, contest: Contest, trials: usize) -> Result<Test> {
    let lowered = if contest.is_flat_in_level() {
        Lowered::NOTHING
    } else {
        said!("measuring how loud A and B play the song, to match them");
        player
            .levels_of_the_contest(contest, &AtomicBool::new(false))?
            .lowered()?
    };
    let test = Test::of_a_setting(trials, clock_seed(), contest, lowered)?;
    player
        .request(Command::Audition(Some(Audition {
            contest,
            heard: Arm::A,
            lowered,
        })))?
        .wait_for(AUDITION_BEGUN_WITHIN)?;
    Ok(test)
}

pub fn run(
    cli: &Cli,
    config: &Config,
    file: &OsString,
    trials: usize,
    asked: AbxArgs,
) -> Result<()> {
    let mut named = crate::queue_items(std::slice::from_ref(file));
    if named.len() > 1 {
        return Err(Error::AbxSeveralSongs { named: named.len() });
    }
    let song = named.pop().ok_or(Error::NothingPlayableNamed)?;

    let engine = crate::engine_config(cli, config);
    let contest = contest_of(asked, &engine)?;
    let resolved = Arc::clone(&engine.equaliser);
    let player = Arc::new(Player::with_sources(
        engine,
        Arc::new(resonate_codec::Sources::local()),
    )?);
    crate::confirm_sink(&player, crate::wanted_sink(cli, config).as_ref());

    player.send(Command::SetRepeat(RepeatMode::Track))?;
    player.send(Command::Load {
        items: vec![song],
        start_at: 0,
        autoplay: true,
    })?;

    let (asked_to_quit, quitting) = bounded(1);
    let silenced = Arc::clone(&player);
    signals::quit_when_told(
        asked_to_quit,
        signals::silencing(move || vec![Arc::clone(&silenced)]),
    );

    let status = output_opened(&player)?;
    let mut test = match contest {
        None => the_test(&player, &resolved, &status, trials)?,
        Some(contest) => the_setting_test(&player, contest, trials)?,
    };

    let behind = input::played_in_the_background();
    let keyed = (!behind)
        .then(input::KeyAtATime::where_a_terminal)
        .flatten();
    let events = player.events().clone();
    let strokes = if behind { never() } else { input::strokes() };

    said!("{HELP}");
    said!("{}", test.opening());

    let mut played = Ok(());
    loop {
        select! {
            recv(events) -> event => {
                let Ok(event) = event else { break };
                match event {
                    Event::Failed { track, error } => {
                        told!("{} failed", billing(&player, track));
                        crate::report(&error);
                        played = Err(Error::NothingPlayed { failed: 1 });
                        break;
                    }
                    Event::CommandFailed { command, error } => {
                        told!("{command:?} rejected");
                        crate::report(&error);
                    }
                    Event::QueueFinished => break,
                    _ => {}
                }
            }
            recv(strokes) -> stroke => {
                let Ok(stroke) = stroke else { break };
                match key_of(stroke) {
                    Some(Key::Quit) => break,
                    Some(Key::Help) => said!("{HELP}"),
                    Some(Key::Hear(hear)) => {
                        let Some(command) = test.hearing(hear) else { continue };
                        if let Err(error) = player.send(command) {
                            played = Err(error.into());
                            break;
                        }
                        said!("hearing {}", hear.spoken());
                    }
                    Some(Key::Say(arm)) => {
                        if let Some(verdict) = test.answered(arm) {
                            said!("{}", test.after(verdict));
                        }
                        if test.is_finished() {
                            break;
                        }
                    }
                    None => {}
                }
            }
            recv(quitting) -> _ => break,
        }
    }
    drop(keyed);

    for line in test.conclusion() {
        said!("{line}");
    }
    if let Err(error) = player.send(test.putting_back(&resolved)) {
        tracing::debug!(%error, "the engine had already stopped");
    }
    drop(player);
    played
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Preamp, Q};

    use super::*;
    use crate::cli::Sub;

    const RATE: SampleRate = SampleRate::HZ_48000;

    fn boosting() -> Arc<Profile> {
        let band = Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(1_000.0).expect("in range"),
            BandGain::from_decibels(9.0).expect("in range"),
            Q::BUTTERWORTH,
        );
        Arc::new(
            Profile::new(Preamp::from_decibels(0.0).expect("in range"), vec![band]).expect("bands"),
        )
    }

    fn arms_of(test: &Test) -> &Arms {
        match &test.subject {
            Subject::Equaliser { arms, .. } => arms,
            Subject::Setting { .. } => panic!("a test of the equaliser"),
        }
    }

    fn equalised(heard: Option<Command>) -> Arc<Equalisation> {
        match heard {
            Some(Command::SetEqualisation(equalisation)) => equalisation,
            other => panic!("an equaliser was not heard: {other:?}"),
        }
    }

    fn begun(trials: usize) -> Test {
        Test::begin(
            trials,
            7,
            &boosting(),
            RATE,
            NodeName::new("alsa_output.usb"),
        )
        .expect("a chain that levels")
    }

    #[test]
    fn the_keys_name_what_the_test_can_be_asked() {
        assert_eq!(key_of('a'), Some(Key::Hear(Hear::A)));
        assert_eq!(key_of('b'), Some(Key::Hear(Hear::B)));
        assert_eq!(key_of('x'), Some(Key::Hear(Hear::X)));
        assert_eq!(key_of('1'), Some(Key::Say(Arm::A)));
        assert_eq!(key_of('2'), Some(Key::Say(Arm::B)));
        assert_eq!(key_of('q'), Some(Key::Quit));
        assert_eq!(key_of('?'), Some(Key::Help));
        assert_eq!(key_of('A'), None);
        assert_eq!(key_of('z'), None);
    }

    #[test]
    fn hearing_an_arm_is_the_matched_profile_bound_to_the_sink_in_use() {
        let test = begun(4);

        let heard_a = equalised(test.hearing(Hear::A));
        let heard_b = equalised(test.hearing(Hear::B));
        let sink = NodeName::new("alsa_output.usb");

        assert_eq!(
            heard_a.for_sink(&sink).map(|profile| &**profile),
            Some(&arms_of(&test).a)
        );
        assert_eq!(
            heard_b.for_sink(&sink).map(|profile| &**profile),
            Some(&arms_of(&test).b)
        );
        assert!(heard_a.enabled && heard_b.enabled);
    }

    #[test]
    fn hearing_x_is_the_arm_the_session_drew_for_this_trial() {
        let test = begun(4);
        let x = test.session.x().expect("a trial is due");
        let sink = NodeName::new("alsa_output.usb");

        let heard = equalised(test.hearing(Hear::X));

        assert_eq!(
            heard.for_sink(&sink).map(|profile| &**profile),
            Some(arms_of(&test).profile(x))
        );
    }

    #[test]
    fn answering_every_trial_rightly_ends_on_a_significant_score() {
        let mut test = begun(8);

        while let Some(x) = test.session.x() {
            assert_eq!(test.answered(x), Some(Verdict::Right));
        }

        assert!(test.is_finished());
        assert!(test.hearing(Hear::X).is_none());
        assert_eq!(test.answered(Arm::A), None);
        let lines = test.conclusion();
        assert!(lines[0].starts_with("8 of 8 right"));
        assert!(lines[0].contains("p = 0.0039"));
        assert!(
            lines
                .last()
                .is_some_and(|line| line.contains("you can hear this"))
        );
    }

    #[test]
    fn a_test_left_early_says_how_far_it_got_and_that_a_guess_shows_nothing() {
        let mut test = begun(16);
        let x = test.session.x().expect("a trial is due");
        test.answered(x.other());

        let lines = test.conclusion();

        assert!(lines[0].starts_with("0 of 1 right"));
        assert_eq!(lines[1], "stopped after 1 of 16 trials");
        assert!(lines[2].contains("no difference was shown"));
        assert_eq!(
            begun(16).conclusion(),
            vec!["no trial was answered".to_owned()]
        );
    }

    #[test]
    fn the_verdict_after_an_answer_does_not_name_x_and_counts_on() {
        let mut test = begun(3);
        let x = test.session.x().expect("a trial is due");
        let verdict = test.answered(x).expect("answered");

        let said = test.after(verdict);

        assert!(said.starts_with("right; 1 of 1 right"));
        assert!(said.ends_with("trial 2 of 3"));
    }

    #[test]
    fn the_last_verdict_is_told_alone_for_the_conclusion_to_carry_the_score() {
        let mut test = begun(1);
        let x = test.session.x().expect("a trial is due");
        let verdict = test.answered(x.other()).expect("answered");

        assert_eq!(test.after(verdict), "wrong");
        assert!(test.conclusion()[0].starts_with("0 of 1 right"));
    }

    #[test]
    fn a_session_of_no_trials_or_too_many_does_not_begin() {
        let begin = |trials| {
            Test::begin(
                trials,
                1,
                &boosting(),
                RATE,
                NodeName::new("alsa_output.usb"),
            )
        };

        assert!(matches!(begin(0), Err(Error::Core(_))));
        assert!(matches!(begin(101), Err(Error::Core(_))));
        assert!(begin(100).is_ok());
    }

    #[test]
    fn the_grammar_takes_a_song_and_a_number_of_trials() {
        let parsed = Cli::try_parse_from(["resonate", "abx", "song.flac", "--trials", "24"]);

        let Ok(Cli {
            command: Some(Sub::Abx { file, trials, .. }),
            ..
        }) = parsed
        else {
            panic!("abx beside --trials was refused: {parsed:?}");
        };
        assert_eq!(file, OsString::from("song.flac"));
        assert_eq!(trials, 24);

        let by_default = Cli::try_parse_from(["resonate", "abx", "song.flac"]);
        assert!(matches!(
            by_default,
            Ok(Cli {
                command: Some(Sub::Abx { trials: 16, .. }),
                ..
            })
        ));
        assert!(Cli::try_parse_from(["resonate", "abx"]).is_err());
    }

    fn asked(against: &[&str]) -> AbxArgs {
        let mut words = vec!["resonate", "abx", "song.mp3"];
        words.extend_from_slice(against);
        match Cli::try_parse_from(words) {
            Ok(Cli {
                command: Some(Sub::Abx { against, .. }),
                ..
            }) => against,
            other => panic!("the grammar refused {against:?}: {other:?}"),
        }
    }

    #[test]
    fn a_setting_is_tested_as_the_settings_have_it_against_the_arm_named_or_its_opposite() {
        let set = EngineConfig::default();

        assert_eq!(contest_of(asked(&[]), &set).ok().flatten(), None);
        assert_eq!(
            contest_of(asked(&["--of", "restoration"]), &set)
                .ok()
                .flatten(),
            Some(Contest::Restoration {
                a: Restoration::Off,
                b: Restoration::Extend,
            })
        );
        assert_eq!(
            contest_of(
                asked(&["--of", "restoration", "--against-restoration", "repair"]),
                &set
            )
            .ok()
            .flatten(),
            Some(Contest::Restoration {
                a: Restoration::Off,
                b: Restoration::Repair,
            })
        );
        assert_eq!(
            contest_of(
                asked(&["--of", "resampler", "--against-filter-phase", "minimum"]),
                &set
            )
            .ok()
            .flatten(),
            Some(Contest::Resampling {
                a: Resampling {
                    quality: set.quality,
                    phase: set.filter_phase,
                },
                b: Resampling {
                    quality: Quality::Fast,
                    phase: resonate_engine::FilterPhase::Minimum,
                },
            })
        );
        assert_eq!(
            contest_of(asked(&["--of", "dither"]), &set).ok().flatten(),
            Some(Contest::Dithering {
                a: Dithering {
                    kind: set.dither,
                    shaping: set.noise_shaping,
                },
                b: Dithering {
                    kind: DitherKind::None,
                    shaping: set.noise_shaping,
                },
            })
        );
    }

    #[test]
    fn an_arm_named_for_another_test_is_refused() {
        let set = EngineConfig::default();

        for against in [
            &["--against-quality", "fast"][..],
            &["--of", "restoration", "--against-dither", "none"],
            &["--of", "dither", "--against-restoration", "extend"],
            &["--of", "resampler", "--against-noise-shaping", "flat"],
        ] {
            assert!(
                matches!(
                    contest_of(asked(against), &set),
                    Err(Error::AbxAgainstElsewhere { .. })
                ),
                "{against:?} was taken"
            );
        }
    }

    #[test]
    fn hearing_an_arm_of_a_setting_sends_the_audition_with_that_arm_and_ending_lets_it_go() {
        let contest = Contest::Dithering {
            a: Dithering {
                kind: DitherKind::Triangular,
                shaping: resonate_engine::NoiseShaping::Threshold,
            },
            b: Dithering {
                kind: DitherKind::None,
                shaping: resonate_engine::NoiseShaping::Threshold,
            },
        };
        let test = Test::of_a_setting(4, 7, contest, Lowered::NOTHING).expect("a session");
        let x = test.session.x().expect("a trial is due");

        for (hear, arm) in [(Hear::A, Arm::A), (Hear::B, Arm::B), (Hear::X, x)] {
            assert!(matches!(
                test.hearing(hear),
                Some(Command::Audition(Some(Audition { heard, .. }))) if heard == arm
            ));
        }
        assert!(matches!(
            test.putting_back(&Arc::new(Equalisation::default())),
            Command::Audition(None)
        ));
        assert!(test.opening().starts_with("A and B play at one level"));
    }
}
