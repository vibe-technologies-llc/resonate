use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ahash::AHashSet;
use resonate_core::{AlbumId, MediaLocation, TrackId};
use resonate_vault::{Compacting, Encoding, HeldFile, Kept as VaultKept};
use rusqlite::{OptionalExtension as _, params, types::Value};

use super::{
    Applying, LANDING_GRACE, Library, and_roots, landed_before_it, note_the_delivered_object,
    rooted,
    rows::{RawVaultObject, TrackToVault, rows, still_unheld},
    statements::{TRACKS_IN_THE_VAULT, VAULT_OBJECTS, VAULT_OBJECTS_UNHELD},
    weighed_under,
};
use crate::{
    Compacted, CoverArt, Error, Holdings, Isrc, KeptCover, KeyColumn, Mbid, Pruned, Released,
    Result, StoreOp, Vault, VaultKey, VaultObject, Want, enriched, store,
    vaulted::{self},
};

impl Library {
    pub fn holdings(&self) -> Result<Holdings> {
        let Some(vault) = self.inner.vault.as_ref() else {
            return Err(Error::NoVault);
        };
        vault.holding().map_err(|source| Error::Vault {
            path: vault.root().to_path_buf(),
            source: Box::new(source),
        })
    }

    pub fn vault_objects(&self) -> Result<Vec<VaultObject>> {
        self.vault_objects_read(VAULT_OBJECTS)
    }

    pub fn vault_objects_nothing_names(&self) -> Result<Vec<VaultObject>> {
        self.vault_objects_read(VAULT_OBJECTS_UNHELD)
    }

    fn vault_objects_read(&self, sql: &str) -> Result<Vec<VaultObject>> {
        self.inner
            .read(|connection| {
                rows(connection, sql, Vec::new(), |row| {
                    RawVaultObject::read(row).map(Ok)
                })
            })?
            .into_iter()
            .map(|raw| raw.into_object(&self.inner))
            .collect()
    }

    pub fn prune_the_vault(&self) -> Result<Pruned> {
        self.pruning_the_vault(Applying::Yes)
    }

    pub fn vault_prune_foretold(&self) -> Result<Pruned> {
        self.pruning_the_vault(Applying::No)
    }

    fn pruning_the_vault(&self, applying: Applying) -> Result<Pruned> {
        let _walk = self.walk_the_tree()?;
        let vault = Arc::clone(self.inner.opened_vault()?);
        let refused = |path: &Path, source| Error::Vault {
            path: path.to_path_buf(),
            source: Box::new(source),
        };

        let mut pruned = Pruned::default();
        for object in self.vault_objects_nothing_names()? {
            match applying.forgets(&vault, &object.path) {
                Ok(taken) => {
                    pruned.objects += u64::from(taken);
                    if applying == Applying::Yes {
                        self.forget_vault_object(&object.key)?;
                    }
                }
                Err(error) => {
                    tracing::warn!(path = %object.path.display(), %error, "an object could not be taken away, so its row stays for the next prune");
                    pruned.left += 1;
                }
            }
        }

        let noted = self.vault_keys_noted()?;
        let landed_before = SystemTime::now()
            .checked_sub(LANDING_GRACE)
            .unwrap_or(UNIX_EPOCH);
        for object in vault
            .objects()
            .map_err(|source| refused(vault.root(), source))?
        {
            if noted.contains(&object.key) || !landed_before_it(&object.path, landed_before) {
                continue;
            }
            match applying.forgets(&vault, &object.path) {
                Ok(taken) => pruned.objects += u64::from(taken),
                Err(error) => {
                    tracing::warn!(path = %object.path.display(), %error, "an object no row names could not be taken away");
                    pruned.left += 1;
                }
            }
        }

        let named: AHashSet<VaultKey> = self
            .inner
            .read(|connection| {
                rows(
                    connection,
                    "SELECT cover_key FROM albums WHERE cover_key IS NOT NULL",
                    Vec::new(),
                    |row| {
                        let key: String = row.get(0)?;
                        Ok(VaultKey::read(&key).map_err(|_| {
                            tracing::warn!(key, "an album's cover key does not read as one");
                            Error::NotAVaultKey {
                                held_in: KeyColumn::AlbumCover,
                            }
                        }))
                    },
                )
            })?
            .into_iter()
            .collect();

        let root = vault.root().to_path_buf();
        for cover in vault.covers().map_err(|source| refused(&root, source))? {
            if named.contains(&cover.key) {
                continue;
            }
            if applying
                .forgets(&vault, &cover.path)
                .map_err(|source| refused(&cover.path, source))?
            {
                pruned.covers += 1;
            }
        }

        pruned.staged = match applying {
            Applying::Yes => vault.sweep_the_staging(),
            Applying::No => vault.staging_a_sweep_would_take(),
        }
        .map_err(|source| refused(&root, source))?;
        Ok(pruned)
    }

