use std::time::SystemTime;

use resonate_core::{AlbumId, ArtistId};
use rusqlite::{OptionalExtension as _, params, types::Value};

use super::{
    Library,
    rows::{read_link, rows},
    statements::LINKS_OF_UNPICTURED_ARTISTS,
};
use crate::{
    CoverArt, Error, ImageFormat, Link, PortraitWanted, Result, StoreOp, enriched,
    model::CoverWanted, store,
};

impl Library {
    pub fn ask_again_for_covers(&self) -> Result<usize> {
        self.inner.write(enriched::ask_again_for_covers)
    }

    pub(crate) fn albums_wanting_a_cover(&self) -> Result<Vec<CoverWanted>> {
        self.inner
            .read(|connection| enriched::albums_wanting_a_cover(connection, SystemTime::now()))
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
