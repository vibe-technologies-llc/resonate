use std::{path::PathBuf, sync::Arc};

use gpui::Context;
use resonate_core::{PlaylistId, QueueStamp, Span};
use resonate_library::{Cut, Direction, Kept, Playing, PlaylistEntry, RowOrder, SavedQuery};

use super::{
    Change, LibraryModel, arranged::read_in, put_it_back, unnamed_playlist, watched::put_into,
};
use crate::words;

impl LibraryModel {
    pub fn held_entries_of(&self, id: PlaylistId) -> Option<Vec<PlaylistEntry>> {
        (self.opened == Some(id) && self.narrowing().is_none()).then(|| self.entries.to_vec())
    }

    pub fn playing_playlist(&self, queue: QueueStamp) -> Option<PlaylistId> {
        self.library.playing_playlist(queue)
    }

    pub fn set_playing_playlist(&self, playing: Option<Playing>) {
        self.library.set_playing_playlist(playing);
    }

    pub fn open_playlist(&mut self, opened: Option<PlaylistId>, cx: &mut Context<Self>) {
        if self.opened == opened {
            return;
        }
        self.opened = opened;
        self.held = None;
        self.entries = Arc::default();
        self.reload_playlists(cx);
    }

    pub fn create_playlist(&mut self, name: String, holding: Vec<Cut>, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| {
                library.start_playlist(&name, &holding)?;

                Ok(Some(match holding.len() {
                    0 => words::models::started(name, put_it_back()),
                    added => words::models::started_with(added, name, put_it_back()),
                }))
            },
            cx,
        );
    }

    pub fn save_query(&mut self, name: String, query: SavedQuery, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| {
                let id = library.save_query(&name, &query)?;
                let held = library.playlist(id)?.map_or(0, |found| found.entries);

                Ok(Some(words::models::fills_itself(held, name)))
            },
            cx,
        );
    }

    pub fn revise_query(
        &mut self,
        id: PlaylistId,
        name: String,
        query: SavedQuery,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| {
                library.revise_query(id, &name, &query)?;
                let held = library.playlist(id)?.map_or(0, |found| found.entries);

                Ok(Some(words::models::fills_itself_from_the_new_search(
                    held, name,
                )))
            },
            cx,
        );
    }

    pub fn rename_playlist(&mut self, id: PlaylistId, name: String, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| library.rename_playlist(id, &name).map(|()| None),
            cx,
        );
    }

    pub fn drop_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        if self.opened == Some(id) {
            self.opened = None;
            self.held = None;
            self.entries = Arc::default();
        }
        self.edit(
            Change::Playlist,
            move |library| {
                let name = library.playlist(id)?.map(|found| found.name);
                let dropped = library.remove_playlist(id)?;

                Ok(name
                    .filter(|_| dropped)
                    .map(|name| words::models::discarded(name, put_it_back())))
            },
            cx,
        );
    }

    pub fn duplicate_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| {
                let copy = library.duplicate_playlist(id)?;
                let name = library
                    .playlist(copy)?
                    .map_or_else(|| unnamed_playlist(copy), |found| found.name);

                Ok(Some(words::models::made(name, put_it_back())))
            },
            cx,
        );
    }

    pub fn pin_playlist(&mut self, id: PlaylistId, pinned: bool, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| library.pin_playlist(id, pinned).map(|_| None),
            cx,
        );
    }

    pub fn add_to_playlist(&mut self, id: PlaylistId, holding: Vec<Cut>, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| {
                let added = library.add_to_playlist(id, &holding)?;
                put_into(library, id, added).map(Some)
            },
            cx,
        );
    }

    pub fn insert_into_playlist(
        &mut self,
        id: PlaylistId,
        holding: Vec<Cut>,
        at: usize,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| {
                let added = library.insert_into_playlist(id, &holding, at)?;
                put_into(library, id, added).map(Some)
            },
            cx,
        );
    }

    pub fn remove_from_playlist(&mut self, id: PlaylistId, rows: Span, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| library.remove_from_playlist(id, rows).map(|_| None),
            cx,
        );
    }

    pub fn remove_runs_from_playlist(
        &mut self,
        id: PlaylistId,
        runs: Vec<Span>,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| library.remove_runs_from_playlist(id, &runs).map(|_| None),
            cx,
        );
    }

    pub fn gather_in_playlist(
        &mut self,
        id: PlaylistId,
        runs: Vec<Span>,
        onto: usize,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| library.gather_in_playlist(id, &runs, onto).map(|_| None),
            cx,
        );
    }

    pub fn remove_matching(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        let Some(matching) = self.narrowing().map(ToOwned::to_owned) else {
            return;
        };
        self.edit(
            Change::Playlist,
            move |library| {
                Ok(Some(match library.remove_matching(id, &matching)? {
                    0 => words::models::nothing_shown_was_left_to_take_out().to_owned(),
                    dropped => words::models::took_rows_out(dropped),
                }))
            },
            cx,
        );
    }

    pub fn move_in_playlist(
        &mut self,
        id: PlaylistId,
        rows: Span,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| library.move_in_playlist(id, rows, to).map(|_| None),
            cx,
        );
    }

    pub fn import_playlists(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        if files.is_empty() {
            return;
        }
        self.edit(
            Change::Playlist,
            move |library| {
                let mut said = Vec::with_capacity(files.len());
                for file in &files {
                    said.push(read_in(library.import_playlist(file, None)?));
                }
                Ok(Some(said.join(" · ")))
            },
            cx,
        );
    }

    pub fn export_playlist(&mut self, id: PlaylistId, path: PathBuf, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| {
                let written = library.export_playlist(id, &path)?;
                Ok(Some(words::models::wrote_rows(
                    written.rows,
                    written.format.name(),
                    path.display().to_string(),
                )))
            },
            cx,
        );
    }

    pub fn sort_playlist(
        &mut self,
        id: PlaylistId,
        order: RowOrder,
        reading: Direction,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| {
                Ok(Some(match library.sort_playlist(id, order, reading)? {
                    0 => words::models::already_in_that_order().to_owned(),
                    moved => words::models::moved_rows_into_that_order(moved),
                }))
            },
            cx,
        );
    }

    pub fn keep_playlist_in_order(
        &mut self,
        id: PlaylistId,
        kept: Option<Kept>,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Change::Playlist,
            move |library| {
                let moved = library.keep_playlist_in_order(id, kept)?;
                let Some(_) = kept else {
                    return Ok(Some(
                        words::models::back_in_hand_a_row_added_now().to_owned(),
                    ));
                };

                Ok(Some(match moved {
                    0 => words::models::kept_in_that_order_and_it_was().to_owned(),
                    moved => words::models::kept_in_that_order_and_rows_moved(moved),
                }))
            },
            cx,
        );
    }

    pub fn tidy_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        self.edit(
            Change::Playlist,
            move |library| {
                Ok(Some(match library.tidy_playlist(id)? {
                    0 => words::models::every_row_names_a_file_that_is().to_owned(),
                    removed => words::models::removed_missing_or_doubled_rows(removed),
                }))
            },
            cx,
        );
    }
}
