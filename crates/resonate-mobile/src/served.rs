use std::{
    collections::BTreeMap,
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, LazyLock},
};

use parking_lot::{Mutex, RwLock};
use resonate_codec::{FormatHint, Media, MediaProvider, MediaStream};
use resonate_core::{MediaLocation, Resumable, Resumption, SourceId};
use resonate_engine::{Command, Placement, QueueItem, QueueRows, Unclaimed};
use resonate_subsonic::{Album, Artist, Id, Named, Server, Song, Streamed, Subsonic};

use crate::{
    Error, MobilePlayer, Result, Track,
    downloads::{MusicProvider, failure_of},
};

#[derive(Clone, uniffi::Record)]
pub struct ServedAccount {
    pub server: String,
    pub user: String,
    pub password: String,
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ServedFound {
    pub artists: Vec<ServedArtist>,
    pub albums: Vec<ServedAlbum>,
    pub songs: Vec<ServedSong>,
}

#[derive(Default)]
pub(crate) struct Served {
    slot: Arc<RwLock<Option<Subsonic>>>,
    known: Mutex<BTreeMap<MediaLocation, ServedSong>>,
}

impl Served {
    pub(crate) fn provider(&self) -> Arc<dyn MediaProvider> {
        Arc::new(ServedSongs(Arc::clone(&self.slot)))
    }

    fn open(&self) -> Result<Subsonic> {
        self.slot.read().clone().ok_or(Error::NoServedServer)
    }

    fn given(&self, account: Option<ServedAccount>) {
        let trimmed = |text: String| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        let server = account.and_then(|account| {
            Some(Server {
                url: trimmed(account.server)?,
                user: trimmed(account.user)?,
                password: trimmed(account.password)?,
            })
        });
        let mut held = self.slot.write();
        if held.as_ref().map(Subsonic::server) == server.as_ref() {
            return;
        }
        *held = server.map(Subsonic::at);
        self.known.lock().clear();
    }

    fn remember(&self, songs: &[ServedSong]) {
        let mut known = self.known.lock();
        for song in songs {
            if let Some(location) = MediaLocation::from_uri(&song.uri) {
                known.insert(location, song.clone());
            }
        }
    }

    pub(crate) fn names(location: &MediaLocation) -> bool {
        location.source().as_str() == SUBSONIC
    }

    pub(crate) fn track_of(&self, item: &QueueItem) -> Track {
        let known = self.known.lock().get(&item.location).cloned();
        let uri = item.location.to_uri();
        let song = known.unwrap_or_else(|| ServedSong {
            uri: uri.clone(),
            title: uri.clone(),
            artist: None,
            album: None,
            track: None,
            duration_ms: None,
            format: None,
            cover: None,
        });

        Track {
            id: 0,
            uri,
            name: song.title.clone(),
            title: song.title,
            artist: song.artist.unwrap_or_default(),
            album: song.album.unwrap_or_default(),
            duration_ms: song.duration_ms.unwrap_or_default(),
            codec: song.format.unwrap_or_default(),
            track_number: song.track,
            queue_id: Some(item.id.get()),
            ..Track::default()
        }
    }
}

const SUBSONIC: &str = "subsonic";

fn cover_name(cover: Option<Id>) -> Option<String> {
    cover.map(|cover| cover.as_str().to_owned())
}

fn artist_of(subsonic: &Subsonic, artist: Artist) -> ServedArtist {
    ServedArtist {
        uri: subsonic.location_of(&Named::Artist(artist.id)).to_uri(),
        name: artist.name,
        albums: artist.albums,
        cover: cover_name(artist.cover),
    }
}

fn album_of(subsonic: &Subsonic, album: Album) -> ServedAlbum {
    ServedAlbum {
        uri: subsonic.location_of(&Named::Album(album.id)).to_uri(),
        name: album.name,
        artist: album.artist,
        year: album.year,
        songs: album.songs,
        cover: cover_name(album.cover),
    }
}

fn song_of(subsonic: &Subsonic, song: Song) -> ServedSong {
    ServedSong {
        uri: subsonic.location_of(&Named::Song(song.id)).to_uri(),
        title: song.title,
        artist: song.artist,
        album: song.album,
        track: song.track,
        duration_ms: song
            .length
            .map(|length| u64::try_from(length.as_millis()).unwrap_or(u64::MAX)),
        format: song.suffix,
        cover: cover_name(song.cover),
    }
}

fn named(subsonic: &Subsonic, uri: &str) -> Result<Named> {
    MediaLocation::from_uri(uri)
        .and_then(|location| subsonic.named_by(&location))
        .ok_or(Error::InvalidUri)
}

fn refused(error: &resonate_providers::Error) -> Error {
    failure_of(MusicProvider::Subsonic, error)
}

#[uniffi::export]
impl MobilePlayer {
    pub fn serve_from(&self, account: Option<ServedAccount>) {
        self.served.given(account);
    }

