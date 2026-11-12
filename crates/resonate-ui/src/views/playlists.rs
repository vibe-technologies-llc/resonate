use std::{env, path::PathBuf, rc::Rc, sync::Arc, time::SystemTime};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, FontWeight, MouseButton,
    PathPromptOptions, Pixels, Point, Role, SharedString, Stateful, div, prelude::*, px, rgb,
    uniform_list,
};
use resonate_core::{AlbumId, FrameSpan, MediaLocation, PlaylistId, Span};
use resonate_engine::{Command, Placement, QueueItem, QueueRows, Queued, Unclaimed};
use resonate_library::{
    Cut, Edit, Favoured, Lit, Maker, Playlist, PlaylistEntry, PlaylistSource, Undoable, Upkeep,
};
use smallvec::smallvec;

use crate::{
    Drawn, Notice, ResonateApp, Selection, format,
    icons::{self, Icon},
    models::Picture,
    motion, theme,
    views::{
        browser,
        hint::{self, Names},
        kit::{self, Casts as _, EndsInAnEllipsis, Tone},
        listing::{self, Pictured},
        menu::{self, Called, Menu},
        mosaic::{self, Mosaic},
        pointed::{self, LitUnderThePointer as _},
        reorder::{self, Carried, Chosen, LiftedTo, Listed, Shift, Step},
        root::{Pane, RootView, empty, listed, row, somewhere_in, tall_row},
        scrollbar::Scrollbars,
        sheets,
        sorting::{self, Ordered, Ordering as _},
        spoken::{self, Placed, Spoken},
    },
    words,
};

const LAST_RESORT_FOLDER: &str = "/";

const PLAYLIST_CELL: &str = "playlist-cell";

pub(crate) const ROW_GROUP: &str = "listed-row";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Held {
    pub(crate) rows: Arc<[Cut]>,
    pub(crate) taken_from: Option<PlaylistId>,
}

impl Held {
    pub(crate) fn of(rows: Arc<[Cut]>) -> Self {
        Self {
            rows,
            taken_from: None,
        }
    }

    pub(crate) fn out_of(playlist: PlaylistId, rows: Arc<[Cut]>) -> Self {
        Self {
            rows,
            taken_from: Some(playlist),
        }
    }

    fn asked_as(&self) -> SharedString {
        let held = self.rows.len();
        SharedString::from(match self.taken_from {
            Some(_) => words::playlists::copy_into(held),
            None => words::playlists::add_to(held),
        })
    }
}

pub(crate) struct Offered {
    lists: Arc<[Playlist]>,
    without: Option<usize>,
}

impl Offered {
    pub(crate) fn beside(lists: Arc<[Playlist]>, taken_from: Option<PlaylistId>) -> Self {
        let without =
            taken_from.and_then(|taken| lists.iter().position(|offered| offered.id == taken));

        Self { lists, without }
    }

    pub(crate) fn len(&self) -> usize {
        self.lists.len() - usize::from(self.without.is_some())
    }

    pub(crate) fn row(&self, index: usize) -> Option<&Playlist> {
        let stepped = match self.without {
            Some(without) if index >= without => index + 1,
            _ => index,
        };

        self.lists.get(stepped)
    }
}

pub(crate) struct Reaching {
    rows: Chosen,
    entries: Arc<[PlaylistEntry]>,
    holding: Arc<[Cut]>,
}

impl Reaching {
    fn of(entries: &[PlaylistEntry], rows: Chosen) -> Self {
        let reached: Arc<[PlaylistEntry]> = rows
            .rows()
            .filter_map(|row| entries.get(row).cloned())
            .collect();
        let holding = reached.iter().map(|entry| entry.cut.clone()).collect();

        Self {
            rows,
            entries: reached,
            holding,
        }
    }

