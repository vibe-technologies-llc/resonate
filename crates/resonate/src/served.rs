#[cfg(feature = "online")]
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, LazyLock},
};

#[cfg(all(feature = "ui", feature = "online"))]
use parking_lot::Mutex;
#[cfg(feature = "online")]
use parking_lot::RwLock;
use resonate_codec::Sources;
#[cfg(feature = "online")]
use resonate_codec::{FormatHint, Media, MediaProvider, MediaStream};
use resonate_core::MediaLocation;
#[cfg(feature = "online")]
use resonate_core::SourceId;
#[cfg(feature = "online")]
use resonate_subsonic::{Album, Artist, Id, Named, Song, Streamed, Subsonic};

use crate::{
    cli::SubsonicArgs,
    config::Config,
    error::{Error, Result},
};
#[cfg(feature = "online")]
use crate::{providers, table::Table};

#[derive(Clone, Default)]
pub struct Served {
    #[cfg(feature = "online")]
    slot: Arc<RwLock<Option<Subsonic>>>,
    #[cfg(feature = "online")]
    live: bool,
}

impl Served {
    #[cfg(feature = "online")]
    pub fn of(config: &Config) -> Self {
        Self::over(providers::server_named(config), false)
    }

    #[cfg(not(feature = "online"))]
    pub const fn of(_config: &Config) -> Self {
        Self {}
    }

    #[cfg(feature = "online")]
    fn over(subsonic: Option<Subsonic>, live: bool) -> Self {
        Self {
            slot: Arc::new(RwLock::new(subsonic)),
            live,
        }
    }

    #[cfg(all(feature = "ui", feature = "online"))]
    pub fn for_the_window(config: &Config) -> Self {
        Self::over(providers::server_named(config), true)
    }

    #[cfg(all(feature = "ui", not(feature = "online")))]
    pub const fn for_the_window(_config: &Config) -> Self {
        Self {}
    }

    #[cfg(feature = "online")]
    pub fn sources(&self, sources: Sources) -> Sources {
        if self.live || self.slot.read().is_some() {
            sources.and(Arc::new(ServedSongs(Arc::clone(&self.slot))))
        } else {
            sources
        }
    }

    #[cfg(not(feature = "online"))]
    pub fn sources(&self, sources: Sources) -> Sources {
        sources
    }

    #[cfg(feature = "online")]
    pub fn songs_at(&self, location: &MediaLocation) -> Option<Vec<MediaLocation>> {
        let subsonic = self.slot.read().clone()?;
        let named = subsonic.named_by(location)?;
        if let Named::Song(_) = named {
            return Some(vec![location.clone()]);
        }
        match subsonic.songs_of(&named) {
            Ok(songs) => Some(
                songs
                    .into_iter()
                    .map(|song| subsonic.location_of(&Named::Song(song.id)))
                    .collect(),
            ),
            Err(error) => {
                tracing::warn!(%error, %location, "the Subsonic server did not list what was named");
                Some(Vec::new())
            }
        }
    }

    #[cfg(not(feature = "online"))]
    pub fn songs_at(&self, _location: &MediaLocation) -> Option<Vec<MediaLocation>> {
        None
    }

    #[cfg(all(feature = "ui", feature = "online"))]
    pub fn serving(&self) -> resonate_ui::Serving {
        let slot = Arc::clone(&self.slot);
        let opened: Mutex<Option<Subsonic>> = Mutex::new(None);
        resonate_ui::Serving::new(move |online| {
            let given = providers::server_given(online);
            let mut held = opened.lock();
            let subsonic = match (given, held.take()) {
                (Some(given), Some(kept)) if kept.server() == &given => Some(kept),
                (Some(given), _) => Some(Subsonic::at(given)),
                (None, _) => None,
            };
            held.clone_from(&subsonic);
            *slot.write() = subsonic.clone();
            subsonic.map(|subsonic| Arc::new(ServedBy(subsonic)) as Arc<dyn resonate_ui::Serves>)
        })
    }

    #[cfg(all(feature = "ui", not(feature = "online")))]
    pub fn serving(&self) -> resonate_ui::Serving {
        resonate_ui::Serving::none()
    }
}

#[cfg(feature = "online")]
struct ServedSongs(Arc<RwLock<Option<Subsonic>>>);

