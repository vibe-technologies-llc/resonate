use std::sync::Arc;

use resonate_codec::{StandIn, StoodIn, TagSet};
use resonate_core::{FrameSpan, MediaLocation};
use rusqlite::{OptionalExtension as _, Row, params};

use crate::{
    Error, StoreOp,
    db::Inner,
    store::{self, path_text},
};

const STOOD_IN: &str = "SELECT t.vault_path, t.title, t.artist, coalesce(a.release_title, a.title),
            r.name, t.track_number, t.disc_number, coalesce(a.date, CAST(a.year AS TEXT)),
            t.genre, t.isrc, t.mbid, t.release_track_mbid, t.artist_mbid, a.mbid, r.mbid,
            a.release_group, t.rg_track_gain, t.rg_track_peak, t.rg_album_gain,
            t.rg_album_peak, t.lyrics
       FROM tracks t
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = a.artist_id
      WHERE t.path = ?1 AND t.span_start = ?2 AND t.vault_path IS NOT NULL";

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
        let start = span.map_or(0, |span| span.start().get() as i64);

        let found = self.inner.read(|connection| {
            connection
                .query_row(STOOD_IN, params![path, start], stood_in)
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
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

fn stood_in(row: &Row<'_>) -> rusqlite::Result<(String, TagSet)> {
    let object: String = row.get(0)?;
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

    Ok((object, tags))
}
