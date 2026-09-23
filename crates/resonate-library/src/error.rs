use std::{fmt, io, path::PathBuf, result};

use resonate_core::{
    AlbumId, ArtistId, Mbid, MediaLocation, PlaylistId, ReleaseTrackId, TrackId, WantId,
};
use resonate_vault::VaultKey;
use thiserror::Error;

use crate::{LookupOp, PassKind};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TagName(Box<str>);

impl TagName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaylistName(Box<str>);

impl PlaylistName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlaylistName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldName(Box<str>);

impl FieldName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FieldName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutFault {
    Empty,
    EmptySegment,
    Unclosed,
    Unopened,
    EmptyField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OrderedColumn {
    Sort,
    KeptOrder,
    KeptReading,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EncodedColumn {
    SampleFormat,
    Codec,
    Relation,
    Service,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveOp {
    MakeFolder,
    Rename,
    Copy,
    Stamp,
    Discard,
    Rewrite,
    Settle,
    Prune,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StoreOp {
    Open,
    LayOut,
    Migrate,
    Analyse,
    Prepare,
    Insert,
    Update,
    Delete,
    Query,
    Transaction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaFingerprint(pub u32);

impl fmt::Display for SchemaFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot read {path}", path = path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("library root {path} is not a directory", path = path.display())]
    RootNotADirectory { path: PathBuf },

    #[error("{path} is not a library root", path = path.display())]
    NotARoot { path: PathBuf },

    #[error(
        "library root {path} is inside {inside}, which is a root already",
        path = path.display(),
        inside = inside.display()
    )]
    RootInsideRoot { path: PathBuf, inside: PathBuf },

    #[error("{path} is not valid UTF-8 and cannot be persisted", path = path.display())]
    NonUtf8Path { path: PathBuf },

    #[error("{op:?} failed on {path}", path = path.display())]
    Move {
        op: MoveOp,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("the scanner could not start a thread")]
    ThreadSpawn {
        #[source]
        source: io::Error,
    },

    #[error("the {pass:?} thread stopped without finishing")]
    Stopped { pass: PassKind },

    #[error("a pass is already walking the tree, and only one may walk it at a time")]
    AlreadyWalking,

    #[error("reading tags from {path} failed", path = path.display())]
    Tags {
        path: PathBuf,
        #[source]
        source: Box<resonate_codec::Error>,
    },

    #[error("no track with id {0}")]
    UnknownTrack(TrackId),

    #[error("no album with id {0}")]
    UnknownAlbum(AlbumId),

    #[error("no artist with id {0}")]
    UnknownArtist(ArtistId),

    #[error("no playlist with id {0}")]
    UnknownPlaylist(PlaylistId),

    #[error("no release track with id {0}")]
    UnknownReleaseTrack(ReleaseTrackId),

    #[error("the reference knows no release the recording {recording} came out on")]
    Unreleased { recording: Mbid },

    #[error("the reference holds no release {release}")]
    UnknownRelease { release: Mbid },

    #[error("the release {release} does not carry the recording {recording}")]
    NotOnTheRelease { recording: Mbid, release: Mbid },

    #[error("no want with id {0}")]
    UnknownWant(WantId),

    #[error("the reference could not be reached for {op:?}")]
    Unreachable {
        op: LookupOp,
        #[source]
        source: io::Error,
    },

    #[error("the reference refused {op:?} with status {status}")]
    Refused { op: LookupOp, status: u16 },

    #[error("the reference answered {op:?} with something this build cannot read")]
    Unreadable { op: LookupOp },

    #[error("a playlist is already named {name}")]
    DuplicatePlaylist { name: PlaylistName },

    #[error("a playlist needs a name")]
    UnnamedPlaylist,

    #[error("playlist {playlist} fills itself from a query, and its rows are not a list to edit")]
    NotAList { playlist: PlaylistId },

    #[error("playlist {playlist} holds a list of rows, and has no query to change")]
    NotAQuery { playlist: PlaylistId },

    #[error(
        "playlist {playlist} is kept in an order, and its rows are not a list to place by hand"
    )]
    KeptInOrder { playlist: PlaylistId },

