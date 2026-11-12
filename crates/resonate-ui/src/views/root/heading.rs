use ahash::AHashSet;
use gpui::{
    AnyElement, Context, Div, MouseButton, Role, SharedString, Stateful, Window, div, prelude::*,
    px, rgb,
};
use resonate_core::PlaylistId;

use super::{PINNED_INSET, Pane, RootView, Section, WORDMARK_CAPITALS_CENTRED_BY};
use crate::{
    ResonateApp,
    icons::{self, Icon},
    theme,
    views::{
        chrome,
        hint::{self, Names},
        kit::{self, EndsInAnEllipsis},
        menu::{self, Menu},
        reorder::{self, LiftedTo},
        scrollbar::Scrollbars,
        spoken::Spoken,
    },
    words,
};

impl RootView {
    pub(super) fn header(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let moves_the_window = chrome::the_header_moves_the_window(window);
        let controls_are_ours = chrome::the_window_controls_are_ours(window);
        let corners = chrome::rounded_within_the_frame(window);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_4()
            .h(px(theme::header_height()))
            .px_4()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .rounded_tl(corners.top_left)
            .rounded_tr(corners.top_right)
            .bg(rgb(theme::surface()))
            .when(moves_the_window, chrome::titlebar)
            .child(self.wordmark(window))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .min_w(px(0.0))
                    .child(self.search_field(window, cx))
                    .child(
                        self.in_the_pane_ring(
                            kit::icon_button(
                                "listen",
                                Icon::Listen,
                                words::root::listen_hint(key!(listen)),
                            )
                            .flex_none()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                            |this, _, cx| this.open_the_listener(cx),
                            cx,
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .w(px(theme::sidebar_width() - 32.0))
                    .justify_end()
                    .child(self.zone_switcher(cx))
                    .when(controls_are_ours, |bar| {
                        bar.child(self.window_controls(window, cx))
                    }),
            )
    }

    pub(super) fn wordmark(&self, window: &Window) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .w(px(theme::sidebar_width() - 32.0))
            .when_some(chrome::room_for_the_traffic_lights(window), |mark, room| {
                mark.pl(room).overflow_hidden()
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(theme::wordmark_mark()))
                    .rounded_md()
                    .bg(rgb(theme::accent()))
                    .child(icons::icon(
                        Icon::Resonate,
                        theme::root_icon(),
                        theme::accent_ink(),
                    )),
            )
            .child(
                div()
                    .pt(px(WORDMARK_CAPITALS_CENTRED_BY))
                    .text_size(px(theme::text_base()))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child("Resonate"),
            )
    }

    pub(super) fn search_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search.read(cx);
        let searching = search.is_focused(window);
        let empty = search.text().is_empty();