    pub fn served_artists(&self) -> Result<Vec<ServedArtist>> {
        self.require_online()?;
        let subsonic = self.served.open()?;
        let artists = subsonic.artists().map_err(|error| refused(&error))?;
        Ok(artists
            .into_iter()
            .map(|artist| artist_of(&subsonic, artist))
            .collect())
    }

    pub fn served_albums(&self, artist: String) -> Result<Vec<ServedAlbum>> {
        self.require_online()?;
        let subsonic = self.served.open()?;
        let Named::Artist(id) = named(&subsonic, &artist)? else {
            return Err(Error::InvalidUri);
        };
        let held = subsonic.artist(&id).map_err(|error| refused(&error))?;
        Ok(held
            .albums
            .into_iter()
            .map(|album| album_of(&subsonic, album))
            .collect())
    }

    pub fn served_songs(&self, album: String) -> Result<Vec<ServedSong>> {
        self.require_online()?;
        let subsonic = self.served.open()?;
        let Named::Album(id) = named(&subsonic, &album)? else {
            return Err(Error::InvalidUri);
        };
        let held = subsonic.album(&id).map_err(|error| refused(&error))?;
        let songs: Vec<ServedSong> = held
            .songs
            .into_iter()
            .map(|song| song_of(&subsonic, song))
            .collect();
        self.served.remember(&songs);
        Ok(songs)
    }

    pub fn served_search(&self, words: String) -> Result<ServedFound> {
        self.require_online()?;
        let subsonic = self.served.open()?;
        let found = subsonic.search(&words).map_err(|error| refused(&error))?;
        let songs: Vec<ServedSong> = found
            .songs
            .into_iter()
            .map(|song| song_of(&subsonic, song))
            .collect();
        self.served.remember(&songs);
        Ok(ServedFound {
            artists: found
                .artists
                .into_iter()
                .map(|artist| artist_of(&subsonic, artist))
                .collect(),
            albums: found
                .albums
                .into_iter()
                .map(|album| album_of(&subsonic, album))
                .collect(),
            songs,
        })
    }

    pub fn served_song_uris(&self, uri: String) -> Result<Vec<String>> {
        self.require_online()?;
        let subsonic = self.served.open()?;
        let named = named(&subsonic, &uri)?;
        if let Named::Song(_) = named {
            return Ok(vec![uri]);
        }
        let songs = subsonic.songs_of(&named).map_err(|error| refused(&error))?;
        let songs: Vec<ServedSong> = songs
            .into_iter()
            .map(|song| song_of(&subsonic, song))
            .collect();
        self.served.remember(&songs);
        Ok(songs.into_iter().map(|song| song.uri).collect())
    }

    pub fn served_cover(&self, uri: String, known: Option<String>, side: u32) -> Result<Vec<u8>> {
        self.require_online()?;
        let subsonic = self.served.open()?;
        let cover = match known.as_deref().and_then(Id::new) {
            Some(known) => known,
            None => subsonic
                .cover_of(&named(&subsonic, &uri)?)
                .map_err(|error| refused(&error))?,
        };
        subsonic
            .cover(&cover, side)
            .map_err(|error| refused(&error))
    }

