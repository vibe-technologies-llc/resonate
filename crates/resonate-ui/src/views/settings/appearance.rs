use gpui::{Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px, rgb};
use resonate_core::{
    Accent, Appearance, Following, ScrollbarMode, Spectral, SpectrumBands, SpectrumFalls,
    SpectrumFloor, SpectrumTilt, Switch, TextSize, Theme,
};
use resonate_words::LanguageChoice;

use crate::{
    ConfigKey, ResonateApp, Setting, Tabs, WindowButtons, WindowSize, theme,
    views::{
        hint::Names,
        kit::{self, EndsInAnEllipsis as _},
        root::{Pane, RootView},
        settings::{Choice, Rows, find::Category, note, switch_row},
        spoken::Spoken,
    },
    words,
};

const ITS_OWN_ID: &str = "accent-of-the-palette";

const FOLLOW_SCHEME_ID: &str = "follow-the-desktop-scheme";

const FOLLOW_TEXT_SIZE_ID: &str = "follow-the-desktop-text-size";

const MINIMISE_ID: &str = "show-the-minimise-button";

const MAXIMISE_ID: &str = "show-the-maximise-button";

const VOLUME_WHEEL_ID: &str = "wheel-the-volume";
const MOUSE_NAVIGATION_ID: &str = "mouse-navigation";
const REDUCED_MOTION_ID: &str = "reduce-motion";
const SUGGESTIONS_TAB_ID: &str = "show-the-suggestions-tab";

const MISSING_TAB_ID: &str = "show-the-missing-tab";

const TAB_COUNTS_ID: &str = "show-the-tab-counts";

const REMEMBER_TAB_ID: &str = "remember-the-last-tab";

const REMEMBER_WINDOW_SIZE_ID: &str = "remember-the-window-size";

const REMEMBER_SETTINGS_CATEGORY_ID: &str = "remember-the-settings-category";

impl Choice for TextSize {
    const ALL: &'static [Self] = &TextSize::ALL;

    fn label(self) -> &'static str {
        TextSize::label(self)
    }

    fn meaning(self) -> SharedString {
        SharedString::from(words::settings_appearance::text_size_meaning(format!(
            "{:.0}",
            self.scale() * 100.0
        )))
    }
}

impl Choice for LanguageChoice {
    const ALL: &'static [Self] = &LanguageChoice::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::System => words::settings_appearance::follow_the_system(),
            Self::Chosen(language) => language.endonym(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::System => words::settings_appearance::the_first_language_the_system_lists(),
            Self::Chosen(_) => {
                words::settings_appearance::spoken_whatever_language_the_system_lists()
            }
        })
    }
}

impl Choice for ScrollbarMode {
    const ALL: &'static [Self] = &[Self::Shown, Self::AutoHidden, Self::Hidden];

    fn label(self) -> &'static str {
        match self {
            Self::Shown => words::settings_appearance::always(),
            Self::AutoHidden => words::settings_appearance::while_scrolling(),
            Self::Hidden => words::settings_appearance::never(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Shown => words::settings_appearance::every_list_and_pane_that_scrolls(),
            Self::AutoHidden => words::settings_appearance::a_bar_is_drawn_while_its(),
            Self::Hidden => words::settings_appearance::no_bar_is_drawn_a_list(),
        })
    }
}

impl Choice for SpectrumTilt {
    const ALL: &'static [Self] = &[Self::Flat, Self::Pink, Self::Steep];

    fn label(self) -> &'static str {
        match self {
            Self::Flat => words::settings_appearance::flat(),
            Self::Pink => words::settings_appearance::n_3_db_an_octave(),
            Self::Steep => words::settings_appearance::n_4_5_db_an_octave(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Flat => words::settings_appearance::each_band_is_drawn_at_the(),
            Self::Pink => words::settings_appearance::the_treble_is_lifted_3_db(),
            Self::Steep => words::settings_appearance::the_treble_is_lifted_4_5(),
        })
    }
}

impl Choice for SpectrumFloor {
    const ALL: &'static [Self] = &[Self::Shallow, Self::Middling, Self::Deep];

