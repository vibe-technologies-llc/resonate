use std::{env, path::PathBuf, sync::Arc, time::SystemTime};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, FontWeight, Image, MouseButton,
    PathPromptOptions, Pixels, Point, SharedString, Stateful, div, prelude::*, px, rgb, rgba,
    uniform_list,
};
use resonate_core::{AlbumId, PlaylistId, Span};
use resonate_engine::{Command, Placement, QueueItem, Unclaimed};
use resonate_library::{Cut, Edit, Favoured, Lit, Playlist, PlaylistEntry, Undoable};
use smallvec::smallvec;

use crate::{
    Drawn, Notice, Selection, format,
    icons::{self, Icon},
    theme,
    views::{
        browser::{self, OPEN_ARTIST_HINT, row_controls},
        hint::{self, Names},
        kit::{self, EndsInAnEllipsis, Tone},
        listing::{self, Pictured},
        menu::{self, Called, Menu},
        mosaic::{self, Framed, Mosaic},
        pointed::{self, LitUnderThePointer as _},
        reorder::{self, Carried, Listed, MOVING_HINT, Shift, Step},
        root::{Pane, RootView, empty, row, somewhere_in, tall_row},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NEW_HINT: &str = "Start a playlist, and name it";

const PLAY_HINT: &str = "Play this playlist in order from the top";

const SHUFFLE_HINT: &str = "Play this playlist, shuffled";

const NO_FILE_PICKER: &str = "Couldn't open the file picker";

pub(crate) const NEXT_HINT: &str = "Hear this straight after the track playing";

pub(crate) const QUEUE_HINT: &str =
    "Queue this after what is already queued, ahead of the rest of what is playing";

const PLAYLIST_NEXT_HINT: &str = "Hear this playlist straight after the track playing";

const PLAYLIST_QUEUE_HINT: &str =
    "Queue this playlist after what is already queued, ahead of the rest of what is playing";

const RENAME_HINT: &str = "Give this playlist another name";

const PIN_HINT: &str =
    "Keep this playlist at the top of the list, whichever order it is read in, and in the sidebar";

const UNPIN_HINT: &str = "Let this playlist sit where the order puts it";

const DISCARD_HINT: &str = "Forget this playlist. The files stay where they are";

const BACK_HINT: &str = "Back to every playlist";

const IMPORT_HINT: &str = "Read playlists in from M3U, PLS or XSPF files";

const TIDY_HINT: &str =
    "Drop rows whose files are gone and repeated rows, keeping the first of each file";

const SORT_HINT: &str = "Put every row in order at once, rather than moving them one at a time, or keep the list in that \
     order. A row no scan has seen goes to the end, except under file name, which reads the path \
     every row carries";

const ONCE_HINT: &str = "Put the rows in order now, and leave a row added later at the end";

const ALWAYS_HINT: &str = "Keep the list in that order, so a row added later lands where the order puts it rather than at \
     the end";

const NARROWED_HINT: &str = "A search is narrowing this playlist, so the rows beside one are not here for it to move \
     between: clear the search to put them in order by hand again. Play, Play next, Add to queue \
     and Drop shown take the rows shown; Sort, Tidy and Export take the playlist whole, however \
     little of it a search has left on screen";

pub(crate) const KEPT_HINT: &str = "This playlist is kept in order, so a row lands where the order puts it rather than where it \
     is dropped. Press Sort to read it another way round, or to put it back in hand";

const FILLS_ITSELF_HINT: &str = "This playlist is a search: it holds whatever the library matches now, so nothing in it is a \
     row to move or drop. Press to change what it looks for";

pub(crate) const SAVE_SEARCH_HINT: &str =
    "Keep this search as a playlist that fills itself as the library grows";

const LAST_RESORT_FOLDER: &str = "/";

pub(crate) const ADD_HINT: &str = "Put this track in a playlist";

pub(crate) const ADD_REACHED_HINT: &str = "Put every reached row in a playlist";

const ADD_SONGS_HINT: &str = "Choose songs from the library to add to this playlist";

pub(crate) const ADD_SONG_HINT: &str = "Add this track to the playlist";

pub(crate) const FINISH_ADDING_HINT: &str = "Return to the playlist";

const PLAYLIST_CELL: &str = "playlist-cell";

const PLAY_ON_THE_ART: f32 = 36.0;

const PLAY_MARK_ON_THE_ART: f32 = 0.45;

const OPEN_HINT: &str = "Open this playlist";

const MORE_HINT: &str = "Queue, rename, duplicate, export or discard this playlist";

const DROP_SHOWN_HINT: &str = "Take every row shown here out of the playlist. The rows the search does not match stay, and \
     the files stay where they are";

const REMOVE_HINT: &str = "Take this row out of the playlist";

const REMOVE_REACHED_HINT: &str = "Take every reached row out of the playlist";

const UNDO_KEY: &str = "ctrl-z does the same, and each press walks back one more edit";

const REDO_KEY: &str = "ctrl-shift-z does the same, and each press puts back one more edit";

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
        match (self.taken_from.is_some(), self.rows.len()) {
            (true, 1) => SharedString::new_static("Copy this track into"),
            (true, held) => SharedString::from(format!("Copy {held} tracks into")),
            (false, 1) => SharedString::new_static("Add this track to"),
            (false, held) => SharedString::from(format!("Add {held} tracks to")),
        }
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
    rows: Span,
    entries: Arc<[PlaylistEntry]>,
    holding: Arc<[Cut]>,
}

impl Reaching {
    fn of(entries: &[PlaylistEntry], rows: Span) -> Self {
        let reached: Arc<[PlaylistEntry]> = entries.get(rows.range()).unwrap_or_default().into();
        let holding = reached.iter().map(|entry| entry.cut.clone()).collect();

        Self {
            rows,
            entries: reached,
            holding,
        }
    }

    fn acting_on(reaching: Option<&Self>, rows: Span) -> Option<&Self> {
        reaching.filter(|reaching| reaching.rows == rows)
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
    const fn saying(self) -> &'static str {
        match self {
            Self::Grid => "Draw the playlists as a grid of covers, the way albums are",
            Self::List => {
                "Draw the playlists as a list, with how often and how lately each was played"
            }
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

const KEEPING: [(&str, bool, &str); 2] = [
    ("Just once", false, ONCE_HINT),
    ("From now on", true, ALWAYS_HINT),
];

pub(crate) fn queue_items(entries: &[PlaylistEntry], beside: &[QueueItem]) -> Vec<QueueItem> {
    let mut minting = Unclaimed::beside(beside);

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
                    "No playlist is named that.",
                    Some("A playlist has only a name to answer with."),
                )
            } else {
                kit::empty_offering(
                    Icon::Playlists,
                    "No playlists yet.",
                    Some(
                        "Start one here, read one in from another player, or press + on any track or queue row.",
                    ),
                    self.first_playlist_offers(cx),
                )
            }
        } else {
            match self.playlists_drawn {
                PlaylistsDrawn::Grid => self.playlist_grid(saved, playing, cx),
                PlaylistsDrawn::List => Scrollbars::of(cx)
                    .around(
                        "playlists-scrollbar",
                        self.all_playlist_rows.clone(),
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
                        .track_scroll(self.all_playlist_rows.clone())
                        .h_full()
                        .w_full(),
                    )
                    .into_any_element(),
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .child(listed)
            .into_any_element()
    }

    fn first_playlist_offers(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .gap_2()
            .child(
                kit::button(
                    "first-playlist",
                    Some(Icon::Plus),
                    "New playlist",
                    NEW_HINT,
                    Tone::Primary,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.name_a_playlist(Naming::New, window, cx);
                })),
            )
            .child(
                kit::button(
                    "first-import",
                    Some(Icon::Import),
                    "Import",
                    IMPORT_HINT,
                    Tone::Outlined,
                )
                .on_click(cx.listener(|this, _, _, cx| this.import_playlists(cx))),
            )
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
        let measured = self.grid_width.clone();
        let laid_out = self.grid_width.get() > px(0.0);

        div()
            .relative()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .pt_5()
            .child(kit::measures_its_width(measured))
            .when(laid_out, |body| {
                body.child(
                    uniform_list(
                        "playlist-grid",
                        rows,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            let now = this.drawn_at();
                            range
                                .map(|index| {
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
                                        .h(px(theme::grid_row()))
                                        .children(cells)
                                })
                                .collect()
                        }),
                    )
                    .track_scroll(self.all_playlist_rows.clone())
                    .h_full()
                    .w_full(),
                )
            })
            .child(
                Scrollbars::of(cx)
                    .vertical("playlist-grid-scrollbar", self.all_playlist_rows.clone()),
            )
            .into_any_element()
    }

    fn playlists_drawn_as(&self, cx: &mut Context<Self>) -> Div {
        let drawn = self.playlists_drawn;
        let choice = |as_: PlaylistsDrawn, label: &'static str, cx: &mut Context<Self>| {
            kit::segment(("playlists-drawn", as_ as usize), label, drawn == as_)
                .names(as_.saying())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.playlists_drawn = as_;
                    cx.notify();
                }))
        };

        kit::segmented()
            .child(choice(PlaylistsDrawn::Grid, "Grid", cx))
            .child(choice(PlaylistsDrawn::List, "List", cx))
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
                    .child(self.orders_a_listing("playlists-sort", cx))
            })
            .child(
                kit::icon_button("import-playlists", Icon::Import, IMPORT_HINT)
                    .on_click(cx.listener(|this, _, _, cx| this.import_playlists(cx))),
            )
            .when(naming.is_none(), |bar| {
                bar.child(
                    kit::button(
                        "new-playlist",
                        Some(Icon::Plus),
                        "New playlist",
                        NEW_HINT,
                        Tone::Primary,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.name_a_playlist(Naming::New, window, cx);
                    })),
                )
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
                            .child(kit::eyebrow("COLLECTION"))
                            .child(kit::title("Playlists"))
                            .child(kit::subtitle(format::counted(
                                saved,
                                "playlist",
                                "playlists",
                            ))),
                    )
                    .child(actions),
            )
            .when(!reads.is_empty(), |pane| pane.child(listing::reads(&reads)))
            .when(any_saved && self.ordering, |pane| {
                pane.child(self.playlists_order(cx))
            })
            .when_some(naming, |pane, naming| {
                pane.child(self.naming_row(naming, cx))
            })
    }

    fn playlists_order(&self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let held = library.playlist_order();
        let reading = library.playlist_reading();

        sorting::order_row(
            "playlist-order",
            "playlist-reading",
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
            Naming::New => "Name it",
            Naming::Query(None) => "Name this search",
            Naming::Query(Some(_)) => "Change this search",
            Naming::Rename(_) => "Rename it",
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
        let cap = self.query_cap;

        let mut caps = sorting::shape("Rows");
        for (index, held) in CAPS.into_iter().enumerate() {
            caps = caps.child(
                kit::chip(("search-cap", index), capped(held), held == cap)
                    .on_click(cx.listener(move |this, _, _, cx| this.cap_a_search(held, cx))),
            );
        }

        sorting::order_row(
            "search-order",
            "search-reading",
            self.query_sort,
            self.query_reading,
            |this, order, cx| this.sort_a_search(order, cx),
            |this, reading, cx| this.read_a_search(reading, cx),
            cx,
        )
        .child(caps)
        .when_some(cap.filter(|_| !CAPS.contains(&cap)), |shape, held| {
            shape.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme::faint()))
                    .child(SharedString::from(format!(
                        "{held} rows is the cap, which is none of these."
                    ))),
            )
        })
    }

    pub(crate) fn name_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("playlist-name")
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_md()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(theme::tinted(theme::accent(), 0x99))
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.name.update(cx, |name, _| name.take_focus(window));
                cx.notify();
            }))
            .child(self.name.clone())
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
        let playing = self.playing_row(opened, cx);
        let reaching = rows
            .are_reached()
            .then(|| self.reaching(Shift::Playlist(opened)))
            .flatten()
            .map(|reached| Reaching::of(&entries, reached));

        let listed = if entries.is_empty() {
            self.nothing_in_the_playlist(opened, rows, cx)
        } else {
            let held = Arc::clone(&entries);
            let scroll = self.playlist_rows.clone();
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
            .track_scroll(scroll.clone())
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
            .child(Scrollbars::of(cx).vertical("playlist-scrollbar", self.playlist_rows.clone()))
            .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .when(!entries.is_empty(), |pane| {
                pane.child(listing::columns(
                    "#",
                    true,
                    rows.control_count(),
                    sorting::playlist_sorted(self),
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
                "Nothing in this playlist matches the search.",
                None,
            ),
            Rows::Matched => kit::empty_offering(
                Icon::Search,
                "Nothing in the library matches this search yet.",
                Some("It fills itself as the library grows, or its search can be changed."),
                kit::button(
                    "revise-an-empty-search",
                    Some(Icon::Search),
                    "Edit search",
                    FILLS_ITSELF_HINT,
                    Tone::Outlined,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.revise_search(opened, window, cx);
                })),
            ),
            Rows::InHand | Rows::Kept => kit::empty_offering(
                Icon::Playlists,
                "This playlist is empty.",
                Some("Choose songs from the library, or press + on any track or queue row."),
                kit::button(
                    "fill-an-empty-playlist",
                    Some(Icon::Plus),
                    "Add songs",
                    ADD_SONGS_HINT,
                    Tone::Primary,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.add_songs_to_playlist(opened, cx);
                })),
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
        let sorting = self.sorting == Some(opened) && rows.are_edited() && !entries.is_empty();
        let shown = entries.len();
        let undoable = library.undoable();
        let redoable = library.redoable();
        let albums = library.pictured(opened);
        let pinned = named.as_ref().is_some_and(|named| named.pinned.is_some());
        let history = named
            .as_ref()
            .map(|named| history_of(named, self.drawn_at()));
        let playlist_name = named
            .as_ref()
            .map_or_else(|| format!("playlist {opened}"), |named| named.name.clone());
        let name = SharedString::from(playlist_name.clone());
        let under = named.as_ref().map_or_else(String::new, |named| {
            let mut parts: format::Parts<String> = smallvec![if narrowed {
                shown_of(shown, named).to_string()
            } else {
                counted(named).to_string()
            }];
            match rows {
                Rows::Matched => parts.push("fills itself from a search".to_owned()),
                Rows::Kept => parts.push("kept in order".to_owned()),
                Rows::Narrowed => parts.push("narrowed by the search".to_owned()),
                Rows::InHand => {}
            }
            if named.plays > 0 {
                parts.push(listing::times(named.plays).to_string());
            }
            parts.join(" · ")
        });
        let eyebrow = match rows {
            Rows::Matched => "SAVED SEARCH",
            Rows::InHand | Rows::Kept | Rows::Narrowed => "PLAYLIST",
        };

        let normal = named.as_ref().is_none_or(|named| named.query.is_none());
        let actions = kit::action_row()
            .w_full()
            .pt_2()
            .when(!entries.is_empty(), |bar| {
                let played = Arc::clone(entries);
                bar.child(
                    kit::button(
                        "play-playlist",
                        Some(Icon::Play),
                        "Play",
                        PLAY_HINT,
                        Tone::Primary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.plays_in_order(cx);
                        this.play_playlist(opened, &played, 0, !narrowed, cx);
                    })),
                )
                .child({
                    let played = Arc::clone(entries);
                    kit::button(
                        "shuffle-playlist",
                        Some(Icon::Shuffle),
                        "Shuffle",
                        SHUFFLE_HINT,
                        Tone::Outlined,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.play_playlist(
                            opened,
                            &played,
                            somewhere_in(played.len()),
                            !narrowed,
                            cx,
                        );
                        this.send(Command::SetShuffle(true), cx);
                    }))
                })
            })
            .when(naming.is_none() && !rows.are_edited(), |bar| {
                bar.child(
                    kit::button(
                        "revise-search",
                        Some(Icon::Search),
                        "Edit search",
                        FILLS_ITSELF_HINT,
                        Tone::Ghost,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.revise_search(opened, window, cx);
                    })),
                )
            })
            .when(normal && naming.is_none(), |bar| {
                bar.child(
                    kit::icon_button("add-songs-to-playlist", Icon::Plus, ADD_SONGS_HINT).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.add_songs_to_playlist(opened, cx);
                        }),
                    ),
                )
            })
            .when(
                matches!(rows, Rows::Narrowed) && !entries.is_empty(),
                |bar| {
                    bar.child(
                        kit::button(
                            "drop-shown",
                            Some(Icon::Discard),
                            "Drop shown",
                            DROP_SHOWN_HINT,
                            Tone::Ghost,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library
                                .update(cx, |library, cx| library.remove_matching(opened, cx));
                        })),
                    )
                },
            )
            .when(normal && !entries.is_empty(), |bar| {
                bar.child(
                    kit::icon_button("tidy-playlist", Icon::Tidy, TIDY_HINT).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.library
                                .update(cx, |library, cx| library.tidy_playlist(opened, cx));
                        },
                    )),
                )
                .child(
                    kit::icon_button("sort-playlist", Icon::Sort, SORT_HINT)
                        .when(sorting, |button| {
                            button
                                .bg(rgb(theme::hover()))
                                .text_color(rgb(theme::text()))
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.sort_a_playlist(opened, cx)),
                        ),
                )
            })
            .when(named.is_some(), |bar| {
                bar.child(
                    match pinned {
                        true => kit::lit_mark("pin-playlist", Icon::Pinned, UNPIN_HINT),
                        false => kit::icon_button("pin-playlist", Icon::Pin, PIN_HINT),
                    }
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.pin_playlist(opened, !pinned, cx));
                    })),
                )
                .child(
                    kit::icon_button("more-of-the-playlist", Icon::More, MORE_HINT).on_click(
                        cx.listener(move |this, event: &ClickEvent, _, cx| {
                            this.open_a_menu(playlist_menu(event.position(), opened, pinned), cx);
                        }),
                    ),
                )
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
                        Rows::InHand => MOVING_HINT,
                        Rows::Narrowed => NARROWED_HINT,
                        Rows::Kept | Rows::Matched => KEPT_HINT,
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
                    .child(kit::hero_title(name, self.hero_width.get())),
            )
            .child(
                kit::icon_button("select-playlist-title", Icon::Rename, RENAME_HINT)
                    .opacity(0.0)
                    .group_hover("playlist-title", |mark| mark.opacity(1.0))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.name_a_playlist(Naming::Rename(opened), window, cx);
                    })),
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
            .child(kit::measures_its_width(self.hero_width.clone()))
            .child(kit::measures_its_height(self.hero_height.clone()))
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
            .child(div().flex().child(
                kit::way_back("all-playlists", "Playlists", BACK_HINT).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.show_playlist(None, cx);
                    },
                )),
            ))
            .child(kit::hero().child(art).child(about))
            .when(!reads.is_empty(), |pane| pane.child(listing::reads(&reads)))
            .when(sorting, |pane| pane.child(self.rows_in_order(opened, cx)))
            .when_some(naming, |pane, naming| {
                pane.child(self.naming_row(naming, cx))
            })
    }

    fn rows_in_order(&self, opened: PlaylistId, cx: &mut Context<Self>) -> Div {
        let keeping = self.keeping;
        let mut keeps = sorting::shape("Keeps");
        for (index, (label, wanted, saying)) in KEEPING.into_iter().enumerate() {
            keeps = keeps.child(
                kit::chip(
                    ("row-keeping", index),
                    SharedString::new_static(label),
                    wanted == keeping,
                )
                .names(saying)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.keep_in_order(opened, wanted, cx);
                })),
            );
        }

        sorting::order_row(
            "row-order",
            "row-reading",
            self.row_order,
            self.row_reading,
            move |this, order, cx| this.put_in_order(opened, order, this.row_reading, cx),
            move |this, reading, cx| this.put_in_order(opened, this.row_order, reading, cx),
            cx,
        )
        .child(keeps)
    }

    pub(crate) fn playing_playlist(&self, cx: &App) -> Option<PlaylistId> {
        let queue = self.player.read(cx).state().queue_stamp;
        self.library.read(cx).playing_playlist(queue)
    }

    fn playing_row(&self, opened: PlaylistId, cx: &App) -> Option<usize> {
        if self.playing_playlist(cx) != Some(opened) {
            return None;
        }

        self.player.read(cx).state().loaded_position
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
            Some(track) => listing::scanned(track),
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
            self.acting_on(shift, index)
        } else {
            Span::one(index)
        };
        let position = entry.position;
        let dropping = if reaches {
            acting_on
        } else {
            Span::one(position)
        };
        let carried = Carried {
            shift,
            rows: acting_on,
            title: drawn.title.clone(),
        };
        let held = Arc::clone(entries);
        let last = entries.len();
        let acted_on = Reaching::acting_on(reaching, acting_on);
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
        let favourite = drawn.favourite;
        let location = entry.cut.location.clone();
        let span = entry.span();
        let title = drawn.title.clone();
        let artist = drawn.artist.clone();
        let listed = row(current)
            .id(listing::keyed_by("entry", &(&entry.cut, index)))
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
                    OPEN_ARTIST_HINT,
                    drawn.artist_id.map(Selection::Artist),
                    cx,
                )
                .flex_shrink()
                .ends_in_an_ellipsis(),
            ))
            .child(listing::format_cell(drawn.shape))
            .child(listing::heard(drawn.plays, drawn.played, self.drawn_at()))
            .child(listing::length_cell(drawn.length))
            .child(
                row_controls()
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
                        if acting_on.is_one_row() {
                            ADD_HINT
                        } else {
                            ADD_REACHED_HINT
                        },
                        move || Held::out_of(opened, Arc::clone(&holding)),
                        cx,
                    ))
                    .when(rows.are_edited(), |controls| {
                        controls.child(
                            kit::icon_button(
                                ("drop-entry", index),
                                Icon::Close,
                                if acting_on.is_one_row() {
                                    REMOVE_HINT
                                } else {
                                    REMOVE_REACHED_HINT
                                },
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.drop_rows(shift, dropping, cx);
                                },
                            )),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let extending = event.modifiers().shift && reaches;
                if reaches {
                    this.reach_at(shift, index, extending, cx);
                }
                if !extending {
                    this.play_playlist(opened, &held, index, !narrowed, cx);
                }
            }));
        let edited = rows.are_edited();
        let listed = menu::opens_a_menu(
            listed,
            move |this, at, cx| {
                let names = Called {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: this.album_named(album, &location, span, cx),
                };
                let taken = if reaches {
                    this.acting_on(shift, index)
                } else {
                    Span::one(position)
                };
                let put = Arc::clone(&menued);

                Menu::at(at)
                    .under(Icon::Play, menu::PLAY, "enter", {
                        let rows = Arc::clone(&playing);

                        move |this, _, cx| {
                            this.play_playlist(opened, &rows, index, !narrowed, cx);
                        }
                    })
                    .holds(move || Held::out_of(opened, Arc::clone(&put)))
                    .reaches(album, artist_id)
                    .when_some(scanned, |menu, track| {
                        menu.favours(Favoured::Track(track), favourite)
                    })
                    .offers_the_file(&location, names)
                    .when_some(scanned, Menu::shares)
                    .apart()
                    .when_some(edited.then_some(taken), |menu, taken| {
                        menu.under(
                            Icon::Discard,
                            menu::REMOVE_ROW,
                            "delete",
                            move |this, _, cx| this.drop_rows(shift, taken, cx),
                        )
                    })
            },
            cx,
        );

        if !rows.are_edited() {
            return listed;
        }
        if !rows.are_moved() {
            return reorder::marked(listed, reached);
        }
        reorder::movable(listed, index, carried, reached, cx)
    }

    pub(crate) fn playlist_picker(&self, holding: &Held, cx: &mut Context<Self>) -> AnyElement {
        let asked = holding.asked_as();
        let offered = Offered::beside(self.library.read(cx).lists(), holding.taken_from);
        let shown = offered.len().min(theme::PICKER_ROWS);

        let rows = uniform_list(
            "picker-rows",
            offered.len(),
            cx.processor(move |_this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| offered.row(index).map(|playlist| picked(playlist, cx)))
                    .collect()
            }),
        )
        .track_scroll(self.picker_rows.clone())
        .w_full()
        .h(theme::width(theme::row_height() * shown as f32));

        div()
            .id("playlist-picker")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme::scrim()))
            .on_click(cx.listener(|this, _, window, cx| this.stop_naming(window, cx)))
            .child(
                div()
                    .id("playlist-picker-panel")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .w(theme::width(theme::picker_width()))
                    .rounded_xl()
                    .bg(rgb(theme::surface()))
                    .border_1()
                    .border_color(rgb(theme::outline()))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0.0, 0.0, 0.0, 0.55),
                        offset: gpui::point(px(0.0), px(12.0)),
                        blur_radius: px(40.0),
                        spread_radius: px(0.0),
                    }])
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(kit::eyebrow("PLAYLIST"))
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
                                            self.picker_rows.clone(),
                                        )),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(kit::eyebrow("OR A NEW ONE"))
                            .child(self.name_field(cx)),
                    ),
            )
            .into_any_element()
    }

    fn undoing(&self, undoable: &Undoable, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "undo-edit",
            Some(Icon::Undo),
            "Undo",
            puts_back(undoable),
            Tone::Ghost,
        )
        .on_click(cx.listener(|this, _, _, cx| this.undo_edit(cx)))
    }

    fn redoing(&self, redoable: &Undoable, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "redo-edit",
            Some(Icon::Redo),
            "Redo",
            does_again(redoable),
            Tone::Ghost,
        )
        .on_click(cx.listener(|this, _, _, cx| this.redo_edit(cx)))
    }

    pub(crate) fn import_playlists(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(SharedString::new_static("Import")),
        });

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    let reported = this.update(cx, |this, cx| {
                        this.report(Notice::Trouble(NO_FILE_PICKER.to_owned()), cx);
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
            || format!("playlist {playlist}.m3u8"),
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
                        this.report(Notice::Trouble(NO_FILE_PICKER.to_owned()), cx);
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
        let (icon, saying) = match at {
            Placement::Next => (Icon::QueueNext, NEXT_HINT),
            Placement::Queued | Placement::At(_) => (Icon::QueueLast, QUEUE_HINT),
        };

        kit::icon_button(id, icon, saying).on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.queue(&holding(), at, cx);
        }))
    }

    pub(crate) fn add_control(
        &self,
        id: impl Into<ElementId>,
        saying: &'static str,
        holding: impl Fn() -> Held + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        kit::icon_button(id, Icon::Plus, saying).on_click(cx.listener(
            move |this, _, window, cx| {
                cx.stop_propagation();
                this.hold_for_a_playlist(holding(), window, cx);
            },
        ))
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
        let reached = self.reaches(Shift::Listing(Listed::Playlists), at);
        let albums = self.library.read(cx).pictured(id);
        let art = self.playlist_art(
            &playlist.name,
            playlist.query.is_some(),
            &albums,
            theme::row_cover(),
            cx,
        );

        let listed =
            tall_row(playing)
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
                                .when(pinned, |row| {
                                    row.child(kit::badge("PINNED", theme::accent()))
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(theme::text_xs()))
                                .text_color(rgb(theme::muted()))
                                .truncate()
                                .child(counted(playlist)),
                        ),
                )
                .child(listing::heard(playlist.plays, playlist.played, now))
                .child(
                    row_controls()
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
                        .child(control("play-saved", id, Icon::Play, PLAY_HINT).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                let entries = this.library.read(cx).entries_of(id);
                                this.plays_in_order(cx);
                                this.play_playlist(id, &entries, 0, true, cx);
                            }),
                        ))
                        .child(
                            control("shuffle-saved", id, Icon::Shuffle, SHUFFLE_HINT).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let entries = this.library.read(cx).entries_of(id);
                                    this.play_playlist(
                                        id,
                                        &entries,
                                        somewhere_in(entries.len()),
                                        true,
                                        cx,
                                    );
                                    this.send(Command::SetShuffle(true), cx);
                                }),
                            ),
                        )
                        .child(
                            control("next-saved", id, Icon::QueueNext, PLAYLIST_NEXT_HINT)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let entries = this.library.read(cx).entries_of(id);
                                    this.queue(&entries, Placement::Next, cx);
                                })),
                        )
                        .child(
                            control("last-saved", id, Icon::QueueLast, PLAYLIST_QUEUE_HINT)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let entries = this.library.read(cx).entries_of(id);
                                    this.queue(&entries, Placement::Queued, cx);
                                })),
                        )
                        .when(playlist.query.is_none(), |controls| {
                            controls.child(
                                control("add-songs", id, Icon::Plus, ADD_SONGS_HINT).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.add_songs_to_playlist(id, cx);
                                    }),
                                ),
                            )
                        })
                        .child(
                            control("drop-saved", id, Icon::Discard, DISCARD_HINT).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.library
                                        .update(cx, |library, cx| library.drop_playlist(id, cx));
                                }),
                            ),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.show_playlist(Some(id), cx);
                }));

        reorder::marked(
            menu::opens_a_menu(listed, move |_, at, _| playlist_menu(at, id, pinned), cx),
            reached,
        )
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
        let side = theme::grid_cover();
        let pinned = playlist.pinned.is_some();
        let searched = playlist.query.is_some();
        let reached = self.reaches(Shift::Listing(Listed::Playlists), at);
        let albums = self.library.read(cx).pictured(id);
        let art = self.playlist_art(&playlist.name, searched, &albums, side, cx);
        let cell_id = ElementId::from(("playlist-cell", id.get() as usize));
        let pointed = pointed::is_pointed_at(&cell_id);
        let about = about_a_cell(playlist, now);

        let cell = div()
            .id(cell_id.clone())
            .follows_the_pointer(cell_id)
            .group(PLAYLIST_CELL)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(OPEN_HINT)
            .child(
                div()
                    .relative()
                    .rounded(px(mosaic::ART_ROUNDING))
                    .group_hover(PLAYLIST_CELL, |frame| {
                        frame.shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0.0, 0.0, 0.0, 0.5),
                            offset: gpui::point(px(0.0), px(8.0)),
                            blur_radius: px(24.0),
                            spread_radius: px(0.0),
                        }])
                    })
                    .child(art)
                    .when(reached, |frame| frame.child(browser::reached_ring()))
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
                            .bg(theme::tinted(theme::background(), 0xb0)),
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.show_playlist(Some(id), cx);
            }));

        menu::opens_a_menu(cell, move |_, at, _| playlist_menu(at, id, pinned), cx)
    }

    fn pin_mark(&self, id: PlaylistId, pinned: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let mark = match pinned {
            true => kit::lit_mark(("pin-cell", id.get() as usize), Icon::Pinned, UNPIN_HINT),
            false => kit::icon_button(("pin-cell", id.get() as usize), Icon::Pin, PIN_HINT),
        };

        mark.on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.library
                .update(cx, |library, cx| library.pin_playlist(id, !pinned, cx));
        }))
    }

    fn play_on_the_art(&self, id: PlaylistId, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(("play-cell", id.get() as usize))
            .flex()
            .items_center()
            .justify_center()
            .size(px(PLAY_ON_THE_ART))
            .rounded_full()
            .bg(rgb(theme::accent()))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.45),
                offset: gpui::point(px(0.0), px(4.0)),
                blur_radius: px(12.0),
                spread_radius: px(0.0),
            }])
            .names(PLAY_HINT)
            .child(icons::icon(
                Icon::Play,
                PLAY_ON_THE_ART * PLAY_MARK_ON_THE_ART,
                theme::ink_over(theme::accent()),
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.plays_in_order(cx);
                let entries = this.library.read(cx).entries_of(id);
                this.play_playlist(id, &entries, 0, true, cx);
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
        let drawn: Vec<Arc<Image>> = albums
            .iter()
            .filter_map(|album| {
                self.library
                    .update(cx, |library, cx| library.cover(*album, Drawn::InAGrid, cx))
            })
            .take(mosaic::TILES)
            .collect();

        Mosaic {
            drawn: &drawn,
            ground: mosaic::named_accents(name),
            mark: if query { Icon::Search } else { Icon::Playlists },
            name,
        }
        .drawn(side, Framed::Alone)
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
        (true, _) => Some(("SEARCH", theme::repacked())),
        (false, true) => Some(("KEPT", theme::muted())),
        (false, false) => None,
    }
}

