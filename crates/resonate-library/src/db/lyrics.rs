use std::time::SystemTime;

use resonate_core::{FrameSpan, MediaLocation, TrackId};

use super::Library;
use crate::{
    KeptLyrics, LyricText, LyricsAhead, Result,
    playlist::{self},
    store, sung,
};

impl Library {
    pub fn kept_lyrics(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<KeptLyrics>> {
        let path = playlist::local_path(location)?;
        let (start, _) = store::span_columns(span);

        self.inner
            .read(|connection| sung::kept(connection, &path, start))
    }

    pub fn keep_lyrics(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        told: Option<&LyricText>,
    ) -> Result<bool> {
        let path = playlist::local_path(location)?;
        let (start, _) = store::span_columns(span);

        self.inner
            .write(|transaction| sung::keep(transaction, &path, start, told, SystemTime::now()))
    }

    pub fn lyrics_ahead(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<LyricsAhead> {
        let path = playlist::local_path(location)?;
        let (start, _) = store::span_columns(span);

        self.inner
            .read(|connection| sung::ahead(connection, &path, start))
    }

    pub fn hold_lyrics_ahead(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        ahead: LyricsAhead,
    ) -> Result<()> {
        let path = playlist::local_path(location)?;
        let (start, _) = store::span_columns(span);

        self.inner
            .write(|transaction| sung::hold_ahead(transaction, &path, start, ahead))
    }

    pub fn lyrics_to_ask(&self, refresh: bool) -> Result<Vec<TrackId>> {
        self.inner
            .read(|connection| sung::to_ask(connection, refresh, SystemTime::now()))
    }

    pub(crate) fn lyrics_asking(&self, id: TrackId) -> Result<Option<sung::Asking>> {
        self.inner.read(|connection| sung::asking(connection, id))
    }

    pub(crate) fn note_lyrics_refused(&self, asking: &sung::Asking) -> Result<()> {
        self.inner.write(|transaction| {
            sung::note_refused(
                transaction,
                &asking.path,
                asking.span_start,
                SystemTime::now(),
            )
        })
    }

    pub(crate) fn keep_lyrics_of(
        &self,
        asking: &sung::Asking,
        told: Option<&LyricText>,
    ) -> Result<bool> {
        self.inner.write(|transaction| {
            sung::keep(
                transaction,
                &asking.path,
                asking.span_start,
                told,
                SystemTime::now(),
            )
        })
    }
}
