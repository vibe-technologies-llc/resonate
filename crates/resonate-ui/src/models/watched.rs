use std::{
    mem,
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
    sync::Arc,
    thread::{self},
    time::{Duration, Instant, SystemTime},
};

use ahash::AHashMap;
use gpui::{App, AsyncApp, WeakEntity};
use resonate_core::{AlbumId, ArtistId, PlaylistId};
use resonate_library::{
    AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, ArtistTotals, FromTheLink, Library, Missing,
    Playlist, Reference, RootsWatch, ScanHandle, ScanOptions, TrackQuery, Upkeep, Window,
};

use super::{
    ARTIST_ALBUMS, Asked, Browsed, FAVOURITES_AT_MOST, GONE_QUIET_FOR, Grew, Grown,
    HIDDEN_LISTED_AT_MOST, Heard, LOOK_AHEAD, LibraryModel, Loaded, MISSING_AT_MOST, MOST_LISTENED,
    PAGE, PICTURED_AT_MOST, PINNED_IN_THE_SIDEBAR, Paged, Prompted, ROOTS_LOOKED_AT_EVERY_UNSEEN,
    ROOTS_QUIET_FOR, ROOTS_READ_EVERY, Reached, Reading, Selection, Shelves, Standing,
    UNCOVERED_ROOTS_TRIED_AGAIN_AFTER, Wanted, WantsRead,
    pictures::{Picture, drawn_square, drawn_within, magnified_of},
    put_it_back, unnamed_playlist,
};
use crate::{held::Rows, toast, words};

#[derive(Default)]
pub(super) struct Watching {
    pub(super) watch: Option<RootsWatch>,
    pub(super) read_at: Option<Instant>,
    pub(super) laid_at: Option<Instant>,
    pub(super) uncovered_wait: Duration,
    pub(super) tried: Option<Vec<PathBuf>>,
    pub(super) there: Option<bool>,
    pub(super) gone: Vec<PathBuf>,
    pub(super) absent: Vec<PathBuf>,
    pub(super) returned: Vec<PathBuf>,
}

impl Watching {
    pub(super) fn is_due_to_read_the_roots(&self) -> bool {
        self.read_at
            .is_none_or(|at| at.elapsed() >= ROOTS_READ_EVERY)
    }

    pub(super) fn following(mut self, library: &Library) -> Self {
        self.read_at = Some(Instant::now());
        let roots = match library.roots() {
            Ok(roots) => roots,
            Err(error) => {
                tracing::debug!(%error, "the roots to watch could not be read");
                return self;
            }
        };
        let (present, absent): (Vec<PathBuf>, Vec<PathBuf>) =
            roots.into_iter().partition(|root| root.is_dir());
        self.there = Some(!present.is_empty());
        let to_cover_again = self
            .watch
            .as_ref()
            .is_some_and(RootsWatch::leaves_a_root_uncovered)
            && self.laid_at.is_some_and(|at| {
                at.elapsed() >= self.uncovered_wait.max(UNCOVERED_ROOTS_TRIED_AGAIN_AFTER)
            });
        if self.tried.as_deref() == Some(present.as_slice()) && self.watch.is_some() {
            if to_cover_again && let Some(watch) = self.watch.as_mut() {
                watch.cover_again();
                self.laid_at = Some(Instant::now());
                self.uncovered_wait =
                    watch.tried_again_after(self.uncovered_wait, UNCOVERED_ROOTS_TRIED_AGAIN_AFTER);
            }
            return self;
        }
        let laid = match self.watch.take() {
            Some(previous) => RootsWatch::replacing(previous, &present),
            None => RootsWatch::over(&present),
        };
        let Some(laid) = laid else {
            return self;
        };

        if self.tried.is_some() {
            let returned = self.absent.iter().filter(|root| present.contains(root));
            self.returned.extend(returned.cloned());
        }
        self.absent = absent;
        self.uncovered_wait =
            laid.tried_again_after(Duration::ZERO, UNCOVERED_ROOTS_TRIED_AGAIN_AFTER);
        self.watch = Some(laid);
        self.tried = Some(present);
        self.laid_at = Some(Instant::now());
        self
    }

    pub(super) fn came_back(&mut self) -> Vec<PathBuf> {
        mem::take(&mut self.returned)
    }

    pub(super) const fn has_read_the_roots(&self) -> bool {
        self.there.is_some()
    }

    pub(super) fn finds_a_root_there(&self) -> bool {
        self.there == Some(true)
    }

