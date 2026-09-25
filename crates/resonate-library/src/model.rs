use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use resonate_codec::{Codec, ReplayGain};
use resonate_core::{
    AlbumId, ArtistId, FrameSpan, Frames, ListenId, MediaLocation, PlaylistId, QueueStamp,
    ReleaseTrackId, StreamSpec, TrackId, WantId,
};
use resonate_vault::{Encoding, Form, VaultKey};

use crate::{Genre, Isrc, Kept, LifeSpan, Link, Mbid, SavedQuery};

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: TrackId,
    pub location: MediaLocation,
    pub title: String,
    pub artist: Option<String>,
    pub artist_id: Option<ArtistId>,
    pub album_id: Option<AlbumId>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration: Option<Frames>,
    pub spec: StreamSpec,
    pub codec: Codec,
    pub replay_gain: ReplayGain,
    pub file_size: u64,
    pub modified: SystemTime,
    pub added: SystemTime,
    pub plays: u32,
    pub played: Option<SystemTime>,
    pub span: Option<FrameSpan>,
    pub favourite: Option<SystemTime>,
    pub genre: Option<String>,
    pub hidden: bool,
    pub alternatives: u32,
    pub delivered: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Counted {
    pub track: Track,
    pub listen: ListenId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Favoured {
    Track(TrackId),
    Album(AlbumId),
    Artist(ArtistId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artist_id: Option<ArtistId>,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub track_count: u32,
    pub artist_count: u32,
    pub has_cover_art: bool,
    pub mbid: Option<Mbid>,
    pub missing: u32,
    pub favourite: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub album_count: u32,
    pub track_count: u32,
    pub mbid: Option<Mbid>,
    pub has_portrait: bool,
    pub favourite: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CoverSource {
    #[default]
    File,
    Archive,
    Vault,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Released {
    pub released: u64,
    pub stranded: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub objects: u64,
    pub covers: u64,
    pub staged: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultObject {
    pub key: VaultKey,
    pub form: Form,
    pub path: PathBuf,
    pub bytes: u64,
    pub spec: StreamSpec,
    pub frames: Option<Frames>,
    pub taken_from: PathBuf,
    pub took: SystemTime,
    pub was_bytes: u64,
    pub was_codec: Codec,
    pub validated: bool,
    pub encoding: Encoding,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Measured {
    pub rows: u32,
    pub length: Option<Duration>,
    pub lossless: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoverWanted {
    pub(crate) album: AlbumId,
    pub(crate) from: CoverFrom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoverFrom {
    Release { release: Mbid, group: Option<Mbid> },
    Group(Mbid),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortraitWanted {
    pub artist: ArtistId,
    pub links: Vec<Link>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArtistTotals {
    pub albums: u32,
    pub tracks: u32,
    pub length: Option<Duration>,
    pub plays: u32,
    pub played: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldMedium {
    pub position: u32,
    pub format: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseDetail {
    pub mbid: Option<Mbid>,
    pub group: Option<Mbid>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub cover_source: CoverSource,
    pub asked: Option<SystemTime>,
    pub answered: Option<SystemTime>,
    pub links: Vec<Link>,
    pub media: Vec<HeldMedium>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldReleaseTrack {
    pub id: ReleaseTrackId,
    pub album: AlbumId,
    pub disc: u32,
    pub position: u32,
    pub number: Option<String>,
    pub title: String,
    pub artist: Option<String>,
    pub recording: Option<Mbid>,
    pub track_mbid: Option<Mbid>,
    pub length: Option<Duration>,
    pub isrc: Option<String>,
    pub track: Option<TrackId>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistDetail {
    pub name: String,
    pub mbid: Option<Mbid>,
    pub sort_name: Option<String>,
    pub kind: Option<String>,
    pub gender: Option<String>,
    pub country: Option<String>,
    pub area: Option<String>,
    pub began_in: Option<String>,
    pub span: LifeSpan,
    pub disambiguation: Option<String>,
    pub asked: Option<SystemTime>,
    pub answered: Option<SystemTime>,
    pub genres: Vec<Genre>,
    pub links: Vec<Link>,
    pub releases_unheld: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingTrack {
    pub album: AlbumId,
    pub album_title: String,
    pub owner: Option<String>,
    pub release_track: ReleaseTrackId,
    pub disc: u32,
    pub position: u32,
    pub number: Option<String>,
    pub title: String,
    pub artist: Option<String>,
    pub length: Option<Duration>,
    pub want: Option<WantId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnheldRelease {
    pub artist: ArtistId,
    pub artist_name: String,
    pub mbid: Mbid,
    pub title: String,
    pub kind: Option<String>,
    pub first_released: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Missing {
    pub tracks: u64,
    pub releases: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Want {
    pub id: WantId,
    pub release_track: ReleaseTrackId,
    pub album: AlbumId,
    pub album_title: String,
    pub title: String,
    pub artist: Option<String>,
    pub recording: Option<Mbid>,
    pub track: Option<Mbid>,
    pub release: Option<Mbid>,
    pub isrc: Option<Isrc>,
    pub length: Option<Duration>,
    pub disc: u32,
    pub position: u32,
    pub wanted: SystemTime,
    pub tried: Option<SystemTime>,
    pub offered: Option<String>,
    pub held: Option<TrackId>,
    pub links: Vec<Link>,
    pub release_links: Vec<Link>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptLyrics {
    pub text: Option<String>,
    pub synced: bool,
    pub taken: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptIndex {
    pub text: String,
    pub taken: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptCorrection {
    pub text: Option<String>,
    pub taken: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unfinished {
    pub refresh: bool,
    pub began: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumToAsk {
    pub id: AlbumId,
    pub title: String,
    pub owner: Option<String>,
    pub track_count: u32,
    pub year: Option<i32>,
    pub mbid: Option<Mbid>,
    pub group: Option<Mbid>,
    pub barcode: Option<String>,
    pub catalog_number: Option<String>,
    pub tagged_tracks: Option<u32>,
    pub owner_mbid: Option<Mbid>,
    pub has_cover: bool,
    pub has_release_rows: bool,
    pub rematch_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackToAsk {
    pub id: TrackId,
    pub title: String,
    pub tagged_title: Option<String>,
    pub artist: Option<String>,
    pub tagged_artist: Option<String>,
    pub artist_mbid: Option<Mbid>,
    pub album: Option<AlbumId>,
    pub album_title: Option<String>,
    pub album_answered: bool,
    pub owner: Option<String>,
    pub owner_mbid: Option<Mbid>,
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
    pub length: Option<Duration>,
    pub mbid: Option<Mbid>,
    pub isrc: Option<Isrc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistToAsk {
    pub id: ArtistId,
    pub name: String,
    pub mbid: Option<Mbid>,
    pub has_portrait: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub entries: u32,
    pub duration: Option<Duration>,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub played: Option<SystemTime>,
    pub plays: u32,
    pub query: Option<SavedQuery>,
    pub kept: Option<Kept>,
    pub pinned: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NamedPlaylist {
    pub id: PlaylistId,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Playing {
    pub playlist: PlaylistId,
    pub queue: QueueStamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cut {
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
}

impl Cut {
    pub const fn whole(location: MediaLocation) -> Self {
        Self {
            location,
            span: None,
        }
    }

    pub fn of(track: &Track) -> Self {
        Self {
            location: track.location.clone(),
            span: track.span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistEntry {
    pub position: usize,
    pub cut: Cut,
    pub track: Option<Track>,
}

impl PlaylistEntry {
    pub const fn location(&self) -> &MediaLocation {
        &self.cut.location
    }

    pub const fn span(&self) -> Option<FrameSpan> {
        self.cut.span
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistFormat {
    #[default]
    M3u,
    Pls,
    Xspf,
}

impl PlaylistFormat {
    const EXTENSIONS: [(&'static str, Self); 4] = [
        ("m3u", Self::M3u),
        ("m3u8", Self::M3u),
        ("pls", Self::Pls),
        ("xspf", Self::Xspf),
    ];

    pub fn of(path: &Path) -> Self {
        let Some(extension) = path.extension().and_then(OsStr::to_str) else {
            return Self::default();
        };

        Self::EXTENSIONS
            .into_iter()
            .find(|(named, _)| named.eq_ignore_ascii_case(extension))
            .map_or_else(Self::default, |(_, format)| format)
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::M3u => "m3u8",
            Self::Pls => "pls",
            Self::Xspf => "xspf",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::M3u => "M3U",
            Self::Pls => "PLS",
            Self::Xspf => "XSPF",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SheetEncoding {
    #[default]
    Utf8,
    Windows1252,
}

impl SheetEncoding {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Windows1252 => "Windows-1252",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Exported {
    pub rows: usize,
    pub format: PlaylistFormat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub id: PlaylistId,
    pub name: String,
    pub added: usize,
    pub already: usize,
    pub elsewhere: usize,
    pub missing: usize,
    pub short: usize,
    pub format: PlaylistFormat,
    pub encoding: SheetEncoding,
}
