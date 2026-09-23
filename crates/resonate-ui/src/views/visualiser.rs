use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Corners, Div, Entity, PathBuilder, Pixels, Point, Render,
    SharedString, Task, Window, canvas, div, fill, linear_color_stop, linear_gradient, point,
    prelude::*, px, rgb,
};
use resonate_core::{Frames, SampleRate};
use resonate_engine::{Caught, Tap, Tapped};

use crate::{
    PlayerModel, Selection, format,
    icons::Icon,
    spectrum::{CEILING_DB, Column, FLOOR_DB, MARKED_EVERY_DB, Spectrum, height_of, rising_edge},
    theme,
    views::{
        browser::{OPEN_ALBUM_HINT, OPEN_ARTIST_HINT},
        hint::{self, Names},
        kit,
        root::RootView,
        settings::{across_at, marked_frequencies},
        transport::Playing,
    },
};

const NOTHING_PLAYING: &str = "Nothing is playing, so there is nothing to draw.";

const MARKED: &str = "A DSD stream sent as DoP carries markers rather than samples, so there is \
                      nothing here to draw.";

const MARKED_MORE: &str = "With DoP off under Output the stream is decimated to PCM, and the \
                           pane draws that.";

const READS_HINT: &str = "What reaches the device — after the equaliser, the volume and the \
                          dither — drawn as it is heard rather than as it is decoded. The \
                          spectrum is sixth-octave bands, tilted up 3 dB an octave about 1 kHz \
                          so that pink noise stands level, falling back slowly with each band's \
                          peak held above it.";

const SPECTRUM_HINT: &str = "Sixth-octave bands from 20 Hz to 20 kHz, with each band's peak held";

const SCOPE_HINT: &str = "The waveform, left over right, held still on a rising edge";

const DRAWN_BEFORE_ANYTHING_IS_HEARD: SampleRate = SampleRate::HZ_48000;
const SCOPE_SPAN: Duration = Duration::from_millis(20);
const LABEL_ROOM: f32 = 22.0;
const HEAD_ROOM: f32 = 12.0;
const BAR_GAP: f32 = 3.0;
const BAR_ROUNDING: f32 = 2.0;
const PEAK_THICKNESS: f32 = 2.0;
const BAR_FOOT_ALPHA: u8 = 0x30;
const TRACE_LINE: f32 = 1.5;
const TRACE_POINTS_AT_MOST: usize = 960;
const RIGHT_TRACE_ALPHA: u8 = 0x80;
const SCOPE_FILLS: f32 = 0.9;
const HALF: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Showing {
    #[default]
    Spectrum,
    Scope,
}

impl Showing {
    const ALL: [Self; 2] = [Self::Spectrum, Self::Scope];

    const fn label(self) -> &'static str {
        match self {
            Self::Spectrum => "Spectrum",
            Self::Scope => "Scope",
        }
    }

    const fn about(self) -> &'static str {
        match self {
            Self::Spectrum => SPECTRUM_HINT,
            Self::Scope => SCOPE_HINT,
        }
    }
}

pub(crate) struct Visualiser {
    player: Entity<PlayerModel>,
    showing: Showing,
    spectrum: Spectrum,
    left: Vec<f32>,
    right: Vec<f32>,
    mid: Vec<f32>,
    traced: Option<usize>,
    drawn_at: Option<Instant>,
    settling: Task<()>,
}

impl Visualiser {
    pub(crate) fn new(player: Entity<PlayerModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&player, |_, _, cx| cx.notify()).detach();

