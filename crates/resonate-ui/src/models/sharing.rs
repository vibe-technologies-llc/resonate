use std::sync::Arc;

use gpui::Context;
use resonate_core::TrackId;

use super::{
    LibraryModel, Notice,
    told::{Sharing, sharing_told},
};
use crate::{clipboard, toast, words};

impl LibraryModel {
    pub fn share_file(&mut self, track: TrackId, cx: &mut Context<Self>) {
        let Some(reference) = self.reference.clone().filter(|_| self.choices.online) else {
            toast::tell(
                Notice::Noted(words::models::turn_online_on_to_upload_a_file().to_owned()),
                cx,
            );
            return;
        };
        let library = Arc::clone(&self.library);
        toast::tell(
            Notice::Noted(words::models::uploading_file_to_catbox().to_owned()),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let Some(track) = library.track(track)? else {
                        return Ok(None);
                    };
                    if track.span.is_some() {
                        return Ok(None);
                    }
                    let Some(path) = track
                        .location
                        .as_path()
                        .filter(|_| track.location.is_local())
                    else {
                        return Ok(None);
                    };
                    reference.share_file(path)
                })
                .await;
            let _ = this.update(cx, |_, cx| {
                let notice = match result {
                    Ok(Some(link)) => {
                        clipboard::copy(link, cx);
                        Notice::Done(words::models::public_catbox_file_link_is_on_the().to_owned())
                    }
                    Ok(None) => Notice::Noted(
                        words::models::only_whole_local_files_can_be_uploaded().to_owned(),
                    ),
                    Err(error) => {
                        toast::could_not(words::models::upload_the_file_to_catbox(), &error)
                    }
                };
                toast::tell(notice, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn share(&mut self, track: TrackId, cx: &mut Context<Self>) {
        self.share_each(vec![track], cx);
    }

    pub fn share_each(&mut self, tracks: Vec<TrackId>, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        let reference = self.reference.clone().filter(|_| self.choices.online);

        self.watchers._shared = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    tracks
                        .into_iter()
                        .map(|track| {
                            library.shareable(track).map(|shared| {
                                shared.map(|shared| match &reference {
                                    Some(reference) => {
                                        shared.streamed_where_asked(reference.as_ref())
                                    }
                                    None => shared,
                                })
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            let outcome = this.update(cx, |_, cx| {
                let Sharing { link, notice } = sharing_told(read);
                if let Some(link) = link {
                    clipboard::copy(link, cx);
                }
                toast::tell(notice, cx);
                cx.notify();
            });
            let _ = outcome;
        });
    }
}
