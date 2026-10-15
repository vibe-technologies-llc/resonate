use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use unicode_normalization::UnicodeNormalization;

use crate::{Error, MobilePlayer, Result, StoreOp, Track, catalog::read};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MusicPlaylist {
    pub id: u64,
    pub name: String,
    pub songs: u64,
    pub revision: u64,
}

#[derive(Clone)]
struct SavedPlaylist {
    name: String,
    revision: u64,
    created: i64,
    rows: Vec<u64>,
}

pub(crate) struct PlaylistUndo {
    id: u64,
    before: Option<SavedPlaylist>,
    after: Option<u64>,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn playlists(&self) -> Result<Vec<MusicPlaylist>> {
        let connection = self.catalog.0.lock();
        let mut query = connection.prepare("SELECT id, name, (SELECT count(*) FROM playlist_rows WHERE playlist = p.id), revision FROM playlists p ORDER BY folded, id").map_err(read_error)?;
        query
            .query_map([], |row| {
                Ok(MusicPlaylist {
                    id: row.get::<_, i64>(0)? as u64,
                    name: row.get(1)?,
                    songs: row.get::<_, i64>(2)? as u64,
                    revision: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub fn playlist_tracks(&self, id: u64, offset: u32, limit: u32) -> Result<Vec<Track>> {
        let connection = self.catalog.0.lock();
        let mut query = connection.prepare("SELECT d.*, p.position FROM playlist_rows p JOIN documents d ON d.id = p.track WHERE p.playlist = ?1 AND d.missing = 0 ORDER BY p.position LIMIT ?2 OFFSET ?3").map_err(read_error)?;
        query
            .query_map(params![id as i64, limit.min(500), offset], |row| {
                let mut track = read(row)?;
                track.queue_id = Some(row.get::<_, i64>("position")? as u64);
                Ok(track)
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub fn create_playlist(&self, name: String) -> Result<u64> {
        self.make_playlist(name, Vec::new())
    }

    pub fn rename_playlist(&self, id: u64, revision: u64, name: String) -> Result<()> {
        let name = valid_name(&name)?;
        self.edit_playlist(id, Some(revision), |connection| {
            refuse_duplicate(connection, &name, id)?;
            connection
                .execute(
                    "UPDATE playlists SET name = ?2, folded = ?3 WHERE id = ?1",
                    params![id as i64, name, folded(&name)],
                )
                .map_err(write_error)?;
            Ok(())
        })
    }

    pub fn add_to_playlist(&self, id: u64, tracks: Vec<u64>) -> Result<()> {
        self.edit_playlist(id, None, |connection| {
            let first: i64 = connection
                .query_row(
                    "SELECT coalesce(max(position) + 1, 0) FROM playlist_rows WHERE playlist = ?1",
                    [id as i64],
                    |row| row.get(0),
                )
                .map_err(read_error)?;
            for (position, track) in tracks.iter().enumerate() {
                connection
                    .execute(
                        "INSERT INTO playlist_rows(playlist, position, track) VALUES (?1, ?2, ?3)",
                        params![id as i64, first + position as i64, *track as i64],
                    )
                    .map_err(write_error)?;
            }
            Ok(())
        })
    }

    pub fn remove_from_playlist(&self, id: u64, revision: u64, position: u32) -> Result<()> {
        self.edit_playlist(id, Some(revision), |connection| {
            let mut rows = playlist_rows(connection, id)?;
            let shown = shown_positions(connection, &rows)?;
            let Some(at) = shown.get(position as usize) else {
                return Err(Error::PlaylistChanged);
            };
            rows.remove(*at);
            replace_rows(connection, id, &rows)
        })
    }

    pub fn move_in_playlist(&self, id: u64, revision: u64, from: u32, to: u32) -> Result<()> {
        self.edit_playlist(id, Some(revision), |connection| {
            let mut rows = playlist_rows(connection, id)?;
            let shown = shown_positions(connection, &rows)?;
            let (Some(from), Some(to)) = (shown.get(from as usize), shown.get(to as usize)) else {
                return Err(Error::PlaylistChanged);
            };
            let row = rows.remove(*from);
            rows.insert(*to, row);
            replace_rows(connection, id, &rows)
        })
    }

    pub fn delete_playlist(&self, id: u64, revision: u64) -> Result<()> {
        let mut history = self.playlist_undo.lock();
        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let before = saved_playlist(&transaction, id)?.ok_or(Error::UnknownPlaylist)?;
        if before.revision != revision {
            return Err(Error::PlaylistChanged);
        }
        transaction
            .execute("DELETE FROM playlists WHERE id = ?1", [id as i64])
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)?;
        remember_playlist(
            &mut history,
            PlaylistUndo {
                id,
                before: Some(before),
                after: None,
            },
        );
        Ok(())
    }

    pub fn undo_playlist(&self) -> Result<bool> {
        let mut history = self.playlist_undo.lock();
        let Some(step) = history.last() else {
            return Ok(false);
        };
        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let current = saved_playlist(&transaction, step.id)?;
        if current.as_ref().map(|playlist| playlist.revision) != step.after {
            history.pop();
            return Err(Error::PlaylistChanged);
        }
        match step.before.as_ref() {
            Some(before) => {
                refuse_duplicate(&transaction, &before.name, step.id)?;
                transaction.execute("INSERT INTO playlists(id, name, folded, revision, created) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET name = excluded.name, folded = excluded.folded, revision = excluded.revision, created = excluded.created", params![step.id as i64, before.name, folded(&before.name), before.revision as i64, before.created]).map_err(write_error)?;
                replace_rows(&transaction, step.id, &before.rows)?;
            }
            None => {
                transaction
                    .execute("DELETE FROM playlists WHERE id = ?1", [step.id as i64])
                    .map_err(write_error)?;
            }
        }
        transaction.commit().map_err(write_error)?;
        history.pop();
        Ok(true)
    }
}

impl MobilePlayer {
    pub(crate) fn make_playlist(&self, name: String, tracks: Vec<u64>) -> Result<u64> {
        let name = valid_name(&name)?;
        let mut history = self.playlist_undo.lock();
        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        refuse_duplicate(&transaction, &name, 0)?;
        transaction
            .execute(
                "INSERT INTO playlists(name, folded, created) VALUES (?1, ?2, ?3)",
                params![name, folded(&name), created_now()],
            )
            .map_err(write_error)?;
        let id = transaction.last_insert_rowid() as u64;
        replace_rows(&transaction, id, &tracks)?;
        transaction.commit().map_err(write_error)?;
        remember_playlist(
            &mut history,
            PlaylistUndo {
                id,
                before: None,
                after: Some(0),
            },
        );
        Ok(id)
    }

    fn edit_playlist(
        &self,
        id: u64,
        revision: Option<u64>,
        edit: impl FnOnce(&Connection) -> Result<()>,
    ) -> Result<()> {
        let mut history = self.playlist_undo.lock();
        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let before = saved_playlist(&transaction, id)?.ok_or(Error::UnknownPlaylist)?;
        if revision.is_some_and(|revision| revision != before.revision) {
            return Err(Error::PlaylistChanged);
        }
        edit(&transaction)?;
        transaction
            .execute(
                "UPDATE playlists SET revision = revision + 1 WHERE id = ?1",
                [id as i64],
            )
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)?;
        remember_playlist(
            &mut history,
            PlaylistUndo {
                id,
                after: Some(before.revision + 1),
                before: Some(before),
            },
        );
        Ok(())
    }
}

fn remember_playlist(history: &mut Vec<PlaylistUndo>, step: PlaylistUndo) {
    history.push(step);
    while history.len() > 1
        && (history.len() > 32
            || history
                .iter()
                .filter_map(|step| step.before.as_ref())
                .map(|saved| saved.rows.len())
                .sum::<usize>()
                > 50000)
    {
        history.remove(0);
    }
}

fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 255 {
        return Err(Error::InvalidPlaylistName);
    }
    Ok(name.nfc().collect())
}

pub(crate) fn created_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_nanos()).ok())
        .unwrap_or_default()
}

pub(crate) fn folded(name: &str) -> String {
    name.to_lowercase().nfc().collect()
}

fn refuse_duplicate(connection: &Connection, name: &str, except: u64) -> Result<()> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE folded = ?1 AND id != ?2)",
            params![folded(name), except as i64],
            |row| row.get(0),
        )
        .map_err(read_error)?;
    if exists {
        Err(Error::DuplicatePlaylistName)
    } else {
        Ok(())
    }
}

