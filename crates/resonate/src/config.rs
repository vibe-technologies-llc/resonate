use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use clap::ValueEnum as _;
#[cfg(any(feature = "ui", test))]
use resonate_core::Appearance;
use resonate_core::{
    Accent, AppId, Icon, Pictured, Presence, ScrollbarMode, Shown, TextSize, Theme, Trim, Volume,
};
use resonate_engine::{
    DitherKind, FilterPhase, NoiseShaping, Quality, ReplayGainMode, Restoration, SkipUnderRepeat,
};
use resonate_eq::{Binding, ProfileName};
use resonate_library::Layout;
use resonate_listen::Listening;
use resonate_pipewire::NodeName;
use toml_edit::{DocumentMut, Item, Table};

use crate::{
    ConfigKey, Error, Result, ValueKind,
    cli::{DitherArg, FilterPhaseArg, NoiseShapingArg, QualityArg},
};

const LISTENS_FOR_SECONDS: std::ops::RangeInclusive<u64> = 4..=60;

const LOCK_SUFFIX: &str = ".lock";

const STAGING_SUFFIX: &str = ".new";

static STAGED: AtomicU64 = AtomicU64::new(0);

fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = env::var_os(var).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(fallback))
}

pub fn data_dir() -> Result<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share")
        .map(|dir| dir.join("resonate"))
        .ok_or(Error::NoDataDir)
}

#[cfg(feature = "ui")]
pub fn icon_theme_dir() -> Result<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share")
        .map(|dir| dir.join("icons").join("hicolor"))
        .ok_or(Error::NoDataDir)
}

pub fn library_path() -> Result<PathBuf> {
    let dir = data_dir()?;
    fs::create_dir_all(&dir).map_err(|source| Error::CreateDir {
        path: dir.clone(),
        source,
    })?;
    Ok(dir.join("library.db"))
}

pub fn vault_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("vault"))
}

pub fn equaliser_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("equaliser"))
}

pub fn config_path() -> Result<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", ".config")
        .map(|dir| dir.join("resonate").join("config.toml"))
        .ok_or(Error::NoConfigDir)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub sink: Option<NodeName>,
    pub library: Option<PathBuf>,
    pub vault: Option<PathBuf>,
    pub quality: Option<Quality>,
    pub filter_phase: Option<FilterPhase>,
    pub true_peak: Option<bool>,
    pub restoration: Option<Restoration>,
    pub dither: Option<DitherKind>,
    pub noise_shaping: Option<NoiseShaping>,
    pub replay_gain: Option<ReplayGainMode>,
    pub pre_amp: Option<Trim>,
    pub untagged: Option<Trim>,
    pub bit_perfect: Option<bool>,
    pub dop: Option<bool>,
    pub force_graph_rate: Option<bool>,
    pub bluetooth_wake: Option<bool>,
    pub bluetooth_lead: Option<Duration>,
    pub bluetooth_awake: Option<Duration>,
    pub volume: Option<Volume>,
    pub buffer: Option<Duration>,
    pub theme: Option<Theme>,
    pub accent: Option<Accent>,
    pub text_size: Option<TextSize>,
    pub online: Option<bool>,
    pub enrich_after_scan: Option<bool>,
    pub study: Option<bool>,
    pub skip_repeats_queue: Option<bool>,
    pub contact: Option<String>,
    pub acoustid_key: Option<String>,
    pub audd_token: Option<String>,
    pub listen_from: Option<Listening>,
    pub listen_for: Option<Duration>,
    pub equaliser: Option<bool>,
    pub equaliser_for: Option<Bindings>,
    pub resume: Option<bool>,
    pub organise_as: Option<Layout>,
    pub notify: Option<bool>,
    pub minimise_button: Option<bool>,
    pub maximise_button: Option<bool>,
    pub scroll_volume: Option<bool>,
    pub scrollbars: Option<ScrollbarMode>,
    pub suggestions_tab: Option<bool>,
    pub missing_tab: Option<bool>,
    pub tab_counts: Option<bool>,
    pub inbox: Option<PathBuf>,
    pub discord: Option<bool>,
    pub discord_app: Option<AppId>,
    pub discord_shows: Option<Shown>,
    pub discord_art: Option<Pictured>,
    pub discord_icon: Option<Icon>,
    pub discord_progress: Option<bool>,
    pub discord_paused: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings {
    fallback: Option<Binding>,
    by_sink: BTreeMap<NodeName, Binding>,
}

impl Bindings {
    pub const fn fallback(&self) -> Option<&Binding> {
        self.fallback.as_ref()
    }

    pub fn by_sink(&self) -> impl Iterator<Item = (&NodeName, &Binding)> {
        self.by_sink.iter()
    }

    pub fn for_sink(&self, sink: Option<&NodeName>) -> Option<(Option<&NodeName>, &Binding)> {
        sink.and_then(|sink| self.by_sink.get_key_value(sink))
            .map(|(owner, bound)| (Some(owner), bound))
            .or_else(|| self.fallback().map(|bound| (None, bound)))
    }

    pub fn is_empty(&self) -> bool {
        self.fallback.is_none() && self.by_sink.is_empty()
    }
}

impl Config {
    #[cfg(any(feature = "ui", test))]
    pub fn appearance(&self) -> Appearance {
        let worn = Appearance::default();

        Appearance {
            theme: self.theme.unwrap_or(worn.theme),
            accent: self.accent.or(worn.accent),
            text_size: self.text_size.unwrap_or(worn.text_size),
        }
    }

    #[cfg(any(feature = "online", test))]
    pub fn online_enabled(&self) -> bool {
        self.online.unwrap_or(true)
    }

    pub fn enriches_after_scan(&self) -> bool {
        self.enrich_after_scan.unwrap_or(true)
    }

    pub fn studies(&self) -> bool {
        self.study.unwrap_or(true)
    }

    pub fn skip_under_repeat(&self) -> SkipUnderRepeat {
        match self.skip_repeats_queue {
            Some(false) => SkipUnderRepeat::KeepsRepeatingTheTrack,
            Some(true) | None => SkipUnderRepeat::RepeatsTheQueue,
        }
    }

    pub fn equaliser_on(&self) -> bool {
        self.equaliser.unwrap_or(false)
    }

    pub fn resumes(&self) -> bool {
        self.resume.unwrap_or(true)
    }

