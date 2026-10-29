use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, Context, Div, SharedString, Stateful, Window, div, prelude::*, px, rgb,
    uniform_list,
};
use resonate_core::MediaLocation;
use resonate_engine::{Command, Placement, QueueItem, Unclaimed};

use crate::{
    Drawn, Notice, format,
    icons::{self, Icon},
    served::ServedSong,
    serving::{Failure, Page, Place, Row, favourites_in},
    theme,
    views::{
        browser::controls_width,
        hint::Names,
        kit::{self, EndsInAnEllipsis as _, Tone},
        listing,
        root::{RootView, framed_cover, row},
        scrollbar::Scrollbars,
        spoken::{self, Spoken as _},
    },
};

const BACK_HINT: &str = "Back to where this was opened from";

const PLAY_HINT: &str = "Play everything here, in order";

const NEXT_HINT: &str = "Play this next";

const QUEUE_HINT: &str = "Add this to the end of the queue";

const OPEN_HINT: &str = "See what the server holds under this";

const PLAY_THIS_HINT: &str = "Play this, and what follows it here";

const ASK_AGAIN_HINT: &str = "Ask the server again";

pub(crate) const SEARCH_PLACEHOLDER: &str = "Search the server, then press enter";

const NOTHING_HERE: &str = "The server holds nothing here.";

const NOTHING_LISTED: &str = "This service lists nothing until it is asked.";

const FAVOURITES: &str = "Favourites";

const SEARCH_FIRST: &str = "Search it above for an artist, an album or a song.";

const CHOOSE_HINT: &str = "Browse this service";

const NOTHING_FOUND: &str = "The server found nothing for that.";

const ASKING: &str = "Asking the server…";

const SERVER_CONTROLS: usize = 3;

const SEARCH_ID: &str = "served-search";

impl RootView {
    pub(crate) fn server_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.server.read(cx);
        let place = model.place().clone();
        let page = model.page().clone();
        let way_back = model.way_back_name();

        let heading = self.server_heading(&place, &page, way_back, cx);
        let body = match page {
            Page::Reading => kit::empty(Icon::Server, ASKING, None),
            Page::Failed(failure) => self.server_failed(failure, cx),
            Page::Read(rows) if rows.is_empty() => match place {
                Place::Found { .. } => kit::empty(Icon::Server, NOTHING_FOUND, None),
                Place::Artists => kit::empty(Icon::Server, NOTHING_LISTED, Some(SEARCH_FIRST)),
                Place::Artist { .. } | Place::Album { .. } => {
                    kit::empty(Icon::Server, NOTHING_HERE, None)
                }
            },
            Page::Read(rows) => self.server_rows_listed(rows, place, cx),
        };