    pub(super) fn settled(&self) -> Vec<PathBuf> {
        self.watch
            .as_ref()
            .map(|watch| watch.settled(ROOTS_QUIET_FOR))
            .unwrap_or_default()
    }

    pub(super) fn forget_what_went(&mut self, library: &Library) -> u64 {
        if let Some(watch) = &self.watch {
            for path in watch.taken_away(GONE_QUIET_FOR) {
                if !self.gone.contains(&path) {
                    self.gone.push(path);
                }
            }
        }
        if self.gone.is_empty() {
            return 0;
        }

        match library.forget_the_gone(&self.gone) {
            Ok(forgotten) => {
                self.gone.clear();
                forgotten
            }
            Err(resonate_library::Error::AlreadyWalking) => 0,
            Err(error) => {
                tracing::warn!(%error, "the files taken away under the roots could not be forgotten yet");
                0
            }
        }
    }
}

pub(super) fn walk(
    library: &Library,
    roots: Vec<PathBuf>,
    reading: Reading,
    prompted: Prompted,
) -> resonate_library::Result<Option<ScanHandle>> {
    let workers = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let options = ScanOptions {
        roots,
        incremental: reading == Reading::WhatChanged,
        follow_symlinks: false,
        extract_cover_art: true,
        workers,
    };
    match prompted {
        Prompted::OnItsOwn => library.scan_what_is_held(options),
        Prompted::ByHand | Prompted::ByTheInbox => library.scan(options).map(Some),
    }
}

pub(super) fn load(
    library: &Library,
    mut asked: Asked,
    wanted: Wanted,
) -> resonate_library::Result<Loaded> {
    let meant = match asked.text.as_deref().filter(|_| !asked.as_typed) {
        Some(text) if asked.album.is_none() && asked.artist.is_none() => library.meant(text)?,
        Some(_) | None => None,
    };
    asked.meant = meant.as_ref().map(|meant| meant.searched.clone());
    let narrowing = asked.text.as_deref();

    let (browsed, grown) = match wanted {
        Wanted::ThePage => (None, Some(grown(library, &asked)?)),
        wanted if wanted.reads_the_listing() => (
            Some(browsed(library, &asked, wanted.reads_what_stands())?),
            None,
        ),
        _ => (None, None),
    };
    let shelves = match wanted.reads_the_shelves() {
        true => Some(shelves(
            library,
            &asked,
            narrowing,
            wanted.reads_the_wants(),
        )?),
        false => None,
    };

    Ok(Loaded {
        browsed,
        grown,
        shelves,
        meant,
    })
}

pub(super) fn shelves(
    library: &Library,
    asked: &Asked,
    narrowing: Option<&str>,
    reads_the_wants: bool,
) -> resonate_library::Result<Shelves> {
    let (held, entries) = match asked.opened {
        Some(opened) => (
            library.playlist(opened)?,
            library.playlist_entries(opened, narrowing)?,
        ),
        None => (None, Vec::new()),
    };
    let wants = reads_the_wants
        .then(|| {
            library
                .wants_as_they_stand()
                .map(|wants| WantsRead::of(&wants))
        })
        .transpose()?;

    let playlists = library.playlists(asked.order, asked.reading, narrowing)?;
    let pictured = pictures_of(library, playlists.iter().chain(held.as_ref()))?;

    Ok(Shelves {
        playlists,
        lists: library.playlist_lists(asked.order, asked.reading)?,
        pictured,
        pinned: library.pinned_playlists(PINNED_IN_THE_SIDEBAR)?,
        held,
        entries,
        wants,
        missing_tracks: library.missing_tracks(narrowing, Some(MISSING_AT_MOST))?,
        unheld_releases: library.unheld_releases(narrowing, Some(MISSING_AT_MOST))?,
        missing: library.missing_counted(narrowing)?,
        dismissed: library.dismissed()?,
    })
}

pub(super) fn brought_back(brought: Missing) -> String {
    let counted: [(_, fn(_) -> String); 2] = [
        (brought.tracks, words::models::missing_tracks),
        (brought.releases, words::models::unheld_releases),
    ];
    let parts: Vec<String> = counted
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, said)| said(count))
        .collect();

    match parts.as_slice() {
        [] => words::models::nothing_was_dismissed().to_owned(),
        [only] => words::models::brought_back(only),
        [one, other, ..] => words::models::brought_back(words::models::both(one, other)),
    }
}

