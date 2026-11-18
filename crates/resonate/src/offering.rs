use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    path::Path,
    sync::Arc,
};
#[cfg(feature = "ui")]
use std::{path::PathBuf, thread};

use parking_lot::Mutex;
use resonate_codec::{DecodeStatus, Decoder, Error as CodecError, Sources};
#[cfg(feature = "ui")]
use resonate_core::Switch;
use resonate_core::{
    AlbumId, ArtistId, AudioBuffer, FrameSpan, Frames, MediaLocation, PlaylistId, StreamSpec,
    TrackId, is_lettered,
};
use resonate_library::{
    Album, AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Asked, Clause, Column, Condition,
    Error as LibraryError, Library, PlaylistOrder, Reach, Search, SortOrder, Track, TrackQuery,
    Word,
};
use resonate_upnp::{
    AsWave, AudioKind, Catalog, Content, Entry, Field, Held, Kind, Opened, Page, Pcm, Picture,
    Query, Searched, Shelf, Shelved, Song, Test, Udn, Within, wave_bytes, waves_alike,
};
#[cfg(feature = "ui")]
use resonate_upnp::{MediaServer, Offering, ServerName};

#[cfg(feature = "ui")]
use crate::config::{self, Config};

const UDN_KEPT_IN: &str = "media-server-udn";

const CURSORS_AT_MOST: usize = 256;

const LISTS_KEPT_AT_MOST: usize = 32;

const SONGS_IN: SortOrder = SortOrder::Title;

const ON_AN_ALBUM_IN: SortOrder = SortOrder::AlbumThenTrack;

const CLAUSES_AT_MOST: usize = 32;

const ALTERNATIVES_AT_MOST: usize = 16;

const QUOTE: char = '"';

