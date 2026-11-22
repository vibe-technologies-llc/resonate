use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Context, Div, ElementId, FontWeight, ObjectFit, SharedString, Stateful,
    div, img, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{ArtistId, ArtistsDrawn};
use resonate_library::{Artist, ArtistFound, Column, Favoured};

use super::{
    Chooses,
    albums::Caption,
    grid::CELL_GROUP,
    menus::{artist_menu_among, artist_menu_favoured},
    missing_albums::not_held_heading,
    rows::{REACHED_RING, cursor_ring, reached_ring},
};
use crate::{
    Portrayed, Selection,
    held::Rows,
    icons::Icon,
    models::Picture,
    theme,
    views::{
        hint::Names,
        kit,
        kit::EndsInAnEllipsis,
        listing, menu,
        playlists::ROW_GROUP,
        pointed,
        pointed::LitUnderThePointer,
        reorder::{Listed, Shift},
        root::{RootView, tall_row},
        scrollbar::Scrollbars,
        sorting::Ordered,
        spoken,
        spoken::Spoken,
    },
    words,
};

const ARTIST_FOUND_CELL_WIDENING: f32 = 1.8;

impl RootView {
    fn artists_found_beyond(&self, cx: &mut Context<Self>) -> Option<Div> {
        let found = self.library.read(cx).artists_found();
        (self.search_in_front(cx).is_some() && !found.is_empty())
            .then(|| self.artists_found_strip(&found, None, cx))
    }

