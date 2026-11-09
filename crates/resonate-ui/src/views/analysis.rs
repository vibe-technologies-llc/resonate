use std::{borrow::Cow, sync::Arc, time::Duration};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Div, FontWeight, MouseButton, MouseDownEvent, ObjectFit,
    Pixels, Point, SharedString, Stateful, canvas, div, fill, img, point, prelude::*, px, relative,
    rgb,
};
use resonate_engine::{Analysis, Finding, Judgement, Levels, Loudness, Reach, Spectrum, Verdict};
use resonate_library::{Agreement, HeardAs};

use crate::{
    analysis::{Drawn, Hearing, Row, Studying},
    analysis_plot::{
        SPECTRUM_CEILING_DB, SPECTRUM_FLOOR_DB, SPECTRUM_MARKED_EVERY_DB, frequency_marks,
        height_of, time_marks,
    },
    format,
    icons::{self, Icon},
    models::Notice,
    theme, toast,
    views::{
        hint::{self, Names},
        inspector::{beside, card, fields},
        kit::{self, EndsInAnEllipsis as _},
        plot,
        root::RootView,
        scrollbar::Scrollbars,
        spoken::{self, Spoken as _},
        transport::Playing,
    },
    words,
};

const WAVEFORM_HEIGHT: f32 = 168.0;
const SPECTROGRAM_HEIGHT: f32 = 220.0;
const SPECTRUM_HEIGHT: f32 = 160.0;
const WAVE_FILLS: f32 = 0.92;
const WAVE_ALPHA: u8 = 0x70;
const PLAYED_ALPHA: u8 = 0x14;
const PLAYHEAD_WIDTH: f32 = 2.0;
const SPECTRUM_LINE: f32 = 1.5;
const SPECTRUM_WASH_ALPHA: u8 = 0x30;
const CUTOFF_WIDTH: f32 = 1.5;
const LABEL_ROOM: f32 = 18.0;
const LABEL_GROUND_ALPHA: u8 = 0xc0;
const HEARD_SHOWN: usize = 5;
const HALF: f32 = 0.5;
const HZ_A_KILOHERTZ: f32 = 1_000.0;

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Genuine => words::analysis::genuine(),
        Verdict::Suspect => words::analysis::suspect(),
        Verdict::Fake => words::analysis::fake(),
        Verdict::Lossy => words::analysis::lossy(),
        Verdict::NotJudged => words::analysis::not_judged(),
    }
}

fn verdict_colour(verdict: Verdict) -> u32 {
    match verdict {
        Verdict::Genuine => theme::lossless(),
        Verdict::Suspect => theme::suspect(),
        Verdict::Fake => theme::failure(),
        Verdict::Lossy => theme::lossy(),
        Verdict::NotJudged => theme::faint(),
    }
}

impl RootView {
    pub(crate) fn analysis_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.player.read(cx);
        let state = model.state().clone();
        let digest = model.digest();
        if state.current.is_none() {
            return kit::empty(
                Icon::Analysis,
                words::analysis::nothing_playing(),
                Some(words::analysis::nothing_playing_more()),
            );
        }
        let Some(row) = self.queued_row(&state, cx) else {
            return kit::empty(
                Icon::Analysis,
                words::analysis::nothing_playing(),
                Some(words::analysis::nothing_playing_more()),
            );
        };
        let played = state.current.map_or(0.0, |track| {
            format::progress(track.position, track.duration)
        });

        self.analysis.update(cx, |model, cx| {
            model.follow(
                Row {
                    location: row.location.clone(),
                    span: row.span,
                },
                cx,
            );
        });
        let (studying, hearing, recognises) = {
            let model = self.analysis.read(cx);
            (model.studying(), model.hearing(), model.recognises())
        };

        let playing = self.playing(&state, digest.as_deref(), cx);
        let readback = match &studying {
            Studying::Done(drawn) => Some(read_back(&drawn.analysis)),
            Studying::Running(watch) => Some(watch.share().map_or_else(
                || words::analysis::decoding().to_owned(),
                |share| words::analysis::decoding_share(format!("{:.0}", share * 100.0)),
            )),
            Studying::Idle | Studying::Failed => None,
        };
        let heading = self.analysis_heading(&playing, readback, cx);

