use std::{cmp::Reverse, ops::Range, time::Duration};

use gpui::{Context, SharedString};
use resonate_core::{Accent, AlbumId, ArtistId, PlaylistId, Theme, ZoneId};
use resonate_engine::{NodeName, PlaybackState, RepeatMode, Until};
use resonate_eq::{Binding, ProfileName};
use resonate_library::{Album, Artist, Track};

use super::matching::{Scope, fuzzy_match};
use crate::{
    ResonateApp, app, format,
    icons::Icon,
    theme,
    views::{
        root::{Pane, RootView},
        settings::{Category, Group, Page},
    },
    words,
};

const SLEEP_MINUTES: [u64; 5] = [15, 30, 45, 60, 90];
const SECONDS_A_MINUTE: u64 = 60;
const ROOT_ENTRIES_AT_MOST: usize = 60;
const SETTINGS_MATCHES_AT_MOST: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Root,
    Themes,
    Accents,
    Outputs,
    Zones,
    Equaliser,
    Sleep,
}

impl Mode {
    pub(crate) fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Root => return None,
            Self::Themes => words::command_palette::mode_themes(),
            Self::Accents => words::command_palette::mode_accents(),
            Self::Outputs => words::command_palette::mode_outputs(),
            Self::Zones => words::command_palette::mode_zones(),
            Self::Equaliser => words::command_palette::mode_equaliser(),
            Self::Sleep => words::command_palette::mode_sleep(),
        })
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Root => Icon::Search,
            Self::Themes | Self::Accents => Icon::Appearance,
            Self::Outputs => Icon::Volume,
            Self::Zones => Icon::Server,
            Self::Equaliser => Icon::Equaliser,
            Self::Sleep => Icon::Sleep,
        }
    }

    pub(crate) fn placeholder(self) -> &'static str {
        match self {
            Self::Root => words::command_palette::placeholder(),
            Self::Themes => words::command_palette::placeholder_themes(),
            Self::Accents => words::command_palette::placeholder_accents(),
            Self::Outputs => words::command_palette::placeholder_outputs(),
            Self::Zones => words::command_palette::placeholder_zones(),
            Self::Equaliser => words::command_palette::placeholder_equaliser(),
            Self::Sleep => words::command_palette::placeholder_sleep(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SectionKind {
    Recent,
    Playback,
    Navigation,
    Audio,
    Appearance,
    Library,
    Settings,
    Playlists,
    Artists,
    Albums,
    Tracks,
}

impl SectionKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Recent => words::command_palette::section_recent(),
            Self::Playback => words::command_palette::section_playback(),
            Self::Navigation => words::command_palette::section_navigation(),
            Self::Audio => words::command_palette::section_audio(),
            Self::Appearance => words::command_palette::section_appearance(),
            Self::Library => words::command_palette::section_library(),
            Self::Settings => words::command_palette::section_settings(),
            Self::Playlists => words::command_palette::section_playlists(),
            Self::Artists => words::command_palette::section_artists(),
            Self::Albums => words::command_palette::section_albums(),
            Self::Tracks => words::command_palette::section_tracks(),
        }
    }

    const fn matches_scope(self, scope: Scope) -> bool {
        match scope {
            Scope::All => true,
            Scope::Commands => matches!(
                self,
                Self::Recent
                    | Self::Playback
                    | Self::Navigation
                    | Self::Audio
                    | Self::Appearance
                    | Self::Library
            ),
            Scope::Settings => matches!(self, Self::Settings),
            Scope::Music => matches!(self, Self::Artists | Self::Albums | Self::Tracks),
            Scope::Playlists => matches!(self, Self::Playlists),
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum CommandKey {
    TogglePlayPause,
    Next,
    Previous,
    Stop,
    ToggleShuffle,
    CycleRepeat,
    ToggleMute,
    VolumeUp,
    VolumeDown,
    SeekForward,
    SeekBack,
    SeekFurtherForward,
    SeekFurtherBack,
    ClearQueue,
    ToggleFavouritePlaying,
    GoToPlayingAlbum,
    GoToPlayingArtist,
    OpenListen,
    OpenSleep,
    FocusSearch,
    GoBack,
    GoForward,
    GoToPane(Pane),
    OpenOutputs,
    OpenZones,
    AddZone,
    ToggleEqualiser,
    OpenEqualiser,
    OpenThemes,
    OpenAccents,
    ToggleReducedMotion,
    ToggleFullScreen,
    ScanLibrary,
    EnrichLibrary,
    OpenDownloads,
    OpenWrapped,
}

#[derive(Clone, Debug)]
pub(crate) enum Leading {
    Icon(Icon),
    ThemeSwatch(Theme),
    AccentDot(Option<Accent>),
}

#[derive(Clone, Debug)]
pub(crate) enum StatusBadge {
    ActiveCheck,
    State(SharedString),
    DrillDown,
}

#[derive(Clone, Debug)]
pub(crate) enum PaletteAction {
    TogglePlayPause,
    Next,
    Previous,
    Stop,
    ToggleShuffle,
    CycleRepeat,
    ToggleMute,
    VolumeStep(f32),
    SeekBy(i64),
    ClearQueue,
    ToggleFavouritePlaying,
    GoToPlayingAlbum(AlbumId),
    GoToPlayingArtist(ArtistId),
    OpenListen,
    FocusSearch,
    GoBack,
    GoForward,
    GoToPane(Pane),
    EnterMode(Mode),
    SelectSink(Option<NodeName>),
    SelectZone(ZoneId),
    MovePlaybackTo(ZoneId),
    AddZone,
    ToggleEqualiser,
    SelectEqProfile(Option<ProfileName>),
    SetSleep(Option<Until>),
    SelectTheme(Theme),
    SelectAccent(Option<Accent>),
    ToggleReducedMotion,
    ToggleFullScreen,
    ScanLibrary,
    EnrichLibrary,
    OpenDownloads,
    OpenWrapped,
    OpenSettings(Category, Option<Page>),
    OpenPlaylist(PlaylistId),
    OpenArtist(ArtistId),
    OpenAlbum(AlbumId),
    PlayTrack(Box<Track>),
}

#[derive(Clone, Debug)]
pub(crate) struct PaletteEntry {
    pub(crate) section: SectionKind,
    pub(crate) leading: Leading,
    pub(crate) title: SharedString,
    pub(crate) subtitle: Option<SharedString>,
    pub(crate) status: Option<StatusBadge>,
    pub(crate) shortcut: Option<&'static str>,
    pub(crate) action: PaletteAction,
    pub(crate) mru_key: Option<CommandKey>,
    pub(crate) highlights: Vec<Range<usize>>,
}

struct Candidate {
    section: SectionKind,
    leading: Leading,
    title: SharedString,
    subtitle: Option<SharedString>,
    keywords: String,
    status: Option<StatusBadge>,
    shortcut: Option<&'static str>,
    action: PaletteAction,
    mru_key: Option<CommandKey>,
    show_when_empty: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LibraryHits {
    pub(crate) query: String,
    pub(crate) loading: bool,
    pub(crate) artists: Vec<Artist>,
    pub(crate) albums: Vec<Album>,
    pub(crate) tracks: Vec<Track>,
}

pub(crate) fn entries_for(
    root: &RootView,
    mode: Mode,
    raw_query: &str,
    recent: &[CommandKey],
    hits: &LibraryHits,
    cx: &mut Context<RootView>,
) -> Vec<PaletteEntry> {
    let (scope, query) = if mode == Mode::Root {
        Scope::parse(raw_query)
    } else {
        (Scope::All, raw_query.trim())
    };

    let candidates = match mode {
        Mode::Root => root_candidates(root, scope, query, hits, cx),
        Mode::Themes => theme_candidates(),
        Mode::Accents => accent_candidates(),
        Mode::Outputs => output_candidates(root, cx),
        Mode::Zones => zone_candidates(root, cx),
        Mode::Equaliser => equaliser_candidates(root, cx),
        Mode::Sleep => sleep_candidates(root, cx),
    };

    rank_candidates(mode, scope, query, recent, candidates)
}

fn rank_candidates(
    mode: Mode,
    scope: Scope,
    query: &str,
    recent: &[CommandKey],
    candidates: Vec<Candidate>,
) -> Vec<PaletteEntry> {
    let empty = query.is_empty();
    let mut ranked: Vec<(bool, SectionKind, Reverse<i32>, usize, PaletteEntry)> = Vec::new();
    let mut settings_added = 0_usize;

    for (order, candidate) in candidates.into_iter().enumerate() {
        if mode == Mode::Root && !candidate.section.matches_scope(scope) {
            continue;
        }
        if empty && mode == Mode::Root && scope == Scope::All && !candidate.show_when_empty {
            continue;
        }

        let mru_rank = candidate
            .mru_key
            .and_then(|key| recent.iter().position(|recent_key| *recent_key == key));

        let Some(matched) = fuzzy_match(
            query,
            &candidate.title,
            candidate.subtitle.as_deref(),
            &candidate.keywords,
            mru_rank,
        ) else {
            continue;
        };

        if mode == Mode::Root && candidate.section == SectionKind::Settings {
            if settings_added >= SETTINGS_MATCHES_AT_MOST && scope != Scope::Settings {
                continue;
            }
            settings_added += 1;
        }

        let promote_to_recent =
            empty && mode == Mode::Root && scope == Scope::All && mru_rank.is_some();
        let section = if promote_to_recent {
            SectionKind::Recent
        } else {
            candidate.section
        };

        ranked.push((
            !empty,
            section,
            Reverse(matched.score),
            order,
            PaletteEntry {
                section,
                leading: candidate.leading,
                title: candidate.title,
                subtitle: candidate.subtitle,
                status: candidate.status,
                shortcut: candidate.shortcut,
                action: candidate.action,
                mru_key: candidate.mru_key,
                highlights: matched.highlights,
            },
        ));
    }

    if empty {
        ranked.sort_by_key(|(_, section, score, order, _)| (*section, *score, *order));
    } else {
        ranked.sort_by_key(|(_, section, score, order, _)| (*score, *section, *order));
    }

    let cap = if mode == Mode::Root {
        ROOT_ENTRIES_AT_MOST
    } else {
        usize::MAX
    };

    ranked
        .into_iter()
        .take(cap)
        .map(|(.., entry)| entry)
        .collect()
}

fn root_candidates(
    root: &RootView,
    scope: Scope,
    query: &str,
    hits: &LibraryHits,
    cx: &mut Context<RootView>,
) -> Vec<Candidate> {
    let mut list = Vec::new();
    let empty = query.is_empty();

    if matches!(scope, Scope::All | Scope::Commands) {
        add_playback_candidates(&mut list, root, cx);
        add_navigation_candidates(&mut list, root, cx);
        add_audio_candidates(&mut list, root, cx);
        add_appearance_candidates(&mut list, empty, cx);
        add_library_action_candidates(&mut list, root, cx);
    }

    if matches!(scope, Scope::All | Scope::Settings) {
        add_settings_candidates(&mut list, !empty || scope == Scope::Settings);
    }

    if matches!(scope, Scope::All | Scope::Playlists) {
        add_playlist_candidates(&mut list, root, cx, scope == Scope::Playlists || !empty);
    }

    if matches!(scope, Scope::All | Scope::Music) {
        add_library_hit_candidates(&mut list, hits);
    }

    list
}

fn add_playback_candidates(list: &mut Vec<Candidate>, root: &RootView, cx: &mut Context<RootView>) {
    let state = root.player.read(cx).state().clone();
    let playing = state.playback == PlaybackState::Playing;
    let queue_len = root.player.read(cx).queue().len();
    let muted = root.is_muted(cx);
    let playing_now = root.playing_now(cx);

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(if playing { Icon::Pause } else { Icon::Play }),
        title: SharedString::new_static(if playing {
            words::command_palette::cmd_pause()
        } else {
            words::command_palette::cmd_play()
        }),
        subtitle: None,
        keywords: words::command_palette::kw_playback().to_owned(),
        status: None,
        shortcut: Some(key!(play_pause)),
        action: PaletteAction::TogglePlayPause,
        mru_key: Some(CommandKey::TogglePlayPause),
        show_when_empty: true,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Next),
        title: SharedString::new_static(words::command_palette::cmd_next()),
        subtitle: None,
        keywords: words::command_palette::kw_next().to_owned(),
        status: None,
        shortcut: Some(key!(next)),
        action: PaletteAction::Next,
        mru_key: Some(CommandKey::Next),
        show_when_empty: true,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Previous),
        title: SharedString::new_static(words::command_palette::cmd_previous()),
        subtitle: None,
        keywords: words::command_palette::kw_previous().to_owned(),
        status: None,
        shortcut: Some(key!(previous)),
        action: PaletteAction::Previous,
        mru_key: Some(CommandKey::Previous),
        show_when_empty: true,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Stop),
        title: SharedString::new_static(words::command_palette::cmd_stop()),
        subtitle: None,
        keywords: words::command_palette::kw_stop().to_owned(),
        status: None,
        shortcut: Some(key!(stop)),
        action: PaletteAction::Stop,
        mru_key: Some(CommandKey::Stop),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Shuffle),
        title: SharedString::new_static(words::command_palette::cmd_shuffle()),
        subtitle: None,
        keywords: words::command_palette::kw_shuffle().to_owned(),
        status: Some(StatusBadge::State(SharedString::new_static(
            if state.shuffle {
                words::command_palette::state_on()
            } else {
                words::command_palette::state_off()
            },
        ))),
        shortcut: Some(key!(shuffle)),
        action: PaletteAction::ToggleShuffle,
        mru_key: Some(CommandKey::ToggleShuffle),
        show_when_empty: true,
    });

    let repeat_label = match state.repeat {
        RepeatMode::Off => words::command_palette::state_repeat_off(),
        RepeatMode::Queue => words::command_palette::state_repeat_queue(),
        RepeatMode::Track => words::command_palette::state_repeat_track(),
    };

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(if state.repeat == RepeatMode::Track {
            Icon::RepeatTrack
        } else {
            Icon::Repeat
        }),
        title: SharedString::new_static(words::command_palette::cmd_repeat()),
        subtitle: None,
        keywords: words::command_palette::kw_repeat().to_owned(),
        status: Some(StatusBadge::State(SharedString::new_static(repeat_label))),
        shortcut: Some(key!(repeat)),
        action: PaletteAction::CycleRepeat,
        mru_key: Some(CommandKey::CycleRepeat),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(if muted { Icon::Muted } else { Icon::Volume }),
        title: SharedString::new_static(words::command_palette::cmd_mute()),
        subtitle: None,
        keywords: words::command_palette::kw_mute().to_owned(),
        status: muted.then(|| {
            StatusBadge::State(SharedString::new_static(
                words::command_palette::state_muted(),
            ))
        }),
        shortcut: Some(key!(mute)),
        action: PaletteAction::ToggleMute,
        mru_key: Some(CommandKey::ToggleMute),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Volume),
        title: SharedString::new_static(words::command_palette::cmd_volume_up()),
        subtitle: None,
        keywords: words::command_palette::kw_volume().to_owned(),
        status: None,
        shortcut: Some(key!(louder)),
        action: PaletteAction::VolumeStep(crate::views::root::VOLUME_STEP),
        mru_key: Some(CommandKey::VolumeUp),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Volume),
        title: SharedString::new_static(words::command_palette::cmd_volume_down()),
        subtitle: None,
        keywords: words::command_palette::kw_volume().to_owned(),
        status: None,
        shortcut: Some(key!(quieter)),
        action: PaletteAction::VolumeStep(-crate::views::root::VOLUME_STEP),
        mru_key: Some(CommandKey::VolumeDown),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Next),
        title: SharedString::new_static(words::command_palette::cmd_seek_forward()),
        subtitle: None,
        keywords: words::command_palette::kw_seek().to_owned(),
        status: None,
        shortcut: Some(key!(seek)),
        action: PaletteAction::SeekBy(app::seek_step()),
        mru_key: Some(CommandKey::SeekForward),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Previous),
        title: SharedString::new_static(words::command_palette::cmd_seek_back()),
        subtitle: None,
        keywords: words::command_palette::kw_seek().to_owned(),
        status: None,
        shortcut: Some(key!(seek_back)),
        action: PaletteAction::SeekBy(-app::seek_step()),
        mru_key: Some(CommandKey::SeekBack),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Next),
        title: SharedString::new_static(words::command_palette::cmd_seek_further_forward()),
        subtitle: None,
        keywords: words::command_palette::kw_seek().to_owned(),
        status: None,
        shortcut: Some(key!(seek_further)),
        action: PaletteAction::SeekBy(app::seek_further()),
        mru_key: Some(CommandKey::SeekFurtherForward),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Previous),
        title: SharedString::new_static(words::command_palette::cmd_seek_further_back()),
        subtitle: None,
        keywords: words::command_palette::kw_seek().to_owned(),
        status: None,
        shortcut: Some(key!(seek_further_back)),
        action: PaletteAction::SeekBy(-app::seek_further()),
        mru_key: Some(CommandKey::SeekFurtherBack),
        show_when_empty: false,
    });

    if queue_len > 0 {
        list.push(Candidate {
            section: SectionKind::Playback,
            leading: Leading::Icon(Icon::Discard),
            title: SharedString::new_static(words::command_palette::cmd_clear_queue()),
            subtitle: Some(SharedString::from(words::command_palette::subtitle_tracks(
                queue_len,
            ))),
            keywords: words::command_palette::kw_queue().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::ClearQueue,
            mru_key: Some(CommandKey::ClearQueue),
            show_when_empty: false,
        });
    }

    if playing_now.track.is_some() {
        let favoured = playing_now.favourite;

        list.push(Candidate {
            section: SectionKind::Playback,
            leading: Leading::Icon(if favoured {
                Icon::Favourited
            } else {
                Icon::Favourite
            }),
            title: SharedString::new_static(if favoured {
                words::command_palette::cmd_unfavour_playing()
            } else {
                words::command_palette::cmd_favour_playing()
            }),
            subtitle: None,
            keywords: words::command_palette::kw_favourite().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::ToggleFavouritePlaying,
            mru_key: Some(CommandKey::ToggleFavouritePlaying),
            show_when_empty: false,
        });
    }

    if let Some(album) = playing_now.album {
        list.push(Candidate {
            section: SectionKind::Playback,
            leading: Leading::Icon(Icon::Albums),
            title: SharedString::new_static(words::command_palette::cmd_go_to_playing_album()),
            subtitle: None,
            keywords: words::command_palette::kw_playing_album().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::GoToPlayingAlbum(album),
            mru_key: Some(CommandKey::GoToPlayingAlbum),
            show_when_empty: false,
        });
    }

    if let Some(artist) = playing_now.artist {
        list.push(Candidate {
            section: SectionKind::Playback,
            leading: Leading::Icon(Icon::Artists),
            title: SharedString::new_static(words::command_palette::cmd_go_to_playing_artist()),
            subtitle: None,
            keywords: words::command_palette::kw_playing_artist().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::GoToPlayingArtist(artist),
            mru_key: Some(CommandKey::GoToPlayingArtist),
            show_when_empty: false,
        });
    }

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Listen),
        title: SharedString::new_static(words::command_palette::cmd_listen()),
        subtitle: None,
        keywords: words::command_palette::kw_listen().to_owned(),
        status: None,
        shortcut: Some(key!(listen)),
        action: PaletteAction::OpenListen,
        mru_key: Some(CommandKey::OpenListen),
        show_when_empty: false,
    });

    let sleep_status = state.sleeping.map(|asleep| {
        let reading = match asleep.until {
            Until::After(_) => {
                SharedString::from(format::counting_down(asleep.left.unwrap_or_default()))
            }
            Until::EndOfTrack => SharedString::new_static(words::transport::end_of_track_reading()),
            Until::EndOfQueue => SharedString::new_static(words::transport::end_of_queue_reading()),
        };
        StatusBadge::State(reading)
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Sleep),
        title: SharedString::new_static(words::command_palette::cmd_sleep_timer()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_sleep(),
        )),
        keywords: words::command_palette::kw_sleep().to_owned(),
        status: sleep_status.or(Some(StatusBadge::DrillDown)),
        shortcut: None,
        action: PaletteAction::EnterMode(Mode::Sleep),
        mru_key: Some(CommandKey::OpenSleep),
        show_when_empty: false,
    });
}