    fn acting_on<'a>(reaching: Option<&'a Self>, rows: &Chosen) -> Option<&'a Self> {
        reaching.filter(|reaching| reaching.rows == *rows)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Placing {
    playlist: PlaylistId,
    index: usize,
    playing: Option<usize>,
    rows: Rows,
    narrowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rows {
    InHand,
    Kept,
    Narrowed,
    Matched,
}

impl Rows {
    pub(crate) fn of(playlist: &Playlist, narrowed: bool) -> Self {
        match (playlist.query.is_some(), playlist.kept.is_some(), narrowed) {
            (true, _, _) => Self::Matched,
            (false, _, true) => Self::Narrowed,
            (false, true, false) => Self::Kept,
            (false, false, false) => Self::InHand,
        }
    }

    pub(crate) const fn are_moved(self) -> bool {
        matches!(self, Self::InHand)
    }

    pub(crate) const fn are_reached(self) -> bool {
        matches!(self, Self::InHand | Self::Kept)
    }

    pub(crate) const fn are_edited(self) -> bool {
        !matches!(self, Self::Matched)
    }

    pub(crate) const fn control_count(self) -> usize {
        match self {
            Self::InHand => 6,
            Self::Kept | Self::Narrowed => 4,
            Self::Matched => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlaylistsDrawn {
    #[default]
    Grid,
    List,
}

impl PlaylistsDrawn {
    fn saying(self) -> &'static str {
        match self {
            Self::Grid => words::playlists::grid_hint(),
            Self::List => words::playlists::list_hint(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Naming {
    New,
    Query(Option<PlaylistId>),
    Rename(PlaylistId),
}

impl Naming {
    pub(crate) const fn playlist(self) -> Option<PlaylistId> {
        match self {
            Self::New | Self::Query(None) => None,
            Self::Query(Some(playlist)) | Self::Rename(playlist) => Some(playlist),
        }
    }

    pub(crate) const fn is_a_search(self) -> bool {
        matches!(self, Self::Query(_))
    }
}

const CAPS: [Option<usize>; 4] = [None, Some(25), Some(50), Some(100)];

fn keeping_choices() -> [(&'static str, bool, &'static str); 2] {
    [
        (
            words::playlists::just_once(),
            false,
            words::playlists::once_hint(),
        ),
        (
            words::playlists::from_now_on(),
            true,
            words::playlists::always_hint(),
        ),
    ]
}

fn playing_entry(queued: &Queued, loaded: usize, entries: &[PlaylistEntry]) -> Option<usize> {
    let drawn = queued.rows.loaded_at().position(|at| at == loaded)?;
    let playing = queued.rows.get(drawn)?;
    let is_the_cut = |location: &MediaLocation, span: Option<FrameSpan>| {
        *location == playing.location && span == playing.span
    };
    let heard_before = queued
        .rows
        .iter()
        .zip(queued.rows.loaded_at())
        .filter(|(row, at)| *at < loaded && is_the_cut(&row.location, row.span))
        .count();

    let mut same: Vec<(usize, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| is_the_cut(entry.location(), entry.span()))
        .map(|(index, entry)| (entry.position, index))
        .collect();
    same.sort_unstable();
    same.get(heard_before)
        .or_else(|| same.last())
        .map(|(_, index)| *index)
}

pub(crate) fn queue_items(entries: &[PlaylistEntry], beside: &QueueRows) -> Vec<QueueItem> {
    let mut minting = Unclaimed::beside(beside.iter().map(Arc::as_ref));

    entries
        .iter()
        .map(|entry| QueueItem {
            id: entry
                .track
                .as_ref()
                .map_or_else(|| minting.mint(), |track| track.id),
            location: entry.location().clone(),
            span: entry.span(),
        })
        .collect()
}

impl RootView {
    pub(crate) fn playlists_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.social.shared_lists = cx
            .global::<ResonateApp>()
            .together
            .as_ref()
            .map(|together| together.shared_playlists())
            .unwrap_or_default();

        match self.library.read(cx).opened() {
            Some(opened) => self.one_playlist(opened, cx),
            None => self.every_playlist(cx),
        }
    }

    fn every_playlist(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let saved = self.library.read(cx).saved_playlists();
        let playing = self.playing_playlist(cx);
        let narrowed = self.library.read(cx).narrowing().is_some();
        let heading = self.playlists_heading(narrowed || !saved.is_empty(), cx);

        let listed = if saved.is_empty() {
            if narrowed {
                empty(
                    Icon::Playlists,
                    words::playlists::no_playlists_match(),
                    Some(words::playlists::none_named_that()),
                )
            } else {
                kit::empty_offering(
                    Icon::Playlists,
                    words::playlists::no_playlists_yet(),
                    Some(words::playlists::how_to_start()),
                    self.first_playlist_offers(cx),
                )
            }
        } else {
            match self.shows.playlists {
                PlaylistsDrawn::Grid => self.playlist_grid(saved, playing, cx),
                PlaylistsDrawn::List => Scrollbars::of(cx)
                    .around(
                        "playlists-scrollbar",
                        self.scrolls.all_playlist_rows.clone(),
                        uniform_list(
                            "playlists",
                            saved.len(),
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                let now = this.drawn_at();
                                let mut rows = Vec::new();
                                for index in range {
                                    let Some(playlist) = saved.get(index) else {
                                        continue;
                                    };
                                    rows.push(this.playlist_row(
                                        playlist,
                                        index,
                                        playing == Some(playlist.id),
                                        now,
                                        cx,
                                    ));
                                }
                                rows
                            }),
                        )
                        .track_scroll(&self.scrolls.all_playlist_rows)
                        .h_full()
                        .w_full(),
                    )
                    .into_any_element(),
            }
        };

        kit::pane().child(heading).child(listed).into_any_element()
    }

    fn first_playlist_offers(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .gap_2()
            .child(self.in_the_pane_ring(
                kit::button(
                    "first-playlist",
                    Some(Icon::Plus),
                    words::playlists::new_playlist(),
                    words::playlists::new_hint(),
                    Tone::Outlined,
                ),
                |this, window, cx| {
                    this.name_a_playlist(Naming::New, window, cx);
                },
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::button(
                    "first-import",
                    Some(Icon::Import),
                    words::playlists::import(),
                    words::playlists::import_hint(),
                    Tone::Outlined,
                ),
                |this, _, cx| this.import_playlists(cx),
                cx,
            ))
    }

    fn playlist_grid(
        &mut self,
        saved: Arc<[Playlist]>,
        playing: Option<PlaylistId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let columns = self.grid_columns();
        let held = saved.len();
        let rows = held.div_ceil(columns.max(1));
        let measured = self.rooms.grid_width.clone();
        let laid_out = self.rooms.grid_width.get() > px(0.0);

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
                        "playlist-grid",
                        rows,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            let now = this.drawn_at();
                            range
                                .map(|index| {
                                    let tall = this.grid_row();
                                    let cells = saved
                                        .iter()
                                        .enumerate()
                                        .skip(index * columns)
                                        .take(columns)
                                        .map(|(at, playlist)| {
                                            this.playlist_cell(
                                                playlist,
                                                at,
                                                playing == Some(playlist.id),
                                                now,
                                                cx,
                                            )
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
                    .track_scroll(&self.scrolls.all_playlist_rows)
                    .h_full()
                    .w_full(),
                )
            })
            .child(Scrollbars::of(cx).vertical(
                "playlist-grid-scrollbar",
                self.scrolls.all_playlist_rows.clone(),
            ))
            .into_any_element()
    }

    fn playlists_drawn_as(&self, cx: &mut Context<Self>) -> Div {
        let drawn = self.shows.playlists;
        let choice = |as_: PlaylistsDrawn, label: &'static str, cx: &mut Context<Self>| {
            self.in_the_pane_ring(
                kit::segment(("playlists-drawn", as_ as usize), label, drawn == as_)
                    .names(as_.saying()),
                move |this, _, cx| {
                    this.shows.playlists = as_;
                    cx.notify();
                },
                cx,
            )
        };

        kit::segmented()
            .child(choice(PlaylistsDrawn::Grid, words::playlists::grid(), cx))
            .child(choice(PlaylistsDrawn::List, words::playlists::list(), cx))
    }

    fn playlists_heading(&self, any_saved: bool, cx: &mut Context<Self>) -> Div {
        let naming = self.naming.filter(|naming| !naming.is_a_search());
        let library = self.library.read(cx);
        let reads = library.search().reads();
        let undoable = library.undoable();
        let redoable = library.redoable();
        let saved = library.playlists().len();

        let actions = kit::actions()
            .when_some(undoable, |bar, undoable| {
                bar.child(self.undoing(&undoable, cx))
            })
            .when_some(redoable, |bar, redoable| {
                bar.child(self.redoing(&redoable, cx))
            })
            .when(any_saved, |bar| {
                bar.child(self.playlists_drawn_as(cx))
                    .child(self.orders_a_listing("playlists-sort", Ordered::Playlists, cx))
            })
            .child(self.in_the_pane_ring(
                kit::icon_button(
                    "import-playlists",
                    Icon::Import,
                    words::playlists::import_hint(),
                ),
                |this, _, cx| this.import_playlists(cx),
                cx,
            ))
            .when(naming.is_none(), |bar| {
                bar.child(self.in_the_pane_ring(
                    kit::button(
                        "new-playlist",
                        Some(Icon::Plus),
                        words::playlists::new_playlist(),
                        words::playlists::new_hint(),
                        Tone::Primary,
                    ),
                    |this, window, cx| {
                        this.name_a_playlist(Naming::New, window, cx);
                    },
                    cx,
                ))
            });

        kit::heading()
            .child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(theme::heading_name()))
                            .gap_1()
                            .child(kit::title(words::playlists::playlists()))
                            .when(saved > 0, |column| {
                                column.child(kit::subtitle(words::playlists::playlists_counted(
                                    saved,
                                )))
                            }),
                    )
                    .child(actions),
            )
            .when(!reads.is_empty(), |pane| pane.child(listing::reads(&reads)))
            .when_some(naming, |pane, naming| {
                pane.child(self.naming_row(naming, cx))
            })
    }

    pub(crate) fn playlists_order(&self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let held = library.playlist_order();
        let reading = library.playlist_reading();

        self.order_choices(
            held,
            reading,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_playlists(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_playlists(reading, cx));
            },
            cx,
        )
    }

    pub(crate) fn naming_row(&self, naming: Naming, cx: &mut Context<Self>) -> Div {
        let label = match naming {
            Naming::New => words::playlists::name_it(),
            Naming::Query(None) => words::playlists::name_this_search(),
            Naming::Query(Some(_)) => words::playlists::change_this_search(),
            Naming::Rename(_) => words::playlists::rename_it(),
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_lg()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(kit::eyebrow(label).w(theme::width(theme::field_label())))
                    .child(self.name_field(cx)),
            )
            .when(naming.is_a_search(), |row| row.child(self.search_shape(cx)))
    }

    fn search_shape(&self, cx: &mut Context<Self>) -> Div {
        let cap = self.orders.query_cap;

        let mut caps = kit::segmented();
        for (index, held) in CAPS.into_iter().enumerate() {
            caps = caps.child(self.in_the_pane_ring(
                kit::segment(("search-cap", index), capped(held), held == cap),
                move |this, _, cx| this.cap_a_search(held, cx),
                cx,
            ));
        }
        let ordered_as = format!(
            "{} · {}",
            self.orders.query_sort.named(),
            self.orders.query_sort.read(self.orders.query_reading)
        );

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                sorting::shape(words::playlists::order()).child(self.in_the_pane_ring_at(
                    kit::pick(
                        "search-order",
                        ordered_as,
                        words::playlists::search_order_hint(),
                    ),
                    |this, at, cx| this.order_a_listing(Ordered::SavedSearch, at, cx),
                    cx,
                )),
            )
            .child(sorting::shape(words::playlists::rows()).child(caps))
            .when_some(cap.filter(|_| !CAPS.contains(&cap)), |shape, held| {
                shape.child(div().text_xs().text_color(rgb(theme::faint())).child(
                    SharedString::from(words::playlists::cap_is_none_of_these(held)),
                ))
            })
    }

    pub(crate) fn search_in_order(&self, cx: &mut Context<Self>) -> Div {
        self.order_choices(
            self.orders.query_sort,
            self.orders.query_reading,
            |this, order, cx| this.sort_a_search(order, cx),
            |this, reading, cx| this.read_a_search(reading, cx),
            cx,
        )
    }

    pub(crate) fn name_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::framed_field("playlist-name", true)
            .flex_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.typed
                    .name
                    .update(cx, |name, cx| name.take_focus(window, cx));
                cx.notify();
            }))
            .child(self.typed.name.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
    }

    fn one_playlist(&mut self, opened: PlaylistId, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let entries = library.entries();
        let narrowed = library.narrowing().is_some();
        let rows = library
            .opened_playlist()
            .map_or(Rows::InHand, |held| Rows::of(held, narrowed));
        let heading = self.playlist_heading(opened, &entries, cx);
        let playing = self.playing_row(opened, &entries, cx);
        let reaching = rows
            .are_reached()
            .then(|| self.reached_chosen(Shift::Playlist(opened)))
            .flatten()
            .map(|reached| Reaching::of(&entries, reached));

        let listed = if entries.is_empty() {
            self.nothing_in_the_playlist(opened, rows, cx)
        } else {
            let held = Arc::clone(&entries);
            let scroll = self.scrolls.playlist_rows.clone();
            let listing = uniform_list(
                "playlist-entries",
                entries.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    let mut listed = Vec::new();
                    for index in range {
                        let Some(entry) = held.get(index) else {
                            continue;
                        };
                        listed.push(this.entry_row(
                            Placing {
                                playlist: opened,
                                index,
                                playing,
                                rows,
                                narrowed,
                            },
                            &held,
                            reaching.as_ref(),
                            entry,
                            cx,
                        ));
                    }
                    listed
                }),
            )
            .track_scroll(&scroll)
            .h_full()
            .w_full();

            reorder::follows_a_drag(
                div()
                    .id("playlist-rows")
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0)),
                scroll,
                entries.len(),
                cx,
            )
            .child(listing)
            .child(
                Scrollbars::of(cx)
                    .vertical("playlist-scrollbar", self.scrolls.playlist_rows.clone()),
            )
            .into_any_element()
        };

        kit::pane()
            .child(heading)
            .when(!entries.is_empty(), |pane| {
                pane.child(listing::columns(
                    "#",
                    true,
                    rows.control_count(),
                    sorting::playlist_sorted(self),
                    &self.rooms.columns_fit,
                    &self.controls,
                    cx,
                ))
            })
            .child(div().flex().flex_1().min_h(px(0.0)).child(listed))
            .into_any_element()
    }

    fn nothing_in_the_playlist(
        &self,
        opened: PlaylistId,
        rows: Rows,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match rows {
            Rows::Narrowed => empty(
                Icon::Playlists,
                words::playlists::no_songs_match(),
                Some(words::playlists::nothing_here_matches()),
            ),
            Rows::Matched => kit::empty_offering(
                Icon::Search,
                words::playlists::nothing_matches_yet(),
                Some(words::playlists::nothing_matches_the_search_yet()),
                self.in_the_pane_ring(
                    kit::button(
                        "revise-an-empty-search",
                        Some(Icon::Search),
                        words::playlists::edit_search(),
                        words::playlists::fills_itself_hint(),
                        Tone::Outlined,
                    ),
                    move |this, window, cx| {
                        this.revise_search(opened, window, cx);
                    },
                    cx,
                ),
            ),
            Rows::InHand | Rows::Kept => kit::empty_offering(
                Icon::Playlists,
                words::playlists::this_playlist_is_empty(),
                Some(words::playlists::how_to_fill()),
                self.in_the_pane_ring(
                    kit::button(
                        "fill-an-empty-playlist",
                        Some(Icon::Plus),
                        words::playlists::add_songs(),
                        words::playlists::add_songs_hint(),
                        Tone::Primary,
                    ),
                    move |this, _, cx| {
                        this.add_songs_to_playlist(opened, cx);
                    },
                    cx,
                ),
            ),
        }
    }

    fn playlist_heading(
        &self,
        opened: PlaylistId,
        entries: &Arc<[PlaylistEntry]>,
        cx: &mut Context<Self>,
    ) -> Div {
        let library = self.library.read(cx);
        let named = library.opened_playlist().cloned();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();
        let naming = self.naming.filter(|naming| !naming.is_a_search());
        let rows = named
            .as_ref()
            .map_or(Rows::InHand, |held| Rows::of(held, narrowed));
        let shown = entries.len();
        let undoable = library.undoable();
        let redoable = library.redoable();
        let albums = library.pictured(opened);
        let pinned = named.as_ref().is_some_and(|named| named.pinned.is_some());
        let history = named
            .as_ref()
            .map(|named| history_of(named, self.drawn_at()));
        let playlist_name = named.as_ref().map_or_else(
            || words::playlists::unnamed(opened.to_string()),
            |named| named.name.clone(),
        );
        let name = SharedString::from(playlist_name.clone());
        let under = named.as_ref().map_or_else(String::new, |named| {
            let mut parts: format::Parts<String> = smallvec![if narrowed {
                shown_of(shown, named).to_string()
            } else {
                counted(named).to_string()
            }];
            match rows {
                Rows::Matched => parts.push(words::playlists::fills_itself().to_owned()),
                Rows::Kept => parts.push(words::playlists::kept_in_order().to_owned()),
                Rows::Narrowed => parts.push(words::playlists::narrowed_by_the_search().to_owned()),
                Rows::InHand => {}
            }
            if let Some(source) = named.source.as_ref() {
                parts.push(came_from(source));
            }
            if named.plays > 0 {
                parts.push(listing::times(named.plays).to_string());
            }
            parts.join(" · ")
        });
        let eyebrow = match rows {
            Rows::Matched => words::playlists::saved_search(),
            Rows::InHand | Rows::Kept | Rows::Narrowed => words::playlists::playlist(),
        };

        let normal = named.as_ref().is_none_or(|named| named.query.is_none());
        let actions = kit::action_row()
            .w_full()
            .pt_2()
            .when(!entries.is_empty(), |bar| {
                let played = Arc::clone(entries);
                bar.child(self.in_the_pane_ring(
                    kit::button(
                        "play-playlist",
                        Some(Icon::Play),
                        words::playlists::play(),
                        words::playlists::play_hint(),
                        Tone::Primary,
                    ),
                    move |this, _, cx| {
                        this.plays_in_order(cx);
                        this.play_playlist(opened, &played, 0, !narrowed, cx);
                    },
                    cx,
                ))
                .child({
                    let played = Arc::clone(entries);
                    self.in_the_pane_ring(
                        kit::button(
                            "shuffle-playlist",
                            Some(Icon::Shuffle),
                            words::playlists::shuffle(),
                            words::playlists::shuffle_hint(),
                            Tone::Outlined,
                        ),
                        move |this, _, cx| {
                            this.play_playlist(
                                opened,
                                &played,
                                somewhere_in(played.len()),
                                !narrowed,
                                cx,
                            );
                            this.send(Command::SetShuffle(true), cx);
                        },
                        cx,
                    )
                })
            })
            .when(naming.is_none() && !rows.are_edited(), |bar| {
                bar.child(self.in_the_pane_ring(
                    kit::button(
                        "revise-search",
                        Some(Icon::Search),
                        words::playlists::edit_search(),
                        words::playlists::fills_itself_hint(),
                        Tone::Ghost,
                    ),
                    move |this, window, cx| {
                        this.revise_search(opened, window, cx);
                    },
                    cx,
                ))
            })
            .when(normal && naming.is_none(), |bar| {
                bar.child(self.in_the_pane_ring(
                    kit::icon_button(
                        "add-songs-to-playlist",
                        Icon::Plus,
                        words::playlists::add_songs_hint(),
                    ),
                    move |this, _, cx| {
                        this.add_songs_to_playlist(opened, cx);
                    },
                    cx,
                ))
            })
            .when(
                matches!(rows, Rows::Narrowed) && !entries.is_empty(),
                |bar| {
                    bar.child(self.in_the_pane_ring(
                        kit::button(
                            "drop-shown",
                            Some(Icon::Discard),
                            words::playlists::drop_shown(),
                            words::playlists::drop_shown_hint(),
                            Tone::Ghost,
                        ),
                        move |this, _, cx| {
                            this.library
                                .update(cx, |library, cx| library.remove_matching(opened, cx));
                        },
                        cx,
                    ))
                },
            )
            .when(normal && !entries.is_empty(), |bar| {
                bar.child(self.in_the_pane_ring(
                    kit::icon_button("tidy-playlist", Icon::Tidy, words::playlists::tidy_hint()),
                    move |this, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.tidy_playlist(opened, cx));
                    },
                    cx,
                ))
                .child(self.in_the_pane_ring_at(
                    kit::icon_button("sort-playlist", Icon::Sort, words::playlists::sort_hint()),
                    move |this, at, cx| this.sort_a_playlist(opened, at, cx),
                    cx,
                ))
            })
            .when_some(
                named.as_ref().and_then(|named| named.source.clone()),
                |bar, source| {
                    let service = source.link.service();
                    let (label, saying, other) = match source.upkeep {
                        Upkeep::Once => (
                            words::playlists::keep_in_step(service),
                            words::playlists::keep_in_step_hint(service),
                            Upkeep::InStep,
                        ),
                        Upkeep::InStep => (
                            words::playlists::in_step(service),
                            words::playlists::in_step_hint(service),
                            Upkeep::Once,
                        ),
                    };
                    bar.child(self.in_the_pane_ring(
                        kit::button(
                            "keep-playlist-in-step",
                            Some(Icon::Link),
                            label,
                            saying,
                            match source.upkeep {
                                Upkeep::Once => Tone::Ghost,
                                Upkeep::InStep => Tone::Outlined,
                            },
                        ),
                        move |this, _, cx| {
                            this.library.update(cx, |library, cx| {
                                library.keep_playlist_from_its_link(opened, other, cx);
                            });
                        },
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(
                            "refresh-playlist",
                            Icon::Redo,
                            words::playlists::refresh_hint(),
                        ),
                        move |this, _, cx| {
                            this.library
                                .update(cx, |library, cx| library.refresh_playlist(opened, cx));
                        },
                        cx,
                    ))
                },
            )
            .when(named.is_some(), |bar| {
                bar.child(self.in_the_pane_ring(
                    match pinned {
                        true => kit::lit_mark(
                            "pin-playlist",
                            Icon::Pinned,
                            words::playlists::unpin_hint(),
                        ),
                        false => kit::icon_button(
                            "pin-playlist",
                            Icon::Pin,
                            words::playlists::pin_hint(),
                        ),
                    },
                    move |this, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.pin_playlist(opened, !pinned, cx));
                    },
                    cx,
                ))
                .child(self.in_the_pane_ring_at(
                    kit::icon_button(
                        "more-of-the-playlist",
                        Icon::More,
                        words::playlists::more_hint(),
                    ),
                    move |this, at, cx| {
                        this.open_a_menu(playlist_menu(at, opened, pinned, cx), cx);
                    },
                    cx,
                ))
            })
            .when_some(undoable, |bar, undoable| {
                bar.child(self.undoing(&undoable, cx))
            })
            .when_some(redoable, |bar, redoable| {
                bar.child(self.redoing(&redoable, cx))
            })
            .when(!entries.is_empty() && rows.are_edited(), |bar| {
                bar.child(hint::explains(
                    "playlist-moving",
                    match rows {
                        Rows::InHand => reorder::moving_hint(),
                        Rows::Narrowed => words::playlists::narrowed_hint().to_owned(),
                        Rows::Kept | Rows::Matched => words::playlists::kept_hint().to_owned(),
                    },
                ))
            });

        let name_selector = div()
            .group("playlist-title")
            .flex()
            .items_center()
            .gap_2()
            .min_w(px(0.0))
            .child(
                div()
                    .id("edit-playlist-title")
                    .flex_1()
                    .min_w(px(0.0))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.name_a_playlist(Naming::Rename(opened), window, cx);
                    }))
                    .child(kit::hero_title(name, self.rooms.hero_width.get())),
            )
            .child(
                self.in_the_pane_ring(
                    kit::icon_button(
                        "select-playlist-title",
                        Icon::Rename,
                        words::playlists::rename_hint(),
                    )
                    .opacity(0.0)
                    .group_hover("playlist-title", |mark| mark.opacity(1.0)),
                    move |this, window, cx| {
                        cx.stop_propagation();
                        this.name_a_playlist(Naming::Rename(opened), window, cx);
                    },
                    cx,
                ),
            );

        let art = self.playlist_art(
            &playlist_name,
            named.as_ref().is_some_and(|named| named.query.is_some()),
            &albums,
            self.hero_side(),
            cx,
        );
        let about = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .child(kit::measures_its_width(self.rooms.hero_width.clone()))
            .child(kit::measures_its_height(self.rooms.hero_height.clone()))
            .child(kit::eyebrow(eyebrow))
            .child(name_selector)
            .child(kit::subtitle(under))
            .when_some(history, |about, history| {
                about.child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .text_color(rgb(theme::faint()))
                        .child(history),
                )
            })
            .child(actions);

        kit::heading()
            .child(div().flex().child(self.in_the_pane_ring(
                kit::way_back(
                    words::playlists::all_playlists(),
                    words::playlists::playlists(),
                    words::playlists::back_hint(),
                ),
                |this, _, cx| {
                    this.show_playlist(None, cx);
                },
                cx,
            )))
            .child(kit::hero().child(art).child(about))
            .when(!reads.is_empty(), |pane| pane.child(listing::reads(&reads)))
            .when_some(naming, |pane, naming| {
                pane.child(self.naming_row(naming, cx))
            })
    }

    pub(crate) fn rows_in_order(&self, opened: PlaylistId, cx: &mut Context<Self>) -> Div {
        let keeping = self.orders.keeping;
        let mut keeps = kit::segmented().w_full();
        for (index, (label, wanted, saying)) in keeping_choices().into_iter().enumerate() {
            keeps = keeps.child(
                self.in_the_pane_ring(
                    kit::segment(
                        ("row-keeping", index),
                        SharedString::new_static(label),
                        wanted == keeping,
                    )
                    .flex_1()
                    .names(saying),
                    move |this, _, cx| {
                        this.keep_in_order(opened, wanted, cx);
                    },
                    cx,
                ),
            );
        }

        self.order_choices(
            self.orders.row_order,
            self.orders.row_reading,
            move |this, order, cx| this.put_in_order(opened, order, this.orders.row_reading, cx),
            move |this, reading, cx| this.put_in_order(opened, this.orders.row_order, reading, cx),
            cx,
        )
        .child(kit::rule().mx_1())
        .child(
            kit::field(words::playlists::keep_this_order(), keeps)
                .flex_none()
                .px_1()
                .pb_1(),
        )
    }

    pub(crate) fn playing_playlist(&self, cx: &App) -> Option<PlaylistId> {
        let queue = self.player.read(cx).state().queue_stamp;
        self.library.read(cx).playing_playlist(queue)
    }

    fn playing_row(
        &self,
        opened: PlaylistId,
        entries: &[PlaylistEntry],
        cx: &App,
    ) -> Option<usize> {
        if self.playing_playlist(cx) != Some(opened) {
            return None;
        }

        let player = self.player.read(cx);
        let loaded = player.state().loaded_position?;
        playing_entry(&player.queued(), loaded, entries)
    }

    fn entry_row(
        &mut self,
        placing: Placing,
        entries: &Arc<[PlaylistEntry]>,
        reaching: Option<&Reaching>,
        entry: &PlaylistEntry,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Placing {
            playlist: opened,
            index,
            playing,
            rows,
            narrowed,
        } = placing;
        let current = playing == Some(index);
        let drawn = match entry.track.clone() {
            Some(track) => {
                let favourite = self.library.read(cx).favours_track(&track);
                listing::scanned(track, favourite)
            }
            None => self
                .player
                .read(cx)
                .media(entry.location(), None)
                .map_or_else(
                    || listing::unread(entry.location()),
                    |info| listing::read(&info, entry.location()),
                ),
        };
        let cover = self.cover(
            Pictured::Track {
                album: drawn.album,
                file: entry.location(),
            },
            cx,
        );
        let shift = Shift::Playlist(opened);
        let reaches = rows.are_reached();
        let reached = reaches && self.reaches(shift, index);
        let acting_on = if reaches {
            self.acting_on_chosen(shift, index)
        } else {
            Chosen::of(Span::one(index))
        };
        let acts_on_one = acting_on.count() == 1;
        let position = entry.position;
        let dropping = if reaches {
            acting_on.clone()
        } else {
            Chosen::of(Span::one(position))
        };
        let carried = Carried {
            shift,
            rows: acting_on.clone(),
            title: drawn.title.clone(),
        };
        let held = Arc::clone(entries);
        let last = entries.len();
        let acted_on = Reaching::acting_on(reaching, &acting_on);
        let queueing: Arc<[PlaylistEntry]> = match acted_on {
            Some(reaching) => Arc::clone(&reaching.entries),
            None => Arc::from([entry.clone()]),
        };
        let holding: Arc<[Cut]> = match acted_on {
            Some(reaching) => Arc::clone(&reaching.holding),
            None => Arc::from([entry.cut.clone()]),
        };

        let menued = Arc::clone(&holding);
        let playing = held.clone();
        let album = drawn.album;
        let artist_id = drawn.artist_id;
        let scanned = drawn.track;
        let versioned = entry.track.clone();
        let favourite = drawn.favourite;
        let location = entry.cut.location.clone();
        let span = entry.span();
        let title = drawn.title.clone();
        let artist = drawn.artist.clone();
        let fitted = self.rooms.columns_fit.shown();
        let entry_id = listing::keyed_by("entry", &(&entry.cut, index));
        let listed = row(current)
            .id(entry_id)
            .debug_selector(move || format!("entry-{index}"))
            .listed_as(|| spoken::song(&title, Some(&artist)))
            .placed(Placed {
                at: index,
                of: last,
            })
            .chosen(reached)
            .group(ROW_GROUP)
            .cursor_pointer()
            .hover(|entry| entry.bg(rgb(theme::hover())))
            .child(if current {
                listing::playing_mark()
            } else {
                listing::number_cell(SharedString::from((entry.position + 1).to_string()))
            })
            .child(cover)
            .child(listing::title_cell(drawn.title, Lit::new(), current))
            .child(listing::artist_cell(
                self.opens(
                    ("entry-artist", index),
                    drawn.artist,
                    words::browser::open_artist_hint(),
                    drawn.artist_id.map(Selection::Artist),
                    cx,
                )
                .flex_shrink(1.0)
                .ends_in_an_ellipsis(),
            ))
            .when(fitted.format, |row| {
                row.child(listing::format_cell(drawn.shape))
            })
            .when(fitted.heard, |row| {
                row.child(listing::heard(drawn.plays, drawn.played, self.drawn_at()))
            })
            .child(listing::length_cell(drawn.length))
            .child(
                self.row_controls(("entry-controls", index), cx)
                    .child(self.queue_control(
                        ("entry-next", index),
                        Placement::Next,
                        {
                            let queueing = Arc::clone(&queueing);
                            move || Arc::clone(&queueing)
                        },
                        cx,
                    ))
                    .child(self.queue_control(
                        ("entry-last", index),
                        Placement::Queued,
                        move || Arc::clone(&queueing),
                        cx,
                    ))
                    .when(rows.are_moved(), |controls| {
                        controls
                            .child(self.mover("raise-entry", Step::Above, index, last, shift, cx))
                            .child(self.mover("lower-entry", Step::Below, index, last, shift, cx))
                    })
                    .child(self.add_control(
                        ("entry-to-playlist", index),
                        if acts_on_one {
                            words::playlists::add_hint()
                        } else {
                            words::playlists::add_reached_hint()
                        },
                        move || Held::out_of(opened, Arc::clone(&holding)),
                        cx,
                    ))
                    .when(rows.are_edited(), |controls| {
                        controls.child(self.in_the_pane_ring(
                            kit::icon_button(
                                ("drop-entry", index),
                                Icon::Close,
                                if acts_on_one {
                                    words::playlists::remove_hint()
                                } else {
                                    words::playlists::remove_reached_hint()
                                },
                            ),
                            move |this, _, cx| {
                                cx.stop_propagation();
                                this.drop_chosen(shift, &dropping, cx);
                            },
                            cx,
                        ))
                    }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if reaches {
                    if this.reach_by_click(shift, index, event.modifiers(), cx) {
                        return;
                    }
                    this.reach_at(shift, index, false, cx);
                }
                this.play_playlist(opened, &held, index, !narrowed, cx);
            }));
        let edited = rows.are_edited();
        let reach = if reaches {
            self.at_the_reach(shift, index)
        } else {
            None
        };
        let listed = menu::opens_a_reachable_menu(
            listed,
            reach,
            move |this, at, cx| {
                let names = Called {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: this.album_named(album, &location, span, cx),
                };
                let taken = if reaches {
                    this.acting_on_chosen(shift, index)
                } else {
                    Chosen::of(Span::one(position))
                };
                let put = Arc::clone(&menued);

                Menu::at(at)
                    .under(Icon::Play, words::menu::play(), "enter", {
                        let rows = Arc::clone(&playing);

                        move |this, _, cx| {
                            this.play_playlist(opened, &rows, index, !narrowed, cx);
                        }
                    })
                    .apart()
                    .holds(move || Held::out_of(opened, Arc::clone(&put)))
                    .when_some(scanned, |menu, track| {
                        menu.favours(Favoured::Track(track), favourite)
                    })
                    .reaches(scanned, album, artist_id)
                    .when_some(versioned.clone(), |menu, track| {
                        let chosen = this.library.read(cx).has_a_version_chosen(track.id);
                        menu.looks_for_versions(track, at, chosen)
                    })
                    .apart()
                    .when_some(scanned, Menu::shares)
                    .offers_the_file(&location, names)
                    .apart()
                    .when_some(edited.then_some(taken), |menu, taken| {
                        menu.under(
                            Icon::Discard,
                            words::menu::remove_row(),
                            "delete",
                            move |this, _, cx| this.drop_chosen(shift, &taken, cx),
                        )
                    })
            },
            cx,
        );

        if !rows.are_edited() {
            return listed;
        }
        let cursor_only = reaches && self.stands_unchosen_on(shift, index);
        let listed = if rows.are_moved() {
            reorder::movable(listed, index, carried, reached, cx)
        } else {
            reorder::marked(listed, reached)
        };
        reorder::cursor_marked(listed, cursor_only)
    }

    pub(crate) fn playlist_picker(&self, holding: &Held, cx: &mut Context<Self>) -> AnyElement {
        let asked = holding.asked_as();
        let offered = Offered::beside(self.library.read(cx).lists(), holding.taken_from);
        let shown = offered.len().min(theme::PICKER_ROWS);

        let rows = uniform_list(
            "picker-rows",
            offered.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        offered
                            .row(index)
                            .map(|playlist| picked(this, playlist, cx))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scrolls.picker_rows)
        .w_full()
        .h(theme::width(theme::row_height() * shown as f32));

        kit::over_a_scrim(motion::lifted_in(
            kit::sheet()
                .id("playlist-picker-panel")
                .dialog(words::playlists::picker_label())
                .track_focus(&self.sheet_panels.holding(sheets::PICKER_PANEL, cx))
                .gap_3()
                .w(theme::width(theme::picker_width()))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(kit::eyebrow(words::playlists::playlist()))
                        .child(
                            div()
                                .text_size(px(theme::text_lg()))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(asked),
                        ),
                )
                .when(shown > 0, |panel| {
                    panel.child(
                        div()
                            .rounded_md()
                            .overflow_hidden()
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(theme::width(theme::row_height() * shown as f32))
                                    .child(rows)
                                    .child(Scrollbars::of(cx).vertical(
                                        "playlist-picker-scrollbar",
                                        self.scrolls.picker_rows.clone(),
                                    )),
                            ),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(kit::eyebrow(if shown > 0 {
                            words::playlists::or_a_new_one()
                        } else {
                            words::playlists::a_new_one()
                        }))
                        .child(self.name_field(cx)),
                ),
            "playlist-picker-arrives",
        ))
        .id("playlist-picker")
        .on_click(cx.listener(|this, _, window, cx| this.stop_naming(window, cx)))
        .into_any_element()
    }

    fn undoing(&self, undoable: &Undoable, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_pane_ring(
            kit::button(
                "undo-edit",
                Some(Icon::Undo),
                words::playlists::undo(),
                puts_back(undoable),
                Tone::Ghost,
            ),
            |this, _, cx| this.undo_edit(cx),
            cx,
        )
    }

    fn redoing(&self, redoable: &Undoable, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_pane_ring(
            kit::button(
                "redo-edit",
                Some(Icon::Redo),
                words::playlists::redo(),
                does_again(redoable),
                Tone::Ghost,
            ),
            |this, _, cx| this.redo_edit(cx),
            cx,
        )
    }

    pub(crate) fn import_playlists(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(SharedString::new_static(words::playlists::import())),
        });

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    let reported = this.update(cx, |this, cx| {
                        this.report(
                            Notice::Trouble(words::playlists::no_file_picker().to_owned()),
                            cx,
                        );
                    });
                    let _ = reported;
                    return;
                }
            };

            let read = this.update(cx, |this, cx| {
                this.library
                    .update(cx, |library, cx| library.import_playlists(chosen, cx));
            });
            let _ = read;
        })
        .detach();
    }

    pub(crate) fn export_playlist(&self, playlist: PlaylistId, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let suggested = library.saved_playlist(playlist).map_or_else(
            || format!("{}.m3u8", words::playlists::unnamed(playlist.to_string())),
            |named| format!("{}.m3u8", named.name),
        );
        let beside = library
            .roots()
            .first()
            .cloned()
            .or_else(env::home_dir)
            .unwrap_or_else(|| PathBuf::from(LAST_RESORT_FOLDER));
        let picked = cx.prompt_for_new_path(&beside, Some(&suggested));

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    let reported = this.update(cx, |this, cx| {
                        this.report(
                            Notice::Trouble(words::playlists::no_file_picker().to_owned()),
                            cx,
                        );
                    });
                    let _ = reported;
                    return;
                }
            };

            let written = this.update(cx, |this, cx| {
                this.library.update(cx, |library, cx| {
                    library.export_playlist(playlist, chosen, cx);
                });
            });
            let _ = written;
        })
        .detach();
    }

    pub(crate) fn queue_control(
        &self,
        id: impl Into<ElementId>,
        at: Placement,
        holding: impl Fn() -> Arc<[PlaylistEntry]> + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (icon, saying) = queueing_look(at);

        let holding = Rc::new(holding);
        self.in_the_pane_ring(
            kit::icon_button(id, icon, saying),
            move |this, _, cx| {
                cx.stop_propagation();
                this.queue(&holding(), at, cx);
            },
            cx,
        )
    }

    pub(crate) fn queue_chosen_control(
        &self,
        id: impl Into<ElementId>,
        at: Placement,
        shift: Shift,
        row: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (icon, saying) = queueing_look(at);

        self.in_the_pane_ring(
            kit::icon_button(id, icon, saying),
            move |this, _, cx| {
                cx.stop_propagation();
                let chosen = this.acting_on_chosen(shift, row);
                let tracks = this.tracks_chosen(shift, &chosen, cx);
                this.queue(&listed(&tracks), at, cx);
            },
            cx,
        )
    }

    pub(crate) fn add_control(
        &self,
        id: impl Into<ElementId>,
        saying: &'static str,
        holding: impl Fn() -> Held + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let holding = Rc::new(holding);
        self.in_the_pane_ring(
            kit::icon_button(id, Icon::Plus, saying),
            move |this, window, cx| {
                cx.stop_propagation();
                this.hold_for_a_playlist(holding(), window, cx);
            },
            cx,
        )
    }
}

