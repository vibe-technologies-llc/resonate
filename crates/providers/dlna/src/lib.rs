use std::{
    io,
    net::SocketAddr,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{SourceId, titles::Rating};
use resonate_providers::{
    Album, AlbumSongs, Artist, ArtistAlbums, Error, Found, Id, Named, PlaysInPlace, ProviderOp,
    Result, Song, Streamed, not_offered,
};
use resonate_upnp::{
    Container, Error as UpnpError, Item, Listing, Located, MediaServers, ObjectId, ServerFound, Udn,
};

pub const SCHEME: &str = "dlna";

pub const TITLE: &str = "DLNA";

pub const A_LISTING_IS_KEPT_FOR: Duration = Duration::from_secs(10);

const UDN_ENDS: char = '/';

pub struct Dlna {
    source: SourceId,
    servers: MediaServers,
    kept: Mutex<Option<Kept>>,
}

struct Kept {
    server: Udn,
    container: ObjectId,
    listing: Listing,
    read_at: Instant,
}

struct Object {
    server: Udn,
    id: ObjectId,
}

impl Object {
    fn read(id: &Id) -> Option<Self> {
        let (server, object) = id.as_str().split_once(UDN_ENDS)?;
        Some(Self {
            server: udn_of(server)?,
            id: ObjectId::new(object)?,
        })
    }

    fn id(server: &Udn, object: &ObjectId) -> Option<Id> {
        Id::new(&format!("{server}{UDN_ENDS}{object}"))
    }
}

fn udn_of(spelt: &str) -> Option<Udn> {
    let spelt = spelt.trim();
    let readable =
        !spelt.is_empty() && !spelt.contains(UDN_ENDS) && !spelt.chars().any(char::is_control);
    readable.then(|| Udn::new(spelt))
}

impl Dlna {
    pub fn on_the_network() -> Self {
        Self::over(MediaServers::on_the_network())
    }

    pub fn searching(to: SocketAddr, within: Duration) -> Self {
        Self::over(MediaServers::searching(to, within))
    }

    fn over(servers: MediaServers) -> Self {
        Self {
            source: SourceId::new(SCHEME).expect("the scheme is a source name"),
            servers,
            kept: Mutex::new(None),
        }
    }

    fn failed(&self, op: ProviderOp) -> impl Fn(UpnpError) -> Error + '_ {
        move |error| failure(&self.source, op, error)
    }

    fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op,
        }
    }

    fn object(&self, id: &Id, op: ProviderOp) -> Result<Object> {
        Object::read(id).ok_or_else(|| self.unreadable(op))
    }

    fn server(&self, udn: &Udn, op: ProviderOp) -> Result<ServerFound> {
        self.servers.server(udn).map_err(self.failed(op))
    }

    fn children(&self, object: &Object) -> Result<Listing> {
        if let Some(kept) = self.kept.lock().as_ref()
            && kept.server == object.server
            && kept.container == object.id
            && kept.read_at.elapsed() < A_LISTING_IS_KEPT_FOR
        {
            return Ok(kept.listing.clone());
        }
        let server = self.server(&object.server, ProviderOp::Browse)?;
        let listing = server
            .children(&object.id)
            .map_err(self.failed(ProviderOp::Browse))?;
        *self.kept.lock() = Some(Kept {
            server: object.server.clone(),
            container: object.id.clone(),
            listing: listing.clone(),
            read_at: Instant::now(),
        });
        Ok(listing)
    }

    fn described(&self, object: &Object, op: ProviderOp) -> Result<(ServerFound, Listing)> {
        let server = self.server(&object.server, op)?;
        let described = server.metadata(&object.id).map_err(self.failed(op))?;
        Ok((server, described))
    }

    fn album_from(server: &Udn, container: Container) -> Option<Album> {
        let id = Object::id(server, &container.id)?;
        Some(Album {
            cover: container
                .cover
                .as_ref()
                .and_then(|cover| cover_id(server, cover)),
            id,
            name: container.title,
            artist: container.artist,
            year: None,
            songs: container.children,
            rating: Rating::Unmarked,
        })
    }

    fn song_from(server: &Udn, item: Item) -> Option<Song> {
        let id = Object::id(server, &item.id)?;
        let played = item.played_from();
        Some(Song {
            id,
            length: played.and_then(|resource| resource.length),
            suffix: played
                .and_then(|resource| resource.mime.as_deref())
                .and_then(suffix_of)
                .map(str::to_owned),
            cover: item
                .cover
                .as_ref()
                .and_then(|cover| cover_id(server, cover)),
            title: item.title,
            artist: item.artist,
            album: item.album,
            disc: None,
            track: item.track,
            favourite: None,
            rating: Rating::Unmarked,
        })
    }

    fn albums_of(server: &Udn, listing: Listing) -> Vec<Album> {
        listing
            .containers
            .into_iter()
            .filter_map(|container| Self::album_from(server, container))
            .collect()
    }

    fn songs_in(server: &Udn, listing: Listing) -> Vec<Song> {
        listing
            .items
            .into_iter()
            .filter_map(|item| Self::song_from(server, item))
            .collect()
    }

    fn artist_of(server: &ServerFound) -> Option<Artist> {
        Some(Artist {
            id: Id::new(udn_of(server.udn().as_str())?.as_str())?,
            name: server.name().to_owned(),
            albums: None,
            cover: None,
        })
    }
}

