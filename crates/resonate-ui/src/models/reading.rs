use std::{
    mem,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use gpui::Context;
use resonate_core::AlbumId;
use resonate_engine::QueueItem;
use resonate_library::{Measured, Search, Track};

use super::{
    ALBUMS_HELD, Asked, Browsed, COVERS_WARMED, Forget, Grew, Grown, Heard, Held, LibraryModel,
    Loaded, Paged, Reached, Selection, Shelves, Wanted, WantsRead,
    arranged::{Named, missing_track_rows, queued_rows, unheld_release_rows},
    pictures::{AtSide, Drawn, renewed},
    watched::{drawn_cover, grew, load, picture_or_trouble, rows_renewed},
};
use crate::{
    drawing::Drawer,
    held::Rows,
    recent::Recent,
    toast,
    views::statistics::{BARS_AT_MOST, Chart},
    words,
};

impl LibraryModel {
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.forget_what_the_catalog_may_have_changed();
        self.name_what_is_held_again(cx);
        self.read(Wanted::Everything, cx);
    }

    fn forget_albums_the_catalog_moved_under(&mut self) {
        let now = self.library.rows_stamp();
        if !self
            .read_albums_at
            .is_some_and(|held| held.still_holds_at(now))
        {
            self.read_albums = Recent::new(ALBUMS_HELD);
        }
        self.read_albums_at = Some(now);
    }

    fn forget_what_the_catalog_may_have_changed(&mut self) {
        self.covers.forget_where(Option::is_none);
        self.portraits.forget_where(Option::is_none);
        self.read_albums.forget_where(|_| true);
    }

    fn name_what_is_held_again(&mut self, cx: &mut Context<Self>) {
        let held: Vec<(QueueItem, Option<Track>)> = self
            .named
            .entries()
            .map(|(id, named)| {
                let item = QueueItem {
                    id: *id,
                    location: named.location.clone(),
                    span: named.span,
                };
                (item, named.track.clone())
            })
            .collect();
        if held.is_empty() {
            return;
        }

        let library = Arc::clone(&self.library);
        self._naming_again = cx.spawn(async move |this, cx| {
            let (held, read) = cx
                .background_executor()
                .spawn(async move {
                    let items: Vec<QueueItem> = held.iter().map(|(item, _)| item.clone()).collect();
                    let read = queued_rows(&library, &items);
                    (held, read)
                })
                .await;
            let landed = this.update(cx, |this, cx| {
                let mut changed = false;
                for ((item, before), track) in held.into_iter().zip(read) {
                    let standing = this
                        .named
                        .peek(&item.id)
                        .is_some_and(|now| now.names(&item) && now.track == before);
                    if standing && track != before {
                        this.named.insert(
                            item.id,
                            Named {
                                location: item.location,
                                span: item.span,
                                track,
                            },
                        );
                        changed = true;
                    }
                }
                if changed {
                    this.revision = this.revision.wrapping_add(1);
                    cx.notify();
                }
            });
            let _ = landed;
        });
    }

    pub fn reload_playlists(&mut self, cx: &mut Context<Self>) {
        self.read(Wanted::ThePlaylists, cx);
    }

    pub(super) fn read(&mut self, wanted: Wanted, cx: &mut Context<Self>) {
        self.read_after(Duration::ZERO, wanted, cx);
    }

    pub(super) fn asked(&self) -> Asked {
        let (album, artist) = match self.selection {
            Selection::Everything => (None, None),
            Selection::Album(album) => (Some(album), None),
            Selection::Artist(artist) => (None, Some(artist)),
        };
        Asked {
            text: (!self.query.is_empty()).then(|| self.query.clone()),
            meant: None,
            as_typed: self.as_typed,
            album,
            artist,
            opened: self.opened,
            order: self.order,
            reading: self.reading,
            sorting: self.sorting,
            reach: self.reach,
            held: match self.page_moved {
                true => Held::default(),
                false => Held {
                    albums: self.albums.len(),
                    artists: self.artists.len(),
                    tracks: Reached::of(&self.tracks),
                    scoped: Reached::of(&self.scoped),
                },
            },
            window: self.window,
            heard: self.statistics_shown,
        }
    }

    pub(super) fn read_after(
        &mut self,
        settling: Duration,
        wanted: Wanted,
        cx: &mut Context<Self>,
    ) {
        let wanted = self
            .in_flight
            .map_or(wanted, |reading| reading.with(wanted));
        self.in_flight = Some(wanted);
        let library = Arc::clone(&self.library);
        let asked = self.asked();
        let asked_for = self.query.clone();
        let read_ahead = self.first_read.take().filter(|first| {
            wanted == Wanted::Everything && settling.is_zero() && first.asked == asked
        });
        if let Some(first) = read_ahead.as_ref()
            && let Ok(read) = first.read.try_recv()
        {
            self.in_flight = None;
            self.landed(read, asked_for, cx);
            return;
        }

        self._load = cx.spawn(async move |this, cx| {
            if !settling.is_zero() {
                cx.background_executor().timer(settling).await;
            }
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    read_ahead
                        .and_then(|first| first.read.recv().ok())
                        .unwrap_or_else(|| load(&library, asked, wanted))
                })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.in_flight = None;
                this.landed(loaded, asked_for, cx);
            });
            let _ = outcome;
        });
    }

    pub(super) fn landed(
        &mut self,
        loaded: resonate_library::Result<Loaded>,
        asked_for: String,
        cx: &mut Context<Self>,
    ) {
        match loaded {
            Ok(loaded) => {
                self.unreadable = false;
                if loaded.browsed.is_some() {
                    self.listed_for = asked_for;
                }
                let shelved = loaded.shelves.is_some();
                let gone = loaded.browsed.as_ref().and_then(|browsed| browsed.gone);
                if self.take(loaded) == Grew::Moved {
                    self.read(Wanted::ThePage, cx);
                }
                for (written, favourite) in mem::take(&mut self.favours_written) {
                    if self.favoured.get(&written) == Some(&favourite) {
                        self.favoured.remove(&written);
                    }
                }
                if gone == Some(self.selection) {
                    self.select(Selection::Everything, cx);
                }
                self.take_down_what_moved();
                self.warm_the_covers(cx);
                if shelved {
                    self.ask_when_due(cx);
                }
            }
            Err(error) => {
                tracing::error!(%error, "the library could not be read");
                self.unreadable = true;
                if self.listed_for != asked_for {
                    self.forget_the_listing();
                    self.listed_for = asked_for;
                }
                toast::tell(
                    toast::could_not(words::models::read_the_library(), &error),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(super) fn take(&mut self, loaded: Loaded) -> Grew {
        self.revision = self.revision.wrapping_add(1);
        self.forget_albums_the_catalog_moved_under();
        if let Some(shelves) = loaded.shelves {
            self.take_the_shelves(shelves);
        }
        if loaded.browsed.is_some() || loaded.grown.is_some() {
            self.search = Search::read(
                loaded
                    .meant
                    .as_ref()
                    .map_or(self.query.as_str(), |meant| meant.searched.as_str()),
            );
            self.meant = loaded.meant;
        }
        if let Some(browsed) = loaded.browsed {
            self.page_moved = false;
            self.take_the_listing(browsed);
            return Grew::Renewed;
        }
        let Some(grown) = loaded.grown else {
            return Grew::Same;
        };
        let grew = self.take_the_growth(grown);
        match grew {
            Grew::Same => {}
            Grew::Moved => self.page_moved = true,
            Grew::Renewed => {
                self.index_the_favourites();
                self.restate_the_listing();
                self.ask_about_what_is_drawn();
            }
        }
        grew
    }

    fn take_the_wants(&mut self, wants: WantsRead) {
        self.wanted = wants.wanted;
        self.next_try = wants.next_try;
        if !mem::replace(&mut self.downloads_restored, true) {
            self.downloads.restored(wants.unfinished);
        }
        self.downloads.followed(&wants.standings);
        if let Some(track) = self
            .downloads
            .landed_to_play(&wants.standings, Instant::now())
        {
            self.arrived = Some(track);
        }
        self.standings = wants.standings;
    }

    fn take_the_shelves(&mut self, shelves: Shelves) {
        if let Some(wants) = shelves.wants {
            self.take_the_wants(wants);
        }
        if renewed(&mut self.missing_tracks, shelves.missing_tracks) {
            self.missing_track_rows = missing_track_rows(
                self.missing_tracks
                    .iter()
                    .map(|track| (track.album, track.disc)),
            )
            .into();
        }
        if renewed(&mut self.unheld_releases, shelves.unheld_releases) {
            self.unheld_release_rows =
                unheld_release_rows(self.unheld_releases.iter().map(|release| release.artist))
                    .into();
        }
        self.missing = shelves.missing;
        self.dismissed = shelves.dismissed;
        renewed(&mut self.playlists, shelves.playlists);
        renewed(&mut self.lists, shelves.lists);
        self.pictured = shelves.pictured;
        renewed(&mut self.pinned, shelves.pinned);
        self.held = shelves.held;
        renewed(&mut self.entries, shelves.entries);
        if self.held.is_none() {
            self.opened = None;
        }
    }

    fn take_the_growth(&mut self, grown: Grown) -> Grew {
        let Grown { from, page } = grown;
        let albums = grew(&mut self.albums, from.albums, page.albums);
        if albums == Grew::Renewed {
            self.index_the_albums();
        }
        let grew = albums
            .and(grew(&mut self.artists, from.artists, page.artists))
            .and(grew(&mut self.tracks, from.tracks.rows, page.tracks))
            .and(grew(&mut self.scoped, from.scoped.rows, page.scoped));
        if from == Held::default() {
            self.page_moved = false;
        }
        grew
    }

    fn forget_the_listing(&mut self) {
        self.albums = Rows::default();
        self.artists = Rows::default();
        self.tracks = Rows::default();
        self.scoped = Rows::default();
        self.albums_counted = 0;
        self.artists_counted = 0;
        self.tracks_measured = Measured::default();
        self.scoped_measured = Measured::default();
        self.index_the_albums();
        self.restate_the_listing();
    }

    fn take_the_page(&mut self, paged: Paged) {
        if rows_renewed(&mut self.albums, paged.albums) {
            self.index_the_albums();
        }
        rows_renewed(&mut self.artists, paged.artists);
        rows_renewed(&mut self.tracks, paged.tracks);
        rows_renewed(&mut self.scoped, paged.scoped);
    }

    pub(super) fn take_what_was_heard(&mut self, heard: Heard) {
        self.most_listened = Arc::new(heard.most_listened);
        if renewed(&mut self.by_day, heard.by_day) {
            self.charted = Arc::new(Chart::of(&self.by_day, SystemTime::now(), BARS_AT_MOST));
        }
        self.habits = Arc::new(heard.habits);
    }

    pub(crate) fn statistics_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.statistics_shown = shown;
        if shown && self.heard_owed {
            self.read(Wanted::Everything, cx);
        }
    }

    fn take_the_listing(&mut self, browsed: Browsed) {
        self.take_the_page(browsed.paged);
        renewed(&mut self.favourite_albums, browsed.favourite_albums);
        renewed(&mut self.favourite_artists, browsed.favourite_artists);
        renewed(&mut self.favourite_tracks, browsed.favourite_tracks);
        if let Some(standing) = browsed.standing {
            self.statistics = standing.statistics;
            self.heard_owed = standing.heard.is_none();
            if let Some(heard) = standing.heard {
                self.take_what_was_heard(heard);
            }
            self.suggestions = standing.suggestions;
            self.roots = standing.roots;
            if self.hidden.as_ref() != &standing.hidden {
                self.hidden = Arc::new(standing.hidden);
            }
        }
        self.sung = browsed.sung;
        renewed(&mut self.kept_songs, browsed.kept_songs);
        renewed(&mut self.kept_albums, browsed.kept_albums);
        self.kept_for = browsed.kept_for;
        renewed(&mut self.songs_not_held, browsed.songs_not_held);
        renewed(&mut self.albums_not_held, browsed.albums_not_held);
        self.albums_counted = browsed.albums_counted;
        self.artists_counted = browsed.artists_counted;
        self.tracks_measured = browsed.tracks_measured;
        self.scoped_measured = browsed.scoped_measured;
        renewed(&mut self.release_tracks, browsed.release_tracks);
        self.release = browsed.release;
        self.artist = browsed.artist;
        renewed(&mut self.artist_albums, browsed.artist_albums);
        self.artist_totals = browsed.artist_totals;
        self.album = browsed.album;
        self.instead = browsed.instead;
        self.index_the_favourites();
        self.restate_the_listing();
        self.ask_about_what_is_drawn();
    }

    fn index_the_albums(&mut self) {
        self.album_at.clear();
        for (at, album) in self.albums.iter().enumerate() {
            self.album_at.insert(album.id, at);
        }
    }

    fn index_the_favourites(&mut self) {
        self.favourite_album_ids.clear();
        self.favourite_artist_ids.clear();

        for album in self
            .album
            .iter()
            .chain(self.albums.iter())
            .chain(self.favourite_albums.iter())
            .filter(|album| album.favourite.is_some())
        {
            self.favourite_album_ids.insert(album.id);
        }
        for artist in self
            .artists
            .iter()
            .chain(self.favourite_artists.iter())
            .filter(|artist| artist.favourite.is_some())
        {
            self.favourite_artist_ids.insert(artist.id);
        }
    }

    fn warm_the_covers(&mut self, cx: &mut Context<Self>) {
        if self.warmed {
            return;
        }
        self.warmed = true;

        let wanted: Vec<AlbumId> = self
            .albums
            .iter()
            .filter(|album| album.has_cover_art)
            .map(|album| album.id)
            .take(COVERS_WARMED)
            .collect();
        if wanted.is_empty() {
            self.warmed = false;
            return;
        }

        let library = Arc::clone(&self.library);
        self._warming = cx.spawn(async move |this, cx| {
            for id in wanted {
                let Ok(sized) = this.update(cx, |this, _| {
                    let sized = AtSide {
                        key: id,
                        side: Drawn::InAGrid.side(this.scale),
                    };
                    (!this.covers.holds(&sized) && !this.decoding.holds(&sized)).then_some(sized)
                }) else {
                    return;
                };
                let Some(sized) = sized else {
                    continue;
                };

                let reading = Arc::clone(&library);
                let side = sized.side;
                let Ok(drawing) = this.update(cx, |_, cx| {
                    cx.global::<Drawer>()
                        .draw(move || drawn_cover(&reading, id, side))
                }) else {
                    return;
                };
                let read = drawing.await;

                let landed = this.update(cx, |this, cx| {
                    let decoded = picture_or_trouble(read, words::models::read_a_cover(), cx);
                    this.covers.insert(sized, decoded).forget(cx);
                    cx.notify();
                });
                if landed.is_err() {
                    return;
                }
            }
        });
    }
}