fn queueing_look(at: Placement) -> (Icon, &'static str) {
    match at {
        Placement::Next => (Icon::QueueNext, words::playlists::next_hint()),
        Placement::Queued | Placement::At(_) => (Icon::QueueLast, words::playlists::queue_hint()),
    }
}

impl RootView {
    fn playlist_row(
        &mut self,
        playlist: &Playlist,
        at: usize,
        playing: bool,
        now: SystemTime,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = playlist.id;
        let name = SharedString::from(playlist.name.clone());
        let pinned = playlist.pinned.is_some();
        let kind = kind_of(playlist);
        let shared_as = self
            .social
            .shared_lists
            .iter()
            .find(|(shared, _)| *shared == playlist.id)
            .map(|(_, editable)| *editable);
        let albums = self.library.read(cx).pictured(id);
        let art = self.playlist_art(
            &playlist.name,
            playlist.query.is_some(),
            &albums,
            theme::row_cover(),
            cx,
        );

        let listed = tall_row(playing)
            .id(id.get() as usize)
            .group(ROW_GROUP)
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .child(art)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .truncate()
                                    .ends_in_an_ellipsis()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(if playing {
                                        theme::accent()
                                    } else {
                                        theme::text()
                                    }))
                                    .child(name),
                            )
                            .when_some(kind, |row, (label, colour)| {
                                row.child(kit::badge(label, colour))
                            })
                            .when_some(shared_as, |row, editable| {
                                row.child(kit::badge(
                                    if editable {
                                        words::playlists::shared()
                                    } else {
                                        words::playlists::followed()
                                    },
                                    theme::accent(),
                                ))
                            })
                            .when(pinned, |row| {
                                row.child(kit::badge(words::playlists::pinned(), theme::accent()))
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(counted(playlist)),
                    ),
            )
            .child(listing::heard(playlist.plays, playlist.played, now))
            .child(
                self.row_controls(("playlist-controls", id.get() as usize), cx)
                    .child(
                        control("pin-saved", id, pin_icon(pinned), pin_hint(pinned)).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.library.update(cx, |library, cx| {
                                    library.pin_playlist(id, !pinned, cx);
                                });
                            }),
                        ),
                    )
                    .child(
                        control("play-saved", id, Icon::Play, words::playlists::play_hint())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.play_the_playlist(id, cx);
                            })),
                    )
                    .child(
                        control(
                            "shuffle-saved",
                            id,
                            Icon::Shuffle,
                            words::playlists::shuffle_hint(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.shuffle_the_playlist(id, cx);
                        })),
                    )
                    .child(
                        control(
                            "next-saved",
                            id,
                            Icon::QueueNext,
                            words::playlists::playlist_next_hint(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.queue_the_playlist(id, Placement::Next, cx);
                        })),
                    )
                    .child(
                        control(
                            "last-saved",
                            id,
                            Icon::QueueLast,
                            words::playlists::playlist_queue_hint(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.queue_the_playlist(id, Placement::Queued, cx);
                        })),
                    )
                    .when(playlist.query.is_none(), |controls| {
                        controls.child(
                            control(
                                "add-songs",
                                id,
                                Icon::Plus,
                                words::playlists::add_songs_hint(),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.add_songs_to_playlist(id, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        control(
                            "drop-saved",
                            id,
                            Icon::Discard,
                            words::playlists::discard_hint(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.library
                                .update(cx, |library, cx| library.drop_playlist(id, cx));
                        })),
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if this.reach_by_click(Shift::Listing(Listed::Playlists), at, event.modifiers(), cx)
                {
                    return;
                }
                this.show_playlist(Some(id), cx);
            }));
        let listed = match playlist.query {
            None => reorder::takes_a_lift(listed, LiftedTo::Playlist(id), cx),
            Some(_) => listed,
        };

        let reach = self.at_the_reach(Shift::Listing(Listed::Playlists), at);
        let listed = menu::opens_a_reachable_menu(
            listed,
            reach,
            move |this, menu_at, cx| {
                playlist_menu_among_the_chosen(this, at, menu_at, id, pinned, cx)
            },
            cx,
        );
        self.marked_where_reached(Shift::Listing(Listed::Playlists), at, listed)
    }

    fn playlist_cell(
        &mut self,
        playlist: &Playlist,
        at: usize,
        playing: bool,
        now: SystemTime,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = playlist.id;
        let side = self.grid_side();
        let pinned = playlist.pinned.is_some();
        let searched = playlist.query.is_some();
        let reached = self.reaches(Shift::Listing(Listed::Playlists), at);
        let albums = self.library.read(cx).pictured(id);
        let art = self.playlist_art(&playlist.name, searched, &albums, side, cx);
        let cell_id = ElementId::from(("playlist-cell", id.get() as usize));
        let pointed = pointed::is_pointed_at(&cell_id);
        let about = match self
            .social
            .shared_lists
            .iter()
            .find(|(shared, _)| *shared == playlist.id)
        {
            Some((_, true)) => SharedString::from(words::playlists::shared_about(
                about_a_cell(playlist, now).as_ref(),
            )),
            Some((_, false)) => SharedString::from(words::playlists::followed_about(
                about_a_cell(playlist, now).as_ref(),
            )),
            None => about_a_cell(playlist, now),
        };
        let cursor_only = self.stands_unchosen_on(Shift::Listing(Listed::Playlists), at);

        let cell = div()
            .id(cell_id.clone())
            .debug_selector(move || format!("playlist-{}", id.get()))
            .follows_the_pointer(cell_id)
            .group(PLAYLIST_CELL)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(words::playlists::open_hint())
            .child(
                div()
                    .relative()
                    .rounded(px(mosaic::ART_ROUNDING))
                    .group_hover(PLAYLIST_CELL, |frame| frame.casting(kit::Cast::Short))
                    .child(art)
                    .when(reached, |frame| frame.child(browser::reached_ring()))
                    .when(cursor_only, |frame| frame.child(browser::cursor_ring()))
                    .child(
                        self.pin_mark(id, pinned, cx)
                            .when(!pinned, |mark| {
                                mark.opacity(0.0)
                                    .group_hover(PLAYLIST_CELL, |mark| mark.opacity(1.0))
                            })
                            .absolute()
                            .top_1()
                            .right_1()
                            .rounded_md()
                            .bg(kit::cover_veil()),
                    )
                    .when(playlist.entries > 0, |frame| {
                        frame.child(
                            self.play_on_the_art(id, cx)
                                .absolute()
                                .bottom_2()
                                .right_2()
                                .opacity(0.0)
                                .group_hover(PLAYLIST_CELL, |mark| mark.opacity(1.0)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .min_w(px(0.0))
                            .when(playing, |line| {
                                line.child(icons::icon(
                                    Icon::Volume,
                                    theme::row_control_icon(),
                                    theme::accent(),
                                ))
                            })
                            .when(searched, |line| {
                                line.child(icons::icon(
                                    Icon::Search,
                                    theme::row_control_icon(),
                                    theme::muted(),
                                ))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(theme::text_sm()))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(if pointed || playing {
                                        theme::accent()
                                    } else {
                                        theme::text()
                                    }))
                                    .truncate()
                                    .ends_in_an_ellipsis()
                                    .child(SharedString::from(playlist.name.clone())),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(about),
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if this.reach_by_click(Shift::Listing(Listed::Playlists), at, event.modifiers(), cx)
                {
                    return;
                }
                this.show_playlist(Some(id), cx);
            }));
        let cell = match playlist.query {
            None => reorder::takes_a_lift(cell, LiftedTo::Playlist(id), cx),
            Some(_) => cell,
        };

        let reach = self.at_the_reach(Shift::Listing(Listed::Playlists), at);
        menu::opens_a_reachable_menu(
            cell,
            reach,
            move |this, menu_at, cx| {
                playlist_menu_among_the_chosen(this, at, menu_at, id, pinned, cx)
            },
            cx,
        )
    }

    fn pin_mark(&self, id: PlaylistId, pinned: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let mark = match pinned {
            true => kit::lit_mark(
                ("pin-cell", id.get() as usize),
                Icon::Pinned,
                words::playlists::unpin_hint(),
            ),
            false => kit::icon_button(
                ("pin-cell", id.get() as usize),
                Icon::Pin,
                words::playlists::pin_hint(),
            ),
        };

        self.in_the_pane_ring(
            mark,
            move |this, _, cx| {
                cx.stop_propagation();
                this.library
                    .update(cx, |library, cx| library.pin_playlist(id, !pinned, cx));
            },
            cx,
        )
    }

    fn play_on_the_art(&self, id: PlaylistId, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::play_on_the_art(
            ("play-cell", id.get() as usize),
            words::playlists::play_hint(),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.play_the_playlist(id, cx);
        }))
    }

    fn playlist_art(
        &self,
        name: &str,
        query: bool,
        albums: &[AlbumId],
        side: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let drawn: Vec<Picture> = albums
            .iter()
            .filter_map(|album| {
                self.library
                    .update(cx, |library, cx| library.cover(*album, Drawn::InAGrid, cx))
            })
            .take(mosaic::TILES)
            .collect();

        Mosaic {
            drawn: &drawn,
            mark: if query { Icon::Search } else { Icon::Playlists },
            name,
        }
        .drawn(side)
    }

    pub(crate) fn add_songs_to_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        self.adding_songs_to = Some(id);
        self.show_everything(cx);
        self.set_pane(Pane::Tracks, cx);
    }

    pub(crate) fn finish_adding_songs(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.adding_songs_to.take() else {
            return;
        };
        self.show_playlist(Some(id), cx);
        self.set_pane(Pane::Playlists, cx);
    }
}