    #[error("playlist {playlist} is both sides of the copy, and would only double itself")]
    IntoItself { playlist: PlaylistId },

    #[error("playlist {playlist} stores {column:?} code {code}, which this build does not know")]
    UnknownOrder {
        playlist: PlaylistId,
        column: OrderedColumn,
        code: i64,
    },

    #[error(
        "{path} holds {held} bytes, past the {limit} a playlist file may hold",
        path = path.display()
    )]
    PlaylistFileTooLarge {
        path: PathBuf,
        held: u64,
        limit: u64,
    },

    #[error("{path} is not valid UTF-8, and a playlist file is read as text", path = path.display())]
    NonUtf8PlaylistFile { path: PathBuf },

    #[error("{path} names no file a playlist could be written to", path = path.display())]
    NotAPlaylistFile { path: PathBuf },

    #[error("{location} is not a local file, and only the local source has a catalog here")]
    NotALocalFile { location: Box<MediaLocation> },

    #[error("the naming layout holds {fault:?} at byte {at}")]
    LayoutSyntax { at: usize, fault: LayoutFault },

    #[error("the naming layout asks for {field}, which names nothing a track is known by")]
    UnknownLayoutField { field: FieldName },

    #[error("part {segment} of the naming layout steps out of the folder it is written under")]
    LayoutEscapes { segment: usize },

    #[error("track {track} stores {column:?} code {code}, which this build does not know")]
    UnknownEncoding {
        track: TrackId,
        column: EncodedColumn,
        code: i64,
    },

    #[error("a stored {column:?} code {code} is one this build does not know")]
    UnknownLinkCode { column: EncodedColumn, code: i64 },

    #[error("album {album} stores image format code {code}, which this build does not know")]
    UnknownImageFormat { album: AlbumId, code: i64 },

    #[error("the cover art stored for album {album} is in no image format this build can name")]
    UntypedCoverArt { album: AlbumId },

    #[error("album {album} stores cover source code {code}, which this build does not know")]
    UnknownCoverSource { album: AlbumId, code: i64 },

    #[error("the vault holds an object in form {code}, which this build does not know")]
    UnknownVaultForm { code: i64 },

    #[error("vault object {key} stores {column:?} code {code}, which this build does not know")]
    UnknownVaultEncoding {
        key: VaultKey,
        column: EncodedColumn,
        code: i64,
    },

    #[error("{named} is not a vault key")]
    NotAVaultKey { named: Box<str> },

    #[error("no vault is open, so nothing can be kept in one")]
    NoVault,

    #[error("the vault refused {path}", path = path.display())]
    Vault {
        path: PathBuf,
        #[source]
        source: Box<resonate_vault::Error>,
    },

    #[error("artist {artist} stores image format code {code}, which this build does not know")]
    UnknownPortraitFormat { artist: ArtistId, code: i64 },

    #[error("the portrait stored for artist {artist} is in no image format this build can name")]
    UntypedPortrait { artist: ArtistId },

    #[error("database operation {op:?} failed")]
    Store {
        op: StoreOp,
        #[source]
        source: Box<rusqlite::Error>,
    },

    #[error("catalog was written to schema {found}, this build writes {expected}")]
    SchemaMismatch {
        found: SchemaFingerprint,
        expected: SchemaFingerprint,
    },

    #[error(transparent)]
    Domain(#[from] resonate_core::Error),
}

impl Error {
    pub fn store(op: StoreOp, source: rusqlite::Error) -> Self {
        Self::Store {
            op,
            source: Box::new(source),
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_travels_to_whichever_thread_asked() {
        const fn carried<T: Send + Sync>() {}
        carried::<Error>();
    }

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
