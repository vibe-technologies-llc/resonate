use std::{fmt::Write as _, time::Duration};

use resonate_core::AlbumId;

use crate::catalog::{AudioKind, Held, Object, Shelf, Song};

pub(crate) const DLNA_FEATURES: &str =
    "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";

const OPENS: &str = concat!(
    "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" ",
    "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" ",
    "xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\" ",
    "xmlns:dlna=\"urn:schemas-dlna-org:metadata-1-0/\">"
);

const CLOSES: &str = "</DIDL-Lite>";

pub(crate) const FOLDER: &str = "object.container.storageFolder";

pub(crate) const ARTIST: &str = "object.container.person.musicArtist";

pub(crate) const ALBUM: &str = "object.container.album.musicAlbum";

pub(crate) const PLAYLIST: &str = "object.container.playlistContainer";

pub(crate) const TRACK: &str = "object.item.audioItem.musicTrack";

const NO_PARENT: &str = "-1";

pub(crate) const SONGS_UNDER: &str = "/resonate/";

pub(crate) const COVERS_UNDER: &str = "/cover/";

pub(crate) fn xml_text(text: &str, into: &mut String) {
    for letter in text.chars() {
        match letter {
            '&' => into.push_str("&amp;"),
            '<' => into.push_str("&lt;"),
            '>' => into.push_str("&gt;"),
            '"' => into.push_str("&quot;"),
            '\'' => into.push_str("&apos;"),
            '\t' | '\n' | '\r' => into.push(letter),
            letter if letter < ' ' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            letter => into.push(letter),
        }
    }
}

pub(crate) fn escaped(text: &str) -> String {
    let mut into = String::with_capacity(text.len());
    xml_text(text, &mut into);
    into
}

pub(crate) fn song_path(song: &Song) -> String {
    format!(
        "{SONGS_UNDER}{}.{}",
        Object::Song(song.id),
        song.kind.extension()
    )
}

pub(crate) fn cover_path(album: AlbumId) -> String {
    format!("{COVERS_UNDER}{}", Object::Shelf(Shelf::Album(album)))
}

pub(crate) fn protocol_of(kind: AudioKind) -> String {
    format!("http-get:*:{}:{DLNA_FEATURES}", kind.mime())
}

struct Container<'a> {
    id: Object,
    parent: Option<Object>,
    title: &'a str,
    class: &'static str,
    children: u32,
    artist: Option<&'a str>,
    cover: Option<AlbumId>,
}

pub(crate) struct Didl<'a> {
    written: String,
    base: &'a str,
}

impl<'a> Didl<'a> {
    pub(crate) fn new(base: &'a str) -> Self {
        Self {
            written: OPENS.to_owned(),
            base,
        }
    }

    pub(crate) fn root(&mut self, title: &str, children: u32) {
        self.container(&Container {
            id: Object::Root,
            parent: None,
            title,
            class: FOLDER,
            children,
            artist: None,
            cover: None,
        });
    }

    pub(crate) fn shelf(&mut self, held: &Held, parent: Object) {
        let class = match held.shelf {
            Shelf::Artists | Shelf::Albums | Shelf::Songs | Shelf::Playlists => FOLDER,
            Shelf::Artist(_) => ARTIST,
            Shelf::Album(_) => ALBUM,
            Shelf::Playlist(_) => PLAYLIST,
        };
        self.container(&Container {
            id: Object::Shelf(held.shelf),
            parent: Some(parent),
            title: &held.title,
            class,
            children: held.children,
            artist: held.artist.as_deref(),
            cover: held.cover,
        });
    }

    fn container(&mut self, container: &Container<'_>) {
        let parent = container
            .parent
            .map_or_else(|| NO_PARENT.to_owned(), |parent| parent.to_string());
        let _ = write!(
            self.written,
            "<container id=\"{}\" parentID=\"{parent}\" restricted=\"1\" searchable=\"1\" \
             childCount=\"{}\"><dc:title>",
            container.id, container.children,
        );
        xml_text(container.title, &mut self.written);
        let _ = write!(
            self.written,
            "</dc:title><upnp:class>{}</upnp:class>",
            container.class
        );
        if let Some(artist) = container.artist {
            self.element("upnp:artist", artist);
        }
        if let Some(album) = container.cover {
            self.cover(album);
        }
        self.written.push_str("</container>");
    }

