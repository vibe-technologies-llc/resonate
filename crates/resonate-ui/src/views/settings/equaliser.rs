use std::{
    cell::Cell as Slot,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    App, BorderStyle, Bounds, Canvas, Context, Div, Entity, Hsla, MouseButton, MouseDownEvent,
    PathPromptOptions, Pixels, Point, ScrollEvent, SharedString, Stateful, Task, Window, canvas,
    div, linear_color_stop, linear_gradient, point, prelude::*, px, quad, relative, rgb, size,
};
use resonate_core::{
    HeadYaw, MediaLocation, SampleRate, SpeakerAngle, SpeakerElevation, Speakers,
    eq::{Band, BandKind, Preamp, Traced, TracedOn},
};
use resonate_engine::{
    AnalysisError, Applied, BandGains, Command, DspError, Impulse, Loudest, NodeName,
    PairedReadError, Player, Sources, Speaker, Watch, Watching, binaural_wave, read_binaural_pair,
    read_impulse,
};
use resonate_eq::{Binding, BuiltIn, Device, MEASURERS_BEST_FIRST, ProfileName};
use resonate_library::Library;
use resonate_listen::Listening;

use crate::{
    Bindings, Measurers, Notice, PlayerModel, ResonateApp, Setting,
    app::{
        BAND_CONTEXT, BandHigher, BandLouder, BandLower, BandNarrower, BandQuieter, BandWider,
        CONTROL_CONTEXT,
    },
    equaliser::{
        CURVE_COLUMNS, Cell, DRAWN_BETWEEN_MILLIBELS, Editing, Goal, HERTZ_A_KILOHERTZ, Nudge,
        Sounding,
    },
    icons::Icon,
    theme, toast,
    views::{
        hint::Names as _,
        kit::{self, EndsInAnEllipsis as _, Tone},
        menu::Menu,
        plot,
        root::RootView,
        settings::{
            Armed, Rows, action,
            curve::{CURVE_LINE, HeldBand, PIXELS_PER_NOTCH, Plot, Plotted, across_at},
            note, switch_row,
        },
        spoken::Spoken as _,
    },
    words,
};

const DRAG_TOLD_EVERY: Duration = Duration::from_millis(50);
const CURVE_ASPECT: f32 = 2.6;

const CURVE_MARKED_AT_HZ: [f64; 3] = [100.0, 1_000.0, 10_000.0];
const LEVEL_MARKED_EVERY_DB: f32 = 5.0;
const DRAWN_AT: SampleRate = SampleRate::HZ_48000;
const HANDLE_RADIUS: f32 = 5.0;
const CHOSEN_HANDLE_RADIUS: f32 = 6.5;
const HANDLE_BORDER: f32 = 2.0;
const CHANNEL_SWATCH: f32 = 12.0;

impl RootView {
    pub(super) fn equalising_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let on = self.equaliser.read(cx).bindings().enabled;
        let bound = self.equaliser.read(cx).bindings().binds_anything();

