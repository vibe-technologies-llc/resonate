use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use resonate_core::{FrameSpan, MediaLocation, PlaylistId};
use resonate_vault::Kept as VaultKept;

use super::Library;
use crate::{
    CoverArt, Cut, ImageFormat, Mbid, Result, answers,
    synced::{
        self, HeldSong, OpenedSong, SyncedListen, SyncedPlaylist, SyncedQuery, SyncedQueryBody,
    },
};

impl Library {
    pub fn synced_playlists(&self) -> Result<Vec<SyncedPlaylist>> {
        synced::playlists(&self.inner)
    }

    pub fn write_synced_playlist(
        &self,
        at: Option<(PlaylistId, i64)>,
        name: &str,
        entries: &[resonate_core::Recording],
    ) -> Result<SyncedPlaylist> {
        synced::write_playlist(&self.inner, at, name, entries)
    }

    pub fn synced_queries(&self) -> Result<Vec<SyncedQuery>> {
        synced::queries(&self.inner)
    }

    pub fn write_synced_query(
        &self,
        at: Option<(PlaylistId, i64)>,
        body: &SyncedQueryBody,
    ) -> Result<Option<SyncedQuery>> {
        synced::write_query(&self.inner, at, body)
    }

    pub fn shared_playlist(&self, id: PlaylistId, created: i64) -> Result<Option<SyncedPlaylist>> {
        synced::shared_playlist(&self.inner, id, created)
    }

    pub fn write_shared_playlist(
        &self,
        at: Option<(PlaylistId, i64)>,
        name: &str,
        entries: &[resonate_core::Recording],
        editable: bool,
    ) -> Result<SyncedPlaylist> {
        synced::write_shared_playlist(&self.inner, at, name, entries, editable)
    }

    pub fn flag_playlist_shared(
        &self,
        id: PlaylistId,
        created: i64,
        editable: bool,
    ) -> Result<Option<SyncedPlaylist>> {
        synced::flag_playlist_shared(&self.inner, id, created, editable)
    }

    pub fn unflag_playlist_shared(&self, id: PlaylistId, created: i64) -> Result<()> {
        synced::unflag_playlist_shared(&self.inner, id, created)
    }

    pub fn playlist_stamp(&self, id: PlaylistId) -> Result<i64> {
        synced::created_of(&self.inner, id)
    }

    pub fn playlists_shared(&self) -> Result<Vec<(PlaylistId, bool)>> {
        synced::shared_as(&self.inner)
    }

    pub fn remove_synced_playlist(&self, id: PlaylistId, created: i64) -> Result<bool> {
        synced::remove_playlist(&self.inner, id, created)
    }

    pub fn favoured_recordings(&self) -> Result<Vec<resonate_core::Recording>> {
        synced::favourites(&self.inner)
    }

    pub fn favour_recording(
        &self,
        recording: &resonate_core::Recording,
        favoured: bool,
    ) -> Result<bool> {
        synced::favour(&self.inner, recording, favoured)
    }

    pub fn listens_to_sync(&self, after: u64, most: usize) -> Result<Vec<SyncedListen>> {
        let settles_after = *self.inner.listens_settle_after.lock();
        synced::listens_after(&self.inner, after, most, settles_after)
    }

    pub fn settle_listens_after(&self, settles_after: Duration) {
        *self.inner.listens_settle_after.lock() = settles_after;
    }

    pub fn credit_listen_elsewhere(
        &self,
        recording: &resonate_core::Recording,
        origin: &[u8],
        listen: u64,
        at: i64,
        heard: Duration,
    ) -> Result<bool> {
        synced::credit(&self.inner, recording, origin, listen, at, heard)
    }

    pub fn answers_to_share(
        &self,
        after: i64,
        most: usize,
    ) -> Result<Vec<resonate_core::Answered>> {
        answers::after(&self.inner, after, most)
    }

    pub fn take_shared_answer(
        &self,
        recording: &resonate_core::Recording,
        answer: &resonate_core::Answer,
    ) -> Result<bool> {
        answers::take(&self.inner, recording, answer)
    }

    pub fn covers_wanted(&self, most: usize) -> Result<Vec<Mbid>> {
        answers::covers_wanted(&self.inner, most)
    }

    pub fn cover_to_share(&self, release: &Mbid) -> Result<Option<CoverArt>> {
        let Some(album) = answers::album_covering(&self.inner, release)? else {
            return Ok(None);
        };
        self.cover_art(album)
    }