fn pictures_of<'a>(
    library: &Library,
    playlists: impl Iterator<Item = &'a Playlist>,
) -> resonate_library::Result<AHashMap<PlaylistId, Arc<[AlbumId]>>> {
    let mut pictured = AHashMap::new();
    for playlist in playlists {
        if pictured.contains_key(&playlist.id) {
            continue;
        }
        let albums = library.playlist_pictures(playlist.id, PICTURED_AT_MOST)?;
        pictured.insert(playlist.id, Arc::from(albums));
    }
    Ok(pictured)
}

pub(super) fn tell_of_listen_time(
    this: &WeakEntity<LibraryModel>,
    error: &resonate_library::Error,
    cx: &mut AsyncApp,
) {
    let told = this.update(cx, |_, cx| {
        toast::tell(
            toast::could_not(words::models::keep_how_long_that_was_heard(), error),
            cx,
        );
    });
    let _ = told;
}

pub(super) fn picture_or_trouble(
    read: Option<resonate_library::Result<Option<Picture>>>,
    doing: &str,
    cx: &mut App,
) -> Option<Picture> {
    match read? {
        Ok(picture) => picture,
        Err(error) => {
            tracing::warn!(%error, doing, "a picture could not be read");
            toast::tell(toast::could_not(doing, &error), cx);
            None
        }
    }
}

pub(super) fn drawn_cover(
    library: &Library,
    id: AlbumId,
    side: NonZeroU32,
) -> resonate_library::Result<Option<Picture>> {
    let art = library.cover_art(id)?;
    Ok(art.as_ref().and_then(|art| drawn_within(art, side)))
}

pub(super) fn whole_cover_of(
    library: &Library,
    id: AlbumId,
) -> resonate_library::Result<Option<Picture>> {
    let art = library.cover_art(id)?;
    Ok(art.as_ref().and_then(magnified_of))
}

pub(super) fn drawn_portrait(
    library: &Library,
    id: ArtistId,
    side: NonZeroU32,
) -> resonate_library::Result<Option<Picture>> {
    let art = library.portrait(id)?;
    Ok(art.as_ref().and_then(|art| drawn_square(art, side)))
}

impl Asked {
    pub(super) fn searched(&self) -> Option<String> {
        self.meant.clone().or_else(|| self.text.clone())
    }

    pub(super) fn listing(
        &self,
        album: Option<AlbumId>,
        artist: Option<ArtistId>,
        limit: Option<usize>,
    ) -> TrackQuery {
        TrackQuery {
            album,
            artist,
            text: self.searched(),
            sort: self.sorting.tracks,
            reading: self.sorting.tracks_read,
            limit,
            offset: 0,
        }
    }

    fn scoping(&self) -> bool {
        self.album.is_some() || self.artist.is_some()
    }

    pub(super) fn albums(&self) -> AlbumQuery {
        AlbumQuery {
            artist: None,
            text: self.searched(),
            sort: self.sorting.albums,
            reading: self.sorting.albums_read,
            limit: Some(self.reach),
            offset: 0,
        }
    }

    pub(super) fn artists(&self) -> ArtistQuery {
        ArtistQuery {
            text: self.searched(),
            sort: self.sorting.artists,
            reading: self.sorting.artists_read,
            limit: Some(self.reach),
            offset: 0,
        }
    }
}

pub(super) fn grown(library: &Library, asked: &Asked) -> resonate_library::Result<Grown> {
    let from = asked.held;
    let overlapping = |held: usize| held.saturating_sub(1);
    let limit = |held: usize| Some(asked.reach.saturating_sub(overlapping(held)).max(1));
    let tracks_after = |album, artist, reached: Reached| {
        let query = TrackQuery {
            offset: overlapping(reached.rows),
            ..asked.listing(album, artist, limit(reached.rows))
        };
        match reached.last {
            Some(last) => library.tracks_from(&query, last),
            None => library.tracks(&query),
        }
    };

    Ok(Grown {
        from,
        page: Paged {
            albums: library.albums(&AlbumQuery {
                limit: limit(from.albums),
                offset: overlapping(from.albums),
                ..asked.albums()
            })?,
            artists: library.artists(&ArtistQuery {
                limit: limit(from.artists),
                offset: overlapping(from.artists),
                ..asked.artists()
            })?,
            tracks: tracks_after(None, None, from.tracks)?,
            scoped: match asked.scoping() {
                true => tracks_after(asked.album, asked.artist, from.scoped)?,
                false => Vec::new(),
            },
        },
    })
}

