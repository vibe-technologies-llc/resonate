use std::{ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, Context, Div, FontWeight, Role, SharedString, Stateful, Window, div,
    prelude::*, px, rgb, transparent_black,
};
use resonate_library::{
    AlbumFound, ArtistFound, FilledFromTheLink, FollowedLink, FollowedPlaylist, FollowedProfile,
    Linked, PlaylistLink, ProfileLink, Upkeep,
};

use crate::{
    Beyond, LibraryModel, Notice, Selection,
    icons::Icon,
    theme, toast,
    views::{
        browser::{Chooses, Plays, Rowed, reached_ring},
        hint::Names,
        kit::{self, Found as _, Tone},
        listing,
        playlists::Naming,
        pointed::LitUnderThePointer,
        reorder::{self, Listed, Shift, Step},
        root::{Pane, RootView},
        scrollbar::{SHELF_INSET, Scrollbars},
        sorting::Ordered,
        spoken::Spoken as _,
    },
    words,
};

pub(crate) const SONGS_AT_THE_TOP: usize = 5;

pub(crate) const FOUND_AT_THE_TOP: usize = 6;

const STRIP_AT_MOST: usize = 24;

const ARTIST_AT_THE_TOP: f32 = 72.0;

fn profile_told(name: &str, service: &str, kept: usize, wanted: usize) -> String {
    let told = words::search::keeping_in_step(kept, name, service);
    match wanted {
        0 => told,
        wanted => words::search::downloading_songs(wanted, told),
    }
}

