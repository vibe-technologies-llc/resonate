use std::ops::Range;

use gpui::{
    AnyElement, App, Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px,
    rgb, transparent_black, uniform_list,
};
use resonate_library::{FollowedLink, Linked};

use crate::{
    Beyond, Notice, Selection, format,
    icons::Icon,
    theme, toast,
    views::{
        browser::Plays,
        hint::Names,
        kit::{self, Found as _, Tone},
        listing,
        playlists::{Naming, SAVE_SEARCH_HINT},
        pointed::LitUnderThePointer,
        reorder::{self, Listed, Shift},
        root::{Pane, RootView, empty},
        scrollbar::{SHELF_INSET, Scrollbars},
    },
};

pub(crate) const SONGS_AT_THE_TOP: usize = 5;

pub(crate) const FOUND_AT_THE_TOP: usize = 6;

const STRIP_AT_MOST: usize = 24;

const ARTIST_AT_THE_TOP: f32 = 72.0;

const NOTHING_MATCHES: &str = "Nothing in your library matches.";

const NOTHING_ELSEWHERE: &str = "Nothing found beyond your library.";

const ONLINE_IS_OFF: &str = "Turn Online on in Settings to look for songs on MusicBrainz too.";

const ASKING: &str = "Asking MusicBrainz…";

const ASKING_BESIDE_A_LOOKUP: &str = "Asking MusicBrainz, which answers one request a second and \
                                      is answering the running lookup too…";

const UNREACHED: &str = "MusicBrainz could not be reached";

const AS_TYPED_HINT: &str =
    "The words were read as a title by an artist; search for them just as they were typed";

const ASK_AGAIN_HINT: &str = "Ask MusicBrainz for these words again";

const PRESS_TO_DOWNLOAD: &str = "Press a song to download it";

const LOOKING_THE_SONG_UP: &str = "Looking up the song that link names…";

const LOOKING_THE_ALBUM_UP: &str = "Looking up the album that link names…";

const ONLINE_TO_FOLLOW_A_LINK: &str = "Turn Online on in Settings to download what a link names";

const NO_SONG_AT_THE_LINK: &str = "No service could tell which song that link names";

const NO_ALBUM_AT_THE_LINK: &str = "No service could tell which album that link names";

const FOLLOWING_THE_LINK: &str = "look up what that link names";

struct Told {
    looking: &'static str,
    nothing: &'static str,
}

