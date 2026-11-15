use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    path::Path,
    sync::Arc,
};
#[cfg(feature = "ui")]
use std::{path::PathBuf, thread};

use parking_lot::Mutex;
#[cfg(feature = "ui")]
use resonate_core::Switch;
use resonate_core::{AlbumId, PlaylistId, TrackId};
use resonate_library::{
    Album, AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Error as LibraryError, Library,
    PlaylistOrder, SortOrder, Track, TrackQuery,
};
use resonate_upnp::{
    AudioKind, Catalog, Entry, Held, Opened, Page, Picture, Shelf, Shelved, Song, Udn,
};
#[cfg(feature = "ui")]
use resonate_upnp::{MediaServer, Offering};

#[cfg(feature = "ui")]
use crate::config::{self, Config};

const UDN_KEPT_IN: &str = "media-server-udn";

const CURSORS_AT_MOST: usize = 256;

const LISTS_KEPT_AT_MOST: usize = 32;

const SONGS_IN: SortOrder = SortOrder::Title;

const ON_AN_ALBUM_IN: SortOrder = SortOrder::AlbumThenTrack;

pub(crate) struct Shelves {
    library: Arc<Library>,
    cursors: Mutex<Cursors>,
    lists: Mutex<Lists>,
}

#[derive(Default)]
struct Cursors {
    revision: u64,
    total: Option<u32>,
    at: BTreeMap<u32, TrackId>,
}

#[derive(Default)]
struct Lists {
    revision: u64,
    kept: HashMap<PlaylistId, Arc<Vec<TrackId>>>,
}

impl Shelves {
    pub(crate) fn over(library: Arc<Library>) -> Self {
        Self {
            library,
            cursors: Mutex::new(Cursors::default()),
            lists: Mutex::new(Lists::default()),
        }
    }

    fn songs(&self, page: Page) -> Result<Shelved, LibraryError> {
        let query = |offset: u32, limit: u32| TrackQuery {
            sort: SONGS_IN,
            reading: SONGS_IN.reads(),
            offset: offset as usize,
            limit: Some(limit as usize),
            ..TrackQuery::default()
        };
        let revision = self.library.catalog_revision();
        let (counted, at) = {
            let mut cursors = self.cursors.lock();
            if cursors.revision != revision {
                *cursors = Cursors {
                    revision,
                    ..Cursors::default()
                };
            }
            (cursors.total, cursors.at.get(&page.from).copied())
        };
        let total = match counted {
            Some(total) => total,
            None => {
                let total = self.library.whole_files_counted(&query(0, 0))?;
                let mut cursors = self.cursors.lock();
                if cursors.revision == revision {
                    cursors.total = Some(total);
                }
                total
            }
        };
        if page.most == 0 || page.from >= total {
            return Ok(Shelved {
                held: Vec::new(),
                total,
            });
        }

        let read_one_past = query(page.from, page.most.saturating_add(1));
        let mut rows = match at {
            Some(at) => self.library.whole_files_from(&read_one_past, at)?,
            None => self.library.whole_files(&read_one_past)?,
        };
        if rows.len() > page.most as usize {
            let next = rows.split_off(page.most as usize);
            if let Some(next) = next.first() {
                let mut cursors = self.cursors.lock();
                if cursors.revision == revision {
                    if cursors.at.len() >= CURSORS_AT_MOST {
                        cursors.at.clear();
                    }
                    cursors.at.insert(page.until(), next.id);
                }
            }
        }
        Ok(Shelved {
            held: self.songs_of(&rows)?,
            total,
        })
    }

    fn songs_of(&self, rows: &[Track]) -> Result<Vec<Entry>, LibraryError> {
        let mut albums: Vec<AlbumId> = rows.iter().filter_map(|track| track.album_id).collect();
        albums.sort_unstable();
        albums.dedup();
        let titled: HashMap<AlbumId, String> =
            self.library.album_titles(&albums)?.into_iter().collect();
        Ok(rows
            .iter()
            .filter_map(|track| {
                let album = track.album_id.and_then(|album| titled.get(&album));
                song_of(track, album.map(String::as_str))
            })
            .map(Entry::Song)
            .collect())
    }

