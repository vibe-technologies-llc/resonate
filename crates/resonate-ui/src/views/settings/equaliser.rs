use std::{cell::Cell as Slot, rc::Rc};

use gpui::{
    BorderStyle, Bounds, Canvas, Context, Div, Hsla, MouseButton, MouseDownEvent, PathBuilder,
    Pixels, Point, ScrollWheelEvent, SharedString, Stateful, Window, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, quad, relative, rgb, size,
};
use resonate_core::{
    SampleRate,
    eq::{Band, BandKind, Preamp},
};
use resonate_engine::{Command, NodeName};
use resonate_eq::{Binding, Device, ProfileName};

use crate::{
    Setting,
    equaliser::{CURVE_COLUMNS, Cell, DRAWN_BETWEEN_MILLIBELS, Editing},
    icons::Icon,
    theme,
    views::{
        hint::Names as _,
        kit::{self, Tone},
        listing,
        root::RootView,
        settings::{
            action,
            curve::{CURVE_LINE, HeldBand, PIXELS_PER_NOTCH, Plot, Plotted, across_at},
            hugging, note, rows, switch_row,
        },
    },
};

const CURVE_HEIGHT: f32 = 168.0;
const CURVE_MARKED_AT_HZ: [f64; 3] = [100.0, 1_000.0, 10_000.0];
const LEVEL_MARKED_EVERY_DB: f32 = 5.0;
const DRAWN_AT: SampleRate = SampleRate::HZ_48000;
const HANDLE_RADIUS: f32 = 5.0;
const CHOSEN_HANDLE_RADIUS: f32 = 6.5;
const HANDLE_BORDER: f32 = 2.0;

const NOT_BIT_PERFECT: &str = "On, the samples reaching the device are not the file's own: the \
                               playback bar reads converted rather than bit-perfect, and a DSD \
                               stream packed as DoP is left untouched.";

const NOTHING_BOUND: &str = "Nothing is bound to this device, so nothing is applied. Press the \
                             curve to give it a curve of its own, bind a kept profile above, \
                             import an EqualizerAPO file, or fetch the correction measured for \
                             what is plugged in.";

const SHAPED_BY_HAND: &str = "Press the curve to add a band, drag a handle to move it, scroll \
                              over one to change its Q, and right-press it to take it out.";

const FOLLOWS_THE_DEFAULT: &str = "follows the default";
const BOUND_TO_NOTHING: &str = "nothing";
const ITS_OWN_CURVE: &str = "own curve";

impl RootView {
    pub(super) fn equalising_group(&mut self, cx: &mut Context<Self>) -> Div {
        let on = self.equaliser.read(cx).bindings().enabled;
        let bound = self.equaliser.read(cx).bindings().binds_anything();

        kit::section_body()
            .child(self.in_the_ring(
                "equaliser",
                switch_row(
                    "Apply the equaliser",
                    if bound {
                        "Off, the samples reach the device as the file wrote them"
                    } else {
                        "Nothing is bound to any device yet: press the curve below to shape one"
                    },
                    on,
                    "equaliser",
                ),
                move |this, _, cx| this.switch_the_equaliser(!on, cx),
                cx,
            ))
            .when(on, |body| body.child(note(NOT_BIT_PERFECT)))
    }

    pub(super) fn bound_to_group(&mut self, cx: &mut Context<Self>) -> Div {
        let devices: Vec<(NodeName, String)> = self
            .player
            .read(cx)
            .sinks()
            .iter()
            .map(|sink| (sink.name.clone(), sink.description.clone()))
            .collect();
        let kept = self.equaliser.read(cx).kept().to_vec();

        let mut listed =
            rows().child(self.binding_row(None, "Every other device".to_owned(), &kept, cx));
        for (name, description) in devices {
            listed = listed.child(self.binding_row(Some(name), description, &kept, cx));
        }

        kit::section_body().child(listed)
    }