    pub fn notifies(&self) -> bool {
        self.notify.unwrap_or(true)
    }

    #[cfg(feature = "ui")]
    pub fn window_buttons(&self) -> resonate_ui::WindowButtons {
        let shown = resonate_ui::WindowButtons::SHOWN;

        resonate_ui::WindowButtons {
            minimise: self.minimise_button.unwrap_or(shown.minimise),
            maximise: self.maximise_button.unwrap_or(shown.maximise),
        }
    }

    #[cfg(feature = "ui")]
    pub fn scrolls_the_volume(&self) -> bool {
        self.scroll_volume.unwrap_or(true)
    }

    #[cfg(feature = "ui")]
    pub fn scrollbars(&self) -> ScrollbarMode {
        self.scrollbars.unwrap_or_default()
    }

    #[cfg(feature = "ui")]
    pub fn tabs(&self) -> resonate_ui::Tabs {
        let built = resonate_ui::Tabs::AS_BUILT;

        resonate_ui::Tabs {
            suggestions: self.suggestions_tab.unwrap_or(built.suggestions),
            missing: self.missing_tab.unwrap_or(built.missing),
            counts: self.tab_counts.unwrap_or(built.counts),
        }
    }

    pub fn presence(&self) -> Presence {
        let off = Presence::OFF;

        Presence {
            enabled: self.discord.unwrap_or(off.enabled),
            app: self.discord_app.or(off.app),
            shown: self.discord_shows.unwrap_or(off.shown),
            pictured: self.discord_art.unwrap_or(off.pictured),
            icon: self.discord_icon.clone().or(off.icon),
            progress: self.discord_progress.unwrap_or(off.progress),
            while_paused: self.discord_paused.unwrap_or(off.while_paused),
        }
    }

    pub fn organise_as(&self) -> Layout {
        self.organise_as.clone().unwrap_or_default()
    }
}

pub fn load(explicit: Option<&Path>) -> Result<Config> {
    let (path, demanded) = match explicit {
        Some(path) => (path.to_path_buf(), true),
        None => (config_path()?, false),
    };

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound && !demanded => {
            return Ok(Config::default());
        }
        Err(source) => return Err(Error::ReadConfig { path, source }),
    };
    parse(&path, &text)
}

