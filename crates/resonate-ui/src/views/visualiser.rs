use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Corners, Div, Entity, Pixels, Point, Render, SharedString,
    Task, Window, canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px,
    rgb,
};
use resonate_core::{Frames, SampleRate};
use resonate_engine::{Caught, PlaybackState, Tap, Tapped};
use smallvec::SmallVec;

use crate::{
    PlayerModel,
    icons::Icon,
    spectrum::{
        self, CEILING_DB, Column, FLOOR_DB, MARKED_EVERY_DB, Spectrum, height_of, rising_edge,
    },
    stereo::{self, Meter, Metered, Stereo},
    theme,
    views::{
        hint::{self, Names},
        kit, plot,
        root::RootView,
        settings::marked_at,
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

const SPECTRUM_HINT: &str =
    "Sixth-octave bands from 20 Hz to what the rate carries, with each band's peak held";

const SCOPE_HINT: &str = "The waveform, left over right, held still on a rising edge";

const STEREO_HINT: &str = "Where the sound stands between the speakers: a goniometer, mid up and \
                           side across, beside each channel's level and how alike the two are";

const LEVEL_HINT: &str =
    "Each channel's loudness over the last moment, its highest sample held above it";

const CORRELATION_HINT: &str = "How alike the two channels are: +1 the same sound in each, 0 \
                                unrelated, below 0 out of phase";

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
const BARS_HELD_INLINE: usize = 80;

type Levels = SmallVec<[f32; TRACE_POINTS_AT_MOST]>;
type Traced = SmallVec<[Point<Pixels>; TRACE_POINTS_AT_MOST]>;
type Columns = SmallVec<[Column; BARS_HELD_INLINE]>;
const RIGHT_TRACE_ALPHA: u8 = 0x80;
const DOTS_AT_MOST: usize = 2_048;
const DOT: f32 = 1.5;
const DOT_ALPHA: u8 = 0x90;
const METER_WIDTH: f32 = 10.0;
const METER_GAP: f32 = 6.0;
const METERS_ROOM: f32 = 2.0 * METER_WIDTH + METER_GAP + 2.0 * PLOT_INSET;
const CORRELATION_ROOM: f32 = 28.0;
const CORRELATION_TRACK: f32 = 6.0;
const CORRELATION_MARK: f32 = 3.0;
const PLOT_INSET: f32 = 16.0;

type Dots = SmallVec<[(f32, f32); DOTS_AT_MOST]>;
const SCOPE_FILLS: f32 = 0.9;
const HALF: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Showing {
    #[default]
    Spectrum,
    Scope,
    Stereo,
}

impl Showing {
    const ALL: [Self; 3] = [Self::Spectrum, Self::Scope, Self::Stereo];

    const fn label(self) -> &'static str {
        match self {
            Self::Spectrum => "Spectrum",
            Self::Scope => "Scope",
            Self::Stereo => "Stereo",
        }
    }

    const fn about(self) -> &'static str {
        match self {
            Self::Spectrum => SPECTRUM_HINT,
            Self::Scope => SCOPE_HINT,
            Self::Stereo => STEREO_HINT,
        }
    }
}