    fn binding_row(
        &self,
        sink: Option<NodeName>,
        description: String,
        kept: &[ProfileName],
        cx: &mut Context<Self>,
    ) -> Div {
        let bindings = self.equaliser.read(cx).bindings().clone();
        let named = bindings.named(sink.as_ref()).cloned();
        let under = sink.as_ref().map_or_else(
            || "the binding a device that names none of its own falls back to".to_owned(),
            |sink| sink.as_str().to_owned(),
        );

        let mut choices = kit::segmented()
            .child(self.bound_option(
                sink.clone(),
                None,
                if sink.is_some() && bindings.fallback.is_some() {
                    FOLLOWS_THE_DEFAULT.to_owned()
                } else {
                    BOUND_TO_NOTHING.to_owned()
                },
                named.is_none(),
                cx,
            ))
            .child(self.bound_option(
                sink.clone(),
                Some(Binding::Own),
                ITS_OWN_CURVE.to_owned(),
                named == Some(Binding::Own),
                cx,
            ));
        for name in kept {
            let binding = Binding::Profile(name.clone());
            let chosen = named.as_ref() == Some(&binding);
            choices = choices.child(self.bound_option(
                sink.clone(),
                Some(binding),
                name.as_str().to_owned(),
                chosen,
                cx,
            ));
        }

        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .py_2()
            .child(super::named(description, under))
            .child(div().flex().child(choices))
    }

