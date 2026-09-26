use gpui::{Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px, rgb};
use resonate_core::{Accent, Appearance, ScrollbarMode, TextSize, Theme};

use crate::{
    ResonateApp, Setting, SettingKey, Tabs, WindowButtons, WindowSize, theme,
    views::{
        hint::Names,
        kit,
        root::{Pane, RootView},
        settings::{Choice, find::Category, note, switch_row},
    },
};

const KEEPS_ITS_OWN: &str = "Each palette draws each accent in its own tones, and the ringed \
                             swatch is whichever one the palette was built around.";

const ITS_OWN: &str = "Whichever accent this palette was built around";

const ITS_OWN_ID: &str = "accent-of-the-palette";

const MINIMISE_ID: &str = "show-the-minimise-button";

const MAXIMISE_ID: &str = "show-the-maximise-button";

const VOLUME_WHEEL_ID: &str = "wheel-the-volume";
const SUGGESTIONS_TAB_ID: &str = "show-the-suggestions-tab";

const MISSING_TAB_ID: &str = "show-the-missing-tab";

const TAB_COUNTS_ID: &str = "show-the-tab-counts";

const REMEMBER_TAB_ID: &str = "remember-the-last-tab";

const REMEMBER_WINDOW_SIZE_ID: &str = "remember-the-window-size";

const REMEMBER_SETTINGS_CATEGORY_ID: &str = "remember-the-settings-category";

const WINDOW_BUTTONS_NOTE: &str = "A button the compositor does not offer is left out whatever \
                                   this says, and a window the compositor draws a titlebar for \
                                   carries that titlebar's buttons rather than these.";

impl Choice for TextSize {
    const ALL: &'static [Self] = &[Self::Small, Self::Medium, Self::Large];

    fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::from(format!(
            "Everything that holds text — the type, the rows, the sidebar and every control — \
             is drawn at {:.0}% of the middling size.",
            self.scale() * 100.0
        ))
    }
}

impl Choice for ScrollbarMode {
    const ALL: &'static [Self] = &[Self::Shown, Self::AutoHidden, Self::Hidden];

    fn label(self) -> &'static str {
        match self {
            Self::Shown => "Always",
            Self::AutoHidden => "While scrolling",
            Self::Hidden => "Never",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Shown => {
                "Every list and pane that scrolls draws a bar down its edge that shows where the \
                 view stands and can be dragged."
            }
            Self::AutoHidden => {
                "A bar is drawn while its list is scrolling and for a moment after, and whenever \
                 the pointer is over the edge it sits on, so it can still be taken hold of."
            }
            Self::Hidden => "No bar is drawn; a list still scrolls with the wheel and the keys.",
        })
    }
}

impl RootView {
    pub(super) fn colour_group(&mut self, cx: &mut Context<Self>) -> Div {
        let worn = theme::worn();
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

        kit::section_body()
            .child(kit::field("Theme", shelf))
            .child(kit::field(
                "Accent",
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(accents)
                    .child(note(KEEPS_ITS_OWN)),
            ))
    }

    fn palette(&self, palette: Theme, worn: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let chosen = worn.theme == palette;
        let dressed = Appearance {
            theme: palette,
            ..worn
        };

        self.in_the_ring(
            palette.label(),
            div()
                .id(SharedString::new_static(palette.as_str()))
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
            move |this, _, cx| this.wear(dressed, &Setting::Theme(palette), cx),
            cx,
        )
    }

    fn accent(
        &self,
        accent: Option<Accent>,
        worn: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let dressed = Appearance { accent, ..worn };
        let colour = theme::sample(dressed).accent;
        let id = accent.map_or(ITS_OWN_ID, Accent::as_str);
        let saying = accent.map_or(ITS_OWN, Accent::label);

        self.in_the_ring(
            saying,
            kit::dot_swatch(id, colour, worn.accent == accent)
                .when(accent.is_none(), |swatch| {
                    swatch.border_1().border_color(rgb(theme::outline()))
                })
                .names(saying),
            move |this, _, cx| this.wear(dressed, &Setting::Accent(accent), cx),
            cx,
        )
    }

    pub(super) fn layout_group(&mut self, cx: &mut Context<Self>) -> Div {
        let worn = theme::worn();

        kit::section_body().child(kit::field(
            "Text size",
            self.choices(
                "text-size",
                Some(worn.text_size),
                cx,
                |this, size: TextSize, cx| {
                    this.wear(
                        Appearance {
                            text_size: size,
                            ..theme::worn()
                        },
                        &Setting::TextSize(size),
                        cx,
                    );
                },
            ),
        ))
    }

    pub(super) fn window_buttons_group(&mut self, cx: &mut Context<Self>) -> Div {
        let shown = cx.global::<ResonateApp>().window_buttons;
        let flip_minimise = WindowButtons {
            minimise: !shown.minimise,
            ..shown
        };
        let flip_maximise = WindowButtons {
            maximise: !shown.maximise,
            ..shown
        };

        kit::section_body()
            .child(self.in_the_ring(
                MINIMISE_ID,
                switch_row(
                    "Show the minimise button",
                    "Off, the window is put away only by the compositor's own gesture",
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
                    "Show the maximise button",
                    "Off, a double press on the titlebar still fills the screen",
                    shown.maximise,
                    MAXIMISE_ID,
                ),
                move |this, _, cx| {
                    this.show_window_buttons(flip_maximise, cx);
                    this.store(&Setting::MaximiseButton(flip_maximise.maximise), cx);
                },
                cx,
            ))
            .child(note(WINDOW_BUTTONS_NOTE))
    }