fn add_navigation_candidates(list: &mut Vec<Candidate>, root: &RootView, cx: &Context<RootView>) {
    let tabs = cx.global::<ResonateApp>().tabs;
    let serving = root.server.read(cx).is_open();
    let in_front = root.in_front(cx);

    list.push(Candidate {
        section: SectionKind::Navigation,
        leading: Leading::Icon(Icon::Search),
        title: SharedString::new_static(words::command_palette::cmd_focus_search()),
        subtitle: None,
        keywords: words::command_palette::kw_search().to_owned(),
        status: None,
        shortcut: Some(key!(find)),
        action: PaletteAction::FocusSearch,
        mru_key: Some(CommandKey::FocusSearch),
        show_when_empty: false,
    });

    if root.can_go_back() {
        list.push(Candidate {
            section: SectionKind::Navigation,
            leading: Leading::Icon(Icon::Back),
            title: SharedString::new_static(words::command_palette::cmd_go_back()),
            subtitle: None,
            keywords: words::command_palette::kw_history().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::GoBack,
            mru_key: Some(CommandKey::GoBack),
            show_when_empty: false,
        });
    }

    if root.can_go_forward() {
        list.push(Candidate {
            section: SectionKind::Navigation,
            leading: Leading::Icon(Icon::ChevronRight),
            title: SharedString::new_static(words::command_palette::cmd_go_forward()),
            subtitle: None,
            keywords: words::command_palette::kw_history().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::GoForward,
            mru_key: Some(CommandKey::GoForward),
            show_when_empty: false,
        });
    }

    for pane in Pane::ALL {
        if !pane.is_there(tabs, serving) {
            continue;
        }

        let shortcut = match pane {
            Pane::Queue => Some(key!(queue)),
            Pane::Settings => Some(key!(find_a_setting)),
            _ => None,
        };
        let show_when_empty = matches!(
            pane,
            Pane::Albums
                | Pane::Artists
                | Pane::Tracks
                | Pane::Queue
                | Pane::Lyrics
                | Pane::Settings
        );

        list.push(Candidate {
            section: SectionKind::Navigation,
            leading: Leading::Icon(pane.icon()),
            title: SharedString::new_static(pane.label()),
            subtitle: Some(SharedString::new_static(pane.about())),
            keywords: pane.as_str().to_owned(),
            status: (pane == in_front).then(|| {
                StatusBadge::State(SharedString::new_static(
                    words::command_palette::state_current(),
                ))
            }),
            shortcut,
            action: PaletteAction::GoToPane(pane),
            mru_key: Some(CommandKey::GoToPane(pane)),
            show_when_empty,
        });
    }
}