#[cfg(feature = "online")]
impl MediaProvider for ServedSongs {
    fn source(&self) -> &SourceId {
        static SUBSONIC: LazyLock<SourceId> =
            LazyLock::new(|| SourceId::new("subsonic").unwrap_or_else(|_| SourceId::local()));
        &SUBSONIC
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

#[cfg(all(feature = "ui", feature = "online"))]
struct ServedBy(Subsonic);

#[cfg(all(feature = "ui", feature = "online"))]
impl ServedBy {
    fn unreadable(&self) -> resonate_providers::Error {
        resonate_providers::Error::Unreadable {
            provider: self.0.source_id().clone(),
            op: resonate_providers::ProviderOp::Browse,
        }
    }

    fn cover_of(cover: Option<Id>) -> Option<resonate_ui::ServedCover> {
        cover.map(|cover| resonate_ui::ServedCover(cover.as_str().to_owned()))
    }

    fn artist(&self, artist: Artist) -> resonate_ui::ServedArtist {
        resonate_ui::ServedArtist {
            location: self.0.location_of(&Named::Artist(artist.id)),
            name: artist.name,
            albums: artist.albums,
            cover: Self::cover_of(artist.cover),
        }
    }

    fn album(&self, album: Album) -> resonate_ui::ServedAlbum {
        resonate_ui::ServedAlbum {
            location: self.0.location_of(&Named::Album(album.id)),
            name: album.name,
            artist: album.artist,
            year: album.year,
            songs: album.songs,
            cover: Self::cover_of(album.cover),
        }
    }

    fn song(&self, song: Song) -> resonate_ui::ServedSong {
        resonate_ui::ServedSong {
            location: self.0.location_of(&Named::Song(song.id)),
            title: song.title,
            artist: song.artist,
            album: song.album,
            track: song.track,
            length: song.length,
            format: song.suffix,
            cover: Self::cover_of(song.cover),
        }
    }
}

#[cfg(all(feature = "ui", feature = "online"))]
impl resonate_ui::Serves for ServedBy {
    fn names(&self, location: &MediaLocation) -> bool {
        self.0.named_by(location).is_some()
    }

    fn artists(&self) -> resonate_providers::Result<Vec<resonate_ui::ServedArtist>> {
        Ok(self
            .0
            .artists()?
            .into_iter()
            .map(|artist| self.artist(artist))
            .collect())
    }

    fn albums_of(
        &self,
        artist: &MediaLocation,
    ) -> resonate_providers::Result<Vec<resonate_ui::ServedAlbum>> {
        let Some(Named::Artist(id)) = self.0.named_by(artist) else {
            return Err(self.unreadable());
        };
        Ok(self
            .0
            .artist(&id)?
            .albums
            .into_iter()
            .map(|album| self.album(album))
            .collect())
    }

    fn songs_of(
        &self,
        album: &MediaLocation,
    ) -> resonate_providers::Result<Vec<resonate_ui::ServedSong>> {
        let Some(Named::Album(id)) = self.0.named_by(album) else {
            return Err(self.unreadable());
        };
        Ok(self
            .0
            .album(&id)?
            .songs
            .into_iter()
            .map(|song| self.song(song))
            .collect())
    }

    fn search(&self, words: &str) -> resonate_providers::Result<resonate_ui::ServedFound> {
        let found = self.0.search(words)?;
        Ok(resonate_ui::ServedFound {
            artists: found
                .artists
                .into_iter()
                .map(|artist| self.artist(artist))
                .collect(),
            albums: found
                .albums
                .into_iter()
                .map(|album| self.album(album))
                .collect(),
            songs: found
                .songs
                .into_iter()
                .map(|song| self.song(song))
                .collect(),
        })
    }

    fn expand(&self, named: &MediaLocation) -> resonate_providers::Result<Vec<MediaLocation>> {
        let Some(named) = self.0.named_by(named) else {
            return Err(self.unreadable());
        };
        if let Named::Song(_) = named {
            return Ok(vec![self.0.location_of(&named)]);
        }
        Ok(self
            .0
            .songs_of(&named)?
            .into_iter()
            .map(|song| self.0.location_of(&Named::Song(song.id)))
            .collect())
    }

    fn cover(
        &self,
        of: &MediaLocation,
        known: Option<&resonate_ui::ServedCover>,
        side: u32,
    ) -> resonate_providers::Result<Vec<u8>> {
        let cover = match known {
            Some(known) => Id::new(&known.0).ok_or_else(|| self.unreadable())?,
            None => {
                let named = self.0.named_by(of).ok_or_else(|| self.unreadable())?;
                self.0.cover_of(&named)?
            }
        };
        self.0.cover(&cover, side)
    }
}

#[cfg(feature = "online")]
struct Streaming(Streamed);

#[cfg(feature = "online")]
impl Read for Streaming {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(feature = "online")]
impl Seek for Streaming {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.0.seek(to)
    }
}

#[cfg(feature = "online")]
impl MediaStream for Streaming {
    fn is_seekable(&self) -> bool {
        self.0.length().is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.0.length()
    }
}

#[cfg(feature = "online")]
pub fn browse(config: &Config, wanted: &SubsonicArgs) -> Result<()> {
    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    let subsonic = providers::server_named(config).ok_or(Error::NoSubsonicServer)?;

    if let Some(id) = wanted.artist.as_deref() {
        let held = subsonic.artist(&server_id(id)?)?;
        said!("{}", albums_table(&subsonic, &held.albums).render());
        return Ok(());
    }
    if let Some(id) = wanted.album.as_deref() {
        let held = subsonic.album(&server_id(id)?)?;
        said!("{}", songs_table(&subsonic, &held.songs).render());
        return Ok(());
    }
    if let Some(words) = wanted.search.as_deref() {
        let found = subsonic.search(words)?;
        said!("{}", artists_table(&subsonic, &found.artists).render());
        said!("{}", albums_table(&subsonic, &found.albums).render());
        said!("{}", songs_table(&subsonic, &found.songs).render());
        return Ok(());
    }
    said!(
        "{}",
        artists_table(&subsonic, &subsonic.artists()?).render()
    );
    Ok(())
}

#[cfg(not(feature = "online"))]
pub fn browse(_config: &Config, _wanted: &SubsonicArgs) -> Result<()> {
    Err(Error::NoSubsonic)
}

#[cfg(feature = "online")]
fn server_id(id: &str) -> Result<Id> {
    Id::new(id).ok_or(Error::BlankServerId)
}

#[cfg(feature = "online")]
fn uri_of(subsonic: &Subsonic, named: Named) -> String {
    subsonic.location_of(&named).to_uri()
}

#[cfg(feature = "online")]
fn counted(count: Option<u32>) -> String {
    count.map_or_else(|| "-".to_owned(), |count| count.to_string())
}

#[cfg(feature = "online")]
fn artists_table(subsonic: &Subsonic, artists: &[Artist]) -> Table {
    let mut table = Table::new(vec!["ARTIST", "ALBUMS", "URI"]);
    for artist in artists {
        table.push(vec![
            artist.name.clone(),
            counted(artist.albums),
            uri_of(subsonic, Named::Artist(artist.id.clone())),
        ]);
    }
    table
}

#[cfg(feature = "online")]
fn albums_table(subsonic: &Subsonic, albums: &[Album]) -> Table {
    let mut table = Table::new(vec!["ALBUM", "ARTIST", "YEAR", "SONGS", "URI"]);
    for album in albums {
        table.push(vec![
            album.name.clone(),
            album.artist.clone().unwrap_or_else(|| "-".to_owned()),
            album
                .year
                .map_or_else(|| "-".to_owned(), |year| year.to_string()),
            counted(album.songs),
            uri_of(subsonic, Named::Album(album.id.clone())),
        ]);
    }
    table
}

#[cfg(feature = "online")]
fn songs_table(subsonic: &Subsonic, songs: &[Song]) -> Table {
    let mut table = Table::new(vec!["#", "TITLE", "ARTIST", "LENGTH", "FORMAT", "URI"]);
    for song in songs {
        table.push(vec![
            counted(song.track),
            song.title.clone(),
            song.artist.clone().unwrap_or_else(|| "-".to_owned()),
            crate::played(song.length),
            song.suffix.clone().unwrap_or_else(|| "-".to_owned()),
            uri_of(subsonic, Named::Song(song.id.clone())),
        ]);
    }
    table
}

#[cfg(all(test, feature = "online"))]
mod tests {
    use std::{
        ffi::OsString,
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    use resonate_codec::{DecodeStatus, Decoder, probe};
    use resonate_core::{AudioBuffer, Frames, SampleData, SampleFormat};
    use resonate_subsonic::Server;

    use super::*;

    const FRAMES_A_BLOCK: usize = 192;
    const BLOCKS: usize = 6;
    const CHANNELS: usize = 2;
    const RATE: u64 = 44_100;
    const BITS: u64 = 16;
    const LAST_STREAMINFO: u8 = 0x80;
    const STREAMINFO_BYTES: u8 = 34;
    const FIXED_BLOCKING: [u8; 2] = [0xFF, 0xF8];
    const A_192_BLOCK_AT_44100: u8 = 0x19;
    const TWO_CHANNELS_OF_16_BITS: u8 = 0x18;
    const VERBATIM: u8 = 0x02;
    const CRC8_POLYNOMIAL: u8 = 0x07;
    const CRC16_POLYNOMIAL: u16 = 0x8005;
    const ALBUM: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","album":{"id":"al-1","name":"Meddle","song":[{"id":"so-1","title":"One","suffix":"flac"},{"id":"so-2","title":"Two","suffix":"flac"}]}}}"#;

    fn sample(frame: usize, channel: usize) -> i16 {
        i16::try_from((frame * 37 + channel * 1000) % 20_000).expect("a small sample") - 10_000
    }

    fn crc8(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0, |crc, byte| {
            (0..8).fold(crc ^ byte, |crc, _| {
                (crc << 1) ^ if crc & 0x80 == 0 { 0 } else { CRC8_POLYNOMIAL }
            })
        })
    }

