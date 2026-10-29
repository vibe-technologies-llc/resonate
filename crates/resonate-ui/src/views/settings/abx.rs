use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gpui::{Context, Div, SharedString, Task, div, prelude::*, px, rgb};
use resonate_core::{
    abx::{Arm, Arms, Score, Session, TRIALS_BY_DEFAULT, Verdict},
    eq::Profile,
};
use resonate_engine::{
    Audition, Command, CommandKind, Contest, DitherKind, Dithering, Equalisation, Error,
    FilterPhase, Lowered, NodeName, NoiseShaping, OutputSettings, Player, Quality, Resampling,
    Restoration,
};

use crate::{
    Notice,
    icons::Icon,
    theme, toast,
    views::{
        kit,
        root::RootView,
        settings::{Choice, action, hugging, note, reading},
    },
};

const SHORT_TEST: usize = TRIALS_BY_DEFAULT / 2;
const LONG_TEST: usize = TRIALS_BY_DEFAULT * 2;

const AUDITION_BEGUN_WITHIN: Duration = Duration::from_secs(10);

const NOTHING_TO_TEST: &str = "Nothing is bound to the device in use, so there is no chain to \
                               test. Shape a curve under Bands first.";

const NOT_LEVELLABLE: &str = "The chain cannot be level-matched to its bypass, so it is not \
                              tested";

const NOT_PLAYING: &str = "Play a song first: the test is heard on one playing, and a DSD stream \
                           sent as it is cannot be switched within";

const ALIKE: &str = "A and B would sound the same on this stream, so there is nothing to tell \
                     apart";

const NOT_MEASURED: &str = "A and B could not be measured against each other, so the test cannot \
                            level them";

const TOO_FAR_APART: &str = "A and B play too far apart in loudness to be matched, so the test \
                             cannot level them";

const TRIALS_REFUSED: &str = "The listening test takes between one and a hundred trials";

const MEASURING: &str = "Measuring…";
const STARTING: &str = "Starting…";

macro_rules! how_it_works {
    ($lead:literal) => {
        concat!(
            $lead,
            " X is one of the two, drawn afresh for every trial: hear A, B and X as often as you \
             like on the song playing, then say which X was. A change is heard a few moments \
             after you press, within the audio already buffered."
        )
    };
}

const HOW_THE_EQUALISER_IS_TESTED: &str = how_it_works!(
    "A is the equaliser as set, B is the signal with it bypassed. Each is lowered, never raised, \
     until their loudness across 20 Hz to 16 kHz agrees to 0.1 dB, so the louder one cannot give \
     itself away. The room correction stays on in both."
);

const HOW_RESTORATION_IS_TESTED: &str = how_it_works!(
    "A is lossy restoration as set, B the setting you choose. Restoration changes how loud a \
     song plays, so the song is first measured through both, which takes a moment, and the \
     louder is lowered, never raised, until the two agree. A lossless file is touched by \
     neither."
);

const HOW_THE_RESAMPLER_IS_TESTED: &str = how_it_works!(
    "A is the resampler as set, B the quality and phase you choose. Both play at one level. The \
     resampler works only where the song's rate has to be changed to the device's; where it \
     does not, the two sound alike and the test is refused."
);

const HOW_DITHER_IS_TESTED: &str = how_it_works!(
    "A is the dither as set, B the kind and shaping you choose. Both play at one level. Dither \
     works only where the bits are cut down to the device's depth; where they are not, the two \
     sound alike and the test is refused."
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Testing {
    Equaliser,
    Restoration,
    Resampler,
    Dither,
}

impl Choice for Testing {
    const ALL: &'static [Self] = &[
        Self::Equaliser,
        Self::Restoration,
        Self::Resampler,
        Self::Dither,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Equaliser => "Equaliser",
            Self::Restoration => "Restoration",
            Self::Resampler => "Resampler",
            Self::Dither => "Dither",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Equaliser => HOW_THE_EQUALISER_IS_TESTED,
            Self::Restoration => HOW_RESTORATION_IS_TESTED,
            Self::Resampler => HOW_THE_RESAMPLER_IS_TESTED,
            Self::Dither => HOW_DITHER_IS_TESTED,
        })
    }
}

