use std::time::SystemTime;

use resonate_core::{FrameSpan, MediaLocation, PlaylistId, SourceId, TrackId, ZoneId, ZoneName};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opened {
    Accepted,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tuned {
    Applied,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reloaded {
    Applied,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reloadable {
    Equaliser,
    Submission,
    Sync,
    Tidal,
}

impl Reloadable {
    pub const ALL: [Self; 4] = [Self::Equaliser, Self::Submission, Self::Sync, Self::Tidal];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Equaliser => "equaliser",
            Self::Submission => "submission",
            Self::Sync => "sync",
            Self::Tidal => "tidal",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tunable {
    Sink,
    Quality,
    FilterPhase,
    Dither,
    NoiseShaping,
    BitPerfect,
}

impl Tunable {
    pub const ALL: [Self; 6] = [
        Self::Sink,
        Self::Quality,
        Self::FilterPhase,
        Self::Dither,
        Self::NoiseShaping,
        Self::BitPerfect,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Sink => "sink",
            Self::Quality => "quality",
            Self::FilterPhase => "filter-phase",
            Self::Dither => "dither",
            Self::NoiseShaping => "noise-shaping",
            Self::BitPerfect => "bit-perfect",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.name() == name)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Heard {
    pub plays: u32,
    pub played: Option<SystemTime>,
    pub favourite: bool,
}

pub trait Host: Send + Sync + 'static {
    fn identity(&self) -> String;

    fn zone(&self) -> ZoneName {
        ZoneName::first_given_to(ZoneId::FIRST)
    }

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

    fn answers_headsets(&self) -> bool {
        false
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

    fn held_as(&self, _location: &MediaLocation, _span: Option<FrameSpan>) -> Option<TrackId> {
        None
    }

    fn tune(&self, _setting: Tunable, _value: &str) -> Tuned {
        Tuned::Refused
    }

    fn reload(&self, _group: Reloadable, _settings: &str) -> Reloaded {
        Reloaded::Refused
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