fn add_audio_candidates(list: &mut Vec<Candidate>, root: &RootView, cx: &Context<RootView>) {
    let player = root.player.read(cx);
    let current_sink = player
        .output_settings()
        .sink
        .as_ref()
        .and_then(|named| {
            player
                .sinks()
                .iter()
                .find(|sink| sink.is_named(named))
                .map(|sink| sink.description.clone())
        })
        .unwrap_or_else(|| words::command_palette::cmd_system_default_output().to_owned());

    list.push(Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Volume),
        title: SharedString::new_static(words::command_palette::cmd_switch_output()),
        subtitle: Some(SharedString::from(current_sink)),
        keywords: words::command_palette::kw_output().to_owned(),
        status: Some(StatusBadge::DrillDown),
        shortcut: None,
        action: PaletteAction::EnterMode(Mode::Outputs),
        mru_key: Some(CommandKey::OpenOutputs),
        show_when_empty: true,
    });

    let active_zone = root
        .switcher
        .name_of(root.switcher.selected())
        .map(|name| SharedString::from(name.as_str().to_owned()));

    list.push(Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Server),
        title: SharedString::new_static(words::command_palette::cmd_switch_zone()),
        subtitle: active_zone,
        keywords: words::command_palette::kw_zone().to_owned(),
        status: Some(StatusBadge::DrillDown),
        shortcut: None,
        action: PaletteAction::EnterMode(Mode::Zones),
        mru_key: Some(CommandKey::OpenZones),
        show_when_empty: root.switcher.several(),
    });

    list.push(Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Plus),
        title: SharedString::new_static(words::command_palette::cmd_add_zone()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_zone(),
        )),
        keywords: words::command_palette::kw_zone().to_owned(),
        status: None,
        shortcut: None,
        action: PaletteAction::AddZone,
        mru_key: Some(CommandKey::AddZone),
        show_when_empty: false,
    });

    let eq_on = root.equaliser.read(cx).bindings().enabled;

    list.push(Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Equaliser),
        title: SharedString::new_static(words::command_palette::cmd_toggle_equaliser()),
        subtitle: None,
        keywords: words::command_palette::kw_equaliser().to_owned(),
        status: Some(StatusBadge::State(SharedString::new_static(if eq_on {
            words::command_palette::state_on()
        } else {
            words::command_palette::state_off()
        }))),
        shortcut: None,
        action: PaletteAction::ToggleEqualiser,
        mru_key: Some(CommandKey::ToggleEqualiser),
        show_when_empty: true,
    });

    list.push(Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Equaliser),
        title: SharedString::new_static(words::command_palette::cmd_switch_equaliser()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_equaliser(),
        )),
        keywords: words::command_palette::kw_equaliser().to_owned(),
        status: Some(StatusBadge::DrillDown),
        shortcut: None,
        action: PaletteAction::EnterMode(Mode::Equaliser),
        mru_key: Some(CommandKey::OpenEqualiser),
        show_when_empty: false,
    });
}