    pub fn compact_the_vault_covers(&self) -> Result<Compacted> {
        self.compacting_the_vault_covers(Applying::Yes)
    }

    pub fn vault_covers_compaction_foretold(&self) -> Result<Compacted> {
        self.compacting_the_vault_covers(Applying::No)
    }

    fn compacting_the_vault_covers(&self, applying: Applying) -> Result<Compacted> {
        let _walk = self.walk_the_tree()?;
        let vault = Arc::clone(self.inner.opened_vault()?);
        let covers = vault.covers().map_err(|source| Error::Vault {
            path: vault.root().to_path_buf(),
            source: Box::new(source),
        })?;

        let mut compacted = Compacted::default();
        for cover in covers {
            let compacting = match applying {
                Applying::Yes => vault.compact_cover(&cover.path),
                Applying::No => vault.foretell_compacting(&cover.path),
            };
            let (was, now) = match compacting {
                Ok(Compacting::NotHeld) => continue,
                Ok(Compacting::NoSmaller | Compacting::Unproven) => {
                    compacted.kept += 1;
                    continue;
                }
                Ok(Compacting::Foretold { was, now }) => (was, now),
                Ok(Compacting::Beside { jxl, was, now }) => {
                    if applying == Applying::Yes
                        && let Err(error) = self.settle_the_cover(&vault, &cover, &jxl)
                    {
                        tracing::warn!(path = %cover.path.display(), %error, "a cover's smaller form stands beside it, and the JPEG stays for the next pass");
                        compacted.left += 1;
                        continue;
                    }
                    (was, now)
                }
                Err(error) => {
                    tracing::warn!(path = %cover.path.display(), %error, "a cover could not be weighed for a transcode");
                    compacted.left += 1;
                    continue;
                }
            };
            compacted.covers += 1;
            compacted.was += was;
            compacted.now += now;
        }
        Ok(compacted)
    }

