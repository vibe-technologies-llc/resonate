use std::{
    cmp::Reverse,
    slice,
    sync::Arc,
    time::{Duration, SystemTime},
};

use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, ElementId, FontWeight, MouseButton,
    MouseDownEvent, ObjectFit, Pixels, Point, SharedString, Stateful, anchored, deferred, div,
    hsla, img, point, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{AlbumId, ArtistId, ArtistsDrawn, ReleaseTrackId, TrackId};
use resonate_engine::Placement;
use resonate_library::{
    Album, AlbumFound, AlbumNotHeld, Artist, ArtistDetail, ArtistFound, ArtistTotals, Column, Cut,
    Favoured, Found, Genre, GroupRelease, HeldMedium, HeldReleaseTrack, Link, Lit, Mbid, Measured,
    MissingTrack, Performer, PlaylistEntry, Recording, RecordingRelease, ReleaseDetail, Service,
    Track, in_the_order_worth_offering,
};
use smallvec::smallvec;

use crate::{
    Drawn, LibraryModel, ListedRow, Portrayed, Pressings, ResonateApp, Selection,
    downloads::Fetching,
    format,
    icons::{self, Icon},
    models::{Notice, Picture},
    motion, theme, toast,
    views::{
        hint::Names,
        kit::{self, EndsInAnEllipsis, KeepsItsWidth, Press, Tone},
        listing::{self, Pictured},
        menu::{self, Called, Menu},
        missing::MissingShows,
        playlists::{ADD_SONG_HINT, FINISH_ADDING_HINT, Held, Naming, ROW_GROUP, SAVE_SEARCH_HINT},
        pointed::{self, LitUnderThePointer},
        reorder::{self, Listed, Shift},
        root::{Deleting, Magnified, Pane, RootView, empty, framed_cover, listed, row, tall_row},
        scrollbar::{SHELF_INSET, Scrollbars},
        sorting,
        transport::COVER_HINT,
    },
};

const SEVERAL: &str = "Various artists";
const FORGET_THE_MATCH: &str = "Not this record";
const OTHER_PRESSINGS: &str = "Other pressings";
const OTHER_PRESSINGS_HINT: &str =
    "List every pressing of this record MusicBrainz holds, and take the one these files are";
const ASKING_FOR_PRESSINGS: &str = "Asking MusicBrainz for the pressings…";
const NO_PRESSINGS: &str = "MusicBrainz named no pressings";
const FIND_THE_RECORD: &str = "Find the record";
const FIND_THE_RECORD_HINT: &str = "Ask MusicBrainz for the releases named like this album, and \
                                    take the one these files are";
const ASKING_FOR_RELEASES: &str = "Asking MusicBrainz for releases named like this…";
const NO_RELEASES: &str = "MusicBrainz named no release like this";
const TAKE_THE_PRESSING_HINT: &str = "Take this pressing for the album in place of the one in use";
const PRESSING_IN_USE_HINT: &str = "The pressing the album is matched to now";
const PRESSINGS_SHOWN: usize = 12;
const FORGET_THE_MATCH_HINT: &str =
    "The lookup took the wrong release: forget it, and never take it for this album again";
const FORGET_THE_MATCH_ARMED: &str = "Press again to forget the match";
const FORGET_THE_MATCH_ARMED_HINT: &str = "Takes away the release, its rows and what it linked, \
     and the next lookup asks again without it";

const OTHER_COPIES_HINT: &str =
    "Also held in other formats; this is the best of them, and the row's menu plays the others";

const WAY_BACK_HINT: &str = keyed!("Back to where this was opened from", key!(leave));

const ORDER_HINT: &str = "Choose what this listing is put in order by";

const PLAY_ALL_HINT: &str = "Play every track listed here in order, in place of the queue";

const SHUFFLE_ALL_HINT: &str = "Play every track listed here, shuffled";

pub(crate) const OPEN_ALBUM_HINT: &str = "See the tracks on this album";

pub(crate) const OPEN_ARTIST_HINT: &str = "See every track by this artist";

const INSTEAD_HINT: &str = "Search for the spelling the catalog holds instead";

const SUNG_HINT: &str = "Search the lyrics for these words, in the order they were typed";

const WANT_HINT: &str = "Mark this track wanted, so a provider can be asked for it";

const UNWANT_HINT: &str = "Stop wanting this track";

const DISMISS_MISSING_HINT: &str =
    "Take this track off the Missing list, and stop wanting it if it was wanted";

const WANT_FOUND_HINT: &str = "Download this song: its release is added to the catalog and the \
                               providers are asked for it. Right-click to choose the release";

const FETCH_FOUND_HINT: &str = "Download this song: its release is added to the catalog and the \
                                providers are asked for it now, the sidebar following how it goes";

const ARTIST_FOUND_CELL_WIDENING: f32 = 1.8;

const OPENING: &str = "Opening…";

const OPEN_ARTIST_FOUND_HINT: &str =
    "Open this artist to see their releases and download the songs you want";

const OPEN_ALBUM_NOT_HELD_HINT: &str = "Open this album to see its songs and download the ones you \
                                        want";

const GET_ALBUM_REST_HINT: &str = "Ask the providers for every missing track on this album";

const NOT_HELD_HEADING: &str = "Not in your library";

const FETCH_FOUND_AGAIN_HINT: &str = "Ask the providers for this song again";

const DELETE_FROM_DISK: &str = "Delete from disk…";

const RELEASES_OFFERED: usize = 10;
const PLACE_ON: &str = "Place on a release…";
const READ_THE_REST: &str = "Read the rest";
const READ_THE_REST_HINT: &str =
    "Ask MusicBrainz for the releases past the first thousand this artist is credited on";
const NOTHING_PLACES_IT: &str = "Nothing has identified this track, so it is on no release yet";

const UNHELD_COVER: f32 = 0.45;

const UNHELD_HINT: &str = "See what this artist has released that the library does not hold";

const RECORD_HINT: &str = "About this record";

const GENRES_HINT: &str = "Genres";

const RECORD_LABEL: f32 = 112.0;

pub(crate) const FAVOUR_HINT: &str = "Keep this among your favourites";

pub(crate) const UNFAVOUR_HINT: &str = "Take this out of your favourites";

pub(crate) const ROW_CONTROLS: usize = 4;

pub(crate) const TRACK_CONTROLS: usize = 3;

pub(crate) const TRACK_ADD_CONTROLS: usize = 2;

const CONTROL_GAP: f32 = 2.0;

pub(crate) fn controls_width(controls: usize) -> f32 {
    theme::row_control() * controls as f32 + CONTROL_GAP * controls.saturating_sub(1) as f32
}

const CELL_GROUP: &str = "album-cell";

const SHELF_AT_MOST: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Plays {
    TheseRows,
    AsTheListingIsDrawn { in_an_album: bool },
}

