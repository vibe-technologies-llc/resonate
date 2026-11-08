use resonate_core::{Isrc, Mbid, folded_letters};
use resonate_library::{
    AlbumMatch, ArtistMatch, ArtistPressings, ArtistProfile, Barcode, BarcodeMatch, ByArtist,
    Discography, GroupAsked, GroupMatch, Recording, RecordingAsked, RecordingMatch, Release,
    ReleaseAsked, ReleaseGroup, ReleaseMatch, SongsAsked,
};

use crate::{
    Error, Malformed, Result,
    wire::{Reader, Writer},
};

const ENCODED_AS: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Release(Mbid),
    FindRelease(ReleaseAsked),
    Barcode(Barcode),
    Recording(Mbid),
    Isrc(Isrc),
    FindRecording(RecordingAsked),
    FindSongs(SongsAsked),
    Group(Mbid),
    FindGroup(GroupAsked),
    FindAlbums(String),
    Artist(Mbid),
    FindArtist(String),
    GroupsOf { artist: Mbid, from: u32 },
    ReleasesOfGroup(Mbid),
    ReleasesOfArtist { artist: Mbid, from: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Release(Option<Release>),
    Releases(Vec<ReleaseMatch>),
    Barcoded(Vec<BarcodeMatch>),
    Recording(Option<Recording>),
    Recordings(Vec<Recording>),
    RecordingMatches(Vec<RecordingMatch>),
    Group(Option<ReleaseGroup>),
    Groups(Vec<GroupMatch>),
    Albums(Vec<AlbumMatch>),
    Artist(Option<ArtistProfile>),
    Artists(Vec<ArtistMatch>),
    Discography(Discography),
    Pressings(Vec<Release>),
    ArtistPressings(ArtistPressings),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Entity {
    Release,
    Recording,
    Group,
    Artist,
}

impl Entity {
    pub const ALL: [Self; 4] = [Self::Release, Self::Recording, Self::Group, Self::Artist];

    pub const fn code(self) -> u8 {
        match self {
            Self::Release => 0,
            Self::Recording => 1,
            Self::Group => 2,
            Self::Artist => 3,
        }
    }

    pub fn coded(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|entity| entity.code() == code)
    }
}

