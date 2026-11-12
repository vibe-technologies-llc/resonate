use std::time::SystemTime;

use resonate_core::{AlbumId, ArtistId, TrackId};
use rusqlite::{OptionalExtension as _, params};

use super::Library;
use crate::{
    AlbumToAsk, ArtistProfile, ArtistRelease, ArtistToAsk, Certainty, CoverArt, Error, Fruitless,
    Isrc, Mbid, Recording, RecordingRelease, Reference, Release, ReleaseGroup, Result, StoreOp,
    TrackToAsk, Unfinished, Waits, enrich, enriched, store,
};

impl Library {
    pub fn artist_named(&self, name: &str) -> Result<Option<ArtistId>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT id FROM artists WHERE key = ?1",
                    params![store::folded_letters(name)],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
                .map(|id| ArtistId::new(id as u64).map_err(Error::from))
                .transpose()
        })
    }

    pub fn stamp_album_asked(&self, album: AlbumId, why: Fruitless) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::stamp_album_asked(transaction, album, why, SystemTime::now())
        })
    }

    pub fn bill_an_artist(&self, name: &str, mbid: &Mbid) -> Result<ArtistId> {
        self.inner.write(|transaction| {
            let id = store::artist_named_in(transaction, name, Some(mbid.as_str()))?;
            ArtistId::new(id as u64).map_err(Error::from)
        })
    }

    pub fn settle_the_credits(&self) -> Result<()> {
        self.inner.write(store::sweep_orphans)
    }

    pub fn stamp_artist_asked(&self, artist: ArtistId, why: Fruitless) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::stamp_artist_asked(transaction, artist, why, SystemTime::now())
        })
    }

    pub fn forget_the_match(&self, album: AlbumId) -> Result<bool> {
        self.inner
            .write(|transaction| enriched::forget_the_match(transaction, album))
    }

    pub fn take_pressing(
        &self,
        album: AlbumId,
        reference: &dyn Reference,
        release: &Mbid,
    ) -> Result<bool> {
        let Some(found) = reference.release(release)? else {
            return Ok(false);
        };
        self.inner.write(|transaction| {
            enriched::forgive(transaction, album, &found.id)?;
            enriched::land_release(
                transaction,
                album,
                &found,
                enrich::Pressing::Fitted(enrich::Fit::Exact),
                SystemTime::now(),
            )
        })?;
        self.rematch(album)?;
        Ok(true)
    }

    pub fn recordings_of(
        &self,
        track: TrackId,
        reference: &dyn Reference,
    ) -> Result<Vec<Recording>> {
        let Some(asked) = self.track_as_heard(track)? else {
            return Ok(Vec::new());
        };
        enrich::recordings_to_place_on(reference, &asked)
    }

    pub fn recordings_like(
        &self,
        track: TrackId,
        reference: &dyn Reference,
        words: &str,
    ) -> Result<Vec<Recording>> {
        let Some(asked) = self.track_as_heard(track)? else {
            return Ok(Vec::new());
        };
        enrich::recordings_like(reference, &asked, words)
    }

    pub fn take_recording(
        &self,
        track: TrackId,
        reference: &dyn Reference,
        chosen: &Recording,
    ) -> Result<bool> {
        let recording = enrich::told_where_it_sits(reference, chosen.clone())?;
        let release = crate::elsewhere::in_the_order_worth_offering(&recording.releases)
            .first()
            .map(|release| (*release).clone());

        self.landed_as(
            track,
            reference,
            &recording,
            Certainty::Exactly,
            release.as_ref(),
        )
    }

    pub fn place_on(
        &self,
        track: TrackId,
        reference: &dyn Reference,
        recording: &Recording,
        release: &Mbid,
    ) -> Result<bool> {
        let Some(chosen) = recording
            .releases
            .iter()
            .find(|offered| offered.id == *release)
        else {
            return Ok(false);
        };

        self.landed_as(track, reference, recording, Certainty::Nearly, Some(chosen))
    }

    pub(super) fn landed_as(
        &self,
        track: TrackId,
        reference: &dyn Reference,
        recording: &Recording,
        certainty: Certainty,
        release: Option<&RecordingRelease>,
    ) -> Result<bool> {
        self.land_recording(track, recording, certainty, release)?;
        let Some(release) = release else {
            return Ok(true);
        };
        let album = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT album_id FROM tracks WHERE id = ?1",
                    params![track.get() as i64],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;
        match album.flatten() {
            Some(album) => self.take_pressing(AlbumId::new(album as u64)?, reference, &release.id),
            None => Ok(true),
        }
    }

    pub fn refuses(&self, album: AlbumId, mbid: &Mbid) -> Result<bool> {
        self.inner
            .read(|connection| enriched::refuses(connection, album, mbid))
    }

    pub fn land_release(&self, album: AlbumId, release: &Release) -> Result<()> {
        self.land_pressing(album, release, enrich::Pressing::Fitted(enrich::Fit::Exact))
    }

    pub(crate) fn land_pressing(
        &self,
        album: AlbumId,
        release: &Release,
        pressing: enrich::Pressing,
    ) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::land_release(transaction, album, release, pressing, SystemTime::now())
        })
    }

    pub fn stamp_track_asked(&self, track: TrackId, why: Fruitless) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::stamp_track_asked(transaction, track, why, SystemTime::now())
        })
    }

    pub fn land_recording(
        &self,
        track: TrackId,
        recording: &Recording,
        certainty: Certainty,
        release: Option<&RecordingRelease>,
    ) -> Result<bool> {
        self.inner.write(|transaction| {
            enriched::land_recording(
                transaction,
                track,
                recording,
                certainty,
                release,
                SystemTime::now(),
            )
        })
    }

    pub fn land_release_group(&self, album: AlbumId, group: &ReleaseGroup) -> Result<()> {
        self.inner.write(|transaction| {
            let now = SystemTime::now();
            enriched::stamp_album_asked(transaction, album, Fruitless::Missed, now)?;
            enriched::land_release_group(transaction, album, group, now)
        })
    }

    pub(crate) fn note_enrichment_began(&self, refresh: bool) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::note_enrichment_began(transaction, refresh, SystemTime::now())
        })
    }

    pub(crate) fn note_enrichment_finished(&self) -> Result<()> {
        self.inner.write(enriched::note_enrichment_finished)
    }

    pub fn unfinished_enrichment(&self) -> Result<Option<Unfinished>> {
        self.inner.read(enriched::unfinished_enrichment)
    }

    pub fn rematch(&self, album: AlbumId) -> Result<u32> {
        self.inner
            .write(|transaction| enriched::rematch_release_tracks(transaction, album))
    }

    pub fn rematch_each(&self, albums: &[AlbumId]) -> Result<u32> {
        if albums.is_empty() {
            return Ok(0);
        }
        self.inner.write(|transaction| {
            albums.iter().try_fold(0, |matched, album| {
                enriched::rematch_release_tracks(transaction, *album).map(|more| matched + more)
            })
        })
    }

    pub fn land_archive_cover(&self, album: AlbumId, art: &CoverArt) -> Result<bool> {
        let Some(vault) = self.inner.vault.clone() else {
            return self
                .inner
                .write(|transaction| enriched::land_archive_cover(transaction, album, art));
        };

        let kept = vault.keep_cover(art).map_err(|source| Error::Vault {
            path: vault.root().to_path_buf(),
            source: Box::new(source),
        })?;
        let within = self.inner.within_the_vault(&kept.path)?;
        self.inner
            .write(|transaction| enriched::land_vault_cover(transaction, album, kept.key, &within))
    }

    pub(crate) fn covered_by_a_thumbnail(&self, album: AlbumId) -> Result<bool> {
        self.inner
            .read(|connection| enriched::covered_by_a_thumbnail(connection, album))
    }

    pub fn land_artist(&self, artist: ArtistId, profile: &ArtistProfile) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::land_artist(transaction, artist, profile, SystemTime::now())
        })
    }

    pub fn land_artist_releases(
        &self,
        artist: ArtistId,
        releases: &[ArtistRelease],
        (unread, read_to): (u32, u32),
    ) -> Result<usize> {
        self.inner.write(|transaction| {
            enriched::land_artist_releases(
                transaction,
                artist,
                releases,
                (unread, read_to),
                enriched::Discographed::Afresh,
            )
        })
    }

    pub fn read_the_rest_of(&self, artist: ArtistId, reference: &dyn Reference) -> Result<usize> {
        let held = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT mbid, releases_read_to FROM artists
                      WHERE id = ?1 AND releases_unread > 0",
                    params![artist.get() as i64],
                    |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, u32>(1)?)),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;
        let Some((Some(mbid), read_to)) = held else {
            return Ok(0);
        };
        let Some(mbid) = store::mbid_in(Some(&mbid)) else {
            return Ok(0);
        };
        let further = reference.release_groups_of(&mbid, read_to)?;
        let kept: Vec<ArtistRelease> = further
            .releases
            .into_iter()
            .filter(enrich::worth_keeping)
            .collect();
        self.inner.write(|transaction| {
            enriched::land_artist_releases(
                transaction,
                artist,
                &kept,
                (further.unread, further.read_to.max(read_to)),
                enriched::Discographed::Further,
            )
        })
    }

    pub fn land_portrait(&self, artist: ArtistId, art: &CoverArt) -> Result<bool> {
        self.inner
            .write(|transaction| enriched::land_portrait(transaction, artist, art))
    }

    pub fn write_artist_mbid(&self, artist: ArtistId, mbid: &Mbid) -> Result<()> {
        self.inner
            .write(|transaction| enriched::write_artist_mbid(transaction, artist, mbid))
    }

    pub fn albums_to_ask(&self, waits: Waits, refresh: bool) -> Result<Vec<AlbumToAsk>> {
        self.inner.read(|connection| {
            enriched::albums_to_ask(connection, waits, refresh, SystemTime::now())
        })
    }

    pub fn album_to_ask(&self, id: AlbumId) -> Result<Option<AlbumToAsk>> {
        self.inner
            .read(|connection| enriched::album_to_ask(connection, id))
    }

    pub fn album_if_due(
        &self,
        id: AlbumId,
        waits: Waits,
        refresh: bool,
    ) -> Result<Option<AlbumToAsk>> {
        self.inner.read(|connection| {
            enriched::album_if_due(connection, id, waits, refresh, SystemTime::now())
        })
    }

    pub fn artist_is_due(&self, id: ArtistId, waits: Waits, refresh: bool) -> Result<bool> {
        self.inner.read(|connection| {
            enriched::artist_is_due(connection, id, waits, refresh, SystemTime::now())
        })
    }

    pub fn track_is_due(&self, id: TrackId, waits: Waits, refresh: bool) -> Result<bool> {
        self.inner.read(|connection| {
            enriched::track_is_due(connection, id, waits, refresh, SystemTime::now())
        })
    }

    pub fn ask_about_again(&self, track: TrackId) -> Result<bool> {
        self.inner
            .write(|transaction| enriched::ask_about_again(transaction, track))
    }

    pub fn set_isrc(&self, track: TrackId, isrc: &Isrc) -> Result<()> {
        self.inner
            .write(|transaction| enriched::set_isrc(transaction, track, isrc))
    }

    pub fn tracks_to_ask(&self, waits: Waits, refresh: bool) -> Result<Vec<TrackId>> {
        self.inner.read(|connection| {
            enriched::tracks_to_ask(connection, waits, refresh, SystemTime::now())
        })
    }

    pub(crate) fn track_as_heard(&self, id: TrackId) -> Result<Option<TrackToAsk>> {
        self.inner
            .read(|connection| enriched::track_as_heard(connection, id))
    }

    pub fn track_to_ask(&self, id: TrackId) -> Result<Option<TrackToAsk>> {
        self.inner
            .read(|connection| enriched::track_to_ask(connection, id))
    }

    pub fn artist_to_ask(&self, id: ArtistId) -> Result<Option<ArtistToAsk>> {
        self.inner
            .read(|connection| enriched::artist_to_ask(connection, id))
    }

    pub fn artists_to_ask(&self, waits: Waits, refresh: bool) -> Result<Vec<ArtistId>> {
        self.inner.read(|connection| {
            enriched::artists_to_ask(connection, waits, refresh, SystemTime::now())
        })
    }
}
