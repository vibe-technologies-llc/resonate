mod answering;
#[cfg(windows)]
mod apartment;
mod art;
mod blocking;
mod bluez;
mod bus_peer;
mod caret;
#[cfg(any(windows, test))]
mod cost;
#[cfg(windows)]
mod costing;
#[cfg(unix)]
mod desk_bus;
#[cfg(windows)]
mod desk_toast;
mod desktop;
mod error;
#[cfg(target_os = "linux")]
mod gaming;
mod host;
mod icons;
mod identity;
#[cfg(unix)]
mod idle;
mod interfaces;
#[cfg(unix)]
mod metered;
#[cfg(unix)]
mod notifier;
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
#[cfg(windows)]
mod smtc;
#[cfg(any(windows, test))]
mod toast;
mod track;
mod tracklist;
#[cfg(any(windows, test))]
mod transport;
#[cfg(unix)]
mod trash;
mod wire;

#[cfg(target_os = "linux")]
pub use gaming::watch_game_mode;

#[cfg(windows)]
pub use crate::costing::watch_the_meter;
#[cfg(windows)]
pub use crate::error::WindowsOp;
#[cfg(unix)]
pub use crate::metered::{meter_of_network_manager, watch_the_meter};
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
    identity::APP_USER_MODEL_ID,
    preferences::{Contrast, Motion, Preferences, Scheme, desktop_preferences, watch_the_desktop},
    remote::{
        Described, PlayerName, Queueing, ROWS_A_CALL, Running, Seeking, Session, Standing,
        ZoneAsked,
    },
    service::{Mpris, Teller, Told},
    track::{PlaybackStatus, utc_stamp},
    wire::{Call, Fault, WireFault},
};