        kit::pane().child(heading).child(body).into_any_element()
    }

    fn server_failed(&self, failure: Failure, cx: &mut Context<Self>) -> AnyElement {
        kit::empty_offering(
            Icon::Server,
            failure.says(),
            Some(failure.advises()),
            self.in_the_pane_ring(
                kit::button(
                    "server-ask-again",
                    None,
                    "Try again",
                    ASK_AGAIN_HINT,
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
            (Place::Artists, Page::Read(rows)) if favourites_in(rows) => FAVOURITES.to_owned(),
            _ => place.title(),
        };
        let playable = place.location().cloned();

        let mut actions = kit::actions();
        if let Some(whole) = playable {
            let playing = whole.clone();
            let queueing = whole;
            actions = actions
                .child(self.in_the_pane_ring(
                    kit::button(
                        "server-play",
                        Some(Icon::Play),
                        "Play",
                        PLAY_HINT,
                        Tone::Primary,
                    ),
                    move |this, _, cx| this.play_what_the_server_holds(playing.clone(), cx),
                    cx,
                ))
                .child(self.in_the_pane_ring(
                    kit::icon_button("server-queue", Icon::QueueLast, QUEUE_HINT),
                    move |this, _, cx| {
                        this.queue_what_the_server_holds(queueing.clone(), Placement::Queued, cx);
                    },
                    cx,
                ));
        }
        actions = actions.child(self.server_search_field(cx));

        let model = self.server.read(cx);
        let titles = model.titles();
        let chosen = model.chosen();
        let eyebrow = match place {
            Place::Artists => titles
                .get(chosen)
                .map_or_else(|| place.eyebrow().to_owned(), |title| title.to_uppercase()),
            Place::Artist { .. } | Place::Album { .. } | Place::Found { .. } => {
                place.eyebrow().to_owned()
            }
        };
        let chooser = (titles.len() > 1).then(|| self.service_chooser(&titles, chosen, cx));

        let heading = kit::heading()
            .when_some(chooser, |heading, chooser| heading.child(chooser))
            .when_some(way_back, |heading, name| {
                heading.child(self.in_the_pane_ring(
                    kit::way_back("server-back", name, BACK_HINT),
                    |this, _, cx| {
                        this.server.update(cx, |server, cx| {
                            server.back(cx);
                        });
                        this.server_search.update(cx, |field, cx| field.clear(cx));
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
                        .child(kit::eyebrow(eyebrow))
                        .child(kit::title(SharedString::from(title)))
                        .when_some(counted, |column, counted| {
                            column.child(kit::subtitle(SharedString::from(counted)))
                        }),
                )
                .child(actions),
        )
    }

    fn service_chooser(&self, titles: &[String], chosen: usize, cx: &mut Context<Self>) -> Div {
        titles
            .iter()
            .enumerate()
            .fold(kit::segmented(), |chooser, (index, title)| {
                chooser.child(
                    self.in_the_pane_ring(
                        kit::segment(("served-service", index), title.clone(), index == chosen)
                            .names(CHOOSE_HINT),
                        move |this, _, cx| {
                            this.server
                                .update(cx, |server, cx| server.choose(index, cx));
                            this.server_search.update(cx, |field, cx| field.clear(cx));
                        },
                        cx,
                    ),
                )
            })
    }

    fn server_search_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(SEARCH_ID)
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .w(theme::width(theme::picker_width()))
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.border_color(theme::tinted(theme::accent(), 0x99)))
            .child(icons::icon(
                Icon::Search,
                theme::search_icon(),
                theme::faint(),
            ))
            .child(self.server_search.clone())
            .on_click(cx.listener(|this, _, window, cx| {
                this.server_search
                    .update(cx, |field, cx| field.take_focus(window, cx));
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
        let songs_here = matches!(place, Place::Album { .. });

        Scrollbars::of(cx)
            .around(
                "server-scrollbar",
                self.server_rows.clone(),
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
                .track_scroll(&self.server_rows)
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
            .cursor_pointer()
            .listed_as(move || said.clone())
            .names(match row {
                Row::Song(_) => PLAY_THIS_HINT,
                Row::Heading(_) | Row::Artist(_) | Row::Album(_) => OPEN_HINT,
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
                        div()
                            .truncate()
                            .ends_in_an_ellipsis()
                            .text_color(rgb(theme::text()))
                            .child(SharedString::from(title)),
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
                this.press_a_row_of_the_server(pressed.clone(), from.clone(), cx);
            }));

        row_frame()
            .child(open)
            .child(self.server_controls(index, location, cx))
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
        cx: &mut Context<Self>,
    ) -> Div {
        let playing = location.clone();
        let next = location.clone();
        let queued = location;
        let controls = self.row_controls(("server-controls", index), cx);

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .w(px(controls_width(SERVER_CONTROLS)))
            .child(
                controls
                    .child(self.in_the_pane_ring(
                        kit::icon_button(("server-row-play", index), Icon::Play, PLAY_HINT),
                        move |this, _, cx| this.play_what_the_server_holds(playing.clone(), cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(("server-row-next", index), Icon::QueueNext, NEXT_HINT),
                        move |this, _, cx| {
                            this.queue_what_the_server_holds(next.clone(), Placement::Next, cx);
                        },
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(("server-row-queue", index), Icon::QueueLast, QUEUE_HINT),
                        move |this, _, cx| {
                            this.queue_what_the_server_holds(queued.clone(), Placement::Queued, cx);
                        },
                        cx,
                    )),
            )
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

    pub(crate) fn search_the_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let words = self.server_search.read(cx).text().trim().to_owned();
        self.server.update(cx, |server, cx| {
            if words.is_empty() {
                server.leave_the_search(cx);
            } else {
                server.open(Place::Found { words }, cx);
            }
        });
        window.focus(&self.focus, cx);
        cx.notify();
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
            Row::Song(song) => match from {
                Some((locations, start)) => self.play_served(locations, start, cx),
                None => self.play_served(vec![song.location], 0, cx),
            },
            Row::Heading(_) => {}
        }
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
                        Notice::Trouble(format!(
                            "Couldn't read that — {}",
                            Failure::of(&error).says().to_lowercase()
                        )),
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
            artist
                .albums
                .map(|albums| format::counted(albums as usize, "album", "albums")),
        ),
        Row::Album(album) => (
            album.name.clone(),
            album.artist.clone(),
            Some(
                [
                    album.year.map(|year| year.to_string()),
                    album
                        .songs
                        .map(|songs| format::counted(songs as usize, "song", "songs")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
            )
            .filter(|said| !said.is_empty()),
        ),
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
        Place::Artists if !favourites_in(rows) => format::counted(
            count(|row| matches!(row, Row::Artist(_))),
            "artist",
            "artists",
        ),
        Place::Artist { .. } => {
            format::counted(count(|row| matches!(row, Row::Album(_))), "album", "albums")
        }
        Place::Album { .. } => {
            format::counted(count(|row| matches!(row, Row::Song(_))), "song", "songs")
        }
        Place::Artists | Place::Found { .. } => format!(
            "{} · {} · {}",
            format::counted(
                count(|row| matches!(row, Row::Artist(_))),
                "artist",
                "artists"
            ),
            format::counted(count(|row| matches!(row, Row::Album(_))), "album", "albums"),
            format::counted(count(|row| matches!(row, Row::Song(_))), "song", "songs"),
        ),
    }
}

fn queued_words(rows: usize, at: Placement, play: bool) -> String {
    let songs = format::counted(rows, "song", "songs");

    match (play, at) {
        (true, _) => format!("{songs} queued and playing"),
        (false, Placement::Next) => format!("{songs} to play next"),
        (false, Placement::Queued | Placement::At(_)) => format!("{songs} added to the queue"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::TestAppContext;
    use resonate_core::SourceId;
    use resonate_providers::{Error, ProviderOp, Result};

    use super::*;
    use crate::{
        Online, Pane,
        driven::Driven,
        served::{ServedAlbum, ServedArtist, ServedCover, ServedFound, Serves},
        serving::{Failure, rows_of_found},
    };

    fn at(key: &str) -> MediaLocation {
        MediaLocation::from_uri(&format!("subsonic:{key}")).expect("a subsonic location")
    }

    fn subsonic() -> SourceId {
        SourceId::new("subsonic").expect("a source")
    }

    fn song(id: &str, title: &str, track: u32) -> ServedSong {
        ServedSong {
            location: at(&format!("track/{id}")),
            title: title.to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            track: Some(track),
            length: Some(Duration::from_secs(357)),
            format: Some("flac".to_owned()),
            cover: None,
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
        assert_eq!(songs_from(&[Row::Heading("SONGS")], 0), None);
    }

    #[test]
    fn a_search_is_laid_out_under_a_heading_for_each_kind_it_found_and_none_for_the_rest() {
        let found = ServedFound {
            songs: vec![song("so-1", "One", 1)],
            ..ServedFound::default()
        };

        let rows = rows_of_found(found);

        assert!(matches!(rows[0], Row::Heading("SONGS")));
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

    #[gpui::test]
    fn the_server_is_a_pane_only_while_an_account_is_given_and_online_is_on(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], Online::default());

        assert!(!driven.read(|root, cx| root.server.read(cx).is_open()));

        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        assert!(driven.read(|root, cx| root.server.read(cx).is_open()));

        driven.click("tab-server");
        assert_eq!(driven.read(|root, _| root.pane), Pane::Server);
    }

    #[gpui::test]
    fn an_artist_opens_to_its_albums_and_an_album_to_its_songs_and_a_song_plays_the_album_from_it(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::serving(cx, vec![Arc::new(Held)], an_account());
        driven.click("tab-server");
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
        driven.click("tab-server");
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
        assert_eq!(first, (true, "0 artists · 1 album · 1 song".to_owned()));
        assert_eq!(queued, "tidal:track/12");
    }

    #[gpui::test]
    fn two_services_are_chosen_between_in_the_pane_and_a_song_found_on_the_second_plays_from_it(
        cx: &mut TestAppContext,
    ) {
        let mut driven =
            Driven::serving(cx, vec![Arc::new(Held), Arc::new(Searched)], an_account());
        driven.click("tab-server");
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
}
