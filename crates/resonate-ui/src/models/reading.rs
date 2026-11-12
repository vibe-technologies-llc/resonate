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
            .naming
            .read_albums_at
            .is_some_and(|held| held.still_holds_at(now))
        {
            self.naming.read_albums = Recent::new(ALBUMS_HELD);
        }
        self.naming.read_albums_at = Some(now);
    }

    fn forget_what_the_catalog_may_have_changed(&mut self) {
        self.pictures.covers.forget_where(Option::is_none);
        self.pictures.portraits.forget_where(Option::is_none);
        self.naming.read_albums.forget_where(|_| true);
    }

    fn name_what_is_held_again(&mut self, cx: &mut Context<Self>) {
        let held: Vec<(QueueItem, Option<Track>)> = self
            .naming
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
        self.naming._again = cx.spawn(async move |this, cx| {
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
                        .naming
                        .named
                        .peek(&item.id)
                        .is_some_and(|now| now.names(&item) && now.track == before);
                    if standing && track != before {
                        this.naming.named.insert(
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
            as_typed: self.outreach.as_typed,
            album,
            artist,
            opened: self.shelf.opened,
            order: self.shelf.order,
            reading: self.shelf.reading,
            sorting: self.sorting,
            reach: self.browse.reach,
            held: match self.browse.page_moved {
                true => Held::default(),
                false => Held {
                    albums: self.browse.albums.len(),
                    artists: self.browse.artists.len(),
                    tracks: Reached::of(&self.browse.tracks),
                    scoped: Reached::of(&self.browse.scoped),
                },
            },
            window: self.window,
            heard: self.charts.shown,
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
                for (written, favourite) in mem::take(&mut self.favours.written) {
                    if self.favours.favoured.get(&written) == Some(&favourite) {
                        self.favours.favoured.remove(&written);
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
            self.outreach.meant = loaded.meant;
        }
        if let Some(browsed) = loaded.browsed {
            self.browse.page_moved = false;
            self.take_the_listing(browsed);
            return Grew::Renewed;
        }
        let Some(grown) = loaded.grown else {
            return Grew::Same;
        };
        let grew = self.take_the_growth(grown);
        match grew {
            Grew::Same => {}
            Grew::Moved => self.browse.page_moved = true,
            Grew::Renewed => {
                self.index_the_favourites();
                self.restate_the_listing();
                self.ask_about_what_is_drawn();
            }
        }
        grew
    }

    fn take_the_wants(&mut self, wants: WantsRead) {
        self.wishes.wanted = wants.wanted;
        self.outreach.next_try = wants.next_try;
        if !mem::replace(&mut self.delivered.restored, true) {
            self.delivered.downloads.restored(wants.unfinished);
        }
        self.delivered.downloads.followed(&wants.standings);
        if let Some(track) = self
            .delivered
            .downloads
            .landed_to_play(&wants.standings, Instant::now())
        {
            self.arrived = Some(track);
        }
        self.wishes.standings = wants.standings;
    }

    fn take_the_shelves(&mut self, shelves: Shelves) {
        if let Some(wants) = shelves.wants {
            self.take_the_wants(wants);
        }
        if renewed(&mut self.gaps.tracks, shelves.missing_tracks) {
            self.gaps.track_rows = missing_track_rows(
                self.gaps
                    .tracks
                    .iter()
                    .map(|track| (track.album, track.disc)),
            )
            .into();
        }
        if renewed(&mut self.gaps.releases, shelves.unheld_releases) {
            self.gaps.release_rows =
                unheld_release_rows(self.gaps.releases.iter().map(|release| release.artist)).into();
        }
        self.gaps.missing = shelves.missing;
        self.gaps.dismissed = shelves.dismissed;
        renewed(&mut self.shelf.playlists, shelves.playlists);
        renewed(&mut self.shelf.lists, shelves.lists);
        self.shelf.pictured = shelves.pictured;
        renewed(&mut self.shelf.pinned, shelves.pinned);
        self.shelf.held = shelves.held;
        renewed(&mut self.shelf.entries, shelves.entries);
        if self.shelf.held.is_none() {
            self.shelf.opened = None;
        }
    }

    fn take_the_growth(&mut self, grown: Grown) -> Grew {
        let Grown { from, page } = grown;
        let albums = grew(&mut self.browse.albums, from.albums, page.albums);
        if albums == Grew::Renewed {
            self.index_the_albums();
        }
        let grew = albums
            .and(grew(&mut self.browse.artists, from.artists, page.artists))
            .and(grew(&mut self.browse.tracks, from.tracks.rows, page.tracks))
            .and(grew(&mut self.browse.scoped, from.scoped.rows, page.scoped));
        if from == Held::default() {
            self.browse.page_moved = false;
        }
        grew
    }

    fn forget_the_listing(&mut self) {
        self.browse.albums = Rows::default();
        self.browse.artists = Rows::default();
        self.browse.tracks = Rows::default();
        self.browse.scoped = Rows::default();
        self.browse.albums_counted = 0;
        self.browse.artists_counted = 0;
        self.browse.tracks_measured = Measured::default();
        self.browse.scoped_measured = Measured::default();
        self.index_the_albums();
        self.restate_the_listing();
    }

    fn take_the_page(&mut self, paged: Paged) {
        if rows_renewed(&mut self.browse.albums, paged.albums) {
            self.index_the_albums();
        }
        rows_renewed(&mut self.browse.artists, paged.artists);
        rows_renewed(&mut self.browse.tracks, paged.tracks);
        rows_renewed(&mut self.browse.scoped, paged.scoped);
    }

    pub(super) fn take_what_was_heard(&mut self, heard: Heard) {
        self.charts.most_listened = Arc::new(heard.most_listened);
        if renewed(&mut self.charts.by_day, heard.by_day) {
            self.charts.charted = Arc::new(Chart::of(
                &self.charts.by_day,
                SystemTime::now(),
                BARS_AT_MOST,
            ));
        }
        self.charts.habits = Arc::new(heard.habits);
    }

    pub(crate) fn statistics_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.charts.shown = shown;
        if shown && self.charts.owed {
            self.read(Wanted::Everything, cx);
        }
    }

    fn take_the_listing(&mut self, browsed: Browsed) {
        self.take_the_page(browsed.paged);
        renewed(&mut self.favours.albums, browsed.favourite_albums);
        renewed(&mut self.favours.artists, browsed.favourite_artists);
        renewed(&mut self.favours.tracks, browsed.favourite_tracks);
        if let Some(standing) = browsed.standing {
            self.charts.statistics = standing.statistics;
            self.charts.owed = standing.heard.is_none();
            if let Some(heard) = standing.heard {
                self.take_what_was_heard(heard);
            }
            self.suggestions = standing.suggestions;
            self.roots = standing.roots;
            if self.hidden.as_ref() != &standing.hidden {
                self.hidden = Arc::new(standing.hidden);
            }
        }
        self.browse.sung = browsed.sung;
        renewed(&mut self.unheld.kept_songs, browsed.kept_songs);
        renewed(&mut self.unheld.kept_albums, browsed.kept_albums);
        self.unheld.kept_for = browsed.kept_for;
        renewed(&mut self.unheld.songs, browsed.songs_not_held);
        renewed(&mut self.unheld.albums, browsed.albums_not_held);
        self.browse.albums_counted = browsed.albums_counted;
        self.browse.artists_counted = browsed.artists_counted;
        self.browse.tracks_measured = browsed.tracks_measured;
        self.browse.scoped_measured = browsed.scoped_measured;
        renewed(&mut self.detail.release_tracks, browsed.release_tracks);
        self.detail.release = browsed.release;
        self.detail.artist = browsed.artist;
        renewed(&mut self.detail.artist_albums, browsed.artist_albums);
        self.detail.artist_totals = browsed.artist_totals;
        self.detail.album = browsed.album;
        self.instead = browsed.instead;
        self.index_the_favourites();
        self.restate_the_listing();
        self.ask_about_what_is_drawn();
    }

    fn index_the_albums(&mut self) {
        self.browse.album_at.clear();
        for (at, album) in self.browse.albums.iter().enumerate() {
            self.browse.album_at.insert(album.id, at);
        }
    }

    fn index_the_favourites(&mut self) {
        self.favours.album_ids.clear();
        self.favours.artist_ids.clear();

        for album in self
            .detail
            .album
            .iter()
            .chain(self.browse.albums.iter())
            .chain(self.favours.albums.iter())
            .filter(|album| album.favourite.is_some())
        {
            self.favours.album_ids.insert(album.id);
        }
        for artist in self
            .browse
            .artists
            .iter()
            .chain(self.favours.artists.iter())
            .filter(|artist| artist.favourite.is_some())
        {
            self.favours.artist_ids.insert(artist.id);
        }
    }

    fn warm_the_covers(&mut self, cx: &mut Context<Self>) {
        if self.warmed {
            return;
        }
        self.warmed = true;

        let wanted: Vec<AlbumId> = self
            .browse
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
                        side: Drawn::InAGrid.side(this.pictures.scale),
                    };
                    (!this.pictures.covers.holds(&sized) && !this.pictures.decoding.holds(&sized))
                        .then_some(sized)
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
                    this.pictures.covers.insert(sized, decoded).forget(cx);
                    cx.notify();
                });
                if landed.is_err() {
                    return;
                }
            }
        });
    }
}
