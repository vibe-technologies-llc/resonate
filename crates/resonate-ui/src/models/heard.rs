use std::{mem, sync::Arc, time::Duration};

use gpui::{Context, Task};
use resonate_engine::Played;

use super::{LibraryModel, arranged::Named, watched::tell_of_listen_time};
use crate::{toast, words};

impl LibraryModel {
    pub fn track_heard(&mut self, heard: Played, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self.counted = None;

        self._counted = cx.spawn(async move |this, cx| {
            let counted = cx
                .background_executor()
                .spawn(
                    async move { library.track_played(&heard.location, heard.span, heard.heard) },
                )
                .await;

            let outcome = this.update(cx, |this, cx| match counted {
                Err(error) => {
                    tracing::warn!(%error, "a play was not counted");
                    toast::tell(
                        toast::could_not(words::models::count_that_play(), &error),
                        cx,
                    );
                }
                Ok(None) => {}
                Ok(Some(counted)) => {
                    this.counted = Some(counted.listen);
                    if let Some(track) = counted.track {
                        this.revision = this.revision.wrapping_add(1);
                        this.named.insert(track.id, Named::of(&track));
                        this.reload(cx);
                    }
                }
            });
            let _ = outcome;
        });
    }

    pub fn track_hearing(&mut self, heard: Duration, cx: &mut Context<Self>) {
        let Some(listen) = self.counted else {
            return;
        };
        let library = Arc::clone(&self.library);
        let before = mem::replace(&mut self._settled, Task::ready(()));

        self._settled = cx.spawn(async move |this, cx| {
            before.await;
            let kept = cx
                .background_executor()
                .spawn(async move { library.listened(listen, heard) })
                .await;
            if let Err(error) = kept {
                tracing::warn!(%error, "how long a play has been heard for was not kept");
                tell_of_listen_time(&this, &error, cx);
            }
        });
    }

    pub fn track_passed(&mut self, heard: Duration, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        let before = mem::replace(&mut self._passed, Task::ready(()));

        self._passed = cx.spawn(async move |this, cx| {
            before.await;
            let kept = cx
                .background_executor()
                .spawn(async move { library.passed(heard) })
                .await;
            if let Err(error) = kept {
                tracing::warn!(%error, "how long a passing listen lasted was not kept");
                tell_of_listen_time(&this, &error, cx);
            }
        });
    }

    pub fn track_settled(&mut self, heard: Duration, cx: &mut Context<Self>) {
        let Some(listen) = self.counted.take() else {
            return;
        };
        let library = Arc::clone(&self.library);
        let before = mem::replace(&mut self._settled, Task::ready(()));

        self._settled = cx.spawn(async move |this, cx| {
            before.await;
            let kept = cx
                .background_executor()
                .spawn(async move { library.listened(listen, heard) })
                .await;
            if let Err(error) = kept {
                tracing::warn!(%error, "how long a play was heard for was not kept");
                tell_of_listen_time(&this, &error, cx);
            }
        });
    }
}