    fn album_held(&self, album: &Album) -> Result<Held, LibraryError> {
        Ok(Held {
            shelf: Shelf::Album(album.id),
            title: album.title.clone(),
            artist: album.artist.clone(),
            children: self.library.whole_files_counted(&on_the_album(album.id))?,
            cover: album.has_cover_art.then_some(album.id),
        })
    }

    fn albums(&self, query: AlbumQuery, page: Page) -> Result<Shelved, LibraryError> {
        let total = self.library.albums_counted(&query)?;
        if page.most == 0 {
            return Ok(Shelved {
                held: Vec::new(),
                total,
            });
        }
        let held = self
            .library
            .albums(&AlbumQuery {
                offset: page.from as usize,
                limit: Some(page.most as usize),
                ..query
            })?
            .iter()
            .map(|album| self.album_held(album).map(Entry::Shelf))
            .collect::<Result<_, _>>()?;
        Ok(Shelved { held, total })
    }

    fn listed(&self, id: PlaylistId) -> Result<Option<Arc<Vec<TrackId>>>, LibraryError> {
        let revision = self.library.catalog_revision();
        {
            let mut lists = self.lists.lock();
            if lists.revision != revision {
                *lists = Lists {
                    revision,
                    kept: HashMap::new(),
                };
            }
            if let Some(kept) = lists.kept.get(&id) {
                return Ok(Some(Arc::clone(kept)));
            }
        }
        if self.library.playlist(id)?.is_none() {
            return Ok(None);
        }
        let songs: Arc<Vec<TrackId>> = Arc::new(
            self.library
                .playlist_entries(id, None)?
                .into_iter()
                .filter_map(|entry| entry.track)
                .filter(is_served)
                .map(|track| track.id)
                .collect(),
        );
        let mut lists = self.lists.lock();
        if lists.revision == revision {
            if lists.kept.len() >= LISTS_KEPT_AT_MOST {
                lists.kept.clear();
            }
            lists.kept.insert(id, Arc::clone(&songs));
        }
        Ok(Some(songs))
    }

    fn played_from(&self, id: PlaylistId, page: Page) -> Result<Option<Shelved>, LibraryError> {
        let Some(listed) = self.listed(id)? else {
            return Ok(None);
        };
        let wanted: Vec<TrackId> = listed
            .iter()
            .skip(page.from as usize)
            .take(page.most as usize)
            .copied()
            .collect();
        let mut read: HashMap<TrackId, Track> = self
            .library
            .tracks_with_ids(&wanted)?
            .into_iter()
            .map(|track| (track.id, track))
            .collect();
        let rows: Vec<Track> = wanted.iter().filter_map(|id| read.remove(id)).collect();
        Ok(Some(Shelved {
            held: self.songs_of(&rows)?,
            total: u32::try_from(listed.len()).unwrap_or(u32::MAX),
        }))
    }
}

fn on_the_album(album: AlbumId) -> TrackQuery {
    TrackQuery {
        album: Some(album),
        sort: ON_AN_ALBUM_IN,
        reading: ON_AN_ALBUM_IN.reads(),
        ..TrackQuery::default()
    }
}

fn is_served(track: &Track) -> bool {
    track.span.is_none() && !track.hidden
}

fn kind_of(path: &Path) -> Option<AudioKind> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(AudioKind::of_extension)
}

fn local_file(track: &Track) -> Option<&Path> {
    track
        .location
        .source()
        .is_local()
        .then(|| track.location.locator().as_path())
        .flatten()
}

fn song_of(track: &Track, album: Option<&str>) -> Option<Song> {
    let kind = kind_of(local_file(track)?)?;
    let spec = track.spec;
    Some(Song {
        id: track.id,
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: album.map(str::to_owned),
        cover: track.album_id,
        genre: track.genre.clone(),
        track_number: track.track_number,
        duration: track.duration.map(|frames| frames.to_duration(spec.rate)),
        kind,
        size: track.file_size,
        rate: spec.rate.hz(),
        channels: u16::from(spec.channels.count().get()),
        bits: track
            .codec
            .is_lossless()
            .then(|| u16::from(spec.format.valid_bits())),
    })
}

impl Catalog for Shelves {
    type Failure = LibraryError;