    fn bound_option(
        &self,
        sink: Option<NodeName>,
        binding: Option<Binding>,
        label: String,
        chosen: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let bound = sink.as_ref().map_or_else(
            || "every-other-device".to_owned(),
            |sink| format!("sink-{}", sink.as_str()),
        );
        let to = match binding.as_ref() {
            None => "none".to_owned(),
            Some(Binding::Own) => "own".to_owned(),
            Some(Binding::Profile(name)) => format!("kept-{}", ProfileName::as_str(name)),
        };
        let id = format!("bind-{bound}-{to}");
        let segment = kit::segment(gpui::SharedString::from(id.clone()), label, chosen);

        segment
            .track_focus(&self.controls.at(&id, cx))
            .tab_stop(true)
            .key_context(crate::app::CONTROL_CONTEXT)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.bind_a_curve(sink.clone(), binding.clone(), cx);
            }))
    }

    pub(super) fn bands_group(&mut self, cx: &mut Context<Self>) -> Div {
        self.follow_the_binding(cx);
        let rate = self
            .player
            .read(cx)
            .state()
            .output
            .map_or(DRAWN_AT, |output| output.negotiated.rate);

        let model = self.equaliser.read(cx);
        let showing = model.shown_curve().cloned();
        let bands = model.shown_bands();
        let preamp = model
            .shown()
            .map_or(Preamp::NONE, resonate_core::eq::Profile::preamp);
        let peak = model.peak_db();
        let notice = model.notice().cloned();
        let chosen = model.chosen();

        let curve = self.curve(
            rate,
            Handles {
                bands: bands.clone(),
                chosen,
                lift: preamp,
            },
            cx,
        );
        let Some(shown) = showing else {
            return kit::section_body()
                .child(curve)
                .child(note(NOTHING_BOUND))
                .child(self.band_actions(false, cx))
                .when_some(notice, |body, notice| body.child(listing::noticed(&notice)));
        };

        let mut listed = rows();
        for (row, band) in bands.iter().enumerate() {
            listed = listed.child(self.band_row(row, *band, chosen == Some(row), cx));
        }

        kit::section_body()
            .child(curve)
            .child(note(SHAPED_BY_HAND))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(kit::field("Curve", kit::figure(shown.spoken())))
                    .child(kit::field("Preamp", self.preamp_cell(preamp, cx)))
                    .child(kit::field("Peak", kit::figure(format!("{peak:+.2} dB")))),
            )
            .when(!bands.is_empty(), |body| body.child(listed))
            .child(self.band_actions(true, cx))
            .when_some(notice, |body, notice| body.child(listing::noticed(&notice)))
    }

    fn curve(
        &mut self,
        rate: SampleRate,
        handles: Handles,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let drawn = self.equaliser.update(cx, |model, _| model.drawn(rate));
        let widest = self
            .held_band
            .map_or_else(|| widest_drawn(&drawn), |held| held.widest);

        div()
            .id("equaliser-curve")
            .relative()
            .h(px(CURVE_HEIGHT))
            .w_full()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .overflow_hidden()
            .cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.press_the_curve(event.position, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.take_a_band_out_at(event.position, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                if this.turn_a_q_at(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(traced(
                drawn,
                widest,
                handles,
                Rc::clone(&self.curve_plotted),
            ))
            .child(marked_levels(widest))
            .children(marked_frequencies())
    }

    fn preamp_cell(&self, preamp: Preamp, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(kit::figure(format!("{preamp}")))
            .child(self.in_the_ring(
                "fit-the-preamp",
                kit::button(
                    "fit-the-preamp",
                    None,
                    "Fit",
                    "Set the preamp from the curve's own peak, so the loudest part of it reaches \
                     full scale and no further",
                    Tone::Ghost,
                ),
                |this, _, cx| this.fit_the_preamp(cx),
                cx,
            ))
    }

    fn band_row(&self, row: usize, band: Band, chosen: bool, cx: &mut Context<Self>) -> Div {
        let editing = self.equaliser.read(cx).editing();
        let shaping = self.equaliser.read(cx).shaping() == Some(row);

        let line = div()
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .px_1()
            .rounded_md()
            .when(chosen, |line| line.bg(theme::tinted(theme::accent(), 0x14)))
            .child(self.band_number(row, chosen, cx))
            .child(self.kind_cell(row, band.kind, cx))
            .child(self.numeric_cell(row, Cell::Frequency, editing, cx))
            .child(self.numeric_cell(row, Cell::Gain, editing, cx))
            .child(self.numeric_cell(row, Cell::Q, editing, cx))
            .child(self.in_the_ring_at(
                format!("band-on-{row}"),
                kit::switch(gpui::SharedString::from(format!("band-on-{row}")), band.on),
                move |this, _, cx| this.switch_a_band(row, cx),
                cx,
            ))
            .child(self.in_the_ring_at(
                format!("drop-band-{row}"),
                kit::icon_button(
                    gpui::SharedString::from(format!("drop-band-{row}")),
                    Icon::Discard,
                    "Take this band out",
                ),
                move |this, _, cx| this.drop_a_band(row, cx),
                cx,
            ));

        div()
            .flex()
            .flex_col()
            .child(line)
            .when(shaping, |held| held.child(self.kinds(row, band.kind, cx)))
    }

    fn band_number(&self, row: usize, chosen: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = format!("band-choose-{row}");
        self.in_the_ring_at(
            id.clone(),
            div()
                .id(gpui::SharedString::from(id))
                .flex()
                .justify_center()
                .w(px(theme::row_plays() / 2.0))
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(gpui::transparent_black())
                .child(
                    kit::figure(format!("{}", row + 1))
                        .when(chosen, |figure| figure.text_color(rgb(theme::accent()))),
                )
                .names("Choose this band on the curve"),
            move |this, _, cx| this.choose_a_band(row, cx),
            cx,
        )
    }

    fn kind_cell(&self, row: usize, kind: BandKind, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_ring_at(
            format!("band-kind-{row}"),
            kit::chip(
                gpui::SharedString::from(format!("band-kind-{row}")),
                kind.label(),
                false,
            )
            .names("What shape this band gives the curve"),
            move |this, _, cx| this.shape_a_band(row, cx),
            cx,
        )
    }

    fn kinds(&self, row: usize, chosen: BandKind, cx: &mut Context<Self>) -> Div {
        let mut offered = kit::segmented();
        for kind in BandKind::ALL {
            let id = format!("band-kind-{row}-{}", kind.as_str());
            offered = offered.child(
                kit::segment(
                    gpui::SharedString::from(id.clone()),
                    kind.label(),
                    kind == chosen,
                )
                .track_focus(&self.controls.at(&id, cx))
                .tab_stop(true)
                .key_context(crate::app::CONTROL_CONTEXT)
                .on_click(cx.listener(move |this, _, _, cx| this.retype_a_band(row, kind, cx))),
            );
        }
        div().flex().pb_2().child(offered)
    }

    fn numeric_cell(
        &self,
        row: usize,
        cell: Cell,
        editing: Option<Editing>,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = editing == Some(Editing { row, cell });
        let held = self.equaliser.read(cx);
        let shown = held.band(row).map_or_else(String::new, |band| match cell {
            Cell::Frequency => format!("{}", band.frequency),
            Cell::Gain => format!("{}", band.gain),
            Cell::Q => format!("{}", band.q),
        });

        let width = match cell {
            Cell::Frequency => theme::field_label(),
            Cell::Gain => theme::field_label() * 0.8,
            Cell::Q => theme::field_label() * 0.6,
        };

        let frame = div()
            .flex()
            .items_center()
            .h(px(theme::row_height() * 0.8))
            .w(theme::width(width))
            .px_2()
            .rounded_md()
            .text_size(px(theme::text_xs()))
            .when_else(
                open,
                |cell| {
                    cell.bg(rgb(theme::background()))
                        .border_1()
                        .border_color(rgb(theme::accent()))
                },
                |cell| cell.border_1().border_color(rgb(theme::border())),
            );

        if open {
            return div().child(
                frame
                    .id("band-cell")
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        this.leave_figure(window, cx);
                    }))
                    .child(self.figure.clone()),
            );
        }

        let id = format!(
            "band-{}-{row}",
            match cell {
                Cell::Frequency => "frequency",
                Cell::Gain => "gain",
                Cell::Q => "q",
            }
        );
        div().child(
            self.in_the_ring_at(
                id.clone(),
                frame
                    .id(gpui::SharedString::from(id))
                    .cursor_text()
                    .hover(|cell| cell.bg(rgb(theme::hover())))
                    .child(kit::figure(shown)),
                move |this, window, cx| this.edit_a_cell(row, cell, window, cx),
                cx,
            ),
        )
    }

    fn band_actions(&self, showing: bool, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(action(
                "add-a-band",
                "Add a band",
                Icon::Plus,
                false,
                |this, _, cx| this.add_a_band(cx),
                self,
                cx,
            ))
            .child(action(
                "import-a-profile",
                "Import…",
                Icon::Import,
                false,
                |this, window, cx| this.import_a_profile(window, cx),
                self,
                cx,
            ))
            .when(showing, |row| {
                row.child(action(
                    "export-a-profile",
                    "Export…",
                    Icon::Export,
                    false,
                    |this, window, cx| this.export_a_profile(window, cx),
                    self,
                    cx,
                ))
            })
    }

    pub(super) fn measured_group(&mut self, cx: &mut Context<Self>) -> Div {
        let model = self.equaliser.read(cx);
        let reachable = model.has_a_source();
        let looking = model.is_looking();
        let found: Vec<Device> = model.found().into_iter().cloned().collect();
        let sink = self
            .player
            .read(cx)
            .sinks()
            .iter()
            .find(|sink| sink.is_default)
            .map(|sink| sink.description.clone());
        let suggested = sink
            .as_deref()
            .and_then(|description| model.suggested(description))
            .cloned();
        let knows = model.knows_the_catalogue();

        let mut listed = rows();
        for device in &found {
            listed = listed.child(self.measured_row(device, cx));
        }

        kit::section_body()
            .when(!reachable, |body| {
                body.child(note(
                    "Nothing is reached: turn Online on, or this build carries no network.",
                ))
            })
            .when(reachable, |body| {
                body.child(self.looking_field(cx))
                    .when(looking, |held| held.child(note("Asking AutoEq…")))
                    .when(!found.is_empty(), |held| held.child(listed))
            })
            .when_some(sink, |body, description| {
                body.child(self.plugged_in(&description, suggested.as_ref(), knows, reachable, cx))
            })
    }

    fn looking_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("look-for-a-device")
            .flex()
            .items_center()
            .gap_2()
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.border_color(theme::tinted(theme::accent(), 0x99)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.looking
                    .update(cx, |looking, _| looking.take_focus(window));
                cx.notify();
            }))
            .child(self.looking.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
    }

    fn measured_row(&self, device: &Device, cx: &mut Context<Self>) -> Div {
        let id = format!("fetch-{}", device.id);
        let label = device.label.clone();
        let taken = device.id.clone();

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_2()
                    .child(div().truncate().child(device.label.clone()))
                    .child(kit::badge(device.measured_by.clone(), theme::muted()))
                    .when_some(device.rig.clone(), |row, rig| row.child(kit::figure(rig))),
            )
            .child(self.in_the_ring_at(
                id.clone(),
                kit::button(
                    gpui::SharedString::from(id),
                    Some(Icon::Import),
                    "Use this",
                    "Fetch this measurement and keep it here as a profile",
                    Tone::Outlined,
                ),
                move |this, _, cx| this.fetch_a_correction(taken.clone(), label.clone(), cx),
                cx,
            ))
    }

    fn plugged_in(
        &self,
        description: &str,
        suggested: Option<&Device>,
        knows: bool,
        reachable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .child(kit::field(
                "What is plugged in",
                div()
                    .text_size(px(theme::text_sm()))
                    .child(gpui::SharedString::from(description.to_owned())),
            ))
            .when(reachable && !knows, |body| {
                body.child(hugging(self.in_the_ring(
                    "read-the-catalogue",
                    kit::button(
                        "read-the-catalogue",
                        Some(Icon::Globe),
                        "Look it up",
                        "Read AutoEq's list of measured devices and weigh this device's own name \
                         against it",
                        Tone::Outlined,
                    ),
                    |this, _, cx| this.read_the_catalogue(cx),
                    cx,
                )))
            })
            .when(knows && suggested.is_none(), |body| {
                body.child(note(
                    "Nothing measured answers to that name clearly enough to be worth guessing \
                     at. Search above if you know what is plugged in.",
                ))
            })
            .when_some(suggested.cloned(), |body, device| {
                body.child(self.measured_row(&device, cx))
            })
    }
}

