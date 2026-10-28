use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{Context, Div, div, prelude::*, px, rgb};
use resonate_core::{
    abx::{Arm, Arms, Score, Session, TRIALS_BY_DEFAULT, Verdict},
    eq::Profile,
};
use resonate_engine::{Command, Equalisation, NodeName};

use crate::{
    Notice,
    icons::Icon,
    theme,
    views::{
        kit,
        root::RootView,
        settings::{action, hugging, note},
    },
};

const SHORT_TEST: usize = TRIALS_BY_DEFAULT / 2;
const LONG_TEST: usize = TRIALS_BY_DEFAULT * 2;

const NOTHING_TO_TEST: &str = "Nothing is bound to the device in use, so there is no chain to \
                               test. Shape a curve under Bands first.";

const NOT_LEVELLABLE: &str = "The chain cannot be level-matched to its bypass, so it is not \
                              tested";

const HOW_IT_WORKS: &str = "A is the equaliser as set, B is the signal with it bypassed. Each is \
                            lowered, never raised, until their loudness across 20 Hz to 16 kHz \
                            agrees to 0.1 dB, so the louder one cannot give itself away. X is one \
                            of the two, drawn afresh for every trial: hear A, B and X as often \
                            as you like on the song playing, then say which X was. A change is \
                            heard a few moments after you press, within the audio already \
                            buffered. The room correction stays on in both.";

pub(crate) struct Audition {
    session: Session,
    arms: Arms,
    sink: NodeName,
    heard: Option<Arm>,
    last: Option<Verdict>,
}

impl Audition {
    fn begin(
        trials: usize,
        chain: &Profile,
        rate: resonate_core::SampleRate,
        sink: NodeName,
    ) -> Option<Self> {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        let session = Session::new(trials, seed).ok()?;
        let arms = Arms::matched(chain, rate).ok()?;
        Some(Self {
            session,
            arms,
            sink,
            heard: None,
            last: None,
        })
    }

    fn equalisation(&self, arm: Arm) -> Equalisation {
        Equalisation::auditioning(self.sink.clone(), Arc::new(self.arms.profile(arm).clone()))
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

impl RootView {
    pub(super) fn abx_group(&mut self, cx: &mut Context<Self>) -> Div {
        let mut body = kit::section_body().child(note(HOW_IT_WORKS));

        let Some(audition) = self.audition.as_ref() else {
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
            return body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(starts(SHORT_TEST))
                    .child(starts(TRIALS_BY_DEFAULT))
                    .child(starts(LONG_TEST)),
            );
        };

        let finished = audition.session.is_finished();
        let heard = audition.heard;
        let standing = audition.standing();
        let line = verdicts_in_a_line(&audition.session);
        let residual = audition.arms.residual_db();
        let last = audition.last;
        let significant = audition.session.score().is_significant();

        body = body
            .child(
                div()
                    .text_size(px(theme::text_base()))
                    .text_color(rgb(theme::text()))
                    .child(standing),
            )
            .child(note(format!(
                "Levels matched to {residual:.3} dB. {}",
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

    fn begin_a_listening_test(&mut self, trials: usize, cx: &mut Context<Self>) {
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
        let Some(audition) = Audition::begin(trials, &chain, rate, sink) else {
            self.report(Notice::Trouble(NOT_LEVELLABLE.to_owned()), cx);
            return;
        };
        self.audition = Some(audition);
        cx.notify();
    }

    fn hear_in_the_test(&mut self, hear: Hear, cx: &mut Context<Self>) {
        let Some(audition) = self.audition.as_mut() else {
            return;
        };
        let arm = match hear {
            Hear::A => Arm::A,
            Hear::B => Arm::B,
            Hear::X => match audition.session.x() {
                Some(x) => x,
                None => return,
            },
        };
        audition.heard = match hear {
            Hear::X => None,
            Hear::A => Some(Arm::A),
            Hear::B => Some(Arm::B),
        };
        let equalisation = audition.equalisation(arm);
        self.send(Command::SetEqualisation(Arc::new(equalisation)), cx);
        cx.notify();
    }

    fn answer_the_test(&mut self, said: Arm, cx: &mut Context<Self>) {
        let Some(audition) = self.audition.as_mut() else {
            return;
        };
        audition.last = audition.session.answer(said).ok();
        audition.heard = None;
        cx.notify();
    }

    pub(crate) fn end_the_listening_test(&mut self, cx: &mut Context<Self>) {
        if self.audition.take().is_none() {
            return;
        }
        self.tell_the_engine(cx);
        cx.notify();
    }
}

enum Hear {
    A,
    B,
    X,
}