        kit::rows()
            .child(self.in_the_ring(
                "equaliser",
                switch_row(
                    words::settings_equaliser::apply_the_equaliser(),
                    if bound {
                        words::settings_equaliser::off_the_samples_reach_the_device()
                    } else {
                        words::settings_equaliser::nothing_is_bound_to_any_device()
                    },
                    on,
                    "equaliser",
                ),
                move |this, _, cx| this.switch_the_equaliser(!on, cx),
                cx,
            ))
            .note(words::settings_equaliser::not_bit_perfect())
    }

    pub(super) fn bound_to_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let devices: Vec<(NodeName, String)> = self
            .player
            .read(cx)
            .sinks()
            .iter()
            .map(|sink| (sink.name.clone(), sink.description.clone()))
            .collect();
        let kept = self.equaliser.read(cx).kept().to_vec();
        let away = unplugged(self.equaliser.read(cx).bindings(), &devices);

        let mut listed = kit::rows().child(self.binding_row(
            None,
            words::settings_equaliser::every_other_device().to_owned(),
            &kept,
            cx,
        ));
        for (name, description) in devices {
            listed = listed.child(self.binding_row(Some(name), description, &kept, cx));
        }
        for name in away {
            listed = listed.child(self.binding_row(
                Some(name),
                words::settings_equaliser::not_plugged_in().to_owned(),
                &kept,
                cx,
            ));
        }

        listed.note(words::settings_equaliser::bound_to_note())
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
            || words::settings_equaliser::the_binding_a_device_that_names().to_owned(),
            |sink| sink.as_str().to_owned(),
        );

        let mut choices = kit::segmented()
            .child(self.bound_option(
                sink.clone(),
                None,
                if sink.is_some() && bindings.fallback.is_some() {
                    words::settings_equaliser::follows_the_default().to_owned()
                } else {
                    words::settings_equaliser::bound_to_nothing().to_owned()
                },
                named.is_none(),
                cx,
            ))
            .child(self.bound_option(
                sink.clone(),
                Some(Binding::Own),
                words::settings_equaliser::its_own_curve().to_owned(),
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

        kit::stacked_row(description, under, div().flex().child(choices))
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

        self.option_named(
            kit::segment(gpui::SharedString::from(id.clone()), label, chosen),
            id,
            move |this, _, cx| this.bind_a_curve(sink.clone(), binding.clone(), cx),
            cx,
        )
    }

    pub(super) fn bands_group(&mut self, cx: &mut Context<Self>) -> Rows {
        self.follow_the_binding(cx);
        let rate = self.curve_rate(cx);

        let model = self.equaliser.read(cx);
        let showing = model.shown_curve().cloned();
        let bands = model.shown_bands();
        let preamp = model
            .shown()
            .map_or(Preamp::NONE, resonate_core::eq::Profile::preamp);
        let peak = model.peak_db(rate);
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
            return kit::rows()
                .child(curve)
                .child(self.band_actions(false, cx))
                .note(words::settings_equaliser::nothing_bound());
        };

        let mut listed = kit::rows().child(curve).child(kit::row(
            shown.spoken(),
            words::settings_equaliser::reaches_before_the_preamp(format!("{peak:+.2}")),
            self.preamp_cell(preamp, cx),
        ));
        for (row, band) in bands.iter().enumerate() {
            listed = listed.child(self.band_row(row, *band, chosen == Some(row), cx));
        }

        listed
            .child(self.band_actions(true, cx))
            .note(words::settings_equaliser::shaped_by_hand())
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
            .debug_selector(|| "equaliser-curve".to_owned())
            .pictured_as(words::settings_equaliser::the_curve(), || {
                curve_said(&handles.bands, widest)
            })
            .relative()
            .w_full()
            .aspect_ratio(CURVE_ASPECT)
            .min_h(px(theme::curve_height_at_least()))
            .max_h(px(theme::curve_height_at_most()))
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::soft_edge()))
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
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                if this.turn_a_q_at(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .when(drawn.len() > 1, |curve| {
                curve.child(channels_told_apart(&drawn))
            })
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
                    words::settings_equaliser::fit(),
                    words::settings_equaliser::set_the_preamp_from_the_curves(),
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
            .when(chosen, |line| line.bg(kit::chosen_wash()))
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
                    words::settings_equaliser::take_this_band_out(),
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
                .hover(|number| number.bg(rgb(theme::hover())))
                .border_1()
                .border_color(gpui::transparent_black())
                .child(
                    kit::figure(format!("{}", row + 1))
                        .when(chosen, |figure| figure.text_color(rgb(theme::accent()))),
                )
                .names(words::settings_equaliser::band_keys_hint(
                    key!(band_higher),
                    key!(band_louder),
                    key!(band_lower),
                    key!(band_narrower),
                    key!(band_quieter),
                    key!(band_wider),
                )),
            move |this, _, cx| this.choose_a_band(row, cx),
            cx,
        )
        .key_context(
            gpui::KeyContext::parse(&format!("{CONTROL_CONTEXT} {BAND_CONTEXT}"))
                .unwrap_or_default(),
        )
        .on_action(cx.listener(move |this, _: &BandHigher, _, cx| {
            this.nudge_a_band(row, Nudge::Higher, cx);
        }))
        .on_action(cx.listener(move |this, _: &BandLower, _, cx| {
            this.nudge_a_band(row, Nudge::Lower, cx);
        }))
        .on_action(cx.listener(move |this, _: &BandLouder, _, cx| {
            this.nudge_a_band(row, Nudge::Louder, cx);
        }))
        .on_action(cx.listener(move |this, _: &BandQuieter, _, cx| {
            this.nudge_a_band(row, Nudge::Quieter, cx);
        }))
        .on_action(cx.listener(move |this, _: &BandNarrower, _, cx| {
            this.nudge_a_band(row, Nudge::Narrower, cx);
        }))
        .on_action(cx.listener(move |this, _: &BandWider, _, cx| {
            this.nudge_a_band(row, Nudge::Wider, cx);
        }))
    }

    fn kind_cell(&self, row: usize, kind: BandKind, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_ring_at(
            format!("band-kind-{row}"),
            kit::pick(
                gpui::SharedString::from(format!("band-kind-{row}")),
                kind.label(),
                words::settings_equaliser::what_shape_this_band_gives_the(),
            ),
            move |this, _, cx| this.shape_a_band(row, cx),
            cx,
        )
    }

    fn kinds(&self, row: usize, chosen: BandKind, cx: &mut Context<Self>) -> Div {
        let mut offered = kit::segmented();
        for kind in BandKind::ALL {
            let id = format!("band-kind-{row}-{}", kind.as_str());
            offered = offered.child(self.option_named(
                kit::segment(
                    gpui::SharedString::from(id.clone()),
                    kind.label(),
                    kind == chosen,
                ),
                id,
                move |this, _, cx| this.retype_a_band(row, kind, cx),
                cx,
            ));
        }
        div().flex().pt_2().child(offered)
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
        let selector = id.clone();
        div().child(
            self.in_the_ring_at(
                id.clone(),
                frame
                    .id(gpui::SharedString::from(id))
                    .debug_selector(move || selector)
                    .cursor_text()
                    .hover(|cell| cell.bg(rgb(theme::hover())))
                    .child(kit::figure(shown)),
                move |this, window, cx| this.edit_a_cell(row, cell, window, cx),
                cx,
            ),
        )
    }

    fn band_actions(&self, showing: bool, cx: &mut Context<Self>) -> Div {
        let own = matches!(
            self.equaliser.read(cx).shown_curve(),
            Some(crate::equaliser::Curve::Own(_))
        );
        let armed = self.is_armed(&Armed::DiscardTheCurve);

        kit::action_row()
            .child(action(
                "add-a-band",
                words::settings_equaliser::add_a_band(),
                Icon::Plus,
                false,
                |this, _, cx| this.add_a_band(cx),
                self,
                cx,
            ))
            .child(action(
                "import-a-profile",
                words::settings_equaliser::import(),
                Icon::Import,
                false,
                |this, window, cx| this.import_a_profile(window, cx),
                self,
                cx,
            ))
            .when(showing, |row| {
                row.child(action(
                    "export-a-profile",
                    words::settings_equaliser::export(),
                    Icon::Export,
                    false,
                    |this, window, cx| this.export_a_profile(window, cx),
                    self,
                    cx,
                ))
            })
            .when(showing && own, |row| {
                row.child(action(
                    "keep-the-curve",
                    words::settings_equaliser::keep_as_a_profile(),
                    Icon::Plus,
                    false,
                    |this, _, cx| this.keep_the_curve(cx),
                    self,
                    cx,
                ))
            })
            .when(showing, |row| {
                row.child(action(
                    "discard-the-curve",
                    if armed {
                        words::settings_equaliser::press_again_to_discard()
                    } else {
                        words::settings_equaliser::discard()
                    },
                    Icon::Discard,
                    false,
                    move |this, _, cx| {
                        if armed {
                            this.discard_the_curve(cx);
                        } else {
                            this.arm(Armed::DiscardTheCurve);
                            cx.notify();
                        }
                    },
                    self,
                    cx,
                ))
            })
    }

    pub(super) fn autoeq_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let in_use = self
            .player
            .read(cx)
            .sink_in_use()
            .map(|sink| (sink.name.clone(), sink.description.clone()));
        let model = self.equaliser.read(cx);
        let reachable = model.has_a_source();
        let looking = model.is_looking();
        let found: Vec<Device> = model.found().into_iter().cloned().collect();
        let suggested = in_use
            .as_ref()
            .and_then(|(name, description)| model.suggested(description, Some(name)))
            .cloned();
        let knows = model.knows_the_catalogue();

        let mut listed = kit::rows()
            .when_some(in_use, |body, (_, description)| {
                body.child(self.plugged_in(&description, suggested.as_ref(), knows, reachable, cx))
            })
            .when(!reachable, |body| {
                body.child(note(
                    words::settings_equaliser::nothing_is_reached_turn_online_on(),
                ))
            })
            .when(reachable, |body| {
                body.child(self.text_field(
                    "look-for-a-device",
                    &self.looking,
                    |this, window, cx| this.let_go_of_the_field(window, cx),
                    cx,
                ))
                .when(looking, |held| {
                    held.child(note(words::settings_equaliser::asking_autoeq()))
                })
            });
        if reachable {
            for device in &found {
                listed = listed.child(self.measured_row(device, cx));
            }
        }
        listed.note(words::settings_equaliser::autoeq_note())
    }

    pub(super) fn measurers_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let in_use = self
            .player
            .read(cx)
            .sink_in_use()
            .map(|sink| (sink.name.clone(), sink.description.clone()));

        kit::rows()
            .child(self.measurer_choice(cx))
            .when_some(in_use, |body, (name, description)| {
                body.child(self.measurer_choice_for(name, &description, cx))
            })
            .note(words::settings_equaliser::measurers_note())
    }

    pub(super) fn own_measurement_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let model = self.equaliser.read(cx);
        let goal = model.goal().clone();
        let fitting = model.is_fitting_a_measurement();

        let aimed_at = match &goal {
            Goal::BuiltIn(built_in) => built_in.label().to_owned(),
            Goal::File(_) => goal.spoken(),
        };
        let target = self.in_the_pane_ring_at(
            kit::pick(
                "fit-target",
                aimed_at,
                words::settings_equaliser::fit_toward_hint(),
            ),
            move |this, at, cx| {
                let menu = fit_toward_menu(at, &this.equaliser.read(cx).goal().clone());
                this.open_a_menu(menu, cx);
            },
            cx,
        );

        kit::rows()
            .child(kit::row(
                words::settings_equaliser::fit_toward(),
                words::settings_equaliser::fit_toward_hint(),
                target,
            ))
            .child(kit::action_row().child(action(
                "fit-a-measurement",
                if fitting {
                    words::settings_equaliser::fitting()
                } else {
                    words::settings_equaliser::fit_a_measurement()
                },
                Icon::Import,
                fitting,
                |this, _, cx| this.choose_a_measurement(cx),
                self,
                cx,
            )))
            .note(words::settings_equaliser::own_measurement_note())
    }

    pub(super) fn room_measurement_group(&mut self, cx: &mut Context<Self>) -> Rows {
        if !self.listen.read(cx).has_looked_for_microphones() {
            self.listen
                .update(cx, |listen, cx| listen.look_for_microphones(cx));
        }
        let listen = self.listen.read(cx);
        let from = listen.from().clone();
        let microphones = listen.microphones().to_vec();
        let measuring = self.equaliser.read(cx).is_measuring_the_room();

        let heard_through = match &from {
            Listening::Desktop => words::settings_equaliser::desktop_heard().to_owned(),
            Listening::Microphone(None) => {
                words::settings_equaliser::default_microphone_heard().to_owned()
            }
            Listening::Microphone(Some(node)) => microphones
                .iter()
                .find(|(name, _)| *name == node.to_string())
                .map_or_else(|| node.to_string(), |(_, described)| described.clone()),
        };
        let sources = self.in_the_pane_ring_at(
            kit::pick(
                "measure-from",
                heard_through,
                words::settings_equaliser::heard_through_hint(),
            ),
            move |this, at, cx| {
                let listen = this.listen.read(cx);
                let menu = heard_through_menu(at, listen.from(), listen.microphones());
                this.open_a_menu(menu, cx);
            },
            cx,
        );
        let held = self.equaliser.read(cx).calibration().cloned();
        let named = held
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned());

        kit::rows()
            .child(kit::row(
                words::settings_equaliser::heard_through(),
                words::settings_equaliser::heard_through_hint(),
                sources,
            ))
            .child(kit::stacked_row(
                words::settings_equaliser::microphone_calibration(),
                named
                    .clone()
                    .unwrap_or_else(|| words::settings_equaliser::uncalibrated().to_owned()),
                kit::action_row()
                    .child(action(
                        "measure-calibration",
                        words::settings_equaliser::choose_a_calibration(),
                        Icon::Import,
                        measuring,
                        |this, _, cx| this.choose_a_calibration(cx),
                        self,
                        cx,
                    ))
                    .when(named.is_some(), |row| {
                        row.child(action(
                            "measure-calibration-forget",
                            words::settings_equaliser::remove(),
                            Icon::Close,
                            measuring,
                            |this, _, cx| this.calibrate_with(None, cx),
                            self,
                            cx,
                        ))
                    }),
            ))
            .child(kit::field(
                words::settings_equaliser::keep_as(),
                self.text_field(
                    "measure-profile-name",
                    &self.measured_as,
                    |this, window, cx| this.let_go_of_the_field(window, cx),
                    cx,
                ),
            ))
            .child(
                kit::action_row()
                    .child(action(
                        "measure-the-room",
                        if measuring {
                            words::settings_equaliser::measuring()
                        } else {
                            words::settings_equaliser::measure()
                        },
                        Icon::Listen,
                        measuring,
                        |this, _, cx| this.measure_the_room(cx),
                        self,
                        cx,
                    ))
                    .when(measuring, |row| {
                        row.child(action(
                            "stop-measuring",
                            words::settings_equaliser::stop(),
                            Icon::Stop,
                            false,
                            |this, _, cx| this.stop_measuring_the_room(cx),
                            self,
                            cx,
                        ))
                    }),
            )
            .note(words::settings_equaliser::measure_note())
    }

    pub(crate) fn calibrate_with(&mut self, file: Option<PathBuf>, cx: &mut Context<Self>) {
        self.equaliser.update(cx, |model, cx| {
            model.calibrate_with(file);
            cx.notify();
        });
        cx.notify();
    }

    fn choose_a_calibration(&self, cx: &mut Context<Self>) {
        self.pick_a_file(
            words::settings_equaliser::calibrate_with(),
            |this, file, cx| this.calibrate_with(Some(file), cx),
            cx,
        );
    }

    pub(crate) fn stop_measuring_the_room(&mut self, cx: &mut Context<Self>) {
        self.equaliser.update(cx, |model, cx| {
            model.stop_measuring_the_room();
            cx.notify();
        });
        cx.notify();
    }

    fn measure_the_room(&mut self, cx: &mut Context<Self>) {
        let Some(rig) = cx.global::<ResonateApp>().measures.clone() else {
            toast::tell(
                Notice::Trouble(words::settings_equaliser::no_measuring().to_owned()),
                cx,
            );
            return;
        };
        let through = self
            .player
            .read(cx)
            .sink_in_use()
            .map(|sink| sink.name.clone());
        let from = self.listen.read(cx).from().clone();
        let typed = self.measured_as.read(cx).text().trim().to_owned();
        let called = if typed.is_empty() {
            None
        } else {
            match ProfileName::new(&typed) {
                Ok(name) => Some(name),
                Err(error) => {
                    toast::tell(
                        toast::eq_could_not(
                            words::settings_equaliser::keep_a_profile_under_that_name(),
                            &error,
                        ),
                        cx,
                    );
                    return;
                }
            }
        };
        self.equaliser.update(cx, |model, cx| {
            model.measure_the_room(
                Sounding {
                    rig,
                    through,
                    from,
                    called,
                },
                cx,
            );
        });
        cx.notify();
    }

    fn aim_at(&mut self, goal: Goal, cx: &mut Context<Self>) {
        self.equaliser.update(cx, |model, cx| {
            model.aim_at(goal);
            cx.notify();
        });
        cx.notify();
    }

    fn pick_a_file(
        &self,
        prompt: &'static str,
        taken: impl Fn(&mut Self, PathBuf, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::new_static(prompt)),
        });

        cx.spawn(async move |this, cx| {
            let answered = chosen.await;
            let outcome = this.update(cx, |this, cx| match answered {
                Ok(Ok(Some(paths))) => {
                    if let Some(file) = paths.into_iter().next() {
                        taken(this, file, cx);
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    toast::tell(
                        Notice::Trouble(words::settings_equaliser::no_file_picker().to_owned()),
                        cx,
                    );
                }
                Err(error) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    toast::tell(
                        Notice::Trouble(words::settings_equaliser::no_file_picker().to_owned()),
                        cx,
                    );
                }
            });
            let _ = outcome;
        })
        .detach();
    }

    fn choose_a_measurement(&self, cx: &mut Context<Self>) {
        self.pick_a_file(
            words::settings_equaliser::fit(),
            |this, file, cx| {
                this.equaliser
                    .update(cx, |model, cx| model.fit_a_measurement(file, cx));
                cx.notify();
            },
            cx,
        );
    }

    fn choose_a_target(&self, cx: &mut Context<Self>) {
        self.pick_a_file(
            words::settings_equaliser::choose(),
            |this, file, cx| this.aim_at(Goal::File(file), cx),
            cx,
        );
    }

    fn measurer_choice(&mut self, cx: &mut Context<Self>) -> Div {
        let preferred = cx.global::<ResonateApp>().measurers.fallback.clone();
        let choices = self.measurer_pick(
            MeasurerPick {
                id: "measurer",
                unset: words::settings_equaliser::best_ranked(),
                saying: words::settings_equaliser::whose_correction_to_prefer_where_several(),
                preferred,
            },
            |this, measurer, cx| this.prefer_a_measurer(measurer, cx),
            cx,
        );

        kit::row(
            words::settings_equaliser::for_every_device(),
            words::settings_equaliser::best_ranked_leaves_it_to_the(),
            choices,
        )
    }

    fn measurer_choice_for(
        &mut self,
        sink: NodeName,
        description: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let preferred = cx
            .global::<ResonateApp>()
            .measurers
            .of_the_device(&sink)
            .map(str::to_owned);
        let choices = self.measurer_pick(
            MeasurerPick {
                id: "device-measurer",
                unset: words::settings_equaliser::as_for_the_rest(),
                saying: words::settings_equaliser::whose_correction_to_prefer_for_this(),
                preferred,
            },
            move |this, measurer, cx| this.prefer_a_measurer_for(sink.clone(), measurer, cx),
            cx,
        );

        kit::row(
            words::settings_equaliser::for_the_device(description),
            words::settings_equaliser::as_for_the_rest_takes_the(),
            choices,
        )
    }

    fn measurer_pick(
        &self,
        offered: MeasurerPick,
        pick: impl Fn(&mut Self, Option<String>, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let held = offered
            .preferred
            .clone()
            .unwrap_or_else(|| offered.unset.to_owned());

        self.in_the_pane_ring_at(
            kit::pick(offered.id, held, offered.saying),
            move |this, at, cx| {
                this.open_a_menu(measurer_menu(at, &offered, &pick), cx);
            },
            cx,
        )
    }

    pub(crate) fn prefer_a_measurer(&mut self, measurer: Option<String>, cx: &mut Context<Self>) {
        let mut measurers = cx.global::<ResonateApp>().measurers.clone();
        measurers.prefer_for_every_device(measurer.clone());

        self.wear_the_measurers(measurers, cx);
        self.store(&Setting::EqualiserMeasurer(measurer), cx);
        cx.notify();
    }

    pub(crate) fn prefer_a_measurer_for(
        &mut self,
        sink: NodeName,
        measurer: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut measurers = cx.global::<ResonateApp>().measurers.clone();
        measurers.prefer_for(sink.clone(), measurer.clone());

        self.wear_the_measurers(measurers, cx);
        self.store(&Setting::EqualiserMeasurerFor { sink, measurer }, cx);
        cx.notify();
    }

    pub(crate) fn wear_the_measurers(&mut self, measurers: Measurers, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.measurers = measurers.clone());

        let typed = self.looking.read(cx).text().to_owned();
        let sink = self.the_sink_in_use(cx);
        self.equaliser.update(cx, |model, cx| {
            model.prefer(measurers);
            if model.knows_the_catalogue() {
                model.look(&typed, sink.as_ref(), cx);
            }
            cx.notify();
        });
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
                    .child(
                        div()
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(device.label.clone()),
                    )
                    .child(kit::badge(device.measured_by.clone(), theme::muted()))
                    .when_some(device.rig.clone(), |row, rig| row.child(kit::figure(rig))),
            )
            .child(self.in_the_ring_at(
                id.clone(),
                kit::button(
                    gpui::SharedString::from(id),
                    Some(Icon::Import),
                    words::settings_equaliser::use_this(),
                    words::settings_equaliser::fetch_this_measurement_and_keep_it(),
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
            .child(kit::named(
                words::settings_equaliser::what_is_plugged_in(),
                description.to_owned(),
            ))
            .when(reachable && !knows, |body| {
                body.child(kit::action_row().child(self.in_the_ring(
                    "read-the-catalogue",
                    kit::button(
                        "read-the-catalogue",
                        Some(Icon::Globe),
                        words::settings_equaliser::look_it_up(),
                        words::settings_equaliser::read_autoeqs_list_of_measured_devices(),
                        Tone::Outlined,
                    ),
                    |this, _, cx| this.read_the_catalogue(cx),
                    cx,
                )))
            })
            .when(knows && suggested.is_none(), |body| {
                body.child(note(
                    words::settings_equaliser::nothing_measured_answers_to_that_name(),
                ))
            })
            .when_some(suggested.cloned(), |body, device| {
                body.child(self.measured_row(&device, cx))
            })
    }
}

#[derive(Clone)]
struct MeasurerPick {
    id: &'static str,
    unset: &'static str,
    saying: &'static str,
    preferred: Option<String>,
}

fn measurer_menu(
    at: Point<Pixels>,
    offered: &MeasurerPick,
    pick: &(impl Fn(&mut RootView, Option<String>, &mut Context<RootView>) + Clone + 'static),
) -> Menu {
    let is_preferred = |name: &str| {
        offered
            .preferred
            .as_deref()
            .is_some_and(|held| held.eq_ignore_ascii_case(name))
    };
    let picked = pick.clone();
    let mut menu = Menu::at(at).chooses(
        offered.unset,
        offered.preferred.is_none(),
        move |this, _, cx| picked(this, None, cx),
    );
    for name in MEASURERS_BEST_FIRST {
        let picked = pick.clone();
        menu = menu.chooses(name, is_preferred(name), move |this, _, cx| {
            picked(this, Some(name.to_owned()), cx);
        });
    }
    let outside_the_ranking = offered.preferred.clone().filter(|held| {
        !MEASURERS_BEST_FIRST
            .iter()
            .any(|name| name.eq_ignore_ascii_case(held))
    });
    match outside_the_ranking {
        Some(held) => {
            let picked = pick.clone();
            let named = held.clone();
            menu.chooses(held, true, move |this, _, cx| {
                picked(this, Some(named.clone()), cx);
            })
        }
        None => menu,
    }
}

fn fit_toward_menu(at: Point<Pixels>, goal: &Goal) -> Menu {
    let mut menu = Menu::at(at);
    for built_in in BuiltIn::ALL {
        menu = menu.chooses(
            built_in.label(),
            *goal == Goal::BuiltIn(built_in),
            move |this, _, cx| this.aim_at(Goal::BuiltIn(built_in), cx),
        );
    }

    menu.apart().chooses(
        words::settings_equaliser::a_file_as_the_target(),
        matches!(goal, Goal::File(_)),
        |this, _, cx| this.choose_a_target(cx),
    )
}

fn heard_through_menu(
    at: Point<Pixels>,
    from: &Listening,
    microphones: &[(String, String)],
) -> Menu {
    let hears = |source: Listening| {
        move |this: &mut RootView, _: &mut Window, cx: &mut Context<RootView>| {
            let source = source.clone();
            this.listen
                .update(cx, |listen, cx| listen.choose(source, cx));
            cx.notify();
        }
    };
    let mut menu = Menu::at(at)
        .chooses(
            words::settings_equaliser::desktop_heard(),
            *from == Listening::Desktop,
            hears(Listening::Desktop),
        )
        .chooses(
            words::settings_equaliser::default_microphone_heard(),
            *from == Listening::Microphone(None),
            hears(Listening::Microphone(None)),
        );
    for (name, described) in microphones {
        let source = Listening::Microphone(Some(NodeName::new(name.clone())));
        menu = menu.chooses(described.clone(), *from == source, hears(source.clone()));
    }

    menu
}

fn curve_said(bands: &[Band], widest: f32) -> String {
    let drawn = words::settings_equaliser::curve_drawn(bands.len(), format!("{widest:.0}"));
    if bands.is_empty() {
        return drawn;
    }

    let described: Vec<String> = bands
        .iter()
        .enumerate()
        .map(|(row, band)| format!("{}, {}", row + 1, band_said(*band)))
        .collect();

    format!("{drawn}: {}", described.join("; "))
}

fn band_said(band: Band) -> String {
    let hertz = band.frequency.hertz();
    let at = match hertz >= HERTZ_A_KILOHERTZ {
        true => words::settings_equaliser::kilohertz(trimmed(hertz / HERTZ_A_KILOHERTZ)),
        false => words::settings_equaliser::hertz(trimmed(hertz)),
    };
    let gain = match band.kind.uses_gain() {
        true => words::settings_equaliser::band_gain(format!("{:+.1}", band.gain.decibels())),
        false => String::new(),
    };
    let off = match band.on {
        true => "",
        false => words::settings_equaliser::off(),
    };

    words::settings_equaliser::band_said(at, gain, band.kind.label().to_lowercase(), off)
}

fn trimmed(number: f64) -> String {
    let written = format!("{number:.2}");
    written
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn widest_drawn(traced: &[Traced]) -> f32 {
    let floor = DRAWN_BETWEEN_MILLIBELS as f32 / 1_000.0;
    let every = traced.iter().flat_map(|traced| traced.decibels.iter());
    let peak = every.fold(0.0_f64, |highest, decibels| {
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
    marked_at(CURVE_MARKED_AT_HZ, across_at)
}

pub(crate) fn marked_at(
    marks: impl IntoIterator<Item = f64>,
    across: impl Fn(f64) -> f32,
) -> Vec<Div> {
    marks
        .into_iter()
        .map(|hertz| {
            axis_label(spelt_hertz(hertz))
                .absolute()
                .bottom_1()
                .left(relative(across(hertz)))
        })
        .collect()
}

struct Handles {
    bands: Vec<Band>,
    chosen: Option<usize>,
    lift: Preamp,
}

fn channels_told_apart(traced: &[Traced]) -> Div {
    div()
        .absolute()
        .top_1()
        .left_2()
        .flex()
        .flex_wrap()
        .gap_2()
        .children(traced.iter().enumerate().map(|(nth, traced)| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .w(px(CHANNEL_SWATCH))
                        .h(px(CURVE_LINE * 2.0))
                        .rounded_sm()
                        .bg(rgb(ink_of(traced.on, nth))),
                )
                .child(axis_label(spoken_channel(traced.on).to_owned()))
        }))
}

fn ink_of(on: TracedOn, nth: usize) -> u32 {
    match on {
        TracedOn::EveryChannel | TracedOn::EveryOtherChannel => theme::accent(),
        TracedOn::Channel(_) => theme::beside_the_accent(nth),
    }
}

fn spoken_channel(on: TracedOn) -> &'static str {
    match on {
        TracedOn::EveryChannel => words::settings_equaliser::every_channel(),
        TracedOn::EveryOtherChannel => words::settings_equaliser::every_other_channel(),
        TracedOn::Channel(channel) => match channel {
            0 => words::settings_equaliser::left(),
            1 => words::settings_equaliser::right(),
            2 => words::settings_equaliser::centre(),
            3 => words::settings_equaliser::subwoofer(),
            4 => words::settings_equaliser::rear_left(),
            5 => words::settings_equaliser::rear_right(),
            6 => words::settings_equaliser::side_left(),
            7 => words::settings_equaliser::side_right(),
            _ => words::settings_equaliser::a_channel(),
        },
    }
}

fn traced(
    curve: std::sync::Arc<[Traced]>,
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

            plot::stroke(
                window,
                &[
                    point(bounds.left(), level(0.0)),
                    point(bounds.right(), level(0.0)),
                ],
                1.0,
                rgb(theme::soft_edge()),
            );

            let plotted_of = |traced: &Traced| -> Vec<gpui::Point<Pixels>> {
                traced
                    .decibels
                    .iter()
                    .enumerate()
                    .map(|(at, decibels)| point(across(at), level(*decibels)))
                    .collect()
            };

            for traced in curve.iter().filter(|traced| !traced.on.is_one_channel()) {
                let plotted = plotted_of(traced);
                plot::wash(
                    window,
                    &plotted,
                    level(0.0),
                    linear_gradient(
                        180.0,
                        linear_color_stop(theme::tinted(theme::accent(), 0x40), 0.0),
                        linear_color_stop(theme::tinted(theme::accent(), 0x10), 1.0),
                    ),
                );
                plot::stroke(window, &plotted, CURVE_LINE, rgb(theme::accent()));
            }
            for (nth, traced) in curve.iter().enumerate() {
                if traced.on.is_one_channel() {
                    let plotted = plotted_of(traced);
                    plot::stroke(window, &plotted, CURVE_LINE, rgb(ink_of(traced.on, nth)));
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

    pub(crate) fn carry_bindings_to_renamed_sinks(
        &mut self,
        player: &Entity<PlayerModel>,
        cx: &mut Context<Self>,
    ) {
        let sinks = player.read(cx).sinks();
        let bindings = self.equaliser.read(cx).bindings().carried_to(sinks);
        let measurers = cx.global::<ResonateApp>().measurers.carried_to(sinks);
        if bindings.is_empty() && measurers.is_empty() {
            return;
        }

        for carry in bindings {
            let Some(binding) = self
                .equaliser
                .read(cx)
                .bindings()
                .named(Some(&carry.from))
                .cloned()
            else {
                continue;
            };
            if !self
                .equaliser
                .update(cx, |model, _| model.carry_over(&carry))
            {
                continue;
            }
            self.store(
                &Setting::EqualiserFor {
                    sink: carry.from,
                    binding: None,
                },
                cx,
            );
            self.store(
                &Setting::EqualiserFor {
                    sink: carry.to,
                    binding: Some(binding),
                },
                cx,
            );
            self.tell_the_engine(cx);
            self.follow_the_binding(cx);
        }
        for carry in measurers {
            let held = cx
                .global::<ResonateApp>()
                .measurers
                .of_the_device(&carry.from)
                .map(str::to_owned);
            if let Some(measurer) = held {
                self.prefer_a_measurer_for(carry.from, None, cx);
                self.prefer_a_measurer_for(carry.to, Some(measurer), cx);
            }
        }
        cx.notify();
    }

    fn the_sink_in_use(&self, cx: &mut Context<Self>) -> Option<NodeName> {
        self.player
            .read(cx)
            .sink_in_use()
            .map(|sink| sink.name.clone())
    }

    fn shape_its_own_curve(&mut self, cx: &mut Context<Self>) {
        if self.equaliser.read(cx).shown().is_some() {
            return;
        }
        let sink = self.the_sink_in_use(cx);
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
            self.tell_the_engine_at_most_so_often(cx);
            cx.notify();
        }
    }

    fn tell_the_engine_at_most_so_often(&mut self, cx: &mut Context<Self>) {
        if let Some(untold) = self.band_untold.as_mut() {
            *untold = true;
            return;
        }
        self.tell_the_engine(cx);
        self.band_untold = Some(false);
        self.band_retuning = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DRAG_TOLD_EVERY).await;
            let _ = this.update(cx, |this, cx| {
                if this.band_untold.take() == Some(true) {
                    this.tell_the_engine_at_most_so_often(cx);
                }
            });
        });
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

    fn turn_a_q_at(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) -> bool {
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

    fn nudge_a_band(&mut self, row: usize, nudge: Nudge, cx: &mut Context<Self>) {
        let moved = self
            .equaliser
            .update(cx, |model, cx| model.nudge(row, nudge, cx));
        if moved {
            self.tell_the_engine(cx);
            cx.notify();
        }
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

    pub(crate) fn follow_the_bindings_read_again(
        &mut self,
        bindings: Bindings,
        cx: &mut Context<Self>,
    ) {
        self.held_band = None;
        self.equaliser
            .update(cx, |model, _| model.rebound(bindings));
        self.follow_the_binding(cx);
        cx.notify();
    }

    pub(crate) fn follow_the_binding(&mut self, cx: &mut Context<Self>) {
        let sink = self.the_sink_in_use(cx);
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
            figure.open_with(written, cx);
            figure.take_focus(window, cx);
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
                .update(cx, |figure, cx| figure.open_with(String::new(), cx));
            self.controls.lets_go(window, cx);
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
            .update(cx, |figure, cx| figure.open_with(String::new(), cx));
        self.controls.lets_go(window, cx);
        cx.notify();
    }

    pub(crate) fn figure_left(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.equaliser.read(cx).editing() else {
            return;
        };
        let typed = self.figure.read(cx).text().to_owned();
        let taken = self
            .equaliser
            .update(cx, |model, cx| model.take(editing, &typed, cx));

        if taken {
            self.tell_the_engine(cx);
        } else {
            self.equaliser.update(cx, |model, _| model.leave_cell());
        }
        self.figure
            .update(cx, |figure, cx| figure.open_with(String::new(), cx));
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

    pub(super) fn curve_rate(&self, cx: &App) -> SampleRate {
        self.player
            .read(cx)
            .state()
            .output
            .map_or(DRAWN_AT, |output| output.negotiated.rate)
    }

    fn fit_the_preamp(&mut self, cx: &mut Context<Self>) {
        let rate = self.curve_rate(cx);
        self.equaliser
            .update(cx, |model, cx| model.fit_the_preamp(rate, cx));
        self.tell_the_engine(cx);
        self.fit_the_preamp_to_the_queue(cx);
        cx.notify();
    }

    fn fit_the_preamp_to_the_queue(&mut self, cx: &mut Context<Self>) {
        self.fitting_the_preamp = Fitting::idle();
        let Some(shown) = self.equaliser.read(cx).shown().cloned() else {
            return;
        };
        let rate = self.curve_rate(cx);
        let engine = self.player.read(cx).engine();
        let library = self.library.read(cx).catalog();

        let room = cx.global::<ResonateApp>().convolution.clone();
        let speakers = speakers_worn(cx);

        let watch = Arc::new(Watch::default());
        let measuring = Arc::clone(&watch);
        let profile = Arc::new(shown);
        let measured_for = Arc::clone(&profile);
        let measured = cx.background_executor().spawn(async move {
            let applied = Applied::from(profile).corrected_by(
                room.as_deref()
                    .and_then(|file| room_correcting_the_fit(file, speakers)),
            );
            fitted_through_the_chain(&library, &engine, &applied, rate, measuring.as_ref())
        });
        let landing = cx.spawn(async move |this, cx| {
            let measured = measured.await;
            let _ = this.update(cx, |this, cx| match measured {
                Ok(Fitted { loudest, room }) => {
                    let Some(fitted) = loudest.fitted_preamp().or(room) else {
                        return;
                    };
                    let landed = this.equaliser.update(cx, |model, cx| {
                        model.fit_the_preamp_to_the_music(&measured_for, fitted, cx)
                    });
                    if landed {
                        this.tell_the_engine(cx);
                        this.report(Notice::Done(fitted_to(loudest.measured)), cx);
                        cx.notify();
                    }
                }
                Err(AnalysisError::Stopped) => {}
                Err(error) => {
                    tracing::warn!(%error, "the queue could not be measured for the preamp")
                }
            });
        });
        self.fitting_the_preamp = Fitting::measuring(watch, landing);
    }

    fn import_a_profile(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(words::settings_equaliser::import_2().into()),
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
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    toast::tell(
                        Notice::Trouble(words::settings_equaliser::no_file_picker().to_owned()),
                        cx,
                    );
                }
                Err(error) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    toast::tell(
                        Notice::Trouble(words::settings_equaliser::no_file_picker().to_owned()),
                        cx,
                    );
                }
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
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    toast::tell(
                        Notice::Trouble(words::settings_equaliser::no_file_picker().to_owned()),
                        cx,
                    );
                }
                Err(error) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    toast::tell(
                        Notice::Trouble(words::settings_equaliser::no_file_picker().to_owned()),
                        cx,
                    );
                }
            });
            let _ = outcome;
        })
        .detach();
    }

    pub(crate) fn look_for_a_device(&mut self, cx: &mut Context<Self>) {
        let typed = self.looking.read(cx).text().to_owned();
        let sink = self.the_sink_in_use(cx);
        self.equaliser
            .update(cx, |model, cx| model.look(&typed, sink.as_ref(), cx));
        cx.notify();
    }

    pub(crate) fn leave_looking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.looking.update(cx, |looking, cx| looking.clear(cx));
        self.look_for_a_device(cx);
        window.focus(&self.focus, cx);
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

    fn keep_the_curve(&mut self, cx: &mut Context<Self>) {
        let Some(crate::equaliser::Curve::Own(owner)) =
            self.equaliser.read(cx).shown_curve().cloned()
        else {
            return;
        };
        let described = owner.as_ref().and_then(|owner| {
            self.player
                .read(cx)
                .sinks()
                .iter()
                .find(|sink| sink.is_named(owner))
                .map(|sink| sink.description.clone())
        });
        let called = match (described, owner) {
            (Some(description), _) => description,
            (None, Some(owner)) => owner.as_str().to_owned(),
            (None, None) => words::settings_equaliser::every_other_device().to_owned(),
        };
        self.equaliser
            .update(cx, |model, cx| model.keep_as_a_profile(&called, cx));
        cx.notify();
    }

    fn discard_the_curve(&mut self, cx: &mut Context<Self>) {
        self.lower(&Armed::DiscardTheCurve);
        let Some(curve) = self.equaliser.read(cx).shown_curve().cloned() else {
            return;
        };
        let bound: Vec<Option<NodeName>> = {
            let bindings = self.equaliser.read(cx).bindings();
            bindings
                .by_sink
                .iter()
                .filter(|(sink, binding)| crate::equaliser::Curve::of(Some(sink), binding) == curve)
                .map(|(sink, _)| Some(sink.clone()))
                .chain(
                    bindings
                        .fallback
                        .as_ref()
                        .filter(|binding| crate::equaliser::Curve::of(None, binding) == curve)
                        .map(|_| None),
                )
                .collect()
        };
        for sink in bound {
            self.bind_a_curve(sink, None, cx);
        }
        self.equaliser
            .update(cx, |model, cx| model.discard(&curve, cx));
        self.tell_the_engine(cx);
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

const HEAD_YAWS: [i16; 5] = [-45, -20, 0, 20, 45];

const SPEAKER_ANGLES: [u8; 5] = [20, 30, 45, 60, 90];

const SPEAKER_ELEVATIONS: [i16; 5] = [0, 10, 20, 30, 45];

const BINAURAL_FILE: &str = "binaural-response.wav";

impl RootView {
    fn room_is_crossed(&self) -> bool {
        self.room_shape
            .as_ref()
            .is_some_and(|(_, crossed)| *crossed)
    }

    fn look_at_the_room(&mut self, file: &Path, cx: &mut Context<Self>) {
        if self
            .room_shape
            .as_ref()
            .is_some_and(|(seen, _)| seen == file)
        {
            return;
        }
        self.room_shape = Some((file.to_path_buf(), false));
        let reading = file.to_path_buf();
        let speakers = speakers_worn(cx);
        let read = cx.background_executor().spawn({
            let reading = reading.clone();
            async move { read_impulse(&Sources::local(), &MediaLocation::local(&reading), speakers) }
        });
        self.looking_at_the_room = cx.spawn(async move |this, cx| {
            let crossed = matches!(read.await, Ok(Some(impulse)) if impulse.is_crossed());
            let _ = this.update(cx, |this, cx| {
                if this
                    .room_shape
                    .as_ref()
                    .is_some_and(|(seen, _)| *seen == reading)
                {
                    this.room_shape = Some((reading, crossed));
                    cx.notify();
                }
            });
        });
    }

    pub(super) fn room_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let held = cx.global::<ResonateApp>().convolution.clone();
        if let Some(file) = &held {
            self.look_at_the_room(file, cx);
        }

        kit::rows()
            .child(match &held {
                Some(file) => note(file.display().to_string()),
                None => note(words::settings_equaliser::no_room()),
            })
            .when(held.is_some() && self.room_is_crossed(), |body| {
                body.child(note(words::settings_equaliser::binaural_note()))
            })
            .child(
                kit::action_row()
                    .child(action(
                        "choose-a-response",
                        words::settings_equaliser::choose_a_response(),
                        Icon::Folder,
                        false,
                        |this, _, cx| this.choose_a_response(cx),
                        self,
                        cx,
                    ))
                    .child(action(
                        "choose-a-speaker-pair",
                        words::settings_equaliser::choose_a_pair(),
                        Icon::Folder,
                        false,
                        |this, _, cx| this.choose_a_speaker_pair(cx),
                        self,
                        cx,
                    ))
                    .when(held.is_some(), |row| {
                        row.child(action(
                            "stop-correcting",
                            words::settings_equaliser::stop_correcting(),
                            Icon::Close,
                            false,
                            |this, _, cx| this.correct_the_room(None, cx),
                            self,
                            cx,
                        ))
                    }),
            )
            .note(words::settings_equaliser::room_note())
    }

    pub(super) fn speakers_group(&mut self, cx: &mut Context<Self>) -> Rows {
        kit::rows()
            .child(self.speaker_angle_field(cx))
            .child(self.speaker_elevation_field(cx))
            .child(self.head_yaw_field(cx))
    }

    fn speaker_angle_field(&mut self, cx: &mut Context<Self>) -> Div {
        let held = cx.global::<ResonateApp>().speaker_angle;
        let mut presets = kit::segmented();
        for degrees in SPEAKER_ANGLES {
            let Ok(angle) = SpeakerAngle::from_degrees(degrees) else {
                continue;
            };
            let id = SharedString::from(format!("speaker-angle-{degrees}"));
            presets = presets.child(
                self.in_the_ring_at(
                    id.to_string(),
                    kit::segment(id, format!("{degrees}°"), held == angle)
                        .names(words::settings_equaliser::speakers_either_side(degrees)),
                    move |this, _, cx| this.set_the_speaker_angle(angle, cx),
                    cx,
                ),
            );
        }
        let stepper = self.degrees_stepper(
            "speaker-angle",
            format!("{held}°"),
            (
                words::settings_equaliser::bring_the_speakers_a_degree_closer(),
                |this, cx| {
                    let held = cx.global::<ResonateApp>().speaker_angle.degrees();
                    if let Ok(angle) = SpeakerAngle::from_degrees(held.saturating_sub(1)) {
                        this.set_the_speaker_angle(angle, cx);
                    }
                },
            ),
            (
                words::settings_equaliser::put_the_speakers_a_degree_further(),
                |this, cx| {
                    let held = cx.global::<ResonateApp>().speaker_angle.degrees();
                    if let Ok(angle) = SpeakerAngle::from_degrees(held.saturating_add(1)) {
                        this.set_the_speaker_angle(angle, cx);
                    }
                },
            ),
            cx,
        );
        kit::field(
            words::settings_equaliser::speaker_angle(),
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .child(stepper)
                .child(presets),
        )
        .child(note(words::settings_equaliser::speaker_note()))
    }

    fn speaker_elevation_field(&mut self, cx: &mut Context<Self>) -> Div {
        let held = cx.global::<ResonateApp>().speaker_elevation;
        let mut presets = kit::segmented();
        for degrees in SPEAKER_ELEVATIONS {
            let Ok(elevation) = SpeakerElevation::from_degrees(degrees) else {
                continue;
            };
            let id = SharedString::from(format!("speaker-elevation-{degrees}"));
            presets = presets.child(
                self.in_the_ring_at(
                    id.to_string(),
                    kit::segment(id, format!("{degrees}°"), held == elevation)
                        .names(words::settings_equaliser::speakers_above_the_ears(degrees)),
                    move |this, _, cx| this.set_the_speaker_elevation(elevation, cx),
                    cx,
                ),
            );
        }
        let stepper = self.degrees_stepper(
            "speaker-elevation",
            format!("{held}°"),
            (
                words::settings_equaliser::lower_the_speakers_a_degree(),
                |this, cx| {
                    let held = i16::from(cx.global::<ResonateApp>().speaker_elevation.degrees());
                    if let Ok(elevation) = SpeakerElevation::from_degrees(held - 1) {
                        this.set_the_speaker_elevation(elevation, cx);
                    }
                },
            ),
            (
                words::settings_equaliser::raise_the_speakers_a_degree(),
                |this, cx| {
                    let held = i16::from(cx.global::<ResonateApp>().speaker_elevation.degrees());
                    if let Ok(elevation) = SpeakerElevation::from_degrees(held + 1) {
                        this.set_the_speaker_elevation(elevation, cx);
                    }
                },
            ),
            cx,
        );
        kit::field(
            words::settings_equaliser::speaker_elevation(),
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .child(stepper)
                .child(presets),
        )
        .child(note(words::settings_equaliser::elevation_note()))
    }

    fn head_yaw_field(&mut self, cx: &mut Context<Self>) -> Div {
        let held = cx.global::<ResonateApp>().head_yaw;
        let mut presets = kit::segmented();
        for degrees in HEAD_YAWS {
            let Ok(yaw) = HeadYaw::from_degrees(degrees) else {
                continue;
            };
            let id = SharedString::from(format!("head-yaw-{degrees}"));
            presets = presets.child(
                self.in_the_ring_at(
                    id.to_string(),
                    kit::segment(id, format!("{degrees}°"), held == yaw)
                        .names(words::settings_equaliser::head_turned_by(degrees)),
                    move |this, _, cx| this.set_the_head_yaw(yaw, cx),
                    cx,
                ),
            );
        }
        let stepper = self.degrees_stepper(
            "head-yaw",
            format!("{held}°"),
            (
                words::settings_equaliser::turn_the_head_a_degree_to(),
                |this, cx| {
                    let held = i16::from(cx.global::<ResonateApp>().head_yaw.degrees());
                    if let Ok(yaw) = HeadYaw::from_degrees(held - 1) {
                        this.set_the_head_yaw(yaw, cx);
                    }
                },
            ),
            (
                words::settings_equaliser::turn_the_head_a_degree_to_2(),
                |this, cx| {
                    let held = i16::from(cx.global::<ResonateApp>().head_yaw.degrees());
                    if let Ok(yaw) = HeadYaw::from_degrees(held + 1) {
                        this.set_the_head_yaw(yaw, cx);
                    }
                },
            ),
            cx,
        );
        kit::field(
            words::settings_equaliser::head_turned(),
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .child(stepper)
                .child(presets),
        )
        .child(note(words::settings_equaliser::yaw_note()))
    }

    fn degrees_stepper(
        &mut self,
        id: &'static str,
        reading: String,
        less: (&'static str, fn(&mut Self, &mut Context<Self>)),
        more: (&'static str, fn(&mut Self, &mut Context<Self>)),
        cx: &mut Context<Self>,
    ) -> Div {
        let (less_hint, step_less) = less;
        let (more_hint, step_more) = more;
        let less_id = format!("{id}-less");
        let more_id = format!("{id}-more");
        kit::stepper()
            .child(self.in_the_ring_at(
                less_id.clone(),
                kit::step(SharedString::from(less_id), Icon::Minus, less_hint),
                move |this, _, cx| step_less(this, cx),
                cx,
            ))
            .child(kit::stepper_reading(reading))
            .child(self.in_the_ring_at(
                more_id.clone(),
                kit::step(SharedString::from(more_id), Icon::Plus, more_hint),
                move |this, _, cx| step_more(this, cx),
                cx,
            ))
    }

    pub(crate) fn set_the_speaker_angle(&mut self, angle: SpeakerAngle, cx: &mut Context<Self>) {
        if cx.global::<ResonateApp>().speaker_angle == angle {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.speaker_angle = angle);
        self.store(&Setting::SpeakerAngle(angle), cx);
        self.read_the_room_again(cx);
    }

    pub(crate) fn set_the_speaker_elevation(
        &mut self,
        elevation: SpeakerElevation,
        cx: &mut Context<Self>,
    ) {
        if cx.global::<ResonateApp>().speaker_elevation == elevation {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.speaker_elevation = elevation);
        self.store(&Setting::SpeakerElevation(elevation), cx);
        self.read_the_room_again(cx);
    }

    pub(crate) fn set_the_head_yaw(&mut self, yaw: HeadYaw, cx: &mut Context<Self>) {
        if cx.global::<ResonateApp>().head_yaw == yaw {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.head_yaw = yaw);
        self.store(&Setting::HeadYaw(yaw), cx);
        self.read_the_room_again(cx);
    }

    fn read_the_room_again(&mut self, cx: &mut Context<Self>) {
        let held = cx.global::<ResonateApp>().convolution.clone();
        if let Some(file) = held {
            self.correct_the_room(Some(file), cx);
        }
        cx.notify();
    }

    fn choose_a_response(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::new_static(words::settings_equaliser::choose())),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(chosen))) = picked.await else {
                return;
            };
            let Some(file) = chosen.into_iter().next() else {
                return;
            };
            let chose = this.update(cx, |this, cx| this.correct_the_room(Some(file), cx));
            let _ = chose;
        })
        .detach();
    }

    fn choose_a_speaker_pair(&self, cx: &mut Context<Self>) {
        let left = cx.prompt_for_paths(speaker_options(words::settings_equaliser::left_speaker()));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(chosen))) = left.await else {
                return;
            };
            let Some(left) = chosen.into_iter().next() else {
                return;
            };
            let Ok(right) = this.update(cx, |_, cx| {
                cx.prompt_for_paths(speaker_options(words::settings_equaliser::right_speaker()))
            }) else {
                return;
            };
            let Ok(Ok(Some(chosen))) = right.await else {
                return;
            };
            let Some(right) = chosen.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.correct_the_room_by_a_pair(left, right, cx)
            });
        })
        .detach();
    }

    fn correct_the_room_by_a_pair(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let kept = cx
            .global::<ResonateApp>()
            .places
            .equaliser
            .with_file_name(BINAURAL_FILE);
        let assembling = kept.clone();
        let speakers = speakers_worn(cx);
        let assembled = cx.background_executor().spawn(async move {
            let whole = read_binaural_pair(
                &Sources::local(),
                &MediaLocation::local(&left),
                &MediaLocation::local(&right),
                speakers,
            )
            .map_err(|error| pair_refusal(&error))?;
            let wave = binaural_wave(&whole).map_err(|error| pair_refusal(&error))?;
            write_beside(&assembling, &wave).map_err(|error| {
                tracing::warn!(%error, "the assembled binaural response could not be kept");
                words::settings_equaliser::kept_not().to_owned()
            })
        });
        self.reading_the_room = cx.spawn(async move |this, cx| {
            let assembled = assembled.await;
            let _ = this.update(cx, |this, cx| match assembled {
                Ok(()) => this.correct_the_room(Some(kept), cx),
                Err(words) => this.report(Notice::Trouble(words), cx),
            });
        });
    }

    pub(crate) fn correct_the_room(&mut self, file: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(file) = file else {
            self.reading_the_room = Task::ready(());
            cx.update_global::<ResonateApp, _>(|global, _| global.convolution = None);
            self.send(Command::SetConvolution(None), cx);
            self.store(&Setting::Convolution(None), cx);
            cx.notify();
            return;
        };
        let reading = file.clone();
        let speakers = speakers_worn(cx);
        let read = cx.background_executor().spawn(async move {
            read_impulse(&Sources::local(), &MediaLocation::local(&reading), speakers)
        });
        self.reading_the_room = cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(Some(impulse)) => {
                    this.room_shape = Some((file.clone(), impulse.is_crossed()));
                    cx.update_global::<ResonateApp, _>(|global, _| {
                        global.convolution = Some(file.clone());
                    });
                    this.send(Command::SetConvolution(Some(Arc::new(impulse))), cx);
                    this.store(&Setting::Convolution(Some(file)), cx);
                    this.report(
                        Notice::Done(words::settings_equaliser::correcting().to_owned()),
                        cx,
                    );
                    cx.notify();
                }
                Ok(None) => this.report(
                    Notice::Trouble(words::settings_equaliser::no_taps().to_owned()),
                    cx,
                ),
                Err(error) => {
                    tracing::warn!(%error, "a room's response could not be read");
                    this.report(
                        Notice::Trouble(words::settings_equaliser::unread().to_owned()),
                        cx,
                    );
                }
            });
        });
    }
}