fn history_of(playlist: &Playlist, now: SystemTime) -> SharedString {
    let made = format!("Made {}", format::since(playlist.created, now));
    match playlist.played {
        Some(played) => SharedString::from(format!(
            "{made} · last played {}",
            format::since(played, now)
        )),
        None => SharedString::from(format!("{made} · never played")),
    }
}

fn about_a_cell(playlist: &Playlist, now: SystemTime) -> SharedString {
    let tracks = format::counted(playlist.entries as usize, "track", "tracks");
    match playlist.played {
        Some(played) => {
            SharedString::from(format!("{tracks} · played {}", format::since(played, now)))
        }
        None => match playlist.duration {
            Some(length) => SharedString::from(format!("{tracks} · {}", format::spanned(length))),
            None => SharedString::from(tracks),
        },
    }
}

fn playlist_menu(at: Point<Pixels>, id: PlaylistId, pinned: bool) -> Menu {
    Menu::at(at)
        .does(Icon::Play, menu::PLAY, move |this, _, cx| {
            let entries = this.library.read(cx).entries_of(id);
            this.play_playlist(id, &entries, 0, true, cx);
        })
        .does(Icon::QueueNext, menu::PLAY_NEXT, move |this, _, cx| {
            let entries = this.library.read(cx).entries_of(id);
            this.queue(&entries, Placement::Next, cx);
        })
        .does(Icon::QueueLast, menu::ADD_TO_QUEUE, move |this, _, cx| {
            let entries = this.library.read(cx).entries_of(id);
            this.queue(&entries, Placement::Queued, cx);
        })
        .apart()
        .does(pin_icon(pinned), pin_label(pinned), move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.pin_playlist(id, !pinned, cx));
        })
        .does(Icon::Rename, menu::RENAME, move |this, window, cx| {
            this.name_a_playlist(Naming::Rename(id), window, cx);
        })
        .does(Icon::Playlists, menu::DUPLICATE, move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.duplicate_playlist(id, cx));
        })
        .does(Icon::Export, menu::EXPORT, move |this, _, cx| {
            this.export_playlist(id, cx);
        })
        .does(Icon::Discard, menu::DISCARD, move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.drop_playlist(id, cx));
        })
}