impl RootView {
    pub(crate) fn favour_mark(
        &self,
        id: impl Into<ElementId>,
        what: Favoured,
        already: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let saying = match already {
            true => UNFAVOUR_HINT,
            false => FAVOUR_HINT,
        };
        let mark = kit::star(id, already, SharedString::from(format!("{what:?}")), saying);

        mark.on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.library
                .update(cx, |library, cx| library.favour(what, !already, cx));
        }))
    }

    pub(crate) fn favour_mark_under(
        &self,
        id: impl Into<ElementId>,
        what: Favoured,
        already: bool,
        group: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.favour_mark(id, what, already, cx)
            .when(!already, |mark| {
                mark.opacity(0.0)
                    .group_hover(group, |mark| mark.opacity(1.0))
            })
    }

    pub(crate) fn albums(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let albums = library.albums();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();
        let nothing = albums.is_empty();
        let counted = library.albums_counted() as usize;
        let beyond = self.albums_found_beyond(cx);
        let found_nothing = (nothing && beyond.is_none()).then(|| {
            self.nothing_beyond(cx).unwrap_or_else(|| {
                self.nothing_matched(
                    Icon::Albums,
                    if narrowed {
                        "No albums match."
                    } else {
                        "No albums yet."
                    },
                    (!narrowed).then_some("Add a music folder from Settings to scan one in."),
                    cx,
                )
            })
        });
        let columns = self.grid_columns();
        let held = albums.len();
        let rows = held.div_ceil(columns.max(1));
        let measured = self.grid_width.clone();
        let laid_out = self.grid_width.get() > px(0.0);
        if laid_out {
            self.land_where_it_was_left(rows);
        }

        let heading = self.search_heading(cx).unwrap_or_else(|| {
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
                                .child(kit::eyebrow("LIBRARY"))
                                .child(kit::title("Albums"))
                                .child(kit::subtitle(format::counted(counted, "album", "albums"))),
                        )
                        .when(!reads.is_empty(), |row| row.child(listing::reads(&reads)))
                        .child(kit::actions().child(self.orders_a_listing("order-albums", cx))),
                )
                .when(self.ordering, |heading| {
                    heading.child(self.albums_in_order(cx))
                })
        });

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
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
                                                        let reached = this.reaches(
                                                            Shift::Listing(Listed::Albums),
                                                            at,
                                                        );
                                                        this.album_cell(album, reached, cx)
                                                    });
                                                drawn.push(
                                                    div()
                                                        .flex()
                                                        .gap(px(theme::grid_gap()))
                                                        .px_6()
                                                        .h(px(theme::grid_row()))
                                                        .children(cells),
                                                );
                                            }
                                            drawn
                                        },
                                    ),
                                )
                                .track_scroll(self.album_rows.clone())
                                .h_full()
                                .w_full(),
                            )
                        })
                        .child(
                            Scrollbars::of(cx).vertical("album-scrollbar", self.album_rows.clone()),
                        ),
                )
            })
            .when_some(beyond, Div::child)
            .into_any_element()
    }

    fn albums_found_beyond(&self, cx: &mut Context<Self>) -> Option<Div> {
        let found = self.library.read(cx).albums_found();
        (self.search_in_front(cx).is_some() && !found.is_empty())
            .then(|| self.albums_found_strip(&found, cx))
    }

    fn artists_found_beyond(&self, cx: &mut Context<Self>) -> Option<Div> {
        let found = self.library.read(cx).artists_found();
        (self.search_in_front(cx).is_some() && !found.is_empty())
            .then(|| self.artists_found_strip(&found, cx))
    }

    pub(crate) fn grid_columns(&self) -> usize {
        let width = f32::from(self.grid_width.get()) - 48.0;
        let cell = theme::grid_cover() + theme::grid_gap();
        (((width + theme::grid_gap()) / cell).floor() as usize).max(1)
    }

    fn album_cell(&self, album: &Album, reached: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        self.album_cell_captioned(album, theme::grid_cover(), Caption::ByArtist, reached, cx)
    }

    pub(crate) fn album_cell_at(
        &self,
        album: &Album,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.album_cell_captioned(album, side, Caption::ByArtist, false, cx)
    }

    fn album_cell_captioned(
        &self,
        album: &Album,
        side: f32,
        caption: Caption,
        reached: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = album.id;
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
            .or_else(|| (album.artist_count > 1).then(|| SharedString::new_static(SEVERAL)))
            .filter(|_| match caption {
                Caption::ByArtist => true,
                Caption::Beside(artist) => album.artist_id != Some(artist),
            });
        let counted = format::counted(album.track_count as usize, "track", "tracks");
        let beside = match (caption, artist.is_some(), album.year) {
            (Caption::Beside(_), false, Some(year)) => {
                Some(SharedString::from(format!("{year} · {counted}")))
            }
            (_, _, Some(year)) => Some(SharedString::from(year.to_string())),
            (_, true, None) => None,
            (_, false, None) => Some(SharedString::from(counted)),
        };
        let parted = artist.is_some() && beside.is_some();
        let under = div()
            .flex()
            .min_w(px(0.0))
            .text_size(px(theme::text_xs()))
            .text_color(rgb(theme::muted()))
            .when_some(artist, |line, artist| {
                line.child(
                    self.opens(
                        ("album-artist", id.get() as usize),
                        listing::matched(artist, lit_artist),
                        OPEN_ARTIST_HINT,
                        owner.map(Selection::Artist),
                        cx,
                    )
                    .ends_in_an_ellipsis(),
                )
            })
            .when(parted, |line| {
                line.child(div().flex_none().px_1().child("·"))
            })
            .when_some(beside, |line, beside| {
                line.child(div().flex_none().whitespace_nowrap().child(beside))
            });

        let cell_id = ElementId::from(("album-cell", id.get() as usize));
        let pointed = pointed::is_pointed_at(&cell_id);

        let cell = div()
            .id(cell_id.clone())
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(OPEN_ALBUM_HINT)
            .child(
                div()
                    .relative()
                    .rounded(px(8.0))
                    .group_hover(CELL_GROUP, |frame| {
                        frame.shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0.0, 0.0, 0.0, 0.5),
                            offset: gpui::point(px(0.0), px(8.0)),
                            blur_radius: px(24.0),
                            spread_radius: px(0.0),
                        }])
                    })
                    .child(cover)
                    .when(reached, |frame| frame.child(reached_ring()))
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
                        .bg(theme::tinted(theme::background(), 0xb0)),
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.opened(Selection::Album(id), cx);
            }));

        menu::opens_a_menu(
            cell,
            move |_, at, _| album_menu(at, id, owner).favours(Favoured::Album(id), favourite),
            cx,
        )
    }

    pub(crate) fn nothing_matched(
        &mut self,
        icon: Icon,
        message: &'static str,
        more: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(instead) = self.library.read(cx).instead().map(ToOwned::to_owned) else {
            return match self.sung_offer(Tone::Outlined, cx) {
                Some(offer) => kit::empty_offering(icon, message, more, offer),
                None => empty(icon, message, more),
            };
        };
        let offered = instead.clone();

        kit::empty_offering(
            icon,
            message,
            more,
            kit::button(
                "search-instead",
                Some(Icon::Search),
                format!("Did you mean {instead}?"),
                INSTEAD_HINT,
                Tone::Outlined,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.search_instead(offered.clone(), window, cx);
            })),
        )
    }

    pub(crate) fn sung_offer(&self, tone: Tone, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let sung = self.library.read(cx).sung()?.clone();
        let offered = sung.query;

        Some(
            kit::button(
                "search-the-lyrics",
                Some(Icon::Lyrics),
                format!(
                    "Sung in {}",
                    format::counted(sung.tracks as usize, "track", "tracks")
                ),
                SUNG_HINT,
                tone,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.search_instead(offered.clone(), window, cx);
            })),
        )
    }

    pub(crate) fn artists(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let artists = self.library.read(cx).artists();
        let narrowed = self.library.read(cx).narrowing().is_some();
        let reads = self.library.read(cx).search().reads();
        let selected = self.library.read(cx).selection();
        let nothing = artists.is_empty();
        let counted = self.library.read(cx).artists_counted() as usize;
        let held = artists.len();
        let drawn = self.artists_drawn;
        if drawn == ArtistsDrawn::List {
            self.land_where_it_was_left(held);
        }

        let beyond = self.artists_found_beyond(cx);
        let found_nothing = (nothing && beyond.is_none()).then(|| {
            self.nothing_beyond(cx).unwrap_or_else(|| {
                self.nothing_matched(
                    Icon::Artists,
                    if narrowed {
                        "No artists match."
                    } else {
                        "No artists yet."
                    },
                    (!narrowed).then_some("Add a music folder from Settings to scan one in."),
                    cx,
                )
            })
        });

        let heading = self.search_heading(cx).unwrap_or_else(|| {
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
                                .child(kit::eyebrow("LIBRARY"))
                                .child(kit::title("Artists"))
                                .child(kit::subtitle(format::counted(
                                    counted, "artist", "artists",
                                ))),
                        )
                        .when(!reads.is_empty(), |row| row.child(listing::reads(&reads)))
                        .child(
                            kit::actions()
                                .child(self.artists_drawn_as(drawn, cx))
                                .child(self.orders_a_listing("order-artists", cx)),
                        ),
                )
                .when(self.ordering, |heading| {
                    heading.child(self.artists_in_order(cx))
                })
        });
        let grid =
            (!nothing && drawn == ArtistsDrawn::Grid).then(|| self.artist_grid(&artists, cx));

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .when_some(found_nothing, |pane, nothing| pane.child(nothing))
            .when_some(grid, Div::child)
            .when(!nothing && drawn == ArtistsDrawn::List, |pane| {
                pane.child(
                    Scrollbars::of(cx).around(
                        "artist-scrollbar",
                        self.artist_rows.clone(),
                        uniform_list(
                            "artists",
                            held,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                this.reach_further(range.end, held, cx);
                                let mut rows = Vec::new();
                                for index in range {
                                    let Some(artist) = artists.get(index) else {
                                        continue;
                                    };
                                    let id = artist.id;
                                    let chosen = selected == Selection::Artist(id);
                                    let favourite = this
                                        .library
                                        .read(cx)
                                        .favours(Favoured::Artist(id), artist.favourite.is_some());
                                    let reached =
                                        this.reaches(Shift::Listing(Listed::Artists), index);

                                    let listed = reorder::marked(
                                        tall_row(chosen)
                                            .id(("artist", id.get()))
                                            .group(ROW_GROUP)
                                            .cursor_pointer()
                                            .hover(|entry| entry.bg(rgb(theme::hover())))
                                            .names(OPEN_ARTIST_HINT)
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
                                                format::counted(
                                                    artist.album_count as usize,
                                                    "album",
                                                    "albums"
                                                ),
                                                format::counted(
                                                    artist.track_count as usize,
                                                    "track",
                                                    "tracks"
                                                )
                                            )))
                                            .child(this.favour_mark_under(
                                                ("artist-favourite", index),
                                                Favoured::Artist(id),
                                                favourite,
                                                ROW_GROUP,
                                                cx,
                                            ))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.opened(Selection::Artist(id), cx);
                                            })),
                                        reached,
                                    );
                                    rows.push(menu::opens_a_menu(
                                        listed,
                                        move |_, at, _| {
                                            artist_menu(at, id)
                                                .favours(Favoured::Artist(id), favourite)
                                        },
                                        cx,
                                    ));
                                }
                                rows
                            }),
                        )
                        .track_scroll(self.artist_rows.clone())
                        .h_full()
                        .w_full(),
                    ),
                )
            })
            .when_some(beyond, Div::child)
            .into_any_element()
    }

    pub(crate) fn tracks(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tracks = self.library.read(cx).listing();
        let narrowed = self.library.read(cx).narrowing().is_some();
        let scoped = self.library.read(cx).selection() != Selection::Everything;
        if tracks.is_empty() && !narrowed && !scoped {
            return empty(
                Icon::Tracks,
                "No tracks yet.",
                Some("Add a music folder from Settings to scan one in."),
            );
        }
        let playing = self.playing_now(cx).track;
        let rows = self.library.read(cx).rows();
        let nothing = tracks.is_empty() && rows.is_empty();
        let found_nothing = nothing.then(|| {
            self.nothing_beyond(cx)
                .unwrap_or_else(|| self.nothing_matched(Icon::Tracks, "No tracks match.", None, cx))
        });
        let heading = self.heading(cx);
        let in_an_album = matches!(self.library.read(cx).selection(), Selection::Album(_));
        let rowed = !rows.is_empty();
        let release_tracks = self.library.read(cx).release_tracks();
        let found = self.library.read(cx).found();
        let media: Arc<[HeldMedium]> = self
            .library
            .read(cx)
            .release()
            .map(|release| release.media.clone())
            .unwrap_or_default()
            .into();
        let held = tracks.len();
        let listed = if rowed { rows.len() } else { held };
        self.land_where_it_was_left(listed);
        let records = match self.library.read(cx).selection() {
            Selection::Artist(artist) => {
                let library = self.library.read(cx);
                let held = library.artist_albums().len() + library.albums_not_held().len();
                (self.artist_shows.within(held) == ArtistShows::Records)
                    .then(|| self.artist_records(artist, cx))
            }
            Selection::Everything | Selection::Album(_) => None,
        };
        let listing_shown = records.is_none();
        let nothing = nothing && listing_shown;
        let found_nothing = found_nothing.filter(|_| listing_shown);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading.flex_none())
            .when_some(records, |pane, records| {
                pane.child(Scrollbars::of(cx).around(
                    "artist-records-scrollbar",
                    self.artist_records_scroll.clone(),
                    records,
                ))
            })
            .when(!nothing && listing_shown, |pane| {
                pane.child(listing::columns(
                    "#",
                    !in_an_album,
                    if self.adding_songs_to.is_some() {
                        TRACK_ADD_CONTROLS
                    } else {
                        TRACK_CONTROLS
                    },
                    sorting::tracks_sorted(self, cx),
                    &self.columns_fit,
                    cx,
                ))
            })
            .when_some(found_nothing, |pane, nothing| pane.child(nothing))
            .when(!nothing && listing_shown, |pane| {
                pane.child(
                    Scrollbars::of(cx).around(
                        "track-scrollbar",
                        self.track_rows.clone(),
                        uniform_list(
                            "tracks",
                            listed,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                this.reach_further(range.end, held, cx);
                                let mut drawn = Vec::new();
                                for index in range {
                                    let row = if rowed {
                                        rows.get(index).copied()
                                    } else {
                                        Some(ListedRow::Held(index))
                                    };
                                    match row {
                                        Some(ListedRow::Disc(disc)) => {
                                            drawn.push(
                                                disc_heading(disc, &media).into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Held(held)) => {
                                            let Some(track) = tracks.get(held) else {
                                                continue;
                                            };
                                            let reached =
                                                this.reaches(Shift::Listing(Listed::Tracks), index);
                                            drawn.push(
                                                reorder::marked(
                                                    this.track_row(
                                                        &tracks,
                                                        held,
                                                        track,
                                                        playing == Some(track.id),
                                                        Plays::AsTheListingIsDrawn { in_an_album },
                                                        cx,
                                                    ),
                                                    reached,
                                                )
                                                .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Missing(missing)) => {
                                            let Some(row) = release_tracks.get(missing) else {
                                                continue;
                                            };
                                            drawn.push(
                                                this.unheld_row(missing, Unheld::from(row), cx)
                                                    .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::NotHeld(songs)) => {
                                            drawn.push(
                                                Self::songs_not_held_heading(songs)
                                                    .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Found(at)) => {
                                            let Some(row) = found.get(at) else {
                                                continue;
                                            };
                                            drawn.push(
                                                this.found_row(at, row, cx).into_any_element(),
                                            );
                                        }
                                        None => {}
                                    }
                                }
                                drawn
                            }),
                        )
                        .track_scroll(self.track_rows.clone())
                        .h_full()
                        .w_full(),
                    ),
                )
            })
            .into_any_element()
    }

    pub(crate) fn track_row(
        &self,
        tracks: &Arc<[Track]>,
        index: usize,
        track: &Track,
        playing: bool,
        plays: Plays,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let in_an_album = plays == Plays::AsTheListingIsDrawn { in_an_album: true };
        let played = Arc::clone(tracks);
        let menued = Arc::clone(tracks);
        let adding_to = self.adding_songs_to;
        let id = track.id;
        let favourite = self
            .library
            .read(cx)
            .favours(Favoured::Track(id), track.favourite.is_some());
        let (lit_title, lit_artist) = {
            let search = self.library.read(cx).search();
            (
                search.lit(&track.title, Column::Title),
                track
                    .artist
                    .as_deref()
                    .map_or_else(Lit::new, |name| search.lit(name, Column::Artist)),
            )
        };
        let fitted = self.columns_fit.shown();
        let number = match (in_an_album, track.track_number) {
            (true, Some(number)) => SharedString::from(number.to_string()),
            (true, None) => SharedString::new_static(""),
            (false, _) => SharedString::from((index + 1).to_string()),
        };

        let row = row(playing)
            .id(("track", id.get()))
            .debug_selector(move || format!("track-{}", id.get()))
            .group(ROW_GROUP)
            .cursor_pointer()
            .hover(|entry| entry.bg(rgb(theme::hover())))
            .child(if playing {
                listing::playing_mark()
            } else {
                listing::number_cell(number)
            })
            .when(!in_an_album, |row| {
                row.child(self.cover(
                    Pictured::Track {
                        album: track.album_id,
                        file: &track.location,
                    },
                    cx,
                ))
            })
            .child(if track.alternatives > 0 {
                listing::tagged_title_cell(
                    SharedString::from(track.title.clone()),
                    lit_title,
                    playing,
                    kit::tag(format!("+{}", track.alternatives))
                        .id(("track-copies", index))
                        .names(OTHER_COPIES_HINT),
                )
            } else {
                listing::title_cell(SharedString::from(track.title.clone()), lit_title, playing)
            })
            .child(listing::artist_cell(
                self.opens(
                    ("track-artist", index),
                    listing::matched(
                        SharedString::from(track.artist.clone().unwrap_or_default()),
                        lit_artist,
                    ),
                    OPEN_ARTIST_HINT,
                    track.artist_id.map(Selection::Artist),
                    cx,
                )
                .flex_shrink()
                .ends_in_an_ellipsis(),
            ))
            .when(fitted.format, |row| {
                row.child(listing::format_cell(Some((track.codec, track.spec))))
            })
            .when(fitted.heard, |row| {
                row.child(listing::heard(track.plays, track.played, self.drawn_at()))
            })
            .child(listing::length_cell(SharedString::from(
                track
                    .duration
                    .map(|duration| format::clock(duration, track.spec.rate))
                    .unwrap_or_default(),
            )))
            .child(
                trailing_controls()
                    .child(self.favour_mark_under(
                        ("track-favourite", index),
                        Favoured::Track(id),
                        favourite,
                        ROW_GROUP,
                        cx,
                    ))
                    .child(
                        row_controls()
                            .when_some(adding_to, |controls, target| {
                                let cut = Cut::of(track);
                                controls.child(
                                    kit::icon_button(
                                        ("add-song-to-playlist", index),
                                        Icon::Plus,
                                        ADD_SONG_HINT,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.library.update(cx, |library, cx| {
                                                library.add_to_playlist(
                                                    target,
                                                    vec![cut.clone()],
                                                    cx,
                                                );
                                            });
                                        },
                                    )),
                                )
                            })
                            .when(adding_to.is_none(), |controls| {
                                controls
                                    .child(self.queue_control(
                                        ("track-next", index),
                                        Placement::Next,
                                        queueing(tracks, index),
                                        cx,
                                    ))
                                    .child(self.queue_control(
                                        ("track-last", index),
                                        Placement::Queued,
                                        queueing(tracks, index),
                                        cx,
                                    ))
                            }),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| match plays {
                Plays::TheseRows => this.play(&played, index, cx),
                Plays::AsTheListingIsDrawn { .. } => {
                    let Some((drawn, start)) = this.library.read(cx).played_from_held(index) else {
                        return;
                    };
                    this.play_the_listing_from(&drawn, start, window, cx);
                }
            }));

        menu::opens_a_menu(
            row,
            move |this, at, cx| {
                let Some(track) = menued.get(index) else {
                    return Menu::at(at);
                };
                let track_id = track.id;
                let hidden = track.hidden;
                let (icon, hiding) = if hidden {
                    (Icon::Undo, "Show in library")
                } else {
                    (Icon::Discard, "Hide from library")
                };
                let favourite = this
                    .library
                    .read(cx)
                    .favours(Favoured::Track(track.id), track.favourite.is_some());
                let queued = listed(slice::from_ref(track));
                let held = Cut::of(track);
                let delivered = track.delivered.then(|| track.clone());
                let names = Called {
                    title: SharedString::from(track.title.clone()),
                    artist: SharedString::from(track.artist.clone().unwrap_or_default()),
                    album: this.album_named(track.album_id, &track.location, track.span, cx),
                };
                let others = (track.alternatives > 0).then(|| {
                    (
                        track.clone(),
                        this.library.read(cx).alternatives_of(track.id),
                    )
                });

                let placeable = this.library.read(cx).can_enrich().then_some(track_id);
                let deleting = Deleting::of(track);
                let in_the_queue = this
                    .is_in_the_queue(&track.location, track.span, cx)
                    .then(|| (track.location.clone(), track.span));

                Menu::at(at)
                    .queues(move || Arc::clone(&queued))
                    .when_some(in_the_queue, |menu, (location, span)| {
                        menu.does(Icon::Discard, menu::TAKE_OUT, move |this, _, cx| {
                            this.take_out_of_the_queue(&location, span, cx);
                        })
                    })
                    .holds(move || Held::of(Arc::from([held.clone()])))
                    .when_some(others, Menu::offers_the_other_copies)
                    .reaches(Some(track.id), track.album_id, track.artist_id)
                    .favours(Favoured::Track(track.id), favourite)
                    .offers_the_file(&track.location, names)
                    .shares(track.id)
                    .when_some(placeable, |menu, track| {
                        menu.does(Icon::Disc, PLACE_ON, move |this, _, cx| {
                            this.offer_releases_to_place_on(track, at, cx);
                        })
                    })
                    .apart()
                    .does(icon, hiding, move |this, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.hide_track(track_id, !hidden, cx);
                        });
                    })
                    .when_some(delivered, |menu, delivered| {
                        menu.does(Icon::Close, menu::FORGET_DELIVERY, move |this, _, cx| {
                            this.library.update(cx, |library, cx| {
                                library.forget_delivered(&delivered, cx);
                            });
                        })
                    })
                    .does(Icon::Delete, DELETE_FROM_DISK, move |this, _, cx| {
                        this.ask_to_delete(deleting.clone(), cx);
                    })
            },
            cx,
        )
    }

    pub(crate) fn unheld_row(
        &self,
        index: usize,
        unheld: Unheld,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Unheld {
            asks,
            number,
            title,
            artist,
            length,
            beside,
        } = unheld;
        let download = match (&beside, &asks) {
            (Beside::AnAlbum, Asks::Row(release_track))
                if self.library.read(cx).wanted(*release_track).is_none() =>
            {
                Some(*release_track)
            }
            _ => None,
        };
        let dismissed = match (&beside, &asks) {
            (Beside::ARun, Asks::Row(release_track)) => Some(*release_track),
            _ => None,
        };
        let id = match &asks {
            Asks::Row(release_track) => {
                ElementId::NamedInteger("unheld-row".into(), release_track.get())
            }
            Asks::Found(found) => listing::keyed_by("found", &found.recording),
        };
        let fetching = match (&beside, &asks) {
            (Beside::AnAlbum, Asks::Row(release_track)) => {
                self.library.read(cx).fetching_want(*release_track)
            }
            _ => None,
        };
        let told = match &asks {
            Asks::Row(release_track) => self.library.read(cx).told_want(*release_track),
            Asks::Found(found) => self.library.read(cx).told_found(found),
        };
        let controls = match (&beside, self.adding_songs_to.is_some()) {
            (Beside::ARun, _) => ROW_CONTROLS,
            (_, true) => TRACK_ADD_CONTROLS,
            (_, false) => TRACK_CONTROLS,
        };
        let performer = match &asks {
            Asks::Found(found) => found.performer.clone(),
            Asks::Row(_) => None,
        };
        let mark = self.want_mark(asks, cx);
        let fitted = self.columns_fit.shown();
        let (title, lit_title, lit_artist) = match &beside {
            Beside::AnAlbum | Beside::ARun => (title, Lit::new(), Lit::new()),
            Beside::ASearch { .. } => {
                let search = self.library.read(cx).search();
                (
                    title.clone(),
                    search.lit(&title, Column::Title),
                    search.lit(&artist, Column::Artist),
                )
            }
        };
        let artist = listing::matched(artist, lit_artist);

        let row = row(false)
            .child(listing::number_cell(number))
            .when_some(beside.pictured(), |row, sleeve| {
                row.child(match sleeve {
                    Sleeve::Released(release) => {
                        let art = self.library.update(cx, |library, cx| {
                            library.released_cover(release, Drawn::InARow, cx)
                        });
                        match art {
                            Some(art) => {
                                framed_cover(Some(art), theme::row_cover()).opacity(UNHELD_COVER)
                            }
                            None => unheld_cover(theme::row_cover()),
                        }
                    }
                    Sleeve::Unknown => unheld_cover(theme::row_cover()),
                })
            })
            .child(
                listing::title_cell(title, lit_title, false)
                    .debug_selector(move || format!("unheld-title-{index}"))
                    .text_color(rgb(theme::faint())),
            )
            .child(
                listing::artist_cell(
                    self.performed_by(("missing-artist", index), artist, performer, cx)
                        .debug_selector(move || format!("unheld-artist-{index}"))
                        .flex_shrink()
                        .ends_in_an_ellipsis(),
                )
                .text_color(rgb(theme::faint())),
            )
            .when_some(
                match beside {
                    Beside::AnAlbum => Some(listing::format_cell(None).when_some(
                        fetching,
                        |cell, fetching| {
                            cell.child(
                                div()
                                    .text_size(px(theme::text_sm()))
                                    .text_color(rgb(fetching_colour(fetching)))
                                    .truncate()
                                    .ends_in_an_ellipsis()
                                    .child(told.clone().unwrap_or_else(|| fetching.saying())),
                            )
                        },
                    )),
                    Beside::ARun => None,
                    Beside::ASearch {
                        on, fetching: None, ..
                    } => Some(
                        listing::format_cell(None).child(
                            kit::figure(on)
                                .text_color(rgb(theme::faint()))
                                .truncate()
                                .ends_in_an_ellipsis(),
                        ),
                    ),
                    Beside::ASearch {
                        fetching: Some(fetching),
                        ..
                    } => Some(
                        listing::format_cell(None).child(
                            div()
                                .text_size(px(theme::text_sm()))
                                .text_color(rgb(fetching_colour(fetching)))
                                .truncate()
                                .ends_in_an_ellipsis()
                                .child(told.unwrap_or_else(|| fetching.saying())),
                        ),
                    ),
                },
                |row, format| {
                    row.when(fitted.format, |row| row.child(format))
                        .when(fitted.heard, |row| row.child(listing::unheard()))
                },
            )
            .child(listing::length_cell(length).text_color(rgb(theme::faint())))
            .child(
                controls_place_of(controls)
                    .gap_1()
                    .when_some(dismissed, |controls, release_track| {
                        controls.child(
                            kit::icon_button(
                                ("dismiss-missing", release_track.get()),
                                Icon::Close,
                                DISMISS_MISSING_HINT,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.library.update(cx, |library, cx| {
                                        library.dismiss_missing(release_track, cx);
                                    });
                                },
                            )),
                        )
                    })
                    .child(mark),
            );

        row.id(id).when_some(download, |row, release_track| {
            row.cursor_pointer()
                .hover(|row| row.bg(rgb(theme::hover())))
                .names(WANT_HINT)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.want(release_track, cx));
                }))
        })
    }

    fn performed_by(
        &self,
        id: impl Into<ElementId>,
        label: impl IntoElement,
        performer: Option<Performer>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match performer {
            Some(Performer::Held(artist)) => self.opens(
                id,
                label,
                OPEN_ARTIST_HINT,
                Some(Selection::Artist(artist)),
                cx,
            ),
            Some(Performer::Elsewhere(found)) => self.pressed_to(
                id,
                label,
                OPEN_ARTIST_FOUND_HINT,
                Some(move |this: &mut Self, cx: &mut Context<Self>| {
                    this.open_artist_found(found.clone(), cx);
                }),
                cx,
            ),
            None => self.opens(id, label, OPEN_ARTIST_HINT, None, cx),
        }
    }

    pub(crate) fn found_row(
        &self,
        index: usize,
        found: &Found,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let fetching = self.library.read(cx).fetching_found(found);
        let row = self
            .unheld_row(index, Unheld::found(found, fetching), cx)
            .debug_selector(move || format!("found-{index}"));
        let hint = match fetching {
            None => FETCH_FOUND_HINT,
            Some(fetching) if fetching.can_be_asked_again() => FETCH_FOUND_AGAIN_HINT,
            Some(_) => return row,
        };

        let fetched = found.clone();
        row.cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(hint)
            .on_click(cx.listener(move |this, _, _, cx| {
                let wanted = fetched.clone();
                this.library
                    .update(cx, |library, cx| library.want_found(wanted, cx));
            }))
    }

    fn songs_not_held_heading(rows: usize) -> Div {
        run_heading(SharedString::from(format!(
            "Not in your library · {} · press one to download it",
            format::counted(rows, "song", "songs")
        )))
    }

    fn want_mark(&self, asks: Asks, cx: &mut Context<Self>) -> Stateful<Div> {
        match asks {
            Asks::Row(release_track) => match self.library.read(cx).wanted(release_track) {
                Some(want) => {
                    kit::icon_button(("unwant", release_track.get()), Icon::Wanted, UNWANT_HINT)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.library
                                .update(cx, |library, cx| library.unwant(want, cx));
                        }))
                }
                None => kit::icon_button(("want", release_track.get()), Icon::Want, WANT_HINT)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.library
                            .update(cx, |library, cx| library.want(release_track, cx));
                    })),
            },
            Asks::Found(found) => {
                let fetching = self
                    .library
                    .read(cx)
                    .fetching_found(&found)
                    .filter(|fetching| !fetching.can_be_asked_again());
                if let Some(fetching) = fetching {
                    return kit::mark_when(
                        Press::Greyed,
                        listing::keyed_by("wanting-found", &found.recording),
                        Icon::Wanted,
                        fetching.saying(),
                        ROW_GROUP,
                    );
                }
                let offered = Found::clone(&found);
                let wanting = kit::icon_button(
                    listing::keyed_by("want-found", &found.recording),
                    Icon::Want,
                    WANT_FOUND_HINT,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let wanted = Found::clone(&found);
                    this.library
                        .update(cx, |library, cx| library.want_found(wanted, cx));
                }));
                menu::opens_a_menu(wanting, move |_, at, _| releases_to_want(at, &offered), cx)
            }
        }
    }

    fn artists_drawn_as(&self, drawn: ArtistsDrawn, cx: &mut Context<Self>) -> Div {
        let choice = |as_: ArtistsDrawn, label: &'static str, cx: &mut Context<Self>| {
            kit::segment(("artists-drawn", as_ as usize), label, drawn == as_)
                .names(saying(as_))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.draw_the_artists_as(as_, cx);
                }))
        };

        kit::segmented()
            .child(choice(ArtistsDrawn::List, "List", cx))
            .child(choice(ArtistsDrawn::Grid, "Grid", cx))
    }

    fn artist_grid(&mut self, artists: &Arc<[Artist]>, cx: &mut Context<Self>) -> Div {
        let artists = Arc::clone(artists);
        let held = artists.len();
        let columns = self.grid_columns();
        let rows = held.div_ceil(columns.max(1));
        let measured = self.grid_width.clone();
        let laid_out = self.grid_width.get() > px(0.0);
        if laid_out {
            self.land_where_it_was_left(rows);
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
                                        .h(px(theme::grid_row()))
                                        .children(cells)
                                })
                                .collect()
                        }),
                    )
                    .track_scroll(self.artist_rows.clone())
                    .h_full()
                    .w_full(),
                )
            })
            .child(Scrollbars::of(cx).vertical("artist-scrollbar", self.artist_rows.clone()))
    }

    fn artist_cell(
        &mut self,
        artist: &Artist,
        at: usize,
        chosen: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = artist.id;
        let side = theme::grid_cover();
        let reached = self.reaches(Shift::Listing(Listed::Artists), at);
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
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(OPEN_ARTIST_HINT)
            .child(
                div()
                    .relative()
                    .rounded(px(side / 2.0))
                    .child(pictured)
                    .when(reached, |frame| {
                        frame.child(reached_ring().rounded(px(side / 2.0 + REACHED_RING + 1.0)))
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
                        .bg(theme::tinted(theme::background(), 0xb0)),
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
                                format::counted(artist.album_count as usize, "album", "albums"),
                                format::counted(artist.track_count as usize, "track", "tracks")
                            )),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.opened(Selection::Artist(id), cx);
            }));

        menu::opens_a_menu(
            cell,
            move |_, at, _| artist_menu(at, id).favours(Favoured::Artist(id), favourite),
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

    fn heading(&self, cx: &mut Context<Self>) -> Div {
        if let Some(searched) = self.search_heading(cx) {
            return searched;
        }
        let heading = match self.library.read(cx).selection() {
            Selection::Everything => self.library_heading(cx),
            Selection::Album(id) => self.album_page_heading(id, cx),
            Selection::Artist(id) => self.artist_page_heading(id, cx),
        };

        self.under_the_heading(heading, cx)
    }

    fn under_the_heading(&self, heading: Div, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let reads = library.search().reads();
        let naming = self.naming.filter(|naming| naming.is_a_search());

        heading
            .when(!reads.is_empty(), |heading| {
                heading.child(listing::reads(&reads))
            })
            .when(self.ordering, |heading| {
                heading.child(self.tracks_in_order(cx))
            })
            .when_some(naming, |heading, naming| {
                heading.child(self.naming_row(naming, cx))
            })
    }

    fn library_heading(&self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let searching = !library.query().is_empty();
        let naming = self.naming.filter(|naming| naming.is_a_search());
        let adding_to = self.adding_songs_to;
        let target = adding_to.and_then(|id| library.saved_playlist(id));
        let title = target.map_or_else(
            || "All tracks".to_owned(),
            |playlist| format!("Add songs to {}", playlist.name),
        );
        let under = target.map_or_else(
            || summary(library.listed(), None),
            |playlist| format!("Choose tracks to add to {}", playlist.name),
        );

        let sung = self.sung_offer(Tone::Ghost, cx);
        let actions = kit::actions()
            .when_some(adding_to, |bar, _| {
                bar.child(
                    kit::button(
                        "finish-adding-songs",
                        Some(Icon::Back),
                        "Done",
                        FINISH_ADDING_HINT,
                        Tone::Ghost,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.finish_adding_songs(cx))),
                )
            })
            .when(adding_to.is_none(), |bar| {
                bar.when_some(sung, |bar, sung| bar.child(sung))
                    .when(searching && naming.is_none(), |bar| {
                        bar.child(
                            kit::button(
                                "save-search",
                                Some(Icon::Search),
                                "Save this search",
                                SAVE_SEARCH_HINT,
                                Tone::Ghost,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.name_a_playlist(Naming::Query(None), window, cx);
                                },
                            )),
                        )
                    })
                    .child(self.orders_a_listing("order-tracks", cx))
                    .child(self.shuffle_all(cx))
                    .child(self.play_all(cx))
            });

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(theme::heading_name()))
                        .gap_1()
                        .child(kit::eyebrow(if adding_to.is_some() {
                            "ADD SONGS"
                        } else {
                            "LIBRARY"
                        }))
                        .child(kit::title(title))
                        .child(kit::subtitle(under)),
                )
                .child(actions),
        )
    }

    fn album_page_heading(&self, id: AlbumId, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let album = library.album_of(id);
        let title = SharedString::from(
            album.map_or_else(|| format!("album {id}"), |album| album.title.clone()),
        );
        let owner = album.and_then(|album| {
            album
                .artist_id
                .zip(album.artist.clone().map(SharedString::from))
        });
        let under = summary(library.listed(), album.and_then(|album| album.year));
        let record = library.release().and_then(record_of);
        let findable = library.can_enrich() && !library.release().is_some_and(is_matched);
        let record = record.map(drop).or(findable.then_some(()));
        let favourite = library.favoured_album(id);
        let has_missing_tracks = album.is_some_and(|album| album.missing > 0);

        let cover = self
            .cover_sized(Pictured::Album(id), Drawn::OnThePage, self.hero_side(), cx)
            .id("magnify-scoped-cover")
            .cursor_pointer()
            .hover(|cover| cover.opacity(kit::LIT))
            .names(COVER_HINT)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.magnify(Magnified::Album(id), cx);
            }));
        let cover = menu::opens_a_menu(
            cover,
            move |this, at, cx| {
                let owner = this
                    .library
                    .read(cx)
                    .album_of(id)
                    .and_then(|album| album.artist_id);

                album_menu(at, id, owner).apart().does(
                    Icon::Albums,
                    menu::MAGNIFY,
                    move |this, _, cx| this.magnify(Magnified::Album(id), cx),
                )
            },
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
            .child(kit::eyebrow("ALBUM"))
            .child(kit::hero_title(title, self.hero_width.get()))
            .when_some(owner, |column, (artist, name)| {
                column.child(
                    div()
                        .flex()
                        .min_w(px(0.0))
                        .text_size(px(theme::text_base()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::text()))
                        .child(
                            self.opens(
                                "scoped-owner",
                                name,
                                OPEN_ARTIST_HINT,
                                Some(Selection::Artist(artist)),
                                cx,
                            )
                            .keeps_its_width(),
                        ),
                )
            })
            .child(kit::subtitle(under))
            .child(
                self.page_actions(Favoured::Album(id), favourite, true, true, cx)
                    .when(has_missing_tracks, |row| {
                        row.child(
                            kit::button(
                                "download-album-missing",
                                Some(Icon::Download),
                                "Get the rest",
                                GET_ALBUM_REST_HINT,
                                Tone::Ghost,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.library.update(cx, |library, cx| {
                                        library.want_missing_tracks(id, cx)
                                    });
                                },
                            )),
                        )
                    })
                    .when_some(record, |row, _| {
                        row.child(
                            kit::icon_button("album-record", Icon::Info, RECORD_HINT).on_click(
                                cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    this.show_the_record(id, event.position(), cx);
                                }),
                            ),
                        )
                    }),
            );

        kit::heading()
            .child(self.way_back(cx))
            .child(kit::hero().child(cover).child(about))
    }

    fn artist_page_heading(&self, id: ArtistId, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let name = artist_named(library, id);
        let under = held_by_the_artist(library.artist_totals(), self.drawn_at());
        let detail = library.artist_detail();
        let profile = detail.and_then(profile_line);
        let genres = detail
            .map(|detail| genre_names(&detail.genres))
            .unwrap_or_default();
        let services = detail
            .map(|detail| service_names(&detail.links))
            .unwrap_or_default();
        let missing_shown = cx.global::<ResonateApp>().tabs.missing;
        let unheld = detail
            .map(|detail| {
                (
                    detail.releases_unheld as usize,
                    detail.releases_unread as usize,
                )
            })
            .filter(|(unheld, _)| missing_shown && *unheld > 0);
        let reads_further = library.can_enrich();
        let favourite = library.favoured_artist(id);
        let records = library.artist_albums().len();
        let any_records = records + library.albums_not_held().len();
        let tracks = library.listed().rows as usize;
        let shows = self.artist_shows.within(any_records);

        let side = self.hero_side();
        let portrait = match self.library.update(cx, |library, cx| {
            library.portrait(id, Portrayed::OnThePage, cx)
        }) {
            Some(art) => portrait_frame(art, side).into_any_element(),
            None => kit::avatar(&name, true)
                .size(px(side))
                .text_size(px(side * theme::text_title() * 1.6 / theme::scope_cover()))
                .into_any_element(),
        };

        let about = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .child(kit::measures_its_width(self.hero_width.clone()))
            .child(kit::measures_its_height(self.hero_height.clone()))
            .child(kit::eyebrow("ARTIST"))
            .child(kit::hero_title(
                SharedString::from(name),
                self.hero_width.get(),
            ))
            .child(kit::subtitle(under))
            .when_some(profile, |column, profile| {
                column.child(kit::subtitle(profile))
            })
            .when_some(
                heard_on("artist-heard-on", services, self.hero_width.get()),
                Div::child,
            )
            .child(
                self.page_actions(
                    Favoured::Artist(id),
                    favourite,
                    shows == ArtistShows::Tracks,
                    true,
                    cx,
                )
                .when(!genres.is_empty(), |row| {
                    row.child(
                        kit::icon_button("artist-genres", Icon::Info, GENRES_HINT).on_click(
                            cx.listener(move |this, event: &ClickEvent, _, cx| {
                                this.show_the_artist(id, event.position(), cx);
                            }),
                        ),
                    )
                })
                .when_some(unheld, |row, (unheld, unread)| {
                    row.child(
                        kit::button(
                            "unheld-releases",
                            Some(Icon::Missing),
                            match unread {
                                0 => format!(
                                    "{} not held",
                                    format::counted(unheld, "release", "releases")
                                ),
                                unread => format!(
                                    "{} not held, {unread} more unread",
                                    format::counted(unheld, "release", "releases")
                                ),
                            },
                            UNHELD_HINT,
                            Tone::Ghost,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_what_is_missing(MissingShows::Releases, cx);
                            this.set_pane(Pane::Missing, cx);
                        })),
                    )
                    .when(unread > 0 && reads_further, |row| {
                        row.child(
                            kit::button(
                                "read-the-rest",
                                Some(Icon::Import),
                                READ_THE_REST,
                                READ_THE_REST_HINT,
                                Tone::Ghost,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.library
                                        .update(cx, |library, cx| library.read_the_rest_of(id, cx));
                                },
                            )),
                        )
                    })
                })
                .when(any_records > 0, |row| {
                    row.child(div().flex_1())
                        .child(self.artist_tabs(shows, records, tracks, cx))
                }),
            );

        kit::heading()
            .child(self.way_back(cx))
            .child(kit::hero().child(portrait).child(about))
    }

    fn artist_tabs(
        &self,
        shows: ArtistShows,
        records: usize,
        tracks: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let tab =
            |shown: ArtistShows, label: &'static str, count: usize, cx: &mut Context<Self>| {
                kit::segment(("artist-shows", shown as usize), label, shows == shown)
                    .gap_2()
                    .child(kit::figure(count.to_string()).text_color(rgb(theme::faint())))
                    .on_click(cx.listener(move |this, _, _, cx| this.show_of_the_artist(shown, cx)))
            };

        kit::segmented()
            .child(tab(ArtistShows::Records, "Albums", records, cx))
            .child(tab(ArtistShows::Tracks, "Tracks", tracks, cx))
    }

    fn show_of_the_artist(&mut self, shows: ArtistShows, cx: &mut Context<Self>) {
        self.artist_shows = shows;
        if shows == ArtistShows::Records {
            self.ordering = false;
        }
        cx.notify();
    }

    fn way_back(&self, cx: &mut Context<Self>) -> Div {
        let label = self
            .way_back_to()
            .unwrap_or_else(|| SharedString::new_static(self.in_front(cx).label()));

        div().flex().child(
            kit::way_back("way-back", label, WAY_BACK_HINT)
                .on_click(cx.listener(|this, _, _, cx| this.step_back(cx))),
        )
    }

    fn page_actions(
        &self,
        what: Favoured,
        already: bool,
        sortable: bool,
        shuffleable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        kit::action_row()
            .flex_nowrap()
            .w_full()
            .pt_2()
            .child(self.play_all(cx))
            .when(shuffleable, |row| row.child(self.shuffle_all(cx)))
            .child(self.favour_mark("scope-favourite", what, already, cx))
            .when(sortable, |row| {
                row.child(self.orders_a_listing("order-tracks", cx))
            })
    }

    fn show_the_record(&mut self, album: AlbumId, at: Point<Pixels>, cx: &mut Context<Self>) {
        if matches!(self.record, Some(OpenedRecord::Album { album: open, .. }) if open == album) {
            self.record = None;
        } else {
            self.close_the_menu(cx);
            self.record = Some(OpenedRecord::Album {
                album,
                at,
                forgetting: false,
            });
        }
        cx.notify();
    }

    fn show_the_artist(&mut self, artist: ArtistId, at: Point<Pixels>, cx: &mut Context<Self>) {
        if matches!(self.record, Some(OpenedRecord::Artist { artist: open, .. }) if open == artist)
        {
            self.record = None;
        } else {
            self.close_the_menu(cx);
            self.record = Some(OpenedRecord::Artist { artist, at });
        }
        cx.notify();
    }

    pub(crate) fn record_over_the_app(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (at, card) = {
            let opened = *self.record.as_ref()?;
            let library = self.library.read(cx);
            match opened {
                OpenedRecord::Album {
                    album,
                    at,
                    forgetting,
                } => {
                    if library.selection() != Selection::Album(album) {
                        return None;
                    }
                    let release = library.release();
                    let findable = library.can_enrich() && !release.is_some_and(is_matched);
                    let record = release
                        .and_then(record_of)
                        .or_else(|| findable.then(Record::bare))?;
                    let group = release
                        .and_then(|release| release.group.clone())
                        .filter(|_| library.can_enrich());
                    let current = release.and_then(|release| release.mbid.clone());
                    let pressings = library.pressings_of(album).cloned();
                    let card = record_card(&record);
                    let card = match (group, findable) {
                        (Some(group), _) => card.child(self.other_pressings(
                            album,
                            Asking::Group(group),
                            current,
                            pressings,
                            cx,
                        )),
                        (None, true) => card.child(self.other_pressings(
                            album,
                            Asking::Search,
                            current,
                            pressings,
                            cx,
                        )),
                        (None, false) => card,
                    };
                    let card = match record.matched {
                        true => card.child(self.not_this_record(album, at, forgetting, cx)),
                        false => card,
                    };
                    (at, card)
                }
                OpenedRecord::Artist { artist, at } => {
                    if library.selection() != Selection::Artist(artist) {
                        return None;
                    }
                    let genres = library
                        .artist_detail()
                        .map(|detail| genre_names(&detail.genres))
                        .unwrap_or_default();
                    if genres.is_empty() {
                        return None;
                    }
                    (at, genre_card(&genres))
                }
            }
        };

        Some(self.detail_over(at, card, cx))
    }

    fn other_pressings(
        &self,
        album: AlbumId,
        asking: Asking,
        current: Option<Mbid>,
        pressings: Option<Pressings>,
        cx: &mut Context<Self>,
    ) -> Div {
        let (label, hint, waiting, none) = match asking {
            Asking::Group(_) => (
                OTHER_PRESSINGS,
                OTHER_PRESSINGS_HINT,
                ASKING_FOR_PRESSINGS,
                NO_PRESSINGS,
            ),
            Asking::Search => (
                FIND_THE_RECORD,
                FIND_THE_RECORD_HINT,
                ASKING_FOR_RELEASES,
                NO_RELEASES,
            ),
        };
        let listed = match pressings {
            None => {
                return div().child(
                    kit::button(
                        "other-pressings",
                        Some(Icon::Disc),
                        label,
                        hint,
                        Tone::Ghost,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let asking = asking.clone();
                        this.library.update(cx, |library, cx| match asking {
                            Asking::Group(group) => {
                                library.ask_for_pressings(album, group, cx);
                            }
                            Asking::Search => library.ask_for_releases(album, cx),
                        });
                    })),
                );
            }
            Some(Pressings::Asking) => return pressing_note(waiting),
            Some(Pressings::Unanswered) => return pressing_note(none),
            Some(Pressings::Listed(listed)) => listed,
        };

        let mut column = div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(kit::eyebrow(format!("PRESSINGS · {}", listed.len())));
        for (at, pressing) in listed.iter().take(PRESSINGS_SHOWN).enumerate() {
            let in_use = current.as_ref() == Some(&pressing.id);
            let chosen = pressing.id.clone();
            column = column.child(
                div()
                    .id(("pressing", at))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .when(!in_use, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(rgb(theme::hover())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.record = None;
                                let taken = chosen.clone();
                                this.library.update(cx, |library, cx| {
                                    library.take_pressing(album, taken, cx);
                                });
                                cx.notify();
                            }))
                    })
                    .names(if in_use {
                        PRESSING_IN_USE_HINT
                    } else {
                        TAKE_THE_PRESSING_HINT
                    })
                    .child(
                        kit::figure(pressing_line(pressing))
                            .truncate()
                            .ends_in_an_ellipsis(),
                    )
                    .when(in_use, |row| {
                        row.child(kit::badge("IN USE", theme::accent()))
                    }),
            );
        }
        if listed.len() > PRESSINGS_SHOWN {
            column = column.child(pressing_note(&format!(
                "and {} more",
                listed.len() - PRESSINGS_SHOWN
            )));
        }
        column
    }

    fn not_this_record(
        &self,
        album: AlbumId,
        at: Point<Pixels>,
        forgetting: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (label, hint) = match forgetting {
            true => (FORGET_THE_MATCH_ARMED, FORGET_THE_MATCH_ARMED_HINT),
            false => (FORGET_THE_MATCH, FORGET_THE_MATCH_HINT),
        };

        kit::button(
            "not-this-record",
            Some(Icon::Discard),
            label,
            hint,
            Tone::Ghost,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if forgetting {
                this.record = None;
                this.library
                    .update(cx, |library, cx| library.forget_the_match(album, cx));
            } else {
                this.record = Some(OpenedRecord::Album {
                    album,
                    at,
                    forgetting: true,
                });
            }
            cx.notify();
        }))
    }

    fn detail_over(
        &self,
        at: Point<Pixels>,
        card: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.record = None;
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.record = None;
                    cx.notify();
                }),
            )
            .child(
                deferred(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.0))
                        .child(motion::lifted_in(card, "record-arrives")),
                )
                .with_priority(1),
            )
            .into_any_element()
    }

    pub(crate) fn play_all(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "play-all",
            Some(Icon::Play),
            "Play",
            PLAY_ALL_HINT,
            Tone::Primary,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.with_everything_listed(window, cx, |this, listing, _, cx| {
                this.plays_in_order(cx);
                this.play(&listing, 0, cx);
            });
        }))
    }

    pub(crate) fn shuffle_all(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "shuffle-all",
            Some(Icon::Shuffle),
            "Shuffle",
            SHUFFLE_ALL_HINT,
            Tone::Outlined,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.with_everything_listed(window, cx, |this, listing, _, cx| {
                this.play_shuffled(&listing, cx);
            });
        }))
    }

    fn artist_records(&self, artist: ArtistId, cx: &mut Context<Self>) -> Stateful<Div> {
        let albums = self.library.read(cx).artist_albums();
        let not_held = self.library.read(cx).albums_not_held();
        let mut cells = Vec::new();
        for album in albums.iter() {
            cells.push(
                self.album_cell_captioned(
                    album,
                    theme::grid_cover(),
                    Caption::Beside(artist),
                    false,
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
                self.album_not_held_cell(album, theme::grid_cover(), cx)
                    .into_any_element(),
            );
        }

        div()
            .id(("artist-records", artist.get() as usize))
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&self.artist_records_scroll)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(theme::grid_gap()))
                    .px_6()
                    .pt_5()
                    .pb_6()
                    .children(cells),
            )
    }

    fn album_not_held_cell(
        &self,
        album: &AlbumNotHeld,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.unheld_album_cell(
            UnheldAlbum {
                group: &album.release.mbid,
                pressing: album.pressing.as_ref(),
                title: &album.release.title,
                caption: described(&album.release.kind, album.release.first_released.as_deref()),
            },
            side,
            cx,
        )
    }

    pub(crate) fn album_found_cell(
        &self,
        album: &AlbumFound,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let by = SharedString::from(album.artist.clone());
        let described = described(&album.kind, album.first_released.as_deref());
        let caption = match described.is_empty() {
            true => by.to_string(),
            false => format!("{by} · {described}"),
        };

        let selector = format!("album-found-{}", album.group);

        self.unheld_album_cell(
            UnheldAlbum {
                group: &album.group,
                pressing: None,
                title: &album.title,
                caption,
            },
            side,
            cx,
        )
        .debug_selector(move || selector.clone())
    }

    fn unheld_album_cell(
        &self,
        album: UnheldAlbum<'_>,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let group = album.group.clone();
        let fetching = self.library.read(cx).fetching_album(&group);
        let opening = self.library.read(cx).is_opening_album(&group);
        let can_ask = self.library.read(cx).can_enrich();
        let art = self.library.update(cx, |library, cx| {
            library.released_cover_with_group(
                album.pressing.unwrap_or(&group),
                Some(&group),
                Drawn::InAGrid,
                cx,
            )
        });
        let cover = match art {
            Some(art) => framed_cover(Some(art), side).opacity(UNHELD_COVER),
            None => unheld_cover(side),
        };
        let under = match fetching {
            Some(fetching) => div()
                .text_color(rgb(fetching_colour(fetching)))
                .child(fetching.saying()),
            None if opening => div().text_color(rgb(theme::accent())).child(OPENING),
            None => div()
                .text_color(rgb(theme::faint()))
                .child(SharedString::from(album.caption)),
        };
        let pressable = can_ask;

        let cell = div()
            .id(listing::keyed_by("album-not-held", &group))
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .child(
                div()
                    .relative()
                    .rounded(px(8.0))
                    .when(pressable, |frame| {
                        frame.group_hover(CELL_GROUP, |frame| {
                            frame.shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0.0, 0.0, 0.0, 0.5),
                                offset: gpui::point(px(0.0), px(8.0)),
                                blur_radius: px(24.0),
                                spread_radius: px(0.0),
                            }])
                        })
                    })
                    .child(cover),
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
                            .text_color(rgb(theme::faint()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(album.title.to_owned())),
                    )
                    .child(
                        under
                            .text_size(px(theme::text_xs()))
                            .truncate()
                            .ends_in_an_ellipsis(),
                    ),
            );
        if !pressable {
            return cell;
        }

        cell.cursor_pointer()
            .names(OPEN_ALBUM_NOT_HELD_HINT)
            .on_click(cx.listener(move |this, _, _, cx| {
                let landing = group.clone();
                let landed = this
                    .library
                    .update(cx, |library, cx| library.land_album_not_held(landing, cx));
                cx.spawn(async move |this, cx| {
                    let Some(album) = landed.await else {
                        return;
                    };
                    let _ = this.update(cx, |this, cx| this.opened(Selection::Album(album), cx));
                })
                .detach();
            }))
    }

    pub(crate) fn album_shelf(
        &self,
        id: &'static str,
        named: String,
        albums: &[Album],
        cx: &mut Context<Self>,
    ) -> Div {
        let mut cells = Vec::new();
        for album in albums.iter().take(SHELF_AT_MOST) {
            cells.push(
                self.album_cell_at(album, theme::shelf_cover(), cx)
                    .into_any_element(),
            );
        }

        let scroll = self
            .shelf_scrolls
            .borrow_mut()
            .entry(id)
            .or_default()
            .clone();
        shelf(id, named, albums.len(), cells, scroll, Scrollbars::of(cx))
    }

    pub(crate) fn artist_shelf_of(
        &self,
        id: &'static str,
        named: String,
        artists: &[Artist],
        cx: &mut Context<Self>,
    ) -> Div {
        let mut cells = Vec::new();
        for artist in artists.iter().take(SHELF_AT_MOST) {
            cells.push(
                self.artist_cell_at(artist, theme::shelf_cover(), cx)
                    .into_any_element(),
            );
        }

        let scroll = self
            .shelf_scrolls
            .borrow_mut()
            .entry(id)
            .or_default()
            .clone();
        shelf(id, named, artists.len(), cells, scroll, Scrollbars::of(cx))
    }

    pub(crate) fn artist_cell_at(
        &self,
        artist: &Artist,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = artist.id;
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
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(OPEN_ARTIST_HINT)
            .child(picture)
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.opened(Selection::Artist(id), cx);
            }));

        menu::opens_a_menu(
            cell,
            move |_, at, _| artist_menu(at, id).favours(Favoured::Artist(id), favourite),
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
            .follows_the_pointer(cell_id)
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap_2p5()
            .w(px(side * ARTIST_FOUND_CELL_WIDENING))
            .cursor_pointer()
            .names(OPEN_ARTIST_FOUND_HINT)
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
                        .child(OPENING),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_artist_found(opening.clone(), cx);
            }))
    }

    pub(crate) fn orders_a_listing(
        &self,
        named: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        kit::icon_button(named, Icon::Sort, ORDER_HINT)
            .when(self.ordering, |button| button.bg(rgb(theme::hover())))
            .on_click(cx.listener(|this, _, _, cx| this.order_a_listing(cx)))
    }

    pub(crate) fn tracks_in_order(&self, cx: &mut Context<Self>) -> Div {
        let sorting = self.library.read(cx).sorting();

        sorting::order_row(
            "track-order",
            "track-reading",
            sorting.tracks,
            sorting.tracks_read,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_tracks(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_tracks(reading, cx));
            },
            cx,
        )
    }

    pub(crate) fn albums_in_order(&self, cx: &mut Context<Self>) -> Div {
        let sorting = self.library.read(cx).sorting();

        sorting::order_row(
            "album-order",
            "album-reading",
            sorting.albums,
            sorting.albums_read,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_albums(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_albums(reading, cx));
            },
            cx,
        )
    }

    pub(crate) fn artists_in_order(&self, cx: &mut Context<Self>) -> Div {
        let sorting = self.library.read(cx).sorting();

        sorting::order_row(
            "artist-order",
            "artist-reading",
            sorting.artists,
            sorting.artists_read,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_artists(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_artists(reading, cx));
            },
            cx,
        )
    }
}