    fn settle_the_cover(&self, vault: &Vault, cover: &HeldFile, jxl: &Path) -> Result<()> {
        let within = self.inner.within_the_vault(jxl)?;
        let key = cover.key.to_string();
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "UPDATE albums SET cover_path = ?1 WHERE cover_key = ?2",
                    params![within, key],
                )
                .map(|_| ())
                .map_err(|source| Error::store(StoreOp::Update, source))
        })?;
        vault
            .forget(&cover.path)
            .map(|_| ())
            .map_err(|source| Error::Vault {
                path: cover.path.clone(),
                source: Box::new(source),
            })
    }

    fn vault_keys_noted(&self) -> Result<AHashSet<VaultKey>> {
        let texts = self.inner.read(|connection| {
            rows(
                connection,
                "SELECT key FROM vault_objects
                 UNION SELECT vault_key FROM tracks WHERE vault_key IS NOT NULL",
                Vec::new(),
                |row| Ok(Ok(row.get::<_, String>(0)?)),
            )
        })?;
        Ok(texts
            .iter()
            .filter_map(|text| VaultKey::read(text).ok())
            .collect())
    }

    pub fn release_from_vault(&self, roots: &[PathBuf]) -> Result<Released> {
        self.releasing_from_the_vault(roots, Applying::Yes)
    }

    pub fn vault_release_foretold(&self, roots: &[PathBuf]) -> Result<Released> {
        self.releasing_from_the_vault(roots, Applying::No)
    }

    fn releasing_from_the_vault(&self, roots: &[PathBuf], applying: Applying) -> Result<Released> {
        let _walk = self.walk_the_tree()?;
        let known = self.roots()?;
        if let Some(stranger) = roots.iter().find(|root| !known.contains(root)) {
            return Err(Error::NotARoot {
                path: stranger.clone(),
            });
        }

        let named = rooted(roots)?;
        let sql = and_roots(TRACKS_IN_THE_VAULT, named.len());
        let vaulted: Vec<(i64, PathBuf)> = self.inner.read(|connection| {
            rows(connection, &sql, named.clone(), |row| {
                Ok(Ok((row.get(0)?, store::path_of(&row.get::<_, String>(1)?))))
            })
        })?;
        let (standing, stranded): (Vec<_>, Vec<_>) =
            vaulted.into_iter().partition(|(_, path)| path.exists());

        if applying == Applying::No {
            return Ok(Released {
                released: standing.len() as u64,
                stranded: stranded.len() as u64,
            });
        }

        let released = self.released(&standing)?;

        Ok(Released {
            released,
            stranded: stranded.len() as u64,
        })
    }

    pub(super) fn released(&self, standing: &[(i64, PathBuf)]) -> Result<u64> {
        self.inner.write(|transaction| {
            let mut released = 0;
            for (id, _) in standing {
                released += transaction
                    .execute(
                        "UPDATE tracks SET vault_key = NULL, vault_path = NULL, vault_bits = NULL
                          WHERE id = ?1",
                        params![id],
                    )
                    .map_err(|source| Error::store(StoreOp::Update, source))?;
                transaction
                    .execute("DELETE FROM kept_tags WHERE track_id = ?1", params![id])
                    .map_err(|source| Error::store(StoreOp::Delete, source))?;
            }
            Ok(released as u64)
        })
    }

    pub fn release_the_rows_of(&self, keys: &[VaultKey]) -> Result<Released> {
        let _walk = self.walk_the_tree()?;
        let mut vaulted: Vec<(i64, PathBuf)> = Vec::new();
        for key in keys {
            let named = key.to_string();
            vaulted.extend(self.inner.read(|connection| {
                rows(
                    connection,
                    "SELECT id, path FROM tracks WHERE vault_key = ?1",
                    vec![Value::Text(named.clone())],
                    |row| Ok(Ok((row.get(0)?, store::path_of(&row.get::<_, String>(1)?)))),
                )
            })?);
        }
        let (standing, stranded): (Vec<_>, Vec<_>) =
            vaulted.into_iter().partition(|(_, path)| path.exists());

        let released = self.released(&standing)?;

        Ok(Released {
            released,
            stranded: stranded.len() as u64,
        })
    }

    pub fn let_go_of_vault_covers(&self, keys: &[VaultKey]) -> Result<u64> {
        self.inner.write(|transaction| {
            let mut let_go = 0;
            for key in keys {
                let_go += transaction
                    .execute(
                        "UPDATE albums SET cover_key = NULL, cover_path = NULL WHERE cover_key = ?1",
                        params![key.to_string()],
                    )
                    .map_err(|source| Error::store(StoreOp::Update, source))?;
            }
            Ok(let_go as u64)
        })
    }

    pub fn forget_vault_object(&self, key: &VaultKey) -> Result<bool> {
        let named = key.to_string();
        self.inner.write(|transaction| {
            transaction
                .execute("DELETE FROM vault_objects WHERE key = ?1", params![named])
                .map(|gone| gone > 0)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub fn note_validated(&self, key: &VaultKey, validated: bool) -> Result<()> {
        let named = key.to_string();
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "UPDATE vault_objects SET validated = ?1 WHERE key = ?2",
                    params![i64::from(validated), named],
                )
                .map(|_| ())
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub(crate) fn note_vaulted(&self, row: &TrackToVault, kept: &VaultKept) -> Result<()> {
        let key = kept.key.to_string();
        let held = self.inner.within_the_vault(&kept.path)?;
        let taken_from = store::path_text(&row.path).into_owned();
        let took = store::to_nanos(SystemTime::now());
        let encoding = weighed_under(kept, row.renewing);

        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO vault_objects (key, form, path, bytes, sample_rate, channels,
                            sample_format, frames, taken_from, took, was_bytes, was_codec,
                            encoding, validated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1)
                     ON CONFLICT(key) DO UPDATE
                        SET bytes = excluded.bytes, encoding = excluded.encoding, validated = 1
                      WHERE excluded.encoding > vault_objects.encoding",
                    params![
                        key,
                        store::vault_form_code(kept.form),
                        held,
                        kept.bytes as i64,
                        kept.spec.rate.hz(),
                        kept.spec.channel_count().get(),
                        store::format_code(kept.spec.format),
                        kept.frames.get() as i64,
                        taken_from,
                        took,
                        kept.was as i64,
                        store::codec_code(row.codec),
                        i64::from(encoding.get()),
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            transaction
                .execute(
                    "UPDATE tracks SET vault_key = ?1, vault_path = ?2, vault_bits = ?3
                      WHERE id = ?4",
                    params![key, held, i64::from(kept.bits), row.id.get() as i64],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            vaulted::keep_what_the_catalog_cannot_fill(
                transaction,
                row.id.get() as i64,
                &kept.declared,
            )?;

            transaction
                .execute(
                    "DELETE FROM vault_refused WHERE track_id = ?1",
                    params![row.id.get() as i64],
                )
                .map(|_| ())
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub(crate) fn note_vault_refused(&self, row: &TrackToVault) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO vault_refused (track_id, under) VALUES (?1, ?2)
                     ON CONFLICT(track_id) DO UPDATE SET under = excluded.under",
                    params![
                        row.id.get() as i64,
                        i64::from(Encoding::OF_THIS_BUILD.get())
                    ],
                )
                .map(|_| ())
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub(crate) fn note_delivered(
        &self,
        want: &Want,
        kept: &VaultKept,
        from: &MediaLocation,
    ) -> Result<Option<TrackId>> {
        let key = kept.key.to_string();
        let held = self.inner.within_the_vault(&kept.path)?;
        let path = store::path_text(&kept.path).into_owned();
        let offered = from.to_uri();
        let now = SystemTime::now();
        let took = store::to_nanos(now);
        let modified = std::fs::metadata(&kept.path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(now);
        let declared = &kept.declared;
        let gain = declared.heard_gain();

        let id = self.inner.write(|transaction| {
            if !still_unheld(transaction, want.release_track)? {
                return Ok(None);
            }
            note_the_delivered_object(transaction, kept, &held, &offered, took)?;

            let artist = want
                .artist
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .map(|name| store::artist_named_in(transaction, name, None))
                .transpose()?;
            let landed: Option<i64> = transaction
                .query_row(
                    "INSERT INTO tracks (
                         root_id, path, title, artist, artist_id, album_id, track_number,
                         disc_number, duration, sample_rate, channels, sample_format, codec,
                         file_size, modified, added, seen, mbid, release_track_mbid, isrc,
                         vault_key, vault_path, genre, lyrics, rg_track_gain, rg_track_peak,
                         rg_album_gain, rg_album_peak, vault_bits
                     ) VALUES (
                         NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, 0, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27
                     )
                     ON CONFLICT(path, span_start) DO NOTHING
                     RETURNING id",
                    params![
                        path,
                        want.title,
                        want.artist,
                        artist,
                        want.album.get() as i64,
                        i64::from(want.position),
                        i64::from(want.disc),
                        kept.frames.get() as i64,
                        kept.spec.rate.hz(),
                        kept.spec.channel_count().get(),
                        store::format_code(kept.spec.format),
                        store::codec_code(kept.codec),
                        kept.bytes as i64,
                        store::to_nanos(modified),
                        took,
                        want.recording.as_ref().map(Mbid::as_str),
                        want.track.as_ref().map(Mbid::as_str),
                        want.isrcs.first().map(Isrc::as_str),
                        key,
                        held,
                        declared.genre,
                        declared.lyrics,
                        gain.track_gain.map(|gain| f64::from(gain.get())),
                        gain.track_peak.map(f64::from),
                        gain.album_gain.map(|gain| f64::from(gain.get())),
                        gain.album_peak.map(f64::from),
                        i64::from(kept.bits),
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            let id = match landed {
                Some(id) => {
                    store::index_row(
                        transaction,
                        id,
                        &want.title,
                        want.artist.as_deref().unwrap_or_default(),
                        &want.album_title,
                        &store::indexed_genre_of(transaction, declared.genre.as_deref(), artist)?,
                    )?;
                    vaulted::keep_what_the_catalog_cannot_fill(transaction, id, declared)?;
                    id
                }
                None => transaction
                    .query_row(
                        "SELECT id FROM tracks WHERE path = ?1 AND span_start = 0",
                        params![path],
                        |row| row.get(0),
                    )
                    .map_err(|source| Error::store(StoreOp::Query, source))?,
            };
            transaction
                .execute(
                    "UPDATE release_tracks SET track_id = ?1 WHERE id = ?2 AND track_id IS NULL",
                    params![id, want.release_track.get() as i64],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            Ok(Some(id))
        })?;

        id.map(|id| TrackId::new(id as u64).map_err(Error::from))
            .transpose()
    }

    pub(crate) fn note_received(
        &self,
        recording: &resonate_core::Recording,
        kept: &VaultKept,
        from: &str,
    ) -> Result<Option<TrackId>> {
        let key = kept.key.to_string();
        let held = self.inner.within_the_vault(&kept.path)?;
        let path = store::path_text(&kept.path).into_owned();
        let now = SystemTime::now();
        let took = store::to_nanos(now);
        let modified = std::fs::metadata(&kept.path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(now);
        let declared = &kept.declared;
        let gain = declared.heard_gain();
        let artist_name = Some(recording.artist.trim()).filter(|name| !name.is_empty());
        let album_title = Some(recording.album.trim()).filter(|title| !title.is_empty());
        let mbid = recording.mbid.as_ref().map(Mbid::as_str);

        let id = self.inner.write(|transaction| {
            note_the_delivered_object(transaction, kept, &held, from, took)?;

            let artist = artist_name
                .map(|name| store::artist_named_in(transaction, name, None))
                .transpose()?;
            let album = album_title
                .map(|title| store::album_received_in(transaction, title, artist_name, declared))
                .transpose()?;
            let landed: Option<i64> = transaction
                .query_row(
                    "INSERT INTO tracks (
                         root_id, path, title, artist, artist_id, album_id, track_number,
                         disc_number, duration, sample_rate, channels, sample_format, codec,
                         file_size, modified, added, seen, mbid, vault_key, vault_path, genre,
                         lyrics, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak,
                         vault_bits, track_total
                     ) VALUES (
                         NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, 0, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
                     )
                     ON CONFLICT(path, span_start) DO NOTHING
                     RETURNING id",
                    params![
                        path,
                        recording.title,
                        artist_name,
                        artist,
                        album,
                        declared.track_number.map(i64::from),
                        declared.disc_number.map(i64::from),
                        kept.frames.get() as i64,
                        kept.spec.rate.hz(),
                        kept.spec.channel_count().get(),
                        store::format_code(kept.spec.format),
                        store::codec_code(kept.codec),
                        kept.bytes as i64,
                        store::to_nanos(modified),
                        took,
                        mbid,
                        key,
                        held,
                        declared.genre,
                        declared.lyrics,
                        gain.track_gain.map(|gain| f64::from(gain.get())),
                        gain.track_peak.map(f64::from),
                        gain.album_gain.map(|gain| f64::from(gain.get())),
                        gain.album_peak.map(f64::from),
                        i64::from(kept.bits),
                        declared.track_total.map(i64::from),
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            let Some(id) = landed else {
                return Ok(None);
            };
            store::index_row(
                transaction,
                id,
                &recording.title,
                artist_name.unwrap_or_default(),
                album_title.unwrap_or_default(),
                &store::indexed_genre_of(transaction, declared.genre.as_deref(), artist)?,
            )?;
            vaulted::keep_what_the_catalog_cannot_fill(transaction, id, declared)?;
            Ok(Some(id))
        })?;

        id.map(|id| TrackId::new(id as u64).map_err(Error::from))
            .transpose()
    }

    pub(crate) fn note_vaulted_cover(
        &self,
        album: AlbumId,
        kept: &KeptCover,
        encoded: &CoverArt,
    ) -> Result<bool> {
        let within = self.inner.within_the_vault(&kept.path)?;
        self.inner.write(|transaction| {
            enriched::vault_the_cover(transaction, album, kept.key, &within, &encoded.bytes)
        })
    }
}