#[derive(Default)]
pub(crate) struct Chosen {
    restoration: Option<Restoration>,
    quality: Option<Quality>,
    phase: Option<FilterPhase>,
    dither: Option<DitherKind>,
    shaping: Option<NoiseShaping>,
}

impl Chosen {
    fn restoration_against(&self, set: &OutputSettings) -> Restoration {
        self.restoration
            .unwrap_or(if set.restoration == Restoration::Off {
                Restoration::Extend
            } else {
                Restoration::Off
            })
    }

    fn quality_against(&self, set: &OutputSettings) -> Quality {
        self.quality.unwrap_or(if set.quality == Quality::Fast {
            Quality::VeryHigh
        } else {
            Quality::Fast
        })
    }

    fn phase_against(&self, set: &OutputSettings) -> FilterPhase {
        self.phase.unwrap_or(set.filter_phase)
    }

    fn dither_against(&self, set: &OutputSettings) -> DitherKind {
        self.dither.unwrap_or(if set.dither == DitherKind::None {
            DitherKind::Triangular
        } else {
            DitherKind::None
        })
    }

    fn shaping_against(&self, set: &OutputSettings) -> NoiseShaping {
        self.shaping.unwrap_or(set.noise_shaping)
    }

    fn contest(&self, testing: Testing, set: &OutputSettings) -> Option<Contest> {
        match testing {
            Testing::Equaliser => None,
            Testing::Restoration => Some(Contest::Restoration {
                a: set.restoration,
                b: self.restoration_against(set),
            }),
            Testing::Resampler => Some(Contest::Resampling {
                a: Resampling {
                    quality: set.quality,
                    phase: set.filter_phase,
                },
                b: Resampling {
                    quality: self.quality_against(set),
                    phase: self.phase_against(set),
                },
            }),
            Testing::Dither => Some(Contest::Dithering {
                a: Dithering {
                    kind: set.dither,
                    shaping: set.noise_shaping,
                },
                b: Dithering {
                    kind: self.dither_against(set),
                    shaping: self.shaping_against(set),
                },
            }),
        }
    }
}

pub(crate) struct Setup {
    testing: Testing,
    chosen: Chosen,
    measuring: Measuring,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            testing: Testing::Equaliser,
            chosen: Chosen::default(),
            measuring: Measuring::idle(),
        }
    }
}

struct Measuring {
    stop: Option<Arc<AtomicBool>>,
    what: &'static str,
    _landing: Task<()>,
}

impl Measuring {
    fn idle() -> Self {
        Self {
            stop: None,
            what: STARTING,
            _landing: Task::ready(()),
        }
    }

    fn under(stop: Arc<AtomicBool>, what: &'static str, landing: Task<()>) -> Self {
        Self {
            stop: Some(stop),
            what,
            _landing: landing,
        }
    }

    fn finish(&mut self) {
        self.stop = None;
    }

    const fn is_busy(&self) -> bool {
        self.stop.is_some()
    }
}

impl Drop for Measuring {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

enum Subject {
    Equaliser { arms: Arms, sink: NodeName },
    Setting { contest: Contest, lowered: Lowered },
}

pub(crate) struct ListeningTest {
    session: Session,
    subject: Subject,
    heard: Option<Arm>,
    last: Option<Verdict>,
}

fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64)
}

const fn arm_named(arm: Arm) -> &'static str {
    match arm {
        Arm::A => "A",
        Arm::B => "B",
    }
}

impl ListeningTest {
    fn of_the_equaliser(
        trials: usize,
        chain: &Profile,
        rate: resonate_core::SampleRate,
        sink: NodeName,
    ) -> Option<Self> {
        let session = Session::new(trials, clock_seed()).ok()?;
        let arms = Arms::matched(chain, rate).ok()?;
        Some(Self {
            session,
            subject: Subject::Equaliser { arms, sink },
            heard: None,
            last: None,
        })
    }

