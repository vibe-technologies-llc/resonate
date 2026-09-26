use std::{
    cmp::Reverse,
    slice,
    sync::Arc,
    time::{Duration, SystemTime},
};

use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, ElementId, FontWeight, Image, MouseButton,
    MouseDownEvent, ObjectFit, Pixels, Point, SharedString, Stateful, anchored, deferred, div,
    hsla, img, point, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{AlbumId, ArtistId, ReleaseTrackId};
use resonate_engine::Placement;
use resonate_library::{
    Album, Artist, ArtistDetail, ArtistTotals, Column, Cut, Favoured, Found, Genre, HeldMedium,
    HeldReleaseTrack, Link, Lit, Measured, MissingTrack, PlaylistEntry, ReleaseDetail, Service,
    Track,
};
use smallvec::smallvec;

use crate::{
    Beyond, Drawn, LibraryModel, ListedRow, Portrayed, ResonateApp, Selection, format,
    icons::{self, Icon},
    theme,
    views::{
        hint::Names,
        kit::{self, EndsInAnEllipsis, KeepsItsWidth, Press, Tone},
        listing::{self, Pictured},
        menu::{self, Called, Menu},
        missing::MissingShows,
        playlists::{ADD_ALL_HINT, ADD_HINT, Held, Naming, ROW_GROUP, SAVE_SEARCH_HINT},
        pointed::{self, LitUnderThePointer},
        reorder::{self, Listed, Shift},
        root::{Magnified, Pane, RootView, empty, listed, row, tall_row},
        scrollbar::Scrollbars,
        sorting,
        transport::COVER_HINT,
    },
};

const SEVERAL: &str = "Various artists";

const OTHER_COPIES_HINT: &str =
    "Also held in other formats; this is the best of them, and the row's menu plays the others";

const WAY_BACK_HINT: &str = "Back to where this was opened from — escape";

const ORDER_HINT: &str = "Choose what this listing is put in order by";

const NEXT_ALL_HINT: &str = "Hear every track listed here straight after the track playing";

const QUEUE_ALL_HINT: &str = "Queue every track listed here after what is already queued, ahead of the rest of what is playing";

const PLAY_ALL_HINT: &str = "Play every track listed here, in place of the queue";

const SHUFFLE_ALL_HINT: &str = "Play every track listed here, shuffled";

pub(crate) const OPEN_ALBUM_HINT: &str = "See the tracks on this album";

pub(crate) const OPEN_ARTIST_HINT: &str = "See every track by this artist";

const INSTEAD_HINT: &str = "Search for the spelling the catalog holds instead";

const SUNG_HINT: &str = "Search the lyrics for these words, in the order they were typed";

const WANT_HINT: &str = "Mark this track wanted, so a provider can be asked for it";

const UNWANT_HINT: &str = "Stop wanting this track";

const WANT_FOUND_HINT: &str =
    "Mark this song wanted: its release is added to the catalog and the providers are asked for it";

const WANTING_HINT: &str = "Adding its release to the catalog";

const UNHELD_COVER: f32 = 0.45;

const UNHELD_HINT: &str = "See what this artist has released that the library does not hold";

const RECORD_HINT: &str = "About this record";

const GENRES_HINT: &str = "Genres";

const RECORD_LABEL: f32 = 112.0;

pub(crate) const FAVOUR_HINT: &str = "Keep this among your favourites";

pub(crate) const UNFAVOUR_HINT: &str = "Take this out of your favourites";

const CONTROLS: f32 = 4.0;

const CONTROL_GAP: f32 = 2.0;

