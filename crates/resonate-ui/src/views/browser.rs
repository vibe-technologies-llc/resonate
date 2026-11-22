mod albums;
mod artists;
mod grid;
mod heading;
mod menus;
mod missing_albums;
mod missing_rows;
mod ordering;
mod pressings;
mod record;
mod rows;
mod shelf;
#[cfg(test)]
mod tests;
mod tracks;

use self::rows::CONTROL_GAP;
pub(crate) use self::{
    artists::portrait_frame,
    heading::{ArtistShows, year_of},
    missing_rows::{Unheld, fetching_colour},
    record::OpenedRecord,
    rows::{controls_place, cursor_ring, reached_ring},
};
use crate::{
    theme,
    views::{
        menu::AtTheReach,
        reorder::{Listed, Shift},
    },
};

pub(crate) const ROW_CONTROLS: usize = 4;

pub(crate) const TRACK_CONTROLS: usize = 3;

pub(crate) const TRACK_ADD_CONTROLS: usize = 2;

pub(crate) fn controls_width(controls: usize) -> f32 {
    theme::row_control() * controls as f32 + CONTROL_GAP * controls.saturating_sub(1) as f32
}

pub(crate) const SHELF_AT_MOST: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rowed {
    pub(crate) playing: bool,
    pub(crate) plays: Plays,
    pub(crate) reach: Option<AtTheReach>,
    pub(crate) chooses: Option<Chooses>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chooses {
    pub(crate) shift: Shift,
    pub(crate) row: usize,
}

impl Chooses {
    pub(crate) const fn among(listed: Listed, row: usize) -> Self {
        Self {
            shift: Shift::Listing(listed),
            row,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Plays {
    TheseRows,
    AsTheListingIsDrawn { in_an_album: bool },
}
