use gpui::{Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_core::{Accent, Appearance, TextSize, Theme};

use crate::{
    ResonateApp, Setting, Tabs, WindowButtons, theme,
    views::{
        hint::Names,
        kit,
        root::{Pane, RootView},
        settings::{Choice, note, switch_row},
    },
};

const KEEPS_ITS_OWN: &str = "Each palette draws each accent in its own tones, and the ringed \
                             swatch is whichever one the palette was built around.";

const ITS_OWN: &str = "Whichever accent this palette was built around";

const ITS_OWN_ID: &str = "accent-of-the-palette";

const MINIMISE_ID: &str = "show-the-minimise-button";

const MAXIMISE_ID: &str = "show-the-maximise-button";

const VOLUME_WHEEL_ID: &str = "wheel-the-volume";
const SCROLLBARS_ID: &str = "draw-scrollbars";

const SUGGESTIONS_TAB_ID: &str = "show-the-suggestions-tab";

const MISSING_TAB_ID: &str = "show-the-missing-tab";

const TAB_COUNTS_ID: &str = "show-the-tab-counts";

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
        let drawn = cx.global::<ResonateApp>().scrollbars;

        kit::section_body().child(self.in_the_ring(
            SCROLLBARS_ID,
            switch_row(
                "Draw scrollbars",
                "Off, a list still scrolls with the wheel and the keys",
                drawn,
                SCROLLBARS_ID,
            ),
            move |this, _, cx| {
                this.draw_scrollbars(!drawn, cx);
                this.store(&Setting::Scrollbars(!drawn), cx);
            },
            cx,
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

    pub(crate) fn draw_scrollbars(&self, drawn: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.scrollbars = drawn);
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
}
