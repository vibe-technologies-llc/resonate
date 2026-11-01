mod answering;
mod art;
mod blocking;
mod bluez;
mod bus_peer;
mod caret;
mod desktop;
mod error;
mod host;
mod icons;
mod idle;
mod interfaces;
mod metered;
mod notify;
mod peer;
#[cfg(windows)]
mod pipe;
mod playlists;
mod preferences;
#[cfg(any(windows, test))]
mod registry;
mod remote;
mod service;
mod track;
mod tracklist;
#[cfg(unix)]
mod trash;
mod wire;

#[cfg(unix)]
pub use crate::trash::{Trashing, trash_through_the_portal};
pub use crate::{
    answering::Answering,
    caret::{Blinking, caret_blinking},
    error::{BusOp, Error, PipeOp, Result},
    host::{
        HandedOver, Heard, Host, Opened, PlaylistInfo, PlaylistOrder, Playlists, Reloadable,
        Reloaded, Tunable, Tuned,
    },
    icons::tell_the_icons_changed,
    metered::{meter_of_network_manager, watch_the_meter},
    preferences::{Contrast, Motion, Preferences, desktop_preferences},
    remote::{
        Described, PlayerName, Queueing, ROWS_A_CALL, Running, Seeking, Session, Standing,
        ZoneAsked,
    },
    service::{Mpris, Teller, Told},
    track::{PlaybackStatus, utc_stamp},
    wire::{Call, Fault, WireFault},
};
