use std::sync::Arc;

use gpui::Context;
use resonate_core::PlaylistId;
use resonate_library::{Replaceable, Replacing};

use super::{
    LibraryModel, Notice, Wanted,
    told::{self, folds_moved},
};
use crate::{toast, words};

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
            let replaced = cx
                .background_executor()
                .spawn(async move { library.replace(&chosen) })
                .await;
            let landed = this.update(cx, |this, cx| this.replaced(replaced, cx));
            let _ = landed;
        });
        replacing.detach();
    }

    fn replaced(&mut self, replaced: resonate_library::Result<Replacing>, cx: &mut Context<Self>) {
        match replaced {
            Ok(replacing) => {
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
}
