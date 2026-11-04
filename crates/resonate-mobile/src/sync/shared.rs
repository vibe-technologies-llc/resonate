use resonate_core::Recording;
use resonate_sync::{
    LocalPlaylistId, Refused,
    shared::{LocalShared, SharedCatalog},
};
use rusqlite::{Connection, OptionalExtension, params};

use super::{DocumentCatalog, a_free_name, entries_of, unavailable};
use crate::playlists;

fn flagged(connection: &Connection, at: LocalPlaylistId) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM playlist_shares WHERE playlist = ?1 AND created = ?2)",
        params![at.id, at.created],
        |row| row.get(0),
    )
}

fn local_at(connection: &Connection, at: LocalPlaylistId) -> rusqlite::Result<Option<LocalShared>> {
    let name = connection
        .query_row(
            "SELECT name FROM playlists WHERE id = ?1 AND created = ?2",
            params![at.id, at.created],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    name.map(|name| {
        Ok(LocalShared {
            at,
            name,
            entries: entries_of(connection, at.id)?,
        })
    })
    .transpose()
}

pub(super) fn hides(connection: &Connection, at: LocalPlaylistId) -> rusqlite::Result<bool> {
    flagged(connection, at)
}

impl SharedCatalog for DocumentCatalog {
    fn shared_read(&self, at: LocalPlaylistId) -> Result<Option<LocalShared>, Refused> {
        let connection = self.catalog.0.lock();
        if !flagged(&connection, at).map_err(unavailable)? {
            return Ok(None);
        }

        local_at(&connection, at).map_err(unavailable)
    }

    fn shared_write(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
        editable: bool,
    ) -> Result<LocalShared, Refused> {
        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(unavailable)?;
        let mut rows = Vec::with_capacity(entries.len());
        for entry in entries {
            rows.extend(self.resolved_in(&transaction, entry)?);
        }

        let held = match at {
            Some(at) => transaction
                .query_row(
                    "SELECT id FROM playlists WHERE id = ?1 AND created = ?2",
                    params![at.id, at.created],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(unavailable)?,
            None => None,
        };
        let id = match held {
            Some(id) => id,
            None => {
                let named = a_free_name(&transaction, name, None).map_err(unavailable)?;
                transaction
                    .execute(
                        "INSERT INTO playlists(name, folded, created) VALUES (?1, ?2, ?3)",
                        params![named, playlists::folded(&named), playlists::created_now()],
                    )
                    .map_err(unavailable)?;
                transaction.last_insert_rowid()
            }
        };
        let named = a_free_name(&transaction, name, Some(id)).map_err(unavailable)?;
        transaction
            .execute(
                "UPDATE playlists SET name = ?2, folded = ?3, revision = revision + 1 WHERE id = ?1",
                params![id, named, playlists::folded(&named)],
            )
            .map_err(unavailable)?;
        playlists::replace_rows(&transaction, id as u64, &rows).map_err(|error| {
            tracing::warn!(%error, "a shared playlist could not be written");
            Refused::Unavailable
        })?;
        let created: i64 = transaction
            .query_row("SELECT created FROM playlists WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .map_err(unavailable)?;
        transaction
            .execute(
                "INSERT INTO playlist_shares(playlist, created, editable) VALUES (?1, ?2, ?3)
                 ON CONFLICT(playlist) DO UPDATE SET created = ?2, editable = ?3",
                params![id, created, editable],
            )
            .map_err(unavailable)?;
        let at = LocalPlaylistId { id, created };
        let written = local_at(&transaction, at)
            .map_err(unavailable)?
            .ok_or(Refused::Unavailable)?;
        transaction.commit().map_err(unavailable)?;
        self.arrived();

        Ok(written)
    }

    fn shared_flag(
        &self,
        at: LocalPlaylistId,
        editable: bool,
    ) -> Result<Option<LocalShared>, Refused> {
        let connection = self.catalog.0.lock();
        let Some(local) = local_at(&connection, at).map_err(unavailable)? else {
            return Ok(None);
        };
        connection
            .execute(
                "INSERT INTO playlist_shares(playlist, created, editable) VALUES (?1, ?2, ?3)
                 ON CONFLICT(playlist) DO UPDATE SET created = ?2, editable = ?3",
                params![at.id, at.created, editable],
            )
            .map_err(unavailable)?;

        Ok(Some(local))
    }

    fn shared_unflag(&self, at: LocalPlaylistId) -> Result<(), Refused> {
        self.catalog
            .0
            .lock()
            .execute(
                "DELETE FROM playlist_shares WHERE playlist = ?1 AND created = ?2",
                params![at.id, at.created],
            )
            .map_err(unavailable)?;

        Ok(())
    }

    fn shared_held_as(&self, recording: &Recording) -> Result<Option<Recording>, Refused> {
        let connection = self.catalog.0.lock();
        let Some(id) = self.resolved_in(&connection, recording)? else {
            return Ok(None);
        };

        connection
            .query_row(
                &format!(
                    "SELECT {} FROM documents d WHERE d.id = ?1",
                    super::RECORDING_COLUMNS
                ),
                [id as i64],
                |row| super::recording_at(row, 0),
            )
            .optional()
            .map_err(unavailable)
    }

    fn shared_revision(&self) -> u64 {
        self.catalog
            .0
            .lock()
            .query_row(
                "SELECT coalesce(sum(revision), 0) + count(*) FROM playlists",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .cast_unsigned()
    }
}

#[cfg(test)]
mod tests {
    use resonate_sync::{Catalog, shared::SharedCatalog};

    use super::*;
    use crate::{
        Error,
        sync::tests::{documents_of, folder, player, song},
    };

    #[test]
    fn a_list_sharing_leaves_the_personal_view_and_says_it_was_hidden_not_deleted() {
        let there = folder("shared-hides");
        let phone = player(&there);
        let song = song(&phone, "content://test/one.flac", "One", None);
        let id = phone
            .create_playlist("Road Trip".to_owned())
            .expect("a list");
        phone
            .add_to_playlist(id, vec![song.id])
            .expect("a song on it");
        let documents = documents_of(&phone, &there);
        let at = phone.synced_playlist(id).expect("its identity");

        assert_eq!(documents.playlists().expect("listed").len(), 1);
        assert!(!documents.hides_as_shared(at).expect("asked"));

        let local = documents
            .shared_flag(at, true)
            .expect("flagged")
            .expect("the list is there");

        assert_eq!(local.entries.len(), 1, "sharing keeps every song");
        assert!(documents.playlists().expect("listed").is_empty());
        assert!(documents.hides_as_shared(at).expect("asked"));
        assert_eq!(
            phone.playlists().expect("shown").len(),
            1,
            "it is still shown"
        );

        documents.shared_unflag(at).expect("unflagged");

        assert_eq!(documents.playlists().expect("listed").len(), 1);
    }

    #[test]
    fn a_followed_list_refuses_every_edit_and_an_edited_one_takes_them() {
        let there = folder("shared-follow");
        let phone = player(&there);
        let song = song(&phone, "content://test/two.flac", "Two", None);
        let id = phone
            .create_playlist("Followed".to_owned())
            .expect("a list");
        phone
            .add_to_playlist(id, vec![song.id])
            .expect("a song on it");
        let documents = documents_of(&phone, &there);
        let at = phone.synced_playlist(id).expect("its identity");

        documents.shared_flag(at, false).expect("flagged");

        assert!(matches!(
            phone.add_to_playlist(id, vec![song.id]),
            Err(Error::PlaylistFollowed)
        ));

        documents.shared_flag(at, true).expect("made editable");

        assert!(phone.add_to_playlist(id, vec![song.id]).is_ok());
    }

    #[test]
    fn a_shared_list_written_here_is_flagged_as_it_is_made_and_keeps_only_what_is_held() {
        let there = folder("shared-write");
        let phone = player(&there);
        let held = song(&phone, "content://test/three.flac", "Three", None);
        let documents = documents_of(&phone, &there);
        let recording = documents
            .shared_held_as(&Recording {
                mbid: None,
                title: held.title.clone(),
                artist: held.artist.clone(),
                album: held.album.clone(),
                length: None,
            })
            .expect("asked")
            .expect("held");
        let absent = Recording {
            mbid: None,
            title: "Nobody here has this".to_owned(),
            artist: "Nobody".to_owned(),
            album: String::new(),
            length: None,
        };

        let written = documents
            .shared_write(None, "From a friend", &[recording, absent], false)
            .expect("written");

        assert_eq!(written.entries.len(), 1);
        assert!(documents.hides_as_shared(written.at).expect("asked"));
        assert!(documents.playlists().expect("listed").is_empty());
    }
}
