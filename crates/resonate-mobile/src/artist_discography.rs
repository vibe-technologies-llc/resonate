use std::{cmp::Ordering, collections::HashSet};

use resonate_core::{Mbid, folded_letters};
use resonate_library::{
    ArtistRelease, Discography, LookupOp, Reference, Release, pressing_of, worth_keeping,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, album_songs::songs_of, catalog::Catalog,
    collections::local_match, features::SearchSong,
};

const DISCOGRAPHY_KEPT_FOR_SECONDS: i64 = 30 * 24 * 60 * 60;
const FURTHER_PAGES_AT_MOST: u32 = 3;
const GROUPS_READ_AT_MOST: u32 = 8;

const ARTIST_OF_THE_NAME: &str = "SELECT artist_mbid FROM documents
    WHERE missing = 0 AND artist_mbid IS NOT NULL AND folded(artist) = ?1
    GROUP BY artist_mbid ORDER BY count(*) DESC, artist_mbid LIMIT 1";

const DISCOGRAPHY_IS_FRESH: &str = "SELECT EXISTS (SELECT 1 FROM artist_discographies
    WHERE artist = ?1 AND read_at > unixepoch() - ?2)";

const RELEASES_KEPT: &str = "SELECT release_group, title, kind, released
    FROM artist_releases WHERE artist = ?1";

const HELD_BY_THE_ARTIST: &str = "SELECT coalesce(release_group, ''), coalesce(release, ''),
        folded(album), folded(title)
    FROM documents
    WHERE missing = 0 AND (artist_mbid = ?1 OR folded(artist) = ?2 OR folded(album_artist) = ?2)";

const GROUPS_OF_ALBUMS_HELD: &str = "SELECT release_group FROM album_art
    WHERE release_group IS NOT NULL AND album_key IN (
        SELECT album_key FROM documents
        WHERE missing = 0 AND album_key IS NOT NULL
            AND (artist_mbid = ?1 OR folded(artist) = ?2 OR folded(album_artist) = ?2)
    )";

const SONGS_OF_THE_GROUP: &str =
    "SELECT recording, title, artist, artist_id, album, release, release_group,
        disc, position, duration_ms, isrcs
    FROM group_songs WHERE release_group = ?1 ORDER BY disc, position";

const GROUP_IS_READ: &str = "SELECT EXISTS (SELECT 1 FROM groups_read WHERE release_group = ?1)";

#[derive(Clone, Debug, uniffi::Record)]
pub struct ReleaseNotHeld {
    pub group: String,
    pub title: String,
    pub kind: String,
    pub year: Option<u32>,
    pub songs: u32,
    pub songs_read: bool,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn artist_releases_not_held(&self, name: String) -> Result<Vec<ReleaseNotHeld>> {
        self.catalog.artist_releases_not_held(&name)
    }

    pub fn artist_songs_not_held(&self, name: String) -> Result<Vec<SearchSong>> {
        self.catalog.artist_songs_not_held(&name)
    }

    pub fn release_songs_not_held(&self, artist: String, group: String) -> Result<Vec<SearchSong>> {
        self.catalog.release_songs_not_held(&artist, &group)
    }

    pub fn learn_artist_discography(&self, name: String) -> Result<bool> {
        if self.require_online().is_err() {
            return Ok(false);
        }

        learned_discography(&self.catalog, &name, |artist, from| {
            self.services.online.release_groups_of(artist, from)
        })
    }

    pub fn learn_artist_songs(&self, name: String) -> Result<u32> {
        if self.require_online().is_err() {
            return Ok(0);
        }

        learned_songs(&self.catalog, &name, |group| {
            self.services.online.releases_of_group(group)
        })
    }

    pub fn learn_release_songs(&self, group: String) -> Result<bool> {
        if self.require_online().is_err() {
            return Ok(false);
        }

        learned_group(&self.catalog, &group, |group| {
            self.services.online.releases_of_group(group)
        })
    }
}