fn widest_drawn(curve: &[f64]) -> f32 {
    let floor = DRAWN_BETWEEN_MILLIBELS as f32 / 1_000.0;
    let peak = curve.iter().fold(0.0_f64, |highest, decibels| {
        if decibels.is_finite() {
            highest.max(decibels.abs())
        } else {
            highest
        }
    });

    #[expect(
        clippy::cast_possible_truncation,
        reason = "a decibel reading fits an f32"
    )]
    let stepped = (peak as f32 / LEVEL_MARKED_EVERY_DB).ceil() * LEVEL_MARKED_EVERY_DB;

    stepped.max(floor)
}

fn spelt_hertz(hertz: f64) -> String {
    if hertz >= 1_000.0 {
        format!("{:.0}k", hertz / 1_000.0)
    } else {
        format!("{hertz:.0}")
    }
}

fn axis_label(text: String) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::faint()))
        .child(SharedString::from(text))
}

fn marked_levels(widest: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .justify_between()
        .items_end()
        .px_2()
        .py_1()
        .child(axis_label(format!("+{widest:.0}")))
        .child(axis_label(format!("-{widest:.0}")))
}

pub(crate) fn marked_frequencies() -> Vec<Div> {
    CURVE_MARKED_AT_HZ
        .into_iter()
        .map(|hertz| {
            axis_label(spelt_hertz(hertz))
                .absolute()
                .bottom_1()
                .left(relative(across_at(hertz)))
        })
        .collect()
}

