use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    fs::{self},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use resonate_core::{
    Accent, AppId, ArtistsDrawn, HeadYaw, Icon, Pictured, Presence, RemoteTrust, ScrollbarMode,
    Shown, SpeakerAngle, SpectrumBands, SpectrumFalls, SpectrumFloor, SpectrumTilt, Switch,
    TextSize, Theme, Volume,
};
#[cfg(any(feature = "ui", test))]
use resonate_core::{Appearance, Following};
use resonate_engine::{
    Crossfeed, DitherKind, FilterPhase, NodeName, NoiseShaping, Plays, PreviousRestarts, Quality,
    ReplayGainMode, Restoration, ShuffleStyle, SkipUnderRepeat, SmartShuffle, Trial,
};
use resonate_eq::{Binding, ProfileName};
use resonate_library::{HistoryKept, Layout, MusicFilters};
use resonate_listen::Listening;
use resonate_upnp::{ServerName, Udn, Vouched};
use resonate_words::{Language, LanguageChoice, RegionChoice};
use toml_edit::Item;

#[cfg(feature = "ui")]
use super::reading::tab;
#[cfg(unix)]
use super::{
    places::entered_by_its_owner_alone,
    writing::{STAGING_SUFFIX, followed},
};
use super::{
    places::{library_folder_for, windows_variable_for},
    reading::{
        SETTINGS_CATEGORIES, SETTINGS_CATEGORIES_SPELT_BEFORE, document, named_on_the_command_line,
        parse, settings_category,
    },
    writing::write,
    *,
};
use crate::{
    ConfigKey, Error, Result, ValueKind,
    cli::{DitherArg, FilterPhaseArg, NoiseShapingArg, QualityArg},
};

mod accounts;
mod appearance;
mod equaliser;
mod man_page;
mod places;
mod reading;
mod writing;

fn spoken(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn read(text: &str) -> Result<Config> {
    let mut refusals = Vec::new();

    let config = parse(Path::new("/config.toml"), text, |error| {
        refusals.push(error)
    })?;
    match refusals.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(config),
    }
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let path = env::temp_dir()
            .join(format!(
                "resonate-config-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("config.toml");
        Self { path }
    }

    fn seed(self, text: &str) -> Self {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        write(&self.path, text).expect("a writable temporary directory");
        self
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.path).expect("the settings file was written")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn kept(name: &str) -> Binding {
    Binding::Profile(ProfileName::new(name).expect("a usable name"))
}

fn bound<'b>(bindings: &'b Bindings, sink: Option<&NodeName>) -> Option<&'b Binding> {
    bindings.for_sink(sink).map(|(_, binding)| binding)
}
