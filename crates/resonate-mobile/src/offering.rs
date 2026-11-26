use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_codec::{ImageFormat, Sources};
use resonate_core::{AlbumId, ArtistId, PlaylistId, TrackId, folded_letters, is_lettered};
use resonate_upnp::{
    AudioKind, Content, Entry, Field, Held, Kind, MediaServer, Offering, Opened, Page, Picture,
    Query, Searched, ServerName, Shelf, Shelved, Song, Test, Udn, Within,
};
use rusqlite::{OptionalExtension as _, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::Catalog,
    documents::location,
    saved_searches::saved_query,
    search::{CollectionOrder, Index, Row, Searching, album_order, artist_order},
    turns::ReaderShare,
};

const UDN_KEPT_IN: &str = "media-server-udn";

const LISTS_KEPT_AT_MOST: usize = 32;

const READERS_SHARED_AT_MOST: usize = 4;

const A_TURN_IS_AWAITED_AT_MOST: Duration = Duration::from_secs(5);

const LETTERS_BEGUN_AT_LEAST: usize = 2;

const NAMED_FROM: u64 = 0xcbf2_9ce4_8422_2325;

const NAMED_BY: u64 = 0x0000_0100_0000_01b3;

const PLAYLISTS_BY_NAME: &str = "SELECT id, name FROM playlists ORDER BY folded, id";

const PLAYLISTS_HELD: &str = "SELECT COUNT(*) FROM playlists";

const PLAYLISTS_PAGED: &str =
    "SELECT id, name FROM playlists ORDER BY folded, id LIMIT ?1 OFFSET ?2";

const PLAYLIST_ROWS_COUNTED: &str = "SELECT r.playlist, COUNT(*) FROM playlist_rows r JOIN documents d ON d.id = r.track WHERE d.missing = 0 AND served(d.uri, d.name) GROUP BY r.playlist";

const SAVED_PLAYLISTS: &str = "SELECT playlist FROM playlist_queries";

const A_PLAYLISTS_PAGE: &str = "SELECT r.track FROM playlist_rows r JOIN documents d ON d.id = r.track WHERE r.playlist = ?1 AND d.missing = 0 AND served(d.uri, d.name) ORDER BY r.position LIMIT ?2 OFFSET ?3";

const A_PLAYLISTS_NAME: &str = "SELECT name FROM playlists WHERE id = ?1";

const A_PLAYLISTS_SONGS: &str = "SELECT r.track FROM playlist_rows r JOIN documents d ON d.id = r.track WHERE r.playlist = ?1 AND d.missing = 0 ORDER BY r.position";

const SIZES_KNOWN: &str = "SELECT id, size FROM documents WHERE missing = 0 AND size IS NOT NULL";

const NO_VALUES: [&str; 2] = ["", ""];

pub(crate) struct Shelves {
    catalog: Arc<Catalog>,
    searching: Arc<Searching>,
    sources: Arc<Sources>,
    view: Mutex<Option<Arc<View>>>,
    readers: ReaderShare,
}

struct View {
    index: Arc<Index>,
    songs: Vec<usize>,
    songs_by_an_artist: Vec<usize>,
    songs_on_an_album: Vec<usize>,
    song_at: HashMap<u64, usize>,
    albums: Vec<Album>,
    albums_by_an_artist: Vec<usize>,
    album_at: HashMap<AlbumId, usize>,
    album_of_group: HashMap<usize, usize>,
    artists: Vec<Artist>,
    artist_at: HashMap<ArtistId, usize>,
    sizes: HashMap<u64, u64>,
    lists: Mutex<HashMap<u64, Arc<Vec<usize>>>>,
    counts: Mutex<Option<Arc<Counts>>>,
}

struct Counts {
    rows: HashMap<u64, u32>,
    saved: HashSet<u64>,
    searched: Mutex<HashMap<u64, u32>>,
}

struct Album {
    id: AlbumId,
    group: usize,
    songs: Vec<usize>,
}

struct Artist {
    id: ArtistId,
    group: usize,
    albums: Vec<usize>,
}

struct Playlist {
    id: PlaylistId,
    name: String,
}

enum Wanted {
    All(Vec<Self>),
    Any(Vec<Self>),
    Named {
        field: Field,
        whole: String,
        denied: bool,
    },
    Begun {
        field: Field,
        words: Vec<String>,
        denied: bool,
    },
    Present {
        field: Field,
        held: bool,
    },
}

impl Shelves {
    pub(crate) fn over(
        catalog: Arc<Catalog>,
        searching: Arc<Searching>,
        sources: Arc<Sources>,
    ) -> Self {
        Self {
            catalog,
            searching,
            sources,
            view: Mutex::new(None),
            readers: ReaderShare::of(READERS_SHARED_AT_MOST, A_TURN_IS_AWAITED_AT_MOST),
        }
    }

    fn view(&self) -> Result<Arc<View>> {
        let index = self.searching.index(&self.catalog)?;

        if let Some(view) = self
            .view
            .lock()
            .as_ref()
            .filter(|view| Arc::ptr_eq(&view.index, &index))
        {
            return Ok(Arc::clone(view));
        }

        let view = Arc::new(View::of(index, self.sizes()?));

        *self.view.lock() = Some(Arc::clone(&view));
        Ok(view)
    }

    fn sizes(&self) -> Result<HashMap<u64, u64>> {
        let connection = self.catalog.0.lock();
        let mut query = connection.prepare_cached(SIZES_KNOWN).map_err(read_error)?;

        let sizes = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
                ))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(read_error)?;
        Ok(sizes)
    }

    fn playlists(&self) -> Result<Vec<Playlist>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare_cached(PLAYLISTS_BY_NAME)
            .map_err(read_error)?;

        let listed = query
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok(listed
            .into_iter()
            .filter_map(|(id, name)| {
                PlaylistId::new(id as u64)
                    .ok()
                    .map(|id| Playlist { id, name })
            })
            .collect())
    }

    fn playlists_paged(&self, page: Page) -> Result<(Vec<Playlist>, u32)> {
        let connection = self.catalog.0.lock();
        let total: i64 = connection
            .query_row(PLAYLISTS_HELD, [], |row| row.get(0))
            .map_err(read_error)?;
        let mut query = connection
            .prepare_cached(PLAYLISTS_PAGED)
            .map_err(read_error)?;

        let listed = query
            .query_map(params![i64::from(page.most), i64::from(page.from)], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok((
            listed
                .into_iter()
                .filter_map(|(id, name)| {
                    PlaylistId::new(id as u64)
                        .ok()
                        .map(|id| Playlist { id, name })
                })
                .collect(),
            counted(usize::try_from(total).unwrap_or_default()),
        ))
    }

    fn counts_in_sql(&self) -> Result<Counts> {
        let connection = self.catalog.0.lock();

        let rows = connection
            .prepare_cached(PLAYLIST_ROWS_COUNTED)
            .map_err(read_error)?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    u32::try_from(row.get::<_, i64>(1)?).unwrap_or(u32::MAX),
                ))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(read_error)?;
        let saved = connection
            .prepare_cached(SAVED_PLAYLISTS)
            .map_err(read_error)?
            .query_map([], |row| row.get::<_, i64>(0).map(|id| id as u64))
            .map_err(read_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(read_error)?;
        Ok(Counts {
            rows,
            saved,
            searched: Mutex::new(HashMap::new()),
        })
    }

    fn counts(&self, view: &View) -> Result<Arc<Counts>> {
        if let Some(counts) = view.counts.lock().as_ref() {
            return Ok(Arc::clone(counts));
        }

        let counts = Arc::new(self.counts_in_sql()?);

        *view.counts.lock() = Some(Arc::clone(&counts));
        Ok(counts)
    }

    fn count_of(&self, view: &View, id: PlaylistId) -> Result<u32> {
        let counts = self.counts(view)?;

        if !counts.saved.contains(&id.get()) {
            return Ok(counts.rows.get(&id.get()).copied().unwrap_or_default());
        }
        if let Some(kept) = counts.searched.lock().get(&id.get()) {
            return Ok(*kept);
        }

        let counted = counted(self.listed(view, id)?.len());

        counts.searched.lock().insert(id.get(), counted);
        Ok(counted)
    }

    fn playlist_tracks(&self, id: PlaylistId, page: Page) -> Result<Vec<u64>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare_cached(A_PLAYLISTS_PAGE)
            .map_err(read_error)?;

        query
            .query_map(
                params![id.get() as i64, i64::from(page.most), i64::from(page.from)],
                |row| row.get::<_, i64>(0).map(|id| id as u64),
            )
            .map_err(read_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(read_error)
    }

    fn playlist_songs(&self, view: &View, id: PlaylistId, page: Page) -> Result<Option<Shelved>> {
        if self.playlist(id)?.is_none() {
            return Ok(None);
        }
        if self.counts(view)?.saved.contains(&id.get()) {
            return Ok(Some(view.songs_paged(&self.listed(view, id)?, page)));
        }

        let total = self.count_of(view, id)?;

        if page.most == 0 || page.from >= total {
            return Ok(Some(Shelved {
                held: Vec::new(),
                total,
            }));
        }

        let rows: Vec<usize> = self
            .playlist_tracks(id, page)?
            .into_iter()
            .filter_map(|song| view.song_at.get(&song).copied())
            .collect();
        Ok(Some(Shelved {
            held: view.songs_of(&rows),
            total,
        }))
    }

    fn playlist(&self, id: PlaylistId) -> Result<Option<Playlist>> {
        let name = self
            .catalog
            .0
            .lock()
            .query_row(A_PLAYLISTS_NAME, [id.get() as i64], |row| row.get(0))
            .optional()
            .map_err(read_error)?;
        Ok(name.map(|name| Playlist { id, name }))
    }

    fn listed(&self, view: &View, id: PlaylistId) -> Result<Arc<Vec<usize>>> {
        if let Some(kept) = view.lists.lock().get(&id.get()) {
            return Ok(Arc::clone(kept));
        }

        let saved = saved_query(&self.catalog.0.lock(), id.get())?;
        let songs: Vec<u64> = match saved {
            Some(query) => {
                let (index, found) = self.searching.saved(&self.catalog, &query)?;
                found
                    .into_iter()
                    .map(|at| index.rows[at].track.id)
                    .collect()
            }
            None => {
                let connection = self.catalog.0.lock();
                let mut query = connection
                    .prepare_cached(A_PLAYLISTS_SONGS)
                    .map_err(read_error)?;
                query
                    .query_map([id.get() as i64], |row| {
                        row.get::<_, i64>(0).map(|id| id as u64)
                    })
                    .map_err(read_error)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(read_error)?
            }
        };
        let rows = Arc::new(
            songs
                .into_iter()
                .filter_map(|song| view.song_at.get(&song).copied())
                .collect::<Vec<_>>(),
        );

        let mut lists = view.lists.lock();

        if lists.len() >= LISTS_KEPT_AT_MOST {
            lists.clear();
        }
        lists.insert(id.get(), Arc::clone(&rows));
        Ok(rows)
    }

    fn songs_within(&self, view: &View, within: Within) -> Result<Arc<Vec<usize>>> {
        Ok(match within {
            Within::Catalog => Arc::new(view.songs.clone()),
            Within::Artists => Arc::new(view.songs_by_an_artist.clone()),
            Within::Albums => Arc::new(view.songs_on_an_album.clone()),
            Within::Artist(id) => Arc::new(
                view.artist_at
                    .get(&id)
                    .map(|at| view.songs_by(*at))
                    .unwrap_or_default(),
            ),
            Within::Album(id) => Arc::new(
                view.album_at
                    .get(&id)
                    .map(|at| view.albums[*at].songs.clone())
                    .unwrap_or_default(),
            ),
            Within::Playlist(id) => match self.playlist(id)? {
                Some(_) => self.listed(view, id)?,
                None => Arc::new(Vec::new()),
            },
        })
    }

    fn playlist_held(&self, view: &View, playlist: Playlist) -> Result<Held> {
        Ok(Held {
            shelf: Shelf::Playlist(playlist.id),
            children: self.count_of(view, playlist.id)?,
            title: playlist.name,
            artist: None,
            cover: None,
        })
    }

    fn playlists_held(&self, view: &View, found: Vec<Playlist>, page: Page) -> Result<Shelved> {
        let total = counted(found.len());
        let held = found
            .into_iter()
            .skip(page.from as usize)
            .take(page.most as usize)
            .map(|playlist| self.playlist_held(view, playlist).map(Entry::Shelf))
            .collect::<Result<_>>()?;
        Ok(Shelved { held, total })
    }
}