        let body = match studying {
            Studying::Idle | Studying::Running(_) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(rgb(theme::muted()))
                .child(words::analysis::analysing())
                .into_any_element(),
            Studying::Failed => kit::trouble(
                Icon::Analysis,
                words::analysis::failed(),
                Some(words::analysis::failed_more()),
            ),
            Studying::Done(drawn) => self.analysed(&drawn, played, &hearing, recognises, cx),
        };

        kit::pane().child(heading).child(body).into_any_element()
    }

    fn analysis_heading(
        &self,
        playing: &Playing,
        readback: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        kit::heading().child(
            kit::heading_row()
                .child(self.playing_heading("analysis-title", "analysis-artist", playing, cx))
                .child(
                    kit::actions()
                        .children(readback.map(kit::figure))
                        .child(hint::explains(
                            "analysis-reads",
                            words::analysis::reads_hint(),
                        )),
                ),
        )
    }

    fn analysed(
        &mut self,
        drawn: &Arc<Drawn>,
        played: f32,
        hearing: &Hearing,
        recognises: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let analysis = &drawn.analysis;
        let stops = vec![theme::surface(), theme::accent(), theme::text()];
        let painted = self
            .analysis
            .update(cx, |model, cx| model.spectrogram(stops, cx));
        let plotted = self.analysis.read(cx).plotted.clone();
        let taking = hearing.leads().then(|| {
            div().flex().pt_2().child(self.in_the_pane_ring(
                kit::button(
                    "take-the-heard-name",
                    Some(Icon::Check),
                    words::analysis::take_the_name(),
                    words::analysis::take_the_name_hint(),
                    kit::Tone::Outlined,
                ),
                |this, _, cx| this.take_the_heard_name(cx),
                cx,
            ))
        });
        let heard = heard_card(hearing, recognises).children(taking);
        let (leading, trailing) = if hearing.leads() {
            (Some(heard), None)
        } else {
            (None, Some(heard))
        };

        let pane = div()
            .id("analysis")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_4()
            .p_6()
            .overflow_y_scroll()
            .track_scroll(&self.analysis_scroll)
            .children(leading)
            .child(verdict_card(analysis))
            .child(
                card(words::analysis::waveform()).child(
                    div()
                        .id("analysis-waveform")
                        .pictured_as(words::analysis::waveform(), || {
                            waveform_said(&analysis.study.levels, &analysis.study.loudness)
                        })
                        .relative()
                        .h(px(WAVEFORM_HEIGHT + LABEL_ROOM))
                        .cursor_pointer()
                        .names(words::analysis::waveform_hint())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                let bounds = plotted.get();
                                let wide = f32::from(bounds.size.width);
                                if wide <= 0.0 {
                                    return;
                                }
                                let across = f32::from(event.position.x - bounds.origin.x) / wide;
                                this.seek_to(across.clamp(0.0, 1.0), cx);
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(WAVEFORM_HEIGHT))
                                .child(waveform(
                                    Arc::clone(&drawn.lanes),
                                    played,
                                    self.analysis.read(cx).plotted.clone(),
                                )),
                        )
                        .children(time_labels(analysis)),
                ),
            )
            .child(spectrogram_card(analysis, painted, played))
            .child(spectrum_card(drawn))
            .child(beside(vec![
                levels_card(analysis).into_any_element(),
                source_card(analysis).into_any_element(),
            ]))
            .children(trailing);

        Scrollbars::of(cx)
            .around("analysis-scrollbar", self.analysis_scroll.clone(), pane)
            .into_any_element()
    }
}