fn document(path: &Path, text: &str) -> Result<DocumentMut> {
    text.parse().map_err(|source| Error::ConfigSyntax {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

fn parse(path: &Path, text: &str) -> Result<Config> {
    let document = document(path, text)?;

    let mut config = Config::default();
    for (name, value) in document.iter() {
        let Some(key) = ConfigKey::parse(name) else {
            tracing::warn!(key = name, path = %path.display(), "ignoring an unknown setting");
            continue;
        };
        let at = At { path, key };

        match key {
            ConfigKey::Sink => config.sink = Some(NodeName::new(at.string(value)?)),
            ConfigKey::Library => config.library = Some(PathBuf::from(at.string(value)?)),
            ConfigKey::Vault => config.vault = Some(PathBuf::from(at.string(value)?)),
            ConfigKey::Quality => config.quality = Some(at.one_of(value, quality)?),
            ConfigKey::FilterPhase => {
                config.filter_phase = Some(at.one_of(value, filter_phase)?);
            }
            ConfigKey::Dither => config.dither = Some(at.one_of(value, dither)?),
            ConfigKey::NoiseShaping => {
                config.noise_shaping = Some(at.one_of(value, noise_shaping)?);
            }
            ConfigKey::ReplayGain => config.replay_gain = Some(at.one_of(value, replay_gain)?),
            ConfigKey::ReplayGainPreAmp => config.pre_amp = Some(at.trim(value)?),
            ConfigKey::ReplayGainUntagged => config.untagged = Some(at.trim(value)?),
            ConfigKey::BitPerfect => config.bit_perfect = Some(at.boolean(value)?),
            ConfigKey::Dop => config.dop = Some(at.boolean(value)?),
            ConfigKey::TruePeak => config.true_peak = Some(at.boolean(value)?),
            ConfigKey::RestoreLossy => {
                config.restoration = Some(at.one_of(value, Restoration::named)?);
            }
            ConfigKey::ForceGraphRate => config.force_graph_rate = Some(at.boolean(value)?),
            ConfigKey::BluetoothWake => config.bluetooth_wake = Some(at.boolean(value)?),
            ConfigKey::BluetoothLeadMs => {
                let millis = u64::try_from(at.integer(value)?).map_err(|_| at.rejected())?;
                config.bluetooth_lead = Some(Duration::from_millis(millis));
            }
            ConfigKey::BluetoothAwakeS => {
                let seconds = u64::try_from(at.integer(value)?).map_err(|_| at.rejected())?;
                config.bluetooth_awake = Some(Duration::from_secs(seconds));
            }
            ConfigKey::Volume => {
                let position = at.float(value)?;
                let volume = Volume::new(position as f32).map_err(|_| at.rejected())?;
                config.volume = Some(volume);
            }
            ConfigKey::Theme => config.theme = Some(at.one_of(value, Theme::parse)?),
            ConfigKey::Accent => config.accent = Some(at.one_of(value, Accent::parse)?),
            ConfigKey::TextSize => {
                config.text_size = Some(at.one_of(value, TextSize::parse)?);
            }
            ConfigKey::Online => config.online = Some(at.boolean(value)?),
            ConfigKey::EnrichAfterScan => config.enrich_after_scan = Some(at.boolean(value)?),
            ConfigKey::Study => config.study = Some(at.boolean(value)?),
            ConfigKey::SkipRepeatsQueue => {
                config.skip_repeats_queue = Some(at.boolean(value)?);
            }
            ConfigKey::Contact => config.contact = given(at.string(value)?),
            ConfigKey::AcoustidKey => config.acoustid_key = given(at.string(value)?),
            ConfigKey::AuddToken => config.audd_token = given(at.string(value)?),
            ConfigKey::ListenFrom => {
                config.listen_from = Some(Listening::named(at.string(value)?));
            }
            ConfigKey::ListenFor => {
                let seconds = u64::try_from(at.integer(value)?)
                    .ok()
                    .filter(|seconds| LISTENS_FOR_SECONDS.contains(seconds))
                    .ok_or_else(|| at.rejected())?;
                config.listen_for = Some(Duration::from_secs(seconds));
            }
            ConfigKey::Inbox => config.inbox = given(at.string(value)?).map(PathBuf::from),
            ConfigKey::Equaliser => config.equaliser = Some(at.boolean(value)?),
            ConfigKey::Resume => config.resume = Some(at.boolean(value)?),
            ConfigKey::Notify => config.notify = Some(at.boolean(value)?),
            ConfigKey::MinimiseButton => config.minimise_button = Some(at.boolean(value)?),
            ConfigKey::MaximiseButton => config.maximise_button = Some(at.boolean(value)?),
            ConfigKey::ScrollVolume => config.scroll_volume = Some(at.boolean(value)?),
            ConfigKey::Scrollbars => {
                config.scrollbars = Some(match value.as_bool() {
                    Some(drawn) => ScrollbarMode::of_a_switch(drawn),
                    None => at.one_of(value, ScrollbarMode::parse)?,
                });
            }
            ConfigKey::SuggestionsTab => config.suggestions_tab = Some(at.boolean(value)?),
            ConfigKey::MissingTab => config.missing_tab = Some(at.boolean(value)?),
            ConfigKey::TabCounts => config.tab_counts = Some(at.boolean(value)?),
            ConfigKey::OrganiseAs => config.organise_as = Some(at.one_of(value, layout)?),
            ConfigKey::EqualiserFor => {
                config.equaliser_for.get_or_insert_default().by_sink = bindings(at, value)?;
            }
            ConfigKey::EqualiserProfile => {
                config.equaliser_for.get_or_insert_default().fallback = at.binding(value)?;
            }
            ConfigKey::Discord => config.discord = Some(at.boolean(value)?),
            ConfigKey::DiscordApp => config.discord_app = at.unless_blank(value, AppId::parse)?,
            ConfigKey::DiscordShows => {
                config.discord_shows = Some(at.one_of(value, Shown::parse)?);
            }
            ConfigKey::DiscordArt => config.discord_art = Some(at.one_of(value, Pictured::parse)?),
            ConfigKey::DiscordIcon => config.discord_icon = at.unless_blank(value, Icon::parse)?,
            ConfigKey::DiscordProgress => config.discord_progress = Some(at.boolean(value)?),
            ConfigKey::DiscordPaused => config.discord_paused = Some(at.boolean(value)?),
            ConfigKey::BufferMs => {
                let millis = u64::try_from(at.integer(value)?).map_err(|_| at.rejected())?;
                config.buffer = Some(Duration::from_millis(millis));
            }
        }
    }
    Ok(config)
}

#[derive(Clone, Copy)]
struct At<'a> {
    path: &'a Path,
    key: ConfigKey,
}

impl At<'_> {
    fn mistyped(self, expected: ValueKind) -> Error {
        Error::ConfigType {
            path: self.path.to_path_buf(),
            key: self.key,
            expected,
        }
    }

    fn rejected(self) -> Error {
        Error::ConfigValue {
            path: self.path.to_path_buf(),
            key: self.key,
        }
    }

    fn string(self, value: &Item) -> Result<&str> {
        value.as_str().ok_or_else(|| self.mistyped(ValueKind::Text))
    }

    fn boolean(self, value: &Item) -> Result<bool> {
        value
            .as_bool()
            .ok_or_else(|| self.mistyped(ValueKind::Boolean))
    }

    fn integer(self, value: &Item) -> Result<i64> {
        value
            .as_integer()
            .ok_or_else(|| self.mistyped(ValueKind::Integer))
    }

    fn float(self, value: &Item) -> Result<f64> {
        value
            .as_float()
            .or_else(|| value.as_integer().map(|whole| whole as f64))
            .ok_or_else(|| self.mistyped(ValueKind::Number))
    }

    fn trim(self, value: &Item) -> Result<Trim> {
        Trim::from_decibels(self.float(value)?).map_err(|_| self.rejected())
    }

    fn one_of<T>(self, value: &Item, read: impl Fn(&str) -> Option<T>) -> Result<T> {
        read(self.string(value)?).ok_or_else(|| self.rejected())
    }

    fn unless_blank<T>(self, value: &Item, read: impl Fn(&str) -> Option<T>) -> Result<Option<T>> {
        given(self.string(value)?)
            .map(|text| read(&text).ok_or_else(|| self.rejected()))
            .transpose()
    }

    fn binding(self, value: &Item) -> Result<Option<Binding>> {
        if let Some(own) = value.as_bool() {
            return own
                .then_some(Some(Binding::Own))
                .ok_or_else(|| self.rejected());
        }
        let Some(named) = given(self.string(value)?) else {
            return Ok(None);
        };
        ProfileName::new(&named)
            .map(|name| Some(Binding::Profile(name)))
            .map_err(|_| self.rejected())
    }

    fn table(self, value: &Item) -> Result<&dyn toml_edit::TableLike> {
        value
            .as_table_like()
            .ok_or_else(|| self.mistyped(ValueKind::Table))
    }
}

fn bindings(at: At<'_>, value: &Item) -> Result<BTreeMap<NodeName, Binding>> {
    let mut by_sink = BTreeMap::new();

    for (named, bound) in at.table(value)?.iter() {
        let Ok(Some(binding)) = at.binding(bound) else {
            tracing::warn!(
                sink = named,
                path = %at.path.display(),
                "ignoring a binding that names neither a profile nor the device's own curve"
            );
            continue;
        };

        by_sink.insert(NodeName::new(named), binding);
    }
    Ok(by_sink)
}

pub fn written(binding: &Binding) -> toml_edit::Value {
    match binding {
        Binding::Profile(name) => name.as_str().into(),
        Binding::Own => true.into(),
    }
}

pub fn store(path: &Path, key: ConfigKey, value: impl Into<toml_edit::Value>) -> Result<()> {
    edited(path, |document, _| {
        document[key.as_str()] = toml_edit::value(value);
        Ok(true)
    })
}

pub fn clear(path: &Path, key: ConfigKey) -> Result<()> {
    edited(path, |document, _| {
        Ok(document.remove(key.as_str()).is_some())
    })
}

pub fn store_in_table(
    path: &Path,
    key: ConfigKey,
    entry: &str,
    value: impl Into<toml_edit::Value>,
) -> Result<()> {
    edited(path, |document, target| {
        let held = document.entry(key.as_str()).or_insert_with(|| {
            let mut fresh = Table::new();
            fresh.decor_mut().set_prefix("\n");
            Item::Table(fresh)
        });
        let table = held.as_table_like_mut().ok_or_else(|| Error::ConfigType {
            path: target.to_path_buf(),
            key,
            expected: ValueKind::Table,
        })?;

        table.insert(entry, toml_edit::value(value));
        Ok(true)
    })
}

pub fn clear_in_table(path: &Path, key: ConfigKey, entry: &str) -> Result<()> {
    edited(path, |document, _| {
        let Some(table) = document
            .get_mut(key.as_str())
            .and_then(Item::as_table_like_mut)
        else {
            return Ok(false);
        };
        if table.remove(entry).is_none() {
            return Ok(false);
        }
        if table.is_empty() {
            document.remove(key.as_str());
        }
        Ok(true)
    })
}

fn edited(path: &Path, edit: impl FnOnce(&mut DocumentMut, &Path) -> Result<bool>) -> Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let _alone = held_alone(&target)?;
    let mut document = document(&target, &read_or_empty(&target)?)?;
    if !edit(&mut document, &target)? {
        return Ok(());
    }
    write(&target, &document.to_string())
}