pub(crate) struct Fitting {
    watch: Option<Arc<Watch>>,
    _landing: Task<()>,
}

impl Fitting {
    pub(crate) fn idle() -> Self {
        Self {
            watch: None,
            _landing: Task::ready(()),
        }
    }

    fn measuring(watch: Arc<Watch>, landing: Task<()>) -> Self {
        Self {
            watch: Some(watch),
            _landing: landing,
        }
    }
}

impl Drop for Fitting {
    fn drop(&mut self) {
        if let Some(watch) = &self.watch {
            watch.stop();
        }
    }
}

struct Fitted {
    loudest: Loudest,
    room: Option<Preamp>,
}

fn speakers_worn(cx: &App) -> Speakers {
    let worn = cx.global::<ResonateApp>();
    Speakers {
        angle: worn.speaker_angle,
        elevation: worn.speaker_elevation,
        yaw: worn.head_yaw,
    }
}

fn room_correcting_the_fit(file: &Path, speakers: Speakers) -> Option<Arc<Impulse>> {
    match read_impulse(&Sources::local(), &MediaLocation::local(file), speakers) {
        Ok(impulse) => impulse.map(Arc::new),
        Err(error) => {
            tracing::warn!(%error, "the room's response could not be read for the preamp");
            None
        }
    }
}