impl RootView {
    fn take_the_heard_name(&mut self, cx: &mut Context<Self>) {
        let taking = self
            .analysis
            .update(cx, |analysis, cx| analysis.take_the_name(cx));
        cx.spawn(async move |this, cx| {
            let taken = taking.await;
            let _ = this.update(cx, |this, cx| {
                let notice = match taken {
                    Ok(Some((track, heard))) => {
                        this.library
                            .update(cx, |library, cx| library.renamed(track, cx));
                        Notice::Done(words::analysis::renamed(billed(&heard)))
                    }
                    Ok(None) => Notice::Trouble(words::analysis::nothing_to_take().to_owned()),
                    Err(error) => {
                        tracing::warn!(%error, "the name the audio was heard as was not taken");
                        toast::could_not(words::analysis::rename_the_track(), &error)
                    }
                };
                this.report(notice, cx);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Hearing {
    pub(crate) const fn leads(&self) -> bool {
        matches!(
            self,
            Self::Heard {
                agreement: Some(Agreement::Disagrees | Agreement::Unnamed),
                ..
            }
        )
    }
}

fn read_back(analysis: &Analysis) -> String {
    let examined = &analysis.study.examined;
    let codec = examined.codec.to_string();
    let rate = examined.rate.kilohertz();

    match examined.declared_bits {
        Some(bits) => words::analysis::read_back_with_depth(bits, codec, rate),
        None => words::analysis::read_back(codec, rate),
    }
}

fn verdict_card(analysis: &Analysis) -> Div {
    let judgement = &analysis.study.judgement;
    let colour = verdict_colour(judgement.verdict);
    let told: Vec<Stateful<Div>> = judgement
        .findings
        .iter()
        .enumerate()
        .map(|(nth, finding)| finding_row(nth, finding))
        .collect();

    kit::card()
        .gap_0()
        .child(
            div()
                .id("verdict")
                .read_plainly_as(|| SharedString::from(judgement.verdict.told()))
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_size(px(theme::text_lg()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colour))
                        .child(verdict_name(judgement.verdict)),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text()))
                        .child(judgement.verdict.told()),
                ),
        )
        .when(!told.is_empty(), |card| {
            card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .mt_3()
                    .pt_3()
                    .border_t_1()
                    .border_color(rgb(theme::soft_edge()))
                    .children(told),
            )
        })
}

fn finding_row(nth: usize, finding: &Finding) -> Stateful<Div> {
    let line = theme::text_sm() * FINDING_LEADING;
    let (mark, said_in) = match Severity::of(finding) {
        Severity::Fault | Severity::Caution => (Icon::Alert, theme::text()),
        Severity::Aside => (Icon::Info, theme::muted()),
    };

    div()
        .id(("finding", nth))
        .read_plainly_as(|| spoken::joined([severity_word(finding), &finding.told()]))
        .flex()
        .items_start()
        .gap_2p5()
        .text_size(px(theme::text_sm()))
        .line_height(px(line))
        .text_color(rgb(said_in))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(px(line))
                .child(icons::icon(
                    mark,
                    theme::text_sm() * FINDING_MARK_EMS,
                    finding_colour(finding),
                )),
        )
        .child(div().flex_1().min_w(px(0.0)).child(finding.told()))
}

const FINDING_MARK_EMS: f32 = 1.2;
const FINDING_LEADING: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Severity {
    Fault,
    Caution,
    Aside,
}

impl Severity {
    const fn of(finding: &Finding) -> Self {
        match finding {
            Finding::Wall { lossy: Some(_), .. }
            | Finding::Upsampled { .. }
            | Finding::Padded { .. }
            | Finding::QuantiserSteps { .. }
            | Finding::IntensityStereo { .. } => Self::Fault,
            Finding::Clipped { .. } | Finding::DcOffset { .. } | Finding::MonoAsStereo => {
                Self::Caution
            }
            Finding::Wall { lossy: None, .. }
            | Finding::Rolloff { .. }
            | Finding::TooLittleHeard
            | Finding::FromDsd => Self::Aside,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Fault => words::analysis::fault(),
            Self::Caution => words::analysis::caution(),
            Self::Aside => words::analysis::note(),
        }
    }
}

fn severity_word(finding: &Finding) -> &'static str {
    Severity::of(finding).word()
}

fn finding_colour(finding: &Finding) -> u32 {
    match Severity::of(finding) {
        Severity::Fault => theme::failure(),
        Severity::Caution => theme::suspect(),
        Severity::Aside => theme::muted(),
    }
}

fn axis_label(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::faint()))
        .whitespace_nowrap()
        .child(text.into())
}

fn time_labels(analysis: &Analysis) -> Vec<Div> {
    time_marks(analysis.study.examined.length)
        .into_iter()
        .map(|mark| {
            axis_label(mark.label)
                .absolute()
                .bottom_0()
                .left(relative(mark.share))
        })
        .collect()
}

type Edge = fn(&Reach) -> f32;

fn filled_between(
    window: &mut gpui::Window,
    reaches: &[Reach],
    edges: (Edge, Edge),
    place: (f32, f32, f32, f32),
    colour: impl Into<gpui::Background>,
) {
    let (left, wide, middle, reach) = place;
    let (upper, lower) = edges;
    let steps = reaches.len().saturating_sub(1).max(1) as f32;
    let at = |nth: usize, level: f32| -> Point<Pixels> {
        point(
            px(left + nth as f32 / steps * wide),
            px(middle - level.clamp(-1.0, 1.0) * reach),
        )
    };
    let over: Vec<Point<Pixels>> = reaches
        .iter()
        .enumerate()
        .map(|(nth, one)| at(nth, upper(one)))
        .collect();
    let under: Vec<Point<Pixels>> = reaches
        .iter()
        .enumerate()
        .map(|(nth, one)| at(nth, lower(one)))
        .collect();
    plot::between(window, &over, &under, colour);
}