fn playlist_told(
    name: &str,
    filled: Option<FilledFromTheLink>,
    wanted: usize,
    unnamed: usize,
) -> String {
    let mut told = match filled {
        Some(FilledFromTheLink {
            started: true,
            added,
            ..
        }) => words::search::made_of(added, name),
        Some(FilledFromTheLink { added: 0, .. }) => words::search::already_holds_every_song(name),
        Some(FilledFromTheLink { added, .. }) => words::search::added_to(added, name),
        None => words::search::nothing_held_yet(name),
    };
    if wanted > 0 {
        told = words::search::downloading_more(wanted, told);
    }
    if unnamed > 0 {
        told = words::search::unnamed(unnamed, told);
    }
    told
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopEntry {
    Song(usize),
    Found(usize),
    Artist(usize),
    ArtistFound(usize),
    Album(usize),
    AlbumFound(usize),
}

type Entered = fn(usize) -> TopEntry;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TopRun {
    songs: usize,
    found: usize,
    artists: usize,
    artists_found: usize,
    albums: usize,
    albums_found: usize,
}

impl TopRun {
    pub(crate) fn of(library: &LibraryModel) -> Self {
        Self {
            songs: library.listing().len().min(SONGS_AT_THE_TOP),
            found: library.found().len().min(FOUND_AT_THE_TOP),
            artists: library.artists().len().min(STRIP_AT_MOST),
            artists_found: library.artists_found().len(),
            albums: library.albums().len().min(STRIP_AT_MOST),
            albums_found: library.albums_found().len(),
        }
    }

    const fn runs(self) -> [(usize, Entered); 6] {
        [
            (self.songs, TopEntry::Song),
            (self.found, TopEntry::Found),
            (self.artists, TopEntry::Artist),
            (self.artists_found, TopEntry::ArtistFound),
            (self.albums, TopEntry::Album),
            (self.albums_found, TopEntry::AlbumFound),
        ]
    }

    pub(crate) const fn songs(self) -> usize {
        self.songs
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
        let to_artists = self.songs + self.found;
        let to_albums = to_artists + self.artists + self.artists_found;
        match wanted {
            TopEntry::Song(at) => at,
            TopEntry::Found(at) => self.songs + at,
            TopEntry::Artist(at) => to_artists + at,
            TopEntry::ArtistFound(at) => to_artists + self.artists + at,
            TopEntry::Album(at) => to_albums + at,
            TopEntry::AlbumFound(at) => to_albums + self.albums + at,
        }
    }

    fn line_of(self, row: usize) -> Line {
        let mut before = 0;
        for ((rows, _), strip) in self.runs().into_iter().zip(STRIP_RUNS) {
            if row < before + rows {
                return match strip {
                    true => Line::Strip(before..before + rows),
                    false => Line::Row(row),
                };
            }
            before += rows;
        }
        Line::Row(row)
    }

    pub(crate) fn line_beyond(self, row: usize, step: Step) -> Option<usize> {
        let line = self.line_of(row);
        match step {
            Step::Below => {
                let end = line.end();
                (end < self.len()).then_some(end)
            }
            Step::Above => line
                .start()
                .checked_sub(1)
                .map(|before| self.line_of(before).start()),
        }
    }

    pub(crate) fn along_the_strip(self, row: usize, step: Step) -> Option<Option<usize>> {
        let Line::Strip(cells) = self.line_of(row) else {
            return None;
        };
        Some(match step {
            Step::Below => (row + 1 < cells.end).then_some(row + 1),
            Step::Above => (row > cells.start).then(|| row - 1),
        })
    }
}

const STRIP_RUNS: [bool; 6] = [false, false, true, true, true, true];

enum Line {
    Row(usize),
    Strip(Range<usize>),
}

impl Line {
    fn start(&self) -> usize {
        match self {
            Self::Row(row) => *row,
            Self::Strip(cells) => cells.start,
        }
    }

    fn end(&self) -> usize {
        match self {
            Self::Row(row) => row + 1,
            Self::Strip(cells) => cells.end,
        }
    }
}

struct Told {
    looking: &'static str,
    nothing: &'static str,
}

impl Told {
    fn of(link: &FollowedLink) -> Self {
        match link {
            FollowedLink::Song(_) => Self {
                looking: words::search::looking_the_song_up(),
                nothing: words::search::no_song_at_the_link(),
            },
            FollowedLink::Album(_) => Self {
                looking: words::search::looking_the_album_up(),
                nothing: words::search::no_album_at_the_link(),
            },
            FollowedLink::Artist(_) => Self {
                looking: words::search::looking_the_artist_up(),
                nothing: words::search::no_artist_at_the_link(),
            },
            FollowedLink::Playlist(_) => Self {
                looking: words::search::looking_the_playlist_up(),
                nothing: words::search::no_playlist_at_the_link(),
            },
            FollowedLink::Profile(_) => Self {
                looking: words::search::looking_the_profile_up(),
                nothing: words::search::no_profile_at_the_link(),
            },
        }
    }
}

enum Following {
    Linked(resonate_library::Result<Linked>),
    Playlist(
        PlaylistLink,
        resonate_library::Result<Option<FollowedPlaylist>>,
    ),
    Profile(
        ProfileLink,
        resonate_library::Result<Option<FollowedProfile>>,
    ),
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

    fn label(self) -> &'static str {
        match self {
            Self::Top => words::search::top_results(),
            Self::Songs => words::search::songs(),
            Self::Albums => words::search::albums(),
            Self::Artists => words::search::artists(),
        }
    }

    fn saying(self) -> &'static str {
        match self {
            Self::Top => words::search::top_results_hint(),
            Self::Songs => words::search::songs_hint(),
            Self::Albums => words::search::albums_hint(),
            Self::Artists => words::search::artists_hint(),
        }
    }

    const fn ordered_by(self) -> Option<(&'static str, Ordered)> {
        match self {
            Self::Songs => Some(("order-tracks", Ordered::Tracks)),
            Self::Albums => Some(("order-albums", Ordered::Albums)),
            Self::Artists => Some(("order-artists", Ordered::Artists)),
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

    fn label(self) -> &'static str {
        match self {
            Self::Songs => words::search::songs(),
            Self::Found => words::search::not_in_your_library(),
            Self::Albums => words::search::albums(),
            Self::Artists => words::search::artists(),
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

    fn see_all(self) -> &'static str {
        match self {
            Self::Songs => words::search::see_every_song(),
            Self::Found => words::search::see_every_song_beyond(),
            Self::Albums => words::search::see_every_album(),
            Self::Artists => words::search::see_every_artist(),
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
            return words::search::nothing_in_your_library().to_owned();
        }

        words::search::in_your_library(counts([
            (self.songs, words::search::songs_counted),
            (self.albums, words::search::albums_counted),
            (self.artists, words::search::artists_counted),
        ]))
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
        let found = counts([
            (self.songs_found(), words::search::songs_counted),
            (self.albums_found, words::search::albums_counted),
            (self.artists_found, words::search::artists_counted),
        ]);

        Some(match self.elsewhere? {
            Beyond::Elsewhere(_) => words::search::not_in_it(found),
            Beyond::Refining(_) => words::search::not_in_it_asking_for_the_rest(found),
            Beyond::Asking if found.is_empty() => words::search::asking_for_more().to_owned(),
            Beyond::Asking => words::search::not_in_it_asking_for_more(found),
            Beyond::Unreached(_) if found.is_empty() => words::search::unreached().to_owned(),
            Beyond::Unreached(_) => words::search::not_in_it_unreached(found),
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

type SaysTheCount = fn(usize) -> String;

fn counts<const KINDS: usize>(kinds: [(usize, SaysTheCount); KINDS]) -> String {
    kinds
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, said)| said(count))
        .collect::<Vec<_>>()
        .join(" · ")
}

impl RootView {
    pub(crate) fn matches_said(&self, cx: &App) -> Option<SharedString> {
        self.search_in_front(cx)?;
        let matched = Matched::read(self, cx);

        Some(SharedString::from(matched.in_the_library()))
    }

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
            toast::tell(
                Notice::Noted(words::search::online_to_follow_a_link().to_owned()),
                cx,
            );
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
                        FollowedLink::Playlist(playlist) => Following::Playlist(
                            playlist.clone(),
                            library.follow_playlist_link(reference, playlist),
                        ),
                        FollowedLink::Profile(profile) => Following::Profile(
                            profile.clone(),
                            library.follow_profile(reference, profile, Upkeep::InStep),
                        ),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| match followed {
                Following::Linked(linked) => this.followed(linked, &told, cx),
                Following::Playlist(link, playlist) => {
                    this.followed_a_playlist(link, playlist, &told, cx);
                }
                Following::Profile(link, profile) => {
                    this.followed_a_profile(&link, profile, &told, cx);
                }
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
                Notice::Done(words::search::already_in_your_library(title.as_str())),
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
            Err(error) => toast::tell(
                toast::could_not(words::search::following_the_link(), &error),
                cx,
            ),
        }
    }

    fn followed_a_playlist(
        &mut self,
        link: PlaylistLink,
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
                toast::tell(
                    toast::could_not(words::search::following_the_link(), &error),
                    cx,
                );
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
                .spawn(
                    async move { library.fill_from_the_link(&link, &filling, &held, Upkeep::Once) },
                )
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
                Err(error) => toast::tell(
                    toast::could_not(words::search::filling_the_playlist(), &error),
                    cx,
                ),
            });
        })
        .detach();
    }

    fn followed_a_profile(
        &mut self,
        link: &ProfileLink,
        followed: resonate_library::Result<Option<FollowedProfile>>,
        told: &Told,
        cx: &mut Context<Self>,
    ) {
        let followed = match followed {
            Ok(Some(followed)) if !followed.playlists.is_empty() => followed,
            Ok(_) => {
                toast::tell(Notice::Trouble(told.nothing.to_owned()), cx);
                return;
            }
            Err(error) => {
                toast::tell(
                    toast::could_not(words::search::following_the_link(), &error),
                    cx,
                );
                return;
            }
        };
        let kept = followed.playlists.len();
        let mut wanted = 0;
        for read in followed.playlists {
            for found in read.found {
                wanted += 1;
                self.library
                    .update(cx, |library, cx| library.want_found(found, cx));
            }
        }
        self.library
            .update(cx, |library, cx| library.reload_playlists(cx));
        self.set_pane(Pane::Playlists, cx);
        toast::tell(
            Notice::Done(profile_told(&followed.name, link.service(), kept, wanted)),
            cx,
        );
    }

    pub(crate) fn followed_the_account(
        &mut self,
        followed: resonate_library::Result<Option<FollowedProfile>>,
        upkeep: Upkeep,
        cx: &mut Context<Self>,
    ) {
        match upkeep {
            Upkeep::InStep => self.followed_a_profile(
                &ProfileLink::SpotifyAccount,
                followed,
                &Told::of(&FollowedLink::Profile(ProfileLink::SpotifyAccount)),
                cx,
            ),
            Upkeep::Once => match followed {
                Ok(Some(moved)) => {
                    let kept = moved.playlists.len();
                    self.library
                        .update(cx, |library, cx| library.reload_playlists(cx));
                    self.set_pane(Pane::Playlists, cx);
                    toast::tell(
                        Notice::Done(words::search::moved_to_resonate(kept, moved.name.as_str())),
                        cx,
                    );
                }
                Ok(None) => toast::tell(
                    Notice::Trouble(words::search::no_profile_at_the_link().to_owned()),
                    cx,
                ),
                Err(error) => toast::tell(
                    toast::could_not(words::search::following_the_link(), &error),
                    cx,
                ),
            },
        }
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
            .map(|meant| words::search::read_as(meant.artist.as_str(), meant.title.as_str()));
        let summary = match matched.beyond() {
            Some(beyond) => format!("{} · {beyond}", matched.in_the_library()),
            None => matched.in_the_library(),
        };
        let as_typed = meant.is_some().then(|| {
            self.in_the_pane_ring(
                kit::button(
                    "search-as-typed",
                    None,
                    words::search::search_as_typed(),
                    words::search::as_typed_hint(),
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
                    words::search::save_this_search(),
                    words::playlists::save_search_hint(),
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
            .map(|(named, ordered)| self.orders_a_listing(named, ordered, cx));
        let plays = (matched.songs > 0).then(|| (self.shuffle_all(cx), self.play_all(cx)));
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
                                .child(kit::eyebrow(words::search::search()))
                                .child(kit::title(SharedString::from(words::search::query(
                                    query.as_str(),
                                ))))
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
                .children(named)
                .child(tabs),
        )
    }

    fn search_tabs(
        &self,
        shows: SearchShows,
        matched: Matched,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut tabs = div()
            .id("search-tabs")
            .spoken_as(Role::TabList)
            .flex()
            .flex_wrap()
            .items_end()
            .gap_5();
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

        let header = div()
            .id(id.clone())
            .found_as(&id)
            .named_as(Role::Tab, tab.label(), tab.saying())
            .chosen(chosen)
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
            });

        self.in_the_pane_ring(
            header,
            move |this, _, cx| this.show_in_the_search(tab, cx),
            cx,
        )
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
        let pane = kit::pane().children(heading);

        if tracks.is_empty()
            && albums.is_empty()
            && artists.is_empty()
            && found.is_empty()
            && artists_found.is_empty()
            && albums_found.is_empty()
        {
            let nothing = match matched.elsewhere {
                Some(Beyond::Asking | Beyond::Refining(_)) => kit::empty(
                    Icon::Search,
                    words::search::asking(),
                    Some(words::search::looking_beyond()),
                ),
                Some(Beyond::Unreached(_)) => Self::unreached(),
                Some(Beyond::Elsewhere(_)) | None => self.nothing_matched(
                    Icon::Search,
                    words::search::nothing_matches(),
                    Some(words::search::nothing_held_matches()),
                    cx,
                ),
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

        if !tracks.is_empty() {
            let mut rows = div().flex().flex_col();
            for (index, track) in tracks.iter().enumerate().take(SONGS_AT_THE_TOP) {
                let row = run.row_of(TopEntry::Song(index));
                let shift = Shift::Listing(Listed::Top);
                let reached = self.reaches(shift, row);
                let listed = self.track_row(
                    &tracks,
                    index,
                    track,
                    Rowed {
                        playing: playing == Some(track.id),
                        plays: Plays::AsTheListingIsDrawn { in_an_album: false },
                        reach: self.at_the_reach(shift, row),
                        chooses: Some(Chooses::among(Listed::Top, row)),
                    },
                    cx,
                );
                rows = rows.child(
                    self.marked_where_reached(shift, row, listed)
                        .when(reached, |row| row.child(self.brought_into_view(None))),
                );
            }
            sections = sections
                .child(self.section_heading(Section::Songs, matched.songs, None, cx))
                .child(rows);
        }

        if let Some(beyond) = matched.elsewhere {
            let said = match beyond {
                Beyond::Elsewhere(_) => words::search::press_to_download(),
                Beyond::Refining(_) | Beyond::Asking => words::search::asking(),
                Beyond::Unreached(_) => words::search::unreached(),
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

        if !artists.is_empty() {
            let cells = artists
                .iter()
                .take(STRIP_AT_MOST)
                .enumerate()
                .map(|(at, artist)| {
                    let cell = self.artist_cell_at(artist, ARTIST_AT_THE_TOP, None, cx);
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

        if !albums.is_empty() {
            let cells = albums
                .iter()
                .take(STRIP_AT_MOST)
                .enumerate()
                .map(|(at, album)| {
                    let cell = self.album_cell_at(album, theme::shelf_cover(), None, cx);
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
                    words::search::see_all(held),
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

        self.found_strip(words::search::artists_not_held(), STRIP, cells, cx)
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

        self.found_strip(words::search::albums_not_held(), STRIP, cells, cx)
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
            Some(self.search_scroll.clone()),
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
                words::search::try_again(),
                words::search::ask_again_hint(),
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
            Beyond::Asking | Beyond::Refining(_) => Some(kit::empty(
                Icon::Search,
                words::search::asking(),
                Some(words::search::looking_beyond()),
            )),
            Beyond::Unreached(_) => Some(Self::unreached()),
            Beyond::Elsewhere(_) => None,
        }
    }

    pub(crate) fn unreached() -> AnyElement {
        kit::trouble(
            Icon::Search,
            words::search::unreached(),
            Some(words::search::unreached_more()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{TopEntry, TopRun};
    use crate::views::reorder::Step;

    fn run() -> TopRun {
        TopRun {
            songs: 2,
            found: 1,
            artists: 3,
            artists_found: 0,
            albums: 4,
            albums_found: 2,
        }
    }

    #[test]
    fn songs_come_before_the_artists_and_albums() {
        let run = run();

        assert_eq!(run.row_of(TopEntry::Song(0)), 0);
        assert_eq!(run.row_of(TopEntry::Found(0)), 2);
        assert_eq!(run.row_of(TopEntry::Artist(0)), 3);
        assert_eq!(run.row_of(TopEntry::Album(0)), 6);
        assert_eq!(run.row_of(TopEntry::AlbumFound(1)), 11);
        assert_eq!(run.at(4), Some(TopEntry::Artist(1)));
        assert_eq!(run.len(), 12);
    }

    #[test]
    fn down_leaves_a_strip_for_the_line_below_and_up_for_the_line_above() {
        let run = run();

        assert_eq!(run.line_beyond(0, Step::Below), Some(1));
        assert_eq!(run.line_beyond(2, Step::Below), Some(3));
        assert_eq!(run.line_beyond(3, Step::Below), Some(6));
        assert_eq!(run.line_beyond(5, Step::Below), Some(6));
        assert_eq!(run.line_beyond(6, Step::Below), Some(10));
        assert_eq!(run.line_beyond(11, Step::Below), None);
        assert_eq!(run.line_beyond(10, Step::Above), Some(6));
        assert_eq!(run.line_beyond(7, Step::Above), Some(3));
        assert_eq!(run.line_beyond(4, Step::Above), Some(2));
        assert_eq!(run.line_beyond(0, Step::Above), None);
    }

    #[test]
    fn the_cells_of_a_strip_are_stepped_along_and_a_song_is_not_one() {
        let run = run();

        assert_eq!(run.along_the_strip(3, Step::Below), Some(Some(4)));
        assert_eq!(run.along_the_strip(5, Step::Below), Some(None));
        assert_eq!(run.along_the_strip(3, Step::Above), Some(None));
        assert_eq!(run.along_the_strip(5, Step::Above), Some(Some(4)));
        assert_eq!(run.along_the_strip(1, Step::Below), None);
    }
}