pub(super) fn refreshed_where_due(
    library: &Library,
    reference: &dyn Reference,
) -> Vec<(PlaylistId, resonate_library::Result<Option<FromTheLink>>)> {
    let mut read = Vec::new();
    match library.profiles_due_a_refresh(SystemTime::now()) {
        Ok(due) => {
            for profile in due {
                match library.follow_profile(reference, &profile, Upkeep::InStep) {
                    Ok(Some(followed)) => read.extend(
                        followed
                            .playlists
                            .into_iter()
                            .map(|from| (from.playlist, Ok(Some(from)))),
                    ),
                    Ok(None) => {
                        tracing::info!(profile = %profile.page(), "a followed profile names nothing now");
                    }
                    Err(error) => {
                        tracing::warn!(%error, profile = %profile.page(), "a followed profile went unread");
                    }
                }
            }
        }
        Err(error) => tracing::warn!(%error, "the profiles followed went unread"),
    }
    match library.playlists_due_a_refresh(SystemTime::now()) {
        Ok(due) => read.extend(
            due.into_iter()
                .map(|playlist| (playlist, library.refresh_playlist(reference, playlist))),
        ),
        Err(error) => {
            tracing::warn!(%error, "the playlists kept in step with a link went unread");
        }
    }
    read
}

pub(super) fn link_told(name: &str, added: usize, removed: usize, wanted: usize) -> String {
    let told = match (added, removed) {
        (0, 0) => words::models::as_its_link_has_it(name),
        (added, 0) => words::models::added_songs_to(added, name),
        (0, removed) => words::models::took_songs_out_as_its_link_did(name, removed),
        (added, removed) => words::models::added_and_took_out(added, name, removed),
    };
    match wanted {
        0 => told,
        wanted => [told, words::models::downloading_more(wanted)].join(" · "),
    }
}

pub(super) fn put_into(
    library: &Library,
    id: PlaylistId,
    added: usize,
) -> resonate_library::Result<String> {
    let name = library
        .playlist(id)?
        .map_or_else(|| unnamed_playlist(id), |found| found.name);

    Ok(match added {
        0 => words::models::nothing_went_into(name),
        added => words::models::put_tracks_in(added, name, put_it_back()),
    })
}

pub(super) fn workers_beside(playing: bool) -> Option<NonZeroUsize> {
    if !playing {
        return None;
    }
    let cores = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    NonZeroUsize::new(cores / 2).or(Some(NonZeroUsize::MIN))
}

pub(super) fn roots_looked_at_now(seen: bool, looked_at: Option<Instant>) -> bool {
    seen || looked_at.is_none_or(|at| at.elapsed() >= ROOTS_LOOKED_AT_EVERY_UNSEEN)
}

pub(super) fn reach_covering(reach: usize, drawn_to: usize) -> usize {
    let wanted = drawn_to
        .saturating_add(LOOK_AHEAD)
        .div_ceil(PAGE)
        .saturating_mul(PAGE);
    wanted.max(reach.saturating_add(PAGE))
}

pub(super) fn grew<T: PartialEq>(held: &mut Rows<T>, from: usize, read: Vec<T>) -> Grew {
    if from == 0 {
        return match rows_renewed(held, read) {
            true => Grew::Renewed,
            false => Grew::Same,
        };
    }
    if held.len() != from {
        return Grew::Moved;
    }
    let mut read = read.into_iter();
    let Some(overlap) = read.next() else {
        return Grew::Moved;
    };
    if held.last() != Some(&overlap) {
        return Grew::Moved;
    }
    let after: Vec<T> = read.collect();
    if after.is_empty() {
        return Grew::Same;
    }
    *held = held.appended(after);
    Grew::Renewed
}

pub(super) fn rows_renewed<T: PartialEq>(held: &mut Rows<T>, read: Vec<T>) -> bool {
    if *held == *read {
        return false;
    }
    *held = Rows::from(read);
    true
}

pub(super) fn paged(library: &Library, asked: &Asked) -> resonate_library::Result<Paged> {
    let reach = Some(asked.reach);
    Ok(Paged {
        albums: library.albums(&asked.albums())?,
        artists: library.artists(&asked.artists())?,
        tracks: library.tracks(&asked.listing(None, None, reach))?,
        scoped: match asked.scoping() {
            true => library.tracks(&asked.listing(asked.album, asked.artist, reach))?,
            false => Vec::new(),
        },
    })
}

pub(super) fn standing(
    library: &Library,
    window: Window,
    heard: bool,
) -> resonate_library::Result<Standing> {
    Ok(Standing {
        statistics: library.statistics(window)?,
        heard: match heard {
            true => Some(what_was_heard(library, window)?),
            false => None,
        },
        suggestions: library.suggestions()?,
        roots: library.roots()?,
        hidden: library.hidden_tracks(HIDDEN_LISTED_AT_MOST)?,
    })
}