const WHOLE: char = '=';

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
            offset: offset as usize,
            limit: Some(limit as usize),
            ..songs_in_order()
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
                let total = self.library.tracks_counted(&query(0, 0))?;
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
            Some(at) => self.library.tracks_from(&read_one_past, at)?,
            None => self.library.tracks(&read_one_past)?,
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

    fn artists(&self, query: ArtistQuery, page: Page) -> Result<Shelved, LibraryError> {
        let library = &self.library;
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
        Ok(Shelved { held, total })
    }

    fn songs_in(&self, query: TrackQuery, page: Page) -> Result<Shelved, LibraryError> {
        let total = self.library.tracks_counted(&query)?;
        if page.most == 0 || page.from >= total {
            return Ok(Shelved {
                held: Vec::new(),
                total,
            });
        }
        let rows = self.library.tracks(&TrackQuery {
            offset: page.from as usize,
            limit: Some(page.most as usize),
            ..query
        })?;
        Ok(Shelved {
            held: self.songs_of(&rows)?,
            total,
        })
    }

    fn playlists(&self, named: Option<&str>, page: Page) -> Result<Shelved, LibraryError> {
        let library = &self.library;
        let total = library.playlist_count(named)?;
        let mut held = Vec::new();
        if page.most > 0 {
            for playlist in library.playlists_within(
                PlaylistOrder::Name,
                PlaylistOrder::Name.reads(),
                named,
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
        Ok(Shelved { held, total })
    }

    fn songs_found(
        &self,
        within: Within,
        text: Option<String>,
        page: Page,
    ) -> Result<Shelved, LibraryError> {
        let query = match within {
            Within::Catalog => songs_in_order(),
            Within::Artists => TrackQuery {
                by_an_artist: true,
                ..songs_in_order()
            },
            Within::Albums => TrackQuery {
                on_an_album: true,
                ..songs_in_order()
            },
            Within::Artist(id) => played_by(id),
            Within::Album(id) => on_the_album(id),
            Within::Playlist(id) => return self.played_from_matching(id, text.as_deref(), page),
        };
        self.songs_in(TrackQuery { text, ..query }, page)
    }

    fn played_from_matching(
        &self,
        id: PlaylistId,
        text: Option<&str>,
        page: Page,
    ) -> Result<Shelved, LibraryError> {
        if text.is_none() {
            return Ok(self.played_from(id, page)?.unwrap_or(Shelved {
                held: Vec::new(),
                total: 0,
            }));
        }
        let matching: Vec<Track> = self
            .library
            .playlist_entries(id, text)?
            .into_iter()
            .filter_map(|entry| entry.track)
            .filter(is_served)
            .collect();
        let rows: Vec<Track> = matching
            .iter()
            .skip(page.from as usize)
            .take(page.most as usize)
            .cloned()
            .collect();
        Ok(Shelved {
            held: self.songs_of(&rows)?,
            total: u32::try_from(matching.len()).unwrap_or(u32::MAX),
        })
    }

    fn album_held(&self, album: &Album) -> Result<Held, LibraryError> {
        Ok(Held {
            shelf: Shelf::Album(album.id),
            title: album.title.clone(),
            artist: album.artist.clone(),
            children: self.library.tracks_counted(&on_the_album(album.id))?,
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
        let read: HashMap<TrackId, Track> = self
            .library
            .tracks_with_ids(&wanted)?
            .into_iter()
            .map(|track| (track.id, track))
            .collect();
        let rows: Vec<Track> = wanted
            .iter()
            .filter_map(|id| read.get(id).cloned())
            .collect();
        Ok(Some(Shelved {
            held: self.songs_of(&rows)?,
            total: u32::try_from(listed.len()).unwrap_or(u32::MAX),
        }))
    }
}

fn songs_in_order() -> TrackQuery {
    TrackQuery {
        sort: SONGS_IN,
        reading: SONGS_IN.reads(),
        ..TrackQuery::default()
    }
}

struct Decoding {
    sources: Arc<Sources>,
    location: MediaLocation,
    span: FrameSpan,
    decoder: Decoder,
    block: AudioBuffer,
    spec: StreamSpec,
}

impl Decoding {
    fn open(
        sources: Arc<Sources>,
        location: MediaLocation,
        span: FrameSpan,
    ) -> Result<Self, CodecError> {
        let (mut decoder, info) = Decoder::open_span(&sources, &location, span)?;
        decoder.set_output_format(info.spec.format);
        let spec = StreamSpec {
            format: info.spec.format,
            ..decoder.sounding()
        };
        Ok(Self {
            sources,
            location,
            span,
            decoder,
            block: AudioBuffer::empty(spec),
            spec,
        })
    }
}

impl Pcm for Decoding {
    type Failure = CodecError;

    fn seek(&mut self, to: Frames) -> Result<Frames, CodecError> {
        if self.decoder.info().is_seekable {
            return self.decoder.seek(to);
        }
        *self = Self::open(Arc::clone(&self.sources), self.location.clone(), self.span)?;
        Ok(Frames::ZERO)
    }

    fn next_block(&mut self) -> Result<Option<&AudioBuffer>, CodecError> {
        Ok(match self.decoder.next_block(&mut self.block)? {
            DecodeStatus::Decoded => Some(&self.block),
            DecodeStatus::EndOfStream | DecodeStatus::Reformatted(_) => None,
        })
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

fn played_by(artist: ArtistId) -> TrackQuery {
    TrackQuery {
        artist: Some(artist),
        sort: ON_AN_ALBUM_IN,
        reading: ON_AN_ALBUM_IN.reads(),
        ..TrackQuery::default()
    }
}

fn is_served(track: &Track) -> bool {
    !track.hidden
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

fn waved(track: &Track) -> Option<(StreamSpec, Frames)> {
    Some((track.spec, track.duration?))
}

fn song_of(track: &Track, album: Option<&str>) -> Option<Song> {
    let file = local_file(track)?;
    let spec = track.spec;
    let (kind, content, size) = match track.span {
        None => (kind_of(file)?, Content::Untouched, track.file_size),
        Some(_) => {
            let (spec, frames) = waved(track)?;
            (
                AudioKind::Wave,
                Content::Converted,
                wave_bytes(spec, frames),
            )
        }
    };
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
        content,
        size: Some(size),
        rate: spec.rate.hz(),
        channels: u16::from(spec.channels.count().get()),
        bits: track
            .codec
            .is_lossless()
            .then(|| u16::from(spec.format.valid_bits())),
    })
}

impl Shelves {
    fn cut_opened(&self, track: &Track, span: FrameSpan) -> Option<Opened> {
        let (announced, frames) = waved(track)?;
        match Decoding::open(
            Arc::new(self.library.sources()),
            track.location.clone(),
            span,
        ) {
            Ok(cut) if waves_alike(cut.spec, announced) => Some(Opened {
                bytes: Box::new(AsWave::over(cut, announced, frames)),
                kind: AudioKind::Wave,
                content: Content::Converted,
            }),
            Ok(cut) => {
                tracing::debug!(decoded = ?cut.spec, announced = ?announced, location = %track.location, "a cut decodes otherwise than its row says, so it is not served");
                None
            }
            Err(error) => {
                tracing::debug!(%error, location = %track.location, "a cut asked for on the network would not decode");
                None
            }
        }
    }
}

impl Catalog for Shelves {
    type Failure = LibraryError;

    fn searched(
        &self,
        kind: Kind,
        within: Within,
        query: &Query,
        page: Page,
    ) -> Result<Searched, LibraryError> {
        let found = match (kind, within) {
            (Kind::Song, _) => searched_as(query, song_column, Denials::Read)
                .map(|text| self.songs_found(within, text, page)),
            (Kind::Album, Within::Catalog | Within::Albums) => {
                searched_as(query, album_column, Denials::Refused)
                    .map(|text| self.albums(albums_named(text), page))
            }
            (Kind::Album, Within::Artists) => searched_as(query, album_column, Denials::Refused)
                .map(|text| {
                    self.albums(
                        AlbumQuery {
                            by_an_artist: true,
                            ..albums_named(text)
                        },
                        page,
                    )
                }),
            (Kind::Album, Within::Artist(id)) => searched_as(query, album_column, Denials::Refused)
                .map(|text| {
                    self.albums(
                        AlbumQuery {
                            text,
                            ..by_the_artist(id)
                        },
                        page,
                    )
                }),
            (Kind::Artist, Within::Catalog | Within::Artists) => {
                searched_as(query, artist_column, Denials::Refused)
                    .map(|text| self.artists(artists_named(text), page))
            }
            (Kind::Playlist, Within::Catalog) => {
                playlists_named(query).map(|named| self.playlists(named.as_deref(), page))
            }
            (Kind::Album | Kind::Artist | Kind::Playlist, _) => Some(Ok(Shelved {
                held: Vec::new(),
                total: 0,
            })),
        };
        match found {
            Some(shelved) => Ok(Searched::Found(shelved?)),
            None => Ok(Searched::Unanswered),
        }
    }

    fn shelved(&self, shelf: Shelf, page: Page) -> Result<Option<Shelved>, LibraryError> {
        let library = &self.library;
        Ok(Some(match shelf {
            Shelf::Artists => self.artists(artists_named(None), page)?,
            Shelf::Artist(id) => {
                if library.artist_detail(id)?.is_none() {
                    return Ok(None);
                }
                self.albums(by_the_artist(id), page)?
            }
            Shelf::Albums => self.albums(albums_named(None), page)?,
            Shelf::Album(id) => {
                let Some(album) = library.album(id)? else {
                    return Ok(None);
                };
                let query = on_the_album(id);
                let total = library.tracks_counted(&query)?;
                let rows = if page.most == 0 {
                    Vec::new()
                } else {
                    library.tracks(&TrackQuery {
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
            Shelf::Playlists => self.playlists(None, page)?,
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
        if let Some(span) = track.span {
            return Ok(self.cut_opened(&track, span));
        }
        let Some(kind) = kind_of(path) else {
            return Ok(None);
        };
        match File::open(path) {
            Ok(file) => Ok(Some(Opened {
                bytes: Box::new(file),
                kind,
                content: Content::Untouched,
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

fn artists_named(text: Option<String>) -> ArtistQuery {
    ArtistQuery {
        text,
        sort: ArtistOrder::Name,
        reading: ArtistOrder::Name.reads(),
        ..ArtistQuery::default()
    }
}

fn albums_named(text: Option<String>) -> AlbumQuery {
    AlbumQuery {
        text,
        sort: AlbumOrder::Title,
        reading: AlbumOrder::Title.reads(),
        ..AlbumQuery::default()
    }
}

const fn song_column(field: Field) -> Option<Column> {
    Some(match field {
        Field::Title => Column::Title,
        Field::Artist => Column::Artist,
        Field::Album => Column::Album,
        Field::Genre => Column::Genre,
    })
}

const fn album_column(field: Field) -> Option<Column> {
    match field {
        Field::Title => Some(Column::Album),
        Field::Artist => Some(Column::Artist),
        Field::Album | Field::Genre => None,
    }
}

const fn artist_column(field: Field) -> Option<Column> {
    match field {
        Field::Title => Some(Column::Artist),
        Field::Artist | Field::Album | Field::Genre => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Denials {
    Read,
    Refused,
}

#[derive(Clone)]
struct Atom {
    word: Word,
    denied: bool,
}

enum Formula {
    All(Vec<Self>),
    Any(Vec<Self>),
    Atom(Atom),
}

fn has_letters(text: &str) -> bool {
    text.chars().any(is_lettered)
}

fn unquoted(text: &str) -> String {
    text.chars().filter(|letter| *letter != QUOTE).collect()
}

fn begun_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| unquoted(word).trim_start_matches(WHOLE).to_owned())
        .filter(|word| has_letters(word))
        .collect()
}

fn atom(column: Column, text: String, reach: Reach, denied: bool) -> Formula {
    Formula::Atom(Atom {
        word: Word {
            column: Some(column),
            text,
            reach,
        },
        denied,
    })
}

fn formula(
    query: &Query,
    column_of: fn(Field) -> Option<Column>,
    denials: Denials,
) -> Option<Formula> {
    let refuses_denials = denials == Denials::Refused;
    Some(match query {
        Query::Everything => Formula::All(Vec::new()),
        Query::All(all) => Formula::All(
            all.iter()
                .map(|one| formula(one, column_of, denials))
                .collect::<Option<_>>()?,
        ),
        Query::Any(any) => Formula::Any(
            any.iter()
                .map(|one| formula(one, column_of, denials))
                .collect::<Option<_>>()?,
        ),
        Query::Present { .. } => return None,
        Query::Text { field, test, text } => {
            let column = column_of(*field)?;
            match test {
                Test::Is | Test::IsNot => {
                    let denied = *test == Test::IsNot;
                    if (denied && refuses_denials) || !has_letters(text) {
                        return None;
                    }
                    atom(column, unquoted(text), Reach::Whole, denied)
                }
                Test::Contains | Test::Lacks => {
                    let denied = *test == Test::Lacks;
                    let words = begun_words(text);
                    if (denied && refuses_denials) || words.is_empty() {
                        return None;
                    }
                    let atoms = words
                        .into_iter()
                        .map(|word| atom(column, word, Reach::Begins, denied))
                        .collect();
                    if denied {
                        Formula::Any(atoms)
                    } else {
                        Formula::All(atoms)
                    }
                }
            }
        }
    })
}

fn clauses(formula: &Formula) -> Option<Vec<Vec<Atom>>> {
    match formula {
        Formula::Atom(atom) => Some(vec![vec![atom.clone()]]),
        Formula::All(all) => {
            let mut held = Vec::new();
            for one in all {
                held.extend(clauses(one)?);
                if held.len() > CLAUSES_AT_MOST {
                    return None;
                }
            }
            Some(held)
        }
        Formula::Any(any) => {
            let mut held: Vec<Vec<Atom>> = vec![Vec::new()];
            for one in any {
                let either = clauses(one)?;
                let mut crossed = Vec::new();
                for before in &held {
                    for clause in &either {
                        if before.len() + clause.len() > ALTERNATIVES_AT_MOST {
                            return None;
                        }
                        crossed.push(before.iter().chain(clause).cloned().collect());
                        if crossed.len() > CLAUSES_AT_MOST {
                            return None;
                        }
                    }
                }
                held = crossed;
            }
            Some(held)
        }
    }
}

fn searched_as(
    query: &Query,
    column_of: fn(Field) -> Option<Column>,
    denials: Denials,
) -> Option<Option<String>> {
    let clauses = clauses(&formula(query, column_of, denials)?)?;
    if clauses.iter().any(Vec::is_empty) {
        return None;
    }
    if clauses.is_empty() {
        return Some(None);
    }
    let search = Search {
        clauses: clauses
            .into_iter()
            .map(|any| Clause {
                any: any
                    .into_iter()
                    .map(|atom| Asked {
                        denied: atom.denied,
                        all: std::iter::once(Condition::Word(atom.word)).collect(),
                    })
                    .collect(),
            })
            .collect(),
    };
    Some(Some(search.to_string()))
}

fn playlists_named(query: &Query) -> Option<Option<String>> {
    let named = |query: &Query| match query {
        Query::Text {
            field: Field::Title,
            test: Test::Contains,
            text,
        } => {
            let words: Vec<&str> = text
                .split(|letter: char| !is_lettered(letter))
                .filter(|word| !word.is_empty())
                .collect();
            (!words.is_empty()).then(|| words.join(" "))
        }
        _ => None,
    };
    match query {
        Query::Everything => Some(None),
        Query::All(all) => all
            .iter()
            .map(named)
            .collect::<Option<Vec<String>>>()
            .map(|words| Some(words.join(" "))),
        query => named(query).map(Some),
    }
}

fn by_the_artist(id: ArtistId) -> AlbumQuery {
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
    state: Arc<Mutex<Offer>>,
}

#[cfg(feature = "ui")]
#[derive(Default)]
struct Offer {
    server: Option<MediaServer<Shelves>>,
    wanted: bool,
    restarting: bool,
    name: Option<ServerName>,
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
            state: Arc::new(Mutex::new(Offer {
                name: config.offer_name().cloned(),
                ..Offer::default()
            })),
        });
        if config.is_on(Switch::ShareCatalogOnNetwork) {
            offered.share(true);
        }
        offered
    }

    pub(crate) fn share(&self, on: bool) {
        let mut state = self.state.lock();
        state.wanted = on;
        if !on {
            if let Some(stopping) = state.server.take() {
                spawn_the_stop(stopping);
            }
            return;
        }
        if state.server.is_some() || state.restarting {
            return;
        }
        state.server = started(&self.library, self.kept_in.as_deref(), state.name.clone());
    }

    pub(crate) fn shared(&self) -> bool {
        let state = self.state.lock();
        state.wanted && (state.server.is_some() || state.restarting)
    }

    pub(crate) fn named(&self) -> String {
        self.state
            .lock()
            .name
            .as_ref()
            .map_or_else(String::new, |name| name.as_str().to_owned())
    }

    pub(crate) fn rename(&self, typed: &str) {
        let name = ServerName::new(typed);
        let mut state = self.state.lock();
        if state.name == name {
            return;
        }
        state.name = name;
        if state.restarting {
            return;
        }
        let Some(stopping) = state.server.take() else {
            return;
        };
        state.restarting = true;

        let held = Arc::clone(&self.state);
        let library = Arc::clone(&self.library);
        let kept_in = self.kept_in.clone();
        let spawned = thread::Builder::new()
            .name("resonate-offer-rename".to_owned())
            .spawn(move || {
                drop(stopping);
                let mut state = held.lock();
                state.restarting = false;
                if state.wanted {
                    state.server = started(&library, kept_in.as_deref(), state.name.clone());
                }
            });
        if let Err(error) = spawned {
            state.restarting = false;
            tracing::warn!(%error, "the media server could not be started again under its new name");
        }
    }

    pub(crate) fn leave(&self) {
        let mut state = self.state.lock();
        state.wanted = false;
        drop(state.server.take());
    }
}

#[cfg(feature = "ui")]
fn started(
    library: &Arc<Library>,
    kept_in: Option<&Path>,
    name: Option<ServerName>,
) -> Option<MediaServer<Shelves>> {
    let udn = kept_in.map_or_else(Udn::minted, kept_udn);
    match MediaServer::start(
        Arc::new(Shelves::over(Arc::clone(library))),
        udn,
        Offering::on_the_network().named(name),
    ) {
        Ok(server) => {
            tracing::info!(
                port = server.port(),
                "the catalog is offered on the network"
            );
            Some(server)
        }
        Err(error) => {
            tracing::warn!(%error, "the catalog could not be offered on the network");
            None
        }
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

    fn name(&self) -> String {
        self.named()
    }

    fn rename(&self, typed: &str) {
        Self::rename(self, typed);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        io::{self, Read as _, Write as _},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
        time::Duration,
    };

    use resonate_library::Cut;
    use resonate_upnp::{MediaServer, Offering};

    use super::*;
    use crate::fixtures::{
        ALBUM, ARTIST, TITLE, scanned, scratch, silent_wave, sounding_wave, tagged_wave,
    };

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

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
    fn the_shelves_page_every_song_by_key_and_a_sheets_cuts_among_them() {
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
        let rows = library.tracks(&TrackQuery::default()).expect("the rows");
        let shelves = Shelves::over(Arc::clone(&library));

        let (by_ones, total) = shelved_songs(&shelves, 1);
        let (by_twos, _) = shelved_songs(&shelves, 2);
        let cut = rows
            .iter()
            .find(|track| track.span.is_some())
            .expect("a sheet's cut");
        let first = rows
            .iter()
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

        assert_eq!(total, 5);
        assert_eq!(by_ones, ["Alpha", "Bravo", "Charlie", "Cut one", "Cut two"]);
        assert_eq!(by_twos, by_ones);
        assert_eq!(
            shelves
                .song(cut.id)
                .expect("read")
                .map(|song| (song.kind, song.content)),
            Some((AudioKind::Wave, Content::Converted))
        );
        assert_eq!(
            shelves
                .song(first.id)
                .expect("read")
                .map(|song| (song.kind, song.content)),
            Some((AudioKind::Wave, Content::Untouched))
        );
        assert_eq!(
            bytes,
            tagged_wave(&[(TITLE, "Alpha"), (ARTIST, "Band"), (ALBUM, "Record")])
        );
        assert_eq!(
            shelves.song(first.id).expect("read").map(|song| song.album),
            Some(Some("Record".to_owned()))
        );
    }

    const SHEET_RATE: u32 = 44_100;

    const SHEET_FRAMES: u64 = 88_200;

    const SECOND_CUT_FROM: u64 = 23_520;

    const WAVE_HEADER_BYTES: usize = 44;

    const STEREO_S16_FRAME_BYTES: usize = 4;

    const ASKED_WITHIN: Duration = Duration::from_secs(5);

    const DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

    fn asked(port: u16, request: &str) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(SocketAddr::new(LOOPBACK, port)).expect("connects");
        stream
            .set_read_timeout(Some(ASKED_WITHIN))
            .expect("a read timeout");
        stream.write_all(request.as_bytes()).expect("asks");
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

    fn got(port: u16, path: &str, range: Option<&str>) -> (String, Vec<u8>) {
        let range = range.map_or_else(String::new, |range| format!("Range: bytes={range}\r\n"));
        asked(
            port,
            &format!(
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{range}Connection: close\r\n\r\n"
            ),
        )
    }

    fn described(port: u16, song: TrackId) -> String {
        let body = format!(
            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:Browse xmlns:u=\"{DIRECTORY}\"><ObjectID>song-{song}</ObjectID><BrowseFlag>BrowseMetadata</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"
        );
        let (head, answer) = asked(
            port,
            &format!(
                "POST /control/ContentDirectory HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{DIRECTORY}#Browse\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        String::from_utf8(answer).expect("text")
    }

    fn decoded(library: &Library, cut: &Track) -> Vec<u8> {
        let (mut decoder, info) =
            Decoder::open_span(&library.sources(), &cut.location, cut.span.expect("a cut"))
                .expect("the cut decodes");
        let mut block = AudioBuffer::empty(info.spec);
        let mut bytes = Vec::new();
        while decoder.next_block(&mut block).expect("a block") == DecodeStatus::Decoded {
            bytes.extend_from_slice(block.as_bytes());
        }
        bytes
    }

    fn header_word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    fn header_half(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    #[test]
    fn a_cut_is_served_as_a_wave_of_its_decode_and_a_range_answers_its_frames() {
        let root = scratch("offered-cut");
        let sheet_file = sounding_wave(20);
        fs::write(root.join("whole.wav"), &sheet_file).expect("a sheet's file");
        fs::write(root.join("whole.cue"), CUT).expect("a sheet");
        let library = Arc::new(scanned(&root));
        let mut cuts: Vec<Track> = library
            .tracks(&TrackQuery::default())
            .expect("the rows")
            .into_iter()
            .filter(|track| track.span.is_some())
            .collect();
        cuts.sort_by_key(|track| track.span.map(FrameSpan::start));
        let told = UdpSocket::bind(SocketAddr::new(LOOPBACK, 0)).expect("a socket told of it");
        let server = MediaServer::start(
            Arc::new(Shelves::over(Arc::clone(&library))),
            Udn::minted(),
            Offering {
                heard_on: SocketAddr::new(LOOPBACK, 0),
                told: told.local_addr().expect("an address"),
                every: Duration::from_secs(60),
                serving_on: LOOPBACK,
                name: None,
            },
        )
        .expect("the server");
        let port = server.port();
        let source = &sheet_file[WAVE_HEADER_BYTES..];

        let mut served = Vec::new();
        for cut in &cuts {
            let decode = decoded(&library, cut);
            let path = format!("/resonate/song-{}.wav", cut.id);
            let (head, body) = got(port, &path, None);
            let wanted_from = WAVE_HEADER_BYTES + 1_000 * STEREO_S16_FRAME_BYTES;
            let wanted_to = WAVE_HEADER_BYTES + 1_100 * STEREO_S16_FRAME_BYTES - 1;
            let (part_head, part) = got(port, &path, Some(&format!("{wanted_from}-{wanted_to}")));
            let (mid_frame_head, mid_frame) = got(
                port,
                &path,
                Some(&format!(
                    "{}-",
                    WAVE_HEADER_BYTES + 3 * STEREO_S16_FRAME_BYTES + 1
                )),
            );
            served.push((
                cut.span.map(FrameSpan::start),
                decode,
                head,
                body,
                part_head,
                part,
                mid_frame_head,
                mid_frame,
                described(port, cut.id),
            ));
        }
        drop(server);
        let _ = fs::remove_dir_all(&root);

        assert_eq!(served.len(), 2);
        for (start, decode, head, body, part_head, part, mid_frame_head, mid_frame, didl) in served
        {
            let start = start.expect("a cut").get();
            let frames = if start == 0 {
                SECOND_CUT_FROM
            } else {
                SHEET_FRAMES - SECOND_CUT_FROM
            };
            let data_bytes = frames as usize * STEREO_S16_FRAME_BYTES;
            let length = WAVE_HEADER_BYTES + data_bytes;
            let from = start as usize * STEREO_S16_FRAME_BYTES;
            let millis = frames * 1_000 / u64::from(SHEET_RATE);

            assert!(head.starts_with("HTTP/1.1 200"), "{head}");
            assert!(head.contains("Content-Type: audio/wav\r\n"), "{head}");
            assert!(
                head.contains(&format!("Content-Length: {length}\r\n")),
                "{head}"
            );
            assert!(head.contains("Accept-Ranges: bytes\r\n"), "{head}");
            assert!(
                head.contains("contentFeatures.dlna.org: DLNA.ORG_OP=01;DLNA.ORG_CI=1;"),
                "{head}"
            );
            assert_eq!(body.len(), length);
            assert_eq!(&body[..4], b"RIFF");
            assert_eq!(header_word(&body, 4) as usize, length - 8);
            assert_eq!(&body[8..16], b"WAVEfmt ");
            assert_eq!(header_half(&body, 20), 1);
            assert_eq!(header_half(&body, 22), 2);
            assert_eq!(header_word(&body, 24), SHEET_RATE);
            assert_eq!(header_half(&body, 32), STEREO_S16_FRAME_BYTES as u16);
            assert_eq!(header_half(&body, 34), 16);
            assert_eq!(&body[36..40], b"data");
            assert_eq!(header_word(&body, 40) as usize, data_bytes);
            assert_eq!(body[WAVE_HEADER_BYTES..], decode[..]);
            assert_eq!(body[WAVE_HEADER_BYTES..], source[from..from + data_bytes]);

            assert!(part_head.starts_with("HTTP/1.1 206"), "{part_head}");
            assert!(
                part_head.contains(&format!(
                    "Content-Range: bytes {}-{}/{length}\r\n",
                    WAVE_HEADER_BYTES + 1_000 * STEREO_S16_FRAME_BYTES,
                    WAVE_HEADER_BYTES + 1_100 * STEREO_S16_FRAME_BYTES - 1
                )),
                "{part_head}"
            );
            assert_eq!(
                part,
                decode[1_000 * STEREO_S16_FRAME_BYTES..1_100 * STEREO_S16_FRAME_BYTES]
            );
            assert!(
                mid_frame_head.starts_with("HTTP/1.1 206"),
                "{mid_frame_head}"
            );
            assert_eq!(mid_frame, decode[3 * STEREO_S16_FRAME_BYTES + 1..]);

            assert!(
                didl.contains(
                    "protocolInfo=&quot;http-get:*:audio/wav:DLNA.ORG_OP=01;DLNA.ORG_CI=1;"
                ),
                "{didl}"
            );
            assert!(
                didl.contains(&format!("size=&quot;{length}&quot;")),
                "{didl}"
            );
            assert!(
                didl.contains(&format!(
                    "duration=&quot;0:00:0{}.{:03}&quot;",
                    millis / 1_000,
                    millis % 1_000
                )),
                "{didl}"
            );
            assert!(didl.contains("bitsPerSample=&quot;16&quot;"), "{didl}");
            assert!(didl.contains(".wav&lt;/res&gt;"), "{didl}");
        }
    }

    const RATE_FIELD: usize = 24;

    const BYTE_RATE_FIELD: usize = 28;

    const ANOTHER_RATE: u32 = 48_000;

    #[test]
    fn a_cut_that_decodes_otherwise_than_its_row_says_is_not_served() {
        let root = scratch("offered-reformatted");
        fs::write(root.join("whole.wav"), sounding_wave(20)).expect("a sheet's file");
        fs::write(root.join("whole.cue"), CUT).expect("a sheet");
        let library = Arc::new(scanned(&root));
        let cut = library
            .tracks(&TrackQuery::default())
            .expect("the rows")
            .into_iter()
            .find(|track| track.span.is_some())
            .expect("a cut");
        let shelves = Shelves::over(Arc::clone(&library));

        let served_as_scanned = shelves.bytes_of(cut.id).expect("read").is_some();
        let mut retold = sounding_wave(20);
        retold[RATE_FIELD..RATE_FIELD + 4].copy_from_slice(&ANOTHER_RATE.to_le_bytes());
        retold[BYTE_RATE_FIELD..BYTE_RATE_FIELD + 4]
            .copy_from_slice(&(ANOTHER_RATE * STEREO_S16_FRAME_BYTES as u32).to_le_bytes());
        fs::write(root.join("whole.wav"), retold).expect("the file rewritten");
        let decoded = Decoding::open(
            Arc::new(library.sources()),
            cut.location.clone(),
            cut.span.expect("a span"),
        )
        .map(|decoding| decoding.spec.rate.hz());
        let served_once_rewritten = shelves.bytes_of(cut.id).expect("read").is_some();
        let announced = shelves
            .song(cut.id)
            .expect("read")
            .map(|song| (song.rate, song.size));
        let _ = fs::remove_dir_all(&root);

        assert!(served_as_scanned);
        assert_eq!(decoded.ok(), Some(ANOTHER_RATE));
        assert!(!served_once_rewritten);
        assert_eq!(
            announced,
            Some((
                SHEET_RATE,
                Some(
                    (WAVE_HEADER_BYTES + SECOND_CUT_FROM as usize * STEREO_S16_FRAME_BYTES) as u64
                )
            ))
        );
    }

    const SHEET_BY_A_BAND: &str = "PERFORMER \"Sheet Band\"\nTITLE \"Sheet Album\"\nFILE \"whole.wav\" \
                                   WAVE\n  TRACK 01 AUDIO\n    TITLE \"Cut one\"\n    INDEX 01 \
                                   00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Cut two\"\n    INDEX 01 \
                                   00:00:40\n";

    #[test]
    fn a_search_under_artists_or_albums_finds_a_sheets_cuts_on_their_album_and_by_their_band() {
        let root = scratch("offered-nested-cuts");
        fs::write(root.join("whole.wav"), sounding_wave(20)).expect("a sheet's file");
        fs::write(root.join("whole.cue"), SHEET_BY_A_BAND).expect("a sheet");
        fs::write(root.join("loose.wav"), tagged_wave(&[(TITLE, "Loose")])).expect("a song");
        let shelves = Shelves::over(Arc::new(scanned(&root)));
        let _ = fs::remove_dir_all(&root);
        let every = Page::first(10);
        let everything = Query::Everything;

        let under_the_albums = found(&shelves, Kind::Song, Within::Albums, &everything, every);
        let under_the_artists = found(&shelves, Kind::Song, Within::Artists, &everything, every);
        let albums = found(&shelves, Kind::Album, Within::Albums, &everything, every);

        assert_eq!(
            under_the_albums,
            (vec!["Cut one".to_owned(), "Cut two".to_owned()], 2)
        );
        assert_eq!(under_the_artists, under_the_albums);
        assert_eq!(albums, (vec!["Sheet Album".to_owned()], 1));
    }

    const GENRE: &[u8; 4] = b"TCON";

    fn text(field: Field, test: Test, text: &str) -> Query {
        Query::Text {
            field,
            test,
            text: text.to_owned(),
        }
    }

    fn titles(searched: Searched) -> (Vec<String>, u32) {
        let Searched::Found(shelved) = searched else {
            panic!("the search was not answered");
        };
        let titles = shelved
            .held
            .into_iter()
            .map(|entry| match entry {
                Entry::Song(song) => song.title,
                Entry::Shelf(held) => held.title,
            })
            .collect();
        (titles, shelved.total)
    }

    #[test]
    fn a_search_from_the_network_is_read_through_the_librarys_own_folded_search() {
        let root = scratch("offered-search");
        for (file, title, artist, album, genre) in [
            ("a.wav", "Human Behaviour", "Björk", "Debut", "Pop"),
            ("b.wav", "Army of Me", "Bjork", "Post", "Electronic"),
            ("c.wav", "Hyperballad", "Other", "Rest", "Pop"),
        ] {
            fs::write(
                root.join(file),
                tagged_wave(&[
                    (TITLE, title),
                    (ARTIST, artist),
                    (ALBUM, album),
                    (GENRE, genre),
                ]),
            )
            .expect("a song");
        }
        let library = Arc::new(scanned(&root));
        let shelves = Shelves::over(Arc::clone(&library));
        let every = Page::first(10);
        let searched = |kind: Kind, query: &Query, page: Page| {
            shelves
                .searched(kind, Within::Catalog, query, page)
                .expect("searched")
        };

        let by_the_artist = titles(searched(
            Kind::Song,
            &text(Field::Artist, Test::Is, "BJORK"),
            every,
        ));
        let second_page = titles(searched(
            Kind::Song,
            &text(Field::Artist, Test::Is, "björk"),
            Page { from: 1, most: 1 },
        ));
        let begun = titles(searched(
            Kind::Song,
            &text(Field::Title, Test::Contains, "behav"),
            every,
        ));
        let but_not = titles(searched(
            Kind::Song,
            &Query::All(vec![
                text(Field::Artist, Test::Contains, "bjork"),
                text(Field::Title, Test::Lacks, "army"),
            ]),
            every,
        ));
        let either = titles(searched(
            Kind::Song,
            &Query::Any(vec![
                text(Field::Genre, Test::Is, "electronic"),
                text(Field::Album, Test::Contains, "rest"),
            ]),
            every,
        ));
        let everything = titles(searched(Kind::Song, &Query::Everything, Page::counting()));
        let albums = titles(searched(
            Kind::Album,
            &text(Field::Title, Test::Contains, "post"),
            every,
        ));
        let artists = titles(searched(
            Kind::Artist,
            &text(Field::Title, Test::Contains, "bjo"),
            every,
        ));
        let unanswered = [
            (
                Kind::Song,
                Query::Present {
                    field: Field::Genre,
                    held: true,
                },
            ),
            (Kind::Song, text(Field::Title, Test::Contains, "!!!")),
            (Kind::Album, text(Field::Title, Test::Lacks, "post")),
            (Kind::Artist, text(Field::Title, Test::IsNot, "other")),
            (Kind::Artist, text(Field::Album, Test::Is, "post")),
            (Kind::Playlist, text(Field::Title, Test::Is, "mix")),
        ]
        .map(|(kind, query)| searched(kind, &query, every));
        let _ = fs::remove_dir_all(&root);

        assert_eq!(by_the_artist.1, 2);
        assert_eq!(by_the_artist.0, ["Army of Me", "Human Behaviour"]);
        assert_eq!(second_page, (vec!["Human Behaviour".to_owned()], 2));
        assert_eq!(begun.0, ["Human Behaviour"]);
        assert_eq!(but_not.0, ["Human Behaviour"]);
        assert_eq!(either.0, ["Army of Me", "Hyperballad"]);
        assert_eq!(everything, (Vec::new(), 3));
        assert_eq!(albums.0, ["Post"]);
        assert_eq!(artists.1, 1);
        assert!(
            unanswered
                .iter()
                .all(|searched| *searched == Searched::Unanswered)
        );
    }

    const TRACK_NUMBER: &[u8; 4] = b"TRCK";

    fn found(
        shelves: &Shelves,
        kind: Kind,
        within: Within,
        query: &Query,
        page: Page,
    ) -> (Vec<String>, u32) {
        titles(
            shelves
                .searched(kind, within, query, page)
                .expect("searched"),
        )
    }

    #[test]
    fn a_search_under_an_artist_an_album_or_a_playlist_answers_what_it_holds_in_its_order() {
        let root = scratch("offered-within");
        for (file, title, artist, album, number) in [
            ("a.wav", "Human Behaviour", "Björk", "Debut", "1"),
            ("b.wav", "Venus as a Boy", "Björk", "Debut", "2"),
            ("c.wav", "Army of Me", "Bjork", "Post", "1"),
            ("d.wav", "Hyperballad", "Other", "Rest", "1"),
        ] {
            fs::write(
                root.join(file),
                tagged_wave(&[
                    (TITLE, title),
                    (ARTIST, artist),
                    (ALBUM, album),
                    (TRACK_NUMBER, number),
                ]),
            )
            .expect("a song");
        }
        let library = Arc::new(scanned(&root));
        let shelves = Shelves::over(Arc::clone(&library));

        let rows = library.tracks(&TrackQuery::default()).expect("the rows");
        let row = |title: &str| {
            rows.iter()
                .find(|track| track.title == title)
                .expect("a song")
        };
        let artist_of = |title: &str| row(title).artist_id.expect("an artist");
        let bjork = artist_of("Army of Me");
        let other = artist_of("Hyperballad");
        let debut = row("Human Behaviour").album_id.expect("an album");
        let mix = library.create_playlist("Mix").expect("a playlist");
        library
            .add_to_playlist(
                mix,
                &["Hyperballad", "Army of Me", "Human Behaviour"].map(|title| Cut::of(row(title))),
            )
            .expect("listed");
        let every = Page::first(10);
        let army = text(Field::Title, Test::Contains, "army");
        let by_bjork = text(Field::Artist, Test::Is, "björk");

        let her_albums = found(
            &shelves,
            Kind::Album,
            Within::Artist(bjork),
            &Query::Everything,
            every,
        );
        let her_post = found(
            &shelves,
            Kind::Album,
            Within::Artist(bjork),
            &text(Field::Title, Test::Contains, "post"),
            every,
        );
        let her_songs = found(
            &shelves,
            Kind::Song,
            Within::Artist(bjork),
            &Query::Everything,
            every,
        );
        let her_second_song = found(
            &shelves,
            Kind::Song,
            Within::Artist(bjork),
            &Query::Everything,
            Page { from: 1, most: 1 },
        );
        let her_army = found(&shelves, Kind::Song, Within::Artist(bjork), &army, every);
        let others_army = found(&shelves, Kind::Song, Within::Artist(other), &army, every);
        let on_debut = found(
            &shelves,
            Kind::Song,
            Within::Album(debut),
            &Query::Everything,
            every,
        );
        let second_on_debut = found(
            &shelves,
            Kind::Song,
            Within::Album(debut),
            &Query::Everything,
            Page { from: 1, most: 1 },
        );
        let venus_on_debut = found(
            &shelves,
            Kind::Song,
            Within::Album(debut),
            &text(Field::Title, Test::Contains, "venus"),
            every,
        );
        let army_on_debut = found(&shelves, Kind::Song, Within::Album(debut), &army, every);
        let in_the_mix = found(
            &shelves,
            Kind::Song,
            Within::Playlist(mix),
            &Query::Everything,
            every,
        );
        let hers_in_the_mix = found(
            &shelves,
            Kind::Song,
            Within::Playlist(mix),
            &by_bjork,
            every,
        );
        let second_of_hers_in_the_mix = found(
            &shelves,
            Kind::Song,
            Within::Playlist(mix),
            &by_bjork,
            Page { from: 1, most: 1 },
        );
        let counted_in_the_mix = found(
            &shelves,
            Kind::Song,
            Within::Playlist(mix),
            &by_bjork,
            Page::counting(),
        );
        let no_artist_on_debut = found(
            &shelves,
            Kind::Artist,
            Within::Album(debut),
            &Query::Everything,
            every,
        );
        let no_album_in_the_mix = found(
            &shelves,
            Kind::Album,
            Within::Playlist(mix),
            &Query::Everything,
            every,
        );
        let _ = fs::remove_dir_all(&root);

        assert_eq!(bjork, artist_of("Human Behaviour"));
        assert_ne!(bjork, other);
        assert_eq!(her_albums.1, 2);
        assert_eq!(
            her_albums.0.iter().collect::<BTreeSet<_>>(),
            ["Debut".to_owned(), "Post".to_owned()].iter().collect()
        );
        assert_eq!(her_post, (vec!["Post".to_owned()], 1));
        assert_eq!(her_songs.1, 3);
        assert_eq!(her_songs.0.len(), 3);
        assert!(!her_songs.0.contains(&"Hyperballad".to_owned()));
        assert_eq!(her_second_song, (vec![her_songs.0[1].clone()], 3));
        assert_eq!(her_army, (vec!["Army of Me".to_owned()], 1));
        assert_eq!(others_army, (Vec::new(), 0));
        assert_eq!(
            on_debut,
            (
                vec!["Human Behaviour".to_owned(), "Venus as a Boy".to_owned()],
                2
            )
        );
        assert_eq!(second_on_debut, (vec!["Venus as a Boy".to_owned()], 2));
        assert_eq!(venus_on_debut, (vec!["Venus as a Boy".to_owned()], 1));
        assert_eq!(army_on_debut, (Vec::new(), 0));
        assert_eq!(
            in_the_mix,
            (
                vec![
                    "Hyperballad".to_owned(),
                    "Army of Me".to_owned(),
                    "Human Behaviour".to_owned()
                ],
                3
            )
        );
        assert_eq!(
            hers_in_the_mix,
            (
                vec!["Army of Me".to_owned(), "Human Behaviour".to_owned()],
                2
            )
        );
        assert_eq!(
            second_of_hers_in_the_mix,
            (vec!["Human Behaviour".to_owned()], 2)
        );
        assert_eq!(counted_in_the_mix, (Vec::new(), 2));
        assert_eq!(no_artist_on_debut, (Vec::new(), 0));
        assert_eq!(no_album_in_the_mix, (Vec::new(), 0));
    }

    #[test]
    fn a_search_under_artists_or_albums_finds_what_is_nested_below_and_nothing_loose() {
        let root = scratch("offered-nested");
        for (file, tags) in [
            (
                "a.wav",
                &[
                    (TITLE, "Human Behaviour"),
                    (ARTIST, "Björk"),
                    (ALBUM, "Debut"),
                ][..],
            ),
            (
                "b.wav",
                &[(TITLE, "Army of Me"), (ARTIST, "Björk"), (ALBUM, "Post")][..],
            ),
            ("c.wav", &[(TITLE, "Demo"), (ARTIST, "Björk")][..]),
            ("d.wav", &[(TITLE, "Stray"), (ALBUM, "Loose Ends")][..]),
            ("e.wav", &[(TITLE, "Nobody")][..]),
        ] {
            fs::write(root.join(file), tagged_wave(tags)).expect("a song");
        }
        let library = Arc::new(scanned(&root));
        let shelves = Shelves::over(Arc::clone(&library));
        let every = Page::first(10);
        let everything = Query::Everything;
        let army = text(Field::Title, Test::Contains, "army");

        let artists = found(&shelves, Kind::Artist, Within::Artists, &everything, every);
        let albums_under_the_artists =
            found(&shelves, Kind::Album, Within::Artists, &everything, every);
        let songs_under_the_artists =
            found(&shelves, Kind::Song, Within::Artists, &everything, every);
        let albums = found(&shelves, Kind::Album, Within::Albums, &everything, every);
        let songs_under_the_albums =
            found(&shelves, Kind::Song, Within::Albums, &everything, every);
        let second_song_under_the_albums = found(
            &shelves,
            Kind::Song,
            Within::Albums,
            &everything,
            Page { from: 1, most: 1 },
        );
        let army_under_the_albums = found(&shelves, Kind::Song, Within::Albums, &army, every);
        let counted_under_the_artists = found(
            &shelves,
            Kind::Song,
            Within::Artists,
            &everything,
            Page::counting(),
        );
        let no_playlist_under_the_albums =
            found(&shelves, Kind::Playlist, Within::Albums, &everything, every);
        let no_artist_under_the_albums =
            found(&shelves, Kind::Artist, Within::Albums, &everything, every);
        let every_song = found(&shelves, Kind::Song, Within::Catalog, &everything, every);
        let _ = fs::remove_dir_all(&root);

        assert_eq!(artists, (vec!["Björk".to_owned()], 1));
        assert_eq!(
            albums_under_the_artists,
            (vec!["Debut".to_owned(), "Post".to_owned()], 2)
        );
        assert_eq!(
            songs_under_the_artists,
            (
                vec![
                    "Army of Me".to_owned(),
                    "Demo".to_owned(),
                    "Human Behaviour".to_owned()
                ],
                3
            )
        );
        assert_eq!(
            albums,
            (
                vec![
                    "Debut".to_owned(),
                    "Loose Ends".to_owned(),
                    "Post".to_owned()
                ],
                3
            )
        );
        assert_eq!(
            songs_under_the_albums,
            (
                vec![
                    "Army of Me".to_owned(),
                    "Human Behaviour".to_owned(),
                    "Stray".to_owned()
                ],
                3
            )
        );
        assert_eq!(
            second_song_under_the_albums,
            (vec!["Human Behaviour".to_owned()], 3)
        );
        assert_eq!(army_under_the_albums, (vec!["Army of Me".to_owned()], 1));
        assert_eq!(counted_under_the_artists, (Vec::new(), 3));
        assert_eq!(no_playlist_under_the_albums, (Vec::new(), 0));
        assert_eq!(no_artist_under_the_albums, (Vec::new(), 0));
        assert_eq!(every_song.1, 5);
    }

    #[test]
    fn a_query_from_the_network_is_written_as_a_search_that_reads_back_as_itself() {
        for query in [
            text(Field::Title, Test::Contains, "Say \"hi\" =now"),
            text(Field::Artist, Test::Is, "AC/DC \"live\""),
            Query::Any(vec![
                text(Field::Title, Test::Lacks, "one two"),
                Query::All(vec![
                    text(Field::Genre, Test::IsNot, "Rock"),
                    text(Field::Album, Test::Contains, "-or"),
                ]),
            ]),
        ] {
            let written = searched_as(&query, song_column, Denials::Read)
                .flatten()
                .expect("a search");

            assert_eq!(Search::read(&written).to_string(), written, "{query:?}");
            assert!(!written.contains("\"\""), "{written}");
        }
        let many = Query::Any(
            (0..ALTERNATIVES_AT_MOST + 1)
                .map(|at| text(Field::Title, Test::Is, &format!("word{at}")))
                .collect(),
        );

        assert_eq!(searched_as(&many, song_column, Denials::Read), None);
        assert_eq!(
            searched_as(&Query::Everything, song_column, Denials::Read),
            Some(None)
        );
    }

    #[test]
    fn a_playlist_that_names_a_song_twice_pages_every_row_it_counts() {
        let root = scratch("offered-repeat");
        for (file, title) in [("a.wav", "Alpha"), ("b.wav", "Bravo")] {
            fs::write(
                root.join(file),
                tagged_wave(&[(TITLE, title), (ARTIST, "Band"), (ALBUM, "Record")]),
            )
            .expect("a song");
        }
        let library = Arc::new(scanned(&root));
        let shelves = Shelves::over(Arc::clone(&library));

        let rows = library.tracks(&TrackQuery::default()).expect("the rows");
        let row = |title: &str| {
            rows.iter()
                .find(|track| track.title == title)
                .expect("a song")
        };
        let mix = library.create_playlist("Twice").expect("a playlist");
        library
            .add_to_playlist(
                mix,
                &["Alpha", "Bravo", "Alpha"].map(|title| Cut::of(row(title))),
            )
            .expect("listed");

        let titles_of = |page: Page| {
            let shelved = shelves
                .shelved(Shelf::Playlist(mix), page)
                .expect("a page")
                .expect("the shelf");
            let titles: Vec<String> = shelved
                .held
                .into_iter()
                .filter_map(|entry| match entry {
                    Entry::Song(song) => Some(song.title),
                    Entry::Shelf(_) => None,
                })
                .collect();
            (titles, shelved.total)
        };
        let whole = titles_of(Page::first(10));
        let first_page = titles_of(Page { from: 0, most: 2 });
        let second_page = titles_of(Page { from: 2, most: 2 });
        let _ = fs::remove_dir_all(&root);

        assert_eq!(
            whole,
            (vec!["Alpha".into(), "Bravo".into(), "Alpha".into()], 3)
        );
        assert_eq!(first_page, (vec!["Alpha".into(), "Bravo".into()], 3));
        assert_eq!(second_page, (vec!["Alpha".into()], 3));
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