    fn shelved(&self, shelf: Shelf, page: Page) -> Result<Option<Shelved>, LibraryError> {
        let library = &self.library;
        Ok(Some(match shelf {
            Shelf::Artists => {
                let query = ArtistQuery {
                    sort: ArtistOrder::Name,
                    reading: ArtistOrder::Name.reads(),
                    ..ArtistQuery::default()
                };
                let total = library.artists_counted(&query)?;
                let mut held = Vec::new();
                if page.most > 0 {
                    for artist in library.artists(&ArtistQuery {
                        offset: page.from as usize,
                        limit: Some(page.most as usize),
                        ..query
                    })? {
                        held.push(Entry::Shelf(Held {
                            shelf: Shelf::Artist(artist.id),
                            children: library.albums_counted(&by_the_artist(artist.id))?,
                            title: artist.name,
                            artist: None,
                            cover: None,
                        }));
                    }
                }
                Shelved { held, total }
            }
            Shelf::Artist(id) => {
                if library.artist_detail(id)?.is_none() {
                    return Ok(None);
                }
                self.albums(by_the_artist(id), page)?
            }
            Shelf::Albums => self.albums(
                AlbumQuery {
                    sort: AlbumOrder::Title,
                    reading: AlbumOrder::Title.reads(),
                    ..AlbumQuery::default()
                },
                page,
            )?,
            Shelf::Album(id) => {
                let Some(album) = library.album(id)? else {
                    return Ok(None);
                };
                let query = on_the_album(id);
                let total = library.whole_files_counted(&query)?;
                let rows = if page.most == 0 {
                    Vec::new()
                } else {
                    library.whole_files(&TrackQuery {
                        offset: page.from as usize,
                        limit: Some(page.most as usize),
                        ..query
                    })?
                };
                let held = rows
                    .iter()
                    .filter_map(|track| song_of(track, Some(&album.title)))
                    .map(Entry::Song)
                    .collect();
                Shelved { held, total }
            }
            Shelf::Songs => self.songs(page)?,
            Shelf::Playlists => {
                let total = library.playlist_count(None)?;
                let mut held = Vec::new();
                if page.most > 0 {
                    for playlist in library.playlists_within(
                        PlaylistOrder::Name,
                        PlaylistOrder::Name.reads(),
                        None,
                        page.from as usize,
                        page.most as usize,
                    )? {
                        let children = self
                            .listed(playlist.id)?
                            .map_or(0, |listed| u32::try_from(listed.len()).unwrap_or(u32::MAX));
                        held.push(Entry::Shelf(Held {
                            shelf: Shelf::Playlist(playlist.id),
                            title: playlist.name,
                            artist: None,
                            children,
                            cover: None,
                        }));
                    }
                }
                Shelved { held, total }
            }
            Shelf::Playlist(id) => return self.played_from(id, page),
        }))
    }

    fn shelf(&self, shelf: Shelf) -> Result<Option<Held>, LibraryError> {
        let library = &self.library;
        match shelf {
            Shelf::Artist(id) => {
                let Some(artist) = library.artist_detail(id)? else {
                    return Ok(None);
                };
                Ok(Some(Held {
                    shelf,
                    title: artist.name,
                    artist: None,
                    children: library.albums_counted(&by_the_artist(id))?,
                    cover: None,
                }))
            }
            Shelf::Album(id) => library
                .album(id)?
                .map(|album| self.album_held(&album))
                .transpose(),
            Shelf::Playlist(id) => {
                let Some(playlist) = library.playlist(id)? else {
                    return Ok(None);
                };
                let children = self
                    .listed(id)?
                    .map_or(0, |listed| u32::try_from(listed.len()).unwrap_or(u32::MAX));
                Ok(Some(Held {
                    shelf,
                    title: playlist.name,
                    artist: None,
                    children,
                    cover: None,
                }))
            }
            Shelf::Artists | Shelf::Albums | Shelf::Songs | Shelf::Playlists => Ok(None),
        }
    }

    fn song(&self, id: TrackId) -> Result<Option<Song>, LibraryError> {
        let Some(track) = self.library.track(id)?.filter(is_served) else {
            return Ok(None);
        };
        let album = match track.album_id {
            Some(album) => self.library.album(album)?.map(|album| album.title),
            None => None,
        };
        Ok(song_of(&track, album.as_deref()))
    }