fn add_appearance_candidates(list: &mut Vec<Candidate>, empty: bool, cx: &Context<RootView>) {
    let worn = theme::worn();
    let reduced = cx.global::<ResonateApp>().reduced_motion;

    list.push(Candidate {
        section: SectionKind::Appearance,
        leading: Leading::ThemeSwatch(worn.theme),
        title: SharedString::new_static(words::command_palette::cmd_switch_theme()),
        subtitle: Some(SharedString::new_static(worn.theme.label())),
        keywords: words::command_palette::kw_theme().to_owned(),
        status: Some(StatusBadge::DrillDown),
        shortcut: None,
        action: PaletteAction::EnterMode(Mode::Themes),
        mru_key: Some(CommandKey::OpenThemes),
        show_when_empty: true,
    });

    list.push(Candidate {
        section: SectionKind::Appearance,
        leading: Leading::AccentDot(worn.accent),
        title: SharedString::new_static(words::command_palette::cmd_switch_accent()),
        subtitle: Some(SharedString::new_static(worn.accent.map_or_else(
            words::command_palette::cmd_palette_own_accent,
            Accent::label,
        ))),
        keywords: words::command_palette::kw_accent().to_owned(),
        status: Some(StatusBadge::DrillDown),
        shortcut: None,
        action: PaletteAction::EnterMode(Mode::Accents),
        mru_key: Some(CommandKey::OpenAccents),
        show_when_empty: false,
    });

    if !empty {
        for palette in Theme::ALL {
            list.push(Candidate {
                section: SectionKind::Appearance,
                leading: Leading::ThemeSwatch(palette),
                title: SharedString::new_static(palette.label()),
                subtitle: Some(SharedString::new_static(
                    words::command_palette::subtitle_theme(),
                )),
                keywords: format!(
                    "{} {}",
                    words::command_palette::kw_theme(),
                    palette.as_str()
                ),
                status: (worn.theme == palette).then_some(StatusBadge::ActiveCheck),
                shortcut: None,
                action: PaletteAction::SelectTheme(palette),
                mru_key: None,
                show_when_empty: false,
            });
        }
    }

    list.push(Candidate {
        section: SectionKind::Appearance,
        leading: Leading::Icon(Icon::Appearance),
        title: SharedString::new_static(words::command_palette::cmd_toggle_reduced_motion()),
        subtitle: None,
        keywords: words::command_palette::kw_motion().to_owned(),
        status: Some(StatusBadge::State(SharedString::new_static(if reduced {
            words::command_palette::state_on()
        } else {
            words::command_palette::state_off()
        }))),
        shortcut: None,
        action: PaletteAction::ToggleReducedMotion,
        mru_key: Some(CommandKey::ToggleReducedMotion),
        show_when_empty: false,
    });

    list.push(Candidate {
        section: SectionKind::Appearance,
        leading: Leading::Icon(Icon::Window),
        title: SharedString::new_static(words::command_palette::cmd_toggle_full_screen()),
        subtitle: None,
        keywords: words::command_palette::kw_fullscreen().to_owned(),
        status: None,
        shortcut: None,
        action: PaletteAction::ToggleFullScreen,
        mru_key: Some(CommandKey::ToggleFullScreen),
        show_when_empty: false,
    });
}