const fn saying(drawn: ArtistsDrawn) -> &'static str {
    match drawn {
        ArtistsDrawn::List => "Draw the artists as a list, a small portrait beside each name",
        ArtistsDrawn::Grid => "Draw the artists as a grid of large portraits, the way albums are",
    }
}

const AVATAR_LETTER: f32 = 0.36;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArtistShows {
    #[default]
    Records,
    Tracks,
}

impl ArtistShows {
    const fn within(self, records: usize) -> Self {
        match (self, records) {
            (Self::Records, 0) => Self::Tracks,
            (shows, _) => shows,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caption {
    ByArtist,
    Beside(ArtistId),
}

fn album_menu(at: Point<Pixels>, album: AlbumId, owner: Option<ArtistId>) -> Menu {
    Menu::at(at)
        .does(Icon::Play, menu::PLAY, move |this, window, cx| {
            this.plays_everything_in(Selection::Album(album), Placement::Queued, true, window, cx);
        })
        .does(Icon::QueueNext, menu::PLAY_NEXT, move |this, window, cx| {
            this.plays_everything_in(Selection::Album(album), Placement::Next, false, window, cx);
        })
        .does(
            Icon::QueueLast,
            menu::ADD_TO_QUEUE,
            move |this, window, cx| {
                this.plays_everything_in(
                    Selection::Album(album),
                    Placement::Queued,
                    false,
                    window,
                    cx,
                );
            },
        )
        .apart()
        .does(Icon::Albums, menu::OPEN_ALBUM, move |this, _, cx| {
            this.opened(Selection::Album(album), cx);
        })
        .when_some(owner, |menu, owner| {
            menu.does(Icon::Artists, menu::GO_TO_ARTIST, move |this, _, cx| {
                this.opened(Selection::Artist(owner), cx);
            })
        })
}

fn artist_menu(at: Point<Pixels>, artist: ArtistId) -> Menu {
    Menu::at(at)
        .does(Icon::Play, menu::PLAY, move |this, window, cx| {
            this.plays_everything_in(
                Selection::Artist(artist),
                Placement::Queued,
                true,
                window,
                cx,
            );
        })
        .does(Icon::QueueNext, menu::PLAY_NEXT, move |this, window, cx| {
            this.plays_everything_in(
                Selection::Artist(artist),
                Placement::Next,
                false,
                window,
                cx,
            );
        })
        .does(
            Icon::QueueLast,
            menu::ADD_TO_QUEUE,
            move |this, window, cx| {
                this.plays_everything_in(
                    Selection::Artist(artist),
                    Placement::Queued,
                    false,
                    window,
                    cx,
                );
            },
        )
        .apart()
        .does(Icon::Artists, menu::OPEN_ARTIST, move |this, _, cx| {
            this.opened(Selection::Artist(artist), cx);
        })
}

pub(crate) enum Asks {
    Row(ReleaseTrackId),
    Found(Box<Found>),
}

pub(crate) enum Beside {
    AnAlbum,
    ARun,
    ASearch {
        pictured: Sleeve,
        on: SharedString,
        fetching: Option<Fetching>,
    },
}

pub(crate) enum Sleeve {
    Released(Mbid),
    Unknown,
}

impl Beside {
    const fn pictured(&self) -> Option<&Sleeve> {
        match self {
            Self::AnAlbum | Self::ARun => None,
            Self::ASearch { pictured, .. } => Some(pictured),
        }
    }
}

pub(crate) struct Unheld {
    asks: Asks,
    number: SharedString,
    title: SharedString,
    artist: SharedString,
    length: SharedString,
    beside: Beside,
}

impl Unheld {
    fn of(
        release_track: ReleaseTrackId,
        number: Option<&str>,
        position: u32,
        title: &str,
        artist: Option<&str>,
        length: Option<Duration>,
    ) -> Self {
        Self {
            asks: Asks::Row(release_track),
            number: SharedString::from(
                number.map_or_else(|| position.to_string(), ToOwned::to_owned),
            ),
            title: SharedString::from(title.to_owned()),
            artist: SharedString::from(artist.unwrap_or_default().to_owned()),
            length: SharedString::from(length.map(format::spanned).unwrap_or_default()),
            beside: Beside::AnAlbum,
        }
    }

    pub(crate) fn short_of(row: &MissingTrack) -> Self {
        Self {
            beside: Beside::ARun,
            ..Self::from(row)
        }
    }

    fn found(found: &Found, fetching: Option<Fetching>) -> Self {
        Self {
            number: SharedString::new_static(""),
            title: SharedString::from(found.title.clone()),
            artist: SharedString::from(found.artist.clone()),
            length: SharedString::from(found.length.map(format::spanned).unwrap_or_default()),
            beside: Beside::ASearch {
                pictured: found.release.as_ref().map_or(Sleeve::Unknown, |release| {
                    Sleeve::Released(release.id.clone())
                }),
                on: SharedString::from(
                    found
                        .release
                        .as_ref()
                        .map(|release| release.title.clone())
                        .unwrap_or_default(),
                ),
                fetching,
            },
            asks: Asks::Found(Box::new(found.clone())),
        }
    }
}

fn pressing_line(pressing: &GroupRelease) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(
        pressing
            .date
            .clone()
            .unwrap_or_else(|| "undated".to_owned()),
    );
    if let Some(country) = &pressing.country {
        parts.push(country.clone());
    }
    if let Some(tracks) = pressing.track_count {
        parts.push(format!("{tracks} tracks"));
    }
    parts.join(" · ")
}

fn pressing_note(said: &str) -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::faint()))
        .child(SharedString::from(said.to_owned()))
}

