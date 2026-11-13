use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Pixels, Point, SharedString, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, rgb, uniform_list,
};
use resonate_core::MediaLocation;
use resonate_engine::{Command, Placement, QueueItem, Unclaimed};

use crate::{
    Drawn, Notice, format,
    icons::{self, Icon},
    served::ServedSong,
    serving::{Failure, Page, Place, Row, ServerModel, favourites_in},
    theme,
    views::{
        browser::controls_width,
        field::{Field, Submitted},
        hint::Names,
        kit::{self, EndsInAnEllipsis as _, Tone},
        listing,
        menu::Menu,
        root::{RootView, framed_cover, row},
        scrollbar::Scrollbars,
        spoken::{self, Spoken as _},
    },
    words,
};

const SEGMENTS_AT_MOST: usize = 3;

const SERVER_CONTROLS: usize = 4;

const SEARCH_ID: &str = "served-search";

pub(crate) struct ServicesPane {
    pub(crate) search: Entity<Field>,
    pub(crate) rows: UniformListScrollHandle,
    server: Entity<ServerModel>,
    window_focus: FocusHandle,
}

impl ServicesPane {
    pub(crate) fn new(
        server: Entity<ServerModel>,
        window_focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| Field::new(words::served::search_placeholder, window, cx));
        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
            this.search_the_server(window, cx);
        })
        .detach();

        Self {
            search,
            rows: UniformListScrollHandle::new(),
            server,
            window_focus,
        }
    }

    fn search_the_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let words = self.search.read(cx).text().trim().to_owned();
        self.server.update(cx, |server, cx| {
            if words.is_empty() {
                server.leave_the_search(cx);
            } else {
                server.open(Place::Found { words }, cx);
            }
        });
        window.focus(&self.window_focus, cx);
        cx.notify();
    }
}

impl RootView {
    pub(crate) fn server_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.server.read(cx);
        let place = model.place().clone();
        let page = model.page().clone();
        let way_back = model.way_back_name();

        let heading = self.server_heading(&place, &page, way_back, cx);
        let body = match page {
            Page::Reading => kit::empty(
                Icon::Server,
                words::served::asking(),
                Some(words::served::asking_more()),
            ),
            Page::Failed(failure) => self.server_failed(failure, cx),
            Page::Read(rows) if rows.is_empty() => match place {
                Place::Found { .. } => kit::empty(
                    Icon::Server,
                    words::served::nothing_found(),
                    Some(words::served::nothing_found_more()),
                ),
                Place::Artists => kit::empty(
                    Icon::Server,
                    words::served::nothing_listed(),
                    Some(words::served::search_first()),
                ),
                Place::Artist { .. } | Place::Album { .. } | Place::Mix { .. } => kit::empty(
                    Icon::Server,
                    words::served::nothing_here(),
                    Some(words::served::nothing_here_more()),
                ),
            },
            Page::Read(rows) => self.server_rows_listed(rows, place, cx),
        };

