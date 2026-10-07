use std::sync::Arc;

use resonate_codec::{StandIn, StoodIn, TagField, TagSet};
use resonate_core::{FrameSpan, MediaLocation};
use rusqlite::{Connection, OptionalExtension as _, Row, Transaction, params};

use crate::{
    Error, StoreOp,
    db::Inner,
    store::{self, path_text},
};

const FILLED_FROM_THE_CATALOG: [TagField; 19] = [
    TagField::Title,
    TagField::Artist,
    TagField::Album,
    TagField::AlbumArtist,
    TagField::TrackNumber,
    TagField::DiscNumber,
    TagField::Date,
    TagField::Genre,
    TagField::Isrc,
    TagField::MusicBrainzTrackId,
    TagField::MusicBrainzReleaseTrackId,
    TagField::MusicBrainzArtistId,
    TagField::MusicBrainzAlbumId,
    TagField::MusicBrainzAlbumArtistId,
    TagField::MusicBrainzReleaseGroupId,
    TagField::ReplayGainTrackGain,
    TagField::ReplayGainTrackPeak,
    TagField::ReplayGainAlbumGain,
    TagField::ReplayGainAlbumPeak,
];

const KEPT: &str = "SELECT field, value FROM kept_tags WHERE track_id = ?1";

const STOOD_IN: &str = "SELECT t.vault_path, t.title, t.artist, coalesce(a.release_title, a.title),
            r.name, t.track_number, t.disc_number, coalesce(a.date, CAST(a.year AS TEXT)),
            t.genre, t.isrc, t.mbid, t.release_track_mbid, t.artist_mbid, a.mbid, r.mbid,
            a.release_group, t.rg_track_gain, t.rg_track_peak, t.rg_album_gain,
            t.rg_album_peak, t.lyrics, t.id
       FROM tracks t
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = a.artist_id
      WHERE t.path = ?1 AND t.span_start = ?2 AND t.span_frames IS ?3
        AND t.vault_path IS NOT NULL";

pub(crate) struct Vaulted {
    inner: Arc<Inner>,
}

impl Vaulted {
    pub(crate) const fn over(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}

impl StandIn for Vaulted {
    fn stands_in(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<StoodIn> {
        let path = path_text(location.as_path()?).ok()?.to_owned();
        let (start, frames) = store::span_columns(span);

        let found = self.inner.read(|connection| {
            let Some((within, id, mut tags)) = connection
                .query_row(STOOD_IN, params![path, start, frames], stood_in)
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };
            fill_what_was_kept(connection, id, &mut tags)?;
            Ok(Some((within, tags)))
        });
        let found = found.and_then(|found| {
            found
                .map(|(within, tags)| {
                    self.inner.in_the_vault(&within).map(|object| StoodIn {
                        location: MediaLocation::local(object),
                        tags,
                    })
                })
                .transpose()
        });
        match found {
            Ok(found) => found,
            Err(error) => {
                tracing::debug!(%error, %location, "the catalog could not say what stands in for a row");
                None
            }
        }
    }
}

pub(crate) fn keep_what_the_catalog_cannot_fill(
    transaction: &Transaction<'_>,
    track: i64,
    declared: &TagSet,
) -> crate::Result<()> {
    transaction
        .execute("DELETE FROM kept_tags WHERE track_id = ?1", params![track])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    let kept = TagField::ALL
        .into_iter()
        .filter(|field| !FILLED_FROM_THE_CATALOG.contains(field))
        .filter_map(|field| Some((field, field.read(declared)?)));
    for (field, value) in kept {
        transaction
            .execute(
                "INSERT INTO kept_tags (track_id, field, value) VALUES (?1, ?2, ?3)",
                params![track, field.as_str(), value],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }
    Ok(())
}

fn fill_what_was_kept(connection: &Connection, track: i64, tags: &mut TagSet) -> crate::Result<()> {
    let mut kept = connection
        .prepare_cached(KEPT)
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let rows = kept
        .query_map(params![track], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    for row in rows {
        let (field, value) = row.map_err(|source| Error::store(StoreOp::Query, source))?;
        if let Some(field) =
            TagField::named(&field).filter(|field| !FILLED_FROM_THE_CATALOG.contains(field))
        {
            field.set(tags, &value);
        }
    }
    Ok(())
}

fn stood_in(row: &Row<'_>) -> rusqlite::Result<(String, i64, TagSet)> {
    let object: String = row.get(0)?;
    let id: i64 = row.get(21)?;
    let tags = TagSet {
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        album_artist: row.get(4)?,
        track_number: row.get(5)?,
        disc_number: row.get(6)?,
        date: row.get(7)?,
        genre: row.get(8)?,
        isrc: row.get(9)?,
        musicbrainz_track_id: row.get(10)?,
        musicbrainz_release_track_id: row.get(11)?,
        musicbrainz_artist_id: row.get(12)?,
        musicbrainz_album_id: row.get(13)?,
        musicbrainz_album_artist_id: row.get(14)?,
        musicbrainz_release_group_id: row.get(15)?,
        replay_gain: store::replay_gain(row.get(16)?, row.get(17)?, row.get(18)?, row.get(19)?),
        lyrics: row.get(20)?,
        ..TagSet::default()
    };

    Ok((object, id, tags))
}
