use std::time::{SystemTime, UNIX_EPOCH};

use resonate_library::{KEPT_IN_STEP_EVERY, PlaylistLink};
use rusqlite::{Connection, OptionalExtension as _, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp,
    collections::{ImportedPlaylist, local_match},
    features::SearchSong,
    playlists::replace_rows,
};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PlaylistLinkInfo {
    pub page: String,
    pub service: String,
    pub kept_in_step: bool,
    pub refreshed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RefreshedPlaylist {
    pub id: u64,
    pub added: u32,
    pub removed: u32,
    pub missing: u32,
    pub wanted: u32,
}

#[uniffi::export]
pub fn playlist_link_service(text: String) -> Option<String> {
    PlaylistLink::read(&text).map(|link| link.service().to_owned())
}

struct Matched {
    tracks: Vec<u64>,
    unmatched: Vec<SearchSong>,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn import_linked_playlist(
        &self,
        link: String,
        name: String,
        songs: Vec<SearchSong>,
        kept_in_step: bool,
    ) -> Result<ImportedPlaylist> {
        let link = PlaylistLink::read(&link).ok_or(Error::NotAPlaylistLink)?;
        let matched = self.matched(&songs)?;
        let held = matched.tracks.len() as u32;
        let missing = matched.unmatched.len() as u32;

        let id = match (kept_in_step, self.playlist_kept_from(&link)?) {
            (true, Some(standing)) => {
                self.mirrored(standing, &matched.tracks)?;
                standing
            }
            (true, None) => self.make_playlist_named_free(&name, matched.tracks)?.0,
            (false, _) if matched.tracks.is_empty() => return Err(Error::NoLocalMatches),
            (false, _) => self.make_playlist_named_free(&name, matched.tracks)?.0,
        };
        self.remember_link(id, &link, kept_in_step)?;
        if kept_in_step && !matched.unmatched.is_empty() {
            self.save_wanted(matched.unmatched)?;
        }

        Ok(ImportedPlaylist {
            id,
            matched: held,
            missing,
        })
    }

    pub fn playlist_link(&self, id: u64) -> Result<Option<PlaylistLinkInfo>> {
        let stored = link_of(&self.catalog.0.lock(), id)?;
        stored.map(|stored| stored.info()).transpose()
    }

    pub fn keep_playlist_in_step(&self, id: u64, kept_in_step: bool) -> Result<()> {
        let changed = self
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE playlist_links SET kept = ?2 WHERE playlist = ?1",
                params![id as i64, kept_in_step],
            )
            .map_err(write_error)?;
        if changed == 0 {
            return Err(Error::NoPlaylistLink);
        }
        Ok(())
    }

    pub fn forget_playlist_link(&self, id: u64) -> Result<()> {
        self.catalog
            .0
            .lock()
            .execute(
                "DELETE FROM playlist_links WHERE playlist = ?1",
                [id as i64],
            )
            .map_err(write_error)?;
        Ok(())
    }

    pub fn refresh_linked_playlist(&self, id: u64) -> Result<RefreshedPlaylist> {
        self.require_online()?;
        let stored = link_of(&self.catalog.0.lock(), id)?.ok_or(Error::NoPlaylistLink)?;
        let link = stored.link()?;
        let Some((_, songs)) = self.songs_of_a_playlist(&link)? else {
            return Err(Error::UnreadablePlaylistLink);
        };
        let matched = self.matched(&songs)?;
        let missing = matched.unmatched.len() as u32;

        let (added, removed) = match stored.kept {
            true => self.mirrored(id, &matched.tracks)?,
            false => (self.filled(id, &matched.tracks)?, 0),
        };
        let wanted = match stored.kept && !matched.unmatched.is_empty() {
            true => self.save_wanted(matched.unmatched)? as u32,
            false => 0,
        };
        self.catalog
            .0
            .lock()
            .execute(
                "UPDATE playlist_links SET refreshed = ?2 WHERE playlist = ?1",
                params![id as i64, seconds(SystemTime::now())],
            )
            .map_err(write_error)?;

        Ok(RefreshedPlaylist {
            id,
            added,
            removed,
            missing,
            wanted,
        })
    }

    pub fn playlists_due_a_refresh(&self) -> Result<Vec<u64>> {
        let due_before = seconds(SystemTime::now()) - KEPT_IN_STEP_EVERY.as_secs() as i64;
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare(
                "SELECT l.playlist FROM playlist_links l JOIN playlists p ON p.id = l.playlist
                 WHERE l.kept = 1 AND l.refreshed <= ?1 ORDER BY l.refreshed, l.playlist",
            )
            .map_err(read_error)?;
        query
            .query_map([due_before], |row| Ok(row.get::<_, i64>(0)? as u64))
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }
}

impl MobilePlayer {
    fn matched(&self, songs: &[SearchSong]) -> Result<Matched> {
        let mut matched = Matched {
            tracks: Vec::new(),
            unmatched: Vec::new(),
        };
        for song in songs.iter().take(500) {
            match local_match(&self.catalog.0.lock(), song)? {
                Some(track) => matched.tracks.push(track),
                None if !song.title.trim().is_empty() => matched.unmatched.push(song.clone()),
                None => {}
            }
        }
        Ok(matched)
    }

