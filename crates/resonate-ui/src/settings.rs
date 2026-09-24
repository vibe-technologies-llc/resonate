use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use resonate_core::{
    Accent, AppId, Icon, Pictured, Presence, ScrollbarMode, Shown, TextSize, Theme, Trim, Volume,
};
use resonate_engine::{
    DitherKind, FilterPhase, NodeName, NoiseShaping, Quality, ReplayGainMode, Restoration,
    SkipUnderRepeat,
};
use resonate_eq::Binding;
use resonate_listen::Listening;
use resonate_providers::Providers;

use crate::{Launcher, Result, equaliser::Curve};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingKey {
    Sink,
    Quality,
    FilterPhase,
    TruePeak,
    Restoration,
    Dither,
    NoiseShaping,
    ReplayGain,
    PreAmp,
    Untagged,
    BitPerfect,
    Dop,
    ForceGraphRate,
    BluetoothWake,
    BluetoothLead,
    BluetoothAwake,
    Buffer,
    Volume,
    Theme,
    Accent,
    TextSize,
    Online,
    EnrichAfterScan,
    Study,
    Contact,
    AcoustidKey,
    AuddToken,
    ListenbrainzToken,
    ListenFrom,
    ListenFor,
    Equaliser,
    EqualiserFor,
    EqualiserProfile,
    Resume,
    SkipUnderRepeat,
    OrganiseAs,
    Notify,
    MinimiseButton,
    MaximiseButton,
    ScrollVolume,
    Scrollbars,
    SuggestionsTab,
    MissingTab,
    TabCounts,
    Inbox,
    Discord,
    DiscordApp,
    DiscordShows,
    DiscordArt,
    DiscordIcon,
    DiscordProgress,
    DiscordPaused,
}

impl SettingKey {
    pub const ALL: [Self; 52] = [
        Self::Sink,
        Self::Quality,
        Self::FilterPhase,
        Self::TruePeak,
        Self::Restoration,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::PreAmp,
        Self::Untagged,
        Self::BitPerfect,
        Self::Dop,
        Self::ForceGraphRate,
        Self::BluetoothWake,
        Self::BluetoothLead,
        Self::BluetoothAwake,
        Self::Buffer,
        Self::Volume,
        Self::Theme,
        Self::Accent,
        Self::TextSize,
        Self::Online,
        Self::EnrichAfterScan,
        Self::Study,
        Self::Contact,
        Self::AcoustidKey,
        Self::AuddToken,
        Self::ListenbrainzToken,
        Self::ListenFrom,
        Self::ListenFor,
        Self::Equaliser,
        Self::EqualiserFor,
        Self::EqualiserProfile,
        Self::Resume,
        Self::SkipUnderRepeat,
        Self::OrganiseAs,
        Self::Notify,
        Self::MinimiseButton,
        Self::MaximiseButton,
        Self::ScrollVolume,
        Self::Scrollbars,
        Self::SuggestionsTab,
        Self::MissingTab,
        Self::TabCounts,
        Self::Inbox,
        Self::Discord,
        Self::DiscordApp,
        Self::DiscordShows,
        Self::DiscordArt,
        Self::DiscordIcon,
        Self::DiscordProgress,
        Self::DiscordPaused,
    ];
}

#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Sink(Option<NodeName>),
    Quality(Quality),
    FilterPhase(FilterPhase),
    TruePeak(bool),
    Restoration(Restoration),
    Dither(DitherKind),
    NoiseShaping(NoiseShaping),
    ReplayGain(ReplayGainMode),
    PreAmp(Trim),
    Untagged(Trim),
    BitPerfect(bool),
    Dop(bool),
    ForceGraphRate(bool),
    BluetoothWake(bool),
    BluetoothLead(Duration),
    BluetoothAwake(Duration),
    Buffer(Duration),
    Volume(Volume),
    Theme(Theme),
    Accent(Option<Accent>),
    TextSize(TextSize),
    Online(bool),
    EnrichAfterScan(bool),
    Study(bool),
    Contact(String),
    AcoustidKey(String),
    AuddToken(String),
    ListenbrainzToken(String),
    ListenFrom(Listening),
    ListenFor(Duration),
    Equaliser(bool),
    EqualiserFor {
        sink: NodeName,
        binding: Option<Binding>,
    },
    EqualiserProfile(Option<Binding>),
    Resume(bool),
    SkipUnderRepeat(SkipUnderRepeat),
    OrganiseAs(String),
    Notify(bool),
    MinimiseButton(bool),
    MaximiseButton(bool),
    ScrollVolume(bool),
    Scrollbars(ScrollbarMode),
    SuggestionsTab(bool),
    MissingTab(bool),
    TabCounts(bool),
    Inbox(PathBuf),
    Discord(bool),
    DiscordApp(Option<AppId>),
    DiscordShows(Shown),
    DiscordArt(Pictured),
    DiscordIcon(Option<Icon>),
    DiscordProgress(bool),
    DiscordPaused(bool),
}

