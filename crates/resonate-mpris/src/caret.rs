use std::time::Duration;

use zbus::{
    blocking::{Connection, connection},
    zvariant::OwnedValue,
};

use crate::{BusOp, Error, Result};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const READ_ONE: &str = "ReadOne";
const GNOME_INTERFACE: &str = "org.gnome.desktop.interface";
const GNOME_BLINKS: &str = "cursor-blink";
const GNOME_BLINK_TIME: &str = "cursor-blink-time";
const KDE_GLOBALS: &str = "org.kde.kdeglobals.KDE";
const KDE_BLINK_RATE: &str = "CursorBlinkRate";
const ANSWERS_WITHIN: Duration = Duration::from_secs(1);
const HALVES_A_CYCLE: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blinking {
    Steady,
    Every(Duration),
    Unsaid,
}

impl Blinking {
    fn of_a_cycle(millis: u64) -> Self {
        match millis {
            0 => Self::Steady,
            millis => Self::Every(Duration::from_millis(millis / HALVES_A_CYCLE)),
        }
    }
}

pub fn caret_blinking() -> Blinking {
    match asked() {
        Ok(blinking) => blinking,
        Err(error) => {
            tracing::debug!(%error, "the desktop was not asked how its caret blinks");
            Blinking::Unsaid
        }
    }
}

fn asked() -> Result<Blinking> {
    let connection = connection::Builder::session()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .method_timeout(ANSWERS_WITHIN)
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))?;

    if let Some(blinks) = read(&connection, GNOME_INTERFACE, GNOME_BLINKS)
        .and_then(|value| bool::try_from(value).ok())
    {
        if !blinks {
            return Ok(Blinking::Steady);
        }
        return Ok(read(&connection, GNOME_INTERFACE, GNOME_BLINK_TIME)
            .and_then(whole_millis)
            .map_or(Blinking::Unsaid, Blinking::of_a_cycle));
    }

    Ok(read(&connection, KDE_GLOBALS, KDE_BLINK_RATE)
        .and_then(whole_millis)
        .map_or(Blinking::Unsaid, Blinking::of_a_cycle))
}

fn read(connection: &Connection, namespace: &str, key: &str) -> Option<OwnedValue> {
    connection
        .call_method(
            Some(PORTAL),
            OBJECT_PATH,
            Some(SETTINGS),
            READ_ONE,
            &(namespace, key),
        )
        .and_then(|reply| reply.body().deserialize::<OwnedValue>())
        .map_err(|error| {
            tracing::debug!(%error, namespace, key, "the desktop did not answer for a setting");
        })
        .ok()
}

fn whole_millis(value: OwnedValue) -> Option<u64> {
    if let Ok(millis) = i32::try_from(&value) {
        return u64::try_from(millis).ok();
    }
    if let Ok(millis) = u32::try_from(&value) {
        return Some(u64::from(millis));
    }
    String::try_from(value)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_of_nothing_is_a_caret_that_never_blinks_and_any_other_is_halved() {
        assert_eq!(Blinking::of_a_cycle(0), Blinking::Steady);
        assert_eq!(
            Blinking::of_a_cycle(1_200),
            Blinking::Every(Duration::from_millis(600))
        );
    }

    #[test]
    fn a_rate_is_read_whichever_way_the_desktop_types_it() {
        assert_eq!(whole_millis(OwnedValue::from(1_000_i32)), Some(1_000));
        assert_eq!(whole_millis(OwnedValue::from(1_000_u32)), Some(1_000));
        assert_eq!(
            whole_millis(
                OwnedValue::try_from(zbus::zvariant::Value::from("1000")).expect("a string")
            ),
            Some(1_000)
        );
        assert_eq!(whole_millis(OwnedValue::from(-1_i32)), None);
    }
}
