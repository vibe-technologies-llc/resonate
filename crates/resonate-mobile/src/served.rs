use std::{
    collections::BTreeMap,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};
use resonate_codec::{FormatHint, Media, MediaProvider, MediaStream, Sources};
use resonate_core::{MediaLocation, Resumable, Resumption, SourceId};
use resonate_engine::{Command, Placement, QueueItem, QueueRows, Unclaimed};
use resonate_providers::{Album, Artist, Found, Id, Mix, Named, PlaysInPlace, Song, Streamed};

use crate::{
    Error, MobilePlayer, Result, Track,
    downloads::{DownloadAccount, MusicProvider, failure_of},
};

pub(crate) const PLAYED_IN_PLACE: [&str; 5] =
    ["subsonic", "tidal", "resonate", "hifi-api", "monochrome"];

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ServedService {
    pub scheme: String,
    pub title: String,
    pub favourites: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ServedArtist {
    pub uri: String,
    pub name: String,
    pub albums: Option<u32>,
    pub cover: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ServedAlbum {
    pub uri: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub songs: Option<u32>,
    pub cover: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ServedSong {
    pub uri: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track: Option<u32>,
    pub duration_ms: Option<u64>,
    pub format: Option<String>,
    pub cover: Option<String>,
    pub favourite: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ServedFound {
    pub artists: Vec<ServedArtist>,
    pub albums: Vec<ServedAlbum>,
    pub songs: Vec<ServedSong>,
}

#[derive(Clone)]
struct Service {
    account: DownloadAccount,
    plays: Arc<dyn PlaysInPlace>,
}

impl Service {
    fn refused(&self, error: &resonate_providers::Error) -> Error {
        failure_of(self.account.provider, error)
    }

    fn uri_of(&self, named: Named) -> String {
        self.plays.location_of(&named).to_uri()
    }

    fn artist_of(&self, artist: Artist) -> ServedArtist {
        ServedArtist {
            uri: self.uri_of(Named::Artist(artist.id)),
            name: artist.name,
            albums: artist.albums,
            cover: cover_name(artist.cover),
        }
    }

    fn album_of(&self, album: Album) -> ServedAlbum {
        ServedAlbum {
            uri: self.uri_of(Named::Album(album.id)),
            name: album.name,
            artist: album.artist,
            year: album.year,
            songs: album.songs,
            cover: cover_name(album.cover),
        }
    }

    fn mix_of(&self, mix: Mix) -> ServedAlbum {
        ServedAlbum {
            uri: self.uri_of(Named::Mix(mix.id)),
            name: mix.name,
            artist: mix.subtitle,
            year: None,
            songs: None,
            cover: None,
        }
    }

    fn song_of(&self, song: Song) -> ServedSong {
        ServedSong {
            uri: self.uri_of(Named::Song(song.id)),
            title: song.title,
            artist: song.artist,
            album: song.album,
            track: song.track,
            duration_ms: song
                .length
                .map(|length| u64::try_from(length.as_millis()).unwrap_or(u64::MAX)),
            format: song.suffix,
            cover: cover_name(song.cover),
            favourite: self
                .plays
                .keeps_favourites()
                .then(|| song.favourite.unwrap_or(false)),
        }
    }

    fn found_of(&self, found: Found) -> ServedFound {
        ServedFound {
            artists: found
                .artists
                .into_iter()
                .map(|artist| self.artist_of(artist))
                .collect(),
            albums: found
                .albums
                .into_iter()
                .map(|album| self.album_of(album))
                .chain(found.mixes.into_iter().map(|mix| self.mix_of(mix)))
                .collect(),
            songs: found
                .songs
                .into_iter()
                .map(|song| self.song_of(song))
                .collect(),
        }
    }

    fn named(&self, uri: &str) -> Result<Named> {
        MediaLocation::from_uri(uri)
            .and_then(|location| self.plays.named_by(&location))
            .ok_or(Error::InvalidUri)
    }
}

type Slot = Arc<RwLock<Vec<Service>>>;

#[derive(Default)]
pub(crate) struct Served {
    slot: Slot,
    known: Mutex<BTreeMap<MediaLocation, ServedSong>>,
    marked: Mutex<BTreeMap<MediaLocation, bool>>,
}

impl Served {
    pub(crate) fn sources(&self, sources: Sources) -> Sources {
        PLAYED_IN_PLACE
            .iter()
            .filter_map(|scheme| SourceId::new(scheme).ok())
            .fold(sources, |sources, scheme| {
                sources.and(Arc::new(ServedSongs {
                    scheme,
                    slot: Arc::clone(&self.slot),
                }))
            })
    }

    fn services(&self) -> Vec<Service> {
        self.slot.read().clone()
    }

    fn named(&self, scheme: &str) -> Result<Service> {
        self.slot
            .read()
            .iter()
            .find(|service| service.plays.scheme().as_str() == scheme)
            .cloned()
            .ok_or(Error::NoServedServer)
    }

    fn serving(&self, uri: &str) -> Result<Service> {
        let location = MediaLocation::from_uri(uri).ok_or(Error::InvalidUri)?;
        if !Self::names(&location) {
            return Err(Error::InvalidUri);
        }
        self.named(location.source().as_str())
    }

    fn given(
        &self,
        accounts: Vec<DownloadAccount>,
        made: impl Fn(DownloadAccount) -> Option<Arc<dyn PlaysInPlace>>,
    ) {
        let accounts: Vec<DownloadAccount> = accounts.into_iter().filter(is_ready).collect();
        let mut held = self.slot.write();
        let standing: Vec<&DownloadAccount> = held.iter().map(|service| &service.account).collect();
        if standing == accounts.iter().collect::<Vec<_>>() {
            return;
        }
        let mut services: Vec<Service> = Vec::new();
        for account in accounts {
            let kept = held
                .iter()
                .find(|service| service.account == account)
                .map(|service| Arc::clone(&service.plays));
            let Some(plays) = kept.or_else(|| made(account.clone())) else {
                continue;
            };
            if services
                .iter()
                .any(|service| service.plays.scheme() == plays.scheme())
            {
                continue;
            }
            services.push(Service { account, plays });
        }
        *held = services;
        self.known.lock().clear();
        self.marked.lock().clear();
    }

    fn remember(&self, songs: &mut [ServedSong]) {
        let marked = self.marked.lock();
        let mut known = self.known.lock();
        for song in songs {
            let Some(location) = MediaLocation::from_uri(&song.uri) else {
                continue;
            };
            if let (Some(favourite), Some(mark)) = (&mut song.favourite, marked.get(&location)) {
                *favourite = *mark;
            }
            known.insert(location, song.clone());
        }
    }

    fn mark(&self, location: MediaLocation, favoured: bool) {
        if let Some(song) = self.known.lock().get_mut(&location) {
            song.favourite = Some(favoured);
        }
        self.marked.lock().insert(location, favoured);
    }

    pub(crate) fn names(location: &MediaLocation) -> bool {
        PLAYED_IN_PLACE.contains(&location.source().as_str())
    }

    pub(crate) fn song_at(&self, location: &MediaLocation) -> ServedSong {
        let known = self.known.lock().get(location).cloned();
        known.unwrap_or_else(|| {
            let uri = location.to_uri();
            ServedSong {
                uri: uri.clone(),
                title: uri,
                artist: None,
                album: None,
                track: None,
                duration_ms: None,
                format: None,
                cover: None,
                favourite: self.marked.lock().get(location).copied(),
            }
        })
    }

    pub(crate) fn kept(&self, songs: &[ServedSong]) {
        self.remember(&mut songs.to_vec());
    }

    pub(crate) fn track_of(&self, item: &QueueItem) -> Track {
        let song = self.song_at(&item.location);

        Track {
            id: 0,
            uri: song.uri.clone(),
            name: song.title.clone(),
            title: song.title,
            artist: song.artist.unwrap_or_default(),
            album: song.album.unwrap_or_default(),
            duration_ms: song.duration_ms.unwrap_or_default(),
            codec: song.format.unwrap_or_default(),
            track_number: song.track,
            favourite: song.favourite.unwrap_or(false),
            queue_id: Some(item.id.get()),
            ..Track::default()
        }
    }
}

fn is_ready(account: &DownloadAccount) -> bool {
    let given = |field: &str| !field.trim().is_empty();
    match account.provider {
        MusicProvider::Subsonic => {
            given(&account.server) && given(&account.user) && given(&account.password)
        }
        MusicProvider::Tidal => given(&account.client_id) && given(&account.refresh_token),
        MusicProvider::Monochrome | MusicProvider::HifiApi => true,
    }
}

fn cover_name(cover: Option<Id>) -> Option<String> {
    cover.map(|cover| cover.as_str().to_owned())
}

#[uniffi::export]
impl MobilePlayer {
    pub fn serve_from(&self, accounts: Vec<DownloadAccount>) {
        self.served
            .given(accounts, |account| self.downloads.in_place_of(account));
    }

    pub fn served_services(&self) -> Vec<ServedService> {
        self.served
            .services()
            .into_iter()
            .map(|service| ServedService {
                scheme: service.plays.scheme().as_str().to_owned(),
                title: service.plays.title().to_owned(),
                favourites: service.plays.keeps_favourites(),
            })
            .collect()
    }

    pub fn served_first_page(&self, service: String) -> Result<ServedFound> {
        self.require_online()?;
        let service = self.served.named(&service)?;
        let first = service
            .plays
            .first_page()
            .map_err(|error| service.refused(&error))?;
        let mut found = service.found_of(first);
        self.served.remember(&mut found.songs);
        Ok(found)
    }

    pub fn served_albums(&self, artist: String) -> Result<Vec<ServedAlbum>> {
        self.require_online()?;
        let service = self.served.serving(&artist)?;
        let Named::Artist(id) = service.named(&artist)? else {
            return Err(Error::InvalidUri);
        };
        let held = service
            .plays
            .artist(&id)
            .map_err(|error| service.refused(&error))?;
        Ok(held
            .albums
            .into_iter()
            .map(|album| service.album_of(album))
            .collect())
    }

    pub fn served_songs(&self, album: String) -> Result<Vec<ServedSong>> {
        self.require_online()?;
        let service = self.served.serving(&album)?;
        let named = service.named(&album)?;
        if !matches!(named, Named::Album(_) | Named::Mix(_)) {
            return Err(Error::InvalidUri);
        }
        let held = service
            .plays
            .songs_of(&named)
            .map_err(|error| service.refused(&error))?;
        let mut songs: Vec<ServedSong> =
            held.into_iter().map(|song| service.song_of(song)).collect();
        self.served.remember(&mut songs);
        Ok(songs)
    }

    pub fn served_search(&self, service: String, words: String) -> Result<ServedFound> {
        self.require_online()?;
        let service = self.served.named(&service)?;
        let found = service
            .plays
            .search(&words)
            .map_err(|error| service.refused(&error))?;
        let mut found = service.found_of(found);
        self.served.remember(&mut found.songs);
        Ok(found)
    }

    pub fn served_song_uris(&self, uri: String) -> Result<Vec<String>> {
        self.require_online()?;
        let service = self.served.serving(&uri)?;
        let named = service.named(&uri)?;
        if let Named::Song(_) = named {
            return Ok(vec![uri]);
        }
        let songs = service
            .plays
            .songs_of(&named)
            .map_err(|error| service.refused(&error))?;
        let mut songs: Vec<ServedSong> = songs
            .into_iter()
            .map(|song| service.song_of(song))
            .collect();
        self.served.remember(&mut songs);
        Ok(songs.into_iter().map(|song| song.uri).collect())
    }

    pub fn served_cover(&self, uri: String, known: Option<String>, side: u32) -> Result<Vec<u8>> {
        self.require_online()?;
        let service = self.served.serving(&uri)?;
        let cover = match known.as_deref().and_then(Id::new) {
            Some(known) => known,
            None => service
                .plays
                .cover_of(&service.named(&uri)?)
                .map_err(|error| service.refused(&error))?,
        };
        service
            .plays
            .cover(&cover, side)
            .map_err(|error| service.refused(&error))
    }

    pub fn favour_served_song(&self, uri: String, favoured: bool) -> Result<()> {
        self.require_online()?;
        let service = self.served.serving(&uri)?;
        let Named::Song(id) = service.named(&uri)? else {
            return Err(Error::InvalidUri);
        };
        let location = MediaLocation::from_uri(&uri).ok_or(Error::InvalidUri)?;
        service
            .plays
            .favour(&id, favoured)
            .map_err(|error| service.refused(&error))?;
        self.served.mark(location, favoured);
        Ok(())
    }

    pub fn load_served(&self, uris: Vec<String>, start_at: u32, autoplay: bool) -> Result<()> {
        let items = self.served_items(uris, &QueueRows::default())?;
        if !autoplay {
            return self.command_landed(Command::Resume(paused(items, start_at as usize)));
        }
        self.command(Command::Load {
            items,
            start_at: start_at as usize,
            autoplay,
        })
    }

    pub fn play_served_next(&self, uris: Vec<String>) -> Result<()> {
        self.queue_served(uris, |_| Placement::Next)
    }

    pub fn add_served_to_queue(&self, uris: Vec<String>) -> Result<()> {
        self.queue_served(uris, Placement::At)
    }
}

impl MobilePlayer {
    fn served_items(&self, uris: Vec<String>, beside: &QueueRows) -> Result<Vec<QueueItem>> {
        let mut minting = Unclaimed::beside(beside.iter().map(Arc::as_ref));
        uris.into_iter()
            .map(|uri| {
                let service = self.served.serving(&uri)?;
                let Named::Song(_) = service.named(&uri)? else {
                    return Err(Error::InvalidUri);
                };
                let location = MediaLocation::from_uri(&uri).ok_or(Error::InvalidUri)?;
                Ok(QueueItem::whole(minting.mint(), location))
            })
            .collect()
    }

    fn queue_served(&self, uris: Vec<String>, placed: impl Fn(usize) -> Placement) -> Result<()> {
        self.edit_queue(|queued, _| {
            let items = self.served_items(uris, &queued.rows)?;
            if items.is_empty() {
                return Ok(None);
            }
            if queued.rows.is_empty() {
                return Ok(Some(Command::Resume(paused(items, 0))));
            }
            Ok(Some(Command::Insert {
                items,
                at: placed(queued.rows.len()),
                play: false,
            }))
        })
    }
}

fn paused(items: Vec<QueueItem>, row: usize) -> Resumption {
    Resumption {
        rows: items
            .into_iter()
            .map(|item| Resumable {
                location: item.location,
                span: item.span,
                held: Some(item.id),
            })
            .collect(),
        row,
        ..Resumption::default()
    }
}

struct ServedSongs {
    scheme: SourceId,
    slot: Slot,
}

impl MediaProvider for ServedSongs {
    fn source(&self) -> &SourceId {
        &self.scheme
    }

    fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
        let plays = self
            .slot
            .read()
            .iter()
            .find(|service| service.plays.scheme() == location.source())
            .map(|service| Arc::clone(&service.plays));
        let named = plays.as_ref().and_then(|plays| plays.named_by(location));
        let (Some(plays), Some(Named::Song(id))) = (plays, named) else {
            return Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            });
        };
        let streamed = plays
            .streamed(&id)
            .map_err(|error| resonate_codec::Error::Io {
                location: location.clone(),
                source: io::Error::other(error),
            })?;
        let hint = streamed
            .media_type
            .as_deref()
            .map(|media_type| FormatHint::MediaType(Box::from(media_type)));
        Ok(Media {
            stream: Box::new(Streaming(streamed)),
            hint,
        })
    }
}

struct Streaming(Streamed);

impl Read for Streaming {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Streaming {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.0.seek(to)
    }
}

impl MediaStream for Streaming {
    fn is_seekable(&self) -> bool {
        self.0.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.0.length()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    use resonate_codec::probe;
    use resonate_core::{
        ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec, TrackId, ZoneId,
    };
    use resonate_engine::{PlaybackState, PlayerState, TrackState};

    use super::*;
    use crate::{
        catalog::{KeptRow, KeptServed},
        harness::{Harness, RATE, silence},
    };

    const ARTISTS: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","artists":{"index":[{"name":"P","artist":[{"id":"ar-1","name":"Pink Floyd","albumCount":1,"coverArt":"ar-1"}]}]}}}"#;
    const ARTIST: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","artist":{"id":"ar-1","name":"Pink Floyd","album":[{"id":"al-1","name":"Meddle","artist":"Pink Floyd","year":1971,"songCount":2,"coverArt":"al-1"}]}}}"#;
    const ALBUM: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","album":{"id":"al-1","name":"Meddle","artist":"Pink Floyd","song":[{"id":"so-1","title":"One of These Days","artist":"Pink Floyd","album":"Meddle","track":1,"duration":2,"suffix":"wav","coverArt":"mf-1"},{"id":"so-2","title":"A Pillow of Winds","artist":"Pink Floyd","album":"Meddle","track":2,"duration":2,"suffix":"wav"}]}}}"#;
    const SEARCH: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{"song":[{"id":"so-2","title":"A Pillow of Winds"}]}}}"#;
    const MONOCHROME_SEARCH: &str = r#"{"tracks":[{"trackId":77,"title":"Echoes","artistNames":["Pink Floyd"],"duration":2000,"playable":true},{"trackId":78,"title":"Unplayable","playable":false}]}"#;
    const PICTURE: &[u8] = b"\xFF\xD8 a picture";

    const STARRED: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;

    fn answered(stream: TcpStream, asked_log: &Mutex<Vec<String>>) {
        let mut reader = BufReader::new(stream);
        let mut asked = String::new();
        if reader.read_line(&mut asked).is_err() {
            return;
        }
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) if header == "\r\n" => break,
                Ok(_) => {}
            }
        }
        asked_log.lock().push(asked.clone());
        let (kind, body): (&str, Vec<u8>) = if asked.contains("/star") || asked.contains("/unstar")
        {
            ("application/json", STARRED.into())
        } else if asked.contains("/getArtists") {
            ("application/json", ARTISTS.into())
        } else if asked.contains("/getArtist") {
            ("application/json", ARTIST.into())
        } else if asked.contains("/getAlbum") {
            ("application/json", ALBUM.into())
        } else if asked.contains("/search3") {
            ("application/json", SEARCH.into())
        } else if asked.contains("/search/tracks") {
            ("application/json", MONOCHROME_SEARCH.into())
        } else if asked.contains("/getCoverArt") {
            ("image/jpeg", PICTURE.to_vec())
        } else {
            ("audio/wav", silence())
        };
        let mut stream = reader.into_inner();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(&body);
    }

    fn online() -> Harness {
        let harness = Harness::new();
        harness.player.configure_online(true);
        harness
    }

    fn fake_asked() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let server = format!("http://{}", listener.local_addr().expect("a bound address"));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&asked);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answered(stream, &log);
            }
        });
        (server, asked)
    }

    fn fake() -> String {
        fake_asked().0
    }

    fn of(provider: MusicProvider, server: String) -> DownloadAccount {
        DownloadAccount {
            provider,
            server,
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
            client_id: String::new(),
            client_secret: String::new(),
            refresh_token: String::new(),
        }
    }

    fn account() -> DownloadAccount {
        of(MusicProvider::Subsonic, fake())
    }

    fn monochrome() -> DownloadAccount {
        DownloadAccount {
            password: String::new(),
            ..of(MusicProvider::Monochrome, fake())
        }
    }

    #[test]
    fn without_a_server_nothing_is_browsed() {
        let harness = online();

        assert!(harness.player.served_services().is_empty());
        assert!(matches!(
            harness.player.served_first_page("subsonic".to_owned()),
            Err(Error::NoServedServer)
        ));
    }

    #[test]
    fn a_server_is_browsed_from_its_artists_to_its_songs_and_each_has_its_cover() {
        let harness = online();
        harness.player.serve_from(vec![account()]);

        let artists = harness
            .player
            .served_first_page("subsonic".to_owned())
            .expect("the artists")
            .artists;
        let albums = harness
            .player
            .served_albums(artists[0].uri.clone())
            .expect("the albums");
        let songs = harness
            .player
            .served_songs(albums[0].uri.clone())
            .expect("the songs");
        let found = harness
            .player
            .served_search("subsonic".to_owned(), "pillow".to_owned())
            .expect("a search");
        let picture = harness
            .player
            .served_cover(albums[0].uri.clone(), albums[0].cover.clone(), 300)
            .expect("a cover");
        let expanded = harness
            .player
            .served_song_uris(artists[0].uri.clone())
            .expect("the songs of an artist");

        assert_eq!(artists[0].uri, "subsonic:artist/ar-1");
        assert_eq!(artists[0].albums, Some(1));
        assert_eq!(albums[0].uri, "subsonic:album/al-1");
        assert_eq!(albums[0].year, Some(1971));
        assert_eq!(
            songs
                .iter()
                .map(|song| song.uri.as_str())
                .collect::<Vec<_>>(),
            ["subsonic:track/so-1", "subsonic:track/so-2"]
        );
        assert_eq!(songs[0].duration_ms, Some(2000));
        assert_eq!(found.songs[0].title, "A Pillow of Winds");
        assert_eq!(picture, PICTURE);
        assert_eq!(expanded.len(), 2);
    }

    #[test]
    fn every_service_set_is_offered_and_each_is_searched_and_played_from_its_own_uris() {
        let harness = online();
        harness
            .player
            .serve_from(vec![account(), monochrome(), account()]);

        let services = harness.player.served_services();
        let first = harness
            .player
            .served_first_page("monochrome".to_owned())
            .expect("a first page");
        let found = harness
            .player
            .served_search("monochrome".to_owned(), "echoes".to_owned())
            .expect("a search");
        harness
            .player
            .add_served_to_queue(vec![found.songs[0].uri.clone()])
            .expect("a song queued");
        let queue = harness.player.queue().expect("the queue");
        let info = probe(
            &harness.player.sources,
            &MediaLocation::from_uri(&found.songs[0].uri).expect("a location"),
        )
        .expect("the song probes off the service");
        let refused = harness
            .player
            .served_albums("monochrome:artist/1".to_owned());

        assert_eq!(
            services,
            [
                ServedService {
                    scheme: "subsonic".to_owned(),
                    title: "Subsonic".to_owned(),
                    favourites: true,
                },
                ServedService {
                    scheme: "monochrome".to_owned(),
                    title: "Monochrome".to_owned(),
                    favourites: false,
                },
            ]
        );
        assert_eq!(first, ServedFound::default());
        assert_eq!(
            found
                .songs
                .iter()
                .map(|song| song.uri.as_str())
                .collect::<Vec<_>>(),
            ["monochrome:track/77"]
        );
        assert_eq!(queue[0].title, "Echoes");
        assert_eq!(queue[0].artist, "Pink Floyd");
        assert_eq!(info.spec.rate.hz(), RATE);
        assert!(matches!(refused, Err(Error::InvalidUri)));
    }

    #[test]
    fn a_service_declined_as_hosted_is_not_served_and_a_service_no_longer_given_goes() {
        let harness = online();
        let hosted = DownloadAccount {
            server: String::new(),
            ..monochrome()
        };
        harness.player.configure_hosted_services(false);
        harness.player.serve_from(vec![account(), hosted.clone()]);

        let declined = harness.player.served_services();

        harness.player.configure_hosted_services(true);
        harness.player.serve_from(vec![hosted]);

        let allowed = harness.player.served_services();

        assert_eq!(declined.len(), 1);
        assert_eq!(declined[0].scheme, "subsonic");
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0].scheme, "monochrome");
        assert!(matches!(
            harness.player.served_first_page("subsonic".to_owned()),
            Err(Error::NoServedServer)
        ));
    }

    #[test]
    fn a_blank_account_serves_nothing_and_a_uri_of_another_kind_is_invalid() {
        let harness = online();
        harness.player.serve_from(vec![DownloadAccount {
            password: " ".to_owned(),
            ..account()
        }]);

        assert!(matches!(
            harness.player.served_first_page("subsonic".to_owned()),
            Err(Error::NoServedServer)
        ));

        harness.player.serve_from(vec![account()]);

        assert!(matches!(
            harness
                .player
                .served_albums("subsonic:album/al-1".to_owned()),
            Err(Error::InvalidUri)
        ));
        assert!(matches!(
            harness
                .player
                .served_albums("content://test/a.wav".to_owned()),
            Err(Error::InvalidUri)
        ));
    }

    #[test]
    fn served_songs_are_queued_named_and_opened_by_the_players_sources() {
        let harness = online();
        harness.player.serve_from(vec![account()]);
        let songs = harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs");
        let uris: Vec<String> = songs.iter().map(|song| song.uri.clone()).collect();

        harness
            .player
            .load_served(uris.clone(), 0, false)
            .expect("a paused load");
        harness.settled(|_| harness.player.queue().is_ok_and(|queue| queue.len() == 2));
        harness
            .player
            .add_to_queue(harness.ids("a"))
            .expect("a song of the library");
        harness
            .player
            .add_served_to_queue(vec![uris[1].clone()])
            .expect("an append");
        harness
            .player
            .play_served_next(vec![uris[0].clone()])
            .expect("a play next");
        let queue = harness.player.queue().expect("the queue");
        let info = probe(
            &harness.player.sources,
            &MediaLocation::from_uri(&uris[0]).expect("a location"),
        )
        .expect("the song probes off the server");

        assert_eq!(
            queue
                .iter()
                .map(|track| track.title.as_str())
                .collect::<Vec<_>>(),
            [
                "One of These Days",
                "One of These Days",
                "A Pillow of Winds",
                "a.wav",
                "A Pillow of Winds"
            ]
        );
        assert!(queue.iter().all(|track| track.queue_id.is_some()));
        assert_eq!(queue[0].artist, "Pink Floyd");
        assert_eq!(queue[0].duration_ms, 2000);
        assert_eq!(info.spec.rate.hz(), RATE);
        harness
            .player
            .save()
            .expect("a save over a queue holding served songs");
    }

    #[test]
    fn a_queue_holding_served_songs_is_kept_with_their_names_and_resumed_from_their_uris() {
        let harness = online();
        harness.player.serve_from(vec![account()]);
        let songs = harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs");
        harness
            .player
            .load_served(vec![songs[0].uri.clone()], 0, false)
            .expect("a paused load");
        harness
            .player
            .add_to_queue(harness.ids("a"))
            .expect("a song of the library");
        harness
            .player
            .add_served_to_queue(vec![songs[1].uri.clone()])
            .expect("an append");

        harness.settled(|_| {
            harness.player.save().expect("a save");
            !harness
                .player
                .catalog
                .resume_rows_of(ZoneId::FIRST)
                .expect("the kept queue")
                .0
                .is_empty()
        });
        let (kept, row, _) = harness
            .player
            .catalog
            .resume_rows_of(ZoneId::FIRST)
            .expect("the kept queue");
        harness.player.serve_from(Vec::new());
        harness.player.serve_from(vec![account()]);
        harness
            .player
            .resume_saved_in(ZoneId::FIRST)
            .expect("a resumption");
        let queue = harness.player.queue().expect("the queue");

        assert_eq!(kept.len(), 3);
        assert_eq!(row, 0);
        assert!(matches!(
            &kept[0],
            KeptRow::Served(KeptServed { song, .. }) if song.title == "One of These Days"
        ));
        assert!(matches!(kept[1], KeptRow::Document(_)));
        assert_eq!(
            queue
                .iter()
                .map(|track| (track.title.as_str(), track.uri.as_str()))
                .collect::<Vec<_>>(),
            [
                ("One of These Days", "subsonic:track/so-1"),
                ("a.wav", queue[1].uri.as_str()),
                ("A Pillow of Winds", "subsonic:track/so-2"),
            ]
        );
        assert_eq!(queue[0].artist, "Pink Floyd");
    }

    #[test]
    fn a_uri_that_is_not_a_song_is_refused_to_the_queue() {
        let harness = online();
        harness.player.serve_from(vec![account()]);

        let loaded = harness
            .player
            .load_served(vec!["subsonic:album/al-1".to_owned()], 0, true);

        assert!(matches!(loaded, Err(Error::InvalidUri)));
    }

    fn listened_to(harness: &Harness, uri: &str, seconds: u64) {
        let rows = harness
            .player
            .playing
            .player()
            .expect("an open player")
            .queued()
            .rows;
        let id: TrackId = rows.get(0).expect("a queue row").id;
        let rate = SampleRate::HZ_44100;
        let frames = |count: u64| Frames(u64::from(rate.hz()) * count);

        for position in [0, seconds / 2, seconds] {
            let state = PlayerState {
                playback: PlaybackState::Playing,
                current: Some(TrackState {
                    id,
                    source: StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::S16),
                    position: frames(position),
                    duration: Some(frames(seconds)),
                }),
                queue_position: Some(0),
                ..PlayerState::default()
            };
            harness
                .player
                .count_listening(ZoneId::FIRST, &state, &rows)
                .expect("a listen counted");
        }
        assert_eq!(
            harness.player.queue().expect("the queue")[0].uri,
            uri,
            "the queue holds the song heard"
        );
    }

    #[test]
    fn a_song_played_in_place_is_a_counted_listen_in_the_totals_and_the_days_and_told_to_no_one() {
        let harness = online();
        harness.player.serve_from(vec![account()]);
        let uri = "subsonic:track/so-1".to_owned();
        harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs");
        harness
            .player
            .load_served(vec![uri.clone()], 0, false)
            .expect("a paused load");
        harness.settled(|_| harness.player.queue().is_ok_and(|queue| queue.len() == 1));

        listened_to(&harness, &uri, 4);
        harness.player.finish_listening().expect("a listen settled");

        let statistics = harness.player.statistics(0).expect("the statistics");
        let detail = harness
            .player
            .listening_detail(0, 10, "UTC".to_owned(), 0)
            .expect("the detail");
        let kept: Vec<(Option<i64>, Option<String>, i64, i64)> = {
            let connection = harness.player.catalog.0.lock();
            let mut query = connection
                .prepare("SELECT track, served, heard_ms, counted FROM listens ORDER BY id")
                .expect("a query");
            query
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("listens")
                .collect::<rusqlite::Result<_>>()
                .expect("rows")
        };

        assert_eq!(kept, [(None, Some(uri), 4_000, 1)]);
        assert_eq!(statistics.plays, 1);
        assert_eq!(statistics.listened_ms, 4_000);
        assert!(statistics.top_tracks.is_empty());
        assert_eq!(detail.songs_heard, 1);
        assert_eq!(detail.days.iter().map(|day| day.plays).sum::<u64>(), 1);
        assert!(detail.artists.is_empty() && detail.albums.is_empty());
        assert!(
            harness
                .player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT count(*) FROM listens l JOIN documents d ON d.id = l.track",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .is_ok_and(|joined| joined == 0)
        );
    }

    #[test]
    fn a_service_with_an_account_marks_its_songs_and_one_without_draws_no_star() {
        let harness = online();
        harness.player.serve_from(vec![account(), monochrome()]);

        let services = harness.player.served_services();
        let starring = harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs");
        let unstarring = harness
            .player
            .served_search("monochrome".to_owned(), "echoes".to_owned())
            .expect("a search")
            .songs;

        assert_eq!(
            services
                .iter()
                .map(|service| service.favourites)
                .collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(starring[0].favourite, Some(false));
        assert_eq!(unstarring[0].favourite, None);
        assert!(matches!(
            harness
                .player
                .favour_served_song(unstarring[0].uri.clone(), true),
            Err(Error::ProviderUnreadable { .. })
        ));
    }

    #[test]
    fn a_song_is_starred_and_unstarred_by_the_services_own_call_and_drawn_so_until_the_accounts_move()
     {
        let harness = online();
        let (server, asked) = fake_asked();
        harness
            .player
            .serve_from(vec![of(MusicProvider::Subsonic, server)]);
        let uri = "subsonic:track/so-1".to_owned();
        harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs");
        harness
            .player
            .load_served(vec![uri.clone()], 0, false)
            .expect("a paused load");
        harness.settled(|_| harness.player.queue().is_ok_and(|queue| queue.len() == 1));

        harness
            .player
            .favour_served_song(uri.clone(), true)
            .expect("a star");
        let starred = harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs again");
        let playing = harness.player.queue().expect("the queue");

        harness
            .player
            .favour_served_song(uri.clone(), false)
            .expect("a star taken out");
        let unstarred = harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs once more");

        let calls: Vec<String> = asked
            .lock()
            .iter()
            .filter(|call| call.contains("/star") || call.contains("/unstar"))
            .cloned()
            .collect();

        assert_eq!(calls.len(), 2);
        assert!(
            calls[0].contains("/star?") && calls[0].contains("id=so-1"),
            "{calls:?}"
        );
        assert!(
            calls[1].contains("/unstar?") && calls[1].contains("id=so-1"),
            "{calls:?}"
        );
        assert_eq!(starred[0].favourite, Some(true));
        assert_eq!(starred[1].favourite, Some(false));
        assert!(playing[0].favourite);
        assert_eq!(unstarred[0].favourite, Some(false));
    }

    #[test]
    fn a_star_is_refused_for_a_uri_that_is_no_song_and_while_online_is_off() {
        let harness = online();
        harness.player.serve_from(vec![account()]);

        let album = harness
            .player
            .favour_served_song("subsonic:album/al-1".to_owned(), true);
        let document = harness
            .player
            .favour_served_song("content://test/a.wav".to_owned(), true);
        harness.player.configure_online(false);
        let offline = harness
            .player
            .favour_served_song("subsonic:track/so-1".to_owned(), true);

        assert!(matches!(album, Err(Error::InvalidUri)));
        assert!(matches!(document, Err(Error::InvalidUri)));
        assert!(matches!(offline, Err(Error::OnlineOff)));
    }

    struct Mixing;

    impl PlaysInPlace for Mixing {
        fn scheme(&self) -> &resonate_core::SourceId {
            static TIDAL: std::sync::LazyLock<resonate_core::SourceId> =
                std::sync::LazyLock::new(|| resonate_core::SourceId::new("tidal").expect("a name"));
            &TIDAL
        }

        fn title(&self) -> &'static str {
            "TIDAL"
        }

        fn artists(&self) -> resonate_providers::Result<Vec<Artist>> {
            Ok(Vec::new())
        }

        fn artist(&self, _: &Id) -> resonate_providers::Result<resonate_providers::ArtistAlbums> {
            Err(resonate_providers::not_offered(
                self.scheme(),
                resonate_providers::ProviderOp::Browse,
            ))
        }

        fn album(&self, _: &Id) -> resonate_providers::Result<resonate_providers::AlbumSongs> {
            Err(resonate_providers::not_offered(
                self.scheme(),
                resonate_providers::ProviderOp::Browse,
            ))
        }

        fn mix(&self, id: &Id) -> resonate_providers::Result<resonate_providers::MixSongs> {
            Ok(resonate_providers::MixSongs {
                mix: Mix {
                    id: id.clone(),
                    name: "My Mix 1".to_owned(),
                    subtitle: None,
                },
                songs: vec![Song {
                    title: "Echoes".to_owned(),
                    ..Song::only(Id::new("12").expect("an id"))
                }],
            })
        }

        fn search(&self, _: &str) -> resonate_providers::Result<Found> {
            Ok(Found::default())
        }

        fn streamed(&self, _: &Id) -> resonate_providers::Result<Streamed> {
            Err(resonate_providers::not_offered(
                self.scheme(),
                resonate_providers::ProviderOp::Stream,
            ))
        }

        fn cover(&self, _: &Id, _: u32) -> resonate_providers::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn cover_of(&self, _: &Named) -> resonate_providers::Result<Id> {
            Err(resonate_providers::not_offered(
                self.scheme(),
                resonate_providers::ProviderOp::Cover,
            ))
        }
    }

    #[test]
    fn a_mix_is_shown_to_the_phones_among_the_albums_and_opens_to_its_songs_by_its_uri() {
        let service = Service {
            account: of(MusicProvider::Tidal, String::new()),
            plays: Arc::new(Mixing),
        };
        let found = Found {
            mixes: vec![Mix {
                id: Id::new("0a1b").expect("an id"),
                name: "My Mix 1".to_owned(),
                subtitle: Some("Pink Floyd and more".to_owned()),
            }],
            ..Found::default()
        };

        let shown = service.found_of(found);
        let named = service.named(&shown.albums[0].uri).expect("a mix");
        let songs = service.plays.songs_of(&named).expect("the mix's songs");

        assert_eq!(shown.albums[0].uri, "tidal:mix/0a1b");
        assert_eq!(shown.albums[0].name, "My Mix 1");
        assert_eq!(
            shown.albums[0].artist.as_deref(),
            Some("Pink Floyd and more")
        );
        assert!(matches!(named, Named::Mix(_)));
        assert_eq!(songs[0].title, "Echoes");
    }
}
