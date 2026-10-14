use resonate_codec::MediaInfo;
use rusqlite::{OptionalExtension, params};

use crate::{
    Error, Result, StoreOp, Track,
    catalog::{Catalog, read},
    features::{AnalysisReport, LibraryStatistics, TrackFilter, TrackOrder},
};

type StoredLyrics = (Option<String>, Option<String>, String);

impl Catalog {
    pub(crate) fn tagged(&self, id: u64, info: &MediaInfo) -> Result<()> {
        let tags = &info.tags;
        self.0.lock().execute("UPDATE documents SET album_artist = coalesce(nullif(?2, ''), album_artist), genre = coalesce(nullif(?3, ''), genre), date = coalesce(nullif(?4, ''), date), track_number = ?5, disc_number = ?6, bits = ?7, channels = ?8, recording = coalesce(?9, recording), release = coalesce(?10, release), artist_mbid = coalesce(?11, artist_mbid) WHERE id = ?1", params![id as i64, tags.album_artist, tags.genre, tags.date, tags.track_number, tags.disc_number, info.bits_per_coded_sample, info.spec.channels.count().get(), tags.musicbrainz_track_id, tags.musicbrainz_album_id, tags.musicbrainz_artist_id]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn artwork(&self, id: u64) -> Result<Option<Vec<u8>>> {
        self.0
            .lock()
            .query_row(
                "SELECT bytes FROM artwork WHERE track = ?1",
                [id as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn keep_artwork(
        &self,
        id: u64,
        bytes: &[u8],
        origin: &str,
        replace: bool,
    ) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let changed = transaction.execute("INSERT INTO artwork(track, bytes, origin) VALUES (?1, ?2, ?3) ON CONFLICT(track) DO UPDATE SET bytes = excluded.bytes, origin = excluded.origin WHERE (?4 OR artwork.origin = excluded.origin) AND artwork.bytes != excluded.bytes", params![id as i64, bytes, origin, replace]).map_err(|source| Error::store(StoreOp::Write, source))?;
        if changed > 0 {
            transaction
                .execute(
                    "UPDATE documents SET art_revision = art_revision + 1 WHERE id = ?1",
                    [id as i64],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub(crate) fn lyric_text(&self, id: u64) -> Result<Option<StoredLyrics>> {
        self.0
            .lock()
            .query_row(
                "SELECT text, document, origin FROM lyrics WHERE track = ?1",
                [id as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn keep_lyrics(
        &self,
        id: u64,
        text: Option<&str>,
        document: Option<&str>,
        origin: &str,
    ) -> Result<()> {
        self.0.lock().execute("INSERT INTO lyrics(track, text, document, origin) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(track) DO UPDATE SET text = excluded.text, document = excluded.document, origin = excluded.origin, asked = unixepoch()", params![id as i64, text, document, origin]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn learned(
        &self,
        id: u64,
        recording: Option<&str>,
        release: Option<&str>,
        artist: Option<&str>,
        album_artist: &str,
    ) -> Result<()> {
        self.0.lock().execute("UPDATE documents SET recording = coalesce(recording, ?2), release = coalesce(release, ?3), artist_mbid = coalesce(artist_mbid, ?4), album_artist = CASE WHEN album_artist = '' THEN ?5 ELSE album_artist END WHERE id = ?1", params![id as i64, recording, release, artist, album_artist]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn filtered(
        &self,
        filter: &TrackFilter,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Track>> {
        let columns: &[&str] = match filter.order {
            TrackOrder::Title => &["d.title COLLATE NOCASE"],
            TrackOrder::Album => &[
                "d.album COLLATE NOCASE",
                "d.album_artist COLLATE NOCASE",
                "coalesce(d.disc_number, 1)",
                "coalesce(d.track_number, 0)",
            ],
            TrackOrder::Artist => &[
                "d.artist COLLATE NOCASE",
                "d.album COLLATE NOCASE",
                "coalesce(d.disc_number, 1)",
                "coalesce(d.track_number, 0)",
            ],
            TrackOrder::Duration => &["d.duration_ms"],
            TrackOrder::Added => &["d.id"],
            TrackOrder::Plays => {
                &["(SELECT count(*) FROM listens l WHERE l.track = d.id AND l.counted = 1)"]
            }
        };
        let direction = if filter.descending { "DESC" } else { "ASC" };
        let order = columns
            .iter()
            .map(|column| format!("{column} {direction}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT d.* FROM documents d WHERE instr(folded(d.title || ' ' || d.artist || ' ' || d.album || ' ' || d.album_artist), folded(?1)) > 0 AND (?2 = '' OR folded(d.genre) = folded(?2)) AND (?3 = '' OR folded(d.artist) = folded(?3)) AND (?4 = '' OR d.album = ?4) AND d.rate >= ?5 AND coalesce(d.bits, 0) >= ?6 AND (NOT ?7 OR d.favourite = 1) AND (NOT ?8 OR NOT EXISTS (SELECT 1 FROM listens l WHERE l.track = d.id AND l.counted = 1)) ORDER BY {order}, d.id LIMIT ?9 OFFSET ?10"
        );
        let connection = self.0.lock();
        let mut query = connection
            .prepare(&sql)
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        query
            .query_map(
                params![
                    filter.text,
                    filter.genre,
                    filter.artist,
                    filter.album,
                    filter.minimum_rate,
                    filter.minimum_bits,
                    filter.favourites,
                    filter.unplayed,
                    limit.min(500),
                    offset
                ],
                read,
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn statistics(&self, days: u32) -> Result<LibraryStatistics> {
        let connection = self.0.lock();
        let (songs, albums, artists, duration_ms) = connection.query_row("SELECT count(*), count(DISTINCT album || char(0) || album_artist), count(DISTINCT artist), coalesce(sum(duration_ms), 0) FROM documents", [], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64, row.get::<_, i64>(2)? as u64, row.get::<_, i64>(3)? as u64))).map_err(|source| Error::store(StoreOp::Read, source))?;
        let (plays, listened_ms) = connection.query_row("SELECT coalesce(sum(counted), 0), coalesce(sum(heard_ms), 0) FROM listens WHERE ?1 = 0 OR began >= unixepoch() - ?1 * 86400", [days], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))).map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut query = connection.prepare("SELECT d.* FROM documents d JOIN listens l ON l.track = d.id WHERE (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) AND l.counted = 1 GROUP BY d.id ORDER BY sum(l.heard_ms) DESC, count(*) DESC, d.title LIMIT 20").map_err(|source| Error::store(StoreOp::Read, source))?;
        let top_tracks = query
            .query_map([days], read)
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(LibraryStatistics {
            songs,
            albums,
            artists,
            duration_ms,
            plays,
            listened_ms,
            top_tracks,
        })
    }

    pub(crate) fn keep_study(&self, id: u64, report: &AnalysisReport) -> Result<()> {
        self.0.lock().execute("INSERT INTO studies(track, verdict, peak, rms, true_peak, loudness, dynamic_range, clipped, bits) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(track) DO UPDATE SET verdict = excluded.verdict, peak = excluded.peak, rms = excluded.rms, true_peak = excluded.true_peak, loudness = excluded.loudness, dynamic_range = excluded.dynamic_range, clipped = excluded.clipped, bits = excluded.bits", params![id as i64, report.verdict, report.peak, report.rms, report.true_peak, report.loudness_lufs, report.dynamic_range, report.clipped.min(i64::MAX as u64) as i64, report.bits_in_use]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn rescanning_an_untagged_delivery_keeps_its_selected_names() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let track = Track {
            uri: "content://test/delivery.wav".into(),
            name: "delivery.wav".into(),
            title: "Selected title".into(),
            artist: "Selected artist".into(),
            album: "Selected album".into(),
            ..Track::default()
        };
        let saved = catalog
            .upsert(track.clone())
            .expect("selected delivery metadata");
        let rescanned = catalog
            .upsert(Track {
                title: track.name.clone(),
                artist: String::new(),
                album: String::new(),
                ..track
            })
            .expect("an untagged rescan");
        assert_eq!(saved.id, rescanned.id);
        assert_eq!(saved.title, rescanned.title);
        assert_eq!(saved.artist, rescanned.artist);
        assert_eq!(saved.album, rescanned.album);
    }

    #[test]
    fn companion_artwork_refreshes_without_replacing_an_embedded_cover() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let track = catalog
            .upsert(Track {
                uri: "content://test/art.flac".into(),
                ..Track::default()
            })
            .expect("a track");
        catalog
            .keep_artwork(track.id, &[1], "folder", false)
            .expect("a folder cover");
        catalog
            .keep_artwork(track.id, &[2], "folder", false)
            .expect("a changed folder cover");
        assert_eq!(
            catalog.artwork(track.id).expect("the changed picture"),
            Some(vec![2])
        );
        catalog
            .keep_artwork(track.id, &[3], "embedded", true)
            .expect("an embedded cover");
        catalog
            .keep_artwork(track.id, &[4], "folder", false)
            .expect("a fallback");
        catalog
            .keep_artwork(track.id, &[5], "coverartarchive", false)
            .expect("a remote fallback");
        assert_eq!(
            catalog.artwork(track.id).expect("the embedded picture"),
            Some(vec![3])
        );
        assert_eq!(
            catalog
                .track(track.id)
                .expect("the updated track")
                .art_revision,
            3
        );
    }

    #[test]
    fn every_document_order_and_direction_is_valid_and_search_folds_unicode() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        catalog
            .upsert(Track {
                uri: "content://test/song.flac".into(),
                title: "Björk — Song".into(),
                ..Track::default()
            })
            .expect("an indexed song");
        for order in [
            TrackOrder::Title,
            TrackOrder::Album,
            TrackOrder::Artist,
            TrackOrder::Duration,
            TrackOrder::Added,
            TrackOrder::Plays,
        ] {
            for descending in [true, false] {
                let filter = TrackFilter {
                    text: "BJÖRK".into(),
                    order,
                    descending,
                    ..TrackFilter::default()
                };
                assert_eq!(
                    catalog
                        .filtered(&filter, 0, 100)
                        .expect("a filtered ordering")
                        .len(),
                    1
                );
            }
        }
    }
}