const fn pin_icon(pinned: bool) -> Icon {
    match pinned {
        true => Icon::Pinned,
        false => Icon::Pin,
    }
}

const fn pin_label(pinned: bool) -> &'static str {
    match pinned {
        true => menu::UNPIN,
        false => menu::PIN,
    }
}

const fn pin_hint(pinned: bool) -> &'static str {
    match pinned {
        true => UNPIN_HINT,
        false => PIN_HINT,
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

fn puts_back(undoable: &Undoable) -> SharedString {
    let name = &undoable.name;
    let doing = match undoable.edit {
        Edit::Started => format!("Take {name} away again, as though it had never been started"),
        Edit::Renamed => format!("Name it {name} again"),
        Edit::Revised => format!("Put back the search {name} filled itself from"),
        Edit::Discarded => format!("Put {name} back, with everything it held"),
        Edit::Added | Edit::Copied | Edit::Imported => {
            format!("Take back out of {name} what that edit put in")
        }
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            format!("Put back in {name} what that edit took out")
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => {
            format!("Put the rows of {name} back in the order they were in")
        }
    };

    SharedString::from(format!(
        "{doing}. {UNDO_KEY}. {}",
        steps_behind(undoable.behind)
    ))
}

fn does_again(redoable: &Undoable) -> SharedString {
    let name = &redoable.name;
    let doing = match redoable.edit {
        Edit::Started => format!("Start {name} again, with everything it held"),
        Edit::Renamed => format!("Name it {name} again"),
        Edit::Revised => format!("Put back the search {name} was given"),
        Edit::Discarded => format!("Take {name} away again, with everything it holds"),
        Edit::Added | Edit::Copied | Edit::Imported => {
            format!("Put back into {name} what that edit had put in")
        }
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            format!("Take back out of {name} what that edit had taken out")
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => {
            format!("Put the rows of {name} back in the order that edit left them in")
        }
    };

    SharedString::from(format!(
        "{doing}. {REDO_KEY}. {}",
        steps_behind(redoable.behind)
    ))
}

fn steps_behind(steps: usize) -> String {
    match steps {
        0 => "It is the last one there is to walk".to_owned(),
        1 => "One more step is behind it".to_owned(),
        more => format!("{more} more steps are behind it"),
    }
}

fn capped(rows: Option<usize>) -> SharedString {
    match rows {
        Some(rows) => SharedString::from(rows.to_string()),
        None => SharedString::new_static("No cap"),
    }
}

fn shown_of(shown: usize, playlist: &Playlist) -> SharedString {
    SharedString::from(format!("{shown} of {} tracks", playlist.entries))
}

fn picked(playlist: &Playlist, cx: &mut Context<RootView>) -> AnyElement {
    let id = playlist.id;

    row(false)
        .id(("picker", id.get() as usize))
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
                .child(SharedString::from(playlist.name.clone())),
        )
        .child(kit::figure(counted(playlist)))
        .on_click(cx.listener(move |this, _, window, cx| {
            let Some(holding) = this.adding.take() else {
                return;
            };
            this.library.update(cx, |library, cx| {
                library.add_to_playlist(id, holding.rows.to_vec(), cx);
            });
            this.stop_naming(window, cx);
        }))
        .into_any_element()
}