fn kind_of(playlist: &Playlist) -> Option<(&'static str, u32)> {
    match (playlist.query.is_some(), playlist.kept.is_some()) {
        (true, _) => Some((words::playlists::search(), theme::repacked())),
        (false, true) => Some((words::playlists::kept(), theme::muted())),
        (false, false) => None,
    }
}

fn history_of(playlist: &Playlist, now: SystemTime) -> SharedString {
    let made = format::since(playlist.created, now);
    SharedString::from(match playlist.played {
        Some(played) => words::playlists::made_and_last_played(made, format::since(played, now)),
        None => words::playlists::made_and_never_played(made),
    })
}

fn about_a_cell(playlist: &Playlist, now: SystemTime) -> SharedString {
    let tracks = words::playlists::tracks(playlist.entries);
    match playlist.played {
        Some(played) => SharedString::from(words::playlists::tracks_played(
            format::since(played, now),
            tracks,
        )),
        None => match playlist.duration {
            Some(length) => SharedString::from(format!("{tracks} · {}", format::spanned(length))),
            None => SharedString::from(tracks),
        },
    }
}

fn playlist_menu_among_the_chosen(
    this: &RootView,
    row: usize,
    at: Point<Pixels>,
    id: PlaylistId,
    pinned: bool,
    cx: &App,
) -> Menu {
    let chosen = this.acting_on_chosen(Shift::Listing(Listed::Playlists), row);
    if chosen.count() > 1 {
        let playlists = this.library.read(cx).playlists_at(chosen.rows());
        return chosen_playlists_menu(at, playlists);
    }
    playlist_menu(at, id, pinned, cx)
}