struct Handles {
    bands: Vec<Band>,
    chosen: Option<usize>,
    lift: Preamp,
}

fn traced(
    curve: std::sync::Arc<[f64]>,
    widest: f32,
    handles: Handles,
    plotted: Rc<Slot<Plotted>>,
) -> Canvas<()> {
    let lift = handles.lift;
    canvas(
        move |bounds, _, _| {
            plotted.set(Plotted {
                bounds,
                widest,
                lift,
            });
        },
        move |bounds, (), window, _| {
            let plot = Plot::within(bounds, widest, lift);
            let wide = f32::from(bounds.size.width);
            let left = f32::from(bounds.origin.x);
            let steps = CURVE_COLUMNS.saturating_sub(1).max(1) as f32;

            let level = |decibels: f64| px(plot.y_of(decibels));
            let across = |at: usize| px(left + at as f32 / steps * wide);

            let mut zero = PathBuilder::stroke(px(1.0));
            zero.move_to(point(bounds.left(), level(0.0)));
            zero.line_to(point(bounds.right(), level(0.0)));
            match zero.build() {
                Ok(drawn) => window.paint_path(drawn, rgb(theme::outline())),
                Err(error) => tracing::debug!(%error, "the curve's zero line would not tessellate"),
            }

            let plotted: Vec<gpui::Point<Pixels>> = curve
                .iter()
                .enumerate()
                .map(|(at, decibels)| point(across(at), level(*decibels)))
                .collect();
            if let (Some(first), Some(last)) = (plotted.first(), plotted.last())
                && plotted.len() >= 2
            {
                let mut under = PathBuilder::fill();
                under.move_to(point(first.x, level(0.0)));
                for at in &plotted {
                    under.line_to(*at);
                }
                under.line_to(point(last.x, level(0.0)));
                under.close();
                match under.build() {
                    Ok(shaded) => window.paint_path(
                        shaded,
                        linear_gradient(
                            180.0,
                            linear_color_stop(theme::tinted(theme::accent(), 0x40), 0.0),
                            linear_color_stop(theme::tinted(theme::accent(), 0x10), 1.0),
                        ),
                    ),
                    Err(error) => tracing::debug!(%error, "the curve's wash would not tessellate"),
                }

                let mut line = PathBuilder::stroke(px(CURVE_LINE));
                line.move_to(*first);
                for at in plotted.iter().skip(1) {
                    line.line_to(*at);
                }
                match line.build() {
                    Ok(drawn) => window.paint_path(drawn, rgb(theme::accent())),
                    Err(error) => tracing::debug!(%error, "the curve would not tessellate"),
                }
            }

            for (row, band) in handles.bands.iter().enumerate() {
                window.paint_quad(handle(plot, *band, handles.chosen == Some(row)));
            }
        },
    )
    .size_full()
}