    fn label(self) -> &'static str {
        match self {
            Self::Shallow => words::settings_appearance::n_60_db(),
            Self::Middling => words::settings_appearance::n_78_db(),
            Self::Deep => words::settings_appearance::n_96_db(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::from(words::settings_appearance::floor_meaning(
            self.db().to_string(),
        ))
    }
}

impl Choice for SpectrumBands {
    const ALL: &'static [Self] = &[Self::Thirds, Self::Sixths, Self::Twelfths];

    fn label(self) -> &'static str {
        match self {
            Self::Thirds => words::settings_appearance::thirds(),
            Self::Sixths => words::settings_appearance::sixths(),
            Self::Twelfths => words::settings_appearance::twelfths(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::from(words::settings_appearance::bands_meaning(
            self.per_octave().to_string(),
        ))
    }
}

impl Choice for SpectrumFalls {
    const ALL: &'static [Self] = &[Self::Slowly, Self::Middling, Self::Quickly];

    fn label(self) -> &'static str {
        match self {
            Self::Slowly => words::settings_appearance::slowly(),
            Self::Middling => words::settings_appearance::middling(),
            Self::Quickly => words::settings_appearance::quickly(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::from(words::settings_appearance::falls_meaning(
            self.db_per_second().to_string(),
        ))
    }
}

impl RootView {
    pub(super) fn colour_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let worn = theme::worn();
        let following = theme::chosen().follows.scheme;
        let mut shelf = div().flex().flex_wrap().gap_2();
        for palette in Theme::ALL {
            shelf = shelf.child(self.palette(palette, worn, cx));
        }

        let mut accents = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(self.accent(None, worn, cx));
        for accent in Accent::ALL {
            accents = accents.child(self.accent(Some(accent), worn, cx));
        }

        kit::rows()
            .child(self.in_the_ring(
                FOLLOW_SCHEME_ID,
                switch_row(
                    words::settings_appearance::follow_the_desktops_light_or_dark(),
                    words::settings_appearance::wear_the_light_or_dark_counterpart(),
                    following,
                    FOLLOW_SCHEME_ID,
                ),
                move |this, _, cx| this.follow_the_desktop_scheme(!following, cx),
                cx,
            ))
            .child(kit::field(words::settings_appearance::theme(), shelf))
            .child(kit::field(
                words::settings_appearance::accent(),
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(accents)
                    .child(note(words::settings_appearance::keeps_its_own())),
            ))
            .note(words::settings_appearance::follow_scheme_note())
    }

    fn palette(&self, palette: Theme, worn: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let chosen = worn.theme == palette;
        let dressed = Appearance {
            theme: palette,
            ..worn
        };
        let kept = theme::chosen();
        let wearing = Appearance {
            theme: palette,
            follows: Following {
                scheme: false,
                ..kept.follows
            },
            ..kept
        };

        self.in_the_ring(
            palette.label(),
            div()
                .id(SharedString::new_static(palette.as_str()))
                .chosen_among(palette.label(), String::new, chosen)
                .flex()
                .flex_col()
                .flex_none()
                .gap_1p5()
                .w(theme::width(theme::theme_swatch()))
                .p_1p5()
                .rounded_lg()
                .cursor_pointer()
                .border_1()
                .when_else(
                    chosen,
                    |card| card.border_color(rgb(theme::accent())),
                    |card| {
                        card.border_color(rgb(theme::border()))
                            .hover(|card| card.bg(rgb(theme::hover())))
                    },
                )
                .child(kit::preview(dressed))
                .child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .truncate()
                        .ends_in_an_ellipsis()
                        .when_else(
                            chosen,
                            |name| {
                                name.text_color(rgb(theme::text()))
                                    .font_weight(FontWeight::MEDIUM)
                            },
                            |name| name.text_color(rgb(theme::muted())),
                        )
                        .child(palette.label()),
                ),
            move |this, _, cx| {
                this.wear(wearing, &Setting::Theme(palette), cx);
                this.store(&Setting::FollowDesktopScheme(false), cx);
            },
            cx,
        )
    }