fn add_library_action_candidates(
    list: &mut Vec<Candidate>,
    root: &RootView,
    cx: &Context<RootView>,
) {
    let library = root.library.read(cx);
    let scanning = library.is_busy();
    let enriching = library.enrich_stats().is_some();
    let has_downloads = !library.downloads().is_empty() || library.keeps_left().is_some();
    let has_plays = root.statistics.read(cx).counts().plays > 0;

    list.push(Candidate {
        section: SectionKind::Library,
        leading: Leading::Icon(Icon::Redo),
        title: SharedString::new_static(words::command_palette::cmd_scan_library()),
        subtitle: None,
        keywords: words::command_palette::kw_scan().to_owned(),
        status: scanning.then(|| {
            StatusBadge::State(SharedString::new_static(
                words::command_palette::state_running(),
            ))
        }),
        shortcut: None,
        action: PaletteAction::ScanLibrary,
        mru_key: Some(CommandKey::ScanLibrary),
        show_when_empty: false,
    });

    if library.can_enrich() {
        list.push(Candidate {
            section: SectionKind::Library,
            leading: Leading::Icon(Icon::Globe),
            title: SharedString::new_static(words::command_palette::cmd_enrich_library()),
            subtitle: None,
            keywords: words::command_palette::kw_enrich().to_owned(),
            status: enriching.then(|| {
                StatusBadge::State(SharedString::new_static(
                    words::command_palette::state_running(),
                ))
            }),
            shortcut: None,
            action: PaletteAction::EnrichLibrary,
            mru_key: Some(CommandKey::EnrichLibrary),
            show_when_empty: false,
        });
    }

    if has_downloads {
        list.push(Candidate {
            section: SectionKind::Library,
            leading: Leading::Icon(Icon::Download),
            title: SharedString::new_static(words::command_palette::cmd_open_downloads()),
            subtitle: None,
            keywords: words::command_palette::kw_downloads().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenDownloads,
            mru_key: Some(CommandKey::OpenDownloads),
            show_when_empty: false,
        });
    }

    if has_plays {
        list.push(Candidate {
            section: SectionKind::Library,
            leading: Leading::Icon(Icon::Statistics),
            title: SharedString::new_static(words::command_palette::cmd_open_wrapped()),
            subtitle: None,
            keywords: words::command_palette::kw_wrapped().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenWrapped,
            mru_key: Some(CommandKey::OpenWrapped),
            show_when_empty: false,
        });
    }
}