#[derive(Default)]
struct HeldByTheArtist {
    groups: HashSet<String>,
    releases: HashSet<String>,
    albums: HashSet<String>,
    titles: HashSet<String>,
}

impl HeldByTheArtist {
    fn holds(&self, release: &KeptRelease) -> bool {
        let title = folded_letters(release.title.trim());

        self.groups.contains(&release.group)
            || self.albums.contains(&title)
            || (release.kind == SINGLE && self.titles.contains(&title))
    }

    fn holds_a_release_of(&self, songs: &[SearchSong]) -> bool {
        songs
            .iter()
            .any(|song| !song.release.is_empty() && self.releases.contains(&song.release))
    }

    fn holds_the_title(&self, song: &SearchSong) -> bool {
        self.titles.contains(&folded_letters(song.title.trim()))
    }
}

const SINGLE: &str = "Single";

struct KeptRelease {
    group: String,
    title: String,
    kind: String,
    released: String,
}

impl KeptRelease {
    fn year(&self) -> Option<u32> {
        self.released.get(..4).and_then(|year| year.parse().ok())
    }

    fn kind_rank(&self) -> u8 {
        match self.kind.as_str() {
            "Album" => 0,
            "EP" => 1,
            _ => 2,
        }
    }
}

fn kept_release(row: &rusqlite::Row<'_>) -> rusqlite::Result<KeptRelease> {
    Ok(KeptRelease {
        group: row.get(0)?,
        title: row.get(1)?,
        kind: row.get(2)?,
        released: row.get(3)?,
    })
}

fn newest_albums_first(one: &KeptRelease, other: &KeptRelease) -> Ordering {
    one.kind_rank()
        .cmp(&other.kind_rank())
        .then_with(|| one.released.is_empty().cmp(&other.released.is_empty()))
        .then_with(|| other.released.cmp(&one.released))
        .then_with(|| one.title.cmp(&other.title))
}