    fn bytes_of(&self, id: TrackId) -> Result<Option<Opened>, LibraryError> {
        let Some(track) = self.library.track(id)?.filter(is_served) else {
            return Ok(None);
        };
        let Some(path) = local_file(&track) else {
            return Ok(None);
        };
        let Some(kind) = kind_of(path) else {
            return Ok(None);
        };
        match File::open(path) {
            Ok(file) => Ok(Some(Opened {
                bytes: Box::new(file),
                kind,
            })),
            Err(error) => {
                tracing::debug!(%error, path = %path.display(), "a song asked for on the network is not there");
                Ok(None)
            }
        }
    }

    fn cover(&self, album: AlbumId) -> Result<Option<Picture>, LibraryError> {
        Ok(self.library.cover_art(album)?.map(|cover| Picture {
            mime: cover.format.media_type(),
            bytes: cover.bytes,
        }))
    }

    fn revision(&self) -> u32 {
        let revision = self.library.catalog_revision();
        u32::try_from(revision % (u64::from(u32::MAX) + 1)).unwrap_or_default()
    }
}

fn by_the_artist(id: resonate_core::ArtistId) -> AlbumQuery {
    AlbumQuery {
        artist: Some(id),
        sort: AlbumOrder::Year,
        reading: AlbumOrder::Year.reads(),
        ..AlbumQuery::default()
    }
}

pub(crate) fn kept_udn(folder: &Path) -> Udn {
    let kept_in = folder.join(UDN_KEPT_IN);
    if let Some(kept) = fs::read_to_string(&kept_in)
        .ok()
        .and_then(|text| Udn::read(&text))
    {
        return kept;
    }
    let minted = Udn::minted();
    if let Err(error) = fs::write(&kept_in, minted.as_str()) {
        tracing::warn!(%error, path = %kept_in.display(), "the media server's name on the network could not be kept, so it changes next run");
    }
    minted
}

#[cfg(feature = "ui")]
pub(crate) struct Offered {
    library: Arc<Library>,
    kept_in: Option<PathBuf>,
    running: Mutex<Option<MediaServer<Shelves>>>,
}

#[cfg(feature = "ui")]
impl Offered {
    pub(crate) fn for_the_window(config: &Config, library: &Arc<Library>) -> Arc<Self> {
        let kept_in = config::private_data_dir()
            .inspect_err(|error| {
                tracing::warn!(%error, "no folder keeps the media server's name on the network");
            })
            .ok();
        let offered = Arc::new(Self {
            library: Arc::clone(library),
            kept_in,
            running: Mutex::new(None),
        });
        if config.is_on(Switch::ShareCatalogOnNetwork) {
            offered.share(true);
        }
        offered
    }

    pub(crate) fn share(&self, on: bool) {
        let mut running = self.running.lock();
        if !on {
            if let Some(stopping) = running.take() {
                spawn_the_stop(stopping);
            }
            return;
        }
        if running.is_some() {
            return;
        }
        let udn = self.kept_in.as_deref().map_or_else(Udn::minted, kept_udn);
        match MediaServer::start(
            Arc::new(Shelves::over(Arc::clone(&self.library))),
            udn,
            Offering::on_the_network(),
        ) {
            Ok(server) => {
                tracing::info!(
                    port = server.port(),
                    "the catalog is offered on the network"
                );
                *running = Some(server);
            }
            Err(error) => tracing::warn!(%error, "the catalog could not be offered on the network"),
        }
    }

    pub(crate) fn shared(&self) -> bool {
        self.running.lock().is_some()
    }

    pub(crate) fn leave(&self) {
        drop(self.running.lock().take());
    }
}

#[cfg(feature = "ui")]
fn spawn_the_stop(stopping: MediaServer<Shelves>) {
    let spawned = thread::Builder::new()
        .name("resonate-offer-stop".to_owned())
        .spawn(move || drop(stopping));
    if let Err(error) = spawned {
        tracing::warn!(%error, "the media server is stopped on the window's thread");
    }
}

