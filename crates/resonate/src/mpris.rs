use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use clap::ValueEnum;
use crossbeam_channel::{Receiver, Sender};
use resonate_codec::Sources;
use resonate_core::{FrameSpan, MediaLocation, SourceId, TrackId};
use resonate_engine::{Command, NodeName, Placement, Player, QueueItem, Unclaimed};
use resonate_library::Library;
use resonate_mpris::{Heard, Host, Mpris, Opened, Reloadable, Reloaded, Tunable, Tuned};

use crate::{
    cli::{DitherArg, FilterPhaseArg, NoiseShapingArg, QualityArg},
    config, equaliser, names_a_sheet,
    playlists::Collection,
    sheet_items,
};

const IDENTITY: &str = "Resonate";
const DESKTOP_ENTRY: &str = "resonate";
const SANDBOXED_AS: &str = "FLATPAK_ID";
const OPENS_IN_FRONT: bool = true;
const MIME_TYPES: [&str; 20] = [
    "audio/flac",
    "audio/x-flac",
    "audio/mpeg",
    "audio/mp4",
    "audio/x-m4b",
    "audio/aac",
    "audio/ogg",
    "audio/x-vorbis+ogg",
    "audio/opus",
    "audio/x-opus+ogg",
    "audio/x-wav",
    "audio/wav",
    "audio/x-matroska",
    "audio/x-aiff",
    "audio/aiff",
    "audio/x-caf",
    "audio/x-dsf",
    "audio/x-dff",
    "audio/x-wavpack",
    "audio/x-ape",
];

fn entry_named(sandboxed_as: Option<&str>) -> String {
    sandboxed_as
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(DESKTOP_ENTRY)
        .to_owned()
}

struct Attention {
    changes: Receiver<bool>,
    held: AtomicBool,
}

impl Attention {
    fn held(&self) -> bool {
        for held in self.changes.try_iter() {
            self.held.store(held, Ordering::Release);
        }
        self.held.load(Ordering::Acquire)
    }
}

struct Desktop {
    player: Arc<Player>,
    sources: Arc<Sources>,
    library: Option<Arc<Library>>,
    settings: Option<PathBuf>,
    quit: Option<Sender<()>>,
    attention: Option<Attention>,
    raise: Option<Sender<()>>,
    #[cfg(feature = "ui")]
    followed: Option<Sender<resonate_ui::Followed>>,
    notify: Arc<AtomicBool>,
}

pub(crate) struct Window {
    pub(crate) attention: Receiver<bool>,
    pub(crate) raise: Sender<()>,
    #[cfg(feature = "ui")]
    pub(crate) followed: Sender<resonate_ui::Followed>,
}

impl Host for Desktop {
    fn identity(&self) -> String {
        IDENTITY.to_owned()
    }

    fn desktop_entry(&self) -> Option<String> {
        Some(entry_named(env::var(SANDBOXED_AS).ok().as_deref()))
    }

    fn can_quit(&self) -> bool {
        self.quit.is_some()
    }

    fn can_raise(&self) -> bool {
        self.raise.is_some()
    }

    fn raise(&self) {
        let Some(raise) = self.raise.as_ref() else {
            return;
        };
        if raise.try_send(()).is_err() {
            tracing::debug!("a raise from the session bus arrived while one was already pending");
        }
    }

    fn attended(&self) -> bool {
        self.attention.as_ref().is_some_and(Attention::held)
    }

    fn notifies(&self) -> bool {
        self.notify.load(Ordering::Acquire)
    }

    fn answers_headsets(&self) -> bool {
        true
    }

    fn quit(&self) {
        let Some(quit) = self.quit.as_ref() else {
            return;
        };
        if quit.try_send(()).is_err() {
            tracing::debug!("a quit from the session bus arrived while one was already pending");
        }
    }

    fn sources(&self) -> Vec<SourceId> {
        self.sources.names()
    }

    fn mime_types(&self) -> Vec<String> {
        MIME_TYPES.iter().map(|kind| (*kind).to_owned()).collect()
    }