fn chosen_playlists_menu(at: Point<Pixels>, playlists: Vec<PlaylistId>) -> Menu {
    let (play, next, last) = (playlists.clone(), playlists.clone(), playlists);

    Menu::at(at)
        .does(Icon::Play, words::menu::play(), move |this, _, cx| {
            this.queue_the_playlists(play.clone(), Placement::Queued, true, cx);
        })
        .does(
            Icon::QueueNext,
            words::menu::play_next(),
            move |this, _, cx| {
                this.queue_the_playlists(next.clone(), Placement::Next, false, cx);
            },
        )
        .does(
            Icon::QueueLast,
            words::menu::add_to_queue(),
            move |this, _, cx| {
                this.queue_the_playlists(last.clone(), Placement::Queued, false, cx);
            },
        )
}

pub(crate) fn playlist_menu(at: Point<Pixels>, id: PlaylistId, pinned: bool, cx: &App) -> Menu {
    let menu = Menu::at(at)
        .does(Icon::Play, words::menu::play(), move |this, _, cx| {
            this.play_the_playlist(id, cx);
        })
        .does(
            Icon::QueueNext,
            words::menu::play_next(),
            move |this, _, cx| {
                this.queue_the_playlist(id, Placement::Next, cx);
            },
        )
        .does(
            Icon::QueueLast,
            words::menu::add_to_queue(),
            move |this, _, cx| {
                this.queue_the_playlist(id, Placement::Queued, cx);
            },
        )
        .apart()
        .does(pin_icon(pinned), pin_label(pinned), move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.pin_playlist(id, !pinned, cx));
        })
        .does(
            Icon::Rename,
            words::menu::rename(),
            move |this, window, cx| {
                this.name_a_playlist(Naming::Rename(id), window, cx);
            },
        )
        .does(
            Icon::Playlists,
            words::menu::duplicate(),
            move |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.duplicate_playlist(id, cx));
            },
        )
        .apart()
        .does(
            Icon::Share,
            words::playlists::share_with_people(),
            move |this, _, cx| {
                this.open_sharing(id, cx);
            },
        )
        .does(Icon::Export, words::menu::export(), move |this, _, cx| {
            this.export_playlist(id, cx);
        });

    synced_entries(menu, id, cx).apart().does(
        Icon::Discard,
        words::menu::discard(),
        move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.drop_playlist(id, cx));
        },
    )
}