impl View {
    fn of(index: Arc<Index>, sizes: HashMap<u64, u64>) -> Self {
        let rows = &index.rows;
        let served: Vec<bool> = rows
            .iter()
            .map(|row| kind_of(&row.track).is_some())
            .collect();

        let mut songs: Vec<usize> = (0..rows.len()).filter(|at| served[*at]).collect();

        songs.sort_by(|one, other| in_title_order(&rows[*one], &rows[*other]));

        let song_at = songs.iter().map(|at| (rows[*at].track.id, *at)).collect();

        let mut groups: Vec<usize> = (0..index.albums.len()).collect();

        groups.sort_by(|one, other| {
            album_order(
                CollectionOrder::Alphabetical,
                &index.albums[*one],
                &index.albums[*other],
            )
        });

        let mut albums: Vec<Album> = Vec::new();
        let mut album_at = HashMap::new();
        let mut album_of_group = HashMap::new();

        for group in groups {
            let held = &index.albums[group];
            let songs: Vec<usize> = held.rows.iter().copied().filter(|at| served[*at]).collect();

            if songs.is_empty() {
                continue;
            }

            let id = AlbumId::of(named(&held.key));

            if album_at.contains_key(&id) {
                tracing::warn!(album = %held.title, "an album named like another is left off the network");
                continue;
            }
            album_at.insert(id, albums.len());
            album_of_group.insert(group, albums.len());
            albums.push(Album { id, group, songs });
        }

        let mut groups: Vec<usize> = (0..index.artists.len()).collect();

        groups.sort_by(|one, other| {
            artist_order(
                CollectionOrder::Alphabetical,
                &index.artists[*one],
                &index.artists[*other],
            )
        });

        let mut artists: Vec<Artist> = Vec::new();
        let mut artist_at = HashMap::new();

        for group in groups {
            let held = &index.artists[group];

            if held.filed.is_empty() || !held.rows.iter().any(|at| served[*at]) {
                continue;
            }

            let id = ArtistId::of(named(&held.filed));

            if artist_at.contains_key(&id) {
                tracing::warn!(artist = %held.name, "an artist named like another is left off the network");
                continue;
            }

            let mut by_them: Vec<usize> = held
                .rows
                .iter()
                .filter_map(|at| rows[*at].album_group)
                .filter_map(|group| album_of_group.get(&group).copied())
                .collect();

            by_them.sort_unstable();
            by_them.dedup();
            by_them.sort_by(|one, other| {
                album_order(
                    CollectionOrder::Year,
                    &index.albums[albums[*one].group],
                    &index.albums[albums[*other].group],
                )
            });
            artist_at.insert(id, artists.len());
            artists.push(Artist {
                id,
                group,
                albums: by_them,
            });
        }

        let theirs: HashSet<usize> = artists
            .iter()
            .flat_map(|artist| index.artists[artist.group].rows.iter().copied())
            .collect();
        let songs_by_an_artist = songs
            .iter()
            .copied()
            .filter(|at| theirs.contains(at))
            .collect();
        let songs_on_an_album = songs
            .iter()
            .copied()
            .filter(|at| {
                rows[*at]
                    .album_group
                    .is_some_and(|group| album_of_group.contains_key(&group))
            })
            .collect();
        let held_by_an_artist: HashSet<usize> = artists
            .iter()
            .flat_map(|artist| artist.albums.iter().copied())
            .collect();
        let albums_by_an_artist = (0..albums.len())
            .filter(|at| held_by_an_artist.contains(at))
            .collect();

        Self {
            index,
            songs,
            songs_by_an_artist,
            songs_on_an_album,
            song_at,
            albums,
            albums_by_an_artist,
            album_at,
            album_of_group,
            artists,
            artist_at,
            sizes,
            lists: Mutex::new(HashMap::new()),
            counts: Mutex::new(None),
        }
    }

    fn songs_by(&self, at: usize) -> Vec<usize> {
        let artist = &self.artists[at];
        let theirs: HashSet<usize> = self.index.artists[artist.group]
            .rows
            .iter()
            .copied()
            .collect();

        let mut songs: Vec<usize> = artist
            .albums
            .iter()
            .flat_map(|album| self.albums[*album].songs.iter().copied())
            .filter(|row| theirs.contains(row))
            .collect();

        let on_their_albums: HashSet<usize> = songs.iter().copied().collect();

        songs.extend(
            self.songs
                .iter()
                .copied()
                .filter(|row| theirs.contains(row) && !on_their_albums.contains(row)),
        );
        songs
    }

    fn album_held(&self, at: usize) -> Held {
        let album = &self.albums[at];
        let group = &self.index.albums[album.group];
        Held {
            shelf: Shelf::Album(album.id),
            title: group.title.clone(),
            artist: written(&group.artist),
            children: counted(album.songs.len()),
            cover: group.has_a_picture(&self.index.rows).then_some(album.id),
        }
    }

    fn artist_held(&self, at: usize) -> Held {
        let artist = &self.artists[at];
        Held {
            shelf: Shelf::Artist(artist.id),
            title: self.index.artists[artist.group].name.clone(),
            artist: None,
            children: counted(artist.albums.len()),
            cover: None,
        }
    }

    fn song_of(&self, at: usize) -> Option<Song> {
        let row = &self.index.rows[at];
        let track = &row.track;
        let album = row
            .album_group
            .and_then(|group| self.album_of_group.get(&group))
            .map(|at| &self.albums[*at]);
        let cover = album
            .filter(|album| self.index.albums[album.group].has_a_picture(&self.index.rows))
            .map(|album| album.id);

        Some(Song {
            id: TrackId::new(track.id).ok()?,
            title: track.title.clone(),
            artist: written(&track.artist),
            album: written(&track.album),
            cover,
            genre: written(&track.genre),
            track_number: track.track_number,
            duration: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
            kind: kind_of(track)?,
            content: Content::Untouched,
            size: self.sizes.get(&track.id).copied(),
            rate: track.rate,
            channels: u16::try_from(track.channels).unwrap_or(u16::MAX),
            bits: track
                .bits
                .filter(|_| row.codec.is_lossless())
                .and_then(|bits| u16::try_from(bits).ok()),
        })
    }

    fn songs_of(&self, rows: &[usize]) -> Vec<Entry> {
        rows.iter()
            .filter_map(|at| self.song_of(*at))
            .map(Entry::Song)
            .collect()
    }

