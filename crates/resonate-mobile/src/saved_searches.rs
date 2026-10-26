use resonate_library::{Direction, SavedQuery, SortOrder};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, MobilePlayer, Result, StoreOp};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum SearchOrder {
    #[default]
    Relevance,
    Album,
    Title,
    Artist,
    Added,
    Duration,
    Plays,
    Played,
    Favourites,
    PlaysThisMonth,
}

impl From<SearchOrder> for SortOrder {
    fn from(order: SearchOrder) -> Self {
        match order {
            SearchOrder::Relevance => Self::Relevance,
            SearchOrder::Album => Self::AlbumThenTrack,
            SearchOrder::Title => Self::Title,
            SearchOrder::Artist => Self::Artist,
            SearchOrder::Added => Self::DateAdded,
            SearchOrder::Duration => Self::Duration,
            SearchOrder::Plays => Self::Plays,
            SearchOrder::Played => Self::Played,
            SearchOrder::Favourites => Self::Favourited,
            SearchOrder::PlaysThisMonth => Self::PlaysThisMonth,
        }
    }
}

impl From<SortOrder> for SearchOrder {
    fn from(sort: SortOrder) -> Self {
        match sort {
            SortOrder::Relevance => Self::Relevance,
            SortOrder::AlbumThenTrack => Self::Album,
            SortOrder::Title => Self::Title,
            SortOrder::Artist => Self::Artist,
            SortOrder::DateAdded => Self::Added,
            SortOrder::Duration => Self::Duration,
            SortOrder::Plays => Self::Plays,
            SortOrder::Played => Self::Played,
            SortOrder::Favourited => Self::Favourites,
            SortOrder::PlaysThisMonth => Self::PlaysThisMonth,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct SavedSearch {
    pub text: String,
    pub order: SearchOrder,
    pub descending: bool,
    pub rows: Option<u32>,
}

impl SavedSearch {
    pub(crate) fn query(&self) -> SavedQuery {
        let text = self.text.trim();

        SavedQuery {
            text: (!text.is_empty()).then(|| text.to_owned()),
            sort: self.order.into(),
            reading: if self.descending {
                Direction::Descending
            } else {
                Direction::Ascending
            },
            limit: self.rows.filter(|rows| *rows > 0).map(|rows| rows as usize),
        }
    }

    pub(crate) fn of(query: &SavedQuery) -> Self {
        Self {
            text: query.text.clone().unwrap_or_default(),
            order: query.sort.into(),
            descending: query.reading == Direction::Descending,
            rows: query
                .limit
                .map(|rows| u32::try_from(rows).unwrap_or(u32::MAX)),
        }
    }
}

#[uniffi::export]
pub fn search_order_reads_descending(order: SearchOrder) -> bool {
    SortOrder::from(order).reads() == Direction::Descending
}

#[uniffi::export]
impl MobilePlayer {
    pub fn save_search(&self, name: String, search: SavedSearch) -> Result<u64> {
        self.make_saved_search(name, &search.query())
    }

    pub fn revise_search(
        &self,
        id: u64,
        revision: u64,
        name: String,
        search: SavedSearch,
    ) -> Result<()> {
        self.revise_saved_search(id, revision, name, &search.query())
    }
}

pub(crate) struct StoredQuery {
    pub(crate) text: Option<String>,
    pub(crate) sort: u8,
    pub(crate) reading: u8,
    pub(crate) rows: Option<u32>,
}

impl StoredQuery {
    pub(crate) fn of(query: &SavedQuery) -> Self {
        Self {
            text: query.text.clone(),
            sort: query.sort.code(),
            reading: query.reading.code(),
            rows: query
                .limit
                .map(|rows| u32::try_from(rows).unwrap_or(u32::MAX)),
        }
    }

    pub(crate) fn query(&self) -> Option<SavedQuery> {
        Some(SavedQuery {
            text: self.text.clone(),
            sort: SortOrder::coded(self.sort)?,
            reading: Direction::coded(self.reading)?,
            limit: self.rows.map(|rows| rows as usize),
        })
    }
}

pub(crate) fn stored_query(connection: &Connection, id: u64) -> Result<Option<StoredQuery>> {
    connection
        .query_row(
            "SELECT text, sort, reading, max_rows FROM playlist_queries WHERE playlist = ?1",
            [id as i64],
            |row| {
                Ok(StoredQuery {
                    text: row.get(0)?,
                    sort: row.get(1)?,
                    reading: row.get(2)?,
                    rows: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Read, source))
}

pub(crate) fn saved_query(connection: &Connection, id: u64) -> Result<Option<SavedQuery>> {
    Ok(stored_query(connection, id)?.and_then(|stored| stored.query()))
}

pub(crate) fn is_a_saved_search(connection: &Connection, id: u64) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist_queries WHERE playlist = ?1)",
            [id as i64],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Read, source))
}

pub(crate) fn refuse_a_saved_search(connection: &Connection, id: u64) -> Result<()> {
    if is_a_saved_search(connection, id)? {
        Err(Error::NotAList)
    } else {
        Ok(())
    }
}

pub(crate) fn write_stored_query(
    connection: &Connection,
    id: u64,
    stored: &StoredQuery,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO playlist_queries(playlist, text, sort, reading, max_rows) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(playlist) DO UPDATE SET text = excluded.text, sort = excluded.sort, reading = excluded.reading, max_rows = excluded.max_rows",
        params![id as i64, stored.text, stored.sort, stored.reading, stored.rows],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Track,
        playlists::{MusicPlaylist, tests::player},
    };