    const fn of_a_setting(session: Session, contest: Contest, lowered: Lowered) -> Self {
        Self {
            session,
            subject: Subject::Setting { contest, lowered },
            heard: None,
            last: None,
        }
    }

    const fn testing(&self) -> Testing {
        match &self.subject {
            Subject::Equaliser { .. } => Testing::Equaliser,
            Subject::Setting {
                contest: Contest::Restoration { .. },
                ..
            } => Testing::Restoration,
            Subject::Setting {
                contest: Contest::Resampling { .. },
                ..
            } => Testing::Resampler,
            Subject::Setting {
                contest: Contest::Dithering { .. },
                ..
            } => Testing::Dither,
        }
    }

    fn command(&self, arm: Arm) -> Command {
        match &self.subject {
            Subject::Equaliser { arms, sink } => Command::SetEqualisation(Arc::new(
                Equalisation::auditioning(sink.clone(), Arc::new(arms.profile(arm).clone())),
            )),
            Subject::Setting { contest, lowered } => Command::Audition(Some(Audition {
                contest: *contest,
                heard: arm,
                lowered: *lowered,
            })),
        }
    }

    fn levels_said(&self) -> String {
        match &self.subject {
            Subject::Equaliser { arms, .. } => {
                format!("Levels matched to {:.3} dB", arms.residual_db())
            }
            Subject::Setting { contest, .. } if contest.is_flat_in_level() => {
                "Both play at one level".to_owned()
            }
            Subject::Setting { lowered, .. } if lowered.by.is_none() => {
                "Measured through both, the two play at one level".to_owned()
            }
            Subject::Setting { lowered, .. } => format!(
                "{} lowered by {:.2} dB to match the other",
                arm_named(lowered.arm),
                f64::from(lowered.by.millibels().abs()) / 100.0
            ),
        }
    }

    fn standing(&self) -> String {
        let score = self.session.score();
        if score.answered == 0 {
            return format!("Trial 1 of {}", self.session.planned());
        }
        let trial = if self.session.is_finished() {
            "Finished".to_owned()
        } else {
            format!("Trial {} of {}", score.answered + 1, self.session.planned())
        };
        format!("{trial} — {}", scored(score))
    }
}

fn scored(score: Score) -> String {
    format!(
        "{} of {} right, chance of that by guessing {:.1}%",
        score.right,
        score.answered,
        score.p_value() * 100.0
    )
}