        kit::pane().child(heading).child(body).into_any_element()
    }

    fn server_failed(&self, failure: Failure, cx: &mut Context<Self>) -> AnyElement {
        kit::trouble_offering(
            Icon::Server,
            failure.says(),
            Some(failure.advises()),
            self.in_the_pane_ring(
                kit::button(
                    "server-ask-again",
                    None,
                    words::served::try_again(),
                    words::served::ask_again_hint(),
                    Tone::Outlined,
                ),
                |this, _, cx| this.server.update(cx, |server, cx| server.read_again(cx)),
                cx,
            ),
        )
    }

    fn server_heading(
        &self,
        place: &Place,
        page: &Page,
        way_back: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let counted = match page {
            Page::Read(rows) => Some(counted_in(rows, place)),
            Page::Reading | Page::Failed(_) => None,
        };
        let title = match (place, page) {
            (Place::Artists, Page::Read(rows)) if favourites_in(rows) => {
                words::served::favourites().to_owned()
            }
            _ => place.title(),
        };
        let playable = place
            .location()
            .filter(|whole| self.server.read(cx).plays_songs(whole))
            .cloned();

        let mut actions = kit::actions();
        if let Some(whole) = playable {
            let playing = whole.clone();
            let queueing = whole;
            actions = actions
                .child(self.in_the_pane_ring(
                    kit::button(
                        "server-play",
                        Some(Icon::Play),
                        words::served::play(),
                        words::served::play_hint(),
                        Tone::Primary,
                    ),
                    move |this, _, cx| this.play_what_the_server_holds(playing.clone(), cx),
                    cx,
                ))
                .child(self.in_the_pane_ring(
                    kit::icon_button("server-queue", Icon::QueueLast, words::served::queue_hint()),
                    move |this, _, cx| {
                        this.queue_what_the_server_holds(queueing.clone(), Placement::Queued, cx);
                    },
                    cx,
                ));
        }
        let keepable = match (place, page) {
            (Place::Mix { location, name }, Page::Read(rows))
                if !rows.is_empty() && self.server.read(cx).keeps_songs(location) =>
            {
                Some((name.clone(), Arc::clone(rows)))
            }
            _ => None,
        };
        if let Some((name, rows)) = keepable {
            actions = actions.child(self.in_the_pane_ring(
                kit::button(
                    "server-keep",
                    Some(Icon::Download),
                    words::served::keep(),
                    words::served::keep_all_hint(),
                    Tone::Primary,
                ),
                move |this, _, cx| {
                    this.library.update(cx, |library, cx| {
                        library.keep_mix(name.clone(), &rows, cx);
                    });
                },
                cx,
            ));
        }
        actions = actions.child(self.server_search_field(cx));

        let model = self.server.read(cx);
        let titles = model.titles();
        let chosen = model.chosen();
        let chooser = (titles.len() > 1).then(|| self.service_chooser(&titles, chosen, cx));
        let eyebrow = match place {
            Place::Artists if chooser.is_some() => None,
            Place::Artists => Some(
                titles
                    .get(chosen)
                    .map_or_else(|| place.eyebrow().to_owned(), |title| title.clone()),
            ),
            Place::Artist { .. }
            | Place::Album { .. }
            | Place::Mix { .. }
            | Place::Found { .. } => Some(place.eyebrow().to_owned()),
        };

        let heading = kit::heading()
            .when_some(chooser, |heading, chooser| heading.child(chooser))
            .when_some(way_back, |heading, name| {
                heading.child(self.in_the_pane_ring(
                    kit::way_back("server-back", name, words::served::back_hint()),
                    |this, _, cx| {
                        this.server.update(cx, |server, cx| {
                            server.back(cx);
                        });
                        this.clear_the_services_search(cx);
                    },
                    cx,
                ))
            });

        heading.child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(theme::width(theme::heading_name()))
                        .gap_1()
                        .when_some(eyebrow, |column, eyebrow| {
                            column.child(kit::eyebrow(eyebrow))
                        })
                        .child(kit::title(SharedString::from(title)))
                        .when_some(counted, |column, counted| {
                            column.child(kit::subtitle(SharedString::from(counted)))
                        }),
                )
                .child(actions),
        )
    }

    fn service_chooser(&self, titles: &[String], chosen: usize, cx: &mut Context<Self>) -> Div {
        if titles.len() > SEGMENTS_AT_MOST {
            return self.service_pick(titles, chosen, cx);
        }

        titles
            .iter()
            .enumerate()
            .fold(kit::tab_strip(), |chooser, (index, title)| {
                chooser.child(
                    self.in_the_pane_ring(
                        kit::tab(("served-service", index), title.clone(), index == chosen)
                            .names(words::served::choose_hint()),
                        move |this, _, cx| this.choose_a_service(index, cx),
                        cx,
                    ),
                )
            })
    }

    fn service_pick(&self, titles: &[String], chosen: usize, cx: &mut Context<Self>) -> Div {
        let offered = titles.to_vec();
        let shown = titles.get(chosen).cloned().unwrap_or_default();

        div().flex().flex_none().child(self.in_the_pane_ring_at(
            kit::pick("served-service", shown, words::served::choose_among_hint()),
            move |this, at, cx| {
                let menu = services_menu(at, &offered, chosen);
                this.open_a_menu(menu, cx);
            },
            cx,
        ))
    }

    fn choose_a_service(&mut self, index: usize, cx: &mut Context<Self>) {
        self.server
            .update(cx, |server, cx| server.choose(index, cx));
        self.clear_the_services_search(cx);
    }

    pub(crate) fn clear_the_services_search(&self, cx: &mut App) {
        self.services
            .read(cx)
            .search
            .clone()
            .update(cx, |field, cx| {
                field.clear(cx);
            });
    }

    fn server_search_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::framed_field(SEARCH_ID, false)
            .w(theme::width(theme::picker_width()))
            .child(icons::icon(
                Icon::Search,
                theme::search_icon(),
                theme::faint(),
            ))
            .child(self.services.read(cx).search.clone())
            .on_click(cx.listener(|this, _, window, cx| {
                let search = this.services.read(cx).search.clone();
                search.update(cx, |field, cx| field.take_focus(window, cx));
                cx.notify();
            }))
    }

    fn server_rows_listed(
        &mut self,
        rows: Arc<[Row]>,
        place: Place,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = rows.len();
        let songs_here = matches!(place, Place::Album { .. } | Place::Mix { .. });
        let scroll = self.services.read(cx).rows.clone();

        Scrollbars::of(cx)
            .around(
                "server-scrollbar",
                scroll.clone(),
                uniform_list(
                    "server-rows",
                    shown,
                    cx.processor(move |this, range: Range<usize>, _, cx| {
                        range
                            .filter_map(|index| {
                                let row = rows.get(index)?;
                                Some(
                                    this.server_row(index, row, &rows, songs_here, cx)
                                        .into_any_element(),
                                )
                            })
                            .collect()
                    }),
                )
                .track_scroll(&scroll)
                .h_full()
                .w_full(),
            )
            .into_any_element()
    }

    fn server_row(
        &mut self,
        index: usize,
        row: &Row,
        rows: &Arc<[Row]>,
        songs_here: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(location) = row.location().cloned() else {
            return self.server_heading_row(row).into_any_element();
        };
        let cover = self.served_cover(&location, row, cx);
        let (title, by, figure) = describe(row);
        let said = spoken::joined([title.as_str(), by.as_deref().unwrap_or_default()]);

        let from = songs_here.then(|| songs_from(rows, index)).flatten();
        let pressed = row.clone();
        let plays = self.server.read(cx).plays_songs(&location);
        let press_does_nothing = row.song().is_some() && !plays;
        let song_number = row
            .song()
            .and_then(|song| song.track)
            .map(|track| SharedString::from(track.to_string()));

        let open = div()
            .id(("server-row", index))
            .debug_selector(move || format!("server-row-{index}"))
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .items_center()
            .gap(px(listing::COLUMN_GAP))
            .when(!press_does_nothing, |open| open.cursor_pointer())
            .listed_as(move || said.clone())
            .names(match row {
                Row::Song(_) if !plays => words::served::not_played_here_hint(),
                Row::Song(_) => words::served::play_this_hint(),
                Row::Heading(_) | Row::Artist(_) | Row::Album(_) | Row::Mix(_) => {
                    words::served::open_hint()
                }
            })
            .when_some(song_number, |open, number| {
                open.child(listing::number_cell(number))
            })
            .child(framed_cover(cover, theme::row_cover()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div().flex().items_center().gap_2().child(
                            div()
                                .min_w(px(0.0))
                                .truncate()
                                .ends_in_an_ellipsis()
                                .text_color(rgb(theme::text()))
                                .child(SharedString::from(title)),
                        ),
                    )
                    .when_some(by, |column, by| {
                        column.child(
                            div()
                                .truncate()
                                .ends_in_an_ellipsis()
                                .text_size(px(theme::text_xs()))
                                .text_color(rgb(theme::muted()))
                                .child(SharedString::from(by)),
                        )
                    }),
            )
            .when_some(figure, |open, figure| open.child(kit::figure(figure)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !press_does_nothing {
                    this.press_a_row_of_the_server(pressed.clone(), from.clone(), cx);
                }
            }));

        let favourite = row.song().cloned();
        row_frame()
            .group(crate::views::playlists::ROW_GROUP)
            .when(!press_does_nothing, |row| {
                row.hover(|row| row.bg(rgb(theme::hover())))
            })
            .child(open)
            .child(self.server_controls(index, location, favourite, cx))
            .into_any_element()
    }

    fn server_heading_row(&self, row: &Row) -> Div {
        let Row::Heading(name) = row else {
            return div();
        };
        row_frame().items_end().child(kit::eyebrow(*name))
    }

    fn server_controls(
        &self,
        index: usize,
        location: MediaLocation,
        favourite: Option<ServedSong>,
        cx: &mut Context<Self>,
    ) -> Div {
        let keepable = favourite
            .clone()
            .filter(|song| self.server.read(cx).keeps_songs(&song.location));
        let keep = keepable.map(|song| {
            let row = Row::Song(song);
            self.in_the_pane_ring(
                kit::icon_button(
                    ("server-row-keep", index),
                    Icon::Download,
                    words::served::keep_hint(),
                ),
                move |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.keep(&row, cx));
                },
                cx,
            )
        });
        let starred =
            favourite.filter(|song| self.server.read(cx).keeps_favourites(&song.location));
        let star = starred.map(|song| {
            let favoured = self.server.read(cx).is_favoured(&song);
            let saying = if favoured {
                words::browser::unfavour_hint()
            } else {
                words::browser::favour_hint()
            };
            let which = SharedString::from(song.location.to_uri());
            self.in_the_pane_ring(
                kit::star(("server-row-favourite", index), favoured, which, saying),
                move |this, _, cx| this.favour_a_served_song(song.clone(), cx),
                cx,
            )
        });
        let plays = self.server.read(cx).plays_songs(&location);
        let playing = location.clone();
        let next = location.clone();
        let queued = location;
        let controls = self
            .row_controls(("server-controls", index), cx)
            .children(star)
            .children(keep);

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .w(px(controls_width(SERVER_CONTROLS)))
            .child(controls.when(plays, |controls| {
                controls
                    .child(self.in_the_pane_ring(
                        kit::icon_button(
                            ("server-row-play", index),
                            Icon::Play,
                            words::served::play_hint(),
                        ),
                        move |this, _, cx| this.play_what_the_server_holds(playing.clone(), cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(
                            ("server-row-next", index),
                            Icon::QueueNext,
                            words::served::next_hint(),
                        ),
                        move |this, _, cx| {
                            this.queue_what_the_server_holds(next.clone(), Placement::Next, cx);
                        },
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(
                            ("server-row-queue", index),
                            Icon::QueueLast,
                            words::served::queue_hint(),
                        ),
                        move |this, _, cx| {
                            this.queue_what_the_server_holds(queued.clone(), Placement::Queued, cx);
                        },
                        cx,
                    ))
            }))
    }

    fn served_cover(
        &mut self,
        location: &MediaLocation,
        row: &Row,
        cx: &mut Context<Self>,
    ) -> Option<crate::models::Picture> {
        let known = row.cover().cloned();
        self.server.update(cx, |server, cx| {
            server.cover(location, known.as_ref(), Drawn::InARow, cx)
        })
    }

    fn press_a_row_of_the_server(
        &mut self,
        pressed: Row,
        from: Option<(Vec<MediaLocation>, usize)>,
        cx: &mut Context<Self>,
    ) {
        match pressed {
            Row::Artist(artist) => self.server.update(cx, |server, cx| {
                server.open(
                    Place::Artist {
                        location: artist.location,
                        name: artist.name,
                    },
                    cx,
                );
            }),
            Row::Album(album) => self.server.update(cx, |server, cx| {
                server.open(
                    Place::Album {
                        location: album.location,
                        name: album.name,
                    },
                    cx,
                );
            }),
            Row::Mix(mix) => self.server.update(cx, |server, cx| {
                server.open(
                    Place::Mix {
                        location: mix.location,
                        name: mix.name,
                    },
                    cx,
                );
            }),
            Row::Song(song) => match from {
                Some((locations, start)) => self.play_served(locations, start, cx),
                None => self.play_served(vec![song.location], 0, cx),
            },
            Row::Heading(_) => {}
        }
    }

    fn favour_a_served_song(&mut self, song: ServedSong, cx: &mut Context<Self>) {
        let Some(serves) = self.server.read(cx).serves() else {
            return;
        };
        let favoured = !self.server.read(cx).is_favoured(&song);
        let location = song.location;
        self.server.update(cx, |server, cx| {
            server.note_favoured(location.clone(), favoured);
            cx.notify();
        });
        cx.spawn(async move |this, cx| {
            let kept = {
                let location = location.clone();
                cx.background_executor()
                    .spawn(async move { serves.favour(&location, favoured) })
                    .await
            };
            let _ = this.update(cx, |this, cx| {
                let Err(error) = kept else {
                    return;
                };
                tracing::warn!(%error, "the service did not keep the favourite");
                this.server
                    .update(cx, |server, _| server.note_favoured(location, !favoured));
                this.report(
                    Notice::Trouble(words::served::favourite_unchanged(
                        Failure::of(&error).said_after(),
                    )),
                    cx,
                );
            });
        })
        .detach();
    }

    pub(crate) fn play_served(
        &mut self,
        locations: Vec<MediaLocation>,
        start_at: usize,
        cx: &mut Context<Self>,
    ) {
        let mut minting = Unclaimed::beside(&[]);
        let items: Vec<QueueItem> = locations
            .into_iter()
            .map(|location| QueueItem::whole(minting.mint(), location))
            .collect();
        if items.is_empty() {
            return;
        }
        self.plays_in_order(cx);
        self.library
            .update(cx, |library, _| library.set_playing_playlist(None));
        self.send(
            Command::Load {
                items,
                start_at,
                autoplay: true,
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn queue_served(
        &mut self,
        locations: Vec<MediaLocation>,
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        let mut minting = Unclaimed::beside(self.player.read(cx).queue().iter().map(Arc::as_ref));
        let items: Vec<QueueItem> = locations
            .into_iter()
            .map(|location| QueueItem::whole(minting.mint(), location))
            .collect();
        let rows = items.len();
        if rows == 0 {
            return;
        }
        let play = self.player.read(cx).state().current.is_none();
        self.send(Command::Insert { items, at, play }, cx);
        self.report(Notice::Done(queued_words(rows, at, play)), cx);
        cx.notify();
    }

    pub(crate) fn play_what_the_server_holds(
        &mut self,
        named: MediaLocation,
        cx: &mut Context<Self>,
    ) {
        self.with_the_songs_of(named, cx, |this, songs, cx| this.play_served(songs, 0, cx));
    }

    pub(crate) fn queue_what_the_server_holds(
        &mut self,
        named: MediaLocation,
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        self.with_the_songs_of(named, cx, move |this, songs, cx| {
            this.queue_served(songs, at, cx);
        });
    }

    fn with_the_songs_of(
        &mut self,
        named: MediaLocation,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Vec<MediaLocation>, &mut Context<Self>) + 'static,
    ) {
        let Some(serves) = self.server.read(cx).serves() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let expanded = cx
                .background_executor()
                .spawn(async move { serves.expand(&named) })
                .await;
            let _ = this.update(cx, |this, cx| match expanded {
                Ok(songs) => then(this, songs, cx),
                Err(error) => {
                    tracing::warn!(%error, "the Subsonic server did not list what was pressed");
                    this.report(
                        Notice::Trouble(words::served::unread(Failure::of(&error).said_after())),
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}

fn row_frame() -> Div {
    row(false)
}

fn describe(row: &Row) -> (String, Option<String>, Option<String>) {
    match row {
        Row::Heading(name) => ((*name).to_owned(), None, None),
        Row::Artist(artist) => (
            artist.name.clone(),
            None,
            artist.albums.map(words::served::albums),
        ),
        Row::Album(album) => (
            album.name.clone(),
            album.artist.clone(),
            Some(
                [
                    album.year.map(|year| year.to_string()),
                    album.songs.map(words::served::songs),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
            )
            .filter(|said| !said.is_empty()),
        ),
        Row::Mix(mix) => (mix.name.clone(), mix.artist.clone(), None),
        Row::Song(song) => (
            song.title.clone(),
            by_line(song),
            song.length.map(format::spanned),
        ),
    }
}

fn by_line(song: &ServedSong) -> Option<String> {
    match (song.artist.as_deref(), song.album.as_deref()) {
        (Some(artist), Some(album)) => Some(format!("{artist} · {album}")),
        (Some(only), None) | (None, Some(only)) => Some(only.to_owned()),
        (None, None) => None,
    }
}

pub(crate) fn songs_from(rows: &[Row], index: usize) -> Option<(Vec<MediaLocation>, usize)> {
    let songs: Vec<(usize, MediaLocation)> = rows
        .iter()
        .enumerate()
        .filter_map(|(at, row)| row.song().map(|song| (at, song.location.clone())))
        .collect();
    let start = songs.iter().position(|(at, _)| *at == index)?;

    Some((
        songs.into_iter().map(|(_, location)| location).collect(),
        start,
    ))
}

fn counted_in(rows: &[Row], place: &Place) -> String {
    let count = |matching: fn(&Row) -> bool| rows.iter().filter(|row| matching(row)).count();
    match place {
        Place::Artists if !favourites_in(rows) => {
            words::served::artists(count(|row| matches!(row, Row::Artist(_))))
        }
        Place::Artist { .. } => words::served::albums(count(|row| matches!(row, Row::Album(_)))),
        Place::Album { .. } | Place::Mix { .. } => {
            words::served::songs(count(|row| matches!(row, Row::Song(_))))
        }
        Place::Artists | Place::Found { .. } => format!(
            "{} · {} · {} · {}",
            words::served::artists(count(|row| matches!(row, Row::Artist(_)))),
            words::served::albums(count(|row| matches!(row, Row::Album(_)))),
            words::served::mixes(count(|row| matches!(row, Row::Mix(_)))),
            words::served::songs(count(|row| matches!(row, Row::Song(_)))),
        ),
    }
}

fn queued_words(rows: usize, at: Placement, play: bool) -> String {
    match (play, at) {
        (true, _) => words::served::queued_and_playing(rows),
        (false, Placement::Next) => words::served::to_play_next(rows),
        (false, Placement::Queued | Placement::At(_)) => words::served::added_to_the_queue(rows),
    }
}

fn services_menu(at: Point<Pixels>, titles: &[String], chosen: usize) -> Menu {
    titles
        .iter()
        .enumerate()
        .fold(Menu::at(at), |menu, (index, title)| {
            menu.chooses(title.clone(), index == chosen, move |this, _, cx| {
                this.choose_a_service(index, cx);
            })
        })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::TestAppContext;
    use resonate_core::SourceId;
    use resonate_providers::{Error, ProviderOp, Result};

    use super::*;
    use crate::{
        Online, Pane, ResonateApp,
        driven::Driven,
        served::{ServedAlbum, ServedArtist, ServedCover, ServedFound, Serves},
        serving::{Failure, rows_of_found},
        views::remote,
    };

    const SCROLLED_PAST_EVERY_CATEGORY: f32 = 2_000.0;

    fn at(key: &str) -> MediaLocation {
        MediaLocation::from_uri(&format!("subsonic:{key}")).expect("a subsonic location")
    }

    fn subsonic() -> SourceId {
        SourceId::new("subsonic").expect("a source")
    }

    fn song(id: &str, title: &str, track: u32) -> ServedSong {
        ServedSong {
            rating: resonate_core::titles::Rating::Unmarked,
            location: at(&format!("track/{id}")),
            title: title.to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            track: Some(track),
            length: Some(Duration::from_secs(357)),
            format: Some("flac".to_owned()),
            cover: None,
            favourite: None,
        }
    }

    struct Held;

    impl Serves for Held {
        fn scheme(&self) -> &SourceId {
            static SUBSONIC: std::sync::LazyLock<SourceId> = std::sync::LazyLock::new(subsonic);
            &SUBSONIC
        }

        fn title(&self) -> &str {
            "Subsonic"
        }

        fn names(&self, location: &MediaLocation) -> bool {
            location.to_uri().starts_with("subsonic:")
        }

        fn artists(&self) -> Result<Vec<ServedArtist>> {
            Ok(vec![ServedArtist {
                location: at("artist/ar-1"),
                name: "Pink Floyd".to_owned(),
                albums: Some(1),
                cover: None,
            }])
        }

        fn albums_of(&self, _: &MediaLocation) -> Result<Vec<ServedAlbum>> {
            Ok(vec![ServedAlbum {
                location: at("album/al-1"),
                name: "Meddle".to_owned(),
                artist: Some("Pink Floyd".to_owned()),
                year: Some(1971),
                songs: Some(2),
                cover: Some(ServedCover("al-1".to_owned())),
            }])
        }

        fn songs_of(&self, _: &MediaLocation) -> Result<Vec<ServedSong>> {
            Ok(vec![
                song("so-1", "One of These Days", 1),
                song("so-2", "A Pillow of Winds", 2),
            ])
        }

        fn search(&self, _: &str) -> Result<ServedFound> {
            Ok(ServedFound {
                songs: vec![song("so-2", "A Pillow of Winds", 2)],
                ..ServedFound::default()
            })
        }

        fn expand(&self, named: &MediaLocation) -> Result<Vec<MediaLocation>> {
            Ok(vec![named.clone()])
        }

        fn cover(&self, _: &MediaLocation, _: Option<&ServedCover>, _: u32) -> Result<Vec<u8>> {
            Err(Error::Unreadable {
                provider: resonate_core::SourceId::new("subsonic").expect("a source"),
                op: ProviderOp::Cover,
            })
        }
    }

    struct Searched;

    impl Serves for Searched {
        fn scheme(&self) -> &SourceId {
            static TIDAL: std::sync::LazyLock<SourceId> =
                std::sync::LazyLock::new(|| SourceId::new("tidal").expect("a source"));
            &TIDAL
        }

        fn title(&self) -> &str {
            "TIDAL"
        }

        fn names(&self, location: &MediaLocation) -> bool {
            location.source() == self.scheme()
        }

        fn artists(&self) -> Result<Vec<ServedArtist>> {
            Ok(Vec::new())
        }

        fn albums_of(&self, _: &MediaLocation) -> Result<Vec<ServedAlbum>> {
            Ok(Vec::new())
        }

        fn songs_of(&self, _: &MediaLocation) -> Result<Vec<ServedSong>> {
            Ok(Vec::new())
        }

        fn search(&self, _: &str) -> Result<ServedFound> {
            Ok(ServedFound {
                songs: vec![ServedSong {
                    location: MediaLocation::from_uri("tidal:track/11").expect("a location"),
                    ..song("so-1", "One of These Days", 1)
                }],
                ..ServedFound::default()
            })
        }

        fn expand(&self, named: &MediaLocation) -> Result<Vec<MediaLocation>> {
            Ok(vec![named.clone()])
        }

        fn cover(&self, _: &MediaLocation, _: Option<&ServedCover>, _: u32) -> Result<Vec<u8>> {
            Err(Error::NotOffered {
                provider: self.scheme().clone(),
                op: ProviderOp::Cover,
            })
        }
    }

    struct Favoured;

    impl Serves for Favoured {
        fn scheme(&self) -> &SourceId {
            Searched.scheme()
        }

        fn title(&self) -> &str {
            "TIDAL"
        }

        fn names(&self, location: &MediaLocation) -> bool {
            location.source() == self.scheme()
        }

        fn artists(&self) -> Result<Vec<ServedArtist>> {
            Ok(Vec::new())
        }

        fn first_page(&self) -> Result<ServedFound> {
            Ok(ServedFound {
                albums: vec![ServedAlbum {
                    location: MediaLocation::from_uri("tidal:album/7").expect("a location"),
                    name: "Meddle".to_owned(),
                    artist: Some("Pink Floyd".to_owned()),
                    year: None,
                    songs: None,
                    cover: None,
                }],
                songs: vec![ServedSong {
                    location: MediaLocation::from_uri("tidal:track/12").expect("a location"),
                    ..song("so-2", "Echoes", 6)
                }],
                ..ServedFound::default()
            })
        }

        fn albums_of(&self, _: &MediaLocation) -> Result<Vec<ServedAlbum>> {
            Ok(Vec::new())
        }

        fn songs_of(&self, _: &MediaLocation) -> Result<Vec<ServedSong>> {
            Ok(Vec::new())
        }

        fn search(&self, _: &str) -> Result<ServedFound> {
            Ok(ServedFound::default())
        }

        fn expand(&self, named: &MediaLocation) -> Result<Vec<MediaLocation>> {
            Ok(vec![named.clone()])
        }

        fn cover(&self, _: &MediaLocation, _: Option<&ServedCover>, _: u32) -> Result<Vec<u8>> {
            Searched.cover(&at("track/so-1"), None, 0)
        }
    }

    struct Kept {
        favoured: Arc<parking_lot::Mutex<Vec<(MediaLocation, bool)>>>,
    }

    impl Serves for Kept {
        fn scheme(&self) -> &SourceId {
            Searched.scheme()
        }

        fn title(&self) -> &str {
            "TIDAL"
        }

        fn names(&self, location: &MediaLocation) -> bool {
            location.source() == self.scheme()
        }

        fn artists(&self) -> Result<Vec<ServedArtist>> {
            Ok(Vec::new())
        }

        fn first_page(&self) -> Result<ServedFound> {
            Ok(ServedFound {
                mixes: vec![ServedAlbum {
                    location: MediaLocation::from_uri("tidal:mix/0a1b").expect("a location"),
                    name: "My Mix 1".to_owned(),
                    artist: Some("Pink Floyd and more".to_owned()),
                    year: None,
                    songs: None,
                    cover: None,
                }],
                songs: vec![
                    ServedSong {
                        location: MediaLocation::from_uri("tidal:track/11").expect("a location"),
                        favourite: Some(true),
                        ..song("so-1", "One of These Days", 1)
                    },
                    ServedSong {
                        location: MediaLocation::from_uri("tidal:track/12").expect("a location"),
                        ..song("so-2", "Echoes", 6)
                    },
                ],
                ..ServedFound::default()
            })
        }

        fn albums_of(&self, _: &MediaLocation) -> Result<Vec<ServedAlbum>> {
            Ok(Vec::new())
        }

        fn songs_of(&self, location: &MediaLocation) -> Result<Vec<ServedSong>> {
            assert_eq!(location.to_uri(), "tidal:mix/0a1b");
            Ok(vec![ServedSong {
                location: MediaLocation::from_uri("tidal:track/13").expect("a location"),
                ..song("so-3", "Summer '68", 1)
            }])
        }

        fn search(&self, _: &str) -> Result<ServedFound> {
            Ok(ServedFound::default())
        }

        fn expand(&self, named: &MediaLocation) -> Result<Vec<MediaLocation>> {
            Ok(vec![named.clone()])
        }

        fn keeps_favourites(&self) -> bool {
            true
        }

        fn favour(&self, song: &MediaLocation, favoured: bool) -> Result<()> {
            self.favoured.lock().push((song.clone(), favoured));
            Ok(())
        }

        fn cover(&self, _: &MediaLocation, _: Option<&ServedCover>, _: u32) -> Result<Vec<u8>> {
            Searched.cover(&at("track/so-1"), None, 0)
        }
    }

    fn an_account() -> Online {
        Online {
            enabled: true,
            subsonic: "http://server.test".to_owned(),
            subsonic_user: "listener".to_owned(),
            subsonic_password: "sesame".to_owned(),
            ..Online::default()
        }
    }

    #[test]
    fn songs_pressed_on_an_album_queue_every_song_of_it_from_the_one_pressed() {
        let rows = vec![
            Row::Song(song("so-1", "One", 1)),
            Row::Song(song("so-2", "Two", 2)),
            Row::Song(song("so-3", "Three", 3)),
        ];

        let (locations, start) = songs_from(&rows, 1).expect("a song is pressed");

        assert_eq!(locations.len(), 3);
        assert_eq!(start, 1);
        assert_eq!(locations[start], at("track/so-2"));
        assert_eq!(songs_from(&[Row::Heading("Songs")], 0), None);
    }

    #[test]
    fn a_search_is_laid_out_under_a_heading_for_each_kind_it_found_and_none_for_the_rest() {
        let found = ServedFound {
            songs: vec![song("so-1", "One", 1)],
            ..ServedFound::default()
        };

        let rows = rows_of_found(found);

        assert!(matches!(rows[0], Row::Heading("Songs")));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn a_server_that_turns_the_account_away_is_told_apart_from_one_that_cannot_be_reached() {
        let source = resonate_core::SourceId::new("subsonic").expect("a source");
        let unwelcome = Error::Unwelcome {
            provider: source.clone(),
            op: ProviderOp::Browse,
            code: 40,
        };
        let unreadable = Error::Unreadable {
            provider: source.clone(),
            op: ProviderOp::Browse,
        };
        let unreached = Error::Io {
            provider: source,
            op: ProviderOp::Browse,
            source: std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        };

        assert_eq!(Failure::of(&unwelcome), Failure::Unwelcome);
        assert_eq!(Failure::of(&unreached), Failure::Unreached);
        assert_eq!(Failure::of(&unreadable), Failure::Unreadable);
    }

    fn open_the_server(driven: &mut Driven) {
        driven.click("tab-server");
    }

    #[gpui::test]
    fn the_server_is_a_pane_only_while_an_account_is_given_and_online_is_on(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], Online::default());

        assert!(!driven.read(|root, cx| root.server.read(cx).is_open()));

        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        assert!(driven.read(|root, cx| root.server.read(cx).is_open()));

        open_the_server(&mut driven);
        assert_eq!(driven.read(|root, _| root.pane), Pane::Server);
    }

    #[gpui::test]
    fn the_services_row_stands_last_in_the_collection_only_while_a_service_is_set(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], Online::default());
        assert!(driven.cx.debug_bounds("tab-server").is_none());

        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        let row = driven.bounds_of("tab-server");

        assert!(driven.bounds_of("tab-party").bottom() <= row.top());
        assert!(row.bottom() <= driven.bounds_of("tab-lyrics").top());
    }

    #[gpui::test]
    fn a_service_let_go_takes_its_row_with_it_and_leaves_the_pane_for_the_first_tab(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        open_the_server(&mut driven);

        let online_set_to = |driven: &mut Driven, enabled| {
            driven.cx.update(|_, cx| {
                cx.update_global::<ResonateApp, _>(|app, _| app.online.enabled = enabled);
            });
            driven.settle();
        };

        online_set_to(&mut driven, false);
        assert!(driven.cx.debug_bounds("tab-server").is_none());
        assert_eq!(driven.read(|root, _| root.pane), Pane::default());

        online_set_to(&mut driven, true);
        assert!(driven.cx.debug_bounds("tab-server").is_some());
        assert_eq!(driven.read(|root, _| root.pane), Pane::default());
    }

    #[gpui::test]
    fn the_pane_keys_stop_at_the_services_row_and_read_its_first_page_only_then(
        cx: &mut TestAppContext,
    ) {
        let service = Arc::new(NotPlayed {
            keeps: true,
            ..NotPlayed::default()
        });
        let read = Arc::clone(&service.first_pages_read);
        let times_read = || read.load(std::sync::atomic::Ordering::SeqCst);
        let mut driven = Driven::serving(cx, vec![service], an_account());
        driven.click("tab-party");
        assert_eq!(times_read(), 0, "the account was touched before the row");

        driven.cx.simulate_keystrokes("ctrl-tab");
        driven.until(|root, cx| matches!(root.server.read(cx).page(), Page::Read(_)));
        assert_eq!(driven.read(|root, _| root.pane), Pane::Server);
        assert_eq!(times_read(), 1);

        driven.cx.simulate_keystrokes("ctrl-tab");
        driven.settle();
        assert_eq!(driven.read(|root, _| root.pane), Pane::Lyrics);
    }

    #[gpui::test]
    fn with_no_service_set_the_pane_keys_pass_from_the_collection_to_what_is_playing(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], Online::default());
        driven.click("tab-party");

        driven.cx.simulate_keystrokes("ctrl-tab");
        driven.settle();

        assert_eq!(driven.read(|root, _| root.pane), Pane::Lyrics);
    }

    #[gpui::test]
    fn the_smallest_window_scrolls_the_categories_to_the_services_row_and_keeps_settings_standing(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        let shortest = px(theme::WINDOW_MIN_HEIGHT);
        driven
            .cx
            .simulate_resize(gpui::size(px(theme::WINDOW_MIN_WIDTH), shortest));
        driven.settle();
        assert!(driven.read(|root, _| root.scrolls.sidebar.max_offset().y) > px(0.0));

        let categories = driven.centre_of("tab-albums");
        driven.scroll_at(categories, SCROLLED_PAST_EVERY_CATEGORY);
        driven.click("tab-server");

        assert_eq!(driven.read(|root, _| root.pane), Pane::Server);
        assert!(driven.bounds_of("tab-settings").bottom() <= shortest);
    }

    #[gpui::test]
    fn other_devices_make_way_for_the_services_row_as_for_every_category(cx: &mut TestAppContext) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        driven.root.update(&mut driven.cx, |root, cx| {
            root.remotely.songs = vec![remote::tests::song()];
            cx.notify();
        });
        driven.settle();
        let room_with_the_cards = driven.read(|root, _| root.scrolls.sidebar.bounds().size.height);
        driven.click("toggle-other-devices");
        let room = driven.read(|root, _| root.scrolls.sidebar.bounds().size.height);
        driven.click("toggle-other-devices");
        let categories = driven.read(|root, _| {
            root.scrolls
                .sidebar
                .bounds_for_item(0)
                .expect("the categories")
                .size
                .height
        });
        let spare = f32::from(room_with_the_cards - categories);
        let cards = f32::from(room - room_with_the_cards);
        let too_short_for_the_cards = crate::driven::TALL - spare - cards / 2.0;

        driven.cx.simulate_resize(gpui::size(
            px(crate::driven::WIDE),
            px(too_short_for_the_cards),
        ));
        driven.settle();

        assert!(driven.read(|root, _| root.remotely.collapsed));
        assert_eq!(
            driven.read(|root, _| root.scrolls.sidebar.max_offset().y),
            px(0.0)
        );
        driven.click("tab-server");
        assert_eq!(driven.read(|root, _| root.pane), Pane::Server);
    }

    #[gpui::test]
    fn an_artist_opens_to_its_albums_and_an_album_to_its_songs_and_a_song_plays_the_album_from_it(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        open_the_server(&mut driven);
        driven.until(|root, cx| matches!(root.server.read(cx).page(), Page::Read(_)));

        driven.click("server-row-0");
        driven.until(|root, cx| {
            matches!(root.server.read(cx).place(), Place::Artist { .. })
                && matches!(root.server.read(cx).page(), Page::Read(_))
        });
        driven.click("server-row-0");
        driven.until(|root, cx| {
            matches!(root.server.read(cx).place(), Place::Album { .. })
                && matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 2)
        });
        driven.click("server-row-1");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        let queued = driven.read(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .iter()
                .map(|item| item.location.to_uri())
                .collect::<Vec<_>>()
        });
        assert_eq!(queued, ["subsonic:track/so-1", "subsonic:track/so-2"]);
    }

    #[gpui::test]
    fn a_service_with_favourites_opens_on_them_and_a_favourite_song_plays_from_the_first_page(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Favoured)], an_account());
        open_the_server(&mut driven);
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 4),
        );

        let first = driven.read(|root, cx| match root.server.read(cx).page() {
            Page::Read(rows) => (favourites_in(rows), counted_in(rows, &Place::Artists)),
            Page::Reading | Page::Failed(_) => (false, String::new()),
        });
        driven.click("server-row-3");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        let queued = driven.read(|root, cx| root.player.read(cx).queue()[0].location.to_uri());
        assert_eq!(
            first,
            (true, "0 artists · 1 album · 0 mixes · 1 song".to_owned())
        );
        assert_eq!(queued, "tidal:track/12");
    }

    #[gpui::test]
    fn a_mix_on_the_first_page_opens_to_its_songs_and_a_song_of_it_plays(cx: &mut TestAppContext) {
        let kept = Kept {
            favoured: Arc::default(),
        };
        let mut driven = Driven::serving(cx, vec![Arc::new(kept)], an_account());
        open_the_server(&mut driven);
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 5),
        );

        driven.click("server-row-1");
        driven.until(|root, cx| {
            matches!(root.server.read(cx).place(), Place::Mix { .. })
                && matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 1)
        });
        driven.click("server-row-0");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        let queued = driven.read(|root, cx| root.player.read(cx).queue()[0].location.to_uri());
        assert_eq!(queued, "tidal:track/13");
    }

    #[gpui::test]
    fn a_star_on_a_served_song_favours_it_in_the_service_and_a_favourite_one_is_taken_out(
        cx: &mut TestAppContext,
    ) {
        let favoured = Arc::default();
        let kept = Kept {
            favoured: Arc::clone(&favoured),
        };
        let mut driven = Driven::serving(cx, vec![Arc::new(kept)], an_account());
        open_the_server(&mut driven);
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 5),
        );

        driven.click("server-row-favourite-4");
        driven.until(|_, _| favoured.lock().len() == 1);
        driven.click("server-row-favourite-3");
        driven.until(|_, _| favoured.lock().len() == 2);

        let stars = driven.read(|root, cx| {
            let server = root.server.read(cx);
            match server.page() {
                Page::Read(rows) => rows
                    .iter()
                    .filter_map(Row::song)
                    .map(|song| server.is_favoured(song))
                    .collect::<Vec<_>>(),
                Page::Reading | Page::Failed(_) => Vec::new(),
            }
        });
        assert_eq!(
            favoured
                .lock()
                .iter()
                .map(|(location, favoured)| (location.to_uri(), *favoured))
                .collect::<Vec<_>>(),
            [
                ("tidal:track/12".to_owned(), true),
                ("tidal:track/11".to_owned(), false)
            ]
        );
        assert_eq!(stars, [false, true]);
    }

    #[derive(Default)]
    struct NotPlayed {
        keeps: bool,
        first_pages_read: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Serves for NotPlayed {
        fn scheme(&self) -> &SourceId {
            Searched.scheme()
        }

        fn title(&self) -> &str {
            "YouTube Music"
        }

        fn names(&self, location: &MediaLocation) -> bool {
            location.source() == self.scheme()
        }

        fn artists(&self) -> Result<Vec<ServedArtist>> {
            Ok(Vec::new())
        }

        fn first_page(&self) -> Result<ServedFound> {
            self.first_pages_read
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ServedFound {
                mixes: vec![ServedAlbum {
                    location: MediaLocation::from_uri("tidal:mix/LM").expect("a location"),
                    name: "Liked songs".to_owned(),
                    artist: None,
                    year: None,
                    songs: None,
                    cover: None,
                }],
                ..ServedFound::default()
            })
        }

        fn albums_of(&self, _: &MediaLocation) -> Result<Vec<ServedAlbum>> {
            Ok(Vec::new())
        }

        fn songs_of(&self, _: &MediaLocation) -> Result<Vec<ServedSong>> {
            Ok(vec![ServedSong {
                location: MediaLocation::from_uri("tidal:track/13").expect("a location"),
                ..song("so-3", "Summer '68", 1)
            }])
        }

        fn search(&self, _: &str) -> Result<ServedFound> {
            Ok(ServedFound::default())
        }

        fn expand(&self, named: &MediaLocation) -> Result<Vec<MediaLocation>> {
            Ok(vec![named.clone()])
        }

        fn plays_songs(&self) -> bool {
            false
        }

        fn keeps_songs(&self) -> bool {
            self.keeps
        }

        fn cover(&self, _: &MediaLocation, _: Option<&ServedCover>, _: u32) -> Result<Vec<u8>> {
            Searched.cover(&at("track/so-1"), None, 0)
        }
    }

    fn the_mix_opened(driven: &mut Driven) {
        open_the_server(driven);
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 2),
        );
        driven.settle();
        driven.click("server-row-1");
        driven.until(|root, cx| {
            matches!(root.server.read(cx).place(), Place::Mix { .. })
                && matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 1)
        });
        driven.settle();
    }

    #[gpui::test]
    fn a_service_that_keeps_songs_is_asked_for_its_first_page_only_once_the_pane_is_shown(
        cx: &mut TestAppContext,
    ) {
        let service = Arc::new(NotPlayed {
            keeps: true,
            ..NotPlayed::default()
        });
        let read = Arc::clone(&service.first_pages_read);
        let times_read = || read.load(std::sync::atomic::Ordering::SeqCst);
        let mut driven = Driven::serving(cx, vec![service], an_account());
        driven.settle();
        assert_eq!(times_read(), 0, "the account was touched at launch");

        open_the_server(&mut driven);
        driven.until(|root, cx| matches!(root.server.read(cx).page(), Page::Read(_)));
        assert_eq!(times_read(), 1);

        driven.click("tab-albums");
        driven.settle();
        driven.click("tab-server");
        driven.settle();
        assert_eq!(driven.read(|root, _| root.pane), Pane::Server);
        assert_eq!(times_read(), 1, "a page read is not read again");

        driven.click("tab-server");
        driven.settle();
        assert_eq!(times_read(), 1, "a press on the row in front reads nothing");
    }

    #[gpui::test]
    fn a_service_that_keeps_no_songs_reads_its_first_page_at_once(cx: &mut TestAppContext) {
        let service = Arc::new(NotPlayed::default());
        let read = Arc::clone(&service.first_pages_read);
        let mut driven = Driven::serving(cx, vec![service], an_account());

        driven.until(|root, cx| matches!(root.server.read(cx).page(), Page::Read(_)));

        assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[gpui::test]
    fn a_service_that_keeps_songs_offers_a_keep_on_a_song_and_a_mix_and_a_press_queues_them(
        cx: &mut TestAppContext,
    ) {
        let folder = crate::driven::Folder::new();
        let mut driven = Driven::serving(
            cx,
            vec![Arc::new(NotPlayed {
                keeps: true,
                ..NotPlayed::default()
            })],
            an_account(),
        );
        let music = folder.path().to_path_buf();
        driven.cx.update(|_, cx| {
            gpui::BorrowAppContext::update_global::<crate::ResonateApp, _>(cx, |global, _| {
                global.music_folder = Some(music);
            });
        });
        the_mix_opened(&mut driven);

        assert!(driven.cx.debug_bounds("server-row-keep-0").is_some());
        driven.click("server-keep");
        let library = driven.read(|_, cx| Arc::clone(&cx.global::<crate::ResonateApp>().library));
        driven.until(|_, _| library.keeps_pending().is_ok_and(|queued| queued == 1));
        driven.until(|root, cx| root.library.read(cx).keeps_left().is_none());

        driven.click("server-row-keep-0");
        driven.settle();
        driven.until(|root, cx| root.library.read(cx).keeps_left().is_none());
        assert_eq!(library.keeps_pending().expect("a count"), 2);
    }

    #[gpui::test]
    fn a_mix_of_a_service_that_plays_no_song_opens_and_offers_nothing_to_play_or_queue(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(NotPlayed::default())], an_account());
        open_the_server(&mut driven);
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 2),
        );
        driven.settle();
        assert!(driven.cx.debug_bounds("server-row-1").is_some());
        assert!(driven.cx.debug_bounds("server-row-play-1").is_none());

        driven.click("server-row-1");
        driven.until(|root, cx| {
            matches!(root.server.read(cx).place(), Place::Mix { .. })
                && matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 1)
        });
        driven.settle();

        assert!(driven.cx.debug_bounds("server-row-0").is_some());
        for offered in [
            "server-play",
            "server-queue",
            "server-row-play-0",
            "server-row-next-0",
            "server-row-queue-0",
            "server-keep",
            "server-row-keep-0",
        ] {
            assert!(driven.cx.debug_bounds(offered).is_none(), "{offered}");
        }

        driven.click("server-row-0");
        driven.settle();
        assert!(driven.read(|root, cx| root.player.read(cx).queue().is_empty()));
    }

    #[gpui::test]
    fn two_services_are_chosen_between_in_the_pane_and_a_song_found_on_the_second_plays_from_it(
        cx: &mut TestAppContext,
    ) {
        let mut driven =
            Driven::serving(cx, vec![Arc::new(Held), Arc::new(Searched)], an_account());
        open_the_server(&mut driven);
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 1),
        );

        driven.click("served-service-1");
        driven.until(|root, cx| {
            root.server.read(cx).chosen() == 1
                && matches!(root.server.read(cx).page(), Page::Read(rows) if rows.is_empty())
        });
        driven.root.update(&mut driven.cx, |root, cx| {
            root.server.update(cx, |server, cx| {
                server.open(
                    Place::Found {
                        words: "days".to_owned(),
                    },
                    cx,
                )
            });
        });
        driven.until(
            |root, cx| matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 2),
        );
        driven.click("server-row-1");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        let queued = driven.read(|root, cx| root.player.read(cx).queue()[0].location.to_uri());
        let named = driven.read(|root, cx| {
            let server = root.server.read(cx);
            (
                server.names(&at("track/so-1")),
                server.names(&MediaLocation::from_uri("tidal:track/11").expect("a location")),
                server.titles(),
            )
        });
        assert_eq!(queued, "tidal:track/11");
        assert_eq!(
            named,
            (true, true, vec!["Subsonic".to_owned(), "TIDAL".to_owned()])
        );
    }

    #[gpui::test]
    fn more_services_than_fit_a_bar_are_chosen_from_a_menu(cx: &mut TestAppContext) {
        let mut driven = Driven::serving(
            cx,
            vec![
                Arc::new(Held),
                Arc::new(Searched),
                Arc::new(Favoured),
                Arc::new(NotPlayed::default()),
            ],
            an_account(),
        );
        open_the_server(&mut driven);
        driven.until(|root, cx| root.server.read(cx).titles().len() == 4);

        driven.click("served-service");
        driven.click("menu-entry-1");
        driven.until(|root, cx| root.server.read(cx).chosen() == 1);

        assert!(driven.read(|root, _| root.menu.is_none()));
    }

    #[gpui::test]
    fn words_entered_in_the_services_search_are_asked_of_the_service_and_escape_clears_them(
        cx: &mut TestAppContext,
    ) {
        let mut driven =
            Driven::serving(cx, vec![Arc::new(Held), Arc::new(Searched)], an_account());
        open_the_server(&mut driven);
        driven.click("served-service-1");
        driven.until(|root, cx| root.server.read(cx).chosen() == 1);

        driven.click(SEARCH_ID);
        driven.cx.simulate_input("days");
        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| {
            matches!(root.server.read(cx).place(), Place::Found { words } if words == "days")
                && matches!(root.server.read(cx).page(), Page::Read(rows) if rows.len() == 2)
        });
        let left_the_field = driven.read(|root, cx| {
            let window_holds_it = root.focus.clone();
            (
                window_holds_it,
                root.services.read(cx).search.read(cx).text().to_owned(),
            )
        });
        let window_focused = driven
            .cx
            .update(|window, _| left_the_field.0.is_focused(window));
        driven.click(SEARCH_ID);
        driven.cx.simulate_input(" again");
        driven.cx.simulate_keystrokes("escape");
        driven.settle();
        let cleared =
            driven.read(|root, cx| root.services.read(cx).search.read(cx).text().to_owned());

        assert_eq!(left_the_field.1, "days");
        assert!(window_focused);
        assert_eq!(cleared, "");
    }
}