fn synced_entries(menu: Menu, id: PlaylistId, cx: &App) -> Menu {
    let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
        return menu;
    };
    let missing = syncing.missing_from(id);
    let kept = syncing.keeps_songs_of(id);
    let mut menu = menu.apart();
    if missing > 0 {
        for device in syncing
            .shown()
            .devices
            .into_iter()
            .filter(|device| device.online && !device.this)
        {
            let fetching = Arc::clone(&syncing);
            let label = words::playlists::fetch_missing(device.name.as_str(), missing);
            menu = menu.does(Icon::Download, label, move |this, _, cx| {
                fetching.fetch_missing(id, &device.name);
                this.report(
                    Notice::Noted(words::playlists::on_their_way(device.name.as_str())),
                    cx,
                );
            });
        }
    }
    let label = if kept {
        words::playlists::stop_keeping_its_songs()
    } else {
        words::playlists::keep_its_songs_here()
    };
    menu.does(Icon::Download, label, move |this, _, cx| {
        syncing.keep_songs_of(id, !kept);
        let said = if kept {
            words::playlists::songs_no_longer_fetched()
        } else {
            words::playlists::songs_fetched()
        };
        this.report(Notice::Done(said.to_owned()), cx);
    })
}

const fn pin_icon(pinned: bool) -> Icon {
    match pinned {
        true => Icon::Pinned,
        false => Icon::Pin,
    }
}