pub(crate) fn controls_width() -> f32 {
    theme::row_control() * CONTROLS + CONTROL_GAP * (CONTROLS - 1.0)
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
        let mark = match already {
            true => kit::lit_mark(id, Icon::Favourited, UNFAVOUR_HINT),
            false => kit::icon_button(id, Icon::Favourite, FAVOUR_HINT),
        };

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
        let found_nothing = nothing.then(|| {
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
        });
        let columns = self.grid_columns();
        let held = albums.len();
        let rows = held.div_ceil(columns.max(1));
        let measured = self.grid_width.clone();
        let laid_out = self.grid_width.get() > px(0.0);
        if laid_out {
            self.land_where_it_was_left(rows);
        }

        let heading = kit::heading()
            .child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
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
                        .child(kit::measures_its_width(measured))
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
            .into_any_element()
    }

    pub(crate) fn grid_columns(&self) -> usize {
        let width = f32::from(self.grid_width.get()) - 48.0;
        let cell = theme::grid_cover() + theme::grid_gap();
        (((width + theme::grid_gap()) / cell).floor() as usize).max(1)
    }

    fn album_cell(&self, album: &Album, reached: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        self.album_cell_captioned(album, theme::grid_cover(), Caption::ByArtist, reached, cx)
    }

    fn album_cell_at(&self, album: &Album, side: f32, cx: &mut Context<Self>) -> Stateful<Div> {
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

    fn nothing_matched(
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

    fn sung_offer(&self, tone: Tone, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
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

        let found_nothing = nothing.then(|| {
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
        });

        let heading = kit::heading()
            .child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(kit::eyebrow("LIBRARY"))
                            .child(kit::title("Artists"))
                            .child(kit::subtitle(format::counted(counted, "artist", "artists"))),
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
                                            .id(index)
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
        let found_nothing =
            nothing.then(|| self.nothing_matched(Icon::Tracks, "No tracks match.", None, cx));
        let heading = self.heading(cx);
        let in_an_album = matches!(self.library.read(cx).selection(), Selection::Album(_));
        let rowed = !rows.is_empty();
        let release_tracks = self.library.read(cx).release_tracks();
        let unheld = self.library.read(cx).unheld();
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
                let held = self.library.read(cx).artist_albums().len();
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
                    sorting::tracks_sorted(self, cx),
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
                                        Some(ListedRow::Beyond(beyond)) => {
                                            drawn.push(beyond_heading(beyond).into_any_element());
                                        }
                                        Some(ListedRow::Unheld(at)) => {
                                            let Some(row) = unheld.get(at) else {
                                                continue;
                                            };
                                            drawn.push(
                                                this.unheld_row(at, Unheld::searched_for(row), cx)
                                                    .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Found(at)) => {
                                            let Some(row) = found.get(at) else {
                                                continue;
                                            };
                                            drawn.push(
                                                this.unheld_row(at, Unheld::found(row), cx)
                                                    .into_any_element(),
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
        let holding = Arc::clone(tracks);
        let menued = Arc::clone(tracks);
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
        let number = match (in_an_album, track.track_number) {
            (true, Some(number)) => SharedString::from(number.to_string()),
            (true, None) => SharedString::new_static(""),
            (false, _) => SharedString::from((index + 1).to_string()),
        };

        let row = row(playing)
            .id(index)
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
            .child(listing::format_cell(Some((track.codec, track.spec))))
            .child(listing::heard(track.plays, track.played, self.drawn_at()))
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
                            .child(self.add_control(
                                ("track-to-playlist", index),
                                ADD_HINT,
                                move || held_at(&holding, index),
                                cx,
                            )),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| match plays {
                Plays::TheseRows => this.play(&played, index, cx),
                Plays::AsTheListingIsDrawn { .. } => {
                    let Some((drawn, start)) = this.library.read(cx).played_from_held(index) else {
                        return;
                    };
                    this.play(&drawn, start, cx);
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

                Menu::at(at)
                    .queues(move || Arc::clone(&queued))
                    .holds(move || Held::of(Arc::from([held.clone()])))
                    .when_some(others, Menu::offers_the_other_copies)
                    .reaches(track.album_id, track.artist_id)
                    .favours(Favoured::Track(track.id), favourite)
                    .offers_the_file(&track.location, names)
                    .shares(track.id)
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
            },
            cx,
        )
    }

    pub(crate) fn unheld_row(&self, index: usize, unheld: Unheld, cx: &mut Context<Self>) -> Div {
        let Unheld {
            asks,
            number,
            title,
            artist,
            length,
            beside,
        } = unheld;
        let mark = self.want_mark(index, asks, cx);
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

        row(false)
            .child(listing::number_cell(number))
            .when_some(beside.pictured(), |row, pictured| {
                row.child(match pictured {
                    Some(album) => self
                        .cover(listing::Pictured::Album(album), cx)
                        .opacity(UNHELD_COVER),
                    None => unheld_cover(),
                })
            })
            .child(listing::title_cell(title, lit_title, false).text_color(rgb(theme::faint())))
            .child(
                listing::artist_cell(
                    self.opens(
                        ("missing-artist", index),
                        artist,
                        OPEN_ARTIST_HINT,
                        None,
                        cx,
                    )
                    .flex_shrink()
                    .ends_in_an_ellipsis(),
                )
                .text_color(rgb(theme::faint())),
            )
            .when_some(
                match beside {
                    Beside::AnAlbum => Some(listing::format_cell(None)),
                    Beside::ARun => None,
                    Beside::ASearch { on, .. } => Some(
                        listing::format_cell(None)
                            .child(kit::figure(on).text_color(rgb(theme::faint())).truncate()),
                    ),
                },
                |row, format| row.child(format).child(listing::unheard()),
            )
            .child(listing::length_cell(length).text_color(rgb(theme::faint())))
            .child(controls_place().child(mark))
    }

    fn want_mark(&self, index: usize, asks: Asks, cx: &mut Context<Self>) -> Stateful<Div> {
        match asks {
            Asks::Row(release_track) => match self.library.read(cx).wanted(release_track) {
                Some(want) => kit::icon_button(("unwant", index), Icon::Wanted, UNWANT_HINT)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.unwant(want, cx));
                    })),
                None => kit::icon_button(("want", index), Icon::Want, WANT_HINT).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.want(release_track, cx));
                    }),
                ),
            },
            Asks::Found(found) => {
                if self.library.read(cx).is_wanting(&found) {
                    return kit::mark_when(
                        Press::Greyed,
                        ("wanting-found", index),
                        Icon::Wanted,
                        WANTING_HINT,
                        ROW_GROUP,
                    );
                }
                kit::icon_button(("want-found", index), Icon::Want, WANT_FOUND_HINT).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let wanted = found.clone();
                        this.library
                            .update(cx, |library, cx| library.want_found(wanted, cx));
                    }),
                )
            }
        }
    }

    fn artists_drawn_as(&self, drawn: ArtistsDrawn, cx: &mut Context<Self>) -> Div {
        let choice = |as_: ArtistsDrawn, label: &'static str, cx: &mut Context<Self>| {
            kit::segment(("artists-drawn", as_ as usize), label, drawn == as_)
                .names(as_.saying())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.artists_drawn = as_;
                    cx.notify();
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
            .child(kit::measures_its_width(measured))
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
        let under = summary(library.listed(), None);

        let sung = self.sung_offer(Tone::Ghost, cx);
        let actions = kit::actions()
            .when_some(sung, |bar, sung| bar.child(sung))
            .when(searching && naming.is_none(), |bar| {
                bar.child(
                    kit::button(
                        "save-search",
                        Some(Icon::Search),
                        "Save this search",
                        SAVE_SEARCH_HINT,
                        Tone::Ghost,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.name_a_playlist(Naming::Query(None), window, cx);
                    })),
                )
            })
            .child(self.add_all_to_a_playlist(cx))
            .child(self.orders_a_listing("order-tracks", cx))
            .child(self.queue_all(Placement::Next, cx))
            .child(self.queue_all(Placement::Queued, cx))
            .child(self.play_all(cx));

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(theme::heading_name()))
                        .gap_1()
                        .child(kit::eyebrow("LIBRARY"))
                        .child(kit::title("All tracks"))
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
        let favourite = library.favoured_album(id);

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
            .map(|detail| detail.releases_unheld as usize)
            .filter(|unheld| missing_shown && *unheld > 0);
        let favourite = library.favoured_artist(id);
        let records = library.artist_albums().len();
        let tracks = library.listed().rows as usize;
        let shows = self.artist_shows.within(records);

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
                    false,
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
                .when_some(unheld, |row, unheld| {
                    row.child(
                        kit::button(
                            "unheld-releases",
                            Some(Icon::Missing),
                            format!(
                                "{} not held",
                                format::counted(unheld, "release", "releases")
                            ),
                            UNHELD_HINT,
                            Tone::Ghost,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_what_is_missing(MissingShows::Releases, cx);
                            this.set_pane(Pane::Missing, cx);
                        })),
                    )
                })
                .when(records > 0, |row| {
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
            self.record = Some(OpenedRecord::Album { album, at });
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
                OpenedRecord::Album { album, at } => {
                    if library.selection() != Selection::Album(album) {
                        return None;
                    }
                    let record = library.release().and_then(record_of)?;
                    (at, record_card(&record))
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
                        .child(card),
                )
                .with_priority(1),
            )
            .into_any_element()
    }

    fn play_all(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "play-all",
            Some(Icon::Play),
            "Play",
            PLAY_ALL_HINT,
            Tone::Primary,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.with_everything_listed(window, cx, |this, listing, _, cx| {
                this.play(&listing, 0, cx);
            });
        }))
    }

    fn shuffle_all(&self, cx: &mut Context<Self>) -> Stateful<Div> {
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

    fn queue_all(&self, placement: Placement, cx: &mut Context<Self>) -> Stateful<Div> {
        let (id, icon, label, saying) = match placement {
            Placement::Next => ("next-all", Icon::QueueNext, menu::PLAY_NEXT, NEXT_ALL_HINT),
            Placement::Queued | Placement::At(_) => (
                "last-all",
                Icon::QueueLast,
                menu::ADD_TO_QUEUE,
                QUEUE_ALL_HINT,
            ),
        };

        kit::button(id, Some(icon), label, saying, Tone::Outlined).on_click(cx.listener(
            move |this, _, window, cx| {
                this.with_everything_listed(window, cx, move |this, listing, _, cx| {
                    let queued = listed(&listing);
                    this.queue(&queued, placement, cx);
                });
            },
        ))
    }

    fn add_all_to_a_playlist(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "add-all",
            Some(Icon::Plus),
            "Add to playlist",
            ADD_ALL_HINT,
            Tone::Ghost,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.with_everything_listed(window, cx, |this, listing, window, cx| {
                let holding: Arc<[Cut]> = listing.iter().map(Cut::of).collect();
                this.hold_for_a_playlist(Held::of(holding), window, cx);
            });
        }))
    }

    fn artist_records(&self, artist: ArtistId, cx: &mut Context<Self>) -> Stateful<Div> {
        let albums = self.library.read(cx).artist_albums();
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

    fn artist_cell_at(&self, artist: &Artist, side: f32, cx: &mut Context<Self>) -> Stateful<Div> {
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

    pub(crate) fn orders_a_listing(
        &self,
        named: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        kit::icon_button(named, Icon::Sort, ORDER_HINT)
            .when(self.ordering, |button| button.bg(rgb(theme::hover())))
            .on_click(cx.listener(|this, _, _, cx| this.order_a_listing(cx)))
    }

    fn tracks_in_order(&self, cx: &mut Context<Self>) -> Div {
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

    fn albums_in_order(&self, cx: &mut Context<Self>) -> Div {
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

    fn artists_in_order(&self, cx: &mut Context<Self>) -> Div {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArtistsDrawn {
    #[default]
    List,
    Grid,
}

impl ArtistsDrawn {
    const fn saying(self) -> &'static str {
        match self {
            Self::List => "Draw the artists as a list, a small portrait beside each name",
            Self::Grid => "Draw the artists as a grid of large portraits, the way albums are",
        }
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
    Found(Found),
}

pub(crate) enum Beside {
    AnAlbum,
    ARun,
    ASearch {
        pictured: Option<AlbumId>,
        on: SharedString,
    },
}

impl Beside {
    const fn pictured(&self) -> Option<Option<AlbumId>> {
        match self {
            Self::AnAlbum | Self::ARun => None,
            Self::ASearch { pictured, .. } => Some(*pictured),
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

    fn searched_for(row: &MissingTrack) -> Self {
        Self {
            number: SharedString::new_static(""),
            artist: SharedString::from(
                row.artist
                    .clone()
                    .or_else(|| row.owner.clone())
                    .unwrap_or_default(),
            ),
            beside: Beside::ASearch {
                pictured: Some(row.album),
                on: SharedString::from(row.album_title.clone()),
            },
            ..Self::from(row)
        }
    }

    fn found(found: &Found) -> Self {
        Self {
            number: SharedString::new_static(""),
            title: SharedString::from(found.title.clone()),
            artist: SharedString::from(found.artist.clone()),
            length: SharedString::from(found.length.map(format::spanned).unwrap_or_default()),
            beside: Beside::ASearch {
                pictured: None,
                on: SharedString::from(
                    found
                        .release
                        .as_ref()
                        .map(|release| release.title.clone())
                        .unwrap_or_default(),
                ),
            },
            asks: Asks::Found(found.clone()),
        }
    }
}

fn beyond_heading(beyond: Beyond) -> Div {
    let said = match beyond {
        Beyond::InTheCatalog(rows) => format!(
            "Not in the library · {}",
            format::counted(rows, "track", "tracks")
        ),
        Beyond::Elsewhere(rows) => format!(
            "Found on MusicBrainz · {}",
            format::counted(rows, "song", "songs")
        ),
        Beyond::Asking => "Asking MusicBrainz…".to_owned(),
    };

    run_heading(SharedString::from(said))
}

fn unheld_cover() -> Div {
    let side = theme::row_cover();

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
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_end()
        .w(px(controls_width()))
}

pub(crate) fn portrait_frame(art: Arc<Image>, side: f32) -> Div {
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
    let record = Record { facts, note, on };

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
}

impl Record {
    fn is_empty(&self) -> bool {
        self.facts.is_empty() && self.note.is_none() && self.on.is_empty()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OpenedRecord {
    Album { album: AlbumId, at: Point<Pixels> },
    Artist { artist: ArtistId, at: Point<Pixels> },
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

fn held_at(tracks: &Arc<[Track]>, index: usize) -> Held {
    Held::of(
        tracks
            .get(index)
            .map_or_else(|| Arc::from([]), |track| Arc::from([Cut::of(track)])),
    )
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
        .px_6()
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

fn reached_ring() -> Div {
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
}