    pub fn load_served(&self, uris: Vec<String>, start_at: u32, autoplay: bool) -> Result<()> {
        let items = self.served_items(uris, &QueueRows::default())?;
        if !autoplay {
            return self.command(Command::Resume(paused(items, start_at as usize)));
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
        let subsonic = self.served.open()?;
        let mut minting = Unclaimed::beside(beside.iter().map(Arc::as_ref));
        uris.into_iter()
            .map(|uri| {
                let Named::Song(_) = named(&subsonic, &uri)? else {
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

struct ServedSongs(Arc<RwLock<Option<Subsonic>>>);

impl MediaProvider for ServedSongs {
    fn source(&self) -> &SourceId {
        static SOURCE: LazyLock<SourceId> =
            LazyLock::new(|| SourceId::new(SUBSONIC).unwrap_or_else(|_| SourceId::local()));
        &SOURCE
    }

    fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
        let subsonic = self.0.read().clone();
        let named = subsonic
            .as_ref()
            .and_then(|subsonic| subsonic.named_by(location));
        let (Some(subsonic), Some(Named::Song(id))) = (subsonic, named) else {
            return Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            });
        };
        let streamed = subsonic
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
        self.0.length().is_some()
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

    use super::*;
    use crate::harness::{Harness, RATE, silence};

    const ARTISTS: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","artists":{"index":[{"name":"P","artist":[{"id":"ar-1","name":"Pink Floyd","albumCount":1,"coverArt":"ar-1"}]}]}}}"#;
    const ARTIST: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","artist":{"id":"ar-1","name":"Pink Floyd","album":[{"id":"al-1","name":"Meddle","artist":"Pink Floyd","year":1971,"songCount":2,"coverArt":"al-1"}]}}}"#;
    const ALBUM: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","album":{"id":"al-1","name":"Meddle","artist":"Pink Floyd","song":[{"id":"so-1","title":"One of These Days","artist":"Pink Floyd","album":"Meddle","track":1,"duration":2,"suffix":"wav","coverArt":"mf-1"},{"id":"so-2","title":"A Pillow of Winds","artist":"Pink Floyd","album":"Meddle","track":2,"duration":2,"suffix":"wav"}]}}}"#;
    const SEARCH: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{"song":[{"id":"so-2","title":"A Pillow of Winds"}]}}}"#;
    const PICTURE: &[u8] = b"\xFF\xD8 a picture";

    fn answered(stream: TcpStream) {
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
        let (kind, body): (&str, Vec<u8>) = if asked.contains("/getArtists") {
            ("application/json", ARTISTS.into())
        } else if asked.contains("/getArtist") {
            ("application/json", ARTIST.into())
        } else if asked.contains("/getAlbum") {
            ("application/json", ALBUM.into())
        } else if asked.contains("/search3") {
            ("application/json", SEARCH.into())
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

    fn account() -> ServedAccount {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let server = format!("http://{}", listener.local_addr().expect("a bound address"));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answered(stream);
            }
        });

        ServedAccount {
            server,
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
        }
    }

    #[test]
    fn without_a_server_nothing_is_browsed() {
        let harness = online();

        assert!(matches!(
            harness.player.served_artists(),
            Err(Error::NoServedServer)
        ));
    }

    #[test]
    fn a_server_is_browsed_from_its_artists_to_its_songs_and_each_has_its_cover() {
        let harness = online();
        harness.player.serve_from(Some(account()));

        let artists = harness.player.served_artists().expect("the artists");
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
            .served_search("pillow".to_owned())
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
    fn a_blank_account_serves_nothing_and_a_uri_of_another_kind_is_invalid() {
        let harness = online();
        harness.player.serve_from(Some(ServedAccount {
            password: " ".to_owned(),
            ..account()
        }));

        assert!(matches!(
            harness.player.served_artists(),
            Err(Error::NoServedServer)
        ));

        harness.player.serve_from(Some(account()));

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
    fn served_songs_are_queued_named_and_opened_by_the_players_sources_and_never_saved() {
        let harness = online();
        harness.player.serve_from(Some(account()));
        let songs = harness
            .player
            .served_songs("subsonic:album/al-1".to_owned())
            .expect("the songs");
        let uris: Vec<String> = songs.iter().map(|song| song.uri.clone()).collect();

        harness
            .player
            .load_served(uris.clone(), 0, false)
            .expect("a paused load");
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
    fn a_uri_that_is_not_a_song_is_refused_to_the_queue() {
        let harness = online();
        harness.player.serve_from(Some(account()));

        let loaded = harness
            .player
            .load_served(vec!["subsonic:album/al-1".to_owned()], 0, true);

        assert!(matches!(loaded, Err(Error::InvalidUri)));
    }
}