        Self {
            player,
            showing: Showing::default(),
            spectrum: Spectrum::new(DRAWN_BEFORE_ANYTHING_IS_HEARD),
            left: Vec::new(),
            right: Vec::new(),
            mid: Vec::new(),
            traced: None,
            drawn_at: None,
            settling: Task::ready(()),
        }
    }

    pub(crate) const fn showing(&self) -> Showing {
        self.showing
    }

    pub(crate) fn show(&mut self, showing: Showing) {
        self.showing = showing;
    }

    fn readback(&self) -> SharedString {
        let rate = format::kilohertz(self.spectrum.rate());
        match self.showing {
            Showing::Spectrum => format!("{}-point · {rate} kHz", self.spectrum.points()),
            Showing::Scope => format!("{} ms · {rate} kHz", SCOPE_SPAN.as_millis()),
        }
        .into()
    }

    fn catch(&mut self, tap: Option<&Tap>, now: Instant, frames: usize) -> Caught {
        self.left.resize(frames, 0.0);
        self.right.resize(frames, 0.0);
        let caught = match tap {
            Some(tap) => tap.around(now, &mut self.left, &mut self.right),
            None => {
                self.left.fill(0.0);
                self.right.fill(0.0);
                Caught { tapped: 0 }
            }
        };

        self.mid.clear();
        self.mid.extend(
            self.left
                .iter()
                .zip(self.right.iter())
                .map(|(left, right)| (left + right) * HALF),
        );
        caught
    }

    fn read_the_spectrum(&mut self, tap: Option<&Tap>, now: Instant, step: Duration) -> bool {
        let points = self.spectrum.points();
        let caught = self.catch(tap, now, points);
        self.spectrum.take(&self.mid, step);

        caught.tapped == 0 && !self.spectrum.is_at_rest()
    }

    fn read_the_scope(&mut self, tap: Option<&Tap>, now: Instant) {
        let span = spanned(self.spectrum.rate());
        let caught = self.catch(tap, now, span * 2);
        self.traced = (caught.tapped > 0).then(|| rising_edge(&self.mid, span));
    }

    fn settle(&mut self, cx: &mut Context<Self>) {
        self.settling = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(PlayerModel::poll_interval())
                .await;
            let settled = this.update(cx, |_, cx| cx.notify());
            let _ = settled;
        });
    }

    fn trace(&self) -> Option<(Vec<f32>, Vec<f32>)> {
        let from = self.traced?;
        let span = spanned(self.spectrum.rate());
        let step = span.div_ceil(TRACE_POINTS_AT_MOST).max(1);
        let thinned = |levels: &[f32]| -> Vec<f32> {
            levels
                .iter()
                .skip(from)
                .take(span)
                .step_by(step)
                .copied()
                .collect()
        };

        Some((thinned(&self.left), thinned(&self.right)))
    }
}

fn spanned(rate: SampleRate) -> usize {
    usize::try_from(Frames::from_duration(SCOPE_SPAN, rate).get()).unwrap_or(0)
}

impl Render for Visualiser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tapped = self.player.read(cx).tap();
        let tap = match &tapped {
            Tapped::Samples(tap) => Some(tap.as_ref()),
            Tapped::Nothing | Tapped::Markers => None,
        };
        if let Some(rate) = tap.map(Tap::rate)
            && rate != self.spectrum.rate()
        {
            self.spectrum = Spectrum::new(rate);
        }

        let now = Instant::now();
        let step = self
            .drawn_at
            .map_or(Duration::ZERO, |then| now.saturating_duration_since(then));
        self.drawn_at = Some(now);

        let settling = match self.showing {
            Showing::Spectrum => self.read_the_spectrum(tap, now, step),
            Showing::Scope => {
                self.read_the_scope(tap, now);
                false
            }
        };
        if settling {
            self.settle(cx);
        }

        let plot = div().relative().size_full();
        match self.showing {
            Showing::Spectrum => plot
                .child(bars(self.spectrum.columns().collect()))
                .children(marked_frequencies()),
            Showing::Scope => plot.child(traced(self.trace())),
        }
    }
}