    fn songs_paged(&self, rows: &[usize], page: Page) -> Shelved {
        Shelved {
            held: self.songs_of(paged(rows, page)),
            total: counted(rows.len()),
        }
    }

    fn song_values(&self, at: usize, field: Field) -> [&str; 2] {
        let row = &self.index.rows[at];
        match field {
            Field::Title => [&row.filed_title, ""],
            Field::Artist => [&row.filed_artist, &row.filed_album_artist],
            Field::Album => [&row.filed_album, ""],
            Field::Genre => [&row.filed_genre, ""],
        }
    }

    fn album_values(&self, at: usize, field: Field) -> [&str; 2] {
        let group = &self.index.albums[self.albums[at].group];
        match field {
            Field::Title => [&group.filed_title, ""],
            Field::Artist => [&group.filed_artist, ""],
            Field::Album | Field::Genre => NO_VALUES,
        }
    }

    fn artist_values(&self, at: usize, field: Field) -> [&str; 2] {
        match field {
            Field::Title => [&self.index.artists[self.artists[at].group].filed, ""],
            Field::Artist | Field::Album | Field::Genre => NO_VALUES,
        }
    }
}

impl resonate_upnp::Catalog for Shelves {
    type Failure = Error;

    fn searched(&self, kind: Kind, within: Within, query: &Query, page: Page) -> Result<Searched> {
        let Some(wanted) = Wanted::of(query) else {
            return Ok(Searched::Unanswered);
        };
        let _turn = self.readers.turn()?;
        let view = self.view()?;

        let shelved = match (kind, within) {
            (Kind::Song, Within::Playlist(id)) if matches!(query, Query::Everything) => {
                self.playlist_songs(&view, id, page)?.unwrap_or(Shelved {
                    held: Vec::new(),
                    total: 0,
                })
            }
            (Kind::Song, _) => {
                let found: Vec<usize> = self
                    .songs_within(&view, within)?
                    .iter()
                    .copied()
                    .filter(|at| wanted.holds(&|field| view.song_values(*at, field)))
                    .collect();
                view.songs_paged(&found, page)
            }
            (
                Kind::Album,
                Within::Catalog | Within::Artists | Within::Albums | Within::Artist(_),
            ) => {
                let albums: Vec<usize> = match within {
                    Within::Artist(id) => view
                        .artist_at
                        .get(&id)
                        .map(|at| view.artists[*at].albums.clone())
                        .unwrap_or_default(),
                    Within::Artists => view.albums_by_an_artist.clone(),
                    Within::Catalog | Within::Albums | Within::Album(_) | Within::Playlist(_) => {
                        (0..view.albums.len()).collect()
                    }
                };
                let found: Vec<usize> = albums
                    .into_iter()
                    .filter(|at| wanted.holds(&|field| view.album_values(*at, field)))
                    .collect();
                Shelved {
                    held: paged(&found, page)
                        .iter()
                        .map(|at| Entry::Shelf(view.album_held(*at)))
                        .collect(),
                    total: counted(found.len()),
                }
            }
            (Kind::Artist, Within::Catalog | Within::Artists) => {
                let found: Vec<usize> = (0..view.artists.len())
                    .filter(|at| wanted.holds(&|field| view.artist_values(*at, field)))
                    .collect();
                Shelved {
                    held: paged(&found, page)
                        .iter()
                        .map(|at| Entry::Shelf(view.artist_held(*at)))
                        .collect(),
                    total: counted(found.len()),
                }
            }
            (Kind::Playlist, Within::Catalog) => {
                let found = self
                    .playlists()?
                    .into_iter()
                    .filter(|playlist| {
                        let filed = folded_letters(playlist.name.trim());
                        wanted.holds(&|field| match field {
                            Field::Title => [filed.as_str(), ""],
                            Field::Artist | Field::Album | Field::Genre => NO_VALUES,
                        })
                    })
                    .collect();
                self.playlists_held(&view, found, page)?
            }
            (Kind::Album | Kind::Artist | Kind::Playlist, _) => Shelved {
                held: Vec::new(),
                total: 0,
            },
        };
        Ok(Searched::Found(shelved))
    }

    fn shelved(&self, shelf: Shelf, page: Page) -> Result<Option<Shelved>> {
        let _turn = self.readers.turn()?;
        let view = self.view()?;

        Ok(Some(match shelf {
            Shelf::Artists => Shelved {
                held: paged(&(0..view.artists.len()).collect::<Vec<_>>(), page)
                    .iter()
                    .map(|at| Entry::Shelf(view.artist_held(*at)))
                    .collect(),
                total: counted(view.artists.len()),
            },
            Shelf::Artist(id) => {
                let Some(at) = view.artist_at.get(&id) else {
                    return Ok(None);
                };
                let albums = &view.artists[*at].albums;
                Shelved {
                    held: paged(albums, page)
                        .iter()
                        .map(|at| Entry::Shelf(view.album_held(*at)))
                        .collect(),
                    total: counted(albums.len()),
                }
            }
            Shelf::Albums => Shelved {
                held: paged(&(0..view.albums.len()).collect::<Vec<_>>(), page)
                    .iter()
                    .map(|at| Entry::Shelf(view.album_held(*at)))
                    .collect(),
                total: counted(view.albums.len()),
            },
            Shelf::Album(id) => {
                let Some(at) = view.album_at.get(&id) else {
                    return Ok(None);
                };
                view.songs_paged(&view.albums[*at].songs, page)
            }
            Shelf::Songs => view.songs_paged(&view.songs, page),
            Shelf::Playlists => {
                let (names, total) = self.playlists_paged(page)?;
                Shelved {
                    held: names
                        .into_iter()
                        .map(|playlist| self.playlist_held(&view, playlist).map(Entry::Shelf))
                        .collect::<Result<_>>()?,
                    total,
                }
            }
            Shelf::Playlist(id) => return self.playlist_songs(&view, id, page),
        }))
    }

    fn shelf(&self, shelf: Shelf) -> Result<Option<Held>> {
        let _turn = self.readers.turn()?;
        let view = self.view()?;

        match shelf {
            Shelf::Artist(id) => Ok(view.artist_at.get(&id).map(|at| view.artist_held(*at))),
            Shelf::Album(id) => Ok(view.album_at.get(&id).map(|at| view.album_held(*at))),
            Shelf::Playlist(id) => self
                .playlist(id)?
                .map(|playlist| self.playlist_held(&view, playlist))
                .transpose(),
            Shelf::Artists | Shelf::Albums | Shelf::Songs | Shelf::Playlists => Ok(None),
        }
    }

    fn song(&self, id: TrackId) -> Result<Option<Song>> {
        let _turn = self.readers.turn()?;
        let view = self.view()?;

        Ok(view.song_at.get(&id.get()).and_then(|at| view.song_of(*at)))
    }

    fn bytes_of(&self, id: TrackId) -> Result<Option<Opened>> {
        let _turn = self.readers.turn()?;
        let view = self.view()?;
        let Some(at) = view.song_at.get(&id.get()) else {
            return Ok(None);
        };

        let track = &view.index.rows[*at].track;
        let Some(kind) = kind_of(track) else {
            return Ok(None);
        };

        match self.sources.open(&location(&track.uri)?) {
            Ok(media) => Ok(Some(Opened {
                bytes: Box::new(media.stream),
                kind,
                content: Content::Untouched,
            })),
            Err(error) => {
                tracing::debug!(%error, "a song asked for on the network could not be opened");
                Ok(None)
            }
        }
    }

    fn cover(&self, album: AlbumId) -> Result<Option<Picture>> {
        let _turn = self.readers.turn()?;
        let view = self.view()?;
        let Some(at) = view.album_at.get(&album) else {
            return Ok(None);
        };

        let group = &view.index.albums[view.albums[*at].group];

        if !group.has_a_picture(&view.index.rows) {
            return Ok(None);
        }

        Ok(self
            .catalog
            .pictured(group.artwork_track)?
            .and_then(|bytes| {
                ImageFormat::sniff(&bytes).map(|format| Picture {
                    mime: format.media_type(),
                    bytes,
                })
            }))
    }

    fn revision(&self) -> u32 {
        let revision = self.catalog.revision();
        u32::try_from(revision % (u64::from(u32::MAX) + 1)).unwrap_or_default()
    }
}

impl Wanted {
    fn of(query: &Query) -> Option<Self> {
        Some(match query {
            Query::Everything => Self::All(Vec::new()),
            Query::All(all) => Self::All(all.iter().map(Self::of).collect::<Option<_>>()?),
            Query::Any(any) => Self::Any(any.iter().map(Self::of).collect::<Option<_>>()?),
            Query::Present { field, held } => Self::Present {
                field: *field,
                held: *held,
            },
            Query::Text { field, test, text } => match test {
                Test::Is | Test::IsNot => {
                    if !text.chars().any(is_lettered) {
                        return None;
                    }
                    Self::Named {
                        field: *field,
                        whole: folded_letters(text.trim()),
                        denied: *test == Test::IsNot,
                    }
                }
                Test::Contains | Test::Lacks => {
                    let words = pieces_of(&folded_letters(text))
                        .map(str::to_owned)
                        .collect::<Vec<_>>();

                    if words.is_empty()
                        || !words.iter().all(|word| begins_with_enough_letters(word))
                    {
                        return None;
                    }
                    Self::Begun {
                        field: *field,
                        words,
                        denied: *test == Test::Lacks,
                    }
                }
            },
        })
    }