    fn crc16(bytes: &[u8]) -> u16 {
        bytes.iter().fold(0, |crc, byte| {
            (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
                (crc << 1)
                    ^ if crc & 0x8000 == 0 {
                        0
                    } else {
                        CRC16_POLYNOMIAL
                    }
            })
        })
    }

    fn flac() -> Vec<u8> {
        let frames = (FRAMES_A_BLOCK * BLOCKS) as u64;
        let block = u16::try_from(FRAMES_A_BLOCK).expect("a small block");
        let mut file = b"fLaC".to_vec();
        file.extend_from_slice(&[LAST_STREAMINFO, 0, 0, STREAMINFO_BYTES]);
        file.extend_from_slice(&block.to_be_bytes());
        file.extend_from_slice(&block.to_be_bytes());
        file.extend_from_slice(&[0; 6]);
        let packed = (RATE << 44) | ((CHANNELS as u64 - 1) << 41) | ((BITS - 1) << 36) | frames;
        file.extend_from_slice(&packed.to_be_bytes());
        file.extend_from_slice(&[0; 16]);

        for nth in 0..BLOCKS {
            let mut frame = FIXED_BLOCKING.to_vec();
            frame.extend_from_slice(&[
                A_192_BLOCK_AT_44100,
                TWO_CHANNELS_OF_16_BITS,
                u8::try_from(nth).expect("a frame number under 128"),
            ]);
            frame.push(crc8(&frame));
            for channel in 0..CHANNELS {
                frame.push(VERBATIM);
                for at in 0..FRAMES_A_BLOCK {
                    frame.extend_from_slice(
                        &sample(nth * FRAMES_A_BLOCK + at, channel).to_be_bytes(),
                    );
                }
            }
            frame.extend_from_slice(&crc16(&frame).to_be_bytes());
            file.extend_from_slice(&frame);
        }
        file
    }

