use std::{fmt, time::Duration};

use roxmltree::Node;

use crate::{Asked, Error, Located, Result, description::document};

pub const ENTRIES_A_LISTING_AT_MOST: usize = 5_000;

pub const TEXT_CHARS_AT_MOST: usize = 512;

const OBJECT_ID_BYTES_AT_MOST: usize = 1_024;

const RESOURCES_AN_ITEM_AT_MOST: usize = 16;

const ROOT: &str = "0";

const AUDIO_CLASS: &str = "object.item.audioItem";

const ALBUM_CLASS: &str = "object.container.album";

const HTTP_GET: &str = "http-get";

const CONVERTED: &str = "DLNA.ORG_CI=1";

const HOURS_AT_MOST: u64 = 9_999;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(Box<str>);

impl ObjectId {
    pub fn root() -> Self {
        Self(Box::from(ROOT))
    }

    pub fn new(id: &str) -> Option<Self> {
        let readable = !id.is_empty()
            && id.len() <= OBJECT_ID_BYTES_AT_MOST
            && !id.chars().any(char::is_control);
        readable.then(|| Self(Box::from(id)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        &*self.0 == ROOT
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub containers: Vec<Container>,
    pub items: Vec<Item>,
}

impl Listing {
    pub fn len(&self) -> usize {
        self.containers.len() + self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn extend(&mut self, more: Self) {
        let room = ENTRIES_A_LISTING_AT_MOST.saturating_sub(self.len());
        let containers = more.containers.len().min(room);
        self.containers
            .extend(more.containers.into_iter().take(containers));
        let room = room - containers;
        self.items.extend(more.items.into_iter().take(room));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub id: ObjectId,
    pub title: String,
    pub class: String,
    pub children: Option<u32>,
    pub artist: Option<String>,
    pub cover: Option<Located>,
}

impl Container {
    pub fn is_an_album(&self) -> bool {
        self.class.starts_with(ALBUM_CLASS)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: ObjectId,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub track: Option<u32>,
    pub cover: Option<Located>,
    pub resources: Vec<Resource>,
}

impl Item {
    pub fn played_from(&self) -> Option<&Resource> {
        self.resources
            .iter()
            .find(|resource| !resource.converted)
            .or_else(|| self.resources.first())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub at: Located,
    pub mime: Option<String>,
    pub converted: bool,
    pub size: Option<u64>,
    pub length: Option<Duration>,
    pub rate: Option<u32>,
    pub channels: Option<u16>,
    pub bits: Option<u16>,
}

pub(crate) struct Origin<'a> {
    pub served_from: &'a Located,
    pub also_serving: &'a dyn Fn(&Located) -> bool,
}

impl Origin<'_> {
    fn admits(&self, at: &Located) -> bool {
        at.is_on_host_of(self.served_from) || (self.also_serving)(at)
    }
}

pub(crate) fn read_from(text: &str, origin: &Origin<'_>) -> Result<Listing> {
    let document = document(text, Asked::Action(crate::Action::Browse))?;
    let root = document.root_element();
    if root.tag_name().name() != "DIDL-Lite" {
        return Err(Error::NotAListing);
    }

    let mut listing = Listing::default();
    for entry in root.children().filter(Node::is_element) {
        if listing.len() >= ENTRIES_A_LISTING_AT_MOST {
            break;
        }
        match entry.tag_name().name() {
            "container" => {
                if let Some(container) = container_of(entry, origin) {
                    listing.containers.push(container);
                }
            }
            "item" => {
                if let Some(item) = item_of(entry, origin) {
                    listing.items.push(item);
                }
            }
            _ => {}
        }
    }
    Ok(listing)
}

fn container_of(node: Node<'_, '_>, origin: &Origin<'_>) -> Option<Container> {
    let id = ObjectId::new(node.attribute("id")?)?;
    let title = said(node, "title")?;
    Some(Container {
        id,
        title,
        class: said(node, "class").unwrap_or_default(),
        children: node
            .attribute("childCount")
            .and_then(|count| count.trim().parse().ok()),
        artist: artist_of(node),
        cover: cover_of(node, origin),
    })
}

fn item_of(node: Node<'_, '_>, origin: &Origin<'_>) -> Option<Item> {
    let class = said(node, "class")?;
    if !class.starts_with(AUDIO_CLASS) {
        return None;
    }
    let id = ObjectId::new(node.attribute("id")?)?;
    let title = said(node, "title")?;
    let resources: Vec<Resource> = node
        .children()
        .filter(|child| child.tag_name().name() == "res")
        .filter_map(|res| resource_of(res, origin))
        .take(RESOURCES_AN_ITEM_AT_MOST)
        .collect();
    if resources.is_empty() {
        return None;
    }
    Some(Item {
        id,
        title,
        artist: artist_of(node).or_else(|| said(node, "creator")),
        album: said(node, "album"),
        genre: said(node, "genre"),
        track: said(node, "originalTrackNumber").and_then(|number| number.parse().ok()),
        cover: cover_of(node, origin),
        resources,
    })
}

fn artist_of(node: Node<'_, '_>) -> Option<String> {
    let artists: Vec<Node<'_, '_>> = node
        .children()
        .filter(|child| child.tag_name().name() == "artist")
        .collect();
    let performing = artists
        .iter()
        .find(|artist| {
            artist
                .attribute("role")
                .is_none_or(|role| role.eq_ignore_ascii_case("Performer"))
        })
        .or_else(|| artists.first())?;
    readable_text(*performing)
}

fn cover_of(node: Node<'_, '_>, origin: &Origin<'_>) -> Option<Located> {
    node.children()
        .filter(|child| child.tag_name().name() == "albumArtURI")
        .filter_map(readable_text)
        .find_map(|address| on_the_server(&address, origin))
}

fn resource_of(node: Node<'_, '_>, origin: &Origin<'_>) -> Option<Resource> {
    let protocol = node.attribute("protocolInfo").unwrap_or_default();
    let mut fields = protocol.split(':');
    let transport = fields.next().unwrap_or_default().trim();
    if !transport.eq_ignore_ascii_case(HTTP_GET) {
        return None;
    }
    let _network = fields.next();
    let mime = fields
        .next()
        .map(str::trim)
        .filter(|mime| !mime.is_empty() && *mime != "*")
        .map(str::to_owned);
    let converted = fields.next().is_some_and(|flags| flags.contains(CONVERTED));

    let at = on_the_server(&readable_text(node)?, origin)?;
    let number = |name: &str| node.attribute(name).map(str::trim);
    Some(Resource {
        at,
        mime,
        converted,
        size: number("size").and_then(|size| size.parse().ok()),
        length: number("duration").and_then(length_of),
        rate: number("sampleFrequency").and_then(|rate| rate.parse().ok()),
        channels: number("nrAudioChannels").and_then(|channels| channels.parse().ok()),
        bits: number("bitsPerSample").and_then(|bits| bits.parse().ok()),
    })
}

fn on_the_server(address: &str, origin: &Origin<'_>) -> Option<Located> {
    let at = origin.served_from.join(address).ok()?;
    if !origin.admits(&at) {
        tracing::debug!(%at, served_from = %origin.served_from, "a media server's listing names an address that is not the server's");
        return None;
    }
    Some(at)
}

pub(crate) fn length_of(clock: &str) -> Option<Duration> {
    let (whole, fraction) = match clock.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (clock, None),
    };
    let mut parts = whole.split(':');
    let hours = digits(parts.next()?)?;
    let minutes = digits(parts.next()?)?;
    let seconds = digits(parts.next()?)?;
    if parts.next().is_some() || hours > HOURS_AT_MOST || minutes >= 60 || seconds >= 60 {
        return None;
    }
    let fraction = match fraction {
        None => Duration::ZERO,
        Some(fraction) => fraction_of(fraction)?,
    };
    Some(Duration::from_secs(hours * 3_600 + minutes * 60 + seconds) + fraction)
}

fn digits(text: &str) -> Option<u64> {
    let all_digits =
        !text.is_empty() && text.len() <= 4 && text.bytes().all(|b| b.is_ascii_digit());
    all_digits.then(|| text.parse().ok()).flatten()
}

fn fraction_of(text: &str) -> Option<Duration> {
    if let Some((above, below)) = text.split_once('/') {
        let above = digits(above)?;
        let below = digits(below)?;
        if below == 0 || above >= below {
            return None;
        }
        return Some(Duration::from_secs_f64(above as f64 / below as f64));
    }
    let all_digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if !all_digits {
        return None;
    }
    let millis: String = text.chars().chain("000".chars()).take(3).collect();
    Some(Duration::from_millis(millis.parse().ok()?))
}

fn said(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.tag_name().name() == name)
        .and_then(readable_text)
}

fn readable_text(node: Node<'_, '_>) -> Option<String> {
    let text: String = node
        .text()?
        .chars()
        .map(|letter| if letter.is_control() { ' ' } else { letter })
        .take(TEXT_CHARS_AT_MOST)
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, served_from: &Located) -> Result<Listing> {
        read_from(
            text,
            &Origin {
                served_from,
                also_serving: &|_| false,
            },
        )
    }

    fn served_from() -> Located {
        Located::parse("http://192.168.1.4:8200/rootDesc.xml").expect("an address")
    }

    const MINIDLNA: &str = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">
<container id="1$4" parentID="1" restricted="1" searchable="1" childCount="12"><dc:title>All Music</dc:title><upnp:class>object.container.storageFolder</upnp:class><upnp:storageUsed>-1</upnp:storageUsed></container>
<container id="1$7$1" parentID="1$7" restricted="1" childCount="9"><dc:title>Kind of Blue</dc:title><upnp:class>object.container.album.musicAlbum</upnp:class><upnp:artist>Miles Davis</upnp:artist><upnp:albumArtURI dlna:profileID="JPEG_TN">http://192.168.1.4:8200/AlbumArt/31-2.jpg</upnp:albumArtURI></container>
<item id="64$0$1" parentID="64$0" restricted="1"><dc:title>So What</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class><dc:creator>Miles Davis</dc:creator><upnp:artist>Miles Davis</upnp:artist><upnp:album>Kind of Blue</upnp:album><upnp:genre>Jazz</upnp:genre><upnp:originalTrackNumber>1</upnp:originalTrackNumber><dc:date>1959-01-01</dc:date><upnp:albumArtURI dlna:profileID="JPEG_TN">http://192.168.1.4:8200/AlbumArt/31-2.jpg</upnp:albumArtURI><res size="22413312" duration="0:09:22.533" bitrate="176400" sampleFrequency="44100" nrAudioChannels="2" protocolInfo="http-get:*:audio/x-flac:DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000">http://192.168.1.4:8200/MediaItems/22.flac</res></item>
<item id="64$0$2" parentID="64$0" restricted="1"><dc:title>A picture</dc:title><upnp:class>object.item.imageItem.photo</upnp:class><res protocolInfo="http-get:*:image/jpeg:*">http://192.168.1.4:8200/MediaItems/9.jpg</res></item>
</DIDL-Lite>"#;

    #[test]
    fn a_minidlna_listing_reads_as_its_folders_albums_and_songs_and_a_picture_is_passed_over() {
        let listing = read(MINIDLNA, &served_from()).expect("reads");

        assert_eq!(listing.containers.len(), 2);
        assert_eq!(listing.containers[0].id.as_str(), "1$4");
        assert_eq!(listing.containers[0].children, Some(12));
        assert!(!listing.containers[0].is_an_album());
        assert!(listing.containers[1].is_an_album());
        assert_eq!(listing.containers[1].artist.as_deref(), Some("Miles Davis"));
        assert_eq!(
            listing.containers[1]
                .cover
                .as_ref()
                .map(ToString::to_string),
            Some("http://192.168.1.4:8200/AlbumArt/31-2.jpg".to_owned())
        );

        assert_eq!(listing.items.len(), 1);
        let song = &listing.items[0];
        let played = song.played_from().expect("a resource");
        assert_eq!(song.title, "So What");
        assert_eq!(song.album.as_deref(), Some("Kind of Blue"));
        assert_eq!(song.track, Some(1));
        assert_eq!(
            played.at.to_string(),
            "http://192.168.1.4:8200/MediaItems/22.flac"
        );
        assert_eq!(played.mime.as_deref(), Some("audio/x-flac"));
        assert_eq!(played.size, Some(22_413_312));
        assert_eq!(played.length, Some(Duration::from_millis(562_533)));
        assert_eq!(played.rate, Some(44_100));
        assert_eq!(played.channels, Some(2));
    }

    #[test]
    fn the_original_is_played_before_a_converted_copy_and_an_address_on_another_host_is_passed_over()
     {
        let listing = r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="a" parentID="0"><dc:title>Song</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class>
            <res protocolInfo="http-get:*:audio/mpeg:DLNA.ORG_PN=MP3;DLNA.ORG_CI=1">http://192.168.1.4:8200/t/a.mp3</res>
            <res protocolInfo="http-get:*:audio/flac:DLNA.ORG_CI=0">http://10.9.9.9/a.flac</res>
            <res protocolInfo="rtsp-rtp-udp:*:audio/flac:*">http://192.168.1.4:8200/a.rtp</res>
            <res protocolInfo="http-get:*:audio/flac:DLNA.ORG_OP=01">/MediaItems/a.flac</res>
            <upnp:albumArtURI>http://127.0.0.1:631/printer</upnp:albumArtURI></item></DIDL-Lite>"#;

        let read = read(listing, &served_from()).expect("reads");
        let song = &read.items[0];

        assert_eq!(song.resources.len(), 2);
        assert_eq!(
            song.played_from().map(|res| res.at.to_string()),
            Some("http://192.168.1.4:8200/MediaItems/a.flac".to_owned())
        );
        assert_eq!(song.cover, None);
    }

    #[test]
    fn an_address_on_another_host_is_taken_only_where_that_host_is_vouched_for_as_the_servers() {
        let listing = r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="a" parentID="0"><dc:title>Song</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class>
            <res protocolInfo="http-get:*:audio/flac:*">http://10.9.9.9/a.flac</res>
            <res protocolInfo="http-get:*:audio/flac:*">http://192.168.7.4:8200/a.flac</res>
            <upnp:albumArtURI>http://192.168.7.4:8200/a.jpg</upnp:albumArtURI></item></DIDL-Lite>"#;
        let other = Located::parse("http://192.168.7.4/").expect("an address");
        let also_serving = |at: &Located| at.is_on_host_of(&other);

        let read = read_from(
            listing,
            &Origin {
                served_from: &served_from(),
                also_serving: &also_serving,
            },
        )
        .expect("reads");
        let song = &read.items[0];

        assert_eq!(song.resources.len(), 1);
        assert_eq!(
            song.played_from().map(|res| res.at.to_string()),
            Some("http://192.168.7.4:8200/a.flac".to_owned())
        );
        assert_eq!(
            song.cover.as_ref().map(ToString::to_string),
            Some("http://192.168.7.4:8200/a.jpg".to_owned())
        );
    }

    #[test]
    fn every_servers_listing_a_fuzz_seed_holds_reads_as_songs_played_from_that_server() {
        let seeds = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/dlna");
        let served_from = Located::parse("http://10.0.0.9:8200/rootDesc.xml").expect("an address");

        for server in ["minidlna", "plex", "jellyfin", "resonate"] {
            let text = std::fs::read_to_string(seeds.join(server)).expect("a seed");
            let listing = read(&text, &served_from).expect("the seed reads");
            let song = listing.items.first().expect("a song");
            let played = song.played_from().expect("a resource");

            assert!(played.at.is_on_host_of(&served_from), "{server}");
            assert!(!played.converted, "{server}");
            assert!(played.length.is_some(), "{server}");
        }
    }

    #[test]
    fn a_length_is_read_in_every_form_the_spec_writes_and_nothing_else() {
        assert_eq!(length_of("0:03:30"), Some(Duration::from_secs(210)));
        assert_eq!(
            length_of("1:00:00.5"),
            Some(Duration::from_secs_f64(3_600.5))
        );
        assert_eq!(
            length_of("00:00:01.1/4"),
            Some(Duration::from_secs_f64(1.25))
        );
        assert_eq!(length_of("0:61:00"), None);
        assert_eq!(length_of("0:00:01.4/4"), None);
        assert_eq!(length_of("0:00:01.1/0"), None);
        assert_eq!(length_of("-1:00:00"), None);
        assert_eq!(length_of("99999:00:00"), None);
        assert_eq!(length_of(""), None);
    }

    #[test]
    fn what_is_not_didl_lite_is_refused_and_an_entry_without_an_id_or_title_is_passed_over() {
        let soap = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"/>"#;
        let thin = r#"<DIDL-Lite><container parentID="0"><dc:title xmlns:dc="d">No id</dc:title></container><container id="b"/><item id="c"><upnp:class xmlns:upnp="u">object.item.audioItem</upnp:class></item></DIDL-Lite>"#;

        assert!(matches!(
            read(soap, &served_from()),
            Err(Error::NotAListing)
        ));
        assert_eq!(
            read(thin, &served_from()).expect("reads"),
            Listing::default()
        );
    }

    #[test]
    fn an_object_id_is_the_servers_own_spelling_and_a_control_character_is_refused() {
        assert_eq!(
            ObjectId::new("64$0$1").map(|id| id.to_string()),
            Some("64$0$1".to_owned())
        );
        assert!(ObjectId::root().is_root());
        assert_eq!(ObjectId::new(""), None);
        assert_eq!(ObjectId::new("a\nb"), None);
        assert_eq!(ObjectId::new(&"x".repeat(2_000)), None);
    }
}