    fn holds<'a>(&self, values: &dyn Fn(Field) -> [&'a str; 2]) -> bool {
        match self {
            Self::All(all) => all.iter().all(|one| one.holds(values)),
            Self::Any(any) => any.iter().any(|one| one.holds(values)),
            Self::Present { field, held } => {
                values(*field).iter().any(|value| !value.is_empty()) == *held
            }
            Self::Named {
                field,
                whole,
                denied,
            } => {
                let named = values(*field)
                    .iter()
                    .any(|value| !value.is_empty() && value == whole);
                named != *denied
            }
            Self::Begun {
                field,
                words,
                denied,
            } => {
                let held = values(*field);
                let begun = words.iter().all(|word| {
                    held.iter()
                        .any(|value| pieces_of(value).any(|piece| piece.starts_with(word.as_str())))
                });
                begun != *denied
            }
        }
    }
}

fn begins_with_enough_letters(word: &str) -> bool {
    word.chars().filter(|letter| is_lettered(*letter)).count() >= LETTERS_BEGUN_AT_LEAST
}

fn pieces_of(text: &str) -> impl Iterator<Item = &str> {
    text.split(|letter: char| !is_lettered(letter))
        .filter(|piece| !piece.is_empty())
}

fn in_title_order(one: &Row, other: &Row) -> Ordering {
    (&one.filed_title, &one.filed_artist, one.track.id).cmp(&(
        &other.filed_title,
        &other.filed_artist,
        other.track.id,
    ))
}

fn kind_of(track: &Track) -> Option<AudioKind> {
    served_as(&track.uri, &track.name)
}

pub(crate) fn served_as(uri: &str, name: &str) -> Option<AudioKind> {
    let at = location(uri).ok()?;
    name.rsplit_once('.')
        .and_then(|(_, extension)| AudioKind::of_extension(extension))
        .or_else(|| at.extension().and_then(AudioKind::of_extension))
}

fn named(key: &str) -> NonZeroU64 {
    let hashed = key.bytes().fold(NAMED_FROM, |hashed, byte| {
        (hashed ^ u64::from(byte)).wrapping_mul(NAMED_BY)
    });
    NonZeroU64::new(hashed).unwrap_or(NonZeroU64::MIN)
}

