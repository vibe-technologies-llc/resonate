use gpui::{
    AnyElement, Context, Div, MouseButton, Role, SharedString, Stateful, Window, div, prelude::*,
    px, rgb,
};
use resonate_core::AlbumId;

use super::{Magnified, PANE_GROUP, PANE_REGION, Pane, RootView, chosen_ground, framed_cover};
use crate::{
    Drawn, format,
    icons::{self, Icon},
    models::Picture,
    motion, theme,
    views::{
        chrome,
        hint::Names,
        kit::{self, EndsInAnEllipsis},
        listing::Pictured,
        part::Region,
        settings::Page,
        spoken::Spoken,
    },
    words,
};

impl RootView {
    pub(super) fn enrichment_status(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let row = div()
            .id("enriching")
            .named_as(
                Role::Button,
                words::root::enriching_label(),
                words::root::enriching_hint(),
            )
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::sidebar_row()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(words::root::enriching_hint())
            .child(icons::icon(Icon::Globe, theme::pane_icon(), theme::faint()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(words::root::enriching_label()),
            );

        self.in_the_standing_ring(
            "enriching",
            row,
            |this, _, cx| {
                this.set_pane(Pane::Settings, cx);
                this.open_the_settings_page(Page::Enrichment, cx);
            },
            cx,
        )
    }

    pub(crate) fn in_the_standing_ring(
        &self,
        named: &'static str,
        mut control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let handle = self.standing_controls.at(named, cx);
        self.standing_controls
            .captioned_by_its_name(named, &mut control);
        Self::ringed(&handle, control, press, cx)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
    }

    pub(super) fn pane_row(
        &self,
        pane: Pane,
        count: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let chosen = pane == self.in_front(cx);
        let colour = if chosen {
            theme::text()
        } else {
            theme::muted()
        };
        let mark = if chosen {
            theme::accent()
        } else {
            theme::muted()
        };

        let row = div()
            .id(SharedString::new_static(pane.label()))
            .debug_selector(|| format!("tab-{}", pane.as_str()))
            .group(PANE_GROUP)
            .relative()
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::sidebar_row()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(theme::text_sm()))
            .when(chosen, |row| row.font_weight(gpui::FontWeight::MEDIUM))
            .hover(|row| row.bg(rgb(theme::hover())))
            .text_color(rgb(colour))
            .child(chosen_ground(chosen))
            .child(icons::lit_on_hover(
                icons::icon(pane.icon(), theme::pane_icon(), mark),
                PANE_GROUP,
            ))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(pane.label()),
            )
            .named_as(Role::Tab, pane.label(), pane.about())
            .chosen(chosen)
            .names(pane.about())
            .when_some(count, |row, count| {
                row.child(kit::figure(count.to_string()).text_color(rgb(theme::faint())))
            });
        self.in_the_standing_ring(
            pane.label(),
            row,
            move |this, _, cx| this.choose_pane(pane, cx),
            cx,
        )
    }

    pub(crate) fn drawn_cover(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<(Picture, Magnified)> {
        let (album, file) = pictured.parts();

        if let Some(album) = album
            && let Some(art) = self
                .library
                .update(cx, |library, cx| library.cover(album, drawn, cx))
        {
            return Some((art, Magnified::Album(album)));
        }

        let file = file?;
        if self.server.read(cx).names(file) {
            let art = self
                .server
                .update(cx, |server, cx| server.cover(file, None, drawn, cx))?;
            return Some((art, Magnified::Served(file.clone())));
        }
        let art = self
            .player
            .update(cx, |player, cx| player.art(file, drawn, cx))?;
        Some((art, Magnified::File(file.clone())))
    }

    pub(crate) fn is_pictureless(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> bool {
        let (album, file) = pictured.parts();
        let album_lacks =
            album.is_none_or(|album| self.library.read(cx).cover_is_absent(album, drawn));
        let file_lacks = file.is_none_or(|file| {
            let server = self.server.read(cx);
            if server.names(file) {
                server.cover_is_absent(file, None, drawn)
            } else {
                self.player.read(cx).art_is_absent(file, drawn)
            }
        });
        album_lacks && file_lacks
    }

    pub(crate) fn covers_ahead(
        &self,
        albums: impl Iterator<Item = AlbumId>,
        cx: &mut Context<Self>,
    ) {
        self.library.update(cx, |library, cx| {
            for album in albums {
                if !library.cover_ahead(album, Drawn::InARow, cx) {
                    break;
                }
            }
        });
    }

    pub(super) fn magnified_art(
        &self,
        magnified: &Magnified,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        match magnified {
            Magnified::Album(album) => self
                .library
                .update(cx, |library, cx| library.whole_cover(*album, cx)),
            Magnified::File(file) => self
                .player
                .update(cx, |player, cx| player.whole_art(file, cx)),
            Magnified::Served(file) => self.server.update(cx, |server, cx| {
                server.cover(file, None, Drawn::NowPlaying, cx)
            }),
        }
    }

    pub(super) fn magnified_title(
        &self,
        magnified: &Magnified,
        cx: &mut Context<Self>,
    ) -> Option<SharedString> {
        match magnified {
            Magnified::Album(album) => self
                .library
                .update(cx, |library, _| library.album_title(*album))
                .map(SharedString::from),
            Magnified::File(file) | Magnified::Served(file) => Some(SharedString::from(
                self.player
                    .read(cx)
                    .media(file, None)
                    .and_then(|info| info.tags.title.clone())
                    .unwrap_or_else(|| format::stem(file)),
            )),
        }
    }

    pub(crate) fn hero_side(&self) -> f32 {
        let measured = self.rooms.hero_height.get();
        if measured > px(0.0) {
            measured / px(1.0)
        } else {
            theme::scope_cover()
        }
    }

    pub(crate) fn cover(&self, pictured: Pictured<'_>, cx: &mut Context<Self>) -> Div {
        self.cover_sized(pictured, Drawn::InARow, theme::row_cover(), cx)
    }

    pub(crate) fn cover_sized(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let art = self.drawn_cover(pictured, drawn, cx).map(|(art, _)| art);
        framed_cover(art, side)
    }

    pub(crate) fn region(
        &mut self,
        region: Region,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match region {
            Region::Header => self.header(window, cx).into_any_element(),
            Region::Sidebar => self.sidebar(window, cx).into_any_element(),
            Region::Pane => {
                self.rooms.grid_width.seen_in(window.viewport_size().width);
                self.finding_holds_the_caret = self.typed.finding.read(cx).is_focused(window);
                let shown = self.pane_shown(cx);
                let named = match self.search_in_front(cx) {
                    Some(_) => words::root::search_region(),
                    None => self.pane.label(),
                };
                motion::faded_in(
                    div()
                        .id(PANE_REGION)
                        .regioned(named)
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(self.content(cx)),
                    shown,
                    motion::ARRIVES_OVER,
                )
                .into_any_element()
            }
            Region::Transport => self.transport(chrome::rounded_within_the_frame(window), cx),
        }
    }

    pub(super) fn pane_shown(&self, cx: &mut Context<Self>) -> SharedString {
        let searching = self.search_in_front(cx).is_some();
        let selection = self.library.read(cx).selection();

        SharedString::from(match (searching, self.pane) {
            (true, _) => "pane-search".to_owned(),
            (false, Pane::Tracks) => format!("pane-tracks-{selection:?}"),
            (false, pane) => format!("pane-{pane:?}"),
        })
    }

    pub(super) fn content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.controls.opening();
        if let Some(shows) = self.search_in_front(cx) {
            return self.search_pane(shows, cx);
        }

        match self.pane {
            Pane::Albums => self.albums(cx),
            Pane::Artists => self.artists(cx),
            Pane::Tracks => self.tracks(cx),
            Pane::Statistics => self.statistics_pane(cx),
            Pane::Party => self.party_pane(cx).into_any_element(),
            Pane::Queue => self.queue_pane(cx),
            Pane::Playlists => self.playlists_pane(cx),
            Pane::Favourites => self.favourites_pane(cx),
            Pane::Suggestions => self.suggestions_pane(cx),
            Pane::Missing => self.missing_pane(cx),
            Pane::Server => self.server_pane(cx),
            Pane::Lyrics => self.lyrics_pane(cx),
            Pane::Inspector => self.inspector(cx),
            Pane::Visualiser => self.visualiser_pane(cx),
            Pane::Analysis => self.analysis_pane(cx),
            Pane::Settings => self.settings(cx),
        }
    }
}