fn verdicts_in_a_line(session: &Session) -> String {
    session
        .verdicts()
        .map(|verdict| match verdict {
            Verdict::Right => "✓",
            Verdict::Wrong => "✗",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn refusal(error: &Error) -> String {
    match error {
        Error::NothingToAudition => NOT_PLAYING.to_owned(),
        Error::ArmsAlike => ALIKE.to_owned(),
        Error::LevelsUnmeasured => NOT_MEASURED.to_owned(),
        Error::LevelsTooFarApart { .. } => TOO_FAR_APART.to_owned(),
        other => toast::would_not_do(CommandKind::Audition, other.cause()),
    }
}

fn begin_the_audition(
    engine: &Player,
    contest: Contest,
    stop: &AtomicBool,
) -> Result<Lowered, Error> {
    let lowered = if contest.is_flat_in_level() {
        Lowered::NOTHING
    } else {
        engine.levels_of_the_contest(contest, stop)?.lowered()?
    };
    engine
        .request(Command::Audition(Some(Audition {
            contest,
            heard: Arm::A,
            lowered,
        })))?
        .wait_for(AUDITION_BEGUN_WITHIN)?;
    Ok(lowered)
}

fn said_of_a(testing: Testing, set: &OutputSettings) -> Option<String> {
    match testing {
        Testing::Equaliser => None,
        Testing::Restoration => Some(set.restoration.label().to_owned()),
        Testing::Resampler => Some(format!(
            "{} · {}",
            set.quality.label(),
            set.filter_phase.label()
        )),
        Testing::Dither => Some(format!(
            "{} · {}",
            set.dither.label(),
            set.noise_shaping.label()
        )),
    }
}

impl RootView {
    pub(super) fn abx_group(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(test) = self.audition.as_ref() else {
            return self.setting_up_a_test(cx);
        };

        let finished = test.session.is_finished();
        let heard = test.heard;
        let standing = test.standing();
        let line = verdicts_in_a_line(&test.session);
        let levels = test.levels_said();
        let last = test.last;
        let significant = test.session.score().is_significant();
        let how = test.testing().meaning();

        let mut body = kit::section_body()
            .child(note(how))
            .child(
                div()
                    .text_size(px(theme::text_base()))
                    .text_color(rgb(theme::text()))
                    .child(standing),
            )
            .child(note(format!(
                "{levels}. {}",
                match heard {
                    Some(Arm::A) => "Hearing A",
                    Some(Arm::B) => "Hearing B",
                    None => "Hearing nothing chosen yet",
                }
            )));

        if !line.is_empty() {
            body = body.child(note(line));
        }
        if let Some(last) = last
            && !finished
        {
            body = body.child(note(match last {
                Verdict::Right => "The last one was right",
                Verdict::Wrong => "The last one was wrong",
            }));
        }

        if finished {
            body = body.child(note(if significant {
                "That is unlikely to be luck: you can hear this."
            } else {
                "That could well be luck: no difference was shown."
            }));
        } else {
            body = body
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(action(
                            "abx-hear-a",
                            "Hear A",
                            Icon::Play,
                            false,
                            |this, _, cx| this.hear_in_the_test(Hear::A, cx),
                            self,
                            cx,
                        ))
                        .child(action(
                            "abx-hear-b",
                            "Hear B",
                            Icon::Play,
                            false,
                            |this, _, cx| this.hear_in_the_test(Hear::B, cx),
                            self,
                            cx,
                        ))
                        .child(action(
                            "abx-hear-x",
                            "Hear X",
                            Icon::Play,
                            false,
                            |this, _, cx| this.hear_in_the_test(Hear::X, cx),
                            self,
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(action(
                            "abx-x-is-a",
                            "X is A",
                            Icon::Check,
                            false,
                            |this, _, cx| this.answer_the_test(Arm::A, cx),
                            self,
                            cx,
                        ))
                        .child(action(
                            "abx-x-is-b",
                            "X is B",
                            Icon::Check,
                            false,
                            |this, _, cx| this.answer_the_test(Arm::B, cx),
                            self,
                            cx,
                        )),
                );
        }

        body.child(hugging(action(
            "abx-end",
            if finished { "Done" } else { "End the test" },
            Icon::Stop,
            false,
            |this, _, cx| this.end_the_listening_test(cx),
            self,
            cx,
        )))
    }

    fn setting_up_a_test(&mut self, cx: &mut Context<Self>) -> Div {
        let testing = self.test_setup.testing;
        let set = self.player.read(cx).engine().output_settings();
        let busy = self.test_setup.measuring.is_busy();
        let what = self.test_setup.measuring.what;

        let mut body = kit::section_body().child(self.choices(
            "abx-testing",
            testing,
            cx,
            |this, testing, cx| {
                this.test_setup.testing = testing;
                cx.notify();
            },
        ));

        if let Some(held) = said_of_a(testing, &set) {
            body = body.child(reading("A", held));
        }
        body = body.child(self.against(testing, &set, cx));

        if busy {
            return body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(action(
                        "abx-measuring",
                        what,
                        Icon::Spinner,
                        true,
                        |_, _, _| {},
                        self,
                        cx,
                    ))
                    .child(action(
                        "abx-stop-measuring",
                        "Stop",
                        Icon::Stop,
                        false,
                        |this, _, cx| this.stop_measuring(cx),
                        self,
                        cx,
                    )),
            );
        }

        let mut starts = |trials: usize| {
            action(
                match trials {
                    SHORT_TEST => "abx-short",
                    LONG_TEST => "abx-long",
                    _ => "abx-default",
                },
                format!("Begin {trials} trials"),
                Icon::Play,
                false,
                move |this, _, cx| this.begin_a_listening_test(trials, cx),
                self,
                cx,
            )
        };
        body.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(starts(SHORT_TEST))
                .child(starts(TRIALS_BY_DEFAULT))
                .child(starts(LONG_TEST)),
        )
    }

    fn against(&self, testing: Testing, set: &OutputSettings, cx: &mut Context<Self>) -> Div {
        let chosen = &self.test_setup.chosen;
        let column = div().flex().flex_col().gap_3();
        match testing {
            Testing::Equaliser => column,
            Testing::Restoration => column.child(self.against_row(
                "abx-against-restoration",
                "B is",
                chosen.restoration_against(set),
                cx,
                |this, value, cx| {
                    this.test_setup.chosen.restoration = Some(value);
                    cx.notify();
                },
            )),
            Testing::Resampler => column
                .child(self.against_row(
                    "abx-against-quality",
                    "B's quality",
                    chosen.quality_against(set),
                    cx,
                    |this, value, cx| {
                        this.test_setup.chosen.quality = Some(value);
                        cx.notify();
                    },
                ))
                .child(self.against_row(
                    "abx-against-phase",
                    "B's phase",
                    chosen.phase_against(set),
                    cx,
                    |this, value, cx| {
                        this.test_setup.chosen.phase = Some(value);
                        cx.notify();
                    },
                )),
            Testing::Dither => column
                .child(self.against_row(
                    "abx-against-dither",
                    "B's dither",
                    chosen.dither_against(set),
                    cx,
                    |this, value, cx| {
                        this.test_setup.chosen.dither = Some(value);
                        cx.notify();
                    },
                ))
                .child(self.against_row(
                    "abx-against-shaping",
                    "B's noise shaping",
                    chosen.shaping_against(set),
                    cx,
                    |this, value, cx| {
                        this.test_setup.chosen.shaping = Some(value);
                        cx.notify();
                    },
                )),
        }
    }

    fn against_row<T: Choice>(
        &self,
        id: &'static str,
        caption: &'static str,
        held: T,
        cx: &mut Context<Self>,
        taken: impl Fn(&mut Self, T, &mut Context<Self>) + Copy + 'static,
    ) -> Div {
        let mut row = kit::segmented();

        for (index, value) in T::ALL.iter().enumerate() {
            let value = *value;
            row = row.child(self.option_in_the_ring(
                kit::segment((id, index), value.label(), held == value),
                id,
                index,
                move |this, _, cx| taken(this, value, cx),
                cx,
            ));
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(note(caption))
            .child(div().flex().child(row))
    }

    fn begin_a_listening_test(&mut self, trials: usize, cx: &mut Context<Self>) {
        match self.test_setup.testing {
            Testing::Equaliser => self.begin_testing_the_equaliser(trials, cx),
            _ => self.begin_testing_a_setting(trials, cx),
        }
    }

    fn begin_testing_the_equaliser(&mut self, trials: usize, cx: &mut Context<Self>) {
        let rate = self.curve_rate(cx);
        let sink = self
            .player
            .read(cx)
            .sink_in_use()
            .map(|sink| sink.name.clone());
        let chain = self
            .equaliser
            .read(cx)
            .shown()
            .map(|profile| Arc::new(profile.clone()).at_rate(rate));

        let (Some(sink), Some(chain)) = (sink, chain) else {
            self.report(Notice::Trouble(NOTHING_TO_TEST.to_owned()), cx);
            return;
        };
        let Some(test) = ListeningTest::of_the_equaliser(trials, &chain, rate, sink) else {
            self.report(Notice::Trouble(NOT_LEVELLABLE.to_owned()), cx);
            return;
        };
        self.audition = Some(test);
        cx.notify();
    }

    fn begin_testing_a_setting(&mut self, trials: usize, cx: &mut Context<Self>) {
        let engine = self.player.read(cx).engine();
        let set = engine.output_settings();
        let Some(contest) = self
            .test_setup
            .chosen
            .contest(self.test_setup.testing, &set)
        else {
            return;
        };
        let Ok(session) = Session::new(trials, clock_seed()) else {
            self.report(Notice::Trouble(TRIALS_REFUSED.to_owned()), cx);
            return;
        };

        let stop = Arc::new(AtomicBool::new(false));
        let asked = Arc::clone(&stop);
        let begun = cx
            .background_executor()
            .spawn(async move { begin_the_audition(&engine, contest, &asked) });
        let landing = cx.spawn(async move |this, cx| {
            let begun = begun.await;
            let _ = this.update(cx, |this, cx| {
                this.test_setup.measuring.finish();
                match begun {
                    Ok(lowered) => {
                        this.audition =
                            Some(ListeningTest::of_a_setting(session, contest, lowered));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "the listening test could not begin");
                        this.report(Notice::Trouble(refusal(&error)), cx);
                    }
                }
                cx.notify();
            });
        });

        let what = if contest.is_flat_in_level() {
            STARTING
        } else {
            MEASURING
        };
        self.test_setup.measuring = Measuring::under(stop, what, landing);
        cx.notify();
    }

    fn stop_measuring(&mut self, cx: &mut Context<Self>) {
        self.test_setup.measuring = Measuring::idle();
        self.send(Command::Audition(None), cx);
        cx.notify();
    }

    fn hear_in_the_test(&mut self, hear: Hear, cx: &mut Context<Self>) {
        let Some(test) = self.audition.as_mut() else {
            return;
        };
        let arm = match hear {
            Hear::A => Arm::A,
            Hear::B => Arm::B,
            Hear::X => match test.session.x() {
                Some(x) => x,
                None => return,
            },
        };
        test.heard = match hear {
            Hear::X => None,
            Hear::A => Some(Arm::A),
            Hear::B => Some(Arm::B),
        };
        let command = test.command(arm);
        self.send(command, cx);
        cx.notify();
    }

    fn answer_the_test(&mut self, said: Arm, cx: &mut Context<Self>) {
        let Some(test) = self.audition.as_mut() else {
            return;
        };
        test.last = test.session.answer(said).ok();
        test.heard = None;
        cx.notify();
    }

    pub(crate) fn end_the_listening_test(&mut self, cx: &mut Context<Self>) {
        let Some(test) = self.audition.take() else {
            return;
        };
        match test.subject {
            Subject::Equaliser { .. } => self.tell_the_engine(cx),
            Subject::Setting { .. } => self.send(Command::Audition(None), cx),
        }
        cx.notify();
    }
}

enum Hear {
    A,
    B,
    X,
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use resonate_core::abx::Arm;
    use resonate_engine::EngineConfig;
    use resonate_library::Library;

    use super::*;
    use crate::driven::Driven;

    fn set_as(configure: impl FnOnce(&mut EngineConfig)) -> OutputSettings {
        let mut config = EngineConfig::default();
        configure(&mut config);
        OutputSettings::of(&config)
    }

    #[test]
    fn b_is_the_opposite_of_what_is_set_until_the_listener_chooses() {
        let chosen = Chosen::default();

        let off = set_as(|config| config.restoration = Restoration::Off);
        let extended = set_as(|config| config.restoration = Restoration::Extend);
        assert_eq!(chosen.restoration_against(&off), Restoration::Extend);
        assert_eq!(chosen.restoration_against(&extended), Restoration::Off);

        let fast = set_as(|config| config.quality = Quality::Fast);
        let high = set_as(|config| config.quality = Quality::High);
        assert_eq!(chosen.quality_against(&fast), Quality::VeryHigh);
        assert_eq!(chosen.quality_against(&high), Quality::Fast);

        let plain = set_as(|config| config.dither = DitherKind::None);
        let triangular = set_as(|config| config.dither = DitherKind::Triangular);
        assert_eq!(chosen.dither_against(&plain), DitherKind::Triangular);
        assert_eq!(chosen.dither_against(&triangular), DitherKind::None);
    }

    #[test]
    fn phase_and_shaping_follow_the_settings_until_chosen() {
        let set = set_as(|config| {
            config.filter_phase = FilterPhase::Minimum;
            config.noise_shaping = NoiseShaping::Lipshitz;
        });
        let mut chosen = Chosen::default();

        assert_eq!(chosen.phase_against(&set), FilterPhase::Minimum);
        assert_eq!(chosen.shaping_against(&set), NoiseShaping::Lipshitz);

        chosen.phase = Some(FilterPhase::Linear);
        chosen.shaping = Some(NoiseShaping::None);
        assert_eq!(chosen.phase_against(&set), FilterPhase::Linear);
        assert_eq!(chosen.shaping_against(&set), NoiseShaping::None);
    }

    #[test]
    fn each_test_is_the_contest_of_the_settings_against_the_arm_chosen() {
        let set = set_as(|config| {
            config.restoration = Restoration::Repair;
            config.quality = Quality::High;
            config.filter_phase = FilterPhase::Intermediate;
            config.dither = DitherKind::Triangular;
            config.noise_shaping = NoiseShaping::Threshold;
        });
        let chosen = Chosen {
            restoration: Some(Restoration::Extend),
            quality: Some(Quality::Balanced),
            phase: None,
            dither: None,
            shaping: Some(NoiseShaping::None),
        };

        assert_eq!(chosen.contest(Testing::Equaliser, &set), None);
        assert_eq!(
            chosen.contest(Testing::Restoration, &set),
            Some(Contest::Restoration {
                a: Restoration::Repair,
                b: Restoration::Extend,
            })
        );
        assert_eq!(
            chosen.contest(Testing::Resampler, &set),
            Some(Contest::Resampling {
                a: Resampling {
                    quality: Quality::High,
                    phase: FilterPhase::Intermediate,
                },
                b: Resampling {
                    quality: Quality::Balanced,
                    phase: FilterPhase::Intermediate,
                },
            })
        );
        assert_eq!(
            chosen.contest(Testing::Dither, &set),
            Some(Contest::Dithering {
                a: Dithering {
                    kind: DitherKind::Triangular,
                    shaping: NoiseShaping::Threshold,
                },
                b: Dithering {
                    kind: DitherKind::None,
                    shaping: NoiseShaping::None,
                },
            })
        );
    }

    #[test]
    fn a_test_of_a_setting_hears_the_audition_with_the_arm_and_the_level_it_was_begun_on() {
        let contest = Contest::Dithering {
            a: Dithering {
                kind: DitherKind::None,
                shaping: NoiseShaping::None,
            },
            b: Dithering {
                kind: DitherKind::Triangular,
                shaping: NoiseShaping::None,
            },
        };
        let session = Session::new(4, 7).expect("a session");
        let test = ListeningTest::of_a_setting(session, contest, Lowered::NOTHING);

        for arm in [Arm::A, Arm::B] {
            assert!(matches!(
                test.command(arm),
                Command::Audition(Some(Audition { heard, lowered, .. }))
                    if heard == arm && lowered == Lowered::NOTHING
            ));
        }
        assert_eq!(test.testing(), Testing::Dither);
        assert_eq!(test.levels_said(), "Both play at one level");
    }

    #[test]
    fn a_measured_difference_is_said_as_the_arm_lowered_and_by_how_much() {
        let contest = Contest::Restoration {
            a: Restoration::Off,
            b: Restoration::Extend,
        };
        let by = resonate_core::Trim::from_millibels(12).expect("a trim");
        let lowered = Lowered { arm: Arm::B, by };
        let session = Session::new(4, 7).expect("a session");

        let test = ListeningTest::of_a_setting(session, contest, lowered);

        assert_eq!(test.testing(), Testing::Restoration);
        assert_eq!(
            test.levels_said(),
            "B lowered by 0.12 dB to match the other"
        );

        let equal = ListeningTest::of_a_setting(
            Session::new(4, 7).expect("a session"),
            contest,
            Lowered::NOTHING,
        );
        assert_eq!(
            equal.levels_said(),
            "Measured through both, the two play at one level"
        );
    }

    #[test]
    fn every_refusal_of_the_test_is_a_plain_notice_in_the_windows_words() {
        for error in [
            Error::NothingToAudition,
            Error::ArmsAlike,
            Error::LevelsUnmeasured,
            Error::LevelsTooFarApart { millibels: 5000 },
        ] {
            let said = refusal(&error);

            assert!(!said.ends_with('.'), "{said} reads as a sentence");
            assert!(
                !said.contains("Error"),
                "{said} speaks the engine's language"
            );
            assert!(
                !said.contains("millibel"),
                "{said} speaks the engine's language"
            );
        }

        assert_ne!(
            refusal(&Error::ArmsAlike),
            refusal(&Error::LevelsUnmeasured)
        );
        assert_ne!(
            refusal(&Error::LevelsUnmeasured),
            refusal(&Error::LevelsTooFarApart { millibels: 5000 })
        );
        assert_eq!(
            refusal(&Error::EngineStopped),
            toast::would_not_do(CommandKind::Audition, Error::EngineStopped.cause())
        );
    }

    #[gpui::test]
    fn a_test_of_a_setting_with_nothing_playing_is_refused_in_a_toast_and_begins_nothing(
        cx: &mut TestAppContext,
    ) {
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        let mut driven = Driven::open(cx, library);
        driven.click("tab-settings");
        driven.click("category-Equaliser");
        driven.scroll("settings", 6000.0);

        driven.click("abx-testing-1");
        driven.scroll("settings", 6000.0);
        driven.click("abx-default");
        driven.until(|root, cx| {
            !root.test_setup.measuring.is_busy() && toast::showing_text(cx).is_some()
        });

        assert_eq!(
            driven.read(|_, cx| toast::showing_text(cx)).as_deref(),
            Some(NOT_PLAYING)
        );
        assert!(driven.read(|root, _| root.audition.is_none()));

        driven.click("abx-testing-3");
        driven.scroll("settings", 6000.0);
        driven.click("abx-default");
        driven.until(|root, cx| {
            !root.test_setup.measuring.is_busy()
                && toast::showing_text(cx).is_some_and(|said| said.ends_with("(×2)"))
        });

        assert!(driven.read(|root, _| root.audition.is_none()));
    }

    #[gpui::test]
    fn a_test_being_measured_shows_it_and_can_be_stopped_before_it_begins(cx: &mut TestAppContext) {
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        let mut driven = Driven::open(cx, library);
        driven.click("tab-settings");
        driven.click("category-Equaliser");
        driven.scroll("settings", 6000.0);
        driven.click("abx-testing-1");

        let stop = Arc::new(AtomicBool::new(false));
        let watched = Arc::clone(&stop);
        driven.root.update(&mut driven.cx, |root, cx| {
            root.test_setup.measuring = Measuring::under(stop, MEASURING, Task::ready(()));
            cx.notify();
        });
        driven.scroll("settings", 6000.0);

        driven.bounds_of("abx-measuring");
        driven.click("abx-stop-measuring");

        assert!(driven.read(|root, _| !root.test_setup.measuring.is_busy()));
        assert!(watched.load(Ordering::Relaxed));
        assert!(driven.read(|root, _| root.audition.is_none()));
    }
}
