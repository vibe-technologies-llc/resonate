use std::{mem, sync::Arc};

use gpui::{Context, Task};
use resonate_core::TrackId;

use super::{LibraryModel, Wanted, told::deletion_told};
use crate::toast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    ThisDevice,
    EveryDevice,
}

impl LibraryModel {
    pub fn delete_tracks(
        &mut self,
        tracks: Vec<TrackId>,
        title: String,
        reach: Reach,
        cx: &mut Context<Self>,
    ) {
        let library = Arc::clone(&self.library);
        let asked = tracks.len();
        let before = mem::replace(&mut self._edit, Task::ready(()));

        self._edit = cx.spawn(async move |this, cx| {
            before.await;

            let deleted = cx
                .background_executor()
                .spawn(async move {
                    match reach {
                        Reach::ThisDevice => library.delete_tracks(&tracks),
                        Reach::EveryDevice => library.delete_tracks_everywhere(&tracks),
                    }
                })
                .await;

            let told = this.update(cx, |this, cx| {
                toast::tell(deletion_told(deleted, &title, asked), cx);
                this.read(Wanted::Everything, cx);
            });
            let _ = told;
        });
    }
}