    fn song(player: &MobilePlayer, name: &str, genre: &str, favourite: bool) -> u64 {
        let track = player
            .catalog
            .upsert(Track {
                uri: format!("content://test/{name}.flac"),
                title: name.into(),
                artist: "Artist".into(),
                rate: 96_000,
                codec: "flac".into(),
                ..Track::default()
            })
            .expect("a song");
        player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET genre = ?2, favourite = ?3 WHERE id = ?1",
                rusqlite::params![track.id as i64, genre, favourite],
            )
            .expect("tags");
        track.id
    }

    fn titles(player: &MobilePlayer, id: u64) -> Vec<String> {
        player
            .playlist_tracks(id, 0, 100)
            .expect("its songs")
            .into_iter()
            .map(|track| track.title)
            .collect()
    }

    #[test]
    fn a_saved_search_answers_from_the_catalog_in_its_order_and_under_its_cap() {
        let player = player();
        song(&player, "Bravo", "Jazz", false);
        song(&player, "Alpha", "Jazz", true);
        song(&player, "Charlie", "Rock", false);
        song(&player, "Delta", "Jazz", false);

        let jazz = player
            .save_search(
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    order: SearchOrder::Title,
                    descending: true,
                    rows: Some(2),
                },
            )
            .expect("saved");
        let loved = player
            .save_search(
                "Loved first".into(),
                SavedSearch {
                    order: SearchOrder::Favourites,
                    descending: true,
                    ..SavedSearch::default()
                },
            )
            .expect("saved");

        assert_eq!(titles(&player, jazz), ["Delta", "Bravo"]);
        assert_eq!(titles(&player, loved)[0], "Alpha");
        assert_eq!(titles(&player, loved).len(), 4);
        let listed = player.playlists().expect("the playlists");
        let shown = listed
            .iter()
            .find(|playlist| playlist.id == jazz)
            .expect("the saved search");
        assert_eq!(shown.songs, 2);
        assert_eq!(
            shown.search.as_ref().map(|search| search.text.as_str()),
            Some("genre:jazz")
        );

        song(&player, "Echo", "Jazz", false);

        assert_eq!(titles(&player, jazz), ["Echo", "Delta"]);
    }

    #[test]
    fn a_saved_search_refuses_list_edits_and_is_renamed_deleted_and_restored_whole() {
        let player = player();
        let one = song(&player, "One", "Jazz", false);
        let id = player
            .save_search(
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    ..SavedSearch::default()
                },
            )
            .expect("saved");

        assert!(matches!(
            player.add_to_playlist(id, vec![one]),
            Err(Error::NotAList)
        ));
        assert!(matches!(
            player.remove_from_playlist(id, 0, 0),
            Err(Error::NotAList)
        ));
        assert!(matches!(
            player.create_playlist("JAZZ".into()),
            Err(Error::DuplicatePlaylistName)
        ));

        player
            .rename_playlist(id, 0, "Smooth".into())
            .expect("renamed");
        player.delete_playlist(id, 1).expect("deleted");
        assert!(player.playlists().expect("none").is_empty());
        assert!(player.undo_playlist().expect("restored"));

        let restored = player.playlists().expect("the playlists");
        assert_eq!(restored[0].name, "Smooth");
        assert_eq!(
            restored[0]
                .search
                .as_ref()
                .map(|search| search.text.as_str()),
            Some("genre:jazz")
        );
        assert_eq!(titles(&player, id), ["One"]);
    }

    #[test]
    fn every_order_reads_back_through_its_stored_codes() {
        for order in [
            SearchOrder::Relevance,
            SearchOrder::Album,
            SearchOrder::Title,
            SearchOrder::Artist,
            SearchOrder::Added,
            SearchOrder::Duration,
            SearchOrder::Plays,
            SearchOrder::Played,
            SearchOrder::Favourites,
            SearchOrder::PlaysThisMonth,
        ] {
            let search = SavedSearch {
                text: "is:hires".into(),
                order,
                descending: search_order_reads_descending(order),
                rows: Some(25),
            };
            let stored = StoredQuery::of(&search.query());

            assert_eq!(stored.query().as_ref().map(SavedSearch::of), Some(search));
        }
    }

    fn listed(player: &MobilePlayer, id: u64) -> MusicPlaylist {
        player
            .playlists()
            .expect("the playlists")
            .into_iter()
            .find(|playlist| playlist.id == id)
            .expect("the playlist")
    }

    #[test]
    fn a_saved_search_is_revised_in_text_order_cap_and_name_as_one_undoable_step() {
        let player = player();
        song(&player, "Bravo", "Jazz", false);
        song(&player, "Alpha", "Jazz", false);
        song(&player, "Charlie", "Rock", false);
        let id = player
            .save_search(
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    order: SearchOrder::Title,
                    ..SavedSearch::default()
                },
            )
            .expect("saved");
        let before = listed(&player, id);
        let revised = SavedSearch {
            text: "genre:rock or genre:jazz".into(),
            order: SearchOrder::Title,
            descending: true,
            rows: Some(2),
        };

        assert_eq!(titles(&player, id), ["Alpha", "Bravo"]);

        player
            .revise_search(id, before.revision, "Everything".into(), revised.clone())
            .expect("revised");

        let after = listed(&player, id);
        assert_eq!(after.name, "Everything");
        assert_eq!(after.search, Some(revised));
        assert_eq!(after.songs, 2);
        assert!(after.revision > before.revision);
        assert_eq!(titles(&player, id), ["Charlie", "Bravo"]);

        assert!(player.undo_playlist().expect("undone"));

        let undone = listed(&player, id);
        assert_eq!(undone.name, "Jazz");
        assert_eq!(undone.search, before.search);
        assert_eq!(titles(&player, id), ["Alpha", "Bravo"]);
    }

    #[test]
    fn a_revision_of_a_stale_a_taken_or_a_list_playlist_is_refused_and_changes_nothing() {
        let player = player();
        let one = song(&player, "One", "Jazz", false);
        let jazz = player
            .save_search(
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    ..SavedSearch::default()
                },
            )
            .expect("saved");
        let rock = player
            .save_search(
                "Rock".into(),
                SavedSearch {
                    text: "genre:rock".into(),
                    ..SavedSearch::default()
                },
            )
            .expect("saved");
        let list = player.create_playlist("Mix".into()).expect("a list");
        player.add_to_playlist(list, vec![one]).expect("added");
        let revision = listed(&player, jazz).revision;
        let search = SavedSearch {
            text: "genre:blues".into(),
            ..SavedSearch::default()
        };

        assert!(matches!(
            player.revise_search(jazz, revision + 1, "Jazz".into(), search.clone()),
            Err(Error::PlaylistChanged)
        ));
        assert!(matches!(
            player.revise_search(jazz, revision, "rock".into(), search.clone()),
            Err(Error::DuplicatePlaylistName)
        ));
        assert!(matches!(
            player.revise_search(jazz, revision, "  ".into(), search.clone()),
            Err(Error::InvalidPlaylistName)
        ));
        assert!(matches!(
            player.revise_search(list, listed(&player, list).revision, "Mix".into(), search),
            Err(Error::NotASavedSearch)
        ));

        let held = listed(&player, jazz);
        assert_eq!(held.revision, revision);
        assert_eq!(
            held.search.map(|search| search.text),
            Some("genre:jazz".into())
        );
        assert_eq!(listed(&player, rock).name, "Rock");
        assert_eq!(titles(&player, list), ["One"]);
    }

    #[test]
    fn a_cap_of_none_or_zero_asks_every_row() {
        let player = player();
        song(&player, "One", "Jazz", false);
        song(&player, "Two", "Jazz", false);
        let id = player
            .save_search(
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    rows: Some(1),
                    ..SavedSearch::default()
                },
            )
            .expect("saved");

        assert_eq!(titles(&player, id).len(), 1);

        player
            .revise_search(
                id,
                listed(&player, id).revision,
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    rows: Some(0),
                    ..SavedSearch::default()
                },
            )
            .expect("revised");

        assert_eq!(titles(&player, id).len(), 2);
        assert_eq!(
            listed(&player, id).search.and_then(|search| search.rows),
            None
        );
    }
}