fn handle(plot: Plot, band: Band, chosen: bool) -> gpui::PaintQuad {
    let (x, y) = plot.handle(band);
    let radius = if chosen {
        CHOSEN_HANDLE_RADIUS
    } else {
        HANDLE_RADIUS
    };
    let edge = if band.on {
        theme::accent()
    } else {
        theme::faint()
    };
    let fill = if chosen {
        theme::accent()
    } else {
        theme::surface()
    };

    quad(
        Bounds::new(
            point(px(x - radius), px(y - radius)),
            size(px(radius * 2.0), px(radius * 2.0)),
        ),
        px(radius),
        Hsla::from(rgb(fill)),
        px(HANDLE_BORDER),
        Hsla::from(rgb(edge)),
        BorderStyle::Solid,
    )
}

impl RootView {
    pub(crate) fn switch_the_equaliser(&mut self, on: bool, cx: &mut Context<Self>) {
        self.equaliser.update(cx, |model, _| model.switch(on));
        self.store(&Setting::Equaliser(on), cx);
        self.tell_the_engine(cx);
    }

    pub(crate) fn bind_a_curve(
        &mut self,
        sink: Option<NodeName>,
        binding: Option<Binding>,
        cx: &mut Context<Self>,
    ) {
        self.held_band = None;
        self.equaliser
            .update(cx, |model, _| model.bind(sink.as_ref(), binding.clone()));
        let written = match sink {
            Some(sink) => Setting::EqualiserFor { sink, binding },
            None => Setting::EqualiserProfile(binding),
        };
        self.store(&written, cx);
        self.tell_the_engine(cx);
        cx.notify();
    }

    fn the_default_sink(&self, cx: &mut Context<Self>) -> Option<NodeName> {
        self.player
            .read(cx)
            .sinks()
            .iter()
            .find(|sink| sink.is_default)
            .map(|sink| sink.name.clone())
    }

    fn shape_its_own_curve(&mut self, cx: &mut Context<Self>) {
        if self.equaliser.read(cx).shown().is_some() {
            return;
        }
        let sink = self.the_default_sink(cx);
        self.bind_a_curve(sink, Some(Binding::Own), cx);
        if !self.equaliser.read(cx).bindings().enabled {
            self.switch_the_equaliser(true, cx);
        }
    }

    fn at(position: Point<Pixels>) -> (f32, f32) {
        (f32::from(position.x), f32::from(position.y))
    }

