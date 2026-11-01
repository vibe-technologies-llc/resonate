mod art;
mod blocking;
mod bluez;
mod caret;
mod desktop;
mod error;
mod host;
mod icons;
mod idle;
mod interfaces;
mod metered;
mod notify;
mod playlists;
mod preferences;
mod remote;
mod service;
mod track;
mod tracklist;
#[cfg(unix)]
mod trash;

#[cfg(unix)]
pub use crate::trash::{Trashing, trash_through_the_portal};
pub use crate::{
    caret::{Blinking, caret_blinking},
    error::{BusOp, Error, Result},
    host::{
        HandedOver, Heard, Host, Opened, PlaylistInfo, PlaylistOrder, Playlists, Reloadable,
        Reloaded, Tunable, Tuned,
    },
    icons::tell_the_icons_changed,
    metered::{meter_of_network_manager, watch_the_meter},
    preferences::{Contrast, Motion, Preferences, Scheme, desktop_preferences, watch_the_desktop},
    remote::{
        Described, PlayerName, Queueing, ROWS_A_CALL, Running, Seeking, Session, Standing,
        ZoneAsked,
    },
    service::{Mpris, Teller, Told},
    track::{PlaybackStatus, utc_stamp},
};
