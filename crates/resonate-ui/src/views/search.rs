use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px,
    rgb, transparent_black,
};
use resonate_library::{AlbumFound, ArtistFound, Filled, FollowedLink, FollowedPlaylist, Linked};

use crate::{
    Beyond, LibraryModel, Notice, Selection, format,
    icons::Icon,
    theme, toast,
    views::{
        browser::{Plays, Rowed, reached_ring},
        hint::Names,
        kit::{self, Found as _, Tone},
        listing,
        playlists::{Naming, SAVE_SEARCH_HINT},
        pointed::LitUnderThePointer,
        reorder::{self, Listed, Shift},
        root::{Pane, RootView},
        scrollbar::{SHELF_INSET, Scrollbars},
    },
};

pub(crate) const SONGS_AT_THE_TOP: usize = 5;

pub(crate) const FOUND_AT_THE_TOP: usize = 6;

const STRIP_AT_MOST: usize = 24;

const ARTIST_AT_THE_TOP: f32 = 72.0;

const ARTISTS_NOT_HELD: &str = "Artists not in your library";

const ALBUMS_NOT_HELD: &str = "Albums not in your library";

const NOTHING_MATCHES: &str = "Nothing in your library matches.";

const ASKING: &str = "Asking MusicBrainz…";

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

const LOOKING_THE_ARTIST_UP: &str = "Looking up the artist that link names…";

const NO_ARTIST_AT_THE_LINK: &str = "No service could tell which artist that link names";

const FOLLOWING_THE_LINK: &str = "look up what that link names";

const LOOKING_THE_PLAYLIST_UP: &str = "Looking up every song that playlist holds…";

const NO_PLAYLIST_AT_THE_LINK: &str = "No service could read the playlist that link names";

const FILLING_THE_PLAYLIST: &str = "make the playlist that link names";

fn playlist_told(name: &str, filled: Option<Filled>, wanted: usize, unnamed: usize) -> String {
    let mut told = match filled {
        Some(Filled {
            started: true,
            added,
            ..
        }) => format!(
            "Made “{name}” of {}",
            format::counted(added, "song", "songs")
        ),
        Some(Filled { added: 0, .. }) => format!("“{name}” already holds every song you have"),
        Some(Filled { added, .. }) => format!(
            "Added {} to “{name}”",
            format::counted(added, "song", "songs")
        ),
        None => format!("Nothing “{name}” holds is in your library yet"),
    };
    if wanted > 0 {
        told.push_str(&format!(
            " · downloading {}; paste the link again once they arrive",
            format::counted(wanted, "more", "more")
        ));
    }
    if unnamed > 0 {
        told.push_str(&format!(
            " · {} no service could name",
            format::counted(unnamed, "song", "songs")
        ));
    }
    told
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopEntry {
    Artist(usize),
    ArtistFound(usize),
    Song(usize),
    Found(usize),
    Album(usize),
    AlbumFound(usize),
}

type Entered = fn(usize) -> TopEntry;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TopRun {
    artists: usize,
    artists_found: usize,
    songs: usize,
    found: usize,
    albums: usize,
    albums_found: usize,
}

impl TopRun {
    pub(crate) fn of(library: &LibraryModel) -> Self {
        Self {
            artists: library.artists().len().min(STRIP_AT_MOST),
            artists_found: library.artists_found().len(),
            songs: library.listing().len().min(SONGS_AT_THE_TOP),
            found: library.found().len().min(FOUND_AT_THE_TOP),
            albums: library.albums().len().min(STRIP_AT_MOST),
            albums_found: library.albums_found().len(),
        }
    }

    const fn runs(self) -> [(usize, Entered); 6] {
        [
            (self.artists, TopEntry::Artist),
            (self.artists_found, TopEntry::ArtistFound),
            (self.songs, TopEntry::Song),
            (self.found, TopEntry::Found),
            (self.albums, TopEntry::Album),
            (self.albums_found, TopEntry::AlbumFound),
        ]
    }

    pub(crate) fn len(self) -> usize {
        self.runs().iter().map(|(rows, _)| rows).sum()
    }

    pub(crate) fn at(self, row: usize) -> Option<TopEntry> {
        let mut before = 0;
        for (rows, entry) in self.runs() {
            if row < before + rows {
                return Some(entry(row - before));
            }
            before += rows;
        }
        None
    }

    pub(crate) const fn row_of(self, wanted: TopEntry) -> usize {
        let to_songs = self.artists + self.artists_found;
        let to_albums = to_songs + self.songs + self.found;
        match wanted {
            TopEntry::Artist(at) => at,
            TopEntry::ArtistFound(at) => self.artists + at,
            TopEntry::Song(at) => to_songs + at,
            TopEntry::Found(at) => to_songs + self.songs + at,
            TopEntry::Album(at) => to_albums + at,
            TopEntry::AlbumFound(at) => to_albums + self.albums + at,
        }
    }
}

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
            FollowedLink::Artist(_) => Self {
                looking: LOOKING_THE_ARTIST_UP,
                nothing: NO_ARTIST_AT_THE_LINK,
            },
            FollowedLink::Playlist(_) => Self {
                looking: LOOKING_THE_PLAYLIST_UP,
                nothing: NO_PLAYLIST_AT_THE_LINK,
            },
        }
    }
}

