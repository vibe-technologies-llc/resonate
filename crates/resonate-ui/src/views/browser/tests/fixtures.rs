use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use parking_lot::Mutex;
use resonate_core::{Isrc, SourceId};
use resonate_library::{
    AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings, ArtistProfile,
    Barcode, BarcodeMatch, CoverArt, Credit, Discography, GroupAsked, GroupMatch, Issued, Link,
    LinkNames, LinkedPlaylist, LinkedProfile, ListedSong, LookupOp, LyricText, LyricsAsked, Mbid,
    Medium, PlaylistLink, ProfileLink, Recording, RecordingAsked, RecordingMatch, RecordingRelease,
    Reference, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch, ReleaseTrack, Result, SongLink,
    SongsAsked, StreamAsked,
};
use resonate_providers::{Identity, Obtained, Provider};

use super::enriching::HEROES_MISSPELT;

pub(super) const HEROES_TONIGHT: &str = "1a7d3b23-842a-4e57-8a8b-0f8b96a25f20";

const HEROES_TONIGHT_RELEASE: &str = "d96b3b34-e52b-4f6a-bfa2-c52daddd64a1";

pub(super) const HEROES_TONIGHT_ISRC: &str = "GB2LD0902006";

pub(super) const HEROES_TONIGHT_GROUP: &str = "6c0b1f5e-3a2d-4f7e-9b8c-1d2e3f4a5b6c";

pub(super) const LIGHTS_GROUP: &str = "8e1f2a3b-4c5d-4e6f-8a7b-9c0d1e2f3a4b";

const LIGHTS_RELEASE: &str = "2f3a4b5c-6d7e-4f8a-9b0c-1d2e3f4a5b6c";

const SKYLINE: &str = "4a5b6c7d-8e9f-4a0b-8c1d-2e3f4a5b6c7d";

const JANJI: &str = "0b3c6f5d-2e1a-4c8b-9d7e-6f5a4b3c2d1e";

pub(super) const NCS_PLAYLIST: u64 = 1_234;

pub(super) fn mbid(id: &str) -> Mbid {
    Mbid::new(id).expect("an mbid")
}

pub(super) fn janji() -> Vec<Credit> {
    vec![Credit {
        name: "Janji".to_owned(),
        joined_by: " & Johnning".to_owned(),
        mbid: Some(mbid(JANJI)),
    }]
}

pub(super) fn ncs_cover() -> CoverArt {
    let mut bytes = Vec::new();

    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("an image in memory");
    CoverArt {
        format: resonate_library::ImageFormat::Png,
        bytes,
    }
}

pub(super) struct MusicBrainz {
    source: SourceId,
    pub(super) searched: Arc<Mutex<Vec<String>>>,
    pub(super) albums_searched: Arc<Mutex<Vec<String>>>,
    pub(super) refusing: Arc<AtomicBool>,
    pub(super) refusing_albums: Arc<AtomicBool>,
}

impl MusicBrainz {
    pub(super) fn new() -> Self {
        Self {
            source: SourceId::new("musicbrainz").expect("a source name"),
            searched: Arc::default(),
            albums_searched: Arc::default(),
            refusing: Arc::default(),
            refusing_albums: Arc::default(),
        }
    }

    pub(super) fn sharing(&self) -> Self {
        Self {
            source: self.source.clone(),
            searched: Arc::clone(&self.searched),
            albums_searched: Arc::clone(&self.albums_searched),
            refusing: Arc::clone(&self.refusing),
            refusing_albums: Arc::clone(&self.refusing_albums),
        }
    }
}

