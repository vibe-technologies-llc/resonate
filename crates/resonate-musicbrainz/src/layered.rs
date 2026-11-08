use std::{path::Path, sync::Arc};

use parking_lot::RwLock;
use resonate_core::{Isrc, Link, Mbid, SourceId};
use resonate_library::{
    AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings, ArtistProfile,
    Barcode, BarcodeMatch, CoverArt, Discography, GroupAsked, GroupMatch, LinkNames,
    LinkedPlaylist, LinkedProfile, LyricText, LyricsAsked, PlaylistLink, ProfileLink, Recording,
    RecordingAsked, RecordingMatch, Reference, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch,
    SongLink, SongsAsked, StreamAsked,
};

use crate::{Answer, Asked, Kept, Mirror};

pub const MUSICBRAINZ: &str = "musicbrainz";

type Looked<T> = resonate_library::Result<T>;

#[derive(Default)]
pub struct Mirrored(RwLock<Option<Arc<Mirror>>>);

impl Mirrored {
    pub fn held(&self) -> Option<Arc<Mirror>> {
        self.0.read().clone()
    }

    pub fn hold(&self, mirror: Option<Arc<Mirror>>) {
        *self.0.write() = mirror;
    }
}

pub trait Peers: Send + Sync {
    fn consult(&self, asked: &Asked) -> Option<Answer>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Kept,
    Mirror,
    Peer,
    Online,
}

pub struct Layered {
    source: SourceId,
    kept: Option<Arc<Kept>>,
    mirror: Option<Arc<Mirrored>>,
    peers: Option<Arc<dyn Peers>>,
    fallback: Option<Arc<dyn Reference>>,
}

impl Layered {
    pub fn new() -> Self {
        Self {
            source: SourceId::new(MUSICBRAINZ).unwrap_or_else(|_| SourceId::local()),
            kept: None,
            mirror: None,
            peers: None,
            fallback: None,
        }
    }

    pub fn keeping(self, kept: Option<Arc<Kept>>) -> Self {
        Self { kept, ..self }
    }

    pub fn mirrored(self, mirror: Option<Arc<Mirrored>>) -> Self {
        Self { mirror, ..self }
    }

    pub fn consulting(self, peers: Option<Arc<dyn Peers>>) -> Self {
        Self { peers, ..self }
    }

    pub fn falling_back_on(self, fallback: Option<Arc<dyn Reference>>) -> Self {
        let source = fallback
            .as_ref()
            .map_or_else(|| self.source.clone(), |fallback| fallback.source().clone());
        Self {
            source,
            fallback,
            ..self
        }
    }

    pub fn answers_anything(&self) -> bool {
        self.kept.is_some()
            || self.peers.is_some()
            || self.fallback.is_some()
            || self
                .mirror
                .as_ref()
                .is_some_and(|mirror| mirror.held().is_some())
    }

    pub fn answered(&self, asked: &Asked) -> Option<(Layer, Answer)> {
        if let Some(kept) = &self.kept {
            match kept.answer(asked) {
                Ok(Some(answer)) => return Some((Layer::Kept, answer)),
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "the kept answers could not be read"),
            }
        }
        if let Some(mirror) = self.mirror.as_ref().and_then(|mirror| mirror.held()) {
            match mirror.answer(asked) {
                Ok(answer) if !answer.is_a_miss() => {
                    self.kept_here(asked, &answer);
                    return Some((Layer::Mirror, answer));
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "the MusicBrainz mirror could not be read"),
            }
        }
        if let Some(peers) = &self.peers
            && let Some(answer) = peers.consult(asked)
            && asked.is_answered_by(&answer)
            && !answer.is_a_miss()
        {
            self.kept_here(asked, &answer);
            return Some((Layer::Peer, answer));
        }
        None
    }

    fn kept_here(&self, asked: &Asked, answer: &Answer) {
        if let Some(kept) = &self.kept
            && let Err(error) = kept.keep(asked, answer)
        {
            tracing::warn!(%error, "an answer could not be kept");
        }
    }

    fn asked(
        &self,
        asked: Asked,
        online: impl FnOnce(&dyn Reference) -> Looked<Answer>,
        miss: Answer,
    ) -> Looked<Answer> {
        if let Some((_, answer)) = self.answered(&asked) {
            return Ok(answer);
        }
        let Some(fallback) = &self.fallback else {
            return Ok(miss);
        };
        let answer = online(fallback.as_ref())?;
        if !answer.is_a_miss() {
            self.kept_here(&asked, &answer);
        }
        Ok(answer)
    }
}

