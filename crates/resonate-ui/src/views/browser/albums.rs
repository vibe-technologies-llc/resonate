use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, FontWeight, Pixels, SharedString,
    Stateful, div, prelude::*, px, rgb, uniform_list,
};
use resonate_core::ArtistId;
use resonate_library::{Album, Column, Favoured, Lit};

use super::{
    Chooses,
    grid::CELL_GROUP,
    menus::{album_menu, album_menu_ending, chosen_albums_menu},
    rows::{cursor_ring, reached_ring},
};
use crate::{
    Drawn, Selection,
    held::Rows,
    icons::Icon,
    theme,
    views::{
        hint::Names,
        kit,
        kit::{Casts as _, EndsInAnEllipsis},
        listing,
        listing::Pictured,
        menu, pointed,
        pointed::LitUnderThePointer,
        reorder,
        reorder::{Lifted, Listed, Shift},
        root::RootView,
        scrollbar::Scrollbars,
        sorting::Ordered,
        spoken,
        spoken::Spoken,
    },
    words,
};

const CAPTION_DOT: &str = "·";
const CAPTION_DOT_PADDING: f32 = 4.0;
const CAPTION_ROOM_SPARE: f32 = 1.0;

fn caption_font() -> gpui::Font {
    theme::ui(FontWeight::NORMAL)
}

fn artist_room_beside(side: f32, beside: &str, cx: &App) -> Pixels {
    let font = caption_font();
    let size = px(theme::text_xs());

    let taken = kit::width_of(CAPTION_DOT, &font, size, cx)
        + px(2.0 * CAPTION_DOT_PADDING)
        + kit::width_of(beside, &font, size, cx)
        + px(CAPTION_ROOM_SPARE);

    px(side) - taken
}