    pub(crate) fn playlist_kept_from(&self, link: &PlaylistLink) -> Result<Option<u64>> {
        self.catalog
            .0
            .lock()
            .query_row(
                "SELECT l.playlist FROM playlist_links l JOIN playlists p ON p.id = l.playlist
                 WHERE l.kept = 1 AND l.link = ?1 ORDER BY l.playlist LIMIT 1",
                [link.page()],
                |row| Ok(row.get::<_, i64>(0)? as u64),
            )
            .optional()
            .map_err(read_error)
    }

    fn mirrored(&self, id: u64, tracks: &[u64]) -> Result<(u32, u32)> {
        let held = rows_of(&self.catalog.0.lock(), id)?;
        if held == tracks {
            return Ok((0, 0));
        }
        let moved = (beyond(tracks, &held), beyond(&held, tracks));
        self.edit_playlist(id, None, |connection| replace_rows(connection, id, tracks))?;
        Ok(moved)
    }

    fn filled(&self, id: u64, tracks: &[u64]) -> Result<u32> {
        let held = rows_of(&self.catalog.0.lock(), id)?;
        let lacking = tracks
            .iter()
            .filter(|track| !held.contains(track))
            .copied()
            .collect::<Vec<_>>();
        if lacking.is_empty() {
            return Ok(0);
        }
        let mut rows = held;
        rows.extend_from_slice(&lacking);
        self.edit_playlist(id, None, |connection| replace_rows(connection, id, &rows))?;
        Ok(lacking.len() as u32)
    }

    fn remember_link(&self, id: u64, link: &PlaylistLink, kept_in_step: bool) -> Result<()> {
        self.catalog
            .0
            .lock()
            .execute(
                "INSERT INTO playlist_links(playlist, link, kept, refreshed) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(playlist) DO UPDATE
                 SET link = excluded.link, kept = excluded.kept, refreshed = excluded.refreshed",
                params![
                    id as i64,
                    link.page(),
                    kept_in_step,
                    seconds(SystemTime::now())
                ],
            )
            .map_err(write_error)?;
        Ok(())
    }
}

pub(crate) struct StoredLink {
    page: String,
    kept: bool,
    refreshed: i64,
}

impl StoredLink {
    fn link(&self) -> Result<PlaylistLink> {
        PlaylistLink::read(&self.page).ok_or(Error::UnreadablePlaylistLink)
    }

    fn info(&self) -> Result<PlaylistLinkInfo> {
        Ok(PlaylistLinkInfo {
            service: self.link()?.service().to_owned(),
            page: self.page.clone(),
            kept_in_step: self.kept,
            refreshed: u64::try_from(self.refreshed).unwrap_or_default(),
        })
    }
}

