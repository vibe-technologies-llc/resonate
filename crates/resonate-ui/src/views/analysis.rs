use std::{borrow::Cow, sync::Arc};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Div, MouseButton, MouseDownEvent, ObjectFit, PathBuilder,
    Pixels, Point, SharedString, canvas, div, fill, img, point, prelude::*, px, relative, rgb,
};
use resonate_engine::{Analysis, Finding, Reach, Verdict};
use resonate_library::{Agreement, HeardAs};

use crate::{
    Selection,
    analysis::{Drawn, Hearing, Row, Studying},
    analysis_plot::{
        SPECTRUM_CEILING_DB, SPECTRUM_FLOOR_DB, SPECTRUM_MARKED_EVERY_DB, frequency_marks,
        height_of, time_marks,
    },
    format,
    icons::Icon,
    models::Notice,
    theme, toast,
    views::{
        browser::{OPEN_ALBUM_HINT, OPEN_ARTIST_HINT},
        hint::{self, Names},
        inspector::{beside, card, fields},
        kit,
        root::RootView,
        scrollbar::Scrollbars,
        transport::Playing,
    },
};

const NOTHING_PLAYING: &str = "Nothing is playing, so there is nothing to analyse.";

const NOTHING_PLAYING_MORE: &str = "The pane decodes the playing track from end to end: its \
                                    waveform, its spectrum, whether its lossless claim holds and \
                                    what its audio is.";

const FAILED: &str = "This track could not be decoded to the end.";

const FAILED_MORE: &str = "The file may be truncated or damaged; the log says where the decoder \
                           stopped.";

const ANALYSING: &str = "Decoding the whole track…";

const READS_HINT: &str = "The whole track decoded once, off the audio thread. A lossy encode \
                          throws away what lies above a fixed frequency, so a lossless file made \
                          from one shows a wall in its spectrum where the encoder cut; an \
                          upsample shows the wall where the lower rate ended; padded bits are \
                          low bits that never move.";

const TAKE_THE_NAME: &str = "Take this name";

const NOTHING_TO_TAKE: &str = "The catalog holds no recognition of this track to take a name from";

const TAKE_THE_NAME_HINT: &str = "Write the title, the artist and the recording the audio was \
                                  heard as into the catalog in place of what the file names. The \
                                  file itself is left as it is.";

const WAVEFORM_HINT: &str = "Press anywhere on the waveform to play from there";

const NO_SERVICE: &str = "No recognition service is set. An AcoustID key under Settings › Online \
                          lets the pane say what the audio is, and the lookup names tracks the \
                          catalog could not.";

const NO_PRINT: &str = "This stream could not be fingerprinted.";

const REFUSED: &str = "The recognition service refused or could not be reached.";

const ASKING: &str = "Asking what the audio is…";

const UNHEARD: &str = "Nothing the service holds sounds like this.";

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