fn waveform(
    lanes: Arc<[Vec<Reach>]>,
    played: f32,
    plotted: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
) -> Canvas<()> {
    canvas(
        move |bounds, _, _| plotted.set(bounds),
        move |bounds, (), window, _| {
            let left = f32::from(bounds.origin.x);
            let wide = f32::from(bounds.size.width);
            let top = f32::from(bounds.top());
            let tall = f32::from(bounds.size.height);
            let accent = theme::accent();

            window.paint_quad(fill(
                Bounds::from_corners(
                    point(px(left), px(top)),
                    point(px(left + played * wide), px(top + tall)),
                ),
                theme::tinted(accent, PLAYED_ALPHA),
            ));

            let count = lanes.len().max(1) as f32;
            let lane_tall = tall / count;
            for (nth, lane) in lanes.iter().enumerate() {
                let middle = top + lane_tall * (nth as f32 + HALF);
                let reach = lane_tall * HALF * WAVE_FILLS;
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(px(left), px(middle)),
                        point(px(left + wide), px(middle + 1.0)),
                    ),
                    rgb(theme::border()),
                ));
                filled_between(
                    window,
                    lane,
                    (|reach| reach.high, |reach| reach.low),
                    (left, wide, middle, reach),
                    theme::tinted(accent, WAVE_ALPHA),
                );
                filled_between(
                    window,
                    lane,
                    (|reach| reach.rms, |reach| -reach.rms),
                    (left, wide, middle, reach),
                    rgb(accent),
                );
            }

            let x = left + played.clamp(0.0, 1.0) * wide;
            window.paint_quad(fill(
                Bounds::from_corners(
                    point(px(x - PLAYHEAD_WIDTH * HALF), px(top)),
                    point(px(x + PLAYHEAD_WIDTH * HALF), px(top + tall)),
                ),
                rgb(theme::text()),
            ));
        },
    )
    .absolute()
    .size_full()
}

fn cutoff_share(analysis: &Analysis, nyquist_hz: f32) -> Option<f32> {
    let cutoff = analysis.study.judgement.cutoff?;
    (nyquist_hz > 0.0).then(|| (cutoff.hz as f32 / nyquist_hz).clamp(0.0, 1.0))
}

fn spectrogram_card(
    analysis: &Analysis,
    painted: Option<crate::models::Picture>,
    played: f32,
) -> Div {
    let nyquist = analysis.spectrogram.nyquist_hz();
    let marks = frequency_marks(nyquist);
    let cutoff = cutoff_share(analysis, nyquist as f32);
    let colour = verdict_colour(analysis.study.judgement.verdict);

    let mut plot = div()
        .id("analysis-spectrogram")
        .pictured_as(words::analysis::spectrogram(), || {
            spectrogram_said(
                &analysis.study.judgement,
                nyquist,
                analysis.study.spectrum.heard(),
            )
        })
        .relative()
        .h(px(SPECTROGRAM_HEIGHT))
        .rounded_md()
        .overflow_hidden()
        .bg(rgb(theme::surface()));
    if let Some(image) = painted {
        plot = plot.child(img(image).size_full().object_fit(ObjectFit::Fill));
    }
    plot = plot
        .children(marks.into_iter().map(|mark| {
            axis_label(mark.label)
                .absolute()
                .left_1()
                .bottom(relative(mark.share))
                .px_1()
                .rounded_sm()
                .bg(theme::tinted(theme::surface(), LABEL_GROUND_ALPHA))
        }))
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(played.clamp(0.0, 1.0)))
                .w(px(PLAYHEAD_WIDTH))
                .bg(theme::tinted(theme::text(), 0xa0)),
        );
    if let Some(share) = cutoff {
        plot = plot.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(relative(share))
                .h(px(CUTOFF_WIDTH))
                .bg(rgb(colour)),
        );
    }

    card(words::analysis::spectrogram()).child(plot)
}

