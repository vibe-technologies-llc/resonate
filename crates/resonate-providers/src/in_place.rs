use std::{
    fmt,
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{
    MediaLocation, Recording, SourceId, folded_letters,
    titles::{self, Rating},
};

use crate::{
    error::{Error, ProviderOp, Result},
    listing::A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY,
};

const SONG_KEY: &str = "track/";
const ALBUM_KEY: &str = "album/";
const ARTIST_KEY: &str = "artist/";
const MIX_KEY: &str = "mix/";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(Box<str>);

impl Id {
    pub fn new(id: &str) -> Option<Self> {
        let id = id.trim();
        (!id.is_empty()).then(|| Self(Box::from(id)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    Song(Id),
    Album(Id),
    Artist(Id),
    Mix(Id),
}

impl Named {
    pub fn read(location: &MediaLocation, source: &SourceId) -> Option<Self> {
        if location.source() != source {
            return None;
        }
        let key = location.locator().as_key()?;
        [
            (SONG_KEY, Self::Song as fn(Id) -> Self),
            (ALBUM_KEY, Self::Album),
            (ARTIST_KEY, Self::Artist),
            (MIX_KEY, Self::Mix),
        ]
        .into_iter()
        .find_map(|(prefix, named)| key.strip_prefix(prefix).and_then(Id::new).map(named))
    }

    pub fn location(&self, source: &SourceId) -> MediaLocation {
        let (prefix, id) = match self {
            Self::Song(id) => (SONG_KEY, id),
            Self::Album(id) => (ALBUM_KEY, id),
            Self::Artist(id) => (ARTIST_KEY, id),
            Self::Mix(id) => (MIX_KEY, id),
        };
        MediaLocation::new(source.clone(), format!("{prefix}{id}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artist {
    pub id: Id,
    pub name: String,
    pub albums: Option<u32>,
    pub cover: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Album {
    pub id: Id,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub songs: Option<u32>,
    pub cover: Option<Id>,
    pub rating: Rating,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mix {
    pub id: Id,
    pub name: String,
    pub subtitle: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub id: Id,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub disc: Option<u32>,
    pub track: Option<u32>,
    pub length: Option<Duration>,
    pub suffix: Option<String>,
    pub cover: Option<Id>,
    pub favourite: Option<bool>,
    pub rating: Rating,
}

impl Song {
    pub fn only(id: Id) -> Self {
        Self {
            id,
            title: String::new(),
            artist: None,
            album: None,
            disc: None,
            track: None,
            length: None,
            suffix: None,
            cover: None,
            favourite: None,
            rating: Rating::Unmarked,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistAlbums {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumSongs {
    pub album: Album,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixSongs {
    pub mix: Mix,
    pub songs: Vec<Song>,
}

fn explicit_first<T>(
    listed: Vec<T>,
    rating: impl Fn(&T) -> Rating,
    same_take: impl Fn(&T, &T) -> bool,
) -> Vec<T> {
    let listed: Vec<T> = listed
        .into_iter()
        .filter(|item| rating(item) != Rating::Clean)
        .collect();
    let places: Vec<(bool, usize)> = listed
        .iter()
        .enumerate()
        .map(|(at, item)| {
            let explicit = rating(item) == Rating::Explicit;
            let stands_aside = !explicit
                && listed
                    .iter()
                    .any(|other| rating(other) == Rating::Explicit && same_take(item, other));
            let place = if explicit {
                (0..at)
                    .find(|earlier| same_take(&listed[*earlier], item))
                    .unwrap_or(at)
            } else {
                at
            };

            (stands_aside, place)
        })
        .collect();

    let mut kept: Vec<(usize, T)> = listed
        .into_iter()
        .zip(places)
        .filter(|(_, (stands_aside, _))| !stands_aside)
        .map(|(item, (_, place))| (place, item))
        .collect();
    kept.sort_by_key(|(place, _)| *place);

    kept.into_iter().map(|(_, item)| item).collect()
}

fn folded_or_nothing(name: Option<&str>) -> String {
    name.map(folded_letters).unwrap_or_default()
}

fn songs_alike(one: &Song, other: &Song) -> bool {
    folded_letters(titles::bare(&one.title)) == folded_letters(titles::bare(&other.title))
        && folded_or_nothing(one.artist.as_deref()) == folded_or_nothing(other.artist.as_deref())
        && match (one.length, other.length) {
            (Some(one), Some(other)) => one.abs_diff(other) <= A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY,
            _ => true,
        }
}

fn albums_alike(one: &Album, other: &Album) -> bool {
    folded_letters(titles::bare(&one.name)) == folded_letters(titles::bare(&other.name))
        && folded_or_nothing(one.artist.as_deref()) == folded_or_nothing(other.artist.as_deref())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
    pub mixes: Vec<Mix>,
}

impl Found {
    pub fn explicit_first(self) -> Self {
        Self {
            albums: explicit_first(self.albums, |album| album.rating, albums_alike),
            songs: explicit_first(self.songs, |song| song.rating, songs_alike),
            ..self
        }
    }
}

pub trait SeekingBytes: Read + Seek + Send {}

impl<T: Read + Seek + Send> SeekingBytes for T {}

enum Bytes {
    Seeking(Mutex<Box<dyn SeekingBytes>>),
    Onward(Mutex<Box<dyn Read + Send>>),
}

pub struct Streamed {
    bytes: Bytes,
    length: Option<u64>,
    pub media_type: Option<String>,
}

impl Streamed {
    pub fn seeking(
        reader: impl Read + Seek + Send + 'static,
        length: Option<u64>,
        media_type: Option<String>,
    ) -> Self {
        Self {
            bytes: Bytes::Seeking(Mutex::new(Box::new(reader))),
            length,
            media_type,
        }
    }

    pub fn onward(reader: impl Read + Send + 'static, media_type: Option<String>) -> Self {
        Self {
            bytes: Bytes::Onward(Mutex::new(Box::new(reader))),
            length: None,
            media_type,
        }
    }

    pub const fn length(&self) -> Option<u64> {
        self.length
    }

    pub const fn is_seekable(&self) -> bool {
        matches!(self.bytes, Bytes::Seeking(_)) && self.length.is_some()
    }
}

impl Read for Streamed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.bytes {
            Bytes::Seeking(reader) => reader.get_mut().read(buf),
            Bytes::Onward(reader) => reader.get_mut().read(buf),
        }
    }
}

impl Seek for Streamed {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match &mut self.bytes {
            Bytes::Seeking(reader) => reader.get_mut().seek(to),
            Bytes::Onward(_) => Err(io::Error::from(ErrorKind::Unsupported)),
        }
    }
}

pub trait PlaysInPlace: Send + Sync {
    fn scheme(&self) -> &SourceId;

    fn title(&self) -> &'static str;

    fn artists(&self) -> Result<Vec<Artist>>;

    fn artist(&self, id: &Id) -> Result<ArtistAlbums>;

    fn album(&self, id: &Id) -> Result<AlbumSongs>;

    fn search(&self, words: &str) -> Result<Found>;

    fn mix(&self, _id: &Id) -> Result<MixSongs> {
        Err(not_offered(self.scheme(), ProviderOp::Browse))
    }

    fn streamed(&self, song: &Id) -> Result<Streamed>;

    fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>>;

    fn cover_of(&self, named: &Named) -> Result<Id>;

    fn favourites(&self) -> Result<Found> {
        Ok(Found::default())
    }

    fn keeps_favourites(&self) -> bool {
        false
    }

    fn plays_songs(&self) -> bool {
        true
    }

    fn keeps_songs(&self) -> bool {
        false
    }

    fn favour(&self, _song: &Id, _favoured: bool) -> Result<()> {
        Err(not_offered(self.scheme(), ProviderOp::Favourite))
    }

    fn first_page(&self) -> Result<Found> {
        let artists = self.artists()?;
        if artists.is_empty() {
            return self.favourites();
        }
        Ok(Found {
            artists,
            ..Found::default()
        })
    }

    fn songs_of(&self, named: &Named) -> Result<Vec<Song>> {
        match named {
            Named::Song(id) => Ok(vec![Song::only(id.clone())]),
            Named::Album(id) => Ok(self.album(id)?.songs),
            Named::Artist(id) => {
                let mut songs = Vec::new();
                for album in self.artist(id)?.albums {
                    songs.extend(self.album(&album.id)?.songs);
                }
                Ok(songs)
            }
            Named::Mix(id) => Ok(self.mix(id)?.songs),
        }
    }

    fn named_by(&self, location: &MediaLocation) -> Option<Named> {
        Named::read(location, self.scheme())
    }

    fn location_of(&self, named: &Named) -> MediaLocation {
        named.location(self.scheme())
    }
}

pub fn not_offered(provider: &SourceId, op: ProviderOp) -> Error {
    Error::NotOffered {
        provider: provider.clone(),
        op,
    }
}

const THE_SAME_SONG_LASTS_WITHIN: Duration = Duration::from_secs(3);

const SEARCH_RESULTS_LOOKED_AT: usize = 20;

pub fn found_exactly(
    services: &[Arc<dyn PlaysInPlace>],
    recording: &Recording,
) -> Option<MediaLocation> {
    let words = format!("{} {}", recording.artist, recording.title);
    let title = folded_letters(&recording.title);
    let artist = folded_letters(&recording.artist);

    let mut played = services.iter().filter(|service| service.plays_songs());
    played.find_map(|service| {
        let found = service.search(&words).ok()?;
        let offered = found
            .songs
            .into_iter()
            .take(SEARCH_RESULTS_LOOKED_AT)
            .find(|offered| {
                folded_letters(&offered.title) == title
                    && offered
                        .artist
                        .as_deref()
                        .is_some_and(|named| folded_letters(named) == artist)
                    && offered
                        .length
                        .zip(recording.length)
                        .is_some_and(|(one, other)| {
                            one.abs_diff(other) <= THE_SAME_SONG_LASTS_WITHIN
                        })
            })?;

        Some(Named::Song(offered.id).location(service.scheme()))
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn a_song(id: &str, title: &str, rating: Rating) -> Song {
        Song {
            title: title.to_owned(),
            artist: Some("Ada".to_owned()),
            length: Some(Duration::from_secs(200)),
            rating,
            ..Song::only(Id::new(id).expect("an id"))
        }
    }

    fn ids_of(found: &Found) -> Vec<&str> {
        found.songs.iter().map(|song| song.id.as_str()).collect()
    }

    #[test]
    fn a_search_never_lists_a_clean_take_and_the_explicit_one_stands_where_its_sibling_did() {
        let found = Found {
            songs: vec![
                a_song("other", "Another Song", Rating::Unmarked),
                a_song("plain", "Bad Guy", Rating::Unmarked),
                a_song("clean", "Bad Guy (Clean)", Rating::Clean),
                a_song("third", "A Third Song", Rating::Unmarked),
                a_song("explicit", "Bad Guy", Rating::Explicit),
            ],
            ..Found::default()
        };

        assert_eq!(
            ids_of(&found.explicit_first()),
            ["other", "explicit", "third"]
        );
    }

    #[test]
    fn a_song_only_ever_marked_clean_is_gone_from_a_search_and_one_unmarked_stays() {
        let found = Found {
            songs: vec![
                a_song("clean", "Only Clean (Clean)", Rating::Clean),
                a_song("plain", "Instrumental", Rating::Unmarked),
            ],
            ..Found::default()
        };

        assert_eq!(ids_of(&found.explicit_first()), ["plain"]);
    }

    #[test]
    fn songs_named_alike_of_other_lengths_are_other_songs_and_both_stay() {
        let mut long = a_song("long", "Bad Guy", Rating::Unmarked);
        long.length = Some(Duration::from_secs(400));
        let found = Found {
            songs: vec![long, a_song("explicit", "Bad Guy", Rating::Explicit)],
            ..Found::default()
        };

        assert_eq!(ids_of(&found.explicit_first()), ["long", "explicit"]);
    }

    #[test]
    fn an_edition_of_an_album_marked_explicit_stands_in_for_the_one_beside_it() {
        let album = |id: &str, rating| Album {
            id: Id::new(id).expect("an id"),
            name: "When We All Fall Asleep".to_owned(),
            artist: Some("Ada".to_owned()),
            year: None,
            songs: None,
            cover: None,
            rating,
        };
        let found = Found {
            albums: vec![
                album("plain", Rating::Unmarked),
                album("explicit", Rating::Explicit),
            ],
            ..Found::default()
        };

        let kept = found.explicit_first();

        assert_eq!(kept.albums.len(), 1);
        assert_eq!(kept.albums[0].id.as_str(), "explicit");
    }

    fn subsonic() -> SourceId {
        SourceId::new("subsonic").expect("a source name")
    }

    #[test]
    fn a_song_album_or_artist_is_named_by_a_location_that_reads_back_as_itself() {
        let id = Id::new("al-7").expect("an id");

        for named in [
            Named::Song(id.clone()),
            Named::Album(id.clone()),
            Named::Artist(id.clone()),
            Named::Mix(id),
        ] {
            let location = named.location(&subsonic());
            let uri = location.to_uri();

            assert_eq!(
                MediaLocation::from_uri(&uri).and_then(|read| Named::read(&read, &subsonic())),
                Some(named)
            );
        }
        assert_eq!(
            Named::read(
                &MediaLocation::from_uri("subsonic:track/42").expect("a uri"),
                &subsonic()
            ),
            Some(Named::Song(Id::new("42").expect("an id")))
        );
        assert_eq!(
            Named::read(
                &MediaLocation::from_uri("tidal:track/42").expect("a uri"),
                &subsonic()
            ),
            None
        );
        assert_eq!(
            Named::read(&MediaLocation::local("/music/track/42"), &subsonic()),
            None
        );
        assert_eq!(
            Named::read(
                &MediaLocation::from_uri("subsonic:playlist/1").expect("a uri"),
                &subsonic()
            ),
            None
        );
    }

    #[test]
    fn a_stream_read_onward_refuses_a_seek_and_says_it_cannot_and_one_of_known_length_seeks() {
        let mut onward = Streamed::onward(Cursor::new(vec![1, 2, 3]), None);
        let mut seeking = Streamed::seeking(Cursor::new(vec![1, 2, 3]), Some(3), None);
        let mut read = [0; 1];

        let refused = onward.seek(SeekFrom::Start(1));
        let landed = seeking.seek(SeekFrom::Start(2)).expect("a seek");
        seeking.read_exact(&mut read).expect("a byte");

        assert!(!onward.is_seekable());
        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(ErrorKind::Unsupported)
        );
        assert!(seeking.is_seekable());
        assert_eq!(landed, 2);
        assert_eq!(read, [3]);
    }

    struct OneSearch {
        scheme: SourceId,
        songs: Vec<Song>,
        plays: bool,
    }

    impl PlaysInPlace for OneSearch {
        fn scheme(&self) -> &SourceId {
            &self.scheme
        }

        fn title(&self) -> &'static str {
            "a search"
        }

        fn artists(&self) -> Result<Vec<Artist>> {
            Err(not_offered(&self.scheme, ProviderOp::Browse))
        }

        fn artist(&self, _id: &Id) -> Result<ArtistAlbums> {
            Err(not_offered(&self.scheme, ProviderOp::Browse))
        }

        fn album(&self, _id: &Id) -> Result<AlbumSongs> {
            Err(not_offered(&self.scheme, ProviderOp::Browse))
        }

        fn plays_songs(&self) -> bool {
            self.plays
        }

        fn search(&self, _words: &str) -> Result<Found> {
            assert!(
                self.plays,
                "a service nothing can be played from was searched"
            );
            Ok(Found {
                artists: Vec::new(),
                albums: Vec::new(),
                songs: self.songs.clone(),
                mixes: Vec::new(),
            })
        }

        fn streamed(&self, _song: &Id) -> Result<Streamed> {
            Err(not_offered(&self.scheme, ProviderOp::Browse))
        }

        fn cover(&self, _cover: &Id, _side: u32) -> Result<Vec<u8>> {
            Err(not_offered(&self.scheme, ProviderOp::Browse))
        }

        fn cover_of(&self, _named: &Named) -> Result<Id> {
            Err(not_offered(&self.scheme, ProviderOp::Browse))
        }
    }

    fn offered(id: &str, title: &str, artist: &str, seconds: Option<u64>) -> Song {
        Song {
            title: title.to_owned(),
            artist: Some(artist.to_owned()),
            length: seconds.map(Duration::from_secs),
            ..Song::only(Id::new(id).expect("an id"))
        }
    }

    fn wanted() -> Recording {
        Recording {
            mbid: None,
            title: "Café Song".to_owned(),
            artist: "The Band".to_owned(),
            album: String::new(),
            length: Some(Duration::from_secs(200)),
        }
    }

    #[test]
    fn only_the_same_title_artist_and_length_is_taken_as_the_song_a_party_plays() {
        let service: Arc<dyn PlaysInPlace> = Arc::new(OneSearch {
            scheme: SourceId::new("subsonic").expect("a scheme"),
            songs: vec![
                offered("other-title", "Cafe Song Remix", "The Band", Some(200)),
                offered("other-artist", "Cafe Song", "Another Band", Some(200)),
                offered("other-length", "Cafe Song", "The Band", Some(260)),
                offered("no-length", "Cafe Song", "The Band", None),
                offered("this-one", "CAFE SONG", "the band", Some(202)),
            ],
            plays: true,
        });

        let found = found_exactly(&[service], &wanted()).expect("the song is offered");

        assert!(found.to_uri().contains("this-one"));
    }

    #[test]
    fn a_service_plays_songs_unless_it_says_it_does_not() {
        let service = OneSearch {
            scheme: SourceId::new("subsonic").expect("a scheme"),
            songs: Vec::new(),
            plays: true,
        };

        assert!(service.plays_songs());
        assert!(!service.keeps_songs());
    }

    #[test]
    fn a_service_holding_nothing_exact_finds_nothing() {
        let service: Arc<dyn PlaysInPlace> = Arc::new(OneSearch {
            scheme: SourceId::new("subsonic").expect("a scheme"),
            songs: vec![offered("near", "Cafe Song", "The Band", Some(260))],
            plays: true,
        });

        assert_eq!(found_exactly(&[service], &wanted()), None);
    }

    #[test]
    fn a_service_that_plays_no_song_is_never_searched_for_a_partys_song() {
        let scheme = |name: &str| SourceId::new(name).expect("a scheme");
        let kept_only: Arc<dyn PlaysInPlace> = Arc::new(OneSearch {
            scheme: scheme("ytmusic"),
            songs: vec![offered("kept", "Cafe Song", "The Band", Some(200))],
            plays: false,
        });
        let played: Arc<dyn PlaysInPlace> = Arc::new(OneSearch {
            scheme: scheme("subsonic"),
            songs: vec![offered("played", "Cafe Song", "The Band", Some(200))],
            plays: true,
        });

        let found = found_exactly(&[kept_only, played], &wanted()).expect("the song is offered");

        assert_eq!(found.to_uri(), "subsonic:track/played");
    }
}