    pub(crate) fn song(&mut self, song: &Song, parent: Object) {
        let _ = write!(
            self.written,
            "<item id=\"{}\" parentID=\"{parent}\" restricted=\"1\"><dc:title>",
            Object::Song(song.id),
        );
        xml_text(&song.title, &mut self.written);
        let _ = write!(self.written, "</dc:title><upnp:class>{TRACK}</upnp:class>");
        if let Some(artist) = &song.artist {
            self.element("dc:creator", artist);
            self.element("upnp:artist", artist);
        }
        if let Some(album) = &song.album {
            self.element("upnp:album", album);
        }
        if let Some(genre) = &song.genre {
            self.element("upnp:genre", genre);
        }
        if let Some(number) = song.track_number {
            let _ = write!(
                self.written,
                "<upnp:originalTrackNumber>{number}</upnp:originalTrackNumber>"
            );
        }
        if let Some(album) = song.cover {
            self.cover(album);
        }
        let _ = write!(
            self.written,
            "<res protocolInfo=\"{}\"",
            protocol_of(song.kind)
        );
        if let Some(size) = song.size {
            let _ = write!(self.written, " size=\"{size}\"");
        }
        if let Some(duration) = song.duration {
            let _ = write!(self.written, " duration=\"{}\"", clock(duration));
        }
        if song.rate > 0 {
            let _ = write!(self.written, " sampleFrequency=\"{}\"", song.rate);
        }
        if song.channels > 0 {
            let _ = write!(self.written, " nrAudioChannels=\"{}\"", song.channels);
        }
        if let Some(bits) = song.bits {
            let _ = write!(self.written, " bitsPerSample=\"{bits}\"");
        }
        let _ = write!(
            self.written,
            ">{}{}</res></item>",
            self.base,
            song_path(song)
        );
    }

    fn element(&mut self, name: &str, text: &str) {
        let _ = write!(self.written, "<{name}>");
        xml_text(text, &mut self.written);
        let _ = write!(self.written, "</{name}>");
    }

    fn cover(&mut self, album: AlbumId) {
        let _ = write!(
            self.written,
            "<upnp:albumArtURI>{}{}</upnp:albumArtURI>",
            self.base,
            cover_path(album)
        );
    }

    pub(crate) fn finished(mut self) -> String {
        self.written.push_str(CLOSES);
        self.written
    }
}

pub(crate) fn clock(duration: Duration) -> String {
    let millis = duration.as_millis();
    let seconds = millis / 1000;
    format!(
        "{}:{:02}:{:02}.{:03}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        millis % 1000
    )
}

#[cfg(test)]
mod tests {
    use resonate_core::TrackId;

    use super::*;
    use crate::description::document;

    fn song() -> Song {
        Song {
            id: TrackId::new(9).expect("an id"),
            title: "Rock & <Roll>\u{1}".to_owned(),
            artist: Some("A \"B\"".to_owned()),
            album: Some("C'D".to_owned()),
            cover: AlbumId::new(4).ok(),
            genre: None,
            track_number: Some(3),
            duration: Some(Duration::from_millis(3_723_045)),
            kind: AudioKind::Flac,
            size: Some(1234),
            rate: 96_000,
            channels: 2,
            bits: Some(24),
        }
    }

    #[test]
    fn a_song_is_written_escaped_and_reads_back_as_a_document_naming_its_file() {
        let mut didl = Didl::new("http://10.0.0.2:8200");
        didl.song(&song(), Object::Shelf(Shelf::Songs));
        let written = didl.finished();

        let read = document(&written, crate::Asked::Description).expect("a document");
        let title = read
            .descendants()
            .find(|node| node.tag_name().name() == "title")
            .and_then(|node| node.text());
        let res = read
            .descendants()
            .find(|node| node.tag_name().name() == "res")
            .expect("a resource");

        assert_eq!(title, Some("Rock & <Roll>"));
        assert_eq!(
            res.text(),
            Some("http://10.0.0.2:8200/resonate/song-9.flac")
        );
        assert_eq!(res.attribute("size"), Some("1234"));
        assert_eq!(res.attribute("duration"), Some("1:02:03.045"));
        assert_eq!(res.attribute("bitsPerSample"), Some("24"));
        assert!(
            res.attribute("protocolInfo")
                .is_some_and(|info| info.starts_with("http-get:*:audio/flac:DLNA.ORG_OP=01"))
        );
        assert!(written.contains("<upnp:albumArtURI>http://10.0.0.2:8200/cover/album-4<"));
    }

    #[test]
    fn a_song_whose_size_is_unknown_carries_no_size_rather_than_none() {
        let unsized_song = Song {
            size: None,
            ..song()
        };
        let mut didl = Didl::new("http://10.0.0.2:8200");
        didl.song(&unsized_song, Object::Shelf(Shelf::Songs));
        let written = didl.finished();

        let read = document(&written, crate::Asked::Description).expect("a document");
        let res = read
            .descendants()
            .find(|node| node.tag_name().name() == "res")
            .expect("a resource");

        assert_eq!(res.attribute("size"), None);
        assert_eq!(res.attribute("sampleFrequency"), Some("96000"));
    }
}