fn cutoff_said(judgement: &Judgement) -> String {
    let wall = match judgement.cutoff {
        Some(cutoff) => words::analysis::falls_away(
            format!("{:.0}", cutoff.drop_db.abs()),
            format!("{:.1}", cutoff.hz as f32 / HZ_A_KILOHERTZ),
        ),
        None => words::analysis::no_cutoff().to_owned(),
    };

    format!("{wall}. {}", judgement.verdict.told())
}

fn spectrogram_said(judgement: &Judgement, nyquist_hz: u32, heard: Duration) -> String {
    words::analysis::spectrogram_said(
        cutoff_said(judgement),
        format!("{:.1}", nyquist_hz as f32 / HZ_A_KILOHERTZ),
        format::spanned(heard),
    )
}

fn spectrum_said(judgement: &Judgement, spectrum: &Spectrum) -> String {
    let nyquist = spectrum.nyquist_hz();
    let level = |from: f32, to: f32, named: fn(String) -> String| {
        spectrum
            .band_db(from, to.min(nyquist))
            .map(|db| named(format!("{db:.0}")))
    };
    let averages: Vec<String> = [
        level(0.0, 1_000.0, words::analysis::level_under_one_kilohertz),
        level(
            1_000.0,
            4_000.0,
            words::analysis::level_from_one_to_four_kilohertz,
        ),
        level(
            4_000.0,
            10_000.0,
            words::analysis::level_from_four_to_ten_kilohertz,
        ),
        level(
            10_000.0,
            nyquist,
            words::analysis::level_above_ten_kilohertz,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    match averages.is_empty() {
        true => cutoff_said(judgement),
        false => words::analysis::spectrum_said(cutoff_said(judgement), averages.join(", ")),
    }
}

fn waveform_said(levels: &Levels, loudness: &Loudness) -> String {
    let values: Vec<String> = [
        decibels(levels.peak).map(words::analysis::waveform_peak),
        loudness
            .integrated
            .map(|lufs| words::analysis::waveform_loudness(format!("{lufs:.1}"))),
        loudness
            .dynamic_range
            .map(words::analysis::waveform_dynamic_range),
    ]
    .into_iter()
    .flatten()
    .collect();

    match values.is_empty() {
        true => words::analysis::waveform_hint().to_owned(),
        false => format!(
            "{}; {}",
            words::analysis::waveform_hint(),
            values.join(", ")
        ),
    }
}

fn spectrum_card(drawn: &Drawn) -> Div {
    let analysis = &drawn.analysis;
    let nyquist = analysis.study.spectrum.nyquist_hz();
    let cutoff = cutoff_share(analysis, nyquist);
    let colour = verdict_colour(analysis.study.judgement.verdict);
    let series = Arc::clone(&drawn.spectrum);

    let mut plot = div()
        .id("analysis-spectrum")
        .pictured_as(words::analysis::average_spectrum(), || {
            spectrum_said(&analysis.study.judgement, &analysis.study.spectrum)
        })
        .relative()
        .h(px(SPECTRUM_HEIGHT + LABEL_ROOM))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(SPECTRUM_HEIGHT))
                .child(spectrum_line(series, cutoff, colour)),
        )
        .children(
            frequency_marks(nyquist.round() as u32)
                .into_iter()
                .map(|mark| {
                    axis_label(mark.label)
                        .absolute()
                        .bottom_0()
                        .left(relative(mark.share))
                }),
        );
    if let Some(cutoff) = analysis.study.judgement.cutoff {
        plot = plot.child(
            axis_label(words::analysis::kilohertz(format!(
                "{:.1}",
                cutoff.hz as f32 / HZ_A_KILOHERTZ
            )))
            .absolute()
            .top_0()
            .right_1()
            .text_color(rgb(colour)),
        );
    }

    card(words::analysis::average_spectrum()).child(plot)
}

