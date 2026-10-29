use std::path::PathBuf;
#[cfg(feature = "online")]
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
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
use resonate_providers::{Album, Artist, Found, Id, Mix, Named, PlaysInPlace, Song, Streamed};

#[cfg(feature = "online")]
use crate::providers::{self, InPlace};
#[cfg(feature = "online")]
use crate::table::Table;
use crate::{
    cli::BrowsingArgs,
    config::Config,
    error::{Error, Result},
};

#[cfg(feature = "online")]
const PLAYED_IN_PLACE: [&str; 5] = ["subsonic", "tidal", "resonate", "hifi-api", "monochrome"];

#[derive(Clone, Default)]
pub struct Served {
    #[cfg(feature = "online")]
    slot: Arc<RwLock<InPlace>>,
    #[cfg(feature = "online")]
    live: bool,
    #[cfg(all(feature = "ui", feature = "online"))]
    settings: Option<PathBuf>,
}

impl Served {
    #[cfg(feature = "online")]
    pub fn of(config: &Config, settings: Option<PathBuf>) -> Self {
        Self::over(providers::in_place_named(config, settings), false)
    }

    #[cfg(not(feature = "online"))]
    pub fn of(_config: &Config, _settings: Option<PathBuf>) -> Self {
        Self {}
    }

    #[cfg(feature = "online")]
    fn over(services: InPlace, live: bool) -> Self {
        Self {
            slot: Arc::new(RwLock::new(services)),
            live,
            #[cfg(feature = "ui")]
            settings: None,
        }
    }

    #[cfg(all(feature = "ui", feature = "online"))]
    pub fn for_the_window(config: &Config, settings: Option<PathBuf>) -> Self {
        Self {
            settings: settings.clone(),
            ..Self::over(providers::in_place_named(config, settings), true)
        }
    }

    #[cfg(all(feature = "ui", not(feature = "online")))]
    pub fn for_the_window(_config: &Config, _settings: Option<PathBuf>) -> Self {
        Self {}
    }

    #[cfg(feature = "online")]
    fn schemes(&self) -> Vec<SourceId> {
        if self.live {
            return PLAYED_IN_PLACE
                .iter()
                .filter_map(|scheme| SourceId::new(scheme).ok())
                .collect();
        }
        self.slot
            .read()
            .iter()
            .map(|service| service.scheme().clone())
            .collect()
    }

    #[cfg(feature = "online")]
    pub fn sources(&self, sources: Sources) -> Sources {
        self.schemes().into_iter().fold(sources, |sources, scheme| {
            sources.and(Arc::new(ServedSongs {
                scheme,
                slot: Arc::clone(&self.slot),
            }))
        })
    }

    #[cfg(not(feature = "online"))]
    pub fn sources(&self, sources: Sources) -> Sources {
        sources
    }

