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
#[cfg(fuzzing)]
mod fuzzing;
#[cfg(target_os = "linux")]
mod gaming;
mod host;
mod icons;
mod identity;
#[cfg(unix)]
mod idle;
mod interfaces;
#[cfg(any(windows, target_os = "macos", test))]
mod listening;
#[cfg(all(unix, not(target_os = "macos")))]
mod metered;
#[cfg(unix)]
mod notifier;
mod notify;
#[cfg(target_os = "macos")]
mod nowplaying;
#[cfg(target_os = "macos")]
mod pathing;
mod peer;
#[cfg(windows)]
mod pipe;
mod playlists;
mod preferences;
#[cfg(any(windows, target_os = "macos", test))]
mod registry;
mod remote;
#[cfg(any(target_os = "macos", test))]
mod route;
mod service;
#[cfg(windows)]
mod smtc;
#[cfg(any(target_os = "macos", all(unix, test)))]
mod socket;
#[cfg(any(windows, test))]
mod toast;
mod track;
mod tracklist;
#[cfg(any(windows, target_os = "macos", test))]
mod transport;
#[cfg(all(unix, not(target_os = "macos")))]
mod trash;
mod wire;

#[cfg_attr(windows, allow(dead_code))]
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(target_os = "linux")]
pub use gaming::watch_game_mode;

#[cfg(windows)]
pub use crate::costing::watch_the_meter;
#[cfg(windows)]
pub use crate::error::WindowsOp;
#[cfg(fuzzing)]
pub use crate::fuzzing::read_what_a_peer_says;
#[cfg(all(unix, not(target_os = "macos")))]
pub use crate::metered::{meter_of_network_manager, watch_the_meter};
#[cfg(target_os = "macos")]
pub use crate::pathing::watch_the_meter;
#[cfg(all(unix, not(target_os = "macos")))]
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

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    const SAYS_WORDS: [&str; 2] = ["idle.rs", "notify.rs"];

    const PROPER: [&str; 5] = [
        "org.freedesktop.portal.Desktop",
        "org.freedesktop.portal.Inhibit",
        "org.freedesktop.portal.Request",
        "Inhibit",
        "Close",
    ];

    #[test]
    fn every_word_the_desktop_shows_for_the_player_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

        let written: Vec<String> = SAYS_WORDS
            .iter()
            .flat_map(|file| {
                let source = fs::read_to_string(root.join(file)).expect("a file that says words");
                resonate_words_build::prose_of(&source)
                    .into_iter()
                    .filter(|said| !PROPER.contains(&said.text.as_str()))
                    .map(move |said| format!("{file}:{} {:?}", said.line, said.text))
            })
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