fn spectrum_line(series: Arc<[f32]>, cutoff: Option<f32>, colour: u32) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let left = f32::from(bounds.origin.x);
            let wide = f32::from(bounds.size.width);
            let top = f32::from(bounds.top());
            let tall = f32::from(bounds.size.height);
            let bottom = top + tall;

            let mut level = SPECTRUM_CEILING_DB;
            while level >= SPECTRUM_FLOOR_DB {
                let y = bottom - height_of(level) * tall;
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(px(left), px(y)),
                        point(px(left + wide), px(y + 1.0)),
                    ),
                    rgb(theme::border()),
                ));
                level -= SPECTRUM_MARKED_EVERY_DB;
            }

            let accent = theme::accent();
            let steps = series.len().saturating_sub(1).max(1) as f32;
            let points: Vec<Point<Pixels>> = series
                .iter()
                .enumerate()
                .map(|(nth, db)| {
                    point(
                        px(left + nth as f32 / steps * wide),
                        px(bottom - height_of(*db) * tall),
                    )
                })
                .collect();
            plot::wash(
                window,
                &points,
                px(bottom),
                theme::tinted(accent, SPECTRUM_WASH_ALPHA),
            );
            plot::stroke(window, &points, SPECTRUM_LINE, rgb(accent));

            if let Some(share) = cutoff {
                let x = left + share * wide;
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(px(x - CUTOFF_WIDTH * HALF), px(top)),
                        point(px(x + CUTOFF_WIDTH * HALF), px(bottom)),
                    ),
                    rgb(colour),
                ));
            }
        },
    )
    .absolute()
    .size_full()
}

fn true_peak(level: f32) -> Option<String> {
    (level > 0.0)
        .then(|| words::analysis::true_peak_reading(format!("{:+.2}", 20.0 * level.log10())))
}

fn decibels(level: f32) -> Option<String> {
    (level > 0.0).then(|| words::analysis::decibels(format!("{:+.2}", 20.0 * level.log10())))
}

fn levels_card(analysis: &Analysis) -> Div {
    let study = &analysis.study;
    let levels = &study.levels;
    let stereo = levels
        .stereo
        .map(|stereo| match (stereo.identical, stereo.correlation) {
            (true, _) => words::analysis::identical().to_owned(),
            (false, Some(correlation)) => format!("{correlation:+.2}"),
            (false, None) => words::analysis::silent().to_owned(),
        });
    let rows: Vec<(&'static str, Option<Cow<'_, str>>)> = vec![
        (
            words::analysis::peak(),
            decibels(levels.peak).map(Cow::Owned),
        ),
        ("RMS", decibels(levels.rms).map(Cow::Owned)),
        (
            words::analysis::true_peak(),
            true_peak(study.loudness.true_peak).map(Cow::Owned),
        ),
        (
            words::analysis::loudness(),
            study
                .loudness
                .integrated
                .map(|lufs| Cow::Owned(words::analysis::lufs(format!("{lufs:.1}")))),
        ),
        (
            words::analysis::loudness_range(),
            study
                .loudness
                .range
                .map(|lu| Cow::Owned(words::analysis::loudness_units(format!("{lu:.1}")))),
        ),
        (
            words::analysis::momentary_max(),
            study
                .loudness
                .momentary_max
                .map(|lufs| Cow::Owned(words::analysis::lufs(format!("{lufs:.1}")))),
        ),
        (
            words::analysis::short_term_max(),
            study
                .loudness
                .short_term_max
                .map(|lufs| Cow::Owned(words::analysis::lufs(format!("{lufs:.1}")))),
        ),
        (
            words::analysis::dynamic_range(),
            study
                .loudness
                .dynamic_range
                .map(|dr| Cow::Owned(format!("DR{dr}"))),
        ),
        (
            words::analysis::clipped(),
            Some(Cow::Owned(words::analysis::clipped_in_runs(
                levels.clipped_runs,
                levels.clipped,
            ))),
        ),
        (
            words::analysis::dc_offset(),
            decibels(levels.dc).map(Cow::Owned),
        ),
        (
            words::analysis::channel_correlation(),
            stereo.map(Cow::Owned),
        ),
    ];
    fields(card(words::analysis::levels()), rows)
}

fn source_card(analysis: &Analysis) -> Div {
    let study = &analysis.study;
    let examined = &study.examined;
    let rows: Vec<(&'static str, Option<Cow<'_, str>>)> = vec![
        (
            words::analysis::codec(),
            Some(Cow::Owned(examined.codec.to_string())),
        ),
        (
            words::analysis::declared_depth(),
            examined
                .declared_bits
                .map(|bits| Cow::Owned(words::analysis::bits(bits))),
        ),
        (
            words::analysis::bits_in_use(),
            study
                .levels
                .bits_in_use
                .map(|bits| Cow::Owned(words::analysis::bits(bits))),
        ),
        (
            words::analysis::rate(),
            Some(Cow::Owned(words::analysis::kilohertz(
                examined.rate.kilohertz(),
            ))),
        ),
        (
            words::analysis::content_ends(),
            study
                .judgement
                .cutoff
                .map(|cutoff| cutoff.hz)
                .or(study.judgement.extent_hz)
                .map(|hz| {
                    Cow::Owned(words::analysis::kilohertz(format!(
                        "{:.1}",
                        hz as f32 / HZ_A_KILOHERTZ
                    )))
                }),
        ),
        (
            words::analysis::heard(),
            Some(Cow::Owned(format::spanned(study.spectrum.heard()))),
        ),
        (
            words::analysis::fingerprint(),
            study
                .print
                .as_ref()
                .map(|print| Cow::Owned(words::analysis::characters(print.encoded().len()))),
        ),
    ];
    fields(card(words::analysis::source()), rows)
}

