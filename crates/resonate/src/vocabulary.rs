use crate::cli::{
    DitherArg, FilterPhaseArg, NoiseShapingArg, PlaylistOrderArg, QualityArg, RowOrderArg, SortArg,
    WindowArg,
};

impl From<FilterPhaseArg> for resonate_engine::FilterPhase {
    fn from(phase: FilterPhaseArg) -> Self {
        match phase {
            FilterPhaseArg::Linear => Self::Linear,
            FilterPhaseArg::Intermediate => Self::Intermediate,
            FilterPhaseArg::Minimum => Self::Minimum,
        }
    }
}

impl From<resonate_engine::FilterPhase> for FilterPhaseArg {
    fn from(phase: resonate_engine::FilterPhase) -> Self {
        match phase {
            resonate_engine::FilterPhase::Linear => Self::Linear,
            resonate_engine::FilterPhase::Intermediate => Self::Intermediate,
            resonate_engine::FilterPhase::Minimum => Self::Minimum,
        }
    }
}

impl From<QualityArg> for resonate_engine::Quality {
    fn from(quality: QualityArg) -> Self {
        match quality {
            QualityArg::Fast => Self::Fast,
            QualityArg::Balanced => Self::Balanced,
            QualityArg::High => Self::High,
            QualityArg::VeryHigh => Self::VeryHigh,
        }
    }
}

impl From<DitherArg> for resonate_engine::DitherKind {
    fn from(dither: DitherArg) -> Self {
        match dither {
            DitherArg::None => Self::None,
            DitherArg::Rectangular => Self::Rectangular,
            DitherArg::Triangular => Self::Triangular,
        }
    }
}

impl From<NoiseShapingArg> for resonate_engine::NoiseShaping {
    fn from(shaping: NoiseShapingArg) -> Self {
        match shaping {
            NoiseShapingArg::Flat => Self::None,
            NoiseShapingArg::Lipshitz => Self::Lipshitz,
            NoiseShapingArg::Threshold => Self::Threshold,
        }
    }
}

impl From<resonate_engine::Quality> for QualityArg {
    fn from(quality: resonate_engine::Quality) -> Self {
        match quality {
            resonate_engine::Quality::Fast => Self::Fast,
            resonate_engine::Quality::Balanced => Self::Balanced,
            resonate_engine::Quality::High => Self::High,
            resonate_engine::Quality::VeryHigh => Self::VeryHigh,
        }
    }
}

impl From<resonate_engine::DitherKind> for DitherArg {
    fn from(dither: resonate_engine::DitherKind) -> Self {
        match dither {
            resonate_engine::DitherKind::None => Self::None,
            resonate_engine::DitherKind::Rectangular => Self::Rectangular,
            resonate_engine::DitherKind::Triangular => Self::Triangular,
        }
    }
}

impl From<resonate_engine::NoiseShaping> for NoiseShapingArg {
    fn from(shaping: resonate_engine::NoiseShaping) -> Self {
        match shaping {
            resonate_engine::NoiseShaping::None => Self::Flat,
            resonate_engine::NoiseShaping::Lipshitz => Self::Lipshitz,
            resonate_engine::NoiseShaping::Threshold => Self::Threshold,
        }
    }
}

impl From<SortArg> for resonate_library::SortOrder {
    fn from(sort: SortArg) -> Self {
        match sort {
            SortArg::Relevance => Self::Relevance,
            SortArg::AlbumThenTrack => Self::AlbumThenTrack,
            SortArg::Title => Self::Title,
            SortArg::Artist => Self::Artist,
            SortArg::DateAdded => Self::DateAdded,
            SortArg::Duration => Self::Duration,
            SortArg::Plays => Self::Plays,
            SortArg::Played => Self::Played,
        }
    }
}

impl From<RowOrderArg> for resonate_library::RowOrder {
    fn from(order: RowOrderArg) -> Self {
        match order {
            RowOrderArg::Album => Self::Album,
            RowOrderArg::Artist => Self::Artist,
            RowOrderArg::Title => Self::Title,
            RowOrderArg::Length => Self::Length,
            RowOrderArg::File => Self::File,
        }
    }
}

impl From<PlaylistOrderArg> for resonate_library::PlaylistOrder {
    fn from(order: PlaylistOrderArg) -> Self {
        match order {
            PlaylistOrderArg::Name => Self::Name,
            PlaylistOrderArg::Created => Self::Created,
            PlaylistOrderArg::Modified => Self::Modified,
            PlaylistOrderArg::Played => Self::Played,
            PlaylistOrderArg::Plays => Self::Plays,
        }
    }
}

impl From<WindowArg> for resonate_library::Window {
    fn from(window: WindowArg) -> Self {
        match window {
            WindowArg::Week => Self::Week,
            WindowArg::Month => Self::Month,
            WindowArg::Year => Self::Year,
            WindowArg::All => Self::Everything,
        }
    }
}
