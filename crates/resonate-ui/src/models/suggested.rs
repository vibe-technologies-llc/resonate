use std::sync::Arc;

use gpui::{Context, Task};
use resonate_core::WantId;
use resonate_library::{SavedQuery, TrackQuery};

use super::{LOOK_AHEAD, LibraryModel, PREVIEWED_AT_MOST, Previewed};
use crate::{toast, words};

impl LibraryModel {
    pub(super) fn asking_for(&self) -> Vec<WantId> {
        self.work
            .polling()
            .map_or_else(Vec::new, |progress| progress.asking_all())
    }

    pub const fn opened_suggestion(&self) -> Option<&Previewed> {
        self.opened_suggestion.as_ref()
    }

    pub fn open_suggestion(&mut self, query: Option<SavedQuery>, cx: &mut Context<Self>) {
        let Some(query) = query else {
            self.opened_suggestion = None;
            self._previewing = Task::ready(());
            cx.notify();
            return;
        };
        if self
            .opened_suggestion
            .as_ref()
            .is_some_and(|held| held.query == query)
        {
            return;
        }
        self.opened_suggestion = Some(Previewed {
            query: query.clone(),
            tracks: Arc::default(),
            asked: PREVIEWED_AT_MOST,
        });
        cx.notify();
        self.read_the_preview(query, PREVIEWED_AT_MOST, cx);
    }

    pub(crate) fn preview_further(&mut self, drawn_to: usize, cx: &mut Context<Self>) {
        let Some(previewed) = self.opened_suggestion.as_mut() else {
            return;
        };
        let held = previewed.tracks.len();
        if held < previewed.asked || drawn_to + LOOK_AHEAD < held {
            return;
        }
        previewed.asked = previewed.asked.saturating_add(PREVIEWED_AT_MOST);
        let (query, asked) = (previewed.query.clone(), previewed.asked);
        self.read_the_preview(query, asked, cx);
    }

    fn read_the_preview(&mut self, query: SavedQuery, asked_for: usize, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self._previewing = cx.spawn(async move |this, cx| {
            let asked = query.clone();
            let read = cx
                .background_executor()
                .spawn(async move {
                    library.tracks(&TrackQuery {
                        limit: Some(asked_for),
                        ..TrackQuery::from(&asked)
                    })
                })
                .await;

            let landed = this.update(cx, |this, cx| {
                let tracks = match read {
                    Ok(tracks) => tracks,
                    Err(error) => {
                        tracing::error!(%error, "a suggestion could not be previewed");
                        toast::tell(
                            toast::could_not(words::models::preview_that_suggestion(), &error),
                            cx,
                        );
                        return;
                    }
                };
                this.remember(&tracks);
                if let Some(previewed) = this
                    .opened_suggestion
                    .as_mut()
                    .filter(|held| held.query == query)
                {
                    previewed.tracks = tracks.into();
                    cx.notify();
                }
            });
            let _ = landed;
        });
    }
}
