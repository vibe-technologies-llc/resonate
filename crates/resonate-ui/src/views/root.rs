use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use gpui::{
    AnyElement, App, BoxShadow, Canvas, Context, Div, ElementId, Entity, FocusHandle, Focusable,
    Image, KeyDownEvent, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, ObjectFit,
    Pixels, Point, Render, ScrollHandle, ScrollStrategy, SharedString, Stateful, Task,
    UniformListScrollHandle, Window, canvas, div, hsla, img, point, prelude::*, px, rgb, rgba,
};
use resonate_core::{AlbumId, MediaLocation, PlaylistId, Span, Volume};
use resonate_engine::{
    Command, Counting, Keeping, Listening, Placement, QueueItem, RepeatMode, stamp_of,
};
use resonate_library::{
    Cut, Direction, EnrichStats, Kept, Playing, Playlist, PlaylistEntry, RowOrder, SavedQuery,
    SortOrder, Track,
};

use crate::{
    Consulted, Drawn, EqualiserModel, LibraryModel, LyricsModel, Notice, PlayerModel, ResonateApp,
    Selection, Setting, Settings, Tabs,
    analysis::AnalysisModel,
    app::{
        CycleRepeat, DropReached, FocusFilter, FocusSearch, GoToTheResults, LeaveControl,
        LeaveSearch, Listen, LowerRow, Next, NextPane, Pause, PlayReached, Previous, PreviousPane,
        Quit, RaiseRow, ReachAbove, ReachBelow, ReachEverything, ReachFirst, ReachLast, ReachNext,
        ReachPageAbove, ReachPageBelow, ReachPrevious, RedoEdit, SeekBackward, SeekForward, Stop,
        TabOnward, TogglePlayPause, ToggleShuffle, UndoEdit, VolumeDown, VolumeUp, WINDOW_CONTEXT,
        WidenAbove, WidenBelow, attend, seek_step,
    },
    format,
    icons::{self, Icon},
    listening::ListenModel,
    theme,
    toast::{self, Toaster},
    views::{
        browser::{ArtistShows, ArtistsDrawn},
        chrome,
        field::{Field, Submitted},
        focus::Controls,
        hint::{self, Names},
        kit::{self, EndsInAnEllipsis},
        listing::Pictured,
        menu::{self, Menu},
        missing::MissingShows,
        playlists::{self, Held, Naming, Rows},
        pointed::{self, LitUnderThePointer},
        queue::{QueueLength, QueueNames, TakenBack, took_out},
        reorder::{Creeping, Listed, Reach, Shift, Step},
        settings::{Category, FILTER_PLACEHOLDER, HeldBand, Plotted},
        slider::{Grab, Rail},
        transport::Resolved,
        typing::{self, TypeAhead, jumped},
        visualiser::Visualiser,
    },
};

const VOLUME_SETTLE: Duration = Duration::from_millis(400);

const WORDMARK_CAPITALS_CENTRED_BY: f32 = 1.0;

pub(crate) const SETTING_UNSAVED: &str =
    "Couldn't save that setting — the settings file couldn't be written";

const LISTEN_BUTTON_HINT: &str = "Listen for a song on the desktop or a microphone — ctrl-l";

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

const CLEAR_SEARCH_HINT: &str = "Clear the search — escape";