    fn accent(
        &self,
        accent: Option<Accent>,
        worn: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let shown = Appearance { accent, ..worn };
        let dressed = Appearance {
            accent,
            ..theme::chosen()
        };
        let colour = theme::sample(shown).accent;
        let id = accent.map_or(ITS_OWN_ID, Accent::as_str);
        let saying = accent.map_or(words::settings_appearance::its_own(), Accent::label);

        self.in_the_ring(
            saying,
            kit::dot_swatch(id, saying, colour, worn.accent == accent)
                .when(accent.is_none(), |swatch| {
                    swatch.border_1().border_color(rgb(theme::soft_edge()))
                })
                .names(saying),
            move |this, _, cx| this.wear(dressed, &Setting::Accent(accent), cx),
            cx,
        )
    }

    pub(super) fn layout_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let worn = theme::worn();
        let following = theme::chosen().follows.text;

        kit::rows()
            .child(self.in_the_ring(
                FOLLOW_TEXT_SIZE_ID,
                switch_row(
                    words::settings_appearance::follow_the_desktops_text_size(),
                    words::settings_appearance::draw_the_text_at_the_size(),
                    following,
                    FOLLOW_TEXT_SIZE_ID,
                ),
                move |this, _, cx| this.follow_the_desktop_text_size(!following, cx),
                cx,
            ))
            .child(kit::field(
                words::settings_appearance::text_size(),
                self.choices(
                    "text-size",
                    worn.text_size,
                    cx,
                    |this, size: TextSize, cx| {
                        let kept = theme::chosen();
                        this.wear(
                            Appearance {
                                text_size: size,
                                follows: Following {
                                    text: false,
                                    ..kept.follows
                                },
                                ..kept
                            },
                            &Setting::TextSize(size),
                            cx,
                        );
                        this.store(&Setting::FollowDesktopTextSize(false), cx);
                    },
                ),
            ))
            .note(words::settings_appearance::follow_text_size_note())
    }

    pub(crate) fn follow_the_desktop_scheme(&mut self, follows: bool, cx: &mut Context<Self>) {
        let worn = theme::worn();
        let kept = theme::chosen();
        let dressed = Appearance {
            theme: worn.theme,
            follows: Following {
                scheme: follows,
                ..kept.follows
            },
            ..kept
        };

        self.wear(dressed, &Setting::Theme(worn.theme), cx);
        self.store(&Setting::FollowDesktopScheme(follows), cx);
    }

    pub(crate) fn follow_the_desktop_text_size(&mut self, follows: bool, cx: &mut Context<Self>) {
        let worn = theme::worn();
        let kept = theme::chosen();
        let dressed = Appearance {
            text_size: worn.text_size,
            follows: Following {
                text: follows,
                ..kept.follows
            },
            ..kept
        };

        self.wear(dressed, &Setting::TextSize(worn.text_size), cx);
        self.store(&Setting::FollowDesktopTextSize(follows), cx);
    }

    pub(super) fn motion_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let reduced = cx.global::<ResonateApp>().reduced_motion;