impl Reference for MusicBrainz {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, id: &Mbid) -> Result<Option<Release>> {
        Ok(match id.as_str() {
            HEROES_TONIGHT_RELEASE => Some(heroes_tonight_release()),
            LIGHTS_RELEASE => Some(lights_release()),
            _ => None,
        })
    }

    fn releases_by_barcode(&self, _: &Barcode) -> Result<Vec<BarcodeMatch>> {
        Ok(Vec::new())
    }

    fn find_release(&self, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
        Ok((asked.title == "Heroes Tonight")
            .then(|| ReleaseMatch {
                release: mbid(HEROES_TONIGHT_RELEASE),
                group: None,
                score: 100,
                title: "Heroes Tonight".to_owned(),
                credit: janji(),
                track_count: Some(1),
                date: Some("2015-12-22".to_owned()),
            })
            .into_iter()
            .collect())
    }

    fn recording(&self, id: &Mbid) -> Result<Option<Recording>> {
        Ok((id.as_str() == HEROES_TONIGHT).then(|| heroes_tonight().into_recording()))
    }

    fn recordings_of_isrc(&self, isrc: &resonate_library::Isrc) -> Result<Vec<Recording>> {
        Ok((isrc.as_str() == HEROES_TONIGHT_ISRC)
            .then(|| heroes_tonight().into_recording())
            .into_iter()
            .collect())
    }

    fn find_recording(&self, asked: &RecordingAsked) -> Result<Vec<RecordingMatch>> {
        Ok((asked.title == HEROES_MISSPELT)
            .then(|| RecordingMatch {
                score: 58,
                ..heroes_tonight()
            })
            .into_iter()
            .collect())
    }

    fn find_songs(&self, asked: &SongsAsked) -> Result<Vec<RecordingMatch>> {
        self.searched.lock().push(asked.words.clone());
        if self.refusing.load(Ordering::Relaxed) {
            return Err(resonate_library::Error::Refused {
                op: LookupOp::FindRecording,
                status: 503,
            });
        }
        Ok(vec![heroes_tonight()])
    }

    fn release_group(&self, id: &Mbid) -> Result<Option<ReleaseGroup>> {
        Ok((id.as_str() == HEROES_TONIGHT_GROUP).then(|| ReleaseGroup {
            id: mbid(HEROES_TONIGHT_GROUP),
            title: "Heroes Tonight".to_owned(),
            credit: janji(),
            kind: Some("Single".to_owned()),
            first_released: Some("2015-12-22".to_owned()),
            disambiguation: None,
            links: Vec::new(),
            releases: Vec::new(),
        }))
    }

    fn find_release_group(&self, _: &GroupAsked) -> Result<Vec<GroupMatch>> {
        Ok(Vec::new())
    }

    fn find_albums(&self, words: &str) -> Result<Vec<AlbumMatch>> {
        self.albums_searched.lock().push(words.to_owned());
        if self.refusing_albums.load(Ordering::Relaxed) {
            return Err(resonate_library::Error::Refused {
                op: LookupOp::FindReleaseGroup,
                status: 503,
            });
        }
        Ok(vec![AlbumMatch {
            group: mbid(HEROES_TONIGHT_GROUP),
            score: 100,
            title: "Heroes Tonight".to_owned(),
            credit: janji(),
            kind: Some("Album".to_owned()),
            secondary: Vec::new(),
            first_released: Some("2015-12-22".to_owned()),
        }])
    }

    fn group_cover(&self, _: &Mbid) -> Result<Option<CoverArt>> {
        Ok(None)
    }

    fn artist(&self, _: &Mbid) -> Result<Option<ArtistProfile>> {
        Ok(None)
    }

    fn find_artist(&self, _: &str) -> Result<Vec<ArtistMatch>> {
        Ok(Vec::new())
    }

    fn release_groups_of(&self, _: &Mbid, _: u32) -> Result<Discography> {
        Ok(Discography::default())
    }

    fn releases_of_group(&self, group: &Mbid) -> Result<Vec<Release>> {
        Ok(match group.as_str() {
            HEROES_TONIGHT_GROUP => vec![Release {
                group: Some(mbid(HEROES_TONIGHT_GROUP)),
                ..heroes_tonight_release()
            }],
            LIGHTS_GROUP => vec![lights_release()],
            _ => Vec::new(),
        })
    }

    fn releases_of_artist(&self, _: &Mbid, _: u32) -> Result<ArtistPressings> {
        Ok(ArtistPressings::default())
    }

    fn cover(&self, _: &Mbid, _: Option<&Mbid>) -> Result<Option<CoverArt>> {
        Ok(Some(ncs_cover()))
    }

    fn portrait(&self, _: &[Link]) -> Result<Option<CoverArt>> {
        Ok(None)
    }

    fn streamed_at(&self, _: &StreamAsked) -> Result<Option<Link>> {
        Ok(None)
    }

    fn lyrics(&self, _: &LyricsAsked) -> Result<Option<LyricText>> {
        Ok(None)
    }

    fn song_linked(&self, _: &SongLink) -> Result<Option<LinkNames>> {
        Ok(Some(LinkNames {
            isrcs: vec![Isrc::new(HEROES_TONIGHT_ISRC).expect("an isrc")],
            length: None,
            title: None,
            artist: None,
        }))
    }

    fn album_linked(&self, _: &AlbumLink) -> Result<Option<AlbumNames>> {
        Ok(None)
    }

    fn artist_linked(&self, _: &ArtistLink) -> Result<Option<String>> {
        Ok(None)
    }

    fn playlist_linked(&self, link: &PlaylistLink) -> Result<Option<LinkedPlaylist>> {
        Ok(
            (link == &PlaylistLink::Deezer(NCS_PLAYLIST)).then(|| LinkedPlaylist {
                name: "NCS".to_owned(),
                songs: vec![ListedSong::Recording(mbid(HEROES_TONIGHT))],
            }),
        )
    }

    fn profile_linked(&self, _link: &ProfileLink) -> Result<Option<LinkedProfile>> {
        Ok(None)
    }

    fn artist_at(&self, _: &str) -> Result<Option<Mbid>> {
        Ok(None)
    }
}