fn verdict_colour(verdict: Verdict) -> u32 {
    match verdict {
        Verdict::Genuine => theme::lossless(),
        Verdict::Suspect => theme::converted(),
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
            return kit::empty(Icon::Analysis, NOTHING_PLAYING, Some(NOTHING_PLAYING_MORE));
        }
        let Some(row) = self.queued_row(&state, cx) else {
            return kit::empty(Icon::Analysis, NOTHING_PLAYING, Some(NOTHING_PLAYING_MORE));
        };
        let played = state.current.map_or(0.0, |track| {
            format::progress(track.position, track.duration)
        });

        self.analysis.update(cx, |model, cx| {
            model.follow(
                Row {
                    location: row.location,
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
                || "decoding".to_owned(),
                |share| format!("decoding · {:.0} %", share * 100.0),
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
                .child(ANALYSING)
                .into_any_element(),
            Studying::Failed => kit::empty(Icon::Analysis, FAILED, Some(FAILED_MORE)),
            Studying::Done(drawn) => self.analysed(&drawn, played, &hearing, recognises, cx),
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .child(body)
            .into_any_element()
    }

    fn analysis_heading(
        &self,
        playing: &Playing,
        readback: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(kit::eyebrow("ANALYSIS"))
                        .child(kit::linked_title(self.opens(
                            "analysis-title",
                            playing.title.clone(),
                            OPEN_ALBUM_HINT,
                            playing.cover.album.map(Selection::Album),
                            cx,
                        )))
                        .child(kit::subtitle(self.opens(
                            "analysis-artist",
                            playing.artist.clone(),
                            OPEN_ARTIST_HINT,
                            playing.artist_id.map(Selection::Artist),
                            cx,
                        ))),
                )
                .child(
                    kit::actions()
                        .children(readback.map(kit::figure))
                        .child(hint::explains("analysis-reads", READS_HINT)),
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
            div().flex().pt_2().child(
                kit::button(
                    "take-the-heard-name",
                    Some(Icon::Check),
                    TAKE_THE_NAME,
                    TAKE_THE_NAME_HINT,
                    kit::Tone::Outlined,
                )
                .on_click(cx.listener(|this, _, _, cx| this.take_the_heard_name(cx))),
            )
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
            .px_6()
            .py_5()
            .overflow_y_scroll()
            .track_scroll(&self.analysis_scroll)
            .children(leading)
            .child(verdict_card(analysis))
            .child(
                card("Waveform").child(
                    div()
                        .id("analysis-waveform")
                        .relative()
                        .h(px(WAVEFORM_HEIGHT + LABEL_ROOM))
                        .cursor_pointer()
                        .names(WAVEFORM_HINT)
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
                        Notice::Done(format!("The track is now {}", billed(&heard)))
                    }
                    Ok(None) => Notice::Trouble(NOTHING_TO_TAKE.to_owned()),
                    Err(error) => {
                        tracing::warn!(%error, "the name the audio was heard as was not taken");
                        toast::could_not("rename the track", &error)
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
    let depth = examined
        .declared_bits
        .map_or_else(String::new, |bits| format!(" · {bits}-bit"));
    format!(
        "{}{depth} · {} kHz",
        examined.codec,
        format::kilohertz(examined.rate)
    )
}

fn verdict_card(analysis: &Analysis) -> Div {
    let judgement = &analysis.study.judgement;
    let colour = verdict_colour(judgement.verdict);
    let told: Vec<Div> = judgement
        .findings
        .iter()
        .map(|finding| {
            div()
                .flex()
                .items_start()
                .gap_2()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .child(kit::mode_dot(finding_colour(finding)).mt(px(6.0)))
                .child(div().flex_1().min_w(px(0.0)).child(finding.told()))
        })
        .collect();

    kit::card()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(kit::badge(
                    judgement.verdict.as_str().to_uppercase(),
                    colour,
                ))
                .child(
                    div()
                        .text_color(rgb(theme::text()))
                        .child(judgement.verdict.told()),
                ),
        )
        .children(told)
}

fn finding_colour(finding: &Finding) -> u32 {
    match finding {
        Finding::Wall { lossy: Some(_), .. }
        | Finding::Upsampled { .. }
        | Finding::Padded { .. } => theme::failure(),
        Finding::Clipped { .. } | Finding::DcOffset { .. } | Finding::MonoAsStereo => {
            theme::converted()
        }
        Finding::Wall { lossy: None, .. }
        | Finding::Rolloff { .. }
        | Finding::TooLittleHeard
        | Finding::FromDsd => theme::faint(),
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
    let Some(first) = reaches.first() else {
        return;
    };

    let mut path = PathBuilder::fill();
    path.move_to(at(0, upper(first)));
    for (nth, one) in reaches.iter().enumerate().skip(1) {
        path.line_to(at(nth, upper(one)));
    }
    for (nth, one) in reaches.iter().enumerate().rev() {
        path.line_to(at(nth, lower(one)));
    }
    path.close();
    match path.build() {
        Ok(built) => window.paint_path(built, colour),
        Err(error) => tracing::debug!(%error, "the waveform would not tessellate"),
    }
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

fn spectrogram_card(analysis: &Analysis, painted: Option<Arc<gpui::Image>>, played: f32) -> Div {
    let nyquist = analysis.spectrogram.nyquist_hz();
    let marks = frequency_marks(nyquist);
    let cutoff = cutoff_share(analysis, nyquist as f32);
    let colour = verdict_colour(analysis.study.judgement.verdict);

    let mut plot = div()
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

    card("Spectrogram").child(plot)
}

fn spectrum_card(drawn: &Drawn) -> Div {
    let analysis = &drawn.analysis;
    let nyquist = analysis.study.spectrum.nyquist_hz();
    let cutoff = cutoff_share(analysis, nyquist);
    let colour = verdict_colour(analysis.study.judgement.verdict);
    let series = Arc::clone(&drawn.spectrum);

    let mut plot = div()
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
            axis_label(format!("{:.1} kHz", cutoff.hz as f32 / HZ_A_KILOHERTZ))
                .absolute()
                .top_0()
                .right_1()
                .text_color(rgb(colour)),
        );
    }

    card("Average spectrum").child(plot)
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
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                let mut wash = PathBuilder::fill();
                wash.move_to(point(first.x, px(bottom)));
                for at in &points {
                    wash.line_to(*at);
                }
                wash.line_to(point(last.x, px(bottom)));
                wash.close();
                if let Ok(built) = wash.build() {
                    window.paint_path(built, theme::tinted(accent, SPECTRUM_WASH_ALPHA));
                }

                let mut line = PathBuilder::stroke(px(SPECTRUM_LINE));
                line.move_to(*first);
                for at in points.iter().skip(1) {
                    line.line_to(*at);
                }
                match line.build() {
                    Ok(built) => window.paint_path(built, rgb(accent)),
                    Err(error) => tracing::debug!(%error, "the spectrum would not tessellate"),
                }
            }

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
    (level > 0.0).then(|| format!("{:+.2} dBTP", 20.0 * level.log10()))
}

fn decibels(level: f32) -> Option<String> {
    (level > 0.0).then(|| format!("{:+.2} dBFS", 20.0 * level.log10()))
}

fn levels_card(analysis: &Analysis) -> Div {
    let study = &analysis.study;
    let levels = &study.levels;
    let stereo = levels
        .stereo
        .map(|stereo| match (stereo.identical, stereo.correlation) {
            (true, _) => "identical".to_owned(),
            (false, Some(correlation)) => format!("{correlation:+.2}"),
            (false, None) => "silent".to_owned(),
        });
    let rows: Vec<(&'static str, Option<Cow<'_, str>>)> = vec![
        ("peak", decibels(levels.peak).map(Cow::Owned)),
        ("rms", decibels(levels.rms).map(Cow::Owned)),
        (
            "true peak",
            true_peak(study.loudness.true_peak).map(Cow::Owned),
        ),
        (
            "loudness",
            study
                .loudness
                .integrated
                .map(|lufs| Cow::Owned(format!("{lufs:.1} LUFS"))),
        ),
        (
            "loudness range",
            study
                .loudness
                .range
                .map(|lu| Cow::Owned(format!("{lu:.1} LU"))),
        ),
        (
            "momentary max",
            study
                .loudness
                .momentary_max
                .map(|lufs| Cow::Owned(format!("{lufs:.1} LUFS"))),
        ),
        (
            "short-term max",
            study
                .loudness
                .short_term_max
                .map(|lufs| Cow::Owned(format!("{lufs:.1} LUFS"))),
        ),
        (
            "dynamic range",
            study
                .loudness
                .dynamic_range
                .map(|dr| Cow::Owned(format!("DR{dr}"))),
        ),
        (
            "clipped",
            Some(Cow::Owned(format!(
                "{} in {} runs",
                levels.clipped, levels.clipped_runs
            ))),
        ),
        ("dc offset", decibels(levels.dc).map(Cow::Owned)),
        ("channel correlation", stereo.map(Cow::Owned)),
    ];
    fields(card("Levels"), rows)
}

fn source_card(analysis: &Analysis) -> Div {
    let study = &analysis.study;
    let examined = &study.examined;
    let rows: Vec<(&'static str, Option<Cow<'_, str>>)> = vec![
        ("codec", Some(Cow::Owned(examined.codec.to_string()))),
        (
            "declared depth",
            examined
                .declared_bits
                .map(|bits| Cow::Owned(format!("{bits} bits"))),
        ),
        (
            "bits in use",
            study
                .levels
                .bits_in_use
                .map(|bits| Cow::Owned(format!("{bits} bits"))),
        ),
        (
            "rate",
            Some(Cow::Owned(format!(
                "{} kHz",
                format::kilohertz(examined.rate)
            ))),
        ),
        (
            "content ends",
            study
                .judgement
                .cutoff
                .map(|cutoff| cutoff.hz)
                .or(study.judgement.extent_hz)
                .map(|hz| Cow::Owned(format!("{:.1} kHz", hz as f32 / HZ_A_KILOHERTZ))),
        ),
        (
            "heard",
            Some(Cow::Owned(format::spanned(study.spectrum.heard()))),
        ),
        (
            "fingerprint",
            study
                .print
                .as_ref()
                .map(|print| Cow::Owned(format!("{} characters", print.encoded().len()))),
        ),
    ];
    fields(card("Source"), rows)
}

fn heard_card(hearing: &Hearing, recognises: bool) -> Div {
    let said = |text: &'static str| {
        div()
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .child(text)
    };
    let panel = card("Recognition");
    match hearing {
        Hearing::Unasked | Hearing::Asking if recognises => panel.child(said(ASKING)),
        Hearing::Unasked | Hearing::Asking | Hearing::Unserved => panel.child(said(NO_SERVICE)),
        Hearing::Unprinted => panel.child(said(NO_PRINT)),
        Hearing::Refused => panel.child(said(REFUSED)),
        Hearing::Heard { matches, .. } if matches.is_empty() => panel.child(said(UNHEARD)),
        Hearing::Heard { matches, agreement } => panel
            .children(agreement.map(|agreement| agreed(agreement, matches.first())))
            .children(matches.iter().take(HEARD_SHOWN).map(heard_row)),
    }
}

fn agreed(agreement: Agreement, top: Option<&HeardAs>) -> Div {
    let (text, colour) = match agreement {
        Agreement::Agrees => (
            "The audio is the song the file names.".to_owned(),
            theme::lossless(),
        ),
        Agreement::Disagrees => (
            top.map_or_else(
                || "The audio is another song than the file names.".to_owned(),
                |top| {
                    format!(
                        "The audio is another song than the file names: {}",
                        billed(top)
                    )
                },
            ),
            theme::failure(),
        ),
        Agreement::Unnamed => (
            top.map_or_else(
                || "The file names nothing.".to_owned(),
                |top| format!("The file names nothing; the audio is {}", billed(top)),
            ),
            theme::converted(),
        ),
        Agreement::Unheard => (UNHEARD.to_owned(), theme::faint()),
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .pb_1()
        .child(kit::mode_dot(colour))
        .child(div().text_color(rgb(theme::text())).child(text))
}

fn billed(heard: &HeardAs) -> String {
    match heard.artist.as_deref() {
        Some(artist) => format!("{} by {artist}", heard.title),
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
                        .child(heard.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::faint()))
                        .truncate()
                        .child(heard.artist.clone().unwrap_or_default()),
                ),
        )
        .child(kit::figure(format!("{} %", heard.score)))
}