    fn press_the_curve(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let plotted = self.curve_plotted.get();
        let plot = Plot::of(plotted);
        let (x, y) = Self::at(position);

        let bands = self.equaliser.read(cx).shown_bands();
        if let Some(row) = plot.nearest(&bands, x, y) {
            self.equaliser
                .update(cx, |model, _| model.choose(Some(row)));
            self.held_band = Some(HeldBand {
                row,
                widest: plotted.widest,
            });
            cx.notify();
            return;
        }

        self.shape_its_own_curve(cx);
        let placed = plot.placed(x, y);
        let added = self
            .equaliser
            .update(cx, |model, cx| model.add_at(placed, cx));
        if let Some(row) = added {
            self.held_band = Some(HeldBand {
                row,
                widest: plotted.widest,
            });
            self.tell_the_engine(cx);
        }
        cx.notify();
    }

    pub(crate) fn drag_a_band(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(held) = self.held_band else {
            return;
        };
        let plotted = self.curve_plotted.get();
        let plot = Plot::within(plotted.bounds, held.widest, plotted.lift);
        let (x, y) = Self::at(position);
        let placed = plot.placed(x, y);

        let moved = self
            .equaliser
            .update(cx, |model, cx| model.move_to(held.row, placed, cx));
        if moved {
            self.tell_the_engine(cx);
            cx.notify();
        }
    }

    pub(crate) fn let_the_band_go(&mut self, cx: &mut Context<Self>) {
        if self.held_band.take().is_some() {
            cx.notify();
        }
    }