fn counted(playlist: &Playlist) -> SharedString {
    let tracks = format::counted(playlist.entries as usize, "track", "tracks");

    match playlist.duration {
        Some(played) => SharedString::from(format!("{tracks} · {}", format::spanned(played))),
        None => SharedString::from(tracks),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::SystemTime};

    use resonate_core::{FrameSpan, Frames, MediaLocation, PlaylistId, Span};
    use resonate_library::{Cut, Playlist, PlaylistEntry};

    use super::{Offered, Reaching};

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
        let items = super::queue_items(&entries, &[]);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].span, Some(cut), "a cue row was queued whole");
        assert_eq!(items[1].span, None);
        assert_ne!(
            items[0].id, items[1].id,
            "two rows no scan has seen were minted one id"
        );
    }

    #[test]
    fn a_reach_is_read_off_the_list_once() {
        let entries = listed(8);
        let reaching = Reaching::of(&entries, Span::between(2, 4));

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
    fn a_reach_past_the_end_of_the_list_holds_nothing() {
        let entries = listed(3);

        assert!(
            Reaching::of(&entries, Span::between(2, 9))
                .entries
                .is_empty()
        );
    }

    #[test]
    fn only_the_rows_a_reach_was_read_for_share_it() {
        let entries = listed(8);
        let reaching = Reaching::of(&entries, Span::between(2, 4));

        assert!(Reaching::acting_on(Some(&reaching), Span::between(2, 4)).is_some());
        assert!(Reaching::acting_on(Some(&reaching), Span::one(3)).is_none());
        assert!(Reaching::acting_on(None, Span::between(2, 4)).is_none());
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
