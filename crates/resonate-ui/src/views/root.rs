use std::{
    cell::{Cell, RefCell},
    future,
    hash::{Hash as _, Hasher as _},
    mem,
    path::PathBuf,
    rc::Rc,
    slice,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet, AHasher};
use futures_channel::oneshot;
use gpui::{
    AnimationElement, AnyElement, App, Canvas, Context, Div, DragMoveEvent, ElementId, Entity,
    ExternalPaths, FocusHandle, Focusable, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, NavigationDirection, ObjectFit, Pixels, Point, Render, Role,
    ScrollHandle, ScrollStrategy, SharedString, Stateful, Task, UniformListScrollHandle, Window,
    canvas, div, img, point, prelude::*, px, rgb, rgba,
};
use resonate_core::{
    AlbumId, ArtistId, ArtistsDrawn, Frames, MediaLocation, PlaylistId, QueueStamp, Span, TrackId,
    Volume, ZoneId,
};
use resonate_engine::{
    Command, Counting, Placement, PlayerState, QueueItem, QueueRows, RepeatMode, Unclaimed,
    stamp_of,
};
use resonate_library::{
    ArtistFound, Cut, Direction, Kept, Library, Playing, Playlist, PlaylistEntry, PlaylistOrder,
    RowOrder, SavedQuery, SortOrder, TokenHeld, Track, TrackQuery, is_a_followed_link,
};

