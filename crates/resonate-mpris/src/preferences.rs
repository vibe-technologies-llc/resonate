use std::time::Duration;

use crate::{
    BusOp, Error, Result,
    blocking::connection,
    caret::{GNOME_INTERFACE, read},
};

const APPEARANCE: &str = "org.freedesktop.appearance";
const CONTRAST: &str = "contrast";
const REDUCED_MOTION: &str = "reduced-motion";
const GNOME_ANIMATES: &str = "enable-animations";
const MORE_CONTRAST: u32 = 1;
const REDUCED: u32 = 1;
const ANSWERS_WITHIN: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contrast {
    More,
    Ordinary,
    Unsaid,
}

impl Contrast {
    const fn of_the_portal(value: u32) -> Self {
        if value == MORE_CONTRAST {
            Self::More
        } else {
            Self::Ordinary
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Reduced,
    Full,
    Unsaid,
}

impl Motion {
    const fn of_the_portal(value: u32) -> Self {
        if value == REDUCED {
            Self::Reduced
        } else {
            Self::Full
        }
    }

    const fn of_animations(animates: bool) -> Self {
        if animates { Self::Full } else { Self::Reduced }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub contrast: Contrast,
    pub motion: Motion,
}

impl Preferences {
    pub const UNSAID: Self = Self {
        contrast: Contrast::Unsaid,
        motion: Motion::Unsaid,
    };
}

pub fn desktop_preferences() -> Preferences {
    match asked() {
        Ok(preferences) => preferences,
        Err(error) => {
            tracing::debug!(%error, "the desktop was not asked for its contrast and motion");
            Preferences::UNSAID
        }
    }
}

fn asked() -> Result<Preferences> {
    let connection = connection::Builder::session()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .method_timeout(ANSWERS_WITHIN)
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))?;

    let contrast = read(&connection, APPEARANCE, CONTRAST)
        .and_then(|value| u32::try_from(&value).ok())
        .map_or(Contrast::Unsaid, Contrast::of_the_portal);

    let motion = read(&connection, APPEARANCE, REDUCED_MOTION)
        .and_then(|value| u32::try_from(&value).ok())
        .map(Motion::of_the_portal)
        .or_else(|| {
            read(&connection, GNOME_INTERFACE, GNOME_ANIMATES)
                .and_then(|value| bool::try_from(value).ok())
                .map(Motion::of_animations)
        })
        .unwrap_or(Motion::Unsaid);

    Ok(Preferences { contrast, motion })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_asks_for_more_contrast_with_a_one_and_for_none_with_anything_else() {
        assert_eq!(Contrast::of_the_portal(1), Contrast::More);
        assert_eq!(Contrast::of_the_portal(0), Contrast::Ordinary);
        assert_eq!(Contrast::of_the_portal(2), Contrast::Ordinary);
    }

    #[test]
    fn the_portal_asks_for_reduced_motion_with_a_one_and_the_toolkit_with_animations_off() {
        assert_eq!(Motion::of_the_portal(1), Motion::Reduced);
        assert_eq!(Motion::of_the_portal(0), Motion::Full);
        assert_eq!(Motion::of_animations(false), Motion::Reduced);
        assert_eq!(Motion::of_animations(true), Motion::Full);
    }
}
