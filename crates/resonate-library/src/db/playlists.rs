use std::{path::Path, sync::atomic::Ordering};

use resonate_core::{AlbumId, PlaylistId, Span};

use super::Library;
use crate::{
    Cut, Direction, Exported, Imported, Kept, NamedPlaylist, Playlist, PlaylistEntry,
    PlaylistOrder, Result, RowOrder, SavedQuery, Undoable,
    playlist::{self},
    undo::{self},
};

impl Library {
    pub fn playlists(
        &self,
        order: PlaylistOrder,
        direction: Direction,
        named: Option<&str>,
    ) -> Result<Vec<Playlist>> {
        playlist::all(&self.inner, order, direction, named)
    }

    pub fn playlists_within(
        &self,
        order: PlaylistOrder,
        direction: Direction,
        named: Option<&str>,
        from: usize,
        most: usize,
    ) -> Result<Vec<Playlist>> {
        playlist::within(&self.inner, order, direction, named, from, Some(most))
    }

    pub fn playlist_lists(
        &self,
        order: PlaylistOrder,
        direction: Direction,
    ) -> Result<Vec<Playlist>> {
        playlist::lists(&self.inner, order, direction)
    }

    pub fn pinned_playlists(&self, most: usize) -> Result<Vec<NamedPlaylist>> {
        playlist::pinned(&self.inner, most)
    }

    pub fn playlist_names(
        &self,
        order: PlaylistOrder,
        direction: Direction,
        from: usize,
        most: Option<usize>,
    ) -> Result<Vec<NamedPlaylist>> {
        playlist::names(&self.inner, order, direction, from, most)
    }

    pub fn playlist_count(&self, named: Option<&str>) -> Result<u32> {
        playlist::count(&self.inner, named)
    }

    pub fn playlist(&self, id: PlaylistId) -> Result<Option<Playlist>> {
        playlist::one(&self.inner, id)
    }

    pub fn playlist_named(&self, name: &str) -> Result<Option<Playlist>> {
        playlist::named(&self.inner, name)
    }

    pub fn playlist_entries(
        &self,
        id: PlaylistId,
        matching: Option<&str>,
    ) -> Result<Vec<PlaylistEntry>> {
        playlist::entries(&self.inner, id, matching)
    }

    pub fn playlist_entries_within(
        &self,
        id: PlaylistId,
        matching: Option<&str>,
        most: usize,
    ) -> Result<(Vec<PlaylistEntry>, usize)> {
        playlist::entries_within(&self.inner, id, matching, Some(most))
    }

    pub fn playlist_cuts(&self, id: PlaylistId) -> Result<Vec<Cut>> {
        playlist::cuts(&self.inner, id)
    }

    pub fn create_playlist(&self, name: &str) -> Result<PlaylistId> {
        playlist::create(&self.inner, name)
    }

    pub fn start_playlist(&self, name: &str, cuts: &[Cut]) -> Result<PlaylistId> {
        playlist::start(&self.inner, name, cuts)
    }

    pub fn save_query(&self, name: &str, query: &SavedQuery) -> Result<PlaylistId> {
        playlist::save_query(&self.inner, name, query)
    }

    pub fn revise_query(&self, id: PlaylistId, name: &str, query: &SavedQuery) -> Result<()> {
        playlist::revise(&self.inner, id, name, query)
    }

    pub fn rename_playlist(&self, id: PlaylistId, name: &str) -> Result<()> {
        playlist::rename(&self.inner, id, name)
    }

    pub fn duplicate_playlist(&self, id: PlaylistId) -> Result<PlaylistId> {
        playlist::duplicate(&self.inner, id)
    }

    pub fn playlist_pictures(&self, id: PlaylistId, at_most: usize) -> Result<Vec<AlbumId>> {
        playlist::pictures(&self.inner, id, at_most)
    }

    pub fn remove_playlist(&self, id: PlaylistId) -> Result<bool> {
        playlist::remove(&self.inner, id)
    }

    pub fn pin_playlist(&self, id: PlaylistId, pinned: bool) -> Result<bool> {
        playlist::pin(&self.inner, id, pinned)
    }

    pub fn add_to_playlist(&self, id: PlaylistId, cuts: &[Cut]) -> Result<usize> {
        playlist::add(&self.inner, id, cuts)
    }

    pub fn insert_into_playlist(&self, id: PlaylistId, cuts: &[Cut], at: usize) -> Result<usize> {
        playlist::insert_at(&self.inner, id, cuts, at)
    }

    pub fn copy_playlist(
        &self,
        from: PlaylistId,
        into: PlaylistId,
        matching: Option<&str>,
    ) -> Result<usize> {
        playlist::copy(&self.inner, from, into, matching)
    }

    pub fn remove_from_playlist(&self, id: PlaylistId, rows: Span) -> Result<bool> {
        playlist::remove_rows(&self.inner, id, rows)
    }

    pub fn remove_runs_from_playlist(&self, id: PlaylistId, runs: &[Span]) -> Result<usize> {
        playlist::remove_runs(&self.inner, id, runs)
    }

    pub fn gather_in_playlist(&self, id: PlaylistId, runs: &[Span], onto: usize) -> Result<bool> {
        playlist::gather_rows(&self.inner, id, runs, onto)
    }

    pub fn remove_matching(&self, id: PlaylistId, matching: &str) -> Result<usize> {
        playlist::remove_matching(&self.inner, id, matching)
    }

    pub fn move_in_playlist(&self, id: PlaylistId, rows: Span, to: usize) -> Result<bool> {
        playlist::move_rows(&self.inner, id, rows, to)
    }

    pub fn sort_playlist(
        &self,
        id: PlaylistId,
        order: RowOrder,
        direction: Direction,
    ) -> Result<usize> {
        playlist::sort_rows(&self.inner, id, order, direction)
    }

    pub fn keep_playlist_in_order(&self, id: PlaylistId, kept: Option<Kept>) -> Result<usize> {
        playlist::keep(&self.inner, id, kept)
    }

    pub fn prune_playlist(&self, id: PlaylistId) -> Result<usize> {
        playlist::prune(&self.inner, id)
    }

    pub fn tidy_playlist(&self, id: PlaylistId) -> Result<usize> {
        playlist::tidy(&self.inner, id)
    }

    pub fn fold_doubles(&self, id: PlaylistId) -> Result<usize> {
        playlist::fold_doubles(&self.inner, id)
    }

    pub fn import_playlist(&self, path: &Path, name: Option<&str>) -> Result<Imported> {
        let imported = playlist::import(&self.inner, path, name)?;
        self.inner.restate_the_statistics();
        Ok(imported)
    }

    pub fn export_playlist(&self, id: PlaylistId, path: &Path) -> Result<Exported> {
        playlist::export(self, id, path)
    }

    pub fn undo(&self) -> Result<Option<Undoable>> {
        undo::undo(&self.inner)
    }

    pub fn undoable(&self) -> Option<Undoable> {
        undo::undoable(&self.inner)
    }

    pub fn redo(&self) -> Result<Option<Undoable>> {
        undo::redo(&self.inner)
    }

    pub fn redoable(&self) -> Option<Undoable> {
        undo::redoable(&self.inner)
    }

    pub fn offer_edits_as_a_model(&self) {
        self.inner.offering.store(true, Ordering::Release);
    }

    pub fn take_what_a_model_offered(&self) -> Result<usize> {
        undo::take_offered(&self.inner)
    }
}
