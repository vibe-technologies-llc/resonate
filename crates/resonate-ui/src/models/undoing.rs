use std::{mem, sync::Arc};

use gpui::{Context, Task};
use resonate_library::{Library, Undoable};

use super::{
    Change, Edited, LibraryModel, Notice, Wanted,
    told::{done_again_as, put_back_as},
};
use crate::{toast, words};

impl LibraryModel {
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.edit(
            Change::Undo,
            |library| {
                Ok(Some(match library.undo()? {
                    Some(put_back) => put_back_as(&put_back),
                    None => words::models::nothing_left_to_put_back().to_owned(),
                }))
            },
            cx,
        );
    }

    pub fn undoable(&self) -> Option<Undoable> {
        self.library.undoable()
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.edit(
            Change::Redo,
            |library| {
                Ok(Some(match library.redo()? {
                    Some(done_again) => done_again_as(&done_again),
                    None => words::models::nothing_left_to_do_again().to_owned(),
                }))
            },
            cx,
        );
    }

    pub fn redoable(&self) -> Option<Undoable> {
        self.library.redoable()
    }

    pub(super) fn edit(
        &mut self,
        what: Change,
        change: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.edited(Wanted::ThePlaylists, what, change, cx);
    }

    pub(super) fn edited(
        &mut self,
        wanted: Wanted,
        what: Change,
        change: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.edited_then(wanted, what, change, |_, _, _| {}, cx);
    }

    pub(super) fn edited_then(
        &mut self,
        wanted: Wanted,
        what: Change,
        change: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        then: impl FnOnce(&mut Self, Edited, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let library = Arc::clone(&self.library);
        let before = mem::replace(&mut self._edit, Task::ready(()));

        self._edit = cx.spawn(async move |this, cx| {
            before.await;
            let done = cx
                .background_executor()
                .spawn(async move { change(&library) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                match done {
                    Err(error) => {
                        tracing::error!(%error, edit = ?what, "an edit could not be made");
                        toast::tell(toast::could_not(what.doing(), &error), cx);
                        then(this, Edited::Failed, cx);
                    }
                    Ok(Some(said)) => {
                        toast::tell(Notice::Done(said), cx);
                        then(this, Edited::Landed, cx);
                    }
                    Ok(None) => then(this, Edited::Landed, cx),
                }
                this.read(wanted, cx);
            });
            let _ = outcome;
        });
    }
}
