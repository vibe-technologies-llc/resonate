use std::time::SystemTime;

use resonate_core::{Chromaprint, FrameSpan, MediaLocation, TrackId};

use super::Library;
use crate::{
    Fingerprinters, Result, Study, enriched,
    studies::{self, Agreement, Heard, HeardAs, Studied, StudiedTrack, StudyFilter, ToStudy},
};

impl Library {
    pub fn study_of(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<(TrackId, Studied)>> {
        self.inner
            .read(|connection| studies::study_of(connection, location, span))
    }

    pub fn track_held(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<TrackId>> {
        self.inner
            .read(|connection| studies::track_held(connection, location, span))
    }

    pub fn note_study(&self, track: TrackId, study: &Study) -> Result<()> {
        self.inner
            .write(|transaction| studies::write_study(transaction, track, study, SystemTime::now()))
    }

    pub(crate) fn note_band_figures(
        &self,
        track: TrackId,
        figures: &resonate_analysis::BandFigures,
    ) -> Result<bool> {
        self.inner
            .write(|transaction| studies::write_band_figures(transaction, track, figures))
    }

    pub fn loudest_kept(
        &self,
        gains: &resonate_analysis::BandGains,
    ) -> Result<resonate_analysis::Loudest> {
        self.inner
            .read(|connection| studies::loudest_kept(connection, gains))
    }

    pub fn unmeasured(
        &self,
        cuts: &[resonate_analysis::Cut],
    ) -> Result<Vec<resonate_analysis::Cut>> {
        self.inner
            .read(|connection| studies::unmeasured(connection, cuts))
    }

    pub(crate) fn note_unstudied(&self, track: TrackId) -> Result<()> {
        self.inner
            .write(|transaction| studies::write_unstudied(transaction, track))
    }

    pub(crate) fn will_not_study(&self, track: TrackId) -> Result<bool> {
        self.inner
            .read(|connection| studies::will_not_study(connection, track))
    }

    pub fn take_what_was_heard(&self, track: TrackId) -> Result<Option<HeardAs>> {
        self.inner.write(|transaction| {
            enriched::land_what_was_heard(transaction, track, SystemTime::now())
        })
    }

    pub fn note_recognition(
        &self,
        track: TrackId,
        heard: Option<&HeardAs>,
        agreement: Agreement,
    ) -> Result<bool> {
        self.inner.write(|transaction| {
            studies::write_recognition(transaction, track, heard, agreement, SystemTime::now())
        })
    }

    pub fn recognise(
        &self,
        fingerprinters: &Fingerprinters,
        track: TrackId,
        print: Chromaprint,
    ) -> Option<Heard> {
        studies::heard_and_noted(self, fingerprinters, track, print)
    }

    pub fn studies(&self, filter: StudyFilter) -> Result<Vec<StudiedTrack>> {
        self.inner
            .read(|connection| studies::studied(connection, filter))
    }

    pub(crate) fn print_held(&self, track: TrackId) -> Result<Option<Chromaprint>> {
        self.inner
            .read(|connection| studies::print_held(connection, track))
    }

    pub(crate) fn to_study(&self, again: bool) -> Result<Vec<ToStudy>> {
        self.inner
            .read(|connection| studies::to_study(connection, again))
    }
}