fn bars(columns: Vec<Column>) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let left = f32::from(bounds.origin.x);
            let wide = f32::from(bounds.size.width);
            let top = f32::from(bounds.top()) + HEAD_ROOM;
            let bottom = f32::from(bounds.bottom()) - LABEL_ROOM;
            let tall = (bottom - top).max(0.0);
            let across = |hertz: f64| left + across_at(hertz) * wide;
            let down = |height: f32| bottom - height * tall;

            let mut level = CEILING_DB;
            while level >= FLOOR_DB {
                let y = down(height_of(level));
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(px(left), px(y)),
                        point(px(left + wide), px(y + 1.0)),
                    ),
                    rgb(theme::border()),
                ));
                level -= MARKED_EVERY_DB;
            }

            let accent = theme::accent();
            for column in columns {
                let from = across(column.low_hz) + BAR_GAP * HALF;
                let to = across(column.high_hz) - BAR_GAP * HALF;
                if to <= from {
                    continue;
                }
                if column.level > 0.0 {
                    window.paint_quad(
                        fill(
                            Bounds::from_corners(
                                point(px(from), px(down(column.level))),
                                point(px(to), px(bottom)),
                            ),
                            linear_gradient(
                                180.0,
                                linear_color_stop(rgb(accent), 0.0),
                                linear_color_stop(theme::tinted(accent, BAR_FOOT_ALPHA), 1.0),
                            ),
                        )
                        .corner_radii(Corners {
                            top_left: px(BAR_ROUNDING),
                            top_right: px(BAR_ROUNDING),
                            bottom_right: px(0.0),
                            bottom_left: px(0.0),
                        }),
                    );
                }
                if column.peak > 0.0 {
                    let y = down(column.peak);
                    window.paint_quad(fill(
                        Bounds::from_corners(
                            point(px(from), px(y - PEAK_THICKNESS)),
                            point(px(to), px(y)),
                        ),
                        rgb(accent),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full()
}

fn traced(trace: Option<(Vec<f32>, Vec<f32>)>) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let left = f32::from(bounds.origin.x);
            let wide = f32::from(bounds.size.width);
            let middle = f32::from(bounds.center().y);
            let reach = f32::from(bounds.size.height) * HALF * SCOPE_FILLS;

            window.paint_quad(fill(
                Bounds::from_corners(
                    point(px(left), px(middle)),
                    point(px(left + wide), px(middle + 1.0)),
                ),
                rgb(theme::outline()),
            ));

            let Some((on_the_left, on_the_right)) = trace else {
                return;
            };
            let accent = theme::accent();
            let drawn = |levels: &[f32]| -> Vec<Point<Pixels>> {
                let steps = levels.len().saturating_sub(1).max(1) as f32;
                levels
                    .iter()
                    .enumerate()
                    .map(|(at, level)| {
                        point(
                            px(left + at as f32 / steps * wide),
                            px(middle - level.clamp(-1.0, 1.0) * reach),
                        )
                    })
                    .collect()
            };
            stroke(
                window,
                &drawn(&on_the_right),
                theme::tinted(accent, RIGHT_TRACE_ALPHA),
            );
            stroke(window, &drawn(&on_the_left), rgb(accent));
        },
    )
    .absolute()
    .size_full()
}

fn stroke(window: &mut Window, points: &[Point<Pixels>], colour: gpui::Rgba) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    if rest.is_empty() {
        return;
    }
    let mut line = PathBuilder::stroke(px(TRACE_LINE));
    line.move_to(*first);
    for at in rest {
        line.line_to(*at);
    }
    match line.build() {
        Ok(path) => window.paint_path(path, colour),
        Err(error) => tracing::debug!(%error, "the scope's trace would not tessellate"),
    }
}

impl RootView {
    pub(crate) fn visualiser_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.player.read(cx);
        let state = model.state().clone();
        let digest = model.digest();
        let tapped = model.tap();
        if state.current.is_none() {
            return kit::empty(Icon::Visualiser, NOTHING_PLAYING, None);
        }

        let playing = self.playing(&state, digest.as_deref(), cx);
        let heading = self.visualiser_heading(&playing, cx);
        let body = match tapped {
            Tapped::Markers => kit::empty(Icon::Visualiser, MARKED, Some(MARKED_MORE)),
            Tapped::Nothing | Tapped::Samples(_) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .px_6()
                .py_5()
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.0))
                        .rounded_lg()
                        .bg(rgb(theme::surface()))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .overflow_hidden()
                        .child(self.visualiser.clone()),
                )
                .into_any_element(),
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

    fn visualiser_heading(&self, playing: &Playing, cx: &mut Context<Self>) -> Div {
        let (chosen, readback) = {
            let visualiser = self.visualiser.read(cx);
            (visualiser.showing(), visualiser.readback())
        };

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(kit::eyebrow("VISUALISER"))
                        .child(kit::title(self.opens(
                            "visualiser-title",
                            playing.title.clone(),
                            OPEN_ALBUM_HINT,
                            playing.cover.album.map(Selection::Album),
                            cx,
                        )))
                        .child(kit::subtitle(self.opens(
                            "visualiser-artist",
                            playing.artist.clone(),
                            OPEN_ARTIST_HINT,
                            playing.artist_id.map(Selection::Artist),
                            cx,
                        ))),
                )
                .child(
                    kit::actions()
                        .child(kit::figure(readback))
                        .child(hint::explains("visualiser-reads", READS_HINT))
                        .child(
                            kit::segmented().children(Showing::ALL.into_iter().enumerate().map(
                                |(index, showing)| {
                                    kit::segment(
                                        ("visualiser-showing", index),
                                        showing.label(),
                                        showing == chosen,
                                    )
                                    .names(showing.about())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.visualiser.update(cx, |visualiser, cx| {
                                                visualiser.show(showing);
                                                cx.notify();
                                            });
                                        },
                                    ))
                                },
                            )),
                        ),
                ),
        )
    }
}