impl Asked {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.byte(ENCODED_AS);
        self.write(&mut writer);
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let version = reader.byte()?;
        if version != ENCODED_AS {
            return Err(Error::Malformed(Malformed::Tag { tag: version }));
        }
        let asked = Self::read(&mut reader)?;
        reader.done()?;
        Ok(asked)
    }

    pub fn key(&self) -> Vec<u8> {
        self.canonical().encoded()
    }

    pub fn names(&self) -> Option<(Entity, &Mbid)> {
        match self {
            Self::Release(id) => Some((Entity::Release, id)),
            Self::Recording(id) => Some((Entity::Recording, id)),
            Self::Group(id) => Some((Entity::Group, id)),
            Self::Artist(id) => Some((Entity::Artist, id)),
            _ => None,
        }
    }

    pub fn is_answered_by(&self, answer: &Answer) -> bool {
        match (self, answer) {
            (Self::Release(id), Answer::Release(release)) => {
                release.as_ref().is_none_or(|release| release.id == *id)
            }
            (Self::Recording(id), Answer::Recording(recording)) => recording
                .as_ref()
                .is_none_or(|recording| recording.id == *id),
            (Self::Group(id), Answer::Group(group)) => {
                group.as_ref().is_none_or(|group| group.id == *id)
            }
            (Self::Artist(id), Answer::Artist(artist)) => {
                artist.as_ref().is_none_or(|artist| artist.mbid == *id)
            }
            (Self::Isrc(isrc), Answer::Recordings(recordings)) => recordings
                .iter()
                .all(|recording| recording.isrcs.contains(isrc)),
            (Self::FindRelease(_), Answer::Releases(_))
            | (Self::Barcode(_), Answer::Barcoded(_))
            | (Self::FindRecording(_) | Self::FindSongs(_), Answer::RecordingMatches(_))
            | (Self::FindGroup(_), Answer::Groups(_))
            | (Self::FindAlbums(_), Answer::Albums(_))
            | (Self::FindArtist(_), Answer::Artists(_))
            | (Self::GroupsOf { .. }, Answer::Discography(_))
            | (Self::ReleasesOfGroup(_), Answer::Pressings(_))
            | (Self::ReleasesOfArtist { .. }, Answer::ArtistPressings(_)) => true,
            _ => false,
        }
    }

    fn canonical(&self) -> Self {
        let folded = |text: &str| folded_letters(text.trim());
        let maybe = |text: &Option<String>| text.as_deref().map(folded);
        match self {
            Self::FindRelease(asked) => Self::FindRelease(ReleaseAsked {
                title: folded(&asked.title),
                artist: maybe(&asked.artist),
                barcode: asked.barcode.as_deref().map(str::trim).map(str::to_owned),
                catalog_number: maybe(&asked.catalog_number),
                ..asked.clone()
            }),
            Self::FindRecording(asked) => Self::FindRecording(RecordingAsked {
                title: folded(&asked.title),
                artist: maybe(&asked.artist),
                release: maybe(&asked.release),
                ..asked.clone()
            }),
            Self::FindSongs(asked) => Self::FindSongs(SongsAsked {
                words: folded(&asked.words),
                typed: folded(&asked.typed),
                by: asked.by.as_ref().map(|by| ByArtist {
                    title: folded(&by.title),
                    artist: folded(&by.artist),
                }),
            }),
            Self::FindGroup(asked) => Self::FindGroup(GroupAsked {
                title: folded(&asked.title),
                artist: maybe(&asked.artist),
                ..asked.clone()
            }),
            Self::FindAlbums(words) => Self::FindAlbums(folded(words)),
            Self::FindArtist(name) => Self::FindArtist(folded(name)),
            other => other.clone(),
        }
    }

    fn write(&self, writer: &mut Writer) {
        match self {
            Self::Release(id) => {
                writer.byte(0);
                writer.mbid(id);
            }
            Self::FindRelease(asked) => {
                writer.byte(1);
                writer.release_asked(asked);
            }
            Self::Barcode(barcode) => {
                writer.byte(2);
                writer.barcode(barcode);
            }
            Self::Recording(id) => {
                writer.byte(3);
                writer.mbid(id);
            }
            Self::Isrc(isrc) => {
                writer.byte(4);
                writer.text(isrc.as_str());
            }
            Self::FindRecording(asked) => {
                writer.byte(5);
                writer.recording_asked(asked);
            }
            Self::FindSongs(asked) => {
                writer.byte(6);
                writer.songs_asked(asked);
            }
            Self::Group(id) => {
                writer.byte(7);
                writer.mbid(id);
            }
            Self::FindGroup(asked) => {
                writer.byte(8);
                writer.group_asked(asked);
            }
            Self::FindAlbums(words) => {
                writer.byte(9);
                writer.text(words);
            }
            Self::Artist(id) => {
                writer.byte(10);
                writer.mbid(id);
            }
            Self::FindArtist(name) => {
                writer.byte(11);
                writer.text(name);
            }
            Self::GroupsOf { artist, from } => {
                writer.byte(12);
                writer.mbid(artist);
                writer.number(u64::from(*from));
            }
            Self::ReleasesOfGroup(id) => {
                writer.byte(13);
                writer.mbid(id);
            }
            Self::ReleasesOfArtist { artist, from } => {
                writer.byte(14);
                writer.mbid(artist);
                writer.number(u64::from(*from));
            }
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self> {
        Ok(match reader.byte()? {
            0 => Self::Release(reader.mbid()?),
            1 => Self::FindRelease(reader.release_asked()?),
            2 => Self::Barcode(reader.barcode()?),
            3 => Self::Recording(reader.mbid()?),
            4 => Self::Isrc(
                Isrc::new(&reader.text()?).map_err(|_| Error::Malformed(Malformed::Isrc))?,
            ),
            5 => Self::FindRecording(reader.recording_asked()?),
            6 => Self::FindSongs(reader.songs_asked()?),
            7 => Self::Group(reader.mbid()?),
            8 => Self::FindGroup(reader.group_asked()?),
            9 => Self::FindAlbums(reader.text()?),
            10 => Self::Artist(reader.mbid()?),
            11 => Self::FindArtist(reader.text()?),
            12 => Self::GroupsOf {
                artist: reader.mbid()?,
                from: reader.small()?,
            },
            13 => Self::ReleasesOfGroup(reader.mbid()?),
            14 => Self::ReleasesOfArtist {
                artist: reader.mbid()?,
                from: reader.small()?,
            },
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        })
    }
}

impl Answer {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.byte(ENCODED_AS);
        match self {
            Self::Release(release) => {
                writer.byte(0);
                maybe(&mut writer, release.as_ref(), Writer::release);
            }
            Self::Releases(found) => {
                writer.byte(1);
                writer.release_matches(found);
            }
            Self::Barcoded(found) => {
                writer.byte(2);
                writer.barcode_matches(found);
            }
            Self::Recording(recording) => {
                writer.byte(3);
                maybe(&mut writer, recording.as_ref(), Writer::recording);
            }
            Self::Recordings(recordings) => {
                writer.byte(4);
                writer.recordings(recordings);
            }
            Self::RecordingMatches(found) => {
                writer.byte(5);
                writer.recording_matches(found);
            }
            Self::Group(group) => {
                writer.byte(6);
                maybe(&mut writer, group.as_ref(), Writer::group);
            }
            Self::Groups(found) => {
                writer.byte(7);
                writer.group_matches(found);
            }
            Self::Albums(found) => {
                writer.byte(8);
                writer.album_matches(found);
            }
            Self::Artist(artist) => {
                writer.byte(9);
                maybe(&mut writer, artist.as_ref(), Writer::artist);
            }
            Self::Artists(found) => {
                writer.byte(10);
                writer.artist_matches(found);
            }
            Self::Discography(discography) => {
                writer.byte(11);
                writer.discography(discography);
            }
            Self::Pressings(pressings) => {
                writer.byte(12);
                writer.releases(pressings);
            }
            Self::ArtistPressings(pressings) => {
                writer.byte(13);
                writer.pressings(pressings);
            }
        }
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let version = reader.byte()?;
        if version != ENCODED_AS {
            return Err(Error::Malformed(Malformed::Tag { tag: version }));
        }
        let answer = match reader.byte()? {
            0 => Self::Release(reader.maybe(Reader::release)?),
            1 => Self::Releases(reader.release_matches()?),
            2 => Self::Barcoded(reader.barcode_matches()?),
            3 => Self::Recording(reader.maybe(Reader::recording)?),
            4 => Self::Recordings(reader.recordings()?),
            5 => Self::RecordingMatches(reader.recording_matches()?),
            6 => Self::Group(reader.maybe(Reader::group)?),
            7 => Self::Groups(reader.group_matches()?),
            8 => Self::Albums(reader.album_matches()?),
            9 => Self::Artist(reader.maybe(Reader::artist)?),
            10 => Self::Artists(reader.artist_matches()?),
            11 => Self::Discography(reader.discography()?),
            12 => Self::Pressings(reader.releases()?),
            13 => Self::ArtistPressings(reader.pressings()?),
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        };
        reader.done()?;
        Ok(answer)
    }

    pub fn is_a_miss(&self) -> bool {
        match self {
            Self::Release(release) => release.is_none(),
            Self::Recording(recording) => recording.is_none(),
            Self::Group(group) => group.is_none(),
            Self::Artist(artist) => artist.is_none(),
            Self::Releases(found) => found.is_empty(),
            Self::Barcoded(found) => found.is_empty(),
            Self::Recordings(found) => found.is_empty(),
            Self::RecordingMatches(found) => found.is_empty(),
            Self::Groups(found) => found.is_empty(),
            Self::Albums(found) => found.is_empty(),
            Self::Artists(found) => found.is_empty(),
            Self::Pressings(found) => found.is_empty(),
            Self::Discography(discography) => discography.releases.is_empty(),
            Self::ArtistPressings(pressings) => pressings.pressings.is_empty(),
        }
    }

    pub fn entities(&self) -> Vec<(Entity, Mbid)> {
        match self {
            Self::Release(Some(release)) => vec![(Entity::Release, release.id.clone())],
            Self::Pressings(releases) => releases
                .iter()
                .map(|release| (Entity::Release, release.id.clone()))
                .collect(),
            Self::ArtistPressings(pressings) => pressings
                .pressings
                .iter()
                .map(|release| (Entity::Release, release.id.clone()))
                .collect(),
            Self::Recording(Some(recording)) => vec![(Entity::Recording, recording.id.clone())],
            Self::Recordings(recordings) => recordings
                .iter()
                .map(|recording| (Entity::Recording, recording.id.clone()))
                .collect(),
            Self::Group(Some(group)) => vec![(Entity::Group, group.id.clone())],
            Self::Artist(Some(artist)) => vec![(Entity::Artist, artist.mbid.clone())],
            _ => Vec::new(),
        }
    }

    pub fn entity(self, entity: Entity, id: &Mbid) -> Option<Self> {
        match (entity, self) {
            (Entity::Release, Self::Release(Some(release))) if release.id == *id => {
                Some(Self::Release(Some(release)))
            }
            (Entity::Release, Self::Pressings(releases)) => releases
                .into_iter()
                .find(|release| release.id == *id)
                .map(|release| Self::Release(Some(release))),
            (Entity::Release, Self::ArtistPressings(pressings)) => pressings
                .pressings
                .into_iter()
                .find(|release| release.id == *id)
                .map(|release| Self::Release(Some(release))),
            (Entity::Recording, Self::Recording(Some(recording))) if recording.id == *id => {
                Some(Self::Recording(Some(recording)))
            }
            (Entity::Recording, Self::Recordings(recordings)) => recordings
                .into_iter()
                .find(|recording| recording.id == *id)
                .map(|recording| Self::Recording(Some(recording))),
            (Entity::Group, Self::Group(Some(group))) if group.id == *id => {
                Some(Self::Group(Some(group)))
            }
            (Entity::Artist, Self::Artist(Some(artist))) if artist.mbid == *id => {
                Some(Self::Artist(Some(artist)))
            }
            _ => None,
        }
    }
}

fn maybe<T>(writer: &mut Writer, value: Option<&T>, write: impl FnOnce(&mut Writer, &T)) {
    match value {
        Some(value) => {
            writer.byte(1);
            write(writer, value);
        }
        None => writer.byte(0),
    }
}
