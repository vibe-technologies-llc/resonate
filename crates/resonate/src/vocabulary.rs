use crate::cli::{
    DitherArg, FilterPhaseArg, NoiseShapingArg, PlaylistOrderArg, QualityArg, RepeatArg,
    RestorationArg, RowOrderArg, SortArg, UpkeepArg, WindowArg,
};

pub trait Meaning<Meant> {
    fn meaning(self) -> Meant;
}

impl Meaning<resonate_engine::RepeatMode> for RepeatArg {
    fn meaning(self) -> resonate_engine::RepeatMode {
        match self {
            Self::Off => resonate_engine::RepeatMode::Off,
            Self::Track => resonate_engine::RepeatMode::Track,
            Self::Queue => resonate_engine::RepeatMode::Queue,
        }
    }
}

impl Meaning<resonate_engine::FilterPhase> for FilterPhaseArg {
    fn meaning(self) -> resonate_engine::FilterPhase {
        match self {
            Self::Linear => resonate_engine::FilterPhase::Linear,
            Self::Intermediate => resonate_engine::FilterPhase::Intermediate,
            Self::Minimum => resonate_engine::FilterPhase::Minimum,
        }
    }
}

impl Meaning<FilterPhaseArg> for resonate_engine::FilterPhase {
    fn meaning(self) -> FilterPhaseArg {
        match self {
            Self::Linear => FilterPhaseArg::Linear,
            Self::Intermediate => FilterPhaseArg::Intermediate,
            Self::Minimum => FilterPhaseArg::Minimum,
        }
    }
}

impl Meaning<resonate_engine::Quality> for QualityArg {
    fn meaning(self) -> resonate_engine::Quality {
        match self {
            Self::LowLatency => resonate_engine::Quality::LowLatency,
            Self::Balanced => resonate_engine::Quality::Balanced,
            Self::High => resonate_engine::Quality::High,
            Self::VeryHigh => resonate_engine::Quality::VeryHigh,
        }
    }
}

impl Meaning<resonate_engine::Restoration> for RestorationArg {
    fn meaning(self) -> resonate_engine::Restoration {
        match self {
            Self::Off => resonate_engine::Restoration::Off,
            Self::Repair => resonate_engine::Restoration::Repair,
            Self::Extend => resonate_engine::Restoration::Extend,
        }
    }
}

impl Meaning<resonate_engine::DitherKind> for DitherArg {
    fn meaning(self) -> resonate_engine::DitherKind {
        match self {
            Self::None => resonate_engine::DitherKind::None,
            Self::Rectangular => resonate_engine::DitherKind::Rectangular,
            Self::Triangular => resonate_engine::DitherKind::Triangular,
        }
    }
}

impl Meaning<resonate_engine::NoiseShaping> for NoiseShapingArg {
    fn meaning(self) -> resonate_engine::NoiseShaping {
        match self {
            Self::Flat => resonate_engine::NoiseShaping::None,
            Self::Lipshitz => resonate_engine::NoiseShaping::Lipshitz,
            Self::Threshold => resonate_engine::NoiseShaping::Threshold,
        }
    }
}

impl Meaning<QualityArg> for resonate_engine::Quality {
    fn meaning(self) -> QualityArg {
        match self {
            Self::LowLatency => QualityArg::LowLatency,
            Self::Balanced => QualityArg::Balanced,
            Self::High => QualityArg::High,
            Self::VeryHigh => QualityArg::VeryHigh,
        }
    }
}

impl Meaning<DitherArg> for resonate_engine::DitherKind {
    fn meaning(self) -> DitherArg {
        match self {
            Self::None => DitherArg::None,
            Self::Rectangular => DitherArg::Rectangular,
            Self::Triangular => DitherArg::Triangular,
        }
    }
}

impl Meaning<NoiseShapingArg> for resonate_engine::NoiseShaping {
    fn meaning(self) -> NoiseShapingArg {
        match self {
            Self::None => NoiseShapingArg::Flat,
            Self::Lipshitz => NoiseShapingArg::Lipshitz,
            Self::Threshold => NoiseShapingArg::Threshold,
        }
    }
}

impl Meaning<resonate_library::SortOrder> for SortArg {
    fn meaning(self) -> resonate_library::SortOrder {
        match self {
            Self::Relevance => resonate_library::SortOrder::Relevance,
            Self::AlbumThenTrack => resonate_library::SortOrder::AlbumThenTrack,
            Self::Title => resonate_library::SortOrder::Title,
            Self::Artist => resonate_library::SortOrder::Artist,
            Self::DateAdded => resonate_library::SortOrder::DateAdded,
            Self::Duration => resonate_library::SortOrder::Duration,
            Self::Plays => resonate_library::SortOrder::Plays,
            Self::Played => resonate_library::SortOrder::Played,
            Self::PlaysThisMonth => resonate_library::SortOrder::PlaysThisMonth,
        }
    }
}

impl Meaning<resonate_library::RowOrder> for RowOrderArg {
    fn meaning(self) -> resonate_library::RowOrder {
        match self {
            Self::Album => resonate_library::RowOrder::Album,
            Self::Artist => resonate_library::RowOrder::Artist,
            Self::Title => resonate_library::RowOrder::Title,
            Self::Length => resonate_library::RowOrder::Length,
            Self::File => resonate_library::RowOrder::File,
        }
    }
}

impl Meaning<resonate_library::PlaylistOrder> for PlaylistOrderArg {
    fn meaning(self) -> resonate_library::PlaylistOrder {
        match self {
            Self::Name => resonate_library::PlaylistOrder::Name,
            Self::Created => resonate_library::PlaylistOrder::Created,
            Self::Modified => resonate_library::PlaylistOrder::Modified,
            Self::Played => resonate_library::PlaylistOrder::Played,
            Self::Plays => resonate_library::PlaylistOrder::Plays,
            Self::PlaysThisMonth => resonate_library::PlaylistOrder::PlaysThisMonth,
        }
    }
}

impl Meaning<resonate_library::Window> for WindowArg {
    fn meaning(self) -> resonate_library::Window {
        match self {
            Self::Week => resonate_library::Window::Week,
            Self::Month => resonate_library::Window::Month,
            Self::Year => resonate_library::Window::Year,
            Self::All => resonate_library::Window::Everything,
        }
    }
}

impl Meaning<resonate_library::Upkeep> for UpkeepArg {
    fn meaning(self) -> resonate_library::Upkeep {
        match self {
            Self::Once => resonate_library::Upkeep::Once,
            Self::InStep => resonate_library::Upkeep::InStep,
        }
    }
}