    fn open(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Opened {
        let queue = self.player.queue();
        let mut minting = Unclaimed::beside(&queue);

        let items = match location.as_path().filter(|_| names_a_sheet(location)) {
            Some(sheet) => sheet_items(&self.sources, sheet, &mut minting),
            None => vec![QueueItem {
                id: self
                    .held_as(location, span)
                    .unwrap_or_else(|| minting.mint()),
                location: location.clone(),
                span,
            }],
        };
        if items.is_empty() {
            return Opened::Refused;
        }
        let insert = Command::Insert {
            items,
            at: Placement::Next,
            play: true,
        };

        match self.player.send(insert) {
            Ok(()) => Opened::Accepted,
            Err(error) => {
                tracing::warn!(%error, "a file opened from the session bus did not reach the engine");
                Opened::Refused
            }
        }
    }

    fn heard(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<Heard> {
        let catalog = self.library.as_ref()?;
        let path = location.as_path()?;

        match catalog.track_at(path, span) {
            Ok(track) => track.map(|track| Heard {
                plays: track.plays,
                played: track.played,
                favourite: track.favourite.is_some(),
            }),
            Err(error) => {
                tracing::debug!(%error, "a play count asked for on the session bus could not be read");
                None
            }
        }
    }

    fn held_as(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<TrackId> {
        held_as(self.library.as_deref()?, location, span)
    }

    fn tune(&self, setting: Tunable, value: &str) -> Tuned {
        let Some(command) = tuning_command(setting, value) else {
            return Tuned::Refused;
        };

        match self.player.send(command) {
            Ok(()) => Tuned::Applied,
            Err(error) => {
                tracing::warn!(%error, "a setting handed over the session bus did not reach the engine");
                Tuned::Refused
            }
        }
    }

    fn reload(&self, group: Reloadable, settings: &str) -> Reloaded {
        if !self.reads(settings) {
            return Reloaded::Refused;
        }

        match group {
            Reloadable::Equaliser => self.equalised_again(Path::new(settings)),
            Reloadable::Submission | Reloadable::Sync if self.library.is_some() => {
                Reloaded::Applied
            }
            Reloadable::Submission | Reloadable::Sync => Reloaded::Refused,
            Reloadable::Tidal => self.tidal_signed_in_again(Path::new(settings)),
        }
    }
}

impl Desktop {
    fn reads(&self, settings: &str) -> bool {
        self.settings
            .as_deref()
            .is_some_and(|own| same_file(own, Path::new(settings)))
    }

    fn equalised_again(&self, settings: &Path) -> Reloaded {
        let config = match config::load(Some(settings)) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(%error, "the equaliser could not be read again from the settings");
                return Reloaded::Refused;
            }
        };
        let equalisation = Arc::new(equaliser::resolved(&config));

        match self.player.send(Command::SetEqualisation(equalisation)) {
            Ok(()) => {
                self.tell_the_window_of_the_bindings(&config);
                Reloaded::Applied
            }
            Err(error) => {
                tracing::warn!(%error, "the equaliser read again did not reach the engine");
                Reloaded::Refused
            }
        }
    }
}

#[cfg(feature = "ui")]
impl Desktop {
    fn tell_the_window(&self, followed: resonate_ui::Followed) -> Reloaded {
        let Some(window) = self.followed.as_ref() else {
            return Reloaded::Refused;
        };

        match window.send(followed) {
            Ok(()) => Reloaded::Applied,
            Err(error) => {
                tracing::debug!(%error, "the window was not told of a change to the settings");
                Reloaded::Refused
            }
        }
    }

    fn tell_the_window_of_the_bindings(&self, config: &config::Config) {
        self.tell_the_window(resonate_ui::Followed::Bindings(equaliser::bound(config)));
    }

    fn tidal_signed_in_again(&self, settings: &Path) -> Reloaded {
        match config::load(Some(settings)) {
            Ok(config) => {
                self.tell_the_window(resonate_ui::Followed::Tidal(tidal_sign_in(&config)))
            }
            Err(error) => {
                tracing::warn!(%error, "the TIDAL account could not be read again from the settings");
                Reloaded::Refused
            }
        }
    }
}

#[cfg(not(feature = "ui"))]
impl Desktop {
    fn tell_the_window_of_the_bindings(&self, _config: &config::Config) {}

    fn tidal_signed_in_again(&self, _settings: &Path) -> Reloaded {
        Reloaded::Refused
    }
}

#[cfg(feature = "ui")]
fn tidal_sign_in(config: &config::Config) -> resonate_ui::TidalSignIn {
    resonate_ui::TidalSignIn {
        client_id: config.tidal_client_id.clone().unwrap_or_default(),
        client_secret: config.tidal_client_secret.clone().unwrap_or_default(),
        refresh_token: config.tidal_refresh_token.clone().unwrap_or_default(),
    }
}

fn same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => first == second,
    }
}

