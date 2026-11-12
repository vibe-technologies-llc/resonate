use std::{borrow::Cow, sync::Arc, time::Duration};

use gpui::{Context, Task};
use resonate_library::{
    AlbumFound, AlbumMatch, AlbumNotHeld, ArtistFound, Found, RecordingMatch, SongsAsked, Sung,
    albums_still_answering, artists_still_answering, songs_asked, still_answering, weighed_for,
};

use super::{
    Answer, Beyond, LibraryModel, ListedRow, Selection, Shown, Wanted,
    arranged::{
        Reaching, album_rows, albums_kept_before_the_rest, beyond_the_listing, elsewhere_standing,
        kept_before_the_rest,
    },
};
use crate::downloads::Download;

impl LibraryModel {
    pub(super) fn restate_the_listing(&mut self) {
        self.restate_what_was_found();
        let listing = self.listing();
        self.browse.rows = match self.selection {
            Selection::Album(_) => album_rows(
                &listing.whole(),
                &self.detail.release_tracks,
                self.arranging(),
            )
            .into(),
            Selection::Artist(_) => beyond_the_listing(Reaching {
                held: listing.len(),
                whole: listing.len() as u64 >= u64::from(self.browse.scoped_measured.rows),
                found: self.unheld.shown.len(),
            })
            .into(),
            Selection::Everything => beyond_the_listing(Reaching {
                held: listing.len(),
                whole: listing.len() as u64 >= u64::from(self.browse.tracks_measured.rows),
                found: self.searched_found(),
            })
            .into(),
        };
    }

    fn searched_found(&self) -> usize {
        match self.query.is_empty() {
            true => 0,
            false => self.unheld.shown.len(),
        }
    }

    pub fn elsewhere(&self) -> Option<Beyond> {
        if self.selection != Selection::Everything || self.query.is_empty() {
            return None;
        }

        elsewhere_standing(
            self.unheld.shown.len(),
            self.is_asking_elsewhere(),
            self.is_unreached(),
        )
    }

    fn restate_what_was_found(&mut self) {
        self.unheld.albums_shown =
            albums_kept_before_the_rest(&self.kept_albums_here(), &self.albums_found_here()).into();
        self.unheld.shown = match self.selection {
            Selection::Everything => {
                kept_before_the_rest(&self.kept_here(), self.found_elsewhere_here())
                    .into_iter()
                    .filter(|found| !self.is_listed_as_held(found))
                    .collect()
            }
            Selection::Artist(_) if self.query.is_empty() => Arc::clone(&self.unheld.songs),
            Selection::Artist(_) => still_answering(&self.unheld.songs, &self.query).into(),
            Selection::Album(_) => Arc::default(),
        };
    }

    fn is_listed_as_held(&self, found: &Found) -> bool {
        let Some(held) = self
            .delivered
            .downloads
            .of(&found.recording)
            .and_then(Download::want)
            .and_then(|want| self.wishes.standings.get(&want))
            .and_then(|standing| standing.held())
        else {
            return false;
        };
        self.browse.tracks.iter().any(|track| track.id == held)
    }