fn cover_id(server: &Udn, cover: &Located) -> Option<Id> {
    Id::new(&format!("{server}{UDN_ENDS}{cover}"))
}

fn suffix_of(mime: &str) -> Option<&'static str> {
    let kind = mime.to_ascii_lowercase();
    let kind = kind.strip_prefix("audio/")?;
    Some(match kind.strip_prefix("x-").unwrap_or(kind) {
        "flac" => "flac",
        "mpeg" | "mp3" => "mp3",
        "mp4" | "m4a" => "m4a",
        "aac" => "aac",
        "ogg" | "vorbis" => "ogg",
        "opus" => "opus",
        "wav" | "wave" | "vnd.wave" => "wav",
        "aiff" => "aiff",
        "dsf" => "dsf",
        "dff" => "dff",
        "wavpack" => "wv",
        "ape" | "monkeys-audio" => "ape",
        _ => return None,
    })
}

fn failure(provider: &SourceId, op: ProviderOp, error: UpnpError) -> Error {
    let provider = provider.clone();
    match error {
        UpnpError::Status { status, .. } => Error::Refused {
            provider,
            op,
            status,
        },
        UpnpError::Fault {
            code: Some(code), ..
        } => Error::TurnedAway {
            provider,
            op,
            code: u16::try_from(code).unwrap_or(u16::MAX),
        },
        UpnpError::UnreadableAddress => Error::Unaddressable { provider, op },
        error @ UpnpError::NoSuchServer(_) => Error::Io {
            provider,
            op,
            source: io::Error::new(io::ErrorKind::NotFound, error),
        },
        error @ (UpnpError::Network { .. } | UpnpError::Search { .. }) => Error::Io {
            provider,
            op,
            source: io::Error::other(error),
        },
        error => {
            tracing::debug!(%error, "a media server's answer could not be read");
            Error::Unreadable { provider, op }
        }
    }
}

impl PlaysInPlace for Dlna {
    fn scheme(&self) -> &SourceId {
        &self.source
    }