enum Following {
    Linked(resonate_library::Result<Linked>),
    Playlist(resonate_library::Result<Option<FollowedPlaylist>>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SearchShows {
    #[default]
    Top,
    Songs,
    Albums,
    Artists,
}

impl SearchShows {
    const ALL: [Self; 4] = [Self::Top, Self::Songs, Self::Albums, Self::Artists];

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
            Self::Top => None,
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Top => "search-top",
            Self::Songs => "search-songs",
            Self::Albums => "search-albums",
            Self::Artists => "search-artists",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Top => "Top results",
            Self::Songs => "Songs",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
        }
    }

    const fn saying(self) -> &'static str {
        match self {
            Self::Top => "The best of each kind of match, on one page",
            Self::Songs => "Every song that matches, in your library and beyond it",
            Self::Albums => "Every album that matches, in your library and beyond it",
            Self::Artists => "Every artist that matches, in your library and beyond it",
        }
    }

    const fn ordered_by(self) -> Option<&'static str> {
        match self {
            Self::Songs => Some("order-tracks"),
            Self::Albums => Some("order-albums"),
            Self::Artists => Some("order-artists"),
            Self::Top => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Songs,
    Found,
    Albums,
    Artists,
}

impl Section {
    const fn shows(self) -> SearchShows {
        match self {
            Self::Songs | Self::Found => SearchShows::Songs,
            Self::Albums => SearchShows::Albums,
            Self::Artists => SearchShows::Artists,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Songs => "Songs",
            Self::Found => "Not in your library",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Songs => "search-songs",
            Self::Found => "search-found",
            Self::Albums => "search-albums",
            Self::Artists => "search-artists",
        }
    }

    const fn shown(self) -> usize {
        match self {
            Self::Songs => SONGS_AT_THE_TOP,
            Self::Found => FOUND_AT_THE_TOP,
            Self::Albums | Self::Artists => STRIP_AT_MOST,
        }
    }