fn tuning_command(setting: Tunable, value: &str) -> Option<Command> {
    Some(match setting {
        Tunable::Sink => Command::SetSink(Some(NodeName::new(value))),
        Tunable::Quality => Command::SetQuality(spelled::<QualityArg>(value)?.into()),
        Tunable::FilterPhase => Command::SetFilterPhase(spelled::<FilterPhaseArg>(value)?.into()),
        Tunable::Dither => Command::SetDither(spelled::<DitherArg>(value)?.into()),
        Tunable::NoiseShaping => {
            Command::SetNoiseShaping(spelled::<NoiseShapingArg>(value)?.into())
        }
        Tunable::BitPerfect => Command::SetBitPerfect(value.parse().ok()?),
    })
}

fn spelled<Choice: ValueEnum>(value: &str) -> Option<Choice> {
    Choice::from_str(value, false).ok()
}

#[cfg(feature = "ui")]
pub(crate) fn tuning_of(cli: &crate::cli::Cli) -> Vec<(Tunable, String)> {
    let mut tuning = Vec::new();
    if let Some(sink) = &cli.sink {
        tuning.push((Tunable::Sink, sink.clone()));
    }
    if let Some(quality) = cli.quality {
        tuning.push((Tunable::Quality, name_of(quality)));
    }
    if let Some(phase) = cli.filter_phase {
        tuning.push((Tunable::FilterPhase, name_of(phase)));
    }
    if let Some(dither) = cli.dither {
        tuning.push((Tunable::Dither, name_of(dither)));
    }
    if let Some(shaping) = cli.noise_shaping {
        tuning.push((Tunable::NoiseShaping, name_of(shaping)));
    }
    if cli.bit_perfect || cli.no_bit_perfect {
        tuning.push((Tunable::BitPerfect, cli.bit_perfect.to_string()));
    }
    tuning
}

#[cfg(feature = "ui")]
fn name_of<Choice: ValueEnum>(choice: Choice) -> String {
    choice
        .to_possible_value()
        .map(|spelling| spelling.get_name().to_owned())
        .unwrap_or_default()
}