pub(crate) fn fetching_colour(fetching: Fetching) -> u32 {
    match fetching {
        Fetching::Downloading { .. } => theme::accent(),
        Fetching::Downloaded => theme::done(),
        Fetching::GaveUp | Fetching::NoProvider | Fetching::Unwanted => theme::failure(),
        Fetching::Landing
        | Fetching::Queued
        | Fetching::Unreached { .. }
        | Fetching::Retrying { .. } => theme::muted(),
    }
}

fn not_held_heading(albums: usize, under_the_held: bool) -> Div {
    div()
        .w_full()
        .when(under_the_held, |heading| heading.pt_4())
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::muted()))
        .child(SharedString::from(format!(
            "{NOT_HELD_HEADING} · {}",
            format::counted(albums, "release", "releases")
        )))
}

struct UnheldAlbum<'a> {
    group: &'a Mbid,
    pressing: Option<&'a Mbid>,
    title: &'a str,
    caption: String,
}

fn described(kind: &Option<String>, first_released: Option<&str>) -> String {
    [
        kind.clone(),
        first_released
            .and_then(|date| date.get(..4))
            .map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

fn unheld_cover(side: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(side / 4.0))
        .border_1()
        .border_dashed()
        .border_color(rgb(theme::border()))
        .child(icons::icon(Icon::Missing, side * 0.5, theme::faint()))
}

impl From<&HeldReleaseTrack> for Unheld {
    fn from(row: &HeldReleaseTrack) -> Self {
        Self::of(
            row.id,
            row.number.as_deref(),
            row.position,
            &row.title,
            row.artist.as_deref(),
            row.length,
        )
    }
}

impl From<&MissingTrack> for Unheld {
    fn from(row: &MissingTrack) -> Self {
        Self::of(
            row.release_track,
            row.number.as_deref(),
            row.position,
            &row.title,
            row.artist.as_deref(),
            row.length,
        )
    }
}

pub(crate) fn controls_place() -> Div {
    controls_place_of(ROW_CONTROLS)
}

fn controls_place_of(controls: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_end()
        .w(px(controls_width(controls)))
}

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

fn disc_heading(disc: u32, media: &[HeldMedium]) -> Div {
    let named = media
        .iter()
        .find(|medium| medium.position == disc)
        .and_then(|medium| medium.title.clone().or_else(|| medium.format.clone()));
    let spelt = match named {
        Some(named) => format!("Disc {disc} · {named}"),
        None => format!("Disc {disc}"),
    };

    run_heading(SharedString::from(spelt))
}

pub(crate) fn run_heading(named: impl IntoElement) -> Div {
    row(false).child(
        div()
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .border_b_1()
            .border_color(rgb(theme::border()))
            .text_size(px(theme::text_xs()))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::muted()))
            .child(named),
    )
}

