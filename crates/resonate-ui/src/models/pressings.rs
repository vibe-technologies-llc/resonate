use std::{path::Path, sync::Arc};

use gpui::{Context, Task};
use resonate_core::{AlbumId, ArtistId, TrackId};
use resonate_library::{GroupRelease, Isrc, Mbid, Recording, ReleaseAsked, Track, Wording};

use super::{
    Change, Edited, LibraryModel, Pressings, Wanted, read_further,
    told::{in_the_order_they_came_out, named_with_its_credit},
};
use crate::{toast, words};

impl LibraryModel {
    pub fn pressings_asked(&self, group: Mbid, cx: &mut Context<Self>) -> Option<Task<Pressings>> {
        let reference = self.reference.clone().filter(|_| self.choices.online)?;

        Some(cx.spawn(async move |_, cx| {
            let asked = cx
                .background_executor()
                .spawn(async move { reference.release_group(&group) })
                .await;
            match asked {
                Ok(Some(group)) => Pressings::Listed(in_the_order_they_came_out(group.releases)),
                Ok(None) => Pressings::Unanswered,
                Err(error) => {
                    tracing::warn!(%error, "a release group's pressings could not be read");
                    cx.update(|cx| {
                        toast::tell(
                            toast::could_not(words::models::read_that_album_s_pressings(), &error),
                            cx,
                        );
                    });
                    Pressings::Unanswered
                }
            }
        }))
    }

    pub fn releases_asked(
        &self,
        album: AlbumId,
        cx: &mut Context<Self>,
    ) -> Option<Task<Pressings>> {
        let reference = self.reference.clone().filter(|_| self.choices.online)?;
        let held = self.album_of(album)?;
        let release = self.detail.release.as_ref();
        let asked = ReleaseAsked {
            title: held.title.clone(),
            artist: held.artist.clone(),
            artist_mbid: None,
            barcode: release.and_then(|release| release.barcode.clone()),
            catalog_number: release.and_then(|release| release.catalog_number.clone()),
            wording: Wording::Words,
        };

        Some(cx.background_executor().spawn(async move {
            match reference.find_release(&asked) {
                Ok(found) if !found.is_empty() => Pressings::Listed(Arc::from(
                    found
                        .into_iter()
                        .map(|found| GroupRelease {
                            title: named_with_its_credit(&found),
                            id: found.release,
                            date: found.date,
                            country: None,
                            track_count: found.track_count,
                            discs: Vec::new(),
                        })
                        .collect::<Vec<_>>(),
                )),
                Ok(_) => Pressings::Unanswered,
                Err(error) => {
                    tracing::warn!(%error, "the releases an album might be could not be found");
                    Pressings::Unanswered
                }
            }
        }))
    }

    pub fn take_pressing(&mut self, album: AlbumId, release: Mbid, cx: &mut Context<Self>) -> bool {
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            return false;
        };
        self.edited(
            Wanted::Everything,
            Change::TakePressing,
            move |library| {
                library
                    .take_pressing(album, reference.as_ref(), &release)
                    .map(|took| took.then(|| words::models::took_the_pressing().to_owned()))
            },
            cx,
        );
        true
    }

    pub fn releases_it_could_sit_on(
        &self,
        track: TrackId,
        cx: &mut Context<Self>,
    ) -> Option<Task<resonate_library::Result<Vec<Recording>>>> {
        let reference = self.reference.clone().filter(|_| self.choices.online)?;
        let library = Arc::clone(&self.library);
        Some(
            cx.background_executor()
                .spawn(async move { library.recordings_of(track, reference.as_ref()) }),
        )
    }

    pub fn place_on(
        &mut self,
        track: TrackId,
        recording: Arc<Recording>,
        release: Mbid,
        cx: &mut Context<Self>,
    ) {
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            return;
        };
        self.edited(
            Wanted::Everything,
            Change::PlaceOn,
            move |library| {
                library
                    .place_on(track, reference.as_ref(), &recording, &release)
                    .map(|placed| placed.then(|| words::models::placed_on().to_owned()))
            },
            cx,
        );
    }

    pub fn recordings_like(
        &self,
        track: TrackId,
        words: String,
        cx: &mut Context<Self>,
    ) -> Option<Task<resonate_library::Result<Vec<Recording>>>> {
        let reference = self.reference.clone().filter(|_| self.choices.online)?;
        let library = Arc::clone(&self.library);
        Some(
            cx.background_executor()
                .spawn(async move { library.recordings_like(track, reference.as_ref(), &words) }),
        )
    }

    pub fn take_recording(
        &mut self,
        track: TrackId,
        recording: Arc<Recording>,
        cx: &mut Context<Self>,
    ) {
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            return;
        };
        self.edited(
            Wanted::Everything,
            Change::TakeRecording,
            move |library| {
                library
                    .take_recording(track, reference.as_ref(), &recording)
                    .map(|taken| {
                        Some(
                            if taken {
                                words::models::took_the_recording()
                            } else {
                                words::models::not_taken()
                            }
                            .to_owned(),
                        )
                    })
            },
            cx,
        );
    }

    pub fn give_an_isrc(&mut self, track: TrackId, isrc: Isrc, cx: &mut Context<Self>) {
        self.edited_then(
            Wanted::Everything,
            Change::GiveIsrc,
            move |library| {
                library
                    .set_isrc(track, &isrc)
                    .map(|()| Some(words::models::gave_the_isrc().to_owned()))
            },
            move |this, edited, cx| {
                if edited == Edited::Landed {
                    this.enrichment.sought.track(track);
                    this.enrich(false, cx);
                }
            },
            cx,
        );
    }

    pub fn read_the_rest_of(&mut self, artist: ArtistId, cx: &mut Context<Self>) {
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            return;
        };
        self.edited(
            Wanted::Everything,
            Change::ReadTheRest,
            move |library| {
                library
                    .read_the_rest_of(artist, reference.as_ref())
                    .map(|read| Some(read_further(read)))
            },
            cx,
        );
    }

    pub fn forget_the_match(&mut self, album: AlbumId, cx: &mut Context<Self>) {
        self.edited(
            Wanted::Everything,
            Change::ForgetTheMatch,
            move |library| {
                library
                    .forget_the_match(album)
                    .map(|forgot| forgot.then(|| words::models::forgot_the_match().to_owned()))
            },
            cx,
        );
    }

    pub fn forget_delivered(&mut self, track: &Track, cx: &mut Context<Self>) {
        let Some(path) = track.location.as_path().map(Path::to_path_buf) else {
            return;
        };
        let title = track.title.clone();
        self.edited(
            Wanted::Everything,
            Change::ForgetDelivered,
            move |library| {
                library
                    .forget_delivered(&path)
                    .map(|forgot| forgot.then(|| words::models::forgot_the_delivered(title)))
            },
            cx,
        );
    }
}