    fn range_of(header: &str) -> Option<usize> {
        let (name, value) = header.split_once(':')?;
        if !name.eq_ignore_ascii_case("range") {
            return None;
        }
        value
            .trim()
            .strip_prefix("bytes=")?
            .trim_end_matches('-')
            .parse()
            .ok()
    }

    fn answered(stream: TcpStream, file: &[u8]) {
        let mut reader = BufReader::new(stream);
        let mut asked = String::new();
        if reader.read_line(&mut asked).is_err() {
            return;
        }
        let mut from = None;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) if header == "\r\n" => break,
                Ok(_) => from = range_of(&header).or(from),
            }
        }
        let (head, body) = match from {
            _ if asked.contains("/rest/getAlbum") => (
                "200 OK\r\nContent-Type: application/json".to_owned(),
                ALBUM.as_bytes().to_vec(),
            ),
            Some(from) => (
                format!(
                    "206 Partial\r\nContent-Type: audio/flac\r\nContent-Range: bytes {from}-{}/{}",
                    file.len() - 1,
                    file.len()
                ),
                file[from..].to_vec(),
            ),
            None => (
                "200 OK\r\nContent-Type: audio/flac".to_owned(),
                file.to_vec(),
            ),
        };
        let mut stream = reader.into_inner();
        let _ = write!(
            stream,
            "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(&body);
    }

    fn serving() -> Served {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        let file = flac();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answered(stream, &file);
            }
        });

        Served::over(
            Some(Subsonic::at(Server {
                url,
                user: "listener".to_owned(),
                password: "sesame".to_owned(),
            })),
            false,
        )
    }

    fn decoded(decoder: &mut Decoder) -> Vec<i16> {
        decoder.set_output_format(SampleFormat::S16);
        let mut block = AudioBuffer::empty(decoder.info().spec);
        let mut samples = Vec::new();
        while decoder.next_block(&mut block).expect("a clean decode") == DecodeStatus::Decoded {
            match block.data() {
                SampleData::S16(held) => samples.extend_from_slice(held),
                other => panic!("the decoder ignored the format asked for: {other:?}"),
            }
        }
        samples
    }

    fn written(from: usize) -> Vec<i16> {
        (from..FRAMES_A_BLOCK * BLOCKS)
            .flat_map(|frame| (0..CHANNELS).map(move |channel| sample(frame, channel)))
            .collect()
    }

    #[test]
    fn an_album_on_the_server_is_queued_as_its_songs_and_each_decodes_as_streamed_bit_for_bit() {
        let served = serving();
        let sources = served.sources(Sources::local());

        let queued = crate::queue_items_served(&[OsString::from("subsonic:album/al-1")], &served);
        let uris: Vec<_> = queued.iter().map(|item| item.location.to_uri()).collect();
        let info = probe(&sources, &queued[0].location).expect("the song probes");
        let (mut decoder, opened) =
            Decoder::open(&sources, &queued[1].location).expect("the song opens");
        let whole = decoded(&mut decoder);

        assert_eq!(uris, ["subsonic:track/so-1", "subsonic:track/so-2"]);
        assert_eq!(info.bits_per_coded_sample, Some(16));
        assert_eq!(
            info.duration,
            Some(Frames((FRAMES_A_BLOCK * BLOCKS) as u64))
        );
        assert!(
            opened.is_seekable,
            "a stream of known length seeks by range"
        );
        assert_eq!(whole, written(0));
    }

    #[test]
    fn a_streamed_song_seeks_back_and_decodes_on_from_where_it_landed() {
        let served = serving();
        let sources = served.sources(Sources::local());
        let location = MediaLocation::from_uri("subsonic:track/so-1").expect("a uri");
        let (mut decoder, _) = Decoder::open(&sources, &location).expect("the song opens");
        let _ = decoded(&mut decoder);

        let landed = decoder
            .seek(Frames(FRAMES_A_BLOCK as u64 * 2))
            .expect("a seek back");
        let rest = decoded(&mut decoder);

        let from = usize::try_from(landed.0).expect("a small frame");
        assert_eq!(rest, written(from));
    }

    #[test]
    fn a_song_uri_is_a_file_here_unless_a_server_is_named_and_then_it_is_queued_as_named() {
        let named = OsString::from("subsonic:track/so-1");

        let unserved = crate::queue_items_served(std::slice::from_ref(&named), &Served::default());
        let served = crate::queue_items_served(std::slice::from_ref(&named), &serving());

        assert!(unserved[0].location.is_local());
        assert_eq!(served[0].location.to_uri(), "subsonic:track/so-1");
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_window_plays_a_server_once_an_account_is_given_and_not_before() {
        let url = serving()
            .slot
            .read()
            .as_ref()
            .map(|subsonic| subsonic.server().url.clone())
            .expect("a server");
        let window = Served::for_the_window(&crate::config::Config::default());
        let sources = window.sources(Sources::local());
        let location = MediaLocation::from_uri("subsonic:track/so-1").expect("a uri");
        let account = resonate_ui::Online {
            enabled: true,
            subsonic: url,
            subsonic_user: "listener".to_owned(),
            subsonic_password: "sesame".to_owned(),
            ..resonate_ui::Online::default()
        };

        let before = probe(&sources, &location);
        let opened = window.serving().opened(&account);
        let after = probe(&sources, &location);
        let withdrawn = window.serving().opened(&resonate_ui::Online::default());
        let again = probe(&sources, &location);

        assert!(before.is_err());
        assert!(opened.is_some());
        assert!(after.is_ok());
        assert!(withdrawn.is_none());
        assert!(again.is_err());
    }
}
