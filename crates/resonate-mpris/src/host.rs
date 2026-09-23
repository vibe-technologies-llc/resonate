use std::time::SystemTime;

use resonate_core::{FrameSpan, MediaLocation, PlaylistId, SourceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opened {
    Accepted,
    Refused,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Heard {
    pub plays: u32,
    pub played: Option<SystemTime>,
}

pub trait Host: Send + Sync + 'static {
    fn identity(&self) -> String;

    fn desktop_entry(&self) -> Option<String> {
        None
    }

    fn can_raise(&self) -> bool {
        false
    }

    fn can_quit(&self) -> bool {
        false
    }

    fn attended(&self) -> bool {
        false
    }

    fn notifies(&self) -> bool {
        true
    }

    fn raise(&self) {}

    fn quit(&self) {}

    fn sources(&self) -> Vec<SourceId> {
        vec![SourceId::local()]
    }

    fn mime_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(&self, _location: &MediaLocation, _span: Option<FrameSpan>) -> Opened {
        Opened::Refused
    }

    fn heard(&self, _location: &MediaLocation, _span: Option<FrameSpan>) -> Option<Heard> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaylistInfo {
    pub id: PlaylistId,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistOrder {
    #[default]
    Alphabetical,
    CreationDate,
    ModifiedDate,
    LastPlayDate,
}

impl PlaylistOrder {
    pub const ALL: [Self; 4] = [
        Self::Alphabetical,
        Self::CreationDate,
        Self::ModifiedDate,
        Self::LastPlayDate,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Alphabetical => "Alphabetical",
            Self::CreationDate => "CreationDate",
            Self::ModifiedDate => "ModifiedDate",
            Self::LastPlayDate => "LastPlayDate",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|order| order.name() == name)
    }
}

pub trait Playlists: Send + Sync + 'static {
    fn revision(&self) -> u64;

    fn count(&self) -> usize;

    fn listing(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: usize,
        most: Option<usize>,
    ) -> Vec<PlaylistInfo>;

    fn playing(&self) -> Option<PlaylistInfo>;

    fn activate(&self, playlist: PlaylistId) -> Opened;
}
