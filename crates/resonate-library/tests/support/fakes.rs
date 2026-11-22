use std::{
    fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, SourceId};
use resonate_library::{
    AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings, ArtistProfile,
    ArtistRelease, Barcode, BarcodeMatch, Billed, CoverArt, Discography, Error, GroupAsked,
    GroupMatch, Isrc, Link, LinkNames, LinkedPlaylist, LinkedProfile, ListeningService, LookupOp,
    Love, Loved, LovedNames, LyricText, LyricsAsked, Mbid, PlaylistLink, ProfileLink, Recording,
    RecordingAsked, RecordingMatch, Reference, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch,
    Removal, Result, SUBMITTED_AT_ONCE, Scrobble, Scrobbler, SongLink, SongsAsked, StreamAsked,
    TokenHeld, Trashes, Wording,
};
use resonate_providers::{
    Delivery, Error as ProvidedError, Extension, Identity, Obtained, Opened, Opening, Provider,
    ProviderOp, Providers, Result as ProvidedResult,
};

pub(crate) struct TrashedByTheDesktop {
    pub(crate) asked: Mutex<Vec<PathBuf>>,
    pub(crate) answer: Removal,
}

impl Trashes for TrashedByTheDesktop {
    fn trash(&self, file: &Path) -> Removal {
        self.asked.lock().push(file.to_path_buf());
        if self.answer == Removal::Removed {
            fs::remove_file(file).expect("the desktop takes the file");
        }
        self.answer
    }
}

#[derive(Default)]
pub(crate) struct Told {
    pub(crate) batches: Mutex<Vec<Vec<Scrobble>>>,
    pub(crate) loves: Mutex<Vec<(Mbid, Love)>>,
    pub(crate) named: Mutex<Vec<Option<LovedNames>>>,
    pub(crate) malformed: Option<&'static str>,
    pub(crate) love_refused: Option<(&'static str, u16)>,
    pub(crate) unreachable: std::sync::atomic::AtomicBool,
    pub(crate) at_once: Option<NonZeroUsize>,
    pub(crate) batches_before_unreachable: Option<usize>,
}

impl Told {
    pub(crate) fn taking(at_once: usize, batches_before_unreachable: usize) -> Self {
        Self {
            at_once: NonZeroUsize::new(at_once),
            batches_before_unreachable: Some(batches_before_unreachable),
            ..Self::default()
        }
    }

    pub(crate) fn refusing(title: &'static str) -> Self {
        Self {
            malformed: Some(title),
            ..Self::default()
        }
    }

    pub(crate) fn refusing_love(recording: &'static str, status: u16) -> Self {
        Self {
            love_refused: Some((recording, status)),
            ..Self::default()
        }
    }

    pub(crate) fn titles(&self) -> Vec<Vec<String>> {
        self.batches
            .lock()
            .iter()
            .map(|batch| batch.iter().map(|told| told.billed.title.clone()).collect())
            .collect()
    }
}

impl Scrobbler for Told {
    fn service(&self) -> ListeningService {
        ListeningService::ListenBrainz
    }

    fn at_once(&self) -> NonZeroUsize {
        self.at_once
            .unwrap_or(NonZeroUsize::new(SUBMITTED_AT_ONCE).expect("a batch size"))
    }

    fn submit(&self, listens: &[Scrobble]) -> Result<()> {
        let broken = self
            .batches_before_unreachable
            .is_some_and(|accepted| self.batches.lock().len() >= accepted);

        if broken || self.unreachable.load(Ordering::Relaxed) {
            return Err(Error::Unreachable {
                op: LookupOp::Submit,
                source: io::Error::from(io::ErrorKind::ConnectionRefused),
            });
        }
        self.batches.lock().push(listens.to_vec());
        match self.malformed {
            Some(title) if listens.iter().any(|told| told.billed.title == title) => {
                Err(Error::Refused {
                    op: LookupOp::Submit,
                    status: 400,
                })
            }
            _ => Ok(()),
        }
    }

