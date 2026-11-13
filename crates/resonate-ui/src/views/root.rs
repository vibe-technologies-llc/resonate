mod acting;
mod deletion;
mod downloading;
mod heading;
mod keys;
mod landing;
mod loudness;
mod made;
mod magnifying;
mod panes;
mod playlist_actions;
mod pointer;
mod reaching;
mod regions;
mod searching;
mod started;
#[cfg(test)]
mod tests;
mod told;
mod typing_ahead;
mod wayback;

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ahash::AHashMap;
use futures_channel::oneshot;
use gpui::{
    AnyElement, App, Context, Div, DragMoveEvent, Entity, ExternalPaths, FocusHandle, Focusable,
    KeyDownEvent, MouseButton, MouseDownEvent, NavigationDirection, ObjectFit, Pixels, Point,
    Render, ScrollHandle, SharedString, Task, UniformListScrollHandle, Window, div, img,
    prelude::*, px, rgb,
};
use resonate_core::{
    AlbumId, ArtistsDrawn, MediaLocation, PlaylistId, QueueStamp, TrackId, Volume, ZoneId,
};
use resonate_engine::{Command, Placement, PlayerState, RepeatMode};
use resonate_library::{
    Cut, Direction, Library, PlaylistEntry, PlaylistOrder, RowOrder, SortOrder, TokenHeld, Track,
    TrackQuery,
};

use crate::{
    EqualiserModel, LibraryModel, LyricsModel, Notice, PlayerModel, ResonateApp, Selection, Tabs,
    WindowSize,
    analysis::AnalysisModel,
    app::{
        About, ChooseReached, CloseTheWindow, CycleRepeat, DropReached, FocusFilter, FocusSearch,
        GoToTheResults, HoldThePicture, HopAbove, HopBelow, LeaveControl, LeaveSearch, Listen,
        LowerRow, Minimise, Next, NextPane, OpenTheMenu, PasteAway, Pause, PlayPauseUnlessTyping,
        PlayReached, Previous, PreviousPane, Quit, RaiseRow, ReachAbove, ReachBelow,
        ReachEverything, ReachFirst, ReachLast, ReachNext, ReachPageAbove, ReachPageBelow,
        ReachPrevious, RedoEdit, SeekBackward, SeekForward, SeekFurtherBackward,
        SeekFurtherForward, Stop, TabOnward, ToggleFullScreen, ToggleMute, TogglePlayPause,
        ToggleQueue, ToggleShuffle, UndoEdit, VolumeDown, VolumeUp, WINDOW_CONTEXT, WidenAbove,
        WidenBelow, Zoom, seek_further, seek_step,
    },
    icons::Icon,
    listening::ListenModel,
    models::{Picture, Scale, Sorting},
    motion,
    serial::Serial,
    serving::ServerModel,
    settings::{ListOffered, SettingsWriter},
    theme,
    toast::{self},
    views::{
        announcing,
        browser::{ArtistShows, OpenedRecord},
        chrome,
        dropping::{FolderStanding, Incoming, TakingIn},
        field::{self, Field},
        focus::Controls,
        hint::{self},
        kit::{self},
        listing::{self},
        matching::Matching,
        menu::{self, Menu},
        missing::MissingShows,
        part::Parts,
        party::PartyPane,
        playlists::{Held, Naming, PlaylistsDrawn},
        queue::{QueueMeasure, QueueNames, TakenBack},
        reorder::{Creeping, Reach, Step},
        search::SearchShows,
        settings::{
            Armed, Category, CheckingYoutubeMusic, Fitting, HeldBand, LastfmSigning, Page, Plotted,
            QuickConnecting, SigningIn, SigningInToYoutubeMusic, SpotifyState, Tending,
        },
        sharing,
        sheets::{self},
        slider::{Grab, Rail},
        sorting::Ordered,
        spoken::{self},
        transport::{Handovers, Resolved, ShownCover},
        typing::TypeAhead,
        visualiser::Visualiser,
        wrapped,
        zones::{Switcher, ZoneShown},
    },
    words,
};

pub(crate) const VOLUME_STEP: f32 = 0.05;

const VOLUME_SETTLE: Duration = Duration::from_millis(400);

const WINDOW_SIZE_SETTLE: Duration = Duration::from_millis(400);

pub(crate) fn somewhere_in(rows: usize) -> usize {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as usize)
        .unwrap_or_default();

    now.checked_rem(rows).unwrap_or_default()
}

const WORDMARK_CAPITALS_CENTRED_BY: f32 = 1.0;

fn unsaved(written: Result<crate::Result<()>, oneshot::Canceled>) -> Option<Notice> {
    match written {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            tracing::error!(%error, "a setting could not be saved");
            Some(Notice::Trouble(words::root::setting_unsaved().to_owned()))
        }
        Err(oneshot::Canceled) => {
            tracing::error!("the thread writing the settings dropped a batch unanswered");
            Some(Notice::Trouble(
                words::root::setting_writer_stopped().to_owned(),
            ))
        }
    }
}

const PANE_GROUP: &str = "pane";

const PANE_REGION: &str = "pane-region";

const PINNED_INSET: f32 = 22.0;

const DOWNLOADS_PANEL_GAP: f32 = 8.0;