    #[cfg(feature = "online")]
    pub fn songs_at(&self, location: &MediaLocation) -> Option<Vec<MediaLocation>> {
        let service = serving(&self.slot.read(), location)?;
        let named = service.named_by(location)?;
        if let Named::Song(_) = named {
            return Some(vec![location.clone()]);
        }
        match service.songs_of(&named) {
            Ok(songs) => Some(
                songs
                    .into_iter()
                    .map(|song| service.location_of(&Named::Song(song.id)))
                    .collect(),
            ),
            Err(error) => {
                tracing::warn!(%error, %location, "the service did not list what was named");
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
        let settings = self.settings.clone();
        let adapted: Mutex<Vec<Arc<ServedBy>>> = Mutex::new(Vec::new());
        resonate_ui::Serving::new(move |online| {
            let services = providers::in_place_given(online, settings.clone());
            let mut held = adapted.lock();
            let serving: Vec<Arc<ServedBy>> = services
                .iter()
                .map(|service| {
                    held.iter()
                        .find(|kept| Arc::ptr_eq(&kept.0, service))
                        .cloned()
                        .unwrap_or_else(|| Arc::new(ServedBy(Arc::clone(service))))
                })
                .collect();
            held.clone_from(&serving);
            *slot.write() = services;
            serving
                .into_iter()
                .map(|served| served as Arc<dyn resonate_ui::Serves>)
                .collect()
        })
    }

    #[cfg(all(feature = "ui", not(feature = "online")))]
    pub fn serving(&self) -> resonate_ui::Serving {
        resonate_ui::Serving::none()
    }
}

#[cfg(feature = "online")]
fn serving(
    services: &[Arc<dyn PlaysInPlace>],
    location: &MediaLocation,
) -> Option<Arc<dyn PlaysInPlace>> {
    services
        .iter()
        .find(|service| service.scheme() == location.source())
        .cloned()
}

#[cfg(feature = "online")]
struct ServedSongs {
    scheme: SourceId,
    slot: Arc<RwLock<InPlace>>,
}

#[cfg(feature = "online")]
impl MediaProvider for ServedSongs {
    fn source(&self) -> &SourceId {
        &self.scheme
    }

    fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
        let service = serving(&self.slot.read(), location);
        let named = service
            .as_ref()
            .and_then(|service| service.named_by(location));
        let (Some(service), Some(Named::Song(id))) = (service, named) else {
            return Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            });
        };
        let streamed = service
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
struct ServedBy(Arc<dyn PlaysInPlace>);

#[cfg(all(feature = "ui", feature = "online"))]
impl ServedBy {
    fn unreadable(&self) -> resonate_providers::Error {
        resonate_providers::Error::Unreadable {
            provider: self.0.scheme().clone(),
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

    fn mix(&self, mix: Mix) -> resonate_ui::ServedAlbum {
        resonate_ui::ServedAlbum {
            location: self.0.location_of(&Named::Mix(mix.id)),
            name: mix.name,
            artist: mix.subtitle,
            year: None,
            songs: None,
            cover: None,
        }
    }

    fn found(&self, found: Found) -> resonate_ui::ServedFound {
        resonate_ui::ServedFound {
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
            mixes: found.mixes.into_iter().map(|mix| self.mix(mix)).collect(),
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
            favourite: song.favourite,
        }
    }
}

#[cfg(all(feature = "ui", feature = "online"))]
impl resonate_ui::Serves for ServedBy {
    fn scheme(&self) -> &SourceId {
        self.0.scheme()
    }

    fn title(&self) -> &str {
        self.0.title()
    }

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
        let named = match self.0.named_by(album) {
            Some(named @ (Named::Album(_) | Named::Mix(_))) => named,
            _ => return Err(self.unreadable()),
        };
        Ok(self
            .0
            .songs_of(&named)?
            .into_iter()
            .map(|song| self.song(song))
            .collect())
    }

    fn first_page(&self) -> resonate_providers::Result<resonate_ui::ServedFound> {
        Ok(self.found(self.0.first_page()?))
    }

    fn search(&self, words: &str) -> resonate_providers::Result<resonate_ui::ServedFound> {
        Ok(self.found(self.0.search(words)?))
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

    fn keeps_favourites(&self) -> bool {
        self.0.keeps_favourites()
    }

    fn favour(&self, song: &MediaLocation, favoured: bool) -> resonate_providers::Result<()> {
        match self.0.named_by(song) {
            Some(Named::Song(id)) => self.0.favour(&id, favoured),
            _ => Err(self.unreadable()),
        }
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
        self.0.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.0.length()
    }
}

#[cfg(feature = "online")]
pub fn browse(
    config: &Config,
    settings: Option<PathBuf>,
    service: Option<&str>,
    wanted: &BrowsingArgs,
) -> Result<()> {
    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    let services = providers::in_place_named(config, settings);
    let chosen = chosen(services, service)?;
    for table in tables(chosen.as_ref(), wanted)? {
        said!("{}", table.render());
    }
    Ok(())
}

#[cfg(feature = "online")]
pub fn browse_subsonic(
    config: &Config,
    settings: Option<PathBuf>,
    wanted: &BrowsingArgs,
) -> Result<()> {
    if !config.online_enabled() {
        return Err(Error::OnlineOff);
    }
    let services = providers::in_place_named(config, settings);
    let subsonic = chosen(services, Some(SUBSONIC)).map_err(|_| Error::NoSubsonicServer)?;
    for table in tables(subsonic.as_ref(), wanted)? {
        said!("{}", table.render());
    }
    Ok(())
}

#[cfg(not(feature = "online"))]
pub fn browse(
    _config: &Config,
    _settings: Option<PathBuf>,
    _service: Option<&str>,
    _wanted: &BrowsingArgs,
) -> Result<()> {
    Err(Error::NoServed)
}

#[cfg(not(feature = "online"))]
pub fn browse_subsonic(
    _config: &Config,
    _settings: Option<PathBuf>,
    _wanted: &BrowsingArgs,
) -> Result<()> {
    Err(Error::NoServed)
}

#[cfg(feature = "online")]
const SUBSONIC: &str = "subsonic";

#[cfg(feature = "online")]
fn chosen(services: InPlace, asked: Option<&str>) -> Result<Arc<dyn PlaysInPlace>> {
    let Some(asked) = asked else {
        return services.into_iter().next().ok_or(Error::NoServedService);
    };
    let named = asked.trim();
    services
        .into_iter()
        .find(|service| service.scheme().as_str().eq_ignore_ascii_case(named))
        .ok_or_else(|| Error::ServiceNotServed {
            service: named.to_owned(),
        })
}

#[cfg(feature = "online")]
fn tables(service: &dyn PlaysInPlace, wanted: &BrowsingArgs) -> Result<Vec<Table>> {
    if let Some(id) = wanted.artist.as_deref() {
        let held = service.artist(&server_id(id)?)?;
        return Ok(vec![albums_table(service, &held.albums)]);
    }
    if let Some(id) = wanted.album.as_deref() {
        let held = service.album(&server_id(id)?)?;
        return Ok(vec![songs_table(service, &held.songs)]);
    }
    for (id, favoured) in [
        (wanted.favour.as_deref(), true),
        (wanted.unfavour.as_deref(), false),
    ] {
        if let Some(id) = id {
            let song = server_id(id)?;
            service.favour(&song, favoured)?;
            return Ok(vec![favoured_table(service, &song, favoured)]);
        }
    }
    if let Some(id) = wanted.mix.as_deref() {
        let held = service.mix(&server_id(id)?)?;
        return Ok(vec![songs_table(service, &held.songs)]);
    }
    let found = match wanted.search.as_deref() {
        Some(words) => service.search(words)?,
        None => service.first_page()?,
    };
    Ok(found_tables(service, &found))
}

#[cfg(feature = "online")]
fn found_tables(service: &dyn PlaysInPlace, found: &Found) -> Vec<Table> {
    let mut tables = Vec::new();
    if !found.artists.is_empty() {
        tables.push(artists_table(service, &found.artists));
    }
    if !found.albums.is_empty() {
        tables.push(albums_table(service, &found.albums));
    }
    if !found.mixes.is_empty() {
        tables.push(mixes_table(service, &found.mixes));
    }
    if !found.songs.is_empty() || tables.is_empty() {
        tables.push(songs_table(service, &found.songs));
    }
    tables
}

#[cfg(feature = "online")]
fn server_id(id: &str) -> Result<Id> {
    Id::new(id).ok_or(Error::BlankServerId)
}

#[cfg(feature = "online")]
fn uri_of(service: &dyn PlaysInPlace, named: Named) -> String {
    service.location_of(&named).to_uri()
}

#[cfg(feature = "online")]
fn counted(count: Option<u32>) -> String {
    count.map_or_else(|| "-".to_owned(), |count| count.to_string())
}

#[cfg(feature = "online")]
fn artists_table(service: &dyn PlaysInPlace, artists: &[Artist]) -> Table {
    let mut table = Table::new(vec!["ARTIST", "ALBUMS", "URI"]);
    for artist in artists {
        table.push(vec![
            artist.name.clone(),
            counted(artist.albums),
            uri_of(service, Named::Artist(artist.id.clone())),
        ]);
    }
    table
}

#[cfg(feature = "online")]
fn albums_table(service: &dyn PlaysInPlace, albums: &[Album]) -> Table {
    let mut table = Table::new(vec!["ALBUM", "ARTIST", "YEAR", "SONGS", "URI"]);
    for album in albums {
        table.push(vec![
            album.name.clone(),
            album.artist.clone().unwrap_or_else(|| "-".to_owned()),
            album
                .year
                .map_or_else(|| "-".to_owned(), |year| year.to_string()),
            counted(album.songs),
            uri_of(service, Named::Album(album.id.clone())),
        ]);
    }
    table
}

#[cfg(feature = "online")]
fn favoured_table(service: &dyn PlaysInPlace, song: &Id, favoured: bool) -> Table {
    let mut table = Table::new(vec!["FAVOURITE", "URI"]);
    table.push(vec![
        if favoured { "kept" } else { "taken out" }.to_owned(),
        uri_of(service, Named::Song(song.clone())),
    ]);
    table
}

#[cfg(feature = "online")]
fn mixes_table(service: &dyn PlaysInPlace, mixes: &[Mix]) -> Table {
    let mut table = Table::new(vec!["MIX", "ABOUT", "URI"]);
    for mix in mixes {
        table.push(vec![
            mix.name.clone(),
            mix.subtitle.clone().unwrap_or_else(|| "-".to_owned()),
            uri_of(service, Named::Mix(mix.id.clone())),
        ]);
    }
    table
}

#[cfg(feature = "online")]
fn songs_table(service: &dyn PlaysInPlace, songs: &[Song]) -> Table {
    let mut table = Table::new(vec!["#", "TITLE", "ARTIST", "LENGTH", "FORMAT", "URI"]);
    for song in songs {
        table.push(vec![
            counted(song.track),
            song.title.clone(),
            song.artist.clone().unwrap_or_else(|| "-".to_owned()),
            crate::played(song.length),
            song.suffix.clone().unwrap_or_else(|| "-".to_owned()),
            uri_of(service, Named::Song(song.id.clone())),
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

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use resonate_codec::{DecodeStatus, Decoder, probe};
    use resonate_core::{AudioBuffer, Frames, SampleData, SampleFormat};
    use resonate_monochrome::Monochrome;
    use resonate_subsonic::{Server, Subsonic};
    use resonate_tidal::{Account, Endpoints, HifiApi, MediaHosts, Tidal};

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
    const TIDAL_ALBUM: &str = r#"{"items":[{"id":11,"title":"One"},{"id":12,"title":"Two"}]}"#;
    const KEY: &str = "Bearer hush";

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

    struct Asked {
        line: String,
        range: Option<usize>,
        authorization: Option<String>,
    }

    impl Asked {
        fn path(&self) -> &str {
            let target = self.line.split(' ').nth(1).unwrap_or_default();
            target.split('?').next().unwrap_or_default()
        }
    }

    enum Answer {
        Json(String),
        Song,
        Missing,
    }

    fn asked_of(reader: &mut BufReader<TcpStream>) -> Option<Asked> {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut range = None;
        let mut authorization = None;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return None,
                Ok(_) if header == "\r\n" => break,
                Ok(_) => {}
            }
            let Some((name, value)) = header.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "range" => {
                    range = value
                        .strip_prefix("bytes=")
                        .and_then(|from| from.trim_end_matches('-').parse().ok());
                }
                "authorization" => authorization = Some(value.to_owned()),
                _ => {}
            }
        }
        Some(Asked {
            line,
            range,
            authorization,
        })
    }

    fn answered(stream: TcpStream, file: &[u8], route: &dyn Fn(&Asked, &str) -> Answer, own: &str) {
        let mut reader = BufReader::new(stream);
        let Some(asked) = asked_of(&mut reader) else {
            return;
        };
        let (head, body) = match (route(&asked, own), asked.range) {
            (Answer::Json(body), _) => (
                "200 OK\r\nContent-Type: application/json".to_owned(),
                body.into_bytes(),
            ),
            (Answer::Song, Some(from)) => (
                format!(
                    "206 Partial\r\nContent-Type: audio/flac\r\nContent-Range: bytes {from}-{}/{}",
                    file.len() - 1,
                    file.len()
                ),
                file[from..].to_vec(),
            ),
            (Answer::Song, None) => (
                "200 OK\r\nContent-Type: audio/flac".to_owned(),
                file.to_vec(),
            ),
            (Answer::Missing, _) => (
                "404 Not Found\r\nContent-Type: application/json".to_owned(),
                b"{}".to_vec(),
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

    fn fake(route: impl Fn(&Asked, &str) -> Answer + Send + Sync + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        let file = flac();
        let own = url.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answered(stream, &file, &route, &own);
            }
        });
        url
    }

    fn subsonic_url() -> String {
        fake(|asked, _| {
            if asked.path() == "/rest/getAlbum" {
                Answer::Json(ALBUM.to_owned())
            } else {
                Answer::Song
            }
        })
    }

    fn subsonic_at(url: String) -> Arc<dyn PlaysInPlace> {
        Arc::new(Subsonic::at(Server {
            url,
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
        }))
    }

    fn serving() -> Served {
        Served::over(vec![subsonic_at(subsonic_url())], false)
    }

    fn on_loopback() -> MediaHosts {
        MediaHosts {
            scheme: "http".to_owned(),
            domain: "127.0.0.1".to_owned(),
        }
    }

    fn one_flac_playback(own: &str) -> String {
        let manifest = STANDARD.encode(format!(
            r#"{{"mimeType":"audio/flac","codecs":"flac","encryptionType":"NONE","urls":["{own}/media/whole.flac"]}}"#
        ));
        format!(
            r#"{{"assetPresentation":"FULL","manifestMimeType":"application/vnd.tidal.bts","manifest":"{manifest}"}}"#
        )
    }

    fn tidal_serving() -> Served {
        let url = fake(|asked, own| match asked.path() {
            "/auth/token" => Answer::Json(
                r#"{"access_token":"fresh","expires_in":3600,"user":{"countryCode":"GB"}}"#
                    .to_owned(),
            ),
            "/api/albums/7/tracks" => Answer::Json(TIDAL_ALBUM.to_owned()),
            "/api/tracks/11/playbackinfopostpaywall" => Answer::Json(one_flac_playback(own)),
            "/media/whole.flac" => Answer::Song,
            _ => Answer::Missing,
        });
        Served::over(vec![Arc::new(tidal_at(&url))], false)
    }

    fn tidal_at(url: &str) -> Tidal {
        Tidal::at(
            Account {
                client_id: "client".to_owned(),
                client_secret: None,
                refresh_token: "sesame".to_owned(),
            },
            Endpoints {
                auth: format!("{url}/auth/token"),
                device: format!("{url}/auth/device"),
                api: format!("{url}/api/"),
                openapi: format!("{url}/openapi/"),
                images: format!("{url}/images/"),
                media: on_loopback(),
            },
        )
    }

    fn browsing() -> BrowsingArgs {
        BrowsingArgs {
            artist: None,
            album: None,
            mix: None,
            search: None,
            favour: None,
            unfavour: None,
        }
    }

    #[test]
    fn the_terminal_browses_the_service_named_and_a_tidal_accounts_first_page_is_its_favourites() {
        let url = fake(|asked, _| {
            match asked.path() {
            "/auth/token" => Answer::Json(
                r#"{"access_token":"fresh","expires_in":3600,"user":{"countryCode":"GB","userId":42}}"#
                    .to_owned(),
            ),
            "/api/users/42/favorites/albums" => Answer::Json(
                r#"{"items":[{"item":{"id":7,"title":"Meddle","artist":{"name":"Pink Floyd"}}}]}"#
                    .to_owned(),
            ),
            "/api/users/42/favorites/tracks" => Answer::Json(
                r#"{"items":[{"item":{"id":12,"title":"Echoes","duration":1412,"artist":{"name":"Pink Floyd"}}}]}"#
                    .to_owned(),
            ),
            _ => Answer::Missing,
        }
        });
        let services: InPlace = vec![subsonic_at(subsonic_url()), Arc::new(tidal_at(&url))];

        let first = chosen(services.clone(), None).expect("the first service set");
        let tidal = chosen(services.clone(), Some(" TIDAL ")).expect("the service named");
        let unset = chosen(services, Some("monochrome"));
        let none = chosen(Vec::new(), None);
        let rendered: Vec<String> = tables(tidal.as_ref(), &browsing())
            .expect("the first page")
            .iter()
            .map(Table::render)
            .collect();

        assert_eq!(first.scheme().as_str(), "subsonic");
        assert_eq!(tidal.scheme().as_str(), "tidal");
        assert!(
            matches!(unset, Err(Error::ServiceNotServed { service }) if service == "monochrome")
        );
        assert!(matches!(none, Err(Error::NoServedService)));
        assert_eq!(rendered.len(), 2);
        assert!(rendered[0].contains("Meddle") && rendered[0].contains("tidal:album/7"));
        assert!(rendered[1].contains("Echoes") && rendered[1].contains("tidal:track/12"));
    }

    #[test]
    fn a_tidal_accounts_favourite_mixes_are_listed_and_browsed_and_a_song_is_favoured_at_the_terminal()
     {
        let url = fake(|asked, _| {
            match asked.path() {
            "/auth/token" => Answer::Json(
                r#"{"access_token":"fresh","expires_in":3600,"user":{"countryCode":"GB","userId":42}}"#
                    .to_owned(),
            ),
            "/api/users/42/favorites/mixes" => Answer::Json(
                r#"{"items":[{"id":"0a1b","title":"My Mix 1","subTitle":"Pink Floyd and more"}]}"#
                    .to_owned(),
            ),
            "/api/mixes/0a1b/items" => Answer::Json(
                r#"{"items":[{"item":{"id":12,"title":"Echoes","duration":1412,"artist":{"name":"Pink Floyd"}},"type":"track"}]}"#
                    .to_owned(),
            ),
            "/api/users/42/favorites/tracks" => Answer::Json("{}".to_owned()),
            _ => Answer::Missing,
        }
        });
        let tidal = Arc::new(tidal_at(&url));

        let first = tables(tidal.as_ref(), &browsing()).expect("the first page");
        let mix = tables(
            tidal.as_ref(),
            &BrowsingArgs {
                mix: Some("0a1b".to_owned()),
                ..browsing()
            },
        )
        .expect("a mix");
        let favoured = tables(
            tidal.as_ref(),
            &BrowsingArgs {
                favour: Some("12".to_owned()),
                ..browsing()
            },
        )
        .expect("a favourite kept");

        assert_eq!(first.len(), 1);
        assert!(first[0].render().contains("My Mix 1"));
        assert!(first[0].render().contains("Pink Floyd and more"));
        assert!(first[0].render().contains("tidal:mix/0a1b"));
        assert!(mix[0].render().contains("Echoes") && mix[0].render().contains("tidal:track/12"));
        assert!(favoured[0].render().contains("kept"));
        assert!(favoured[0].render().contains("tidal:track/12"));
    }

    #[test]
    fn a_subsonic_servers_first_page_at_the_terminal_is_its_album_by_id_and_a_search_finding_nothing_is_one_empty_table()
     {
        let url = fake(|asked, _| match asked.path() {
            "/rest/getAlbum" => Answer::Json(ALBUM.to_owned()),
            "/rest/search3" => Answer::Json(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{}}}"#
                    .to_owned(),
            ),
            _ => Answer::Missing,
        });
        let subsonic = subsonic_at(url);

        let album = tables(
            subsonic.as_ref(),
            &BrowsingArgs {
                album: Some("al-1".to_owned()),
                ..browsing()
            },
        )
        .expect("an album");
        let nothing = tables(
            subsonic.as_ref(),
            &BrowsingArgs {
                search: Some("nothing".to_owned()),
                ..browsing()
            },
        )
        .expect("a search");
        let blank = tables(
            subsonic.as_ref(),
            &BrowsingArgs {
                artist: Some(" ".to_owned()),
                ..browsing()
            },
        );

        assert!(album[0].render().contains("subsonic:track/so-2"));
        assert_eq!(nothing.len(), 1);
        assert!(matches!(blank, Err(Error::BlankServerId)));
    }