pub(crate) fn link_of(connection: &Connection, id: u64) -> Result<Option<StoredLink>> {
    connection
        .query_row(
            "SELECT link, kept, refreshed FROM playlist_links WHERE playlist = ?1",
            [id as i64],
            |row| {
                Ok(StoredLink {
                    page: row.get(0)?,
                    kept: row.get(1)?,
                    refreshed: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(read_error)
}

pub(crate) fn info_of(connection: &Connection, id: u64) -> Option<PlaylistLinkInfo> {
    match link_of(connection, id).and_then(|stored| stored.map(|stored| stored.info()).transpose())
    {
        Ok(info) => info,
        Err(error) => {
            tracing::warn!(%error, playlist = id, "a playlist's link was not read");
            None
        }
    }
}

fn rows_of(connection: &Connection, id: u64) -> Result<Vec<u64>> {
    let mut query = connection
        .prepare("SELECT track FROM playlist_rows WHERE playlist = ?1 ORDER BY position")
        .map_err(read_error)?;
    query
        .query_map([id as i64], |row| Ok(row.get::<_, i64>(0)? as u64))
        .map_err(read_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_error)
}

fn beyond(these: &[u64], those: &[u64]) -> u32 {
    let mut left = those.to_vec();
    these
        .iter()
        .filter(|track| match left.iter().position(|held| held == *track) {
            Some(at) => {
                left.swap_remove(at);
                false
            }
            None => true,
        })
        .count() as u32
}

fn seconds(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Track, playlists::tests::player};

    const LINK: &str = "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M";

    fn held(player: &MobilePlayer, name: &str, title: &str) -> u64 {
        player
            .catalog
            .upsert(Track {
                uri: format!("resonate-folder://local/{name}.flac"),
                name: format!("{name}.flac"),
                title: title.into(),
                artist: "Air".into(),
                duration_ms: 215_000,
                ..Track::default()
            })
            .expect("a song")
            .id
    }

    fn listed(title: &str) -> SearchSong {
        SearchSong {
            title: title.into(),
            artist: "Air".into(),
            duration_ms: Some(215_000),
            ..SearchSong::default()
        }
    }

    fn rows(player: &MobilePlayer, id: u64) -> Vec<u64> {
        player
            .playlist_tracks(id, 0, 100)
            .expect("the rows")
            .iter()
            .map(|track| track.id)
            .collect()
    }

    #[test]
    fn a_playlist_kept_in_step_takes_the_services_songs_and_order_and_wants_what_is_not_held() {
        let player = player();
        let first = held(&player, "one", "La femme d'argent");
        let second = held(&player, "two", "Sexy Boy");

        let imported = player
            .import_linked_playlist(
                LINK.into(),
                "Night drive".into(),
                vec![listed("Sexy Boy"), listed("Kelly Watch the Stars")],
                true,
            )
            .expect("a playlist kept in step");
        let again = player
            .import_linked_playlist(
                LINK.into(),
                "Night drive".into(),
                vec![listed("La femme d'argent"), listed("Sexy Boy")],
                true,
            )
            .expect("the same playlist read again");
        let link = player
            .playlist_link(imported.id)
            .expect("the link read")
            .expect("a link kept");
        let wanted = player.wanted_songs(0, 10).expect("the wants");

        assert_eq!((imported.matched, imported.missing), (1, 1));
        assert_eq!(again.id, imported.id, "a second playlist was started");
        assert_eq!(rows(&player, imported.id), vec![first, second]);
        assert_eq!(link.service, "Spotify");
        assert!(link.kept_in_step);
        assert_eq!(
            wanted
                .iter()
                .map(|want| want.song.title.as_str())
                .collect::<Vec<_>>(),
            ["Kelly Watch the Stars"]
        );
        assert_eq!(
            player.playlists().expect("the listing")[0].link,
            Some(link),
            "the listing carries the link"
        );
    }

    #[test]
    fn a_playlist_imported_once_only_gains_what_it_lacks_and_wants_nothing() {
        let player = player();
        let first = held(&player, "one", "La femme d'argent");
        let second = held(&player, "two", "Sexy Boy");

        let imported = player
            .import_linked_playlist(
                LINK.into(),
                "Night drive".into(),
                vec![listed("Sexy Boy"), listed("Kelly Watch the Stars")],
                false,
            )
            .expect("a playlist imported once");
        let gained = player
            .filled(imported.id, &[first, second])
            .expect("the lacking song added");

        assert_eq!(gained, 1);
        assert_eq!(rows(&player, imported.id), vec![second, first]);
        assert!(player.wanted_songs(0, 10).expect("the wants").is_empty());
        assert!(matches!(
            player.import_linked_playlist(
                LINK.into(),
                "Elsewhere".into(),
                vec![listed("Kelly Watch the Stars")],
                false,
            ),
            Err(Error::NoLocalMatches)
        ));
    }

    #[test]
    fn a_link_is_kept_in_step_due_once_its_hours_pass_and_forgotten_on_asking() {
        let player = player();
        held(&player, "one", "La femme d'argent");
        let imported = player
            .import_linked_playlist(
                LINK.into(),
                "Night drive".into(),
                vec![listed("La femme d'argent")],
                false,
            )
            .expect("a playlist imported once");

        player
            .keep_playlist_in_step(imported.id, true)
            .expect("kept in step");
        let due_now = player.playlists_due_a_refresh().expect("the due list");
        player
            .catalog
            .0
            .lock()
            .execute("UPDATE playlist_links SET refreshed = 0", [])
            .expect("an old refresh");
        let due_later = player.playlists_due_a_refresh().expect("the due list");
        player
            .forget_playlist_link(imported.id)
            .expect("the link forgotten");

        assert!(due_now.is_empty());
        assert_eq!(due_later, vec![imported.id]);
        assert_eq!(player.playlist_link(imported.id).expect("read"), None);
        assert!(matches!(
            player.keep_playlist_in_step(imported.id, true),
            Err(Error::NoPlaylistLink)
        ));
        assert!(matches!(
            player.import_linked_playlist(
                "https://example.com/list".into(),
                "X".into(),
                Vec::new(),
                true
            ),
            Err(Error::NotAPlaylistLink)
        ));
    }

    #[test]
    fn a_playlist_link_names_its_service_and_any_other_text_names_none() {
        assert_eq!(
            playlist_link_service(format!(" {LINK}?si=x ")).as_deref(),
            Some("Spotify")
        );
        assert_eq!(
            playlist_link_service("https://www.deezer.com/en/playlist/908622995".into()).as_deref(),
            Some("Deezer")
        );
        assert_eq!(
            playlist_link_service("https://www.youtube.com/watch?v=abc&list=PL123".into()),
            None
        );
        assert_eq!(playlist_link_service("Night drive".into()), None);
    }

    #[test]
    fn songs_added_and_taken_out_are_counted_as_runs_of_the_same_song_twice() {
        assert_eq!(beyond(&[1, 2, 2, 3], &[2, 3, 4]), 2);
        assert_eq!(beyond(&[2, 3, 4], &[1, 2, 2, 3]), 1);
    }
}
