use std::{mem::ManuallyDrop, thread, time::Duration};

use zbus::{OwnedVariant, proxy::SignalStream};

use crate::{
    BusOp, Error, Result,
    blocking::{Arrivals, Connection, Proxy, connection},
    caret::{GNOME_INTERFACE, OBJECT_PATH, PORTAL, SETTINGS, read},
};

const APPEARANCE: &str = "org.freedesktop.appearance";
const CONTRAST: &str = "contrast";
const REDUCED_MOTION: &str = "reduced-motion";
const COLOUR_SCHEME: &str = "color-scheme";
const GNOME_ANIMATES: &str = "enable-animations";
const GNOME_TEXT_SCALE: &str = "text-scaling-factor";
const KDE_GENERAL: &str = "org.kde.kdeglobals.General";
const KDE_FONT: &str = "font";
const SETTING_CHANGED: &str = "SettingChanged";
const KDE_FONT_SIZE_FIELD: usize = 1;
const KDE_ORDINARY_POINTS: f64 = 10.0;
const MORE_CONTRAST: u32 = 1;
const REDUCED: u32 = 1;
const PREFERS_DARK: u32 = 1;
const PREFERS_LIGHT: u32 = 2;
const ANSWERS_WITHIN: Duration = Duration::from_millis(300);
const WATCHED: [&str; 3] = [APPEARANCE, GNOME_INTERFACE, KDE_GENERAL];

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
pub enum Scheme {
    Dark,
    Light,
    Unsaid,
}

impl Scheme {
    const fn of_the_portal(value: u32) -> Self {
        match value {
            PREFERS_DARK => Self::Dark,
            PREFERS_LIGHT => Self::Light,
            _ => Self::Unsaid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preferences {
    pub contrast: Contrast,
    pub motion: Motion,
    pub scheme: Scheme,
    pub text_scale: Option<f32>,
}

impl Preferences {
    pub const UNSAID: Self = Self {
        contrast: Contrast::Unsaid,
        motion: Motion::Unsaid,
        scheme: Scheme::Unsaid,
        text_scale: None,
    };
}

pub fn desktop_preferences() -> Preferences {
    match connected().map(|connection| ask(&connection)) {
        Ok(preferences) => preferences,
        Err(error) => {
            tracing::debug!(%error, "the desktop was not asked for its preferences");
            Preferences::UNSAID
        }
    }
}

pub fn watch_the_desktop(told: impl Fn(Preferences) + Send + 'static) -> Result<()> {
    let connection = connected()?;
    let proxy = Proxy::new(&connection, PORTAL, OBJECT_PATH, SETTINGS)
        .map_err(|source| Error::bus(BusOp::Connect, source))?;
    let changes = proxy
        .receive_signal(SETTING_CHANGED)
        .map_err(|source| Error::bus(BusOp::Listen, source))?;
    let first = ask(&connection);

    thread::Builder::new()
        .name("resonate-desktop".to_owned())
        .spawn(move || keep_watching(&connection, changes, first, &told))
        .map(drop)
        .map_err(|_| Error::ThreadStopped)
}

fn keep_watching(
    connection: &Connection,
    changes: Arrivals<SignalStream<'static>>,
    mut standing: Preferences,
    told: &impl Fn(Preferences),
) {
    let mut changes = ManuallyDrop::new(changes);

    for change in changes.by_ref() {
        let body = change.body();
        let Ok((namespace, _, _)) = body.deserialize::<(String, String, OwnedVariant)>() else {
            continue;
        };
        if !WATCHED.contains(&namespace.as_str()) {
            continue;
        }

        let now = ask(connection);
        if now != standing {
            standing = now;
            told(now);
        }
    }
}

fn connected() -> Result<Connection> {
    connection::Builder::session()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .method_timeout(ANSWERS_WITHIN)
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))
}

fn ask(connection: &Connection) -> Preferences {
    let contrast = read(connection, APPEARANCE, CONTRAST)
        .and_then(|value| u32::try_from(&value).ok())
        .map_or(Contrast::Unsaid, Contrast::of_the_portal);

    let motion = read(connection, APPEARANCE, REDUCED_MOTION)
        .and_then(|value| u32::try_from(&value).ok())
        .map(Motion::of_the_portal)
        .or_else(|| {
            read(connection, GNOME_INTERFACE, GNOME_ANIMATES)
                .and_then(|value| bool::try_from(value).ok())
                .map(Motion::of_animations)
        })
        .unwrap_or(Motion::Unsaid);

    let scheme = read(connection, APPEARANCE, COLOUR_SCHEME)
        .and_then(|value| u32::try_from(&value).ok())
        .map_or(Scheme::Unsaid, Scheme::of_the_portal);

    let text_scale = read(connection, GNOME_INTERFACE, GNOME_TEXT_SCALE)
        .and_then(|value| f64::try_from(value).ok())
        .or_else(|| {
            read(connection, KDE_GENERAL, KDE_FONT)
                .and_then(|value| String::try_from(value).ok())
                .and_then(|font| scale_of_a_kde_font(&font))
        })
        .and_then(sane_scale);

    Preferences {
        contrast,
        motion,
        scheme,
        text_scale,
    }
}

fn scale_of_a_kde_font(font: &str) -> Option<f64> {
    font.split(',')
        .nth(KDE_FONT_SIZE_FIELD)
        .and_then(|points| points.trim().parse::<f64>().ok())
        .map(|points| points / KDE_ORDINARY_POINTS)
}

fn sane_scale(scale: f64) -> Option<f32> {
    (scale.is_finite() && scale > 0.0).then_some(scale as f32)
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

    #[test]
    fn the_portal_prefers_dark_with_a_one_light_with_a_two_and_says_nothing_otherwise() {
        assert_eq!(Scheme::of_the_portal(1), Scheme::Dark);
        assert_eq!(Scheme::of_the_portal(2), Scheme::Light);
        assert_eq!(Scheme::of_the_portal(0), Scheme::Unsaid);
        assert_eq!(Scheme::of_the_portal(7), Scheme::Unsaid);
    }

    #[test]
    fn a_kde_font_is_scaled_by_its_point_size_against_the_ordinary_ten() {
        assert_eq!(
            scale_of_a_kde_font("Noto Sans,10,-1,5,400,0,0,0,0,0"),
            Some(1.0)
        );
        assert_eq!(
            scale_of_a_kde_font("Noto Sans,12,-1,5,400,0,0,0,0,0"),
            Some(1.2)
        );
        assert_eq!(scale_of_a_kde_font("Noto Sans"), None);
        assert_eq!(scale_of_a_kde_font("Noto Sans,big"), None);
    }

    #[test]
    fn a_scale_that_is_not_a_positive_number_is_not_one() {
        assert_eq!(sane_scale(1.25), Some(1.25));
        assert_eq!(sane_scale(0.0), None);
        assert_eq!(sane_scale(-1.0), None);
        assert_eq!(sane_scale(f64::NAN), None);
    }
}
