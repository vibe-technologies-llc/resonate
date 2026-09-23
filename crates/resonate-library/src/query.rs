use resonate_core::{AlbumId, ArtistId};

use crate::{Album, Artist, Track};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortOrder {
    #[default]
    Relevance,
    AlbumThenTrack,
    Title,
    Artist,
    DateAdded,
    Duration,
    Plays,
    Played,
    Favourited,
}

impl SortOrder {
    pub const ALL: [Self; 9] = [
        Self::Relevance,
        Self::AlbumThenTrack,
        Self::Title,
        Self::Artist,
        Self::DateAdded,
        Self::Duration,
        Self::Plays,
        Self::Played,
        Self::Favourited,
    ];

    pub const fn reads(self) -> Direction {
        match self {
            Self::Relevance
            | Self::AlbumThenTrack
            | Self::Title
            | Self::Artist
            | Self::Duration => Direction::Ascending,
            Self::DateAdded | Self::Plays | Self::Played | Self::Favourited => {
                Direction::Descending
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AlbumOrder {
    #[default]
    Relevance,
    Title,
    Artist,
    Year,
    Tracks,
    Added,
    Favourited,
}

impl AlbumOrder {
    pub const ALL: [Self; 7] = [
        Self::Relevance,
        Self::Title,
        Self::Artist,
        Self::Year,
        Self::Tracks,
        Self::Added,
        Self::Favourited,
    ];

    pub const fn reads(self) -> Direction {
        match self {
            Self::Relevance | Self::Title | Self::Artist => Direction::Ascending,
            Self::Year | Self::Tracks | Self::Added | Self::Favourited => Direction::Descending,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArtistOrder {
    #[default]
    Relevance,
    Name,
    Albums,
    Tracks,
    Favourited,
}

impl ArtistOrder {
    pub const ALL: [Self; 5] = [
        Self::Relevance,
        Self::Name,
        Self::Albums,
        Self::Tracks,
        Self::Favourited,
    ];

    pub const fn reads(self) -> Direction {
        match self {
            Self::Relevance | Self::Name => Direction::Ascending,
            Self::Albums | Self::Tracks | Self::Favourited => Direction::Descending,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SavedQuery {
    pub text: Option<String>,
    pub sort: SortOrder,
    pub reading: Direction,
    pub limit: Option<usize>,
}

impl From<&SavedQuery> for TrackQuery {
    fn from(saved: &SavedQuery) -> Self {
        Self {
            album: None,
            artist: None,
            text: saved.text.clone(),
            sort: saved.sort,
            reading: saved.reading,
            limit: saved.limit,
            offset: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistOrder {
    #[default]
    Name,
    Created,
    Modified,
    Played,
    Plays,
}

impl PlaylistOrder {
    pub const ALL: [Self; 5] = [
        Self::Name,
        Self::Created,
        Self::Modified,
        Self::Played,
        Self::Plays,
    ];

    pub const fn reads(self) -> Direction {
        match self {
            Self::Name => Direction::Ascending,
            Self::Created | Self::Modified | Self::Played | Self::Plays => Direction::Descending,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RowOrder {
    #[default]
    Album,
    Artist,
    Title,
    Length,
    File,
}

impl RowOrder {
    pub const ALL: [Self; 5] = [
        Self::Album,
        Self::Artist,
        Self::Title,
        Self::Length,
        Self::File,
    ];

    pub const fn reads_the_catalog(self) -> bool {
        !matches!(self, Self::File)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Kept {
    pub order: RowOrder,
    pub reading: Direction,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Ascending,
    Descending,
}

impl Direction {
    pub const ALL: [Self; 2] = [Self::Ascending, Self::Descending];

    pub const fn flipped(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackQuery {
    pub album: Option<AlbumId>,
    pub artist: Option<ArtistId>,
    pub text: Option<String>,
    pub sort: SortOrder,
    pub reading: Direction,
    pub limit: Option<usize>,
    pub offset: usize,
}

impl Default for TrackQuery {
    fn default() -> Self {
        let sort = SortOrder::default();

        Self {
            album: None,
            artist: None,
            text: None,
            sort,
            reading: sort.reads(),
            limit: None,
            offset: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistQuery {
    pub text: Option<String>,
    pub sort: ArtistOrder,
    pub reading: Direction,
    pub limit: Option<usize>,
    pub offset: usize,
}

impl Default for ArtistQuery {
    fn default() -> Self {
        let sort = ArtistOrder::default();

        Self {
            text: None,
            sort,
            reading: sort.reads(),
            limit: None,
            offset: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumQuery {
    pub artist: Option<ArtistId>,
    pub text: Option<String>,
    pub sort: AlbumOrder,
    pub reading: Direction,
    pub limit: Option<usize>,
    pub offset: usize,
}

impl Default for AlbumQuery {
    fn default() -> Self {
        let sort = AlbumOrder::default();

        Self {
            artist: None,
            text: None,
            sort,
            reading: sort.reads(),
            limit: None,
            offset: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}