    pub(crate) fn artists(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let artists = self.library.read(cx).artists();
        let narrowed = self.library.read(cx).narrowing().is_some();
        let reads = self.library.read(cx).search().reads();
        let selected = self.library.read(cx).selection();
        let nothing = artists.is_empty();
        let unreadable = self.library.read(cx).unreadable();
        let counted = self.library.read(cx).artists_counted() as usize;
        let held = artists.len();
        let listed = self.library.read(cx).artists_listed();
        let drawn = self.shows.artists;

        if drawn == ArtistsDrawn::List {
            self.land_where_it_was_left(listed, cx);
        }

        let beyond = self.artists_found_beyond(cx);
        let found_nothing = (nothing && beyond.is_none()).then(|| {
            self.nothing_beyond(cx).unwrap_or_else(|| {
                self.nothing_matched(
                    Icon::Artists,
                    if unreadable {
                        words::browser::library_unreadable()
                    } else if narrowed {
                        words::browser::no_artists_match()
                    } else {
                        words::browser::no_artists_yet()
                    },
                    Some(if unreadable {
                        words::browser::library_unreadable_more()
                    } else if narrowed {
                        words::browser::nothing_held_matches()
                    } else {
                        words::browser::add_a_folder()
                    }),
                    cx,
                )
            })
        });

        let heading = self.search_heading(cx).unwrap_or_else(|| {
            kit::heading().child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(theme::heading_name()))
                            .gap_1()
                            .child(kit::title(words::browser::artists()))
                            .child(kit::subtitle(words::browser::artists_counted(counted))),
                    )
                    .when(!reads.is_empty(), |row| row.child(listing::reads(&reads)))
                    .child(
                        kit::actions()
                            .child(self.artists_drawn_as(drawn, cx))
                            .child(self.orders_a_listing("order-artists", Ordered::Artists, cx)),
                    ),
            )
        });
        let grid =
            (!nothing && drawn == ArtistsDrawn::Grid).then(|| self.artist_grid(&artists, cx));

        kit::pane()
            .child(heading)
            .when_some(found_nothing, |pane, nothing| pane.child(nothing))
            .when_some(grid, Div::child)
            .when(!nothing && drawn == ArtistsDrawn::List, |pane| {
                pane.child(
                    Scrollbars::of(cx).around(
                        "artist-scrollbar",
                        self.scrolls(cx).artist_rows.clone(),
                        uniform_list(
                            "artists",
                            listed,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                this.reach_further(range.end, held, cx);

                                let mut rows = Vec::new();

                                for index in range {
                                    let Some(artist) = artists.get(index) else {
                                        rows.push(tall_row(false).id(("unread-artist", index)));
                                        continue;
                                    };
                                    let id = artist.id;
                                    let chosen = selected == Selection::Artist(id);

                                    let favourite = this
                                        .library
                                        .read(cx)
                                        .favours(Favoured::Artist(id), artist.favourite.is_some());
                                    let shift = Shift::Listing(Listed::Artists);

                                    let listed = this.marked_where_reached(
                                        shift,
                                        index,
                                        tall_row(chosen)
                                            .id(("artist", id.get()))
                                            .debug_selector(move || format!("artist-{}", id.get()))
                                            .listed_as(|| {
                                                spoken::artist(
                                                    &artist.name,
                                                    artist.album_count as usize,
                                                    artist.track_count as usize,
                                                )
                                            })
                                            .group(ROW_GROUP)
                                            .cursor_pointer()
                                            .hover(|entry| entry.bg(rgb(theme::hover())))
                                            .names(words::browser::open_artist_hint())
                                            .child(this.artist_mark(artist, chosen, cx))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.0))
                                                    .truncate()
                                                    .ends_in_an_ellipsis()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(listing::matched(
                                                        artist.name.clone().into(),
                                                        this.library
                                                            .read(cx)
                                                            .search()
                                                            .lit(&artist.name, Column::Artist),
                                                    )),
                                            )
                                            .child(kit::figure(format!(
                                                "{} · {}",
                                                words::browser::albums_counted(artist.album_count),
                                                words::browser::tracks_counted(artist.track_count)
                                            )))
                                            .child(this.favour_mark_under(
                                                ("artist-favourite", index),
                                                Favoured::Artist(id),
                                                favourite,
                                                ROW_GROUP,
                                                cx,
                                            ))
                                            .on_click(cx.listener(
                                                move |this, event: &ClickEvent, _, cx| {
                                                    if this.reach_by_click(
                                                        shift,
                                                        index,
                                                        event.modifiers(),
                                                        cx,
                                                    ) {
                                                        return;
                                                    }
                                                    this.opened(Selection::Artist(id), cx);
                                                },
                                            )),
                                        cx,
                                    );
                                    let reach = this.at_the_reach(shift, index, cx);

                                    rows.push(menu::opens_a_reachable_menu(
                                        listed,
                                        reach,
                                        move |this, at, cx| {
                                            artist_menu_among(
                                                at,
                                                id,
                                                favourite,
                                                Chooses::among(Listed::Artists, index),
                                                this,
                                                cx,
                                            )
                                        },
                                        cx,
                                    ));
                                }
                                rows
                            }),
                        )
                        .track_scroll(&self.scrolls(cx).artist_rows)
                        .h_full()
                        .w_full(),
                    ),
                )
            })
            .when_some(beyond, Div::child)
            .into_any_element()
    }

    fn artists_drawn_as(&self, drawn: ArtistsDrawn, cx: &mut Context<Self>) -> Div {
        let choice = |as_: ArtistsDrawn, label: &'static str, cx: &mut Context<Self>| {
            self.in_the_pane_ring(
                kit::segment(("artists-drawn", as_ as usize), label, drawn == as_)
                    .names(saying(as_)),
                move |this, _, cx| {
                    this.draw_the_artists_as(as_, cx);
                },
                cx,
            )
        };

        kit::segmented()
            .child(choice(ArtistsDrawn::List, words::browser::list(), cx))
            .child(choice(ArtistsDrawn::Grid, words::browser::grid(), cx))
    }

    fn artist_grid(&mut self, artists: &Rows<Artist>, cx: &mut Context<Self>) -> Div {
        let artists = artists.clone();
        let held = artists.len();
        let columns = self.grid_columns();
        let rows = self
            .library
            .read(cx)
            .artists_listed()
            .div_ceil(columns.max(1));
        let measured = self.rooms.grid_width.clone();
        let laid_out = self.rooms.grid_width.get() > px(0.0);

        if laid_out {
            self.land_where_it_was_left(rows, cx);
        }

        let selected = self.library.read(cx).selection();

        div()
            .relative()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .pt_5()
            .child(kit::measures_the_grid(measured))
            .when(laid_out, |body| {
                body.child(
                    uniform_list(
                        "artist-grid",
                        rows,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            this.reach_further(range.end * columns, held, cx);
                            range
                                .map(|index| {
                                    let tall = this.grid_row();
                                    let cells = artists
                                        .iter()
                                        .enumerate()
                                        .skip(index * columns)
                                        .take(columns)
                                        .map(|(at, artist)| {
                                            let chosen = selected == Selection::Artist(artist.id);
                                            this.artist_cell(artist, at, chosen, cx)
                                        });
                                    div()
                                        .flex()
                                        .gap(px(theme::grid_gap()))
                                        .px_6()
                                        .h(px(tall))
                                        .children(cells)
                                })
                                .collect()
                        }),
                    )
                    .track_scroll(&self.scrolls(cx).artist_rows)
                    .h_full()
                    .w_full(),
                )
            })
            .child(
                Scrollbars::of(cx)
                    .vertical("artist-scrollbar", self.scrolls(cx).artist_rows.clone()),
            )
    }

    pub(super) fn artist_cell(
        &mut self,
        artist: &Artist,
        at: usize,
        chosen: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = artist.id;
        let side = self.grid_side();
        let shift = Shift::Listing(Listed::Artists);
        let reached = self.reaches(shift, at, cx);
        let cursor_only = self.stands_unchosen_on(shift, at, cx);
        let favourite = self
            .library
            .read(cx)
            .favours(Favoured::Artist(id), artist.favourite.is_some());
        let lit_name = self
            .library
            .read(cx)
            .search()
            .lit(&artist.name, Column::Artist);
        let portrait = artist
            .has_portrait
            .then(|| {
                self.library.update(cx, |library, cx| {
                    library.portrait(id, Portrayed::InAGrid, cx)
                })
            })
            .flatten();

        let pictured = match portrait {
            Some(art) => portrait_frame(art, side).into_any_element(),
            None => {
                kit::avatar_at(&artist.name, chosen, side, side * AVATAR_LETTER).into_any_element()
            }
        };
        let cell_id = ElementId::from(("artist-cell", id.get() as usize));
        let pointed = pointed::is_pointed_at(&cell_id);

        let cell = div()
            .id(cell_id.clone())
            .debug_selector(move || format!("artist-{}", id.get()))
            .listed_as(|| {
                spoken::artist(
                    &artist.name,
                    artist.album_count as usize,
                    artist.track_count as usize,
                )
            })
            .chosen(reached)
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(words::browser::open_artist_hint())
            .child(
                div()
                    .relative()
                    .rounded(px(side / 2.0))
                    .child(pictured)
                    .when(reached, |frame| {
                        frame.child(reached_ring().rounded(px(side / 2.0 + REACHED_RING + 1.0)))
                    })
                    .when(cursor_only, |frame| {
                        frame.child(cursor_ring().rounded(px(side / 2.0 + REACHED_RING + 1.0)))
                    })
                    .child(
                        self.favour_mark_under(
                            ("artist-cell-favourite", id.get() as usize),
                            Favoured::Artist(id),
                            favourite,
                            CELL_GROUP,
                            cx,
                        )
                        .absolute()
                        .top_1()
                        .right_1()
                        .rounded_md()
                        .bg(kit::cover_veil()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_0p5()
                    .w_full()
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if pointed || chosen {
                                theme::accent()
                            } else {
                                theme::text()
                            }))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(listing::matched(artist.name.clone().into(), lit_name)),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .child(format!(
                                "{} · {}",
                                words::browser::albums_counted(artist.album_count),
                                words::browser::tracks_counted(artist.track_count)
                            )),
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if this.reach_by_click(shift, at, event.modifiers(), cx) {
                    return;
                }
                this.opened(Selection::Artist(id), cx);
            }));

        let reach = self.at_the_reach(shift, at, cx);
        menu::opens_a_reachable_menu(
            cell,
            reach,
            move |this, menu_at, cx| {
                artist_menu_among(
                    menu_at,
                    id,
                    favourite,
                    Chooses::among(Listed::Artists, at),
                    this,
                    cx,
                )
            },
            cx,
        )
        .into_any_element()
    }

    fn artist_mark(&self, artist: &Artist, lit: bool, cx: &mut Context<Self>) -> AnyElement {
        let portrait = artist
            .has_portrait
            .then(|| {
                self.library.update(cx, |library, cx| {
                    library.portrait(artist.id, Portrayed::InARow, cx)
                })
            })
            .flatten();

        match portrait {
            Some(art) => portrait_frame(art, theme::avatar()).into_any_element(),
            None => kit::avatar(&artist.name, lit).into_any_element(),
        }
    }

    pub(super) fn artist_records(&self, artist: ArtistId, cx: &mut Context<Self>) -> Stateful<Div> {
        let albums = Arc::clone(&self.around_the_rows(cx).records);
        let not_held = self.library.read(cx).albums_not_held();
        let mut cells = Vec::new();

        for album in albums.iter() {
            cells.push(
                self.album_cell_captioned(
                    album,
                    self.grid_side(),
                    Caption::Beside(artist),
                    None,
                    cx,
                )
                .into_any_element(),
            );
        }
        if !not_held.is_empty() {
            cells.push(not_held_heading(not_held.len(), !albums.is_empty()).into_any_element());
        }
        for album in not_held.iter() {
            cells.push(
                self.album_not_held_cell(album, self.grid_side(), cx)
                    .into_any_element(),
            );
        }

        div()
            .id(("artist-records", artist.get() as usize))
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&self.scrolls(cx).artist_records)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(theme::grid_gap()))
                    .px_6()
                    .pt_5()
                    .pb_6()
                    .child(kit::measures_the_grid(self.rooms.grid_width.clone()))
                    .children(cells),
            )
    }

    pub(crate) fn artist_cell_at(
        &self,
        artist: &Artist,
        side: f32,
        chooses: Option<Chooses>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = artist.id;
        let reached = chooses.is_some_and(|at| self.reaches(at.shift, at.row, cx));
        let cursor_only = chooses.is_some_and(|at| self.stands_unchosen_on(at.shift, at.row, cx));
        let favourite = self
            .library
            .read(cx)
            .favours(Favoured::Artist(id), artist.favourite.is_some());
        let portrait = artist
            .has_portrait
            .then(|| {
                self.library.update(cx, |library, cx| {
                    library.portrait(id, Portrayed::InAGrid, cx)
                })
            })
            .flatten();
        let picture = match portrait {
            Some(art) => portrait_frame(art, side).into_any_element(),
            None => kit::avatar(&artist.name, false)
                .size(px(side))
                .text_size(px(theme::text_title()))
                .into_any_element(),
        };

        let cell_id = ElementId::from(("favourite-artist", id.get() as usize));
        let pointed = pointed::is_pointed_at(&cell_id);

        let cell = div()
            .id(cell_id.clone())
            .debug_selector(move || format!("artist-{}", id.get()))
            .listed_as(|| {
                spoken::artist(
                    &artist.name,
                    artist.album_count as usize,
                    artist.track_count as usize,
                )
            })
            .chosen(reached)
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(words::browser::open_artist_hint())
            .child(
                div()
                    .relative()
                    .rounded(px(side / 2.0))
                    .child(picture)
                    .when(reached, |frame| {
                        frame.child(reached_ring().rounded(px(side / 2.0 + REACHED_RING + 1.0)))
                    })
                    .when(cursor_only, |frame| {
                        frame.child(cursor_ring().rounded(px(side / 2.0 + REACHED_RING + 1.0)))
                    }),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(theme::text_sm()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if pointed {
                        theme::accent()
                    } else {
                        theme::text()
                    }))
                    .text_center()
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(artist.name.clone())),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if let Some(at) = chooses
                    && this.reach_by_click(at.shift, at.row, event.modifiers(), cx)
                {
                    return;
                }
                this.opened(Selection::Artist(id), cx);
            }));

        let reach = chooses.and_then(|at| self.at_the_reach(at.shift, at.row, cx));

        menu::opens_a_reachable_menu(
            cell,
            reach,
            move |this, at, cx| match chooses {
                Some(chooses) => artist_menu_among(at, id, favourite, chooses, this, cx),
                None => artist_menu_favoured(at, id, favourite),
            },
            cx,
        )
    }

    pub(crate) fn artist_found_cell(
        &self,
        found: &ArtistFound,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let cell_id = listing::keyed_by("artist-found", &found.mbid);
        let pointed = pointed::is_pointed_at(&cell_id);
        let opening = found.clone();
        let being_opened = self.library.read(cx).is_opening_artist(&found.mbid);

        div()
            .id(cell_id.clone())
            .listed_as(|| SharedString::from(found.name.clone()))
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap_2p5()
            .w(px(side * ARTIST_FOUND_CELL_WIDENING))
            .cursor_pointer()
            .names(words::browser::open_artist_found_hint())
            .child(
                kit::avatar(&found.name, false)
                    .size(px(side))
                    .text_size(px(theme::text_title())),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(theme::text_sm()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if pointed {
                        theme::accent()
                    } else {
                        theme::text()
                    }))
                    .text_center()
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(found.name.clone())),
            )
            .when(being_opened, |cell| {
                cell.child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .text_color(rgb(theme::accent()))
                        .child(words::browser::opening()),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_artist_found(opening.clone(), cx);
            }))
    }
}

pub(super) fn saying(drawn: ArtistsDrawn) -> &'static str {
    match drawn {
        ArtistsDrawn::List => words::browser::artists_list_hint(),
        ArtistsDrawn::Grid => words::browser::artists_grid_hint(),
    }
}

const AVATAR_LETTER: f32 = 0.36;

pub(crate) fn portrait_frame(art: Picture, side: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .size(px(side))
        .rounded(px(side / 2.0))
        .overflow_hidden()
        .bg(rgb(theme::raised()))
        .child(
            img(art)
                .size(px(side))
                .object_fit(ObjectFit::Cover)
                .rounded(px(side / 2.0)),
        )
}
