use resonate_core::{Mbid, folded_letters};
use resonate_library::{AlbumLink, ArtistLink, FollowedLink, ListedSong, LookupOp, Reference};
use rusqlite::{Connection, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp,
    catalog::Catalog,
    features::SearchSong,
    online_tools::{SearchArtist, SearchRelease},
};

#[derive(Clone, Debug, uniffi::Record)]
pub struct OnlineCollection {
    pub name: String,
    pub songs: Vec<SearchSong>,
    pub releases: Vec<SearchRelease>,
    pub artists: Vec<SearchArtist>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ImportedPlaylist {
    pub id: u64,
    pub matched: u32,
    pub missing: u32,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct WantedSong {
    pub id: u64,
    pub song: SearchSong,
    pub held: bool,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn open_online_link(&self, text: String) -> Result<OnlineCollection> {
        self.require_online()?;
        let mut collection = OnlineCollection {
            name: String::new(),
            songs: Vec::new(),
            releases: Vec::new(),
            artists: Vec::new(),
        };
        match FollowedLink::read(&text) {
            Some(FollowedLink::Song(_)) => collection.songs = self.search_online(text)?,
            Some(FollowedLink::Album(AlbumLink::Release(id))) => {
                collection.songs = self.release_songs(id.to_string())?
            }
            Some(FollowedLink::Album(AlbumLink::Group(id))) => {
                collection.releases = self.online_releases(id.to_string())?
            }
            Some(FollowedLink::Album(link)) => {
                if let Some(names) = self
                    .services
                    .online
                    .album_linked(&link)
                    .map_err(follow_error)?
                {
                    for code in names.barcodes {
                        for release in self
                            .services
                            .online
                            .releases_by_barcode(&code)
                            .map_err(follow_error)?
                        {
                            if collection
                                .releases
                                .iter()
                                .all(|held| held.id != release.release.as_str())
                            {
                                collection.releases.push(SearchRelease {
                                    id: release.release.to_string(),
                                    title: release.title,
                                    date: String::new(),
                                    country: String::new(),
                                    songs: 0,
                                });
                            }
                        }
                    }
                }
            }
            Some(FollowedLink::Artist(ArtistLink::MusicBrainz(id))) => {
                if let Some(artist) = self.services.online.artist(&id).map_err(follow_error)? {
                    collection.artists.push(SearchArtist {
                        id: id.to_string(),
                        name: artist.name,
                        kind: artist.kind.unwrap_or_default(),
                    });
                }
            }
            Some(FollowedLink::Artist(link)) => {
                if let Some(name) = self
                    .services
                    .online
                    .artist_linked(&link)
                    .map_err(follow_error)?
                {
                    collection.artists = self.search_artists_online(name)?;
                }
            }
            Some(FollowedLink::Playlist(link)) => {
                if let Some(playlist) = self
                    .services
                    .online
                    .playlist_linked(&link)
                    .map_err(follow_error)?
                {
                    collection.name = playlist.name;
                    for song in playlist.songs.into_iter().take(500) {
                        match song {
                            ListedSong::Recording(id) => {
                                if let Some(recording) =
                                    self.services.online.recording(&id).map_err(follow_error)?
                                {
                                    collection
                                        .songs
                                        .push(crate::features::search_song(recording.into()));
                                }
                            }
                            ListedSong::Named(named) | ListedSong::Uploaded(named) => {
                                if let Some(title) = named.title {
                                    collection.songs.push(SearchSong {
                                        recording: String::new(),
                                        title,
                                        artist: named.artist.unwrap_or_default(),
                                        album: String::new(),
                                        duration_ms: named.length.map(|length| {
                                            length.as_millis().min(u128::from(u64::MAX)) as u64
                                        }),
                                        isrcs: named
                                            .isrcs
                                            .iter()
                                            .map(ToString::to_string)
                                            .collect(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            None => {}
        }
        Ok(collection)
    }

    pub fn import_matched_playlist(
        &self,
        name: String,
        songs: Vec<SearchSong>,
    ) -> Result<ImportedPlaylist> {
        let tracks = {
            let connection = self.catalog.0.lock();
            songs
                .iter()
                .take(500)
                .map(|song| local_match(&connection, song))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        };
        if tracks.is_empty() {
            return Err(Error::NoLocalMatches);
        }
        let matched = tracks.len() as u32;
        let id = self.make_playlist(name, tracks)?;
        Ok(ImportedPlaylist {
            id,
            matched,
            missing: songs.len().min(500) as u32 - matched,
        })
    }

    pub fn index_download(
        &self,
        uri: String,
        name: String,
        song: SearchSong,
    ) -> Result<crate::Track> {
        self.validate_download(uri.clone(), song.duration_ms)?;
        let track = self.index_document(uri, name)?;
        let recording = Mbid::new(&song.recording).ok().map(|id| id.to_string());
        self.catalog.0.lock().execute("UPDATE documents SET title = CASE WHEN title = name THEN ?2 ELSE title END, artist = coalesce(nullif(artist, ''), ?3), album = coalesce(nullif(album, ''), ?4), recording = coalesce(recording, ?5) WHERE id = ?1", params![track.id as i64, song.title, song.artist, song.album, recording]).map_err(write_error)?;
        self.catalog.track(track.id)
    }

    pub fn wanted_songs(&self, offset: u32, limit: u32) -> Result<Vec<WantedSong>> {
        self.catalog.wanted(offset, limit)
    }

    pub fn save_wanted(&self, songs: Vec<SearchSong>) -> Result<u64> {
        self.catalog.save_wanted(&songs)
    }

    pub fn remove_wanted(&self, id: u64) -> Result<()> {
        self.catalog
            .0
            .lock()
            .execute("DELETE FROM wanted WHERE id = ?1", [id as i64])
            .map_err(write_error)?;
        Ok(())
    }
}

impl Catalog {
    fn wanted(&self, offset: u32, limit: u32) -> Result<Vec<WantedSong>> {
        let connection = self.0.lock();
        let mut query = connection.prepare("SELECT id, recording, title, artist, album, duration_ms, isrcs FROM wanted ORDER BY added DESC, id DESC LIMIT ?1 OFFSET ?2").map_err(read_error)?;
        let songs = query
            .query_map(params![limit.min(500), offset], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    SearchSong {
                        recording: row.get(1)?,
                        title: row.get(2)?,
                        artist: row.get(3)?,
                        album: row.get(4)?,
                        duration_ms: row
                            .get::<_, Option<i64>>(5)?
                            .map(|value| value.max(0) as u64),
                        isrcs: row
                            .get::<_, String>(6)?
                            .lines()
                            .map(str::to_owned)
                            .collect(),
                    },
                ))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        songs
            .into_iter()
            .map(|(id, song)| {
                Ok(WantedSong {
                    held: local_match(&connection, &song)?.is_some(),
                    id,
                    song,
                })
            })
            .collect()
    }

    fn save_wanted(&self, songs: &[SearchSong]) -> Result<u64> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let mut saved = 0;
        for song in songs.iter().take(500) {
            if song.title.trim().is_empty() {
                return Err(Error::InvalidWantedSong);
            }
            let identity = wanted_key(song);
            let isrcs = song
                .isrcs
                .iter()
                .filter_map(|code| resonate_core::Isrc::new(code).ok())
                .map(|code| code.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            saved += transaction.execute("INSERT INTO wanted(identity, recording, title, artist, album, duration_ms, isrcs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(identity) DO NOTHING", params![identity, song.recording, song.title, song.artist, song.album, song.duration_ms.map(|value| value.min(i64::MAX as u64) as i64), isrcs]).map_err(write_error)? as u64;
        }
        transaction.commit().map_err(write_error)?;
        Ok(saved)
    }
}

fn local_match(connection: &Connection, song: &SearchSong) -> Result<Option<u64>> {
    let mut query = connection.prepare("SELECT id, coalesce(recording = ?1, 0) FROM documents WHERE (?1 != '' AND recording = ?1) OR ((recording IS NULL OR ?1 = '') AND ?3 != '' AND folded(title) = folded(?2) AND folded(artist) = folded(?3) AND (?4 IS NULL OR abs(duration_ms - ?4) <= 5000)) ORDER BY coalesce(recording = ?1, 0) DESC, id LIMIT 2").map_err(read_error)?;
    let matches = query
        .query_map(
            params![
                song.recording,
                song.title,
                song.artist,
                song.duration_ms
                    .map(|value| value.min(i64::MAX as u64) as i64)
            ],
            |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, bool>(1)?)),
        )
        .map_err(read_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_error)?;
    Ok(matches
        .first()
        .and_then(|&(id, coded)| (coded || matches.len() == 1).then_some(id)))
}

fn wanted_key(song: &SearchSong) -> String {
    if let Ok(recording) = Mbid::new(&song.recording) {
        return format!("recording:{recording}");
    }
    let title = folded_letters(song.title.trim());
    let artist = folded_letters(song.artist.trim());
    let album = folded_letters(song.album.trim());
    format!(
        "named:{}:{title}:{}:{artist}:{}:{album}:{}",
        title.len(),
        artist.len(),
        album.len(),
        song.duration_ms.unwrap_or(0)
    )
}

fn follow_error(source: resonate_library::Error) -> Error {
    Error::reference(LookupOp::FollowLink, source)
}
fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}
fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::Track;

    #[test]
    fn saved_wants_are_idempotent_and_local_matches_use_identity_or_folded_names_and_length() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = SearchSong {
            recording: String::new(),
            title: "Joga".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            duration_ms: Some(30000),
            isrcs: vec!["GBN9Y1100089".into()],
        };
        assert_eq!(
            catalog
                .save_wanted(std::slice::from_ref(&song))
                .expect("a wanted song"),
            1
        );
        assert_eq!(
            catalog
                .save_wanted(std::slice::from_ref(&song))
                .expect("the same want"),
            0
        );
        assert!(!catalog.wanted(0, 500).expect("a missing want")[0].held);
        catalog
            .upsert(Track {
                uri: "content://test/song.flac".into(),
                title: "JÓGA".into(),
                artist: "BJORK".into(),
                duration_ms: 30000,
                ..Track::default()
            })
            .expect("a local song");
        let wants = catalog.wanted(0, 500).expect("the held want");
        assert!(wants[0].held);
        assert_eq!(wants[0].song.isrcs, song.isrcs);
    }
}