fn pressings(media: &[HeldMedium]) -> Option<String> {
    let format = media.first()?.format.as_deref()?;
    if media
        .iter()
        .any(|medium| medium.format.as_deref() != Some(format))
    {
        return Some(format!("{} discs", media.len()));
    }

    Some(match media.len() {
        1 => format.to_owned(),
        held => format!("{held} × {format}"),
    })
}

fn releases_to_want(at: Point<Pixels>, found: &Found) -> Menu {
    let mut menu = Menu::at(at);
    for release in found
        .in_the_order_worth_offering()
        .into_iter()
        .take(RELEASES_OFFERED)
    {
        let wanted = found.from(release);
        menu = menu.does(Icon::Disc, release_line(release), move |this, _, cx| {
            let wanted = wanted.clone();
            this.library
                .update(cx, |library, cx| library.want_found(wanted, cx));
        });
    }
    menu
}

fn releases_to_place_on(at: Point<Pixels>, track: TrackId, recording: Recording) -> Menu {
    let recording = Arc::new(recording);
    let mut menu = Menu::at(at);
    for release in in_the_order_worth_offering(&recording.releases)
        .into_iter()
        .take(RELEASES_OFFERED)
    {
        let placed = release.id.clone();
        let placing = Arc::clone(&recording);
        menu = menu.does(Icon::Disc, release_line(release), move |this, _, cx| {
            let placing = Arc::clone(&placing);
            let placed = placed.clone();
            this.library.update(cx, |library, cx| {
                library.place_on(track, placing, placed, cx)
            });
        });
    }
    menu
}

impl RootView {
    fn offer_releases_to_place_on(
        &mut self,
        track: TrackId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(asking) = self.library.update(cx, |library, cx| {
            library.releases_it_could_sit_on(track, cx)
        }) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let asked = asking.await;
            let _ = this.update(cx, |this, cx| match asked {
                Ok(Some(recording)) if !recording.releases.is_empty() => {
                    this.open_a_menu(releases_to_place_on(at, track, recording), cx);
                }
                Ok(_) => this.report(Notice::Trouble(NOTHING_PLACES_IT.to_owned()), cx),
                Err(error) => {
                    tracing::warn!(%error, "the releases a track is on could not be read");
                    this.report(toast::could_not("find the releases it is on", &error), cx);
                }
            });
        })
        .detach();
    }
}

fn release_line(release: &RecordingRelease) -> String {
    let mut line = release.title.clone();
    if let Some(year) = release.date.as_deref().and_then(|date| date.get(..4)) {
        line.push_str(" · ");
        line.push_str(year);
    }
    if let Some(kind) = release.issued.kind.as_deref() {
        line.push_str(" · ");
        line.push_str(kind);
    }
    line
}

fn record_of(release: &ReleaseDetail) -> Option<Record> {
    let mut facts = Vec::new();
    if let Some(date) = release.date.clone() {
        facts.push(Fact {
            label: "Released",
            value: date,
        });
    }
    if let Some(format) = pressings(&release.media) {
        facts.push(Fact {
            label: "Format",
            value: format,
        });
    }
    if let Some(label) = release.label.clone() {
        facts.push(Fact {
            label: "Label",
            value: label,
        });
    }
    if let Some(number) = release.catalog_number.clone() {
        facts.push(Fact {
            label: "Catalogue number",
            value: number,
        });
    }
    if let Some(barcode) = release.barcode.clone() {
        facts.push(Fact {
            label: "Barcode",
            value: barcode,
        });
    }
    if let Some(country) = release.country.clone() {
        facts.push(Fact {
            label: "Country",
            value: country,
        });
    }
    if let Some(kind) = release.kind.clone() {
        facts.push(Fact {
            label: "Kind",
            value: kind,
        });
    }
    let note = release
        .disambiguation
        .clone()
        .filter(|note| !note.trim().is_empty());
    let on = service_names(&release.links);
    let record = Record {
        facts,
        note,
        on,
        matched: is_matched(release),
    };

    (!record.is_empty()).then_some(record)
}

#[derive(Clone)]
struct HeardOn {
    name: &'static str,
    url: SharedString,
}

struct Fact {
    label: &'static str,
    value: String,
}

struct Record {
    facts: Vec<Fact>,
    note: Option<String>,
    on: Vec<HeardOn>,
    matched: bool,
}

#[derive(Clone)]
enum Asking {
    Group(Mbid),
    Search,
}

fn is_matched(release: &ReleaseDetail) -> bool {
    release.mbid.is_some() || release.group.is_some()
}