pub(crate) fn held_as(
    library: &Library,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Option<TrackId> {
    match library.track_at(location.as_path()?, span) {
        Ok(track) => track.map(|track| track.id),
        Err(error) => {
            tracing::debug!(%error, "whether the catalog holds a queued file could not be read");
            None
        }
    }
}

pub(crate) fn claimed_by(library: Option<&Library>, items: Vec<QueueItem>) -> Vec<QueueItem> {
    let Some(library) = library else {
        return items;
    };
    items
        .into_iter()
        .map(|item| match held_as(library, &item.location, item.span) {
            Some(id) => QueueItem { id, ..item },
            None => item,
        })
        .collect()
}

pub(crate) fn start(
    player: &Arc<Player>,
    sources: &Arc<Sources>,
    library: Option<&Arc<Library>>,
    settings: Option<PathBuf>,
    quit: Option<Sender<()>>,
    window: Option<Window>,
    notify: &Arc<AtomicBool>,
) -> Option<Mpris> {
    #[cfg(feature = "ui")]
    let followed = window.as_ref().map(|window| window.followed.clone());
    let (attention, raise) = match window {
        Some(window) => (Some(window.attention), Some(window.raise)),
        None => (None, None),
    };
    let host = Desktop {
        player: Arc::clone(player),
        sources: Arc::clone(sources),
        library: library.map(Arc::clone),
        settings,
        quit,
        attention: attention.map(|changes| Attention {
            changes,
            held: AtomicBool::new(OPENS_IN_FRONT),
        }),
        raise,
        #[cfg(feature = "ui")]
        followed,
        notify: Arc::clone(notify),
    };
    let playlists = library.map(|library| {
        Arc::new(Collection::new(library, player)) as Arc<dyn resonate_mpris::Playlists>
    });

    match Mpris::start(Arc::clone(player), Arc::new(host), playlists) {
        Ok(mpris) => Some(mpris),
        Err(error) => {
            tracing::warn!(%error, "media keys and desktop controls are unavailable");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_mime_types() -> Vec<&'static str> {
        let entry = include_str!("../../../packaging/resonate.desktop");
        entry
            .lines()
            .find_map(|line| line.strip_prefix("MimeType="))
            .expect("the desktop entry declares its mime types")
            .split(';')
            .filter(|kind| !kind.is_empty())
            .collect()
    }

    fn metainfo_media_types() -> Vec<&'static str> {
        let metainfo = include_str!("../../../packaging/org.resonate.Resonate.metainfo.xml");
        metainfo
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("<mediatype>")?
                    .strip_suffix("</mediatype>")
            })
            .collect()
    }

    #[test]
    fn a_setting_handed_over_the_bus_becomes_the_command_that_sets_it_or_is_refused() {
        assert!(matches!(
            tuning_command(Tunable::Quality, "very-high"),
            Some(Command::SetQuality(resonate_engine::Quality::VeryHigh))
        ));
        assert!(matches!(
            tuning_command(Tunable::FilterPhase, "minimum"),
            Some(Command::SetFilterPhase(
                resonate_engine::FilterPhase::Minimum
            ))
        ));
        assert!(matches!(
            tuning_command(Tunable::Dither, "none"),
            Some(Command::SetDither(resonate_engine::DitherKind::None))
        ));
        assert!(matches!(
            tuning_command(Tunable::NoiseShaping, "lipshitz"),
            Some(Command::SetNoiseShaping(
                resonate_engine::NoiseShaping::Lipshitz
            ))
        ));
        assert!(matches!(
            tuning_command(Tunable::BitPerfect, "true"),
            Some(Command::SetBitPerfect(true))
        ));
        assert!(matches!(
            tuning_command(Tunable::Sink, "alsa_output.usb"),
            Some(Command::SetSink(Some(_)))
        ));

        assert!(tuning_command(Tunable::Quality, "ultra").is_none());
        assert!(tuning_command(Tunable::BitPerfect, "maybe").is_none());
    }

    #[test]
    fn every_setting_the_bus_takes_is_spelled_as_its_flag_spells_it() {
        for setting in Tunable::ALL {
            assert_eq!(Tunable::named(setting.name()), Some(setting));
        }
        assert_eq!(
            spelled::<QualityArg>("very-high"),
            Some(QualityArg::VeryHigh)
        );
        assert_eq!(spelled::<QualityArg>("VeryHigh"), None);
    }

    #[test]
    fn the_metainfo_provides_the_media_types_the_bus_offers_and_no_others() {
        let provided = metainfo_media_types();

        for kind in MIME_TYPES {
            assert!(
                provided.contains(&kind),
                "{kind} is missing from the metainfo"
            );
        }
        for kind in provided {
            assert!(
                MIME_TYPES.contains(&kind),
                "{kind} is provided by the metainfo and not offered on the bus"
            );
        }
    }

    #[test]
    fn under_flatpak_the_bus_names_the_entry_the_sandbox_exports() {
        assert_eq!(entry_named(None), DESKTOP_ENTRY);
        assert_eq!(entry_named(Some("")), DESKTOP_ENTRY);
        assert_eq!(
            entry_named(Some("org.resonate.Resonate")),
            "org.resonate.Resonate"
        );
    }

    #[test]
    fn the_desktop_entry_and_the_bus_advertise_the_same_media_types() {
        let declared = entry_mime_types();

        for kind in MIME_TYPES {
            assert!(declared.contains(&kind), "{kind} is missing from the entry");
        }
        for kind in declared {
            assert!(
                MIME_TYPES.contains(&kind),
                "{kind} is claimed by the entry and not offered on the bus"
            );
        }
    }

    #[test]
    fn a_settings_file_is_the_same_one_however_it_is_spelled_and_another_is_not() {
        let folder = crate::fixtures::scratch("mpris-settings");
        let settings = folder.join("config.toml");
        let other = folder.join("other.toml");
        fs::write(&settings, "").expect("a settings file");
        fs::write(&other, "").expect("another settings file");
        let roundabout = folder.join(".").join("config.toml");

        assert!(same_file(&settings, &roundabout));
        assert!(!same_file(&settings, &other));
        assert!(same_file(
            &folder.join("missing.toml"),
            &folder.join("missing.toml")
        ));

        let _ = fs::remove_dir_all(&folder);
    }
}
