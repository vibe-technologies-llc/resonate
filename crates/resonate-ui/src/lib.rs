#[macro_use]
mod keys;
mod analysis;
mod analysis_plot;
mod app;
mod app_menu;
#[cfg(target_os = "linux")]
mod clipboard;
#[cfg(not(target_os = "linux"))]
#[path = "clipboard_gpui.rs"]
mod clipboard;
mod downloads;
mod drawing;
#[cfg(test)]
mod driven;
mod edit;
mod equaliser;
mod error;
mod fonts;
mod format;
mod held;
mod icons;
mod launcher;
mod listening;
mod lyrics;
mod measure;
mod models;
mod motion;
mod recent;
mod served;
mod serving;
mod settings;
mod spectrum;
mod stereo;
mod system;
mod theme;
mod toast;
mod together;
mod views;
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}
mod wrapped_card;

pub use crate::{
    app::{Bus, Leaving, Lookups, PlayerModel, ResonateApp, run},
    equaliser::{Curve, EqualiserModel, Sounding},
    error::{Error, Result, WindowKind},
    launcher::{AppIcon, Launcher},
    listening::Listens,
    lyrics::{Look, LyricsModel},
    measure::{Measures, Unmeasured},
    models::{
        Beyond, Consulted, Drawn, Favourited, LibraryModel, ListedRow, MissingRow, Notice, Pass,
        Planned, Portrayed, Pressings, Previewed, RootWaiting, Selection, Tone,
    },
    served::{ServedAlbum, ServedArtist, ServedCover, ServedFound, ServedSong, Serves, Serving},
    settings::{
        Asking, Bindings, CaretBlink, ChecksYoutubeMusic, Continuing, Ephemeral, Followed,
        HandedOff, JellyfinBegun, JellyfinSession, JellyfinSignIn, ListOffered, Measurers,
        MirrorTold, Mirroring, Newer, Online, Places, Present, Registering, RemoteKey,
        RemoteRowShown, RemoteShown, RemoteZoneShown, Renderers, Setting, SettingChange,
        SettingKey, Settings, SettingsCategory, SignsInToJellyfin, SignsInToYoutubeMusic, Sourcing,
        SpotifySignIn, Stored, Supplying, SyncShown, SyncStanding, SyncedDevice, SyncedPlatform,
        Synchronised, Syncing, Tabs, TidalSignIn, Unkept, Updates, WindowButtons, WindowSize,
        YoutubeMusicConnection, YoutubeMusicSignIn,
    },
    together::{
        PartyReached, PartySeen, PartyStanding, SharedPerson, SharedReach, SharedSeen, Together,
    },
    views::{Pane, RootView},
};