const DOWNLOADS_TOGGLE_PRESS: Duration = Duration::from_millis(600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Magnified {
    Album(AlbumId),
    File(MediaLocation),
    Served(MediaLocation),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pane {
    Albums,
    Artists,
    #[default]
    Tracks,
    Statistics,
    Party,
    Queue,
    Playlists,
    Favourites,
    Suggestions,
    Missing,
    Server,
    Lyrics,
    Inspector,
    Visualiser,
    Analysis,
    Settings,
}

const WAYS_BACK: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Wayback {
    pane: Pane,
    selection: Selection,
    at: LeftAt,
    named: SharedString,
    left_by: Leaving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arrangement {
    selection: Selection,
    sorting: Sorting,
    playlists: (PlaylistOrder, Direction),
    opened_rows: (RowOrder, Direction),
    search: (SortOrder, Direction),
    shelves: (usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leaving {
    ByALink,
    ByATab,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LeftAt {
    row: usize,
    into_the_row: f32,
}

impl Eq for LeftAt {}

impl LeftAt {
    fn read_off(offset: Pixels, row_height: f32) -> Self {
        let past = f32::from(-offset).max(0.0);
        if row_height <= 0.0 {
            return Self {
                row: 0,
                into_the_row: 0.0,
            };
        }
        let row = (past / row_height).floor();
        Self {
            row: row as usize,
            into_the_row: (past - row * row_height) / row_height,
        }
    }

    fn offset_at(self, row_height: f32) -> Pixels {
        px(-(row_height * (self.row as f32 + self.into_the_row)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UnderThePointer {
    at: Point<Pixels>,
    laid_out: u64,
}

impl UnderThePointer {
    fn moved_since(self, last: Option<Self>) -> bool {
        last.is_some_and(|last| last.at == self.at && last.laid_out != self.laid_out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pointer {
    InTheWindow,
    Gone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlayingRow {
    row: usize,
    track: Option<TrackId>,
    queue: QueueStamp,
}

impl PlayingRow {
    fn of(state: &PlayerState) -> Option<Self> {
        Some(Self {
            row: state.queue_position?,
            track: state.current.as_ref().map(|current| current.id),
            queue: state.queue_stamp,
        })
    }

    fn moved_on_from(self, shown: Self) -> bool {
        let another_track = self.track != shown.track;
        let stepped_within_the_same_queue = self.queue == shown.queue && self.row != shown.row;

        another_track || stepped_within_the_same_queue
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Following {
    shown: Option<PlayingRow>,
}

impl Following {
    fn follows(&mut self, playing: Option<PlayingRow>, queue_is_shown: bool) -> Option<usize> {
        let Some(now) = playing else {
            self.shown = None;
            return None;
        };
        if !queue_is_shown {
            return None;
        }

        let shown = self.shown.replace(now);
        shown
            .is_none_or(|shown| now.moved_on_from(shown))
            .then_some(now.row)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Library,
    Collection,
    Playing,
}

impl Section {
    const ALL: [Self; 3] = [Self::Library, Self::Collection, Self::Playing];

    fn label(self) -> &'static str {
        match self {
            Self::Library => words::root::section_library(),
            Self::Collection => words::root::section_collection(),
            Self::Playing => words::root::section_now_playing(),
        }
    }
}

impl Pane {
    pub const ALL: [Self; 16] = [
        Self::Albums,
        Self::Artists,
        Self::Tracks,
        Self::Statistics,
        Self::Party,
        Self::Queue,
        Self::Playlists,
        Self::Favourites,
        Self::Suggestions,
        Self::Missing,
        Self::Server,
        Self::Lyrics,
        Self::Inspector,
        Self::Visualiser,
        Self::Analysis,
        Self::Settings,
    ];

    pub const BROWSE: [Self; 14] = [
        Self::Albums,
        Self::Artists,
        Self::Tracks,
        Self::Statistics,
        Self::Favourites,
        Self::Playlists,
        Self::Suggestions,
        Self::Party,
        Self::Missing,
        Self::Server,
        Self::Lyrics,
        Self::Inspector,
        Self::Visualiser,
        Self::Analysis,
    ];

    pub const fn is_shown(self, tabs: Tabs) -> bool {
        match self {
            Self::Suggestions => tabs.suggestions,
            Self::Missing => tabs.missing,
            Self::Albums
            | Self::Party
            | Self::Artists
            | Self::Tracks
            | Self::Statistics
            | Self::Queue
            | Self::Playlists
            | Self::Favourites
            | Self::Server
            | Self::Lyrics
            | Self::Inspector
            | Self::Visualiser
            | Self::Analysis
            | Self::Settings => true,
        }
    }

    pub(crate) const fn is_there(self, tabs: Tabs, serving: bool) -> bool {
        self.is_shown(tabs) && (serving || !matches!(self, Self::Server))
    }

    pub(crate) const fn follows_the_clock(self) -> bool {
        matches!(self, Self::Inspector | Self::Analysis)
    }

    pub const fn lands_where_it_was_left(self) -> bool {
        matches!(self, Self::Albums | Self::Artists | Self::Tracks)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Albums => words::root::pane_albums(),
            Self::Artists => words::root::pane_artists(),
            Self::Tracks => words::root::pane_tracks(),
            Self::Statistics => words::root::pane_statistics(),
            Self::Party => words::root::pane_listening_party(),
            Self::Queue => words::root::pane_queue(),
            Self::Playlists => words::root::pane_playlists(),
            Self::Favourites => words::root::pane_favourites(),
            Self::Suggestions => words::root::pane_suggestions(),
            Self::Missing => words::root::pane_missing(),
            Self::Server => words::root::pane_services(),
            Self::Lyrics => words::root::pane_lyrics(),
            Self::Inspector => words::root::pane_inspector(),
            Self::Visualiser => words::root::pane_visualiser(),
            Self::Analysis => words::root::pane_analysis(),
            Self::Settings => words::root::pane_settings(),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Tracks => "tracks",
            Self::Statistics => "statistics",
            Self::Party => "party",
            Self::Queue => "queue",
            Self::Playlists => "playlists",
            Self::Favourites => "favourites",
            Self::Suggestions => "suggestions",
            Self::Missing => "missing",
            Self::Server => "server",
            Self::Lyrics => "lyrics",
            Self::Inspector => "inspector",
            Self::Visualiser => "visualiser",
            Self::Analysis => "analysis",
            Self::Settings => "settings",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "albums" => Some(Self::Albums),
            "artists" => Some(Self::Artists),
            "tracks" => Some(Self::Tracks),
            "statistics" => Some(Self::Statistics),
            "queue" => Some(Self::Queue),
            "playlists" => Some(Self::Playlists),
            "favourites" => Some(Self::Favourites),
            "suggestions" => Some(Self::Suggestions),
            "missing" => Some(Self::Missing),
            "server" => Some(Self::Server),
            "lyrics" => Some(Self::Lyrics),
            "inspector" => Some(Self::Inspector),
            "visualiser" => Some(Self::Visualiser),
            "analysis" => Some(Self::Analysis),
            "party" => Some(Self::Party),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    pub(crate) fn about(self) -> &'static str {
        match self {
            Self::Albums => words::root::about_albums(),
            Self::Artists => words::root::about_artists(),
            Self::Tracks => words::root::about_tracks(),
            Self::Statistics => words::root::about_statistics(),
            Self::Party => words::root::about_party(),
            Self::Queue => words::root::about_queue(),
            Self::Playlists => words::root::about_playlists(),
            Self::Favourites => words::root::about_favourites(),
            Self::Suggestions => words::root::about_suggestions(),
            Self::Missing => words::root::about_missing(),
            Self::Server => words::root::about_server(),
            Self::Lyrics => words::root::about_lyrics(),
            Self::Inspector => words::root::about_inspector(),
            Self::Visualiser => words::root::about_visualiser(),
            Self::Analysis => words::root::about_analysis(),
            Self::Settings => words::root::about_settings(),
        }
    }

    pub(crate) const fn section(self) -> Section {
        match self {
            Self::Albums | Self::Artists | Self::Tracks | Self::Statistics => Section::Library,
            Self::Party
            | Self::Playlists
            | Self::Favourites
            | Self::Suggestions
            | Self::Missing
            | Self::Server => Section::Collection,
            Self::Queue
            | Self::Lyrics
            | Self::Inspector
            | Self::Visualiser
            | Self::Analysis
            | Self::Settings => Section::Playing,
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Albums => Icon::Albums,
            Self::Artists => Icon::Artists,
            Self::Tracks => Icon::Tracks,
            Self::Statistics => Icon::Statistics,
            Self::Party => Icon::Party,
            Self::Queue => Icon::Queue,
            Self::Playlists => Icon::Playlists,
            Self::Favourites => Icon::Favourite,
            Self::Suggestions => Icon::Suggestions,
            Self::Missing => Icon::Missing,
            Self::Server => Icon::Server,
            Self::Lyrics => Icon::Lyrics,
            Self::Inspector => Icon::Inspector,
            Self::Visualiser => Icon::Visualiser,
            Self::Analysis => Icon::Analysis,
            Self::Settings => Icon::Settings,
        }
    }
}

pub(crate) struct Scrolls {
    pub(crate) settings: ScrollHandle,
    pub(crate) settings_rail: ScrollHandle,
    pub(crate) sidebar: ScrollHandle,
    pub(crate) inspector: ScrollHandle,
    pub(crate) statistics: ScrollHandle,
    pub(crate) analysis: ScrollHandle,
    pub(crate) suggestions: ScrollHandle,
    pub(crate) artist_records: ScrollHandle,
    pub(crate) ordering: ScrollHandle,
    pub(crate) downloads: ScrollHandle,
    pub(crate) heard_earlier: ScrollHandle,
    pub(crate) likely: ScrollHandle,
    pub(crate) remote: ScrollHandle,
    pub(crate) search: ScrollHandle,
    pub(crate) queue_rows: UniformListScrollHandle,
    pub(crate) playlist_rows: UniformListScrollHandle,
    pub(crate) track_rows: UniformListScrollHandle,
    pub(crate) artist_rows: UniformListScrollHandle,
    pub(crate) album_rows: UniformListScrollHandle,
    pub(crate) all_playlist_rows: UniformListScrollHandle,
    pub(crate) favourite_rows: UniformListScrollHandle,
    pub(crate) missing_rows: UniformListScrollHandle,
    pub(crate) suggestion_rows: UniformListScrollHandle,
    pub(crate) picker_rows: UniformListScrollHandle,
    pub(crate) server_rows: UniformListScrollHandle,
    pub(crate) shelves: RefCell<AHashMap<&'static str, ScrollHandle>>,
    pub(crate) left_at: AHashMap<PlaylistId, UniformListScrollHandle>,
}

pub(crate) struct Rooms {
    pub(crate) grid_width: Rc<kit::GridWidth>,
    pub(crate) hero_width: Rc<Cell<Pixels>>,
    pub(crate) hero_height: Rc<Cell<Pixels>>,
    pub(crate) playing: Rc<Cell<Pixels>>,
    pub(crate) status: Rc<Cell<Pixels>>,
    pub(crate) inspected: Rc<Cell<Pixels>>,
    pub(crate) heading: Rc<Cell<Pixels>>,
    pub(crate) columns_fit: listing::Fitting,
    pub(crate) queue_height: Rc<Cell<Pixels>>,
    pub(crate) reached_unseen: Rc<Cell<bool>>,
}

pub(crate) struct Remotely {
    pub(super) songs: Vec<super::remote::RemoteSong>,
    pub(crate) collapsed: bool,
    pub(crate) fits_from: Option<Pixels>,
    pub(crate) expanded_by_hand: bool,
    pub(crate) heading_pressed_at: Option<Point<Pixels>>,
    pub(crate) controls: Controls,
    pub(crate) driving: Option<(String, String)>,
    pub(crate) driving_redraws: Option<Task<()>>,
    pub(crate) mirror_redraws: Option<Task<()>>,
    pub(crate) driving_rails: std::collections::BTreeMap<u32, crate::views::slider::Rail>,
    pub(crate) driving_rows: usize,
}

pub(crate) struct Accounts {
    pub(crate) spotify: SpotifyState,
    pub(crate) youtube_music_check: CheckingYoutubeMusic,
    pub(crate) youtube_music_sign_in: SigningInToYoutubeMusic,
    pub(crate) jellyfin_quick: QuickConnecting,
    pub(crate) signing_in: SigningIn,
    pub(crate) lastfm: LastfmSigning,
    pub(crate) _token_checked: Task<()>,
}

pub(crate) struct Rails {
    pub(crate) seek: Rail,
    pub(crate) seek_pointed: Option<f32>,
    pub(crate) volume: Rail,
    pub(crate) grabbed: Option<Grab>,
    pub(crate) volume_settled: Task<()>,
    pub(crate) volume_unkept: Option<(ZoneId, Volume)>,
    pub(crate) volume_aimed: Option<f32>,
    pub(crate) muted_from: Option<Volume>,
    pub(crate) band_retuning: Task<()>,
    pub(crate) band_untold: Option<bool>,
    pub(crate) held_band: Option<HeldBand>,
    pub(crate) curve_plotted: Rc<Cell<Plotted>>,
}

pub(crate) struct Orders {
    pub(crate) queue_order: RowOrder,
    pub(crate) queue_reading: Direction,
    pub(crate) row_order: RowOrder,
    pub(crate) row_reading: Direction,
    pub(crate) keeping: bool,
    pub(crate) query_sort: SortOrder,
    pub(crate) query_reading: Direction,
    pub(crate) query_cap: Option<usize>,
    pub(crate) ordering: Option<Ordered>,
    pub(crate) ordering_at: Point<Pixels>,
    pub(crate) ordering_typed: TypeAhead,
}

pub(crate) struct Remembered {
    pub(crate) tab: bool,
    pub(crate) window_size: bool,
    pub(crate) settings_category: bool,
    pub(crate) last_window_size: Option<WindowSize>,
    pub(crate) _window_size_settled: Task<()>,
}

pub(crate) struct Dropped {
    pub(crate) incoming: Option<Incoming>,
    pub(crate) taking_in: Option<TakingIn>,
    pub(crate) watching_the_drag: Task<()>,
    pub(crate) weighing_the_drag: Task<()>,
    pub(crate) _taking_in: Task<()>,
}

pub(crate) struct Typed {
    pub(crate) words: Entity<Field>,
    pub(crate) isrc: Entity<Field>,
    pub(crate) name: Entity<Field>,
    pub(crate) contact: Entity<Field>,
    pub(crate) acoustid: Entity<Field>,
    pub(crate) replication: Entity<Field>,
    pub(crate) audd: Entity<Field>,
    pub(crate) listenbrainz: Entity<Field>,
    pub(crate) subsonic: [Entity<Field>; 3],
    pub(crate) jellyfin: [Entity<Field>; 3],
    pub(crate) tidal: [Entity<Field>; 6],
    pub(crate) lastfm: [Entity<Field>; 2],
    pub(crate) spotify_client: Entity<Field>,
    pub(crate) youtube_music: [Entity<Field>; 2],
    pub(crate) discord_app: Entity<Field>,
    pub(crate) discord_icon: Entity<Field>,
    pub(crate) sync_server: Entity<Field>,
    pub(crate) sync_device: Entity<Field>,
    pub(crate) shared_name: Entity<Field>,
    pub(crate) sync_code: Entity<Field>,
    pub(crate) organising: Entity<Field>,
    pub(crate) minimum_length: Entity<Field>,
    pub(crate) root: Entity<Field>,
    pub(crate) finding: Entity<Field>,
    pub(crate) server_search: Entity<Field>,
    pub(crate) figure: Entity<Field>,
    pub(crate) looking: Entity<Field>,
    pub(crate) measured_as: Entity<Field>,
    pub(crate) zone_name: Entity<Field>,
    pub(crate) search: Entity<Field>,
}

pub(crate) struct SettingsShown {
    pub(crate) category: Category,
    pub(crate) page: Option<Page>,
    pub(crate) music_folder_standing: FolderStanding,
    pub(crate) looking_at_the_music_folder: Task<()>,
    pub(crate) reading_the_room: Task<()>,
    pub(crate) looking_at_the_room: Task<()>,
    pub(crate) room_shape: Option<(std::path::PathBuf, bool)>,
    pub(crate) audition: Option<crate::views::settings::ListeningTest>,
    pub(crate) test_setup: crate::views::settings::Setup,
    pub(crate) fitting_the_preamp: Fitting,
    pub(crate) making_the_vault: Task<()>,
    pub(crate) tending: Tending,
    pub(crate) looking_at_the_typed_root: Task<()>,
    pub(crate) armed: Option<Armed>,
    pub(crate) code_regenerated: bool,
    pub(crate) pairing_shown: bool,
    pub(crate) own_server_asked: bool,
    pub(crate) reset_everything_landed: bool,
    pub(crate) vault_named: Option<PathBuf>,
    pub(crate) newer: Option<crate::Newer>,
    pub(crate) looking_for_updates: Task<()>,
    pub(crate) trying: Option<(resonate_engine::NodeName, Task<()>)>,
    pub(crate) heard: Option<(
        resonate_engine::NodeName,
        Result<resonate_engine::Learning, resonate_engine::Unlearnt>,
    )>,
}

pub(crate) struct Social {
    pub(crate) wrapped: Option<wrapped::Deck>,
    pub(crate) reading_wrapped: Option<Task<()>>,
    pub(crate) sharing: Option<sharing::Sharing>,
    pub(crate) shared_lists: Vec<(PlaylistId, bool)>,
    pub(crate) sharing_redraws: Option<Task<()>>,
    pub(crate) party_offered: Option<String>,
    pub(crate) list_offered: Option<ListOffered>,
}

pub(crate) struct Shows {
    pub(crate) artist: ArtistShows,
    pub(crate) search: SearchShows,
    pub(crate) artists: ArtistsDrawn,
    pub(crate) missing: MissingShows,
    pub(crate) playlists: PlaylistsDrawn,
    pub(crate) bitrate_graph: bool,
    pub(crate) time_left: bool,
}

pub(crate) struct TypedAhead {
    pub(crate) ahead: TypeAhead,
    pub(crate) _stops: Task<()>,
    pub(crate) last_typed: Option<Instant>,
    pub(crate) landed_by_a_jump: Option<Reach>,
    pub(crate) waiting_for_the_results: bool,
    pub(crate) left_for_the_search: Option<(Pane, LeftAt)>,
}

pub struct RootView {
    pub(crate) player: Entity<PlayerModel>,
    pub(crate) library: Entity<LibraryModel>,
    pub(crate) lyrics: Entity<LyricsModel>,
    pub(crate) visualiser: Entity<Visualiser>,
    pub(crate) analysis: Entity<AnalysisModel>,
    pub(crate) listen: Entity<ListenModel>,
    pub(crate) party: Entity<PartyPane>,
    pub(crate) listening_open: bool,
    downloads_open: bool,
    downloads_pressed_off: Option<Instant>,
    pub(crate) deleting: Option<Deleting>,
    pub(crate) matching: Option<Matching>,
    pub(crate) likely_asked: Serial,
    pub(crate) attention: Serial,
    pub(crate) settings_written: SettingsWriter,
    pub(crate) pane: Pane,
    behind_queue: Pane,
    pub(crate) naming: Option<Naming>,
    pub(crate) adding: Option<Held>,
    pub(crate) adding_songs_to: Option<PlaylistId>,
    pub(crate) finding_holds_the_caret: bool,
    pub(crate) server: Entity<ServerModel>,
    pub(crate) equaliser: Entity<EqualiserModel>,
    pub(crate) controls: Controls,
    pub(crate) standing_controls: Controls,
    pub(crate) sheet_panels: Controls,
    pub(crate) sheets: sheets::Standing,
    pub(crate) announced: announcing::Announcer,
    pub(crate) announcing_again: announcing::Waking,
    pub(crate) took_out: TakenBack,
    pub(crate) queue_length: QueueMeasure,
    pub(crate) queue_ordered: Option<Task<()>>,
    pub(crate) earlier_plays_asked: bool,
    came_from: Vec<Wayback>,
    goes_forward: Vec<Wayback>,
    pub(crate) titled: Option<SharedString>,
    pub(crate) landing_on: Option<LeftAt>,
    under_the_pointer: Option<UnderThePointer>,
    pub(crate) menu: Option<Menu>,
    pub(crate) record: Option<OpenedRecord>,
    pub(crate) reach: Option<Reach>,
    arranged_as: Arrangement,
    pub(crate) creeping: Option<Creeping>,
    pub(crate) creeping_on: Task<()>,
    listing_whole: Task<()>,
    pub(crate) zones: Vec<ZoneShown>,
    pub(crate) switcher: Switcher,
    pub(crate) naming_the_zone: Option<ZoneId>,
    following: Following,
    pub(crate) magnified: Option<Magnified>,
    pointer_inside: bool,
    pub(crate) beneath_closes: Task<()>,
    pub(crate) queue_names: QueueNames,
    drawn_at: SystemTime,
    scale: Scale,
    pub(crate) resolved: RefCell<Option<Resolved>>,
    pub(crate) shown_cover: RefCell<Option<ShownCover>>,
    pub(crate) handovers: RefCell<Handovers>,
    pub(crate) focus: FocusHandle,
    pub(crate) menu_focus: FocusHandle,
    pub(crate) parts: Parts,
    pub(crate) scrolls: Scrolls,
    pub(crate) rooms: Rooms,
    pub(crate) remotely: Remotely,
    pub(crate) accounts: Accounts,
    pub(crate) rails: Rails,
    pub(crate) orders: Orders,
    pub(crate) remembered: Remembered,
    pub(crate) dropped: Dropped,
    pub(crate) typed: Typed,
    pub(crate) settings_shown: SettingsShown,
    pub(crate) social: Social,
    pub(crate) shows: Shows,
    pub(crate) typing: TypedAhead,
}

fn token_notice(held: resonate_library::Result<TokenHeld>) -> Notice {
    match held {
        Ok(TokenHeld::By(user)) => Notice::Done(words::root::token_held_by(user)),
        Ok(TokenHeld::Unknown) => Notice::Trouble(words::root::token_unknown().to_owned()),
        Err(error) => {
            tracing::warn!(%error, "ListenBrainz could not be asked about the token");
            Notice::Trouble(words::root::token_unchecked().to_owned())
        }
    }
}

pub(crate) fn framed_cover(art: Option<Picture>, side: f32) -> Div {
    let frame = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(cover_rounding(side)))
        .overflow_hidden()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(kit::cover_edge());

    match art {
        Some(art) => frame.child(
            img(art)
                .size(px(side))
                .object_fit(ObjectFit::Cover)
                .rounded(px(cover_rounding(side))),
        ),
        None => frame.child(kit::uncovered_mark(side)),
    }
}

pub(crate) fn cover_rounding(side: f32) -> f32 {
    if side >= theme::shelf_cover() {
        8.0
    } else if side >= theme::now_playing_cover() {
        6.0
    } else {
        4.0
    }
}

impl Focusable for RootView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl RootView {
    fn follow_the_scale(&mut self, window: &Window, cx: &mut Context<Self>) {
        let scale = Scale::of(window.scale_factor());
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        self.library
            .update(cx, |library, _| library.scaled_by(scale));
        self.player.update(cx, |player, _| player.scaled_by(scale));
        self.lyrics.update(cx, |lyrics, _| lyrics.scaled_by(scale));
        self.server.update(cx, |server, _| server.scaled_by(scale));
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spoken::listened_to(window.is_a11y_active());
        self.hand_the_reader_the_menu(window, cx);
        self.keep_the_focus_with_the_sheets(window, cx);
        self.keep_the_announcements(cx);
        let moved = self.moved_under_a_still_pointer(window, cx);
        hint::asking(self.hints_are_wanted(cx) && !moved);
        let opened = menu::Opened(self.menu.as_ref().and_then(|menu| menu.owner().cloned()));
        if cx.try_global::<menu::Opened>() != Some(&opened) {
            cx.set_global(opened);
        }
        self.drawn_at = SystemTime::now();
        self.follow_the_scale(window, cx);
        let mouse_navigation = cx.global::<ResonateApp>().mouse_navigation;
        let grain = self.grain(window, cx);
        self.player.update(cx, |player, _| player.draw_at(grain));
        self.name_the_window(window, cx);

        let app = div()
            .track_focus(&self.focus)
            .key_context(WINDOW_CONTEXT)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::background()))
            .font_family(theme::ui_face())
            .text_size(px(theme::text_base()))
            .text_color(rgb(theme::text()))
            .when(mouse_navigation, |app| {
                app.on_mouse_down(
                    MouseButton::Navigate(NavigationDirection::Back),
                    cx.listener(|this, _: &MouseDownEvent, _, cx| this.go_back(cx)),
                )
                .on_mouse_down(
                    MouseButton::Navigate(NavigationDirection::Forward),
                    cx.listener(|this, _: &MouseDownEvent, _, cx| this.go_forward(cx)),
                )
            })
            .on_action(cx.listener(|this, _: &TogglePlayPause, _, cx| {
                this.send(Command::TogglePlayPause, cx);
            }))
            .on_action(cx.listener(|this, _: &PlayPauseUnlessTyping, window, cx| {
                this.play_pause_or_type_a_space(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Pause, _, cx| this.send(Command::Pause, cx)))
            .on_action(cx.listener(|this, _: &Stop, _, cx| this.send(Command::Stop, cx)))
            .on_action(cx.listener(|this, _: &Next, _, cx| this.send(Command::Next, cx)))
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.send(Command::Previous, cx)))
            .on_action(cx.listener(|this, _: &SeekForward, _, cx| {
                if this.step_into_the_menu(cx)
                    || this.turn_the_slide(1, cx)
                    || this.something_stands_over_the_pane()
                {
                    return;
                }
                if !this.reach_along_the_strip(Step::Below, cx) {
                    this.seek_by(seek_step(), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SeekBackward, _, cx| {
                if this.back_out_of_the_menu(cx)
                    || this.turn_the_slide(-1, cx)
                    || this.something_stands_over_the_pane()
                {
                    return;
                }
                if !this.reach_along_the_strip(Step::Above, cx) {
                    this.seek_by(-seek_step(), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SeekFurtherForward, _, cx| {
                this.seek_by(seek_further(), cx);
            }))
            .on_action(cx.listener(|this, _: &SeekFurtherBackward, _, cx| {
                this.seek_by(-seek_further(), cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| this.toggle_mute(cx)))
            .on_action(cx.listener(|this, _: &ToggleQueue, _, cx| this.toggle_queue(cx)))
            .on_action(cx.listener(|this, _: &HoldThePicture, _, cx| {
                this.hold_the_picture(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleShuffle, _, cx| {
                let shuffle = this.player.read(cx).state().shuffle;
                this.send(Command::SetShuffle(!shuffle), cx);
            }))
            .on_action(cx.listener(|this, _: &CycleRepeat, _, cx| {
                let repeat = match this.player.read(cx).state().repeat {
                    RepeatMode::Off => RepeatMode::Queue,
                    RepeatMode::Queue => RepeatMode::Track,
                    RepeatMode::Track => RepeatMode::Off,
                };
                this.send(Command::SetRepeat(repeat), cx);
            }))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| this.volume_by(VOLUME_STEP, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| this.volume_by(-VOLUME_STEP, cx)))
            .on_action(cx.listener(|this, _: &ReachAbove, _, cx| {
                this.reach_row(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachBelow, _, cx| {
                this.reach_row(Step::Below, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachFirst, _, cx| {
                this.reach_the_end(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachLast, _, cx| {
                this.reach_the_end(Step::Below, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachPageAbove, _, cx| {
                this.reach_a_page(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachPageBelow, _, cx| {
                this.reach_a_page(Step::Below, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachEverything, _, cx| {
                this.reach_everything(cx);
            }))
            .on_action(cx.listener(|this, _: &ChooseReached, _, cx| {
                this.choose_the_reached(cx);
            }))
            .on_action(cx.listener(|this, _: &HopAbove, _, cx| {
                this.hop_to_a_row(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &HopBelow, _, cx| {
                this.hop_to_a_row(Step::Below, cx);
            }))
            .on_action(cx.listener(|this, _: &DropReached, _, cx| {
                this.drop_reached(cx);
            }))
            .on_action(cx.listener(|this, _: &WidenAbove, _, cx| {
                this.widen_reach(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &WidenBelow, _, cx| {
                this.widen_reach(Step::Below, cx);
            }))
            .on_action(cx.listener(|this, _: &RaiseRow, _, cx| {
                this.move_reached_rows(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &LowerRow, _, cx| {
                this.move_reached_rows(Step::Below, cx);
            }))
            .on_action(cx.listener(|this, _: &PlayReached, window, cx| {
                this.play_reached_rows(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenTheMenu, _, cx| {
                this.open_the_reached_menu(cx);
            }))
            .on_action(cx.listener(|this, _: &UndoEdit, _, cx| this.undo_edit(cx)))
            .on_action(cx.listener(|this, _: &RedoEdit, _, cx| this.redo_edit(cx)))
            .on_action(cx.listener(|this, _: &field::Undo, _, cx| this.undo_edit(cx)))
            .on_action(cx.listener(|this, _: &field::Redo, _, cx| this.redo_edit(cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.enter_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &LeaveSearch, window, cx| {
                this.dismiss_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteAway, window, cx| {
                this.typed
                    .search
                    .update(cx, |search, cx| search.pastes(window, cx));
            }))
            .on_action(cx.listener(|this, _: &GoToTheResults, window, cx| {
                this.go_to_the_results(window, cx);
            }))
            .on_action(cx.listener(|this, _: &TabOnward, window, cx| {
                this.tab_onward(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusFilter, window, cx| {
                this.focus_filter(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Listen, _, cx| this.open_the_listener(cx)))
            .on_action(cx.listener(|this, _: &ReachNext, window, cx| {
                this.tab_past_the_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ReachPrevious, window, cx| {
                this.tab_back_to_the_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NextPane, _, cx| this.step_pane(Step::Below, cx)))
            .on_action(cx.listener(|this, _: &PreviousPane, _, cx| {
                this.step_pane(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &LeaveControl, window, cx| {
                if !this.close_what_stands_over_the_pane(window, cx) {
                    this.let_the_control_go(window, cx);
                }
            }))
            .on_action(|_: &Quit, window: &mut Window, _: &mut App| window.remove_window())
            .on_action(|_: &CloseTheWindow, window: &mut Window, cx: &mut App| {
                chrome::close_the_window(window, cx);
            })
            .on_action(|_: &Minimise, window: &mut Window, _: &mut App| {
                window.minimize_window();
            })
            .on_action(|_: &Zoom, window: &mut Window, _: &mut App| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window: &mut Window, _: &mut App| {
                window.toggle_fullscreen();
            })
            .on_action(cx.listener(|this, _: &About, _, cx| this.say_what_this_is(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.typed(event, window, cx);
            }))
            .on_drag_move::<ExternalPaths>(cx.listener(
                |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    if this.is_already_weighing(event.drag(cx).paths()) {
                        return;
                    }
                    let paths = event.drag(cx).paths().to_vec();
                    this.dragged_over(&paths, cx);
                },
            ))
            .child(self.parts.header())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(self.parts.sidebar())
                    .child(self.parts.pane()),
            )
            .child(self.parts.transport())
            .child(self.drag_surface(cx))
            .child(self.pointer_watch(cx))
            .child(self.composition_watch())
            .when_some(self.announcement(), ParentElement::child)
            .when_some(
                toast::drawn(self.typing.ahead.typed().is_some(), window, cx),
                ParentElement::child,
            )
            .when_some(self.focused_control_named(window), |app, name| {
                app.child(hint::caption(name))
            })
            .when_some(self.type_ahead_pill(), ParentElement::child)
            .when_some(self.downloads_over_the_app(cx), ParentElement::child)
            .when_some(
                self.menu_over_the_app(window.viewport_size(), cx),
                ParentElement::child,
            )
            .when_some(self.record_over_the_app(cx), ParentElement::child)
            .when_some(self.adding.clone(), |app, holding| {
                app.child(self.playlist_picker(&holding, cx))
            })
            .when_some(self.magnified.clone(), |app, magnified| {
                app.child(motion::faded_in(
                    self.magnifier(&magnified, window, cx),
                    "magnifier-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when(self.listening_open, |app| {
                app.child(motion::faded_in(
                    self.listen_sheet(cx),
                    "listen-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when(self.social.sharing.is_some(), |app| {
                app.child(motion::faded_in(
                    self.sharing_sheet(cx),
                    "sharing-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when(self.social.wrapped.is_some(), |app| {
                app.child(motion::faded_in(
                    self.wrapped_sheet(cx),
                    "wrapped-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when_some(self.orders.ordering, |app, ordered| {
                app.child(self.order_sheet(ordered, window.viewport_size(), cx))
            })
            .when(self.social.party_offered.is_some(), |app| {
                app.child(motion::faded_in(
                    self.party_offer_sheet(cx),
                    "party-offer-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when(self.social.list_offered.is_some(), |app| {
                app.child(motion::faded_in(
                    self.list_offer_sheet(cx),
                    "list-offer-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when(self.matching.is_some(), |app| {
                app.child(motion::faded_in(
                    self.matching_sheet(cx),
                    "matching-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when_some(self.deleting.clone(), |app, deleting| {
                app.child(motion::faded_in(
                    self.deletion_sheet(&deleting, cx),
                    "delete-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when_some(self.copying_pill(cx), ParentElement::child)
            .when_some(
                self.drop_overlay(cx)
                    .map(|overlay| motion::faded_in(overlay, "drop-scrim", motion::ARRIVES_OVER)),
                ParentElement::child,
            );

        chrome::frame(window, app)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Deleting {
    pub(crate) tracks: Vec<TrackId>,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) a_cut: bool,
}

impl Deleting {
    pub(crate) fn of(track: &Track) -> Self {
        Self {
            tracks: vec![track.id],
            title: track.title.clone(),
            artist: track.artist.clone().filter(|artist| !artist.is_empty()),
            a_cut: track.span.is_some(),
        }
    }

    pub(crate) fn each(tracks: &[Track]) -> Self {
        match tracks {
            [only] => Self::of(only),
            _ => Self {
                tracks: tracks.iter().map(|track| track.id).collect(),
                title: tracks
                    .first()
                    .map_or_else(String::new, |track| track.title.clone()),
                artist: None,
                a_cut: tracks.iter().any(|track| track.span.is_some()),
            },
        }
    }

    fn asking(&self) -> String {
        words::root::delete_songs(self.tracks.len())
    }

    fn saying(&self) -> String {
        if self.tracks.len() > 1 {
            return words::root::songs_moved_to_the_trash(self.tracks.len());
        }
        match &self.artist {
            Some(artist) => {
                words::root::song_by_moved_to_the_trash(artist.as_str(), self.title.as_str())
            }
            None => words::root::song_moved_to_the_trash(self.title.as_str()),
        }
    }
}

fn read_each(library: &Library, scopes: Vec<Selection>) -> resonate_library::Result<Vec<Track>> {
    let mut read = Vec::new();
    for scoped in scopes {
        read.extend(library.tracks(&whole_of(scoped))?);
    }

    Ok(read)
}

fn whole_of(scoped: Selection) -> TrackQuery {
    let (album, artist) = match scoped {
        Selection::Everything => (None, None),
        Selection::Album(album) => (Some(album), None),
        Selection::Artist(artist) => (None, Some(artist)),
    };

    TrackQuery {
        album,
        artist,
        sort: SortOrder::AlbumThenTrack,
        reading: SortOrder::AlbumThenTrack.reads(),
        ..TrackQuery::default()
    }
}

pub(crate) fn listed(tracks: &[Track]) -> Arc<[PlaylistEntry]> {
    tracks
        .iter()
        .zip(0..)
        .map(|(track, position)| PlaylistEntry {
            position,
            cut: Cut::of(track),
            track: Some(track.clone()),
        })
        .collect()
}

fn queued(rows: usize, at: Placement, play: bool) -> String {
    match (play, at) {
        (true, _) => words::root::queued_and_playing(rows),
        (false, Placement::Next) => words::root::to_play_next(rows),
        (false, Placement::Queued | Placement::At(_)) => words::root::added_to_the_queue(rows),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Landing {
    On(Pane),
    Unscoped(Pane),
    Scoped,
    EveryPlaylist,
}

const fn in_front_of(pane: Pane, selection: Selection) -> Pane {
    match (pane, selection) {
        (Pane::Tracks, Selection::Album(_)) => Pane::Albums,
        (Pane::Tracks, Selection::Artist(_)) => Pane::Artists,
        (pane, _) => pane,
    }
}

fn landing(pressed: Pane, in_front: Pane, selection: Selection, opened: bool) -> Landing {
    let scoped = selection != Selection::Everything;
    let holds_the_scope = matches!(
        (pressed, selection),
        (Pane::Albums, Selection::Album(_)) | (Pane::Artists, Selection::Artist(_))
    );

    match (pressed == in_front, pressed) {
        (true, Pane::Albums | Pane::Artists | Pane::Tracks) if scoped => Landing::Unscoped(pressed),
        (true, Pane::Playlists) if opened => Landing::EveryPlaylist,
        (false, _) if holds_the_scope => Landing::Scoped,
        (false, Pane::Tracks) if scoped => Landing::Unscoped(Pane::Tracks),
        (true | false, _) => Landing::On(pressed),
    }
}

fn stepped_pane(from: Pane, step: Step, tabs: Tabs, serving: bool) -> Pane {
    let listed: Vec<Pane> = Pane::BROWSE
        .into_iter()
        .filter(|pane| pane.is_there(tabs, serving))
        .collect();
    let at = listed
        .iter()
        .position(|pane| *pane == from)
        .unwrap_or_default();
    let next = match step {
        Step::Below => (at + 1) % listed.len(),
        Step::Above => (at + listed.len() - 1) % listed.len(),
    };

    listed[next]
}

pub(crate) fn row(selected: bool) -> Div {
    div()
        .flex()
        .w_full()
        .items_center()
        .gap(px(listing::COLUMN_GAP))
        .px(px(listing::ROW_INSET))
        .h(px(theme::row_height()))
        .text_size(px(theme::text_sm()))
        .when(selected, |row| row.bg(kit::chosen_wash()))
}

pub(crate) fn tall_row(selected: bool) -> Div {
    row(selected).h(px(theme::tall_row_height()))
}

pub(crate) fn empty(icon: Icon, message: &'static str, more: Option<&'static str>) -> AnyElement {
    kit::empty(icon, message, more)
}

fn chosen_ground(chosen: bool) -> impl IntoElement {
    motion::flips(
        div()
            .absolute()
            .inset_0()
            .rounded_md()
            .bg(rgb(theme::raised())),
        "chosen",
        chosen,
        |ground, from, to, share| {
            let at = motion::turned(from, to, share);
            let grown = theme::chosen_mark() * at;

            ground.opacity(at).child(
                div()
                    .absolute()
                    .left_0()
                    .top(px((theme::sidebar_row() - grown) / 2.0))
                    .w(px(2.0))
                    .h(px(grown))
                    .rounded_full()
                    .bg(rgb(theme::accent())),
            )
        },
    )
}