    fn title(&self) -> &'static str {
        TITLE
    }

    fn artists(&self) -> Result<Vec<Artist>> {
        let found = self
            .servers
            .found()
            .map_err(self.failed(ProviderOp::Browse))?;
        Ok(found.iter().filter_map(Self::artist_of).collect())
    }

    fn artist(&self, id: &Id) -> Result<ArtistAlbums> {
        let udn = udn_of(id.as_str()).ok_or_else(|| self.unreadable(ProviderOp::Browse))?;
        let server = self.server(&udn, ProviderOp::Browse)?;
        let artist = Self::artist_of(&server).ok_or_else(|| self.unreadable(ProviderOp::Browse))?;
        let top = self.children(&Object {
            server: udn.clone(),
            id: ObjectId::root(),
        })?;
        Ok(ArtistAlbums {
            artist,
            albums: Self::albums_of(&udn, top),
        })
    }

    fn album(&self, id: &Id) -> Result<AlbumSongs> {
        let object = self.object(id, ProviderOp::Browse)?;
        let (_, described) = self.described(&object, ProviderOp::Browse)?;
        let container = described
            .containers
            .into_iter()
            .next()
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))?;
        let album = Self::album_from(&object.server, container)
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))?;
        let held = self.children(&object)?;
        Ok(AlbumSongs {
            album,
            songs: Self::songs_in(&object.server, held),
        })
    }

    fn albums_within(&self, album: &Id) -> Result<Vec<Album>> {
        let object = self.object(album, ProviderOp::Browse)?;
        let held = self.children(&object)?;
        Ok(Self::albums_of(&object.server, held))
    }

    fn search(&self, words: &str) -> Result<Found> {
        let held = self.servers.held();
        let servers = if held.is_empty() {
            self.servers
                .found()
                .map_err(self.failed(ProviderOp::Search))?
        } else {
            held
        };
        let mut found = Found::default();
        for server in servers {
            match server.songs_named(words) {
                Ok(listing) => {
                    let udn = server.udn().clone();
                    found.albums.extend(Self::albums_of(
                        &udn,
                        Listing {
                            containers: listing.containers.clone(),
                            items: Vec::new(),
                        },
                    ));
                    found.songs.extend(Self::songs_in(&udn, listing));
                }
                Err(error) => {
                    tracing::debug!(%error, server = server.name(), "a media server did not answer a search");
                }
            }
        }
        Ok(found)
    }

    fn streamed(&self, song: &Id) -> Result<Streamed> {
        let object = self.object(song, ProviderOp::Stream)?;
        let (server, described) = self.described(&object, ProviderOp::Stream)?;
        let item = described
            .items
            .into_iter()
            .next()
            .ok_or_else(|| self.unreadable(ProviderOp::Stream))?;
        let played = item
            .played_from()
            .ok_or_else(|| self.unreadable(ProviderOp::Stream))?;
        let fetched = server
            .fetched(&played.at)
            .map_err(self.failed(ProviderOp::Stream))?;
        let media_type = fetched
            .media_type()
            .map(str::to_owned)
            .or_else(|| played.mime.clone());
        let length = fetched.length().or(played.size);
        Ok(Streamed::seeking(fetched, length, media_type))
    }

    fn cover(&self, cover: &Id, _side: u32) -> Result<Vec<u8>> {
        let (udn, at) = cover
            .as_str()
            .split_once(UDN_ENDS)
            .ok_or_else(|| self.unreadable(ProviderOp::Cover))?;
        let udn = udn_of(udn).ok_or_else(|| self.unreadable(ProviderOp::Cover))?;
        let at = Located::parse(at).map_err(self.failed(ProviderOp::Cover))?;
        let server = self.server(&udn, ProviderOp::Cover)?;
        server.picture(&at).map_err(self.failed(ProviderOp::Cover))
    }

    fn cover_of(&self, named: &Named) -> Result<Id> {
        let id = match named {
            Named::Song(id) | Named::Album(id) => id,
            Named::Artist(_) | Named::Mix(_) => {
                return Err(not_offered(&self.source, ProviderOp::Cover));
            }
        };
        let object = self.object(id, ProviderOp::Cover)?;
        let (_, described) = self.described(&object, ProviderOp::Cover)?;
        described
            .containers
            .iter()
            .filter_map(|container| container.cover.as_ref())
            .chain(
                described
                    .items
                    .iter()
                    .filter_map(|item| item.cover.as_ref()),
            )
            .find_map(|cover| cover_id(&object.server, cover))
            .ok_or_else(|| not_offered(&self.source, ProviderOp::Cover))
    }

    fn songs_of(&self, named: &Named) -> Result<Vec<Song>> {
        match named {
            Named::Song(id) => Ok(vec![Song::only(id.clone())]),
            Named::Album(id) => {
                let object = self.object(id, ProviderOp::Browse)?;
                let held = self.children(&object)?;
                Ok(Self::songs_in(&object.server, held))
            }
            Named::Artist(_) | Named::Mix(_) => Err(not_offered(&self.source, ProviderOp::Browse)),
        }
    }

    fn named_by(&self, location: &resonate_core::MediaLocation) -> Option<Named> {
        Named::read(location, &self.source).filter(|named| !matches!(named, Named::Mix(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_is_named_by_its_servers_udn_and_its_own_id_and_reads_back_as_itself() {
        let server = Udn::new("uuid:4d696e69-444c-164e-9d41-001132a1b2c3");
        let object = ObjectId::new("64$0/1").expect("an object id");

        let id = Object::id(&server, &object).expect("an id");
        let read = Object::read(&id).expect("reads back");

        assert_eq!(
            id.as_str(),
            "uuid:4d696e69-444c-164e-9d41-001132a1b2c3/64$0/1"
        );
        assert_eq!(read.server, server);
        assert_eq!(read.id, object);
        assert!(Object::read(&Id::new("no-object").expect("an id")).is_none());
        assert!(Object::read(&Id::new("/0").expect("an id")).is_none());
    }

    #[test]
    fn a_media_type_names_the_suffix_a_decoder_is_hinted_by() {
        assert_eq!(suffix_of("audio/x-flac"), Some("flac"));
        assert_eq!(suffix_of("audio/mpeg"), Some("mp3"));
        assert_eq!(suffix_of("AUDIO/WAV"), Some("wav"));
        assert_eq!(suffix_of("video/mp4"), None);
    }

    #[test]
    fn a_server_gone_is_the_provider_away_and_a_fault_is_the_servers_refusal() {
        let source = SourceId::new(SCHEME).expect("a source");

        let gone = failure(
            &source,
            ProviderOp::Browse,
            UpnpError::NoSuchServer(Udn::new("uuid:x")),
        );
        let refused = failure(
            &source,
            ProviderOp::Browse,
            UpnpError::Fault {
                action: resonate_upnp::Action::Browse,
                code: Some(701),
            },
        );

        assert!(gone.is_the_provider_away());
        assert!(matches!(refused, Error::TurnedAway { code: 701, .. }));
        assert!(!refused.is_the_provider_away());
    }
}