pub(crate) struct Visualiser {
    player: Entity<PlayerModel>,
    showing: Showing,
    spectrum: Spectrum,
    stereo: Stereo,
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
            stereo: Stereo::default(),
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
        let rate = self.spectrum.rate().kilohertz();
        match self.showing {
            Showing::Spectrum => format!("{}-point · {rate} kHz", self.spectrum.points()),
            Showing::Scope => format!("{} ms · {rate} kHz", SCOPE_SPAN.as_millis()),
            Showing::Stereo => format!(
                "{} ms · {rate} kHz",
                Frames(self.spectrum.points() as u64)
                    .to_duration(self.spectrum.rate())
                    .as_millis()
            ),
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

    fn read_the_stereo(&mut self, tap: Option<&Tap>, now: Instant, step: Duration) -> bool {
        let points = self.spectrum.points();
        let caught = self.catch(tap, now, points);
        self.stereo.take(&self.left, &self.right, step);

        caught.tapped == 0 && !self.stereo.is_at_rest()
    }

    fn dots(&self) -> Dots {
        let step = self.left.len().div_ceil(DOTS_AT_MOST).max(1);
        stereo::sides_and_mids(&self.left, &self.right)
            .step_by(step)
            .collect()
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

    fn trace(&self) -> Option<(Levels, Levels)> {
        let from = self.traced?;
        let span = spanned(self.spectrum.rate());
        let step = span.div_ceil(TRACE_POINTS_AT_MOST).max(1);
        let thinned = |levels: &[f32]| -> Levels {
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
        let playing = self.player.read(cx).state().playback == PlaybackState::Playing;
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
            Showing::Stereo => self.read_the_stereo(tap, now, step),
        };
        if settling {
            self.settle(cx);
        }

        let plot = div()
            .relative()
            .size_full()
            .child(follows_the_display(playing && tap.is_some()));
        match self.showing {
            Showing::Spectrum => {
                let top = self.spectrum.top();
                plot.child(bars(self.spectrum.columns().collect(), top))
                    .children(marked_at(spectrum::marked(top), |hertz| {
                        spectrum::across(hertz, top)
                    }))
            }
            Showing::Scope => plot.child(traced(self.trace())),
            Showing::Stereo => {
                let metered = self.stereo.metered();
                plot.child(stereo_plot(self.dots(), metered))
                    .child(stereo_readings(metered))
            }
        }
    }
}

fn follows_the_display(moving: bool) -> Canvas<()> {
    canvas(
        move |_, window, _| {
            if moving {
                window.request_animation_frame();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_0()
}

fn bars(columns: Columns, heard_to: f64) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let left = f32::from(bounds.origin.x);
            let wide = f32::from(bounds.size.width);
            let top = f32::from(bounds.top()) + HEAD_ROOM;
            let bottom = f32::from(bounds.bottom()) - LABEL_ROOM;
            let tall = (bottom - top).max(0.0);
            let across = |hertz: f64| left + spectrum::across(hertz, heard_to) * wide;
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

fn traced(trace: Option<(Levels, Levels)>) -> Canvas<()> {
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
            let drawn = |levels: &[f32]| -> Traced {
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

fn stereo_readings(metered: Metered) -> Div {
    let level = |channel: &str, meter: Meter| format!("{channel} {:.1} dB", meter.peak_db);
    let correlation = metered.correlation.map_or_else(
        || "correlation —".to_owned(),
        |held| format!("correlation {held:+.2}"),
    );

    div()
        .absolute()
        .top_2()
        .left_3()
        .flex()
        .gap_3()
        .child(
            div()
                .id("stereo-levels")
                .names(LEVEL_HINT)
                .child(kit::figure(format!(
                    "{} · {}",
                    level("L", metered.left),
                    level("R", metered.right)
                ))),
        )
        .child(
            div()
                .id("stereo-correlation")
                .names(CORRELATION_HINT)
                .child(kit::figure(correlation)),
        )
}

fn stereo_plot(dots: Dots, metered: Metered) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let left = f32::from(bounds.origin.x) + PLOT_INSET;
            let top = f32::from(bounds.origin.y) + HEAD_ROOM + LABEL_ROOM;
            let wide = (f32::from(bounds.size.width) - METERS_ROOM - 2.0 * PLOT_INSET).max(0.0);
            let tall = (f32::from(bounds.size.height)
                - HEAD_ROOM
                - LABEL_ROOM
                - CORRELATION_ROOM
                - PLOT_INSET)
                .max(0.0);
            let side = wide.min(tall);
            let centre = point(left + wide * HALF, top + tall * HALF);
            let reach = side * HALF * SCOPE_FILLS;

            goniometer(window, centre, reach, &dots);
            let meters_left = left + wide + PLOT_INSET;
            meter(window, meters_left, top, tall, metered.left);
            meter(
                window,
                meters_left + METER_WIDTH + METER_GAP,
                top,
                tall,
                metered.right,
            );
            correlation_meter(
                window,
                left,
                top + tall + CORRELATION_ROOM * HALF,
                wide,
                metered.correlation,
            );
        },
    )
    .absolute()
    .size_full()
}

fn goniometer(window: &mut Window, centre: Point<f32>, reach: f32, dots: &[(f32, f32)]) {
    let guide = rgb(theme::border());
    window.paint_quad(fill(
        Bounds::from_corners(
            point(px(centre.x), px(centre.y - reach)),
            point(px(centre.x + 1.0), px(centre.y + reach)),
        ),
        guide,
    ));
    window.paint_quad(fill(
        Bounds::from_corners(
            point(px(centre.x - reach), px(centre.y)),
            point(px(centre.x + reach), px(centre.y + 1.0)),
        ),
        guide,
    ));
    let steps = (reach as usize).max(1);
    for at in 0..=steps {
        let along = (at as f32 / steps as f32).mul_add(2.0, -1.0) * reach * HALF;
        for (x, y) in [
            (centre.x + along, centre.y - along),
            (centre.x + along, centre.y + along),
        ] {
            window.paint_quad(fill(
                Bounds::from_corners(point(px(x), px(y)), point(px(x + 1.0), px(y + 1.0))),
                guide,
            ));
        }
    }

    let colour = theme::tinted(theme::accent(), DOT_ALPHA);
    for (side, mid) in dots {
        let x = centre.x + side.clamp(-1.0, 1.0) * reach;
        let y = centre.y - mid.clamp(-1.0, 1.0) * reach;
        window.paint_quad(fill(
            Bounds::from_corners(
                point(px(x - DOT * HALF), px(y - DOT * HALF)),
                point(px(x + DOT * HALF), px(y + DOT * HALF)),
            ),
            colour,
        ));
    }
}

fn meter(window: &mut Window, left: f32, top: f32, tall: f32, meter: Meter) {
    let bottom = top + tall;
    let right = left + METER_WIDTH;
    window.paint_quad(
        fill(
            Bounds::from_corners(point(px(left), px(top)), point(px(right), px(bottom))),
            rgb(theme::outline()),
        )
        .corner_radii(Corners::all(px(BAR_ROUNDING))),
    );
    let accent = theme::accent();
    if meter.level > 0.0 {
        window.paint_quad(
            fill(
                Bounds::from_corners(
                    point(px(left), px(bottom - meter.level * tall)),
                    point(px(right), px(bottom)),
                ),
                linear_gradient(
                    180.0,
                    linear_color_stop(rgb(accent), 0.0),
                    linear_color_stop(theme::tinted(accent, BAR_FOOT_ALPHA), 1.0),
                ),
            )
            .corner_radii(Corners::all(px(BAR_ROUNDING))),
        );
    }
    if meter.peak > 0.0 {
        let y = bottom - meter.peak * tall;
        window.paint_quad(fill(
            Bounds::from_corners(
                point(px(left), px(y - PEAK_THICKNESS)),
                point(px(right), px(y)),
            ),
            rgb(accent),
        ));
    }
}

fn correlation_meter(window: &mut Window, left: f32, middle: f32, wide: f32, held: Option<f32>) {
    let half_track = CORRELATION_TRACK * HALF;
    window.paint_quad(
        fill(
            Bounds::from_corners(
                point(px(left), px(middle - half_track)),
                point(px(left + wide), px(middle + half_track)),
            ),
            rgb(theme::outline()),
        )
        .corner_radii(Corners::all(px(half_track))),
    );
    let centre = left + wide * HALF;
    window.paint_quad(fill(
        Bounds::from_corners(
            point(px(centre), px(middle - CORRELATION_TRACK)),
            point(px(centre + 1.0), px(middle + CORRELATION_TRACK)),
        ),
        rgb(theme::border()),
    ));
    let Some(held) = held else {
        return;
    };
    let x = centre + held.clamp(-1.0, 1.0) * wide * HALF;
    let colour = if held < 0.0 {
        theme::failure()
    } else {
        theme::accent()
    };
    window.paint_quad(
        fill(
            Bounds::from_corners(
                point(px(x - CORRELATION_MARK), px(middle - CORRELATION_TRACK)),
                point(px(x + CORRELATION_MARK), px(middle + CORRELATION_TRACK)),
            ),
            rgb(colour),
        )
        .corner_radii(Corners::all(px(CORRELATION_MARK))),
    );
}

fn stroke(window: &mut Window, points: &[Point<Pixels>], colour: gpui::Rgba) {
    plot::stroke(window, points, TRACE_LINE, colour);
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
                .child(self.playing_heading(
                    "VISUALISER",
                    "visualiser-title",
                    "visualiser-artist",
                    playing,
                    cx,
                ))
                .child(
                    kit::actions()
                        .child(kit::figure(readback))
                        .child(hint::explains("visualiser-reads", READS_HINT))
                        .child(
                            kit::segmented().children(Showing::ALL.into_iter().enumerate().map(
                                |(index, showing)| {
                                    self.in_the_pane_ring(
                                        kit::segment(
                                            ("visualiser-showing", index),
                                            showing.label(),
                                            showing == chosen,
                                        )
                                        .names(showing.about()),
                                        move |this, _, cx| {
                                            this.visualiser.update(cx, |visualiser, cx| {
                                                visualiser.show(showing);
                                                cx.notify();
                                            });
                                        },
                                        cx,
                                    )
                                },
                            )),
                        ),
                ),
        )
    }
}
