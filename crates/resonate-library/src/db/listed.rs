use resonate_core::{AlbumId, ArtistId};
use rusqlite::{OptionalExtension as _, params, params_from_iter, types::Value};

use super::{
    Library,
    ordered::{album_order_by, artist_order_by},
    rows::{held_cover, limit, read_album, read_artist, rows},
    searched::{measured, played, scoped_albums, scoped_artists},
    statements::{
        A_FAVOURITE_ALBUM, A_FAVOURITE_ARTIST, WHAT_AN_ARTIST_HOLDS, album_columns, artist_columns,
    },
};
use crate::{
    Album, AlbumQuery, Artist, ArtistQuery, ArtistTotals, CoverArt, Error, Measured, Result,
    Search, SearchResults, StoreOp, TrackQuery,
    meant::{ByArtist, Meant},
    spelling, store,
};

impl Library {
    pub fn album(&self, id: AlbumId) -> Result<Option<Album>> {
        let columns = album_columns(self.music_filters());
        self.inner.read(|connection| {
            connection
                .query_row(
                    &format!("SELECT {columns} FROM albums a WHERE a.id = ?1"),
                    params![id.get() as i64],
                    read_album,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
                .transpose()
        })
    }

    pub fn albums(&self, query: &AlbumQuery) -> Result<Vec<Album>> {
        self.listed_albums(query, None)
    }

    pub fn favourite_albums(&self, query: &AlbumQuery) -> Result<Vec<Album>> {
        self.listed_albums(query, Some(A_FAVOURITE_ALBUM))
    }

    fn listed_albums(&self, query: &AlbumQuery, only: Option<&str>) -> Result<Vec<Album>> {
        let filters = self.music_filters();
        let Some(scoped) = scoped_albums(query, only, filters) else {
            return Ok(Vec::new());
        };

        let columns = album_columns(filters);
        let mut sql = format!("SELECT {columns} FROM albums a{}", scoped.from);
        let mut binds = scoped.binds;

        sql.push_str(" ORDER BY ");
        sql.push_str(album_order_by(query.sort, query.reading, scoped.ranked));
        sql.push_str(" LIMIT ? OFFSET ?");
        binds.push(Value::Integer(limit(query.limit)));
        binds.push(Value::Integer(query.offset as i64));

        self.inner
            .read(|connection| rows(connection, &sql, binds, read_album))
    }

    pub fn albums_counted(&self, query: &AlbumQuery) -> Result<u32> {
        let Some(scoped) = scoped_albums(query, None, self.music_filters()) else {
            return Ok(0);
        };

        self.counted(
            &format!("SELECT count(*) FROM albums a{}", scoped.from),
            scoped.binds,
        )
    }

    pub fn artists_counted(&self, query: &ArtistQuery) -> Result<u32> {
        let Some(scoped) = scoped_artists(query, None, self.music_filters()) else {
            return Ok(0);
        };

        self.counted(
            &format!("SELECT count(*) FROM artists r{}", scoped.from),
            scoped.binds,
        )
    }

    pub fn measured(&self, query: &TrackQuery) -> Result<Measured> {
        measured(&self.inner, query)
    }

    pub fn artist_totals(&self, id: ArtistId) -> Result<ArtistTotals> {
        let mut sql = WHAT_AN_ARTIST_HOLDS.to_owned();
        if let Some(predicate) = self.music_filters().predicate("tracks") {
            sql.push_str(" AND ");
            sql.push_str(&predicate);
        }
        self.inner.read(|connection| {
            connection
                .query_row(&sql, [id.get() as i64], |row| {
                    Ok(ArtistTotals {
                        albums: row.get::<_, i64>(0)? as u32,
                        tracks: row.get::<_, i64>(1)? as u32,
                        length: played(row.get::<_, Option<f64>>(2)?),
                        plays: row.get::<_, Option<i64>>(3)?.unwrap_or_default() as u32,
                        played: row.get::<_, Option<i64>>(4)?.map(store::from_nanos),
                    })
                })
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub(super) fn counted(&self, sql: &str, binds: Vec<Value>) -> Result<u32> {
        self.inner.read(|connection| {
            connection
                .query_row(sql, params_from_iter(binds), |row| row.get::<_, i64>(0))
                .map(|rows| rows as u32)
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn artists(&self, query: &ArtistQuery) -> Result<Vec<Artist>> {
        self.listed_artists(query, None)
    }

    pub fn favourite_artists(&self, query: &ArtistQuery) -> Result<Vec<Artist>> {
        self.listed_artists(query, Some(A_FAVOURITE_ARTIST))
    }

    fn listed_artists(&self, query: &ArtistQuery, only: Option<&str>) -> Result<Vec<Artist>> {
        let filters = self.music_filters();
        let Some(scoped) = scoped_artists(query, only, filters) else {
            return Ok(Vec::new());
        };

        let columns = artist_columns(filters);
        let mut sql = format!("SELECT {columns} FROM artists r{}", scoped.from);
        let mut binds = scoped.binds;
        let ranked = scoped.ranked;

        sql.push_str(" ORDER BY ");
        sql.push_str(artist_order_by(query.sort, query.reading, ranked));
        sql.push_str(" LIMIT ? OFFSET ?");
        binds.push(Value::Integer(limit(query.limit)));
        binds.push(Value::Integer(query.offset as i64));

        self.inner
            .read(|connection| rows(connection, &sql, binds, read_artist))
    }

    pub fn search(&self, text: &str, limit: usize) -> Result<SearchResults> {
        let text = Some(text.to_owned());

        Ok(SearchResults {
            tracks: self.tracks(&TrackQuery {
                text: text.clone(),
                limit: Some(limit),
                ..TrackQuery::default()
            })?,
            albums: self.albums(&AlbumQuery {
                text: text.clone(),
                limit: Some(limit),
                ..AlbumQuery::default()
            })?,
            artists: self.artists(&ArtistQuery {
                text,
                limit: Some(limit),
                ..ArtistQuery::default()
            })?,
        })
    }

    pub fn did_you_mean(&self, text: &str) -> Result<Option<String>> {
        if !spelling::worth_asking(&Search::read(text)) {
            return Ok(None);
        }

        Ok(self.inner.vocabulary()?.did_you_mean(text))
    }

    pub fn meant(&self, text: &str) -> Result<Option<Meant>> {
        let Some(by) = ByArtist::read(text) else {
            return Ok(None);
        };
        let Some(artist) = self.inner.vocabulary()?.artist_named(&by.artist) else {
            return Ok(None);
        };
        let Some(searched) = by.searched_as(&artist) else {
            return Ok(None);
        };
        let measured = self.measured(&TrackQuery {
            text: Some(searched.clone()),
            ..TrackQuery::default()
        })?;

        Ok((measured.rows > 0).then_some(Meant {
            searched,
            title: by.title,
            artist,
        }))
    }

    pub fn names_completing(&self, typed: &str) -> Result<Vec<String>> {
        Ok(self.inner.vocabulary()?.completing(typed))
    }

    pub fn cover_art(&self, id: AlbumId) -> Result<Option<CoverArt>> {
        self.inner.cover_art(id)
    }

    pub(crate) fn cover_the_vault_lacks(&self, id: AlbumId) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT cover_art, cover_format FROM albums
                      WHERE id = ?1 AND cover_path IS NULL AND cover_art IS NOT NULL",
                    params![id.get() as i64],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        stored
            .map(|(bytes, code)| held_cover(id, bytes, code))
            .transpose()
    }
}