use crate::{
    Continuing, Drawn, EqualiserModel, LibraryModel, LyricsModel, Notice, PlayerModel, ResonateApp,
    Selection, Setting, SettingChange, SettingKey, Tabs, WindowSize,
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
        WidenBelow, Zoom, attend, seek_further, seek_step,
    },
    downloads::{self, Download, Fetching},
    format,
    icons::{self, Icon},
    listening::ListenModel,
    models::{Picture, Scale, Sorting},
    motion,
    serving::ServerModel,
    settings::SettingsWriter,
    theme,
    toast::{self, Toaster},
    together::is_an_invitation,
    views::{
        announcing,
        browser::{self, ArtistShows, OpenedRecord},
        chrome,
        dropping::{FolderStanding, Incoming, TakingIn},
        favourites::Shelved,
        field::{Blurred, Caught, Composing, Field, Submitted},
        focus::Controls,
        hint::{self, Names},
        kit::{self, Casts as _, EndsInAnEllipsis},
        listing::{self, Pictured},
        matching::{self, Matching},
        menu::{self, AtTheReach, Menu},
        missing::MissingShows,
        part::{Parts, Region},
        playlists::{self, Held, Naming, PlaylistsDrawn, Rows},
        pointed::{self, LitUnderThePointer},
        queue::{Named, QueueMeasure, QueueNames, TakenBack, took_out},
        reorder::{self, Chosen, Creeping, Lift, Lifted, LiftedTo, Listed, Reach, Shift, Step},
        scrollbar::Scrollbars,
        search::{self, SearchShows, TopEntry, TopRun},
        settings::{
            Account, Armed, Category, CheckingYoutubeMusic, Fitting, HeldBand, LastfmAccount,
            LastfmSigning, Page, Plotted, QuickConnecting, Service, SigningIn,
            SigningInToYoutubeMusic, SpotifyState, TidalAccount, YoutubeMusicField, spotify,
        },
        sharing,
        sheets::{self, MAGNIFIER_PANEL},
        slider::{Grab, Rail},
        sorting::Ordered,
        spoken::{self, Spelt, Spoken},
        transport::{Handovers, Resolved, ShownCover},
        typing::{self, TypeAhead, jumped},
        visualiser::Visualiser,
        wrapped,
        zones::{self, Switcher, ZoneShown},
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

pub struct RootView {
    pub(crate) player: Entity<PlayerModel>,
    pub(crate) library: Entity<LibraryModel>,
    pub(crate) lyrics: Entity<LyricsModel>,
    pub(crate) visualiser: Entity<Visualiser>,
    pub(crate) analysis: Entity<AnalysisModel>,
    pub(crate) listen: Entity<ListenModel>,
    pub(crate) listening_open: bool,
    pub(crate) wrapped: Option<wrapped::Deck>,
    pub(crate) reading_wrapped: Option<Task<()>>,
    pub(crate) sharing: Option<sharing::Sharing>,
    pub(crate) shared_lists: Vec<(PlaylistId, bool)>,
    pub(crate) sharing_redraws: Option<Task<()>>,
    pub(crate) party_redraws: Option<Task<()>>,
    downloads_open: bool,
    downloads_pressed_off: Option<Instant>,
    pub(crate) deleting: Option<Deleting>,
    pub(crate) matching: Option<Matching>,
    pub(crate) typed_words: Entity<Field>,
    pub(crate) typed_isrc: Entity<Field>,
    pub(crate) party_offered: Option<String>,
    pub(crate) incoming: Option<Incoming>,
    pub(crate) taking_in: Option<TakingIn>,
    pub(crate) watching_the_drag: Task<()>,
    pub(crate) weighing_the_drag: Task<()>,
    pub(crate) music_folder_standing: FolderStanding,
    pub(crate) looking_at_the_music_folder: Task<()>,
    pub(crate) reading_the_room: Task<()>,
    pub(crate) looking_at_the_room: Task<()>,
    pub(crate) room_shape: Option<(std::path::PathBuf, bool)>,
    pub(crate) audition: Option<crate::views::settings::ListeningTest>,
    pub(crate) test_setup: crate::views::settings::Setup,
    pub(crate) fitting_the_preamp: Fitting,
    pub(crate) making_the_vault: Task<()>,
    pub(crate) looking_at_the_typed_root: Task<()>,
    pub(crate) showing_time_left: bool,
    pub(crate) _taking_in: Task<()>,
    pub(crate) settings_written: SettingsWriter,
    pub(crate) pane: Pane,
    pub(crate) settings_category: Category,
    pub(crate) settings_page: Option<Page>,
    pub(crate) settings_scroll: ScrollHandle,
    pub(crate) settings_rail_scroll: ScrollHandle,
    pub(crate) sidebar_scroll: ScrollHandle,
    pub(crate) inspector_scroll: ScrollHandle,
    pub(crate) statistics_scroll: ScrollHandle,
    pub(crate) party_scroll: ScrollHandle,
    pub(crate) analysis_scroll: ScrollHandle,
    pub(crate) suggestions_scroll: ScrollHandle,
    pub(crate) artist_records_scroll: ScrollHandle,
    behind_queue: Pane,
    pub(crate) seek_rail: Rail,
    pub(crate) seek_pointed: Option<f32>,
    pub(crate) volume_rail: Rail,
    pub(crate) grabbed: Option<Grab>,
    pub(crate) curve_plotted: Rc<Cell<Plotted>>,
    pub(crate) held_band: Option<HeldBand>,
    pub(crate) bitrate_graph: bool,
    pub(crate) naming: Option<Naming>,
    pub(crate) ordering: Option<Ordered>,
    pub(crate) ordering_at: Point<Pixels>,
    pub(crate) ordering_scroll: ScrollHandle,
    pub(crate) downloads_scroll: ScrollHandle,
    pub(crate) heard_earlier_scroll: ScrollHandle,
    pub(crate) likely_scroll: ScrollHandle,
    pub(crate) ordering_typed: TypeAhead,
    pub(crate) queue_order: RowOrder,
    pub(crate) queue_reading: Direction,
    pub(crate) row_order: RowOrder,
    pub(crate) row_reading: Direction,
    pub(crate) keeping: bool,
    pub(crate) adding: Option<Held>,
    pub(crate) adding_songs_to: Option<PlaylistId>,
    pub(crate) name: Entity<Field>,
    pub(crate) contact: Entity<Field>,
    pub(crate) acoustid: Entity<Field>,
    pub(crate) audd: Entity<Field>,
    pub(crate) listenbrainz: Entity<Field>,
    pub(crate) subsonic: [Entity<Field>; 3],
    pub(crate) jellyfin: [Entity<Field>; 3],
    pub(crate) tidal: [Entity<Field>; 6],
    pub(crate) lastfm: [Entity<Field>; 2],
    pub(crate) spotify_client: Entity<Field>,
    pub(crate) spotify_state: SpotifyState,
    pub(crate) youtube_music: [Entity<Field>; 2],
    pub(crate) youtube_music_check: CheckingYoutubeMusic,
    pub(crate) youtube_music_sign_in: SigningInToYoutubeMusic,
    pub(crate) jellyfin_quick: QuickConnecting,
    pub(crate) discord_app: Entity<Field>,
    pub(crate) discord_icon: Entity<Field>,
    pub(crate) sync_server: Entity<Field>,
    pub(crate) sync_device: Entity<Field>,
    pub(crate) shared_name: Entity<Field>,
    pub(crate) sync_code: Entity<Field>,
    pub(crate) organising: Entity<Field>,
    pub(crate) minimum_length: Entity<Field>,
    pub(crate) typed_root: Entity<Field>,
    pub(crate) finding: Entity<Field>,
    pub(crate) finding_holds_the_caret: bool,
    pub(crate) server: Entity<ServerModel>,
    pub(crate) server_search: Entity<Field>,
    pub(crate) server_rows: UniformListScrollHandle,
    pub(crate) figure: Entity<Field>,
    pub(crate) looking: Entity<Field>,
    pub(crate) measured_as: Entity<Field>,
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
    pub(crate) armed: Option<Armed>,
    pub(crate) code_regenerated: bool,
    pub(crate) pairing_shown: bool,
    pub(super) remote_songs: Vec<super::remote::RemoteSong>,
    pub(super) remote_collapsed: bool,
    pub(super) remote_fits_from: Option<Pixels>,
    pub(super) remote_expanded_by_hand: bool,
    pub(super) remote_heading_pressed_at: Option<Point<Pixels>>,
    pub(super) remote_controls: Controls,
    pub(super) remote_scroll: ScrollHandle,
    pub(crate) driving: Option<(String, String)>,
    pub(crate) driving_redraws: Option<Task<()>>,
    pub(crate) mirror_redraws: Option<Task<()>>,
    pub(crate) driving_rails: std::collections::BTreeMap<u32, crate::views::slider::Rail>,
    pub(crate) driving_rows: usize,
    pub(crate) own_server_asked: bool,
    pub(crate) reset_everything_landed: bool,
    pub(crate) newer: Option<crate::Newer>,
    pub(crate) looking_for_updates: Task<()>,
    pub(crate) vault_named: Option<PathBuf>,
    pub(crate) earlier_plays_asked: bool,
    pub(crate) query_sort: SortOrder,
    pub(crate) query_reading: Direction,
    pub(crate) query_cap: Option<usize>,
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
    pub(crate) shelf_scrolls: RefCell<AHashMap<&'static str, ScrollHandle>>,
    came_from: Vec<Wayback>,
    goes_forward: Vec<Wayback>,
    pub(crate) titled: Option<SharedString>,
    pub(crate) artist_shows: ArtistShows,
    pub(crate) search_shows: SearchShows,
    pub(crate) search_scroll: ScrollHandle,
    pub(crate) artists_drawn: ArtistsDrawn,
    pub(crate) missing_shows: MissingShows,
    pub(crate) playlists_drawn: PlaylistsDrawn,
    pub(crate) landing_on: Option<LeftAt>,
    left_for_the_search: Option<(Pane, LeftAt)>,
    under_the_pointer: Option<UnderThePointer>,
    pub(crate) menu: Option<Menu>,
    pub(crate) record: Option<OpenedRecord>,
    left_at: AHashMap<PlaylistId, UniformListScrollHandle>,
    pub(crate) reach: Option<Reach>,
    arranged_as: Arrangement,
    waiting_for_the_results: bool,
    pub(crate) landed_by_a_jump: Option<Reach>,
    pub(crate) reached_unseen: Rc<Cell<bool>>,
    pub(crate) queue_height: Rc<Cell<Pixels>>,
    pub(crate) creeping: Option<Creeping>,
    pub(crate) creeping_on: Task<()>,
    listing_whole: Task<()>,
    pub(crate) grid_width: Rc<kit::GridWidth>,
    pub(crate) hero_width: Rc<Cell<Pixels>>,
    pub(crate) hero_height: Rc<Cell<Pixels>>,
    pub(crate) playing_room: Rc<Cell<Pixels>>,
    pub(crate) status_room: Rc<Cell<Pixels>>,
    pub(crate) inspected_room: Rc<Cell<Pixels>>,
    pub(crate) heading_room: Rc<Cell<Pixels>>,
    pub(crate) columns_fit: listing::Fitting,
    pub(crate) zones: Vec<ZoneShown>,
    pub(crate) switcher: Switcher,
    pub(crate) naming_the_zone: Option<ZoneId>,
    pub(crate) zone_name: Entity<Field>,
    following: Following,
    pub(crate) magnified: Option<Magnified>,
    pointer_inside: bool,
    pub(crate) volume_settled: Task<()>,
    volume_unkept: Option<(ZoneId, Volume)>,
    pub(crate) band_retuning: Task<()>,
    pub(crate) band_untold: Option<bool>,
    pub(crate) volume_aimed: Option<f32>,
    pub(crate) muted_from: Option<Volume>,
    type_ahead: TypeAhead,
    typing_stops: Task<()>,
    pub(crate) beneath_closes: Task<()>,
    last_typed: Option<Instant>,
    pub(crate) queue_names: QueueNames,
    drawn_at: SystemTime,
    scale: Scale,
    pub(crate) resolved: RefCell<Option<Resolved>>,
    pub(crate) shown_cover: RefCell<Option<ShownCover>>,
    pub(crate) handovers: RefCell<Handovers>,
    pub(crate) focus: FocusHandle,
    pub(crate) menu_focus: FocusHandle,
    pub(crate) search: Entity<Field>,
    pub(crate) remember_tab: bool,
    pub(crate) remember_window_size: bool,
    pub(crate) remember_settings_category: bool,
    pub(crate) last_window_size: Option<WindowSize>,
    window_size_settled: Task<()>,
    token_checked: Task<()>,
    pub(crate) signing_in: SigningIn,
    pub(crate) lastfm_signing: LastfmSigning,
    pub(crate) parts: Parts,
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

impl RootView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let global = cx.global::<ResonateApp>();
        let tabs = global.tabs;
        let remember_tab = global.remember_tab;
        let pane = global
            .last_tab
            .filter(|pane| remember_tab && pane.is_shown(tabs) && *pane != Pane::Server)
            .unwrap_or_default();
        let remember_window_size = global.remember_window_size;
        let remember_settings_category = global.remember_settings_category;
        let settings_category = if remember_settings_category {
            global.last_settings_category
        } else {
            Category::default()
        };
        let last_window_size = WindowSize::from_pixels(window.window_bounds().get_bounds().size);
        let zones = Arc::clone(&global.zones);
        let library = Arc::clone(&global.library);
        let settings = Arc::clone(&global.settings);
        let lyricists = Arc::clone(&global.lyricists);
        let attention = global.attention.clone();
        let online = global.online.clone();
        let presence = global.presence.clone();
        let reference = global.reference.clone();
        let for_the_pass = global.for_the_pass.clone();
        let corrections = Arc::clone(&global.corrections);
        let bindings = global.bindings.clone();
        let measurers = global.measurers.clone();
        let places = global.places.clone();
        let resume = global.resume;
        let organise_as = global.organise_as.clone();
        let sourcing = global.sourcing.clone();
        let engine = Arc::clone(&global.zones.first().player);
        let catalog = Arc::clone(&global.library);
        let fingerprinters = Arc::clone(&global.fingerprinters);

        let library = cx.new(|cx| {
            LibraryModel::new(
                library,
                reference,
                for_the_pass,
                &online,
                resume,
                sourcing,
                cx,
            )
        });
        let arranged_as = Arrangement {
            selection: library.read(cx).selection(),
            sorting: library.read(cx).sorting(),
            playlists: (
                library.read(cx).playlist_order(),
                library.read(cx).playlist_reading(),
            ),
            opened_rows: (RowOrder::default(), Direction::default()),
            search: (SortOrder::default(), SortOrder::default().reads()),
            shelves: (
                library.read(cx).favourited().artists,
                library.read(cx).favourited().albums,
            ),
        };
        let lyrics = cx.new(|_| LyricsModel::new(lyricists));
        let held_zones = zones.listed();
        let switcher = Switcher::of(held_zones.iter().map(|zone| (zone.id, zone.name.clone())));
        let shown_zones: Vec<ZoneShown> = held_zones
            .iter()
            .map(|zone| Self::watch_the_zone(zone, cx))
            .collect();
        let player = shown_zones
            .iter()
            .find(|zone| zone.id == ZoneId::FIRST)
            .map(|zone| zone.player.clone())
            .unwrap_or_else(|| unreachable!("the first zone is always held"));
        let visualiser = cx.new(|cx| Visualiser::new(player.clone(), cx));
        let reaching = cx.global::<ResonateApp>().online.enabled;
        let analysis =
            cx.new(|_| AnalysisModel::new(engine, catalog, Arc::clone(&fingerprinters), reaching));
        cx.observe(&analysis, |_, _, cx| cx.notify()).detach();
        let listens = cx.global::<ResonateApp>().listens.clone();
        let listen = cx.new(|_| ListenModel::new(listens));
        cx.observe(&listen, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<Toaster>(|_, cx| cx.notify()).detach();
        player.read(cx).listen_in(true);
        cx.observe_in(&library, window, |this, library, window, cx| {
            this.forget_what_has_gone(&library, cx);
            this.let_go_of_a_reach_the_rows_left(cx);
            let (relocated, arrived) = library.update(cx, |model, _| {
                (model.take_relocations(), model.take_arrival())
            });
            if !relocated.is_empty() {
                this.send(Command::Relocate(relocated), cx);
            }
            if let Some(track) = arrived {
                this.play_what_was_downloaded(track, cx);
            }
            if this.waiting_for_the_results && !library.read(cx).is_reading_the_search() {
                this.waiting_for_the_results = false;
                this.reach_the_first_result(window, cx);
            }
            cx.notify();
        })
        .detach();
        cx.observe(&lyrics, |_, _, cx| cx.notify()).detach();

        attend(&attention, window.is_window_active());
        cx.observe_window_activation(window, move |_, window, _| {
            attend(&attention, window.is_window_active());
        })
        .detach();

        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let menu_focus = cx.focus_handle();

        let search = cx.new(|cx| {
            Field::new(words::root::search_placeholder(), window, cx)
                .catching(|text| is_a_followed_link(text) || is_an_invitation(text))
                .searching()
        });
        cx.observe(&search, |this, search, cx| {
            let typed_anew = {
                let typed = search.read(cx).text();
                this.library.read(cx).narrowing().unwrap_or_default() != typed
            };
            if !typed_anew {
                return;
            }

            this.waiting_for_the_results = false;
            let query = search.read(cx).text().to_owned();
            this.set_query(query, cx);
        })
        .detach();

        cx.subscribe_in(&search, window, |this, _, Caught(link), window, cx| {
            if is_an_invitation(link) {
                this.accept_an_invitation(link, cx);
            } else {
                this.follow_link(link, window, cx);
            }
        })
        .detach();

        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
            this.library
                .update(cx, |library, cx| library.ask_elsewhere_now(cx));
            this.go_to_the_results(window, cx);
        })
        .detach();

        let zone_name = cx.new(|cx| Field::new(words::zones::name_this_zone(), window, cx));
        cx.subscribe_in(&zone_name, window, |this, _, _: &Submitted, window, cx| {
            this.zone_named(window, cx);
        })
        .detach();

        let name = cx.new(|cx| Field::new(words::root::name_placeholder(), window, cx));
        cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
            this.name_given(window, cx);
        })
        .detach();

        let contact = cx.new(|cx| {
            let mut field = Field::new(words::root::contact_placeholder(), window, cx);
            field.hold(online.contact.clone(), cx);
            field
        });
        cx.subscribe_in(&contact, window, |this, _, _: &Submitted, window, cx| {
            this.contact_given(window, cx);
        })
        .detach();
        cx.subscribe(&contact, |this, _, _: &Blurred, cx| this.contact_left(cx))
            .detach();

        let acoustid = cx.new(|cx| {
            let mut field = Field::new(words::root::acoustid_placeholder(), window, cx).masked();
            field.hold(online.acoustid_key.clone(), cx);
            field
        });
        cx.subscribe_in(&acoustid, window, |this, _, _: &Submitted, window, cx| {
            this.acoustid_key_given(window, cx);
        })
        .detach();
        cx.subscribe(&acoustid, |this, _, _: &Blurred, cx| {
            this.acoustid_key_left(cx)
        })
        .detach();

        let audd = cx.new(|cx| {
            let mut field = Field::new(words::root::audd_placeholder(), window, cx).masked();
            field.hold(online.audd_token.clone(), cx);
            field
        });
        cx.subscribe_in(&audd, window, |this, _, _: &Submitted, window, cx| {
            this.audd_token_given(window, cx);
        })
        .detach();
        cx.subscribe(&audd, |this, _, _: &Blurred, cx| this.audd_token_left(cx))
            .detach();

        let listenbrainz = cx.new(|cx| {
            let mut field =
                Field::new(words::root::listenbrainz_placeholder(), window, cx).masked();
            field.hold(online.listenbrainz_token.clone(), cx);
            field
        });
        cx.subscribe_in(
            &listenbrainz,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.listenbrainz_token_given(window, cx);
            },
        )
        .detach();
        cx.subscribe(&listenbrainz, |this, _, _: &Blurred, cx| {
            this.listenbrainz_token_left(cx)
        })
        .detach();

        let subsonic = Account::fields(Service::Subsonic, &online, window, cx);
        let jellyfin = Account::fields(Service::Jellyfin, &online, window, cx);
        let tidal = TidalAccount::fields(&online, window, cx);
        let lastfm = LastfmAccount::fields(&online, window, cx);
        let spotify_client = spotify::field(&online, window, cx);
        let youtube_music = YoutubeMusicField::fields(&online, window, cx);

        let discord_app = cx.new(|cx| {
            let mut field = Field::new(words::root::discord_app_placeholder(), window, cx);
            field.hold(
                presence.app.map(|app| app.to_string()).unwrap_or_default(),
                cx,
            );
            field
        });
        cx.subscribe_in(
            &discord_app,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.discord_app_given(window, cx);
            },
        )
        .detach();

        let discord_icon = cx.new(|cx| {
            let mut field = Field::new(words::root::discord_icon_placeholder(), window, cx);
            field.hold(
                presence
                    .icon
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                cx,
            );
            field
        });
        cx.subscribe_in(
            &discord_icon,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.discord_icon_given(window, cx);
            },
        )
        .detach();

        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let sync_server = cx.new(|cx| {
            let mut field = Field::new(
                crate::words::settings_sync::sync_server_placeholder(),
                window,
                cx,
            );
            field.hold(synchronised.server.clone(), cx);
            field
        });
        cx.subscribe_in(
            &sync_server,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.sync_server_given(window, cx);
            },
        )
        .detach();
        let sync_device = cx.new(|cx| {
            let mut field = Field::new(
                crate::words::settings_sync::sync_device_placeholder(),
                window,
                cx,
            );
            field.hold(synchronised.device.clone(), cx);
            field
        });
        cx.subscribe_in(
            &sync_device,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.sync_device_given(window, cx);
            },
        )
        .detach();
        let shared_name = cx.new(|cx| {
            let mut field = Field::new(
                crate::words::settings_sync::shared_name_placeholder(),
                window,
                cx,
            );
            field.hold(synchronised.shared_name.clone(), cx);
            field
        });
        cx.subscribe_in(
            &shared_name,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.shared_name_given(window, cx);
            },
        )
        .detach();
        let sync_code = cx.new(|cx| {
            Field::new(
                crate::words::settings_sync::sync_code_placeholder(),
                window,
                cx,
            )
        });
        cx.subscribe_in(&sync_code, window, |this, _, _: &Submitted, window, cx| {
            this.sync_code_given(window, cx);
        })
        .detach();

        let minimum_length = cx.new(|cx| {
            let mut field = Field::new(words::root::seconds_from_0_to_600(), window, cx);
            field.hold(
                cx.global::<ResonateApp>()
                    .library
                    .music_filters()
                    .minimum_length
                    .seconds()
                    .to_string(),
                cx,
            );
            field
        });
        cx.subscribe_in(
            &minimum_length,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.minimum_length_given(window, cx);
            },
        )
        .detach();

        let organising = cx.new(|cx| {
            let mut field = Field::new(words::root::organising_placeholder(), window, cx);
            field.hold(organise_as, cx);
            field
        });
        cx.subscribe_in(&organising, window, |this, _, _: &Submitted, window, cx| {
            this.layout_given(window, cx);
        })
        .detach();
        cx.subscribe(&organising, |this, _, _: &Blurred, cx| this.layout_left(cx))
            .detach();

        let (typed_words, typed_isrc) = matching::matching_fields(window, cx);
        let typed_root = cx.new(|cx| Field::new(words::root::typed_root_placeholder(), window, cx));
        cx.subscribe_in(&typed_root, window, |this, _, _: &Submitted, window, cx| {
            this.root_typed(window, cx);
        })
        .detach();
        cx.subscribe_in(&typed_root, window, |this, _, _: &Blurred, window, cx| {
            this.root_typed(window, cx);
        })
        .detach();

        let finding = cx.new(|cx| {
            Field::new(
                crate::words::settings_pane::filter_placeholder(),
                window,
                cx,
            )
        });
        cx.subscribe_in(&finding, window, |this, _, _: &Submitted, window, cx| {
            this.leave_filter(window, cx);
        })
        .detach();

        let server = cx.new(ServerModel::new);
        cx.observe(&server, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<ResonateApp>(|this, cx| {
            this.server.update(cx, |server, cx| server.refresh(cx));
            if this.pane == Pane::Server && !this.server.read(cx).is_open() {
                this.set_pane(Pane::default(), cx);
            }
        })
        .detach();

        let server_search =
            cx.new(|cx| Field::new(words::served::search_placeholder(), window, cx));
        cx.subscribe_in(
            &server_search,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.search_the_server(window, cx);
            },
        )
        .detach();

        let figure = cx.new(|cx| Field::new(words::root::figure_placeholder(), window, cx));
        cx.subscribe_in(&figure, window, |this, _, _: &Submitted, window, cx| {
            this.figure_given(window, cx);
        })
        .detach();
        cx.subscribe(&figure, |this, _, _: &Blurred, cx| this.figure_left(cx))
            .detach();

        let looking = cx.new(|cx| Field::new(words::root::looking_placeholder(), window, cx));
        cx.subscribe_in(&looking, window, |this, _, _: &Submitted, _window, cx| {
            this.look_for_a_device(cx);
        })
        .detach();

        let measured_as =
            cx.new(|cx| Field::new(words::root::measured_as_placeholder(), window, cx));

        let equaliser = cx.new(|cx| {
            let mut model = EqualiserModel::saved_on_leaving(
                places.equaliser.clone(),
                Arc::clone(&corrections),
                bindings,
                cx,
            );
            model.prefer(measurers);
            model
        });
        cx.observe(&equaliser, |this, equaliser, cx| {
            let (notice, untold) =
                equaliser.update(cx, |model, _| (model.take_notice(), model.take_untold()));
            if let Some(notice) = notice {
                toast::tell(notice, cx);
            }
            if untold {
                this.tell_the_engine(cx);
            }
        })
        .detach();

        Self::watch_remote_playback(cx);

        let mut view = Self {
            player,
            library,
            lyrics,
            visualiser,
            analysis,
            listen,
            listening_open: false,
            wrapped: None,
            reading_wrapped: None,
            sharing: None,
            shared_lists: Vec::new(),
            sharing_redraws: None,
            party_redraws: None,
            downloads_open: false,
            downloads_pressed_off: None,
            deleting: None,
            matching: None,
            typed_words,
            typed_isrc,
            party_offered: None,
            incoming: None,
            taking_in: None,
            watching_the_drag: Task::ready(()),
            weighing_the_drag: Task::ready(()),
            music_folder_standing: FolderStanding::default(),
            looking_at_the_music_folder: Task::ready(()),
            reading_the_room: Task::ready(()),
            looking_at_the_room: Task::ready(()),
            room_shape: None,
            audition: None,
            test_setup: crate::views::settings::Setup::default(),
            fitting_the_preamp: Fitting::idle(),
            making_the_vault: Task::ready(()),
            looking_at_the_typed_root: Task::ready(()),
            showing_time_left: false,
            _taking_in: Task::ready(()),
            settings_written: SettingsWriter::over(settings),
            pane,
            settings_category,
            settings_page: None,
            settings_scroll: ScrollHandle::new(),
            settings_rail_scroll: ScrollHandle::new(),
            sidebar_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            statistics_scroll: ScrollHandle::new(),
            party_scroll: ScrollHandle::new(),
            analysis_scroll: ScrollHandle::new(),
            suggestions_scroll: ScrollHandle::new(),
            artist_records_scroll: ScrollHandle::new(),
            behind_queue: Pane::default(),
            seek_rail: Rail::default(),
            seek_pointed: None,
            volume_rail: Rail::default(),
            grabbed: None,
            curve_plotted: Rc::default(),
            held_band: None,
            bitrate_graph: false,
            naming: None,
            ordering: None,
            ordering_at: Point::default(),
            ordering_scroll: ScrollHandle::new(),
            downloads_scroll: ScrollHandle::new(),
            heard_earlier_scroll: ScrollHandle::new(),
            likely_scroll: ScrollHandle::new(),
            ordering_typed: TypeAhead::default(),
            queue_order: RowOrder::default(),
            queue_reading: Direction::default(),
            row_order: RowOrder::default(),
            row_reading: Direction::default(),
            keeping: false,
            adding: None,
            adding_songs_to: None,
            name,
            contact,
            acoustid,
            audd,
            listenbrainz,
            subsonic,
            jellyfin,
            tidal,
            lastfm,
            spotify_client,
            spotify_state: SpotifyState::default(),
            youtube_music,
            youtube_music_check: CheckingYoutubeMusic::default(),
            youtube_music_sign_in: SigningInToYoutubeMusic::default(),
            jellyfin_quick: QuickConnecting::default(),
            discord_app,
            discord_icon,
            sync_server,
            sync_device,
            shared_name,
            sync_code,
            organising,
            minimum_length,
            typed_root,
            finding,
            finding_holds_the_caret: false,
            server,
            server_search,
            server_rows: UniformListScrollHandle::new(),
            figure,
            looking,
            measured_as,
            equaliser,
            controls: Controls::default(),
            standing_controls: Controls::default(),
            sheet_panels: Controls::default(),
            sheets: sheets::Standing::default(),
            announced: announcing::Announcer::default(),
            announcing_again: None,
            took_out: TakenBack::default(),
            queue_length: QueueMeasure::default(),
            queue_ordered: None,
            armed: None,
            code_regenerated: false,
            pairing_shown: false,
            remote_songs: Vec::new(),
            remote_collapsed: false,
            remote_fits_from: None,
            remote_expanded_by_hand: false,
            remote_heading_pressed_at: None,
            remote_controls: Controls::default(),
            remote_scroll: ScrollHandle::new(),
            driving: None,
            driving_redraws: None,
            mirror_redraws: None,
            driving_rails: std::collections::BTreeMap::new(),
            driving_rows: crate::views::settings::sync::DRIVEN_ROWS_AT_FIRST,
            own_server_asked: false,
            reset_everything_landed: false,
            newer: None,
            looking_for_updates: Task::ready(()),
            vault_named: None,
            earlier_plays_asked: false,
            query_sort: SortOrder::default(),
            query_reading: SortOrder::default().reads(),
            query_cap: None,
            zones: shown_zones,
            switcher,
            naming_the_zone: None,
            zone_name,
            following: Following::default(),
            queue_rows: UniformListScrollHandle::default(),
            playlist_rows: UniformListScrollHandle::default(),
            track_rows: UniformListScrollHandle::default(),
            artist_rows: UniformListScrollHandle::default(),
            album_rows: UniformListScrollHandle::default(),
            all_playlist_rows: UniformListScrollHandle::default(),
            favourite_rows: UniformListScrollHandle::default(),
            missing_rows: UniformListScrollHandle::default(),
            suggestion_rows: UniformListScrollHandle::default(),
            picker_rows: UniformListScrollHandle::default(),
            shelf_scrolls: RefCell::new(AHashMap::new()),
            came_from: Vec::new(),
            goes_forward: Vec::new(),
            titled: None,
            artist_shows: ArtistShows::default(),
            search_shows: SearchShows::default(),
            search_scroll: ScrollHandle::default(),
            artists_drawn: cx.global::<ResonateApp>().artists_drawn,
            missing_shows: MissingShows::default(),
            playlists_drawn: PlaylistsDrawn::default(),
            landing_on: None,
            left_for_the_search: None,
            under_the_pointer: None,
            menu: None,
            record: None,
            left_at: AHashMap::new(),
            reach: None,
            arranged_as,
            waiting_for_the_results: false,
            landed_by_a_jump: None,
            reached_unseen: Rc::default(),
            queue_height: Rc::default(),
            creeping: None,
            creeping_on: Task::ready(()),
            listing_whole: Task::ready(()),
            grid_width: Rc::default(),
            hero_width: Rc::new(Cell::new(px(0.0))),
            hero_height: Rc::new(Cell::new(px(0.0))),
            playing_room: Rc::new(Cell::new(px(0.0))),
            status_room: Rc::new(Cell::new(px(0.0))),
            inspected_room: Rc::new(Cell::new(px(0.0))),
            heading_room: Rc::new(Cell::new(px(0.0))),
            columns_fit: listing::Fitting::default(),
            magnified: None,
            pointer_inside: true,
            volume_settled: Task::ready(()),
            volume_unkept: None,
            band_retuning: Task::ready(()),
            band_untold: None,
            volume_aimed: None,
            muted_from: None,
            type_ahead: TypeAhead::default(),
            typing_stops: Task::ready(()),
            beneath_closes: Task::ready(()),
            last_typed: None,
            queue_names: QueueNames::default(),
            drawn_at: SystemTime::now(),
            scale: Scale::ONE,
            resolved: RefCell::new(None),
            shown_cover: RefCell::new(None),
            handovers: RefCell::default(),
            focus,
            menu_focus,
            search,
            remember_tab,
            remember_window_size,
            remember_settings_category,
            last_window_size,
            window_size_settled: Task::ready(()),
            token_checked: Task::ready(()),
            signing_in: SigningIn::default(),
            lastfm_signing: LastfmSigning::default(),
            parts: Parts::of(&cx.entity(), cx),
        };
        cx.observe_window_bounds(window, |this, window, cx| {
            this.window_bounds_changed(window, cx);
        })
        .detach();
        cx.on_app_quit(|this, cx| {
            this.keep_the_volume(cx);
            future::ready(())
        })
        .detach();
        cx.on_release(|this, _| this.keep_the_volume_as_the_window_leaves())
            .detach();
        view.look_for_a_newer_release(cx);
        view
    }

    pub(crate) const fn drawn_at(&self) -> SystemTime {
        self.drawn_at
    }

    fn window_bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(size) = WindowSize::from_pixels(window.window_bounds().get_bounds().size) else {
            return;
        };
        if self.last_window_size == Some(size) {
            return;
        }
        self.last_window_size = Some(size);
        if !self.remember_window_size {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.window_size = Some(size));

        self.window_size_settled = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WINDOW_SIZE_SETTLE).await;
            let stored = this.update(cx, |this, cx| {
                if this.remember_window_size && this.last_window_size == Some(size) {
                    this.store(&Setting::WindowSize(size), cx);
                }
            });
            let _ = stored;
        });
    }

    pub(crate) fn count_a_play(
        &mut self,
        id: ZoneId,
        player: &Entity<PlayerModel>,
        cx: &mut Context<Self>,
    ) {
        let (state, queue) = {
            let model = player.read(cx);
            (model.state().clone(), model.queue())
        };
        let Some(zone) = self.zones.iter_mut().find(|zone| zone.id == id) else {
            return;
        };
        let Some(counting) = zone.listening.heard(&state, &queue) else {
            return;
        };

        self.library.update(cx, |library, cx| match counting {
            Counting::Counts(played) => library.track_heard(played, cx),
            Counting::Hears(played) => library.track_hearing(played.heard, cx),
            Counting::Settles(played) => library.track_settled(played.heard, cx),
            Counting::Passes(heard) => library.track_passed(heard, cx),
        });
    }

    pub(crate) fn keep_the_queue(
        &mut self,
        id: ZoneId,
        player: &Entity<PlayerModel>,
        cx: &mut Context<Self>,
    ) {
        if !self.library.read(cx).resumes() {
            return;
        }
        let (state, queued) = {
            let model = player.read(cx);
            (model.state().clone(), model.queued())
        };
        let Some(zone) = self.zones.iter_mut().find(|zone| zone.id == id) else {
            return;
        };
        let Some(keep) = zone.resuming.kept(&state, &queued) else {
            return;
        };

        self.library
            .update(cx, |library, cx| library.keep_the_queue(id, keep, cx));
    }

    pub(crate) fn follow_the_playing_row(&mut self, player: &Entity<PlayerModel>, cx: &App) {
        let playing = PlayingRow::of(player.read(cx).state());
        if self
            .following
            .follows(playing, self.pane == Pane::Queue)
            .is_none()
        {
            return;
        }

        self.lift_the_playing_row(cx);
    }

    fn lift_the_playing_row(&self, cx: &App) {
        if let Some(top) = self.queue_parts(cx).opens_at() {
            self.queue_rows
                .scroll_to_item_strict(top, ScrollStrategy::Top);
        }
    }

    fn noticed(cx: &App) -> bool {
        toast::is_showing(cx)
    }

    pub(crate) fn send(&self, command: Command, cx: &mut Context<Self>) {
        let command = match command {
            Command::Load {
                items,
                start_at,
                autoplay,
            } => Command::Load {
                items: self.with_the_versions_chosen(items, false, cx),
                start_at,
                autoplay,
            },
            Command::Insert { items, at, play } => Command::Insert {
                items: self.with_the_versions_chosen(items, true, cx),
                at,
                play,
            },
            other => other,
        };
        let kind = command.kind();
        if let Err(error) = Self::zones(cx).send(self.switcher.selected(), command) {
            tracing::warn!(%error, command = ?kind, "the engine refused a command");
            toast::tell(Notice::Trouble(zones::refusal(&error, &self.switcher)), cx);
        }
    }

    pub(crate) fn send_by_position(&self, command: Command, cx: &mut Context<Self>) {
        self.player.read(cx).send_by_position(command);
    }

    pub(crate) fn report(&self, notice: Notice, cx: &mut Context<Self>) {
        toast::tell(notice, cx);
    }

    pub(crate) fn plays_in_order(&self, cx: &mut Context<Self>) {
        self.send(Command::SetShuffle(false), cx);
    }

    pub(crate) fn play_shuffled(&mut self, tracks: &[Track], cx: &mut Context<Self>) {
        self.play(tracks, somewhere_in(tracks.len()), cx);
        self.send(Command::SetShuffle(true), cx);
    }

    pub(crate) fn draw_the_artists_as(&mut self, drawn: ArtistsDrawn, cx: &mut Context<Self>) {
        if self.artists_drawn == drawn {
            return;
        }
        self.artists_drawn = drawn;
        cx.update_global::<ResonateApp, _>(|global, _| global.artists_drawn = drawn);
        self.store(&Setting::ArtistsDrawn(drawn), cx);
        cx.notify();
    }

    pub(crate) fn store(&mut self, setting: &Setting, cx: &mut Context<Self>) {
        self.store_for(self.switcher.selected(), setting, cx);
    }

    fn store_for(&mut self, zone: ZoneId, setting: &Setting, cx: &mut Context<Self>) {
        if setting.key().is_the_zones_own() && !zone.is_first() {
            return;
        }
        self.settings_written
            .change(SettingChange::Store(setting.clone()));
        self.write_the_settings(cx);
    }

    pub(crate) fn forget(&mut self, key: SettingKey, cx: &mut Context<Self>) {
        if key.is_the_zones_own() && !self.switcher.selected().is_first() {
            return;
        }
        self.settings_written.change(SettingChange::Forget(key));
        self.write_the_settings(cx);
    }

    fn write_the_settings(&mut self, cx: &mut Context<Self>) {
        let Some(landed) = self.settings_written.next_batch() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let written = landed.await;
            let landed = this.update(cx, |this, cx| {
                this.settings_written.landed();
                if let Some(notice) = unsaved(written) {
                    toast::tell(notice, cx);
                }
                this.write_the_settings(cx);
            });
            let _ = landed;
        })
        .detach();
    }

    pub(crate) fn play(&mut self, tracks: &[Track], start_at: usize, cx: &mut Context<Self>) {
        let items: Vec<QueueItem> = tracks
            .iter()
            .map(|track| QueueItem {
                id: track.id,
                location: track.location.clone(),
                span: track.span,
            })
            .collect();
        if items.is_empty() {
            return;
        }

        self.library.update(cx, |library, _| {
            library.set_playing_playlist(None);
            library.remember(tracks);
            if let Some(track) = tracks.get(start_at) {
                library.ask_about(track.album_id, track.artist_id);
            }
        });
        self.send(
            Command::Load {
                items,
                start_at,
                autoplay: true,
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn continue_from(&mut self, device: &str, cx: &mut Context<Self>) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            return;
        };
        let Continuing { cuts, at } = syncing.continue_from(device);
        if cuts.is_empty() {
            self.report(Notice::Trouble(words::root::nothing_continued(device)), cx);
            return;
        }
        let mut minting = Unclaimed::beside(&[]);
        let items = cuts
            .into_iter()
            .map(|cut| QueueItem {
                id: minting.mint(),
                location: cut.location,
                span: cut.span,
            })
            .collect();
        self.library
            .update(cx, |library, _| library.set_playing_playlist(None));
        self.send(
            Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            },
            cx,
        );
        if at != Frames::default() {
            self.send(Command::Seek(at), cx);
        }
        cx.notify();
    }

    pub(crate) fn with_playlist_entries(
        &mut self,
        playlist: PlaylistId,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Vec<PlaylistEntry>, &mut Context<Self>) + 'static,
    ) {
        if let Some(held) = self.library.read(cx).held_entries_of(playlist) {
            then(self, held, cx);
            return;
        }
        let library = self.library.read(cx).catalog();

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.playlist_entries(playlist, None) })
                .await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(entries) => then(this, entries, cx),
                Err(error) => {
                    tracing::error!(%error, playlist = playlist.get(), "a playlist could not be read");
                    toast::tell(toast::could_not(words::root::read_that_playlist(), &error), cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn play_the_playlist(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.with_playlist_entries(playlist, cx, move |this, entries, cx| {
            this.plays_in_order(cx);
            this.play_playlist(playlist, &entries, 0, true, cx);
        });
    }

    pub(crate) fn shuffle_the_playlist(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.with_playlist_entries(playlist, cx, move |this, entries, cx| {
            this.play_playlist(playlist, &entries, somewhere_in(entries.len()), true, cx);
            this.send(Command::SetShuffle(true), cx);
        });
    }

    pub(crate) fn queue_the_playlist(
        &mut self,
        playlist: PlaylistId,
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        self.with_playlist_entries(playlist, cx, move |this, entries, cx| {
            this.queue(&entries, at, cx);
        });
    }

    pub(crate) fn queue_the_playlists(
        &mut self,
        playlists: Vec<PlaylistId>,
        at: Placement,
        now: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(first) = playlists.first().copied() else {
            return;
        };
        let library = self.library.read(cx).catalog();

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let mut entries = Vec::new();
                    for playlist in playlists {
                        entries.extend(library.playlist_entries(playlist, None)?);
                    }
                    Ok::<_, resonate_library::Error>(entries)
                })
                .await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(entries) if now => {
                    this.plays_in_order(cx);
                    this.play_playlist(first, &entries, 0, false, cx);
                }
                Ok(entries) => this.queue(&entries, at, cx),
                Err(error) => {
                    tracing::error!(%error, "playlists could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_those_playlists(), &error),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    pub(crate) fn play_playlist(
        &mut self,
        playlist: PlaylistId,
        entries: &[PlaylistEntry],
        start_at: usize,
        whole: bool,
        cx: &mut Context<Self>,
    ) {
        let items = playlists::queue_items(entries, &QueueRows::default());
        if items.is_empty() {
            return;
        }
        let scanned: Vec<Track> = entries
            .iter()
            .filter_map(|entry| entry.track.clone())
            .collect();

        let queue = stamp_of(&items);
        self.library.update(cx, |library, _| {
            library.remember(&scanned);
            library.set_playing_playlist(whole.then_some(Playing { playlist, queue }));
        });
        self.send(
            Command::Load {
                items,
                start_at,
                autoplay: true,
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn queue(
        &mut self,
        entries: &[PlaylistEntry],
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        let items = playlists::queue_items(entries, &self.player.read(cx).queue());
        let rows = items.len();
        if rows == 0 {
            return;
        }
        let scanned: Vec<Track> = entries
            .iter()
            .filter_map(|entry| entry.track.clone())
            .collect();

        let play = self.player.read(cx).state().current.is_none();

        self.library
            .update(cx, |library, _| library.remember(&scanned));
        self.send(Command::Insert { items, at, play }, cx);
        self.report(Notice::Done(queued(rows, at, play)), cx);
        cx.notify();
    }

    pub(crate) fn hold_for_a_playlist(
        &mut self,
        holding: Held,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if holding.rows.is_empty() {
            return;
        }
        self.naming = None;
        self.ordering = None;
        self.adding = Some(holding);
        self.name.update(cx, |name, cx| {
            name.clear(cx);
            name.take_focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn name_a_playlist(
        &mut self,
        naming: Naming,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adding = None;
        self.ordering = None;
        self.naming = Some(naming);

        let found = naming
            .playlist()
            .and_then(|playlist| self.saved(playlist, cx));
        let query = found
            .as_ref()
            .and_then(|found| found.query.clone())
            .unwrap_or_default();
        self.query_sort = query.sort;
        self.query_reading = query.reading;
        self.query_cap = query.limit;

        let held = found.map_or_else(String::new, |found| found.name);
        self.name.update(cx, |name, cx| {
            name.set_text(held, cx);
            name.take_focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn revise_search(
        &mut self,
        playlist: PlaylistId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self
            .saved(playlist, cx)
            .and_then(|found| found.query)
            .and_then(|query| query.text)
            .unwrap_or_default();

        self.show_everything(cx);
        self.search
            .update(cx, |search, cx| search.set_text(text, cx));
        self.set_pane(Pane::Tracks, cx);
        self.name_a_playlist(Naming::Query(Some(playlist)), window, cx);
    }

    pub(crate) fn sort_a_playlist(
        &mut self,
        playlist: PlaylistId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.naming = None;
        self.adding = None;

        self.read_the_kept_order(Some(playlist), cx);
        self.order_a_listing(Ordered::Rows(playlist), at, cx);
    }

    fn read_the_kept_order(&mut self, playlist: Option<PlaylistId>, cx: &App) {
        let kept = playlist
            .and_then(|playlist| self.saved(playlist, cx))
            .and_then(|found| found.kept);
        self.keeping = kept.is_some();
        let Kept { order, reading } = kept.unwrap_or(Kept {
            order: RowOrder::default(),
            reading: Direction::default(),
        });
        self.row_order = order;
        self.row_reading = reading;
    }

    pub(crate) fn put_in_order(
        &mut self,
        playlist: PlaylistId,
        order: RowOrder,
        reading: Direction,
        cx: &mut Context<Self>,
    ) {
        self.row_order = order;
        self.row_reading = reading;
        let kept = self.keeping.then_some(Kept { order, reading });

        self.library.update(cx, |library, cx| match kept {
            Some(kept) => library.keep_playlist_in_order(playlist, Some(kept), cx),
            None => library.sort_playlist(playlist, order, reading, cx),
        });
        self.let_go_of_a_reach_the_rows_left(cx);
        cx.notify();
    }

    pub(crate) fn keep_in_order(
        &mut self,
        playlist: PlaylistId,
        keeping: bool,
        cx: &mut Context<Self>,
    ) {
        if self.keeping == keeping {
            return;
        }
        self.keeping = keeping;
        let kept = keeping.then_some(Kept {
            order: self.row_order,
            reading: self.row_reading,
        });

        self.library.update(cx, |library, cx| {
            library.keep_playlist_in_order(playlist, kept, cx);
        });
        cx.notify();
    }

    pub(crate) fn undo_edit(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        if self.pane == Pane::Queue && self.put_the_queue_back(cx) {
            return;
        }
        self.library.update(cx, |library, cx| library.undo(cx));
    }

    pub(crate) fn redo_edit(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        if self.pane == Pane::Queue && self.take_the_queue_out_again(cx) {
            return;
        }
        self.library.update(cx, |library, cx| library.redo(cx));
    }

    pub(crate) fn sort_a_search(&mut self, sort: SortOrder, cx: &mut Context<Self>) {
        self.query_sort = sort;
        self.query_reading = sort.reads();
        self.let_go_of_a_reach_the_rows_left(cx);
        cx.notify();
    }

    pub(crate) fn read_a_search(&mut self, reading: Direction, cx: &mut Context<Self>) {
        self.query_reading = reading;
        self.let_go_of_a_reach_the_rows_left(cx);
        cx.notify();
    }

    pub(crate) fn cap_a_search(&mut self, cap: Option<usize>, cx: &mut Context<Self>) {
        self.query_cap = cap;
        cx.notify();
    }

    fn saved(&self, playlist: PlaylistId, cx: &App) -> Option<Playlist> {
        self.library.read(cx).saved_playlist(playlist).cloned()
    }

    pub(crate) fn stop_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        self.adding = None;
        self.name.update(cx, |name, cx| name.clear(cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn name_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.name.read(cx).text().trim().to_owned();
        if given.is_empty() {
            return;
        }

        match (self.naming, self.adding.take()) {
            (_, Some(holding)) => self.library.update(cx, |library, cx| {
                library.create_playlist(given, holding.rows.to_vec(), cx);
            }),
            (Some(Naming::New), None) => self.library.update(cx, |library, cx| {
                library.create_playlist(given, Vec::new(), cx);
            }),
            (Some(Naming::Query(saved)), None) => {
                let library = self.library.read(cx);
                let text = library
                    .meant()
                    .map_or(library.query(), |meant| meant.searched.as_str())
                    .trim()
                    .to_owned();
                let query = SavedQuery {
                    text: (!text.is_empty()).then_some(text),
                    sort: self.query_sort,
                    reading: self.query_reading,
                    limit: self.query_cap,
                };
                self.library.update(cx, |library, cx| match saved {
                    Some(playlist) => library.revise_query(playlist, given, query, cx),
                    None => library.save_query(given, query, cx),
                });
                if saved.is_some() {
                    self.set_pane(Pane::Playlists, cx);
                }
            }
            (Some(Naming::Rename(playlist)), None) => self.library.update(cx, |library, cx| {
                library.rename_playlist(playlist, given, cx);
            }),
            (None, None) => return,
        }
        self.stop_naming(window, cx);
    }

    fn contact_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_contact(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn contact_left(&mut self, cx: &mut Context<Self>) {
        if self.contact.read(cx).text().trim() != cx.global::<ResonateApp>().online.contact {
            self.take_the_contact(cx);
            cx.notify();
        }
    }

    fn take_the_contact(&mut self, cx: &mut Context<Self>) {
        let given = self.contact.read(cx).text().trim().to_owned();
        self.contact
            .update(cx, |contact, cx| contact.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.contact = given.clone());

        let said = if given.is_empty() {
            words::root::contact_cleared()
        } else {
            words::root::contact_taken()
        };
        self.store(&Setting::Contact(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    fn acoustid_key_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_acoustid_key(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn acoustid_key_left(&mut self, cx: &mut Context<Self>) {
        if self.acoustid.read(cx).text().trim() != cx.global::<ResonateApp>().online.acoustid_key {
            self.take_the_acoustid_key(cx);
            cx.notify();
        }
    }

    fn take_the_acoustid_key(&mut self, cx: &mut Context<Self>) {
        let given = self.acoustid.read(cx).text().trim().to_owned();
        self.acoustid
            .update(cx, |key, cx| key.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.acoustid_key = given.clone());

        let said = if given.is_empty() {
            words::root::acoustid_key_cleared()
        } else {
            words::root::acoustid_key_taken()
        };
        self.store(&Setting::AcoustidKey(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn leave_acoustid_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.acoustid.read(cx).is_focused(window) {
            return;
        }
        if self.acoustid.read(cx).text().trim() != cx.global::<ResonateApp>().online.acoustid_key {
            self.acoustid_key_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_acoustid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.acoustid_key.to_owned();
        self.acoustid.update(cx, |field, cx| field.hold(stored, cx));
        self.leave_acoustid_key(window, cx);
    }

    fn audd_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_audd_token(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn audd_token_left(&mut self, cx: &mut Context<Self>) {
        if self.audd.read(cx).text().trim() != cx.global::<ResonateApp>().online.audd_token {
            self.take_the_audd_token(cx);
            cx.notify();
        }
    }

    fn take_the_audd_token(&mut self, cx: &mut Context<Self>) {
        let given = self.audd.read(cx).text().trim().to_owned();
        self.audd
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.audd_token = given.clone());

        let said = if given.is_empty() {
            words::root::audd_token_cleared()
        } else {
            words::root::audd_token_taken()
        };
        self.store(&Setting::AuddToken(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn leave_audd_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.audd.read(cx).is_focused(window) {
            return;
        }
        if self.audd.read(cx).text().trim() != cx.global::<ResonateApp>().online.audd_token {
            self.audd_token_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_audd(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.audd_token.to_owned();
        self.audd.update(cx, |field, cx| field.hold(stored, cx));
        self.leave_audd_token(window, cx);
    }

    fn listenbrainz_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_listenbrainz_token(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn listenbrainz_token_left(&mut self, cx: &mut Context<Self>) {
        if self.listenbrainz.read(cx).text().trim()
            != cx.global::<ResonateApp>().online.listenbrainz_token
        {
            self.take_the_listenbrainz_token(cx);
            cx.notify();
        }
    }

    fn take_the_listenbrainz_token(&mut self, cx: &mut Context<Self>) {
        let given = self.listenbrainz.read(cx).text().trim().to_owned();
        self.listenbrainz
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.listenbrainz_token = given.clone();
        });

        let said = if given.is_empty() {
            words::root::listens_not_sent()
        } else {
            words::root::listens_sent()
        };
        self.store(&Setting::ListenbrainzToken(given.clone()), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        self.check_the_listenbrainz_token(given, cx);
    }

    fn check_the_listenbrainz_token(&mut self, token: String, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(scrobblers) = app.scrobblers.clone().filter(|_| app.online.enabled) else {
            return;
        };
        if token.is_empty() {
            self.token_checked = Task::ready(());
            return;
        }

        self.token_checked = cx.spawn(async move |this, cx| {
            let held = cx
                .background_executor()
                .spawn(async move { scrobblers.under(token).token_held() })
                .await;
            let outcome = this.update(cx, |this, cx| this.report(token_notice(held), cx));
            let _ = outcome;
        });
    }

    pub(crate) fn leave_listenbrainz_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.listenbrainz.read(cx).is_focused(window) {
            return;
        }
        if self.listenbrainz.read(cx).text().trim()
            != cx.global::<ResonateApp>().online.listenbrainz_token
        {
            self.listenbrainz_token_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_listenbrainz(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx
            .global::<ResonateApp>()
            .online
            .listenbrainz_token
            .to_owned();
        self.listenbrainz
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_listenbrainz_token(window, cx);
    }

    pub(crate) fn clear_listenbrainz_token(&self, cx: &mut Context<Self>) {
        self.listenbrainz
            .update(cx, |token, cx| token.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.listenbrainz_token = String::new();
        });
    }

    pub(crate) fn clear_audd_token(&self, cx: &mut Context<Self>) {
        self.audd
            .update(cx, |token, cx| token.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.audd_token = String::new());
    }

    pub(crate) fn clear_acoustid_key(&self, cx: &mut Context<Self>) {
        self.acoustid
            .update(cx, |key, cx| key.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.acoustid_key = String::new());
    }

    pub(crate) fn leave_contact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.contact.read(cx).is_focused(window) {
            return;
        }
        if self.contact.read(cx).text().trim() != cx.global::<ResonateApp>().online.contact {
            self.contact_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_contact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.contact.to_owned();
        self.contact.update(cx, |field, cx| field.hold(stored, cx));
        self.leave_contact(window, cx);
    }

    pub(crate) fn leave_typed_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typed_root.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn leave_organising(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.organising.read(cx).is_focused(window) {
            return;
        }
        if self.organising.read(cx).text().trim() != cx.global::<ResonateApp>().organise_as {
            self.layout_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_organising(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().organise_as.clone();
        self.organising
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_organising(window, cx);
    }

    pub(crate) fn clear_contact(&self, cx: &mut Context<Self>) {
        self.contact
            .update(cx, |contact, cx| contact.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.contact = String::new());
    }

    fn say_what_this_is(&mut self, cx: &mut Context<Self>) {
        self.set_pane(Pane::Settings, cx);
        self.show_settings(Category::About, cx);
    }

    pub(crate) fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_pane(Pane::Settings, cx);
        self.finding
            .update(cx, |finding, cx| finding.take_focus_and_select(window, cx));
        cx.notify();
    }

    pub(crate) fn leave_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.finding.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn in_front(&self, cx: &App) -> Pane {
        in_front_of(self.pane, self.library.read(cx).selection())
    }

    pub(crate) fn choose_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let leaving = self.here_now(cx);
        let library = self.library.read(cx);
        let landing = landing(
            pane,
            self.in_front(cx),
            library.selection(),
            library.opened().is_some(),
        );

        match landing {
            Landing::On(pane) => self.set_pane(pane, cx),
            Landing::Unscoped(pane) => {
                let came_from = mem::take(&mut self.came_from);
                self.show_everything(cx);
                self.came_from = came_from;
                self.set_pane(pane, cx);
            }
            Landing::Scoped => self.set_pane(Pane::Tracks, cx),
            Landing::EveryPlaylist => {
                self.show_playlist(None, cx);
                self.set_pane(Pane::Playlists, cx);
            }
        }

        let arrived = (self.pane, self.library.read(cx).selection());
        if arrived != (leaving.pane, leaving.selection) {
            let leaving = Wayback {
                left_by: Leaving::ByATab,
                ..leaving
            };
            Self::keep_wayback(&mut self.came_from, leaving);
            self.goes_forward.clear();
        }
    }

    pub(crate) fn show_in_the_search(&mut self, shows: SearchShows, cx: &mut Context<Self>) {
        if let Some(pane) = shows.pane() {
            self.set_pane(pane, cx);
        }
        if self.search_shows != shows {
            self.ordering = None;
            self.reach = None;
        }
        self.search_shows = shows;
        cx.notify();
    }

    pub(crate) fn set_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let serving = self.server.read(cx).is_open();
        let pane = if pane.is_there(cx.global::<ResonateApp>().tabs, serving) {
            pane
        } else {
            Pane::default()
        };
        if pane != Pane::Tracks {
            self.adding_songs_to = None;
        }
        pointed::forget();
        self.record = None;
        self.stop_typing(cx);
        let opens_the_queue = pane == Pane::Queue && self.pane != Pane::Queue;
        if self.pane != pane {
            self.ordering = None;
            if let Some(shows) = SearchShows::in_place_of(pane) {
                self.search_shows = shows;
            }
        }
        self.pane = pane;
        self.server
            .update(cx, |server, cx| server.shown(pane == Pane::Server, cx));
        if opens_the_queue {
            self.following = Following::default();
            let player = self.player.clone();
            self.follow_the_playing_row(&player, cx);
        }
        self.landing_on = None;
        self.remember_current_tab(cx);
        cx.notify();
    }

    pub(crate) fn shift_rows(
        &mut self,
        shift: Shift,
        rows: Span,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        match shift {
            Shift::Listing(_) => {}
            Shift::Queue => self.send_by_position(Command::Move { rows, to }, cx),
            Shift::Playlist(playlist) => self.library.update(cx, |library, cx| {
                library.move_in_playlist(playlist, rows, to, cx);
            }),
        }
    }

    pub(crate) fn shift_chosen(
        &mut self,
        shift: Shift,
        chosen: &Chosen,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(rows) = chosen.single() {
            self.shift_rows(shift, rows, to, cx);
            return;
        }
        let Shift::Playlist(playlist) = shift else {
            return;
        };

        let held = self.library.read(cx).entries().len();
        let landed = chosen.gathered_by(to, held);
        let runs = chosen.runs().to_vec();
        self.library.update(cx, |library, cx| {
            library.gather_in_playlist(playlist, runs, to, cx);
        });
        if let Some(landed) = landed {
            self.reach = Some(Reach::spanning(shift, landed.first(), landed.last()));
        }
        self.landed_by_a_jump = None;
        cx.notify();
    }

    fn arrangement(&self, cx: &App) -> Arrangement {
        let library = self.library.read(cx);

        Arrangement {
            selection: library.selection(),
            sorting: library.sorting(),
            playlists: (library.playlist_order(), library.playlist_reading()),
            opened_rows: (self.row_order, self.row_reading),
            search: (self.query_sort, self.query_reading),
            shelves: (library.favourited().artists, library.favourited().albums),
        }
    }

    fn let_go_of_a_reach_the_rows_left(&mut self, cx: &App) {
        let arranged_as = self.arrangement(cx);
        if arranged_as == self.arranged_as {
            return;
        }
        self.arranged_as = arranged_as;
        if self
            .reach
            .as_ref()
            .is_some_and(|reach| reach.shift != Shift::Queue)
        {
            self.reach = None;
        }
    }

    pub(crate) fn reaches(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift && reach.reaches(row))
    }

    pub(crate) fn at_the_reach(&self, shift: Shift, row: usize) -> Option<AtTheReach> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift && reach.row == row)
            .map(|_| AtTheReach { shift, row })
    }

    fn open_the_reached_menu(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return;
        };
        let at = AtTheReach {
            shift,
            row: reach.row,
        };
        if let Some(menu) = menu::offered_at_the_reach(at, self, cx) {
            self.open_a_menu(menu, cx);
        }
    }

    #[cfg(test)]
    pub(crate) const fn reached(&self) -> Option<&Reach> {
        self.reach.as_ref()
    }

    pub(crate) fn stands_unchosen_on(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift && reach.row == row && !reach.is_chosen())
    }

    pub(crate) fn acting_on_chosen(&self, shift: Shift, row: usize) -> Chosen {
        match self.reach.as_ref().filter(|reach| reach.shift == shift) {
            Some(reach) if reach.reaches(row) => reach.selected(),
            _ => Chosen::of(Span::one(row)),
        }
    }

    pub(crate) fn choosable(&self, shift: Shift, chosen: Chosen, cx: &App) -> Chosen {
        chosen.within(self.choosable_rows(shift, cx))
    }

    fn choosable_rows(&self, shift: Shift, cx: &App) -> usize {
        match shift {
            Shift::Listing(Listed::Top) => TopRun::of(self.library.read(cx)).songs(),
            _ => usize::MAX,
        }
    }

    pub(crate) fn reached_chosen(&self, shift: Shift) -> Option<Chosen> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .map(Reach::selected)
    }

    pub(crate) fn tracks_chosen(&self, shift: Shift, chosen: &Chosen, cx: &App) -> Vec<Track> {
        let library = self.library.read(cx);
        let picked = |tracks: &[Track]| -> Vec<Track> {
            chosen
                .rows()
                .filter_map(|row| tracks.get(row).cloned())
                .collect()
        };

        match shift {
            Shift::Listing(Listed::Tracks) => library.tracks_at(chosen.rows()),
            Shift::Listing(Listed::Favourites) => picked(&library.favourite_tracks()),
            Shift::Listing(Listed::Suggested) => library
                .opened_suggestion()
                .map_or_else(Vec::new, |opened| picked(&opened.tracks)),
            Shift::Listing(Listed::Top) => {
                let songs = TopRun::of(library).songs();
                picked(&library.listing()[..songs.min(library.listing().len())])
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn albums_chosen(&self, shift: Shift, chosen: &Chosen, cx: &App) -> Vec<AlbumId> {
        let library = self.library.read(cx);
        match shift {
            Shift::Listing(Listed::FavouriteAlbums) => {
                let shelf = library.favourite_albums();
                chosen
                    .rows()
                    .filter_map(|row| shelf.get(row).map(|album| album.id))
                    .collect()
            }
            _ => library.albums_at(chosen.rows()),
        }
    }

    pub(crate) fn artists_chosen(&self, shift: Shift, chosen: &Chosen, cx: &App) -> Vec<ArtistId> {
        let library = self.library.read(cx);
        match shift {
            Shift::Listing(Listed::FavouriteArtists) => {
                let shelf = library.favourite_artists();
                chosen
                    .rows()
                    .filter_map(|row| shelf.get(row).map(|artist| artist.id))
                    .collect()
            }
            _ => library.artists_at(chosen.rows()),
        }
    }

    pub(crate) fn marked_where_reached<E: Styled + FluentBuilder + Spoken>(
        &self,
        shift: Shift,
        row: usize,
        listed: E,
    ) -> E {
        let reached = self.reaches(shift, row);
        reorder::cursor_marked(
            reorder::marked(listed.chosen(reached), reached),
            self.stands_unchosen_on(shift, row),
        )
    }

    pub(crate) fn reach_by_click(
        &mut self,
        shift: Shift,
        row: usize,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let apart = modifiers.secondary(cx);
        if !shift.chooses_apart() || !(apart || modifiers.shift) {
            return false;
        }
        let choosable = self.choosable_rows(shift, cx);
        if row >= choosable {
            return false;
        }
        self.reach_by_hand(cx);

        let held = self
            .reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .and_then(|reach| reach.within(choosable));
        self.reach = Some(match held {
            Some(reach) if apart => reach.toggled(row),
            Some(reach) => reach.widened_to(row),
            None => Reach::at(shift, row),
        });
        cx.notify();

        true
    }

    fn choose_the_reached(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let choosable = self.choosable_rows(shift, cx);
        if !shift.chooses_apart() || choosable == 0 {
            return;
        }

        self.reach = Some(match self.reach_in(shift, held) {
            Some(reach) if reach.row >= choosable => return,
            Some(reach) => reach.toggled(reach.row),
            None => Reach::at(shift, self.opening_row(shift, held, cx)),
        });
        cx.notify();
    }

    fn hop_to_a_row(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        if !shift.chooses_apart() {
            return;
        }
        let Some(reach) = self.reach_in(shift, held) else {
            return self.reach_row(step, cx);
        };
        let Some(row) = step.landing(Span::one(reach.row), held) else {
            return;
        };
        let choosable = self.choosable_rows(shift, cx);
        if row >= choosable || reach.row >= choosable {
            return;
        }

        self.reach = Some(reach.hopped_to(row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    pub(crate) fn acting_on(&self, shift: Shift, row: usize) -> Span {
        match self.reaching(shift) {
            Some(rows) if rows.holds(row) => rows,
            _ => Span::one(row),
        }
    }

    pub(crate) fn reaching(&self, shift: Shift) -> Option<Span> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .map(Reach::rows)
    }

    pub(crate) fn reach_at(
        &mut self,
        shift: Shift,
        row: usize,
        extending: bool,
        cx: &mut Context<Self>,
    ) {
        let held = self.reach.as_ref().filter(|reach| reach.shift == shift);
        self.landed_by_a_jump = None;
        self.reach = Some(match (extending, held) {
            (true, Some(reach)) => reach.widened_to(row),
            _ => Reach::at(shift, row),
        });
        cx.notify();
    }

    fn reach_row(&mut self, step: Step, cx: &mut Context<Self>) {
        self.reach_by_hand(cx);
        if self.step_the_menu(
            match step {
                Step::Above => -1,
                Step::Below => 1,
            },
            cx,
        ) || self.something_stands_over_the_pane()
        {
            return;
        }

        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        if self.reach_across_the_favourites(shift, held, step, cx) {
            return;
        }
        let row = match self.reach_in(shift, held) {
            Some(reach) => self.line_beyond(shift, reach, step, held, cx),
            None => self.opening_row(shift, held, cx),
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    fn reach_across_the_favourites(
        &mut self,
        shift: Shift,
        held: usize,
        step: Step,
        cx: &mut Context<Self>,
    ) -> bool {
        let Shift::Listing(from) = shift else {
            return false;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return false;
        };
        let shelved = Shelved::of(self.library.read(cx));
        if shelved.rows_of(from) == 0 {
            return false;
        }
        let leaves =
            Shelved::is_a_strip(from) || (step == Step::Above && step.end(reach.rows()) == 0);
        if !leaves {
            return false;
        }

        if let Some(into) = shelved.beyond(from, step) {
            let shift = Shift::Listing(into);
            self.reach = Some(Reach::at(shift, 0));
            self.show_row(shift, 0, cx);
            cx.notify();
        }

        true
    }

    fn line_beyond(&self, shift: Shift, reach: Reach, step: Step, held: usize, cx: &App) -> usize {
        let end = step.end(reach.rows());
        let landing = match shift {
            Shift::Listing(Listed::Top) => TopRun::of(self.library.read(cx)).line_beyond(end, step),
            _ => step.landing(reach.rows(), held),
        };

        landing.unwrap_or(end)
    }

    fn reach_along_the_strip(&mut self, step: Step, cx: &mut Context<Self>) -> bool {
        if self.something_stands_over_the_pane() {
            return false;
        }
        let Some((shift @ Shift::Listing(listed), held)) = self.reachable(cx) else {
            return false;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return false;
        };
        let at = step.end(reach.rows());
        let along = match listed {
            Listed::Top => TopRun::of(self.library.read(cx)).along_the_strip(at, step),
            _ => Shelved::of(self.library.read(cx)).along_the_strip(listed, at, step),
        };
        let Some(along) = along else {
            return false;
        };
        if let Some(row) = along {
            self.reach = Some(Reach::at(shift, row));
            self.show_row(shift, row, cx);
            cx.notify();
        }

        true
    }

    fn reach_the_end(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let row = match step {
            Step::Above => 0,
            Step::Below => held - 1,
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    fn reach_a_page(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let page = self.rows_a_page(shift);
        let from = match self.reach_in(shift, held) {
            Some(reach) => step.end(reach.rows()),
            None => self.opening_row(shift, held, cx),
        };
        let row = match step {
            Step::Above => from.saturating_sub(page),
            Step::Below => from.saturating_add(page).min(held - 1),
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    fn reach_everything(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };

        self.reach = Some(Reach::spanning(shift, 0, held - 1));
        cx.notify();
    }

    fn drop_reached(&mut self, cx: &mut Context<Self>) -> bool {
        if self.something_stands_over_the_pane() {
            return false;
        }
        let Some((shift, held)) = self.reachable(cx) else {
            return false;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return false;
        };
        if !shift.is_edited() {
            return false;
        }
        if matches!(shift, Shift::Playlist(_)) && !self.opened_rows(cx).are_edited() {
            return false;
        }

        self.drop_chosen(shift, &reach.acted_on(), cx);
        true
    }

    fn rows_a_page(&self, shift: Shift) -> usize {
        let listing = match shift {
            Shift::Queue => &self.queue_rows,
            Shift::Playlist(_) => &self.playlist_rows,
            Shift::Listing(Listed::Tracks) => &self.track_rows,
            Shift::Listing(Listed::Albums) => &self.album_rows,
            Shift::Listing(Listed::Artists) => &self.artist_rows,
            Shift::Listing(Listed::Playlists) => &self.all_playlist_rows,
            Shift::Listing(Listed::Favourites) => &self.favourite_rows,
            Shift::Listing(Listed::FavouriteArtists | Listed::FavouriteAlbums) => return 1,
            Shift::Listing(Listed::Missing) => &self.missing_rows,
            Shift::Listing(Listed::Suggested) => &self.suggestion_rows,
            Shift::Listing(Listed::Top) => {
                return search::SONGS_AT_THE_TOP + search::FOUND_AT_THE_TOP;
            }
            Shift::Listing(Listed::Offered) => return self.cards_a_page(),
            Shift::Listing(Listed::Heard) => return self.heard_a_page(),
        };
        let shown = listing
            .0
            .borrow()
            .last_item_size
            .map_or(0.0, |size| f32::from(size.item.height));

        let in_a_grid = shift == Shift::Listing(Listed::Albums)
            || (shift == Shift::Listing(Listed::Artists)
                && self.artists_drawn == ArtistsDrawn::Grid)
            || (shift == Shift::Listing(Listed::Playlists)
                && self.playlists_drawn == PlaylistsDrawn::Grid);
        if in_a_grid {
            return ((shown / self.grid_row()) as usize).max(1) * self.grid_columns();
        }
        ((shown / theme::row_height()) as usize).max(1)
    }

    fn widen_reach(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return self.reach_row(step, cx);
        };
        let Some(row) = step.landing(Span::one(reach.row), held) else {
            return;
        };

        self.reach = Some(reach.widened_to(row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    fn move_reached_rows(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        let Some((shift, held)) = self.movable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return;
        };
        let chosen = reach.acted_on();
        let Some(to) = chosen
            .bounds()
            .and_then(|bounds| step.landing(bounds, held))
        else {
            return;
        };
        let landed = chosen.gathered_by(to, held);

        self.shift_chosen(shift, &chosen, to, cx);
        match (chosen.single(), landed) {
            (Some(rows), _) if rows == reach.rows() => self.reach = Some(reach.stepped(step)),
            (Some(_), Some(landed)) => {
                self.reach = Some(Reach::spanning(shift, landed.first(), landed.last()));
            }
            _ => {}
        }
        self.landed_by_a_jump = None;
        self.show_row(shift, to, cx);
        cx.notify();
    }

    fn play_reached_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.press_the_menu(window, cx) || self.something_stands_over_the_pane() {
            return;
        }

        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return;
        };
        let chosen = self.choosable(shift, reach.selected(), cx);
        let row = if shift.chooses_apart() {
            chosen.first().unwrap_or(reach.row)
        } else {
            reach.rows().first()
        };

        if shift.plays_the_chosen_rows() && chosen.count() > 1 {
            self.play_the_chosen(shift, &chosen, window, cx);
            return;
        }

        match shift {
            Shift::Queue => self.send_by_position(Command::JumpTo(row), cx),
            Shift::Playlist(playlist) => {
                let entries = self.library.read(cx).entries().to_vec();
                self.play_playlist(playlist, &entries, row, true, cx);
            }
            Shift::Listing(Listed::Tracks) => {
                if let Some(found) = self.library.read(cx).found_at(row) {
                    self.library
                        .update(cx, |library, cx| library.fetch_and_play(found, cx));
                    return;
                }
                let Some((played, start)) = self.library.read(cx).played_from(row) else {
                    return;
                };
                self.play_the_listing_from(&played, start, window, cx);
            }
            Shift::Listing(Listed::Albums) => {
                let Some(album) = self.library.read(cx).albums().get(row).map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Album(album), cx);
            }
            Shift::Listing(Listed::Artists) => {
                let Some(artist) = self.library.read(cx).artists().get(row).map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Artist(artist), cx);
            }
            Shift::Listing(Listed::Playlists) => {
                let Some(playlist) = self
                    .library
                    .read(cx)
                    .playlists()
                    .get(row)
                    .map(|held| held.id)
                else {
                    return;
                };
                self.show_playlist(Some(playlist), cx);
            }
            Shift::Listing(Listed::Favourites) => {
                let tracks = self.library.read(cx).favourite_tracks();
                self.play(&tracks, row, cx);
            }
            Shift::Listing(Listed::FavouriteArtists) => {
                let Some(artist) = self
                    .library
                    .read(cx)
                    .favourite_artists()
                    .get(row)
                    .map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Artist(artist), cx);
            }
            Shift::Listing(Listed::FavouriteAlbums) => {
                let Some(album) = self
                    .library
                    .read(cx)
                    .favourite_albums()
                    .get(row)
                    .map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Album(album), cx);
            }
            Shift::Listing(Listed::Missing) => self.open_what_is_missing_at(row, cx),
            Shift::Listing(Listed::Top) => self.press_at_the_top(row, window, cx),
            Shift::Listing(Listed::Suggested) => {
                let Some(tracks) = self
                    .library
                    .read(cx)
                    .opened_suggestion()
                    .map(|opened| Arc::clone(&opened.tracks))
                else {
                    return;
                };
                self.play(&tracks, row, cx);
            }
            Shift::Listing(Listed::Offered) => {
                let Some(query) = self.offered_in_shelf_order(cx).into_iter().nth(row) else {
                    return;
                };
                self.open_offered(query, cx);
            }
            Shift::Listing(Listed::Heard) => self.open_what_was_heard_at(row, cx),
        }
    }

    fn play_the_chosen(
        &mut self,
        shift: Shift,
        chosen: &Chosen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match shift {
            Shift::Listing(Listed::Albums | Listed::FavouriteAlbums) => {
                let albums = self.albums_chosen(shift, chosen, cx);
                self.plays_everything_in_each(
                    albums.into_iter().map(Selection::Album).collect(),
                    Placement::Queued,
                    true,
                    window,
                    cx,
                );
            }
            Shift::Listing(Listed::Artists | Listed::FavouriteArtists) => {
                let artists = self.artists_chosen(shift, chosen, cx);
                self.plays_everything_in_each(
                    artists.into_iter().map(Selection::Artist).collect(),
                    Placement::Queued,
                    true,
                    window,
                    cx,
                );
            }
            Shift::Listing(Listed::Playlists) => {
                let playlists = self.library.read(cx).playlists_at(chosen.rows());
                self.queue_the_playlists(playlists, Placement::Queued, true, cx);
            }
            _ => {
                let tracks = self.tracks_chosen(shift, chosen, cx);
                self.play(&tracks, 0, cx);
            }
        }
    }

    pub(crate) fn drop_queue_runs(&mut self, runs: Vec<Span>, cx: &mut Context<Self>) {
        let queued = self.player.read(cx).queue();
        let kept = self.took_out.keeping_all(&queued, &runs);

        self.reach = Reach::after_dropping_runs(Shift::Queue, &runs, queued.len());
        self.landed_by_a_jump = None;

        self.send_by_position(Command::RemoveAll(runs), cx);
        if let Some(kept) = kept {
            toast::tell(took_out(kept), cx);
        }
        cx.notify();
    }

    pub(crate) fn drop_chosen(&mut self, shift: Shift, chosen: &Chosen, cx: &mut Context<Self>) {
        if let Some(rows) = chosen.single() {
            self.drop_rows(shift, rows, cx);
            return;
        }

        match shift {
            Shift::Queue => self.drop_queue_runs(chosen.runs().to_vec(), cx),
            Shift::Playlist(playlist) => {
                let held = self.library.read(cx).entries().len();
                let runs = chosen.runs().to_vec();
                self.reach = Reach::after_dropping_runs(shift, &runs, held);
                self.landed_by_a_jump = None;
                self.library.update(cx, |library, cx| {
                    library.remove_runs_from_playlist(playlist, runs, cx);
                });
                cx.notify();
            }
            Shift::Listing(_) => {}
        }
    }

    pub(crate) fn drop_rows(&mut self, shift: Shift, rows: Span, cx: &mut Context<Self>) {
        let held = match shift {
            Shift::Queue => {
                self.drop_queue_runs(vec![rows], cx);
                return;
            }
            Shift::Playlist(playlist) => {
                let held = self.library.read(cx).entries().len();
                self.library.update(cx, |library, cx| {
                    library.remove_from_playlist(playlist, rows, cx);
                });
                held
            }
            Shift::Listing(_) => return,
        };

        self.reach = Reach::after_dropping(shift, rows, held);
        self.landed_by_a_jump = None;
        cx.notify();
    }

    pub(crate) fn with_everything_listed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Arc<[Track]>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (library, asked) = self.library.read(cx).listing_whole();
        let drawn = self.library.read(cx).as_drawn();

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok(listing) => then(this, drawn.ordered(listing.into()), window, cx),
                Err(error) => {
                    tracing::error!(%error, "the whole listing could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_that_listing(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    fn with_the_whole_library(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Arc<[Track]>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (library, asked) = self.library.read(cx).whole_library();

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok(tracks) => then(this, tracks.into(), window, cx),
                Err(error) => {
                    tracing::error!(%error, "the whole library could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_the_library(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    pub(crate) fn play_the_listing_from(
        &mut self,
        held: &[Track],
        start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pressed = held.get(start).map(|track| track.id);
        let shuffled = self.player.read(cx).state().shuffle;
        if shuffled && self.library.read(cx).is_searching_the_whole_library() {
            self.with_the_whole_library(window, cx, move |this, library, window, cx| {
                let found = pressed
                    .and_then(|pressed| library.iter().position(|track| track.id == pressed));
                match found {
                    Some(start) => this.play(&library, start, cx),
                    None => this.play_from_the_listing_read_whole(pressed, start, window, cx),
                }
            });
            return;
        }
        if self.library.read(cx).is_the_whole_listing(held) {
            self.play(held, start, cx);
            return;
        }
        self.play_from_the_listing_read_whole(pressed, start, window, cx);
    }

    fn play_from_the_listing_read_whole(
        &mut self,
        pressed: Option<TrackId>,
        start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_everything_listed(window, cx, move |this, listing, _, cx| {
            let found = match pressed {
                Some(pressed) => listing.iter().position(|track| track.id == pressed),
                None => (start < listing.len()).then_some(start),
            };
            let Some(start) = found else {
                tracing::warn!(
                    ?pressed,
                    start,
                    "the row pressed is not in the listing read whole"
                );
                return;
            };
            this.play(&listing, start, cx);
        });
    }

    pub(crate) fn with_the_rows_of(
        &mut self,
        query: &SavedQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Arc<[Track]>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (library, asked) = self.library.read(cx).rows_of(query);

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok(rows) => then(this, rows.into(), window, cx),
                Err(error) => {
                    tracing::error!(%error, "a suggestion's rows could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_that_suggestion(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    pub(crate) fn plays_everything_in(
        &mut self,
        scoped: Selection,
        at: Placement,
        now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.plays_everything_in_each(vec![scoped], at, now, window, cx);
    }

    pub(crate) fn plays_everything_in_each(
        &mut self,
        scopes: Vec<Selection>,
        at: Placement,
        now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let library = self.library.read(cx).catalog();

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read_each(&library, scopes) })
                .await;

            let outcome = this.update(cx, |this, cx| match read {
                Ok(listing) if now => this.play(&listing, 0, cx),
                Ok(listing) => this.queue(&listed(&listing), at, cx),
                Err(error) => {
                    tracing::error!(%error, "a scoped listing could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_those_tracks(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    fn press_at_the_top(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let Some(entry) = TopRun::of(library).at(row) else {
            return;
        };
        match entry {
            TopEntry::Artist(at) => {
                if let Some(artist) = library.artists().get(at).map(|held| held.id) {
                    self.opened(Selection::Artist(artist), cx);
                }
            }
            TopEntry::ArtistFound(at) => {
                if let Some(found) = library.artists_found().get(at).cloned() {
                    self.open_artist_found(found, cx);
                }
            }
            TopEntry::Song(at) => {
                if let Some((played, start)) = library.played_from(at) {
                    self.play_the_listing_from(&played, start, window, cx);
                }
            }
            TopEntry::Found(at) => {
                if let Some(found) = library.found().get(at).cloned() {
                    self.library
                        .update(cx, |library, cx| library.fetch_and_play(found, cx));
                }
            }
            TopEntry::Album(at) => {
                if let Some(album) = library.albums().get(at).map(|held| held.id) {
                    self.opened(Selection::Album(album), cx);
                }
            }
            TopEntry::AlbumFound(at) => {
                if let Some(group) = library
                    .albums_found()
                    .get(at)
                    .map(|found| found.group.clone())
                {
                    self.open_album_not_held(group, cx);
                }
            }
        }
    }

    pub(crate) fn lifted_into(&mut self, lifted: &Lifted, into: LiftedTo, cx: &mut Context<Self>) {
        match &lifted.lift {
            Lift::Tracks(run) => self.landed(&run.tracks(), into, cx),
            Lift::Album(album, _) => self.landed_albums(vec![*album], into, cx),
            Lift::Albums(run) => self.landed_albums(run.albums(), into, cx),
        }
    }

    fn landed_albums(&mut self, albums: Vec<AlbumId>, into: LiftedTo, cx: &mut Context<Self>) {
        let library = self.library.read(cx).catalog();
        let scopes = albums.into_iter().map(Selection::Album).collect();

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read_each(&library, scopes) })
                .await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(tracks) => this.landed(&tracks, into, cx),
                Err(error) => {
                    tracing::error!(%error, "a dragged album could not be read");
                    toast::tell(toast::could_not(words::root::read_that_album(), &error), cx);
                }
            });
        })
        .detach();
    }

    fn landed(&mut self, tracks: &[Track], into: LiftedTo, cx: &mut Context<Self>) {
        if tracks.is_empty() {
            return;
        }
        match into {
            LiftedTo::Queue(at) => self.queue(&listed(tracks), at, cx),
            LiftedTo::Playlist(playlist) => {
                let cuts = tracks.iter().map(Cut::of).collect();
                self.library.update(cx, |library, cx| {
                    library.add_to_playlist(playlist, cuts, cx)
                });
            }
            LiftedTo::PlaylistAt(playlist, at) => {
                let cuts = tracks.iter().map(Cut::of).collect();
                self.library.update(cx, |library, cx| {
                    library.insert_into_playlist(playlist, cuts, at, cx);
                });
            }
        }
    }

    pub(crate) fn reach_further(&mut self, drawn_to: usize, held: usize, cx: &mut Context<Self>) {
        self.library.update(cx, |library, cx| {
            library.reach_further(drawn_to, held, cx);
        });
    }

    pub(crate) fn opened_rows(&self, cx: &App) -> Rows {
        let library = self.library.read(cx);
        let narrowed = library.narrowing().is_some();

        library
            .opened_playlist()
            .map_or(Rows::InHand, |held| Rows::of(held, narrowed))
    }

    fn movable(&self, cx: &App) -> Option<(Shift, usize)> {
        let reached = self.reachable(cx)?;
        match reached.0 {
            Shift::Queue => Some(reached),
            Shift::Playlist(_) => self.opened_rows(cx).are_moved().then_some(reached),
            Shift::Listing(_) => None,
        }
    }

    fn reachable(&self, cx: &App) -> Option<(Shift, usize)> {
        match self.search_in_front(cx) {
            Some(SearchShows::Top) => {
                let rows = TopRun::of(self.library.read(cx)).len();
                return (rows > 0).then_some((Shift::Listing(Listed::Top), rows));
            }
            Some(SearchShows::Songs | SearchShows::Albums | SearchShows::Artists) | None => {}
        }

        match self.pane {
            Pane::Queue => {
                let rows = self.player.read(cx).queue().len();
                (rows > 0).then_some((Shift::Queue, rows))
            }
            Pane::Playlists => {
                let library = self.library.read(cx);
                let Some(opened) = library.opened() else {
                    let rows = library.playlists().len();
                    return (rows > 0).then_some((Shift::Listing(Listed::Playlists), rows));
                };
                let rows = library.entries().len();

                (self.opened_rows(cx).are_reached() && rows > 0)
                    .then_some((Shift::Playlist(opened), rows))
            }
            Pane::Tracks => {
                let rows = self.library.read(cx).listed_rows();

                (rows > 0).then_some((Shift::Listing(Listed::Tracks), rows))
            }
            Pane::Artists => {
                let rows = self.library.read(cx).artists_listed();

                (rows > 0).then_some((Shift::Listing(Listed::Artists), rows))
            }
            Pane::Albums => {
                let rows = self.library.read(cx).albums_listed();

                (rows > 0).then_some((Shift::Listing(Listed::Albums), rows))
            }
            Pane::Favourites => {
                let shelved = Shelved::of(self.library.read(cx));
                let reached = self.reach.as_ref().and_then(|reach| match reach.shift {
                    Shift::Listing(listed) if shelved.rows_of(listed) > 0 => Some(listed),
                    _ => None,
                });
                let listed = reached.or_else(|| shelved.opening())?;

                Some((Shift::Listing(listed), shelved.rows_of(listed)))
            }
            Pane::Missing => {
                let rows = self.missing_rows_shown(cx).len();

                (rows > 0).then_some((Shift::Listing(Listed::Missing), rows))
            }
            Pane::Suggestions => {
                let library = self.library.read(cx);
                let offered = library.suggestions();
                let Some(opened) = library
                    .opened_suggestion()
                    .filter(|opened| offered.iter().any(|held| held.query == opened.query))
                else {
                    let rows = offered.len();
                    return (rows > 0).then_some((Shift::Listing(Listed::Offered), rows));
                };
                let rows = opened.tracks.len();

                (rows > 0).then_some((Shift::Listing(Listed::Suggested), rows))
            }
            Pane::Statistics => {
                let rows = self.heard_rows(cx);

                (rows > 0).then_some((Shift::Listing(Listed::Heard), rows))
            }
            Pane::Party
            | Pane::Server
            | Pane::Lyrics
            | Pane::Inspector
            | Pane::Visualiser
            | Pane::Analysis
            | Pane::Settings => None,
        }
    }

    fn reach_in(&self, shift: Shift, held: usize) -> Option<Reach> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .and_then(|reach| reach.within(held))
    }

    fn opening_row(&self, shift: Shift, rows: usize, cx: &App) -> usize {
        match shift {
            Shift::Queue => self
                .player
                .read(cx)
                .state()
                .queue_position
                .unwrap_or_default()
                .min(rows - 1),
            Shift::Playlist(_) | Shift::Listing(_) => 0,
        }
    }

    fn show_row(&self, shift: Shift, row: usize, cx: &App) {
        match shift {
            Shift::Queue => self
                .queue_rows
                .scroll_to_item(self.queue_parts(cx).line_of(row), ScrollStrategy::Center),
            Shift::Playlist(_) => self
                .playlist_rows
                .scroll_to_item(row, ScrollStrategy::Center),
            Shift::Listing(Listed::Tracks) => {
                self.track_rows.scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Albums) => {
                self.album_rows
                    .scroll_to_item(row / self.grid_columns(), ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Artists) => {
                let at = match self.artists_drawn {
                    ArtistsDrawn::List => row,
                    ArtistsDrawn::Grid => row / self.grid_columns(),
                };
                self.artist_rows.scroll_to_item(at, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Playlists) => {
                let at = match self.playlists_drawn {
                    PlaylistsDrawn::List => row,
                    PlaylistsDrawn::Grid => row / self.grid_columns(),
                };
                self.all_playlist_rows
                    .scroll_to_item(at, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Favourites) => {
                self.favourite_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::FavouriteArtists | Listed::FavouriteAlbums) => {
                self.reached_unseen.set(true);
            }
            Shift::Listing(Listed::Missing) => {
                self.missing_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Suggested) => {
                self.suggestion_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Top | Listed::Offered | Listed::Heard) => {
                self.reached_unseen.set(true);
            }
        }
    }

    fn step_pane(&mut self, step: Step, cx: &mut Context<Self>) {
        let tabs = cx.global::<ResonateApp>().tabs;
        let serving = self.server.read(cx).is_open();
        let stepped = stepped_pane(self.in_front(cx), step, tabs, serving);
        self.choose_pane(stepped, cx);
    }

    pub(crate) fn toggle_queue(&mut self, cx: &mut Context<Self>) {
        if self.pane == Pane::Queue {
            self.set_pane(self.behind_queue, cx);
            return;
        }

        self.behind_queue = self.pane;
        self.set_pane(Pane::Queue, cx);
    }

    pub(crate) fn show_settings(&mut self, category: Category, cx: &mut Context<Self>) {
        let left_a_page = self.settings_page.take().is_some();
        let moved = self.settings_category != category;
        if moved {
            self.settings_category = category;
            self.remember_current_settings_category(cx);
        }
        if moved || left_a_page {
            self.settings_scroll.set_offset(Point::default());
        }
        self.disarm();
        cx.notify();
    }

    fn let_the_control_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm();
        let let_go = self.controls.lets_go(window, cx)
            | self.standing_controls.lets_go(window, cx)
            | self.remote_controls.lets_go(window, cx);
        if let_go {
            cx.notify();
        }
    }

    fn laid_out(&self, cx: &App) -> u64 {
        let library = self.library.read(cx);
        let mut stamp = AHasher::default();
        self.pane.hash(&mut stamp);
        self.settings_category.hash(&mut stamp);
        self.settings_page.hash(&mut stamp);
        self.player.read(cx).queued().revision.hash(&mut stamp);
        Arc::as_ptr(&library.albums()).cast::<()>().hash(&mut stamp);
        Arc::as_ptr(&library.artists())
            .cast::<()>()
            .hash(&mut stamp);
        Arc::as_ptr(&library.tracks()).cast::<()>().hash(&mut stamp);
        library.playlists().len().hash(&mut stamp);
        if self.pane.lands_where_it_was_left() {
            let offset = self.scroll_of(self.pane).0.borrow().base_handle.offset();
            f32::from(offset.y).to_bits().hash(&mut stamp);
        }
        stamp.finish()
    }

    fn moved_under_a_still_pointer(&mut self, window: &Window, cx: &App) -> bool {
        let under = UnderThePointer {
            at: window.mouse_position(),
            laid_out: self.laid_out(cx),
        };
        let moved = under.moved_since(self.under_the_pointer);
        self.under_the_pointer = Some(under);
        moved
    }

    pub(crate) fn focused_control_named(&self, window: &Window) -> Option<SharedString> {
        if !window.last_input_was_keyboard() {
            return None;
        }

        self.controls
            .caption_of_the_focused(window)
            .or_else(|| self.standing_controls.caption_of_the_focused(window))
            .or_else(|| self.remote_controls.caption_of_the_focused(window))
    }

    fn hints_are_wanted(&self, cx: &App) -> bool {
        self.pointer_inside
            && self.grabbed.is_none()
            && self.held_band.is_none()
            && !self.something_stands_over_the_pane()
            && !cx.has_active_drag()
    }

    fn pointer_watch(&self, cx: &mut Context<Self>) -> Canvas<()> {
        let watching = cx.entity();

        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let left = watching.clone();
                window.on_mouse_event(move |_: &MouseExitEvent, _, window, cx| {
                    pointed::let_go(window);
                    left.update(cx, |this, cx| this.pointer_moved_to(Pointer::Gone, cx));
                });

                let came_back = watching.clone();
                window.on_mouse_event(move |_: &MouseMoveEvent, _, _, cx| {
                    came_back.update(cx, |this, cx| {
                        this.pointer_moved_to(Pointer::InTheWindow, cx)
                    });
                });

                let pressed = watching.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    pressed.update(cx, |this, cx| {
                        this.stop_typing(cx);
                    });
                });
            },
        )
        .absolute()
        .size(px(0.0))
    }

    fn composition_watch(&self) -> Canvas<()> {
        let search = self.search.clone();
        let focus = self.focus.clone();
        let welcomed = !self.something_stands_over_the_pane();
        canvas(
            |_, _, _| (),
            move |_, (), window, cx| {
                window.handle_input(
                    &focus,
                    Composing::at_the_window(search.clone(), welcomed),
                    cx,
                );
            },
        )
        .absolute()
        .size(px(0.0))
    }

    fn pointer_moved_to(&mut self, pointer: Pointer, cx: &mut Context<Self>) {
        let inside = pointer == Pointer::InTheWindow;
        if self.pointer_inside == inside {
            return;
        }
        self.pointer_inside = inside;
        if !inside {
            self.lyrics.update(cx, |model, _| model.open_out(false));
            self.seek_pointed = None;
        }
        cx.notify();
    }

    pub(crate) fn opens(
        &self,
        id: impl Into<ElementId>,
        label: impl IntoElement + Spelt,
        saying: &'static str,
        selection: Option<Selection>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let press = selection.map(|selection| {
            move |this: &mut Self, cx: &mut Context<Self>| this.opened(selection, cx)
        });

        self.pressed_to(id, label, saying, press, cx)
    }

    pub(crate) fn pressed_to(
        &self,
        id: impl Into<ElementId>,
        label: impl IntoElement + Spelt,
        saying: &'static str,
        press: Option<impl Fn(&mut Self, &mut Context<Self>) + 'static>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = id.into();

        div()
            .id(id.clone())
            .min_w(px(0.0))
            .truncate()
            .ends_in_an_ellipsis()
            .when(press.is_some(), |named| named.linked_as(&label, saying))
            .child(label)
            .when_some(press, |named, press| {
                named
                    .cursor_pointer()
                    .lit_under_the_pointer(id, |named| {
                        named
                            .text_color(rgb(theme::accent()))
                            .text_decoration_1()
                            .text_decoration_color(rgb(theme::accent()))
                    })
                    .names(saying)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        press(this, cx);
                    }))
            })
    }

    pub(crate) fn open_artist_found(&mut self, found: ArtistFound, cx: &mut Context<Self>) {
        let landed = self
            .library
            .update(cx, |library, cx| library.land_artist_found(found, cx));
        cx.spawn(async move |this, cx| {
            let Some(artist) = landed.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.opened(Selection::Artist(artist), cx));
        })
        .detach();
    }

    pub(crate) fn opened(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let leaving = self.here_now(cx);
        if leaving.selection == selection && leaving.pane == Pane::Tracks {
            return;
        }
        Self::keep_wayback(&mut self.came_from, leaving);
        self.goes_forward.clear();

        if matches!(selection, Selection::Artist(_)) {
            self.artist_shows = ArtistShows::default();
            self.ordering = None;
        }
        self.library
            .update(cx, |library, cx| library.select(selection, cx));
        self.set_pane(Pane::Tracks, cx);
    }

    fn here(&self, cx: &App) -> SharedString {
        let library = self.library.read(cx);
        let named = match (self.pane, library.selection()) {
            (Pane::Tracks, Selection::Album(id)) => {
                library.album_of(id).map(|album| album.title.clone())
            }
            (Pane::Tracks, Selection::Artist(id)) => library
                .artists()
                .iter()
                .find(|artist| artist.id == id)
                .map(|artist| artist.name.clone())
                .or_else(|| library.artist_detail().map(|detail| detail.name.clone())),
            (pane, _) => Some(pane.label().to_owned()),
        };

        named.map_or_else(
            || SharedString::new_static(self.in_front(cx).label()),
            SharedString::from,
        )
    }

    fn here_now(&self, cx: &App) -> Wayback {
        Wayback {
            pane: self.pane,
            selection: self.library.read(cx).selection(),
            at: self.top_row(),
            named: self.here(cx),
            left_by: Leaving::ByALink,
        }
    }

    fn keep_wayback(history: &mut Vec<Wayback>, wayback: Wayback) {
        if history.len() == WAYS_BACK {
            history.remove(0);
        }
        history.push(wayback);
    }

    fn restore_wayback(&mut self, wayback: Wayback, cx: &mut Context<Self>) {
        let Wayback {
            pane,
            selection,
            at,
            named: _,
            left_by: _,
        } = wayback;
        self.library
            .update(cx, |library, cx| library.select(selection, cx));
        self.set_pane(pane, cx);
        self.landing_on = pane.lands_where_it_was_left().then_some(at);
    }

    fn way_back_within(&self, standing: (Pane, Selection)) -> Option<usize> {
        let category = in_front_of(standing.0, standing.1);
        let mut crossed_a_tab = false;
        for (at, back) in self.came_from.iter().enumerate().rev() {
            if (back.pane, back.selection) == standing {
                continue;
            }
            let under_it = in_front_of(back.pane, back.selection) == category;
            if under_it || (!crossed_a_tab && back.left_by == Leaving::ByALink) {
                return Some(at);
            }
            crossed_a_tab |= back.left_by == Leaving::ByATab;
        }
        None
    }

    fn page_shown(&self, cx: &App) -> (Pane, Selection) {
        (self.pane, self.library.read(cx).selection())
    }

    pub(crate) fn way_back_to(&self, cx: &App) -> Option<SharedString> {
        self.way_back_within(self.page_shown(cx))
            .map(|at| self.came_from[at].named.clone())
    }

    pub(crate) fn step_back(&mut self, cx: &mut Context<Self>) {
        match self.way_back_within(self.page_shown(cx)) {
            Some(at) => self.go_back_to(at, cx),
            None => {
                let category = self.in_front(cx);
                self.choose_pane(category, cx);
            }
        }
    }

    fn go_back_to(&mut self, at: usize, cx: &mut Context<Self>) {
        let back = self.came_from.remove(at);
        let current = self.here_now(cx);
        Self::keep_wayback(&mut self.goes_forward, current);
        self.restore_wayback(back, cx);
    }

    fn goes_back_to_a_scope(&self, cx: &App) -> bool {
        self.way_back_within(self.page_shown(cx))
            .is_some_and(|at| self.came_from[at].selection != Selection::Everything)
    }

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        let Some(back) = self.came_from.pop() else {
            return;
        };
        let current = self.here_now(cx);
        Self::keep_wayback(&mut self.goes_forward, current);
        self.restore_wayback(back, cx);
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        let Some(forward) = self.goes_forward.pop() else {
            return;
        };
        let current = self.here_now(cx);
        Self::keep_wayback(&mut self.came_from, current);
        self.restore_wayback(forward, cx);
    }

    pub(crate) fn land_where_it_was_left(&mut self, held: usize) {
        let Some(left_at) = self.landing_on else {
            return;
        };
        if left_at.row >= held {
            if held > 0 {
                self.landing_on = None;
            }
            return;
        }

        let base = self.scroll_of(self.pane).0.borrow().base_handle.clone();
        let offset = base.offset();
        base.set_offset(point(
            offset.x,
            left_at.offset_at(self.row_height_of(self.pane)),
        ));
        self.landing_on = None;
    }

    fn top_row(&self) -> LeftAt {
        let offset = self.scroll_of(self.pane).0.borrow().base_handle.offset().y;
        LeftAt::read_off(offset, self.row_height_of(self.pane))
    }

    fn row_height_of(&self, pane: Pane) -> f32 {
        match pane {
            Pane::Albums => self.grid_row(),
            Pane::Artists if self.artists_drawn == ArtistsDrawn::Grid => self.grid_row(),
            Pane::Artists => theme::tall_row_height(),
            _ => theme::row_height(),
        }
    }

    fn scroll_of(&self, pane: Pane) -> &UniformListScrollHandle {
        match pane {
            Pane::Artists => &self.artist_rows,
            Pane::Albums => &self.album_rows,
            Pane::Queue => &self.queue_rows,
            Pane::Playlists => &self.playlist_rows,
            Pane::Tracks
            | Pane::Statistics
            | Pane::Party
            | Pane::Favourites
            | Pane::Suggestions
            | Pane::Missing
            | Pane::Lyrics
            | Pane::Inspector
            | Pane::Visualiser
            | Pane::Analysis
            | Pane::Settings => &self.track_rows,
            Pane::Server => &self.server_rows,
        }
    }

    pub(crate) fn magnify(&mut self, magnified: Magnified, cx: &mut Context<Self>) {
        self.magnified = Some(magnified);
        cx.notify();
    }

    fn shrink_cover(&mut self, cx: &mut Context<Self>) {
        self.magnified = None;
        cx.notify();
    }

    fn magnifier(
        &self,
        magnified: &Magnified,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let art = self.magnified_art(magnified, cx);
        let title = self.magnified_title(magnified, cx);
        let side = theme::magnified_cover(window.viewport_size());

        div()
            .id("magnified-cover")
            .dialog(words::root::magnified_label())
            .track_focus(&self.sheet_panels.holding(MAGNIFIER_PANEL, cx))
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgba(theme::scrim()))
            .occlude()
            .cursor_pointer()
            .when_some(art, |overlay, art| {
                overlay.child(motion::lifted_in(
                    div()
                        .size(side)
                        .rounded_xl()
                        .overflow_hidden()
                        .bg(rgb(theme::raised()))
                        .casting(kit::Cast::Long)
                        .child(img(art).size(side).object_fit(ObjectFit::Contain)),
                    "magnified-cover-arrives",
                ))
            })
            .when_some(title, |overlay, title| {
                overlay.child(
                    div()
                        .text_size(px(theme::text_lg()))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(theme::text()))
                        .child(title),
                )
            })
            .on_click(cx.listener(|this, _, _, cx| this.shrink_cover(cx)))
    }

    pub(crate) fn toggle_bitrate_graph(&mut self, cx: &mut Context<Self>) {
        self.bitrate_graph = !self.bitrate_graph;
        cx.notify();
    }

    fn typed(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
            return;
        }
        if self.editing(window, cx) {
            return;
        }

        match keystroke.key.as_str() {
            "escape" => self.escaped(window, cx),
            _ if self.something_stands_over_the_pane() => {}
            "backspace" => {
                if !self.drop_typed(cx)
                    && !self.stands_where_a_jump_landed(cx)
                    && !self.drop_reached(cx)
                {
                    self.search.update(cx, |search, cx| search.drop_last(cx));
                }
            }
            _ => {
                let Some(typed) = keystroke
                    .key_char
                    .as_deref()
                    .filter(|key| *key != " " && !key.contains(char::is_control))
                else {
                    return;
                };
                self.type_where_typing_goes(typed, window, cx);
            }
        }
        cx.notify();
    }

    fn jumping(&mut self, cx: &mut Context<Self>) -> Option<(Shift, usize)> {
        let (shift, held) = self.reachable(cx)?;
        match shift {
            Shift::Queue => {}
            Shift::Playlist(_) | Shift::Listing(_) => return None,
        }
        let from = self
            .reach_in(shift, held)
            .map_or_else(|| self.opening_row(shift, held, cx), |reach| reach.row);

        Some((shift, from))
    }

    pub(crate) fn jump_where_typed(&mut self, cx: &mut Context<Self>) {
        if !self.type_ahead.is_live() {
            return;
        }
        let Some((shift, from)) = self.jumping(cx) else {
            return;
        };
        if let Some(names) = self.names_in_the_queue(cx) {
            self.jumped_to(shift, from, &names, cx);
        }
    }

    fn type_where_typing_goes(&mut self, typed: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typed_ahead(typed, cx) {
            self.search.update(cx, |search, cx| {
                search.take_focus(window, cx);
                search.append(typed, cx);
            });
        }
        self.last_typed = Some(Instant::now());
    }

    fn is_typing(&self) -> bool {
        self.type_ahead.is_live()
            || self
                .last_typed
                .is_some_and(|typed| typed.elapsed() < typing::HELD_FOR)
    }

    fn play_pause_or_type_a_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_typing() {
            self.type_where_typing_goes(" ", window, cx);
            cx.notify();
        } else {
            self.send(Command::TogglePlayPause, cx);
        }
    }

    fn typed_ahead(&mut self, letter: &str, cx: &mut Context<Self>) -> bool {
        if self.jumping(cx).is_none() {
            return false;
        }

        self.type_ahead.took(letter, Instant::now());
        self.jump_where_typed(cx);
        self.stops_typing_soon(cx);
        true
    }

    fn drop_typed(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.type_ahead.is_live() {
            return false;
        }

        self.type_ahead.dropped_a_letter(Instant::now());
        self.jump_where_typed(cx);
        self.stops_typing_soon(cx);
        true
    }

    fn jumped_to(&mut self, shift: Shift, from: usize, names: &[Named], cx: &mut Context<Self>) {
        let Some(typed) = self.type_ahead.typed() else {
            return;
        };
        let landing = jumped(names, from, typed);

        self.type_ahead.landed(landing.is_some());
        let Some(row) = landing else {
            return;
        };
        self.reach_at(shift, row, false, cx);
        self.landed_by_a_jump.clone_from(&self.reach);
        self.show_row(shift, row, cx);
    }

    fn stops_typing_soon(&mut self, cx: &mut Context<Self>) {
        self.typing_stops = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(typing::HELD_FOR).await;
            let stopped = this.update(cx, |this, cx| {
                if this.type_ahead.ran_out_by(Instant::now()) {
                    this.stop_typing(cx);
                }
            });
            let _ = stopped;
        });
    }

    fn escaped(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_what_stands_over_the_pane(window, cx) {
            return;
        }
        if self.downloads_open {
            self.close_the_downloads(cx);
        } else if Self::noticed(cx) {
            toast::dismiss(cx);
        } else if !self.stop_typing(cx) {
            self.dismiss_search(window, cx);
        }
    }

    fn close_what_stands_over_the_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.deleting.is_some() {
            self.keep_the_track(cx);
        } else if self.matching.is_some() {
            self.leave_the_matching(window, cx);
        } else if self.party_offered.is_some() {
            self.not_joining_the_party(cx);
        } else if self.listening_open {
            self.close_the_listener(cx);
        } else if self.magnified.is_some() {
            self.shrink_cover(cx);
        } else if self.adding.is_some() {
            self.stop_naming(window, cx);
        } else if self.record.is_some() {
            self.record = None;
            cx.notify();
        } else if self.wrapped.is_some() {
            self.close_wrapped(cx);
        } else if self.sharing.is_some() {
            self.close_sharing(cx);
        } else if self.ordering.is_some() {
            self.close_the_order(cx);
        } else if self.menu.is_some() {
            if !self.back_out_of_the_menu(cx) {
                self.close_the_menu(cx);
            }
        } else {
            return false;
        }
        true
    }

    pub(crate) const fn something_stands_over_the_pane(&self) -> bool {
        self.menu.is_some()
            || self.adding.is_some()
            || self.magnified.is_some()
            || self.record.is_some()
            || self.wrapped.is_some()
            || self.sharing.is_some()
            || self.ordering.is_some()
            || self.listening_open
            || self.deleting.is_some()
            || self.matching.is_some()
            || self.party_offered.is_some()
    }

    fn reach_by_hand(&mut self, cx: &mut Context<Self>) {
        self.stop_typing(cx);
        self.landed_by_a_jump = None;
    }

    fn stands_where_a_jump_landed(&self, cx: &App) -> bool {
        let Some(landed) = self.landed_by_a_jump.as_ref() else {
            return false;
        };

        self.reach.as_ref() == Some(landed)
            && self
                .reachable(cx)
                .is_some_and(|(shift, _)| shift == landed.shift)
    }

    fn stop_typing(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.type_ahead.cleared() {
            return false;
        }
        self.typing_stops = Task::ready(());
        cx.notify();
        true
    }

    fn type_ahead_pill(&self) -> Option<Div> {
        let typed = SharedString::from(self.type_ahead.typed()?.to_owned());
        let says = if self.type_ahead.found() {
            words::root::jumped_to()
        } else {
            words::root::jumped_nowhere()
        };

        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(theme::transport_height() + theme::type_ahead_lift()))
                .flex()
                .justify_center()
                .child(motion::lifted_in(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded_full()
                        .bg(rgb(theme::raised()))
                        .border_1()
                        .border_color(rgb(theme::soft_edge()))
                        .casting(kit::Cast::Slight)
                        .child(kit::eyebrow(says))
                        .child(
                            div()
                                .text_size(px(theme::text_sm()))
                                .text_color(rgb(theme::text()))
                                .whitespace_nowrap()
                                .child(typed),
                        ),
                    "type-ahead-arrives",
                )),
        )
    }

    fn text_fields(&self) -> impl Iterator<Item = &Entity<Field>> {
        [
            &self.search,
            &self.zone_name,
            &self.name,
            &self.typed_words,
            &self.typed_isrc,
            &self.contact,
            &self.acoustid,
            &self.audd,
            &self.listenbrainz,
            &self.discord_app,
            &self.discord_icon,
            &self.sync_server,
            &self.sync_device,
            &self.shared_name,
            &self.sync_code,
            &self.organising,
            &self.minimum_length,
            &self.typed_root,
            &self.finding,
            &self.server_search,
            &self.figure,
            &self.looking,
            &self.spotify_client,
        ]
        .into_iter()
        .chain(&self.subsonic)
        .chain(&self.jellyfin)
        .chain(&self.tidal)
        .chain(&self.lastfm)
        .chain(&self.youtube_music)
    }

    fn editing(&self, window: &Window, cx: &App) -> bool {
        self.text_fields()
            .any(|field| field.read(cx).is_focused(window))
    }

    fn enter_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.take_focus_and_select(window, cx));
        cx.notify();
    }

    fn go_to_the_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        if self.library.read(cx).is_reading_the_search() {
            self.waiting_for_the_results = true;
            self.library
                .update(cx, |library, cx| library.read_the_search_now(cx));
            return;
        }
        self.reach_the_first_result(window, cx);
    }

    fn reach_the_first_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        self.reach = None;
        self.reach_row(Step::Below, cx);
        cx.notify();
    }

    fn tab_onward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.read(cx).is_focused(window) {
            self.go_to_the_results(window, cx);
        } else {
            self.step_the_focus(true, window, cx);
        }
    }

    fn tab_past_the_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) && !self.something_stands_over_the_pane() {
            let search = self.search.focus_handle(cx);
            search.focus(window, cx);
        }
        self.step_the_focus(true, window, cx);
    }

    fn tab_back_to_the_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) && !self.something_stands_over_the_pane() {
            let search = self.search.focus_handle(cx);
            search.focus(window, cx);
            return;
        }
        self.step_the_focus(false, window, cx);
    }

    fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_the_menu(cx) {
            return;
        }
        if self.zone_name.read(cx).is_focused(window) {
            self.stop_naming_the_zone(window, cx);
            return;
        }
        if self.finding.read(cx).is_focused(window) {
            self.finding.update(cx, |finding, cx| finding.clear(cx));
            self.leave_filter(window, cx);
            return;
        }
        if self.server_search.read(cx).is_focused(window) {
            self.server_search.update(cx, |field, cx| field.clear(cx));
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.contact.read(cx).is_focused(window) {
            self.put_back_contact(window, cx);
            return;
        }
        if self.acoustid.read(cx).is_focused(window) {
            self.put_back_acoustid(window, cx);
            return;
        }
        if self.audd.read(cx).is_focused(window) {
            self.put_back_audd(window, cx);
            return;
        }
        if self.listenbrainz.read(cx).is_focused(window) {
            self.put_back_listenbrainz(window, cx);
            return;
        }
        if self.an_account_field_is_focused(window, cx) {
            self.put_back_the_account(window, cx);
            return;
        }
        if self.discord_app.read(cx).is_focused(window) {
            self.leave_discord_app(window, cx);
            return;
        }
        if self.discord_icon.read(cx).is_focused(window) {
            self.leave_discord_icon(window, cx);
            return;
        }
        if [&self.sync_server, &self.sync_device, &self.sync_code]
            .iter()
            .any(|field| field.read(cx).is_focused(window))
        {
            self.leave_sync_field(window, cx);
            return;
        }
        if self.minimum_length.read(cx).is_focused(window) {
            let length = cx
                .global::<ResonateApp>()
                .library
                .music_filters()
                .minimum_length;
            self.minimum_length
                .update(cx, |field, cx| field.hold(length.seconds().to_string(), cx));
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.organising.read(cx).is_focused(window) {
            self.put_back_organising(window, cx);
            return;
        }
        if self.typed_root.read(cx).is_focused(window) {
            self.typed_root
                .update(cx, |typed, cx| typed.hold(String::new(), cx));
            self.leave_typed_root(window, cx);
            return;
        }
        if self.figure.read(cx).is_focused(window) {
            self.leave_figure(window, cx);
            return;
        }
        if self.looking.read(cx).is_focused(window) {
            self.leave_looking(window, cx);
            return;
        }
        if self.matching.is_some() {
            self.leave_the_matching(window, cx);
            return;
        }
        if self.naming.is_some() || self.adding.is_some() {
            self.stop_naming(window, cx);
            return;
        }
        if self.adding_songs_to.is_some() {
            self.finish_adding_songs(cx);
            return;
        }
        if self.pane == Pane::Settings
            && self.finding.read(cx).text().is_empty()
            && self.leave_the_settings_page(cx)
        {
            window.focus(&self.focus, cx);
            return;
        }

        self.reach = None;
        window.focus(&self.focus, cx);
        if self.search.read(cx).text().is_empty() {
            let scoped = self.library.read(cx).selection() != Selection::Everything;
            if scoped || self.goes_back_to_a_scope(cx) {
                self.step_back(cx);
            }
        } else {
            self.search.update(cx, |search, cx| search.clear(cx));
        }
        cx.notify();
    }

    pub(crate) fn show_everything(&mut self, cx: &mut Context<Self>) {
        pointed::forget();
        self.came_from.clear();
        self.goes_forward.clear();
        if self.library.read(cx).selection() == Selection::Everything {
            return;
        }

        self.read_from_the_top(cx);
        self.library
            .update(cx, |library, cx| library.select(Selection::Everything, cx));
    }

    pub(crate) fn search_instead(
        &mut self,
        instead: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_everything(cx);
        self.search.update(cx, |search, cx| {
            search.take_focus(window, cx);
            search.set_text(instead, cx);
        });
    }

    pub(crate) fn let_go_of_the_reach_in(&mut self, shift: Shift) {
        if self
            .reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift)
        {
            self.reach = None;
        }
    }

    fn open_the_search(&mut self, cx: &mut Context<Self>) {
        let browsing = matches!(self.pane, Pane::Albums | Pane::Artists | Pane::Tracks);
        if !browsing {
            if self.library.read(cx).selection() != Selection::Everything {
                self.show_everything(cx);
            }
            self.set_pane(Pane::Tracks, cx);
        }
        self.search_shows = SearchShows::Top;
        self.ordering = None;
    }

    fn leave_the_scroll_for_the_search(&mut self) {
        self.left_for_the_search = self
            .pane
            .lands_where_it_was_left()
            .then(|| (self.pane, self.top_row()));
    }

    fn land_where_the_search_began(&mut self) {
        let began = self.left_for_the_search.take();
        self.landing_on = began
            .filter(|(pane, _)| *pane == self.pane)
            .map(|(_, at)| at)
            .or(self.landing_on);
    }

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        let was = self
            .library
            .read(cx)
            .narrowing()
            .unwrap_or_default()
            .to_owned();
        if was == query {
            return;
        }
        if was.is_empty() {
            self.leave_the_scroll_for_the_search();
            self.open_the_search(cx);
        }
        if query.is_empty() {
            self.land_where_the_search_began();
        }
        self.search_scroll.set_offset(point(px(0.0), px(0.0)));
        self.read_from_the_top(cx);
        if self
            .reach
            .as_ref()
            .is_some_and(|reach| matches!(reach.shift, Shift::Listing(_)))
        {
            self.reach = None;
        }
        self.library
            .update(cx, |library, cx| library.set_query(query, cx));
        cx.notify();
    }

    pub(crate) fn show_playlist(&mut self, opened: Option<PlaylistId>, cx: &mut Context<Self>) {
        if self.library.read(cx).opened() != opened {
            self.read_the_kept_order(opened, cx);
        }
        self.playlist_rows = match opened {
            Some(id) => self.left_at.entry(id).or_default().clone(),
            None => UniformListScrollHandle::default(),
        };
        self.library
            .update(cx, |library, cx| library.open_playlist(opened, cx));
    }

    fn forget_what_has_gone(&mut self, library: &Entity<LibraryModel>, cx: &App) {
        let listing = library.read(cx);
        if listing.narrowing().is_some() {
            return;
        }

        let held: AHashSet<PlaylistId> = listing
            .playlists()
            .iter()
            .map(|playlist| playlist.id)
            .collect();
        self.left_at.retain(|id, _| held.contains(id));
    }

    fn read_from_the_top(&mut self, cx: &App) {
        self.playlist_rows = UniformListScrollHandle::default();
        if let Some(opened) = self.library.read(cx).opened() {
            self.left_at.insert(opened, self.playlist_rows.clone());
        }
    }

    pub(crate) fn volume_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let current = self
            .muted_at(cx)
            .map_or_else(|| self.volume_now(cx), |muted_from| muted_from.get());
        self.set_volume(current + delta, cx);
    }

    fn volume_now(&self, cx: &App) -> f32 {
        self.volume_aimed
            .unwrap_or_else(|| self.player.read(cx).state().volume.get())
    }

    pub(crate) fn muted_at(&self, cx: &App) -> Option<Volume> {
        self.muted_from
            .filter(|_| self.player.read(cx).state().volume == Volume::MUTE)
    }

    pub(crate) fn is_muted(&self, cx: &App) -> bool {
        self.muted_at(cx).is_some() || self.device_is_muted(cx)
    }

    fn device_is_muted(&self, cx: &App) -> bool {
        self.player
            .read(cx)
            .state()
            .output
            .is_some_and(|output| output.device_muted)
    }

    fn device_takes_the_volume(&self, cx: &App) -> bool {
        self.player
            .read(cx)
            .state()
            .output
            .is_some_and(|output| output.device_turned)
    }

    pub(crate) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(muted_from) = self.muted_at(cx) {
            self.set_volume(muted_from.get(), cx);
            return;
        }
        if self.device_takes_the_volume(cx) {
            let muted = self.device_is_muted(cx);
            self.send(Command::SetDeviceMute(!muted), cx);
            cx.notify();
            return;
        }
        let Ok(heard_at) = Volume::new(self.volume_now(cx).clamp(0.0, 1.0)) else {
            return;
        };
        if heard_at == Volume::MUTE {
            return;
        }
        self.volume_aimed = None;
        self.muted_from = Some(heard_at);
        self.send(Command::SetVolume(Volume::MUTE), cx);
        cx.notify();
    }

    pub(crate) fn set_volume(&mut self, position: f32, cx: &mut Context<Self>) {
        let Ok(volume) = Volume::new(position.clamp(0.0, 1.0)) else {
            return;
        };
        self.muted_from = None;
        self.send(Command::SetVolume(volume), cx);
        self.volume_aimed = Some(volume.get());
        self.volume_unkept = Some((self.switcher.selected(), volume));

        self.volume_settled = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VOLUME_SETTLE).await;
            let stored = this.update(cx, |this, cx| this.keep_the_volume(cx));
            let _ = stored;
        });
    }

    pub(crate) fn keep_the_volume(&mut self, cx: &mut Context<Self>) {
        self.volume_aimed = None;
        if let Some(volume) = self.volume_to_keep() {
            self.store_for(ZoneId::FIRST, &Setting::Volume(volume), cx);
        }
    }

    fn volume_to_keep(&mut self) -> Option<Volume> {
        let (zone, volume) = self.volume_unkept.take()?;
        zone.is_first().then_some(volume)
    }

    pub(crate) fn keep_the_volume_as_the_window_leaves(&mut self) {
        if let Some(volume) = self.volume_to_keep() {
            self.settings_written
                .change(SettingChange::Store(Setting::Volume(volume)));
        }
    }

    fn header(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let moves_the_window = chrome::the_header_moves_the_window(window);
        let controls_are_ours = chrome::the_window_controls_are_ours(window);
        let corners = chrome::rounded_within_the_frame(window);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_4()
            .h(px(theme::header_height()))
            .px_4()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .rounded_tl(corners.top_left)
            .rounded_tr(corners.top_right)
            .bg(rgb(theme::surface()))
            .when(moves_the_window, chrome::titlebar)
            .child(self.wordmark(window))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .min_w(px(0.0))
                    .child(self.search_field(window, cx))
                    .child(
                        self.in_the_pane_ring(
                            kit::icon_button(
                                "listen",
                                Icon::Listen,
                                words::root::listen_hint(key!(listen)),
                            )
                            .flex_none()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                            |this, _, cx| this.open_the_listener(cx),
                            cx,
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .w(px(theme::sidebar_width() - 32.0))
                    .justify_end()
                    .child(self.zone_switcher(cx))
                    .when(controls_are_ours, |bar| {
                        bar.child(self.window_controls(window, cx))
                    }),
            )
    }

    fn wordmark(&self, window: &Window) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .w(px(theme::sidebar_width() - 32.0))
            .when_some(chrome::room_for_the_traffic_lights(window), |mark, room| {
                mark.pl(room).overflow_hidden()
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(theme::wordmark_mark()))
                    .rounded_md()
                    .bg(rgb(theme::accent()))
                    .child(icons::icon(
                        Icon::Resonate,
                        theme::root_icon(),
                        theme::accent_ink(),
                    )),
            )
            .child(
                div()
                    .pt(px(WORDMARK_CAPITALS_CENTRED_BY))
                    .text_size(px(theme::text_base()))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child("Resonate"),
            )
    }

    fn search_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search.read(cx);
        let searching = search.is_focused(window);
        let empty = search.text().is_empty();

        let box_of_words = div()
            .id("search")
            .flex()
            .flex_1()
            .max_w(px(theme::search_width()))
            .items_center()
            .gap_2()
            .h(px(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(if searching {
                theme::background()
            } else {
                theme::raised()
            }))
            .border_1()
            .border_color(if searching {
                kit::field_lit()
            } else {
                rgb(theme::soft_edge())
            })
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.bg(rgb(theme::hover())))
            .child(icons::icon(
                Icon::Search,
                theme::search_icon(),
                if searching {
                    theme::accent()
                } else {
                    theme::faint()
                },
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.leave_search(window, cx)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.search
                    .update(cx, |search, cx| search.take_focus(window, cx));
                cx.notify();
            }))
            .child(self.search.clone())
            .when(!empty, |field| field.child(self.clear_search(cx)))
            .when(empty && !searching, |field| {
                field.child(kit::figure(key!(find)).text_color(rgb(theme::faint())))
            })
            .child(hint::explains("search-terms", words::root::search_hint()));

        menu::opens_a_menu(
            box_of_words,
            |this, at, cx| {
                let holding = this.search.read(cx).holds_a_selection();

                Menu::at(at)
                    .when_some(holding.then_some(()), |menu, ()| {
                        menu.under(
                            Icon::Rename,
                            words::root::cut(),
                            key!(cut),
                            |this, window, cx| {
                                this.search.update(cx, |field, cx| field.cuts(window, cx));
                            },
                        )
                        .under(
                            Icon::Export,
                            words::root::copy(),
                            key!(copy),
                            |this, window, cx| {
                                this.search.update(cx, |field, cx| field.copies(window, cx));
                            },
                        )
                    })
                    .under(
                        Icon::Import,
                        words::root::paste(),
                        key!(paste),
                        |this, window, cx| {
                            this.search.update(cx, |field, cx| field.pastes(window, cx));
                        },
                    )
                    .under(
                        Icon::Check,
                        words::root::select_all(),
                        key!(select_everything),
                        |this, window, cx| {
                            this.search
                                .update(cx, |field, cx| field.selects_everything(window, cx));
                        },
                    )
            },
            cx,
        )
        .into_any_element()
    }

    fn clear_search(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("clear-search")
            .group("clear")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(theme::clear_control()))
            .rounded_full()
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme::hover())))
            .names(words::root::clear_search_hint(key!(leave)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.search.update(cx, |search, cx| {
                    search.take_focus(window, cx);
                    search.clear(cx);
                });
            }))
            .child(icons::lit_on_hover(
                icons::icon(Icon::Close, theme::clear_mark(), theme::muted()),
                "clear",
            ))
    }

    fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let library = self.library.read(cx);
        let albums = library.albums_counted() as usize;
        let artists = library.artists_counted() as usize;
        let tracks = library.tracks_counted() as usize;
        let saved = library.playlists().len();
        let missing = library.missing().tracks as usize;
        let favourites = library.favourited().held();
        let offered = library.suggestions().len();
        let plays = library.statistics().plays as usize;
        let tabs = cx.global::<ResonateApp>().tabs;
        let serving = self.server.read(cx).is_open();
        let enriching = library.is_enriching();
        let downloads = !library.downloads().is_empty() || library.keeps_left().is_some();
        let mut browse = div().flex().flex_col().gap_4();
        for section in Section::ALL {
            let mut listed = div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(kit::eyebrow(section.label()).px_3().pb_1p5());
            for pane in Pane::BROWSE
                .into_iter()
                .filter(|pane| pane.section() == section && pane.is_there(tabs, serving))
            {
                let count = match pane {
                    Pane::Albums => Some(albums),
                    Pane::Artists => Some(artists),
                    Pane::Tracks => Some(tracks),
                    Pane::Statistics => (plays > 0).then_some(plays),
                    Pane::Party => None,
                    Pane::Playlists => Some(saved),
                    Pane::Favourites => (favourites > 0).then_some(favourites),
                    Pane::Suggestions => (offered > 0).then_some(offered),
                    Pane::Missing => (missing > 0).then_some(missing),
                    Pane::Queue
                    | Pane::Server
                    | Pane::Lyrics
                    | Pane::Inspector
                    | Pane::Visualiser
                    | Pane::Analysis
                    | Pane::Settings => None,
                };
                listed = listed.child(self.pane_row(pane, count.filter(|_| tabs.counts), cx));
                if pane == Pane::Playlists {
                    listed = listed.children(self.pinned_rows(cx));
                }
            }
            browse = browse.child(listed);
        }

        let content = div()
            .id("sidebar-content")
            .flex()
            .flex_col()
            .flex_none()
            .p_3()
            .w(px(theme::sidebar_width() - 1.0))
            .child(browse);

        let foot = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .p_3()
            .pt_0()
            .w(px(theme::sidebar_width() - 1.0))
            .when(downloads, |column| column.child(self.download_status(cx)))
            .when(enriching, |column| column.child(self.enrichment_status(cx)))
            .when_some(self.remote_playback(window, cx), |column, panel| {
                column.child(panel)
            })
            .child(self.pane_row(Pane::Settings, None, cx));

        div()
            .id("sidebar")
            .relative()
            .named_as(Role::Navigation, words::root::sidebar(), "")
            .flex()
            .flex_none()
            .flex_col()
            .w(px(theme::sidebar_width()))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::surface()))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("sidebar-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll)
                            .child(content),
                    )
                    .child(
                        Scrollbars::of(cx)
                            .vertical("sidebar-scrollbar", self.sidebar_scroll.clone()),
                    ),
            )
            .child(foot)
            .child(self.float_remote_when_categories_scroll(window, cx))
    }

    fn pinned_rows(&self, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let library = self.library.read(cx);
        let pinned = library.pinned();
        let opened = (self.pane == Pane::Playlists)
            .then(|| library.opened())
            .flatten();
        let playing = self.playing_playlist(cx);
        let lists: AHashSet<PlaylistId> = library.lists().iter().map(|list| list.id).collect();

        pinned
            .iter()
            .map(|playlist| {
                let id = playlist.id;
                let colour = if playing == Some(id) {
                    theme::accent()
                } else if opened == Some(id) {
                    theme::text()
                } else {
                    theme::muted()
                };

                div()
                    .id(("pinned-playlist", id.get() as usize))
                    .debug_selector(move || format!("pinned-playlist-{}", id.get()))
                    .flex()
                    .items_center()
                    .h(px(theme::pinned_row()))
                    .pl(px(PINNED_INSET + theme::pane_icon()))
                    .pr_3()
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(colour))
                    .when(opened == Some(id), |row| {
                        row.font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .hover(|row| row.bg(rgb(theme::hover())))
                    .named_as(Role::Link, &playlist.name, words::root::pinned_hint())
                    .names(words::root::pinned_hint())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(playlist.name.clone())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_pane(Pane::Playlists, cx);
                        this.show_playlist(Some(id), cx);
                    }))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .zip(pinned.iter())
            .map(|(row, playlist)| match lists.contains(&playlist.id) {
                true => reorder::takes_a_lift(row, LiftedTo::Playlist(playlist.id), cx),
                false => row,
            })
            .collect()
    }

    fn download_status(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let library = self.library.read(cx);
        let fetching: Vec<Fetching> = library
            .downloads()
            .iter()
            .map(|download| library.fetching(download))
            .collect();
        let keeping = library.keeps_left();
        let underway = keeping.is_some() || fetching.iter().copied().any(Fetching::is_underway);
        let arriving = library.downloads().iter().any(|download| {
            library
                .fetched(download)
                .is_some_and(|fetched| fetched.is_arriving())
        });
        let said = keeping.map_or_else(
            || downloads::summed_up(&fetching, arriving),
            downloads::keeping_said,
        );
        let open = self.downloads_open;
        let hint = if open {
            words::root::hide_downloads_hint()
        } else {
            words::root::show_downloads_hint()
        };
        let mark = if underway {
            theme::accent()
        } else {
            theme::faint()
        };

        let row = div()
            .id("downloads")
            .debug_selector(|| "downloads".to_owned())
            .named_as(Role::Button, &said, hint)
            .thrown(open)
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::sidebar_row()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(hint)
            .child(icons::icon(Icon::Download, theme::pane_icon(), mark))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(said),
            )
            .child(icons::icon(
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronUp
                },
                theme::row_marker_icon(),
                theme::faint(),
            ));

        self.in_the_standing_ring(
            "downloads",
            row,
            |this, _, cx| {
                let closed_by_this_press = this
                    .downloads_pressed_off
                    .take()
                    .is_some_and(|pressed| pressed.elapsed() < DOWNLOADS_TOGGLE_PRESS);
                if closed_by_this_press {
                    return;
                }
                this.downloads_open = !this.downloads_open;
                cx.notify();
            },
            cx,
        )
    }

    fn downloads_over_the_app(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnimationElement<Stateful<Div>>> {
        let library = self.library.read(cx);
        let keeping = library.keeps_left();
        if !self.downloads_open || (library.downloads().is_empty() && keeping.is_none()) {
            return None;
        }
        let rows: Vec<(Download, Fetching)> = library
            .downloads()
            .iter()
            .map(|download| (download.clone(), library.fetching(download)))
            .collect();
        let finished = rows.iter().any(|(_, fetching)| !fetching.is_underway());

        let mut list = div()
            .id("download-list")
            .flex()
            .flex_col()
            .gap_0p5()
            .max_h(px(theme::downloads_height()))
            .overflow_y_scroll()
            .track_scroll(&self.downloads_scroll)
            .when_some(keeping, |list, left| list.child(self.keep_row(left, cx)));
        for (at, (download, fetching)) in rows.into_iter().enumerate() {
            list = list.child(self.download_row(at, &download, fetching, cx));
        }

        let resting = px(theme::transport_height() + DOWNLOADS_PANEL_GAP);

        Some(motion::risen_in(
            div()
                .id("downloads-panel")
                .named_as(Role::Dialog, words::root::downloads_panel_label(), "")
                .absolute()
                .left(px(theme::sidebar_width() + DOWNLOADS_PANEL_GAP))
                .bottom(resting)
                .w(px(theme::downloads_width()))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_lg()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(rgb(theme::soft_edge()))
                .casting(kit::Cast::Short)
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.downloads_pressed_off = Some(Instant::now());
                    this.close_the_downloads(cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .pl_2()
                        .child(kit::eyebrow(words::root::downloads()))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .when(finished, |actions| {
                                    actions.child(self.in_the_pane_ring(
                                        kit::button(
                                            "clear-downloads",
                                            None,
                                            words::root::clear_finished(),
                                            words::root::clear_downloads_hint(),
                                            kit::Tone::Ghost,
                                        ),
                                        |this, _, cx| {
                                            this.library.update(cx, |library, cx| {
                                                library.clear_finished_downloads(cx);
                                            });
                                        },
                                        cx,
                                    ))
                                })
                                .child(self.in_the_pane_ring(
                                    kit::icon_button(
                                        "close-downloads",
                                        Icon::Close,
                                        words::root::hide_downloads_hint(),
                                    ),
                                    |this, _, cx| {
                                        this.close_the_downloads(cx);
                                    },
                                    cx,
                                )),
                        ),
                )
                .child(
                    div().relative().child(list).child(
                        Scrollbars::of(cx)
                            .vertical("downloads-scrollbar", self.downloads_scroll.clone()),
                    ),
                ),
            "downloads-arrives",
            resting,
        ))
    }

    fn close_the_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads_open = false;
        cx.notify();
    }

    fn keep_row(&self, left: u64, cx: &mut Context<Self>) -> Div {
        let said = downloads::keeping_said(left);
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(words::root::keeping_songs()),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::accent()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(said),
                    ),
            )
            .child(self.in_the_pane_ring(
                kit::icon_button("stop-keeping", Icon::Stop, words::root::stop_keeping_hint()),
                |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.stop_keeping(cx));
                },
                cx,
            ))
    }

    fn download_row(
        &self,
        at: usize,
        download: &Download,
        fetching: Fetching,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let recording = download.found.recording.clone();
        let again = download.found.clone();
        let state = browser::fetching_colour(fetching);
        let library = self.library.read(cx);
        let saying = downloads::saying_while(fetching, library.fetched(download).as_ref());
        let album = library.downloaded_album(download);
        let track = library
            .downloaded_track(download)
            .filter(|_| fetching == Fetching::Downloaded);

        let spoken_row = || {
            spoken::joined([
                download.found.title.as_str(),
                download.found.artist.as_str(),
                saying.as_ref(),
            ])
        };

        let text = div()
            .id(listing::keyed_by("open-download", &recording))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::text()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(download.found.title.clone())),
            )
            .child(
                div()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(theme::faint()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(download.found.artist.clone())),
            )
            .child(
                div()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(state))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(saying.clone()),
            );
        let text = match album {
            Some(album) => self.in_the_pane_ring(
                text.cursor_pointer()
                    .names(words::root::open_download_hint())
                    .named_as(
                        Role::Link,
                        &download.found.title,
                        words::root::open_download_hint(),
                    ),
                move |this, _, cx| {
                    this.close_the_downloads(cx);
                    this.opened(Selection::Album(album), cx);
                },
                cx,
            ),
            None => text,
        };

        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .when_some(track, |actions, track| {
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("play-download", &recording),
                        Icon::Play,
                        words::root::play_download_hint(),
                    ),
                    move |this, _, cx| {
                        this.play_what_was_downloaded(track, cx);
                    },
                    cx,
                ))
            })
            .when(fetching.can_be_asked_again(), |actions| {
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("ask-again", &recording),
                        Icon::Redo,
                        words::root::ask_again_hint(),
                    ),
                    move |this, _, cx| {
                        let wanted = again.clone();
                        this.library
                            .update(cx, |library, cx| library.want_found(wanted, cx));
                    },
                    cx,
                ))
            })
            .when(fetching.can_be_cancelled(), |actions| {
                let cancelled = recording.clone();
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("cancel-download", &recording),
                        Icon::Stop,
                        words::root::cancel_download_hint(),
                    ),
                    move |this, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.cancel_download(&cancelled, cx);
                        });
                    },
                    cx,
                ))
            })
            .when(!fetching.is_underway(), |actions| {
                let dismissed = recording.clone();
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("dismiss-download", &recording),
                        Icon::Close,
                        words::root::dismiss_download_hint(),
                    ),
                    move |this, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.dismiss_download(&dismissed, cx);
                        });
                    },
                    cx,
                ))
            });

        div()
            .id(listing::keyed_by("download", &recording))
            .debug_selector(move || format!("download-{at}"))
            .listed_as(spoken_row)
            .flex()
            .items_center()
            .gap_2()
            .pl_3()
            .pr_1()
            .py_1p5()
            .rounded_md()
            .child(text)
            .child(actions)
    }

    fn play_what_was_downloaded(&mut self, track: TrackId, cx: &mut Context<Self>) {
        let catalog = self.library.read(cx).catalog();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { catalog.track(track) })
                .await;
            let played = this.update(cx, |this, cx| match read {
                Ok(Some(track)) => this.play(slice::from_ref(&track), 0, cx),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "a downloaded song could not be read to play");
                    toast::tell(toast::could_not(words::root::play_that_song(), &error), cx);
                }
            });
            let _ = played;
        })
        .detach();
    }

    pub(crate) fn ask_to_delete(&mut self, deleting: Deleting, cx: &mut Context<Self>) {
        self.close_the_menu(cx);
        self.deleting = Some(deleting);
        cx.notify();
    }

    fn keep_the_track(&mut self, cx: &mut Context<Self>) {
        self.deleting = None;
        cx.notify();
    }

    fn delete_the_track(&mut self, cx: &mut Context<Self>) {
        let Some(Deleting { tracks, title, .. }) = self.deleting.take() else {
            return;
        };
        self.library
            .update(cx, |library, cx| library.delete_tracks(tracks, title, cx));
        cx.notify();
    }

    fn deletion_sheet(&self, deleting: &Deleting, cx: &mut Context<Self>) -> Div {
        let card = kit::sheet()
            .id("delete-sheet")
            .dialog(&deleting.asking())
            .track_focus(&self.sheet_panels.holding(sheets::DELETE_PANEL, cx))
            .w(px(theme::confirm_width()))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.keep_the_track(cx)))
            .child(kit::sheet_heading(
                "delete-sheet-asking",
                Icon::Delete,
                theme::failure(),
                deleting.asking(),
            ))
            .child(
                div()
                    .id("delete-sheet-saying")
                    .read_plainly_as(|| SharedString::from(deleting.saying()))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .child(SharedString::from(deleting.saying()))
                    .when(deleting.a_cut, |body| {
                        body.child(words::root::a_cut_goes_whole())
                    })
                    .child(
                        div()
                            .text_color(rgb(theme::text()))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(words::root::moved_to_the_trash()),
                    ),
            )
            .child(
                kit::sheet_actions()
                    .child(self.in_the_pane_ring(
                        kit::button(
                            "keep-the-track",
                            None,
                            words::root::cancel(),
                            words::root::keep_the_track_hint(key!(leave)),
                            kit::Tone::Outlined,
                        ),
                        |this, _, cx| this.keep_the_track(cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::button(
                            "delete-the-track",
                            Some(Icon::Delete),
                            words::root::delete(),
                            words::root::delete_the_track_hint(),
                            kit::Tone::Destructive,
                        ),
                        |this, _, cx| this.delete_the_track(cx),
                        cx,
                    )),
            );

        kit::over_a_scrim(motion::lifted_in(card, "delete-sheet-arrives"))
    }

    fn enrichment_status(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let row = div()
            .id("enriching")
            .named_as(
                Role::Button,
                words::root::enriching_label(),
                words::root::enriching_hint(),
            )
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::sidebar_row()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(words::root::enriching_hint())
            .child(icons::icon(Icon::Globe, theme::pane_icon(), theme::faint()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(words::root::enriching_label()),
            );

        self.in_the_standing_ring(
            "enriching",
            row,
            |this, _, cx| {
                this.set_pane(Pane::Settings, cx);
                this.open_the_settings_page(Page::Enrichment, cx);
            },
            cx,
        )
    }

    pub(crate) fn in_the_standing_ring(
        &self,
        named: &'static str,
        mut control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let handle = self.standing_controls.at(named, cx);
        self.standing_controls
            .captioned_by_its_name(named, &mut control);
        Self::ringed(&handle, control, press, cx)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
    }

    fn pane_row(&self, pane: Pane, count: Option<usize>, cx: &mut Context<Self>) -> Stateful<Div> {
        let chosen = pane == self.in_front(cx);
        let colour = if chosen {
            theme::text()
        } else {
            theme::muted()
        };
        let mark = if chosen {
            theme::accent()
        } else {
            theme::muted()
        };

        let row = div()
            .id(SharedString::new_static(pane.label()))
            .debug_selector(|| format!("tab-{}", pane.as_str()))
            .group(PANE_GROUP)
            .relative()
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::sidebar_row()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(theme::text_sm()))
            .when(chosen, |row| row.font_weight(gpui::FontWeight::MEDIUM))
            .hover(|row| row.bg(rgb(theme::hover())))
            .text_color(rgb(colour))
            .child(chosen_ground(chosen))
            .child(icons::lit_on_hover(
                icons::icon(pane.icon(), theme::pane_icon(), mark),
                PANE_GROUP,
            ))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(pane.label()),
            )
            .named_as(Role::Tab, pane.label(), pane.about())
            .chosen(chosen)
            .names(pane.about())
            .when_some(count, |row, count| {
                row.child(kit::figure(count.to_string()).text_color(rgb(theme::faint())))
            });
        self.in_the_standing_ring(
            pane.label(),
            row,
            move |this, _, cx| this.choose_pane(pane, cx),
            cx,
        )
    }

    pub(crate) fn drawn_cover(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<(Picture, Magnified)> {
        let (album, file) = pictured.parts();

        if let Some(album) = album
            && let Some(art) = self
                .library
                .update(cx, |library, cx| library.cover(album, drawn, cx))
        {
            return Some((art, Magnified::Album(album)));
        }

        let file = file?;
        if self.server.read(cx).names(file) {
            let art = self
                .server
                .update(cx, |server, cx| server.cover(file, None, drawn, cx))?;
            return Some((art, Magnified::Served(file.clone())));
        }
        let art = self
            .player
            .update(cx, |player, cx| player.art(file, drawn, cx))?;
        Some((art, Magnified::File(file.clone())))
    }

    pub(crate) fn is_pictureless(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> bool {
        let (album, file) = pictured.parts();
        let album_lacks =
            album.is_none_or(|album| self.library.read(cx).cover_is_absent(album, drawn));
        let file_lacks = file.is_none_or(|file| {
            let server = self.server.read(cx);
            if server.names(file) {
                server.cover_is_absent(file, None, drawn)
            } else {
                self.player.read(cx).art_is_absent(file, drawn)
            }
        });
        album_lacks && file_lacks
    }

    pub(crate) fn covers_ahead(
        &self,
        albums: impl Iterator<Item = AlbumId>,
        cx: &mut Context<Self>,
    ) {
        self.library.update(cx, |library, cx| {
            for album in albums {
                if !library.cover_ahead(album, Drawn::InARow, cx) {
                    break;
                }
            }
        });
    }

    fn magnified_art(&self, magnified: &Magnified, cx: &mut Context<Self>) -> Option<Picture> {
        match magnified {
            Magnified::Album(album) => self
                .library
                .update(cx, |library, cx| library.whole_cover(*album, cx)),
            Magnified::File(file) => self
                .player
                .update(cx, |player, cx| player.whole_art(file, cx)),
            Magnified::Served(file) => self.server.update(cx, |server, cx| {
                server.cover(file, None, Drawn::NowPlaying, cx)
            }),
        }
    }

    fn magnified_title(
        &self,
        magnified: &Magnified,
        cx: &mut Context<Self>,
    ) -> Option<SharedString> {
        match magnified {
            Magnified::Album(album) => self
                .library
                .update(cx, |library, _| library.album_title(*album))
                .map(SharedString::from),
            Magnified::File(file) | Magnified::Served(file) => Some(SharedString::from(
                self.player
                    .read(cx)
                    .media(file, None)
                    .and_then(|info| info.tags.title.clone())
                    .unwrap_or_else(|| format::stem(file)),
            )),
        }
    }

    pub(crate) fn hero_side(&self) -> f32 {
        let measured = self.hero_height.get();
        if measured > px(0.0) {
            measured / px(1.0)
        } else {
            theme::scope_cover()
        }
    }

    pub(crate) fn cover(&self, pictured: Pictured<'_>, cx: &mut Context<Self>) -> Div {
        self.cover_sized(pictured, Drawn::InARow, theme::row_cover(), cx)
    }

    pub(crate) fn cover_sized(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let art = self.drawn_cover(pictured, drawn, cx).map(|(art, _)| art);
        framed_cover(art, side)
    }

    pub(crate) fn region(
        &mut self,
        region: Region,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match region {
            Region::Header => self.header(window, cx).into_any_element(),
            Region::Sidebar => self.sidebar(window, cx).into_any_element(),
            Region::Pane => {
                self.grid_width.seen_in(window.viewport_size().width);
                self.finding_holds_the_caret = self.finding.read(cx).is_focused(window);
                let shown = self.pane_shown(cx);
                let named = match self.search_in_front(cx) {
                    Some(_) => words::root::search_region(),
                    None => self.pane.label(),
                };
                motion::faded_in(
                    div()
                        .id(PANE_REGION)
                        .regioned(named)
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(self.content(cx)),
                    shown,
                    motion::ARRIVES_OVER,
                )
                .into_any_element()
            }
            Region::Transport => self.transport(chrome::rounded_within_the_frame(window), cx),
        }
    }

    fn pane_shown(&self, cx: &mut Context<Self>) -> SharedString {
        let searching = self.search_in_front(cx).is_some();
        let selection = self.library.read(cx).selection();

        SharedString::from(match (searching, self.pane) {
            (true, _) => "pane-search".to_owned(),
            (false, Pane::Tracks) => format!("pane-tracks-{selection:?}"),
            (false, pane) => format!("pane-{pane:?}"),
        })
    }

    fn content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.controls.opening();
        if let Some(shows) = self.search_in_front(cx) {
            return self.search_pane(shows, cx);
        }

        match self.pane {
            Pane::Albums => self.albums(cx),
            Pane::Artists => self.artists(cx),
            Pane::Tracks => self.tracks(cx),
            Pane::Statistics => self.statistics_pane(cx),
            Pane::Party => self.party_pane(cx).into_any_element(),
            Pane::Queue => self.queue_pane(cx),
            Pane::Playlists => self.playlists_pane(cx),
            Pane::Favourites => self.favourites_pane(cx),
            Pane::Suggestions => self.suggestions_pane(cx),
            Pane::Missing => self.missing_pane(cx),
            Pane::Server => self.server_pane(cx),
            Pane::Lyrics => self.lyrics_pane(cx),
            Pane::Inspector => self.inspector(cx),
            Pane::Visualiser => self.visualiser_pane(cx),
            Pane::Analysis => self.analysis_pane(cx),
            Pane::Settings => self.settings(cx),
        }
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
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.enter_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &LeaveSearch, window, cx| {
                this.dismiss_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteAway, window, cx| {
                this.search
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
            .on_action(|_: &CloseTheWindow, window: &mut Window, _: &mut App| {
                window.remove_window();
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
                toast::drawn(self.type_ahead.typed().is_some(), window, cx),
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
            .when(self.sharing.is_some(), |app| {
                app.child(motion::faded_in(
                    self.sharing_sheet(cx),
                    "sharing-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when(self.wrapped.is_some(), |app| {
                app.child(motion::faded_in(
                    self.wrapped_sheet(cx),
                    "wrapped-scrim",
                    motion::ARRIVES_OVER,
                ))
            })
            .when_some(self.ordering, |app, ordered| {
                app.child(self.order_sheet(ordered, window.viewport_size(), cx))
            })
            .when(self.party_offered.is_some(), |app| {
                app.child(motion::faded_in(
                    self.party_offer_sheet(cx),
                    "party-offer-scrim",
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

#[cfg(test)]
mod tests {
    use gpui::px;
    use resonate_core::{AlbumId, ArtistId};

    use super::{
        Following, Landing, LeftAt, Pane, PlayingRow, QueueStamp, Step, TrackId, UnderThePointer,
        in_front_of, landing, stepped_pane, unsaved,
    };
    use crate::{Selection, Tabs, models::Notice};

    const A_SERVICE: bool = true;
    const NO_SERVICE: bool = false;

    const EVERY_TAB: Tabs = Tabs {
        suggestions: true,
        missing: true,
        counts: true,
    };

    const NO_PLAYLIST_OPEN: bool = false;

    const A_PLAYLIST_OPEN: bool = true;

    fn an_album() -> Selection {
        Selection::Album(AlbumId::new(3).expect("a non-zero id"))
    }

    fn an_artist() -> Selection {
        Selection::Artist(ArtistId::new(5).expect("a non-zero id"))
    }

    #[test]
    fn a_settings_write_whose_writer_died_is_told_as_one_that_failed_is() {
        assert!(unsaved(Ok(Ok(()))).is_none());
        assert!(matches!(
            unsaved(Err(futures_channel::oneshot::Canceled)),
            Some(Notice::Trouble(_))
        ));
    }

    #[test]
    fn a_hint_is_dropped_only_where_what_is_under_a_still_pointer_moved() {
        let at = gpui::point(px(40.0), px(80.0));
        let before = UnderThePointer { at, laid_out: 1 };
        let moved = UnderThePointer { at, laid_out: 2 };
        let pointer_moved = UnderThePointer {
            at: gpui::point(px(41.0), px(80.0)),
            laid_out: 2,
        };

        assert!(moved.moved_since(Some(before)));
        assert!(!before.moved_since(Some(before)));
        assert!(!pointer_moved.moved_since(Some(before)));
        assert!(!before.moved_since(None));
    }

    #[test]
    fn a_list_whose_rows_grew_lands_on_the_row_it_was_left_at_and_as_far_into_it() {
        let left_at = LeftAt::read_off(px(-1_037.0), 36.0);
        assert_eq!(left_at.row, 28);
        assert!((left_at.into_the_row - 29.0 / 36.0).abs() < 1e-5);

        assert_eq!(left_at.offset_at(36.0), px(-1_037.0));
        let grown = f32::from(left_at.offset_at(40.0));
        assert!((grown - -(40.0 * 28.0 + 29.0 / 36.0 * 40.0)).abs() < 1e-3);
    }

    #[test]
    fn an_album_or_an_artist_opened_stands_under_its_own_category_in_the_sidebar() {
        assert_eq!(in_front_of(Pane::Tracks, an_album()), Pane::Albums);
        assert_eq!(in_front_of(Pane::Tracks, an_artist()), Pane::Artists);
        assert_eq!(
            in_front_of(Pane::Tracks, Selection::Everything),
            Pane::Tracks
        );
        assert_eq!(in_front_of(Pane::Lyrics, an_album()), Pane::Lyrics);
    }

    #[test]
    fn pressing_the_category_a_page_stands_under_goes_back_to_the_whole_category() {
        let album_page = in_front_of(Pane::Tracks, an_album());
        let artist_page = in_front_of(Pane::Tracks, an_artist());

        assert_eq!(
            landing(Pane::Albums, album_page, an_album(), NO_PLAYLIST_OPEN),
            Landing::Unscoped(Pane::Albums)
        );
        assert_eq!(
            landing(Pane::Artists, artist_page, an_artist(), NO_PLAYLIST_OPEN),
            Landing::Unscoped(Pane::Artists)
        );
        assert_eq!(
            landing(
                Pane::Playlists,
                Pane::Playlists,
                Selection::Everything,
                A_PLAYLIST_OPEN
            ),
            Landing::EveryPlaylist
        );
    }

    #[test]
    fn coming_back_to_a_category_from_elsewhere_finds_the_page_it_was_left_on() {
        assert_eq!(
            landing(Pane::Albums, Pane::Lyrics, an_album(), NO_PLAYLIST_OPEN),
            Landing::Scoped
        );
        assert_eq!(
            landing(
                Pane::Playlists,
                Pane::Lyrics,
                Selection::Everything,
                A_PLAYLIST_OPEN
            ),
            Landing::On(Pane::Playlists)
        );
        assert_eq!(
            landing(Pane::Artists, Pane::Albums, an_album(), NO_PLAYLIST_OPEN),
            Landing::On(Pane::Artists)
        );
    }

    #[test]
    fn the_tracks_row_always_lists_every_track() {
        let album_page = in_front_of(Pane::Tracks, an_album());

        assert_eq!(
            landing(Pane::Tracks, album_page, an_album(), NO_PLAYLIST_OPEN),
            Landing::Unscoped(Pane::Tracks)
        );
        assert_eq!(
            landing(
                Pane::Tracks,
                Pane::Tracks,
                Selection::Everything,
                NO_PLAYLIST_OPEN
            ),
            Landing::On(Pane::Tracks)
        );
    }

    const ON_SCREEN: bool = true;

    const BEHIND_ANOTHER_PANE: bool = false;

    fn track(id: u64) -> Option<TrackId> {
        TrackId::new(id).ok()
    }

    fn queue_of(rows: &[u64]) -> QueueStamp {
        QueueStamp::of(rows)
    }

    fn playing(row: usize, id: u64, queue: QueueStamp) -> Option<PlayingRow> {
        Some(PlayingRow {
            row,
            track: track(id),
            queue,
        })
    }

    #[test]
    fn the_queue_is_scrolled_to_the_row_that_started_playing() {
        let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let mut following = Following::default();

        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
        assert_eq!(following.follows(playing(8, 9, queue), ON_SCREEN), Some(8));
    }

    #[test]
    fn a_redraw_that_leaves_the_playing_row_where_it_was_scrolls_nothing() {
        let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut following = Following::default();

        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), None);
        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), None);
    }

    #[test]
    fn rows_taken_out_above_the_playing_one_move_it_without_scrolling() {
        let before = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let after = queue_of(&[1, 4, 5, 6, 7, 8]);
        let mut following = Following::default();

        assert_eq!(following.follows(playing(7, 8, before), ON_SCREEN), Some(7));
        assert_eq!(following.follows(playing(5, 8, after), ON_SCREEN), None);
        assert_eq!(following.follows(playing(5, 8, after), ON_SCREEN), None);
    }

    #[test]
    fn a_step_onto_the_same_track_queued_twice_still_scrolls_to_it() {
        let queue = queue_of(&[1, 2, 3]);
        let mut following = Following::default();

        assert_eq!(following.follows(playing(0, 4, queue), ON_SCREEN), Some(0));
        assert_eq!(following.follows(playing(2, 4, queue), ON_SCREEN), Some(2));
    }

    #[test]
    fn another_track_starting_as_the_queue_changes_is_scrolled_to() {
        let before = queue_of(&[1, 2, 3]);
        let after = queue_of(&[1, 3]);
        let mut following = Following::default();

        assert_eq!(following.follows(playing(1, 2, before), ON_SCREEN), Some(1));
        assert_eq!(following.follows(playing(1, 3, after), ON_SCREEN), Some(1));
    }

    #[test]
    fn a_queue_nobody_is_looking_at_is_not_scrolled_until_it_is_drawn_again() {
        let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut following = Following::default();

        assert_eq!(
            following.follows(playing(7, 8, queue), BEHIND_ANOTHER_PANE),
            None
        );
        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
    }

    #[test]
    fn a_queue_that_has_stopped_is_followed_nowhere_and_taken_up_afresh() {
        let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut following = Following::default();

        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
        assert_eq!(following.follows(None, ON_SCREEN), None);
        assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
    }

    #[test]
    fn the_pane_keys_walk_the_sidebar_and_wrap_at_either_end() {
        let listed = Pane::BROWSE;
        let (first, last) = (listed[0], listed[listed.len() - 1]);

        assert_eq!(
            stepped_pane(first, Step::Below, EVERY_TAB, A_SERVICE),
            listed[1]
        );
        assert_eq!(
            stepped_pane(last, Step::Below, EVERY_TAB, A_SERVICE),
            first,
            "the end did not wrap"
        );
        assert_eq!(
            stepped_pane(first, Step::Above, EVERY_TAB, A_SERVICE),
            last,
            "the start did not wrap"
        );

        let walked = listed.iter().fold(first, |pane, _| {
            stepped_pane(pane, Step::Below, EVERY_TAB, A_SERVICE)
        });
        assert_eq!(
            walked, first,
            "walking the whole sidebar did not come back to where it started"
        );
    }

    #[test]
    fn a_pane_the_sidebar_does_not_list_steps_onto_the_first_one_it_does() {
        assert_eq!(
            stepped_pane(Pane::Queue, Step::Below, EVERY_TAB, A_SERVICE),
            Pane::BROWSE[1]
        );
        assert_eq!(
            stepped_pane(Pane::Settings, Step::Below, EVERY_TAB, A_SERVICE),
            Pane::BROWSE[1]
        );
    }

    #[test]
    fn the_pane_keys_step_onto_the_services_row_last_in_the_collection() {
        assert_eq!(
            stepped_pane(Pane::Missing, Step::Below, EVERY_TAB, A_SERVICE),
            Pane::Server
        );
        assert_eq!(
            stepped_pane(Pane::Server, Step::Below, EVERY_TAB, A_SERVICE),
            Pane::Lyrics
        );
        assert_eq!(
            stepped_pane(Pane::Lyrics, Step::Above, EVERY_TAB, A_SERVICE),
            Pane::Server
        );
    }

    #[test]
    fn the_pane_keys_pass_over_the_services_row_where_no_service_is_set() {
        assert_eq!(
            stepped_pane(Pane::Missing, Step::Below, EVERY_TAB, NO_SERVICE),
            Pane::Lyrics
        );
        assert_eq!(
            stepped_pane(Pane::Lyrics, Step::Above, EVERY_TAB, NO_SERVICE),
            Pane::Missing
        );
    }

    #[test]
    fn every_pane_is_listed_once_and_read_back_from_the_name_it_is_stored_under() {
        for pane in Pane::ALL {
            assert_eq!(Pane::parse(pane.as_str()), Some(pane));
            assert_eq!(Pane::ALL.iter().filter(|other| **other == pane).count(), 1);
        }
        for pane in Pane::BROWSE
            .into_iter()
            .chain([Pane::Queue, Pane::Settings])
        {
            assert!(Pane::ALL.contains(&pane));
        }
        assert_eq!(Pane::ALL.len(), Pane::BROWSE.len() + 2);
    }

    #[test]
    fn the_services_pane_is_named_for_the_listener_and_stored_under_the_key_it_always_was() {
        assert_eq!(Pane::Server.label(), "Services");
        assert_eq!(Pane::Server.as_str(), "server");
        assert_eq!(Pane::parse("server"), Some(Pane::Server));
    }

    #[test]
    fn the_pane_keys_step_over_a_tab_that_is_hidden() {
        let hidden = Tabs {
            suggestions: false,
            missing: false,
            counts: true,
        };

        assert_eq!(
            stepped_pane(Pane::Playlists, Step::Below, hidden, NO_SERVICE),
            Pane::Party
        );
        assert_eq!(
            stepped_pane(Pane::Party, Step::Below, hidden, NO_SERVICE),
            Pane::Lyrics
        );
        assert_eq!(
            stepped_pane(Pane::Lyrics, Step::Above, hidden, NO_SERVICE),
            Pane::Party
        );
        assert_eq!(
            stepped_pane(Pane::Playlists, Step::Below, EVERY_TAB, NO_SERVICE),
            Pane::Suggestions
        );
    }

    #[test]
    fn the_missing_tab_is_hidden_until_it_is_asked_for() {
        assert!(Pane::Suggestions.is_shown(Tabs::AS_BUILT));
        assert!(!Pane::Missing.is_shown(Tabs::AS_BUILT));
        assert!(Pane::Missing.is_shown(EVERY_TAB));
    }

    mod driven {
        use std::{path::PathBuf, sync::Arc};

        use gpui::{OperatingSystem, PlatformInfo, PlatformTarget, TestAppContext};
        use resonate_core::{Span, Volume};
        use resonate_library::{Cut, Direction, Library, PlaylistOrder};

        use crate::{
            Followed, Notice, ResonateApp, SettingsCategory,
            app::About,
            driven::{Driven, Folder},
            toast,
            views::{
                menu::Menu,
                party_offer,
                playlists::Held,
                reorder::{Reach, Shift},
                root::{Deleting, Magnified, Pane, RootView},
            },
        };

        fn catalog() -> Arc<Library> {
            Arc::new(Library::open_in_memory().expect("a catalog in memory"))
        }

        fn queued(folder: &Folder, names: &[&str]) -> Vec<PathBuf> {
            names
                .iter()
                .map(|name| folder.tone(&format!("{name}.wav"), 1))
                .collect()
        }

        fn queue_open(cx: &mut TestAppContext, folder: &Folder, names: &[&str]) -> Driven {
            let files = queued(folder, names);
            let mut driven = Driven::opened_in(cx, catalog(), folder);
            driven.play(&files);
            driven.click("queue");
            driven.focus_the_window();
            driven
        }

        fn reach(driven: &mut Driven) -> Option<Reach> {
            driven.read(|root, _| root.reach.clone())
        }

        impl Driven {
            fn focus_the_window(&mut self) {
                let root = self.root.clone();
                self.cx.update(|window, cx| {
                    let focus = root.read(cx).focus.clone();
                    window.focus(&focus, cx);
                });
                self.settle();
            }

            fn lapse_the_type_ahead(&mut self) {
                let root = self.root.clone();
                self.cx.update(|_, cx| {
                    root.update(cx, |root, cx| root.stop_typing(cx));
                });
                self.settle();
                assert!(self.read(|root, _| root.type_ahead.typed().is_none()));
            }
        }

        #[gpui::test]
        fn deleting_the_last_rows_leaves_the_reach_on_the_row_now_last_and_delete_takes_that(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two", "three", "four"]);
            let before = driven.queue();

            driven.cx.simulate_keystrokes("end shift-up delete");
            driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

            assert_eq!(reach(&mut driven), Some(Reach::at(Shift::Queue, 1)));

            driven.cx.simulate_keystrokes("delete");
            driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

            assert_eq!(driven.queue(), [before[0]]);
            assert_eq!(reach(&mut driven), Some(Reach::at(Shift::Queue, 0)));

            driven.cx.simulate_keystrokes("delete");
            driven.until(|root, cx| root.player.read(cx).queue().is_empty());

            assert_eq!(reach(&mut driven), None);
        }

        #[gpui::test]
        fn backspace_after_a_jump_has_lapsed_takes_nothing_out_of_the_queue(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["alpha", "bravo", "charlie"]);

            driven.cx.simulate_keystrokes("c->c");
            driven.until(|root, _| root.reach == Some(Reach::at(Shift::Queue, 2)));
            driven.lapse_the_type_ahead();

            driven.cx.simulate_keystrokes("backspace");
            driven.settle();

            assert_eq!(driven.queue().len(), 3);

            driven.cx.simulate_keystrokes("delete");
            driven.until(|root, cx| root.player.read(cx).queue().len() == 2);
        }

        #[gpui::test]
        fn a_row_reached_by_hand_after_a_jump_is_taken_out_by_backspace(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["alpha", "bravo", "charlie"]);

            driven.cx.simulate_keystrokes("c->c");
            driven.until(|root, _| root.reach == Some(Reach::at(Shift::Queue, 2)));
            driven.lapse_the_type_ahead();

            driven.cx.simulate_keystrokes("up backspace");
            driven.until(|root, cx| root.player.read(cx).queue().len() == 2);
        }

        fn nothing_behind_the_sheet_answers(driven: &mut Driven, rows: usize) {
            nothing_behind_the_sheet_answers_to(
                driven,
                rows,
                "delete enter backspace alt-up down secondary-z",
            );
        }

        fn nothing_behind_the_sheet_answers_to(driven: &mut Driven, rows: usize, keys: &str) {
            let queue = driven.queue();

            driven.cx.simulate_keystrokes(keys);
            driven.cx.simulate_keystrokes("t->t");
            driven.settle();

            assert_eq!(driven.queue(), queue);
            assert_eq!(queue.len(), rows);
            assert_eq!(reach(driven), Some(Reach::at(Shift::Queue, 1)));
            assert!(driven.read(|root, cx| {
                root.type_ahead.typed().is_none() && root.search.read(cx).text().is_empty()
            }));
        }

        #[gpui::test]
        fn the_playlist_picker_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
            driven.cx.simulate_keystrokes("down down");
            let root = driven.root.clone();

            driven.cx.update(|window, cx| {
                root.update(cx, |root, cx| {
                    let rows: Arc<[Cut]> = Arc::from(Vec::new());
                    root.adding = Some(Held::of(rows));
                    window.focus(&root.focus, cx);
                    cx.notify();
                });
            });
            driven.settle();
            nothing_behind_the_sheet_answers(&mut driven, 3);

            driven.cx.simulate_keystrokes("escape");
            assert!(driven.read(|root, _| root.adding.is_none()));
        }

        #[gpui::test]
        fn the_magnified_cover_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
            driven.cx.simulate_keystrokes("down down");
            let location = driven.read(|root, cx| root.player.read(cx).queue()[0].location.clone());
            let root = driven.root.clone();

            driven.cx.update(|window, cx| {
                root.update(cx, |root, cx| {
                    root.magnify(Magnified::File(location), cx);
                    window.focus(&root.focus, cx);
                });
            });
            driven.settle();
            nothing_behind_the_sheet_answers(&mut driven, 3);
        }

        #[gpui::test]
        fn the_listen_sheet_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
            driven.cx.simulate_keystrokes("down down");
            let root = driven.root.clone();

            driven.cx.update(|window, cx| {
                root.update(cx, |root, cx| {
                    root.listening_open = true;
                    window.focus(&root.focus, cx);
                    cx.notify();
                });
            });
            driven.settle();
            nothing_behind_the_sheet_answers_to(
                &mut driven,
                3,
                "delete backspace alt-up down secondary-z",
            );
        }

        #[gpui::test]
        fn the_delete_sheet_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
            driven.cx.simulate_keystrokes("down down");
            let track = driven.queue()[0];
            let root = driven.root.clone();

            driven.cx.update(|window, cx| {
                root.update(cx, |root, cx| {
                    root.deleting = Some(Deleting {
                        tracks: vec![track],
                        title: "one".to_owned(),
                        artist: None,
                        a_cut: false,
                    });
                    window.focus(&root.focus, cx);
                    cx.notify();
                });
            });
            driven.settle();
            nothing_behind_the_sheet_answers_to(
                &mut driven,
                3,
                "delete backspace alt-up down secondary-z",
            );

            driven.cx.simulate_keystrokes("escape");
            assert!(driven.read(|root, _| root.deleting.is_none()));
        }

        fn asking_to_delete(driven: &mut Driven) {
            let track = driven.queue()[0];
            let root = driven.root.clone();

            driven.cx.update(|_, cx| {
                root.update(cx, |root, cx| {
                    root.deleting = Some(Deleting {
                        tracks: vec![track],
                        title: "one".to_owned(),
                        artist: None,
                        a_cut: false,
                    });
                    cx.notify();
                });
            });
            driven.settle();
            driven.settle();
        }

        fn focused_is(driven: &mut Driven, named: &'static str) -> bool {
            let root = driven.root.clone();
            driven.cx.update(|window, cx| {
                root.read(cx)
                    .controls
                    .handle(named)
                    .is_some_and(|handle| handle.is_focused(window))
            })
        }

        #[gpui::test]
        fn focus_enters_the_delete_sheet_on_the_control_that_keeps_the_song(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two"]);

            asking_to_delete(&mut driven);

            assert!(focused_is(&mut driven, "keep-the-track"));
            assert!(!focused_is(&mut driven, "delete-the-track"));

            driven.cx.simulate_keystrokes("enter");
            driven.settle();

            assert!(driven.read(|root, _| root.deleting.is_none()));
            assert_eq!(
                driven.queue().len(),
                2,
                "no key confirms a deletion, enter included"
            );
        }

        #[gpui::test]
        fn tab_stays_inside_the_delete_sheet_in_both_directions(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two"]);
            asking_to_delete(&mut driven);

            for keys in [
                "tab",
                "tab",
                "tab",
                "tab",
                "shift-tab",
                "shift-tab",
                "shift-tab",
            ] {
                driven.cx.simulate_keystrokes(keys);
                driven.settle();

                let inside = driven.read(|root, _| root.front_panel().is_some());
                let root = driven.root.clone();
                let within = driven.cx.update(|window, cx| {
                    root.read(cx)
                        .front_panel()
                        .is_some_and(|panel| panel.contains_focused(window, cx))
                });

                assert!(inside, "the sheet has a panel to keep the focus in");
                assert!(within, "{keys} took the focus out of the delete sheet");
            }
        }

        #[gpui::test]
        fn escape_gives_the_focus_back_to_the_control_the_sheet_was_opened_from(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two"]);
            let root = driven.root.clone();
            let opener = driven.cx.update(|window, cx| {
                let opener = root
                    .read(cx)
                    .standing_controls
                    .handle(Pane::Albums.label())
                    .expect("the Albums tab is in the ring");
                window.focus(&opener, cx);
                opener
            });
            driven.settle();

            asking_to_delete(&mut driven);
            assert!(focused_is(&mut driven, "keep-the-track"));

            driven.cx.simulate_keystrokes("escape");
            driven.settle();
            driven.settle();

            assert!(driven.read(|root, _| root.deleting.is_none()));
            assert!(driven.cx.update(|window, _| opener.is_focused(window)));
        }

        const A_PARTY_LINK: &str = "resonate://party/ABCDE";

        fn offering_a_party(driven: &mut Driven) {
            let root = driven.root.clone();

            driven.cx.update(|_, cx| {
                root.update(cx, |root, cx| {
                    root.follow(Followed::PartyOffered(A_PARTY_LINK.to_owned()), cx);
                });
            });
            driven.settle();
            driven.settle();
        }

        fn together_over(driven: &mut Driven, together: Arc<dyn crate::Together>) {
            driven.cx.update(|_, cx| {
                cx.global_mut::<ResonateApp>().together = Some(together);
            });
        }

        #[gpui::test]
        fn a_party_link_from_outside_asks_first_and_focus_lands_on_the_control_that_declines(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two"]);
            let joining = Arc::new(party_offer::joined::Joining::default());
            together_over(&mut driven, joining.clone());

            offering_a_party(&mut driven);

            assert!(driven.read(|root, _| root.party_offered.is_some()));
            assert!(focused_is(&mut driven, "not-joining-the-party"));
            assert!(!focused_is(&mut driven, "join-the-party"));
            assert!(joining.joined.lock().is_empty(), "nothing joins unasked");

            driven.cx.simulate_keystrokes("enter");
            driven.settle();

            assert!(driven.read(|root, _| root.party_offered.is_none()));
            assert!(
                joining.joined.lock().is_empty(),
                "no key confirms a join, enter included"
            );
        }

        #[gpui::test]
        fn escape_declines_a_party_link_and_the_sheet_holds_the_keys_back(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
            let joining = Arc::new(party_offer::joined::Joining::default());
            together_over(&mut driven, joining.clone());

            driven.cx.simulate_keystrokes("down down");
            offering_a_party(&mut driven);
            driven.focus_the_window();
            nothing_behind_the_sheet_answers_to(
                &mut driven,
                3,
                "backspace alt-up down secondary-z",
            );

            driven.cx.simulate_keystrokes("escape");
            driven.settle();

            assert!(driven.read(|root, _| root.party_offered.is_none()));
            assert!(joining.joined.lock().is_empty());
        }

        #[gpui::test]
        fn joining_an_offered_party_hands_the_link_and_the_zone_shown_to_the_seam(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two"]);
            let joining = Arc::new(party_offer::joined::Joining::default());
            together_over(&mut driven, joining.clone());
            let zone = driven.read(|root, _| root.switcher.selected());

            offering_a_party(&mut driven);
            driven.click("join-the-party");
            driven.settle();
            driven.settle();

            assert!(driven.read(|root, _| root.party_offered.is_none()));
            assert_eq!(
                *joining.joined.lock(),
                vec![(A_PARTY_LINK.to_owned(), zone)]
            );
            assert_eq!(driven.read(|root, _| root.pane), Pane::Party);
        }

        #[gpui::test]
        fn a_party_that_will_not_be_joined_is_told_and_the_pane_stays(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one"]);
            together_over(&mut driven, party_offer::joined::Joining::refusing());

            offering_a_party(&mut driven);
            driven.click("join-the-party");
            driven.settle();
            driven.settle();

            assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);
            assert!(driven.read(|_, cx| RootView::noticed(cx)));
        }

        #[gpui::test]
        fn a_toast_stays_while_the_pointer_is_on_it_and_goes_once_it_leaves(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one"]);

            driven.cx.update(|_, cx| {
                toast::tell(Notice::Done("Queued".to_owned()), cx);
            });
            driven.settle();
            let on_the_toast = driven.centre_of("toast");
            driven.hover(on_the_toast);

            for _ in 0..30 {
                driven
                    .cx
                    .executor()
                    .advance_clock(std::time::Duration::from_secs(1));
                driven.settle();
            }
            assert!(
                driven.read(|_, cx| toast::is_showing(cx)),
                "a toast under the pointer went"
            );

            driven.hover(gpui::point(gpui::px(2.0), gpui::px(2.0)));
            for _ in 0..12 {
                driven
                    .cx
                    .executor()
                    .advance_clock(std::time::Duration::from_secs(1));
                driven.settle();
            }
            assert!(driven.read(|_, cx| !toast::is_showing(cx)));
        }

        #[gpui::test]
        fn escape_closes_an_open_menu_before_it_takes_a_toast_down(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one"]);
            let root = driven.root.clone();

            driven.cx.update(|_, cx| {
                toast::tell(Notice::Done("Queued".to_owned()), cx);
                root.update(cx, |root, cx| {
                    root.menu = Some(Menu::at(gpui::point(gpui::px(40.0), gpui::px(40.0))));
                    cx.notify();
                });
            });
            driven.settle();

            driven.cx.simulate_keystrokes("escape");
            assert!(driven.read(|root, cx| root.menu.is_none() && RootView::noticed(cx)));

            driven.cx.simulate_keystrokes("escape");
            assert!(driven.read(|_, cx| !RootView::noticed(cx)));
        }

        const ON_A_MAC: PlatformInfo = PlatformInfo {
            os: OperatingSystem::Mac,
            target: PlatformTarget::Native,
        };

        #[gpui::test]
        fn on_a_mac_command_steers_the_window_and_control_is_left_to_the_chords_command_cannot_take(
            cx: &mut TestAppContext,
        ) {
            cx.set_platform_info(ON_A_MAC);
            let mut driven = Driven::open(cx, catalog());
            driven.focus_the_window();
            let behind = driven.read(|root, _| root.pane);

            driven.cx.simulate_keystrokes("ctrl-u");
            assert_eq!(driven.read(|root, _| root.pane), behind);

            driven.cx.simulate_keystrokes("cmd-u");
            assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);

            driven.cx.simulate_keystrokes("secondary-u");
            assert_eq!(driven.read(|root, _| root.pane), behind);

            driven.cx.simulate_keystrokes("cmd-,");
            assert_eq!(driven.read(|root, _| root.pane), Pane::Settings);
            assert!(
                driven.cx.update(|window, cx| driven
                    .root
                    .read(cx)
                    .finding
                    .read(cx)
                    .is_focused(window))
            );
        }

        #[gpui::test]
        fn the_menus_about_opens_the_about_category_whatever_stood_there(cx: &mut TestAppContext) {
            let mut driven = Driven::open(cx, catalog());
            driven.focus_the_window();

            driven.cx.dispatch_action(About);

            assert_eq!(driven.read(|root, _| root.pane), Pane::Settings);
            assert_eq!(
                driven.read(|root, _| root.settings_category),
                SettingsCategory::About
            );
        }

        #[gpui::test]
        fn a_key_opens_the_queue_and_the_same_key_goes_back_to_the_pane_it_covered(
            cx: &mut TestAppContext,
        ) {
            let mut driven = Driven::open(cx, catalog());
            driven.focus_the_window();
            let behind = driven.read(|root, _| root.pane);

            driven.cx.simulate_keystrokes("secondary-u");
            assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);

            driven.cx.simulate_keystrokes("secondary-u");
            assert_eq!(driven.read(|root, _| root.pane), behind);
        }

        #[gpui::test]
        fn a_key_mutes_and_the_same_key_brings_back_the_level_it_was_heard_at(
            cx: &mut TestAppContext,
        ) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one"]);
            let heard_at = driven.read(|root, cx| root.player.read(cx).state().volume);

            driven.cx.simulate_keystrokes("ctrl-m");
            driven.until(|root, cx| root.player.read(cx).state().volume == Volume::MUTE);

            driven.cx.simulate_keystrokes("ctrl-m");
            driven.until(|root, cx| root.player.read(cx).state().volume == heard_at);
        }

        #[gpui::test]
        fn two_edits_asked_for_at_once_both_land(cx: &mut TestAppContext) {
            let library = catalog();
            let mut driven = Driven::open(cx, Arc::clone(&library));
            let model = driven.read(|root, _| root.library.clone());

            driven.cx.update(|_, cx| {
                model.update(cx, |model, cx| {
                    model.create_playlist("Meddle".to_owned(), Vec::new(), cx);
                    model.create_playlist("Animals".to_owned(), Vec::new(), cx);
                });
            });
            driven.until(|_, _| {
                library
                    .playlist_lists(PlaylistOrder::Name, Direction::Ascending)
                    .is_ok_and(|lists| lists.len() == 2)
            });
        }

        #[gpui::test]
        fn deleting_every_row_leaves_nothing_reached(cx: &mut TestAppContext) {
            let folder = Folder::new();
            let mut driven = queue_open(cx, &folder, &["one", "two"]);
            let root = driven.root.clone();

            driven.cx.update(|_, cx| {
                root.update(cx, |root, cx| {
                    root.drop_rows(Shift::Queue, Span::between(0, 1), cx);
                });
            });
            driven.until(|root, cx| root.player.read(cx).queue().is_empty());

            assert_eq!(reach(&mut driven), None);
        }
    }
}