    fn kept_here(&self) -> Cow<'_, [Found]> {
        match self.unheld.kept_for.as_deref() {
            Some(_) if !self.can_enrich() || self.query.is_empty() => Cow::Borrowed(&[]),
            Some(asked) if asked == self.query => Cow::Borrowed(&self.unheld.kept_songs),
            Some(_) => Cow::Owned(still_answering(&self.unheld.kept_songs, &self.query)),
            None => Cow::Borrowed(&[]),
        }
    }

    fn kept_albums_here(&self) -> Cow<'_, [AlbumFound]> {
        match self.unheld.kept_for.as_deref() {
            Some(_) if !self.can_enrich() || self.query.is_empty() => Cow::Borrowed(&[]),
            Some(asked) if asked == self.query => Cow::Borrowed(&self.unheld.kept_albums),
            Some(_) => Cow::Owned(albums_still_answering(
                &self.unheld.kept_albums,
                &self.query,
            )),
            None => Cow::Borrowed(&[]),
        }
    }

    fn albums_found_here(&self) -> Cow<'_, [AlbumFound]> {
        match self.unheld.found_for.as_deref() {
            _ if self.query.is_empty() => Cow::Borrowed(&[]),
            Some(asked) if asked == self.query => Cow::Borrowed(&self.unheld.albums_found),
            Some(_) | None if self.is_asking_elsewhere() => Cow::Owned(albums_still_answering(
                &self.unheld.albums_found,
                &self.query,
            )),
            Some(_) | None => Cow::Borrowed(&[]),
        }
    }

    fn found_elsewhere_here(&self) -> &[Found] {
        match self.unheld.found_for.as_deref() {
            Some(asked) if !self.query.is_empty() && asked == self.query => &self.unheld.found,
            Some(_) | None if self.is_asking_elsewhere() => &self.unheld.narrowed,
            Some(_) | None => &[],
        }
    }

    pub fn albums_not_held(&self) -> Arc<[AlbumNotHeld]> {
        Arc::clone(&self.unheld.albums)
    }

    pub fn is_asking_elsewhere(&self) -> bool {
        !self.query.is_empty() && self.outreach.asking.as_deref() == Some(self.query.as_str())
    }

    fn is_unreached(&self) -> bool {
        !self.query.is_empty()
            && !self.is_asking_elsewhere()
            && self.outreach.unreached_for.as_deref() == Some(self.query.as_str())
    }

    pub fn found(&self) -> Arc<[Found]> {
        Arc::clone(&self.unheld.shown)
    }

    pub fn artists_found(&self) -> Arc<[ArtistFound]> {
        if self.selection != Selection::Everything || self.query.is_empty() {
            return Arc::default();
        }
        match self.unheld.found_for.as_deref() {
            Some(asked) if asked == self.query => Arc::clone(&self.unheld.artists_found),
            Some(_) | None if self.is_asking_elsewhere() => {
                artists_still_answering(&self.unheld.artists_found, &self.query).into()
            }
            Some(_) | None => Arc::default(),
        }
    }

    pub fn albums_found(&self) -> Arc<[AlbumFound]> {
        match self.selection == Selection::Everything && !self.query.is_empty() {
            true => Arc::clone(&self.unheld.albums_shown),
            false => Arc::default(),
        }
    }

    pub fn found_at(&self, row: usize) -> Option<Found> {
        match self.browse.rows.get(row)? {
            ListedRow::Found(at) => self.unheld.shown.get(*at).cloned(),
            ListedRow::Disc(_)
            | ListedRow::Held(_)
            | ListedRow::Missing(_)
            | ListedRow::NotHeld(_) => None,
        }
    }

    pub const fn sung(&self) -> Option<&Sung> {
        self.browse.sung.as_ref()
    }

    pub(super) fn ask_elsewhere_after(&mut self, settling: Duration, cx: &mut Context<Self>) {
        let text = self.query.clone();
        let words = match songs_asked(&text) {
            Some(words)
                if self.choices.online
                    && self.reference.is_some()
                    && self.selection == Selection::Everything =>
            {
                words
            }
            Some(_) | None => {
                self.stop_asking_elsewhere();
                return;
            }
        };
        if self.unheld.found_for.as_deref() == Some(text.as_str()) && !self.unheld.found_in_part {
            self.stop_asking_elsewhere();
            return;
        }
        self.unheld.narrowed = still_answering(&self.unheld.found, &text).into();
        self.outreach.asking = Some(text);

        if let Some(answered) = self.outreach.answers.get(&words).cloned() {
            self.unheld._finding = Task::ready(());
            self.show_answer(answered, Shown::Whole, cx);
            return;
        }
        self.unheld._finding = cx.spawn(async move |this, cx| {
            if !settling.is_zero() {
                cx.background_executor().timer(settling).await;
            }
            let _ = this.update(cx, |this, cx| this.reach_out(cx));
        });
    }

    fn stop_asking_elsewhere(&mut self) {
        self.outreach.asking = None;
        self.outreach.owed = None;
        self.unheld.narrowed = Arc::default();
        self.unheld._finding = Task::ready(());
        self.unheld._showing = Task::ready(());
    }

    fn reach_out(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self
            .outreach
            .asking
            .clone()
            .filter(|text| *text == self.query)
        else {
            return;
        };
        let Some(words) = songs_asked(&text) else {
            return;
        };
        if let Some(answered) = self.outreach.answers.get(&words).cloned() {
            self.show_answer(answered, Shown::Whole, cx);
            return;
        }
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            self.stop_asking_elsewhere();
            return;
        };
        if self.outreach.reaching_out.is_some() {
            self.outreach.owed = Some(text);
            return;
        }
        self.outreach.reaching_out = Some(words.clone());

        self.unheld._reaching = cx.spawn(async move |this, cx| {
            let asked = words.clone();
            let asking = Arc::clone(&reference);
            let songs = cx
                .background_executor()
                .spawn(async move { asking.find_songs(&asked) })
                .await;
            let Ok(Some(songs)) = this.update(cx, |this, cx| this.songs_reached(&words, songs, cx))
            else {
                return;
            };
            let asked = words.words.clone();
            let albums = cx
                .background_executor()
                .spawn(async move { reference.find_albums(&asked) })
                .await;
            let _ = this.update(cx, |this, cx| this.albums_reached(words, songs, albums, cx));
        });
    }

    fn is_still_asked(&self, words: &SongsAsked) -> bool {
        self.outreach
            .asking
            .as_deref()
            .filter(|text| *text == self.query)
            .and_then(songs_asked)
            .is_some_and(|asked| asked == *words)
    }

    fn songs_reached(
        &mut self,
        words: &SongsAsked,
        answered: resonate_library::Result<Vec<RecordingMatch>>,
        cx: &mut Context<Self>,
    ) -> Option<Arc<[RecordingMatch]>> {
        let current = self.is_still_asked(words);
        match answered {
            Ok(songs) if current => {
                let songs: Arc<[RecordingMatch]> = weighed_for(words, songs).into();
                self.show_answer(
                    Answer {
                        songs: Arc::clone(&songs),
                        albums: Arc::default(),
                    },
                    Shown::InPart,
                    cx,
                );
                Some(songs)
            }
            Ok(_) => {
                self.reach_out_for_what_is_owed(cx);
                None
            }
            Err(error) if current => {
                tracing::warn!(%error, "a search could not be asked elsewhere");
                self.outreach.reaching_out = None;
                self.outreach.unreached_for = self.outreach.asking.take();
                self.unheld.narrowed = Arc::default();
                self.outreach.owed = None;
                self.restate_the_listing();
                cx.notify();
                None
            }
            Err(error) => {
                tracing::debug!(%error, "a search typed past could not be asked elsewhere");
                self.reach_out_for_what_is_owed(cx);
                None
            }
        }
    }

    fn albums_reached(
        &mut self,
        words: SongsAsked,
        songs: Arc<[RecordingMatch]>,
        answered: resonate_library::Result<Vec<AlbumMatch>>,
        cx: &mut Context<Self>,
    ) {
        self.outreach.reaching_out = None;
        let albums: Arc<[AlbumMatch]> = match answered {
            Ok(albums) => {
                let albums: Arc<[AlbumMatch]> = albums.into();
                self.outreach.answers.insert(
                    words.clone(),
                    Answer {
                        songs: Arc::clone(&songs),
                        albums: Arc::clone(&albums),
                    },
                );
                albums
            }
            Err(error) => {
                tracing::debug!(%error, "albums could not be asked elsewhere");
                Arc::default()
            }
        };
        if self.is_still_asked(&words) {
            self.outreach.owed = None;
            self.show_answer(Answer { songs, albums }, Shown::Whole, cx);
            return;
        }
        self.reach_out_for_what_is_owed(cx);
    }

    fn reach_out_for_what_is_owed(&mut self, cx: &mut Context<Self>) {
        self.outreach.reaching_out = None;
        let owed = self.outreach.owed.take();
        if owed.is_some_and(|owed| self.outreach.asking.as_ref() == Some(&owed)) {
            self.reach_out(cx);
        }
    }

    fn show_answer(&mut self, answered: Answer, shown: Shown, cx: &mut Context<Self>) {
        let Some(text) = self.outreach.asking.clone() else {
            return;
        };
        let library = Arc::clone(&self.library);

        self.unheld._showing = cx.spawn(async move |this, cx| {
            let asked = text.clone();
            let (found, artists, albums) = cx
                .background_executor()
                .spawn(async move {
                    let artists = library.unheld_artists_among(&answered.songs, &asked);
                    let albums = library.unheld_albums_among(&answered.albums, &asked);
                    (
                        library.unheld_among(answered.songs.to_vec(), songs_asked(&asked).as_ref()),
                        artists,
                        albums,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.outreach.asking.as_deref() != Some(text.as_str()) {
                    return;
                }
                if shown == Shown::Whole {
                    this.outreach.asking = None;
                }
                this.unheld.found_in_part = shown == Shown::InPart;
                this.unheld.narrowed = Arc::default();
                this.outreach.unreached_for = None;
                this.unheld.artists_found = match artists {
                    Ok(artists) => artists.into(),
                    Err(error) => {
                        tracing::warn!(%error, "artists found elsewhere could not be weighed against the catalog");
                        Arc::default()
                    }
                };
                this.unheld.albums_found = match albums {
                    Ok(albums) => albums.into(),
                    Err(error) => {
                        tracing::warn!(%error, "albums found elsewhere could not be weighed against the catalog");
                        Arc::default()
                    }
                };
                this.unheld.found = match found {
                    Ok(found) => found.into(),
                    Err(error) => {
                        tracing::warn!(%error, "songs found elsewhere could not be weighed against the catalog");
                        Arc::default()
                    }
                };
                this.unheld.found_for = Some(text);
                this.restate_the_listing();
                cx.notify();
            });
        });
    }

    pub fn is_reading_the_search(&self) -> bool {
        self.in_flight.is_some_and(Wanted::reads_the_listing)
    }

    pub fn read_the_search_now(&mut self, cx: &mut Context<Self>) {
        if self.is_reading_the_search() {
            self.read(Wanted::TheSearch, cx);
        }
    }

    pub fn ask_elsewhere_now(&mut self, cx: &mut Context<Self>) {
        if self.is_asking_elsewhere() {
            self.unheld._finding = Task::ready(());
            self.reach_out(cx);
        }
    }

    pub fn ask_elsewhere_again(&mut self, cx: &mut Context<Self>) {
        self.outreach.unreached_for = None;
        self.unheld.found_for = None;
        self.ask_elsewhere_after(Duration::ZERO, cx);
        self.restate_the_listing();
        cx.notify();
    }
}