        let box_of_words = div()
            .id("search")
            .flex()
            .flex_1()
            .max_w(px(theme::search_width()))
            .items_center()
            .gap_2()
            .h(px(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(if searching {
                theme::background()
            } else {
                theme::raised()
            }))
            .border_1()
            .border_color(if searching {
                kit::field_lit()
            } else {
                rgb(theme::soft_edge())
            })
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.bg(rgb(theme::hover())))
            .child(icons::icon(
                Icon::Search,
                theme::search_icon(),
                if searching {
                    theme::accent()
                } else {
                    theme::faint()
                },
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.leave_search(window, cx)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.search
                    .update(cx, |search, cx| search.take_focus(window, cx));
                cx.notify();
            }))
            .child(self.search.clone())
            .when(!empty, |field| field.child(self.clear_search(cx)))
            .when(empty && !searching, |field| {
                field.child(kit::figure(key!(find)).text_color(rgb(theme::faint())))
            })
            .child(hint::explains("search-terms", words::root::search_hint()));

        menu::opens_a_menu(
            box_of_words,
            |this, at, cx| {
                let holding = this.search.read(cx).holds_a_selection();

                Menu::at(at)
                    .when_some(holding.then_some(()), |menu, ()| {
                        menu.under(
                            Icon::Rename,
                            words::root::cut(),
                            key!(cut),
                            |this, window, cx| {
                                this.search.update(cx, |field, cx| field.cuts(window, cx));
                            },
                        )
                        .under(
                            Icon::Export,
                            words::root::copy(),
                            key!(copy),
                            |this, window, cx| {
                                this.search.update(cx, |field, cx| field.copies(window, cx));
                            },
                        )
                    })
                    .under(
                        Icon::Import,
                        words::root::paste(),
                        key!(paste),
                        |this, window, cx| {
                            this.search.update(cx, |field, cx| field.pastes(window, cx));
                        },
                    )
                    .under(
                        Icon::Check,
                        words::root::select_all(),
                        key!(select_everything),
                        |this, window, cx| {
                            this.search
                                .update(cx, |field, cx| field.selects_everything(window, cx));
                        },
                    )
            },
            cx,
        )
        .into_any_element()
    }

    pub(super) fn clear_search(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("clear-search")
            .group("clear")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(theme::clear_control()))
            .rounded_full()
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme::hover())))
            .names(words::root::clear_search_hint(key!(leave)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.search.update(cx, |search, cx| {
                    search.take_focus(window, cx);
                    search.clear(cx);
                });
            }))
            .child(icons::lit_on_hover(
                icons::icon(Icon::Close, theme::clear_mark(), theme::muted()),
                "clear",
            ))
    }

    pub(super) fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let library = self.library.read(cx);
        let albums = library.albums_counted() as usize;
        let artists = library.artists_counted() as usize;
        let tracks = library.tracks_counted() as usize;
        let saved = library.playlists().len();
        let missing = library.missing().tracks as usize;
        let favourites = library.favourited().held();
        let offered = library.suggestions().len();
        let plays = library.statistics().plays as usize;
        let tabs = cx.global::<ResonateApp>().tabs;
        let serving = self.server.read(cx).is_open();
        let enriching = library.shows_the_lookup();
        let downloads = !library.downloads().is_empty() || library.keeps_left().is_some();
        let mut browse = div().flex().flex_col().gap_4();
        for section in Section::ALL {
            let mut listed = div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(kit::eyebrow(section.label()).px_3().pb_1p5());
            for pane in Pane::BROWSE
                .into_iter()
                .filter(|pane| pane.section() == section && pane.is_there(tabs, serving))
            {
                let count = match pane {
                    Pane::Albums => Some(albums),
                    Pane::Artists => Some(artists),
                    Pane::Tracks => Some(tracks),
                    Pane::Statistics => (plays > 0).then_some(plays),
                    Pane::Party => None,
                    Pane::Playlists => Some(saved),
                    Pane::Favourites => (favourites > 0).then_some(favourites),
                    Pane::Suggestions => (offered > 0).then_some(offered),
                    Pane::Missing => (missing > 0).then_some(missing),
                    Pane::Queue
                    | Pane::Server
                    | Pane::Lyrics
                    | Pane::Inspector
                    | Pane::Visualiser
                    | Pane::Analysis
                    | Pane::Settings => None,
                };
                listed = listed.child(self.pane_row(pane, count.filter(|_| tabs.counts), cx));
                if pane == Pane::Playlists {
                    listed = listed.children(self.pinned_rows(cx));
                }
            }
            browse = browse.child(listed);
        }

        let content = div()
            .id("sidebar-content")
            .flex()
            .flex_col()
            .flex_none()
            .p_3()
            .w(px(theme::sidebar_width() - 1.0))
            .child(browse);

        let foot = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .p_3()
            .pt_0()
            .w(px(theme::sidebar_width() - 1.0))
            .when(downloads, |column| column.child(self.download_status(cx)))
            .when(enriching, |column| column.child(self.enrichment_status(cx)))
            .when_some(self.remote_playback(window, cx), |column, panel| {
                column.child(panel)
            })
            .child(self.pane_row(Pane::Settings, None, cx));

        div()
            .id("sidebar")
            .relative()
            .named_as(Role::Navigation, words::root::sidebar(), "")
            .flex()
            .flex_none()
            .flex_col()
            .w(px(theme::sidebar_width()))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::surface()))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("sidebar-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll)
                            .child(content),
                    )
                    .child(
                        Scrollbars::of(cx)
                            .vertical("sidebar-scrollbar", self.sidebar_scroll.clone()),
                    ),
            )
            .child(foot)
            .child(self.float_remote_when_categories_scroll(window, cx))
    }

    pub(super) fn pinned_rows(&self, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let library = self.library.read(cx);
        let pinned = library.pinned();
        let opened = (self.pane == Pane::Playlists)
            .then(|| library.opened())
            .flatten();
        let playing = self.playing_playlist(cx);
        let lists: AHashSet<PlaylistId> = library.lists().iter().map(|list| list.id).collect();

        pinned
            .iter()
            .map(|playlist| {
                let id = playlist.id;
                let colour = if playing == Some(id) {
                    theme::accent()
                } else if opened == Some(id) {
                    theme::text()
                } else {
                    theme::muted()
                };

                div()
                    .id(("pinned-playlist", id.get() as usize))
                    .debug_selector(move || format!("pinned-playlist-{}", id.get()))
                    .flex()
                    .items_center()
                    .h(px(theme::pinned_row()))
                    .pl(px(PINNED_INSET + theme::pane_icon()))
                    .pr_3()
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(colour))
                    .when(opened == Some(id), |row| {
                        row.font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .hover(|row| row.bg(rgb(theme::hover())))
                    .named_as(Role::Link, &playlist.name, words::root::pinned_hint())
                    .names(words::root::pinned_hint())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(playlist.name.clone())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_pane(Pane::Playlists, cx);
                        this.show_playlist(Some(id), cx);
                    }))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .zip(pinned.iter())
            .map(|(row, playlist)| match lists.contains(&playlist.id) {
                true => reorder::takes_a_lift(row, LiftedTo::Playlist(playlist.id), cx),
                false => row,
            })
            .collect()
    }
}