fn saved_playlist(connection: &Connection, id: u64) -> Result<Option<SavedPlaylist>> {
    let row: Option<(String, i64, i64)> = connection
        .query_row(
            "SELECT name, revision, created FROM playlists WHERE id = ?1",
            [id as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(read_error)?;
    row.map(|(name, revision, created)| {
        Ok(SavedPlaylist {
            name,
            revision: revision as u64,
            created,
            rows: playlist_rows(connection, id)?,
        })
    })
    .transpose()
}

fn shown_positions(connection: &Connection, rows: &[u64]) -> Result<Vec<usize>> {
    let mut query = connection
        .prepare("SELECT id FROM documents WHERE missing = 1")
        .map_err(read_error)?;
    let hidden = query
        .query_map([], |row| row.get::<_, i64>(0).map(|id| id as u64))
        .map_err(read_error)?
        .collect::<rusqlite::Result<std::collections::BTreeSet<_>>>()
        .map_err(read_error)?;
    Ok(rows
        .iter()
        .enumerate()
        .filter(|(_, track)| !hidden.contains(track))
        .map(|(at, _)| at)
        .collect())
}

fn playlist_rows(connection: &Connection, id: u64) -> Result<Vec<u64>> {
    let mut query = connection
        .prepare("SELECT track FROM playlist_rows WHERE playlist = ?1 ORDER BY position")
        .map_err(read_error)?;
    query
        .query_map([id as i64], |row| row.get::<_, i64>(0).map(|id| id as u64))
        .map_err(read_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_error)
}

pub(crate) fn replace_rows(connection: &Connection, id: u64, rows: &[u64]) -> Result<()> {
    connection
        .execute("DELETE FROM playlist_rows WHERE playlist = ?1", [id as i64])
        .map_err(write_error)?;
    for (position, track) in rows.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO playlist_rows(playlist, position, track) VALUES (?1, ?2, ?3)",
                params![id as i64, position as i64, *track as i64],
            )
            .map_err(write_error)?;
    }
    Ok(())
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}
fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use std::{env, path::Path, sync::Arc};

    use parking_lot::Mutex;
    use resonate_codec::Sources;

    use super::*;
    use crate::{PlaybackPreferences, catalog::Catalog, features::Services};

    fn player() -> MobilePlayer {
        MobilePlayer {
            player: Mutex::new(None),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(Sources::local()),
            failure: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: crate::sync::Syncing::under(env::temp_dir()),
            routing: Arc::new(crate::output::Routing::new()),
            enriching: crate::enrichment::Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
        }
    }

    #[test]
    fn document_playlists_keep_duplicates_order_and_restore_one_edit_at_a_time() {
        let player = player();
        let track = player
            .catalog
            .upsert(Track {
                uri: "content://test/song.flac".into(),
                title: "Song".into(),
                ..Track::default()
            })
            .expect("a song");
        let id = player
            .create_playlist("Favourites".into())
            .expect("a new list");
        assert!(matches!(
            player.create_playlist("FAVOURITES".into()),
            Err(Error::DuplicatePlaylistName)
        ));
        player
            .add_to_playlist(id, vec![track.id, track.id])
            .expect("a deliberate repeat");
        assert_eq!(
            player.playlist_tracks(id, 0, 100).expect("the list").len(),
            2
        );
        player
            .remove_from_playlist(id, 1, 0)
            .expect("one row removed");
        assert!(matches!(
            player.remove_from_playlist(id, 1, 0),
            Err(Error::PlaylistChanged)
        ));
        assert!(player.undo_playlist().expect("undo a removal"));
        assert_eq!(
            player
                .playlist_tracks(id, 0, 100)
                .expect("the restored list")
                .len(),
            2
        );
        player
            .rename_playlist(id, 1, "Evening".into())
            .expect("a rename");
        player.delete_playlist(id, 2).expect("discard the list");
        assert!(player.playlists().expect("no lists").is_empty());
        assert!(player.undo_playlist().expect("restore the list"));
        assert_eq!(player.playlists().expect("the list")[0].name, "Evening");
        assert_eq!(
            player.playlist_tracks(id, 0, 100).expect("its rows").len(),
            2
        );
    }
}