    fn playing_now(&self, _playing: &Billed) -> Result<()> {
        Ok(())
    }

    fn love(&self, loved: &Loved, love: Love) -> Result<()> {
        match self.love_refused {
            Some((refused, status)) if loved.recording.as_str() == refused => Err(Error::Refused {
                op: LookupOp::Love,
                status,
            }),
            _ => {
                self.loves.lock().push((loved.recording.clone(), love));
                self.named.lock().push(loved.named.clone());
                Ok(())
            }
        }
    }

    fn token_held(&self) -> Result<TokenHeld> {
        Ok(TokenHeld::Unknown)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) tagged_title: Option<String>,
    pub(crate) tagged_artist: Option<String>,
    pub(crate) isrc: Option<String>,
    pub(crate) release_title: Option<String>,
    pub(crate) asked: Option<i64>,
    pub(crate) answered: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Called {
    Release(Mbid),
    FindRelease(ReleaseAsked),
    Recording(Mbid),
    Isrc(Isrc),
    FindRecording(RecordingAsked),
    FindSongs(String),
    ReleaseGroup(Mbid),
    FindReleaseGroup(GroupAsked),
    FindAlbums(String),
    Artist(Mbid),
    FindArtist(String),
    ReleaseGroupsOf(Mbid),
    ReleasesOfGroup(Mbid),
    ReleasesOfArtist(Mbid, u32),
    Cover(Mbid, Option<Mbid>),
    GroupCover(Mbid),
    Portrait(String),
    StreamedAt(StreamAsked),
    SongLinked(SongLink),
    AlbumLinked(AlbumLink),
    ArtistLinked(ArtistLink),
    ArtistAt(String),
    PlaylistLinked(PlaylistLink),
    ByBarcode(Barcode),
}

impl Called {
    pub(crate) fn op(&self) -> LookupOp {
        match self {
            Self::Release(_) => LookupOp::Release,
            Self::FindRelease(_) => LookupOp::FindRelease,
            Self::Recording(_) => LookupOp::Recording,
            Self::Isrc(_) => LookupOp::Isrc,
            Self::FindRecording(_) | Self::FindSongs(_) => LookupOp::FindRecording,
            Self::ReleaseGroup(_) => LookupOp::ReleaseGroup,
            Self::FindReleaseGroup(_) | Self::FindAlbums(_) => LookupOp::FindReleaseGroup,
            Self::Artist(_) => LookupOp::Artist,
            Self::FindArtist(_) => LookupOp::FindArtist,
            Self::ReleaseGroupsOf(_) => LookupOp::ReleaseGroupsOfArtist,
            Self::ReleasesOfGroup(_) => LookupOp::ReleasesOfGroup,
            Self::ReleasesOfArtist(..) => LookupOp::ReleasesOfArtist,
            Self::Cover(..) => LookupOp::Cover,
            Self::GroupCover(_) => LookupOp::Cover,
            Self::Portrait(_) => LookupOp::Portrait,
            Self::StreamedAt(_) => LookupOp::StreamLink,
            Self::SongLinked(_)
            | Self::AlbumLinked(_)
            | Self::ArtistLinked(_)
            | Self::ArtistAt(_)
            | Self::PlaylistLinked(_) => LookupOp::FollowLink,
            Self::ByBarcode(_) => LookupOp::FindRelease,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    Refused,
    Unreachable,
    Offline,
    Metered,
    TooLarge,
}

#[derive(Default)]
pub(crate) struct Canned {
    pub(crate) releases: Vec<Release>,
    pub(crate) found_releases: Vec<ReleaseMatch>,
    pub(crate) found_releases_in_words: Vec<ReleaseMatch>,
    pub(crate) recordings: Vec<Recording>,
    pub(crate) found_recordings: Vec<RecordingMatch>,
    pub(crate) found_recordings_in_words: Vec<RecordingMatch>,
    pub(crate) found_songs: Vec<RecordingMatch>,
    pub(crate) isrcs: Vec<(Isrc, Recording)>,
    pub(crate) groups: Vec<ReleaseGroup>,
    pub(crate) found_groups: Vec<GroupMatch>,
    pub(crate) found_albums: Vec<AlbumMatch>,
    pub(crate) found_groups_in_words: Vec<GroupMatch>,
    pub(crate) artists: Vec<ArtistProfile>,
    pub(crate) found_artists: Vec<ArtistMatch>,
    pub(crate) artist_releases: Vec<(Mbid, Vec<ArtistRelease>)>,
    pub(crate) releases_unread: u32,
    pub(crate) further_releases: Vec<ArtistRelease>,
    pub(crate) pressings: Vec<(Mbid, Vec<Release>)>,
    pub(crate) artist_pressings: Vec<(Mbid, Vec<ArtistPressings>)>,
    pub(crate) covers: Vec<(Mbid, CoverArt)>,
    pub(crate) group_covers: Vec<(Mbid, CoverArt)>,
    pub(crate) portraits: Vec<(String, CoverArt)>,
    pub(crate) streamed: Option<Link>,
    pub(crate) lyrics: Vec<(&'static str, LyricText)>,
    pub(crate) linked: Option<LinkNames>,
    pub(crate) album_linked: Option<AlbumNames>,
    pub(crate) artist_linked: Option<String>,
    pub(crate) artists_at: Vec<(String, Mbid)>,
    pub(crate) playlist_linked: Option<LinkedPlaylist>,
    pub(crate) playlists_linked: Vec<(PlaylistLink, LinkedPlaylist)>,
    pub(crate) profile_linked: Option<LinkedProfile>,
    pub(crate) barcoded: Vec<BarcodeMatch>,
}

pub(crate) struct Gate {
    pub(crate) op: Option<LookupOp>,
    pub(crate) at: usize,
    pub(crate) started: Sender<()>,
    pub(crate) go: Receiver<()>,
}

pub(crate) struct Fake {
    pub(crate) source: SourceId,
    pub(crate) canned: Canned,
    pub(crate) faults: Vec<(LookupOp, usize, Fault)>,
    pub(crate) calls: Mutex<Vec<Called>>,
    pub(crate) gate: Mutex<Option<Gate>>,
    pub(crate) sung: Mutex<Vec<LyricsAsked>>,
}

impl Fake {
    pub(crate) fn new(canned: Canned) -> Self {
        Self {
            source: SourceId::new("musicbrainz").expect("a nameable source"),
            canned,
            faults: Vec::new(),
            calls: Mutex::new(Vec::new()),
            gate: Mutex::new(None),
            sung: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn faulting(mut self, op: LookupOp, at: usize, fault: Fault) -> Self {
        self.faults.push((op, at, fault));
        self
    }

    pub(crate) fn gated(self) -> (Self, Receiver<()>, Sender<()>) {
        self.gated_on(None)
    }

    pub(crate) fn gated_on(self, op: Option<LookupOp>) -> (Self, Receiver<()>, Sender<()>) {
        self.gated_on_the(op, 0)
    }

    pub(crate) fn gated_on_the(
        self,
        op: Option<LookupOp>,
        at: usize,
    ) -> (Self, Receiver<()>, Sender<()>) {
        let (started, has_started) = mpsc::channel();
        let (go, may_go) = mpsc::channel();

        *self.gate.lock() = Some(Gate {
            op,
            at,
            started,
            go: may_go,
        });
        (self, has_started, go)
    }

    pub(crate) fn calls(&self) -> Vec<Called> {
        self.calls.lock().clone()
    }

    pub(crate) fn called(&self, op: LookupOp) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|called| called.op() == op)
            .count()
    }

    pub(crate) fn note(&self, called: Called) -> Result<()> {
        let op = called.op();
        let nth = self.called(op);

        self.calls.lock().push(called);

        let gate = {
            let mut held = self.gate.lock();
            match held.as_ref() {
                Some(gate) if gate.op.is_none_or(|wanted| wanted == op) && gate.at == nth => {
                    held.take()
                }
                Some(_) | None => None,
            }
        };

        if let Some(gate) = gate {
            let _ = gate.started.send(());
            let _ = gate.go.recv();
        }

        self.fault(op, nth)
    }

    pub(crate) fn sung(&self) -> Vec<LyricsAsked> {
        self.sung.lock().clone()
    }

    pub(crate) fn fault(&self, op: LookupOp, nth: usize) -> Result<()> {
        match self
            .faults
            .iter()
            .find(|(faulted, at, _)| *faulted == op && *at == nth)
        {
            Some((_, _, Fault::Refused)) => Err(Error::Refused { op, status: 503 }),
            Some((_, _, Fault::TooLarge)) => Err(Error::TooLarge {
                op,
                limit: 4 * 1024 * 1024,
            }),
            Some((_, _, Fault::Unreachable)) => Err(Error::Unreachable {
                op,
                source: io::Error::from(io::ErrorKind::ConnectionRefused),
            }),
            Some((_, _, Fault::Offline)) => Err(Error::Offline { op }),
            Some((_, _, Fault::Metered)) => Err(Error::Metered { op }),
            None => Ok(()),
        }
    }
}

impl Reference for Fake {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, id: &Mbid) -> Result<Option<Release>> {
        self.note(Called::Release(id.clone()))?;
        Ok(self
            .canned
            .releases
            .iter()
            .find(|release| release.id == *id)
            .cloned())
    }

    fn find_release(&self, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
        self.note(Called::FindRelease(asked.clone()))?;
        Ok(match asked.wording {
            Wording::Phrase => self.canned.found_releases.clone(),
            Wording::Words => self.canned.found_releases_in_words.clone(),
        })
    }

    fn recording(&self, id: &Mbid) -> Result<Option<Recording>> {
        self.note(Called::Recording(id.clone()))?;
        Ok(self
            .canned
            .recordings
            .iter()
            .find(|recording| recording.id == *id)
            .cloned())
    }

    fn recordings_of_isrc(&self, isrc: &Isrc) -> Result<Vec<Recording>> {
        self.note(Called::Isrc(isrc.clone()))?;
        Ok(self
            .canned
            .isrcs
            .iter()
            .filter(|(held, _)| held == isrc)
            .map(|(_, recording)| recording.clone())
            .collect())
    }

    fn find_recording(&self, asked: &RecordingAsked) -> Result<Vec<RecordingMatch>> {
        self.note(Called::FindRecording(asked.clone()))?;
        Ok(match asked.wording {
            Wording::Phrase => self.canned.found_recordings.clone(),
            Wording::Words => self.canned.found_recordings_in_words.clone(),
        })
    }

    fn find_songs(&self, asked: &SongsAsked) -> Result<Vec<RecordingMatch>> {
        self.note(Called::FindSongs(asked.words.clone()))?;
        Ok(self.canned.found_songs.clone())
    }

    fn release_group(&self, id: &Mbid) -> Result<Option<ReleaseGroup>> {
        self.note(Called::ReleaseGroup(id.clone()))?;
        Ok(self
            .canned
            .groups
            .iter()
            .find(|group| group.id == *id)
            .cloned())
    }

    fn find_release_group(&self, asked: &GroupAsked) -> Result<Vec<GroupMatch>> {
        self.note(Called::FindReleaseGroup(asked.clone()))?;
        Ok(match asked.wording {
            Wording::Phrase => self.canned.found_groups.clone(),
            Wording::Words => self.canned.found_groups_in_words.clone(),
        })
    }

    fn find_albums(&self, words: &str) -> Result<Vec<AlbumMatch>> {
        self.note(Called::FindAlbums(words.to_owned()))?;
        Ok(self.canned.found_albums.clone())
    }

    fn artist(&self, id: &Mbid) -> Result<Option<ArtistProfile>> {
        self.note(Called::Artist(id.clone()))?;
        Ok(self
            .canned
            .artists
            .iter()
            .find(|profile| profile.mbid == *id)
            .cloned())
    }

    fn find_artist(&self, name: &str) -> Result<Vec<ArtistMatch>> {
        self.note(Called::FindArtist(name.to_owned()))?;
        Ok(self.canned.found_artists.clone())
    }

    fn releases_of_artist(&self, artist: &Mbid, from: u32) -> Result<ArtistPressings> {
        self.note(Called::ReleasesOfArtist(artist.clone(), from))?;

        let pages = self
            .canned
            .artist_pressings
            .iter()
            .find(|(held, _)| held == artist)
            .map(|(_, pages)| pages.as_slice())
            .unwrap_or_default();
        let mut starts = std::iter::once(0).chain(pages.iter().map(|page| page.read_to));
        Ok(pages
            .iter()
            .find(|_| starts.next() == Some(from))
            .cloned()
            .unwrap_or_default())
    }

    fn releases_of_group(&self, group: &Mbid) -> Result<Vec<Release>> {
        self.note(Called::ReleasesOfGroup(group.clone()))?;
        Ok(self
            .canned
            .pressings
            .iter()
            .find(|(held, _)| held == group)
            .map(|(_, pressings)| pressings.clone())
            .unwrap_or_default())
    }

    fn release_groups_of(&self, artist: &Mbid, from: u32) -> Result<Discography> {
        const FIRST_READ: u32 = 1_000;

        self.note(Called::ReleaseGroupsOf(artist.clone()))?;
        if from > 0 {
            return Ok(Discography {
                releases: self.canned.further_releases.clone(),
                unread: 0,
                read_to: from + self.canned.releases_unread,
            });
        }

        let releases = self
            .canned
            .artist_releases
            .iter()
            .find(|(held, _)| held == artist)
            .map(|(_, releases)| releases.clone())
            .unwrap_or_default();
        Ok(Discography {
            read_to: if self.canned.releases_unread > 0 {
                FIRST_READ
            } else {
                releases.len() as u32
            },
            releases,
            unread: self.canned.releases_unread,
        })
    }

    fn cover(&self, release: &Mbid, group: Option<&Mbid>) -> Result<Option<CoverArt>> {
        self.note(Called::Cover(release.clone(), group.cloned()))?;
        Ok(self
            .canned
            .covers
            .iter()
            .find(|(held, _)| held == release)
            .map(|(_, art)| art.clone()))
    }

    fn group_cover(&self, group: &Mbid) -> Result<Option<CoverArt>> {
        self.note(Called::GroupCover(group.clone()))?;
        Ok(self
            .canned
            .group_covers
            .iter()
            .find(|(held, _)| held == group)
            .map(|(_, art)| art.clone()))
    }

    fn portrait(&self, links: &[Link]) -> Result<Option<CoverArt>> {
        for url in resonate_library::portrait_urls(links) {
            self.note(Called::Portrait(url.to_owned()))?;
            if let Some(art) = self
                .canned
                .portraits
                .iter()
                .find(|(held, _)| held == url)
                .map(|(_, art)| art.clone())
            {
                return Ok(Some(art));
            }
        }

        Ok(None)
    }

    fn streamed_at(&self, asked: &StreamAsked) -> Result<Option<Link>> {
        self.note(Called::StreamedAt(asked.clone()))?;
        Ok(self.canned.streamed.clone())
    }

    fn song_linked(&self, link: &SongLink) -> Result<Option<LinkNames>> {
        self.note(Called::SongLinked(link.clone()))?;
        Ok(self.canned.linked.clone())
    }

    fn album_linked(&self, link: &AlbumLink) -> Result<Option<AlbumNames>> {
        self.note(Called::AlbumLinked(link.clone()))?;
        Ok(self.canned.album_linked.clone())
    }

    fn artist_linked(&self, link: &ArtistLink) -> Result<Option<String>> {
        self.note(Called::ArtistLinked(link.clone()))?;
        Ok(self.canned.artist_linked.clone())
    }

    fn playlist_linked(&self, link: &PlaylistLink) -> Result<Option<LinkedPlaylist>> {
        self.note(Called::PlaylistLinked(link.clone()))?;
        Ok(self
            .canned
            .playlists_linked
            .iter()
            .find(|(held, _)| held == link)
            .map(|(_, linked)| linked.clone())
            .or_else(|| self.canned.playlist_linked.clone()))
    }

    fn profile_linked(&self, _link: &ProfileLink) -> Result<Option<LinkedProfile>> {
        Ok(self.canned.profile_linked.clone())
    }

    fn artist_at(&self, page: &str) -> Result<Option<Mbid>> {
        self.note(Called::ArtistAt(page.to_owned()))?;
        Ok(self
            .canned
            .artists_at
            .iter()
            .find(|(held, _)| held == page)
            .map(|(_, artist)| artist.clone()))
    }

    fn releases_by_barcode(&self, barcode: &Barcode) -> Result<Vec<BarcodeMatch>> {
        self.note(Called::ByBarcode(barcode.clone()))?;
        Ok(self
            .canned
            .barcoded
            .iter()
            .filter(|found| {
                found
                    .barcode
                    .as_deref()
                    .is_some_and(|held| barcode.names(held))
            })
            .cloned()
            .collect())
    }

    fn lyrics(&self, asked: &LyricsAsked) -> Result<Option<LyricText>> {
        let nth = {
            let mut sung = self.sung.lock();

            sung.push(asked.clone());
            sung.len() - 1
        };

        self.fault(LookupOp::Lyrics, nth)?;
        Ok(self
            .canned
            .lyrics
            .iter()
            .find(|(title, _)| *title == asked.title)
            .map(|(_, told)| told.clone()))
    }
}

#[derive(Clone)]
pub(crate) enum Delivering {
    Nothing,
    File(PathBuf),
    Bytes {
        key: &'static str,
        extension: &'static str,
        bytes: Vec<u8>,
    },
    Endless,
    Stalling,
    DyingPartWay,
    Unopenable,
    GoneWhenOpened,
    GoneUntilTold {
        bytes: Vec<u8>,
    },
}

pub(crate) struct Trickle;

pub(crate) struct Stall {
    pub(crate) began: bool,
}

impl io::Read for Stall {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if !self.began {
            self.began = true;

            let given = into.len().min(64);

            into[..given].fill(0);
            return Ok(given);
        }
        loop {
            thread::park();
        }
    }
}

pub(crate) struct DiesPartWay {
    pub(crate) provider: SourceId,
    pub(crate) began: bool,
}

impl io::Read for DiesPartWay {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if !self.began {
            self.began = true;

