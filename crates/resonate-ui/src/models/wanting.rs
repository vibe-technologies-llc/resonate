use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};

use gpui::{Context, SharedString, Task};
use resonate_core::{AlbumId, ArtistId};
use resonate_library::{ArtistFound, Found, Mbid, PollOptions};
use resonate_providers::Providers;

use super::{LibraryModel, Wanted, told::cover_what_was_wanted};
use crate::{
    downloads::{self, Download, Fetcher, Fetching},
    toast, words,
};

impl LibraryModel {
    pub fn want_found(&mut self, found: Found, cx: &mut Context<Self>) {
        let Some(reference) = self.reference.clone() else {
            return;
        };
        if self.wishes.wanting.contains_key(&found.recording) {
            return;
        }
        self.delivered
            .downloads
            .landing(found.clone(), SystemTime::now());
        cx.notify();
        let library = Arc::clone(&self.library);
        let recording = found.recording.clone();
        let providers = Some(self.providers(cx))
            .filter(Providers::has_a_source)
            .map(Arc::new);

        let wanting = cx.spawn(async move |this, cx| {
            let asked = found.clone();
            let covered_by = Arc::clone(&library);
            let covering_with = Arc::clone(&reference);
            let foreseen_by = Arc::clone(&library);
            let foreseeing = providers.map(|providers| {
                let foreseen = found.clone();
                cx.background_executor()
                    .spawn(async move { foreseen_by.foresee(&providers, &foreseen) })
            });
            let awaiting_the_providers = foreseeing.is_some();
            let wanted = cx
                .background_executor()
                .spawn(async move { library.want_found_uncovered(reference.as_ref(), &asked) })
                .await;
            let covering = wanted
                .as_ref()
                .ok()
                .and_then(|wanted| wanted.covering.clone());
            let was_wanted = wanted.is_ok();

            let landed = this.update(cx, |this, cx| {
                if let Some(finished) = this.wishes.wanting.remove(&found.recording) {
                    finished.detach();
                }
                match wanted.map(|wanted| wanted.wanted) {
                    Ok(want) => {
                        let fetched_by = this.fetched_by(cx);
                        this.delivered
                            .downloads
                            .wanted(&found.recording, want, fetched_by);
                        if fetched_by == Fetcher::AProvider && !awaiting_the_providers {
                            this.fetch_what_was_wanted(PollOptions::default(), cx);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "a song found elsewhere could not be wanted");
                        this.delivered.downloads.unwanted(&found.recording);
                    }
                }
                this.read(Wanted::Everything, cx);
            });
            if landed.is_err() {
                return;
            }
            if let Some(foreseeing) = foreseeing {
                foreseeing.await;
                if was_wanted {
                    let polled = this.update(cx, |this, cx| {
                        if this.fetched_by(cx) == Fetcher::AProvider {
                            this.fetch_what_was_wanted(PollOptions::default(), cx);
                        }
                    });
                    if polled.is_err() {
                        return;
                    }
                } else {
                    covered_by.forget_foresight(&found.recording);
                }
            }
            if let Some(covering) = covering {
                cover_what_was_wanted(&this, cx, covered_by, covering_with, covering).await;
            }
        });
        self.wishes.wanting.insert(recording, wanting);
    }

    pub fn fetch_and_play(&mut self, found: Found, cx: &mut Context<Self>) {
        let recording = found.recording.clone();
        self.want_found(found, cx);
        let landing = self
            .delivered
            .downloads
            .of(&recording)
            .is_some_and(|download| self.fetching(download).is_underway());
        if landing {
            self.delivered
                .downloads
                .play_once_landed(recording, Instant::now());
        }
    }

    pub fn is_awaited(&self, recording: &Mbid) -> bool {
        self.delivered.downloads.is_awaited(recording)
    }

    pub fn want_album(&mut self, group: Mbid, pressing: Option<Mbid>, cx: &mut Context<Self>) {
        let Some(reference) = self.reference.clone() else {
            return;
        };
        if self.unheld.albums_wanted.contains_key(&group) {
            return;
        }
        let library = Arc::clone(&self.library);
        let asked = group.clone();
        let held_under = group.clone();

        let wanting = cx.spawn(async move |this, cx| {
            let covered_by = Arc::clone(&library);
            let covering_with = Arc::clone(&reference);
            let wanted = cx
                .background_executor()
                .spawn(async move {
                    library.want_album_uncovered(reference.as_ref(), &asked, pressing.as_ref())
                })
                .await;
            let covering = wanted
                .as_ref()
                .ok()
                .and_then(|wanted| wanted.covering.clone());

            let landed = this.update(cx, |this, cx| {
                if let Some(finished) = this.unheld.albums_wanted.remove(&group) {
                    finished.detach();
                }
                match wanted.map(|wanted| wanted.wanted) {
                    Ok(wanted) => {
                        let fetched_by = this.fetched_by(cx);
                        let now = SystemTime::now();
                        this.unheld.album_songs.insert(
                            group,
                            wanted
                                .iter()
                                .map(|(song, _)| song.recording.clone())
                                .collect(),
                        );
                        for (song, want) in wanted {
                            let recording = song.recording.clone();
                            this.delivered.downloads.landing(song, now);
                            this.delivered
                                .downloads
                                .wanted(&recording, want, fetched_by);
                        }
                        if fetched_by == Fetcher::AProvider {
                            this.fetch_what_was_wanted(PollOptions::default(), cx);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "an album not held could not be wanted");
                        toast::tell(toast::could_not(words::models::add_the_album(), &error), cx);
                    }
                }
                this.read(Wanted::Everything, cx);
            });
            if landed.is_err() {
                return;
            }
            if let Some(covering) = covering {
                cover_what_was_wanted(&this, cx, covered_by, covering_with, covering).await;
            }
        });
        self.unheld.albums_wanted.insert(held_under, wanting);
        cx.notify();
    }