fn heroes_tonight_release() -> Release {
    Release {
        id: mbid(HEROES_TONIGHT_RELEASE),
        group: None,
        title: "Heroes Tonight".to_owned(),
        credit: janji(),
        date: Some("2015-12-22".to_owned()),
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: Some("Album".to_owned()),
        disambiguation: None,
        has_front_cover: true,
        links: Vec::new(),
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![ReleaseTrack {
                position: 1,
                number: "1".to_owned(),
                title: "Heroes Tonight".to_owned(),
                artist: None,
                recording: Some(mbid(HEROES_TONIGHT)),
                track: None,
                length: None,
                isrcs: vec![HEROES_TONIGHT_ISRC.to_owned()],
                links: Vec::new(),
            }],
        }],
    }
}

fn lights_release() -> Release {
    let song = |position: u32, title: &str, recording: &str| ReleaseTrack {
        position,
        number: position.to_string(),
        title: title.to_owned(),
        artist: None,
        recording: Some(mbid(recording)),
        track: None,
        length: None,
        isrcs: Vec::new(),
        links: Vec::new(),
    };

    Release {
        id: mbid(LIGHTS_RELEASE),
        group: Some(mbid(LIGHTS_GROUP)),
        title: "Lost in the Lights".to_owned(),
        kind: Some("EP".to_owned()),
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![
                song(1, "Heroes Tonight", HEROES_TONIGHT),
                song(2, "Skyline", SKYLINE),
            ],
        }],
        ..heroes_tonight_release()
    }
}

pub(super) fn heroes_tonight() -> RecordingMatch {
    RecordingMatch {
        recording: mbid(HEROES_TONIGHT),
        score: 100,
        title: "Heroes Tonight".to_owned(),
        credit: janji(),
        length: None,
        isrcs: vec![Isrc::new(HEROES_TONIGHT_ISRC).expect("an isrc")],
        releases: vec![RecordingRelease {
            id: mbid(HEROES_TONIGHT_RELEASE),
            title: "Heroes Tonight".to_owned(),
            date: Some("2015-12-22".to_owned()),
            disc: Some(1),
            position: Some(1),
            issued: Issued {
                kind: Some("Album".to_owned()),
                secondary: Vec::new(),
                status: Some("Official".to_owned()),
            },
        }],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Asked {
    pub(super) recording: Option<Mbid>,
    pub(super) isrc: Option<Isrc>,
    pub(super) artist: Option<String>,
}

pub(super) struct Shop {
    pub(super) source: SourceId,
    pub(super) asked: Arc<Mutex<Vec<Asked>>>,
}

impl Provider for Shop {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> resonate_providers::Result<Obtained> {
        self.asked.lock().push(Asked {
            recording: identity.recording.clone(),
            isrc: identity.isrcs.first().cloned(),
            artist: identity.artist.clone(),
        });
        Ok(Obtained::Nothing)
    }
}

pub(super) struct Delivers {
    pub(super) source: SourceId,
    pub(super) file: std::path::PathBuf,
    pub(super) positions: Arc<Mutex<Vec<Option<u32>>>>,
}

impl Provider for Delivers {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> resonate_providers::Result<Obtained> {
        self.positions.lock().push(identity.position);
        Ok(Obtained::Found(resonate_providers::Delivery::File(
            self.file.clone(),
        )))
    }
}
