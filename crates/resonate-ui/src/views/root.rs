use std::{
    cell::{Cell, RefCell},
    hash::{Hash as _, Hasher as _},
    mem,
    path::PathBuf,
    rc::Rc,
    slice,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet, AHasher};
use gpui::{
    AnimationElement, AnyElement, App, BoxShadow, Canvas, Context, Div, DragMoveEvent, ElementId,
    Entity, ExternalPaths, FocusHandle, Focusable, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, NavigationDirection, ObjectFit, Pixels, Point, Render,
    ScrollHandle, ScrollStrategy, SharedString, Stateful, Task, UniformListScrollHandle, Window,
    canvas, div, hsla, img, point, prelude::*, px, rgb, rgba,
};
use resonate_core::{
    AlbumId, ArtistsDrawn, MediaLocation, PlaylistId, QueueStamp, Span, TrackId, Volume,
};
use resonate_engine::{
    Command, Counting, Keeping, Listening, Placement, PlayerState, QueueItem, RepeatMode, stamp_of,
};
use resonate_library::{
    ArtistFound, Cut, Direction, HistoryKept, Kept, Playing, Playlist, PlaylistEntry, RowOrder,
    SavedQuery, SortOrder, TokenHeld, Track, is_a_followed_link,
};

use crate::{
    Drawn, EqualiserModel, LibraryModel, LyricsModel, Notice, PlayerModel, ResonateApp, Selection,
    Setting, SettingChange, SettingKey, Tabs, WindowSize,
    analysis::AnalysisModel,
    app::{
        CycleRepeat, DropReached, FocusFilter, FocusSearch, GoToTheResults, LeaveControl,
        LeaveSearch, Listen, LowerRow, Moved, Next, NextPane, PasteAway, Pause,
        PlayPauseUnlessTyping, PlayReached, Previous, PreviousPane, Quit, RaiseRow, ReachAbove,
        ReachBelow, ReachEverything, ReachFirst, ReachLast, ReachNext, ReachPageAbove,
        ReachPageBelow, ReachPrevious, RedoEdit, SeekBackward, SeekForward, SeekFurtherBackward,
        SeekFurtherForward, Stop, TabOnward, ToggleMute, TogglePlayPause, ToggleQueue,
        ToggleShuffle, UndoEdit, VolumeDown, VolumeUp, WINDOW_CONTEXT, WidenAbove, WidenBelow,
        attend, seek_further, seek_step,
    },
    downloads::{self, Download, Fetching},
    format,
    icons::{self, Icon},
    listening::ListenModel,
    models::{Picture, Scale},
    motion,
    settings::SettingsWriter,
    theme,
    toast::{self, Toaster},
    views::{
        browser::{self, ArtistShows, OpenedRecord},
        chrome,
        dropping::{FolderStanding, Incoming, TakingIn},
        field::{Caught, Field, Submitted},
        focus::Controls,
        hint::{self, Names},
        kit::{self, EndsInAnEllipsis},
        listing::{self, Pictured},
        menu::{self, Menu},
        missing::MissingShows,
        part::{Parts, Region},
        playlists::{self, Held, Naming, PlaylistsDrawn, Rows},
        pointed::{self, LitUnderThePointer},
        queue::{Named, QueueMeasure, QueueNames, TakenBack, took_out},
        reorder::{Creeping, Listed, Reach, Shift, Step},
        search::{self, SearchShows},
        settings::{
            Account, Category, FILTER_PLACEHOLDER, HeldBand, Plotted, SigningIn, TidalAccount,
        },
        slider::{Grab, Rail},
        transport::{Handovers, Resolved, ShownCover},
        typing::{self, TypeAhead, jumped},
        visualiser::Visualiser,
    },
};

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

pub(crate) const SETTING_UNSAVED: &str =
    "Couldn't save that setting — the settings file couldn't be written";

const LISTEN_BUTTON_HINT: &str = keyed!(
    "Listen for a song on the desktop or a microphone",
    key!(listen)
);

const SEARCH_PLACEHOLDER: &str = "Type to search title, artist or album";

const CUT: &str = "Cut";

const COPY: &str = "Copy";

const PASTE: &str = "Paste";

const SELECT_ALL: &str = "Select everything";

const SEARCH_HINT: &str = "Words match a title, artist or album, and a term narrows them — everything written has to hold \
     at once, unless or sits between two, where either will do, and a leading - takes one out. \
     title:, artist: and album: scope the words beside them; year:1970-1979, added:<30d, \
     length:>5m, rate:>=96k, depth:24, codec:flac and is:lossless, lossy, mono, stereo, \
     multichannel and hires narrow by what the file is. plays:>5 is every play a track ever \
     had and plays:>5@30d only the ones inside that span. Anything the grammar cannot read is \
     searched for as the words it was written as.";

const PANE_GROUP: &str = "pane";

const JUMPED_TO: &str = "JUMP TO";

const JUMPED_NOWHERE: &str = "NO MATCH";

const CLEAR_SEARCH_HINT: &str = keyed!("Clear the search", key!(leave));

const PINNED_HINT: &str = "Open this pinned playlist";

const PINNED_ROW: f32 = 26.0;

const PINNED_INSET: f32 = 22.0;

const ENRICHING_HINT: &str =
    "The reference is being asked about the library; opens the Online settings";

const SHOW_DOWNLOADS_HINT: &str = "Show the songs asked for and how each is getting on";

const HIDE_DOWNLOADS_HINT: &str = "Hide the songs asked for";

const DISMISS_DOWNLOAD_HINT: &str = "Take this song off the list";

const DOWNLOADS_PANEL_GAP: f32 = 8.0;

const DOWNLOADS_TOGGLE_PRESS: Duration = Duration::from_millis(600);

const CLEAR_DOWNLOADS_HINT: &str = "Take every finished song off the list";

const ASK_AGAIN_HINT: &str = "Ask the providers for this song again";

const CANCEL_DOWNLOAD_HINT: &str = "Stop downloading this song";

const PLAY_DOWNLOAD_HINT: &str = "Play this song";

const OPEN_DOWNLOAD_HINT: &str = "Open the album this song is on";

const KEEP_THE_TRACK_HINT: &str = keyed!("Keep the song", key!(leave));

const DELETE_THE_TRACK_HINT: &str =
    "Delete the file from disk and take the song out of the library";

const DELETED_FOR_GOOD: &str = "This can't be undone.";

const A_CUT_GOES_WHOLE: &str = "It is cut from a file holding other tracks too, so the whole file goes, and every track cut from it.";

const NAME_PLACEHOLDER: &str = "Name the playlist, then press enter";

const CONTACT_PLACEHOLDER: &str = "How a service may reach you, then press enter";

const ACOUSTID_PLACEHOLDER: &str = "An AcoustID client key, then press enter";

const AUDD_PLACEHOLDER: &str = "An AudD API token, then press enter";

const LISTENBRAINZ_PLACEHOLDER: &str = "A ListenBrainz user token, then press enter";

const DISCORD_APP_PLACEHOLDER: &str = "Your Discord application's id, then press enter";

const DISCORD_ICON_PLACEHOLDER: &str = "An asset key or an image address, then press enter";
const FIGURE_PLACEHOLDER: &str = "A number, then press enter";
const LOOKING_PLACEHOLDER: &str = "Which headphones, then press enter";
const ORGANISING_PLACEHOLDER: &str = "How the files are filed, then press enter";

const TYPED_ROOT_PLACEHOLDER: &str = "Or type a folder, then press enter";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Magnified {
    Album(AlbumId),
    File(MediaLocation),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pane {
    Albums,
    Artists,
    #[default]
    Tracks,
    Statistics,
    Queue,
    Playlists,
    Favourites,
    Suggestions,
    Missing,
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

    const fn label(self) -> &'static str {
        match self {
            Self::Library => "LIBRARY",
            Self::Collection => "COLLECTION",
            Self::Playing => "NOW PLAYING",
        }
    }
}