fn written(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn paged(rows: &[usize], page: Page) -> &[usize] {
    let from = (page.from as usize).min(rows.len());
    let until = from.saturating_add(page.most as usize).min(rows.len());
    &rows[from..until]
}

fn counted(held: usize) -> u32 {
    u32::try_from(held).unwrap_or(u32::MAX)
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn kept_udn(folder: &Path) -> Udn {
    let kept_in = folder.join(UDN_KEPT_IN);

    if let Some(kept) = fs::read_to_string(&kept_in)
        .ok()
        .and_then(|text| Udn::read(&text))
    {
        return kept;
    }

    let minted = Udn::minted();

    if let Err(error) = fs::write(&kept_in, minted.as_str()) {
        tracing::warn!(%error, "the media server's name on the network could not be kept, so it changes next run");
    }
    minted
}

pub(crate) struct Offered {
    kept_in: PathBuf,
    offering: Offering,
    name: Mutex<Option<ServerName>>,
    running: Mutex<Option<MediaServer<Shelves>>>,
}

impl Offered {
    pub(crate) fn under(folder: PathBuf) -> Self {
        Self::on(folder, Offering::on_the_network())
    }

    pub(crate) const fn on(kept_in: PathBuf, offering: Offering) -> Self {
        Self {
            kept_in,
            offering,
            name: Mutex::new(None),
            running: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn untold() -> Self {
        let loopback = std::net::SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), 0);
        Self::on(
            std::env::temp_dir(),
            Offering {
                heard_on: loopback,
                told: loopback,
                every: Duration::from_secs(3_600),
                serving_on: loopback.ip(),
                name: None,
            },
        )
    }

    fn share(&self, on: bool, shelves: impl FnOnce() -> Shelves) -> Result<()> {
        if !on {
            self.leave();
            return Ok(());
        }

        let mut running = self.running.lock();

        if running.is_some() {
            return Ok(());
        }
        *running = Some(self.started(shelves)?);
        Ok(())
    }

    fn started(&self, shelves: impl FnOnce() -> Shelves) -> Result<MediaServer<Shelves>> {
        let name = self.name.lock().clone();
        let server = MediaServer::start(
            Arc::new(shelves()),
            kept_udn(&self.kept_in),
            self.offering.clone().named(name),
        )
        .map_err(|source| Error::CatalogNotOffered {
            source: Box::new(source),
        })?;

        tracing::info!(
            port = server.port(),
            "the catalog is offered on the network"
        );
        Ok(server)
    }

    fn rename(&self, typed: &str, shelves: impl FnOnce() -> Shelves) -> Result<String> {
        let name = ServerName::new(typed);
        let shown = name
            .as_ref()
            .map_or_else(String::new, |name| name.as_str().to_owned());

        {
            let mut kept = self.name.lock();

            if *kept == name {
                return Ok(shown);
            }
            *kept = name;
        }

        let mut running = self.running.lock();

        let Some(stopping) = running.take() else {
            return Ok(shown);
        };

        drop(stopping);
        *running = Some(self.started(shelves)?);
        Ok(shown)
    }

    fn shared(&self) -> bool {
        self.running.lock().is_some()
    }

    pub(crate) fn leave(&self) {
        let stopping = self.running.lock().take();

        drop(stopping);
    }

    #[cfg(test)]
    fn port(&self) -> Option<u16> {
        self.running.lock().as_ref().map(MediaServer::port)
    }
}

impl MobilePlayer {
    fn shelves_offered(&self) -> Shelves {
        Shelves::over(
            Arc::clone(&self.catalog),
            Arc::clone(&self.services.searching),
            Arc::clone(&self.sources),
        )
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn share_catalog_on_network(&self, on: bool) -> Result<()> {
        self.offered.share(on, || self.shelves_offered())
    }

    pub fn catalog_shared_on_network(&self) -> bool {
        self.offered.shared()
    }

    pub fn name_catalog_on_network(&self, name: String) -> Result<String> {
        self.offered.rename(&name, || self.shelves_offered())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
        thread,
        time::Instant,
    };

    use resonate_upnp::Catalog as _;

    use super::*;
    use crate::{
        SavedSearch,
        harness::{Harness, SETTLED_WITHIN, SONGS},
        saved_searches::SearchOrder,
    };

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    const DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    struct Filed {
        name: &'static str,
        title: &'static str,
        artist: &'static str,
        album: &'static str,
        track: u32,
        genre: &'static str,
        codec: &'static str,
    }

    const FILED: [Filed; 5] = [
        Filed {
            name: "blue-train.flac",
            title: "Blue Train",
            artist: "John Coltrane",
            album: "Blue Train",
            track: 1,
            genre: "Jazz",
            codec: "FLAC",
        },
        Filed {
            name: "moments-notice.flac",
            title: "Moment's Notice",
            artist: "John Coltrane",
            album: "Blue Train",
            track: 2,
            genre: "Jazz",
            codec: "FLAC",
        },
        Filed {
            name: "so-what.mp3",
            title: "So What",
            artist: "Miles Davis",
            album: "Kind of Blue",
            track: 1,
            genre: "",
            codec: "MP3",
        },
        Filed {
            name: "freddie-freeloader.mp3",
            title: "Freddie Freeloader",
            artist: "Miles Davis",
            album: "Kind of Blue",
            track: 2,
            genre: "",
            codec: "MP3",
        },
        Filed {
            name: "liner-notes.txt",
            title: "Liner Notes",
            artist: "Miles Davis",
            album: "Kind of Blue",
            track: 3,
            genre: "",
            codec: "",
        },
    ];

    fn written_for(name: &str) -> Vec<u8> {
        name.bytes().cycle().take(4_096).collect()
    }

    fn file(harness: &Harness, song: &Filed) -> u64 {
        let written = written_for(song.name);

        fs::write(harness.folder.join(song.name), &written).expect("a song");

        let connection = harness.player.catalog.0.lock();

        connection
            .execute(
                "INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec, genre, track_number, channels, bits, size)
                 VALUES (?1, ?2, ?3, ?4, ?5, 300000, 44100, ?6, ?7, ?8, 2, 16, ?9)",
                params![
                    format!("{}{}", crate::harness::FOLDER, song.name),
                    song.name,
                    song.title,
                    song.artist,
                    song.album,
                    song.codec,
                    song.genre,
                    song.track,
                    written.len() as i64,
                ],
            )
            .expect("a song filed");
        connection.last_insert_rowid() as u64
    }

    fn filed(harness: &Harness) -> HashMap<&'static str, u64> {
        FILED
            .iter()
            .map(|song| (song.title, file(harness, song)))
            .collect()
    }

    fn shelves(harness: &Harness) -> Shelves {
        Shelves::over(
            Arc::clone(&harness.player.catalog),
            Arc::clone(&harness.player.services.searching),
            Arc::clone(&harness.player.sources),
        )
    }

    fn titled(shelved: &Shelved) -> Vec<String> {
        shelved
            .held
            .iter()
            .map(|entry| match entry {
                Entry::Shelf(held) => format!("{} ({})", held.title, held.children),
                Entry::Song(song) => song.title.clone(),
            })
            .collect()
    }

    fn shelved(shelves: &Shelves, shelf: Shelf, from: u32, most: u32) -> Shelved {
        shelves
            .shelved(shelf, Page { from, most })
            .expect("read")
            .expect("a shelf held")
    }

    fn first_shelf(shelved: &Shelved) -> Shelf {
        match shelved.held.first() {
            Some(Entry::Shelf(held)) => held.shelf,
            _ => panic!("no shelf first"),
        }
    }

    fn text(field: Field, test: Test, text: &str) -> Query {
        Query::Text {
            field,
            test,
            text: text.to_owned(),
        }
    }

    fn found(shelves: &Shelves, kind: Kind, query: &Query, from: u32, most: u32) -> Shelved {
        found_within(shelves, kind, Within::Catalog, query, from, most)
    }

    fn found_within(
        shelves: &Shelves,
        kind: Kind,
        within: Within,
        query: &Query,
        from: u32,
        most: u32,
    ) -> Shelved {
        match shelves
            .searched(kind, within, query, Page { from, most })
            .expect("searched")
        {
            Searched::Found(shelved) => shelved,
            Searched::Unanswered => panic!("{query:?} was not answered"),
        }
    }

    #[test]
    fn the_phones_catalog_is_browsed_as_artists_albums_songs_and_playlists_paged_and_whole_files_only()
     {
        let harness = Harness::new();
        let ids = filed(&harness);

        let evening = harness
            .player
            .make_playlist(
                "Evening".to_owned(),
                vec![ids["So What"], ids["Liner Notes"], ids["Blue Train"]],
            )
            .expect("a playlist");
        let shelves = shelves(&harness);

        let artists = shelved(&shelves, Shelf::Artists, 0, 10);
        let coltrane = shelved(&shelves, first_shelf(&artists), 0, 10);
        let albums = shelved(&shelves, Shelf::Albums, 0, 10);
        let kind_of_blue = shelved(
            &shelves,
            Shelf::Album(match &albums.held[1] {
                Entry::Shelf(held) => match held.shelf {
                    Shelf::Album(id) => id,
                    _ => panic!("not an album"),
                },
                Entry::Song(_) => panic!("not an album"),
            }),
            0,
            10,
        );
        let first_songs = shelved(&shelves, Shelf::Songs, 0, 3);
        let next_songs = shelved(&shelves, Shelf::Songs, 3, 3);
        let counted = shelved(&shelves, Shelf::Songs, 0, 0);
        let playlists = shelved(&shelves, Shelf::Playlists, 0, 10);
        let listed = shelved(
            &shelves,
            Shelf::Playlist(PlaylistId::new(evening).expect("an id")),
            0,
            10,
        );

        assert_eq!(titled(&artists), ["John Coltrane (1)", "Miles Davis (1)"]);
        assert_eq!(artists.total, 2);
        assert_eq!(titled(&coltrane), ["Blue Train (2)"]);
        assert_eq!(titled(&albums), ["Blue Train (2)", "Kind of Blue (2)"]);
        assert_eq!(titled(&kind_of_blue), ["So What", "Freddie Freeloader"]);
        assert_eq!(titled(&first_songs), ["a", "b", "Blue Train"]);
        assert_eq!(titled(&next_songs), ["c", "d", "Freddie Freeloader"]);
        assert_eq!(first_songs.total, 8);
        assert!(counted.held.is_empty());
        assert_eq!(counted.total, 8);
        assert_eq!(titled(&playlists), ["Evening (2)"]);
        assert_eq!(titled(&listed), ["So What", "Blue Train"]);
        assert_eq!(
            shelves
                .shelf(first_shelf(&artists))
                .expect("read")
                .map(|held| held.title),
            Some("John Coltrane".to_owned())
        );
        assert!(
            shelves
                .song(TrackId::new(ids["Liner Notes"]).expect("an id"))
                .expect("read")
                .is_none()
        );
        for unheld in [
            Shelf::Artist(ArtistId::new(1).expect("an id")),
            Shelf::Album(AlbumId::new(1).expect("an id")),
            Shelf::Playlist(PlaylistId::new(evening + 1).expect("an id")),
        ] {
            assert!(
                shelves
                    .shelved(unheld, Page::first(10))
                    .expect("read")
                    .is_none()
            );
            assert!(shelves.shelf(unheld).expect("read").is_none());
        }
    }

    #[test]
    fn a_song_is_described_by_its_row_and_served_through_the_documents_seam_and_a_cover_by_its_album()
     {
        let harness = Harness::new();
        let ids = filed(&harness);

        harness
            .player
            .catalog
            .keep_artwork_of(&[ids["Blue Train"]], PNG, "folder", false)
            .expect("a cover kept");
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET size = NULL WHERE id = ?1",
                [ids["Moment's Notice"] as i64],
            )
            .expect("a size forgotten");

        let shelves = shelves(&harness);
        let blue_train = TrackId::new(ids["Blue Train"]).expect("an id");

        let song = shelves.song(blue_train).expect("read").expect("a song");
        let unsized_song = shelves
            .song(TrackId::new(ids["Moment's Notice"]).expect("an id"))
            .expect("read")
            .expect("a song");
        let mut opened = shelves.bytes_of(blue_train).expect("read").expect("bytes");
        let mut served = Vec::new();

        opened.bytes.read_to_end(&mut served).expect("the bytes");

        let cover = shelves
            .cover(song.cover.expect("a cover named"))
            .expect("read")
            .expect("a cover");

        fs::remove_file(harness.folder.join("so-what.mp3")).expect("removed");

        let gone = shelves
            .bytes_of(TrackId::new(ids["So What"]).expect("an id"))
            .expect("read");

        assert_eq!(song.title, "Blue Train");
        assert_eq!(song.artist.as_deref(), Some("John Coltrane"));
        assert_eq!(song.album.as_deref(), Some("Blue Train"));
        assert_eq!(song.genre.as_deref(), Some("Jazz"));
        assert_eq!(song.track_number, Some(1));
        assert_eq!(song.kind, AudioKind::Flac);
        assert_eq!(song.size, Some(4_096));
        assert_eq!(unsized_song.size, None);
        assert_eq!(song.duration, Some(Duration::from_secs(300)));
        assert_eq!((song.rate, song.channels, song.bits), (44_100, 2, Some(16)));
        assert_eq!(opened.kind, AudioKind::Flac);
        assert_eq!(served, written_for("blue-train.flac"));
        assert_eq!(cover.bytes, PNG);
        assert_eq!(cover.mime, "image/png");
        assert!(gone.is_none());
    }

    #[test]
    fn a_search_from_the_network_is_answered_over_the_folded_catalog_and_an_unreadable_one_is_not()
    {
        let harness = Harness::new();

        filed(&harness);
        harness
            .player
            .save_search(
                "Coltrane".to_owned(),
                SavedSearch {
                    text: "artist:coltrane".to_owned(),
                    order: SearchOrder::Title,
                    descending: false,
                    rows: None,
                },
            )
            .expect("a saved search");

        let shelves = shelves(&harness);

        let by_coltrane = found(
            &shelves,
            Kind::Song,
            &text(Field::Artist, Test::Contains, "colt"),
            0,
            10,
        );
        let so_what = found(
            &shelves,
            Kind::Song,
            &text(Field::Title, Test::Is, "SO WHAT"),
            0,
            10,
        );
        let paged = found(
            &shelves,
            Kind::Song,
            &Query::All(vec![
                text(Field::Album, Test::Contains, "blue"),
                text(Field::Title, Test::IsNot, "blue train"),
            ]),
            1,
            1,
        );
        let jazz = found(
            &shelves,
            Kind::Song,
            &Query::Present {
                field: Field::Genre,
                held: true,
            },
            0,
            10,
        );
        let albums = found(
            &shelves,
            Kind::Album,
            &text(Field::Artist, Test::Is, "miles davis"),
            0,
            10,
        );
        let artists = found(
            &shelves,
            Kind::Artist,
            &Query::Any(vec![
                text(Field::Title, Test::Contains, "mil"),
                text(Field::Album, Test::Contains, "blue"),
            ]),
            0,
            10,
        );
        let playlists = found(
            &shelves,
            Kind::Playlist,
            &text(Field::Title, Test::Lacks, "evening"),
            0,
            10,
        );
        let unreadable = shelves
            .searched(
                Kind::Song,
                Within::Catalog,
                &text(Field::Title, Test::Contains, "!!"),
                Page::first(10),
            )
            .expect("searched");

        assert_eq!(titled(&by_coltrane), ["Blue Train", "Moment's Notice"]);
        assert_eq!(titled(&so_what), ["So What"]);
        assert_eq!(titled(&paged), ["Moment's Notice"]);
        assert_eq!(paged.total, 3);
        assert_eq!(titled(&jazz), ["Blue Train", "Moment's Notice"]);
        assert_eq!(titled(&albums), ["Kind of Blue (2)"]);
        assert_eq!(titled(&artists), ["Miles Davis (1)"]);
        assert_eq!(titled(&playlists), ["Coltrane (2)"]);
        assert_eq!(unreadable, Searched::Unanswered);
    }

    #[test]
    fn a_search_under_an_artist_an_album_or_a_playlist_answers_what_each_holds_in_its_order() {
        let harness = Harness::new();
        let ids = filed(&harness);

        let evening = PlaylistId::new(
            harness
                .player
                .make_playlist(
                    "Evening".to_owned(),
                    vec![ids["So What"], ids["Liner Notes"], ids["Blue Train"]],
                )
                .expect("a playlist"),
        )
        .expect("an id");
        let shelves = shelves(&harness);

        let artists = shelved(&shelves, Shelf::Artists, 0, 10);
        let Shelf::Artist(coltrane) = first_shelf(&artists) else {
            panic!("not an artist");
        };
        let Shelf::Artist(miles) = first_shelf(&shelved(&shelves, Shelf::Artists, 1, 1)) else {
            panic!("not an artist");
        };
        let Shelf::Album(kind_of_blue) = first_shelf(&shelved(&shelves, Shelf::Albums, 1, 1))
        else {
            panic!("not an album");
        };
        let everything = Query::Everything;

        let his_albums = found_within(
            &shelves,
            Kind::Album,
            Within::Artist(miles),
            &everything,
            0,
            10,
        );
        let his_songs = found_within(
            &shelves,
            Kind::Song,
            Within::Artist(miles),
            &everything,
            0,
            10,
        );
        let his_second = found_within(
            &shelves,
            Kind::Song,
            Within::Artist(miles),
            &everything,
            1,
            1,
        );
        let his_freddie = found_within(
            &shelves,
            Kind::Song,
            Within::Artist(miles),
            &text(Field::Title, Test::Contains, "fred"),
            0,
            10,
        );
        let not_coltranes = found_within(
            &shelves,
            Kind::Song,
            Within::Artist(coltrane),
            &text(Field::Title, Test::Contains, "what"),
            0,
            10,
        );
        let on_the_album = found_within(
            &shelves,
            Kind::Song,
            Within::Album(kind_of_blue),
            &everything,
            0,
            10,
        );
        let what_on_the_album = found_within(
            &shelves,
            Kind::Song,
            Within::Album(kind_of_blue),
            &text(Field::Title, Test::Contains, "what"),
            0,
            10,
        );
        let in_the_evening = found_within(
            &shelves,
            Kind::Song,
            Within::Playlist(evening),
            &everything,
            0,
            10,
        );
        let coltrane_in_the_evening = found_within(
            &shelves,
            Kind::Song,
            Within::Playlist(evening),
            &text(Field::Artist, Test::Contains, "colt"),
            0,
            10,
        );
        let no_artist_on_the_album = found_within(
            &shelves,
            Kind::Artist,
            Within::Album(kind_of_blue),
            &everything,
            0,
            10,
        );
        let no_album_in_the_evening = found_within(
            &shelves,
            Kind::Album,
            Within::Playlist(evening),
            &everything,
            0,
            10,
        );
        let unheld = found_within(
            &shelves,
            Kind::Song,
            Within::Artist(ArtistId::new(1).expect("an id")),
            &everything,
            0,
            10,
        );

        assert_eq!(titled(&his_albums), ["Kind of Blue (2)"]);
        assert_eq!(titled(&his_songs), ["So What", "Freddie Freeloader"]);
        assert_eq!(his_songs.total, 2);
        assert_eq!(titled(&his_second), ["Freddie Freeloader"]);
        assert_eq!(his_second.total, 2);
        assert_eq!(titled(&his_freddie), ["Freddie Freeloader"]);
        assert_eq!(his_freddie.total, 1);
        assert_eq!(
            (titled(&not_coltranes), not_coltranes.total),
            (Vec::<String>::new(), 0)
        );
        assert_eq!(titled(&on_the_album), ["So What", "Freddie Freeloader"]);
        assert_eq!(titled(&what_on_the_album), ["So What"]);
        assert_eq!(titled(&in_the_evening), ["So What", "Blue Train"]);
        assert_eq!(in_the_evening.total, 2);
        assert_eq!(titled(&coltrane_in_the_evening), ["Blue Train"]);
        assert_eq!(coltrane_in_the_evening.total, 1);
        for nothing in [no_artist_on_the_album, no_album_in_the_evening, unheld] {
            assert!(nothing.held.is_empty());
            assert_eq!(nothing.total, 0);
        }
    }

    #[test]
    fn a_search_under_artists_or_albums_finds_what_is_nested_below_and_nothing_loose() {
        let harness = Harness::new();

        filed(&harness);
        for loose in [
            Filed {
                name: "demo.flac",
                title: "Demo",
                artist: "Miles Davis",
                album: "",
                track: 1,
                genre: "",
                codec: "FLAC",
            },
            Filed {
                name: "stray.flac",
                title: "Stray",
                artist: "",
                album: "Loose Ends",
                track: 1,
                genre: "",
                codec: "FLAC",
            },
        ] {
            file(&harness, &loose);
        }

        let shelves = shelves(&harness);
        let everything = Query::Everything;
        let blue = text(Field::Title, Test::Contains, "blue");

        let artists = found_within(&shelves, Kind::Artist, Within::Artists, &everything, 0, 10);
        let albums_under_the_artists =
            found_within(&shelves, Kind::Album, Within::Artists, &everything, 0, 10);
        let songs_under_the_artists =
            found_within(&shelves, Kind::Song, Within::Artists, &everything, 0, 10);
        let second_song_under_the_artists =
            found_within(&shelves, Kind::Song, Within::Artists, &everything, 1, 1);
        let albums = found_within(&shelves, Kind::Album, Within::Albums, &everything, 0, 10);
        let songs_under_the_albums =
            found_within(&shelves, Kind::Song, Within::Albums, &everything, 0, 10);
        let blue_under_the_albums =
            found_within(&shelves, Kind::Album, Within::Albums, &blue, 0, 10);
        let blue_songs_under_the_albums =
            found_within(&shelves, Kind::Song, Within::Albums, &blue, 0, 10);
        let no_playlist_under_the_artists = found_within(
            &shelves,
            Kind::Playlist,
            Within::Artists,
            &everything,
            0,
            10,
        );
        let no_artist_under_the_albums =
            found_within(&shelves, Kind::Artist, Within::Albums, &everything, 0, 10);
        let every_song = found(&shelves, Kind::Song, &everything, 0, 10);

        assert_eq!(titled(&artists), ["John Coltrane (1)", "Miles Davis (1)"]);
        assert_eq!(
            titled(&albums_under_the_artists),
            ["Blue Train (2)", "Kind of Blue (2)"]
        );
        assert_eq!(
            titled(&songs_under_the_artists),
            [
                "Blue Train",
                "Demo",
                "Freddie Freeloader",
                "Moment's Notice",
                "So What"
            ]
        );
        assert_eq!(songs_under_the_artists.total, 5);
        assert_eq!(titled(&second_song_under_the_artists), ["Demo"]);
        assert_eq!(second_song_under_the_artists.total, 5);
        assert_eq!(
            titled(&albums),
            ["Blue Train (2)", "Kind of Blue (2)", "Loose Ends (1)"]
        );
        assert_eq!(
            titled(&songs_under_the_albums),
            [
                "Blue Train",
                "Freddie Freeloader",
                "Moment's Notice",
                "So What",
                "Stray"
            ]
        );
        assert_eq!(songs_under_the_albums.total, 5);
        assert_eq!(
            titled(&blue_under_the_albums),
            ["Blue Train (2)", "Kind of Blue (2)"]
        );
        assert_eq!(titled(&blue_songs_under_the_albums), ["Blue Train"]);
        assert_eq!(every_song.total, 10);
        assert!(
            ["Demo", "Stray", "a"]
                .iter()
                .all(|title| titled(&every_song).contains(&(*title).to_owned()))
        );
        for nothing in [no_playlist_under_the_artists, no_artist_under_the_albums] {
            assert!(nothing.held.is_empty());
            assert_eq!(nothing.total, 0);
        }
    }

    fn fetched(port: u16, request: &str, body: &str) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(SocketAddr::new(LOOPBACK, port)).expect("connects");

        stream
            .set_read_timeout(Some(SETTLED_WITHIN))
            .expect("a read timeout");
        stream.write_all(request.as_bytes()).expect("asks");
        stream.write_all(body.as_bytes()).expect("sends the body");

        let mut answer = Vec::new();

        stream.read_to_end(&mut answer).expect("an answer");

        let ends = answer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("a head")
            + 4;
        (
            String::from_utf8_lossy(&answer[..ends]).into_owned(),
            answer[ends..].to_vec(),
        )
    }

    fn got(port: u16, path: &str) -> (String, Vec<u8>) {
        fetched(
            port,
            &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
            "",
        )
    }

    fn browsed(port: u16, object: &str) -> String {
        browsed_by(port, object, "BrowseDirectChildren")
    }

    fn browsed_by(port: u16, object: &str, flag: &str) -> String {
        let body = format!(
            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:Browse xmlns:u=\"{DIRECTORY}\"><ObjectID>{object}</ObjectID><BrowseFlag>{flag}</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"
        );
        let (head, answer) = fetched(
            port,
            &format!(
                "POST /control/ContentDirectory HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{DIRECTORY}#Browse\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
            &body,
        );

        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        String::from_utf8(answer).expect("text")
    }

    fn heard(harness: &Harness, said: &str) -> String {
        let began = Instant::now();
        let mut buffer = [0; 4_096];

        while began.elapsed() < SETTLED_WITHIN {
            let Ok(read) = harness.told.recv(&mut buffer) else {
                continue;
            };
            let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
            if text.contains(said) {
                return text;
            }
        }
        panic!("{said} was never told");
    }

    #[test]
    fn the_switch_offers_the_phones_catalog_under_one_kept_name_and_takes_it_off_the_network() {
        let harness = Harness::new();
        let ids = filed(&harness);
        let resting = harness.player.catalog_shared_on_network();

        harness
            .player
            .share_catalog_on_network(true)
            .expect("offered");

        let port = harness.player.offered.port().expect("a port");
        let alive = heard(&harness, "ssdp:alive");

        harness
            .player
            .share_catalog_on_network(true)
            .expect("still offered");

        let (described_head, described) = got(port, "/description.xml");
        let albums = browsed(port, "albums");
        let (song_head, song) = got(port, &format!("/resonate/song-{}.flac", ids["Blue Train"]));
        let kept = fs::read_to_string(harness.folder.join(UDN_KEPT_IN)).expect("a kept name");
        let shared = harness.player.catalog_shared_on_network();
        let same_port = harness.player.offered.port();

        harness
            .player
            .share_catalog_on_network(false)
            .expect("taken off");

        let byebye = heard(&harness, "ssdp:byebye");
        let stopped = harness.player.catalog_shared_on_network();

        harness
            .player
            .share_catalog_on_network(true)
            .expect("offered again");

        let (_, described_again) = got(
            harness.player.offered.port().expect("a port"),
            "/description.xml",
        );

        harness
            .player
            .share_catalog_on_network(false)
            .expect("taken off again");

        assert!(!resting);
        assert!(shared);
        assert_eq!(same_port, Some(port));
        assert!(alive.contains(&kept), "{alive}");
        assert!(byebye.contains(&kept), "{byebye}");
        assert!(!stopped);
        assert!(String::from_utf8_lossy(&described).contains(&kept));
        assert!(String::from_utf8_lossy(&described_again).contains(&kept));
        assert!(described_head.starts_with("HTTP/1.1 200"));
        assert!(albums.contains("Blue Train"), "{albums}");
        assert!(albums.contains("Kind of Blue"), "{albums}");
        assert!(song_head.starts_with("HTTP/1.1 200"), "{song_head}");
        assert_eq!(song, written_for("blue-train.flac"));
        assert!(!harness.player.catalog_shared_on_network());
    }

    fn named_on_the_network(port: u16) -> (String, String) {
        let (_, described) = got(port, "/description.xml");
        let described = String::from_utf8(described).expect("text");
        let root = browsed_by(port, "0", "BrowseMetadata");
        let between = |text: &str, opens: &str, closes: &str| {
            text.split(opens)
                .nth(1)
                .and_then(|rest| rest.split(closes).next())
                .expect("a name")
                .to_owned()
        };

        (
            between(&described, "<friendlyName>", "</friendlyName>"),
            between(&root, "&lt;dc:title&gt;", "&lt;/dc:title&gt;"),
        )
    }

    #[test]
    fn a_name_typed_on_the_phone_names_the_catalog_and_its_root_on_the_network_at_once() {
        let harness = Harness::new();

        filed(&harness);

        let kept_before = harness
            .player
            .name_catalog_on_network("  Kitchen \n shelf ".to_owned())
            .expect("named");

        harness
            .player
            .share_catalog_on_network(true)
            .expect("offered");

        let first_port = harness.player.offered.port().expect("a port");
        let first = named_on_the_network(first_port);
        let kept_again = harness
            .player
            .name_catalog_on_network("Kitchen shelf".to_owned())
            .expect("named alike");
        let same_port = harness.player.offered.port();

        let renamed = harness
            .player
            .name_catalog_on_network("Study".to_owned())
            .expect("renamed");
        let byebye = heard(&harness, "ssdp:byebye");
        let study = named_on_the_network(harness.player.offered.port().expect("a port"));
        let still_shared = harness.player.catalog_shared_on_network();

        let blank = harness
            .player
            .name_catalog_on_network("   ".to_owned())
            .expect("named by default");
        let unnamed = named_on_the_network(harness.player.offered.port().expect("a port"));

        harness
            .player
            .share_catalog_on_network(false)
            .expect("taken off");

        let resting = harness
            .player
            .name_catalog_on_network("Den".to_owned())
            .expect("named while resting");

        assert_eq!(kept_before, "Kitchen shelf");
        assert_eq!(
            first,
            ("Kitchen shelf".to_owned(), "Kitchen shelf".to_owned())
        );
        assert_eq!(kept_again, "Kitchen shelf");
        assert_eq!(same_port, Some(first_port));
        assert_eq!(renamed, "Study");
        assert!(byebye.contains("uuid:"), "{byebye}");
        assert_eq!(study, ("Study".to_owned(), "Study".to_owned()));
        assert!(still_shared);
        assert_eq!(blank, "");
        assert_eq!(unnamed, ("Resonate".to_owned(), "Resonate".to_owned()));
        assert_eq!(resting, "Den");
        assert!(!harness.player.catalog_shared_on_network());
    }

    fn paged_by(shelves: &Shelves, shelf: Shelf, size: u32) -> (Vec<String>, u32) {
        let mut titles = Vec::new();
        let mut from = 0;

        loop {
            let page = shelved(shelves, shelf, from, size);
            let returned = u32::try_from(page.held.len()).expect("a page");

            titles.extend(titled(&page));
            from += returned;
            if returned == 0 || from >= page.total {
                return (titles, page.total);
            }
        }
    }

    fn titles_found(searched: Searched) -> (Vec<String>, u32) {
        match searched {
            Searched::Found(shelved) => (titled(&shelved), shelved.total),
            Searched::Unanswered => panic!("an answered search"),
        }
    }

    fn document(
        harness: &Harness,
        uri: &str,
        name: &str,
        song: (&str, &str, &str),
        missing: bool,
    ) -> u64 {
        let connection = harness.player.catalog.0.lock();

        connection
            .execute(
                "INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec, missing)
                 VALUES (?1, ?2, ?3, ?4, ?5, 300000, 44100, 'FLAC', ?6)",
                params![uri, name, song.0, song.1, song.2, missing],
            )
            .expect("a document");
        connection.last_insert_rowid() as u64
    }

    #[test]
    fn the_shelves_page_every_song_by_ones_and_by_twos_to_the_same_rows() {
        let harness = Harness::new();

        filed(&harness);

        let shelves = shelves(&harness);

        let (by_ones, total) = paged_by(&shelves, Shelf::Songs, 1);
        let (by_twos, _) = paged_by(&shelves, Shelf::Songs, 2);
        let (by_fives, _) = paged_by(&shelves, Shelf::Songs, 5);
        let whole = titled(&shelved(&shelves, Shelf::Songs, 0, 100));
        let jumped = titled(&shelved(&shelves, Shelf::Songs, 5, 2));
        let past_the_end = shelved(&shelves, Shelf::Songs, 50, 2);

        assert_eq!(total, 8);
        assert_eq!(by_ones, whole);
        assert_eq!(by_twos, whole);
        assert_eq!(by_fives, whole);
        assert_eq!(whole.len(), 8);
        assert_eq!(jumped, whole[5..7]);
        assert!(!whole.contains(&"Liner Notes".to_owned()));
        assert!(past_the_end.held.is_empty());
        assert_eq!(past_the_end.total, 8);
    }

    #[test]
    fn a_catalog_holding_rows_it_cannot_serve_pages_what_it_serves_and_counts_the_same() {
        let harness = Harness::new();
        let ids = filed(&harness);

        let elsewhere = document(
            &harness,
            "file:///elsewhere/elsewhere.flac",
            "elsewhere.flac",
            ("Elsewhere", "Miles Davis", "Kind of Blue"),
            false,
        );
        let gone = document(
            &harness,
            &format!("{}gone.flac", crate::harness::FOLDER),
            "gone.flac",
            ("Gone", "John Coltrane", "Blue Train"),
            false,
        );

        let everything = harness
            .player
            .make_playlist(
                "Everything".to_owned(),
                vec![
                    ids["So What"],
                    ids["Liner Notes"],
                    elsewhere,
                    gone,
                    ids["Blue Train"],
                    ids["Freddie Freeloader"],
                ],
            )
            .expect("a playlist");
        let miles = harness
            .player
            .save_search(
                "Miles".to_owned(),
                SavedSearch {
                    text: "artist:miles".to_owned(),
                    order: SearchOrder::Title,
                    descending: false,
                    rows: None,
                },
            )
            .expect("a saved search");

        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET missing = 1 WHERE id = ?1",
                [gone as i64],
            )
            .expect("a song gone");

        let shelves = shelves(&harness);
        let everything = Shelf::Playlist(PlaylistId::new(everything).expect("an id"));
        let miles = Shelf::Playlist(PlaylistId::new(miles).expect("an id"));
        let albums = shelved(&shelves, Shelf::Albums, 0, 10);

        let children = |shelf: Shelf| {
            shelves
                .shelf(shelf)
                .expect("read")
                .expect("a shelf")
                .children
        };
        let every_song = found(&shelves, Kind::Song, &Query::Everything, 0, 100);
        let (songs, songs_total) = paged_by(&shelves, Shelf::Songs, 1);
        let (everything_by_ones, everything_total) = paged_by(&shelves, everything, 1);
        let (miles_by_twos, miles_total) = paged_by(&shelves, miles, 2);
        let in_everything = found_within(
            &shelves,
            Kind::Song,
            Within::Playlist(match everything {
                Shelf::Playlist(id) => id,
                _ => panic!("not a playlist"),
            }),
            &Query::Everything,
            1,
            1,
        );

        assert_eq!(songs_total, 8);
        assert_eq!(songs.len(), 8);
        assert_eq!(every_song.total, 8);
        assert!(!songs.contains(&"Elsewhere".to_owned()));
        assert!(!songs.contains(&"Gone".to_owned()));
        assert_eq!(titled(&albums), ["Blue Train (2)", "Kind of Blue (2)"]);
        assert_eq!(
            everything_by_ones,
            ["So What", "Blue Train", "Freddie Freeloader"]
        );
        assert_eq!(everything_total, 3);
        assert_eq!(children(everything), 3);
        assert_eq!(titled(&in_everything), ["Blue Train"]);
        assert_eq!(in_everything.total, 3);
        assert_eq!(miles_by_twos, ["Freddie Freeloader", "So What"]);
        assert_eq!(miles_total, 2);
        assert_eq!(children(miles), 2);
        assert_eq!(
            titled(&shelved(&shelves, Shelf::Playlists, 0, 10)),
            ["Everything (3)", "Miles (2)"]
        );
    }

    #[test]
    fn a_playlist_that_names_a_song_twice_pages_every_row_it_counts() {
        let harness = Harness::new();
        let ids = filed(&harness);

        let twice = harness
            .player
            .make_playlist(
                "Twice".to_owned(),
                vec![ids["Blue Train"], ids["So What"], ids["Blue Train"]],
            )
            .expect("a playlist");
        let twice = Shelf::Playlist(PlaylistId::new(twice).expect("an id"));
        let shelves = shelves(&harness);

        let whole = shelved(&shelves, twice, 0, 10);
        let first_page = shelved(&shelves, twice, 0, 2);
        let second_page = shelved(&shelves, twice, 2, 2);

        assert_eq!(titled(&whole), ["Blue Train", "So What", "Blue Train"]);
        assert_eq!(whole.total, 3);
        assert_eq!(titled(&first_page), ["Blue Train", "So What"]);
        assert_eq!(first_page.total, 3);
        assert_eq!(titled(&second_page), ["Blue Train"]);
        assert_eq!(second_page.total, 3);
        assert_eq!(
            shelves
                .shelf(twice)
                .expect("read")
                .map(|held| held.children),
            Some(3)
        );
    }

    const PLAYLISTS: i64 = 500;

    const ROWS_A_PLAYLIST: usize = 1_000;

    fn listed_in_many_playlists(harness: &Harness) {
        let connection = harness.player.catalog.0.lock();
        let songs: Vec<i64> = connection
            .prepare("SELECT id FROM documents ORDER BY id")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get(0))
                    .and_then(Iterator::collect)
            })
            .expect("the songs");
        let filling = connection.unchecked_transaction().expect("a transaction");

        {
            let mut playlist = filling
                .prepare("INSERT INTO playlists (id, name, folded) VALUES (?1, ?2, ?3)")
                .expect("playlists are written");
            let mut entry = filling
                .prepare(
                    "INSERT INTO playlist_rows (playlist, position, track) VALUES (?1, ?2, ?3)",
                )
                .expect("entries are written");

            for list in 1..=PLAYLISTS {
                playlist
                    .execute(params![
                        list,
                        format!("List {list:03}"),
                        format!("list {list:03}")
                    ])
                    .expect("a playlist");
                for position in 0..ROWS_A_PLAYLIST {
                    entry
                        .execute(params![
                            list,
                            position as i64,
                            songs[position % songs.len()]
                        ])
                        .expect("an entry");
                }
            }
        }
        filling.commit().expect("the playlists are kept");
    }

    #[test]
    fn five_hundred_playlists_of_a_thousand_rows_are_browsed_without_loading_an_entry() {
        let harness = Harness::new();

        listed_in_many_playlists(&harness);

        let shelves = shelves(&harness);

        let top: Vec<u32> = Shelf::TOP
            .into_iter()
            .map(|shelf| shelved(&shelves, shelf, 0, 0).total)
            .collect();
        let listed = shelved(&shelves, Shelf::Playlists, 0, PLAYLISTS as u32);
        let tail = shelved(&shelves, Shelf::Playlists, PLAYLISTS as u32 - 2, 10);
        let first = first_shelf(&listed);
        let one = shelves.shelf(first).expect("read").expect("a playlist");
        let near_the_end = shelved(&shelves, first, ROWS_A_PLAYLIST as u32 - 5, 10);
        let loaded = shelves.view().expect("a view").lists.lock().len();

        let children: Vec<u32> = listed
            .held
            .iter()
            .map(|entry| match entry {
                Entry::Shelf(held) => held.children,
                Entry::Song(_) => panic!("a playlist, not a song"),
            })
            .collect();

        assert_eq!(top[2], SONGS.len() as u32);
        assert_eq!(top[3], PLAYLISTS as u32);
        assert_eq!(listed.total, PLAYLISTS as u32);
        assert_eq!(children.len(), PLAYLISTS as usize);
        assert!(
            children
                .iter()
                .all(|counted| *counted == ROWS_A_PLAYLIST as u32)
        );
        assert_eq!(titled(&tail), ["List 499 (1000)", "List 500 (1000)"]);
        assert_eq!(tail.total, PLAYLISTS as u32);
        assert_eq!(one.children, ROWS_A_PLAYLIST as u32);
        assert_eq!(near_the_end.held.len(), 5);
        assert_eq!(near_the_end.total, ROWS_A_PLAYLIST as u32);
        assert_eq!(loaded, 0);
    }

    const SEARCHES_AT_ONCE: usize = 16;

    const SONGS_SEARCHED: usize = 8_000;

    const A_WINDOW_READ_IS_ANSWERED_WITHIN: Duration = Duration::from_millis(250);

    fn filled_with_songs(harness: &Harness) -> u64 {
        let connection = harness.player.catalog.0.lock();
        let filling = connection.unchecked_transaction().expect("a transaction");

        {
            let mut song = filling
                .prepare(
                    "INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec, size)
                     VALUES (?1, ?2, ?3, 'Soloist', 'Sonata', 300000, 44100, 'FLAC', 1)",
                )
                .expect("songs are written");

            for at in 0..SONGS_SEARCHED {
                song.execute(params![
                    format!("{}{at}.flac", crate::harness::FOLDER),
                    format!("{at}.flac"),
                    format!("Song {at}")
                ])
                .expect("a song");
            }
        }
        filling.commit().expect("the songs are kept");
        connection.last_insert_rowid() as u64
    }

    #[test]
    fn sixteen_root_searches_at_once_leave_a_window_read_answered_within_its_deadline() {
        let harness = Harness::new();
        let shown = filled_with_songs(&harness);

        harness
            .player
            .make_playlist("Songs".to_owned(), vec![shown])
            .expect("a playlist");

        let shelves = shelves(&harness);
        let asked = text(Field::Title, Test::Contains, "so");

        let (slowest, searched) = thread::scope(|scope| {
            let searchers: Vec<_> = (0..SEARCHES_AT_ONCE)
                .map(|_| {
                    scope.spawn(|| {
                        [Kind::Artist, Kind::Album, Kind::Playlist, Kind::Song]
                            .map(|kind| {
                                shelves.searched(kind, Within::Catalog, &asked, Page::first(50))
                            })
                            .into_iter()
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut slowest = Duration::ZERO;

            while searchers.iter().any(|searcher| !searcher.is_finished()) {
                let since = Instant::now();

                harness.player.catalog.track(shown).expect("a window read");
                slowest = slowest.max(since.elapsed());
                thread::sleep(Duration::from_millis(5));
            }

            let searched: Vec<_> = searchers
                .into_iter()
                .flat_map(|searcher| searcher.join().expect("a searcher"))
                .collect();
            (slowest, searched)
        });

        let found = searched
            .iter()
            .filter(|outcome| matches!(outcome, Ok(Searched::Found(_))))
            .count();
        let unexpected: Vec<_> = searched
            .iter()
            .filter(|outcome| {
                !matches!(
                    outcome,
                    Ok(Searched::Found(_)) | Err(Error::NoTurnToRead { .. })
                )
            })
            .collect();

        assert!(
            slowest < A_WINDOW_READ_IS_ANSWERED_WITHIN,
            "a window read waited {slowest:?}"
        );
        assert!(found > 0);
        assert!(unexpected.is_empty(), "{unexpected:?}");
    }

    #[test]
    fn a_search_word_of_one_letter_is_unanswered_and_one_of_two_is_read() {
        let harness = Harness::new();

        filed(&harness);

        let shelves = shelves(&harness);
        let every = Page::first(10);

        let searched = |kind: Kind, query: &Query| {
            shelves
                .searched(kind, Within::Catalog, query, every)
                .expect("searched")
        };

        let one_letter = [Kind::Song, Kind::Album, Kind::Artist, Kind::Playlist]
            .map(|kind| searched(kind, &text(Field::Title, Test::Contains, "s")));
        let one_beside_two = searched(Kind::Song, &text(Field::Title, Test::Contains, "so w"));
        let denied_one = searched(Kind::Song, &text(Field::Title, Test::Lacks, "x"));
        let two_letters = titles_found(searched(
            Kind::Song,
            &text(Field::Title, Test::Contains, "so"),
        ));
        let one_whole = titles_found(searched(
            Kind::Song,
            &text(Field::Title, Test::Is, "so what"),
        ));
        let one_letter_whole =
            titles_found(searched(Kind::Song, &text(Field::Title, Test::Is, "a")));

        assert!(
            one_letter
                .iter()
                .all(|searched| *searched == Searched::Unanswered)
        );
        assert_eq!(one_beside_two, Searched::Unanswered);
        assert_eq!(denied_one, Searched::Unanswered);
        assert_eq!(two_letters, (vec!["So What".to_owned()], 1));
        assert_eq!(one_whole, (vec!["So What".to_owned()], 1));
        assert_eq!(one_letter_whole, (vec!["a".to_owned()], 1));
    }
}