#[cfg(feature = "ui")]
impl resonate_ui::CatalogShared for Offered {
    fn shared(&self) -> bool {
        Self::shared(self)
    }

    fn share(&self, on: bool) {
        Self::share(self, on);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read as _};

    use super::*;
    use crate::fixtures::{ALBUM, ARTIST, TITLE, scanned, scratch, silent_wave, tagged_wave};

    const CUT: &str = "FILE \"whole.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Cut one\"\n    \
                       INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Cut two\"\n    INDEX 01 \
                       00:00:40\n";

    fn shelved_songs(shelves: &Shelves, most: u32) -> (Vec<String>, u32) {
        let mut titles = Vec::new();
        let mut from = 0;
        loop {
            let page = shelves
                .shelved(Shelf::Songs, Page { from, most })
                .expect("a page")
                .expect("the shelf");
            let total = page.total;
            let returned = page.held.len() as u32;
            titles.extend(page.held.into_iter().filter_map(|entry| match entry {
                Entry::Song(song) => Some(song.title),
                Entry::Shelf(_) => None,
            }));
            from += returned;
            if returned == 0 || from >= total {
                return (titles, total);
            }
        }
    }

    #[test]
    fn the_shelves_page_whole_files_by_key_and_leave_a_sheets_cuts_out() {
        let root = scratch("offered");
        for (file, title) in [("c.wav", "Charlie"), ("a.wav", "Alpha"), ("b.wav", "Bravo")] {
            fs::write(
                root.join(file),
                tagged_wave(&[(TITLE, title), (ARTIST, "Band"), (ALBUM, "Record")]),
            )
            .expect("a song");
        }
        fs::write(root.join("whole.wav"), silent_wave(20)).expect("a sheet's file");
        fs::write(root.join("whole.cue"), CUT).expect("a sheet");
        let library = Arc::new(scanned(&root));
        let cuts = library
            .tracks(&TrackQuery::default())
            .expect("the rows")
            .into_iter()
            .filter(|track| track.span.is_some())
            .collect::<Vec<_>>();
        let shelves = Shelves::over(Arc::clone(&library));

        let (by_ones, total) = shelved_songs(&shelves, 1);
        let (by_twos, _) = shelved_songs(&shelves, 2);
        let cut = cuts.first().expect("a sheet's cut");
        let first = library
            .whole_files(&TrackQuery::default())
            .expect("the rows")
            .into_iter()
            .find(|track| track.title == "Alpha")
            .expect("a song");
        let mut bytes = Vec::new();
        shelves
            .bytes_of(first.id)
            .expect("read")
            .expect("served")
            .bytes
            .read_to_end(&mut bytes)
            .expect("the file");
        let _ = fs::remove_dir_all(&root);

        assert_eq!(cuts.len(), 2);
        assert_eq!(total, 3);
        assert_eq!(by_ones, ["Alpha", "Bravo", "Charlie"]);
        assert_eq!(by_twos, by_ones);
        assert!(shelves.song(cut.id).expect("read").is_none());
        assert!(shelves.bytes_of(cut.id).expect("read").is_none());
        assert_eq!(
            bytes,
            tagged_wave(&[(TITLE, "Alpha"), (ARTIST, "Band"), (ALBUM, "Record")])
        );
        assert_eq!(
            shelves.song(first.id).expect("read").map(|song| song.album),
            Some(Some("Record".to_owned()))
        );
    }

    fn kept_udn_within(folder: &Path) -> io::Result<Udn> {
        fs::create_dir_all(folder)?;
        Ok(kept_udn(folder))
    }

    #[test]
    fn the_server_is_named_on_the_network_by_one_udn_across_runs() {
        let folder = std::env::temp_dir().join(format!("resonate-offered-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);

        let first = kept_udn_within(&folder).expect("a writable folder");
        let again = kept_udn_within(&folder).expect("a writable folder");
        fs::write(folder.join(UDN_KEPT_IN), "not a udn").expect("written");
        let replaced = kept_udn_within(&folder).expect("a writable folder");
        let _ = fs::remove_dir_all(&folder);

        assert_eq!(first, again);
        assert_ne!(replaced, first);
        assert!(Udn::read(replaced.as_str()).is_some());
    }
}
