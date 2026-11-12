use std::sync::Arc;

use futures_channel::mpsc::unbounded;
use gpui::Context;
use resonate_core::{AlbumId, ArtistId};

use super::{LibraryModel, PageLearning, Selection, Wanted, told::albums_of};

impl LibraryModel {
    pub(super) fn learn_the_songs_of(&mut self, artist: ArtistId, cx: &mut Context<Self>) {
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            return;
        };
        let library = Arc::clone(&self.library);
        let (told, mut heard) = unbounded::<usize>();

        self.watchers._learning = cx.spawn(async move |this, cx| {
            let learning = cx.background_executor().spawn(async move {
                library.learn_the_songs_of_artist(
                    reference.as_ref(),
                    artist,
                    &PageLearning { told },
                )
            });
            while let Ok(songs) = heard.recv().await {
                let mut landed = songs;
                while let Ok(more) = heard.try_recv() {
                    landed += more;
                }
                if landed > 0 {
                    let _ = this.update(cx, |this, cx| {
                        if this.selection == Selection::Artist(artist) {
                            this.read(Wanted::TheSearch, cx);
                        }
                    });
                }
            }
            if let Err(error) = learning.await {
                tracing::debug!(
                    %error,
                    %artist,
                    "the songs of an artist's releases not held could not be read"
                );
            }
        });
    }

    pub fn ask_about(&self, album: Option<AlbumId>, artist: Option<ArtistId>) {
        if let Some(artist) = artist {
            self.enrichment.sought.artist(artist);
        }
        if let Some(album) = album {
            self.enrichment.sought.album(album);
        }
    }

    pub(super) fn ask_about_what_is_drawn(&self) {
        let Selection::Artist(artist) = self.selection else {
            return;
        };
        for album in albums_of(&self.browse.scoped).iter().rev() {
            self.enrichment.sought.album(*album);
        }
        self.enrichment.sought.artist(artist);
    }
}