impl Default for Layered {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! answered_as {
    ($answer:expr, $variant:ident, $miss:expr) => {
        match $answer {
            Answer::$variant(held) => held,
            _ => $miss,
        }
    };
}

impl Reference for Layered {
    fn share_file(&self, path: &Path) -> Looked<Option<String>> {
        match &self.fallback {
            Some(fallback) => fallback.share_file(path),
            None => Ok(None),
        }
    }

    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, id: &Mbid) -> Looked<Option<Release>> {
        let answer = self.asked(
            Asked::Release(id.clone()),
            |online| online.release(id).map(Answer::Release),
            Answer::Release(None),
        )?;
        Ok(answered_as!(answer, Release, None))
    }

    fn find_release(&self, asked: &ReleaseAsked) -> Looked<Vec<ReleaseMatch>> {
        let answer = self.asked(
            Asked::FindRelease(asked.clone()),
            |online| online.find_release(asked).map(Answer::Releases),
            Answer::Releases(Vec::new()),
        )?;
        Ok(answered_as!(answer, Releases, Vec::new()))
    }

    fn releases_by_barcode(&self, barcode: &Barcode) -> Looked<Vec<BarcodeMatch>> {
        let answer = self.asked(
            Asked::Barcode(barcode.clone()),
            |online| online.releases_by_barcode(barcode).map(Answer::Barcoded),
            Answer::Barcoded(Vec::new()),
        )?;
        Ok(answered_as!(answer, Barcoded, Vec::new()))
    }

    fn recording(&self, id: &Mbid) -> Looked<Option<Recording>> {
        let answer = self.asked(
            Asked::Recording(id.clone()),
            |online| online.recording(id).map(Answer::Recording),
            Answer::Recording(None),
        )?;
        Ok(answered_as!(answer, Recording, None))
    }

    fn recordings_of_isrc(&self, isrc: &Isrc) -> Looked<Vec<Recording>> {
        let answer = self.asked(
            Asked::Isrc(isrc.clone()),
            |online| online.recordings_of_isrc(isrc).map(Answer::Recordings),
            Answer::Recordings(Vec::new()),
        )?;
        Ok(answered_as!(answer, Recordings, Vec::new()))
    }

    fn find_recording(&self, asked: &RecordingAsked) -> Looked<Vec<RecordingMatch>> {
        let answer = self.asked(
            Asked::FindRecording(asked.clone()),
            |online| online.find_recording(asked).map(Answer::RecordingMatches),
            Answer::RecordingMatches(Vec::new()),
        )?;
        Ok(answered_as!(answer, RecordingMatches, Vec::new()))
    }

    fn find_songs(&self, asked: &SongsAsked) -> Looked<Vec<RecordingMatch>> {
        let answer = self.asked(
            Asked::FindSongs(asked.clone()),
            |online| online.find_songs(asked).map(Answer::RecordingMatches),
            Answer::RecordingMatches(Vec::new()),
        )?;
        Ok(answered_as!(answer, RecordingMatches, Vec::new()))
    }

    fn release_group(&self, id: &Mbid) -> Looked<Option<ReleaseGroup>> {
        let answer = self.asked(
            Asked::Group(id.clone()),
            |online| online.release_group(id).map(Answer::Group),
            Answer::Group(None),
        )?;
        Ok(answered_as!(answer, Group, None))
    }

    fn find_release_group(&self, asked: &GroupAsked) -> Looked<Vec<GroupMatch>> {
        let answer = self.asked(
            Asked::FindGroup(asked.clone()),
            |online| online.find_release_group(asked).map(Answer::Groups),
            Answer::Groups(Vec::new()),
        )?;
        Ok(answered_as!(answer, Groups, Vec::new()))
    }

    fn find_albums(&self, words: &str) -> Looked<Vec<AlbumMatch>> {
        let answer = self.asked(
            Asked::FindAlbums(words.to_owned()),
            |online| online.find_albums(words).map(Answer::Albums),
            Answer::Albums(Vec::new()),
        )?;
        Ok(answered_as!(answer, Albums, Vec::new()))
    }