    pub fn land_album_not_held(
        &mut self,
        group: Mbid,
        cx: &mut Context<Self>,
    ) -> Task<Option<AlbumId>> {
        let Some(reference) = self.reference.clone() else {
            return Task::ready(None);
        };
        if !self.unheld.albums_opening.insert(group.clone()) {
            return Task::ready(None);
        }
        cx.notify();
        let library = Arc::clone(&self.library);
        let asked = group.clone();

        cx.spawn(async move |this, cx| {
            let covered_by = Arc::clone(&library);
            let covering_with = Arc::clone(&reference);
            let landed = cx
                .background_executor()
                .spawn(async move { library.open_album_uncovered(reference.as_ref(), &asked) })
                .await;

            let opened = this
                .update(cx, |this, cx| {
                    this.unheld.albums_opening.remove(&group);
                    match landed {
                        Ok(landed) => {
                            this.read(Wanted::Everything, cx);
                            Some(landed)
                        }
                        Err(error) => {
                            tracing::error!(%error, "an album not held could not be opened");
                            toast::tell(
                                toast::could_not(words::models::open_the_album(), &error),
                                cx,
                            );
                            None
                        }
                    }
                })
                .ok()
                .flatten()?;

            if let Some(covering) = opened.covering {
                cx.spawn(async move |cx| {
                    cover_what_was_wanted(&this, cx, covered_by, covering_with, covering).await;
                })
                .detach();
            }
            Some(opened.wanted)
        })
    }

    pub fn land_artist_found(
        &mut self,
        found: ArtistFound,
        cx: &mut Context<Self>,
    ) -> Task<Option<ArtistId>> {
        let Some(reference) = self.reference.clone() else {
            return Task::ready(None);
        };
        if !self.unheld.artists_opening.insert(found.mbid.clone()) {
            return Task::ready(None);
        }
        cx.notify();
        let library = Arc::clone(&self.library);

        cx.spawn(async move |this, cx| {
            let landed = cx
                .background_executor()
                .spawn({
                    let found = found.clone();
                    async move { library.open_artist_found(reference.as_ref(), &found) }
                })
                .await;

            this.update(cx, |this, cx| {
                this.unheld.artists_opening.remove(&found.mbid);
                match landed {
                    Ok(artist) => {
                        this.read(Wanted::Everything, cx);
                        Some(artist)
                    }
                    Err(error) => {
                        tracing::error!(%error, "an artist found elsewhere could not be opened");
                        toast::tell(
                            toast::could_not(words::models::open_the_artist(), &error),
                            cx,
                        );
                        None
                    }
                }
            })
            .ok()
            .flatten()
        })
    }

    pub fn want_missing_tracks(&mut self, album: AlbumId, cx: &mut Context<Self>) {
        if !self.unheld.albums_missing_tracks_wanted.insert(album) {
            return;
        }

        let library = Arc::clone(&self.library);
        let wanting = cx.spawn(async move |this, cx| {
            let wanted = cx
                .background_executor()
                .spawn(async move { library.want_missing_tracks(album) })
                .await;

            let landed = this.update(cx, |this, cx| {
                this.unheld.albums_missing_tracks_wanted.remove(&album);
                this.wants_landed(
                    wanted,
                    words::models::download_the_missing_tracks(),
                    Wanted::Everything,
                    cx,
                );
            });
            let _ = landed;
        });
        wanting.detach();
        cx.notify();
    }

    pub fn is_opening_album(&self, group: &Mbid) -> bool {
        self.unheld.albums_opening.contains(group)
    }

    pub fn is_opening_artist(&self, artist: &Mbid) -> bool {
        self.unheld.artists_opening.contains(artist)
    }

    pub fn fetching_album(&self, group: &Mbid) -> Option<Fetching> {
        if self.unheld.albums_wanted.contains_key(group) {
            return Some(Fetching::Landing);
        }
        let fetching: Vec<Fetching> = self
            .unheld
            .album_songs
            .get(group)?
            .iter()
            .filter_map(|recording| self.delivered.downloads.of(recording))
            .map(|download| self.fetching(download))
            .collect();

        let underway = fetching.iter().copied().filter(|each| each.is_underway());
        underway
            .clone()
            .find(|each| matches!(each, Fetching::Downloading { .. }))
            .or_else(|| underway.clone().next())
            .or_else(|| {
                fetching
                    .iter()
                    .copied()
                    .find(|each| *each != Fetching::Downloaded)
            })
            .or_else(|| fetching.first().copied())
    }

    pub(crate) fn told_album(&self, group: &Mbid) -> Option<SharedString> {
        if self.unheld.albums_wanted.contains_key(group) {
            return Some(Fetching::Landing.saying());
        }
        let songs: Vec<&Download> = self
            .unheld
            .album_songs
            .get(group)?
            .iter()
            .filter_map(|recording| self.delivered.downloads.of(recording))
            .collect();
        if songs.is_empty() {
            return None;
        }
        let fetching: Vec<Fetching> = songs.iter().map(|song| self.fetching(song)).collect();
        let arriving = songs.iter().any(|song| {
            self.fetched(song)
                .is_some_and(|fetched| fetched.is_arriving())
        });

        Some(downloads::summed_up(&fetching, arriving))
    }
}