fn fitted_through_the_chain(
    library: &Library,
    engine: &Player,
    applied: &Applied,
    rate: SampleRate,
    watch: &dyn Watching,
) -> Result<Fitted, AnalysisError> {
    let gains = BandGains::of(applied, rate)?;
    let cuts = engine.cuts_to_fit();
    let kept = library.loudest_kept(&gains).unwrap_or_else(|error| {
        tracing::warn!(%error, "the catalog's measured songs could not be read for the preamp");
        Loudest::default()
    });
    let owed = library.unmeasured(&cuts).unwrap_or_else(|error| {
        tracing::warn!(%error, "the catalog's measured songs could not be weighed against the queue");
        cuts
    });
    let decoded = engine.loudest_among(&owed, applied, watch)?;
    let room = applied
        .correction()
        .map(|_| applied.fitted_preamp(rate))
        .transpose()?;
    Ok(Fitted {
        loudest: kept.joined(decoded),
        room,
    })
}

fn fitted_to(songs: usize) -> String {
    words::settings_equaliser::preamp_fitted_to(songs)
}

fn speaker_options(label: &'static str) -> PathPromptOptions {
    PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(SharedString::new_static(label)),
    }
}

fn write_beside(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let staged = target.with_extension(format!("{}.new", std::process::id()));
    std::fs::write(&staged, bytes)?;
    std::fs::rename(&staged, target)
}

