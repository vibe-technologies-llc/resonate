use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{Mbid, folded_letters};
use resonate_library::{
    AlbumLink, ArtistLink, FollowedLink, ListedSong, LookupOp, POLL_AGAIN_AFTER, RETRY_WAITS,
    Reference, TRIES_BEFORE_GIVING_UP,
};
use rusqlite::{Connection, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp,
    catalog::Catalog,
    downloads::{DownloadPhase, Tried},
    features::{Listing, SearchSong, distinct_takes, search_song},
    online_tools::{SearchArtist, SearchRelease},
};

const WANTED_READ_AT_MOST: usize = 500;

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
                                    collection.songs.push(search_song(recording.into(), &[]));
                                }
                            }
                            ListedSong::Named(named) | ListedSong::Uploaded(named) => {
                                if let Some(title) = named.title {
                                    collection.songs.push(SearchSong {
                                        title,
                                        artist: named.artist.unwrap_or_default(),
                                        duration_ms: named.length.map(|length| {
                                            length.as_millis().min(u128::from(u64::MAX)) as u64
                                        }),
                                        isrcs: named
                                            .isrcs
                                            .iter()
                                            .map(ToString::to_string)
                                            .collect(),
                                        ..SearchSong::default()
                                    });
                                }
                            }
                        }
                    }
                    collection.songs = self.offered(collection.songs, Listing::Listed)?;
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

    pub fn index_download(&self, job: u64, uri: String, name: String) -> Result<crate::Track> {
        let job = self.downloads.job(job)?;
        job.moved_to(DownloadPhase::Validating)?;
        let indexed = self.indexed_download(uri, name, &job.song);
        if matches!(indexed, Err(Error::WrongSong))
            && let Some(taken_from) = job.taken_from()
        {
            self.downloads.refuse(taken_from);
        }
        let track = job.settled(indexed)?;
        job.done();
        Ok(track)
    }

    pub fn wanted_songs(&self, offset: u32, limit: u32) -> Result<Vec<WantedSong>> {
        self.catalog.wanted(offset, limit)
    }

    pub fn poll_wanted_due(&self) -> Result<Vec<WantedSong>> {
        self.catalog.wanted_due(SystemTime::now())
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

impl MobilePlayer {
    pub(crate) fn offered(
        &self,
        songs: Vec<SearchSong>,
        listing: Listing,
    ) -> Result<Vec<SearchSong>> {
        offered(&self.catalog.0.lock(), songs, listing)
    }

    pub(crate) fn indexed_download(
        &self,
        uri: String,
        name: String,
        song: &SearchSong,
    ) -> Result<crate::Track> {
        self.validate_download(uri.clone(), song.duration_ms)?;
        let track = self.index_document(uri, name)?;
        let identified = |id: &str| Mbid::new(id).ok().map(|id| id.to_string());
        let connection = self.catalog.0.lock();
        connection.execute("UPDATE documents SET title = CASE WHEN title = name THEN ?2 ELSE title END, artist = coalesce(nullif(artist, ''), ?3), album = coalesce(nullif(album, ''), ?4), recording = coalesce(recording, ?5), release = coalesce(release, ?6), artist_mbid = coalesce(artist_mbid, ?7), track_number = coalesce(track_number, ?8), disc_number = coalesce(disc_number, ?9) WHERE id = ?1", params![track.id as i64, song.title, song.artist, song.album, identified(&song.recording), identified(&song.release), identified(&song.artist_id), song.position, song.disc]).map_err(write_error)?;
        connection
            .execute(
                "UPDATE wanted SET offered_at = unixepoch(), misses = 0 WHERE identity = ?1",
                [wanted_key(song)],
            )
            .map_err(write_error)?;
        drop(connection);
        self.catalog.track(track.id)
    }
}

pub(crate) fn offered(
    connection: &Connection,
    songs: Vec<SearchSong>,
    listing: Listing,
) -> Result<Vec<SearchSong>> {
    let songs = match listing {
        Listing::Searched => distinct_takes(songs),
        Listing::Listed => songs,
    };
    songs
        .into_iter()
        .map(|song| {
            Ok(SearchSong {
                held: local_match(connection, &song)?.is_some(),
                ..song
            })
        })
        .collect()
}

struct WantedRow {
    id: u64,
    song: SearchSong,
    tried: Option<i64>,
    misses: u32,
}

fn wanted_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WantedRow> {
    Ok(WantedRow {
        id: row.get::<_, i64>(0)? as u64,
        song: SearchSong {
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
            ..SearchSong::default()
        },
        tried: row.get(7)?,
        misses: row.get::<_, i64>(8)?.clamp(0, i64::from(u32::MAX)) as u32,
    })
}

fn wanted_song(connection: &Connection, row: WantedRow) -> Result<WantedSong> {
    let held = local_match(connection, &row.song)?.is_some();
    Ok(WantedSong {
        id: row.id,
        song: SearchSong { held, ..row.song },
        held,
    })
}

fn due_at(tried: Option<i64>, misses: u32) -> Option<SystemTime> {
    let Some(tried) = tried else {
        return Some(UNIX_EPOCH);
    };
    let wait = match misses {
        0 => POLL_AGAIN_AFTER,
        misses => *RETRY_WAITS.get(misses as usize - 1)?,
    };
    (UNIX_EPOCH + Duration::from_secs(tried.max(0) as u64)).checked_add(wait)
}

impl Catalog {
    fn wanted(&self, offset: u32, limit: u32) -> Result<Vec<WantedSong>> {
        let connection = self.0.lock();
        let mut query = connection.prepare("SELECT id, recording, title, artist, album, duration_ms, isrcs, tried, misses FROM wanted ORDER BY added DESC, id DESC LIMIT ?1 OFFSET ?2").map_err(read_error)?;
        let rows = query
            .query_map(params![limit.min(500), offset], wanted_row)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        rows.into_iter()
            .map(|row| wanted_song(&connection, row))
            .collect()
    }

    fn wanted_due(&self, now: SystemTime) -> Result<Vec<WantedSong>> {
        let connection = self.0.lock();
        let mut query = connection.prepare("SELECT id, recording, title, artist, album, duration_ms, isrcs, tried, misses FROM wanted WHERE misses < ?1 ORDER BY added, id").map_err(read_error)?;
        let rows = query
            .query_map([TRIES_BEFORE_GIVING_UP], wanted_row)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        let mut due = Vec::new();
        for row in rows {
            if due.len() >= WANTED_READ_AT_MOST {
                break;
            }
            if due_at(row.tried, row.misses).is_none_or(|due_at| due_at > now) {
                continue;
            }
            let wanted = wanted_song(&connection, row)?;
            if !wanted.held {
                due.push(wanted);
            }
        }
        Ok(due)
    }

    pub(crate) fn note_wanted_tried(&self, song: &SearchSong, tried: Tried) -> Result<()> {
        let noted = match tried {
            Tried::Offered => {
                "UPDATE wanted SET tried = unixepoch(), misses = 0 WHERE identity = ?1"
            }
            Tried::Missed => {
                "UPDATE wanted SET tried = unixepoch(), misses = misses + 1 WHERE identity = ?1"
            }
            Tried::Unheard => return Ok(()),
        };
        self.0
            .lock()
            .execute(noted, [wanted_key(song)])
            .map_err(write_error)?;
        Ok(())
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
            title: "Joga".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            duration_ms: Some(30000),
            isrcs: vec!["GBN9Y1100089".into()],
            ..SearchSong::default()
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

    fn take(title: &str, duration_ms: u64, recording: &str) -> SearchSong {
        SearchSong {
            recording: recording.to_owned(),
            title: title.to_owned(),
            artist: "Björk".to_owned(),
            duration_ms: Some(duration_ms),
            ..SearchSong::default()
        }
    }

    #[test]
    fn duplicate_takes_collapse_in_a_search_but_a_listing_keeps_its_repeats_and_both_say_what_is_held()
     {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        catalog
            .upsert(Track {
                uri: "content://test/hunter.flac".into(),
                title: "Hunter".into(),
                artist: "Bjork".into(),
                duration_ms: 255_000,
                ..Track::default()
            })
            .expect("a local song");
        let joga = "1b5b8f5d-6c3f-4f8b-8d3e-2d1f0a6b9c11";
        let songs = vec![
            take("Jóga", 305_000, ""),
            take("Hunter", 255_000, ""),
            take("JOGA", 306_500, joga),
            take("Jóga", 330_000, ""),
            take("Hunter", 255_000, ""),
        ];

        let searched = offered(&catalog.0.lock(), songs.clone(), Listing::Searched)
            .expect("the searched songs");
        let listed = offered(&catalog.0.lock(), songs, Listing::Listed).expect("the listed songs");

        assert_eq!(
            searched
                .iter()
                .map(|song| (song.title.as_str(), song.duration_ms, song.held))
                .collect::<Vec<_>>(),
            vec![
                ("JOGA", Some(306_500), false),
                ("Hunter", Some(255_000), true),
                ("Jóga", Some(330_000), false),
            ]
        );
        assert_eq!(searched[0].recording, joga);
        assert_eq!(listed.len(), 5);
        assert!(listed[1].held && listed[4].held);
    }

    #[test]
    fn a_want_missed_waits_its_turn_and_one_missed_every_time_is_given_up() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = take("Jóga", 305_000, "");
        catalog
            .save_wanted(std::slice::from_ref(&song))
            .expect("a want");
        let now = SystemTime::now();
        let due_now = |catalog: &Catalog| catalog.wanted_due(now).expect("the due wants").len();

        assert_eq!(due_now(&catalog), 1);

        catalog
            .note_wanted_tried(&song, Tried::Unheard)
            .expect("an unheard try");
        assert_eq!(due_now(&catalog), 1);

        catalog
            .note_wanted_tried(&song, Tried::Missed)
            .expect("a miss");
        assert_eq!(due_now(&catalog), 0);
        assert_eq!(
            catalog
                .wanted_due(now + RETRY_WAITS[0] + Duration::from_secs(5))
                .expect("the due wants")
                .len(),
            1
        );

        for _ in 1..TRIES_BEFORE_GIVING_UP {
            catalog
                .note_wanted_tried(&song, Tried::Missed)
                .expect("a miss");
        }
        assert_eq!(
            catalog
                .wanted_due(now + Duration::from_secs(365 * 24 * 60 * 60))
                .expect("the due wants")
                .len(),
            0
        );

        catalog
            .note_wanted_tried(&song, Tried::Offered)
            .expect("an offer");
        assert_eq!(due_now(&catalog), 0);
        assert_eq!(
            catalog
                .wanted_due(now + POLL_AGAIN_AFTER + Duration::from_secs(5))
                .expect("the due wants")
                .len(),
            1
        );
    }
}