impl Record {
    const fn bare() -> Self {
        Self {
            facts: Vec::new(),
            note: None,
            on: Vec::new(),
            matched: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.facts.is_empty() && self.note.is_none() && self.on.is_empty()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OpenedRecord {
    Album {
        album: AlbumId,
        at: Point<Pixels>,
        forgetting: bool,
    },
    Artist {
        artist: ArtistId,
        at: Point<Pixels>,
    },
}

fn detail_card(title: &'static str) -> Stateful<Div> {
    div()
        .id("record")
        .flex()
        .flex_col()
        .w(px(theme::record_width()))
        .p_3()
        .gap_2()
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::outline()))
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.5),
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
        }])
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::text()))
                .child(title),
        )
}

fn record_card(record: &Record) -> Stateful<Div> {
    let mut card = detail_card("About this record");
    for fact in &record.facts {
        card = card.child(fact_row(fact));
    }
    if let Some(note) = &record.note {
        card = card.child(
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child(note.clone()),
        );
    }
    if !record.on.is_empty() {
        card = card.child(record_services(&record.on));
    }
    card
}

fn genre_card(genres: &[String]) -> Stateful<Div> {
    let mut tags = div().flex().flex_wrap().gap_1p5();
    for genre in genres {
        tags = tags.child(kit::tag(genre.clone()));
    }
    detail_card("Genres").child(tags)
}

fn service_names(links: &[Link]) -> Vec<HeardOn> {
    let mut named: Vec<HeardOn> = Vec::new();
    for link in links {
        if link.service == Service::Other {
            continue;
        }
        let name = link.service.title();
        if !named.iter().any(|held| held.name == name) {
            named.push(HeardOn {
                name,
                url: SharedString::from(link.url.clone()),
            });
        }
    }
    named
}

const SERVICES_SHOWN: usize = 4;

fn heard_on(id: &'static str, services: Vec<HeardOn>, room: Pixels) -> Option<Div> {
    if services.is_empty() {
        return None;
    }
    let mut line = kit::wraps_within(room)
        .items_center()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::faint()))
        .child("On\u{a0}");
    for (at, heard) in services.into_iter().take(SERVICES_SHOWN).enumerate() {
        if at > 0 {
            line = line.child(div().px_1().child("·"));
        }
        line = line.child(service_link(
            ElementId::NamedInteger(id.into(), at as u64),
            heard,
        ));
    }
    Some(line)
}

fn fact_row(fact: &Fact) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap_3()
        .child(
            div()
                .w(px(RECORD_LABEL))
                .flex_none()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child(fact.label),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::text()))
                .child(fact.value.clone()),
        )
}

fn record_services(services: &[HeardOn]) -> Div {
    let mut links = div()
        .flex()
        .flex_1()
        .min_w(px(0.0))
        .flex_wrap()
        .items_center()
        .gap_x_2()
        .gap_y_1();
    for (at, heard) in services.iter().cloned().enumerate() {
        links = links.child(service_link(
            ElementId::NamedInteger("record-service".into(), at as u64),
            heard,
        ));
    }

    div()
        .flex()
        .items_baseline()
        .gap_3()
        .child(
            div()
                .w(px(RECORD_LABEL))
                .flex_none()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child("On"),
        )
        .child(links)
}

fn service_link(id: ElementId, heard: HeardOn) -> Stateful<Div> {
    let url = heard.url;

    div()
        .id(id.clone())
        .cursor_pointer()
        .lit_under_the_pointer(id, |link| {
            link.text_color(rgb(theme::accent()))
                .text_decoration_1()
                .text_decoration_color(rgb(theme::accent()))
        })
        .names(format!("Open on {}", heard.name))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            cx.open_url(&url);
        })
        .child(heard.name)
}

fn profile_line(detail: &ArtistDetail) -> Option<String> {
    let mut parts: format::Parts<String> = format::Parts::new();
    if let Some(kind) = detail.kind.as_deref() {
        parts.push(kind.to_owned());
    }
    if let Some(place) = detail.area.as_deref().or(detail.country.as_deref()) {
        parts.push(place.to_owned());
    }
    if let Some(years) = years_of(detail.span.begin.as_deref(), detail.span.end.as_deref()) {
        parts.push(years);
    }

    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn genre_names(genres: &[Genre]) -> Vec<String> {
    let mut weighed: Vec<&Genre> = genres.iter().collect();
    weighed.sort_by_key(|genre| Reverse(genre.weight));
    weighed
        .into_iter()
        .map(|genre| genre.name.clone())
        .collect()
}

fn years_of(begin: Option<&str>, end: Option<&str>) -> Option<String> {
    match (begin.map(year_of), end.map(year_of)) {
        (Some(begin), Some(end)) => Some(format!("{begin}–{end}")),
        (Some(begin), None) => Some(format!("{begin}–")),
        (None, Some(end)) => Some(format!("–{end}")),
        (None, None) => None,
    }
}

pub(crate) fn year_of(date: &str) -> &str {
    date.get(..4).unwrap_or(date)
}

fn artist_named(library: &LibraryModel, id: ArtistId) -> String {
    library
        .artist_detail()
        .map(|detail| detail.name.clone())
        .or_else(|| {
            library
                .artists()
                .iter()
                .find(|artist| artist.id == id)
                .map(|artist| artist.name.clone())
        })
        .unwrap_or_else(|| format!("artist {id}"))
}

fn held_by_the_artist(totals: ArtistTotals, now: SystemTime) -> String {
    let mut parts: format::Parts<String> = smallvec![
        format::counted(totals.albums as usize, "album", "albums"),
        format::counted(totals.tracks as usize, "track", "tracks"),
    ];
    if let Some(length) = totals.length {
        parts.push(format::spanned(length));
    }
    if totals.plays > 0 {
        parts.push(format::counted(totals.plays as usize, "play", "plays"));
    }
    if let Some(played) = totals.played {
        parts.push(format::since(played, now));
    }

    parts.join(" · ")
}

fn summary(listed: Measured, year: Option<i32>) -> String {
    let rows = listed.rows as usize;
    let lossless = listed.lossless as usize;
    let mut parts: format::Parts<String> = smallvec![format::counted(rows, "track", "tracks")];

    if let Some(year) = year {
        parts.insert(0, year.to_string());
    }
    if let Some(total) = listed.length {
        parts.push(format::spanned(total));
    }
    if lossless > 0 && lossless == rows {
        parts.push("all lossless".to_owned());
    } else if lossless > 0 {
        parts.push(format!("{lossless} lossless"));
    }

    parts.join(" · ")
}

fn queueing(tracks: &Arc<[Track]>, index: usize) -> impl Fn() -> Arc<[PlaylistEntry]> + 'static {
    let tracks = Arc::clone(tracks);
    move || {
        tracks
            .get(index)
            .map_or_else(|| Arc::from([]), |track| listed(slice::from_ref(track)))
    }
}

pub(crate) fn row_controls() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_0p5()
        .opacity(0.0)
        .group_hover(ROW_GROUP, |controls| controls.opacity(1.0))
}

pub(crate) fn trailing_controls() -> Div {
    div().flex().flex_none().items_center().gap_0p5()
}

fn shelf(
    id: &'static str,
    named: String,
    held: usize,
    cells: Vec<AnyElement>,
    scroll: gpui::ScrollHandle,
    bars: Scrollbars,
) -> Div {
    let drawn = cells.len();
    let eyebrow = if held > drawn {
        format!("{named} · {drawn} shown")
    } else {
        named
    };
    let strip = div()
        .id(id)
        .flex()
        .flex_none()
        .items_start()
        .gap_4()
        .px(px(SHELF_INSET))
        .pb_3()
        .overflow_x_scroll()
        .track_scroll(&scroll)
        .children(cells);

    div()
        .flex()
        .flex_col()
        .flex_none()
        .border_b_1()
        .border_color(rgb(theme::border()))
        .child(div().px_6().pt_3().pb_2().child(kit::eyebrow(eyebrow)))
        .child(
            div()
                .relative()
                .child(strip)
                .child(bars.horizontal(id, scroll)),
        )
}

pub(crate) fn reached_ring() -> Div {
    div()
        .absolute()
        .inset(px(-REACHED_RING - 1.0))
        .rounded(px(REACHED_ROUNDING))
        .border(px(REACHED_RING))
        .border_color(rgb(theme::accent()))
}

const REACHED_ROUNDING: f32 = 10.0;

const REACHED_RING: f32 = 2.0;

#[cfg(test)]
mod tests {
    use resonate_library::{CoverSource, HeldMedium, Link, ReleaseDetail};

    use super::record_of;

    fn unreleased() -> ReleaseDetail {
        ReleaseDetail {
            mbid: None,
            group: None,
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            cover_source: CoverSource::File,
            may_have_a_front: true,
            asked: None,
            answered: None,
            links: Vec::new(),
            media: Vec::new(),
        }
    }

    #[test]
    fn a_record_names_every_fact_the_release_holds_and_an_empty_one_names_nothing() {
        assert!(record_of(&unreleased()).is_none());

        let mut release = unreleased();
        release.date = Some("1973-03-01".to_owned());
        release.country = Some("GB".to_owned());
        release.label = Some("Harvest".to_owned());
        release.catalog_number = Some("SHVL 804".to_owned());
        release.barcode = Some("077774638220".to_owned());
        release.kind = Some("Album".to_owned());
        release.disambiguation = Some("remaster".to_owned());
        release.media = vec![HeldMedium {
            position: 1,
            format: Some("CD".to_owned()),
            title: None,
        }];
        release.links = vec![
            Link::new(
                "free streaming",
                "https://music.apple.com/us/album/1".to_owned(),
            ),
            Link::new("free streaming", "https://bandcamp.com/album/1".to_owned()),
        ];

        let record = record_of(&release).expect("a full release is a record");
        let facts: Vec<(&str, &str)> = record
            .facts
            .iter()
            .map(|fact| (fact.label, fact.value.as_str()))
            .collect();

        assert_eq!(
            facts,
            [
                ("Released", "1973-03-01"),
                ("Format", "CD"),
                ("Label", "Harvest"),
                ("Catalogue number", "SHVL 804"),
                ("Barcode", "077774638220"),
                ("Country", "GB"),
                ("Kind", "Album"),
            ]
        );
        assert_eq!(record.note.as_deref(), Some("remaster"));
        assert_eq!(
            record.on.iter().map(|heard| heard.name).collect::<Vec<_>>(),
            ["Apple Music", "Bandcamp"]
        );
    }

    mod driven {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        use gpui::{ClipboardItem, TestAppContext};
        use parking_lot::Mutex;
        use resonate_core::{Isrc, SourceId};
        use resonate_library::{
            AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings,
            ArtistProfile, Barcode, BarcodeMatch, CoverArt, Credit, Discography, EnrichOptions,
            Fingerprinters, GroupAsked, GroupMatch, Issued, Library, Link, LinkNames, LookupOp,
            LyricText, LyricsAsked, Mbid, Medium, Recording, RecordingAsked, RecordingMatch,
            RecordingRelease, Reference, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch,
            ReleaseTrack, Result, SongLink, SongsAsked, StreamAsked, Track, TrackQuery,
        };
        use resonate_providers::{Identity, Obtained, Provider, Providers};

        use crate::{
            Beyond, Supplying,
            downloads::Fetching,
            driven::{Driven, Folder, Reaching},
            models::Selection,
            toast,
            views::{
                reorder::{Listed, Shift},
                root::{Deleting, Pane},
                search::SearchShows,
            },
        };

        const HEROES_TONIGHT: &str = "1a7d3b23-842a-4e57-8a8b-0f8b96a25f20";
        const HEROES_TONIGHT_RELEASE: &str = "d96b3b34-e52b-4f6a-bfa2-c52daddd64a1";
        const HEROES_TONIGHT_ISRC: &str = "GB2LD0902006";
        const HEROES_TONIGHT_GROUP: &str = "6c0b1f5e-3a2d-4f7e-9b8c-1d2e3f4a5b6c";
        const JANJI: &str = "0b3c6f5d-2e1a-4c8b-9d7e-6f5a4b3c2d1e";

        fn mbid(id: &str) -> Mbid {
            Mbid::new(id).expect("an mbid")
        }

        fn janji() -> Vec<Credit> {
            vec![Credit {
                name: "Janji".to_owned(),
                joined_by: " & Johnning".to_owned(),
                mbid: Some(mbid(JANJI)),
            }]
        }

