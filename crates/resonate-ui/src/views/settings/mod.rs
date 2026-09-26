mod about;
mod appearance;
mod curve;
mod defaults;
mod desktop;
mod equaliser;
mod find;
mod library;
mod online;
mod output;
mod processing;

use std::sync::atomic::Ordering;

use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px,
    relative, rgb,
};
use resonate_core::Appearance;
use resonate_library::DEFAULT_LAYOUT;
use resonate_listen::{CLIP_BY_DEFAULT, Listening};

pub(crate) use crate::views::settings::{
    curve::{HeldBand, Plotted, across_at},
    equaliser::marked_frequencies,
    find::{Category, Group},
};
use crate::{
    AppIcon, Notice, ResonateApp, Setting, SettingKey,
    app::CONTROL_CONTEXT,
    icons::{self, Icon},
    theme,
    views::{
        hint::{self, Names},
        kit::{self, Press, Tone},
        root::{RootView, SETTING_UNSAVED},
        scrollbar::Scrollbars,
        settings::{
            defaults::{Standing, can_be_put_back, differs, puts_back},
            find::Narrowing,
        },
    },
};

pub(crate) const FILTER_PLACEHOLDER: &str = "Find a setting…";

const REVERT_HINT: &str = "Put this back to what Resonate was built with, and take the key out of \
                           the settings file";

const EXPERIMENTAL: &str = "EXPERIMENTAL";

const RESET_HINT: &str = "Put every setting in this category back to what Resonate was built with";

const NOTHING_FOUND: &str = "No setting answers to that.";

const NOTHING_FOUND_MORE: &str = "A setting is found by its name, by the words its hint uses, or \
                                  by what other players call it.";

trait Choice: Copy + PartialEq + 'static {
    const ALL: &'static [Self];

    fn label(self) -> &'static str;

    fn detail(self) -> Option<SharedString> {
        None
    }

    fn meaning(self) -> SharedString;
}

impl RootView {
    pub(crate) fn settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.controls.opening();

        let narrowing = Narrowing::of(self.finding.read(cx).text());
        let category = self.settings_category;
        let standing = self.standing(cx);
        let rail = self.categories(&narrowing, cx);
        let body = self.shown(&narrowing, category, &standing, cx);
        let footer = (!narrowing.narrows() && category.groups().any(can_be_put_back))
            .then(|| self.reset(category, cx));

        let pane = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(self.settings_heading(&narrowing, category, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w(theme::width(theme::settings_rail()))
                            .min_h(px(0.0))
                            .child(rail)
                            .child(Scrollbars::of(cx).vertical(
                                "settings-categories-scrollbar",
                                self.settings_rail_scroll.clone(),
                            )),
                    )
                    .child(Scrollbars::of(cx).around(
                        "settings-scrollbar",
                        self.settings_scroll.clone(),
                        body,
                    )),
            )
            .when_some(footer, Div::child)
            .into_any_element();