    pub fn take_shared_cover(&self, release: &Mbid, picture: &[u8]) -> Result<bool> {
        let Some(format) = ImageFormat::sniff(picture) else {
            return Ok(false);
        };
        let art = CoverArt {
            format,
            bytes: picture.to_vec(),
        };
        let mut landed = false;
        for album in answers::albums_uncovered(&self.inner, release)? {
            landed |= self.land_archive_cover(album, &art)?;
        }
        Ok(landed)
    }

    pub fn lyricsfiles_wanted(
        &self,
        skip: usize,
        most: usize,
    ) -> Result<Vec<resonate_core::Recording>> {
        answers::lyricsfiles_wanted(&self.inner, skip, most)
    }

    pub fn lyricsfile_to_share(
        &self,
        recording: &resonate_core::Recording,
    ) -> Result<Option<resonate_core::SharedLyricsfile>> {
        answers::lyricsfile_to_share(&self.inner, recording)
    }

    pub fn take_shared_lyricsfile(
        &self,
        recording: &resonate_core::Recording,
        lyricsfile: &resonate_core::SharedLyricsfile,
    ) -> Result<bool> {
        answers::take_lyricsfile(&self.inner, recording, lyricsfile)
    }

    pub fn prints_wanted(&self, skip: usize, most: usize) -> Result<Vec<resonate_core::WholeFile>> {
        answers::prints_wanted(&self.inner, skip, most)
    }

    pub fn print_to_share(
        &self,
        file: resonate_core::WholeFile,
    ) -> Result<Option<resonate_core::SharedPrint>> {
        answers::print_to_share(&self.inner, file)
    }

    pub fn take_shared_print(
        &self,
        file: resonate_core::WholeFile,
        print: &resonate_core::SharedPrint,
    ) -> Result<bool> {
        answers::take_print(&self.inner, file, print)
    }

    pub fn resolve_recording(&self, recording: &resonate_core::Recording) -> Result<Option<Cut>> {
        synced::resolve(&self.inner, recording)
    }

    pub fn cut_with_isrc(&self, isrc: &str) -> Result<Option<Cut>> {
        synced::cut_with_isrc(&self.inner, isrc)
    }

    pub fn holds_recording(&self, recording: &resonate_core::Recording) -> Result<bool> {
        synced::holds(&self.inner, self.vault().map(AsRef::as_ref), recording)
    }

    pub fn deleted_recording(&self, recording: &resonate_core::Recording) -> Result<bool> {
        synced::deleted_here(&self.inner, recording)
    }

    pub fn forgive_deleted_recording(&self, recording: &resonate_core::Recording) -> Result<()> {
        synced::forgive_deletion(&self.inner, recording)
    }

    pub fn forgive_deleted_recordings(&self) -> Result<()> {
        synced::forgive_deletions(&self.inner)
    }

    pub fn held_as(
        &self,
        recording: &resonate_core::Recording,
    ) -> Result<Option<resonate_core::Recording>> {
        synced::held_as(&self.inner, recording)
    }

    pub fn recording_at(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<resonate_core::Recording>> {
        synced::recording_of(&self.inner, location, span)
    }

    pub fn held_songs(&self) -> Result<Vec<HeldSong>> {
        synced::holdings(&self.inner).map(|songs| synced::offered(self, songs))
    }

    pub fn open_song(&self, path: &Path) -> Result<Option<OpenedSong>> {
        synced::open_song(self, path)
    }

    pub fn song_tags(&self, path: &Path) -> Result<synced::HeldTags> {
        synced::tags_of(&self.inner, path)
    }

    pub(crate) fn note_song_received_at(
        &self,
        recording: &resonate_core::Recording,
        path: &Path,
        at: synced::ReceivedAt,
    ) -> Result<()> {
        synced::note_received_at(&self.inner, recording, path, at)
    }

    pub(crate) fn same_file_held(
        &self,
        file: resonate_core::WholeFile,
        landed: &Path,
    ) -> Result<Option<PathBuf>> {
        synced::same_file_held(&self.inner, self.vault().map(AsRef::as_ref), file, landed)
    }

    pub(crate) fn vaulted_already(&self, kept: &VaultKept) -> Result<Option<PathBuf>> {
        synced::vaulted_already(&self.inner, kept)
    }

    pub fn file_song_from_elsewhere(
        &self,
        recording: &resonate_core::Recording,
        tags: &synced::HeldTags,
        extension: &str,
        reader: &mut dyn std::io::Read,
    ) -> Result<synced::SongFiled> {
        synced::file_song(self, recording, tags, extension, reader)
    }
}