    fn take_a_band_out_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let plot = Plot::of(self.curve_plotted.get());
        let (x, y) = Self::at(position);
        let bands = self.equaliser.read(cx).shown_bands();
        if let Some(row) = plot.nearest(&bands, x, y) {
            self.drop_a_band(row, cx);
        }
    }

    fn turn_a_q_at(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) -> bool {
        let plot = Plot::of(self.curve_plotted.get());
        let (x, y) = Self::at(event.position);
        let bands = self.equaliser.read(cx).shown_bands();
        let Some(row) = plot.nearest(&bands, x, y) else {
            return false;
        };

        let turned = f32::from(event.delta.pixel_delta(px(PIXELS_PER_NOTCH)).y);
        let notches = f64::from(turned / PIXELS_PER_NOTCH);
        let changed = self
            .equaliser
            .update(cx, |model, cx| model.turn_the_q(row, notches, cx));
        if changed {
            self.tell_the_engine(cx);
            cx.notify();
        }
        true
    }

    fn choose_a_band(&mut self, row: usize, cx: &mut Context<Self>) {
        self.equaliser.update(cx, |model, _| {
            let chosen = (model.chosen() != Some(row)).then_some(row);
            model.choose(chosen);
        });
        cx.notify();
    }

    pub(crate) fn tell_the_engine(&mut self, cx: &mut Context<Self>) {
        let equalisation = self.equaliser.read(cx).equalisation();
        self.send(
            Command::SetEqualisation(std::sync::Arc::new(equalisation)),
            cx,
        );
    }

    pub(crate) fn follow_the_binding(&mut self, cx: &mut Context<Self>) {
        let sink = self.the_default_sink(cx);
        self.equaliser
            .update(cx, |model, _| model.show(sink.as_ref()));
    }

    fn edit_a_cell(&mut self, row: usize, cell: Cell, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        self.adding = None;
        let written = self.equaliser.update(cx, |model, _| {
            model.edit_cell(row, cell);
            model.written(Editing { row, cell })
        });
        self.figure.update(cx, |figure, cx| {
            figure.hold(written, cx);
            figure.take_focus(window);
        });
        cx.notify();
    }

    pub(crate) fn figure_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.equaliser.read(cx).editing() else {
            return;
        };
        let typed = self.figure.read(cx).text().to_owned();
        let taken = self
            .equaliser
            .update(cx, |model, cx| model.take(editing, &typed, cx));

        if taken {
            self.figure
                .update(cx, |figure, cx| figure.hold(String::new(), cx));
            self.controls.lets_go(window);
            self.tell_the_engine(cx);
        }
        cx.notify();
    }

    pub(crate) fn leave_figure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.figure.read(cx).is_focused(window) {
            return;
        }
        self.equaliser.update(cx, |model, _| model.leave_cell());
        self.figure
            .update(cx, |figure, cx| figure.hold(String::new(), cx));
        self.controls.lets_go(window);
        cx.notify();
    }

    fn shape_a_band(&mut self, row: usize, cx: &mut Context<Self>) {
        let open = self.equaliser.read(cx).shaping() == Some(row);
        self.equaliser
            .update(cx, |model, _| model.shape((!open).then_some(row)));
        cx.notify();
    }

    fn retype_a_band(&mut self, row: usize, kind: BandKind, cx: &mut Context<Self>) {
        self.equaliser
            .update(cx, |model, cx| model.retype(row, kind, cx));
        self.tell_the_engine(cx);
        cx.notify();
    }

    fn switch_a_band(&mut self, row: usize, cx: &mut Context<Self>) {
        self.equaliser
            .update(cx, |model, cx| model.switch_band(row, cx));
        self.tell_the_engine(cx);
        cx.notify();
    }

    fn add_a_band(&mut self, cx: &mut Context<Self>) {
        self.shape_its_own_curve(cx);
        self.equaliser.update(cx, |model, cx| model.add_a_band(cx));
        self.tell_the_engine(cx);
        cx.notify();
    }

    fn drop_a_band(&mut self, row: usize, cx: &mut Context<Self>) {
        self.held_band = None;
        self.equaliser
            .update(cx, |model, cx| model.drop_a_band(row, cx));
        self.tell_the_engine(cx);
        cx.notify();
    }

    fn fit_the_preamp(&mut self, cx: &mut Context<Self>) {
        self.equaliser
            .update(cx, |model, cx| model.fit_the_preamp(cx));
        self.tell_the_engine(cx);
        cx.notify();
    }

    fn import_a_profile(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });

        cx.spawn(async move |this, cx| {
            let answered = chosen.await;
            let outcome = this.update(cx, |this, cx| match answered {
                Ok(Ok(Some(paths))) => {
                    if let Some(from) = paths.into_iter().next() {
                        this.equaliser
                            .update(cx, |model, cx| model.import(from, cx));
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => this.equaliser.update(cx, |model, _| {
                    model.told(crate::models::Notice::Trouble(error.to_string()));
                }),
                Err(error) => this.equaliser.update(cx, |model, _| {
                    model.told(crate::models::Notice::Trouble(error.to_string()));
                }),
            });
            let _ = outcome;
        })
        .detach();
    }

    fn export_a_profile(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let named = self
            .equaliser
            .read(cx)
            .shown_curve()
            .map(crate::equaliser::Curve::file_name);
        let folder = self.equaliser.read(cx).folder().to_path_buf();
        let chosen = cx.prompt_for_new_path(&folder, named.as_deref());

        cx.spawn(async move |this, cx| {
            let answered = chosen.await;
            let outcome = this.update(cx, |this, cx| match answered {
                Ok(Ok(Some(to))) => {
                    this.equaliser.update(cx, |model, cx| model.export(to, cx));
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => this.equaliser.update(cx, |model, _| {
                    model.told(crate::models::Notice::Trouble(error.to_string()));
                }),
                Err(error) => this.equaliser.update(cx, |model, _| {
                    model.told(crate::models::Notice::Trouble(error.to_string()));
                }),
            });
            let _ = outcome;
        })
        .detach();
    }

    pub(crate) fn look_for_a_device(&mut self, cx: &mut Context<Self>) {
        let typed = self.looking.read(cx).text().to_owned();
        self.equaliser
            .update(cx, |model, cx| model.look(&typed, cx));
        cx.notify();
    }

    fn read_the_catalogue(&mut self, cx: &mut Context<Self>) {
        self.equaliser
            .update(cx, |model, cx| model.read_the_catalogue(None, cx));
        cx.notify();
    }

    fn fetch_a_correction(
        &mut self,
        device: resonate_eq::DeviceId,
        label: String,
        cx: &mut Context<Self>,
    ) {
        self.equaliser
            .update(cx, |model, cx| model.fetch(device, label, cx));
        cx.notify();
    }

    pub(crate) fn unbind_every_device(&mut self, cx: &mut Context<Self>) {
        let bound: Vec<Option<NodeName>> = {
            let bindings = self.equaliser.read(cx).bindings();
            bindings
                .by_sink
                .iter()
                .map(|(sink, _)| Some(sink.clone()))
                .chain(bindings.fallback.is_some().then_some(None))
                .collect()
        };
        for sink in bound {
            self.bind_a_curve(sink, None, cx);
        }
    }
}