impl Told {
    const fn of(link: &FollowedLink) -> Self {
        match link {
            FollowedLink::Song(_) => Self {
                looking: LOOKING_THE_SONG_UP,
                nothing: NO_SONG_AT_THE_LINK,
            },
            FollowedLink::Album(_) => Self {
                looking: LOOKING_THE_ALBUM_UP,
                nothing: NO_ALBUM_AT_THE_LINK,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SearchShows {
    #[default]
    Top,
    Songs,
    Albums,
    Artists,
    Elsewhere,
}

impl SearchShows {
    const ALL: [Self; 5] = [
        Self::Top,
        Self::Songs,
        Self::Albums,
        Self::Artists,
        Self::Elsewhere,
    ];

    pub(crate) const fn in_place_of(pane: Pane) -> Option<Self> {
        match pane {
            Pane::Tracks => Some(Self::Songs),
            Pane::Albums => Some(Self::Albums),
            Pane::Artists => Some(Self::Artists),
            _ => None,
        }
    }

    pub(crate) const fn pane(self) -> Option<Pane> {
        match self {
            Self::Songs => Some(Pane::Tracks),
            Self::Albums => Some(Pane::Albums),
            Self::Artists => Some(Pane::Artists),
            Self::Top | Self::Elsewhere => None,
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Top => "search-top",
            Self::Songs => "search-songs",
            Self::Albums => "search-albums",
            Self::Artists => "search-artists",
            Self::Elsewhere => "search-elsewhere",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Top => "Top results",
            Self::Songs => "Songs",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Elsewhere => "Not in your library",
        }
    }

    const fn saying(self) -> &'static str {
        match self {
            Self::Top => "The best of each kind of match, on one page",
            Self::Songs => "Every song in your library that matches",
            Self::Albums => "Every album in your library that matches",
            Self::Artists => "Every artist in your library that matches",
            Self::Elsewhere => {
                "Songs that match but are not in your library, to download with a press"
            }
        }
    }

    const fn see_all(self) -> &'static str {
        match self {
            Self::Top => "",
            Self::Songs => "See every matching song",
            Self::Albums => "See every matching album",
            Self::Artists => "See every matching artist",
            Self::Elsewhere => "See every matching song not in your library",
        }
    }

    const fn ordered_by(self) -> Option<&'static str> {
        match self {
            Self::Songs => Some("order-tracks"),
            Self::Albums => Some("order-albums"),
            Self::Artists => Some("order-artists"),
            Self::Top | Self::Elsewhere => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Matched {
    songs: usize,
    albums: usize,
    artists: usize,
    elsewhere: Option<Beyond>,
    asks_elsewhere: bool,
}

impl Matched {
    fn read(root: &RootView, cx: &App) -> Self {
        let library = root.library.read(cx);

        Self {
            songs: library.tracks_counted() as usize,
            albums: library.albums_counted() as usize,
            artists: library.artists_counted() as usize,
            elsewhere: library.elsewhere(),
            asks_elsewhere: library.can_enrich(),
        }
    }

    const fn held(self) -> usize {
        self.songs + self.albums + self.artists
    }

    fn in_the_library(self) -> String {
        if self.held() == 0 {
            return "Nothing in your library".to_owned();
        }

        [
            (self.songs, "song", "songs"),
            (self.albums, "album", "albums"),
            (self.artists, "artist", "artists"),
        ]
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, one, many)| format::counted(count, one, many))
        .collect::<Vec<_>>()
        .join(" · ")
            + " in your library"
    }

    fn beyond(self) -> Option<String> {
        Some(match self.elsewhere? {
            Beyond::Elsewhere(found) => {
                format!("{} not in it", format::counted(found, "song", "songs"))
            }
            Beyond::Refining(found) => format!(
                "{} not in it so far, asking MusicBrainz for the rest…",
                format::counted(found, "song", "songs")
            ),
            Beyond::Asking => "asking MusicBrainz for more…".to_owned(),
            Beyond::Unreached => "MusicBrainz could not be reached".to_owned(),
        })
    }

    fn counted(self, shows: SearchShows) -> Option<SharedString> {
        let count = match shows {
            SearchShows::Top => return None,
            SearchShows::Songs => self.songs,
            SearchShows::Albums => self.albums,
            SearchShows::Artists => self.artists,
            SearchShows::Elsewhere => match self.elsewhere {
                Some(Beyond::Asking) => return Some(SharedString::new_static("…")),
                Some(Beyond::Refining(found)) => {
                    return Some(SharedString::from(format!("{found}…")));
                }
                Some(Beyond::Elsewhere(found)) => found,
                Some(Beyond::Unreached) | None => 0,
            },
        };

        Some(SharedString::from(count.to_string()))
    }

    fn offers(self, shows: SearchShows) -> bool {
        match shows {
            SearchShows::Elsewhere => self.asks_elsewhere || self.elsewhere.is_some(),
            SearchShows::Top | SearchShows::Songs | SearchShows::Albums | SearchShows::Artists => {
                true
            }
        }
    }
}

impl RootView {
    pub(crate) fn search_in_front(&self, cx: &App) -> Option<SearchShows> {
        let library = self.library.read(cx);
        let browsing = matches!(self.pane, Pane::Albums | Pane::Artists | Pane::Tracks);
        let searching = library.narrowing().is_some();
        let unscoped = library.selection() == Selection::Everything;

        (browsing && searching && unscoped && self.adding_songs_to.is_none())
            .then_some(self.search_shows)
    }

    pub(crate) fn follow_link(
        &mut self,
        pasted: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(link) = FollowedLink::read(pasted) else {
            return;
        };
        let Some((library, reference)) = self.library.read(cx).follows_links() else {
            toast::tell(Notice::Noted(ONLINE_TO_FOLLOW_A_LINK.to_owned()), cx);
            return;
        };
        let told = Told::of(&link);
        toast::tell(Notice::Noted(told.looking.to_owned()), cx);

        self.following_a_link = cx.spawn_in(window, async move |this, cx| {
            let followed = cx
                .background_executor()
                .spawn(async move {
                    match &link {
                        FollowedLink::Song(song) => library.follow_link(reference.as_ref(), song),
                        FollowedLink::Album(album) => {
                            library.follow_album_link(reference.as_ref(), album)
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| this.followed(followed, &told, cx));
        });
    }

    fn followed(
        &mut self,
        followed: resonate_library::Result<Linked>,
        told: &Told,
        cx: &mut Context<Self>,
    ) {
        match followed {
            Ok(
                Linked::Held { title, .. }
                | Linked::HeldAlbum {
                    title, missing: 0, ..
                },
            ) => toast::tell(
                Notice::Done(format!("“{title}” is already in your library")),
                cx,
            ),
            Ok(Linked::Found(found)) => {
                toast::dismiss(cx);
                self.library
                    .update(cx, |library, cx| library.want_found(*found, cx));
            }
            Ok(Linked::HeldAlbum { album, .. }) => {
                toast::dismiss(cx);
                self.library
                    .update(cx, |library, cx| library.want_missing_tracks(album, cx));
            }
            Ok(Linked::Album { group, .. }) => {
                toast::dismiss(cx);
                self.library
                    .update(cx, |library, cx| library.want_album(group, cx));
            }
            Ok(Linked::Unnamed) => {
                toast::tell(Notice::Trouble(told.nothing.to_owned()), cx);
            }
            Err(error) => toast::tell(toast::could_not(FOLLOWING_THE_LINK, &error), cx),
        }
    }

    pub(crate) fn search_pane(&mut self, shows: SearchShows, cx: &mut Context<Self>) -> AnyElement {
        match shows {
            SearchShows::Top => self.top_results(cx),
            SearchShows::Songs => self.tracks(cx),
            SearchShows::Albums => self.albums(cx),
            SearchShows::Artists => self.artists(cx),
            SearchShows::Elsewhere => self.not_in_the_library(cx),
        }
    }

    pub(crate) fn search_heading(&self, cx: &mut Context<Self>) -> Option<Div> {
        let shows = self.search_in_front(cx)?;
        let matched = Matched::read(self, cx);
        let library = self.library.read(cx);
        let query = library.query().to_owned();
        let reads = library.search().reads();
        let naming = self.naming.filter(|naming| naming.is_a_search());

        let meant = library
            .meant()
            .map(|meant| format!("Read as “{}” by {}", meant.title, meant.artist));
        let summary = match matched.beyond() {
            Some(beyond) => format!("{} · {beyond}", matched.in_the_library()),
            None => matched.in_the_library(),
        };
        let as_typed = meant.is_some().then(|| {
            kit::button(
                "search-as-typed",
                None,
                "Search the words as typed",
                AS_TYPED_HINT,
                Tone::Ghost,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.library
                    .update(cx, |library, cx| library.search_as_typed(cx));
            }))
        });
        let sung = self.sung_offer(Tone::Ghost, cx);
        let saves = naming.is_none().then(|| {
            kit::button(
                "save-search",
                Some(Icon::Search),
                "Save this search",
                SAVE_SEARCH_HINT,
                Tone::Ghost,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.name_a_playlist(Naming::Query(None), window, cx);
            }))
        });
        let orders = shows
            .ordered_by()
            .map(|named| self.orders_a_listing(named, cx));
        let plays = (matched.songs > 0).then(|| (self.shuffle_all(cx), self.play_all(cx)));
        let in_order = match (self.ordering, shows) {
            (true, SearchShows::Songs) => Some(self.tracks_in_order(cx)),
            (true, SearchShows::Albums) => Some(self.albums_in_order(cx)),
            (true, SearchShows::Artists) => Some(self.artists_in_order(cx)),
            (true | false, _) => None,
        };
        let named = naming.map(|naming| self.naming_row(naming, cx));
        let tabs = self.search_tabs(shows, matched, cx);

        let actions = kit::actions()
            .children(as_typed)
            .children(sung)
            .children(saves)
            .children(orders)
            .when_some(plays, |bar, (shuffle, play)| bar.child(shuffle).child(play));

        Some(
            kit::heading()
                .pb_0()
                .child(
                    kit::heading_row()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(theme::heading_name()))
                                .gap_1()
                                .child(kit::eyebrow("SEARCH"))
                                .child(kit::title(
                                    div()
                                        .truncate()
                                        .child(SharedString::from(format!("“{query}”"))),
                                ))
                                .when_some(meant, |column, meant| {
                                    column.child(
                                        div()
                                            .text_size(px(theme::text_sm()))
                                            .text_color(rgb(theme::accent()))
                                            .child(meant),
                                    )
                                })
                                .child(kit::subtitle(summary)),
                        )
                        .child(actions),
                )
                .when(!reads.is_empty(), |heading| {
                    heading.child(listing::reads(&reads))
                })
                .children(in_order)
                .children(named)
                .child(tabs),
        )
    }

    fn search_tabs(&self, shows: SearchShows, matched: Matched, cx: &mut Context<Self>) -> Div {
        let mut tabs = div().flex().flex_wrap().items_end().gap_5();
        for tab in SearchShows::ALL
            .into_iter()
            .filter(|tab| matched.offers(*tab))
        {
            tabs = tabs.child(self.search_tab(tab, tab == shows, matched.counted(tab), cx));
        }

        tabs
    }

    fn search_tab(
        &self,
        tab: SearchShows,
        chosen: bool,
        counted: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = gpui::ElementId::from(tab.id());

        div()
            .id(id.clone())
            .found_as(&id)
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .pt_1()
            .pb_2()
            .border_b_2()
            .text_size(px(theme::text_sm()))
            .whitespace_nowrap()
            .cursor_pointer()
            .names(tab.saying())
            .when_else(
                chosen,
                |tab| {
                    tab.border_color(rgb(theme::accent()))
                        .text_color(rgb(theme::text()))
                        .font_weight(FontWeight::SEMIBOLD)
                },
                |tab| {
                    tab.border_color(transparent_black())
                        .text_color(rgb(theme::muted()))
                        .font_weight(FontWeight::MEDIUM)
                        .lit_under_the_pointer(id, |tab| tab.text_color(rgb(theme::text())))
                },
            )
            .child(tab.label())
            .when_some(counted, |label, counted| {
                label.child(
                    div()
                        .px_1p5()
                        .rounded_full()
                        .bg(rgb(if chosen {
                            theme::hover()
                        } else {
                            theme::raised()
                        }))
                        .text_size(px(theme::text_xs()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::muted()))
                        .child(counted),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.show_in_the_search(tab, cx)))
    }

    fn top_results(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let heading = self.search_heading(cx);
        let matched = Matched::read(self, cx);
        let library = self.library.read(cx);
        let tracks = library.listing();
        let albums = library.albums();
        let artists = library.artists();
        let found = library.found();
        let shared_with_a_lookup = library.is_enriching();
        let playing = self.playing_now(cx).track;
        let pane = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .children(heading);

        if tracks.is_empty() && albums.is_empty() && artists.is_empty() && found.is_empty() {
            let nothing = match matched.elsewhere {
                Some(Beyond::Asking | Beyond::Refining(_)) => {
                    kit::empty(Icon::Search, ASKING, Some(NOTHING_MATCHES))
                }
                Some(Beyond::Unreached) => self.unreached(cx),
                Some(Beyond::Elsewhere(_)) | None => {
                    self.nothing_matched(Icon::Search, NOTHING_MATCHES, None, cx)
                }
            };
            return pane.child(nothing).into_any_element();
        }

        let mut sections = div()
            .id("top-results")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .pb_6()
            .overflow_y_scroll()
            .track_scroll(&self.search_scroll);

        if !artists.is_empty() {
            let cells = artists
                .iter()
                .take(STRIP_AT_MOST)
                .map(|artist| {
                    self.artist_cell_at(artist, ARTIST_AT_THE_TOP, cx)
                        .into_any_element()
                })
                .collect();
            sections = sections
                .child(self.section_heading(SearchShows::Artists, matched.artists, None, cx))
                .child(self.strip("search-artist-strip", cells, cx));
        }

        if !tracks.is_empty() {
            let mut rows = div().flex().flex_col();
            for (index, track) in tracks.iter().enumerate().take(SONGS_AT_THE_TOP) {
                let reached = self.reaches(Shift::Listing(Listed::Tracks), index);
                rows = rows.child(reorder::marked(
                    self.track_row(
                        &tracks,
                        index,
                        track,
                        playing == Some(track.id),
                        Plays::AsTheListingIsDrawn { in_an_album: false },
                        cx,
                    ),
                    reached,
                ));
            }
            sections = sections
                .child(self.section_heading(SearchShows::Songs, matched.songs, None, cx))
                .child(rows);
        }

        if let Some(beyond) = matched.elsewhere {
            let said = match beyond {
                Beyond::Elsewhere(_) => PRESS_TO_DOWNLOAD,
                Beyond::Refining(_) | Beyond::Asking if shared_with_a_lookup => {
                    ASKING_BESIDE_A_LOOKUP
                }
                Beyond::Refining(_) | Beyond::Asking => ASKING,
                Beyond::Unreached => UNREACHED,
            };
            let mut rows = div().flex().flex_col();
            for (index, song) in found.iter().enumerate().take(FOUND_AT_THE_TOP) {
                let reached = self.reaches(Shift::Listing(Listed::Found), index);
                rows = rows.child(reorder::marked(self.found_row(index, song, cx), reached));
            }
            let again = (beyond == Beyond::Unreached).then(|| self.ask_again(cx));
            sections = sections
                .child(
                    self.section_heading(SearchShows::Elsewhere, found.len(), Some(said), cx)
                        .children(again),
                )
                .child(rows);
        }

        if !albums.is_empty() {
            let cells = albums
                .iter()
                .take(STRIP_AT_MOST)
                .map(|album| {
                    self.album_cell_at(album, theme::shelf_cover(), cx)
                        .into_any_element()
                })
                .collect();
            sections = sections
                .child(self.section_heading(SearchShows::Albums, matched.albums, None, cx))
                .child(self.strip("search-album-strip", cells, cx));
        }

        pane.child(Scrollbars::of(cx).around(
            "top-results-scrollbar",
            self.search_scroll.clone(),
            sections,
        ))
        .into_any_element()
    }

    fn section_heading(
        &self,
        shows: SearchShows,
        held: usize,
        said: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let shown = match shows {
            SearchShows::Songs => SONGS_AT_THE_TOP,
            SearchShows::Elsewhere => FOUND_AT_THE_TOP,
            SearchShows::Top | SearchShows::Albums | SearchShows::Artists => STRIP_AT_MOST,
        };
        let more = (held > shown).then(|| {
            kit::button(
                gpui::ElementId::from(SharedString::from(format!("see-all-{}", shows.id()))),
                None,
                format!("See all {held}"),
                shows.see_all(),
                Tone::Ghost,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.show_in_the_search(shows, cx)))
        });

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .px_6()
            .pt_5()
            .pb_2()
            .child(
                div()
                    .flex_none()
                    .text_size(px(theme::text_base()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child(shows.label()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(theme::faint()))
                    .children(said),
            )
            .children(more)
    }

    fn strip(&self, id: &'static str, cells: Vec<AnyElement>, cx: &mut Context<Self>) -> Div {
        let scroll = self
            .shelf_scrolls
            .borrow_mut()
            .entry(id)
            .or_default()
            .clone();

        div()
            .relative()
            .flex_none()
            .child(
                div()
                    .id(id)
                    .flex()
                    .items_start()
                    .gap_4()
                    .px(px(SHELF_INSET))
                    .pb_3()
                    .overflow_x_scroll()
                    .track_scroll(&scroll)
                    .children(cells),
            )
            .child(Scrollbars::of(cx).horizontal(id, scroll))
    }

    fn ask_again(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        kit::button(
            "ask-elsewhere-again",
            Some(Icon::Search),
            "Try again",
            ASK_AGAIN_HINT,
            Tone::Ghost,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.library
                .update(cx, |library, cx| library.ask_elsewhere_again(cx));
        }))
    }

    fn unreached(&self, cx: &mut Context<Self>) -> AnyElement {
        kit::empty_offering(Icon::Search, UNREACHED, None, self.ask_again(cx))
    }

    fn not_in_the_library(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let heading = self.search_heading(cx);
        let library = self.library.read(cx);
        let found = library.found();
        let elsewhere = library.elsewhere();
        let can_enrich = library.can_enrich();
        let held = found.len();
        let pane = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .children(heading);

        if found.is_empty() {
            let nothing = match elsewhere {
                Some(Beyond::Asking | Beyond::Refining(_)) => empty(Icon::Search, ASKING, None),
                Some(Beyond::Unreached) => self.unreached(cx),
                Some(Beyond::Elsewhere(_)) | None => empty(
                    Icon::Search,
                    NOTHING_ELSEWHERE,
                    (!can_enrich).then_some(ONLINE_IS_OFF),
                ),
            };
            return pane.child(nothing).into_any_element();
        }

        pane.child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .pt_2()
                .child(
                    Scrollbars::of(cx).around(
                        "found-scrollbar",
                        self.found_rows.clone(),
                        uniform_list(
                            "found-songs",
                            held,
                            cx.processor(move |this, range: Range<usize>, _, cx| {
                                let mut drawn = Vec::new();
                                for index in range {
                                    let Some(song) = found.get(index) else {
                                        continue;
                                    };
                                    let reached =
                                        this.reaches(Shift::Listing(Listed::Found), index);
                                    drawn.push(reorder::marked(
                                        this.found_row(index, song, cx),
                                        reached,
                                    ));
                                }
                                drawn
                            }),
                        )
                        .track_scroll(self.found_rows.clone())
                        .h_full()
                        .w_full(),
                    ),
                ),
        )
        .into_any_element()
    }
}