    pub(super) fn volume_wheel_group(&mut self, cx: &mut Context<Self>) -> Div {
        let wheeled = cx.global::<ResonateApp>().scroll_volume;

        kit::section_body().child(self.in_the_ring(
            VOLUME_WHEEL_ID,
            switch_row(
                "Turn the volume with the wheel",
                "Over the volume slider, each notch moves it five per cent",
                wheeled,
                VOLUME_WHEEL_ID,
            ),
            move |this, _, cx| {
                this.wheel_the_volume(!wheeled, cx);
                this.store(&Setting::ScrollVolume(!wheeled), cx);
            },
            cx,
        ))
    }

    pub(super) fn scrollbars_group(&mut self, cx: &mut Context<Self>) -> Div {
        let mode = cx.global::<ResonateApp>().scrollbars;

        kit::section_body().child(kit::field(
            "Draw scrollbars",
            self.choices(
                "scrollbars",
                Some(mode),
                cx,
                |this, mode: ScrollbarMode, cx| {
                    this.draw_scrollbars(mode, cx);
                    this.store(&Setting::Scrollbars(mode), cx);
                },
            ),
        ))
    }

    pub(super) fn tabs_group(&mut self, cx: &mut Context<Self>) -> Div {
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

        kit::section_body()
            .child(self.in_the_ring(
                SUGGESTIONS_TAB_ID,
                switch_row(
                    "Show Suggestions",
                    "The lists the catalog offers to make out of what it holds",
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
                    "Show Missing",
                    "What the releases are short of, and what the artists put out elsewhere",
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
                    "Show the counts",
                    "The figure beside each tab: how many albums, tracks, plays and the rest",
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

    pub(crate) fn draw_scrollbars(&self, mode: ScrollbarMode, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.scrollbars = mode);
        cx.notify();
    }

    pub(crate) fn wheel_the_volume(&self, wheeled: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.scroll_volume = wheeled);
        cx.notify();
    }

    pub(crate) fn show_window_buttons(&self, shown: WindowButtons, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.window_buttons = shown);
        cx.notify();
    }

    pub(super) fn window_state_group(&mut self, cx: &mut Context<Self>) -> Div {
        let global = cx.global::<ResonateApp>();
        let remember_tab = global.remember_tab;
        let remember_window_size = global.remember_window_size;
        let remember_settings_category = global.remember_settings_category;

        kit::section_body()
            .child(self.in_the_ring(
                REMEMBER_TAB_ID,
                switch_row(
                    "Remember the last tab",
                    "Off, the window opens on Tracks",
                    remember_tab,
                    REMEMBER_TAB_ID,
                ),
                move |this, window, cx| this.remember_tab(!remember_tab, window, cx),
                cx,
            ))
            .child(self.in_the_ring(
                REMEMBER_WINDOW_SIZE_ID,
                switch_row(
                    "Remember the window size",
                    "Off, the window opens at its built-in size",
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
                    "Remember the Settings category",
                    "Off, Settings opens on Output",
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
        self.remember_tab = remember;
        cx.update_global::<ResonateApp, _>(|global, _| global.remember_tab = remember);
        self.store(&Setting::RememberTab(remember), cx);

        if remember {
            self.remember_current_tab(cx);
        } else {
            self.forget(SettingKey::LastTab, cx);
            cx.update_global::<ResonateApp, _>(|global, _| global.last_tab = None);
        }
    }

    pub(crate) fn remember_window_size(
        &mut self,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember_window_size = remember;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.remember_window_size = remember;
        });
        self.store(&Setting::RememberWindowSize(remember), cx);

        if remember {
            self.remember_current_window_size(window, cx);
        } else {
            self.forget(SettingKey::WindowSize, cx);
            self.last_window_size = None;
            cx.update_global::<ResonateApp, _>(|global, _| global.window_size = None);
        }
    }

    pub(crate) fn remember_settings_category(&mut self, remember: bool, cx: &mut Context<Self>) {
        self.remember_settings_category = remember;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.remember_settings_category = remember;
        });
        self.store(&Setting::RememberSettingsCategory(remember), cx);

        if remember {
            self.remember_current_settings_category(cx);
        } else {
            self.forget(SettingKey::LastSettingsCategory, cx);
            cx.update_global::<ResonateApp, _>(|global, _| {
                global.last_settings_category = Category::default();
            });
        }
    }

    pub(crate) fn put_window_state_back(&mut self, cx: &mut Context<Self>) {
        self.remember_tab = true;
        self.remember_window_size = true;
        self.remember_settings_category = true;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.remember_tab = true;
            global.remember_window_size = true;
            global.remember_settings_category = true;
        });
    }

    pub(crate) fn remember_current_tab(&self, cx: &mut Context<Self>) {
        if !self.remember_tab {
            return;
        }
        let tab = self.in_front(cx);
        if cx.global::<ResonateApp>().last_tab == Some(tab) {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.last_tab = Some(tab));
        self.store(&Setting::LastTab(tab), cx);
    }

    pub(crate) fn remember_current_settings_category(&self, cx: &mut Context<Self>) {
        if !self.remember_settings_category
            || cx.global::<ResonateApp>().last_settings_category == self.settings_category
        {
            return;
        }
        let category = self.settings_category;
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.last_settings_category = category;
        });
        self.store(&Setting::LastSettingsCategory(category), cx);
    }

    pub(crate) fn remember_current_window_size(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.remember_window_size {
            return;
        }
        let Some(size) = WindowSize::from_pixels(window.window_bounds().get_bounds().size) else {
            return;
        };
        self.last_window_size = Some(size);
        cx.update_global::<ResonateApp, _>(|global, _| global.window_size = Some(size));
        self.store(&Setting::WindowSize(size), cx);
    }
}