fn held_alone(target: &Path) -> Result<File> {
    let refused = |source| Error::WriteConfig {
        path: target.to_path_buf(),
        source,
    };
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(beside(target, LOCK_SUFFIX))
        .map_err(refused)?;
    lock.lock().map_err(refused)?;
    Ok(lock)
}

fn write(target: &Path, text: &str) -> Result<()> {
    let staged = beside(
        target,
        &format!(
            ".{}-{}{STAGING_SUFFIX}",
            process::id(),
            STAGED.fetch_add(1, Ordering::Relaxed)
        ),
    );
    let written = laid_down(target, &staged, text);
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
    written.map_err(|source| Error::WriteConfig {
        path: target.to_path_buf(),
        source,
    })
}

fn laid_down(target: &Path, staged: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staged)?;
    file.write_all(text.as_bytes())?;
    if let Ok(standing) = fs::metadata(target) {
        file.set_permissions(standing.permissions())?;
    }
    file.sync_all()?;
    fs::rename(staged, target)?;
    if let Some(parent) = target.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn beside(target: &Path, suffix: &str) -> PathBuf {
    let mut named = target.file_name().unwrap_or_default().to_os_string();
    named.push(suffix);
    target.with_file_name(named)
}

fn read_or_empty(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(Error::ReadConfig {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn given(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn layout(text: &str) -> Option<Layout> {
    Layout::read(text).ok()
}

fn quality(text: &str) -> Option<Quality> {
    QualityArg::from_str(text, false).ok().map(Quality::from)
}

fn filter_phase(text: &str) -> Option<FilterPhase> {
    FilterPhaseArg::from_str(text, false)
        .ok()
        .map(FilterPhase::from)
}

fn dither(text: &str) -> Option<DitherKind> {
    DitherArg::from_str(text, false).ok().map(DitherKind::from)
}

fn noise_shaping(text: &str) -> Option<NoiseShaping> {
    NoiseShapingArg::from_str(text, false)
        .ok()
        .map(NoiseShaping::from)
}

fn replay_gain(text: &str) -> Option<ReplayGainMode> {
    match text {
        "off" => Some(ReplayGainMode::Off),
        "track" => Some(ReplayGainMode::Track),
        "album" => Some(ReplayGainMode::Album),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    fn read(text: &str) -> Result<Config> {
        parse(Path::new("/config.toml"), text)
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

    #[test]
    fn an_empty_document_leaves_every_setting_unset() {
        assert_eq!(read("").expect("empty is valid"), Config::default());
    }

    #[test]
    fn every_setting_reads_back_as_its_domain_type() {
        let config = read(
            r#"
            sink = "alsa_output.pci-0000_00_1f.3.analog-stereo"
            library = "/music/library.db"
            quality = "high"
            dither = "rectangular"
            noise-shaping = "lipshitz"
            replay-gain = "album"
            bit-perfect = false
            force-graph-rate = false
            volume = 0.25
            buffer-ms = 750
            theme = "catppuccin-mocha"
            accent = "teal"
            text-size = "large"
            dop = true
            "#,
        )
        .expect("a well formed document");

        assert_eq!(
            config.sink,
            Some(NodeName::new("alsa_output.pci-0000_00_1f.3.analog-stereo"))
        );
        assert_eq!(config.library, Some(PathBuf::from("/music/library.db")));
        assert_eq!(config.quality, Some(Quality::High));
        assert_eq!(config.dither, Some(DitherKind::Rectangular));
        assert_eq!(config.noise_shaping, Some(NoiseShaping::Lipshitz));
        assert_eq!(config.replay_gain, Some(ReplayGainMode::Album));
        assert_eq!(config.bit_perfect, Some(false));
        assert_eq!(config.force_graph_rate, Some(false));
        assert_eq!(config.volume, Volume::new(0.25).ok());
        assert_eq!(config.buffer, Some(Duration::from_millis(750)));
        assert_eq!(config.theme, Some(Theme::CatppuccinMocha));
        assert_eq!(config.accent, Some(Accent::Teal));
        assert_eq!(config.text_size, Some(TextSize::Large));
        assert_eq!(config.dop, Some(true));
        assert_eq!(
            config.appearance(),
            Appearance {
                theme: Theme::CatppuccinMocha,
                accent: Some(Accent::Teal),
                text_size: TextSize::Large,
            }
        );
    }

    #[test]
    fn every_noise_shaping_is_read_by_its_name() {
        for (text, shaping) in [
            ("flat", NoiseShaping::None),
            ("lipshitz", NoiseShaping::Lipshitz),
            ("threshold", NoiseShaping::Threshold),
        ] {
            let config =
                read(&format!("noise-shaping = \"{text}\"")).expect("a well formed document");
            assert_eq!(config.noise_shaping, Some(shaping), "{text}");
        }
    }

    #[test]
    fn an_inbox_is_read_as_a_folder_and_a_blank_one_is_none() {
        let config = read("inbox = \"/music/inbox\"").expect("a well formed document");
        assert_eq!(config.inbox.as_deref(), Some(Path::new("/music/inbox")));

        let config = read("inbox = \"  \"").expect("a well formed document");
        assert_eq!(config.inbox, None);

        assert_eq!(Config::default().inbox, None);
    }

    #[test]
    fn where_and_how_long_to_listen_are_read() {
        let config =
            read("listen-from = \"microphone\"\nlisten-for = 20").expect("a well formed document");
        assert_eq!(config.listen_from, Some(Listening::Microphone(None)));
        assert_eq!(config.listen_for, Some(Duration::from_secs(20)));
        assert!(read("listen-for = 0").is_err());
    }

    #[test]
    fn an_audd_token_is_read_and_a_blank_one_is_none() {
        let config = read("audd-token = \"a token\"").expect("a well formed document");
        assert_eq!(config.audd_token.as_deref(), Some("a token"));
        assert_eq!(
            read("audd-token = \"\"").expect("well formed").audd_token,
            None
        );
        assert_eq!(Config::default().audd_token, None);
    }

    #[test]
    fn discord_is_shown_nothing_unless_it_is_switched_on_and_given_an_application() {
        assert!(!Config::default().presence().active());
        assert_eq!(Config::default().presence(), Presence::OFF);
        assert!(
            !read("discord = true")
                .expect("well formed")
                .presence()
                .active()
        );
        assert!(
            !read("discord-app = \"1234567890123456789\"")
                .expect("well formed")
                .presence()
                .active()
        );
    }

    #[test]
    fn every_discord_setting_is_read_into_the_presence() {
        let config = read(
            "discord = true\n\
             discord-app = \" 1234567890123456789 \"\n\
             discord-shows = \"track\"\n\
             discord-art = \"icon\"\n\
             discord-icon = \"resonate\"\n\
             discord-progress = false\n\
             discord-paused = true\n",
        )
        .expect("a well formed document");

        assert_eq!(
            config.presence(),
            Presence {
                enabled: true,
                app: AppId::parse("1234567890123456789"),
                shown: Shown::Track,
                pictured: Pictured::Icon,
                icon: Icon::parse("resonate"),
                progress: false,
                while_paused: true,
            }
        );
    }

    #[test]
    fn a_blank_discord_application_or_icon_is_none() {
        let config = read("discord-app = \"  \"\ndiscord-icon = \"\"").expect("well formed");

        assert_eq!(config.discord_app, None);
        assert_eq!(config.discord_icon, None);
    }

    #[test]
    fn a_discord_application_that_is_not_a_snowflake_is_refused_by_name() {
        for written in [
            "discord-app = \"resonate\"",
            "discord-app = \"12345\"",
            "discord-shows = \"everything\"",
            "discord-art = \"portrait\"",
            "discord-icon = \"two words\"",
        ] {
            let error = read(written).expect_err("not a value the key takes");
            let named = written.split(' ').next().and_then(ConfigKey::parse);

            assert!(
                matches!(&error, Error::ConfigValue { key, .. } if Some(*key) == named),
                "{written}: {error:?}"
            );
        }
    }

    #[test]
    fn an_acoustid_key_is_read_and_a_blank_one_is_none() {
        let config =
            read("acoustid-key = \"a key the user typed\"").expect("a well formed document");
        assert_eq!(config.acoustid_key.as_deref(), Some("a key the user typed"));

        let config = read("acoustid-key = \"  \"").expect("a well formed document");
        assert_eq!(config.acoustid_key, None);

        assert_eq!(Config::default().acoustid_key, None);
    }

    #[test]
    fn studies_and_a_skip_under_repeat_default_on_and_are_turned_off_by_their_keys() {
        let config = read("").expect("empty is valid");
        assert!(config.studies());
        assert_eq!(config.skip_under_repeat(), SkipUnderRepeat::RepeatsTheQueue);

        let config =
            read("study = false\nskip-repeats-queue = false").expect("a well formed document");
        assert!(!config.studies());
        assert_eq!(
            config.skip_under_repeat(),
            SkipUnderRepeat::KeepsRepeatingTheTrack
        );
    }

    #[test]
    fn online_and_contact_are_read_and_an_empty_contact_is_none() {
        let config = read("online = false\ncontact = \"a contact the user typed\"")
            .expect("a well formed document");
        assert_eq!(config.online, Some(false));
        assert!(!config.online_enabled());
        assert_eq!(config.contact.as_deref(), Some("a contact the user typed"));

        let config = read("online = true\ncontact = \"  \"").expect("a well formed document");
        assert_eq!(config.online, Some(true));
        assert!(config.online_enabled());
        assert_eq!(config.contact, None);

        let config = read("").expect("empty is valid");
        assert!(config.online_enabled());
        assert!(config.enriches_after_scan());
        assert_eq!(config.contact, None);

        let config = read("enrich-after-scan = false").expect("a well formed document");
        assert_eq!(config.enrich_after_scan, Some(false));
        assert!(!config.enriches_after_scan());

        let error = read("online = \"yes\"").expect_err("a string is not a boolean");
        assert!(
            matches!(
                error,
                Error::ConfigType {
                    key: ConfigKey::Online,
                    expected: ValueKind::Boolean,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[cfg(feature = "ui")]
    #[test]
    fn the_volume_turns_with_the_wheel_until_the_file_says_it_does_not() {
        assert!(read("").expect("empty is valid").scrolls_the_volume());
        assert!(
            !read("scroll-volume = false")
                .expect("a boolean is valid")
                .scrolls_the_volume()
        );
    }

    #[cfg(feature = "ui")]
    #[test]
    fn scrollbars_are_drawn_until_the_file_names_another_mode() {
        let mode = |text: &str| read(text).expect("a mode is valid").scrollbars();

        assert_eq!(mode(""), ScrollbarMode::Shown);
        assert_eq!(
            mode("scrollbars = \"auto-hide\""),
            ScrollbarMode::AutoHidden
        );
        assert_eq!(mode("scrollbars = \"hidden\""), ScrollbarMode::Hidden);
        assert!(read("scrollbars = \"sometimes\"").is_err());
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_scrollbars_switch_written_before_the_modes_still_reads() {
        let mode = |text: &str| read(text).expect("a boolean is valid").scrollbars();

        assert_eq!(mode("scrollbars = true"), ScrollbarMode::Shown);
        assert_eq!(mode("scrollbars = false"), ScrollbarMode::Hidden);
    }

    #[cfg(feature = "ui")]
    #[test]
    fn suggestions_are_listed_and_missing_is_not_until_the_file_says_otherwise() {
        let built = resonate_ui::Tabs {
            suggestions: true,
            missing: false,
            counts: true,
        };

        assert_eq!(read("").expect("empty is valid").tabs(), built);
        assert_eq!(
            read("suggestions-tab = false\nmissing-tab = true")
                .expect("booleans are valid")
                .tabs(),
            resonate_ui::Tabs {
                suggestions: false,
                missing: true,
                ..built
            }
        );
        assert_eq!(
            read("tab-counts = false")
                .expect("a boolean is valid")
                .tabs(),
            resonate_ui::Tabs {
                counts: false,
                ..built
            }
        );
    }

    #[cfg(feature = "ui")]
    #[test]
    fn the_window_buttons_are_shown_until_the_file_hides_them_one_at_a_time() {
        let shown = resonate_ui::WindowButtons::SHOWN;

        assert_eq!(read("").expect("empty is valid").window_buttons(), shown);
        assert_eq!(
            read("minimise-button = false")
                .expect("a well formed document")
                .window_buttons(),
            resonate_ui::WindowButtons {
                minimise: false,
                ..shown
            }
        );
        assert_eq!(
            read("maximise-button = false")
                .expect("a well formed document")
                .window_buttons(),
            resonate_ui::WindowButtons {
                maximise: false,
                ..shown
            }
        );

        let error = read("maximise-button = \"no\"").expect_err("a string is not a boolean");
        assert!(
            matches!(
                error,
                Error::ConfigType {
                    key: ConfigKey::MaximiseButton,
                    expected: ValueKind::Boolean,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_file_that_names_no_palette_is_worn_as_resonate_drew_it() {
        let config = read("volume = 0.5").expect("a well formed document");

        assert_eq!(config.appearance(), Appearance::DEFAULT);
    }

    #[test]
    fn a_palette_no_theme_accent_or_size_claims_names_the_key() {
        for text in [
            "theme = \"solarized\"",
            "accent = \"chartreuse\"",
            "text-size = \"enormous\"",
        ] {
            let error = read(text).expect_err("outside the domain");
            assert!(
                matches!(error, Error::ConfigValue { .. }),
                "{text}: {error:?}"
            );
        }
    }

    #[test]
    fn a_whole_number_is_accepted_where_a_fraction_is_expected() {
        assert_eq!(
            read("volume = 1").expect("1 is in range").volume,
            Some(Volume::MAX)
        );
    }

    #[test]
    fn a_setting_of_the_wrong_type_names_the_key_and_the_type_it_wanted() {
        let error = read("quality = 3").expect_err("an integer is not a quality");
        assert!(
            matches!(
                error,
                Error::ConfigType {
                    key: ConfigKey::Quality,
                    expected: ValueKind::Text,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_setting_outside_its_domain_names_the_key() {
        for text in ["quality = \"perfect\"", "volume = 1.5", "buffer-ms = -1"] {
            let error = read(text).expect_err("outside the domain");
            assert!(
                matches!(error, Error::ConfigValue { .. }),
                "{text}: {error:?}"
            );
        }
    }

    #[test]
    fn an_unknown_key_is_ignored_rather_than_refused() {
        let config = read("sink = \"auto_null\"\nvolume-boost = true").expect("unknown keys pass");
        assert_eq!(config.sink, Some(NodeName::new("auto_null")));
    }

    #[test]
    fn storing_a_setting_leaves_the_rest_of_the_file_as_the_user_wrote_it() {
        let scratch = Scratch::new().seed(
            "# my listening setup\nsink = \"auto_null\"\n\n# resampler\nquality = \"fast\"\n",
        );
        store(&scratch.path, ConfigKey::Quality, "high").expect("a writable file");

        let text = scratch.text();
        assert!(text.contains("# my listening setup"), "{text}");
        assert!(text.contains("# resampler"), "{text}");
        assert!(text.contains("quality = \"high\""), "{text}");
        assert_eq!(
            load(Some(&scratch.path)).expect("it reads back").quality,
            Some(Quality::High)
        );
    }

    #[test]
    fn storing_a_setting_creates_the_file_and_its_directory() {
        let scratch = Scratch::new();
        store(&scratch.path, ConfigKey::ReplayGain, "album").expect("a writable file");

        assert_eq!(
            load(Some(&scratch.path))
                .expect("it reads back")
                .replay_gain,
            Some(ReplayGainMode::Album)
        );
    }

    #[test]
    fn storing_a_setting_twice_replaces_it_rather_than_repeating_it() {
        let scratch = Scratch::new();
        store(&scratch.path, ConfigKey::Dither, "none").expect("a writable file");
        store(&scratch.path, ConfigKey::Dither, "triangular").expect("a writable file");

        let text = scratch.text();
        assert_eq!(text.matches("dither").count(), 1, "{text}");
        assert_eq!(
            load(Some(&scratch.path)).expect("it reads back").dither,
            Some(DitherKind::Triangular)
        );
    }

    #[test]
    fn clearing_a_setting_drops_the_key_and_keeps_the_rest() {
        let scratch =
            Scratch::new().seed("sink = \"auto_null\"\n\n# resampler\nquality = \"high\"\n");
        clear(&scratch.path, ConfigKey::Sink).expect("a writable file");

        let text = scratch.text();
        assert!(text.contains("# resampler"), "{text}");
        assert!(!text.contains("sink"), "{text}");

        let config = load(Some(&scratch.path)).expect("it reads back");
        assert_eq!(config.sink, None);
        assert_eq!(config.quality, Some(Quality::High));
    }

    #[test]
    fn the_very_high_level_is_spelt_the_way_its_flag_is() {
        let scratch = Scratch::new().seed("quality = \"fast\"\n");
        store(&scratch.path, ConfigKey::Quality, "very-high").expect("a writable file");

        assert!(scratch.text().contains("quality = \"very-high\""));
        assert_eq!(
            load(Some(&scratch.path)).expect("it reads back").quality,
            Some(Quality::VeryHigh)
        );
    }

    #[test]
    fn a_lossy_restoration_is_read_by_name() {
        let config = read("restore-lossy = \"extend\"").expect("a known restoration");
        assert_eq!(config.restoration, Some(Restoration::Extend));
        assert!(read("restore-lossy = \"magic\"").is_err());
    }

    #[test]
    fn a_filter_phase_is_spelt_the_way_its_flag_is() {
        let config = read("filter-phase = \"minimum\"").expect("a known phase");
        assert_eq!(config.filter_phase, Some(FilterPhase::Minimum));
        assert!(read("filter-phase = \"mixed\"").is_err());
    }

    #[test]
    fn clearing_a_setting_the_file_never_had_leaves_it_alone() {
        let scratch = Scratch::new().seed("quality = \"fast\"\n");
        clear(&scratch.path, ConfigKey::Sink).expect("a writable file");

        assert_eq!(scratch.text(), "quality = \"fast\"\n");
    }

    #[test]
    fn a_symlinked_file_is_written_through_the_link_and_keeps_its_mode() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let scratch = Scratch::new().seed("quality = \"fast\"\n");
        let folder = scratch.path.parent().expect("a folder").to_path_buf();
        let kept = folder.join("dotfiles.toml");
        fs::rename(&scratch.path, &kept).expect("the seed moves");
        fs::set_permissions(&kept, fs::Permissions::from_mode(0o600)).expect("a mode");
        symlink(&kept, &scratch.path).expect("a link");

        store(&scratch.path, ConfigKey::Dither, "none").expect("a writable file");

        assert!(
            fs::symlink_metadata(&scratch.path)
                .expect("the link stands")
                .file_type()
                .is_symlink(),
            "the link was replaced by a plain file"
        );
        assert!(
            fs::read_to_string(&kept)
                .expect("the target")
                .contains("dither")
        );
        assert_eq!(
            fs::metadata(&kept)
                .expect("the target")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let staged: Vec<_> = fs::read_dir(&folder)
            .expect("the folder")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(STAGING_SUFFIX)
            })
            .collect();
        assert!(staged.is_empty(), "a staging file was left behind");
    }

    #[test]
    fn writers_at_once_each_keep_what_they_wrote() {
        const WRITERS: usize = 4;
        const EACH: usize = 25;
        let scratch = Scratch::new().seed("");

        std::thread::scope(|scope| {
            for writer in 0..WRITERS {
                let path = &scratch.path;
                scope.spawn(move || {
                    for entry in 0..EACH {
                        store_in_table(
                            path,
                            ConfigKey::EqualiserFor,
                            &format!("sink-{writer}-{entry}"),
                            "kept",
                        )
                        .expect("a writable file");
                    }
                });
            }
        });

        let document = document(&scratch.path, &scratch.text()).expect("a well formed file");
        let held = document
            .get(ConfigKey::EqualiserFor.as_str())
            .and_then(Item::as_table_like)
            .map_or(0, |table| table.len());
        assert_eq!(held, WRITERS * EACH, "a write was lost to another");
    }

    #[test]
    fn clearing_a_setting_with_no_file_writes_nothing() {
        let scratch = Scratch::new();
        clear(&scratch.path, ConfigKey::Sink).expect("a missing file is nothing to clear");

        assert!(!scratch.path.exists());
    }

    #[test]
    fn a_binding_is_read_per_sink_with_a_fallback_for_the_rest() {
        let config = read(
            r#"
            equaliser = true
            equaliser-profile = "Harman over-ear"

            [equaliser-for]
            "alsa_output.usb-Topping_E30-00.analog-stereo" = "Sennheiser HD 650"
            "auto_null" = "  "
            "#,
        )
        .expect("a well formed document");

        assert_eq!(config.equaliser, Some(true));
        assert!(config.equaliser_on());

        let bindings = config.equaliser_for.expect("a table");
        assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
        assert_eq!(
            bound(
                &bindings,
                Some(&NodeName::new(
                    "alsa_output.usb-Topping_E30-00.analog-stereo"
                ))
            ),
            Some(&kept("Sennheiser HD 650"))
        );
        assert_eq!(
            bound(
                &bindings,
                Some(&NodeName::new("alsa_output.somewhere-else"))
            ),
            Some(&kept("Harman over-ear"))
        );
        assert_eq!(bound(&bindings, None), Some(&kept("Harman over-ear")));
        assert_eq!(bindings.by_sink().count(), 1);
    }

    #[test]
    fn a_sink_named_default_is_bound_on_its_own_rather_than_for_the_rest() {
        let config = read(
            r#"
            equaliser-profile = "Harman over-ear"

            [equaliser-for]
            default = "Sennheiser HD 650"
            "#,
        )
        .expect("a well formed document");

        let bindings = config.equaliser_for.expect("a table");
        assert_eq!(
            bound(&bindings, Some(&NodeName::new("default"))),
            Some(&kept("Sennheiser HD 650")),
            "a device named default was read as the fallback"
        );
        assert_eq!(bound(&bindings, None), Some(&kept("Harman over-ear")));
        assert_eq!(bindings.by_sink().count(), 1);
    }

    #[test]
    fn a_fallback_and_the_devices_are_read_whichever_order_they_are_written_in() {
        let profile_first = read(
            r#"
            equaliser-profile = "Harman over-ear"
            equaliser-for = { "alsa_output.one" = "HD 650" }
            "#,
        )
        .expect("a well formed document");
        let devices_first = read(
            r#"
            equaliser-for = { "alsa_output.one" = "HD 650" }
            equaliser-profile = "Harman over-ear"
            "#,
        )
        .expect("a well formed document");

        for config in [profile_first, devices_first] {
            let bindings = config.equaliser_for.expect("a table");
            assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
            assert_eq!(
                bound(&bindings, Some(&NodeName::new("alsa_output.one"))),
                Some(&kept("HD 650"))
            );
        }
    }

    #[test]
    fn a_build_that_says_nothing_about_the_equaliser_leaves_it_off_and_unbound() {
        let config = read("").expect("empty is valid");

        assert_eq!(config.equaliser, None);
        assert!(!config.equaliser_on());
        assert_eq!(config.equaliser_for, None);
    }

    #[test]
    fn a_binding_that_is_not_a_table_names_the_key_and_the_type_it_wanted() {
        let error = read("equaliser-for = \"harman\"").expect_err("a string is not a table");

        assert!(
            matches!(
                error,
                Error::ConfigType {
                    key: ConfigKey::EqualiserFor,
                    expected: ValueKind::Table,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_binding_written_into_the_table_keeps_the_rest_of_the_file() {
        let scratch = Scratch::new().seed("# my listening setup\nquality = \"fast\"\n");
        let sink = "alsa_output.pci-0000_00_1f.3.analog-stereo";

        store_in_table(
            &scratch.path,
            ConfigKey::EqualiserFor,
            sink,
            "Sennheiser HD 650",
        )
        .expect("a writable file");
        store(
            &scratch.path,
            ConfigKey::EqualiserProfile,
            "Harman over-ear",
        )
        .expect("a writable file");

        let text = scratch.text();
        assert!(text.contains("# my listening setup"), "{text}");
        assert!(text.contains("[equaliser-for]"), "{text}");
        assert!(
            text.contains(&format!("\"{sink}\" = \"Sennheiser HD 650\"")),
            "a dotted node name was written as a path rather than one key: {text}"
        );

        let config = load(Some(&scratch.path)).expect("it reads back");
        let bindings = config.equaliser_for.expect("a table");
        assert_eq!(
            bound(&bindings, Some(&NodeName::new(sink))),
            Some(&kept("Sennheiser HD 650"))
        );
        assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
    }

    fn kept(name: &str) -> Binding {
        Binding::Profile(ProfileName::new(name).expect("a usable name"))
    }

    fn bound<'b>(bindings: &'b Bindings, sink: Option<&NodeName>) -> Option<&'b Binding> {
        bindings.for_sink(sink).map(|(_, binding)| binding)
    }

    #[test]
    fn a_device_bound_to_its_own_curve_is_answered_as_its_own_and_not_the_fallbacks() {
        let config = read(
            r#"
            equaliser-profile = true

            [equaliser-for]
            "bluez_output.headphones" = true
            "alsa_output.one" = "HD 650"
            "alsa_output.two" = false
            "#,
        )
        .expect("a well formed document");

        let bindings = config.equaliser_for.expect("a table");
        let headphones = NodeName::new("bluez_output.headphones");
        assert_eq!(
            bindings.for_sink(Some(&headphones)),
            Some((Some(&headphones), &Binding::Own))
        );
        assert_eq!(
            bindings.for_sink(Some(&NodeName::new("alsa_output.elsewhere"))),
            Some((None, &Binding::Own)),
            "a device naming nothing was handed a curve of its own rather than the fallback's"
        );
        assert_eq!(
            bound(&bindings, Some(&NodeName::new("alsa_output.one"))),
            Some(&kept("HD 650"))
        );
        assert_eq!(bindings.by_sink().count(), 2, "false was read as a binding");
    }

    #[test]
    fn a_fallback_that_is_false_is_refused_rather_than_read_as_nothing() {
        let error = read("equaliser-profile = false").expect_err("false names nothing");

        assert!(
            matches!(
                error,
                Error::ConfigValue {
                    key: ConfigKey::EqualiserProfile,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn an_own_curve_written_into_the_file_reads_back_as_one() {
        let scratch = Scratch::new();
        let sink = "alsa_output.pci-0000_00_1f.3.analog-stereo";

        store_in_table(
            &scratch.path,
            ConfigKey::EqualiserFor,
            sink,
            written(&Binding::Own),
        )
        .expect("a writable file");
        store(
            &scratch.path,
            ConfigKey::EqualiserProfile,
            written(&kept("Harman over-ear")),
        )
        .expect("a writable file");

        let text = scratch.text();
        assert!(text.contains(&format!("\"{sink}\" = true")), "{text}");

        let bindings = load(Some(&scratch.path))
            .expect("it reads back")
            .equaliser_for
            .expect("a table");
        assert_eq!(
            bound(&bindings, Some(&NodeName::new(sink))),
            Some(&Binding::Own)
        );
        assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
    }

    #[test]
    fn clearing_the_last_binding_takes_the_table_away_with_it() {
        let scratch = Scratch::new();
        store_in_table(&scratch.path, ConfigKey::EqualiserFor, "one", "a").expect("writable");
        store_in_table(&scratch.path, ConfigKey::EqualiserFor, "two", "b").expect("writable");

        clear_in_table(&scratch.path, ConfigKey::EqualiserFor, "one").expect("writable");
        assert!(
            scratch.text().contains("[equaliser-for]"),
            "{}",
            scratch.text()
        );

        clear_in_table(&scratch.path, ConfigKey::EqualiserFor, "two").expect("writable");
        assert!(
            !scratch.text().contains("equaliser-for"),
            "an empty table was left behind: {}",
            scratch.text()
        );

        clear_in_table(&scratch.path, ConfigKey::EqualiserFor, "two")
            .expect("nothing to clear is not a failure");
    }

    #[test]
    fn a_table_written_over_a_value_that_is_not_one_refuses_rather_than_destroying_it() {
        let scratch = Scratch::new().seed("equaliser-for = \"harman\"\n");

        assert!(matches!(
            store_in_table(&scratch.path, ConfigKey::EqualiserFor, "one", "a"),
            Err(Error::ConfigType { .. })
        ));
        assert_eq!(scratch.text(), "equaliser-for = \"harman\"\n");
    }

    #[test]
    fn a_layout_the_parser_cannot_read_names_the_organise_as_key() {
        for text in [
            "organise-as = \"{album}/{genre}\"",
            "organise-as = \"{album}/{title\"",
            "organise-as = \"../{album}/{title}\"",
            "organise-as = \"\"",
        ] {
            let error = read(text).expect_err("a template the reader refuses");
            assert!(
                matches!(
                    error,
                    Error::ConfigValue {
                        key: ConfigKey::OrganiseAs,
                        ..
                    }
                ),
                "{text}: {error:?}"
            );
        }

        let error = read("organise-as = 3").expect_err("an integer is not a template");
        assert!(
            matches!(
                error,
                Error::ConfigType {
                    key: ConfigKey::OrganiseAs,
                    expected: ValueKind::Text,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_file_that_names_no_layout_organises_as_this_build_ships() {
        let config = read("volume = 0.5").expect("a well formed document");

        assert_eq!(config.organise_as, None);
        assert_eq!(config.organise_as(), Layout::default());

        let named = read("organise-as = \"{artist}/{year} {album}/{track} {title}\"")
            .expect("a well formed document");

        assert_eq!(
            named.organise_as(),
            Layout::read("{artist}/{year} {album}/{track} {title}")
                .expect("a layout this build writes is one it reads")
        );
        assert_ne!(named.organise_as(), Layout::default());
    }

    #[test]
    fn malformed_toml_is_reported_against_its_path() {
        let error = read("quality = ").expect_err("truncated");
        assert!(matches!(error, Error::ConfigSyntax { .. }), "{error:?}");
    }
}