fn add_settings_candidates(list: &mut Vec<Candidate>, include_groups: bool) {
    if !include_groups {
        return;
    }

    for category in Category::ALL {
        list.push(Candidate {
            section: SectionKind::Settings,
            leading: Leading::Icon(category.icon()),
            title: SharedString::new_static(category.label()),
            subtitle: Some(SharedString::new_static(category.about())),
            keywords: words::command_palette::section_settings().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenSettings(category, None),
            mru_key: None,
            show_when_empty: true,
        });
    }

    for group in Group::ALL {
        if !group.is_offered() {
            continue;
        }

        let category = group.category();
        let page = group.page();
        let subtitle = match page {
            Some(page) => SharedString::from(format!("{} › {}", category.label(), page.title())),
            None => SharedString::new_static(category.label()),
        };

        list.push(Candidate {
            section: SectionKind::Settings,
            leading: Leading::Icon(page.map_or_else(|| category.icon(), Page::icon)),
            title: SharedString::new_static(group.title()),
            subtitle: Some(subtitle),
            keywords: format!("{} {}", group.also_called(), group.hint()),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenSettings(category, page),
            mru_key: None,
            show_when_empty: false,
        });
    }
}

fn add_playlist_candidates(
    list: &mut Vec<Candidate>,
    root: &RootView,
    cx: &Context<RootView>,
    include: bool,
) {
    if !include {
        return;
    }
    for playlist in root.library.read(cx).playlists() {
        list.push(Candidate {
            section: SectionKind::Playlists,
            leading: Leading::Icon(Icon::Playlists),
            title: SharedString::from(playlist.name.clone()),
            subtitle: Some(SharedString::from(words::command_palette::subtitle_tracks(
                playlist.entries as usize,
            ))),
            keywords: String::new(),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenPlaylist(playlist.id),
            mru_key: None,
            show_when_empty: true,
        });
    }
}