impl RootView {
    pub(crate) fn albums(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let albums = library.albums();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();
        let nothing = albums.is_empty();
        let unreadable = library.unreadable();
        let counted = library.albums_counted() as usize;
        let listed = library.albums_listed();
        let beyond = self.albums_found_beyond(cx);

        let found_nothing = (nothing && beyond.is_none()).then(|| {
            self.nothing_beyond(cx).unwrap_or_else(|| {
                self.nothing_matched(
                    Icon::Albums,
                    if unreadable {
                        words::browser::library_unreadable()
                    } else if narrowed {
                        words::browser::no_albums_match()
                    } else {
                        words::browser::no_albums_yet()
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
        let columns = self.grid_columns();
        let held = albums.len();
        let rows = listed.div_ceil(columns.max(1));
        let measured = self.rooms.grid_width.clone();
        let laid_out = self.rooms.grid_width.get() > px(0.0);

        if laid_out {
            self.land_where_it_was_left(rows, cx);
        }

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
                            .child(kit::title(words::browser::albums()))
                            .child(kit::subtitle(words::browser::albums_counted(counted))),
                    )
                    .when(!reads.is_empty(), |row| row.child(listing::reads(&reads)))
                    .child(kit::actions().child(self.orders_a_listing(
                        "order-albums",
                        Ordered::Albums,
                        cx,
                    ))),
            )
        });

        kit::pane()
            .child(heading)
            .when_some(found_nothing, |pane, nothing| pane.child(nothing))
            .when(!nothing, |pane| {
                pane.child(
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
                                    "albums",
                                    rows,
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _, cx| {
                                            this.reach_further(range.end * columns, held, cx);

                                            let mut drawn = Vec::new();

                                            for index in range {
                                                let first = index * columns;
                                                let cells = albums
                                                    .iter()
                                                    .enumerate()
                                                    .skip(first)
                                                    .take(columns)
                                                    .map(|(at, album)| {
                                                        this.album_cell(album, at, cx)
                                                    });

                                                drawn.push(
                                                    div()
                                                        .flex()
                                                        .gap(px(theme::grid_gap()))
                                                        .px_6()
                                                        .h(px(this.grid_row()))
                                                        .children(cells),
                                                );
                                            }
                                            drawn
                                        },
                                    ),
                                )
                                .track_scroll(&self.scrolls(cx).album_rows)
                                .h_full()
                                .w_full(),
                            )
                        })
                        .child(
                            Scrollbars::of(cx)
                                .vertical("album-scrollbar", self.scrolls(cx).album_rows.clone()),
                        ),
                )
            })
            .when_some(beyond, Div::child)
            .into_any_element()
    }

    fn albums_found_beyond(&self, cx: &mut Context<Self>) -> Option<Div> {
        let found = self.library.read(cx).albums_found();
        (self.search_in_front(cx).is_some() && !found.is_empty())
            .then(|| self.albums_found_strip(&found, None, cx))
    }

    fn album_cell(&self, album: &Album, at: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        self.album_cell_captioned(
            album,
            self.grid_side(),
            Caption::ByArtist,
            Some(Chooses::among(Listed::Albums, at)),
            cx,
        )
    }

    pub(crate) fn album_cell_at(
        &self,
        album: &Album,
        side: f32,
        chooses: Option<Chooses>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.album_cell_captioned(album, side, Caption::ByArtist, chooses, cx)
    }

    pub(super) fn album_cell_captioned(
        &self,
        album: &Album,
        side: f32,
        caption: Caption,
        chooses: Option<Chooses>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = album.id;
        let reached = chooses.is_some_and(|at| self.reaches(at.shift, at.row, cx));
        let reach = chooses.and_then(|at| self.at_the_reach(at.shift, at.row, cx));
        let cursor_only = chooses.is_some_and(|at| self.stands_unchosen_on(at.shift, at.row, cx));
        let (lit_title, lit_artist) = {
            let search = self.library.read(cx).search();
            (
                search.lit(&album.title, Column::Album),
                album
                    .artist
                    .as_deref()
                    .map_or_else(Lit::new, |name| search.lit(name, Column::Artist)),
            )
        };
        let cover = self.cover_sized(Pictured::Album(id), Drawn::InAGrid, side, cx);
        let owner = album.artist.as_ref().and(album.artist_id);
        let favourite = self
            .library
            .read(cx)
            .favours(Favoured::Album(id), album.favourite.is_some());
        let artist = album
            .artist
            .clone()
            .map(SharedString::from)
            .or_else(|| {
                (album.artist_count > 1)
                    .then(|| SharedString::new_static(words::browser::several()))
            })
            .filter(|_| match caption {
                Caption::ByArtist => true,
                Caption::Beside(artist) => album.artist_id != Some(artist),
            });
        let counted = words::browser::tracks_counted(album.track_count);

        let beside = match (caption, artist.is_some(), album.year) {
            (Caption::Beside(_), false, Some(year)) => {
                Some(SharedString::from(format!("{year} · {counted}")))
            }
            (_, _, Some(year)) => Some(SharedString::from(year.to_string())),
            (_, true, None) => None,
            (_, false, None) => Some(SharedString::from(counted)),
        };
        let parted = artist.is_some() && beside.is_some();
        let artist_room = beside
            .as_ref()
            .map(|beside| artist_room_beside(side, beside, cx));

        let under = div()
            .flex()
            .min_w(px(0.0))
            .text_size(px(theme::text_xs()))
            .text_color(rgb(theme::muted()))
            .when_some(artist, |line, artist| {
                let named = match artist_room {
                    Some(room) => listing::matched_within(
                        kit::cut_to_fit(artist, room, caption_font(), px(theme::text_xs()), cx),
                        lit_artist,
                    ),
                    None => listing::matched(artist, lit_artist),
                };
                line.child(
                    self.opens(
                        ("album-artist", id.get() as usize),
                        named,
                        words::browser::open_artist_hint(),
                        owner.map(Selection::Artist),
                        cx,
                    )
                    .debug_selector(move || format!("album-artist-{}", id.get()))
                    .ends_in_an_ellipsis(),
                )
            })
            .when(parted, |line| {
                line.child(
                    div()
                        .flex_none()
                        .px(px(CAPTION_DOT_PADDING))
                        .child(CAPTION_DOT),
                )
            })
            .when_some(beside, |line, beside| {
                line.child(div().flex_none().whitespace_nowrap().child(beside))
            });

        let cell_id = ElementId::from(("album-cell", id.get() as usize));
        let pointed = pointed::is_pointed_at(&cell_id);

        let cell = div()
            .id(cell_id.clone())
            .debug_selector(move || format!("album-{}", id.get()))
            .listed_as(|| spoken::song(&album.title, album.artist.as_deref()))
            .chosen(reached)
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(words::browser::open_album_hint())
            .child(
                div()
                    .relative()
                    .rounded(px(8.0))
                    .group_hover(CELL_GROUP, |frame| frame.casting(kit::Cast::Short))
                    .child(cover)
                    .when(reached, |frame| frame.child(reached_ring()))
                    .when(cursor_only, |frame| frame.child(cursor_ring()))
                    .child(
                        self.favour_mark_under(
                            ("album-favourite", id.get() as usize),
                            Favoured::Album(id),
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
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if pointed {
                                theme::accent()
                            } else {
                                theme::text()
                            }))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(listing::matched(
                                SharedString::from(album.title.clone()),
                                lit_title,
                            )),
                    )
                    .child(under),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if let Some(at) = chooses
                    && this.reach_by_click(at.shift, at.row, event.modifiers(), cx)
                {
                    return;
                }
                this.opened(Selection::Album(id), cx);
            }));

        let lifted = match chooses {
            Some(at) => {
                let chosen = self.acting_on_chosen(at.shift, at.row, cx);
                if chosen.count() > 1 {
                    let page = match at.shift {
                        Shift::Listing(Listed::FavouriteAlbums) => {
                            Rows::from(self.library.read(cx).favourite_albums())
                        }
                        _ => self.library.read(cx).albums(),
                    };
                    Lifted::albums(&page, chosen)
                } else {
                    Lifted::album(id, &album.title)
                }
            }
            None => Lifted::album(id, &album.title),
        };
        let cell = reorder::liftable(cell, lifted);

        menu::opens_a_reachable_menu(
            cell,
            reach,
            move |this, at, cx| {
                if let Some(row) = chooses {
                    let chosen = this.acting_on_chosen(row.shift, row.row, cx);
                    if chosen.count() > 1 {
                        let albums = this.albums_chosen(row.shift, &chosen, cx);

                        return chosen_albums_menu(at, row.shift, albums, this, cx);
                    }
                }
                album_menu_ending(
                    album_menu(at, id)
                        .apart()
                        .favours(Favoured::Album(id), favourite),
                    id,
                    owner,
                )
            },
            cx,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Caption {
    ByArtist,
    Beside(ArtistId),
}
