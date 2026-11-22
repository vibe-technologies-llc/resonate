use std::time::SystemTime;

use gpui::{
    Context, Div, FontWeight, Role, SharedString, Stateful, Window, div, prelude::*, px, rgb,
};
use resonate_core::{AlbumId, ArtistId};
use resonate_library::{ArtistDetail, ArtistTotals, Favoured, Measured};
use smallvec::smallvec;

use super::{
    artists::portrait_frame,
    menus::{album_menu, album_menu_ending},
    record::{genre_names, heard_on, is_matched, record_of, service_names},
};
use crate::{
    Drawn, LibraryModel, Portrayed, ResonateApp, Selection, format,
    icons::Icon,
    theme,
    views::{
        hint::Names,
        kit,
        kit::{KeepsItsWidth, Tone},
        listing,
        listing::Pictured,
        menu,
        missing::MissingShows,
        playlists::Naming,
        root::{Magnified, Pane, RootView},
        sorting::Ordered,
        spoken::Spoken,
    },
    words,
};

impl RootView {
    pub(super) fn heading(&self, cx: &mut Context<Self>) -> Div {
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
        let reads = match library.narrows_the_page() {
            true => library.search().reads(),
            false => Vec::new(),
        };
        let naming = self.naming.filter(|naming| naming.is_a_search());
        let narrowing = self.narrowing_the_page(cx);

        heading
            .when(!reads.is_empty() || narrowing.is_some(), |heading| {
                heading.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .when(!reads.is_empty(), |row| row.child(listing::reads(&reads)))
                        .when_some(narrowing, Div::child),
                )
            })
            .when_some(naming, |heading, naming| {
                heading.child(self.naming_row(naming, cx))
            })
    }

    fn narrowing_the_page(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let library = self.library.read(cx);

        if !library.words_wait_beside_the_page() {
            return None;
        }

        let narrows = library.narrows_the_page();
        let button = match narrows {
            true => kit::button(
                "show-the-whole-page",
                Some(Icon::Close),
                words::browser::show_the_whole_page(),
                words::browser::show_the_whole_page_hint(),
                Tone::Ghost,
            ),
            false => kit::button(
                "narrow-by-the-search",
                Some(Icon::Search),
                words::browser::narrow_by_the_search(library.query()),
                words::browser::narrow_by_the_search_hint(),
                Tone::Ghost,
            ),
        };

        Some(self.in_the_pane_ring(
            button,
            move |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.narrow_the_page(!narrows, cx));
            },
            cx,
        ))
    }

    fn library_heading(&self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let searching = !library.query().is_empty();
        let naming = self.naming.filter(|naming| naming.is_a_search());
        let adding_to = self.adding_songs_to;

        let target = adding_to.and_then(|id| library.saved_playlist(id));
        let title = target.map_or_else(
            || words::browser::all_tracks().to_owned(),
            |playlist| words::browser::add_songs_to(playlist.name.as_str()),
        );
        let under = target.map_or_else(
            || summary(library.listed(), None),
            |playlist| words::browser::choose_tracks_to_add_to(playlist.name.as_str()),
        );

        let sung = self.sung_offer(Tone::Ghost, cx);
        let actions = kit::actions()
            .when_some(adding_to, |bar, _| {
                bar.child(self.in_the_pane_ring(
                    kit::button(
                        "finish-adding-songs",
                        Some(Icon::Back),
                        words::browser::done(),
                        words::playlists::finish_adding_hint(),
                        Tone::Ghost,
                    ),
                    |this, _, cx| this.finish_adding_songs(cx),
                    cx,
                ))
            })
            .when(adding_to.is_none(), |bar| {
                bar.when_some(sung, |bar, sung| bar.child(sung))
                    .when(searching && naming.is_none(), |bar| {
                        bar.child(self.in_the_pane_ring(
                            kit::button(
                                "save-search",
                                Some(Icon::Search),
                                words::browser::save_this_search(),
                                words::playlists::save_search_hint(),
                                Tone::Ghost,
                            ),
                            |this, window, cx| {
                                this.name_a_playlist(Naming::Query(None), window, cx);
                            },
                            cx,
                        ))
                    })
                    .child(self.shuffle_all(cx))
                    .child(self.play_all(cx))
                    .child(self.orders_a_listing("order-tracks", Ordered::Tracks, cx))
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
                        .when(adding_to.is_some(), |column| {
                            column.child(kit::eyebrow(words::browser::add_songs_label()))
                        })
                        .child(kit::title(title))
                        .child(kit::subtitle(under)),
                )
                .child(actions),
        )
    }

    fn album_page_heading(&self, id: AlbumId, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let album = library.album_of(id);
        let title = SharedString::from(album.map_or_else(
            || words::browser::unnamed_album(id.to_string()),
            |album| album.title.clone(),
        ));
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
        let words_within = self.hero_words_within();

        let cover = self.in_the_pane_ring(
            self.cover_sized(Pictured::Album(id), Drawn::OnThePage, kit::hero_side(), cx)
                .id("magnify-scoped-cover")
                .said_as(Role::Button, words::transport::cover_hint())
                .cursor_pointer()
                .hover(|cover| cover.opacity(kit::LIT))
                .names(words::transport::cover_hint()),
            move |this, _, cx| this.magnify(Magnified::Album(id), cx),
            cx,
        );
        let cover = menu::opens_a_menu(
            cover,
            move |this, at, cx| {
                let owner = this
                    .library
                    .read(cx)
                    .album_of(id)
                    .and_then(|album| album.artist_id);

                album_menu_ending(album_menu(at, id), id, owner).does(
                    Icon::Albums,
                    words::menu::magnify(),
                    move |this, _, cx| this.magnify(Magnified::Album(id), cx),
                )
            },
            cx,
        );

        let about = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .child(kit::eyebrow(words::browser::album()))
            .child(kit::hero_title(title, words_within))
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
                                words::browser::open_artist_hint(),
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
                        row.child(self.in_the_pane_ring(
                            kit::icon_button(
                                "download-album-missing",
                                Icon::Download,
                                words::browser::get_album_rest_hint(),
                            ),
                            move |this, _, cx| {
                                this.library
                                    .update(cx, |library, cx| library.want_missing_tracks(id, cx));
                            },
                            cx,
                        ))
                    })
                    .when_some(record, |row, _| {
                        row.child(self.in_the_pane_ring_at(
                            kit::icon_button(
                                "album-record",
                                Icon::Info,
                                words::browser::record_hint(),
                            ),
                            move |this, at, cx| this.show_the_record(id, at, cx),
                            cx,
                        ))
                    }),
            );

        kit::heading().child(self.way_back(cx)).child(
            kit::hero(self.rooms.hero_row.clone())
                .child(kit::hero_art(cover))
                .child(about),
        )
    }

    fn artist_page_heading(&self, id: ArtistId, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let around = self.around_the_rows(cx);

        let name = artist_named(library, around.artist.as_ref(), id);
        let under = held_by_the_artist(around.totals, self.drawn_at());
        let detail = around.artist.as_ref();
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
        let records = around.records.len();
        let any_records = records + library.albums_not_held().len();
        let tracks = library.listed().rows as usize;
        let shows = self.shows.artist.within(any_records);

        let words_within = self.hero_words_within();
        let side = kit::hero_side();
        let portrait = match self.library.update(cx, |library, cx| {
            library.portrait(id, Portrayed::OnThePage, cx)
        }) {
            Some(art) => portrait_frame(art, side).into_any_element(),
            None => kit::avatar(&name, true)
                .size(px(side))
                .text_size(px(theme::text_title() * 1.6))
                .into_any_element(),
        };

        let about = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .child(kit::eyebrow(words::browser::artist()))
            .child(kit::hero_title(SharedString::from(name), words_within))
            .child(kit::subtitle(under))
            .when_some(profile, |column, profile| {
                column.child(kit::subtitle(profile))
            })
            .when_some(
                heard_on(self, "artist-heard-on", services, words_within, cx),
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
                    row.child(self.in_the_pane_ring_at(
                        kit::icon_button(
                            "artist-genres",
                            Icon::Info,
                            words::browser::genres_hint(),
                        ),
                        move |this, at, cx| this.show_the_artist(id, at, cx),
                        cx,
                    ))
                })
                .when_some(unheld, |row, (unheld, unread)| {
                    row.child(self.in_the_pane_ring(
                        kit::button(
                            "unheld-releases",
                            Some(Icon::Missing),
                            match unread {
                                0 => words::browser::releases_not_held(unheld),
                                unread => {
                                    words::browser::releases_not_held_more_unread(unheld, unread)
                                }
                            },
                            words::browser::unheld_hint(),
                            Tone::Ghost,
                        ),
                        |this, _, cx| {
                            this.show_what_is_missing(MissingShows::Releases, cx);
                            this.set_pane(Pane::Missing, cx);
                        },
                        cx,
                    ))
                    .when(unread > 0 && reads_further, |row| {
                        row.child(self.in_the_pane_ring(
                            kit::button(
                                "read-the-rest",
                                Some(Icon::Import),
                                words::browser::read_the_rest(),
                                words::browser::read_the_rest_hint(),
                                Tone::Ghost,
                            ),
                            move |this, _, cx| {
                                this.library
                                    .update(cx, |library, cx| library.read_the_rest_of(id, cx));
                            },
                            cx,
                        ))
                    })
                })
                .when(any_records > 0, |row| {
                    row.child(div().flex_1())
                        .child(self.artist_tabs(shows, records, tracks, cx))
                }),
            );

        kit::heading().child(self.way_back(cx)).child(
            kit::hero(self.rooms.hero_row.clone())
                .child(kit::hero_art(portrait))
                .child(about),
        )
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
                self.in_the_pane_ring(
                    kit::segment(("artist-shows", shown as usize), label, shows == shown)
                        .gap_2()
                        .child(kit::figure(format::count(count)).text_color(rgb(theme::faint()))),
                    move |this, _, cx| this.show_of_the_artist(shown, cx),
                    cx,
                )
            };

        kit::segmented()
            .child(tab(
                ArtistShows::Records,
                words::browser::albums(),
                records,
                cx,
            ))
            .child(tab(
                ArtistShows::Tracks,
                words::browser::tracks(),
                tracks,
                cx,
            ))
    }

    fn show_of_the_artist(&mut self, shows: ArtistShows, cx: &mut Context<Self>) {
        self.shows.artist = shows;
        if shows == ArtistShows::Records {
            self.change_the_orders(cx, |orders| orders.ordering = None);
        }
        cx.notify();
    }

    fn way_back(&self, cx: &mut Context<Self>) -> Div {
        let label = self
            .way_back_to(cx)
            .unwrap_or_else(|| SharedString::new_static(self.in_front(cx).label()));

        div().flex().child(self.in_the_pane_ring(
            kit::way_back(
                "way-back",
                label,
                words::browser::way_back_hint(key!(leave)),
            ),
            |this, _, cx| this.step_back(cx),
            cx,
        ))
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
                row.child(self.orders_a_listing("order-tracks", Ordered::Tracks, cx))
            })
    }

    pub(crate) fn play_all(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        self.play_songs_button(
            |this, window, cx| {
                this.with_everything_listed(window, cx, |this, listing, _, cx| {
                    this.plays_in_order(cx);
                    this.play(&listing, 0, cx);
                });
            },
            cx,
        )
    }

    pub(crate) fn shuffle_all(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        self.shuffle_songs_button(
            |this, window, cx| {
                this.with_everything_listed(window, cx, |this, listing, _, cx| {
                    this.play_shuffled(&listing, cx);
                });
            },
            cx,
        )
    }

    pub(crate) fn play_songs_button(
        &self,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.in_the_pane_ring(
            kit::button(
                "play-all",
                Some(Icon::Play),
                words::browser::play(),
                words::browser::play_all_hint(),
                Tone::Primary,
            ),
            press,
            cx,
        )
    }

    pub(crate) fn shuffle_songs_button(
        &self,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.in_the_pane_ring(
            kit::button(
                "shuffle-all",
                Some(Icon::Shuffle),
                words::browser::shuffle(),
                words::browser::shuffle_all_hint(),
                Tone::Outlined,
            ),
            press,
            cx,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArtistShows {
    #[default]
    Records,
    Tracks,
}

impl ArtistShows {
    pub(super) const fn within(self, records: usize) -> Self {
        match (self, records) {
            (Self::Records, 0) => Self::Tracks,
            (shows, _) => shows,
        }
    }
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

pub(super) fn artist_named(
    library: &LibraryModel,
    opened: Option<&ArtistDetail>,
    id: ArtistId,
) -> String {
    opened
        .map(|detail| detail.name.clone())
        .or_else(|| {
            library
                .artists()
                .iter()
                .find(|artist| artist.id == id)
                .map(|artist| artist.name.clone())
        })
        .unwrap_or_else(|| words::browser::unnamed_artist(id.to_string()))
}

fn held_by_the_artist(totals: ArtistTotals, now: SystemTime) -> String {
    let mut parts: format::Parts<String> = smallvec![
        words::browser::albums_counted(totals.albums),
        words::browser::tracks_counted(totals.tracks),
    ];

    if let Some(length) = totals.length {
        parts.push(format::spanned(length));
    }
    if totals.plays > 0 {
        parts.push(words::browser::plays_counted(totals.plays));
    }
    if let Some(played) = totals.played {
        parts.push(format::since(played, now));
    }

    parts.join(" · ")
}

fn summary(listed: Measured, year: Option<i32>) -> String {
    let rows = listed.rows as usize;
    let lossless = listed.lossless as usize;
    let mut parts: format::Parts<String> = smallvec![words::browser::tracks_counted(rows)];

    if let Some(year) = year {
        parts.insert(0, year.to_string());
    }
    if let Some(total) = listed.length {
        parts.push(format::spanned(total));
    }
    if lossless > 0 && lossless == rows {
        parts.push(words::browser::all_lossless().to_owned());
    } else if lossless > 0 {
        parts.push(words::browser::lossless(lossless));
    }

    parts.join(" · ")
}
