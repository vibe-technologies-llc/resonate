use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};

use ahash::{AHashMap, AHashSet};
use resonate_core::AlbumId;
use resonate_vault::Encoding;
use rusqlite::{OptionalExtension as _, params, params_from_iter, types::Value};

use super::{
    Library, PATHS_ASKED_AT_ONCE, and_roots, counted_by_album, paged_under_roots, read_filing,
    rooted,
    rows::{
        RawFiled, RawRelease, RawReleaseTrack, RawToVault, TrackToVault, grouped_links, read_link,
        rows,
    },
    statements::{
        ALBUM_DISCS, FILED_IN_ORDER, FILES_TO_FILE, LINKS_OF_RELEASE_TRACKS, RELEASE_TRACK_COLUMNS,
        TRACKS_TO_FILE, TRACKS_TO_VAULT,
    },
    tracks_among, under_roots,
};
use crate::{
    Error, HeldMedium, HeldReleaseTrack, ReleaseDetail, Result, StoreOp,
    organise::{Filing, TrackToFile},
    store,
};

impl Library {
    pub(crate) fn tracks_to_vault(&self, roots: &[PathBuf]) -> Result<Vec<TrackToVault>> {
        let named = rooted(roots)?;
        let sql = and_roots(TRACKS_TO_VAULT, named.len());
        let this_build = Value::Integer(i64::from(Encoding::OF_THIS_BUILD.get()));
        let keyed_by_the_sound = Value::Integer(i64::from(Encoding::KEYED_BY_THE_SOUND.get()));
        let mut asked = vec![keyed_by_the_sound, this_build.clone(), this_build];
        asked.extend(named);

        self.inner.read(|connection| {
            rows(connection, &sql, asked.clone(), |row| {
                RawToVault::read(row).map(Ok)
            })?
            .into_iter()
            .map(RawToVault::into_vaultable)
            .collect()
        })
    }

    pub(crate) fn album_discs(&self) -> Result<AHashMap<i64, u32>> {
        self.inner
            .read(|connection| counted_by_album(connection, ALBUM_DISCS))
    }

    pub(crate) fn each_file_to_file(
        &self,
        roots: &[PathBuf],
        mut each: impl FnMut(Filing),
    ) -> Result<()> {
        let named = rooted(roots)?;
        let sql = under_roots(FILES_TO_FILE, named.len());

        self.inner.read(|connection| {
            let mut statement = connection
                .prepare(&sql)
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let mut held = statement
                .query(params_from_iter(named))
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            while let Some(row) = held
                .next()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            {
                let (path, root, album, disc) =
                    read_filing(row).map_err(|source| Error::store(StoreOp::Query, source))?;
                each(Filing {
                    path: store::path_of(&path),
                    root: store::path_of(&root),
                    album: album.map(|id| AlbumId::new(id as u64)).transpose()?,
                    disc: disc
                        .and_then(|number| u32::try_from(number).ok())
                        .and_then(NonZeroU32::new),
                });
            }
            Ok(())
        })
    }

    pub(crate) fn tracks_among(
        &self,
        roots: &[PathBuf],
        paths: &[&Path],
    ) -> Result<AHashSet<PathBuf>> {
        let named = rooted(roots)?;
        let mut tracked = AHashSet::new();

        for chunk in paths.chunks(PATHS_ASKED_AT_ONCE) {
            let mut binds = named.clone();
            binds.extend(
                chunk
                    .iter()
                    .map(|path| Value::Text(store::path_text(path).into_owned())),
            );
            let sql = tracks_among(named.len(), chunk.len());
            let found = self.inner.read(|connection| {
                rows(connection, &sql, binds, |row| {
                    row.get::<_, String>(0).map(Ok)
                })
            })?;
            tracked.extend(found.iter().map(|path| store::path_of(path)));
        }

        Ok(tracked)
    }