const ENRICHING_HINT: &str =
    "The reference is being asked about the library; opens the Online settings";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Magnified {
    Album(AlbumId),
    File(MediaLocation),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    at: usize,
    named: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pointer {
    InTheWindow,
    Gone,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Following {
    shown: Option<usize>,
}

impl Following {
    fn follows(&mut self, playing: Option<usize>, queue_is_shown: bool) -> Option<usize> {
        let Some(row) = playing else {
            self.shown = None;
            return None;
        };
        if !queue_is_shown || self.shown == Some(row) {
            return None;
        }

        self.shown = Some(row);
        Some(row)
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
    pub(crate) settings: Arc<dyn Settings>,
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
    pub(crate) name: Entity<Field>,
    pub(crate) contact: Entity<Field>,
    pub(crate) acoustid: Entity<Field>,
    pub(crate) audd: Entity<Field>,
    pub(crate) listenbrainz: Entity<Field>,
    pub(crate) discord_app: Entity<Field>,
    pub(crate) discord_icon: Entity<Field>,
    pub(crate) organising: Entity<Field>,
    pub(crate) finding: Entity<Field>,
    pub(crate) figure: Entity<Field>,
    pub(crate) looking: Entity<Field>,
    pub(crate) equaliser: Entity<EqualiserModel>,
    pub(crate) controls: Controls,
    pub(crate) took_out: TakenBack,
    pub(crate) queue_length: Option<QueueLength>,
    pub(crate) resetting_everything: bool,
    pub(crate) reset_everything_landed: bool,
    pub(crate) moving_the_files: bool,
    pub(crate) writing_the_tags: bool,
    pub(crate) keeping_the_tracks: bool,
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
    pub(crate) artist_shows: ArtistShows,
    pub(crate) artists_drawn: ArtistsDrawn,
    pub(crate) missing_shows: MissingShows,
    landing_on: Option<usize>,
    pub(crate) menu: Option<Menu>,
    left_at: AHashMap<PlaylistId, UniformListScrollHandle>,
    reach: Option<Reach>,
    pub(crate) creeping: Option<Creeping>,
    pub(crate) creeping_on: Task<()>,
    listing_whole: Task<()>,
    pub(crate) grid_width: Rc<Cell<Pixels>>,
    pub(crate) hero_width: Rc<Cell<Pixels>>,
    pub(crate) playing_room: Rc<Cell<Pixels>>,
    listening: Listening,
    resuming: Keeping,
    following: Following,
    magnified: Option<Magnified>,
    pointer_inside: bool,
    volume_settled: Task<()>,
    volume_aimed: Option<f32>,
    pub(crate) muted_from: Option<Volume>,
    type_ahead: TypeAhead,
    typing_stops: Task<()>,
    pub(crate) queue_names: QueueNames,
    drawn_at: SystemTime,
    pub(crate) resolved: RefCell<Option<Resolved>>,
    pub(crate) focus: FocusHandle,
    search: Entity<Field>,
}

impl RootView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let global = cx.global::<ResonateApp>();
        let player = Arc::clone(&global.player);
        let library = Arc::clone(&global.library);
        let settings = Arc::clone(&global.settings);
        let lyricists = Arc::clone(&global.lyricists);
        let attention = global.attention.clone();
        let online = global.online.clone();
        let presence = global.presence.clone();
        let reference = global.reference.clone();
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
                Consulted {
                    reference,
                    fingerprinters: Arc::clone(&fingerprinters),
                },
                &online,
                resume,
                sourcing,
                cx,
            )
        });
        let lyrics = cx.new(|_| LyricsModel::new(lyricists));
        let visualiser = cx.new(|cx| Visualiser::new(player.clone(), cx));
        let analysis = cx.new(|_| AnalysisModel::new(engine, catalog, Arc::clone(&fingerprinters)));
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
            cx.notify();
        })
        .detach();
        cx.observe(&library, |this, library, cx| {
            this.forget_what_has_gone(&library, cx);
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

        let search = cx.new(|cx| Field::new(SEARCH_PLACEHOLDER, window, cx));
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

        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
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
            field.hold(online.contact, cx);
            field
        });
        cx.subscribe_in(&contact, window, |this, _, _: &Submitted, window, cx| {
            this.contact_given(window, cx);
        })
        .detach();

        let acoustid = cx.new(|cx| {
            let mut field = Field::new(ACOUSTID_PLACEHOLDER, window, cx);
            field.hold(online.acoustid_key.clone(), cx);
            field
        });
        cx.subscribe_in(&acoustid, window, |this, _, _: &Submitted, window, cx| {
            this.acoustid_key_given(window, cx);
        })
        .detach();

        let audd = cx.new(|cx| {
            let mut field = Field::new(AUDD_PLACEHOLDER, window, cx);
            field.hold(online.audd_token.clone(), cx);
            field
        });
        cx.subscribe_in(&audd, window, |this, _, _: &Submitted, window, cx| {
            this.audd_token_given(window, cx);
        })
        .detach();

        let listenbrainz = cx.new(|cx| {
            let mut field = Field::new(LISTENBRAINZ_PLACEHOLDER, window, cx);
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

        let organising = cx.new(|cx| {
            let mut field = Field::new(ORGANISING_PLACEHOLDER, window, cx);
            field.hold(organise_as, cx);
            field
        });
        cx.subscribe_in(&organising, window, |this, _, _: &Submitted, window, cx| {
            this.layout_given(window, cx);
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

        let equaliser = cx.new(|_| {
            EqualiserModel::new(places.equaliser.clone(), Arc::clone(&corrections), bindings)
        });
        cx.observe(&equaliser, |_, equaliser, cx| {
            if let Some(notice) = equaliser.update(cx, |model, _| model.take_notice()) {
                toast::tell(notice, cx);
            }
        })
        .detach();

        Self {
            player,
            library,
            lyrics,
            visualiser,
            analysis,
            listen,
            listening_open: false,
            settings,
            pane: Pane::default(),
            settings_category: Category::default(),
            settings_scroll: ScrollHandle::new(),
            settings_rail_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            statistics_scroll: ScrollHandle::new(),
            analysis_scroll: ScrollHandle::new(),
            suggestions_scroll: ScrollHandle::new(),
            artist_records_scroll: ScrollHandle::new(),
            behind_queue: Pane::default(),
            seek_rail: Rail::default(),
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
            name,
            contact,
            acoustid,
            audd,
            listenbrainz,
            discord_app,
            discord_icon,
            organising,
            finding,
            figure,
            looking,
            equaliser,
            controls: Controls::default(),
            took_out: TakenBack::default(),
            queue_length: None,
            resetting_everything: false,
            reset_everything_landed: false,
            moving_the_files: false,
            writing_the_tags: false,
            keeping_the_tracks: false,
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
            artist_shows: ArtistShows::default(),
            artists_drawn: ArtistsDrawn::default(),
            missing_shows: MissingShows::default(),
            landing_on: None,
            menu: None,
            left_at: AHashMap::new(),
            reach: None,
            creeping: None,
            creeping_on: Task::ready(()),
            listing_whole: Task::ready(()),
            grid_width: Rc::new(Cell::new(px(0.0))),
            hero_width: Rc::new(Cell::new(px(0.0))),
            playing_room: Rc::new(Cell::new(px(0.0))),
            magnified: None,
            pointer_inside: true,
            volume_settled: Task::ready(()),
            volume_aimed: None,
            muted_from: None,
            type_ahead: TypeAhead::default(),
            typing_stops: Task::ready(()),
            queue_names: QueueNames::default(),
            drawn_at: SystemTime::now(),
            resolved: RefCell::new(None),
            focus,
            search,
        }
    }

    pub(crate) const fn drawn_at(&self) -> SystemTime {
        self.drawn_at
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
        let playing = player.read(cx).state().queue_position;
        let Some(row) = self.following.follows(playing, self.pane == Pane::Queue) else {
            return;
        };

        self.show_row(Shift::Queue, row, cx);
    }

    fn noticed(cx: &App) -> bool {
        toast::is_showing(cx)
    }

    pub(crate) fn send(&self, command: Command, cx: &mut Context<Self>) {
        self.player.read(cx).send(command);
    }

    pub(crate) fn report(&self, notice: Notice, cx: &mut Context<Self>) {
        toast::tell(notice, cx);
    }

    pub(crate) fn store(&self, setting: &Setting, cx: &mut Context<Self>) {
        let Err(error) = self.settings.store(setting) else {
            return;
        };
        tracing::error!(%error, ?setting, "a setting could not be saved");

        toast::tell(Notice::Trouble(SETTING_UNSAVED.to_owned()), cx);
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
        self.pane = Pane::Tracks;
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
        self.library.update(cx, |library, cx| library.undo(cx));
    }

    pub(crate) fn redo_edit(&mut self, cx: &mut Context<Self>) {
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
                let text = self.library.read(cx).query().trim().to_owned();
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
                    self.pane = Pane::Playlists;
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
            "No contact is sent from the next start"
        } else {
            "The contact is sent from the next start"
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
        window.focus(&self.focus);
        cx.notify();
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
        window.focus(&self.focus);
        cx.notify();
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
        self.store(&Setting::ListenbrainzToken(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn leave_listenbrainz_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.listenbrainz.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
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
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn leave_organising(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.organising.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn clear_contact(&self, cx: &mut Context<Self>) {
        self.contact
            .update(cx, |contact, cx| contact.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.contact = String::new());
    }

    pub(crate) fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::Settings;
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
                self.show_everything(cx);
                self.set_pane(pane, cx);
            }
            Landing::Scoped => self.set_pane(Pane::Tracks, cx),
            Landing::EveryPlaylist => {
                self.show_playlist(None, cx);
                self.set_pane(Pane::Playlists, cx);
            }
        }
    }

    pub(crate) fn set_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let pane = if pane.is_shown(cx.global::<ResonateApp>().tabs) {
            pane
        } else {
            Pane::default()
        };
        pointed::forget();
        self.stop_typing(cx);
        if self.pane != pane {
            self.ordering = false;
        }
        self.pane = pane;
        self.landing_on = None;
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
            Shift::Queue => self.send(Command::Move { rows, to }, cx),
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
        self.stop_typing(cx);
        if self.step_the_menu(
            match step {
                Step::Above => -1,
                Step::Below => 1,
            },
            cx,
        ) {
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
        self.stop_typing(cx);
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
        self.stop_typing(cx);
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
        self.stop_typing(cx);
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
        };
        let shown = listing
            .0
            .borrow()
            .last_item_size
            .map_or(0.0, |size| f32::from(size.item.height));

        let in_a_grid = shift == Shift::Listing(Listed::Albums)
            || (shift == Shift::Listing(Listed::Artists)
                && self.artists_drawn == ArtistsDrawn::Grid);
        if in_a_grid {
            return ((shown / theme::grid_row()) as usize).max(1) * self.grid_columns();
        }
        ((shown / theme::row_height()) as usize).max(1)
    }

    fn widen_reach(&mut self, step: Step, cx: &mut Context<Self>) {
        self.stop_typing(cx);
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
        self.show_row(shift, to, cx);
        cx.notify();
    }

    fn play_reached_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.press_the_menu(window, cx) {
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
            Shift::Queue => self.send(Command::JumpTo(row), cx),
            Shift::Playlist(playlist) => {
                let entries = self.library.read(cx).entries().to_vec();
                self.play_playlist(playlist, &entries, row, true, cx);
            }
            Shift::Listing(Listed::Tracks) => {
                let Some((played, start)) = self.library.read(cx).played_from(row) else {
                    return;
                };
                self.play(&played, start, cx);
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
        }
    }

    pub(crate) fn drop_rows(&mut self, shift: Shift, rows: Span, cx: &mut Context<Self>) {
        match shift {
            Shift::Queue => {
                let queued = self.player.read(cx).queue();
                let kept = self.took_out.keeping(&queued, rows);
                self.send(Command::Remove(rows), cx);
                if let Some(kept) = kept {
                    toast::tell(took_out(kept), cx);
                }
            }
            Shift::Playlist(playlist) => self.library.update(cx, |library, cx| {
                library.remove_from_playlist(playlist, rows, cx);
            }),
            Shift::Listing(_) => return,
        }

        self.reach = Some(Reach::at(shift, rows.first()));
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
        match self.pane {
            Pane::Queue => {
                let rows = self.player.read(cx).queue().len();
                (rows > 0).then_some((Shift::Queue, rows))
            }
            Pane::Playlists => {
                let library = self.library.read(cx);
                let opened = library.opened()?;
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
            Pane::Statistics
            | Pane::Favourites
            | Pane::Suggestions
            | Pane::Missing
            | Pane::Lyrics
            | Pane::Inspector
            | Pane::Visualiser
            | Pane::Analysis
            | Pane::Settings => None,
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

    fn hints_are_wanted(&self, cx: &App) -> bool {
        self.pointer_inside
            && self.grabbed.is_none()
            && self.held_band.is_none()
            && self.adding.is_none()
            && self.magnified.is_none()
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
        let id = id.into();

        div()
            .id(id.clone())
            .min_w(px(0.0))
            .truncate()
            .child(label)
            .when_some(selection, |named, selection| {
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
                        this.opened(selection, cx);
                    }))
            })
    }

    pub(crate) fn opened(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let leaving = Wayback {
            pane: self.pane,
            selection: self.library.read(cx).selection(),
            at: self.top_row(),
            named: self.here(cx),
        };
        if leaving.selection == selection && leaving.pane == Pane::Tracks {
            return;
        }
        if self.came_from.len() == WAYS_BACK {
            self.came_from.remove(0);
        }
        self.came_from.push(leaving);

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

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        let Some(back) = self.came_from.pop() else {
            return;
        };
        self.library
            .update(cx, |library, cx| library.select(back.selection, cx));
        self.set_pane(back.pane, cx);
        self.landing_on = back.pane.lands_where_it_was_left().then_some(back.at);
    }

    pub(crate) fn land_where_it_was_left(&mut self, held: usize) {
        let Some(at) = self.landing_on else {
            return;
        };
        if at >= held {
            if held > 0 {
                self.landing_on = None;
            }
            return;
        }

        self.scroll_of(self.pane)
            .scroll_to_item(at, ScrollStrategy::Top);
        self.landing_on = None;
    }

    fn top_row(&self) -> usize {
        self.scroll_of(self.pane).0.borrow().base_handle.top_item()
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
                overlay.child(
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
                )
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
            "backspace" => {
                if !self.drop_typed(cx) && !self.drop_reached(cx) {
                    self.search.update(cx, |search, cx| search.drop_last(cx));
                }
            }
            "escape" if self.listening_open => self.close_the_listener(cx),
            "escape" if self.magnified.is_some() => self.shrink_cover(cx),
            "escape" if Self::noticed(cx) => toast::dismiss(cx),
            "escape" => {
                if !self.stop_typing(cx) {
                    self.dismiss_search(window, cx);
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
                if !self.typed_ahead(typed, cx) {
                    self.search
                        .update(cx, |search, cx| search.append(typed, cx));
                }
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

    fn jumped_to(&mut self, shift: Shift, from: usize, names: &[String], cx: &mut Context<Self>) {
        let Some(typed) = self.type_ahead.typed() else {
            return;
        };
        let rows: Vec<&str> = names.iter().map(String::as_str).collect();
        let landing = jumped(&rows, from, typed);

        self.type_ahead.landed(landing.is_some());
        let Some(row) = landing else {
            return;
        };
        self.reach_at(shift, row, false, cx);
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
                .child(
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
                ),
        )
    }

    fn editing(&self, window: &Window, cx: &App) -> bool {
        self.search.read(cx).is_focused(window)
            || self.name.read(cx).is_focused(window)
            || self.contact.read(cx).is_focused(window)
            || self.acoustid.read(cx).is_focused(window)
            || self.audd.read(cx).is_focused(window)
            || self.discord_app.read(cx).is_focused(window)
            || self.discord_icon.read(cx).is_focused(window)
            || self.organising.read(cx).is_focused(window)
            || self.finding.read(cx).is_focused(window)
            || self.figure.read(cx).is_focused(window)
            || self.looking.read(cx).is_focused(window)
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
            self.leave_contact(window, cx);
            return;
        }
        if self.acoustid.read(cx).is_focused(window) {
            self.leave_acoustid_key(window, cx);
            return;
        }
        if self.audd.read(cx).is_focused(window) {
            self.leave_audd_token(window, cx);
            return;
        }
        if self.listenbrainz.read(cx).is_focused(window) {
            self.leave_listenbrainz_token(window, cx);
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
        if self.organising.read(cx).is_focused(window) {
            self.leave_organising(window, cx);
            return;
        }
        if self.figure.read(cx).is_focused(window) {
            self.leave_figure(window, cx);
            return;
        }
        if self.naming.is_some() || self.adding.is_some() {
            self.stop_naming(window, cx);
            return;
        }

        self.reach = None;
        window.focus(&self.focus);
        if self.search.read(cx).text().is_empty() {
            let scoped = self.library.read(cx).selection() != Selection::Everything;
            if self.goes_back() || scoped {
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

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if self.library.read(cx).narrowing().unwrap_or_default() == query {
            return;
        }
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

    pub(crate) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(muted_from) = self.muted_at(cx) {
            self.set_volume(muted_from.get(), cx);
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
                    .justify_center()
                    .min_w(px(0.0))
                    .child(self.search_field(window, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .w(px(theme::sidebar_width() - 32.0))
                    .justify_between()
                    .child(
                        kit::icon_button("listen", Icon::Listen, LISTEN_BUTTON_HINT)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| this.open_the_listener(cx))),
                    )
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
        let enriching = library
            .is_enriching()
            .then(|| library.enrich_stats())
            .flatten();

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
                    .when_some(enriching, |column, stats| {
                        column.child(self.enrichment_status(stats, cx))
                    })
                    .child(self.pane_row(Pane::Settings, None, cx)),
            )
    }

    fn enrichment_status(&self, stats: EnrichStats, cx: &mut Context<Self>) -> Stateful<Div> {
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
                this.pane = Pane::Settings;
                this.show_settings(Category::Online, cx);
            }))
            .child(icons::icon(Icon::Globe, theme::text_base(), theme::faint()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::muted()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child("Enriching…"),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::faint()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(format!(
                                "albums {} · tracks {} · artists {}",
                                stats.albums, stats.tracks, stats.artists
                            )),
                    ),
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
            .when(chosen, |row| {
                row.bg(rgb(theme::raised()))
                    .font_weight(gpui::FontWeight::MEDIUM)
            })
            .hover(|row| row.bg(rgb(theme::hover())))
            .text_color(rgb(colour))
            .when(chosen, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(9.0))
                        .w(px(2.0))
                        .h(px(14.0))
                        .rounded_full()
                        .bg(rgb(theme::accent())),
                )
            })
            .child(icons::lit_on_hover(
                icons::icon(pane.icon(), theme::pane_icon(), mark),
                PANE_GROUP,
            ))
            .child(div().flex_1().truncate().child(pane.label()))
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
    ) -> Option<(Arc<Image>, Magnified)> {
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

    fn magnified_art(&self, magnified: &Magnified, cx: &mut Context<Self>) -> Option<Arc<Image>> {
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

    fn content(&mut self, cx: &mut Context<Self>) -> AnyElement {
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

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        hint::asking(self.hints_are_wanted(cx));
        self.drawn_at = SystemTime::now();
        self.player
            .read(cx)
            .listen_in(self.pane == Pane::Visualiser);
        let grain = self.grain(window, cx);
        self.player.update(cx, |player, _| player.draw_at(grain));

        let header = self.header(window, cx);
        let sidebar = self.sidebar(cx);
        let content = self.content(cx);
        let transport = self.transport(chrome::rounded_within_the_frame(window), cx);

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
            .on_action(cx.listener(|this, _: &TogglePlayPause, _, cx| {
                this.send(Command::TogglePlayPause, cx);
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
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(sidebar)
                    .child(div().flex().flex_1().min_w(px(0.0)).child(content)),
            )
            .child(transport)
            .child(self.drag_surface(cx))
            .child(self.pointer_watch(cx))
            .when_some(
                toast::drawn(self.type_ahead.typed().is_some(), cx),
                ParentElement::child,
            )
            .when_some(self.type_ahead_pill(), ParentElement::child)
            .when_some(self.menu_over_the_app(cx), ParentElement::child)
            .when_some(self.adding.clone(), |app, holding| {
                app.child(self.playlist_picker(&holding, cx))
            })
            .when_some(self.magnified.clone(), |app, magnified| {
                app.child(self.magnifier(&magnified, window, cx))
            })
            .when(self.listening_open, |app| app.child(self.listen_sheet(cx)));

        chrome::frame(window, app)
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
        .gap_3()
        .px_6()
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

#[cfg(test)]
mod tests {
    use resonate_core::{AlbumId, ArtistId};

    use super::{Following, Landing, Pane, Step, in_front_of, landing, stepped_pane};
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

    #[test]
    fn the_queue_is_scrolled_to_the_row_that_started_playing() {
        let mut following = Following::default();

        assert_eq!(following.follows(Some(7), ON_SCREEN), Some(7));
        assert_eq!(following.follows(Some(8), ON_SCREEN), Some(8));
    }

    #[test]
    fn a_redraw_that_leaves_the_playing_row_where_it_was_scrolls_nothing() {
        let mut following = Following::default();

        assert_eq!(following.follows(Some(7), ON_SCREEN), Some(7));
        assert_eq!(following.follows(Some(7), ON_SCREEN), None);
        assert_eq!(following.follows(Some(7), ON_SCREEN), None);
    }

    #[test]
    fn a_queue_nobody_is_looking_at_is_not_scrolled_until_it_is_drawn_again() {
        let mut following = Following::default();

        assert_eq!(following.follows(Some(7), BEHIND_ANOTHER_PANE), None);
        assert_eq!(following.follows(Some(7), ON_SCREEN), Some(7));
    }

    #[test]
    fn a_queue_that_has_stopped_is_followed_nowhere_and_taken_up_afresh() {
        let mut following = Following::default();

        assert_eq!(following.follows(Some(7), ON_SCREEN), Some(7));
        assert_eq!(following.follows(None, ON_SCREEN), None);
        assert_eq!(following.follows(Some(7), ON_SCREEN), Some(7));
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
}