        self.controls.forget_what_has_gone();
        pane
    }

    fn settings_heading(
        &self,
        narrowing: &Narrowing,
        category: Category,
        cx: &mut Context<Self>,
    ) -> Div {
        let found = narrowing.anywhere().count();
        let (title, about) = if narrowing.narrows() {
            (
                SharedString::new_static("Found"),
                SharedString::from(match found {
                    0 => "No setting anywhere answers to that".to_owned(),
                    1 => "One setting, wherever it is kept".to_owned(),
                    many => format!("{many} settings, wherever they are kept"),
                }),
            )
        } else {
            (
                SharedString::new_static(category.label()),
                SharedString::new_static(category.about()),
            )
        };

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(theme::width(theme::heading_name()))
                        .gap_1()
                        .child(kit::eyebrow("SETTINGS"))
                        .child(kit::title(title))
                        .child(kit::subtitle(about)),
                )
                .child(kit::actions().child(self.filter_field(cx))),
        )
    }

    fn filter_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let typed = !self.finding.read(cx).text().is_empty();

        div()
            .id("find-a-setting")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .w(theme::width(theme::picker_width()))
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.border_color(theme::tinted(theme::accent(), 0x99)))
            .child(icons::icon(
                Icon::Search,
                theme::search_icon(),
                theme::faint(),
            ))
            .child(self.finding.clone())
            .when_else(
                typed,
                |field| field.child(self.clear_filter(cx)),
                |field| field.child(kit::figure("ctrl-,").text_color(rgb(theme::faint()))),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.finding
                    .update(cx, |finding, _| finding.take_focus(window));
                cx.notify();
            }))
    }

    fn clear_filter(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("clear-the-filter")
            .group("clear-filter")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(theme::width(theme::filter_clear_control()))
            .rounded_full()
            .cursor_pointer()
            .hover(|mark| mark.bg(rgb(theme::hover())))
            .names("Clear what is typed")
            .on_click(cx.listener(|this, _, window, cx| {
                this.finding.update(cx, |finding, cx| finding.clear(cx));
                this.finding
                    .update(cx, |finding, _| finding.take_focus(window));
                cx.notify();
            }))
            .child(icons::lit_on_hover(
                icons::icon(Icon::Close, theme::filter_clear_mark(), theme::muted()),
                "clear-filter",
            ))
    }

    fn categories(&self, narrowing: &Narrowing, cx: &mut Context<Self>) -> Stateful<Div> {
        let chosen = self.settings_category;
        let mut rail = div()
            .id("settings-categories")
            .flex()
            .flex_col()
            .flex_none()
            .gap_0p5()
            .p_2()
            .pr_4()
            .w(theme::width(theme::settings_rail()))
            .bg(rgb(theme::surface()))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .overflow_y_scroll()
            .track_scroll(&self.settings_rail_scroll)
            .h_full();

        for category in Category::ALL {
            let selected = !narrowing.narrows() && category == chosen;
            let found = narrowing.narrows().then(|| narrowing.found(category));
            let ink = if selected {
                theme::text()
            } else {
                theme::muted()
            };

            rail = rail.child(
                self.in_the_ring(
                    category.label(),
                    div()
                        .id(SharedString::new_static(category.label()))
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .text_size(px(theme::text_xs()))
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .text_color(rgb(ink))
                        .when_else(
                            selected,
                            |row| row.bg(rgb(theme::hover())).font_weight(FontWeight::MEDIUM),
                            |row| row.hover(|row| row.bg(rgb(theme::hover()))),
                        )
                        .names(category.about())
                        .child(icons::icon(category.icon(), theme::root_icon(), ink))
                        .child(div().flex_1().min_w(px(0.0)).child(category.label()))
                        .when_some(found, |row, found| {
                            row.child(kit::figure(found.to_string()).text_color(rgb(
                                if found == 0 {
                                    theme::faint()
                                } else {
                                    theme::accent()
                                },
                            )))
                        }),
                    move |this, _, cx| this.show_settings(category, cx),
                    cx,
                ),
            );
        }

        rail
    }

    fn shown(
        &mut self,
        narrowing: &Narrowing,
        category: Category,
        standing: &Standing,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut column = div().flex().flex_col().flex_none().gap_4();

        if narrowing.narrows() {
            let mut last = None;
            for group in narrowing.anywhere() {
                if last != Some(group.category()) {
                    last = Some(group.category());
                    column = column.child(kit::eyebrow(group.category().label().to_uppercase()));
                }
                column = column.child(self.group(group, standing, cx));
            }
            if last.is_none() {
                column = column.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .py_8()
                        .child(
                            div()
                                .text_size(px(theme::text_base()))
                                .text_color(rgb(theme::muted()))
                                .child(NOTHING_FOUND),
                        )
                        .child(
                            div()
                                .text_size(px(theme::text_sm()))
                                .text_color(rgb(theme::faint()))
                                .child(NOTHING_FOUND_MORE),
                        ),
                );
            }
        } else {
            for group in category.groups() {
                column = column.child(self.group(group, standing, cx));
            }
        }

        div()
            .id("settings")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.holds_a_press_armed() {
                    this.lower_every_armed_press();
                    cx.notify();
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .px_5()
            .py_4()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(theme::width(theme::settings_column()))
                    .max_w_full()
                    .child(column),
            )
    }

    fn group(&mut self, group: Group, standing: &Standing, cx: &mut Context<Self>) -> Div {
        let body = match group {
            Group::Device => self.device_group(cx),
            Group::SampleRate => self.sample_rate_group(cx),
            Group::GraphRate => self.graph_rate_group(cx),
            Group::Buffer => self.buffer_group(cx),
            Group::Dop => self.dop_group(cx),
            Group::Bluetooth => self.bluetooth_group(cx),
            Group::Resampler => self.resampler_group(cx),
            Group::Dither => self.dither_group(cx),
            Group::NoiseShaping => self.noise_shaping_group(cx),
            Group::ReplayGain => self.replay_gain_group(cx),
            Group::TruePeak => self.true_peak_group(cx),
            Group::LossySources => self.lossy_sources_group(cx),
            Group::Equalising => self.equalising_group(cx),
            Group::BoundTo => self.bound_to_group(cx),
            Group::Bands => self.bands_group(cx),
            Group::Measured => self.measured_group(cx),
            Group::Folders => self.folders_group(cx),
            Group::Scanning => self.scanning_group(cx),
            Group::Refreshing => self.refreshing_group(cx),
            Group::Tagging => self.tagging_group(cx),
            Group::Organising => self.organising_group(cx),
            Group::Vault => self.vault_group(cx),
            Group::Inbox => self.inbox_group(cx),
            Group::Resuming => self.resuming_group(cx),
            Group::Repeating => self.repeating_group(cx),
            Group::PreviousButton => self.previous_group(cx),
            Group::Lookups => self.lookups_group(cx),
            Group::AfterScan => self.after_scan_group(cx),
            Group::Studies => self.studies_group(cx),
            Group::Contact => self.contact_group(cx),
            Group::Recognition => self.recognition_group(cx),
            Group::Listening => self.listening_group(cx),
            Group::Submitting => self.submitting_group(cx),
            Group::LookUpNow => self.look_up_group(cx),
            Group::Notifications => self.notifications_group(cx),
            Group::Discord => self.discord_group(cx),
            Group::DiscordShows => self.discord_shows_group(cx),
            Group::Colour => self.colour_group(cx),
            Group::Layout => self.layout_group(cx),
            Group::WindowButtons => self.window_buttons_group(cx),
            Group::VolumeWheel => self.volume_wheel_group(cx),
            Group::Scrollbars => self.scrollbars_group(cx),
            Group::Tabs => self.tabs_group(cx),
            Group::WindowState => self.window_state_group(cx),
            Group::Build => self.build_group(cx),
            Group::Places => self.places_group(cx),
            Group::Everything => self.everything_group(cx),
        };

        self.grouped(group, standing, body, cx)
    }

    fn grouped(&self, group: Group, standing: &Standing, body: Div, cx: &mut Context<Self>) -> Div {
        let moved = can_be_put_back(group) && differs(group, standing);

        kit::section()
            .child(
                kit::section_header()
                    .child(kit::section_name(group.title()))
                    .when(group.is_experimental(), |header| {
                        header.child(kit::badge(EXPERIMENTAL, theme::lossy()))
                    })
                    .when(moved, |header| header.child(self.revert(group, cx)))
                    .child(hint::explains(group.title(), group.hint())),
            )
            .child(body)
    }

    fn revert(&self, group: Group, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_ring(
            group.title(),
            kit::icon_button(
                SharedString::from(format!("revert-{}", group.title())),
                Icon::Undo,
                REVERT_HINT,
            ),
            move |this, _, cx| this.put_back(group, cx),
            cx,
        )
    }

    fn reset(&self, category: Category, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap_2()
            .px_5()
            .py_3()
            .bg(rgb(theme::surface()))
            .border_t_1()
            .border_color(rgb(theme::border()))
            .child(self.in_the_ring(
                "reset-the-category",
                kit::button(
                    "reset-the-category",
                    Some(Icon::Undo),
                    format!("Reset {}", category.label().to_lowercase()),
                    RESET_HINT,
                    Tone::Ghost,
                ),
                move |this, _, cx| {
                    for group in category.groups().filter(|group| can_be_put_back(*group)) {
                        this.put_back(group, cx);
                    }
                },
                cx,
            ))
    }

    pub(crate) fn put_back(&mut self, group: Group, cx: &mut Context<Self>) {
        for command in puts_back(group) {
            self.send(command, cx);
        }

        match group {
            Group::Colour => self.dress(
                Appearance {
                    theme: Appearance::DEFAULT.theme,
                    accent: Appearance::DEFAULT.accent,
                    ..theme::worn()
                },
                cx,
            ),
            Group::Layout => self.dress(
                Appearance {
                    text_size: Appearance::DEFAULT.text_size,
                    ..theme::worn()
                },
                cx,
            ),
            Group::Equalising => self.switch_the_equaliser(false, cx),
            Group::BoundTo => self.unbind_every_device(cx),
            Group::Lookups => self.set_online(defaults::ONLINE, cx),
            Group::AfterScan => self.set_after_scan(defaults::ENRICH_AFTER_SCAN, cx),
            Group::Studies => self.set_studies(defaults::STUDY, cx),
            Group::Resuming => self.set_resume(defaults::RESUME, cx),
            Group::Notifications => self.set_notify(defaults::NOTIFY, cx),
            Group::Discord => self.put_discord_back(cx),
            Group::DiscordShows => self.put_discord_shows_back(cx),
            Group::Contact => self.clear_contact(cx),
            Group::Recognition => {
                self.clear_acoustid_key(cx);
                self.clear_audd_token(cx);
            }
            Group::Submitting => self.clear_listenbrainz_token(cx),
            Group::Listening => {
                self.listen.update(cx, |listen, cx| {
                    listen.choose(Listening::Desktop, cx);
                    listen.listen_for(CLIP_BY_DEFAULT, cx);
                });
            }
            Group::Inbox => self
                .library
                .update(cx, |library, cx| library.set_inbox(None, cx)),
            Group::Organising => self.set_organise_as(DEFAULT_LAYOUT.to_owned(), cx),
            Group::WindowButtons => self.show_window_buttons(defaults::WINDOW_BUTTONS, cx),
            Group::VolumeWheel => self.wheel_the_volume(defaults::SCROLL_VOLUME, cx),
            Group::Scrollbars => self.draw_scrollbars(defaults::SCROLLBARS, cx),
            Group::Tabs => self.show_tabs(defaults::TABS, cx),
            Group::WindowState => self.put_window_state_back(cx),
            _ => {}
        }

        for key in group.keys() {
            self.forget(*key, cx);
        }
        cx.notify();
    }

    fn standing(&self, cx: &mut Context<Self>) -> Standing {
        let library = self.library.read(cx);
        let online = library.is_online();
        let after_scan = library.enriches_after_scan();
        let studies = library.studies();
        let resume = library.resumes();
        let skip_under_repeat = self.player.read(cx).state().skip_under_repeat;
        let previous_restarts = self.player.read(cx).state().previous_restarts;
        let notify = cx.global::<ResonateApp>().notify.load(Ordering::Acquire);
        let window_buttons = cx.global::<ResonateApp>().window_buttons;
        let scroll_volume = cx.global::<ResonateApp>().scroll_volume;
        let scrollbars = cx.global::<ResonateApp>().scrollbars;
        let tabs = cx.global::<ResonateApp>().tabs;
        let remember_tab = cx.global::<ResonateApp>().remember_tab;
        let remember_window_size = cx.global::<ResonateApp>().remember_window_size;
        let remember_settings_category = cx.global::<ResonateApp>().remember_settings_category;
        let presence = cx.global::<ResonateApp>().presence.clone();
        let contact_given = !self.contact.read(cx).text().trim().is_empty();
        let key_given = !self.acoustid.read(cx).text().trim().is_empty();
        let token_given = !self.audd.read(cx).text().trim().is_empty();
        let submitting_given = !self.listenbrainz.read(cx).text().trim().is_empty();
        let listen = self.listen.read(cx);
        let listening_from = listen.from().clone();
        let listening_for = listen.length();
        let template_given = self.organising.read(cx).text().trim() != DEFAULT_LAYOUT;
        let inbox_given = self.library.read(cx).inbox().is_some();

        Standing {
            output: self.player.read(cx).output_settings().clone(),
            appearance: theme::worn(),
            online,
            after_scan,
            studies,
            contact_given,
            key_given,
            token_given,
            submitting_given,
            listening_from,
            listening_for,
            resume,
            skip_under_repeat,
            previous_restarts,
            notify,
            window_buttons,
            scroll_volume,
            scrollbars,
            tabs,
            remember_tab,
            remember_window_size,
            remember_settings_category,
            presence,
            template_given,
            inbox_given,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.lower_every_armed_press();
        self.reset_everything_landed = false;
    }

    fn holds_a_press_armed(&self) -> bool {
        self.resetting_everything
            || self.moving_the_files
            || self.writing_the_tags
            || self.keeping_the_tracks
    }

    fn lower_every_armed_press(&mut self) {
        self.resetting_everything = false;
        self.moving_the_files = false;
        self.writing_the_tags = false;
        self.keeping_the_tracks = false;
    }

    pub(crate) fn in_the_ring_at(
        &self,
        named: impl Into<String>,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let named = named.into();
        let clicked = press.clone();

        control
            .track_focus(&self.controls.at(&named, cx))
            .tab_stop(true)
            .key_context(CONTROL_CONTEXT)
            .focus(|control| {
                control
                    .border_color(rgb(theme::accent()))
                    .bg(theme::tinted(theme::accent(), 0x14))
            })
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, window, cx| {
                    press(this, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| clicked(this, window, cx)))
    }

    pub(crate) fn in_the_ring(
        &self,
        named: &'static str,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let clicked = press.clone();

        control
            .track_focus(&self.controls.at(named, cx))
            .tab_stop(true)
            .key_context(CONTROL_CONTEXT)
            .focus(|control| {
                control
                    .border_color(rgb(theme::accent()))
                    .bg(theme::tinted(theme::accent(), 0x14))
            })
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, window, cx| {
                    press(this, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| clicked(this, window, cx)))
    }

    fn choices<T: Choice>(
        &self,
        id: &'static str,
        chosen: Option<T>,
        cx: &mut Context<Self>,
        taken: impl Fn(&mut Self, T, &mut Context<Self>) + Copy + 'static,
    ) -> Div {
        let mut row = kit::segmented();

        for (index, value) in T::ALL.iter().enumerate() {
            let value = *value;
            row = row.child(
                self.option_in_the_ring(
                    kit::segment((id, index), value.label(), chosen == Some(value))
                        .when_some(value.detail(), |option, detail| option.names(detail)),
                    id,
                    index,
                    move |this, _, cx| taken(this, value, cx),
                    cx,
                ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().child(row))
            .when_some(chosen, |column, chosen| {
                column.child(note(chosen.meaning()))
            })
    }

    fn option_in_the_ring(
        &self,
        control: Stateful<Div>,
        id: &'static str,
        index: usize,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let clicked = press.clone();

        control
            .track_focus(&self.controls.at(format!("{id}-{index}"), cx))
            .tab_stop(true)
            .key_context(CONTROL_CONTEXT)
            .focus(|option| option.bg(theme::tinted(theme::accent(), 0x2a)))
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, window, cx| {
                    press(this, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| clicked(this, window, cx)))
    }

    fn forget(&self, key: SettingKey, cx: &mut Context<Self>) {
        let Err(error) = self.settings.forget(key) else {
            return;
        };
        tracing::error!(%error, ?key, "a setting could not be taken out of the file");

        self.report(Notice::Trouble(SETTING_UNSAVED.to_owned()), cx);
    }

    pub(crate) fn dress(&self, dressed: Appearance, cx: &mut Context<Self>) {
        theme::wear(dressed);
        cx.global::<ResonateApp>()
            .launcher
            .show(AppIcon::of(dressed));
        cx.refresh_windows();
    }

    pub(crate) fn wear(&self, dressed: Appearance, setting: &Setting, cx: &mut Context<Self>) {
        self.dress(dressed, cx);
        self.store(setting, cx);
    }
}

fn switch_row(
    label: impl Into<SharedString>,
    under: impl Into<SharedString>,
    on: bool,
    id: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .cursor_pointer()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
        .hover(|row| row.bg(rgb(theme::hover())))
        .child(named(label, under))
        .child(kit::switch(SharedString::from(format!("{id}-track")), on))
}

fn rows() -> Div {
    div().flex().flex_col().gap_1()
}

fn named(title: impl Into<SharedString>, under: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .child(
            div()
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(theme::text_xs()))
                .text_color(rgb(theme::faint()))
                .truncate()
                .child(under.into()),
        )
}

fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::faint()))
        .child(text.into())
}

fn hugging(control: Stateful<Div>) -> Div {
    div().flex().child(control)
}

fn reading(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .flex_none()
                .w(theme::width(theme::field_label()))
                .text_size(px(theme::text_xs()))
                .text_color(rgb(theme::muted()))
                .child(label.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .font_family(theme::mono_face())
                .text_size(px(theme::text_xs()))
                .text_color(rgb(theme::text()))
                .child(value.into()),
        )
}

fn progress(share: f32) -> Div {
    div()
        .w_full()
        .h(theme::width(theme::SCAN_BAR))
        .rounded_full()
        .overflow_hidden()
        .bg(rgb(theme::outline()))
        .child(
            div()
                .h_full()
                .w(relative(share.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(rgb(theme::accent())),
        )
}

fn action(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: Icon,
    busy: bool,
    press: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + Clone + 'static,
    this: &RootView,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let label = label.into();

    if busy {
        return kit::button_when(
            Press::Greyed,
            id,
            Some(icon),
            label.clone(),
            label,
            Tone::Outlined,
        );
    }

    this.in_the_ring(
        id,
        kit::button(id, Some(icon), label.clone(), label, Tone::Outlined),
        press,
        cx,
    )
}