    pub(crate) fn tracks_to_file_after(
        &self,
        roots: &[PathBuf],
        after: Option<&Path>,
        at_most: usize,
        discs: &AHashMap<i64, u32>,
    ) -> Result<Vec<TrackToFile>> {
        let mut binds = rooted(roots)?;
        let sql = paged_under_roots(TRACKS_TO_FILE, binds.len(), after.is_some());
        if let Some(after) = after {
            binds.push(Value::Text(store::path_text(after).into_owned()));
        }
        binds.push(Value::Integer(i64::try_from(at_most).unwrap_or(i64::MAX)));
        self.filed_rows(&sql, binds, discs)
    }

    pub(crate) fn tracks_to_file_at(
        &self,
        roots: &[PathBuf],
        files: &[PathBuf],
        discs: &AHashMap<i64, u32>,
    ) -> Result<Vec<TrackToFile>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let mut binds = rooted(roots)?;
        let mut clauses = Vec::with_capacity(2);
        if !binds.is_empty() {
            clauses.push(format!(
                "roots.path IN ({})",
                vec!["?"; binds.len()].join(", ")
            ));
        }
        clauses.push(format!(
            "tracks.path IN ({})",
            vec!["?"; files.len()].join(", ")
        ));
        for file in files {
            binds.push(Value::Text(store::path_text(file).into_owned()));
        }
        let sql = format!(
            "{TRACKS_TO_FILE} WHERE {}{FILED_IN_ORDER}",
            clauses.join(" AND ")
        );
        self.filed_rows(&sql, binds, discs)
    }

    fn filed_rows(
        &self,
        sql: &str,
        binds: Vec<Value>,
        discs: &AHashMap<i64, u32>,
    ) -> Result<Vec<TrackToFile>> {
        self.inner.read(|connection| {
            rows(connection, sql, binds, |row| RawFiled::read(row).map(Ok))?
                .into_iter()
                .map(|raw| raw.into_filed(discs))
                .collect()
        })
    }

    pub fn release_of(&self, id: AlbumId) -> Result<Option<ReleaseDetail>> {
        let album = id.get() as i64;
        self.inner.read(|connection| {
            let Some(raw) = connection
                .query_row(
                    "SELECT mbid, release_group, date, country, label, catalog_number, barcode,
                            kind, disambiguation, cover_source, asked, answered,
                            front_cover IS NOT 0
                       FROM albums WHERE id = ?1",
                    params![album],
                    RawRelease::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };
            let links = rows(
                connection,
                "SELECT relation, provider, url FROM album_links WHERE album_id = ?1 ORDER BY url",
                vec![Value::Integer(album)],
                |row| read_link(row, 0),
            )?;
            let media = rows(
                connection,
                "SELECT position, format, title FROM release_media
                  WHERE album_id = ?1 ORDER BY position",
                vec![Value::Integer(album)],
                |row| {
                    Ok(Ok(HeldMedium {
                        position: row.get::<_, i64>(0)? as u32,
                        format: row.get(1)?,
                        title: row.get(2)?,
                    }))
                },
            )?;

            raw.into_detail(id, links, media).map(Some)
        })
    }

    pub fn release_tracks(&self, id: AlbumId) -> Result<Vec<HeldReleaseTrack>> {
        let album = id.get() as i64;
        self.inner.read(|connection| {
            let held = rows(
                connection,
                &format!(
                    "SELECT {RELEASE_TRACK_COLUMNS} FROM release_tracks
                      WHERE album_id = ?1 ORDER BY disc, position"
                ),
                vec![Value::Integer(album)],
                |row| RawReleaseTrack::read(row).map(Ok),
            )?;
            let mut links = grouped_links(
                connection,
                LINKS_OF_RELEASE_TRACKS,
                vec![Value::Integer(album)],
            )?;

            held.into_iter()
                .map(|raw| {
                    let links = links.remove(&raw.id).unwrap_or_default();
                    raw.into_held(id, links)
                })
                .collect()
        })
    }
}