fn add_library_hit_candidates(list: &mut Vec<Candidate>, hits: &LibraryHits) {
    for artist in &hits.artists {
        list.push(Candidate {
            section: SectionKind::Artists,
            leading: Leading::Icon(Icon::Artists),
            title: SharedString::from(artist.name.clone()),
            subtitle: Some(SharedString::from(words::command_palette::subtitle_albums(
                artist.album_count as usize,
            ))),
            keywords: String::new(),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenArtist(artist.id),
            mru_key: None,
            show_when_empty: false,
        });
    }

    for album in &hits.albums {
        let artist = album.artist.as_deref().unwrap_or_default();
        let subtitle = match album.year {
            Some(year) if !artist.is_empty() => format!("{artist} · {year}"),
            Some(year) => year.to_string(),
            None => artist.to_owned(),
        };

        list.push(Candidate {
            section: SectionKind::Albums,
            leading: Leading::Icon(Icon::Albums),
            title: SharedString::from(album.title.clone()),
            subtitle: (!subtitle.is_empty()).then(|| SharedString::from(subtitle)),
            keywords: artist.to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::OpenAlbum(album.id),
            mru_key: None,
            show_when_empty: false,
        });
    }

    for track in &hits.tracks {
        let length = track
            .duration
            .map(|frames| format::clock(frames, track.spec.rate));
        let subtitle = match (track.artist.as_deref(), length.as_deref()) {
            (Some(artist), Some(clock)) if !artist.is_empty() => {
                Some(SharedString::from(format!("{artist} · {clock}")))
            }
            (Some(artist), _) if !artist.is_empty() => Some(SharedString::from(artist.to_owned())),
            (_, Some(clock)) => Some(SharedString::from(clock.to_owned())),
            _ => None,
        };

        list.push(Candidate {
            section: SectionKind::Tracks,
            leading: Leading::Icon(Icon::Tracks),
            title: SharedString::from(track.title.clone()),
            subtitle,
            keywords: track.artist.clone().unwrap_or_default(),
            status: None,
            shortcut: None,
            action: PaletteAction::PlayTrack(Box::new(track.clone())),
            mru_key: None,
            show_when_empty: false,
        });
    }
}

fn theme_candidates() -> Vec<Candidate> {
    let worn = theme::worn().theme;
    Theme::ALL
        .into_iter()
        .map(|palette| Candidate {
            section: SectionKind::Appearance,
            leading: Leading::ThemeSwatch(palette),
            title: SharedString::new_static(palette.label()),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_theme(),
            )),
            keywords: palette.as_str().to_owned(),
            status: (worn == palette).then_some(StatusBadge::ActiveCheck),
            shortcut: None,
            action: PaletteAction::SelectTheme(palette),
            mru_key: None,
            show_when_empty: true,
        })
        .collect()
}

fn accent_candidates() -> Vec<Candidate> {
    let worn = theme::worn().accent;
    let mut list = vec![Candidate {
        section: SectionKind::Appearance,
        leading: Leading::AccentDot(None),
        title: SharedString::new_static(words::command_palette::cmd_palette_own_accent()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_accent(),
        )),
        keywords: String::new(),
        status: worn.is_none().then_some(StatusBadge::ActiveCheck),
        shortcut: None,
        action: PaletteAction::SelectAccent(None),
        mru_key: None,
        show_when_empty: true,
    }];

    for accent in Accent::ALL {
        list.push(Candidate {
            section: SectionKind::Appearance,
            leading: Leading::AccentDot(Some(accent)),
            title: SharedString::new_static(accent.label()),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_accent(),
            )),
            keywords: accent.as_str().to_owned(),
            status: (worn == Some(accent)).then_some(StatusBadge::ActiveCheck),
            shortcut: None,
            action: PaletteAction::SelectAccent(Some(accent)),
            mru_key: None,
            show_when_empty: true,
        });
    }
    list
}

fn output_candidates(root: &RootView, cx: &Context<RootView>) -> Vec<Candidate> {
    let player = root.player.read(cx);
    let chosen = player.output_settings().sink.clone();
    let mut list = vec![Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Volume),
        title: SharedString::new_static(words::command_palette::cmd_system_default_output()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_output(),
        )),
        keywords: words::command_palette::kw_output().to_owned(),
        status: chosen.is_none().then_some(StatusBadge::ActiveCheck),
        shortcut: None,
        action: PaletteAction::SelectSink(None),
        mru_key: None,
        show_when_empty: true,
    }];

    for sink in player.sinks() {
        let active = chosen.as_ref().is_some_and(|named| sink.is_named(named));
        let kind = if sink.is_hardware {
            words::command_palette::state_hardware()
        } else {
            words::command_palette::state_virtual()
        };

        list.push(Candidate {
            section: SectionKind::Audio,
            leading: Leading::Icon(Icon::Volume),
            title: SharedString::from(sink.description.clone()),
            subtitle: Some(SharedString::new_static(kind)),
            keywords: sink.name.as_str().to_owned(),
            status: active.then_some(StatusBadge::ActiveCheck),
            shortcut: None,
            action: PaletteAction::SelectSink(Some(sink.name.clone())),
            mru_key: None,
            show_when_empty: true,
        });
    }
    list
}