fn pin_label(pinned: bool) -> &'static str {
    match pinned {
        true => words::menu::unpin(),
        false => words::menu::pin(),
    }
}

fn pin_hint(pinned: bool) -> &'static str {
    match pinned {
        true => words::playlists::unpin_hint(),
        false => words::playlists::pin_hint(),
    }
}

fn control(
    id: &'static str,
    playlist: PlaylistId,
    icon: Icon,
    saying: &'static str,
) -> Stateful<Div> {
    kit::icon_button((id, playlist.get() as usize), icon, saying)
}

fn came_from(source: &PlaylistSource) -> String {
    let service = source.link.service();
    match source.upkeep {
        Upkeep::Once => words::playlists::imported_from(service),
        Upkeep::InStep => words::playlists::kept_in_step_with(service),
    }
}

fn puts_back(undoable: &Undoable) -> SharedString {
    let name = undoable.name.as_str();
    let doing = match undoable.edit {
        Edit::Started => words::playlists::undo_started(name),
        Edit::Renamed => words::playlists::undo_renamed(name),
        Edit::Revised => words::playlists::undo_revised(name),
        Edit::Discarded => words::playlists::undo_discarded(name),
        Edit::Added | Edit::Copied | Edit::Imported => words::playlists::undo_added(name),
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            words::playlists::undo_removed(name)
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => words::playlists::undo_moved(name),
        Edit::Refreshed => words::playlists::undo_refreshed(name),
    };

    SharedString::from(words::playlists::puts_back(
        undoable.behind,
        doing,
        key!(undo),
        made_by(undoable.made_by),
    ))
}

fn does_again(redoable: &Undoable) -> SharedString {
    let name = redoable.name.as_str();
    let doing = match redoable.edit {
        Edit::Started => words::playlists::redo_started(name),
        Edit::Renamed => words::playlists::redo_renamed(name),
        Edit::Revised => words::playlists::redo_revised(name),
        Edit::Discarded => words::playlists::redo_discarded(name),
        Edit::Added | Edit::Copied | Edit::Imported => words::playlists::redo_added(name),
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            words::playlists::redo_removed(name)
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => words::playlists::redo_moved(name),
        Edit::Refreshed => words::playlists::redo_refreshed(name),
    };

    SharedString::from(words::playlists::does_again(
        redoable.behind,
        doing,
        key!(redo),
        made_by(redoable.made_by),
    ))
}

fn made_by(maker: Maker) -> &'static str {
    match maker {
        Maker::ThisRun => "",
        Maker::AModel => words::playlists::made_by_a_model(),
    }
}