    fn monochrome_serving() -> Served {
        let url = fake(|asked, _| match asked.path() {
            "/track/154140652551016448" => Answer::Song,
            _ => Answer::Missing,
        });
        Served::over(vec![Arc::new(Monochrome::at(&url))], false)
    }

    fn keyed_serving() -> Served {
        let url = fake(|asked, _| match asked.path() {
            "/file/" if asked.authorization.as_deref() == Some(KEY) => Answer::Song,
            _ => Answer::Missing,
        });
        let keyed = HifiApi::at(&url).with_secret("hush").known_as_resonate();
        Served::over(vec![Arc::new(keyed)], false)
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

    fn decodes_bit_for_bit_and_seeks_back(served: &Served, uri: &str) {
        let sources = served.sources(Sources::local());
        let location = MediaLocation::from_uri(uri).expect("a uri");

        let (mut decoder, opened) = Decoder::open(&sources, &location).expect("the song opens");
        let whole = decoded(&mut decoder);
        let landed = decoder
            .seek(Frames(FRAMES_A_BLOCK as u64 * 2))
            .expect("a seek back");
        let rest = decoded(&mut decoder);

        assert!(opened.is_seekable, "{uri} is seekable by range");
        assert_eq!(whole, written(0), "{uri}");
        let from = usize::try_from(landed.0).expect("a small frame");
        assert_eq!(rest, written(from), "{uri}");
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
        decodes_bit_for_bit_and_seeks_back(&serving(), "subsonic:track/so-1");
    }

    #[test]
    fn a_song_uri_is_a_file_here_unless_a_server_is_named_and_then_it_is_queued_as_named() {
        let named = OsString::from("subsonic:track/so-1");

        let unserved = crate::queue_items_served(std::slice::from_ref(&named), &Served::default());
        let served = crate::queue_items_served(std::slice::from_ref(&named), &serving());

        assert!(unserved[0].location.is_local());
        assert_eq!(served[0].location.to_uri(), "subsonic:track/so-1");
    }

    #[test]
    fn a_tidal_album_is_queued_as_its_songs_and_a_song_in_one_flac_file_decodes_and_seeks() {
        let served = tidal_serving();

        let queued = crate::queue_items_served(&[OsString::from("tidal:album/7")], &served);
        let uris: Vec<_> = queued.iter().map(|item| item.location.to_uri()).collect();

        assert_eq!(uris, ["tidal:track/11", "tidal:track/12"]);
        decodes_bit_for_bit_and_seeks_back(&served, "tidal:track/11");
    }

    #[test]
    fn a_monochrome_song_decodes_bit_for_bit_and_seeks() {
        decodes_bit_for_bit_and_seeks_back(
            &monochrome_serving(),
            "monochrome:track/154140652551016448",
        );
    }

    #[test]
    fn a_song_the_keyed_server_sends_decodes_bit_for_bit_and_seeks_with_the_key() {
        decodes_bit_for_bit_and_seeks_back(&keyed_serving(), "resonate:track/11");
    }

    #[test]
    fn a_uri_of_a_service_not_set_is_a_file_here() {
        let named = OsString::from("tidal:track/11");

        let queued = crate::queue_items_served(std::slice::from_ref(&named), &serving());

        assert!(queued[0].location.is_local());
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_window_plays_a_server_once_an_account_is_given_and_not_before() {
        let url = subsonic_url();
        let window = Served::for_the_window(&crate::config::Config::default(), None);
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
        assert_eq!(opened.len(), 1);
        assert!(after.is_ok());
        assert!(withdrawn.is_empty());
        assert!(again.is_err());
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_window_offers_every_service_set_and_keeps_each_while_its_settings_stand() {
        let window = Served::for_the_window(&crate::config::Config::default(), None);
        let serving = window.serving();
        let account = resonate_ui::Online {
            enabled: true,
            subsonic: "http://127.0.0.1:9".to_owned(),
            subsonic_user: "listener".to_owned(),
            subsonic_password: "sesame".to_owned(),
            tidal_client_id: "client".to_owned(),
            tidal_refresh_token: "token".to_owned(),
            monochrome: "http://127.0.0.1:9".to_owned(),
            ..resonate_ui::Online::default()
        };

        let opened = serving.opened(&account);
        let again = serving.opened(&account);
        let titles: Vec<_> = opened
            .iter()
            .map(|served| served.title().to_owned())
            .collect();

        assert_eq!(titles, ["Subsonic", "TIDAL", "Monochrome"]);
        assert!(
            opened
                .iter()
                .zip(&again)
                .all(|(opened, again)| Arc::ptr_eq(opened, again))
        );
    }
}