fn heard_card(hearing: &Hearing, recognises: bool) -> Div {
    let said = |text: &'static str| {
        div()
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .child(text)
    };
    let panel = card(words::analysis::recognition());
    match hearing {
        Hearing::Unasked | Hearing::Asking if recognises => {
            panel.child(said(words::analysis::asking()))
        }
        Hearing::Unasked | Hearing::Asking | Hearing::Unserved => {
            panel.child(said(words::analysis::no_service()))
        }
        Hearing::Unprinted => panel.child(said(words::analysis::no_print())),
        Hearing::Refused => panel.child(said(words::analysis::refused())),
        Hearing::Heard { matches, .. } if matches.is_empty() => {
            panel.child(said(words::analysis::unheard()))
        }
        Hearing::Heard { matches, agreement } => panel
            .children(agreement.map(|agreement| agreed(agreement, matches.first())))
            .children(matches.iter().take(HEARD_SHOWN).map(heard_row)),
    }
}

fn agreed(agreement: Agreement, top: Option<&HeardAs>) -> Div {
    let (text, colour) = match agreement {
        Agreement::Agrees => (words::analysis::agrees().to_owned(), theme::lossless()),
        Agreement::Disagrees => (
            top.map_or_else(
                || words::analysis::disagrees().to_owned(),
                |top| words::analysis::disagrees_with(billed(top)),
            ),
            theme::failure(),
        ),
        Agreement::Unnamed => (
            top.map_or_else(
                || words::analysis::unnamed().to_owned(),
                |top| words::analysis::unnamed_heard(billed(top)),
            ),
            theme::converted(),
        ),
        Agreement::Unheard => (words::analysis::unheard().to_owned(), theme::faint()),
    };

    div().pb_1().text_color(rgb(colour)).child(text)
}

fn billed(heard: &HeardAs) -> String {
    match heard.artist.as_deref() {
        Some(artist) => words::analysis::billed(artist, &heard.title),
        None => heard.title.clone(),
    }
}

