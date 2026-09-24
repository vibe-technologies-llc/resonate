use resonate_codec::{Drawing, Likeness};
use resonate_core::AlbumId;
use rusqlite::{OptionalExtension, params};

use crate::{Error, Result, StoreOp, db::Inner};

pub(crate) fn of(inner: &Inner, album: AlbumId, picture: &str) -> Result<Option<Likeness>> {
    let kept = inner.read(|connection| {
        connection
            .query_row(
                "SELECT likeness FROM likenesses WHERE picture = ?1",
                params![picture],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    if let Some(kept) = kept {
        return Ok(kept.as_deref().and_then(Likeness::from_bytes));
    }

    let art = match inner.cover_art(album) {
        Ok(Some(art)) => art,
        Ok(None) => return Ok(None),
        Err(error) => {
            tracing::debug!(%error, album = album.get(), "a cover went unweighed for its likeness");
            return Ok(None);
        }
    };
    let likeness = Drawing::of(&art).and_then(|drawing| drawing.likeness());
    inner.write(|transaction| {
        transaction
            .execute(
                "INSERT OR REPLACE INTO likenesses (picture, likeness) VALUES (?1, ?2)",
                params![picture, likeness.as_ref().map(Likeness::as_bytes)],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))
    })?;
    Ok(likeness)
}