fn kept_song(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchSong> {
    Ok(SearchSong {
        recording: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        artist_id: row.get(3)?,
        album: row.get(4)?,
        release: row.get(5)?,
        release_group: row.get(6)?,
        disc: row.get(7)?,
        position: row.get(8)?,
        duration_ms: row
            .get::<_, Option<i64>>(9)?
            .map(|value| value.max(0) as u64),
        isrcs: row
            .get::<_, String>(10)?
            .split('\n')
            .filter(|isrc| !isrc.is_empty())
            .map(str::to_owned)
            .collect(),
        held: false,
    })
}

impl Catalog {
    fn artist_named(&self, name: &str) -> Result<Option<Mbid>> {
        let found: Option<String> = self
            .0
            .lock()
            .query_row(ARTIST_OF_THE_NAME, [folded_letters(name.trim())], |row| {
                row.get(0)
            })
            .optional()
            .map_err(read_error)?;

        Ok(found.and_then(|id| Mbid::new(&id).ok()))
    }

    fn held_by_the_artist(
        connection: &Connection,
        artist: Option<&Mbid>,
        name: &str,
    ) -> Result<HeldByTheArtist> {
        let id = artist.map(Mbid::as_str);
        let folded = folded_letters(name.trim());
        let mut held = HeldByTheArtist::default();

        let mut documents = connection
            .prepare_cached(HELD_BY_THE_ARTIST)
            .map_err(read_error)?;
        let rows = documents
            .query_map(params![id, folded], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(read_error)?;
        for row in rows {
            let (group, release, album, title) = row.map_err(read_error)?;
            held.groups.insert(group);
            held.releases.insert(release);
            held.albums.insert(album);
            held.titles.insert(title);
        }

        let mut matched = connection
            .prepare_cached(GROUPS_OF_ALBUMS_HELD)
            .map_err(read_error)?;
        let groups = matched
            .query_map(params![id, folded], |row| row.get::<_, String>(0))
            .map_err(read_error)?;
        for group in groups {
            held.groups.insert(group.map_err(read_error)?);
        }
        held.groups.remove("");
        held.releases.remove("");
        held.albums.remove("");

        Ok(held)
    }

    fn releases_kept(
        connection: &Connection,
        artist: &Mbid,
        held: &HeldByTheArtist,
    ) -> Result<Vec<KeptRelease>> {
        let mut releases = connection
            .prepare_cached(RELEASES_KEPT)
            .map_err(read_error)?
            .query_map([artist.as_str()], kept_release)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        releases.retain(|release| {
            let songs = Self::songs_of_the_group(connection, &release.group).unwrap_or_default();
            !held.holds(release) && !held.holds_a_release_of(&songs)
        });
        releases.sort_by(newest_albums_first);

        Ok(releases)
    }

    fn songs_of_the_group(connection: &Connection, group: &str) -> Result<Vec<SearchSong>> {
        connection
            .prepare_cached(SONGS_OF_THE_GROUP)
            .map_err(read_error)?
            .query_map([group], kept_song)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn group_is_read(connection: &Connection, group: &str) -> Result<bool> {
        connection
            .query_row(GROUP_IS_READ, [group], |row| row.get(0))
            .map_err(read_error)
    }

    fn songs_left_by(
        connection: &Connection,
        held: &HeldByTheArtist,
        songs: Vec<SearchSong>,
    ) -> Result<Vec<SearchSong>> {
        songs
            .into_iter()
            .filter_map(|song| {
                if held.holds_the_title(&song) {
                    return None;
                }
                match local_match(connection, &song) {
                    Ok(Some(_)) => None,
                    Ok(None) => Some(Ok(song)),
                    Err(failure) => Some(Err(failure)),
                }
            })
            .collect()
    }

    pub(crate) fn artist_releases_not_held(&self, name: &str) -> Result<Vec<ReleaseNotHeld>> {
        let Some(artist) = self.artist_named(name)? else {
            return Ok(Vec::new());
        };
        let connection = self.0.lock();
        let held = Self::held_by_the_artist(&connection, Some(&artist), name)?;

        Self::releases_kept(&connection, &artist, &held)?
            .into_iter()
            .map(|release| {
                let read = Self::group_is_read(&connection, &release.group)?;
                let songs = Self::songs_of_the_group(&connection, &release.group)?;
                let songs = Self::songs_left_by(&connection, &held, songs)?;
                Ok(ReleaseNotHeld {
                    year: release.year(),
                    songs: songs.len() as u32,
                    songs_read: read,
                    group: release.group,
                    title: release.title,
                    kind: release.kind,
                })
            })
            .collect()
    }

    pub(crate) fn artist_songs_not_held(&self, name: &str) -> Result<Vec<SearchSong>> {
        let Some(artist) = self.artist_named(name)? else {
            return Ok(Vec::new());
        };
        let connection = self.0.lock();
        let held = Self::held_by_the_artist(&connection, Some(&artist), name)?;
        let mut seen_recordings = HashSet::new();
        let mut seen_titles = HashSet::new();
        let mut offered = Vec::new();

        for release in Self::releases_kept(&connection, &artist, &held)? {
            let songs = Self::songs_of_the_group(&connection, &release.group)?;
            for song in Self::songs_left_by(&connection, &held, songs)? {
                let new_recording =
                    song.recording.is_empty() || seen_recordings.insert(song.recording.clone());
                let new_title = seen_titles.insert(folded_letters(song.title.trim()));
                if new_recording && new_title {
                    offered.push(song);
                }
            }
        }

        Ok(offered)
    }

    pub(crate) fn release_songs_not_held(
        &self,
        artist: &str,
        group: &str,
    ) -> Result<Vec<SearchSong>> {
        let id = self.artist_named(artist)?;
        let connection = self.0.lock();
        let held = Self::held_by_the_artist(&connection, id.as_ref(), artist)?;
        let songs = Self::songs_of_the_group(&connection, group)?;

        Self::songs_left_by(&connection, &held, songs)
    }

    fn discography_is_fresh(&self, artist: &Mbid) -> Result<bool> {
        self.0
            .lock()
            .query_row(
                DISCOGRAPHY_IS_FRESH,
                params![artist.as_str(), DISCOGRAPHY_KEPT_FOR_SECONDS],
                |row| row.get(0),
            )
            .map_err(read_error)
    }

    fn keep_discography(&self, artist: &Mbid, releases: &[ArtistRelease]) -> Result<usize> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        transaction
            .execute(
                "DELETE FROM artist_releases WHERE artist = ?1",
                [artist.as_str()],
            )
            .map_err(write_error)?;
        let mut kept = 0;
        {
            let mut insert = transaction
                .prepare_cached(
                    "INSERT OR IGNORE INTO artist_releases
                         (artist, release_group, title, kind, released)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(write_error)?;
            for release in releases.iter().filter(|release| worth_keeping(release)) {
                kept += insert
                    .execute(params![
                        artist.as_str(),
                        release.mbid.as_str(),
                        release.title,
                        release.kind.as_deref().unwrap_or_default(),
                        release.first_released.as_deref().unwrap_or_default(),
                    ])
                    .map_err(write_error)?;
            }
        }
        transaction
            .execute(
                "INSERT INTO artist_discographies (artist, read_at) VALUES (?1, unixepoch())
                 ON CONFLICT(artist) DO UPDATE SET read_at = excluded.read_at",
                [artist.as_str()],
            )
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)?;

        Ok(kept)
    }

    fn keep_the_pressing(&self, group: &str, pressing: Option<&Release>) -> Result<usize> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        transaction
            .execute("DELETE FROM group_songs WHERE release_group = ?1", [group])
            .map_err(write_error)?;
        let mut kept = 0;
        if let Some(pressing) = pressing {
            let mut insert = transaction
                .prepare_cached(
                    "INSERT OR IGNORE INTO group_songs
                         (release_group, disc, position, recording, title, artist, artist_id,
                          album, release, duration_ms, isrcs)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )
                .map_err(write_error)?;
            for song in songs_of(pressing) {
                kept += insert
                    .execute(params![
                        group,
                        song.disc,
                        song.position,
                        song.recording,
                        song.title,
                        song.artist,
                        song.artist_id,
                        song.album,
                        song.release,
                        song.duration_ms
                            .map(|value| value.min(i64::MAX as u64) as i64),
                        song.isrcs.join("\n"),
                    ])
                    .map_err(write_error)?;
            }
        }
        transaction
            .execute(
                "INSERT INTO groups_read (release_group, read_at) VALUES (?1, unixepoch())
                 ON CONFLICT(release_group) DO UPDATE SET read_at = excluded.read_at",
                [group],
            )
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)?;

        Ok(kept)
    }

    fn groups_to_read(&self, artist: &Mbid, name: &str) -> Result<Vec<String>> {
        let connection = self.0.lock();
        let held = Self::held_by_the_artist(&connection, Some(artist), name)?;
        let mut unread = Vec::new();
        for release in Self::releases_kept(&connection, artist, &held)? {
            if !Self::group_is_read(&connection, &release.group)? {
                unread.push(release.group);
            }
        }

        Ok(unread)
    }
}