fn heard_row(heard: &HeardAs) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.0))
                .child(
                    div()
                        .text_color(rgb(theme::text()))
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(heard.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::faint()))
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(heard.artist.clone().unwrap_or_default()),
                ),
        )
        .child(kit::figure(format!("{} %", heard.score)))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use gpui::TestAppContext;
    use resonate_core::{Mbid, SourceId};
    use resonate_engine::{Cutoff, Judgement, Levels, Loudness, Spectrum, Verdict};
    use resonate_library::{
        Credit, Fingerprinters, Fingerprints, Library, Printed, RecordingMatch, Sounded, TrackQuery,
    };

    use super::{spectrogram_said, spectrum_said, verdict_colour, waveform_said};
    use crate::{
        Pane,
        analysis::Hearing,
        driven::{Driven, Folder},
    };

    const HEARD_AS: &str = "0e1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7";

    fn walled_at_16_khz() -> Judgement {
        Judgement {
            verdict: Verdict::Fake,
            cutoff: Some(Cutoff {
                hz: 16_000,
                drop_db: -45.0,
            }),
            extent_hz: None,
            findings: Vec::new(),
        }
    }

    fn unjudged() -> Judgement {
        Judgement {
            verdict: Verdict::NotJudged,
            cutoff: None,
            extent_hz: None,
            findings: Vec::new(),
        }
    }

    fn levels_peaking_at(peak: f32) -> Levels {
        Levels {
            peak,
            rms: 0.1,
            dc: 0.0,
            clipped: 0,
            clipped_runs: 0,
            bits_in_use: None,
            stereo: None,
        }
    }

    fn loudness_of(integrated: Option<f32>, dynamic_range: Option<u8>) -> Loudness {
        Loudness {
            integrated,
            range: None,
            momentary_max: None,
            short_term_max: None,
            dynamic_range,
            true_peak: 0.0,
        }
    }

    #[test]
    fn a_waveform_is_heard_with_its_peak_its_loudness_and_its_dynamic_range() {
        let said = waveform_said(&levels_peaking_at(0.5), &loudness_of(Some(-9.84), Some(8)));

        assert_eq!(
            said,
            "Press anywhere on the waveform to play from there; \
             peak -6.02 dBFS, loudness -9.8 LUFS, dynamic range DR8"
        );
        assert_eq!(
            waveform_said(&levels_peaking_at(0.0), &loudness_of(None, None)),
            "Press anywhere on the waveform to play from there"
        );
    }

    #[test]
    fn a_spectrogram_is_heard_with_its_wall_its_verdict_and_how_far_it_reaches() {
        let said = spectrogram_said(&walled_at_16_khz(), 22_050, Duration::from_secs(185));

        assert!(said.starts_with("The sound falls away near 16.0 kHz, 45 dB down. Fake"));
        assert!(
            said.ends_with("Frequency up to 22.0 kHz over 3:05"),
            "{said}"
        );
        assert!(
            spectrogram_said(&unjudged(), 22_050, Duration::from_secs(5))
                .starts_with("No cutoff was found. Not judged")
        );
    }

    #[test]
    fn an_average_spectrum_is_heard_with_its_level_in_each_stretch_of_the_band() {
        let mut levels = vec![-30.0; 10];
        levels.resize(23, -90.0);
        let spectrum = Spectrum::new(1_000.0, levels, Duration::from_secs(60));

        let said = spectrum_said(&walled_at_16_khz(), &spectrum);

        assert!(said.starts_with("The sound falls away near 16.0 kHz, 45 dB down. Fake"));
        assert!(
            said.ends_with(
                "Average level -30 dB under 1 kHz, -30 dB from 1 to 4 kHz, \
             -30 dB from 4 to 10 kHz, -90 dB above 10 kHz"
            ),
            "{said}"
        );
    }

    struct HearsEchoes {
        source: SourceId,
    }

    impl Fingerprints for HearsEchoes {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn recognise(&self, _: &Sounded) -> resonate_library::Result<Printed> {
            Ok(Printed::Recognised(vec![RecordingMatch {
                recording: Mbid::new(HEARD_AS).expect("a recording id"),
                score: 100,
                title: "Echoes".to_owned(),
                credit: vec![Credit {
                    name: "Pink Floyd".to_owned(),
                    joined_by: String::new(),
                    mbid: None,
                }],
                length: None,
                isrcs: Vec::new(),
                releases: Vec::new(),
            }]))
        }
    }

    #[gpui::test]
    fn the_name_the_audio_was_heard_as_is_taken_by_the_press_that_offers_it(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let misnamed = folder.tagged(
            "misnamed.wav",
            20,
            &[(b"INAM", "Track 03"), (b"IART", "Somebody")],
        );
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        Driven::scanned(&library, &folder);
        let printers = Fingerprinters::none().and(Arc::new(HearsEchoes {
            source: SourceId::new("hears-echoes").expect("a source name"),
        }));
        let mut driven = Driven::recognising(cx, Arc::clone(&library), &folder, printers);

        driven.play(&[misnamed]);
        driven.root.update(&mut driven.cx, |root, cx| {
            root.set_pane(Pane::Analysis, cx);
        });
        driven.until(|root, cx| matches!(root.analysis.read(cx).hearing(), Hearing::Heard { .. }));
        driven.click("take-the-heard-name");
        driven.until(|_, _| {
            library
                .tracks(&TrackQuery::default())
                .is_ok_and(|tracks| tracks.iter().any(|track| track.title == "Echoes"))
        });

        let track = library
            .tracks(&TrackQuery::default())
            .expect("the catalog reads")
            .remove(0);
        assert_eq!(track.artist.as_deref(), Some("Pink Floyd"));
    }

    #[test]
    fn every_verdict_is_drawn_in_a_colour_of_its_own() {
        let verdicts = [
            Verdict::Genuine,
            Verdict::Suspect,
            Verdict::Fake,
            Verdict::Lossy,
            Verdict::NotJudged,
        ];

        let drawn = verdicts.map(verdict_colour);

        for (at, colour) in drawn.iter().enumerate() {
            assert!(
                !drawn[..at].contains(colour),
                "{:?} is drawn in the colour of another verdict",
                verdicts[at]
            );
        }
    }
}