pub(super) fn what_was_heard(library: &Library, window: Window) -> resonate_library::Result<Heard> {
    let (by_day, habits) = library.days_and_habits(window)?;

    Ok(Heard {
        most_listened: library.most_listened(window, MOST_LISTENED)?,
        by_day,
        habits,
    })
}

pub(super) fn browsed(
    library: &Library,
    asked: &Asked,
    with_what_stands: bool,
) -> resonate_library::Result<Browsed> {
    let text = asked.text.as_deref();
    let (album, artist) = (asked.album, asked.artist);
    let albums_asked = asked.albums();
    let artists_asked = asked.artists();
    let favourite_albums_asked = AlbumQuery {
        sort: AlbumOrder::Favourited,
        reading: AlbumOrder::Favourited.reads(),
        limit: Some(FAVOURITES_AT_MOST),
        ..albums_asked.clone()
    };
    let favourite_artists_asked = ArtistQuery {
        sort: ArtistOrder::Favourited,
        reading: ArtistOrder::Favourited.reads(),
        limit: Some(FAVOURITES_AT_MOST),
        ..artists_asked.clone()
    };
    let favourite_tracks_asked = TrackQuery {
        sort: asked.sorting.favourites,
        reading: asked.sorting.favourites_read,
        ..asked.listing(None, None, Some(FAVOURITES_AT_MOST))
    };

    let tracks_measured = library.measured(&asked.listing(None, None, None))?;
    let sung = match text {
        Some(text) => library.sung(text)?,
        None => None,
    };
    let paged = paged(library, asked)?;
    let matched_nothing = paged.albums.is_empty()
        && paged.artists.is_empty()
        && paged.tracks.is_empty()
        && sung.is_none();
    let held_album = match album {
        Some(album) => library.album(album)?,
        None => None,
    };
    let held_artist = match artist {
        Some(artist) => library.artist_detail(artist)?,
        None => None,
    };
    let held_album_is_gone = album.is_some() && held_album.is_none();
    let held_artist_is_gone = artist.is_some() && held_artist.is_none();

    Ok(Browsed {
        instead: match text.filter(|_| matched_nothing) {
            Some(text) => library.did_you_mean(text)?,
            None => None,
        },
        albums_counted: library.albums_counted(&albums_asked)?,
        artists_counted: library.artists_counted(&artists_asked)?,
        tracks_measured,
        scoped_measured: match asked.scoping() {
            true => library.measured(&asked.listing(album, artist, None))?,
            false => tracks_measured,
        },
        favourite_albums: library.favourite_albums(&favourite_albums_asked)?,
        favourite_artists: library.favourite_artists(&favourite_artists_asked)?,
        favourite_tracks: library.favourite_tracks(&favourite_tracks_asked)?,
        standing: match with_what_stands {
            true => Some(standing(library, asked.window, asked.heard)?),
            false => None,
        },
        sung,
        kept_songs: match text.filter(|_| album.is_none() && artist.is_none()) {
            Some(text) => library.songs_kept_for(text)?,
            None => Vec::new(),
        },
        kept_albums: match text.filter(|_| album.is_none() && artist.is_none()) {
            Some(text) => library.albums_kept_for(text)?,
            None => Vec::new(),
        },
        kept_for: text.map(str::to_owned),
        songs_not_held: match artist {
            Some(artist) => library.songs_not_held_by(artist)?,
            None => Vec::new(),
        },
        albums_not_held: match artist {
            Some(artist) => library.albums_not_held_by(artist)?,
            None => Vec::new(),
        },
        paged,
        release_tracks: match album {
            Some(album) => library.release_tracks(album)?,
            None => Vec::new(),
        },
        release: match album {
            Some(album) => library.release_of(album)?,
            None => None,
        },
        artist: held_artist,
        artist_albums: match artist {
            Some(artist) => library.albums(&AlbumQuery {
                artist: Some(artist),
                text: None,
                sort: AlbumOrder::Year,
                reading: AlbumOrder::Year.reads(),
                limit: Some(ARTIST_ALBUMS),
                offset: 0,
            })?,
            None => Vec::new(),
        },
        artist_totals: match artist {
            Some(artist) => library.artist_totals(artist)?,
            None => ArtistTotals::default(),
        },
        album: held_album,
        gone: match (album, artist) {
            (Some(album), _) if held_album_is_gone => Some(Selection::Album(album)),
            (None, Some(artist)) if held_artist_is_gone => Some(Selection::Artist(artist)),
            _ => None,
        },
    })
}