    const fn see_all(self) -> &'static str {
        match self {
            Self::Songs => "See every matching song",
            Self::Found => "See every matching song, in your library and beyond it",
            Self::Albums => "See every matching album",
            Self::Artists => "See every matching artist",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Matched {
    songs: usize,
    albums: usize,
    artists: usize,
    albums_found: usize,
    artists_found: usize,
    elsewhere: Option<Beyond>,
}

impl Matched {
    fn read(root: &RootView, cx: &App) -> Self {
        let library = root.library.read(cx);

        Self {
            songs: library.tracks_counted() as usize,
            albums: library.albums_counted() as usize,
            artists: library.artists_counted() as usize,
            albums_found: library.albums_found().len(),
            artists_found: library.artists_found().len(),
            elsewhere: library.elsewhere(),
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

    fn songs_found(self) -> usize {
        match self.elsewhere {
            Some(Beyond::Elsewhere(found) | Beyond::Refining(found) | Beyond::Unreached(found)) => {
                found
            }
            Some(Beyond::Asking) | None => 0,
        }
    }

    fn beyond(self) -> Option<String> {
        let found = [
            (self.songs_found(), "song", "songs"),
            (self.albums_found, "album", "albums"),
            (self.artists_found, "artist", "artists"),
        ]
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, one, many)| format::counted(count, one, many))
        .collect::<Vec<_>>()
        .join(" · ");

        Some(match self.elsewhere? {
            Beyond::Elsewhere(_) => format!("{found} not in it"),
            Beyond::Refining(_) => {
                format!("{found} not in it so far, asking MusicBrainz for the rest…")
            }
            Beyond::Asking if found.is_empty() => "asking MusicBrainz for more…".to_owned(),
            Beyond::Asking => format!("{found} not in it so far, asking MusicBrainz for more…"),
            Beyond::Unreached(_) if found.is_empty() => UNREACHED.to_owned(),
            Beyond::Unreached(_) => format!("{found} not in it so far; {UNREACHED}"),
        })
    }

    fn counted(self, shows: SearchShows) -> Option<SharedString> {
        let (held, found) = match shows {
            SearchShows::Top => return None,
            SearchShows::Songs => (self.songs, self.songs_found()),
            SearchShows::Albums => (self.albums, self.albums_found),
            SearchShows::Artists => (self.artists, self.artists_found),
        };
        let asking = matches!(self.elsewhere, Some(Beyond::Asking | Beyond::Refining(_)));
        let count = held + found;

        Some(SharedString::from(if asking {
            format!("{count}…")
        } else {
            count.to_string()
        }))
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

        cx.spawn_in(window, async move |this, cx| {
            let followed = cx
                .background_executor()
                .spawn(async move {
                    let reference = reference.as_ref();
                    match &link {
                        FollowedLink::Song(song) => {
                            Following::Linked(library.follow_link(reference, song))
                        }
                        FollowedLink::Album(album) => {
                            Following::Linked(library.follow_album_link(reference, album))
                        }
                        FollowedLink::Artist(artist) => {
                            Following::Linked(library.follow_artist_link(reference, artist))
                        }
                        FollowedLink::Playlist(playlist) => {
                            Following::Playlist(library.follow_playlist_link(reference, playlist))
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| match followed {
                Following::Linked(linked) => this.followed(linked, &told, cx),
                Following::Playlist(playlist) => this.followed_a_playlist(playlist, &told, cx),
            });
        })
        .detach();
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
            Ok(Linked::Album { group, release, .. }) => {
                toast::dismiss(cx);
                self.library
                    .update(cx, |library, cx| library.want_album(group, release, cx));
            }
            Ok(Linked::HeldArtist { artist, .. }) => {
                toast::dismiss(cx);
                self.opened(Selection::Artist(artist), cx);
            }
            Ok(Linked::Artist(found)) => {
                toast::dismiss(cx);
                self.open_artist_found(found, cx);
            }
            Ok(Linked::Unnamed) => {
                toast::tell(Notice::Trouble(told.nothing.to_owned()), cx);
            }
            Err(error) => toast::tell(toast::could_not(FOLLOWING_THE_LINK, &error), cx),
        }
    }

    fn followed_a_playlist(
        &mut self,
        followed: resonate_library::Result<Option<FollowedPlaylist>>,
        told: &Told,
        cx: &mut Context<Self>,
    ) {
        let followed = match followed {
            Ok(Some(followed)) => followed,
            Ok(None) => {
                toast::tell(Notice::Trouble(told.nothing.to_owned()), cx);
                return;
            }
            Err(error) => {
                toast::tell(toast::could_not(FOLLOWING_THE_LINK, &error), cx);
                return;
            }
        };
        let FollowedPlaylist {
            name,
            held,
            found,
            unnamed,
        } = followed;
        let wanted = found.len();
        for found in found {
            self.library
                .update(cx, |library, cx| library.want_found(found, cx));
        }
        if held.is_empty() {
            toast::tell(
                Notice::Noted(playlist_told(&name, None, wanted, unnamed)),
                cx,
            );
            return;
        }

        let library = self.library.read(cx).catalog();
        cx.spawn(async move |this, cx| {
            let filling = name.clone();
            let filled = cx
                .background_executor()
                .spawn(async move { library.fill_playlist_named(&filling, &held) })
                .await;
            let _ = this.update(cx, |this, cx| match filled {
                Ok(filled) => {
                    this.library
                        .update(cx, |library, cx| library.reload_playlists(cx));
                    this.set_pane(Pane::Playlists, cx);
                    this.show_playlist(Some(filled.playlist), cx);
                    toast::tell(
                        Notice::Done(playlist_told(&name, Some(filled), wanted, unnamed)),
                        cx,
                    );
                }
                Err(error) => toast::tell(toast::could_not(FILLING_THE_PLAYLIST, &error), cx),
            });
        })
        .detach();
    }

    pub(crate) fn search_pane(&mut self, shows: SearchShows, cx: &mut Context<Self>) -> AnyElement {
        match shows {
            SearchShows::Top => self.top_results(cx),
            SearchShows::Songs => self.tracks(cx),
            SearchShows::Albums => self.albums(cx),
            SearchShows::Artists => self.artists(cx),
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
            self.in_the_pane_ring(
                kit::button(
                    "search-as-typed",
                    None,
                    "Search the words as typed",
                    AS_TYPED_HINT,
                    Tone::Ghost,
                ),
                |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.search_as_typed(cx));
                },
                cx,
            )
        });
        let sung = self.sung_offer(Tone::Ghost, cx);
        let again =
            matches!(matched.elsewhere, Some(Beyond::Unreached(_))).then(|| self.ask_again(cx));
        let saves = naming.is_none().then(|| {
            self.in_the_pane_ring(
                kit::button(
                    "save-search",
                    Some(Icon::Search),
                    "Save this search",
                    SAVE_SEARCH_HINT,
                    Tone::Ghost,
                ),
                |this, window, cx| {
                    this.name_a_playlist(Naming::Query(None), window, cx);
                },
                cx,
            )
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
            .children(again)
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
        for tab in SearchShows::ALL {
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
        let artists_found = library.artists_found();
        let albums_found = library.albums_found();
        let run = TopRun::of(library);
        let playing = self.playing_now(cx).track;
        let pane = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .children(heading);

        if tracks.is_empty()
            && albums.is_empty()
            && artists.is_empty()
            && found.is_empty()
            && artists_found.is_empty()
            && albums_found.is_empty()
        {
            let nothing = match matched.elsewhere {
                Some(Beyond::Asking | Beyond::Refining(_)) => {
                    kit::empty(Icon::Search, ASKING, Some(NOTHING_MATCHES))
                }
                Some(Beyond::Unreached(_)) => Self::unreached(),
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
                .enumerate()
                .map(|(at, artist)| {
                    let cell = self.artist_cell_at(artist, ARTIST_AT_THE_TOP, cx);
                    self.at_the_top(
                        cell,
                        run.row_of(TopEntry::Artist(at)),
                        Some("search-artist-strip"),
                    )
                })
                .collect();
            sections = sections
                .child(self.section_heading(Section::Artists, matched.artists, None, cx))
                .child(self.strip("search-artist-strip", cells, cx));
        }

        if !artists_found.is_empty() {
            sections = sections.child(self.artists_found_strip(&artists_found, Some(run), cx));
        }

        if !tracks.is_empty() {
            let mut rows = div().flex().flex_col();
            for (index, track) in tracks.iter().enumerate().take(SONGS_AT_THE_TOP) {
                let row = run.row_of(TopEntry::Song(index));
                let reached = self.reaches(Shift::Listing(Listed::Top), row);
                rows = rows.child(
                    reorder::marked(
                        self.track_row(
                            &tracks,
                            index,
                            track,
                            Rowed {
                                playing: playing == Some(track.id),
                                plays: Plays::AsTheListingIsDrawn { in_an_album: false },
                                reach: self.at_the_reach(Shift::Listing(Listed::Top), row),
                            },
                            cx,
                        ),
                        reached,
                    )
                    .when(reached, |row| row.child(self.brought_into_view(None))),
                );
            }
            sections = sections
                .child(self.section_heading(Section::Songs, matched.songs, None, cx))
                .child(rows);
        }

        if let Some(beyond) = matched.elsewhere {
            let said = match beyond {
                Beyond::Elsewhere(_) => PRESS_TO_DOWNLOAD,
                Beyond::Refining(_) | Beyond::Asking => ASKING,
                Beyond::Unreached(_) => UNREACHED,
            };
            let mut rows = div().flex().flex_col();
            for (index, song) in found.iter().enumerate().take(FOUND_AT_THE_TOP) {
                let row = run.row_of(TopEntry::Found(index));
                let reached = self.reaches(Shift::Listing(Listed::Top), row);
                rows = rows.child(
                    reorder::marked(self.found_row(index, song, cx), reached)
                        .when(reached, |row| row.child(self.brought_into_view(None))),
                );
            }
            sections = sections
                .child(self.section_heading(Section::Found, found.len(), Some(said), cx))
                .child(rows);
        }

        if !albums.is_empty() {
            let cells = albums
                .iter()
                .take(STRIP_AT_MOST)
                .enumerate()
                .map(|(at, album)| {
                    let cell = self.album_cell_at(album, theme::shelf_cover(), cx);
                    self.at_the_top(
                        cell,
                        run.row_of(TopEntry::Album(at)),
                        Some("search-album-strip"),
                    )
                })
                .collect();
            sections = sections
                .child(self.section_heading(Section::Albums, matched.albums, None, cx))
                .child(self.strip("search-album-strip", cells, cx));
        }

        if !albums_found.is_empty() {
            sections = sections.child(self.albums_found_strip(&albums_found, Some(run), cx));
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
        section: Section,
        held: usize,
        said: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let more = (held > section.shown()).then(|| {
            self.in_the_pane_ring(
                kit::button(
                    gpui::ElementId::from(SharedString::from(format!("see-all-{}", section.id()))),
                    None,
                    format!("See all {held}"),
                    section.see_all(),
                    Tone::Ghost,
                ),
                move |this, _, cx| this.show_in_the_search(section.shows(), cx),
                cx,
            )
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
                    .child(section.label()),
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

    pub(crate) fn artists_found_strip(
        &self,
        found: &[ArtistFound],
        in_the_top: Option<TopRun>,
        cx: &mut Context<Self>,
    ) -> Div {
        const STRIP: &str = "search-artists-found-strip";
        let cells = found
            .iter()
            .enumerate()
            .map(|(at, artist)| {
                let cell = self.artist_found_cell(artist, ARTIST_AT_THE_TOP, cx);
                match in_the_top {
                    Some(run) => {
                        self.at_the_top(cell, run.row_of(TopEntry::ArtistFound(at)), Some(STRIP))
                    }
                    None => cell.into_any_element(),
                }
            })
            .collect();

        self.found_strip(ARTISTS_NOT_HELD, STRIP, cells, cx)
    }

    pub(crate) fn albums_found_strip(
        &self,
        found: &[AlbumFound],
        in_the_top: Option<TopRun>,
        cx: &mut Context<Self>,
    ) -> Div {
        const STRIP: &str = "search-albums-found-strip";
        let cells = found
            .iter()
            .enumerate()
            .map(|(at, album)| {
                let cell = self.album_found_cell(album, theme::shelf_cover(), cx);
                match in_the_top {
                    Some(run) => {
                        self.at_the_top(cell, run.row_of(TopEntry::AlbumFound(at)), Some(STRIP))
                    }
                    None => cell.into_any_element(),
                }
            })
            .collect();

        self.found_strip(ALBUMS_NOT_HELD, STRIP, cells, cx)
    }

    fn at_the_top(
        &self,
        cell: Stateful<Div>,
        row: usize,
        strip: Option<&'static str>,
    ) -> AnyElement {
        let reached = self.reaches(Shift::Listing(Listed::Top), row);
        div()
            .relative()
            .flex_none()
            .child(cell)
            .when(reached, |held| {
                held.child(reached_ring())
                    .child(self.brought_into_view(strip))
            })
            .into_any_element()
    }

    fn brought_into_view(&self, strip: Option<&'static str>) -> impl IntoElement {
        let across = strip.map(|strip| {
            self.shelf_scrolls
                .borrow_mut()
                .entry(strip)
                .or_default()
                .clone()
        });
        kit::brought_into_view_within(
            self.search_scroll.clone(),
            across,
            Rc::clone(&self.reached_unseen),
        )
    }

    fn found_strip(
        &self,
        named: &'static str,
        id: &'static str,
        cells: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_none()
            .child(
                div()
                    .flex_none()
                    .px_6()
                    .pt_5()
                    .pb_2()
                    .text_size(px(theme::text_base()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child(named),
            )
            .child(self.strip(id, cells, cx))
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
        self.in_the_pane_ring(
            kit::button(
                "ask-elsewhere-again",
                Some(Icon::Search),
                "Try again",
                ASK_AGAIN_HINT,
                Tone::Ghost,
            ),
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.ask_elsewhere_again(cx));
            },
            cx,
        )
    }

    pub(crate) fn nothing_beyond(&self, cx: &App) -> Option<AnyElement> {
        self.search_in_front(cx)?;
        match self.library.read(cx).elsewhere()? {
            Beyond::Asking | Beyond::Refining(_) => {
                Some(kit::empty(Icon::Search, ASKING, Some(NOTHING_MATCHES)))
            }
            Beyond::Unreached(_) => Some(Self::unreached()),
            Beyond::Elsewhere(_) => None,
        }
    }

    pub(crate) fn unreached() -> AnyElement {
        kit::empty(Icon::Search, UNREACHED, None)
    }
}