            let given = into.len().min(64);

            into[..given].fill(0);
            return Ok(given);
        }
        Err(io::Error::other(ProvidedError::ToolFailed {
            provider: self.provider.clone(),
            op: ProviderOp::Download,
            code: Some(1),
        }))
    }
}

impl io::Read for Trickle {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        thread::sleep(Duration::from_millis(5));

        let given = into.len().min(64);

        into[..given].fill(0);
        Ok(given)
    }
}

pub(crate) struct Offering {
    pub(crate) source: SourceId,
    pub(crate) delivering: Delivering,
    pub(crate) asked: Mutex<Vec<Identity>>,
    pub(crate) opened: Arc<AtomicUsize>,
    pub(crate) answers_after: Duration,
}

impl Offering {
    pub(crate) fn new(source: &str, delivering: Delivering) -> Self {
        Self {
            source: SourceId::new(source).expect("a nameable source"),
            delivering,
            asked: Mutex::new(Vec::new()),
            opened: Arc::new(AtomicUsize::new(0)),
            answers_after: Duration::ZERO,
        }
    }

    pub(crate) fn answering_after(self, wait: Duration) -> Self {
        Self {
            answers_after: wait,
            ..self
        }
    }

    pub(crate) fn opened(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

    pub(crate) fn counted(&self, opening: Opening) -> Opening {
        let opened = Arc::clone(&self.opened);
        Opening::new(move || {
            opened.fetch_add(1, Ordering::SeqCst);
            opening.open()
        })
    }

    pub(crate) fn asked(&self) -> Vec<Identity> {
        self.asked.lock().clone()
    }

    pub(crate) fn registered(self: &Arc<Self>) -> Arc<Providers> {
        Arc::new(Providers::none().and(Arc::clone(self) as Arc<dyn Provider>))
    }
}

impl Provider for Offering {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.asked.lock().push(identity.clone());
        thread::sleep(self.answers_after);
        Ok(match self.delivering.clone() {
            Delivering::Nothing => Obtained::Nothing,
            Delivering::File(path) => Obtained::Found(Delivery::File(path)),
            Delivering::Bytes {
                key,
                extension,
                bytes,
            } => Obtained::Found(Delivery::Stream {
                key: key.into(),
                extension: Extension::new(extension)?,
                opening: self.counted(Opening::ready(std::io::Cursor::new(bytes))),
            }),
            Delivering::Endless => Obtained::Found(Delivery::Stream {
                key: "endless".into(),
                extension: Extension::new("wav")?,
                opening: Opening::ready(Trickle),
            }),
            Delivering::Stalling => Obtained::Found(Delivery::Stream {
                key: "stalling".into(),
                extension: Extension::new("wav")?,
                opening: Opening::ready(Stall { began: false }),
            }),
            Delivering::DyingPartWay => Obtained::Found(Delivery::Stream {
                key: "dying".into(),
                extension: Extension::new("wav")?,
                opening: self.counted(Opening::ready(DiesPartWay {
                    provider: self.source.clone(),
                    began: false,
                })),
            }),
            Delivering::Unopenable => {
                let provider = self.source.clone();
                Obtained::Found(Delivery::Stream {
                    key: "unopenable".into(),
                    extension: Extension::new("wav")?,
                    opening: self.counted(Opening::new(move || {
                        Err(ProvidedError::Refused {
                            provider,
                            op: ProviderOp::Download,
                            status: 500,
                        })
                    })),
                })
            }
            Delivering::GoneWhenOpened => Obtained::Found(Delivery::Stream {
                key: "gone".into(),
                extension: Extension::new("wav")?,
                opening: self.counted(Opening::new(|| Ok(Opened::Gone))),
            }),
            Delivering::GoneUntilTold { bytes } => {
                let linked = MediaLocation::new(self.source.clone(), "track/linked");
                if identity.finds_gone(&linked) {
                    Obtained::Found(Delivery::Stream {
                        key: "track/listed".into(),
                        extension: Extension::new("wav")?,
                        opening: self.counted(Opening::ready(std::io::Cursor::new(bytes))),
                    })
                } else {
                    Obtained::Found(Delivery::Stream {
                        key: "track/linked".into(),
                        extension: Extension::new("wav")?,
                        opening: self.counted(Opening::new(|| Ok(Opened::Gone))),
                    })
                }
            }
        })
    }
}

pub(crate) struct Unanswering {
    pub(crate) source: SourceId,
    pub(crate) failure: fn(SourceId) -> resonate_providers::Error,
    pub(crate) asked: Mutex<usize>,
}

impl Unanswering {
    pub(crate) fn new(
        source: &str,
        failure: fn(SourceId) -> resonate_providers::Error,
    ) -> Arc<Self> {
        Arc::new(Self {
            source: SourceId::new(source).expect("a nameable source"),
            failure,
            asked: Mutex::new(0),
        })
    }

    pub(crate) fn asked(&self) -> usize {
        *self.asked.lock()
    }
}

impl Provider for Unanswering {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _identity: &Identity) -> ProvidedResult<Obtained> {
        *self.asked.lock() += 1;
        Err((self.failure)(self.source.clone()))
    }
}

pub(crate) struct LastResort(pub(crate) Arc<Offering>);

impl Provider for LastResort {
    fn source(&self) -> &SourceId {
        self.0.source()
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.0.find(identity)
    }

    fn last_resort(&self) -> bool {
        true
    }
}

pub(crate) type WhenAsked = Box<dyn FnOnce() + Send>;