impl Pane {
    pub const BROWSE: [Self; 12] = [
        Self::Albums,
        Self::Artists,
        Self::Tracks,
        Self::Statistics,
        Self::Playlists,
        Self::Favourites,
        Self::Suggestions,
        Self::Missing,
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
            | Self::Artists
            | Self::Tracks
            | Self::Statistics
            | Self::Queue
            | Self::Playlists
            | Self::Favourites
            | Self::Lyrics
            | Self::Inspector
            | Self::Visualiser
            | Self::Analysis
            | Self::Settings => true,
        }
    }

    pub(crate) const fn follows_the_clock(self) -> bool {
        matches!(self, Self::Inspector | Self::Analysis)
    }

    pub const fn lands_where_it_was_left(self) -> bool {
        matches!(self, Self::Albums | Self::Artists | Self::Tracks)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Tracks => "Tracks",
            Self::Statistics => "Statistics",
            Self::Queue => "Queue",
            Self::Playlists => "Playlists",
            Self::Favourites => "Favourites",
            Self::Suggestions => "Suggestions",
            Self::Missing => "Missing",
            Self::Lyrics => "Lyrics",
            Self::Inspector => "Inspector",
            Self::Visualiser => "Visualiser",
            Self::Analysis => "Analysis",
            Self::Settings => "Settings",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Tracks => "tracks",
            Self::Statistics => "statistics",
            Self::Queue => "queue",
            Self::Playlists => "playlists",
            Self::Favourites => "favourites",
            Self::Suggestions => "suggestions",
            Self::Missing => "missing",
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
            "lyrics" => Some(Self::Lyrics),
            "inspector" => Some(Self::Inspector),
            "visualiser" => Some(Self::Visualiser),
            "analysis" => Some(Self::Analysis),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    pub(crate) const fn about(self) -> &'static str {
        match self {
            Self::Albums => "Every album the library has scanned",
            Self::Artists => "Every artist the library has scanned",
            Self::Tracks => "Every track the library has scanned",
            Self::Statistics => "How much has been played, and what most",
            Self::Queue => "What is queued, in the order it will play",
            Self::Playlists => "Lists you have saved, and what is in them",
            Self::Favourites => "The artists, albums and tracks you have starred",
            Self::Suggestions => "Lists the catalog offers to make out of what it holds",
            Self::Missing => {
                "What the releases are short of, and what the artists have put out \
                              that the library does not hold"
            }
            Self::Lyrics => "The words to the playing track, where a source has them",
            Self::Inspector => "What the playing file is, and what reaches the sink",
            Self::Visualiser => "The spectrum and the waveform of what is being heard",
            Self::Analysis => {
                "The whole playing track: its waveform, its spectrum, whether it is the lossless \
                 file it claims to be and what its audio is"
            }
            Self::Settings => "Output, processing and library settings",
        }
    }

    pub(crate) const fn section(self) -> Section {
        match self {
            Self::Albums | Self::Artists | Self::Tracks | Self::Statistics => Section::Library,
            Self::Playlists | Self::Favourites | Self::Suggestions | Self::Missing => {
                Section::Collection
            }
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
            Self::Queue => Icon::Queue,
            Self::Playlists => Icon::Playlists,
            Self::Favourites => Icon::Favourite,
            Self::Suggestions => Icon::Suggestions,
            Self::Missing => Icon::Missing,
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
    downloads_open: bool,
    downloads_pressed_off: Option<Instant>,
    deleting: Option<Deleting>,
    pub(crate) incoming: Option<Incoming>,
    pub(crate) taking_in: Option<TakingIn>,
    pub(crate) watching_the_drag: Task<()>,
    pub(crate) weighing_the_drag: Task<()>,
    pub(crate) music_folder_standing: FolderStanding,
    pub(crate) looking_at_the_music_folder: Task<()>,
    pub(crate) reading_the_room: Task<()>,
    pub(crate) showing_time_left: bool,
    pub(crate) _taking_in: Task<()>,
    pub(crate) settings_written: SettingsWriter,
    pub(crate) pane: Pane,
    pub(crate) settings_category: Category,
    pub(crate) settings_scroll: ScrollHandle,
    pub(crate) settings_rail_scroll: ScrollHandle,
    pub(crate) inspector_scroll: ScrollHandle,
    pub(crate) statistics_scroll: ScrollHandle,
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
    pub(crate) sorting: Option<PlaylistId>,
    pub(crate) ordering: bool,
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
    pub(crate) tidal: [Entity<Field>; 5],
    pub(crate) discord_app: Entity<Field>,
    pub(crate) discord_icon: Entity<Field>,
    pub(crate) organising: Entity<Field>,
    pub(crate) minimum_length: Entity<Field>,
    pub(crate) typed_root: Entity<Field>,
    pub(crate) finding: Entity<Field>,
    pub(crate) figure: Entity<Field>,
    pub(crate) looking: Entity<Field>,
    pub(crate) equaliser: Entity<EqualiserModel>,
    pub(crate) controls: Controls,
    pub(crate) took_out: TakenBack,
    pub(crate) queue_length: QueueMeasure,
    pub(crate) queue_ordered: Option<Task<()>>,
    pub(crate) resetting_everything: bool,
    pub(crate) discarding_the_curve: bool,
    pub(crate) aging_the_history: Option<HistoryKept>,
    pub(crate) reset_everything_landed: bool,
    pub(crate) moving_the_files: bool,
    pub(crate) walking_the_filing_back: bool,
    pub(crate) walking_the_tags_back: bool,
    pub(crate) writing_the_tags: bool,
    pub(crate) keeping_the_tracks: bool,
    pub(crate) vault_named: Option<PathBuf>,
    pub(crate) telling_the_earlier_plays: bool,
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
    under_the_pointer: Option<UnderThePointer>,
    pub(crate) menu: Option<Menu>,
    pub(crate) record: Option<OpenedRecord>,
    left_at: AHashMap<PlaylistId, UniformListScrollHandle>,
    reach: Option<Reach>,
    landed_by_a_jump: Option<Reach>,
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
    listening: Listening,
    resuming: Keeping,
    following: Following,
    magnified: Option<Magnified>,
    pointer_inside: bool,
    volume_settled: Task<()>,
    pub(crate) band_retuning: Task<()>,
    pub(crate) band_untold: Option<bool>,
    volume_aimed: Option<f32>,
    pub(crate) muted_from: Option<Volume>,
    type_ahead: TypeAhead,
    typing_stops: Task<()>,
    last_typed: Option<Instant>,
    pub(crate) queue_names: QueueNames,
    drawn_at: SystemTime,
    scale: Scale,
    pub(crate) resolved: RefCell<Option<Resolved>>,
    pub(crate) shown_cover: RefCell<Option<ShownCover>>,
    pub(crate) handovers: RefCell<Handovers>,
    pub(crate) focus: FocusHandle,
    pub(crate) search: Entity<Field>,
    pub(crate) remember_tab: bool,
    pub(crate) remember_window_size: bool,
    pub(crate) remember_settings_category: bool,
    pub(crate) last_window_size: Option<WindowSize>,
    window_size_settled: Task<()>,
    token_checked: Task<()>,
    pub(crate) signing_in: SigningIn,
    parts: Parts,
}

fn token_notice(held: resonate_library::Result<TokenHeld>) -> Notice {
    match held {
        Ok(TokenHeld::By(user)) => Notice::Done(format!("The token is {user}'s on ListenBrainz")),
        Ok(TokenHeld::Unknown) => Notice::Trouble(
            "ListenBrainz does not know that token, so nothing will be sent under it".to_owned(),
        ),
        Err(error) => {
            tracing::warn!(%error, "ListenBrainz could not be asked about the token");
            Notice::Trouble("ListenBrainz could not be asked whether it knows the token".to_owned())
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
        .border_color(theme::tinted(theme::text(), 0x0c));

    match art {
        Some(art) => frame.child(
            img(art)
                .size(px(side))
                .object_fit(ObjectFit::Cover)
                .rounded(px(cover_rounding(side))),
        ),
        None => frame.child(icons::icon(Icon::Disc, side * 0.42, theme::faint())),
    }
}

impl RootView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let global = cx.global::<ResonateApp>();
        let tabs = global.tabs;
        let remember_tab = global.remember_tab;
        let pane = global
            .last_tab
            .filter(|pane| remember_tab && pane.is_shown(tabs))
            .unwrap_or_default();
        let remember_window_size = global.remember_window_size;
        let remember_settings_category = global.remember_settings_category;
        let settings_category = if remember_settings_category {
            global.last_settings_category
        } else {
            Category::default()
        };
        let last_window_size = WindowSize::from_pixels(window.window_bounds().get_bounds().size);
        let player = Arc::clone(&global.player);
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
        let places = global.places.clone();
        let resume = global.resume;
        let organise_as = global.organise_as.clone();
        let sourcing = global.sourcing.clone();
        let engine = Arc::clone(&global.player);
        let catalog = Arc::clone(&global.library);
        let fingerprinters = Arc::clone(&global.fingerprinters);

        let player = cx.new(|cx| PlayerModel::new(player, cx));
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
        let lyrics = cx.new(|_| LyricsModel::new(lyricists));
        let visualiser = cx.new(|cx| Visualiser::new(player.clone(), cx));
        let reaching = cx.global::<ResonateApp>().online.enabled;
        let analysis =
            cx.new(|_| AnalysisModel::new(engine, catalog, Arc::clone(&fingerprinters), reaching));
        cx.observe(&analysis, |_, _, cx| cx.notify()).detach();
        let listens = cx.global::<ResonateApp>().listens.clone();
        let listen = cx.new(|_| ListenModel::new(listens));
        cx.observe(&listen, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<Toaster>(|_, cx| cx.notify()).detach();
        cx.observe(&player, |this, player, cx| {
            this.count_a_play(&player, cx);
            this.keep_the_queue(&player, cx);
            this.follow_the_playing_row(&player, cx);
            this.look_for_lyrics(cx);
            match player.read(cx).moved() {
                Moved::Clock => this
                    .parts
                    .the_clock_moved(this.pane.follows_the_clock(), cx),
                Moved::More => cx.notify(),
            }
        })
        .detach();
        cx.observe(&library, |this, library, cx| {
            this.forget_what_has_gone(&library, cx);
            let relocated = library.update(cx, |model, _| model.take_relocations());
            if !relocated.is_empty() {
                this.send(Command::Relocate(relocated), cx);
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
        window.focus(&focus);

        let search =
            cx.new(|cx| Field::new(SEARCH_PLACEHOLDER, window, cx).catching(is_a_followed_link));
        cx.observe(&search, |this, search, cx| {
            let typed_anew = {
                let typed = search.read(cx).text();
                this.library.read(cx).narrowing().unwrap_or_default() != typed
            };
            if !typed_anew {
                return;
            }

            let query = search.read(cx).text().to_owned();
            this.set_query(query, cx);
        })
        .detach();

        cx.subscribe_in(&search, window, |this, _, Caught(link), window, cx| {
            this.follow_link(link, window, cx);
        })
        .detach();

        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
            this.library
                .update(cx, |library, cx| library.ask_elsewhere_now(cx));
            this.go_to_the_results(window, cx);
        })
        .detach();

        let name = cx.new(|cx| Field::new(NAME_PLACEHOLDER, window, cx));
        cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
            this.name_given(window, cx);
        })
        .detach();

        let contact = cx.new(|cx| {
            let mut field = Field::new(CONTACT_PLACEHOLDER, window, cx);
            field.hold(online.contact.clone(), cx);
            field
        });
        cx.subscribe_in(&contact, window, |this, _, _: &Submitted, window, cx| {
            this.contact_given(window, cx);
        })
        .detach();

        let acoustid = cx.new(|cx| {
            let mut field = Field::new(ACOUSTID_PLACEHOLDER, window, cx).masked();
            field.hold(online.acoustid_key.clone(), cx);
            field
        });
        cx.subscribe_in(&acoustid, window, |this, _, _: &Submitted, window, cx| {
            this.acoustid_key_given(window, cx);
        })
        .detach();

        let audd = cx.new(|cx| {
            let mut field = Field::new(AUDD_PLACEHOLDER, window, cx).masked();
            field.hold(online.audd_token.clone(), cx);
            field
        });
        cx.subscribe_in(&audd, window, |this, _, _: &Submitted, window, cx| {
            this.audd_token_given(window, cx);
        })
        .detach();

        let listenbrainz = cx.new(|cx| {
            let mut field = Field::new(LISTENBRAINZ_PLACEHOLDER, window, cx).masked();
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

        let subsonic = Account::fields(&online, window, cx);
        let tidal = TidalAccount::fields(&online, window, cx);

        let discord_app = cx.new(|cx| {
            let mut field = Field::new(DISCORD_APP_PLACEHOLDER, window, cx);
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
            let mut field = Field::new(DISCORD_ICON_PLACEHOLDER, window, cx);
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

        let minimum_length = cx.new(|cx| {
            let mut field = Field::new("Seconds, from 0 to 600", window, cx);
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
            let mut field = Field::new(ORGANISING_PLACEHOLDER, window, cx);
            field.hold(organise_as, cx);
            field
        });
        cx.subscribe_in(&organising, window, |this, _, _: &Submitted, window, cx| {
            this.layout_given(window, cx);
        })
        .detach();

        let typed_root = cx.new(|cx| Field::new(TYPED_ROOT_PLACEHOLDER, window, cx));
        cx.subscribe_in(&typed_root, window, |this, _, _: &Submitted, window, cx| {
            this.root_typed(window, cx);
        })
        .detach();

        let finding = cx.new(|cx| Field::new(FILTER_PLACEHOLDER, window, cx));
        cx.subscribe_in(&finding, window, |this, _, _: &Submitted, window, cx| {
            this.leave_filter(window, cx);
        })
        .detach();

        let figure = cx.new(|cx| Field::new(FIGURE_PLACEHOLDER, window, cx));
        cx.subscribe_in(&figure, window, |this, _, _: &Submitted, window, cx| {
            this.figure_given(window, cx);
        })
        .detach();

        let looking = cx.new(|cx| Field::new(LOOKING_PLACEHOLDER, window, cx));
        cx.subscribe_in(&looking, window, |this, _, _: &Submitted, _window, cx| {
            this.look_for_a_device(cx);
        })
        .detach();

        let equaliser = cx.new(|cx| {
            EqualiserModel::saved_on_leaving(
                places.equaliser.clone(),
                Arc::clone(&corrections),
                bindings,
                cx,
            )
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

        let view = Self {
            player,
            library,
            lyrics,
            visualiser,
            analysis,
            listen,
            listening_open: false,
            downloads_open: false,
            downloads_pressed_off: None,
            deleting: None,
            incoming: None,
            taking_in: None,
            watching_the_drag: Task::ready(()),
            weighing_the_drag: Task::ready(()),
            music_folder_standing: FolderStanding::default(),
            looking_at_the_music_folder: Task::ready(()),
            reading_the_room: Task::ready(()),
            showing_time_left: false,
            _taking_in: Task::ready(()),
            settings_written: SettingsWriter::over(settings),
            pane,
            settings_category,
            settings_scroll: ScrollHandle::new(),
            settings_rail_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            statistics_scroll: ScrollHandle::new(),
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
            sorting: None,
            ordering: false,
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
            tidal,
            discord_app,
            discord_icon,
            organising,
            minimum_length,
            typed_root,
            finding,
            figure,
            looking,
            equaliser,
            controls: Controls::default(),
            took_out: TakenBack::default(),
            queue_length: QueueMeasure::default(),
            queue_ordered: None,
            resetting_everything: false,
            discarding_the_curve: false,
            aging_the_history: None,
            reset_everything_landed: false,
            moving_the_files: false,
            walking_the_filing_back: false,
            walking_the_tags_back: false,
            writing_the_tags: false,
            keeping_the_tracks: false,
            vault_named: None,
            telling_the_earlier_plays: false,
            earlier_plays_asked: false,
            query_sort: SortOrder::default(),
            query_reading: SortOrder::default().reads(),
            query_cap: None,
            listening: Listening::default(),
            resuming: Keeping::default(),
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
            under_the_pointer: None,
            menu: None,
            record: None,
            left_at: AHashMap::new(),
            reach: None,
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
            band_retuning: Task::ready(()),
            band_untold: None,
            volume_aimed: None,
            muted_from: None,
            type_ahead: TypeAhead::default(),
            typing_stops: Task::ready(()),
            last_typed: None,
            queue_names: QueueNames::default(),
            drawn_at: SystemTime::now(),
            scale: Scale::ONE,
            resolved: RefCell::new(None),
            shown_cover: RefCell::new(None),
            handovers: RefCell::default(),
            focus,
            search,
            remember_tab,
            remember_window_size,
            remember_settings_category,
            last_window_size,
            window_size_settled: Task::ready(()),
            token_checked: Task::ready(()),
            signing_in: SigningIn::default(),
            parts: Parts::of(&cx.entity(), cx),
        };
        cx.observe_window_bounds(window, |this, window, cx| {
            this.window_bounds_changed(window, cx);
        })
        .detach();
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

    fn count_a_play(&mut self, player: &Entity<PlayerModel>, cx: &mut Context<Self>) {
        let (state, queue) = {
            let model = player.read(cx);
            (model.state().clone(), model.queue())
        };
        let Some(counting) = self.listening.heard(&state, &queue) else {
            return;
        };

        self.library.update(cx, |library, cx| match counting {
            Counting::Counts(played) => library.track_heard(played, cx),
            Counting::Hears(played) => library.track_hearing(played.heard, cx),
            Counting::Settles(played) => library.track_settled(played.heard, cx),
            Counting::Passes(heard) => library.track_passed(heard, cx),
        });
    }

    fn keep_the_queue(&mut self, player: &Entity<PlayerModel>, cx: &mut Context<Self>) {
        if !self.library.read(cx).resumes() {
            return;
        }
        let (state, queued) = {
            let model = player.read(cx);
            (model.state().clone(), model.queued())
        };
        let Some(keep) = self.resuming.kept(&state, &queued) else {
            return;
        };

        self.library
            .update(cx, |library, cx| library.keep_the_queue(keep, cx));
    }

    fn follow_the_playing_row(&mut self, player: &Entity<PlayerModel>, cx: &App) {
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
        self.player.read(cx).send(command);
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
        self.settings_written
            .change(SettingChange::Store(setting.clone()));
        self.write_the_settings(cx);
    }

    pub(crate) fn forget(&mut self, key: SettingKey, cx: &mut Context<Self>) {
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
                if let Ok(Err(error)) = written {
                    tracing::error!(%error, "a setting could not be saved");
                    toast::tell(Notice::Trouble(SETTING_UNSAVED.to_owned()), cx);
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

    pub(crate) fn play_entries(&mut self, entries: &[PlaylistEntry], cx: &mut Context<Self>) {
        let items = playlists::queue_items(entries, &[]);
        if items.is_empty() {
            return;
        }
        let scanned: Vec<Track> = entries
            .iter()
            .filter_map(|entry| entry.track.clone())
            .collect();

        self.library.update(cx, |library, _| {
            library.remember(&scanned);
            library.set_playing_playlist(None);
        });
        self.send(
            Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn play_playlist(
        &mut self,
        playlist: PlaylistId,
        entries: &[PlaylistEntry],
        start_at: usize,
        whole: bool,
        cx: &mut Context<Self>,
    ) {
        let items = playlists::queue_items(entries, &[]);
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
        self.sorting = None;
        self.adding = Some(holding);
        self.name.update(cx, |name, cx| {
            name.clear(cx);
            name.take_focus(window);
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
        self.sorting = None;
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
            name.take_focus(window);
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

    pub(crate) fn order_a_listing(&mut self, cx: &mut Context<Self>) {
        self.ordering = !self.ordering;
        cx.notify();
    }

    pub(crate) fn sort_a_playlist(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.naming = None;
        self.adding = None;
        self.sorting = (self.sorting != Some(playlist)).then_some(playlist);

        let kept = self.saved(playlist, cx).and_then(|found| found.kept);
        self.keeping = kept.is_some();
        if let Some(kept) = kept {
            self.row_order = kept.order;
            self.row_reading = kept.reading;
        }
        cx.notify();
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
        cx.notify();
    }

    pub(crate) fn read_a_search(&mut self, reading: Direction, cx: &mut Context<Self>) {
        self.query_reading = reading;
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
        window.focus(&self.focus);
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
        let given = self.contact.read(cx).text().trim().to_owned();
        self.contact
            .update(cx, |contact, cx| contact.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.contact = given.clone());

        let said = if given.is_empty() {
            "No contact is sent from the next request on"
        } else {
            "The contact is sent from the next request on"
        };
        self.store(&Setting::Contact(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
        cx.notify();
    }

    fn acoustid_key_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.acoustid.read(cx).text().trim().to_owned();
        self.acoustid
            .update(cx, |key, cx| key.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.acoustid_key = given.clone());

        let said = if given.is_empty() {
            "Nothing is recognised from the next start"
        } else {
            "The key is used from the next start"
        };
        self.store(&Setting::AcoustidKey(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn leave_acoustid_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.acoustid.read(cx).is_focused(window) {
            return;
        }
        if self.acoustid.read(cx).text().trim() != cx.global::<ResonateApp>().online.acoustid_key {
            self.acoustid_key_given(window, cx);
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn put_back_acoustid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.acoustid_key.to_owned();
        self.acoustid.update(cx, |field, cx| field.hold(stored, cx));
        self.leave_acoustid_key(window, cx);
    }

    fn audd_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.audd.read(cx).text().trim().to_owned();
        self.audd
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.audd_token = given.clone());

        let said = if given.is_empty() {
            "AudD is not asked from the next start"
        } else {
            "The token is used from the next start"
        };
        self.store(&Setting::AuddToken(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn leave_audd_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.audd.read(cx).is_focused(window) {
            return;
        }
        if self.audd.read(cx).text().trim() != cx.global::<ResonateApp>().online.audd_token {
            self.audd_token_given(window, cx);
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn put_back_audd(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.audd_token.to_owned();
        self.audd.update(cx, |field, cx| field.hold(stored, cx));
        self.leave_audd_token(window, cx);
    }

    fn listenbrainz_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.listenbrainz.read(cx).text().trim().to_owned();
        self.listenbrainz
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.listenbrainz_token = given.clone();
        });

        let said = if given.is_empty() {
            "Nothing heard is sent to ListenBrainz from now on"
        } else {
            "What is heard from now on is sent to ListenBrainz"
        };
        self.store(&Setting::ListenbrainzToken(given.clone()), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        self.check_the_listenbrainz_token(given, cx);
        window.focus(&self.focus);
        cx.notify();
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
        window.focus(&self.focus);
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
        window.focus(&self.focus);
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
        window.focus(&self.focus);
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
        window.focus(&self.focus);
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
        window.focus(&self.focus);
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
            Self::keep_wayback(&mut self.came_from, leaving);
            self.goes_forward.clear();
        }
    }

    pub(crate) fn show_in_the_search(&mut self, shows: SearchShows, cx: &mut Context<Self>) {
        if let Some(pane) = shows.pane() {
            self.set_pane(pane, cx);
        }
        if self.search_shows != shows {
            self.ordering = false;
            self.reach = None;
        }
        self.search_shows = shows;
        cx.notify();
    }

    pub(crate) fn set_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let pane = if pane.is_shown(cx.global::<ResonateApp>().tabs) {
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
            self.ordering = false;
            if let Some(shows) = SearchShows::in_place_of(pane) {
                self.search_shows = shows;
            }
        }
        self.pane = pane;
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

    pub(crate) fn reaches(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .is_some_and(|reach| reach.shift == shift && reach.rows().holds(row))
    }

    pub(crate) fn acting_on(&self, shift: Shift, row: usize) -> Span {
        match self.reaching(shift) {
            Some(rows) if rows.holds(row) => rows,
            _ => Span::one(row),
        }
    }

    pub(crate) fn reaching(&self, shift: Shift) -> Option<Span> {
        self.reach
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
        let held = self.reach.filter(|reach| reach.shift == shift);
        self.landed_by_a_jump = None;
        self.reach = Some(match (extending, held) {
            (true, Some(reach)) => Reach {
                shift,
                anchor: reach.anchor,
                row,
            },
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
        let row = match self.reach_in(shift, held) {
            Some(reach) => step
                .landing(reach.rows(), held)
                .unwrap_or_else(|| step.end(reach.rows())),
            None => self.opening_row(shift, held, cx),
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
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

        self.reach = Some(Reach {
            shift,
            anchor: 0,
            row: held - 1,
        });
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

        self.drop_rows(shift, reach.rows(), cx);
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
            return ((shown / theme::grid_row()) as usize).max(1) * self.grid_columns();
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

        self.reach = Some(Reach {
            shift,
            anchor: reach.anchor,
            row,
        });
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
        let Some(to) = step.landing(reach.rows(), held) else {
            return;
        };

        self.shift_rows(shift, reach.rows(), to, cx);
        self.reach = Some(reach.stepped(step));
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
        let row = reach.rows().first();

        match shift {
            Shift::Queue => self.send_by_position(Command::JumpTo(row), cx),
            Shift::Playlist(playlist) => {
                let entries = self.library.read(cx).entries().to_vec();
                self.play_playlist(playlist, &entries, row, true, cx);
            }
            Shift::Listing(Listed::Tracks) => {
                if let Some(found) = self.library.read(cx).found_at(row) {
                    self.library
                        .update(cx, |library, cx| library.want_found(found, cx));
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
            Shift::Listing(Listed::Missing) => self.open_what_is_missing_at(row, cx),
            Shift::Listing(Listed::Top) => {
                let library = self.library.read(cx);
                let held = library.listing().len().min(search::SONGS_AT_THE_TOP);
                if row >= held {
                    let Some(found) = library.found().get(row - held).cloned() else {
                        return;
                    };
                    self.library
                        .update(cx, |library, cx| library.want_found(found, cx));
                    return;
                }
                let Some((played, start)) = library.played_from(row) else {
                    return;
                };
                self.play_the_listing_from(&played, start, window, cx);
            }
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

    pub(crate) fn drop_rows(&mut self, shift: Shift, rows: Span, cx: &mut Context<Self>) {
        let held = match shift {
            Shift::Queue => {
                let queued = self.player.read(cx).queue();
                let kept = self.took_out.keeping(&queued, rows);
                self.send_by_position(Command::Remove(rows), cx);
                if let Some(kept) = kept {
                    toast::tell(took_out(kept), cx);
                }
                queued.len()
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
                Err(error) => tracing::error!(%error, "the whole listing could not be read"),
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
        if self.library.read(cx).is_the_whole_listing(held) {
            self.play(held, start, cx);
            return;
        }
        let Some(pressed) = held.get(start).map(|track| track.id) else {
            return;
        };
        self.with_everything_listed(window, cx, move |this, listing, _, cx| {
            let Some(start) = listing.iter().position(|track| track.id == pressed) else {
                tracing::warn!(%pressed, "the row pressed is not in the listing read whole");
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
                Err(error) => tracing::error!(%error, "a suggestion's rows could not be read"),
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
        let (library, mut asked) = self.library.read(cx).listing_whole();
        asked.album = match scoped {
            Selection::Album(album) => Some(album),
            Selection::Everything | Selection::Artist(_) => None,
        };
        asked.artist = match scoped {
            Selection::Artist(artist) => Some(artist),
            Selection::Everything | Selection::Album(_) => None,
        };

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update(cx, |this, cx| match read {
                Ok(listing) if now => this.play(&listing, 0, cx),
                Ok(listing) => this.queue(&listed(&listing), at, cx),
                Err(error) => tracing::error!(%error, "a scoped listing could not be read"),
            });
            let _ = outcome;
        });
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
                let library = self.library.read(cx);
                let songs = library.listing().len().min(search::SONGS_AT_THE_TOP);
                let found = library.found().len().min(search::FOUND_AT_THE_TOP);
                let rows = songs + found;
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
                let rows = self.library.read(cx).artists().len();

                (rows > 0).then_some((Shift::Listing(Listed::Artists), rows))
            }
            Pane::Albums => {
                let rows = self.library.read(cx).albums().len();

                (rows > 0).then_some((Shift::Listing(Listed::Albums), rows))
            }
            Pane::Favourites => {
                let rows = self.library.read(cx).favourite_tracks().len();

                (rows > 0).then_some((Shift::Listing(Listed::Favourites), rows))
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
            Pane::Lyrics | Pane::Inspector | Pane::Visualiser | Pane::Analysis | Pane::Settings => {
                None
            }
        }
    }

    fn reach_in(&self, shift: Shift, held: usize) -> Option<Reach> {
        self.reach
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
            Shift::Listing(Listed::Missing) => {
                self.missing_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Suggested) => {
                self.suggestion_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Top) => {}
            Shift::Listing(Listed::Offered | Listed::Heard) => self.reached_unseen.set(true),
        }
    }

    fn step_pane(&mut self, step: Step, cx: &mut Context<Self>) {
        let tabs = cx.global::<ResonateApp>().tabs;
        let stepped = stepped_pane(self.in_front(cx), step, tabs);
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
        if self.settings_category != category {
            self.settings_category = category;
            self.settings_scroll.set_offset(Point::default());
            self.remember_current_settings_category(cx);
        }
        self.disarm();
        cx.notify();
    }

    fn let_the_control_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm();
        if self.controls.lets_go(window) {
            cx.notify();
        }
    }

    fn laid_out(&self, cx: &App) -> u64 {
        let library = self.library.read(cx);
        let mut stamp = AHasher::default();
        self.pane.hash(&mut stamp);
        self.settings_category.hash(&mut stamp);
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

    fn hints_are_wanted(&self, cx: &App) -> bool {
        self.pointer_inside
            && self.grabbed.is_none()
            && self.held_band.is_none()
            && self.adding.is_none()
            && self.magnified.is_none()
            && self.record.is_none()
            && self.deleting.is_none()
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
        label: impl IntoElement,
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
        label: impl IntoElement,
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
            self.ordering = false;
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
        } = wayback;
        self.library
            .update(cx, |library, cx| library.select(selection, cx));
        self.set_pane(pane, cx);
        self.landing_on = pane.lands_where_it_was_left().then_some(at);
    }

    pub(crate) fn way_back_to(&self) -> Option<SharedString> {
        self.came_from.last().map(|back| back.named.clone())
    }

    pub(crate) fn step_back(&mut self, cx: &mut Context<Self>) {
        if self.goes_back() {
            self.go_back(cx);
        } else {
            let category = self.in_front(cx);
            self.choose_pane(category, cx);
        }
    }

    pub(crate) fn goes_back(&self) -> bool {
        !self.came_from.is_empty()
    }

    fn goes_back_to_a_scope(&self) -> bool {
        self.came_from
            .last()
            .is_some_and(|back| back.selection != Selection::Everything)
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
            Pane::Albums => theme::grid_row(),
            Pane::Artists if self.artists_drawn == ArtistsDrawn::Grid => theme::grid_row(),
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
            | Pane::Favourites
            | Pane::Suggestions
            | Pane::Missing
            | Pane::Lyrics
            | Pane::Inspector
            | Pane::Visualiser
            | Pane::Analysis
            | Pane::Settings => &self.track_rows,
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
                        .shadow(vec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.6),
                            offset: point(px(0.0), px(16.0)),
                            blur_radius: px(48.0),
                            spread_radius: px(0.0),
                        }])
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
            "escape" if self.deleting.is_some() => self.keep_the_track(cx),
            "escape" if self.listening_open => self.close_the_listener(cx),
            "escape" if self.magnified.is_some() => self.shrink_cover(cx),
            "escape" if self.adding.is_some() => self.stop_naming(window, cx),
            "escape" if self.record.is_some() => {
                self.record = None;
                cx.notify();
            }
            "escape" if self.menu.is_some() => {
                self.close_the_menu(cx);
            }
            "escape" if self.downloads_open => self.close_the_downloads(cx),
            "escape" if Self::noticed(cx) => toast::dismiss(cx),
            "escape" => {
                if !self.stop_typing(cx) {
                    self.dismiss_search(window, cx);
                }
            }
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
                search.take_focus(window);
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
        self.landed_by_a_jump = self.reach;
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

    pub(crate) const fn something_stands_over_the_pane(&self) -> bool {
        self.menu.is_some()
            || self.adding.is_some()
            || self.magnified.is_some()
            || self.record.is_some()
            || self.listening_open
            || self.deleting.is_some()
    }

    fn reach_by_hand(&mut self, cx: &mut Context<Self>) {
        self.stop_typing(cx);
        self.landed_by_a_jump = None;
    }

    fn stands_where_a_jump_landed(&self, cx: &App) -> bool {
        let Some(landed) = self.landed_by_a_jump else {
            return false;
        };

        self.reach == Some(landed)
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
            JUMPED_TO
        } else {
            JUMPED_NOWHERE
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
                        .border_color(rgb(theme::outline()))
                        .shadow(vec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.45),
                            offset: point(px(0.0), px(4.0)),
                            blur_radius: px(16.0),
                            spread_radius: px(0.0),
                        }])
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
            &self.name,
            &self.contact,
            &self.acoustid,
            &self.audd,
            &self.listenbrainz,
            &self.discord_app,
            &self.discord_icon,
            &self.organising,
            &self.minimum_length,
            &self.typed_root,
            &self.finding,
            &self.figure,
            &self.looking,
        ]
        .into_iter()
        .chain(&self.subsonic)
        .chain(&self.tidal)
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
        window.focus(&self.focus);
        self.reach = None;
        self.reach_row(Step::Below, cx);
        cx.notify();
    }

    fn tab_onward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.read(cx).is_focused(window) {
            self.go_to_the_results(window, cx);
        } else {
            window.focus_next();
        }
    }

    fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_the_menu(cx) {
            return;
        }
        if self.finding.read(cx).is_focused(window) {
            self.finding.update(cx, |finding, cx| finding.clear(cx));
            self.leave_filter(window, cx);
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
        if self.minimum_length.read(cx).is_focused(window) {
            let length = cx
                .global::<ResonateApp>()
                .library
                .music_filters()
                .minimum_length;
            self.minimum_length
                .update(cx, |field, cx| field.hold(length.seconds().to_string(), cx));
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        if self.organising.read(cx).is_focused(window) {
            self.put_back_organising(window, cx);
            return;
        }
        if self.typed_root.read(cx).is_focused(window) {
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
        if self.naming.is_some() || self.adding.is_some() {
            self.stop_naming(window, cx);
            return;
        }
        if self.adding_songs_to.is_some() {
            self.finish_adding_songs(cx);
            return;
        }

        self.reach = None;
        window.focus(&self.focus);
        if self.search.read(cx).text().is_empty() {
            let scoped = self.library.read(cx).selection() != Selection::Everything;
            if scoped || self.goes_back_to_a_scope() {
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
            search.take_focus(window);
            search.set_text(instead, cx);
        });
    }

    pub(crate) fn let_go_of_the_reach_in(&mut self, shift: Shift) {
        if self.reach.is_some_and(|reach| reach.shift == shift) {
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
        self.ordering = false;
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
            self.open_the_search(cx);
        }
        self.search_scroll.set_offset(point(px(0.0), px(0.0)));
        self.read_from_the_top(cx);
        if self
            .reach
            .is_some_and(|reach| matches!(reach.shift, Shift::Listing(_)))
        {
            self.reach = None;
        }
        self.library
            .update(cx, |library, cx| library.set_query(query, cx));
        cx.notify();
    }

    pub(crate) fn show_playlist(&mut self, opened: Option<PlaylistId>, cx: &mut Context<Self>) {
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

        self.volume_settled = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VOLUME_SETTLE).await;
            let stored = this.update(cx, |this, cx| {
                this.volume_aimed = None;
                this.store(&Setting::Volume(volume), cx);
            });
            let _ = stored;
        });
    }

    fn header(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let client_side = chrome::client_side(window);
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
            .when(client_side, chrome::titlebar)
            .child(self.wordmark())
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
                        kit::icon_button("listen", Icon::Listen, LISTEN_BUTTON_HINT)
                            .flex_none()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| this.open_the_listener(cx))),
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
                    .when(client_side, |bar| {
                        bar.child(self.window_controls(window, cx))
                    }),
            )
    }

    fn wordmark(&self) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .w(px(theme::sidebar_width() - 32.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.0))
                    .rounded_md()
                    .bg(rgb(theme::accent()))
                    .child(icons::icon(Icon::Resonate, 14.0, theme::accent_ink())),
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
            .rounded_md()
            .bg(rgb(if searching {
                theme::background()
            } else {
                theme::raised()
            }))
            .border_1()
            .border_color(if searching {
                theme::tinted(theme::accent(), 0x99)
            } else {
                theme::tinted(theme::outline(), 0xff)
            })
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| {
                field
                    .border_color(rgb(theme::outline()))
                    .bg(rgb(theme::hover()))
            })
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
                    .update(cx, |search, _| search.take_focus(window));
                cx.notify();
            }))
            .child(self.search.clone())
            .when(!empty, |field| field.child(self.clear_search(cx)))
            .when(empty && !searching, |field| {
                field.child(kit::figure("ctrl-f").text_color(rgb(theme::faint())))
            })
            .child(hint::explains("search-terms", SEARCH_HINT));

        menu::opens_a_menu(
            box_of_words,
            |this, at, cx| {
                let holding = this.search.read(cx).holds_a_selection();

                Menu::at(at)
                    .when_some(holding.then_some(()), |menu, ()| {
                        menu.under(Icon::Rename, CUT, "ctrl-x", |this, window, cx| {
                            this.search.update(cx, |field, cx| field.cuts(window, cx));
                        })
                        .under(
                            Icon::Export,
                            COPY,
                            "ctrl-c",
                            |this, window, cx| {
                                this.search.update(cx, |field, cx| field.copies(window, cx));
                            },
                        )
                    })
                    .under(Icon::Import, PASTE, "ctrl-v", |this, window, cx| {
                        this.search.update(cx, |field, cx| field.pastes(window, cx));
                    })
                    .under(Icon::Check, SELECT_ALL, "ctrl-a", |this, window, cx| {
                        this.search
                            .update(cx, |field, cx| field.selects_everything(window, cx));
                    })
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
            .names(CLEAR_SEARCH_HINT)
            .on_click(cx.listener(|this, _, window, cx| {
                this.search.update(cx, |search, cx| {
                    search.take_focus(window);
                    search.clear(cx);
                });
            }))
            .child(icons::lit_on_hover(
                icons::icon(Icon::Close, theme::clear_mark(), theme::muted()),
                "clear",
            ))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
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
        let enriching = library.is_enriching();
        let downloads = !library.downloads().is_empty();

        let mut browse = div().flex().flex_col().gap_4();
        for section in Section::ALL {
            let mut listed = div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(kit::eyebrow(section.label()).px_3().pb_1p5());
            for pane in Pane::BROWSE
                .into_iter()
                .filter(|pane| pane.section() == section && pane.is_shown(tabs))
            {
                let count = match pane {
                    Pane::Albums => Some(albums),
                    Pane::Artists => Some(artists),
                    Pane::Tracks => Some(tracks),
                    Pane::Statistics => (plays > 0).then_some(plays),
                    Pane::Playlists => Some(saved),
                    Pane::Favourites => (favourites > 0).then_some(favourites),
                    Pane::Suggestions => (offered > 0).then_some(offered),
                    Pane::Missing => (missing > 0).then_some(missing),
                    Pane::Queue
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

        div()
            .flex()
            .flex_none()
            .flex_col()
            .justify_between()
            .gap_1()
            .p_3()
            .w(px(theme::sidebar_width()))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::surface()))
            .child(browse)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(downloads, |column| column.child(self.download_status(cx)))
                    .when(enriching, |column| column.child(self.enrichment_status(cx)))
                    .child(self.pane_row(Pane::Settings, None, cx)),
            )
    }

    fn pinned_rows(&self, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let library = self.library.read(cx);
        let pinned = library.pinned();
        let opened = (self.pane == Pane::Playlists)
            .then(|| library.opened())
            .flatten();
        let playing = self.playing_playlist(cx);

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
                    .flex()
                    .items_center()
                    .h(px(PINNED_ROW))
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
                    .names(PINNED_HINT)
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
            .collect()
    }

    fn download_status(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let library = self.library.read(cx);
        let fetching: Vec<Fetching> = library
            .downloads()
            .iter()
            .map(|download| library.fetching(download))
            .collect();
        let underway = fetching.iter().copied().any(Fetching::is_underway);
        let arriving = library.downloads().iter().any(|download| {
            library
                .fetched(download)
                .is_some_and(|fetched| fetched.is_arriving())
        });
        let said = downloads::summed_up(&fetching, arriving);
        let open = self.downloads_open;
        let mark = if underway {
            theme::accent()
        } else {
            theme::faint()
        };

        div()
            .id("downloads")
            .debug_selector(|| "downloads".to_owned())
            .flex()
            .items_center()
            .gap_2p5()
            .px_3()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(if open {
                HIDE_DOWNLOADS_HINT
            } else {
                SHOW_DOWNLOADS_HINT
            })
            .on_click(cx.listener(|this, _, _, cx| {
                let closed_by_this_press = this
                    .downloads_pressed_off
                    .take()
                    .is_some_and(|pressed| pressed.elapsed() < DOWNLOADS_TOGGLE_PRESS);
                if closed_by_this_press {
                    return;
                }
                this.downloads_open = !this.downloads_open;
                cx.notify();
            }))
            .child(icons::icon(Icon::Download, theme::text_base(), mark))
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
            ))
    }

    fn downloads_over_the_app(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnimationElement<Stateful<Div>>> {
        let library = self.library.read(cx);
        if !self.downloads_open || library.downloads().is_empty() {
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
            .overflow_y_scroll();
        for (at, (download, fetching)) in rows.into_iter().enumerate() {
            list = list.child(self.download_row(at, &download, fetching, cx));
        }

        let resting = px(theme::transport_height() + DOWNLOADS_PANEL_GAP);

        Some(motion::risen_in(
            div()
                .id("downloads-panel")
                .absolute()
                .left(px(theme::sidebar_width() + DOWNLOADS_PANEL_GAP))
                .bottom(resting)
                .w(px(theme::downloads_width()))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_lg()
                .bg(rgb(theme::surface()))
                .border_1()
                .border_color(rgb(theme::border()))
                .shadow(vec![BoxShadow {
                    color: hsla(0.0, 0.0, 0.0, 0.5),
                    offset: point(px(0.0), px(12.0)),
                    blur_radius: px(32.0),
                    spread_radius: px(0.0),
                }])
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
                        .child(kit::eyebrow("Downloads"))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .when(finished, |actions| {
                                    actions.child(
                                        kit::button(
                                            "clear-downloads",
                                            None,
                                            "Clear finished",
                                            CLEAR_DOWNLOADS_HINT,
                                            kit::Tone::Ghost,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.library.update(cx, |library, cx| {
                                                    library.clear_finished_downloads(cx);
                                                });
                                            }),
                                        ),
                                    )
                                })
                                .child(
                                    kit::icon_button(
                                        "close-downloads",
                                        Icon::Close,
                                        HIDE_DOWNLOADS_HINT,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.close_the_downloads(cx);
                                        },
                                    )),
                                ),
                        ),
                )
                .child(list),
            "downloads-arrives",
            resting,
        ))
    }

    fn close_the_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads_open = false;
        cx.notify();
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

        let text = div()
            .id(listing::keyed_by("open-download", &recording))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .when_some(album, |text, album| {
                text.cursor_pointer()
                    .names(OPEN_DOWNLOAD_HINT)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_the_downloads(cx);
                        this.opened(Selection::Album(album), cx);
                    }))
            })
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
                    .child(saying),
            );

        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .when_some(track, |actions, track| {
                actions.child(
                    kit::icon_button(
                        listing::keyed_by("play-download", &recording),
                        Icon::Play,
                        PLAY_DOWNLOAD_HINT,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.play_what_was_downloaded(track, cx);
                    })),
                )
            })
            .when(fetching.can_be_asked_again(), |actions| {
                actions.child(
                    kit::icon_button(
                        listing::keyed_by("ask-again", &recording),
                        Icon::Redo,
                        ASK_AGAIN_HINT,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let wanted = again.clone();
                        this.library
                            .update(cx, |library, cx| library.want_found(wanted, cx));
                    })),
                )
            })
            .when(fetching.can_be_cancelled(), |actions| {
                let cancelled = recording.clone();
                actions.child(
                    kit::icon_button(
                        listing::keyed_by("cancel-download", &recording),
                        Icon::Stop,
                        CANCEL_DOWNLOAD_HINT,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.cancel_download(&cancelled, cx);
                        });
                    })),
                )
            })
            .when(!fetching.is_underway(), |actions| {
                let dismissed = recording.clone();
                actions.child(
                    kit::icon_button(
                        listing::keyed_by("dismiss-download", &recording),
                        Icon::Close,
                        DISMISS_DOWNLOAD_HINT,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.dismiss_download(&dismissed, cx);
                        });
                    })),
                )
            });

        div()
            .id(listing::keyed_by("download", &recording))
            .debug_selector(move || format!("download-{at}"))
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
        let Some(Deleting { track, title, .. }) = self.deleting.take() else {
            return;
        };
        self.library
            .update(cx, |library, cx| library.delete_track(track, title, cx));
        cx.notify();
    }

    fn deletion_sheet(&self, deleting: &Deleting, cx: &mut Context<Self>) -> Div {
        let card = div()
            .id("delete-sheet")
            .flex()
            .flex_col()
            .gap_4()
            .w(px(theme::confirm_width()))
            .p_5()
            .rounded_xl()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.5),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(48.0),
                spread_radius: px(0.0),
            }])
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.keep_the_track(cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(Icon::Delete, 18.0, theme::failure()))
                    .child(
                        div()
                            .text_size(px(theme::text_lg()))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::text()))
                            .child("Delete this song?"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .child(SharedString::from(deleting.saying()))
                    .when(deleting.a_cut, |body| body.child(A_CUT_GOES_WHOLE))
                    .child(
                        div()
                            .text_color(rgb(theme::text()))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(DELETED_FOR_GOOD),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        kit::button(
                            "keep-the-track",
                            None,
                            "Cancel",
                            KEEP_THE_TRACK_HINT,
                            kit::Tone::Outlined,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.keep_the_track(cx))),
                    )
                    .child(
                        kit::button(
                            "delete-the-track",
                            Some(Icon::Delete),
                            "Delete",
                            DELETE_THE_TRACK_HINT,
                            kit::Tone::Destructive,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.delete_the_track(cx))),
                    ),
            );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme::scrim()))
            .occlude()
            .child(motion::lifted_in(card, "delete-sheet-arrives"))
    }

    fn enrichment_status(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("enriching")
            .flex()
            .items_center()
            .gap_2p5()
            .px_3()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(ENRICHING_HINT)
            .on_click(cx.listener(|this, _, _, cx| {
                this.set_pane(Pane::Settings, cx);
                this.show_settings(Category::Online, cx);
            }))
            .child(icons::icon(Icon::Globe, theme::text_base(), theme::faint()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child("Enriching…"),
            )
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

        div()
            .id(SharedString::new_static(pane.label()))
            .debug_selector(|| format!("tab-{}", pane.as_str()))
            .group(PANE_GROUP)
            .relative()
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(32.0))
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
            .names(pane.about())
            .when_some(count, |row, count| {
                row.child(kit::figure(count.to_string()).text_color(rgb(theme::faint())))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.choose_pane(pane, cx)))
    }

    pub(crate) fn drawn_cover(
        &self,
        pictured: Pictured<'_>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<(Picture, Magnified)> {
        let (album, file) = match pictured {
            Pictured::Album(album) => (Some(album), None),
            Pictured::Track { album, file } => (album, Some(file)),
        };

        if let Some(album) = album
            && let Some(art) = self
                .library
                .update(cx, |library, cx| library.cover(album, drawn, cx))
        {
            return Some((art, Magnified::Album(album)));
        }

        let file = file?;
        let art = self
            .player
            .update(cx, |player, cx| player.art(file, drawn, cx))?;
        Some((art, Magnified::File(file.clone())))
    }

    fn magnified_art(&self, magnified: &Magnified, cx: &mut Context<Self>) -> Option<Picture> {
        match magnified {
            Magnified::Album(album) => self
                .library
                .update(cx, |library, cx| library.whole_cover(*album, cx)),
            Magnified::File(file) => self
                .player
                .update(cx, |player, cx| player.whole_art(file, cx)),
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
            Magnified::File(file) => Some(SharedString::from(
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
            Region::Sidebar => self.sidebar(cx).into_any_element(),
            Region::Pane => {
                self.grid_width.seen_in(window.viewport_size().width);
                let shown = self.pane_shown(cx);
                motion::faded_in(
                    div()
                        .size_full()
                        .grid()
                        .grid_cols(1)
                        .grid_rows(1)
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
        if let Some(shows) = self.search_in_front(cx) {
            return self.search_pane(shows, cx);
        }

        match self.pane {
            Pane::Albums => self.albums(cx),
            Pane::Artists => self.artists(cx),
            Pane::Tracks => self.tracks(cx),
            Pane::Statistics => self.statistics_pane(cx),
            Pane::Queue => self.queue_pane(cx),
            Pane::Playlists => self.playlists_pane(cx),
            Pane::Favourites => self.favourites_pane(cx),
            Pane::Suggestions => self.suggestions_pane(cx),
            Pane::Missing => self.missing_pane(cx),
            Pane::Lyrics => self.lyrics_pane(cx),
            Pane::Inspector => self.inspector(cx),
            Pane::Visualiser => self.visualiser_pane(cx),
            Pane::Analysis => self.analysis_pane(cx),
            Pane::Settings => self.settings(cx),
        }
    }
}

fn cover_rounding(side: f32) -> f32 {
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
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let moved = self.moved_under_a_still_pointer(window, cx);
        hint::asking(self.hints_are_wanted(cx) && !moved);
        self.drawn_at = SystemTime::now();
        self.follow_the_scale(window, cx);
        self.player
            .read(cx)
            .listen_in(self.pane == Pane::Visualiser);
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
                this.seek_by(seek_step(), cx);
            }))
            .on_action(cx.listener(|this, _: &SeekBackward, _, cx| {
                this.seek_by(-seek_step(), cx);
            }))
            .on_action(cx.listener(|this, _: &SeekFurtherForward, _, cx| {
                this.seek_by(seek_further(), cx);
            }))
            .on_action(cx.listener(|this, _: &SeekFurtherBackward, _, cx| {
                this.seek_by(-seek_further(), cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| this.toggle_mute(cx)))
            .on_action(cx.listener(|this, _: &ToggleQueue, _, cx| this.toggle_queue(cx)))
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
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| this.volume_by(0.05, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| this.volume_by(-0.05, cx)))
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
            .on_action(|_: &ReachNext, window: &mut Window, _: &mut App| window.focus_next())
            .on_action(|_: &ReachPrevious, window: &mut Window, _: &mut App| window.focus_prev())
            .on_action(cx.listener(|this, _: &NextPane, _, cx| this.step_pane(Step::Below, cx)))
            .on_action(cx.listener(|this, _: &PreviousPane, _, cx| {
                this.step_pane(Step::Above, cx);
            }))
            .on_action(cx.listener(|this, _: &LeaveControl, window, cx| {
                this.let_the_control_go(window, cx);
            }))
            .on_action(|_: &Quit, window: &mut Window, _: &mut App| window.remove_window())
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
            .when_some(
                toast::drawn(self.type_ahead.typed().is_some(), cx),
                ParentElement::child,
            )
            .when_some(self.type_ahead_pill(), ParentElement::child)
            .when_some(self.downloads_over_the_app(cx), ParentElement::child)
            .when_some(self.menu_over_the_app(cx), ParentElement::child)
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
    pub(crate) track: TrackId,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) a_cut: bool,
}

impl Deleting {
    pub(crate) fn of(track: &Track) -> Self {
        Self {
            track: track.id,
            title: track.title.clone(),
            artist: track.artist.clone().filter(|artist| !artist.is_empty()),
            a_cut: track.span.is_some(),
        }
    }

    fn saying(&self) -> String {
        match &self.artist {
            Some(artist) => format!(
                "“{}” by {artist} is deleted from disk and taken out of the library, its plays with it.",
                self.title
            ),
            None => format!(
                "“{}” is deleted from disk and taken out of the library, its plays with it.",
                self.title
            ),
        }
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
    let tracks = format::counted(rows, "track", "tracks");

    match (play, at) {
        (true, _) => format!("{tracks} queued and playing"),
        (false, Placement::Next) => format!("{tracks} to play next"),
        (false, Placement::Queued | Placement::At(_)) => format!("{tracks} added to the queue"),
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

fn stepped_pane(from: Pane, step: Step, tabs: Tabs) -> Pane {
    let listed: Vec<Pane> = Pane::BROWSE
        .into_iter()
        .filter(|pane| pane.is_shown(tabs))
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
        .when(selected, |row| row.bg(theme::tinted(theme::accent(), 0x14)))
}

pub(crate) fn tall_row(selected: bool) -> Div {
    row(selected).h(px(theme::tall_row_height()))
}

pub(crate) fn empty(icon: Icon, message: &'static str, more: Option<&'static str>) -> AnyElement {
    kit::empty(icon, message, more)
}

const CHOSEN_MARK: f32 = 14.0;

const CHOSEN_MARK_FROM_THE_TOP: f32 = 9.0;

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

            ground.opacity(at).child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(CHOSEN_MARK_FROM_THE_TOP + CHOSEN_MARK * (1.0 - at) / 2.0))
                    .w(px(2.0))
                    .h(px(CHOSEN_MARK * at))
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
        in_front_of, landing, stepped_pane,
    };
    use crate::{Selection, Tabs};

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

        assert_eq!(stepped_pane(first, Step::Below, EVERY_TAB), listed[1]);
        assert_eq!(
            stepped_pane(last, Step::Below, EVERY_TAB),
            first,
            "the end did not wrap"
        );
        assert_eq!(
            stepped_pane(first, Step::Above, EVERY_TAB),
            last,
            "the start did not wrap"
        );

        let walked = listed
            .iter()
            .fold(first, |pane, _| stepped_pane(pane, Step::Below, EVERY_TAB));
        assert_eq!(
            walked, first,
            "walking the whole sidebar did not come back to where it started"
        );
    }

    #[test]
    fn a_pane_the_sidebar_does_not_list_steps_onto_the_first_one_it_does() {
        assert_eq!(
            stepped_pane(Pane::Queue, Step::Below, EVERY_TAB),
            Pane::BROWSE[1]
        );
        assert_eq!(
            stepped_pane(Pane::Settings, Step::Below, EVERY_TAB),
            Pane::BROWSE[1]
        );
    }

    #[test]
    fn the_pane_keys_step_over_a_tab_that_is_hidden() {
        let hidden = Tabs {
            suggestions: false,
            missing: false,
            counts: true,
        };

        assert_eq!(
            stepped_pane(Pane::Favourites, Step::Below, hidden),
            Pane::Lyrics
        );
        assert_eq!(
            stepped_pane(Pane::Lyrics, Step::Above, hidden),
            Pane::Favourites
        );
        assert_eq!(
            stepped_pane(Pane::Favourites, Step::Below, EVERY_TAB),
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

        use gpui::TestAppContext;
        use resonate_core::{Span, Volume};
        use resonate_library::{Cut, Direction, Library, PlaylistOrder};

        use crate::{
            Notice,
            driven::{Driven, Folder},
            toast,
            views::{
                menu::Menu,
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
            driven.read(|root, _| root.reach)
        }

        impl Driven {
            fn focus_the_window(&mut self) {
                let root = self.root.clone();
                self.cx
                    .update(|window, cx| window.focus(&root.read(cx).focus));
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
            let queue = driven.queue();

            driven
                .cx
                .simulate_keystrokes("delete enter backspace alt-up down ctrl-z");
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
                    window.focus(&root.focus);
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
                    window.focus(&root.focus);
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
                    window.focus(&root.focus);
                    cx.notify();
                });
            });
            driven.settle();
            nothing_behind_the_sheet_answers(&mut driven, 3);
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
                        track,
                        title: "one".to_owned(),
                        artist: None,
                        a_cut: false,
                    });
                    window.focus(&root.focus);
                    cx.notify();
                });
            });
            driven.settle();
            nothing_behind_the_sheet_answers(&mut driven, 3);

            driven.cx.simulate_keystrokes("escape");
            assert!(driven.read(|root, _| root.deleting.is_none()));
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

        #[gpui::test]
        fn a_key_opens_the_queue_and_the_same_key_goes_back_to_the_pane_it_covered(
            cx: &mut TestAppContext,
        ) {
            let mut driven = Driven::open(cx, catalog());
            driven.focus_the_window();
            let behind = driven.read(|root, _| root.pane);

            driven.cx.simulate_keystrokes("ctrl-u");
            assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);

            driven.cx.simulate_keystrokes("ctrl-u");
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