impl Setting {
    pub const fn key(&self) -> SettingKey {
        match self {
            Self::Sink(_) => SettingKey::Sink,
            Self::Quality(_) => SettingKey::Quality,
            Self::FilterPhase(_) => SettingKey::FilterPhase,
            Self::TruePeak(_) => SettingKey::TruePeak,
            Self::Restoration(_) => SettingKey::Restoration,
            Self::Dither(_) => SettingKey::Dither,
            Self::NoiseShaping(_) => SettingKey::NoiseShaping,
            Self::ReplayGain(_) => SettingKey::ReplayGain,
            Self::PreAmp(_) => SettingKey::PreAmp,
            Self::Untagged(_) => SettingKey::Untagged,
            Self::BitPerfect(_) => SettingKey::BitPerfect,
            Self::Dop(_) => SettingKey::Dop,
            Self::ForceGraphRate(_) => SettingKey::ForceGraphRate,
            Self::BluetoothWake(_) => SettingKey::BluetoothWake,
            Self::BluetoothLead(_) => SettingKey::BluetoothLead,
            Self::BluetoothAwake(_) => SettingKey::BluetoothAwake,
            Self::Buffer(_) => SettingKey::Buffer,
            Self::Volume(_) => SettingKey::Volume,
            Self::Theme(_) => SettingKey::Theme,
            Self::Accent(_) => SettingKey::Accent,
            Self::TextSize(_) => SettingKey::TextSize,
            Self::Online(_) => SettingKey::Online,
            Self::EnrichAfterScan(_) => SettingKey::EnrichAfterScan,
            Self::Study(_) => SettingKey::Study,
            Self::Contact(_) => SettingKey::Contact,
            Self::AcoustidKey(_) => SettingKey::AcoustidKey,
            Self::AuddToken(_) => SettingKey::AuddToken,
            Self::ListenbrainzToken(_) => SettingKey::ListenbrainzToken,
            Self::ListenFrom(_) => SettingKey::ListenFrom,
            Self::ListenFor(_) => SettingKey::ListenFor,
            Self::Equaliser(_) => SettingKey::Equaliser,
            Self::EqualiserFor { .. } => SettingKey::EqualiserFor,
            Self::EqualiserProfile(_) => SettingKey::EqualiserProfile,
            Self::Resume(_) => SettingKey::Resume,
            Self::SkipUnderRepeat(_) => SettingKey::SkipUnderRepeat,
            Self::OrganiseAs(_) => SettingKey::OrganiseAs,
            Self::Notify(_) => SettingKey::Notify,
            Self::MinimiseButton(_) => SettingKey::MinimiseButton,
            Self::MaximiseButton(_) => SettingKey::MaximiseButton,
            Self::ScrollVolume(_) => SettingKey::ScrollVolume,
            Self::Scrollbars(_) => SettingKey::Scrollbars,
            Self::SuggestionsTab(_) => SettingKey::SuggestionsTab,
            Self::MissingTab(_) => SettingKey::MissingTab,
            Self::TabCounts(_) => SettingKey::TabCounts,
            Self::Inbox(_) => SettingKey::Inbox,
            Self::Discord(_) => SettingKey::Discord,
            Self::DiscordApp(_) => SettingKey::DiscordApp,
            Self::DiscordShows(_) => SettingKey::DiscordShows,
            Self::DiscordArt(_) => SettingKey::DiscordArt,
            Self::DiscordIcon(_) => SettingKey::DiscordIcon,
            Self::DiscordProgress(_) => SettingKey::DiscordProgress,
            Self::DiscordPaused(_) => SettingKey::DiscordPaused,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Online {
    pub enabled: bool,
    pub after_scan: bool,
    pub studies: bool,
    pub contact: String,
    pub acoustid_key: String,
    pub audd_token: String,
    pub listenbrainz_token: String,
}

#[derive(Clone)]
pub struct Sourcing {
    pub inbox: Option<PathBuf>,
    pub register: fn(Option<&Path>) -> Providers,
}

impl Sourcing {
    pub fn providers(&self) -> Providers {
        (self.register)(self.inbox.as_deref())
    }
}

impl Default for Sourcing {
    fn default() -> Self {
        Self {
            inbox: None,
            register: |_| Providers::none(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings {
    pub enabled: bool,
    pub fallback: Option<Binding>,
    pub by_sink: Vec<(NodeName, Binding)>,
}

impl Bindings {
    fn of_the_device(&self, sink: &NodeName) -> Option<&Binding> {
        self.by_sink
            .iter()
            .find(|(held, _)| held == sink)
            .map(|(_, binding)| binding)
    }

    pub fn bound_to(&self, sink: Option<&NodeName>) -> Option<Curve> {
        let own = sink
            .and_then(|sink| {
                self.of_the_device(sink)
                    .map(|binding| (Some(sink), binding))
            })
            .or_else(|| self.fallback.as_ref().map(|binding| (None, binding)));
        own.map(|(owner, binding)| Curve::of(owner, binding))
    }

    pub fn named(&self, sink: Option<&NodeName>) -> Option<&Binding> {
        match sink {
            Some(sink) => self.of_the_device(sink),
            None => self.fallback.as_ref(),
        }
    }

    pub fn curves(&self) -> impl Iterator<Item = Curve> + '_ {
        self.by_sink
            .iter()
            .map(|(sink, binding)| Curve::of(Some(sink), binding))
            .chain(self.fallback.iter().map(|binding| Curve::of(None, binding)))
    }

    pub fn bind(&mut self, sink: Option<&NodeName>, binding: Option<Binding>) {
        match sink {
            Some(sink) => {
                self.by_sink.retain(|(held, _)| held != sink);
                if let Some(binding) = binding {
                    self.by_sink.push((sink.clone(), binding));
                }
                self.by_sink.sort_by(|one, other| one.0.cmp(&other.0));
            }
            None => self.fallback = binding,
        }
    }

    pub fn binds_anything(&self) -> bool {
        self.fallback.is_some() || !self.by_sink.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowButtons {
    pub minimise: bool,
    pub maximise: bool,
}

impl WindowButtons {
    pub const SHOWN: Self = Self {
        minimise: true,
        maximise: true,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tabs {
    pub suggestions: bool,
    pub missing: bool,
    pub counts: bool,
}

impl Tabs {
    pub const AS_BUILT: Self = Self {
        suggestions: true,
        missing: false,
        counts: true,
    };
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Places {
    pub config: PathBuf,
    pub library: PathBuf,
    pub equaliser: PathBuf,
}

pub struct Stored {
    pub settings: Arc<dyn Settings>,
    pub places: Places,
    pub resume: bool,
    pub organise_as: String,
    pub notify: Arc<AtomicBool>,
    pub window_buttons: WindowButtons,
    pub scroll_volume: bool,
    pub scrollbars: ScrollbarMode,
    pub tabs: Tabs,
    pub presence: Presence,
    pub present: Arc<dyn Present>,
    pub launcher: Arc<dyn Launcher>,
}

pub trait Present: Send + Sync {
    fn follow(&self, presence: &Presence);
}

pub trait Settings: Send + Sync {
    fn store(&self, setting: &Setting) -> Result<()>;

    fn forget(&self, key: SettingKey) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ephemeral;

impl Settings for Ephemeral {
    fn store(&self, _setting: &Setting) -> Result<()> {
        Ok(())
    }

    fn forget(&self, _key: SettingKey) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use resonate_eq::ProfileName;

    use super::*;

    fn kept(name: &str) -> Binding {
        Binding::Profile(ProfileName::new(name).expect("a usable name"))
    }

    #[test]
    fn a_device_given_its_own_curve_is_shown_its_own_and_every_other_the_fallbacks() {
        let headphones = NodeName::new("bluez_output.headphones");
        let speakers = NodeName::new("alsa_output.speakers");
        let mut bindings = Bindings::default();

        assert_eq!(bindings.bound_to(Some(&headphones)), None);

        bindings.bind(Some(&headphones), Some(Binding::Own));
        bindings.bind(None, Some(Binding::Own));
        assert_eq!(
            bindings.bound_to(Some(&headphones)),
            Some(Curve::Own(Some(headphones.clone())))
        );
        assert_eq!(bindings.bound_to(Some(&speakers)), Some(Curve::Own(None)));
        assert_eq!(bindings.bound_to(None), Some(Curve::Own(None)));

        bindings.bind(Some(&speakers), Some(kept("HD 650")));
        assert_eq!(
            bindings.bound_to(Some(&speakers)),
            Some(Curve::Kept(
                ProfileName::new("HD 650").expect("a usable name")
            ))
        );
        assert_eq!(bindings.named(Some(&headphones)), Some(&Binding::Own));

        let curves: Vec<Curve> = bindings.curves().collect();
        assert_eq!(curves.len(), 3);
        assert!(curves.contains(&Curve::Own(Some(headphones.clone()))));
        assert!(curves.contains(&Curve::Own(None)));

        bindings.bind(Some(&headphones), None);
        assert_eq!(bindings.bound_to(Some(&headphones)), Some(Curve::Own(None)));
    }
}