fn side(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::Left => "left",
        Speaker::Right => "right",
    }
}

fn pair_refusal(error: &PairedReadError) -> String {
    match error {
        PairedReadError::Read { speaker, .. } => {
            words::settings_equaliser::speaker_unread(side(*speaker))
        }
        PairedReadError::Empty { speaker } => {
            words::settings_equaliser::speaker_empty(side(*speaker))
        }
        PairedReadError::Pair(DspError::SpeakerNotStereo { speaker, .. }) => {
            words::settings_equaliser::speaker_not_stereo(side(*speaker))
        }
        PairedReadError::Pair(DspError::SpeakerAlreadyCrossed { speaker }) => {
            words::settings_equaliser::speaker_already_crossed(side(*speaker))
        }
        PairedReadError::Pair(DspError::SpeakerRatesDiffer { .. }) => {
            words::settings_equaliser::the_two_files_are_at_different().to_owned()
        }
        PairedReadError::Pair(DspError::SpeakerLengthsDiffer { .. }) => {
            words::settings_equaliser::the_two_files_are_of_different().to_owned()
        }
        PairedReadError::Pair(_) => {
            words::settings_equaliser::the_two_files_could_not_be().to_owned()
        }
    }
}

fn unplugged(bindings: &crate::Bindings, devices: &[(NodeName, String)]) -> Vec<NodeName> {
    bindings
        .by_sink
        .iter()
        .map(|(sink, _)| sink)
        .filter(|sink| !devices.iter().any(|(plugged, _)| plugged == *sink))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::{BandGain, Frequency, Q};

    use super::*;

    fn traced(on: TracedOn, decibels: &[f64]) -> Traced {
        Traced {
            on,
            decibels: decibels.to_vec(),
        }
    }

    #[test]
    fn a_device_bound_and_not_plugged_in_is_listed_and_a_plugged_one_is_not_listed_twice() {
        let headphones = NodeName::new("alsa_output.usb-headphones");
        let speakers = NodeName::new("alsa_output.pci-speakers");
        let bindings = crate::Bindings {
            enabled: true,
            fallback: Some(Binding::Own),
            by_sink: vec![
                (headphones.clone(), Binding::Own),
                (speakers.clone(), Binding::Own),
            ],
        };
        let plugged = [(speakers, "Speakers".to_owned())];

        assert_eq!(
            unplugged(&bindings, &plugged),
            std::slice::from_ref(&headphones)
        );
        assert!(
            unplugged(
                &bindings,
                &[(headphones, String::new()), plugged[0].clone()]
            )
            .is_empty()
        );
        assert!(unplugged(&crate::Bindings::default(), &plugged).is_empty());
    }

    #[test]
    fn a_curve_is_heard_as_its_range_and_every_band_it_is_shaped_by() {
        let low = Band::new(
            BandKind::LowShelf,
            Frequency::from_hertz(105.0).unwrap(),
            BandGain::from_decibels(4.5).unwrap(),
            Q::from_units(0.7).unwrap(),
        );
        let mid = Band::peaking(
            Frequency::from_hertz(2_500.0).unwrap(),
            BandGain::from_decibels(-3.0).unwrap(),
            Q::from_units(1.0).unwrap(),
        );
        let mut off = Band::peaking(
            Frequency::from_hertz(12_000.0).unwrap(),
            BandGain::from_decibels(2.0).unwrap(),
            Q::from_units(1.0).unwrap(),
        );
        off.on = false;

        assert_eq!(
            curve_said(&[low, mid, off], 6.0),
            "Gain against frequency, drawn within 6 dB either side of flat, shaped by 3 bands: \
             1, low shelf at 105 Hz, +4.5 dB; \
             2, peak at 2.5 kHz, -3.0 dB; \
             3, peak at 12 kHz, +2.0 dB, off"
        );
        assert_eq!(
            curve_said(&[], 6.0),
            "Gain against frequency, drawn within 6 dB either side of flat, shaped by 0 bands"
        );
    }

    #[test]
    fn a_curve_plot_is_an_image_whose_description_lists_the_bands() {
        crate::views::spoken::listened_to(true);

        let band = Band::peaking(
            Frequency::from_hertz(1_000.0).unwrap(),
            BandGain::from_decibels(3.0).unwrap(),
            Q::from_units(1.0).unwrap(),
        );
        let plot = crate::views::spoken::node_of(
            &div()
                .id("equaliser-curve")
                .pictured_as(words::settings_equaliser::the_curve(), || {
                    curve_said(&[band], 6.0)
                }),
        )
        .expect("a heard plot is a node");

        assert_eq!(plot.role(), gpui::Role::Image);
        assert_eq!(plot.label(), Some(words::settings_equaliser::the_curve()));
        assert!(
            plot.description()
                .is_some_and(|said| said.ends_with("1, peak at 1 kHz, +3.0 dB"))
        );
    }

    #[test]
    fn a_channel_cut_deeper_than_the_rest_widens_the_plot_and_each_channel_has_an_ink_of_its_own() {
        let _worn = crate::theme::alongside();

        let apart = [
            traced(TracedOn::Channel(0), &[0.0, 3.0]),
            traced(TracedOn::Channel(1), &[0.0, -18.0]),
            traced(TracedOn::EveryOtherChannel, &[0.0, 1.0]),
        ];

        assert_eq!(widest_drawn(&apart), 20.0);
        assert_ne!(ink_of(apart[0].on, 0), ink_of(apart[1].on, 1));
        assert_ne!(ink_of(apart[0].on, 0), theme::accent());
        assert_eq!(ink_of(apart[2].on, 2), theme::accent());
        assert_eq!(spoken_channel(apart[1].on), "Right");
    }
}