    fn group_cover(&self, group: &Mbid) -> Looked<Option<CoverArt>> {
        match &self.fallback {
            Some(fallback) => fallback.group_cover(group),
            None => Ok(None),
        }
    }

    fn artist(&self, id: &Mbid) -> Looked<Option<ArtistProfile>> {
        let answer = self.asked(
            Asked::Artist(id.clone()),
            |online| online.artist(id).map(Answer::Artist),
            Answer::Artist(None),
        )?;
        Ok(answered_as!(answer, Artist, None))
    }

    fn find_artist(&self, name: &str) -> Looked<Vec<ArtistMatch>> {
        let answer = self.asked(
            Asked::FindArtist(name.to_owned()),
            |online| online.find_artist(name).map(Answer::Artists),
            Answer::Artists(Vec::new()),
        )?;
        Ok(answered_as!(answer, Artists, Vec::new()))
    }

    fn release_groups_of(&self, artist: &Mbid, from: u32) -> Looked<Discography> {
        let answer = self.asked(
            Asked::GroupsOf {
                artist: artist.clone(),
                from,
            },
            |online| {
                online
                    .release_groups_of(artist, from)
                    .map(Answer::Discography)
            },
            Answer::Discography(Discography::default()),
        )?;
        Ok(answered_as!(answer, Discography, Discography::default()))
    }

    fn releases_of_group(&self, group: &Mbid) -> Looked<Vec<Release>> {
        let answer = self.asked(
            Asked::ReleasesOfGroup(group.clone()),
            |online| online.releases_of_group(group).map(Answer::Pressings),
            Answer::Pressings(Vec::new()),
        )?;
        Ok(answered_as!(answer, Pressings, Vec::new()))
    }

    fn releases_of_artist(&self, artist: &Mbid, from: u32) -> Looked<ArtistPressings> {
        let answer = self.asked(
            Asked::ReleasesOfArtist {
                artist: artist.clone(),
                from,
            },
            |online| {
                online
                    .releases_of_artist(artist, from)
                    .map(Answer::ArtistPressings)
            },
            Answer::ArtistPressings(ArtistPressings::default()),
        )?;
        Ok(answered_as!(
            answer,
            ArtistPressings,
            ArtistPressings::default()
        ))
    }

    fn cover(&self, release: &Mbid, group: Option<&Mbid>) -> Looked<Option<CoverArt>> {
        match &self.fallback {
            Some(fallback) => fallback.cover(release, group),
            None => Ok(None),
        }
    }

    fn portrait(&self, links: &[Link]) -> Looked<Option<CoverArt>> {
        match &self.fallback {
            Some(fallback) => fallback.portrait(links),
            None => Ok(None),
        }
    }

    fn streamed_at(&self, asked: &StreamAsked) -> Looked<Option<Link>> {
        match &self.fallback {
            Some(fallback) => fallback.streamed_at(asked),
            None => Ok(None),
        }
    }

    fn lyrics(&self, asked: &LyricsAsked) -> Looked<Option<LyricText>> {
        match &self.fallback {
            Some(fallback) => fallback.lyrics(asked),
            None => Ok(None),
        }
    }

    fn song_linked(&self, link: &SongLink) -> Looked<Option<LinkNames>> {
        match &self.fallback {
            Some(fallback) => fallback.song_linked(link),
            None => Ok(None),
        }
    }

    fn album_linked(&self, link: &AlbumLink) -> Looked<Option<AlbumNames>> {
        match &self.fallback {
            Some(fallback) => fallback.album_linked(link),
            None => Ok(None),
        }
    }

    fn artist_linked(&self, link: &ArtistLink) -> Looked<Option<String>> {
        match &self.fallback {
            Some(fallback) => fallback.artist_linked(link),
            None => Ok(None),
        }
    }

    fn playlist_linked(&self, link: &PlaylistLink) -> Looked<Option<LinkedPlaylist>> {
        match &self.fallback {
            Some(fallback) => fallback.playlist_linked(link),
            None => Ok(None),
        }
    }

    fn profile_linked(&self, link: &ProfileLink) -> Looked<Option<LinkedProfile>> {
        match &self.fallback {
            Some(fallback) => fallback.profile_linked(link),
            None => Ok(None),
        }
    }

    fn artist_at(&self, page: &str) -> Looked<Option<Mbid>> {
        match &self.fallback {
            Some(fallback) => fallback.artist_at(page),
            None => Ok(None),
        }
    }
}