fn zone_candidates(root: &RootView, cx: &Context<RootView>) -> Vec<Candidate> {
    let selected = root.switcher.selected();
    let has_current = root.player.read(cx).state().current.is_some();
    let mut list = Vec::new();

    for zone in root.switcher.shown() {
        list.push(Candidate {
            section: SectionKind::Audio,
            leading: Leading::Icon(Icon::Server),
            title: SharedString::from(zone.name.as_str().to_owned()),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_zone(),
            )),
            keywords: words::command_palette::kw_zone().to_owned(),
            status: (zone.id == selected).then_some(StatusBadge::ActiveCheck),
            shortcut: None,
            action: PaletteAction::SelectZone(zone.id),
            mru_key: None,
            show_when_empty: true,
        });
    }

    if has_current {
        for zone in root.switcher.elsewhere() {
            list.push(Candidate {
                section: SectionKind::Audio,
                leading: Leading::Icon(Icon::Next),
                title: SharedString::from(words::command_palette::cmd_move_playback_to(
                    zone.name.as_str(),
                )),
                subtitle: Some(SharedString::new_static(
                    words::command_palette::subtitle_zone(),
                )),
                keywords: words::command_palette::kw_zone().to_owned(),
                status: None,
                shortcut: None,
                action: PaletteAction::MovePlaybackTo(zone.id),
                mru_key: None,
                show_when_empty: true,
            });
        }
    }

    list.push(Candidate {
        section: SectionKind::Audio,
        leading: Leading::Icon(Icon::Plus),
        title: SharedString::new_static(words::command_palette::cmd_add_zone()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_zone(),
        )),
        keywords: words::command_palette::kw_zone().to_owned(),
        status: None,
        shortcut: None,
        action: PaletteAction::AddZone,
        mru_key: None,
        show_when_empty: true,
    });

    list
}

fn equaliser_candidates(root: &RootView, cx: &Context<RootView>) -> Vec<Candidate> {
    let eq = root.equaliser.read(cx);
    let enabled = eq.bindings().enabled;
    let sink = root.player.read(cx).output_settings().sink.clone();

    let current = eq.bindings().named(sink.as_ref()).cloned();

    let mut list = vec![
        Candidate {
            section: SectionKind::Audio,
            leading: Leading::Icon(Icon::Equaliser),
            title: SharedString::new_static(words::command_palette::cmd_toggle_equaliser()),
            subtitle: None,
            keywords: words::command_palette::kw_equaliser().to_owned(),
            status: Some(StatusBadge::State(SharedString::new_static(if enabled {
                words::command_palette::state_on()
            } else {
                words::command_palette::state_off()
            }))),
            shortcut: None,
            action: PaletteAction::ToggleEqualiser,
            mru_key: None,
            show_when_empty: true,
        },
        Candidate {
            section: SectionKind::Audio,
            leading: Leading::Icon(Icon::Close),
            title: SharedString::new_static(words::command_palette::cmd_equaliser_none()),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_equaliser(),
            )),
            keywords: words::command_palette::kw_equaliser().to_owned(),
            status: current.is_none().then_some(StatusBadge::ActiveCheck),
            shortcut: None,
            action: PaletteAction::SelectEqProfile(None),
            mru_key: None,
            show_when_empty: true,
        },
    ];

    for profile in eq.kept() {
        let active = current
            .as_ref()
            .is_some_and(|bound| matches!(bound, Binding::Profile(name) if name == profile));

        list.push(Candidate {
            section: SectionKind::Audio,
            leading: Leading::Icon(Icon::Equaliser),
            title: SharedString::from(profile.as_str().to_owned()),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_equaliser(),
            )),
            keywords: words::command_palette::kw_equaliser().to_owned(),
            status: active.then_some(StatusBadge::ActiveCheck),
            shortcut: None,
            action: PaletteAction::SelectEqProfile(Some(profile.clone())),
            mru_key: None,
            show_when_empty: true,
        });
    }

    list
}

fn sleep_candidates(root: &RootView, cx: &Context<RootView>) -> Vec<Candidate> {
    let sleeping = root.player.read(cx).state().sleeping;
    let mut list = Vec::new();

    for minutes in SLEEP_MINUTES {
        let duration = Duration::from_secs(minutes * SECONDS_A_MINUTE);
        let active = sleeping.is_some_and(|asleep| asleep.until == Until::After(duration));

        list.push(Candidate {
            section: SectionKind::Playback,
            leading: Leading::Icon(Icon::Sleep),
            title: SharedString::from(words::transport::minutes(minutes)),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_sleep(),
            )),
            keywords: words::command_palette::kw_sleep().to_owned(),
            status: active.then(|| {
                StatusBadge::State(SharedString::new_static(
                    words::command_palette::state_active(),
                ))
            }),
            shortcut: None,
            action: PaletteAction::SetSleep(Some(Until::After(duration))),
            mru_key: None,
            show_when_empty: true,
        });
    }

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Tracks),
        title: SharedString::new_static(words::transport::end_of_track()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_sleep(),
        )),
        keywords: words::command_palette::kw_sleep().to_owned(),
        status: sleeping
            .is_some_and(|asleep| asleep.until == Until::EndOfTrack)
            .then_some(StatusBadge::ActiveCheck),
        shortcut: None,
        action: PaletteAction::SetSleep(Some(Until::EndOfTrack)),
        mru_key: None,
        show_when_empty: true,
    });

    list.push(Candidate {
        section: SectionKind::Playback,
        leading: Leading::Icon(Icon::Queue),
        title: SharedString::new_static(words::transport::end_of_queue()),
        subtitle: Some(SharedString::new_static(
            words::command_palette::subtitle_sleep(),
        )),
        keywords: words::command_palette::kw_sleep().to_owned(),
        status: sleeping
            .is_some_and(|asleep| asleep.until == Until::EndOfQueue)
            .then_some(StatusBadge::ActiveCheck),
        shortcut: None,
        action: PaletteAction::SetSleep(Some(Until::EndOfQueue)),
        mru_key: None,
        show_when_empty: true,
    });

    if sleeping.is_some() {
        list.push(Candidate {
            section: SectionKind::Playback,
            leading: Leading::Icon(Icon::Close),
            title: SharedString::new_static(words::transport::sleep_off()),
            subtitle: Some(SharedString::new_static(
                words::command_palette::subtitle_sleep(),
            )),
            keywords: words::command_palette::kw_sleep().to_owned(),
            status: None,
            shortcut: None,
            action: PaletteAction::SetSleep(None),
            mru_key: None,
            show_when_empty: true,
        });
    }

    list
}
