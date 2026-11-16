use std::{
    cmp::Ordering,
    collections::HashMap,
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
    AudioKind, Entry, Field, Held, Kind, MediaServer, Offering, Opened, Page, Picture, Query,
    Searched, Shelf, Shelved, Song, Test, Udn,
};
use rusqlite::OptionalExtension as _;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::Catalog,
    documents::location,
    saved_searches::saved_query,
    search::{CollectionOrder, Index, Row, Searching, album_order, artist_order},
};

const UDN_KEPT_IN: &str = "media-server-udn";

const LISTS_KEPT_AT_MOST: usize = 32;

const NAMED_FROM: u64 = 0xcbf2_9ce4_8422_2325;

const NAMED_BY: u64 = 0x0000_0100_0000_01b3;

const PLAYLISTS_BY_NAME: &str = "SELECT id, name FROM playlists ORDER BY folded, id";

const A_PLAYLISTS_NAME: &str = "SELECT name FROM playlists WHERE id = ?1";

const A_PLAYLISTS_SONGS: &str = "SELECT r.track FROM playlist_rows r JOIN documents d ON d.id = r.track WHERE r.playlist = ?1 AND d.missing = 0 ORDER BY r.position";

const SIZES_KNOWN: &str = "SELECT id, size FROM documents WHERE missing = 0 AND size IS NOT NULL";

const NO_VALUES: [&str; 2] = ["", ""];

pub(crate) struct Shelves {
    catalog: Arc<Catalog>,
    searching: Arc<Searching>,
    sources: Arc<Sources>,
    view: Mutex<Option<Arc<View>>>,
}

struct View {
    index: Arc<Index>,
    songs: Vec<usize>,
    song_at: HashMap<u64, usize>,
    albums: Vec<Album>,
    album_at: HashMap<AlbumId, usize>,
    album_of_group: HashMap<usize, usize>,
    artists: Vec<Artist>,
    artist_at: HashMap<ArtistId, usize>,
    sizes: HashMap<u64, u64>,
    lists: Mutex<HashMap<u64, Arc<Vec<usize>>>>,
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

    fn playlist_held(&self, view: &View, playlist: Playlist) -> Result<Held> {
        Ok(Held {
            shelf: Shelf::Playlist(playlist.id),
            children: counted(self.listed(view, playlist.id)?.len()),
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

        Self {
            index,
            songs,
            song_at,
            albums,
            album_at,
            album_of_group,
            artists,
            artist_at,
            sizes,
            lists: Mutex::new(HashMap::new()),
        }
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
            size: self.sizes.get(&track.id).copied().unwrap_or_default(),
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

    fn searched(&self, kind: Kind, query: &Query, page: Page) -> Result<Searched> {
        let Some(wanted) = Wanted::of(query) else {
            return Ok(Searched::Unanswered);
        };
        let view = self.view()?;

        let shelved = match kind {
            Kind::Song => {
                let found: Vec<usize> = view
                    .songs
                    .iter()
                    .copied()
                    .filter(|at| wanted.holds(&|field| view.song_values(*at, field)))
                    .collect();
                view.songs_paged(&found, page)
            }
            Kind::Album => {
                let found: Vec<usize> = (0..view.albums.len())
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
            Kind::Artist => {
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
            Kind::Playlist => {
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
        };
        Ok(Searched::Found(shelved))
    }

    fn shelved(&self, shelf: Shelf, page: Page) -> Result<Option<Shelved>> {
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
            Shelf::Playlists => self.playlists_held(&view, self.playlists()?, page)?,
            Shelf::Playlist(id) => {
                if self.playlist(id)?.is_none() {
                    return Ok(None);
                }
                view.songs_paged(&self.listed(&view, id)?, page)
            }
        }))
    }

    fn shelf(&self, shelf: Shelf) -> Result<Option<Held>> {
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
        let view = self.view()?;

        Ok(view.song_at.get(&id.get()).and_then(|at| view.song_of(*at)))
    }

    fn bytes_of(&self, id: TrackId) -> Result<Option<Opened>> {
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
            })),
            Err(error) => {
                tracing::debug!(%error, "a song asked for on the network could not be opened");
                Ok(None)
            }
        }
    }

    fn cover(&self, album: AlbumId) -> Result<Option<Picture>> {
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
                    if words.is_empty() {
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
    let at = location(&track.uri).ok()?;
    track
        .name
        .rsplit_once('.')
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
        let server =
            MediaServer::start(Arc::new(shelves()), kept_udn(&self.kept_in), self.offering)
                .map_err(|source| Error::CatalogNotOffered {
                    source: Box::new(source),
                })?;
        tracing::info!(
            port = server.port(),
            "the catalog is offered on the network"
        );
        *running = Some(server);
        Ok(())
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

#[uniffi::export]
impl MobilePlayer {
    pub fn share_catalog_on_network(&self, on: bool) -> Result<()> {
        self.offered.share(on, || {
            Shelves::over(
                Arc::clone(&self.catalog),
                Arc::clone(&self.services.searching),
                Arc::clone(&self.sources),
            )
        })
    }

    pub fn catalog_shared_on_network(&self) -> bool {
        self.offered.shared()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
        time::Instant,
    };

    use resonate_upnp::Catalog as _;
    use rusqlite::params;

    use super::*;
    use crate::{
        SavedSearch,
        harness::{Harness, SETTLED_WITHIN},
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

    fn filed(harness: &Harness) -> HashMap<&'static str, u64> {
        let mut ids = HashMap::new();
        for song in FILED {
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
            ids.insert(song.title, connection.last_insert_rowid() as u64);
        }
        ids
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
        match shelves
            .searched(kind, query, Page { from, most })
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
        let shelves = shelves(&harness);
        let blue_train = TrackId::new(ids["Blue Train"]).expect("an id");

        let song = shelves.song(blue_train).expect("read").expect("a song");
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
        assert_eq!(song.size, 4_096);
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
        let body = format!(
            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:Browse xmlns:u=\"{DIRECTORY}\"><ObjectID>{object}</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"
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
}
