use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{AlbumId, ArtistId};
use rusqlite::{OptionalExtension as _, params, types::Value};

use super::{
    Library,
    rows::{read_link, rows},
    statements::LINKS_OF_UNPICTURED_ARTISTS,
};
use crate::{
    CoverArt, Error, ImageFormat, Link, Mbid, PortraitWanted, Result, StoreOp, enriched,
    model::CoverWanted, store,
};

const FOUND_COVERS_KEPT_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

const FOUND_COVERS_HELD: i64 = 512;

const FOUND_COVERS_BUDGET: i64 = 64 * 1024 * 1024;

const FOUND_COVER_BYTES_AT_MOST: usize = 2 * 1024 * 1024;

fn kept_since(now: SystemTime) -> i64 {
    store::to_nanos(now.checked_sub(FOUND_COVERS_KEPT_FOR).unwrap_or(UNIX_EPOCH))
}

impl Library {
    pub fn found_cover(&self, release: &Mbid) -> Result<Option<CoverArt>> {
        self.found_cover_at(release, SystemTime::now())
    }

    pub(crate) fn found_cover_at(
        &self,
        release: &Mbid,
        now: SystemTime,
    ) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT picture FROM found_covers WHERE release = ?1 AND kept >= ?2",
                    params![release.as_str(), kept_since(now)],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        Ok(stored
            .and_then(|bytes| ImageFormat::sniff(&bytes).map(|format| CoverArt { format, bytes })))
    }

    pub fn keep_found_cover(&self, release: &Mbid, art: &CoverArt) -> Result<bool> {
        self.keep_found_cover_at(release, art, SystemTime::now())
    }

    pub(crate) fn keep_found_cover_at(
        &self,
        release: &Mbid,
        art: &CoverArt,
        now: SystemTime,
    ) -> Result<bool> {
        if art.bytes.is_empty() || art.bytes.len() > FOUND_COVER_BYTES_AT_MOST {
            return Ok(false);
        }

        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT OR REPLACE INTO found_covers (release, kept, picture)
                     VALUES (?1, ?2, ?3)",
                    params![release.as_str(), store::to_nanos(now), art.bytes],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            transaction
                .execute(
                    "DELETE FROM found_covers
                     WHERE kept < ?1 OR id IN (
                       SELECT id FROM (
                         SELECT id, row_number() OVER newest AS position,
                                sum(length(picture)) OVER newest AS bytes
                         FROM found_covers
                         WINDOW newest AS (ORDER BY id DESC)
                       ) WHERE position > ?2 OR bytes > ?3
                     )",
                    params![kept_since(now), FOUND_COVERS_HELD, FOUND_COVERS_BUDGET],
                )
                .map(|_| true)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub fn ask_again_for_covers(&self) -> Result<usize> {
        self.inner.write(enriched::ask_again_for_covers)
    }

    pub(crate) fn albums_wanting_a_cover(&self) -> Result<Vec<CoverWanted>> {
        self.inner
            .read(|connection| enriched::albums_wanting_a_cover(connection, SystemTime::now()))
    }

    pub(crate) fn albums_owed_a_larger_cover(&self) -> Result<Vec<CoverWanted>> {
        self.inner.read(enriched::albums_owed_a_larger_cover)
    }

    pub(crate) fn larger_covers_asked(&self) -> Result<()> {
        self.inner.write(enriched::larger_covers_asked)
    }

    pub(crate) fn note_cover_asked(&self, album: AlbumId) -> Result<()> {
        self.inner
            .write(|transaction| enriched::note_cover_asked(transaction, album, SystemTime::now()))
    }

    pub(crate) fn note_portrait_asked(&self, artist: ArtistId) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::note_portrait_asked(transaction, artist, SystemTime::now())
        })
    }

    pub fn artists_wanting_a_portrait(&self) -> Result<Vec<PortraitWanted>> {
        let asked_before = enriched::portraits_asked_before(SystemTime::now());
        self.inner.read(|connection| {
            let held: Vec<(i64, Link)> = rows(
                connection,
                LINKS_OF_UNPICTURED_ARTISTS,
                vec![Value::from(asked_before)],
                |row| {
                    let artist: i64 = row.get(0)?;

                    Ok(read_link(row, 1)?.map(|link| (artist, link)))
                },
            )?;

            let mut wanting: Vec<PortraitWanted> = Vec::new();

            for (artist, link) in held {
                match wanting.last_mut() {
                    Some(last) if last.artist.get() as i64 == artist => last.links.push(link),
                    _ => wanting.push(PortraitWanted {
                        artist: ArtistId::new(artist as u64)?,
                        links: vec![link],
                    }),
                }
            }

            Ok(wanting
                .into_iter()
                .filter(|wanted| crate::may_be_pictured(&wanted.links))
                .collect())
        })
    }

    pub fn portrait(&self, id: ArtistId) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT portrait, portrait_format FROM artists WHERE id = ?1",
                    params![id.get() as i64],
                    |row| {
                        Ok((
                            row.get::<_, Option<Vec<u8>>>(0)?,
                            row.get::<_, Option<i64>>(1)?,
                        ))
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        let Some((Some(bytes), code)) = stored else {
            return Ok(None);
        };
        let format = match code {
            Some(code) => store::portrait_format_of(code, id)?,
            None => ImageFormat::sniff(&bytes).ok_or(Error::UntypedPortrait { artist: id })?,
        };

        Ok(Some(CoverArt { format, bytes }))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{FOUND_COVER_BYTES_AT_MOST, FOUND_COVERS_HELD, FOUND_COVERS_KEPT_FOR};
    use crate::{CoverArt, ImageFormat, Library, Mbid};

    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn release(n: u32) -> Mbid {
        Mbid::new(&format!("00000000-0000-4000-8000-{n:012}")).expect("an mbid")
    }

    fn a_cover(length: usize) -> CoverArt {
        let mut bytes = PNG_MAGIC.to_vec();

        bytes.resize(length.max(PNG_MAGIC.len()), 7);
        CoverArt {
            format: ImageFormat::Png,
            bytes,
        }
    }

    #[test]
    fn a_found_cover_is_read_back_until_its_month_is_out() {
        let library = Library::open_in_memory().expect("a catalog in memory");
        let kept_at = SystemTime::now();
        let cover = a_cover(64);

        assert!(
            library
                .keep_found_cover_at(&release(1), &cover, kept_at)
                .expect("the cover is kept")
        );
        assert_eq!(
            library
                .found_cover_at(&release(1), kept_at + Duration::from_secs(60))
                .expect("the cover is read"),
            Some(cover)
        );
        assert_eq!(
            library
                .found_cover_at(
                    &release(1),
                    kept_at + FOUND_COVERS_KEPT_FOR + Duration::from_secs(1)
                )
                .expect("the cover is read"),
            None
        );
        assert_eq!(
            library.found_cover(&release(2)).expect("nothing is read"),
            None
        );
    }

    #[test]
    fn the_newest_found_covers_are_kept_and_the_oldest_give_way() {
        let library = Library::open_in_memory().expect("a catalog in memory");
        let now = SystemTime::now();
        let held = u32::try_from(FOUND_COVERS_HELD).expect("a count that fits");

        for n in 0..=held {
            library
                .keep_found_cover_at(&release(n), &a_cover(16), now)
                .expect("the cover is kept");
        }

        assert_eq!(
            library.found_cover_at(&release(0), now).expect("read"),
            None
        );
        assert!(
            library
                .found_cover_at(&release(held), now)
                .expect("read")
                .is_some()
        );
    }

    #[test]
    fn a_cover_too_large_to_keep_is_left_unkept() {
        let library = Library::open_in_memory().expect("a catalog in memory");

        assert!(
            !library
                .keep_found_cover(&release(1), &a_cover(FOUND_COVER_BYTES_AT_MOST + 1))
                .expect("nothing failed")
        );
        assert_eq!(library.found_cover(&release(1)).expect("read"), None);
    }
}