        kit::rows().child(self.in_the_ring(
            REDUCED_MOTION_ID,
            switch_row(
                words::settings_appearance::reduce_motion(),
                words::settings_appearance::things_appear_at_once_instead_of(),
                reduced,
                REDUCED_MOTION_ID,
            ),
            move |this, _, cx| {
                this.reduce_motion(!reduced, cx);
                this.store(&Setting::ReducedMotion(!reduced), cx);
            },
            cx,
        ))
    }

    pub(super) fn window_buttons_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let shown = cx.global::<ResonateApp>().window_buttons;
        let flip_minimise = WindowButtons {
            minimise: !shown.minimise,
            ..shown
        };
        let flip_maximise = WindowButtons {
            maximise: !shown.maximise,
            ..shown
        };

        kit::rows()
            .child(self.in_the_ring(
                MINIMISE_ID,
                switch_row(
                    words::settings_appearance::show_the_minimise_button(),
                    words::settings_appearance::off_the_window_is_put_away(),
                    shown.minimise,
                    MINIMISE_ID,
                ),
                move |this, _, cx| {
                    this.show_window_buttons(flip_minimise, cx);
                    this.store(&Setting::MinimiseButton(flip_minimise.minimise), cx);
                },
                cx,
            ))
            .child(self.in_the_ring(
                MAXIMISE_ID,
                switch_row(
                    words::settings_appearance::show_the_maximise_button(),
                    words::settings_appearance::off_a_double_press_on_the(),
                    shown.maximise,
                    MAXIMISE_ID,
                ),
                move |this, _, cx| {
                    this.show_window_buttons(flip_maximise, cx);
                    this.store(&Setting::MaximiseButton(flip_maximise.maximise), cx);
                },
                cx,
            ))
            .note(words::settings_appearance::window_buttons_note())
    }

    pub(super) fn volume_wheel_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let wheeled = cx.global::<ResonateApp>().scroll_volume;

        kit::rows().child(self.in_the_ring(
            VOLUME_WHEEL_ID,
            switch_row(
                words::settings_appearance::turn_the_volume_with_the_wheel(),
                words::settings_appearance::over_the_volume_slider_each_notch(),
                wheeled,
                VOLUME_WHEEL_ID,
            ),
            move |this, _, cx| {
                this.wheel_the_volume(!wheeled, cx);
                this.store(&Setting::Switched(Switch::ScrollVolume, !wheeled), cx);
            },
            cx,
        ))
    }

    pub(super) fn scrollbars_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let mode = cx.global::<ResonateApp>().scrollbars;

        kit::rows().child(kit::field(
            words::settings_appearance::draw_scrollbars(),
            self.choices("scrollbars", mode, cx, |this, mode: ScrollbarMode, cx| {
                this.draw_scrollbars(mode, cx);
                this.store(&Setting::Scrollbars(mode), cx);
            }),
        ))
    }

    pub(super) fn language_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let chosen = cx.global::<ResonateApp>().language;

        kit::rows()
            .child(kit::field(
                words::settings_appearance::speak(),
                self.choices(
                    "language",
                    chosen,
                    cx,
                    |this, chosen: LanguageChoice, cx| {
                        this.speak(chosen, cx);
                        this.store(&Setting::Language(chosen), cx);
                    },
                ),
            ))
            .note(words::settings_appearance::language_note())
    }

    pub(super) fn mouse_navigation_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let enabled = cx.global::<ResonateApp>().mouse_navigation;

        kit::rows().child(self.in_the_ring(
            MOUSE_NAVIGATION_ID,
            switch_row(
                words::settings_appearance::use_the_mouse_back_and_forward(),
                words::settings_appearance::move_through_the_album_and_artist(),
                enabled,
                MOUSE_NAVIGATION_ID,
            ),
            move |this, _, cx| {
                this.navigate_with_mouse_buttons(!enabled, cx);
                this.store(&Setting::Switched(Switch::MouseNavigation, !enabled), cx);
            },
            cx,
        ))
    }

    pub(super) fn tabs_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let shown = cx.global::<ResonateApp>().tabs;
        let flip_suggestions = Tabs {
            suggestions: !shown.suggestions,
            ..shown
        };
        let flip_missing = Tabs {
            missing: !shown.missing,
            ..shown
        };
        let flip_counts = Tabs {
            counts: !shown.counts,
            ..shown
        };

        kit::rows()
            .child(self.in_the_ring(
                SUGGESTIONS_TAB_ID,
                switch_row(
                    words::settings_appearance::show_suggestions(),
                    words::settings_appearance::the_lists_the_catalog_offers_to(),
                    shown.suggestions,
                    SUGGESTIONS_TAB_ID,
                ),
                move |this, _, cx| {
                    this.show_tabs(flip_suggestions, cx);
                    this.store(&Setting::SuggestionsTab(flip_suggestions.suggestions), cx);
                },
                cx,
            ))
            .child(self.in_the_ring(
                MISSING_TAB_ID,
                switch_row(
                    words::settings_appearance::show_missing(),
                    words::settings_appearance::what_the_releases_are_short_of(),
                    shown.missing,
                    MISSING_TAB_ID,
                ),
                move |this, _, cx| {
                    this.show_tabs(flip_missing, cx);
                    this.store(&Setting::MissingTab(flip_missing.missing), cx);
                },
                cx,
            ))
            .child(self.in_the_ring(
                TAB_COUNTS_ID,
                switch_row(
                    words::settings_appearance::show_the_counts(),
                    words::settings_appearance::the_figure_beside_each_tab_how(),
                    shown.counts,
                    TAB_COUNTS_ID,
                ),
                move |this, _, cx| {
                    this.show_tabs(flip_counts, cx);
                    this.store(&Setting::TabCounts(flip_counts.counts), cx);
                },
                cx,
            ))
    }

    pub(crate) fn show_tabs(&mut self, shown: Tabs, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.tabs = shown);
        if !self.pane.is_shown(shown) {
            self.set_pane(Pane::default(), cx);
        }
        cx.notify();
    }

    pub(super) fn live_spectrum_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let spectral = cx.global::<ResonateApp>().spectral;

        kit::rows()
            .child(kit::field(
                words::settings_appearance::treble_lifted(),
                self.choices("spectrum-tilt", spectral.tilt, cx, |this, tilt, cx| {
                    let held = cx.global::<ResonateApp>().spectral;
                    this.read_the_spectrum(Spectral { tilt, ..held }, cx);
                    this.store(&Setting::SpectrumTilt(tilt), cx);
                }),
            ))
            .child(kit::field(
                words::settings_appearance::floor(),
                self.choices("spectrum-floor", spectral.floor, cx, |this, floor, cx| {
                    let held = cx.global::<ResonateApp>().spectral;
                    this.read_the_spectrum(Spectral { floor, ..held }, cx);
                    this.store(&Setting::SpectrumFloor(floor), cx);
                }),
            ))
            .child(kit::field(
                words::settings_appearance::bands_an_octave(),
                self.choices("spectrum-bands", spectral.bands, cx, |this, bands, cx| {
                    let held = cx.global::<ResonateApp>().spectral;
                    this.read_the_spectrum(Spectral { bands, ..held }, cx);
                    this.store(&Setting::SpectrumBands(bands), cx);
                }),
            ))
            .child(kit::field(
                words::settings_appearance::bars_fall(),
                self.choices("spectrum-falls", spectral.falls, cx, |this, falls, cx| {
                    let held = cx.global::<ResonateApp>().spectral;
                    this.read_the_spectrum(Spectral { falls, ..held }, cx);
                    this.store(&Setting::SpectrumFalls(falls), cx);
                }),
            ))
    }

    pub(crate) fn read_the_spectrum(&self, spectral: Spectral, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.spectral = spectral);
        self.visualiser.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(crate) fn speak(&self, chosen: LanguageChoice, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.language = chosen);
        cx.notify();
    }

    pub(crate) fn draw_scrollbars(&self, mode: ScrollbarMode, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.scrollbars = mode);
        cx.notify();
    }

    pub(crate) fn wheel_the_volume(&self, wheeled: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.scroll_volume = wheeled);
        cx.notify();
    }

    pub(crate) fn reduce_motion(&self, reduced: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.reduced_motion = reduced);
        cx.set_reduce_motion(reduced);
        cx.notify();
    }

    pub(crate) fn navigate_with_mouse_buttons(&self, enabled: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.mouse_navigation = enabled);
        cx.notify();
    }

    pub(crate) fn show_window_buttons(&self, shown: WindowButtons, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.window_buttons = shown);
        cx.notify();
    }

    pub(super) fn window_state_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let global = cx.global::<ResonateApp>();
        let remember_tab = global.remember_tab;
        let remember_window_size = global.remember_window_size;
        let remember_settings_category = global.remember_settings_category;

        kit::rows()
            .child(self.in_the_ring(
                REMEMBER_TAB_ID,
                switch_row(
                    words::settings_appearance::remember_the_last_tab(),
                    words::settings_appearance::off_the_window_opens_on_tracks(),
                    remember_tab,
                    REMEMBER_TAB_ID,
                ),
                move |this, window, cx| this.remember_tab(!remember_tab, window, cx),
                cx,
            ))
            .child(self.in_the_ring(
                REMEMBER_WINDOW_SIZE_ID,
                switch_row(
                    words::settings_appearance::remember_the_window_size(),
                    words::settings_appearance::off_the_window_opens_at_its(),
                    remember_window_size,
                    REMEMBER_WINDOW_SIZE_ID,
                ),
                move |this, window, cx| {
                    this.remember_window_size(!remember_window_size, window, cx);
                },
                cx,
            ))
            .child(self.in_the_ring(
                REMEMBER_SETTINGS_CATEGORY_ID,
                switch_row(
                    words::settings_appearance::remember_the_settings_category(),
                    words::settings_appearance::off_settings_opens_on_output(),
                    remember_settings_category,
                    REMEMBER_SETTINGS_CATEGORY_ID,
                ),
                move |this, _, cx| {
                    this.remember_settings_category(!remember_settings_category, cx);
                },
                cx,
            ))
    }

    pub(crate) fn remember_tab(
        &mut self,
        remember: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remembered.tab = remember;
        cx.update_global::<ResonateApp, _>(|global, _| global.remember_tab = remember);
        self.store(&Setting::Switched(Switch::RememberTab, remember), cx);

        if remember {
            self.remember_current_tab(cx);
        } else {
            self.forget(ConfigKey::LastTab, cx);
            cx.update_global::<ResonateApp, _>(|global, _| global.last_tab = None);
        }
    }

    pub(crate) fn remember_window_size(
        &mut self,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remembered.window_size = remember;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.remember_window_size = remember;
        });
        self.store(&Setting::Switched(Switch::RememberWindowSize, remember), cx);

        if remember {
            self.remember_current_window_size(window, cx);
        } else {
            self.forget(ConfigKey::WindowSize, cx);
            self.remembered.last_window_size = None;
            cx.update_global::<ResonateApp, _>(|global, _| global.window_size = None);
        }
    }

    pub(crate) fn remember_settings_category(&mut self, remember: bool, cx: &mut Context<Self>) {
        self.remembered.settings_category = remember;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.remember_settings_category = remember;
        });
        self.store(
            &Setting::Switched(Switch::RememberSettingsCategory, remember),
            cx,
        );

        if remember {
            self.remember_current_settings_category(cx);
        } else {
            self.forget(ConfigKey::LastSettingsCategory, cx);
            cx.update_global::<ResonateApp, _>(|global, _| {
                global.last_settings_category = Category::default();
            });
        }
    }

    pub(crate) fn remember_current_tab(&mut self, cx: &mut Context<Self>) {
        if !self.remembered.tab {
            return;
        }
        let tab = self.in_front(cx);
        if cx.global::<ResonateApp>().last_tab == Some(tab) {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.last_tab = Some(tab));
        self.store(&Setting::LastTab(tab), cx);
    }

    pub(crate) fn remember_current_settings_category(&mut self, cx: &mut Context<Self>) {
        if !self.remembered.settings_category
            || cx.global::<ResonateApp>().last_settings_category == self.settings_shown.category
        {
            return;
        }
        let category = self.settings_shown.category;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.last_settings_category = category;
        });
        self.store(&Setting::LastSettingsCategory(category), cx);
    }

    pub(crate) fn remember_current_window_size(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.remembered.window_size {
            return;
        }
        let Some(size) = WindowSize::from_pixels(window.window_bounds().get_bounds().size) else {
            return;
        };
        self.remembered.last_window_size = Some(size);
        cx.update_global::<ResonateApp, _>(|global, _| global.window_size = Some(size));
        self.store(&Setting::WindowSize(size), cx);
    }
}
