use std::{sync::Arc, time::SystemTime};

use ahash::AHashSet;
use gpui::Context;
use resonate_core::{PlaylistId, WantId};
use resonate_library::{Replaceable, Replacing, Want};

use super::{
    LibraryModel, Notice, Wanted,
    told::{self, folds_moved},
};
use crate::{downloads, toast, words};

impl LibraryModel {
    pub fn holds_lossy_songs(&self, playlist: PlaylistId) -> bool {
        self.shelf.lossy.contains(&playlist)
    }

    pub fn playlist_name(&self, playlist: PlaylistId) -> Option<String> {
        let shelved = self
            .shelf
            .held
            .iter()
            .chain(self.shelf.playlists.iter())
            .find(|held| held.id == playlist)
            .map(|held| held.name.clone());
        shelved.or_else(|| {
            self.shelf
                .pinned
                .iter()
                .find(|pinned| pinned.id == playlist)
                .map(|pinned| pinned.name.clone())
        })
    }

    pub fn replace_lossy_songs(&mut self, chosen: Vec<Replaceable>, cx: &mut Context<Self>) {
        if chosen.is_empty() {
            return;
        }
        let library = Arc::clone(&self.library);
        let replacing = cx.spawn(async move |this, cx| {
            let (replaced, wanted) = cx
                .background_executor()
                .spawn(async move {
                    let replaced = library.replace(&chosen);
                    let wanted = match &replaced {
                        Ok(replacing) if !replacing.wants.is_empty() => {
                            library.wants().unwrap_or_else(|error| {
                                tracing::warn!(%error, "the wants a replacement placed could not be read");
                                Vec::new()
                            })
                        }
                        Ok(_) | Err(_) => Vec::new(),
                    };
                    (replaced, wanted)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.replaced(replaced, &wanted, cx));
        });
        replacing.detach();
    }

    fn replaced(
        &mut self,
        replaced: resonate_library::Result<Replacing>,
        wanted: &[Want],
        cx: &mut Context<Self>,
    ) {
        match replaced {
            Ok(replacing) => {
                self.list_the_replacements_wanted(&replacing, wanted, cx);
                self.passes.relocated.extend(folds_moved(&replacing.folds));
                toast::tell(told::replaced_now(&replacing), cx);
                if replacing.wanted > 0 {
                    self.fetch_or_say_nobody_can(cx);
                }
            }
            Err(resonate_library::Error::AlreadyWalking) => {
                toast::tell(
                    Notice::Trouble(words::models::already_walking().to_owned()),
                    cx,
                );
            }
            Err(error) => {
                tracing::error!(%error, "the lossy songs could not be replaced");
                toast::tell(
                    toast::could_not(words::models::replace_the_lossy_songs(), &error),
                    cx,
                );
            }
        }
        self.read(Wanted::Everything, cx);
    }

    fn list_the_replacements_wanted(
        &mut self,
        replacing: &Replacing,
        wanted: &[Want],
        cx: &Context<Self>,
    ) {
        let placed: AHashSet<WantId> = replacing.wants.iter().map(|(_, want)| *want).collect();
        let fetched_by = self.fetched_by(cx);
        let now = SystemTime::now();
        for want in wanted.iter().filter(|want| placed.contains(&want.id)) {
            let Some(found) = downloads::found_of(want) else {
                continue;
            };
            let recording = found.recording.clone();
            self.delivered.downloads.landing(found, now);
            self.delivered
                .downloads
                .wanted(&recording, want.id, fetched_by);
        }
    }
}