fn capped(rows: Option<usize>) -> SharedString {
    match rows {
        Some(rows) => SharedString::from(rows.to_string()),
        None => SharedString::new_static(words::playlists::no_cap()),
    }
}

fn shown_of(shown: usize, playlist: &Playlist) -> SharedString {
    SharedString::from(words::playlists::shown_of(playlist.entries, shown))
}

fn picked(root: &RootView, playlist: &Playlist, cx: &mut Context<RootView>) -> AnyElement {
    let id = playlist.id;

    let line = row(false)
        .id(("picker", id.get() as usize))
        .debug_selector(move || format!("picker-{}", id.get()))
        .named_as(Role::Button, &playlist.name, words::playlists::pick_hint())
        .px_3()
        .cursor_pointer()
        .hover(|row| row.bg(rgb(theme::hover())))
        .child(icons::icon(
            Icon::Playlists,
            theme::root_icon(),
            theme::muted(),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .child(SharedString::from(playlist.name.clone())),
        )
        .child(kit::figure(counted(playlist)));

    root.in_the_pane_ring(
        line,
        move |this, window, cx| {
            let Some(holding) = this.adding.take() else {
                return;
            };
            this.library.update(cx, |library, cx| {
                library.add_to_playlist(id, holding.rows.to_vec(), cx);
            });
            this.stop_naming(window, cx);
        },
        cx,
    )
    .into_any_element()
}

fn counted(playlist: &Playlist) -> SharedString {
    let tracks = words::playlists::tracks(playlist.entries);

    match playlist.duration {
        Some(played) => SharedString::from(format!("{tracks} · {}", format::spanned(played))),
        None => SharedString::from(tracks),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::SystemTime};

    use resonate_core::{FrameSpan, Frames, MediaLocation, PlaylistId, Span};
    use resonate_engine::{QueueItem, QueueRows, Queued};
    use resonate_library::{Cut, Playlist, PlaylistEntry};

    use super::{Offered, Reaching};
    use crate::views::reorder::Chosen;

    fn saved(ids: &[u64]) -> Arc<[Playlist]> {
        ids.iter()
            .map(|id| Playlist {
                id: PlaylistId::new(*id).expect("an id above zero"),
                name: format!("playlist {id}"),
                entries: 0,
                duration: None,
                created: SystemTime::UNIX_EPOCH,
                modified: SystemTime::UNIX_EPOCH,
                played: None,
                plays: 0,
                query: None,
                kept: None,
                pinned: None,
                source: None,
            })
            .collect()
    }

    fn named(offered: &Offered) -> Vec<u64> {
        (0..offered.len())
            .map(|index| {
                offered
                    .row(index)
                    .expect("a row under every index")
                    .id
                    .get()
            })
            .collect()
    }

    fn listed(rows: usize) -> Vec<PlaylistEntry> {
        (0..rows)
            .map(|position| PlaylistEntry {
                position,
                cut: Cut::whole(MediaLocation::local(Path::new(&format!(
                    "/music/{position}.flac"
                )))),
                track: None,
            })
            .collect()
    }

    fn cut_at(position: usize, span: Option<FrameSpan>) -> PlaylistEntry {
        PlaylistEntry {
            position,
            cut: Cut {
                location: MediaLocation::local(Path::new("/music/Meddle.flac")),
                span,
            },
            track: None,
        }
    }

    #[test]
    fn a_row_a_sheet_cut_is_queued_as_the_cut_rather_than_the_file_it_came_out_of() {
        let cut = FrameSpan::between(Frames(882_000), Frames(1_764_000));
        let entries = vec![cut_at(0, Some(cut)), cut_at(1, None)];
        let items = super::queue_items(&entries, &QueueRows::default());

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].span, Some(cut), "a cue row was queued whole");
        assert_eq!(items[1].span, None);
        assert_ne!(
            items[0].id, items[1].id,
            "two rows no scan has seen were minted one id"
        );
    }

    fn queued_as(rows: Vec<QueueItem>, loaded_at: Vec<usize>) -> Queued {
        Queued {
            rows: QueueRows::placed(loaded_at.into_iter().zip(rows)),
            ..Queued::default()
        }
    }

    #[test]
    fn the_playing_mark_follows_the_song_however_the_view_is_narrowed_or_ordered() {
        let entries = listed(5);
        let items = super::queue_items(&entries, &QueueRows::default());
        let queued = queued_as(items, vec![0, 1, 2, 3, 4]);

        assert_eq!(super::playing_entry(&queued, 3, &entries), Some(3));

        let narrowed: Vec<PlaylistEntry> = entries
            .iter()
            .filter(|entry| entry.position % 2 == 1)
            .cloned()
            .collect();
        assert_eq!(super::playing_entry(&queued, 3, &narrowed), Some(1));
        assert_eq!(super::playing_entry(&queued, 2, &narrowed), None);

        let reversed: Vec<PlaylistEntry> = entries.iter().rev().cloned().collect();
        assert_eq!(super::playing_entry(&queued, 3, &reversed), Some(1));

        let mut moved = entries.clone();
        let taken = moved.remove(3);
        moved.insert(0, taken);
        for (position, entry) in moved.iter_mut().enumerate() {
            entry.position = position;
        }
        assert_eq!(super::playing_entry(&queued, 3, &moved), Some(0));
    }

    #[test]
    fn a_file_listed_twice_is_marked_at_the_listing_that_plays() {
        let entries = vec![cut_at(0, None), cut_at(1, None), cut_at(2, None)];
        let items = super::queue_items(&entries, &QueueRows::default());
        let shuffled = queued_as(
            vec![items[2].clone(), items[0].clone(), items[1].clone()],
            vec![2, 0, 1],
        );

        assert_eq!(super::playing_entry(&shuffled, 1, &entries), Some(1));
        assert_eq!(super::playing_entry(&shuffled, 2, &entries), Some(2));
        assert_eq!(super::playing_entry(&shuffled, 0, &entries), Some(0));
    }

    #[test]
    fn a_reach_is_read_off_the_list_once() {
        let entries = listed(8);
        let reaching = Reaching::of(&entries, Chosen::of(Span::between(2, 4)));

        assert_eq!(reaching.entries.len(), 3);
        assert_eq!(
            reaching
                .entries
                .iter()
                .map(|entry| entry.position)
                .collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        assert_eq!(
            reaching.holding.as_ref(),
            reaching
                .entries
                .iter()
                .map(|entry| entry.cut.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_reach_past_the_end_of_the_list_holds_only_the_rows_the_list_has() {
        let entries = listed(3);

        assert_eq!(
            Reaching::of(&entries, Chosen::of(Span::between(2, 9)))
                .entries
                .len(),
            1
        );
        assert!(
            Reaching::of(&entries, Chosen::of(Span::between(3, 9)))
                .entries
                .is_empty()
        );
    }

    #[test]
    fn rows_chosen_apart_are_read_off_the_list_in_order() {
        let entries = listed(8);
        let apart = Chosen::of(Span::one(5)).united(Span::one(1));
        let reaching = Reaching::of(&entries, apart.clone());

        assert_eq!(
            reaching
                .entries
                .iter()
                .map(|entry| entry.position)
                .collect::<Vec<_>>(),
            vec![1, 5]
        );
        assert!(Reaching::acting_on(Some(&reaching), &apart).is_some());
    }

    #[test]
    fn only_the_rows_a_reach_was_read_for_share_it() {
        let entries = listed(8);
        let reaching = Reaching::of(&entries, Chosen::of(Span::between(2, 4)));

        let reached = Chosen::of(Span::between(2, 4));

        assert!(Reaching::acting_on(Some(&reaching), &reached).is_some());
        assert!(Reaching::acting_on(Some(&reaching), &Chosen::of(Span::one(3))).is_none());
        assert!(Reaching::acting_on(None, &reached).is_none());
    }

    #[test]
    fn the_picker_offers_every_list_where_the_rows_came_from_none() {
        let offered = Offered::beside(saved(&[3, 1, 2]), None);

        assert_eq!(offered.len(), 3);
        assert_eq!(named(&offered), vec![3, 1, 2]);
    }

    #[test]
    fn the_picker_steps_over_the_playlist_the_rows_came_out_of() {
        let taken_from = PlaylistId::new(1).expect("an id above zero");

        for lists in [&[1, 2, 3], &[2, 1, 3], &[2, 3, 1]] {
            let offered = Offered::beside(saved(lists), Some(taken_from));

            assert_eq!(offered.len(), 2);
            assert_eq!(named(&offered), vec![2, 3]);
            assert!(offered.row(2).is_none());
        }
    }

    #[test]
    fn a_playlist_no_listing_holds_takes_no_row_off_the_picker() {
        let gone = PlaylistId::new(9).expect("an id above zero");
        let offered = Offered::beside(saved(&[1, 2]), Some(gone));

        assert_eq!(offered.len(), 2);
        assert_eq!(named(&offered), vec![1, 2]);
    }
}