fn learned_discography(
    catalog: &Catalog,
    name: &str,
    mut asked: impl FnMut(&Mbid, u32) -> resonate_library::Result<Discography>,
) -> Result<bool> {
    let Some(artist) = catalog.artist_named(name)? else {
        return Ok(false);
    };
    if catalog.discography_is_fresh(&artist)? {
        return Ok(false);
    }

    let mut releases = Vec::new();
    let mut from = 0;
    for _ in 0..=FURTHER_PAGES_AT_MOST {
        let page = asked(&artist, from)
            .map_err(|source| Error::reference(LookupOp::ReleaseGroupsOfArtist, source))?;
        let further = (page.unread > 0 && page.read_to > from).then_some(page.read_to);
        releases.extend(page.releases);
        match further {
            Some(next) => from = next,
            None => break,
        }
    }

    Ok(catalog.keep_discography(&artist, &releases)? > 0)
}

fn learned_group(
    catalog: &Catalog,
    group: &str,
    mut asked: impl FnMut(&Mbid) -> resonate_library::Result<Vec<Release>>,
) -> Result<bool> {
    if Catalog::group_is_read(&catalog.0.lock(), group)? {
        return Ok(false);
    }
    let id = Mbid::new(group)?;
    let pressing = pressing_of(
        asked(&id).map_err(|source| Error::reference(LookupOp::ReleasesOfGroup, source))?,
    );

    Ok(catalog.keep_the_pressing(group, pressing.as_ref())? > 0)
}