        fn ncs_cover() -> CoverArt {
            let mut bytes = Vec::new();
            image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]))
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .expect("an image in memory");
            CoverArt {
                format: resonate_library::ImageFormat::Png,
                bytes,
            }
        }

        struct MusicBrainz {
            source: SourceId,
            searched: Arc<Mutex<Vec<String>>>,
            albums_searched: Arc<Mutex<Vec<String>>>,
            refusing: Arc<AtomicBool>,
            refusing_albums: Arc<AtomicBool>,
        }

        impl MusicBrainz {
            fn new() -> Self {
                Self {
                    source: SourceId::new("musicbrainz").expect("a source name"),
                    searched: Arc::default(),
                    albums_searched: Arc::default(),
                    refusing: Arc::default(),
                    refusing_albums: Arc::default(),
                }
            }

            fn sharing(&self) -> Self {
                Self {
                    source: self.source.clone(),
                    searched: Arc::clone(&self.searched),
                    albums_searched: Arc::clone(&self.albums_searched),
                    refusing: Arc::clone(&self.refusing),
                    refusing_albums: Arc::clone(&self.refusing_albums),
                }
            }
        }

        impl Reference for MusicBrainz {
            fn source(&self) -> &SourceId {
                &self.source
            }

            fn release(&self, id: &Mbid) -> Result<Option<Release>> {
                Ok((id.as_str() == HEROES_TONIGHT_RELEASE).then(heroes_tonight_release))
            }

            fn releases_by_barcode(&self, _: &Barcode) -> Result<Vec<BarcodeMatch>> {
                Ok(Vec::new())
            }

            fn find_release(&self, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
                Ok((asked.title == "Heroes Tonight")
                    .then(|| ReleaseMatch {
                        release: mbid(HEROES_TONIGHT_RELEASE),
                        group: None,
                        score: 100,
                        title: "Heroes Tonight".to_owned(),
                        credit: janji(),
                        track_count: Some(1),
                        date: Some("2015-12-22".to_owned()),
                    })
                    .into_iter()
                    .collect())
            }

            fn recording(&self, id: &Mbid) -> Result<Option<Recording>> {
                Ok((id.as_str() == HEROES_TONIGHT).then(|| heroes_tonight().into_recording()))
            }

            fn recordings_of_isrc(&self, isrc: &resonate_library::Isrc) -> Result<Vec<Recording>> {
                Ok((isrc.as_str() == HEROES_TONIGHT_ISRC)
                    .then(|| heroes_tonight().into_recording())
                    .into_iter()
                    .collect())
            }

            fn find_recording(&self, _: &RecordingAsked) -> Result<Vec<RecordingMatch>> {
                Ok(Vec::new())
            }

            fn find_songs(&self, asked: &SongsAsked) -> Result<Vec<RecordingMatch>> {
                self.searched.lock().push(asked.words.clone());
                if self.refusing.load(Ordering::Relaxed) {
                    return Err(resonate_library::Error::Refused {
                        op: LookupOp::FindRecording,
                        status: 503,
                    });
                }
                Ok(vec![heroes_tonight()])
            }

            fn release_group(&self, id: &Mbid) -> Result<Option<ReleaseGroup>> {
                Ok((id.as_str() == HEROES_TONIGHT_GROUP).then(|| ReleaseGroup {
                    id: mbid(HEROES_TONIGHT_GROUP),
                    title: "Heroes Tonight".to_owned(),
                    credit: janji(),
                    kind: Some("Single".to_owned()),
                    first_released: Some("2015-12-22".to_owned()),
                    disambiguation: None,
                    links: Vec::new(),
                    releases: Vec::new(),
                }))
            }

            fn find_release_group(&self, _: &GroupAsked) -> Result<Vec<GroupMatch>> {
                Ok(Vec::new())
            }

            fn find_albums(&self, words: &str) -> Result<Vec<AlbumMatch>> {
                self.albums_searched.lock().push(words.to_owned());
                if self.refusing_albums.load(Ordering::Relaxed) {
                    return Err(resonate_library::Error::Refused {
                        op: LookupOp::FindReleaseGroup,
                        status: 503,
                    });
                }
                Ok(vec![AlbumMatch {
                    group: mbid(HEROES_TONIGHT_GROUP),
                    score: 100,
                    title: "Heroes Tonight".to_owned(),
                    credit: janji(),
                    kind: Some("Album".to_owned()),
                    secondary: Vec::new(),
                    first_released: Some("2015-12-22".to_owned()),
                }])
            }

            fn group_cover(&self, _: &Mbid) -> Result<Option<CoverArt>> {
                Ok(None)
            }

            fn artist(&self, _: &Mbid) -> Result<Option<ArtistProfile>> {
                Ok(None)
            }

            fn find_artist(&self, _: &str) -> Result<Vec<ArtistMatch>> {
                Ok(Vec::new())
            }

            fn release_groups_of(&self, _: &Mbid, _: u32) -> Result<Discography> {
                Ok(Discography::default())
            }

            fn releases_of_group(&self, group: &Mbid) -> Result<Vec<Release>> {
                Ok((group.as_str() == HEROES_TONIGHT_GROUP)
                    .then(|| Release {
                        group: Some(mbid(HEROES_TONIGHT_GROUP)),
                        ..heroes_tonight_release()
                    })
                    .into_iter()
                    .collect())
            }

            fn releases_of_artist(&self, _: &Mbid, _: u32) -> Result<ArtistPressings> {
                Ok(ArtistPressings::default())
            }

            fn cover(&self, _: &Mbid, _: Option<&Mbid>) -> Result<Option<CoverArt>> {
                Ok(Some(ncs_cover()))
            }

            fn portrait(&self, _: &[Link]) -> Result<Option<CoverArt>> {
                Ok(None)
            }

            fn streamed_at(&self, _: &StreamAsked) -> Result<Option<Link>> {
                Ok(None)
            }

            fn lyrics(&self, _: &LyricsAsked) -> Result<Option<LyricText>> {
                Ok(None)
            }

            fn song_linked(&self, _: &SongLink) -> Result<Option<LinkNames>> {
                Ok(Some(LinkNames {
                    isrcs: vec![Isrc::new(HEROES_TONIGHT_ISRC).expect("an isrc")],
                    length: None,
                    title: None,
                    artist: None,
                }))
            }

            fn album_linked(&self, _: &AlbumLink) -> Result<Option<AlbumNames>> {
                Ok(None)
            }

            fn artist_linked(&self, _: &ArtistLink) -> Result<Option<String>> {
                Ok(None)
            }

            fn artist_at(&self, _: &str) -> Result<Option<Mbid>> {
                Ok(None)
            }
        }

        fn heroes_tonight_release() -> Release {
            Release {
                id: mbid(HEROES_TONIGHT_RELEASE),
                group: None,
                title: "Heroes Tonight".to_owned(),
                credit: janji(),
                date: Some("2015-12-22".to_owned()),
                country: None,
                label: None,
                catalog_number: None,
                barcode: None,
                kind: Some("Album".to_owned()),
                disambiguation: None,
                has_front_cover: true,
                links: Vec::new(),
                media: vec![Medium {
                    position: 1,
                    format: None,
                    title: None,
                    tracks: vec![ReleaseTrack {
                        position: 1,
                        number: "1".to_owned(),
                        title: "Heroes Tonight".to_owned(),
                        artist: None,
                        recording: Some(mbid(HEROES_TONIGHT)),
                        track: None,
                        length: None,
                        isrc: Some(HEROES_TONIGHT_ISRC.to_owned()),
                        links: Vec::new(),
                    }],
                }],
            }
        }

        fn heroes_tonight() -> RecordingMatch {
            RecordingMatch {
                recording: mbid(HEROES_TONIGHT),
                score: 100,
                title: "Heroes Tonight".to_owned(),
                credit: janji(),
                length: None,
                isrcs: vec![Isrc::new(HEROES_TONIGHT_ISRC).expect("an isrc")],
                releases: vec![RecordingRelease {
                    id: mbid(HEROES_TONIGHT_RELEASE),
                    title: "Heroes Tonight".to_owned(),
                    date: Some("2015-12-22".to_owned()),
                    disc: Some(1),
                    position: Some(1),
                    issued: Issued {
                        kind: Some("Album".to_owned()),
                        secondary: Vec::new(),
                        status: Some("Official".to_owned()),
                    },
                }],
            }
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        struct Asked {
            recording: Option<Mbid>,
            isrc: Option<Isrc>,
            artist: Option<String>,
        }

        struct Shop {
            source: SourceId,
            asked: Arc<Mutex<Vec<Asked>>>,
        }

        impl Provider for Shop {
            fn source(&self) -> &SourceId {
                &self.source
            }

            fn find(&self, identity: &Identity) -> resonate_providers::Result<Obtained> {
                self.asked.lock().push(Asked {
                    recording: identity.recording.clone(),
                    isrc: identity.isrc.clone(),
                    artist: identity.artist.clone(),
                });
                Ok(Obtained::Nothing)
            }
        }

        #[gpui::test]
        fn pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let asked = Arc::new(Mutex::new(Vec::new()));
            let told = Arc::clone(&asked);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(move |_: &Supplying<'_>| {
                    Providers::none().and(Arc::new(Shop {
                        source: SourceId::new("shop").expect("a source name"),
                        asked: Arc::clone(&told),
                    }))
                }),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

            let model = driven.read(|root, _| root.library.clone());
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| {
                    library.set_query("Heroes Tonight".to_owned(), cx);
                });
            });
            driven.until(|root, cx| !root.library.read(cx).found().is_empty());

            driven.click("unheld-title-0");
            driven.until(|_, _| !asked.lock().is_empty());
            driven.until(|root, cx| {
                let library = root.library.read(cx);
                library.downloads().first().is_some_and(|download| {
                    matches!(
                        library.fetching(download),
                        Fetching::Retrying { tries: 1, .. }
                    )
                })
            });
            driven.click("downloads");
            driven.bounds_of("download-0");

            assert!(
                driven.read(|_, cx| !toast::is_showing(cx)),
                "the download was told in a toast rather than the sidebar"
            );
            let wants = library.wants().expect("the wants read");
            assert_eq!(wants.len(), 1);
            assert_eq!(wants[0].artist.as_deref(), Some("Janji & Johnning"));
            assert_eq!(
                library.cover_art(wants[0].album).expect("the cover read"),
                Some(ncs_cover())
            );

            assert_eq!(
                asked.lock().clone(),
                vec![Asked {
                    recording: Some(mbid(HEROES_TONIGHT)),
                    isrc: Some(Isrc::new(HEROES_TONIGHT_ISRC).expect("an isrc")),
                    artist: Some("Janji & Johnning".to_owned()),
                }]
            );

            let recording = mbid(HEROES_TONIGHT);
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| library.cancel_download(&recording, cx));
            });
            driven.until(|root, cx| root.library.read(cx).downloads().is_empty());
            assert!(library.wants().expect("the wants read").is_empty());
        }

        struct Delivers {
            source: SourceId,
            file: std::path::PathBuf,
        }

        impl Provider for Delivers {
            fn source(&self) -> &SourceId {
                &self.source
            }

            fn find(&self, _: &Identity) -> resonate_providers::Result<Obtained> {
                Ok(Obtained::Found(resonate_providers::Delivery::File(
                    self.file.clone(),
                )))
            }
        }

        #[gpui::test]
        fn pressing_the_artist_of_a_song_found_on_musicbrainz_opens_the_artist_and_wants_nothing(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(|_: &Supplying<'_>| Providers::none()),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

            typed(&mut driven, "Heroes Tonight");
            driven.until(|root, cx| !root.library.read(cx).found().is_empty());
            driven.click("unheld-artist-0");
            driven.until(|root, cx| {
                matches!(root.library.read(cx).selection(), Selection::Artist(_))
            });

            let janji = library
                .artist_named("Janji")
                .expect("the artist read")
                .expect("the artist landed");
            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).selection()),
                Selection::Artist(janji)
            );
            assert!(
                library.wants().expect("the wants read").is_empty(),
                "pressing the artist wanted the song"
            );
        }

        #[gpui::test]
        fn a_found_song_that_landed_is_listed_once_as_the_held_track(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let music = Folder::new();
            let file = folder.tone("delivered.wav", 1);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(move |_: &Supplying<'_>| {
                    Providers::none().and(Arc::new(Delivers {
                        source: SourceId::new("shop").expect("a source name"),
                        file: file.clone(),
                    }))
                }),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);
            let music_folder = music.path().to_path_buf();
            driven.cx.update(|_, cx| {
                gpui::BorrowAppContext::update_global::<crate::ResonateApp, _>(cx, |global, _| {
                    global.music_folder = Some(music_folder);
                });
            });

            typed(&mut driven, "Heroes Tonight");
            driven.until(|root, cx| !root.library.read(cx).found().is_empty());
            driven.click("unheld-title-0");
            driven.until(|root, cx| {
                let library = root.library.read(cx);
                !library.listing().is_empty() && library.found().is_empty()
            });

            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).listing().len()),
                1
            );
        }

        #[gpui::test]
        fn a_title_by_an_artist_is_searched_as_that_title_by_that_artist(cx: &mut TestAppContext) {
            let folder = Folder::new();
            folder.tagged(
                "1.wav",
                1,
                &[(b"IART", "Stela Cole"), (b"INAM", "You F.O.")],
            );
            folder.tagged(
                "2.wav",
                1,
                &[(b"IART", "Somebody Else"), (b"INAM", "You F.O.")],
            );
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(|_: &Supplying<'_>| Providers::none()),
            };
            let mut driven = Driven::reaching(cx, library, &folder, reaching);

            let search = driven.read(|root, _| root.search.clone());
            driven.cx.update(|_, cx| {
                search.update(cx, |search, cx| {
                    search.set_text("You F O by stela cole".to_owned(), cx);
                });
            });
            driven.until(|root, cx| root.library.read(cx).meant().is_some());

            let artists = |driven: &mut Driven| {
                driven.read(|root, cx| {
                    root.library
                        .read(cx)
                        .listing()
                        .iter()
                        .map(|track| track.artist.clone().unwrap_or_default())
                        .collect::<Vec<_>>()
                })
            };
            assert_eq!(artists(&mut driven), ["Stela Cole"]);

            driven.click("search-as-typed");
            driven.until(|root, cx| root.library.read(cx).meant().is_none());

            assert!(
                artists(&mut driven).is_empty(),
                "the words as typed hold the word by and a misspelt name"
            );
        }

        #[gpui::test]
        fn a_search_begun_on_any_pane_opens_the_top_results(cx: &mut TestAppContext) {
            let folder = Folder::new();
            folder.tagged(
                "one.wav",
                1,
                &[(b"IART", "Janji"), (b"INAM", "Heroes"), (b"IPRD", "Album")],
            );
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            let mut driven = Driven::opened_in(cx, library, &folder);
            let search = driven.read(|root, _| root.search.clone());

            for sidebar in [
                "queue",
                "tab-albums",
                "tab-artists",
                "tab-tracks",
                "tab-settings",
            ] {
                driven.click(sidebar);
                driven.cx.update(|_, cx| {
                    search.update(cx, |search, cx| search.set_text("heroes".to_owned(), cx));
                });
                driven.until(|root, cx| root.search_in_front(cx).is_some());

                let (pane, shows) = driven.read(|root, cx| (root.pane, root.search_in_front(cx)));
                assert_eq!(shows, Some(SearchShows::Top), "searching from {sidebar}");
                assert!(
                    matches!(pane, Pane::Albums | Pane::Artists | Pane::Tracks),
                    "searching from {sidebar} left the pane at {pane:?}"
                );

                driven.cx.update(|_, cx| {
                    search.update(cx, |search, cx| search.clear(cx));
                });
                driven.until(|root, cx| root.search_in_front(cx).is_none());
            }
        }

        #[gpui::test]
        fn songs_not_in_the_library_stand_on_the_first_page_however_many_it_holds(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            for number in 0..40 {
                let title = format!("Heroes {number}");
                folder.tagged(
                    &format!("{number:02}.wav"),
                    1,
                    &[(b"IART", "Janji"), (b"INAM", title.as_str())],
                );
            }
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(|_: &Supplying<'_>| Providers::none()),
            };
            let mut driven = Driven::reaching(cx, library, &folder, reaching);

            let search = driven.read(|root, _| root.search.clone());
            driven.cx.update(|_, cx| {
                search.update(cx, |search, cx| search.set_text("heroes".to_owned(), cx));
            });
            driven.until(|root, cx| {
                let library = root.library.read(cx);
                library.listing().len() == 40 && !library.found().is_empty()
            });
            let found = driven.bounds_of("found-0");
            let seen = driven.cx.update(|window, _| window.viewport_size());

            assert!(
                found.bottom() <= seen.height,
                "the song found elsewhere was drawn below the window, at {found:?}"
            );
            assert_eq!(
                driven.read(|root, cx| root.search_in_front(cx)),
                Some(SearchShows::Top)
            );

            driven.click("search-songs");

            assert_eq!(
                driven.read(|root, cx| (root.pane, root.search_in_front(cx))),
                (Pane::Tracks, Some(SearchShows::Songs))
            );
            let (held, found) = driven.read(|root, cx| {
                let library = root.library.read(cx);
                (library.listing().len(), library.found().len())
            });
            let rows = driven.read(|root, cx| root.library.read(cx).rows());
            assert_eq!(rows.len(), held + 1 + found);
            assert_eq!(rows[held], crate::ListedRow::NotHeld(found));
            assert!(
                rows.iter()
                    .skip(held + 1)
                    .enumerate()
                    .all(|(at, row)| *row == crate::ListedRow::Found(at)),
                "the songs found follow the songs held, under their own heading"
            );
            assert!(driven.read(|root, cx| root.library.read(cx).found_at(held + 1).is_some()));

            driven.click("search-top");
            driven.bounds_of("found-0");

            driven.click("unheld-title-0");
            driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());
        }

        #[gpui::test]
        fn an_album_found_elsewhere_stands_under_the_albums_held_and_in_the_top_results(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            folder.tagged(
                "heroes.wav",
                1,
                &[
                    (b"IART", "Janji"),
                    (b"INAM", "Heroes Again"),
                    (b"IPRD", "Heroes"),
                ],
            );
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(|_: &Supplying<'_>| Providers::none()),
            };
            let mut driven = Driven::reaching(cx, library, &folder, reaching);
            let cell: &'static str =
                Box::leak(format!("album-found-{HEROES_TONIGHT_GROUP}").into_boxed_str());

            let search = driven.read(|root, _| root.search.clone());
            driven.cx.update(|_, cx| {
                search.update(cx, |search, cx| search.set_text("heroes".to_owned(), cx));
            });
            driven.until(|root, cx| !root.library.read(cx).albums_found().is_empty());
            let seen = driven.cx.update(|window, _| window.viewport_size());

            assert!(driven.bounds_of(cell).bottom() <= seen.height);

            driven.click("search-albums");

            assert_eq!(
                driven.read(|root, cx| (root.pane, root.search_in_front(cx))),
                (Pane::Albums, Some(SearchShows::Albums))
            );
            assert!(driven.bounds_of(cell).bottom() <= seen.height);
            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).albums().len()),
                1,
                "the album held is still the grid"
            );
        }

        #[gpui::test]
        fn a_missing_song_pressed_on_an_album_is_listed_among_the_downloads(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            folder.tagged(
                "other.wav",
                1,
                &[
                    (b"IART", "Janji & Johnning"),
                    (b"INAM", "Another Song"),
                    (b"IPRD", "Heroes Tonight"),
                ],
            );
            let asked = Arc::new(Mutex::new(Vec::new()));
            let told = Arc::clone(&asked);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(move |_: &Supplying<'_>| {
                    Providers::none().and(Arc::new(Shop {
                        source: SourceId::new("shop").expect("a source name"),
                        asked: Arc::clone(&told),
                    }))
                }),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            library
                .enrich(
                    Arc::new(MusicBrainz::new()),
                    Arc::new(Fingerprinters::none()),
                    EnrichOptions::default(),
                )
                .expect("the enrichment starts")
                .join()
                .expect("the enrichment finishes");
            let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

            let missing = library
                .missing_tracks(None, None)
                .expect("the missing tracks read")
                .remove(0);
            let model = driven.read(|root, _| root.library.clone());
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| {
                    library.select(Selection::Album(missing.album), cx);
                });
            });
            driven.until(|root, cx| !root.library.read(cx).release_tracks().is_empty());

            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| library.want(missing.release_track, cx));
            });
            driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());
            driven.until(|_, _| !asked.lock().is_empty());

            let listed = driven.read(|root, cx| {
                root.library
                    .read(cx)
                    .downloads()
                    .iter()
                    .map(|download| (download.found.title.clone(), download.found.artist.clone()))
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                listed,
                [("Heroes Tonight".to_owned(), "Janji & Johnning".to_owned())]
            );
            assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
        }

        fn shop_reaching(asked: &Arc<Mutex<Vec<Asked>>>, open: &Arc<AtomicBool>) -> Reaching {
            let told = Arc::clone(asked);
            let opened = Arc::clone(open);
            Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(move |_: &Supplying<'_>| {
                    if !opened.load(Ordering::Relaxed) {
                        return Providers::none();
                    }
                    Providers::none().and(Arc::new(Shop {
                        source: SourceId::new("shop").expect("a source name"),
                        asked: Arc::clone(&told),
                    }))
                }),
            }
        }

        fn heroes_tonight_found() -> resonate_library::Found {
            let matched = heroes_tonight();
            resonate_library::Found {
                recording: matched.recording,
                title: matched.title,
                artist: "Janji & Johnning".to_owned(),
                length: None,
                release: matched.releases.first().cloned(),
                releases: matched.releases,
                performer: None,
            }
        }

        #[gpui::test]
        fn a_song_still_wanted_is_listed_among_the_downloads_when_the_window_opens_again(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let asked = Arc::new(Mutex::new(Vec::new()));
            let open = Arc::new(AtomicBool::new(true));
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            library
                .want_found(&MusicBrainz::new(), &heroes_tonight_found())
                .expect("the song is wanted");

            let mut driven = Driven::reaching(cx, library, &folder, shop_reaching(&asked, &open));
            driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());

            let listed = driven.read(|root, cx| {
                let library = root.library.read(cx);
                library
                    .downloads()
                    .iter()
                    .map(|download| {
                        (
                            download.found.title.clone(),
                            library.fetching(download).is_underway(),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            assert_eq!(listed, [("Heroes Tonight".to_owned(), true)]);
        }

        #[gpui::test]
        fn a_song_asked_for_before_any_provider_was_set_up_is_fetched_once_one_is(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let asked = Arc::new(Mutex::new(Vec::new()));
            let open = Arc::new(AtomicBool::new(false));
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            let mut driven = Driven::reaching(cx, library, &folder, shop_reaching(&asked, &open));

            let model = driven.read(|root, _| root.library.clone());
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| {
                    library.want_found(heroes_tonight_found(), cx);
                });
            });
            driven.until(|root, cx| {
                let library = root.library.read(cx);
                library
                    .downloads()
                    .first()
                    .is_some_and(|download| library.fetching(download) == Fetching::NoProvider)
            });
            assert!(asked.lock().is_empty());

            open.store(true, Ordering::Relaxed);
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| library.sources_moved(cx));
            });
            driven.until(|_, _| !asked.lock().is_empty());

            assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
        }

        #[gpui::test]
        fn a_song_link_pasted_into_the_search_is_downloaded_and_leaves_the_box_as_it_was(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let asked = Arc::new(Mutex::new(Vec::new()));
            let told = Arc::clone(&asked);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(move |_: &Supplying<'_>| {
                    Providers::none().and(Arc::new(Shop {
                        source: SourceId::new("shop").expect("a source name"),
                        asked: Arc::clone(&told),
                    }))
                }),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

            let search = driven.read(|root, _| root.search.clone());
            driven.cx.update(|window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=a1".to_owned(),
                ));
                search.update(cx, |search, cx| search.pastes(window, cx));
            });
            driven.until(|_, _| !asked.lock().is_empty());

            assert_eq!(
                driven.read(|root, cx| root.search.read(cx).text().to_owned()),
                "",
                "the link was neither kept in the box nor swapped for the song's name"
            );
            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).downloads().len()),
                1
            );
            assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
            let wants = library.wants().expect("the wants read");
            assert_eq!(wants.len(), 1);
            assert_eq!(wants[0].title, "Heroes Tonight");
        }

        #[gpui::test]
        fn an_album_link_pasted_into_the_search_wants_every_song_of_the_album_it_names(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let asked = Arc::new(Mutex::new(Vec::new()));
            let told = Arc::clone(&asked);
            let reaching = Reaching {
                reference: Arc::new(MusicBrainz::new()),
                register: Arc::new(move |_: &Supplying<'_>| {
                    Providers::none().and(Arc::new(Shop {
                        source: SourceId::new("shop").expect("a source name"),
                        asked: Arc::clone(&told),
                    }))
                }),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

            let search = driven.read(|root, _| root.search.clone());
            driven.cx.update(|window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(format!(
                    "https://musicbrainz.org/release-group/{HEROES_TONIGHT_GROUP}"
                )));
                search.update(cx, |search, cx| search.pastes(window, cx));
            });
            driven.until(|_, _| !asked.lock().is_empty());

            assert_eq!(
                driven.read(|root, cx| root.search.read(cx).text().to_owned()),
                "",
                "the album link was taken for words to search"
            );
            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).downloads().len()),
                1
            );
            assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
            let wants = library.wants().expect("the wants read");
            assert_eq!(wants.len(), 1);
            assert_eq!(wants[0].album_title, "Heroes Tonight");
        }

        fn searching(musicbrainz: &MusicBrainz, cx: &mut TestAppContext) -> Driven {
            let folder = Folder::new();
            let reaching = Reaching {
                reference: Arc::new(musicbrainz.sharing()),
                register: Arc::new(|_: &Supplying<'_>| Providers::none()),
            };
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

            Driven::reaching(cx, library, &folder, reaching)
        }

        fn typed(driven: &mut Driven, query: &str) {
            let model = driven.read(|root, _| root.library.clone());
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| library.set_query(query.to_owned(), cx));
            });
        }

        fn answered(driven: &mut Driven) {
            driven.until(|root, cx| !root.library.read(cx).is_asking_elsewhere());
        }

        #[gpui::test]
        fn words_searched_again_are_answered_from_memory_rather_than_asked_twice(
            cx: &mut TestAppContext,
        ) {
            let musicbrainz = MusicBrainz::new();
            let mut driven = searching(&musicbrainz, cx);

            typed(&mut driven, "Heroes Tonight");
            answered(&mut driven);
            typed(&mut driven, "janji heroes");
            answered(&mut driven);
            typed(&mut driven, "heroes   TONIGHT");
            let at_once = driven.read(|root, cx| root.library.read(cx).found().len());
            answered(&mut driven);

            assert_eq!(at_once, 1, "an answer held in memory waited to be drawn");
            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).found().len()),
                1
            );
            assert_eq!(
                musicbrainz.searched.lock().clone(),
                ["heroes tonight", "janji heroes"]
            );
        }

        #[gpui::test]
        fn songs_found_for_fewer_words_stay_listed_while_more_are_asked_for(
            cx: &mut TestAppContext,
        ) {
            let musicbrainz = MusicBrainz::new();
            let mut driven = searching(&musicbrainz, cx);

            typed(&mut driven, "heroes");
            answered(&mut driven);
            typed(&mut driven, "heroes ton");
            let narrowed = driven.read(|root, cx| {
                let library = root.library.read(cx);
                (library.found().len(), library.is_asking_elsewhere())
            });
            typed(&mut driven, "radiohead");
            let unrelated = driven.read(|root, cx| root.library.read(cx).found().len());

            assert_eq!(narrowed, (1, true));
            assert_eq!(unrelated, 0);
        }

        #[gpui::test]
        fn a_search_musicbrainz_refused_says_so_and_is_asked_again_on_a_press(
            cx: &mut TestAppContext,
        ) {
            let musicbrainz = MusicBrainz::new();
            musicbrainz.refusing.store(true, Ordering::Relaxed);
            let mut driven = searching(&musicbrainz, cx);

            typed(&mut driven, "heroes tonight");
            driven
                .until(|root, cx| root.library.read(cx).elsewhere() == Some(Beyond::Unreached(0)));
            musicbrainz.refusing.store(false, Ordering::Relaxed);
            driven.click("ask-elsewhere-again");
            driven.until(|root, cx| !root.library.read(cx).found().is_empty());

            assert_eq!(
                musicbrainz.searched.lock().clone(),
                ["heroes tonight", "heroes tonight"]
            );
        }

        fn asked_now(driven: &mut Driven) {
            let model = driven.read(|root, _| root.library.clone());
            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| library.ask_elsewhere_now(cx));
            });
        }

        #[gpui::test]
        fn songs_found_stand_where_musicbrainz_refused_the_albums_which_are_asked_for_again(
            cx: &mut TestAppContext,
        ) {
            let musicbrainz = MusicBrainz::new();
            musicbrainz.refusing_albums.store(true, Ordering::Relaxed);
            let mut driven = searching(&musicbrainz, cx);

            typed(&mut driven, "heroes tonight");
            answered(&mut driven);
            let refused = driven.read(|root, cx| {
                let library = root.library.read(cx);
                (library.found().len(), library.albums_found().len())
            });
            musicbrainz.refusing_albums.store(false, Ordering::Relaxed);
            typed(&mut driven, "janji");
            answered(&mut driven);
            typed(&mut driven, "heroes tonight");
            answered(&mut driven);

            assert_eq!(refused, (1, 0));
            assert_eq!(
                driven.read(|root, cx| root.library.read(cx).albums_found().len()),
                1
            );
            assert_eq!(
                musicbrainz.albums_searched.lock().clone(),
                ["heroes tonight", "janji", "heroes tonight"]
            );
        }

        #[gpui::test]
        fn a_search_typed_past_does_not_ask_musicbrainz_for_its_albums(cx: &mut TestAppContext) {
            let musicbrainz = MusicBrainz::new();
            let mut driven = searching(&musicbrainz, cx);

            typed(&mut driven, "heroes tonight");
            asked_now(&mut driven);
            typed(&mut driven, "janji heroes");
            asked_now(&mut driven);
            answered(&mut driven);

            assert_eq!(
                musicbrainz.searched.lock().clone(),
                ["heroes tonight", "janji heroes"]
            );
            assert_eq!(musicbrainz.albums_searched.lock().clone(), ["janji heroes"]);
        }

        #[gpui::test]
        fn albums_found_stay_listed_while_the_words_are_edited(cx: &mut TestAppContext) {
            let musicbrainz = MusicBrainz::new();
            let mut driven = searching(&musicbrainz, cx);

            typed(&mut driven, "heroes tonight");
            answered(&mut driven);
            typed(&mut driven, "heroes tonigh");
            let while_asked = driven.read(|root, cx| {
                let library = root.library.read(cx);
                (library.albums_found().len(), library.is_asking_elsewhere())
            });

            assert_eq!(while_asked, (1, true));
        }

        #[gpui::test]
        fn every_found_song_below_the_held_ones_can_be_reached_from_the_keyboard(
            cx: &mut TestAppContext,
        ) {
            let musicbrainz = MusicBrainz::new();
            let folder = Folder::new();
            folder.tone("Heroes Tonight.wav", 1);
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            let reaching = Reaching {
                reference: Arc::new(musicbrainz.sharing()),
                register: Arc::new(|_: &Supplying<'_>| Providers::none()),
            };
            let mut driven = Driven::reaching(cx, library, &folder, reaching);

            typed(&mut driven, "heroes tonight");
            answered(&mut driven);
            driven.until(|root, cx| {
                let library = root.library.read(cx);
                !library.listing().is_empty() && !library.found().is_empty()
            });
            driven.focus(|root| &root.search);
            driven.cx.simulate_keystrokes("down down");
            driven.settle();

            assert!(driven.read(|root, _| root.reaches(Shift::Listing(Listed::Top), 1)));
            driven.cx.simulate_keystrokes("enter");
            driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());
        }

        #[gpui::test]
        fn a_search_inside_an_artist_asks_musicbrainz_nothing(cx: &mut TestAppContext) {
            let musicbrainz = MusicBrainz::new();
            let mut driven = searching(&musicbrainz, cx);
            let model = driven.read(|root, _| root.library.clone());
            let artist = resonate_core::ArtistId::new(1).expect("a non-zero id");

            driven.cx.update(|_, cx| {
                model.update(cx, |library, cx| {
                    library.select(Selection::Artist(artist), cx)
                });
            });
            typed(&mut driven, "heroes tonight");
            asked_now(&mut driven);
            driven.settle();

            assert!(musicbrainz.searched.lock().is_empty());
        }

        fn asked_to_delete(driven: &mut Driven, track: &Track) {
            let root = driven.root.clone();
            let deleting = Deleting::of(track);
            driven.cx.update(|_, cx| {
                root.update(cx, |root, cx| root.ask_to_delete(deleting, cx));
            });
            driven.settle();
        }

        #[gpui::test]
        fn deleting_a_track_asks_first_and_takes_its_file_only_once_confirmed(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let doomed = folder.tone("doomed.wav", 1);
            let kept = folder.tone("kept.wav", 1);
            let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
            Driven::scanned(&library, &folder);
            let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
            let track = library
                .tracks(&TrackQuery::default())
                .expect("the tracks read")
                .into_iter()
                .find(|track| {
                    track
                        .location
                        .as_path()
                        .is_some_and(|path| path.ends_with("doomed.wav"))
                })
                .expect("the track was scanned");

            asked_to_delete(&mut driven, &track);
            driven.click("keep-the-track");
            let kept_on_cancel = doomed.exists();
            asked_to_delete(&mut driven, &track);
            driven.click("delete-the-track");
            driven.until(|_, _| !doomed.exists());
            driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);

            assert!(
                kept_on_cancel,
                "the file went before the delete was confirmed"
            );
            assert!(kept.exists());
            assert!(library.track(track.id).expect("the track reads").is_none());
        }
    }
}
