use std::mem;

use gpui::{Context, Task};
use resonate_core::PlaylistId;
use resonate_library::{FromTheLink, Upkeep};

use super::{
    Change, LINKS_LOOKED_AT_EVERY, LINKS_LOOKED_AT_FIRST_AFTER, LibraryModel, Notice,
    unnamed_playlist,
    watched::{link_told, refreshed_where_due},
};
use crate::{toast, words};

impl LibraryModel {
    pub(super) fn keep_the_linked_in_step(&mut self, cx: &mut Context<Self>) {
        self._keeping_in_step = cx.spawn(async move |this, cx| {
            let mut waiting = LINKS_LOOKED_AT_FIRST_AFTER;
            loop {
                cx.background_executor().timer(waiting).await;
                waiting = LINKS_LOOKED_AT_EVERY;
                let Ok(Some((library, reference))) =
                    this.update(cx, |this, _| this.follows_links())
                else {
                    if this.upgrade().is_none() {
                        return;
                    }
                    continue;
                };
                let read = cx
                    .background_executor()
                    .spawn(async move { refreshed_where_due(&library, reference.as_ref()) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.took_in_what_the_links_said(read, false, cx)
                });
            }
        });
    }

    pub fn refresh_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        let Some((library, reference)) = self.follows_links() else {
            toast::tell(
                Notice::Noted(words::models::turn_online_on_to_read_a_playlist().to_owned()),
                cx,
            );
            return;
        };
        let before = mem::replace(&mut self._refreshing, Task::ready(()));

        self._refreshing = cx.spawn(async move |this, cx| {
            before.await;
            let read = cx
                .background_executor()
                .spawn(async move { vec![(id, library.refresh_playlist(reference.as_ref(), id))] })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.took_in_what_the_links_said(read, true, cx)
            });
        });
    }

    pub fn keep_playlist_from_its_link(
        &mut self,
        id: PlaylistId,
        upkeep: Upkeep,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| {
                library.keep_playlist_from_its_link(id, upkeep)?;
                let name = library
                    .playlist(id)?
                    .map_or_else(|| unnamed_playlist(id), |found| found.name);
                Ok(Some(match upkeep {
                    Upkeep::InStep => words::models::kept_in_step_with_its_link(name),
                    Upkeep::Once => words::models::stays_as_it_stands(name),
                }))
            },
            cx,
        );
    }

    fn took_in_what_the_links_said(
        &mut self,
        read: Vec<(PlaylistId, resonate_library::Result<Option<FromTheLink>>)>,
        asked: bool,
        cx: &mut Context<Self>,
    ) {
        let mut moved = false;
        for (playlist, answer) in read {
            match answer {
                Ok(Some(from)) => {
                    let wanted = from.found.len();
                    for found in from.found {
                        self.want_found(found, cx);
                    }
                    let brought = from.added + from.removed + wanted > 0;
                    moved |= brought;
                    if asked || brought {
                        toast::tell(
                            Notice::Done(link_told(&from.name, from.added, from.removed, wanted)),
                            cx,
                        );
                    }
                }
                Ok(None) => {
                    tracing::info!(%playlist, "a playlist's link names no playlist now");
                    if asked {
                        toast::tell(
                            Notice::Trouble(
                                words::models::the_link_names_no_playlist_any_more().to_owned(),
                            ),
                            cx,
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, %playlist, "a playlist was not read again from its link");
                    if asked {
                        toast::tell(
                            toast::could_not(
                                words::models::read_the_playlist_from_its_link(),
                                &error,
                            ),
                            cx,
                        );
                    }
                }
            }
        }
        if moved || asked {
            self.reload_playlists(cx);
        }
    }
}
