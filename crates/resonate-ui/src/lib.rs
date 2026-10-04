#[macro_use]
mod keys;
mod analysis;
mod analysis_plot;
mod app;
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
mod icons;
mod launcher;
mod listening;
mod lyrics;
mod models;
mod recent;
mod settings;
mod spectrum;
mod theme;
mod toast;
mod views;

pub use crate::{
    app::{Bus, Lookups, PlayerModel, ResonateApp, run},
    equaliser::{Curve, EqualiserModel},
    error::{Error, Result, WindowKind},
    launcher::{AppIcon, Launcher},
    listening::Listens,
    lyrics::{Look, LyricsModel},
    models::{
        Beyond, Consulted, Drawn, Favourited, LibraryModel, ListedRow, MissingRow, Notice, Pass,
        Planned, Portrayed, Pressings, Previewed, RootWaiting, Selection, Tone,
    },
    settings::{
        Bindings, CaretBlink, Ephemeral, Online, Places, Present, Registering, Setting,
        SettingChange, SettingKey, Settings, SettingsCategory, Sourcing, Stored, Supplying, Tabs,
        WindowButtons, WindowSize,
    },
    views::{Pane, RootView},
};