fn learned_songs(
    catalog: &Catalog,
    name: &str,
    mut asked: impl FnMut(&Mbid) -> resonate_library::Result<Vec<Release>>,
) -> Result<u32> {
    let Some(artist) = catalog.artist_named(name)? else {
        return Ok(0);
    };

    let mut read = 0;
    for group in catalog
        .groups_to_read(&artist, name)?
        .into_iter()
        .take(GROUPS_READ_AT_MOST as usize)
    {
        learned_group(catalog, &group, &mut asked)?;
        read += 1;
    }

    Ok(read)
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, path::Path, time::Duration};

    use resonate_library::{Credit, Medium, ReleaseTrack};

    use super::*;
    use crate::Track;

    const ARTIST: &str = "44444444-4444-4444-8444-444444444444";
    const HELD_GROUP: &str = "55555555-5555-4555-8555-555555555555";
    const LACKED_GROUP: &str = "66666666-6666-4666-8666-666666666666";
    const SINGLE_GROUP: &str = "77777777-7777-4777-8777-777777777777";
    const PRESSING: &str = "88888888-8888-4888-8888-888888888888";
    const RECORDING: &str = "99999999-9999-4999-8999-999999999999";
    const NAME: &str = "The band";

    fn opened() -> Catalog {
        Catalog::open(Path::new(":memory:")).expect("a catalog")
    }

    fn released(group: &str, title: &str, kind: &str, date: &str) -> ArtistRelease {
        ArtistRelease {
            mbid: Mbid::new(group).expect("an mbid"),
            title: title.into(),
            kind: Some(kind.into()),
            secondary: Vec::new(),
            first_released: Some(date.into()),
        }
    }

    fn discography(releases: Vec<ArtistRelease>) -> Discography {
        Discography {
            releases,
            unread: 0,
            read_to: 0,
        }
    }

    fn held(catalog: &Catalog, title: &str, album: &str) -> Track {
        let song = catalog
            .upsert(Track {
                uri: format!("content://test/{title}.flac"),
                name: format!("{title}.flac"),
                title: title.into(),
                artist: NAME.into(),
                album: album.into(),
                duration_ms: 90_000,
                ..Track::default()
            })
            .expect("a song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET artist_mbid = ?1 WHERE id = ?2",
                params![ARTIST, song.id as i64],
            )
            .expect("an artist id");

        song
    }

    fn pressing(titles: &[&str]) -> Release {
        Release {
            id: Mbid::new(PRESSING).expect("an mbid"),
            group: None,
            title: "Lacked".into(),
            credit: vec![Credit {
                name: NAME.into(),
                joined_by: String::new(),
                mbid: None,
            }],
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media: vec![Medium {
                position: 1,
                format: None,
                title: None,
                tracks: titles
                    .iter()
                    .enumerate()
                    .map(|(at, title)| ReleaseTrack {
                        position: at as u32 + 1,
                        number: (at + 1).to_string(),
                        title: (*title).into(),
                        artist: None,
                        recording: (at == 0).then(|| Mbid::new(RECORDING).expect("an mbid")),
                        track: None,
                        length: Some(Duration::from_secs(100)),
                        isrcs: Vec::new(),
                        links: Vec::new(),
                    })
                    .collect(),
            }],
        }
    }

    fn learned(catalog: &Catalog, releases: Vec<ArtistRelease>) -> bool {
        learned_discography(catalog, NAME, |_, _| Ok(discography(releases.clone())))
            .expect("learned")
    }

    fn groups(lacking: &[ReleaseNotHeld]) -> Vec<&str> {
        lacking
            .iter()
            .map(|release| release.group.as_str())
            .collect()
    }

    fn titles(songs: &[SearchSong]) -> Vec<&str> {
        songs.iter().map(|song| song.title.as_str()).collect()
    }

    #[test]
    fn a_release_the_albums_group_or_title_names_is_held_and_the_rest_are_listed_newest_first() {
        let catalog = opened();
        let song = held(&catalog, "One", "Held album");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET release_group = ?1 WHERE id = ?2",
                params![HELD_GROUP, song.id as i64],
            )
            .expect("a group");
        held(&catalog, "Two", "Same Title");

        assert!(learned(
            &catalog,
            vec![
                released(
                    HELD_GROUP,
                    "Held under another spelling",
                    "Album",
                    "2001-01-01"
                ),
                released(LACKED_GROUP, "Lacked", "Album", "2010-05-05"),
                released(SINGLE_GROUP, "Other", "Single", "2012"),
                released(PRESSING, "Same title", "Album", "2005"),
            ]
        ));

        let lacking = catalog.artist_releases_not_held(NAME).expect("the rows");

        assert_eq!(groups(&lacking), vec![LACKED_GROUP, SINGLE_GROUP]);
        assert_eq!(lacking[0].year, Some(2010));
        assert_eq!(lacking[0].kind, "Album");
        assert!(!lacking[0].songs_read);
    }

    #[test]
    fn a_single_is_held_wherever_its_song_is_and_a_live_album_is_never_kept() {
        let catalog = opened();
        held(&catalog, "Single Song", "Some album");
        let mut live = released(LACKED_GROUP, "Live", "Album", "2003");
        live.secondary = vec!["Live".into()];

        learned(
            &catalog,
            vec![
                released(SINGLE_GROUP, "single song", "Single", "2004"),
                live,
            ],
        );

        assert!(
            catalog
                .artist_releases_not_held(NAME)
                .expect("the rows")
                .is_empty()
        );
    }

    #[test]
    fn the_songs_of_an_unheld_release_are_read_once_and_listed_without_what_is_held() {
        let catalog = opened();
        held(&catalog, "Held song", "Held album");
        learned(
            &catalog,
            vec![released(LACKED_GROUP, "Lacked", "Album", "2010")],
        );
        let mut asked = 0;
        let mut ask = |_: &Mbid| {
            asked += 1;
            Ok(vec![pressing(&["First", "held song", "Third"])])
        };

        assert_eq!(learned_songs(&catalog, NAME, &mut ask).expect("read"), 1);
        assert_eq!(learned_songs(&catalog, NAME, &mut ask).expect("kept"), 0);
        assert_eq!(asked, 1);

        let songs = catalog.artist_songs_not_held(NAME).expect("the songs");
        let lacking = catalog.artist_releases_not_held(NAME).expect("the rows");

        assert_eq!(titles(&songs), vec!["First", "Third"]);
        assert_eq!(songs[0].recording, RECORDING);
        assert_eq!(songs[0].album, "Lacked");
        assert_eq!((songs[0].disc, songs[0].position), (Some(1), Some(1)));
        assert!(!songs[0].held);
        assert_eq!((lacking[0].songs, lacking[0].songs_read), (2, true));
        assert_eq!(
            catalog
                .release_songs_not_held(NAME, LACKED_GROUP)
                .expect("the release's songs")
                .len(),
            2
        );
    }

    #[test]
    fn a_song_on_two_releases_is_offered_once_and_a_release_with_no_pressing_is_read_empty() {
        let catalog = opened();
        held(&catalog, "Held song", "Held album");
        learned(
            &catalog,
            vec![
                released(LACKED_GROUP, "Lacked", "Album", "2010"),
                released(SINGLE_GROUP, "Lacked single", "Single", "2011"),
            ],
        );
        let mut ask = |group: &Mbid| {
            Ok(if group.as_str() == LACKED_GROUP {
                vec![pressing(&["Shared", "Only here"])]
            } else {
                Vec::new()
            })
        };

        assert_eq!(learned_songs(&catalog, NAME, &mut ask).expect("read"), 2);

        let songs = catalog.artist_songs_not_held(NAME).expect("the songs");

        assert_eq!(titles(&songs), vec!["Shared", "Only here"]);
        assert_eq!(learned_songs(&catalog, NAME, &mut ask).expect("kept"), 0);
    }

    #[test]
    fn a_song_downloaded_from_a_release_makes_the_release_held() {
        let catalog = opened();
        held(&catalog, "Held song", "Held album");
        learned(
            &catalog,
            vec![released(LACKED_GROUP, "Lacked", "Album", "2010")],
        );
        learned_songs(&catalog, NAME, |_| Ok(vec![pressing(&["First", "Second"])])).expect("read");
        let landed = held(&catalog, "First", "Lacked album as tagged");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?1, release = ?2 WHERE id = ?3",
                params![RECORDING, PRESSING, landed.id as i64],
            )
            .expect("a download's names");

        assert!(
            catalog
                .artist_releases_not_held(NAME)
                .expect("the rows")
                .is_empty()
        );
    }

    #[test]
    fn a_discography_is_asked_once_a_month_and_an_artist_without_an_id_is_never_asked() {
        let catalog = opened();
        held(&catalog, "Held song", "Held album");
        let asked = Cell::new(0);
        let ask = |_: &Mbid, _: u32| {
            asked.set(asked.get() + 1);
            Ok(discography(vec![released(
                LACKED_GROUP,
                "Lacked",
                "Album",
                "2010",
            )]))
        };

        assert!(learned_discography(&catalog, NAME, &ask).expect("asked"));
        assert!(!learned_discography(&catalog, NAME, &ask).expect("kept"));
        assert!(!learned_discography(&catalog, "Nobody", &ask).expect("unknown"));
        assert_eq!(asked.get(), 1);

        catalog
            .0
            .lock()
            .execute(
                "UPDATE artist_discographies SET read_at = read_at - ?1",
                [DISCOGRAPHY_KEPT_FOR_SECONDS + 1],
            )
            .expect("an old read");

        assert!(learned_discography(&catalog, NAME, &ask).expect("asked again"));
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn a_discography_with_more_to_read_is_followed_a_few_pages_and_kept_whole() {
        let catalog = opened();
        held(&catalog, "Held song", "Held album");
        let mut pages = Vec::new();

        let learned = learned_discography(&catalog, NAME, |_, from| {
            pages.push(from);
            Ok(if from == 0 {
                Discography {
                    releases: vec![released(LACKED_GROUP, "Lacked", "Album", "2010")],
                    unread: 5,
                    read_to: 100,
                }
            } else {
                discography(vec![released(SINGLE_GROUP, "Later", "Album", "2011")])
            })
        })
        .expect("learned");

        assert!(learned);
        assert_eq!(pages, vec![0, 100]);
        assert_eq!(
            groups(&catalog.artist_releases_not_held(NAME).expect("the rows")),
            vec![SINGLE_GROUP, LACKED_GROUP]
        );
    }
}
