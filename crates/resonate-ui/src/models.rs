use std::{
    cell::OnceCell,
    fs, mem,
    num::{NonZeroU32, NonZeroUsize},
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    slice,
    sync::Arc,
    thread,
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use crossbeam_channel::{Receiver, bounded};
use gpui::{App, Context, Image, Task};
use resonate_core::{
    AlbumId, ArtistId, FrameSpan, ListenId, MediaLocation, PlaylistId, QueueStamp, ReleaseTrackId,
    Span, TrackId, WantId,
};
use resonate_engine::{Keep, Played, QueueItem};
use resonate_library::{
    Album, AlbumOrder, AlbumQuery, Artist, ArtistDetail, ArtistOrder, ArtistQuery, ArtistTotals,
    CatalogStamp, CoverArt, Cut, Day, Direction, Drawing, Edit, EnrichOptions, EnrichProgress,
    EnrichStats, EnrichSummary, Favoured, FileTags, Fingerprinters, Found, HeldReleaseTrack,
    ImageFormat, ImportOptions, ImportProgress, ImportStats, ImportSummary, Imported, Kept, Layout,
    Library, LookupOp, Mbid, Measured, Missing, MissingTrack, MostListened, OrganiseOptions,
    OrganiseProgress, OrganiseStats, OrganiseSummary, Playing, Playlist, PlaylistEntry,
    PlaylistOrder, PollOptions, PollProgress, PollStats, PollSummary, Reference, ReleaseDetail,
    RetagOptions, RetagProgress, RetagStats, RetagSummary, RootsWatch, RowOrder, SavedQuery,
    ScanHandle, ScanOptions, ScanProgress, ScanStats, ScanSummary, Search, Shared, SortOrder,
    Sought, Sources, Statistics, Suggestion, Sung, Track, TrackQuery, Undoable, UnheldRelease,
    Window, asks_elsewhere,
};
use resonate_providers::Providers;

use crate::{
    ResonateApp, clipboard,
    drawing::Drawer,
    format,
    recent::{Leaving, Recent},
    settings::{Online, Sourcing},
    theme, toast,
    views::statistics::{BARS_AT_MOST, Chart},
};

const PAGE: usize = 2_000;
const FIRST_READ_THREAD: &str = "resonate-first-read";

const ARTIST_ALBUMS: usize = 500;
const LOOK_AHEAD: usize = 200;
const _: () = assert!(
    LOOK_AHEAD < PAGE,
    "a look-ahead past the page grows the window unasked"
);
const MISSING_AT_MOST: usize = 5_000;
const UNHELD_MATCHED_AT_MOST: usize = 200;
const PREVIEWED_AT_MOST: usize = 500;
const FAVOURITES_AT_MOST: usize = 5_000;
const MOST_LISTENED: usize = 10;
const COVERS_HELD: NonZeroUsize = held(512);
const COVERS_WARMED: usize = 256;
const NAMES_HELD: NonZeroUsize = held(4_096);
const ALBUMS_HELD: NonZeroUsize = held(256);
const PORTRAITS_HELD: NonZeroUsize = held(256);
const DECODES_AT_ONCE: usize = 4;

const SCAN_POLL: Duration = Duration::from_millis(100);
const SEARCH_SETTLE: Duration = Duration::from_millis(150);
const ASKED_ELSEWHERE_AFTER: Duration = Duration::from_millis(700);
const POLLS_PER_RELOAD: u32 = 20;
const FIRST_ASKED_AFTER: Duration = Duration::from_secs(60);
const ASKED_EVERY: Duration = Duration::from_secs(30 * 60);
const WATCHED_EVERY: Duration = Duration::from_secs(2);
const ROOTS_LOOKED_AT_EVERY: Duration = Duration::from_millis(250);
const ROOTS_QUIET_FOR: Duration = Duration::from_secs(2);
const INBOX_LOOKED_AT_EVERY: Duration = Duration::from_secs(1);
const INBOX_QUIET_FOR: Duration = Duration::from_secs(2);
const GONE_QUIET_FOR: Duration = Duration::from_millis(250);

const TAKEN_BACK: &str = " · ctrl-z puts it back";

const WANTED_ELSEWHERE: &str = " — the providers will be asked for it";

const NOTHING_TO_SHARE: &str = "That track isn't in the library, so there's no link to share";

const ALREADY_WALKING: &str = "Another library task is still running — try again once it finishes";

pub(crate) const fn side(pixels: u32) -> NonZeroU32 {
    match NonZeroU32::new(pixels) {
        Some(pixels) => pixels,
        None => panic!("a drawn cover has a side"),
    }
}

pub(crate) const fn held(entries: usize) -> NonZeroUsize {
    match NonZeroUsize::new(entries) {
        Some(entries) => entries,
        None => panic!("a cache holds at least one entry"),
    }
}

struct Browsed {
    albums: Vec<Album>,
    artists: Vec<Artist>,
    tracks: Vec<Track>,
    favourite_albums: Vec<Album>,
    favourite_artists: Vec<Artist>,
    favourite_tracks: Vec<Track>,
    statistics: Statistics,
    most_listened: MostListened,
    by_day: Vec<Day>,
    suggestions: Arc<[Suggestion]>,
    instead: Option<String>,
    unheld: Vec<MissingTrack>,
    sung: Option<Sung>,
    scoped: Vec<Track>,
    albums_counted: u32,
    artists_counted: u32,
    tracks_measured: Measured,
    scoped_measured: Measured,
    roots: Vec<PathBuf>,
    release_tracks: Vec<HeldReleaseTrack>,
    release: Option<ReleaseDetail>,
    artist: Option<ArtistDetail>,
    artist_albums: Vec<Album>,
    artist_totals: ArtistTotals,
    album: Option<Album>,
}

struct Loaded {
    browsed: Option<Browsed>,
    playlists: Vec<Playlist>,
    lists: Vec<Playlist>,
    held: Option<Playlist>,
    entries: Vec<PlaylistEntry>,
    wanted: AHashMap<ReleaseTrackId, WantId>,
    missing_tracks: Vec<MissingTrack>,
    unheld_releases: Vec<UnheldRelease>,
    missing: Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListedRow {
    Disc(u32),
    Held(usize),
    Missing(usize),
    Beyond(Beyond),
    Unheld(usize),
    Found(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beyond {
    InTheCatalog(usize),
    Elsewhere(usize),
    Asking,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Previewed {
    pub query: SavedQuery,
    pub tracks: Arc<[Track]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingRow {
    Album(usize),
    Disc(usize),
    Track(usize),
    Artist(usize),
    Release(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Favourited {
    pub artists: usize,
    pub albums: usize,
    pub tracks: usize,
}

impl Favourited {
    pub const fn held(self) -> usize {
        self.artists + self.albums + self.tracks
    }

    pub const fn is_empty(self) -> bool {
        self.held() == 0
    }

    pub fn counted(self) -> String {
        [
            (self.artists, "artist", "artists"),
            (self.albums, "album", "albums"),
            (self.tracks, "track", "tracks"),
        ]
        .into_iter()
        .filter(|(held, _, _)| *held > 0)
        .map(|(held, one, many)| format::counted(held, one, many))
        .collect::<format::Parts<String>>()
        .join(" · ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    Everything,
    ThePlaylists,
}

#[derive(Clone, PartialEq, Eq)]
struct Asked {
    text: Option<String>,
    album: Option<AlbumId>,
    artist: Option<ArtistId>,
    opened: Option<PlaylistId>,
    order: PlaylistOrder,
    reading: Direction,
    sorting: Sorting,
    reach: usize,
    window: Window,
}

impl Asked {
    fn at_first() -> Self {
        Self {
            text: None,
            album: None,
            artist: None,
            opened: None,
            order: PlaylistOrder::default(),
            reading: PlaylistOrder::default().reads(),
            sorting: Sorting::default(),
            reach: PAGE,
            window: Window::default(),
        }
    }
}

pub(crate) struct FirstRead {
    asked: Asked,
    read: Receiver<resonate_library::Result<Loaded>>,
}

impl FirstRead {
    pub(crate) fn start(library: &Arc<Library>) -> Option<Self> {
        let (sent, read) = bounded(1);
        let reading = Arc::clone(library);
        let asked = Asked::at_first();
        let handed = asked.clone();
        thread::Builder::new()
            .name(FIRST_READ_THREAD.to_owned())
            .spawn(move || {
                let _ = sent.send(load(&reading, handed, Wanted::Everything));
            })
            .inspect_err(|error| tracing::debug!(%error, "the catalog could not be read ahead"))
            .ok()?;
        Some(Self { asked, read })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sorting {
    pub tracks: SortOrder,
    pub tracks_read: Direction,
    pub albums: AlbumOrder,
    pub albums_read: Direction,
    pub artists: ArtistOrder,
    pub artists_read: Direction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Trouble(String),
    Done(String),
    Noted(String),
}

impl Notice {
    pub fn text(&self) -> &str {
        match self {
            Self::Trouble(text) | Self::Done(text) | Self::Noted(text) => text,
        }
    }

    pub const fn tone(&self) -> Tone {
        match self {
            Self::Trouble(_) => Tone::Trouble,
            Self::Done(_) => Tone::Done,
            Self::Noted(_) => Tone::Noted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Trouble,
    Done,
    Noted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    Everything,
    Album(AlbumId),
    Artist(ArtistId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Preview,
    Apply,
}

impl Pass {
    pub(crate) const fn applies(self) -> bool {
        matches!(self, Self::Apply)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Planned {
    #[default]
    Not,
    Shown(CatalogStamp),
    Outdated,
}

impl Planned {
    fn after(pass: Pass, cancelled: bool, read_at: CatalogStamp) -> Self {
        match pass {
            Pass::Preview if !cancelled => Self::Shown(read_at),
            Pass::Preview | Pass::Apply => Self::Not,
        }
    }

    pub const fn is_shown(self) -> bool {
        matches!(self, Self::Shown(_))
    }

    fn outdated_at(self, now: CatalogStamp) -> bool {
        match self {
            Self::Shown(read_at) => !read_at.still_holds_at(now),
            Self::Not | Self::Outdated => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prompted {
    ByHand,
    OnItsOwn,
    ByTheInbox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    WhatChanged,
    Everything,
}

#[derive(Clone, Default)]
enum Work {
    #[default]
    Nothing,
    Scanning(Arc<ScanProgress>),
    Organising(Pass, Arc<OrganiseProgress>),
    Tagging(Pass, Arc<RetagProgress>),
    Importing(Pass, Arc<ImportProgress>),
    Polling(Arc<PollProgress>),
    Forgetting,
}

impl Work {
    const fn is_busy(&self) -> bool {
        !matches!(self, Self::Nothing)
    }

    const fn scanning(&self) -> Option<&Arc<ScanProgress>> {
        match self {
            Self::Scanning(progress) => Some(progress),
            Self::Nothing
            | Self::Organising(..)
            | Self::Tagging(..)
            | Self::Importing(..)
            | Self::Polling(_)
            | Self::Forgetting => None,
        }
    }

    const fn organising(&self) -> Option<(Pass, &Arc<OrganiseProgress>)> {
        match self {
            Self::Organising(pass, progress) => Some((*pass, progress)),
            Self::Nothing
            | Self::Scanning(_)
            | Self::Tagging(..)
            | Self::Importing(..)
            | Self::Polling(_)
            | Self::Forgetting => None,
        }
    }

    const fn tagging(&self) -> Option<(Pass, &Arc<RetagProgress>)> {
        match self {
            Self::Tagging(pass, progress) => Some((*pass, progress)),
            Self::Nothing
            | Self::Scanning(_)
            | Self::Organising(..)
            | Self::Importing(..)
            | Self::Polling(_)
            | Self::Forgetting => None,
        }
    }

    const fn polling(&self) -> Option<&Arc<PollProgress>> {
        match self {
            Self::Polling(progress) => Some(progress),
            Self::Nothing
            | Self::Scanning(_)
            | Self::Organising(..)
            | Self::Tagging(..)
            | Self::Importing(..)
            | Self::Forgetting => None,
        }
    }

    const fn importing(&self) -> Option<(Pass, &Arc<ImportProgress>)> {
        match self {
            Self::Importing(pass, progress) => Some((*pass, progress)),
            Self::Nothing
            | Self::Scanning(_)
            | Self::Organising(..)
            | Self::Tagging(..)
            | Self::Polling(_)
            | Self::Forgetting => None,
        }
    }
}

pub struct Consulted {
    pub reference: Option<Arc<dyn Reference>>,
    pub fingerprinters: Arc<Fingerprinters>,
}

pub struct LibraryModel {
    first_read: Option<FirstRead>,
    library: Arc<Library>,
    reference: Option<Arc<dyn Reference>>,
    fingerprinters: Arc<Fingerprinters>,
    online: bool,
    after_scan: bool,
    studies: bool,
    resume: bool,
    albums: Arc<[Album]>,
    artists: Arc<[Artist]>,
    tracks: Arc<[Track]>,
    favourite_albums: Arc<[Album]>,
    favourite_artists: Arc<[Artist]>,
    favourite_tracks: Arc<[Track]>,
    statistics: Statistics,
    most_listened: Arc<MostListened>,
    by_day: Arc<[Day]>,
    suggestions: Arc<[Suggestion]>,
    opened_suggestion: Option<Previewed>,
    window: Window,
    scoped: Arc<[Track]>,
    rows: Arc<[ListedRow]>,
    unheld: Arc<[MissingTrack]>,
    sung: Option<Sung>,
    found: Arc<[Found]>,
    found_for: Option<String>,
    asking: Option<String>,
    wanting: AHashMap<Mbid, Task<()>>,
    release_tracks: Arc<[HeldReleaseTrack]>,
    release: Option<ReleaseDetail>,
    artist: Option<ArtistDetail>,
    artist_albums: Arc<[Album]>,
    artist_totals: ArtistTotals,
    album: Option<Album>,
    wanted: AHashMap<ReleaseTrackId, WantId>,
    missing_tracks: Arc<[MissingTrack]>,
    unheld_releases: Arc<[UnheldRelease]>,
    missing: Missing,
    missing_track_rows: Arc<[MissingRow]>,
    unheld_release_rows: Arc<[MissingRow]>,
    roots: Vec<PathBuf>,
    playlists: Arc<[Playlist]>,
    lists: Arc<[Playlist]>,
    held: Option<Playlist>,
    entries: Arc<[PlaylistEntry]>,
    opened: Option<PlaylistId>,
    order: PlaylistOrder,
    reading: Direction,
    sorting: Sorting,
    selection: Selection,
    query: String,
    search: Search,
    instead: Option<String>,
    work: Work,
    summary: Option<ScanSummary>,
    organised: Option<(Pass, OrganiseSummary)>,
    previewed: Planned,
    imported: Option<(Pass, ImportSummary)>,
    previewed_import: Planned,
    tagged: Option<(Pass, RetagSummary)>,
    previewed_tags: Planned,
    sourcing: Sourcing,
    polled: Option<PollSummary>,
    enriching: Option<Arc<EnrichProgress>>,
    enriched: Option<EnrichSummary>,
    sought: Arc<Sought>,
    reach: usize,
    albums_counted: u32,
    artists_counted: u32,
    tracks_measured: Measured,
    scoped_measured: Measured,
    revision: u64,
    album_at: AHashMap<AlbumId, usize>,
    favourite_album_ids: AHashSet<AlbumId>,
    favourite_artist_ids: AHashSet<ArtistId>,
    favoured: AHashMap<Favoured, bool>,
    charted: Arc<Chart>,
    covers: Recent<AlbumId, Option<Art>>,
    decoding: AHashSet<AlbumId>,
    magnified: Option<Magnifying<AlbumId>>,
    portraits: Recent<ArtistId, Option<Portrait>>,
    decoding_portraits: AHashSet<ArtistId>,
    _warming: Task<()>,
    warmed: bool,
    named: Recent<TrackId, Named>,
    read_albums: Recent<AlbumId, Option<Album>>,
    counted: Option<ListenId>,
    _load: Task<()>,
    reading_everything: bool,
    _scan: Task<()>,
    _organise: Task<()>,
    _import: Task<()>,
    _poll: Task<()>,
    _asking: Task<()>,
    _watching: Task<()>,
    _watching_roots: Task<()>,
    _watching_inbox: Task<()>,
    _retag: Task<()>,
    _enrich: Task<()>,
    _edit: Task<()>,
    _counted: Task<()>,
    _settled: Task<()>,
    _shared: Task<()>,
    _kept: Task<()>,
    _finding: Task<()>,
    _previewing: Task<()>,
}

impl LibraryModel {
    pub fn new(
        library: Arc<Library>,
        consulted: Consulted,
        online: &Online,
        resume: bool,
        sourcing: Sourcing,
        cx: &mut Context<Self>,
    ) -> Self {
        let Consulted {
            reference,
            fingerprinters,
        } = consulted;
        let first_read = cx
            .has_global::<ResonateApp>()
            .then(|| cx.global_mut::<ResonateApp>().first_read.take())
            .flatten();
        let Asked {
            order,
            reading,
            sorting,
            reach,
            window,
            ..
        } = Asked::at_first();
        let mut model = Self {
            first_read,
            library,
            reference,
            fingerprinters,
            online: online.enabled,
            after_scan: online.after_scan,
            studies: online.studies,
            resume,
            albums: Arc::default(),
            artists: Arc::default(),
            tracks: Arc::default(),
            favourite_albums: Arc::default(),
            favourite_artists: Arc::default(),
            favourite_tracks: Arc::default(),
            statistics: Statistics::default(),
            most_listened: Arc::default(),
            by_day: Arc::default(),
            suggestions: Arc::default(),
            opened_suggestion: None,
            window,
            scoped: Arc::default(),
            rows: Arc::default(),
            unheld: Arc::default(),
            sung: None,
            found: Arc::default(),
            found_for: None,
            asking: None,
            wanting: AHashMap::new(),
            release_tracks: Arc::default(),
            release: None,
            artist: None,
            artist_albums: Arc::default(),
            artist_totals: ArtistTotals::default(),
            album: None,
            wanted: AHashMap::new(),
            missing_tracks: Arc::default(),
            unheld_releases: Arc::default(),
            missing: Missing::default(),
            missing_track_rows: Arc::default(),
            unheld_release_rows: Arc::default(),
            roots: Vec::new(),
            playlists: Arc::default(),
            lists: Arc::default(),
            held: None,
            entries: Arc::default(),
            opened: None,
            order,
            reading,
            sorting,
            selection: Selection::Everything,
            query: String::new(),
            search: Search::default(),
            instead: None,
            work: Work::Nothing,
            summary: None,
            organised: None,
            previewed: Planned::Not,
            imported: None,
            previewed_import: Planned::Not,
            tagged: None,
            previewed_tags: Planned::Not,
            sourcing,
            polled: None,
            enriching: None,
            enriched: None,
            sought: Arc::default(),
            reach,
            albums_counted: 0,
            artists_counted: 0,
            tracks_measured: Measured::default(),
            scoped_measured: Measured::default(),
            revision: 0,
            album_at: AHashMap::new(),
            favourite_album_ids: AHashSet::new(),
            favourite_artist_ids: AHashSet::new(),
            favoured: AHashMap::new(),
            charted: Arc::default(),
            covers: Recent::new(COVERS_HELD),
            decoding: AHashSet::new(),
            magnified: None,
            portraits: Recent::new(PORTRAITS_HELD),
            decoding_portraits: AHashSet::new(),
            _warming: Task::ready(()),
            warmed: false,
            named: Recent::new(NAMES_HELD),
            read_albums: Recent::new(ALBUMS_HELD),
            counted: None,
            _load: Task::ready(()),
            reading_everything: false,
            _scan: Task::ready(()),
            _organise: Task::ready(()),
            _import: Task::ready(()),
            _poll: Task::ready(()),
            _asking: Task::ready(()),
            _watching: Task::ready(()),
            _watching_roots: Task::ready(()),
            _watching_inbox: Task::ready(()),
            _retag: Task::ready(()),
            _enrich: Task::ready(()),
            _edit: Task::ready(()),
            _counted: Task::ready(()),
            _settled: Task::ready(()),
            _shared: Task::ready(()),
            _kept: Task::ready(()),
            _finding: Task::ready(()),
            _previewing: Task::ready(()),
        };
        model.reload(cx);
        model.carry_on_enriching(cx);
        model.ask_on_its_own(cx);
        model.watch_for_writes_elsewhere(cx);
        model.watch_the_roots(cx);
        model.watch_the_inbox(cx);
        model
    }

    fn watch_the_inbox(&mut self, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self._watching_inbox = cx.spawn(async move |this, cx| {
            let mut watched: Option<(PathBuf, Option<RootsWatch>)> = None;
            let mut owed = false;
            loop {
                cx.background_executor().timer(INBOX_LOOKED_AT_EVERY).await;
                let Ok(inbox) = this.update(cx, |this, _| this.sourcing.inbox.clone()) else {
                    return;
                };
                if watched.as_ref().map(|(folder, _)| folder) != inbox.as_ref() {
                    owed = false;
                    watched = match inbox {
                        Some(folder) => {
                            let over = folder.clone();
                            let asked = Arc::clone(&library);
                            let (watch, landed) = cx
                                .background_executor()
                                .spawn(async move {
                                    let landed = asked
                                        .last_tried()
                                        .ok()
                                        .flatten()
                                        .is_some_and(|tried| landed_since(&over, tried));
                                    (RootsWatch::over(slice::from_ref(&over)), landed)
                                })
                                .await;
                            owed = landed;
                            Some((folder, watch))
                        }
                        None => None,
                    };
                }

                if let Some(watch) = watched.as_ref().and_then(|(_, watch)| watch.as_ref()) {
                    let _ = watch.taken_away(Duration::ZERO);
                    owed |= !watch.settled(INBOX_QUIET_FOR).is_empty();
                }
                if !owed {
                    continue;
                }
                let Ok(answered) =
                    this.update(cx, |this, cx| this.poll_as(Prompted::ByTheInbox, cx))
                else {
                    return;
                };
                owed = !answered;
            }
        });
    }

    fn watch_the_roots(&mut self, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self._watching_roots = cx.spawn(async move |this, cx| {
            let mut watching = Watching::default();
            let mut owed: Vec<PathBuf> = Vec::new();
            let mut changed_while_closed = true;
            loop {
                cx.background_executor().timer(ROOTS_LOOKED_AT_EVERY).await;
                let Ok(busy) = this.update(cx, |this, _| this.work.is_busy()) else {
                    return;
                };

                let reading = Arc::clone(&library);
                let (followed, settled, forgotten) = cx
                    .background_executor()
                    .spawn(async move {
                        let mut followed = watching.following(&reading);
                        let forgotten = followed.forget_what_went(&reading);
                        let mut settled = followed.came_back();
                        if !busy {
                            settled.extend(followed.settled());
                        }
                        (followed, settled, forgotten)
                    })
                    .await;
                watching = followed;

                if forgotten > 0 && this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    return;
                }

                for root in settled {
                    if !owed.contains(&root) {
                        owed.push(root);
                    }
                }
                if busy {
                    continue;
                }

                if changed_while_closed && watching.has_read_the_roots() {
                    if !watching.finds_a_root_there() {
                        changed_while_closed = false;
                        continue;
                    }
                    let Ok(started) = this.update(cx, |this, cx| {
                        this.start_scan(Vec::new(), Reading::WhatChanged, Prompted::OnItsOwn, cx)
                    }) else {
                        return;
                    };
                    if started {
                        changed_while_closed = false;
                        owed.clear();
                    }
                    continue;
                }
                if owed.is_empty() {
                    continue;
                }

                let asked = owed.clone();
                let Ok(started) = this.update(cx, |this, cx| {
                    this.start_scan(asked, Reading::WhatChanged, Prompted::OnItsOwn, cx)
                }) else {
                    return;
                };
                if started {
                    owed.clear();
                }
            }
        });
    }

    fn watch_for_writes_elsewhere(&mut self, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self._watching = cx.spawn(async move |this, cx| {
            let mut loaded = library.written_elsewhere();
            let mut seen = loaded;
            loop {
                cx.background_executor().timer(WATCHED_EVERY).await;
                let reading = Arc::clone(&library);
                let now = cx
                    .background_executor()
                    .spawn(async move { reading.written_elsewhere() })
                    .await;
                let Some(now) = now else {
                    continue;
                };
                let settled = seen == Some(now) && loaded != Some(now);
                seen = Some(now);
                if !settled {
                    continue;
                }
                loaded = Some(now);
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    return;
                }
            }
        });
    }

    fn ask_on_its_own(&mut self, cx: &mut Context<Self>) {
        self._asking = cx.spawn(async move |this, cx| {
            let mut wait = FIRST_ASKED_AFTER;
            loop {
                cx.background_executor().timer(wait).await;
                wait = ASKED_EVERY;
                if this
                    .update(cx, |this, cx| this.poll_as(Prompted::OnItsOwn, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
    }

    fn restate_the_listing(&mut self) {
        let listing = self.listing();
        self.rows = match self.selection {
            Selection::Album(_) => {
                album_rows(&listing, &self.release_tracks, self.arranging()).into()
            }
            Selection::Artist(_) => Arc::default(),
            Selection::Everything => beyond_the_listing(Reaching {
                held: listing.len(),
                whole: listing.len() as u64 >= u64::from(self.tracks_measured.rows),
                unheld: self.unheld.len(),
                found: self.found_here().len(),
                asking: self.is_asking_elsewhere(),
            })
            .into(),
        };
    }

    fn found_here(&self) -> &[Found] {
        match self.found_for.as_deref() {
            Some(asked) if !self.query.is_empty() && asked == self.query => &self.found,
            Some(_) | None => &[],
        }
    }

    pub fn is_asking_elsewhere(&self) -> bool {
        !self.query.is_empty() && self.asking.as_deref() == Some(self.query.as_str())
    }

    pub fn unheld(&self) -> Arc<[MissingTrack]> {
        Arc::clone(&self.unheld)
    }

    pub fn found(&self) -> Arc<[Found]> {
        Arc::from(self.found_here())
    }

    pub const fn sung(&self) -> Option<&Sung> {
        self.sung.as_ref()
    }

    pub fn is_wanting(&self, found: &Found) -> bool {
        self.wanting.contains_key(&found.recording)
    }

    fn ask_elsewhere_after(&mut self, settling: Duration, cx: &mut Context<Self>) {
        let text = self.query.clone();
        let reference = match &self.reference {
            Some(reference) if self.online && asks_elsewhere(&text) => Arc::clone(reference),
            Some(_) | None => {
                self.asking = None;
                self._finding = Task::ready(());
                return;
            }
        };
        if self.found_for.as_deref() == Some(text.as_str()) {
            self.asking = None;
            self._finding = Task::ready(());
            return;
        }
        let library = Arc::clone(&self.library);
        self.asking = Some(text.clone());

        self._finding = cx.spawn(async move |this, cx| {
            if !settling.is_zero() {
                cx.background_executor().timer(settling).await;
            }
            let asked = text.clone();
            let found = cx
                .background_executor()
                .spawn(async move { library.found_elsewhere(reference.as_ref(), &asked) })
                .await;

            let landed = this.update(cx, |this, cx| {
                if this.asking.as_deref() != Some(text.as_str()) {
                    return;
                }
                this.asking = None;
                this.found = match found {
                    Ok(found) => found.into(),
                    Err(error) => {
                        tracing::warn!(%error, "a search could not be asked elsewhere");
                        Arc::default()
                    }
                };
                this.found_for = Some(text);
                this.restate_the_listing();
                cx.notify();
            });
            let _ = landed;
        });
    }

    pub fn want_found(&mut self, found: Found, cx: &mut Context<Self>) {
        let Some(reference) = self.reference.clone() else {
            return;
        };
        if self.wanting.contains_key(&found.recording) {
            return;
        }
        cx.notify();
        let library = Arc::clone(&self.library);
        let recording = found.recording.clone();

        let wanting = cx.spawn(async move |this, cx| {
            let asked = found.clone();
            let wanted = cx
                .background_executor()
                .spawn(async move { library.want_found(reference.as_ref(), &asked) })
                .await;

            let landed = this.update(cx, |this, cx| {
                if let Some(finished) = this.wanting.remove(&found.recording) {
                    finished.detach();
                }
                match wanted {
                    Ok(_) => {
                        toast::tell(
                            Notice::Done(format!(
                                "Wanted {} by {}{WANTED_ELSEWHERE}",
                                found.title, found.artist
                            )),
                            cx,
                        );
                        this.found_for = None;
                        this.ask_elsewhere_after(Duration::ZERO, cx);
                        this.poll_as(Prompted::OnItsOwn, cx);
                    }
                    Err(error) => {
                        tracing::error!(%error, "a song found elsewhere could not be wanted");
                        toast::tell(toast::could_not("want that song", &error), cx);
                    }
                }
                this.read(Wanted::Everything, cx);
            });
            let _ = landed;
        });
        self.wanting.insert(recording, wanting);
    }

    pub const fn opened_suggestion(&self) -> Option<&Previewed> {
        self.opened_suggestion.as_ref()
    }

    pub fn open_suggestion(&mut self, query: Option<SavedQuery>, cx: &mut Context<Self>) {
        let Some(query) = query else {
            self.opened_suggestion = None;
            self._previewing = Task::ready(());
            cx.notify();
            return;
        };
        if self
            .opened_suggestion
            .as_ref()
            .is_some_and(|held| held.query == query)
        {
            return;
        }
        self.opened_suggestion = Some(Previewed {
            query: query.clone(),
            tracks: Arc::default(),
        });
        cx.notify();
        self.read_the_preview(query, cx);
    }

    fn read_the_preview(&mut self, query: SavedQuery, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self._previewing = cx.spawn(async move |this, cx| {
            let asked = query.clone();
            let read = cx
                .background_executor()
                .spawn(async move {
                    library.tracks(&TrackQuery {
                        limit: Some(PREVIEWED_AT_MOST),
                        ..TrackQuery::from(&asked)
                    })
                })
                .await;

            let landed = this.update(cx, |this, cx| {
                let tracks = match read {
                    Ok(tracks) => tracks,
                    Err(error) => {
                        tracing::error!(%error, "a suggestion could not be previewed");
                        return;
                    }
                };
                this.remember(&tracks);
                if let Some(previewed) = this
                    .opened_suggestion
                    .as_mut()
                    .filter(|held| held.query == query)
                {
                    previewed.tracks = tracks.into();
                    cx.notify();
                }
            });
            let _ = landed;
        });
    }

    pub fn rows(&self) -> Arc<[ListedRow]> {
        Arc::clone(&self.rows)
    }

    pub fn release_tracks(&self) -> Arc<[HeldReleaseTrack]> {
        Arc::clone(&self.release_tracks)
    }

    pub const fn release(&self) -> Option<&ReleaseDetail> {
        self.release.as_ref()
    }

    pub const fn artist_detail(&self) -> Option<&ArtistDetail> {
        self.artist.as_ref()
    }

    pub fn artist_albums(&self) -> Arc<[Album]> {
        Arc::clone(&self.artist_albums)
    }

    pub const fn artist_totals(&self) -> ArtistTotals {
        self.artist_totals
    }

    pub fn scoped_album(&self, id: AlbumId) -> Option<&Album> {
        self.album.as_ref().filter(|album| album.id == id)
    }

    pub fn wanted(&self, release_track: ReleaseTrackId) -> Option<WantId> {
        self.wanted.get(&release_track).copied()
    }

    pub fn missing_track_rows(&self) -> Arc<[MissingRow]> {
        Arc::clone(&self.missing_track_rows)
    }

    pub fn unheld_release_rows(&self) -> Arc<[MissingRow]> {
        Arc::clone(&self.unheld_release_rows)
    }

    pub fn missing_tracks(&self) -> Arc<[MissingTrack]> {
        Arc::clone(&self.missing_tracks)
    }

    pub fn unheld_releases(&self) -> Arc<[UnheldRelease]> {
        Arc::clone(&self.unheld_releases)
    }

    pub const fn missing(&self) -> Missing {
        self.missing
    }

    pub fn want(&mut self, release_track: ReleaseTrackId, cx: &mut Context<Self>) {
        self.edited_then(
            Wanted::ThePlaylists,
            move |library| library.want(release_track).map(|_| None),
            |this, cx| {
                this.poll_as(Prompted::OnItsOwn, cx);
            },
            cx,
        );
    }

    pub fn unwant(&mut self, want: WantId, cx: &mut Context<Self>) {
        self.edit(move |library| library.unwant(want).map(|_| None), cx);
    }

    pub fn favour(&mut self, what: Favoured, favourite: bool, cx: &mut Context<Self>) {
        self.favoured.insert(what, favourite);
        if let Favoured::Track(id) = what
            && let Some(mut named) = self.named.get(&id).cloned()
            && let Some(track) = named.track.as_mut()
        {
            track.favourite = favourite.then(SystemTime::now);
            self.named.insert(id, named);
            self.revision = self.revision.wrapping_add(1);
        }
        cx.notify();

        self.edited(
            Wanted::Everything,
            move |library| library.favour(what, favourite).map(|_| None),
            cx,
        );
    }

    pub fn renamed(&mut self, id: TrackId, cx: &mut Context<Self>) {
        match self.library.track(id) {
            Ok(Some(track)) => self.remember(slice::from_ref(&track)),
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "a renamed track could not be read back"),
        }
        self.read(Wanted::Everything, cx);
    }

    pub fn hide_track(&mut self, id: TrackId, hidden: bool, cx: &mut Context<Self>) {
        self.edited(
            Wanted::Everything,
            move |library| library.hide_track(id, hidden).map(|_| None),
            cx,
        );
    }

    pub fn forget_delivered(&mut self, track: &Track, cx: &mut Context<Self>) {
        let Some(path) = track.location.as_path().map(Path::to_path_buf) else {
            return;
        };
        let title = track.title.clone();
        self.edited(
            Wanted::Everything,
            move |library| {
                library
                    .forget_delivered(&path)
                    .map(|forgot| forgot.then(|| format!("Forgot the delivered {title}")))
            },
            cx,
        );
    }

    pub fn favourite_albums(&self) -> Arc<[Album]> {
        Arc::clone(&self.favourite_albums)
    }

    pub fn favourite_artists(&self) -> Arc<[Artist]> {
        Arc::clone(&self.favourite_artists)
    }

    pub fn favourite_tracks(&self) -> Arc<[Track]> {
        Arc::clone(&self.favourite_tracks)
    }

    pub fn favours(&self, what: Favoured, as_read: bool) -> bool {
        self.favoured.get(&what).copied().unwrap_or(as_read)
    }

    pub fn favoured_album(&self, id: AlbumId) -> bool {
        self.favours(Favoured::Album(id), self.favourite_album_ids.contains(&id))
    }

    pub fn favoured_artist(&self, id: ArtistId) -> bool {
        self.favours(
            Favoured::Artist(id),
            self.favourite_artist_ids.contains(&id),
        )
    }

    pub fn favourited(&self) -> Favourited {
        Favourited {
            artists: self.favourite_artists.len(),
            albums: self.favourite_albums.len(),
            tracks: self.favourite_tracks.len(),
        }
    }

    pub const fn statistics(&self) -> Statistics {
        self.statistics
    }

    pub fn most_listened(&self) -> Arc<MostListened> {
        Arc::clone(&self.most_listened)
    }

    pub fn listening_by_day(&self) -> Arc<[Day]> {
        Arc::clone(&self.by_day)
    }

    pub(crate) fn charted(&self) -> Arc<Chart> {
        Arc::clone(&self.charted)
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn suggestions(&self) -> Arc<[Suggestion]> {
        Arc::clone(&self.suggestions)
    }

    pub const fn window(&self) -> Window {
        self.window
    }

    pub fn read_over(&mut self, window: Window, cx: &mut Context<Self>) {
        if self.window == window {
            return;
        }
        self.window = window;
        self.reload(cx);
    }

    pub fn share(&mut self, track: TrackId, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);

        self._shared = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.shareable(track) })
                .await;

            let outcome = this.update(cx, |_, cx| {
                toast::tell(
                    match read {
                        Err(error) => {
                            tracing::error!(%error, "a track could not be shared");
                            toast::could_not("share that track", &error)
                        }
                        Ok(None) => Notice::Noted(NOTHING_TO_SHARE.to_owned()),
                        Ok(Some(shared)) => {
                            let said = on_the_clipboard(&shared);
                            clipboard::copy(shared.written(), cx);
                            Notice::Done(said)
                        }
                    },
                    cx,
                );
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub(crate) const fn listed(&self) -> Measured {
        match self.selection {
            Selection::Everything => self.tracks_measured,
            Selection::Album(_) | Selection::Artist(_) => self.scoped_measured,
        }
    }

    pub(crate) const fn albums_counted(&self) -> u32 {
        self.albums_counted
    }

    pub(crate) const fn artists_counted(&self) -> u32 {
        self.artists_counted
    }

    pub(crate) const fn tracks_counted(&self) -> u32 {
        self.tracks_measured.rows
    }

    pub(crate) fn catalog(&self) -> Arc<Library> {
        Arc::clone(&self.library)
    }

    pub(crate) fn listing_whole(&self) -> (Arc<Library>, TrackQuery) {
        let (album, artist) = match self.selection {
            Selection::Everything => (None, None),
            Selection::Album(album) => (Some(album), None),
            Selection::Artist(artist) => (None, Some(artist)),
        };

        (
            Arc::clone(&self.library),
            TrackQuery {
                album,
                artist,
                text: (!self.query.is_empty()).then(|| self.query.clone()),
                sort: self.sorting.tracks,
                reading: self.sorting.tracks_read,
                limit: None,
                offset: 0,
            },
        )
    }

    pub(crate) fn rows_of(&self, query: &SavedQuery) -> (Arc<Library>, TrackQuery) {
        (Arc::clone(&self.library), TrackQuery::from(query))
    }

    pub(crate) fn reach_further(&mut self, drawn_to: usize, held: usize, cx: &mut Context<Self>) {
        if held < self.reach || drawn_to + LOOK_AHEAD < held {
            return;
        }
        self.reach = self.reach.saturating_add(PAGE);
        self.read(Wanted::Everything, cx);
    }

    pub fn played_from(&self, row: usize) -> Option<(Arc<[Track]>, usize)> {
        if self.rows.is_empty() {
            return Some((self.listing(), row));
        }
        self.played_from_held(held_at(&self.rows, row)?)
    }

    pub fn played_from_held(&self, held: usize) -> Option<(Arc<[Track]>, usize)> {
        let listing = self.listing();
        if self.rows.is_empty() {
            return Some((listing, held));
        }
        let order = held_in(&self.rows);
        let start = order.iter().position(|at| *at == held)?;
        let played = order
            .iter()
            .filter_map(|at| listing.get(*at).cloned())
            .collect();

        Some((played, start))
    }

    pub(crate) fn as_drawn(&self) -> AsDrawn {
        match self.selection {
            Selection::Album(_) => AsDrawn::Album {
                release_tracks: Arc::clone(&self.release_tracks),
                arranging: self.arranging(),
            },
            Selection::Everything | Selection::Artist(_) => AsDrawn::Listed,
        }
    }

    const fn arranging(&self) -> Arranging {
        Arranging {
            sort: self.sorting.tracks,
            reading: self.sorting.tracks_read,
        }
    }

    pub fn listed_rows(&self) -> usize {
        match self.rows.is_empty() {
            true => self.listing().len(),
            false => self.rows.len(),
        }
    }

    pub fn albums(&self) -> Arc<[Album]> {
        Arc::clone(&self.albums)
    }

    pub fn artists(&self) -> Arc<[Artist]> {
        Arc::clone(&self.artists)
    }

    pub fn tracks(&self) -> Arc<[Track]> {
        Arc::clone(&self.tracks)
    }

    pub fn listing(&self) -> Arc<[Track]> {
        match self.selection {
            Selection::Everything => Arc::clone(&self.tracks),
            Selection::Album(_) | Selection::Artist(_) => Arc::clone(&self.scoped),
        }
    }

    pub fn album_of(&self, id: AlbumId) -> Option<&Album> {
        self.scoped_album(id)
            .or_else(|| self.album_at.get(&id).and_then(|at| self.albums.get(*at)))
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    pub fn saved_playlists(&self) -> Arc<[Playlist]> {
        Arc::clone(&self.playlists)
    }

    pub fn lists(&self) -> Arc<[Playlist]> {
        Arc::clone(&self.lists)
    }

    pub fn entries(&self) -> Arc<[PlaylistEntry]> {
        Arc::clone(&self.entries)
    }

    pub const fn opened(&self) -> Option<PlaylistId> {
        self.opened
    }

    pub const fn playlist_order(&self) -> PlaylistOrder {
        self.order
    }

    pub const fn playlist_reading(&self) -> Direction {
        self.reading
    }

    pub fn order_playlists(&mut self, order: PlaylistOrder, cx: &mut Context<Self>) {
        if self.order == order {
            return;
        }
        self.order = order;
        self.reading = order.reads();
        self.reload_playlists(cx);
    }

    pub fn read_playlists(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.reading == reading {
            return;
        }
        self.reading = reading;
        self.reload_playlists(cx);
    }

    pub const fn sorting(&self) -> Sorting {
        self.sorting
    }

    pub fn order_tracks(&mut self, sort: SortOrder, cx: &mut Context<Self>) {
        if self.sorting.tracks == sort {
            return;
        }
        self.sorting.tracks = sort;
        self.sorting.tracks_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_tracks(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.tracks_read == reading {
            return;
        }
        self.sorting.tracks_read = reading;
        self.read_listings_again(cx);
    }

    pub fn order_albums(&mut self, sort: AlbumOrder, cx: &mut Context<Self>) {
        if self.sorting.albums == sort {
            return;
        }
        self.sorting.albums = sort;
        self.sorting.albums_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_albums(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.albums_read == reading {
            return;
        }
        self.sorting.albums_read = reading;
        self.read_listings_again(cx);
    }

    pub fn order_artists(&mut self, sort: ArtistOrder, cx: &mut Context<Self>) {
        if self.sorting.artists == sort {
            return;
        }
        self.sorting.artists = sort;
        self.sorting.artists_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_artists(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.artists_read == reading {
            return;
        }
        self.sorting.artists_read = reading;
        self.read_listings_again(cx);
    }

    fn read_listings_again(&mut self, cx: &mut Context<Self>) {
        self.reach = PAGE;
        self.read(Wanted::Everything, cx);
    }

    pub fn opened_playlist(&self) -> Option<&Playlist> {
        self.held.as_ref()
    }

    pub fn saved_playlist(&self, id: PlaylistId) -> Option<&Playlist> {
        self.held
            .as_ref()
            .filter(|held| held.id == id)
            .or_else(|| self.playlists.iter().find(|held| held.id == id))
    }

    pub fn narrowing(&self) -> Option<&str> {
        (!self.query.is_empty()).then_some(self.query.as_str())
    }

    pub fn instead(&self) -> Option<&str> {
        self.instead.as_deref()
    }

    pub fn entries_of(&self, id: PlaylistId) -> Vec<PlaylistEntry> {
        if self.opened == Some(id) && self.narrowing().is_none() {
            return self.entries.to_vec();
        }
        match self.library.playlist_entries(id, None) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, playlist = id.get(), "a playlist could not be read");
                Vec::new()
            }
        }
    }

    pub fn playing_playlist(&self, queue: QueueStamp) -> Option<PlaylistId> {
        self.library.playing_playlist(queue)
    }

    pub fn set_playing_playlist(&self, playing: Option<Playing>) {
        self.library.set_playing_playlist(playing);
    }

    pub fn open_playlist(&mut self, opened: Option<PlaylistId>, cx: &mut Context<Self>) {
        if self.opened == opened {
            return;
        }
        self.opened = opened;
        self.held = None;
        self.entries = Arc::default();
        self.reload_playlists(cx);
    }

    pub fn create_playlist(&mut self, name: String, holding: Vec<Cut>, cx: &mut Context<Self>) {
        self.edit(
            move |library| {
                library.start_playlist(&name, &holding)?;

                Ok(Some(match holding.len() {
                    0 => format!("Started {name}{TAKEN_BACK}"),
                    1 => format!("Started {name} with 1 track{TAKEN_BACK}"),
                    added => format!("Started {name} with {added} tracks{TAKEN_BACK}"),
                }))
            },
            cx,
        );
    }

    pub fn save_query(&mut self, name: String, query: SavedQuery, cx: &mut Context<Self>) {
        self.edit(
            move |library| {
                let id = library.save_query(&name, &query)?;
                let held = library.playlist(id)?.map_or(0, |found| found.entries);

                Ok(Some(format!("{name} fills itself, and holds {held} now")))
            },
            cx,
        );
    }

    pub fn revise_query(
        &mut self,
        id: PlaylistId,
        name: String,
        query: SavedQuery,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| {
                library.revise_query(id, &name, &query)?;
                let held = library.playlist(id)?.map_or(0, |found| found.entries);

                Ok(Some(format!(
                    "{name} fills itself from the new search, and holds {held} now"
                )))
            },
            cx,
        );
    }

    pub fn rename_playlist(&mut self, id: PlaylistId, name: String, cx: &mut Context<Self>) {
        self.edit(
            move |library| library.rename_playlist(id, &name).map(|()| None),
            cx,
        );
    }

    pub fn drop_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        if self.opened == Some(id) {
            self.opened = None;
            self.held = None;
            self.entries = Arc::default();
        }
        self.edit(move |library| library.remove_playlist(id).map(|_| None), cx);
    }

    pub fn pin_playlist(&mut self, id: PlaylistId, pinned: bool, cx: &mut Context<Self>) {
        self.edit(
            move |library| library.pin_playlist(id, pinned).map(|_| None),
            cx,
        );
    }

    pub fn add_to_playlist(&mut self, id: PlaylistId, holding: Vec<Cut>, cx: &mut Context<Self>) {
        self.edit(
            move |library| {
                let added = library.add_to_playlist(id, &holding)?;
                let name = library
                    .playlist(id)?
                    .map_or_else(|| format!("playlist {id}"), |found| found.name);

                Ok(Some(match added {
                    0 => format!("Nothing went into {name}"),
                    1 => format!("Put 1 track in {name}{TAKEN_BACK}"),
                    added => format!("Put {added} tracks in {name}{TAKEN_BACK}"),
                }))
            },
            cx,
        );
    }

    pub fn remove_from_playlist(&mut self, id: PlaylistId, rows: Span, cx: &mut Context<Self>) {
        self.edit(
            move |library| library.remove_from_playlist(id, rows).map(|_| None),
            cx,
        );
    }

    pub fn remove_matching(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        let Some(matching) = self.narrowing().map(ToOwned::to_owned) else {
            return;
        };
        self.edit(
            move |library| {
                Ok(Some(match library.remove_matching(id, &matching)? {
                    0 => "Nothing shown was left to take out".to_owned(),
                    1 => "Took 1 row out".to_owned(),
                    dropped => format!("Took {dropped} rows out"),
                }))
            },
            cx,
        );
    }

    pub fn move_in_playlist(
        &mut self,
        id: PlaylistId,
        rows: Span,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| library.move_in_playlist(id, rows, to).map(|_| None),
            cx,
        );
    }

    pub fn import_playlists(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        if files.is_empty() {
            return;
        }
        self.edit(
            move |library| {
                let mut said = Vec::with_capacity(files.len());
                for file in &files {
                    said.push(read_in(library.import_playlist(file, None)?));
                }
                Ok(Some(said.join(" · ")))
            },
            cx,
        );
    }

    pub fn export_playlist(&mut self, id: PlaylistId, path: PathBuf, cx: &mut Context<Self>) {
        self.edit(
            move |library| {
                let written = library.export_playlist(id, &path)?;
                Ok(Some(format!(
                    "Wrote {} rows to {} as {}",
                    written.rows,
                    path.display(),
                    written.format.name()
                )))
            },
            cx,
        );
    }

    pub fn sort_playlist(
        &mut self,
        id: PlaylistId,
        order: RowOrder,
        reading: Direction,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| {
                Ok(Some(match library.sort_playlist(id, order, reading)? {
                    0 => "Already in that order".to_owned(),
                    1 => "Moved 1 row into that order".to_owned(),
                    moved => format!("Moved {moved} rows into that order"),
                }))
            },
            cx,
        );
    }

    pub fn keep_playlist_in_order(
        &mut self,
        id: PlaylistId,
        kept: Option<Kept>,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| {
                let moved = library.keep_playlist_in_order(id, kept)?;
                let Some(_) = kept else {
                    return Ok(Some(
                        "Back in hand: a row added now lands at the end".to_owned(),
                    ));
                };

                Ok(Some(match moved {
                    0 => "Kept in that order, and it was already in it".to_owned(),
                    1 => "Kept in that order, and 1 row moved into it".to_owned(),
                    moved => format!("Kept in that order, and {moved} rows moved into it"),
                }))
            },
            cx,
        );
    }

    pub fn prune_playlist(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        self.edit(
            move |library| {
                Ok(Some(match library.prune_playlist(id)? {
                    0 => "Every row still names a file that is there".to_owned(),
                    1 => "Dropped 1 row whose file has gone".to_owned(),
                    dropped => format!("Dropped {dropped} rows whose files have gone"),
                }))
            },
            cx,
        );
    }

    pub fn fold_doubles(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        self.edit(
            move |library| {
                Ok(Some(match library.fold_doubles(id)? {
                    0 => "Every row names a file no other row does".to_owned(),
                    1 => "Folded 1 doubled row into the one above it".to_owned(),
                    folded => format!("Folded {folded} doubled rows into the ones above them"),
                }))
            },
            cx,
        );
    }

    pub fn track_heard(&mut self, heard: Played, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        self.counted = None;

        self._counted = cx.spawn(async move |this, cx| {
            let counted = cx
                .background_executor()
                .spawn(async move { library.track_played(&heard.location, heard.span) })
                .await;

            let outcome = this.update(cx, |this, cx| match counted {
                Err(error) => tracing::warn!(%error, "a play was not counted"),
                Ok(None) => {}
                Ok(Some(counted)) => {
                    this.revision = this.revision.wrapping_add(1);
                    this.counted = Some(counted.listen);
                    this.named
                        .insert(counted.track.id, Named::of(&counted.track));
                    this.reload(cx);
                }
            });
            let _ = outcome;
        });
    }

    pub fn track_hearing(&mut self, heard: Duration, cx: &mut Context<Self>) {
        let Some(listen) = self.counted else {
            return;
        };
        let library = Arc::clone(&self.library);

        self._settled = cx.background_executor().spawn(async move {
            if let Err(error) = library.listened(listen, heard) {
                tracing::warn!(%error, "how long a play has been heard for was not kept");
            }
        });
    }

    pub fn track_settled(&mut self, heard: Duration, cx: &mut Context<Self>) {
        let Some(listen) = self.counted.take() else {
            return;
        };
        let library = Arc::clone(&self.library);

        self._settled = cx.background_executor().spawn(async move {
            if let Err(error) = library.listened(listen, heard) {
                tracing::warn!(%error, "how long a play was heard for was not kept");
            }
        });
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.edit(
            |library| {
                Ok(Some(match library.undo()? {
                    Some(put_back) => put_back_as(&put_back),
                    None => "Nothing left to put back".to_owned(),
                }))
            },
            cx,
        );
    }

    pub fn undoable(&self) -> Option<Undoable> {
        self.library.undoable()
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.edit(
            |library| {
                Ok(Some(match library.redo()? {
                    Some(done_again) => done_again_as(&done_again),
                    None => "Nothing left to do again".to_owned(),
                }))
            },
            cx,
        );
    }

    pub fn redoable(&self) -> Option<Undoable> {
        self.library.redoable()
    }

    fn edit(
        &mut self,
        change: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.edited(Wanted::ThePlaylists, change, cx);
    }

    fn edited(
        &mut self,
        wanted: Wanted,
        change: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.edited_then(wanted, change, |_, _| {}, cx);
    }

    fn edited_then(
        &mut self,
        wanted: Wanted,
        change: impl FnOnce(&Library) -> resonate_library::Result<Option<String>> + Send + 'static,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let library = Arc::clone(&self.library);

        self._edit = cx.spawn(async move |this, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { change(&library) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                match done {
                    Err(error) => {
                        tracing::error!(%error, "a playlist could not be edited");
                        toast::tell(toast::could_not("change the playlist", &error), cx);
                    }
                    Ok(Some(said)) => {
                        toast::tell(Notice::Done(said), cx);
                        then(this, cx);
                    }
                    Ok(None) => then(this, cx),
                }
                this.read(wanted, cx);
            });
            let _ = outcome;
        });
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub const fn search(&self) -> &Search {
        &self.search
    }

    pub const fn selection(&self) -> Selection {
        self.selection
    }

    pub const fn is_busy(&self) -> bool {
        self.work.is_busy()
    }

    pub const fn is_scanning(&self) -> bool {
        self.work.scanning().is_some()
    }

    pub fn is_stopping(&self) -> bool {
        self.work
            .scanning()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn stats(&self) -> Option<ScanStats> {
        match self.work.scanning() {
            Some(progress) => Some(progress.snapshot()),
            None => self.summary.map(|summary| summary.stats),
        }
    }

    pub fn was_stopped(&self) -> bool {
        self.summary.is_some_and(|summary| summary.cancelled)
    }

    pub const fn is_organising(&self) -> bool {
        self.work.organising().is_some()
    }

    pub fn is_moving(&self) -> bool {
        self.work
            .organising()
            .is_some_and(|(pass, _)| pass.applies())
    }

    pub fn is_stopping_organise(&self) -> bool {
        self.work
            .organising()
            .is_some_and(|(_, progress)| progress.is_cancelled())
    }

    pub fn organise_stats(&self) -> Option<OrganiseStats> {
        match self.work.organising() {
            Some((_, progress)) => Some(progress.snapshot()),
            None => self.organised.as_ref().map(|(_, summary)| summary.stats),
        }
    }

    pub fn organised(&self) -> Option<(Pass, &OrganiseSummary)> {
        self.organised
            .as_ref()
            .map(|(pass, summary)| (*pass, summary))
    }

    pub const fn previewed(&self) -> Planned {
        self.previewed
    }

    pub fn forget_the_preview(&mut self, cx: &mut Context<Self>) {
        if self.organised.is_none() && self.previewed == Planned::Not {
            return;
        }
        self.organised = None;
        self.previewed = Planned::Not;
        cx.notify();
    }

    fn take_down_what_moved(&mut self) {
        let now = self.library.plans_stamp();
        if self.previewed.outdated_at(now) {
            self.organised = None;
            self.previewed = Planned::Outdated;
        }
        if self.previewed_tags.outdated_at(now) {
            self.tagged = None;
            self.previewed_tags = Planned::Outdated;
        }
        if self.previewed_import.outdated_at(now) {
            self.imported = None;
            self.previewed_import = Planned::Outdated;
        }
    }

    pub const fn is_tagging(&self) -> bool {
        self.work.tagging().is_some()
    }

    pub fn is_writing_tags(&self) -> bool {
        self.work.tagging().is_some_and(|(pass, _)| pass.applies())
    }

    pub fn is_stopping_retag(&self) -> bool {
        self.work
            .tagging()
            .is_some_and(|(_, progress)| progress.is_cancelled())
    }

    pub fn retag_stats(&self) -> Option<RetagStats> {
        match self.work.tagging() {
            Some((_, progress)) => Some(progress.snapshot()),
            None => self.tagged.as_ref().map(|(_, summary)| summary.stats),
        }
    }

    pub fn tagged(&self) -> Option<(Pass, &RetagSummary)> {
        self.tagged.as_ref().map(|(pass, summary)| (*pass, summary))
    }

    pub const fn previewed_tags(&self) -> Planned {
        self.previewed_tags
    }

    pub fn cover(
        &mut self,
        id: AlbumId,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        self.art_of(id, cx).map(|art| art.drawn(drawn))
    }

    pub fn whole_cover(&mut self, id: AlbumId, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        if let Some(magnified) = &self.magnified
            && magnified.names(&id)
        {
            return magnified.whole();
        }
        self.magnified.replace(Magnifying::Reading(id)).forget(cx);

        let library = Arc::clone(&self.library);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { whole_cover_of(&library, id) })
                .await;
            let landed = this.update(cx, |this, cx| {
                if this.magnified.as_ref().is_some_and(|held| held.names(&id)) {
                    this.magnified
                        .replace(Magnifying::Read(id, read))
                        .forget(cx);
                    cx.notify();
                }
            });
            let _ = landed;
        })
        .detach();
        None
    }

    fn art_of(&mut self, id: AlbumId, cx: &mut Context<Self>) -> Option<Art> {
        if let Some(held) = self.covers.get(&id) {
            return held.clone();
        }
        if self.decoding.contains(&id) || self.decoding.len() >= DECODES_AT_ONCE {
            return None;
        }
        self.decoding.insert(id);

        let library = Arc::clone(&self.library);
        let drawing = cx
            .global::<Drawer>()
            .draw(move || decoded_cover(&library, id));
        cx.spawn(async move |this, cx| {
            let decoded = drawing.await.flatten();
            let landed = this.update(cx, |this, cx| {
                this.covers.insert(id, decoded).forget(cx);
                this.decoding.remove(&id);
                cx.notify();
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn portrait(
        &mut self,
        id: ArtistId,
        portrayed: Portrayed,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        self.portrait_of(id, cx)
            .map(|portrait| portrait.portrayed(portrayed))
    }

    fn portrait_of(&mut self, id: ArtistId, cx: &mut Context<Self>) -> Option<Portrait> {
        if let Some(held) = self.portraits.get(&id) {
            return held.clone();
        }
        if self.decoding_portraits.contains(&id) || self.decoding_portraits.len() >= DECODES_AT_ONCE
        {
            return None;
        }
        self.decoding_portraits.insert(id);

        let library = Arc::clone(&self.library);
        let drawing = cx
            .global::<Drawer>()
            .draw(move || decoded_portrait(&library, id));
        cx.spawn(async move |this, cx| {
            let decoded = drawing.await.flatten();
            let landed = this.update(cx, |this, cx| {
                this.portraits.insert(id, decoded).forget(cx);
                this.decoding_portraits.remove(&id);
                cx.notify();
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn remember(&mut self, tracks: &[Track]) {
        self.revision = self.revision.wrapping_add(1);
        for track in tracks {
            self.named.insert(track.id, Named::of(track));
        }
    }

    pub fn alternatives_of(&self, id: TrackId) -> Vec<Track> {
        self.library.alternatives_of(id).unwrap_or_else(|error| {
            tracing::warn!(%error, "the other copies of a track could not be read");
            Vec::new()
        })
    }

    pub fn track_of(&mut self, item: &QueueItem) -> Option<Track> {
        if let Some(held) = self.named.get(&item.id)
            && held.names(item)
        {
            return held.track.clone();
        }

        let found = self.scanned(item);
        self.named.insert(
            item.id,
            Named {
                location: item.location.clone(),
                span: item.span,
                track: found.clone(),
            },
        );
        found
    }

    fn scanned(&self, item: &QueueItem) -> Option<Track> {
        match self.library.track(item.id) {
            Ok(Some(track)) if track.location == item.location => return Some(track),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "a queued track could not be read by id"),
        }
        match self.library.track_at(item.location.as_path()?, item.span) {
            Ok(track) => track,
            Err(error) => {
                tracing::warn!(%error, "a queued track could not be read by path");
                None
            }
        }
    }

    pub fn album_title(&mut self, id: AlbumId) -> Option<String> {
        self.album_anywhere(id).map(|album| album.title)
    }

    fn album_anywhere(&mut self, id: AlbumId) -> Option<Album> {
        if let Some(listed) = self.album_of(id) {
            return Some(listed.clone());
        }
        if let Some(held) = self.read_albums.get(&id) {
            return held.clone();
        }
        let read = self.library.album(id).unwrap_or_else(|error| {
            tracing::warn!(%error, album = %id, "an album the listing does not hold could not be read");
            None
        });
        self.read_albums.insert(id, read.clone());
        read
    }

    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if self.query == query {
            return;
        }
        self.search = Search::read(&query);
        self.query = query;
        self.selection = Selection::Everything;
        self.reach = PAGE;
        self.read_after(SEARCH_SETTLE, Wanted::Everything, cx);
        self.ask_elsewhere_after(ASKED_ELSEWHERE_AFTER, cx);
    }

    pub fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if self.selection == selection {
            return;
        }
        match selection {
            Selection::Everything => {}
            Selection::Album(album) => {
                let owner = self.album_anywhere(album).and_then(|held| held.artist_id);
                self.ask_about(Some(album), owner);
            }
            Selection::Artist(artist) => self.ask_about(None, Some(artist)),
        }
        self.selection = selection;
        self.reach = PAGE;
        self.reload(cx);
    }

    pub fn ask_about(&self, album: Option<AlbumId>, artist: Option<ArtistId>) {
        if let Some(artist) = artist {
            self.sought.artist(artist);
        }
        if let Some(album) = album {
            self.sought.album(album);
        }
    }

    fn ask_about_what_is_drawn(&self) {
        let Selection::Artist(artist) = self.selection else {
            return;
        };
        for album in albums_of(&self.scoped).iter().rev() {
            self.sought.album(*album);
        }
        self.sought.artist(artist);
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.read(Wanted::Everything, cx);
    }

    pub fn reload_playlists(&mut self, cx: &mut Context<Self>) {
        self.read(Wanted::ThePlaylists, cx);
    }

    fn read(&mut self, wanted: Wanted, cx: &mut Context<Self>) {
        self.read_after(Duration::ZERO, wanted, cx);
    }

    fn asked(&self) -> Asked {
        let (album, artist) = match self.selection {
            Selection::Everything => (None, None),
            Selection::Album(album) => (Some(album), None),
            Selection::Artist(artist) => (None, Some(artist)),
        };
        Asked {
            text: (!self.query.is_empty()).then(|| self.query.clone()),
            album,
            artist,
            opened: self.opened,
            order: self.order,
            reading: self.reading,
            sorting: self.sorting,
            reach: self.reach,
            window: self.window,
        }
    }

    fn read_after(&mut self, settling: Duration, wanted: Wanted, cx: &mut Context<Self>) {
        let wanted = if self.reading_everything {
            Wanted::Everything
        } else {
            wanted
        };
        self.reading_everything = wanted == Wanted::Everything;
        let library = Arc::clone(&self.library);
        let asked = self.asked();
        let read_ahead = self.first_read.take().filter(|first| {
            wanted == Wanted::Everything && settling.is_zero() && first.asked == asked
        });
        if let Some(first) = read_ahead.as_ref()
            && let Ok(read) = first.read.try_recv()
        {
            self.reading_everything = false;
            self.landed(read, cx);
            return;
        }

        self._load = cx.spawn(async move |this, cx| {
            if !settling.is_zero() {
                cx.background_executor().timer(settling).await;
            }
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    read_ahead
                        .and_then(|first| first.read.recv().ok())
                        .unwrap_or_else(|| load(&library, asked, wanted))
                })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.reading_everything = false;
                this.landed(loaded, cx);
            });
            let _ = outcome;
        });
    }

    fn landed(&mut self, loaded: resonate_library::Result<Loaded>, cx: &mut Context<Self>) {
        match loaded {
            Ok(loaded) => {
                self.take(loaded);
                self.take_down_what_moved();
                self.warm_the_covers(cx);
            }
            Err(error) => tracing::error!(%error, "the library could not be read"),
        }
        cx.notify();
    }

    fn take(&mut self, loaded: Loaded) {
        self.revision = self.revision.wrapping_add(1);
        self.wanted = loaded.wanted;
        self.missing_track_rows = missing_track_rows(
            loaded
                .missing_tracks
                .iter()
                .map(|track| (track.album, track.disc)),
        )
        .into();
        self.unheld_release_rows =
            unheld_release_rows(loaded.unheld_releases.iter().map(|release| release.artist)).into();
        self.missing_tracks = loaded.missing_tracks.into();
        self.unheld_releases = loaded.unheld_releases.into();
        self.missing = loaded.missing;
        if let Some(browsed) = loaded.browsed {
            self.albums = browsed.albums.into();
            self.artists = browsed.artists.into();
            self.tracks = browsed.tracks.into();
            self.favourite_albums = browsed.favourite_albums.into();
            self.favourite_artists = browsed.favourite_artists.into();
            self.favourite_tracks = browsed.favourite_tracks.into();
            self.statistics = browsed.statistics;
            self.most_listened = Arc::new(browsed.most_listened);
            self.by_day = browsed.by_day.into();
            self.suggestions = browsed.suggestions;
            self.unheld = browsed.unheld.into();
            self.sung = browsed.sung;
            self.scoped = browsed.scoped.into();
            self.albums_counted = browsed.albums_counted;
            self.artists_counted = browsed.artists_counted;
            self.tracks_measured = browsed.tracks_measured;
            self.scoped_measured = browsed.scoped_measured;
            self.roots = browsed.roots;
            self.release_tracks = browsed.release_tracks.into();
            self.release = browsed.release;
            self.artist = browsed.artist;
            self.artist_albums = browsed.artist_albums.into();
            self.artist_totals = browsed.artist_totals;
            self.album = browsed.album;
            self.instead = browsed.instead;
            self.index_the_albums();
            self.index_the_favourites();
            self.charted = Arc::new(Chart::of(&self.by_day, SystemTime::now(), BARS_AT_MOST));
            self.restate_the_listing();
            self.ask_about_what_is_drawn();
        }
        self.playlists = loaded.playlists.into();
        self.lists = loaded.lists.into();
        self.held = loaded.held;
        self.entries = loaded.entries.into();
        if self.held.is_none() {
            self.opened = None;
        }
    }

    fn index_the_albums(&mut self) {
        self.read_albums = Recent::new(ALBUMS_HELD);
        self.album_at.clear();
        for (at, album) in self.albums.iter().enumerate() {
            self.album_at.insert(album.id, at);
        }
    }

    fn index_the_favourites(&mut self) {
        self.favourite_album_ids.clear();
        self.favourite_artist_ids.clear();

        for album in self
            .album
            .iter()
            .chain(self.albums.iter())
            .chain(self.favourite_albums.iter())
            .filter(|album| album.favourite.is_some())
        {
            self.favourite_album_ids.insert(album.id);
        }
        for artist in self
            .artists
            .iter()
            .chain(self.favourite_artists.iter())
            .filter(|artist| artist.favourite.is_some())
        {
            self.favourite_artist_ids.insert(artist.id);
        }
    }

    fn warm_the_covers(&mut self, cx: &mut Context<Self>) {
        if self.warmed {
            return;
        }
        self.warmed = true;

        let wanted: Vec<AlbumId> = self
            .albums
            .iter()
            .filter(|album| album.has_cover_art)
            .map(|album| album.id)
            .take(COVERS_WARMED)
            .collect();
        if wanted.is_empty() {
            self.warmed = false;
            return;
        }

        let library = Arc::clone(&self.library);
        self._warming = cx.spawn(async move |this, cx| {
            for id in wanted {
                let Ok(worth_reading) = this.update(cx, |this, _| {
                    !this.covers.holds(&id) && !this.decoding.contains(&id)
                }) else {
                    return;
                };
                if !worth_reading {
                    continue;
                }

                let reading = Arc::clone(&library);
                let Ok(drawing) = this.update(cx, |_, cx| {
                    cx.global::<Drawer>()
                        .draw(move || decoded_cover(&reading, id))
                }) else {
                    return;
                };
                let decoded = drawing.await.flatten();

                let landed = this.update(cx, |this, cx| {
                    this.covers.insert(id, decoded).forget(cx);
                    cx.notify();
                });
                if landed.is_err() {
                    return;
                }
            }
        });
    }

    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.scan(Vec::new(), cx);
    }

    pub fn read_everything_again(&mut self, cx: &mut Context<Self>) {
        self.start_scan(Vec::new(), Reading::Everything, Prompted::ByHand, cx);
    }

    pub fn look_for_missing_covers(&mut self, cx: &mut Context<Self>) {
        if self.enriching.is_some() || !self.can_enrich() {
            return;
        }
        match self.library.ask_again_for_covers() {
            Ok(_) => self.enrich(false, cx),
            Err(error) => {
                tracing::error!(%error, "the covers still missing could not be asked for again");
                toast::tell(
                    toast::could_not("look for the missing covers again", &error),
                    cx,
                );
                cx.notify();
            }
        }
    }

    pub fn add_roots(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        if roots.is_empty() {
            return;
        }
        self.scan(roots, cx);
    }

    fn scan(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.start_scan(roots, Reading::WhatChanged, Prompted::ByHand, cx);
    }

    fn start_scan(
        &mut self,
        roots: Vec<PathBuf>,
        reading: Reading,
        prompted: Prompted,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.work.is_busy() {
            return false;
        }
        let handle = match walk(&self.library, roots, reading, prompted) {
            Ok(Some(handle)) => handle,
            Ok(None) => return true,
            Err(error) if prompted == Prompted::OnItsOwn => {
                tracing::debug!(%error, "a scan for files changed under the roots could not start yet");
                return false;
            }
            Err(error) => {
                tracing::error!(%error, "the scan could not be started");
                toast::tell(toast::could_not("start the scan", &error), cx);
                cx.notify();
                return false;
            }
        };

        self.work = Work::Scanning(Arc::clone(handle.progress()));
        self.summary = None;
        cx.notify();

        self._scan = cx.spawn(async move |this, cx| {
            let mut polled = 0;
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                polled += 1;

                let shown = this.update(cx, |this, cx| {
                    if polled % POLLS_PER_RELOAD == 0 {
                        this.reload(cx);
                    } else {
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                match handle.join() {
                    Ok(summary) => {
                        this.summary = Some(summary);
                        let brought = summary.stats.added + summary.stats.updated > 0;
                        let worth_asking = prompted == Prompted::ByHand || brought;
                        if !summary.cancelled
                            && this.after_scan
                            && this.can_enrich()
                            && worth_asking
                        {
                            this.enrich(false, cx);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "the scan failed");
                        toast::tell(toast::could_not("finish the scan", &error), cx);
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
        true
    }

    pub const fn is_online(&self) -> bool {
        self.online
    }

    pub fn set_online(&mut self, online: bool, cx: &mut Context<Self>) {
        if self.online == online {
            return;
        }
        self.online = online;
        cx.notify();
    }

    pub const fn resumes(&self) -> bool {
        self.resume
    }

    pub fn set_resume(&mut self, resume: bool, cx: &mut Context<Self>) {
        if self.resume == resume {
            return;
        }
        self.resume = resume;
        if !resume {
            let library = Arc::clone(&self.library);
            self._kept = cx.background_executor().spawn(async move {
                if let Err(error) = library.forget_resumption() {
                    tracing::warn!(%error, "what was kept of a queue could not be discarded");
                }
            });
        }
        cx.notify();
    }

    pub fn keep_the_queue(&mut self, keep: Keep, cx: &Context<Self>) {
        let library = Arc::clone(&self.library);

        self._kept = cx.background_executor().spawn(async move {
            let kept = match keep {
                Keep::Queue(resumption) => library.keep_resumption(&resumption),
                Keep::Order(reordered) => library.keep_order(&reordered),
                Keep::Place { row, at } => library.keep_place(row, at),
            };
            if let Err(error) = kept {
                tracing::warn!(%error, "the queue was not kept for the next run");
            }
        });
    }

    pub const fn enriches_after_scan(&self) -> bool {
        self.after_scan
    }

    pub const fn studies(&self) -> bool {
        self.studies
    }

    pub fn set_studies(&mut self, studies: bool, cx: &mut Context<Self>) {
        if self.studies == studies {
            return;
        }
        self.studies = studies;
        cx.notify();
    }

    pub fn set_after_scan(&mut self, after_scan: bool, cx: &mut Context<Self>) {
        if self.after_scan == after_scan {
            return;
        }
        self.after_scan = after_scan;
        cx.notify();
    }

    pub fn has_reference(&self) -> bool {
        self.reference.is_some()
    }

    pub fn can_enrich(&self) -> bool {
        self.online && self.reference.is_some()
    }

    pub fn is_enriching(&self) -> bool {
        self.enriching.is_some()
    }

    pub fn is_stopping_enrich(&self) -> bool {
        self.enriching
            .as_ref()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn enrich_stats(&self) -> Option<EnrichStats> {
        match self.enriching.as_ref() {
            Some(progress) => Some(progress.snapshot()),
            None => self.enriched.map(|summary| summary.stats),
        }
    }

    pub fn enrich(&mut self, refresh: bool, cx: &mut Context<Self>) {
        if self.enriching.is_some() || !self.online {
            return;
        }
        let Some(reference) = self.reference.clone() else {
            return;
        };
        let asked = EnrichOptions {
            refresh,
            at_most: None,
            sought: Arc::clone(&self.sought),
            studies: self.studies,
        };
        let handle = match self
            .library
            .enrich(reference, Arc::clone(&self.fingerprinters), asked)
        {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the lookup could not be started");
                toast::tell(toast::could_not("start the lookup", &error), cx);
                cx.notify();
                return;
            }
        };

        self.enriching = Some(Arc::clone(handle.progress()));
        self.enriched = None;
        cx.notify();

        self._enrich = cx.spawn(async move |this, cx| {
            let mut polled = 0;
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                polled += 1;

                let shown = this.update(cx, |this, cx| {
                    if polled % POLLS_PER_RELOAD == 0 {
                        this.reload(cx);
                    } else {
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.enriching = None;
                match handle.join() {
                    Ok(summary) => {
                        this.enriched = Some(summary);
                        toast::tell(looked_up(&summary), cx);
                    }
                    Err(error) => {
                        tracing::error!(%error, "the lookup failed");
                        toast::tell(toast::could_not("finish the lookup", &error), cx);
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }

    fn carry_on_enriching(&mut self, cx: &mut Context<Self>) {
        if !self.can_enrich() {
            return;
        }
        let left = match self.library.unfinished_enrichment() {
            Ok(left) => left,
            Err(error) => {
                tracing::warn!(%error, "what a lookup left unfinished could not be read back");
                return;
            }
        };
        let Some(left) = left else {
            return;
        };

        tracing::info!(
            began = ?left.began,
            refresh = left.refresh,
            "carrying on a lookup the last run left unfinished"
        );
        self.enrich(left.refresh, cx);
    }

    pub fn stop_enrich(&mut self, cx: &mut Context<Self>) {
        let Some(progress) = self.enriching.as_ref() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_scan(&mut self, cx: &mut Context<Self>) {
        let Some(progress) = self.work.scanning() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn has_a_vault(&self) -> bool {
        self.library.vault().is_some()
    }

    pub fn is_importing(&self) -> bool {
        self.work.importing().is_some()
    }

    pub fn is_keeping(&self) -> bool {
        self.work
            .importing()
            .is_some_and(|(pass, _)| pass.applies())
    }

    pub fn is_stopping_import(&self) -> bool {
        self.work
            .importing()
            .is_some_and(|(_, progress)| progress.is_cancelled())
    }

    pub fn import_stats(&self) -> Option<ImportStats> {
        match self.work.importing() {
            Some((_, progress)) => Some(progress.snapshot()),
            None => self.imported.as_ref().map(|(_, summary)| summary.stats),
        }
    }

    pub fn imported(&self) -> Option<(Pass, &ImportSummary)> {
        self.imported
            .as_ref()
            .map(|(pass, summary)| (*pass, summary))
    }

    pub const fn previewed_import(&self) -> Planned {
        self.previewed_import
    }

    pub fn import(&mut self, pass: Pass, cx: &mut Context<Self>) {
        if self.work.is_busy() {
            toast::tell(Notice::Trouble(ALREADY_WALKING.to_owned()), cx);
            cx.notify();
            return;
        }
        let read_at = self.library.plans_stamp();
        let handle = match self.library.import(
            Arc::new(Sources::local()),
            ImportOptions {
                apply: pass.applies(),
                ..ImportOptions::default()
            },
        ) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the tracks could not be kept in the vault");
                toast::tell(toast::could_not("keep the tracks in the vault", &error), cx);
                cx.notify();
                return;
            }
        };

        self.work = Work::Importing(pass, Arc::clone(handle.progress()));
        self.imported = None;
        self.previewed_import = Planned::Not;
        cx.notify();

        self._import = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                match handle.join() {
                    Ok(summary) => {
                        this.previewed_import = Planned::after(pass, summary.cancelled, read_at);
                        this.imported = Some((pass, summary));
                    }
                    Err(error) => {
                        tracing::error!(%error, "the tracks were not kept in the vault");
                        toast::tell(toast::could_not("keep the tracks in the vault", &error), cx);
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }

    pub fn inbox(&self) -> Option<&Path> {
        self.sourcing.inbox.as_deref()
    }

    pub fn set_inbox(&mut self, inbox: Option<PathBuf>, cx: &mut Context<Self>) {
        self.sourcing.inbox = inbox;
        self.polled = None;
        cx.notify();
    }

    pub fn can_poll(&self) -> bool {
        self.sourcing.providers().has_a_source()
    }

    pub fn is_polling(&self) -> bool {
        self.work.polling().is_some()
    }

    pub fn is_stopping_poll(&self) -> bool {
        self.work
            .polling()
            .is_some_and(|progress| progress.is_cancelled())
    }

    pub fn poll_stats(&self) -> Option<PollStats> {
        match self.work.polling() {
            Some(progress) => Some(progress.snapshot()),
            None => self.polled.map(|summary| summary.stats),
        }
    }

    pub const fn polled(&self) -> Option<&PollSummary> {
        self.polled.as_ref()
    }

    pub fn poll(&mut self, cx: &mut Context<Self>) {
        self.poll_as(Prompted::ByHand, cx);
    }

    fn poll_as(&mut self, prompted: Prompted, cx: &mut Context<Self>) -> bool {
        if self.work.is_busy() {
            if prompted == Prompted::ByHand {
                toast::tell(Notice::Trouble(ALREADY_WALKING.to_owned()), cx);
                cx.notify();
            }
            return false;
        }
        let providers = Arc::new(self.sourcing.providers());
        let options = match prompted {
            Prompted::ByHand | Prompted::ByTheInbox => PollOptions::ASKING_EVERY_WANT,
            Prompted::OnItsOwn => PollOptions::default(),
        };
        if prompted != Prompted::ByHand && !self.worth_asking_on_its_own(&providers, options) {
            return true;
        }
        let handle = match self.library.poll(providers, options) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the providers could not be asked");
                toast::tell(toast::could_not("ask the providers", &error), cx);
                cx.notify();
                return true;
            }
        };

        self.work = Work::Polling(Arc::clone(handle.progress()));
        self.polled = None;
        cx.notify();

        self._poll = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                match handle.join() {
                    Ok(summary) => this.polled = Some(summary),
                    Err(error) => {
                        tracing::error!(%error, "the providers were not asked");
                        toast::tell(toast::could_not("ask the providers", &error), cx);
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
        true
    }

    fn worth_asking_on_its_own(&self, providers: &Providers, options: PollOptions) -> bool {
        if !providers.has_a_source() {
            return false;
        }
        match self.library.is_a_want_due(options) {
            Ok(due) => due,
            Err(error) => {
                tracing::warn!(%error, "the wants could not be read");
                false
            }
        }
    }

    pub fn stop_poll(&mut self, cx: &mut Context<Self>) {
        let Some(progress) = self.work.polling() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_import(&mut self, cx: &mut Context<Self>) {
        let Some((_, progress)) = self.work.importing() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_organise(&mut self, cx: &mut Context<Self>) {
        let Some((_, progress)) = self.work.organising() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn stop_retag(&mut self, cx: &mut Context<Self>) {
        let Some((_, progress)) = self.work.tagging() else {
            return;
        };
        progress.cancel();
        cx.notify();
    }

    pub fn retag(&mut self, pass: Pass, cx: &mut Context<Self>) {
        if self.work.is_busy() {
            toast::tell(Notice::Trouble(ALREADY_WALKING.to_owned()), cx);
            cx.notify();
            return;
        }
        let read_at = self.library.plans_stamp();
        let handle = match self.library.retag(
            Arc::new(FileTags::default()),
            RetagOptions {
                roots: Vec::new(),
                apply: pass.applies(),
            },
        ) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the catalog could not be written back into the files");
                toast::tell(
                    toast::could_not("write the tags into the files", &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        self.work = Work::Tagging(pass, Arc::clone(handle.progress()));
        self.tagged = None;
        self.previewed_tags = Planned::Not;
        cx.notify();

        self._retag =
            cx.spawn(async move |this, cx| {
                while !handle.is_finished() {
                    cx.background_executor().timer(SCAN_POLL).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }

                let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                match handle.join() {
                    Ok(summary) => {
                        this.previewed_tags = Planned::after(pass, summary.cancelled, read_at);
                        this.tagged = Some((pass, summary));
                    }
                    Err(error) => {
                        tracing::error!(%error, "the catalog was not written back into the files");
                        toast::tell(toast::could_not("write the tags into the files", &error), cx);
                    }
                }
                this.reload(cx);
            });
                let _ = finished;
            });
    }

    pub fn organise(&mut self, layout: Layout, pass: Pass, cx: &mut Context<Self>) {
        if self.work.is_busy() {
            toast::tell(Notice::Trouble(ALREADY_WALKING.to_owned()), cx);
            cx.notify();
            return;
        }
        let read_at = self.library.plans_stamp();
        let handle = match self.library.organise(OrganiseOptions {
            layout,
            apply: pass.applies(),
            ..OrganiseOptions::default()
        }) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the files could not be put in order");
                toast::tell(toast::could_not("put the files in order", &error), cx);
                cx.notify();
                return;
            }
        };

        self.work = Work::Organising(pass, Arc::clone(handle.progress()));
        self.organised = None;
        self.previewed = Planned::Not;
        cx.notify();

        self._organise = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                match handle.join() {
                    Ok(summary) => {
                        this.previewed = Planned::after(pass, summary.cancelled, read_at);
                        this.organised = Some((pass, summary));
                    }
                    Err(error) => {
                        tracing::error!(%error, "the files were not put in order");
                        toast::tell(toast::could_not("put the files in order", &error), cx);
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }

    pub fn forget_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.work.is_busy() {
            return;
        }
        self.work = Work::Forgetting;
        cx.notify();

        let library = Arc::clone(&self.library);
        self._scan = cx.spawn(async move |this, cx| {
            let dropped = cx
                .background_executor()
                .spawn(async move { library.remove_root(&root) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.work = Work::Nothing;
                if let Err(error) = dropped {
                    tracing::error!(%error, "a library folder could not be dropped");
                    toast::tell(toast::could_not("drop that library folder", &error), cx);
                }
                this.summary = None;
                this.reload(cx);
            });
            let _ = outcome;
        });
    }
}

fn put_back_as(put_back: &Undoable) -> String {
    let name = &put_back.name;

    match put_back.edit {
        Edit::Started => format!("{name} is gone again"),
        Edit::Renamed => format!("It is {name} again"),
        Edit::Revised => format!("{name} fills itself from the search it had"),
        Edit::Discarded => format!("Put {name} back"),
        Edit::Added | Edit::Copied | Edit::Imported => {
            format!("Took back out of {name} what went in")
        }
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            format!("Put back in {name} what went out")
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => {
            format!("Put {name} back in the order it was in")
        }
    }
}

fn done_again_as(done_again: &Undoable) -> String {
    let name = &done_again.name;

    match done_again.edit {
        Edit::Started => format!("Started {name} again"),
        Edit::Renamed => format!("It is {name} again"),
        Edit::Revised => format!("{name} fills itself from the search it was given"),
        Edit::Discarded => format!("{name} is gone again"),
        Edit::Added | Edit::Copied | Edit::Imported => {
            format!("Put back into {name} what had gone in")
        }
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            format!("Took back out of {name} what had gone out")
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => {
            format!("Put {name} back in the order that edit left")
        }
    }
}

fn looked_up(summary: &EnrichSummary) -> Notice {
    let stats = summary.stats;
    let mut said = format!(
        "{} and {} answered",
        format::counted(stats.releases as usize, "album", "albums"),
        format::counted(stats.artists as usize, "artist", "artists"),
    );
    if summary.cancelled {
        said.push_str(", stopped early");
    }

    match summary.stopped_by {
        Some(op) => Notice::Trouble(format!(
            "MusicBrainz couldn't be reached while looking up {} · {said}",
            asked_for(op)
        )),
        None => Notice::Noted(said),
    }
}

const fn asked_for(op: LookupOp) -> &'static str {
    match op {
        LookupOp::Release => "a release",
        LookupOp::FindRelease => "a release search",
        LookupOp::Recording => "a recording",
        LookupOp::Isrc => "a recording by its ISRC",
        LookupOp::FindRecording => "a recording search",
        LookupOp::ReleaseGroup => "a release group",
        LookupOp::FindReleaseGroup => "a release group search",
        LookupOp::Artist => "an artist",
        LookupOp::FindArtist => "an artist search",
        LookupOp::ReleaseGroupsOfArtist => "the releases of an artist",
        LookupOp::Cover => "a cover",
        LookupOp::Portrait => "a portrait",
        LookupOp::Lyrics => "lyrics",
        LookupOp::Devices => "the measured devices",
        LookupOp::Correction => "a measured correction",
        LookupOp::Recognise => "a recognition",
        LookupOp::Submit => "a submission of what was heard",
    }
}

fn on_the_clipboard(shared: &Shared) -> String {
    match shared.artist.as_deref() {
        Some(artist) => format!("{artist} — {} is on the clipboard", shared.title),
        None => format!("{} is on the clipboard", shared.title),
    }
}

fn albums_of(tracks: &[Track]) -> Vec<AlbumId> {
    let mut held = AHashSet::new();
    tracks
        .iter()
        .filter_map(|track| track.album_id)
        .filter(|album| held.insert(*album))
        .collect()
}

#[derive(Clone)]
struct Named {
    location: MediaLocation,
    span: Option<FrameSpan>,
    track: Option<Track>,
}

impl Named {
    fn of(track: &Track) -> Self {
        Self {
            location: track.location.clone(),
            span: track.span,
            track: Some(track.clone()),
        }
    }

    fn names(&self, item: &QueueItem) -> bool {
        self.location == item.location && self.span == item.span
    }
}

fn held_at(rows: &[ListedRow], row: usize) -> Option<usize> {
    match rows.get(row)? {
        ListedRow::Held(held) => Some(*held),
        ListedRow::Disc(_)
        | ListedRow::Missing(_)
        | ListedRow::Beyond(_)
        | ListedRow::Unheld(_)
        | ListedRow::Found(_) => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reaching {
    held: usize,
    whole: bool,
    unheld: usize,
    found: usize,
    asking: bool,
}

fn beyond_the_listing(reaching: Reaching) -> Vec<ListedRow> {
    let Reaching {
        held,
        whole,
        unheld,
        found,
        asking,
    } = reaching;
    if !whole || (unheld == 0 && found == 0 && !asking) {
        return Vec::new();
    }

    let mut listed: Vec<ListedRow> = (0..held).map(ListedRow::Held).collect();
    if unheld > 0 {
        listed.push(ListedRow::Beyond(Beyond::InTheCatalog(unheld)));
        listed.extend((0..unheld).map(ListedRow::Unheld));
    }
    if found > 0 {
        listed.push(ListedRow::Beyond(Beyond::Elsewhere(found)));
        listed.extend((0..found).map(ListedRow::Found));
    } else if asking {
        listed.push(ListedRow::Beyond(Beyond::Asking));
    }

    listed
}

pub(crate) enum AsDrawn {
    Listed,
    Album {
        release_tracks: Arc<[HeldReleaseTrack]>,
        arranging: Arranging,
    },
}

impl AsDrawn {
    pub(crate) fn ordered(&self, listing: Arc<[Track]>) -> Arc<[Track]> {
        let Self::Album {
            release_tracks,
            arranging,
        } = self
        else {
            return listing;
        };

        held_in(&album_rows(&listing, release_tracks, *arranging))
            .into_iter()
            .filter_map(|at| listing.get(at).cloned())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Arranging {
    sort: SortOrder,
    reading: Direction,
}

impl Arranging {
    const fn by_seat(self) -> bool {
        matches!(self.sort, SortOrder::Relevance | SortOrder::AlbumThenTrack)
    }
}

fn held_in(rows: &[ListedRow]) -> Vec<usize> {
    rows.iter()
        .filter_map(|row| match row {
            ListedRow::Held(held) => Some(*held),
            ListedRow::Disc(_)
            | ListedRow::Missing(_)
            | ListedRow::Beyond(_)
            | ListedRow::Unheld(_)
            | ListedRow::Found(_) => None,
        })
        .collect()
}

type Seat = (u32, u32);

fn album_rows(
    tracks: &[Track],
    release_tracks: &[HeldReleaseTrack],
    arranging: Arranging,
) -> Vec<ListedRow> {
    let placed: AHashMap<TrackId, Seat> = release_tracks
        .iter()
        .filter_map(|row| row.track.map(|track| (track, (row.disc, row.position))))
        .collect();

    let held = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let place = placed.get(&track.id).copied().unwrap_or((
                track.disc_number.unwrap_or(1),
                track.track_number.unwrap_or(u32::MAX),
            ));
            (place, ListedRow::Held(index))
        })
        .collect();
    let missing = release_tracks
        .iter()
        .enumerate()
        .filter(|(_, row)| row.track.is_none())
        .map(|(index, row)| ((row.disc, row.position), ListedRow::Missing(index)))
        .collect();

    arranged(held, missing, arranging)
}

fn arranged(
    held: Vec<(Seat, ListedRow)>,
    mut missing: Vec<(Seat, ListedRow)>,
    arranging: Arranging,
) -> Vec<ListedRow> {
    if !arranging.by_seat() {
        missing.sort_by_key(|(place, _)| *place);
        return held
            .into_iter()
            .chain(missing)
            .map(|(_, row)| row)
            .collect();
    }

    let mut rows = held;
    rows.append(&mut missing);
    rows.sort_by_key(|(place, _)| *place);
    if arranging.reading == Direction::Descending {
        rows.reverse();
    }
    headed_by_disc(rows)
}

fn headed_by_disc(rows: Vec<((u32, u32), ListedRow)>) -> Vec<ListedRow> {
    let discs: AHashSet<u32> = rows.iter().map(|((disc, _), _)| *disc).collect();
    if discs.len() < 2 {
        return rows.into_iter().map(|(_, row)| row).collect();
    }

    let mut listed = Vec::with_capacity(rows.len() + discs.len());
    let mut heading = None;
    for ((disc, _), row) in rows {
        if heading != Some(disc) {
            listed.push(ListedRow::Disc(disc));
            heading = Some(disc);
        }
        listed.push(row);
    }

    listed
}

fn missing_track_rows(albums: impl Iterator<Item = (AlbumId, u32)>) -> Vec<MissingRow> {
    let mut listed = Vec::new();
    headed_by_album_and_disc(albums, &mut listed);
    listed
}

fn unheld_release_rows(artists: impl Iterator<Item = ArtistId>) -> Vec<MissingRow> {
    let mut listed = Vec::new();
    headed_by_run(
        artists,
        MissingRow::Artist,
        MissingRow::Release,
        &mut listed,
    );
    listed
}

fn over_more_than_one_disc(run: &[(AlbumId, u32)]) -> bool {
    let Some((album, first)) = run.first().copied() else {
        return false;
    };

    run.iter()
        .take_while(|(held, _)| *held == album)
        .any(|(_, disc)| *disc != first)
}

fn headed_by_album_and_disc(
    albums: impl Iterator<Item = (AlbumId, u32)>,
    listed: &mut Vec<MissingRow>,
) {
    let placed: Vec<(AlbumId, u32)> = albums.collect();
    let mut under = None;
    let mut over = None;
    let mut discs_are_headed = false;
    for (index, (album, disc)) in placed.iter().copied().enumerate() {
        if under != Some(album) {
            listed.push(MissingRow::Album(index));
            under = Some(album);
            over = None;
            discs_are_headed = over_more_than_one_disc(&placed[index..]);
        }
        if discs_are_headed && over != Some(disc) {
            listed.push(MissingRow::Disc(index));
            over = Some(disc);
        }
        listed.push(MissingRow::Track(index));
    }
}

fn headed_by_run<K: PartialEq>(
    keys: impl Iterator<Item = K>,
    heading: fn(usize) -> MissingRow,
    row: fn(usize) -> MissingRow,
    listed: &mut Vec<MissingRow>,
) {
    let mut under = None;
    for (index, key) in keys.enumerate() {
        if under.as_ref() != Some(&key) {
            listed.push(heading(index));
            under = Some(key);
        }
        listed.push(row(index));
    }
}

fn read_in(imported: Imported) -> String {
    let mut said = format!(
        "{}: read {} as {}, added {}",
        imported.name,
        imported.format.name(),
        imported.encoding.name(),
        imported.added
    );
    if imported.already > 0 {
        said.push_str(&format!(", {} already in it", imported.already));
    }
    if imported.elsewhere > 0 {
        said.push_str(&format!(
            ", passed over {} naming no local file",
            imported.elsewhere
        ));
    }
    if imported.missing > 0 {
        said.push_str(&format!(
            ", {} naming a file that is not there",
            imported.missing
        ));
    }
    if imported.short > 0 {
        said.push_str(&format!(
            ", {} the sheet declared and did not hold",
            imported.short
        ));
    }
    said
}

#[derive(Default)]
struct Watching {
    watch: Option<RootsWatch>,
    tried: Option<Vec<PathBuf>>,
    there: Option<bool>,
    gone: Vec<PathBuf>,
    absent: Vec<PathBuf>,
    returned: Vec<PathBuf>,
}

impl Watching {
    fn following(mut self, library: &Library) -> Self {
        let roots = match library.roots() {
            Ok(roots) => roots,
            Err(error) => {
                tracing::debug!(%error, "the roots to watch could not be read");
                return self;
            }
        };
        let (present, absent): (Vec<PathBuf>, Vec<PathBuf>) =
            roots.into_iter().partition(|root| root.is_dir());
        self.there = Some(!present.is_empty());
        if self.tried.as_deref() == Some(present.as_slice()) {
            return self;
        }
        let Some(laid) = RootsWatch::over(&present) else {
            return self;
        };

        if self.tried.is_some() {
            let returned = self.absent.iter().filter(|root| present.contains(root));
            self.returned.extend(returned.cloned());
        }
        self.absent = absent;
        self.watch = Some(match self.watch.take() {
            Some(previous) => laid.taking_over(previous),
            None => laid,
        });
        self.tried = Some(present);
        self
    }

    fn came_back(&mut self) -> Vec<PathBuf> {
        mem::take(&mut self.returned)
    }

    const fn has_read_the_roots(&self) -> bool {
        self.there.is_some()
    }

    fn finds_a_root_there(&self) -> bool {
        self.there == Some(true)
    }

    fn settled(&self) -> Vec<PathBuf> {
        self.watch
            .as_ref()
            .map(|watch| watch.settled(ROOTS_QUIET_FOR))
            .unwrap_or_default()
    }

    fn forget_what_went(&mut self, library: &Library) -> u64 {
        if let Some(watch) = &self.watch {
            for path in watch.taken_away(GONE_QUIET_FOR) {
                if !self.gone.contains(&path) {
                    self.gone.push(path);
                }
            }
        }
        if self.gone.is_empty() {
            return 0;
        }

        match library.forget_the_gone(&self.gone) {
            Ok(forgotten) => {
                self.gone.clear();
                forgotten
            }
            Err(resonate_library::Error::AlreadyWalking) => 0,
            Err(error) => {
                tracing::warn!(%error, "the files taken away under the roots could not be forgotten yet");
                0
            }
        }
    }
}

fn walk(
    library: &Library,
    roots: Vec<PathBuf>,
    reading: Reading,
    prompted: Prompted,
) -> resonate_library::Result<Option<ScanHandle>> {
    let workers = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let options = ScanOptions {
        roots,
        incremental: reading == Reading::WhatChanged,
        follow_symlinks: false,
        extract_cover_art: true,
        workers,
    };
    match prompted {
        Prompted::OnItsOwn => library.scan_what_is_held(options),
        Prompted::ByHand | Prompted::ByTheInbox => library.scan(options).map(Some),
    }
}

fn load(library: &Library, asked: Asked, wanted: Wanted) -> resonate_library::Result<Loaded> {
    let Asked {
        text,
        album,
        artist,
        opened,
        order,
        reading,
        sorting,
        reach,
        window,
    } = asked;
    let narrowing = text.as_deref();

    let browsed = match wanted {
        Wanted::Everything => Some(browsed(
            library,
            album,
            artist,
            text.clone(),
            sorting,
            reach,
            window,
        )?),
        Wanted::ThePlaylists => None,
    };
    let (held, entries) = match opened {
        Some(opened) => (
            library.playlist(opened)?,
            library.playlist_entries(opened, narrowing)?,
        ),
        None => (None, Vec::new()),
    };
    let wanted = library
        .wants()?
        .into_iter()
        .map(|want| (want.release_track, want.id))
        .collect();

    Ok(Loaded {
        browsed,
        playlists: library.playlists(order, reading, narrowing)?,
        lists: library.playlist_lists(order, reading)?,
        held,
        entries,
        wanted,
        missing_tracks: library.missing_tracks(narrowing, Some(MISSING_AT_MOST))?,
        unheld_releases: library.unheld_releases(narrowing, Some(MISSING_AT_MOST))?,
        missing: library.missing_counted(narrowing)?,
    })
}

fn decoded_cover(library: &Library, id: AlbumId) -> Option<Art> {
    match library.cover_art(id) {
        Ok(art) => art.as_ref().map(Art::of),
        Err(error) => {
            tracing::warn!(%error, album = id.get(), "cover art could not be read");
            None
        }
    }
}

fn whole_cover_of(library: &Library, id: AlbumId) -> Option<Arc<Image>> {
    match library.cover_art(id) {
        Ok(art) => art.as_ref().map(whole_of),
        Err(error) => {
            tracing::warn!(%error, album = id.get(), "cover art could not be read");
            None
        }
    }
}

fn decoded_portrait(library: &Library, id: ArtistId) -> Option<Portrait> {
    match library.portrait(id) {
        Ok(art) => art.as_ref().map(Portrait::of),
        Err(error) => {
            tracing::warn!(%error, artist = id.get(), "a portrait could not be read");
            None
        }
    }
}

fn browsed(
    library: &Library,
    album: Option<AlbumId>,
    artist: Option<ArtistId>,
    text: Option<String>,
    sorting: Sorting,
    reach: usize,
    window: Window,
) -> resonate_library::Result<Browsed> {
    let listing = |album, artist, limit| TrackQuery {
        album,
        artist,
        text: text.clone(),
        sort: sorting.tracks,
        reading: sorting.tracks_read,
        limit,
        offset: 0,
    };
    let scoping = album.is_some() || artist.is_some();
    let albums_asked = AlbumQuery {
        artist: None,
        text: text.clone(),
        sort: sorting.albums,
        reading: sorting.albums_read,
        limit: Some(reach),
        offset: 0,
    };
    let artists_asked = ArtistQuery {
        text: text.clone(),
        sort: sorting.artists,
        reading: sorting.artists_read,
        limit: Some(reach),
        offset: 0,
    };
    let favourite_albums_asked = AlbumQuery {
        sort: AlbumOrder::Favourited,
        reading: AlbumOrder::Favourited.reads(),
        limit: Some(FAVOURITES_AT_MOST),
        ..albums_asked.clone()
    };
    let favourite_artists_asked = ArtistQuery {
        sort: ArtistOrder::Favourited,
        reading: ArtistOrder::Favourited.reads(),
        limit: Some(FAVOURITES_AT_MOST),
        ..artists_asked.clone()
    };
    let favourite_tracks_asked = TrackQuery {
        sort: SortOrder::Favourited,
        reading: SortOrder::Favourited.reads(),
        ..listing(None, None, Some(FAVOURITES_AT_MOST))
    };
    let tracks_measured = library.measured(&listing(None, None, None))?;
    let unheld = match text.as_deref() {
        Some(text) => library.unheld_matching(text, Some(UNHELD_MATCHED_AT_MOST))?,
        None => Vec::new(),
    };
    let sung = match text.as_deref() {
        Some(text) => library.sung(text)?,
        None => None,
    };
    let albums = library.albums(&albums_asked)?;
    let artists = library.artists(&artists_asked)?;
    let tracks = library.tracks(&listing(None, None, Some(reach)))?;
    let matched_nothing = albums.is_empty()
        && artists.is_empty()
        && tracks.is_empty()
        && unheld.is_empty()
        && sung.is_none();

    Ok(Browsed {
        instead: match text.as_deref().filter(|_| matched_nothing) {
            Some(text) => library.did_you_mean(text)?,
            None => None,
        },
        albums_counted: library.albums_counted(&albums_asked)?,
        artists_counted: library.artists_counted(&artists_asked)?,
        tracks_measured,
        scoped_measured: if scoping {
            library.measured(&listing(album, artist, None))?
        } else {
            tracks_measured
        },
        favourite_albums: library.favourite_albums(&favourite_albums_asked)?,
        favourite_artists: library.favourite_artists(&favourite_artists_asked)?,
        favourite_tracks: library.favourite_tracks(&favourite_tracks_asked)?,
        statistics: library.statistics(window)?,
        most_listened: library.most_listened(window, MOST_LISTENED)?,
        by_day: library.listening_by_day(window)?,
        suggestions: library.suggestions()?,
        unheld,
        sung,
        albums,
        artists,
        tracks,
        scoped: if scoping {
            library.tracks(&listing(album, artist, Some(reach)))?
        } else {
            Vec::new()
        },
        roots: library.roots()?,
        release_tracks: match album {
            Some(album) => library.release_tracks(album)?,
            None => Vec::new(),
        },
        release: match album {
            Some(album) => library.release_of(album)?,
            None => None,
        },
        artist: match artist {
            Some(artist) => library.artist_detail(artist)?,
            None => None,
        },
        artist_albums: match artist {
            Some(artist) => library.albums(&AlbumQuery {
                artist: Some(artist),
                text: None,
                sort: AlbumOrder::Year,
                reading: AlbumOrder::Year.reads(),
                limit: Some(ARTIST_ALBUMS),
                offset: 0,
            })?,
            None => Vec::new(),
        },
        artist_totals: match artist {
            Some(artist) => library.artist_totals(artist)?,
            None => ArtistTotals::default(),
        },
        album: match album {
            Some(album) => library.album(album)?,
            None => None,
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    InARow,
    NowPlaying,
    InAGrid,
}

impl Drawn {
    fn side(self) -> NonZeroU32 {
        let twice = |drawn: f32| side(2 * drawn as u32);

        match self {
            Self::InARow => twice(theme::row_cover()),
            Self::NowPlaying => twice(theme::now_playing_cover()),
            Self::InAGrid => twice(theme::grid_cover()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Portrayed {
    InARow,
    InAGrid,
}

impl Portrayed {
    const fn drawn(self) -> Drawn {
        match self {
            Self::InARow => Drawn::InARow,
            Self::InAGrid => Drawn::InAGrid,
        }
    }
}

struct Sizing<'a, F> {
    art: &'a CoverArt,
    drawing: Option<Drawing>,
    scaled: F,
    as_it_came: OnceCell<Arc<Image>>,
}

impl<'a, F: Fn(&Drawing, NonZeroU32) -> Option<CoverArt>> Sizing<'a, F> {
    fn of(art: &'a CoverArt, scaled: F) -> Self {
        Self {
            art,
            drawing: Drawing::of(art),
            scaled,
            as_it_came: OnceCell::new(),
        }
    }

    fn at(&self, drawn: Drawn) -> Arc<Image> {
        match self
            .drawing
            .as_ref()
            .and_then(|drawing| (self.scaled)(drawing, drawn.side()))
        {
            Some(drawn) => Arc::new(Image::from_bytes(painted(drawn.format), drawn.bytes)),
            None => Arc::clone(self.as_it_came.get_or_init(|| whole_of(self.art))),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Art {
    in_a_row: Arc<Image>,
    now_playing: Arc<Image>,
    in_a_grid: Arc<Image>,
}

impl Art {
    pub(crate) fn of(art: &CoverArt) -> Self {
        let sizing = Sizing::of(art, Drawing::no_larger_than);

        Self {
            in_a_row: sizing.at(Drawn::InARow),
            now_playing: sizing.at(Drawn::NowPlaying),
            in_a_grid: sizing.at(Drawn::InAGrid),
        }
    }

    pub(crate) fn drawn(&self, drawn: Drawn) -> Arc<Image> {
        match drawn {
            Drawn::InARow => Arc::clone(&self.in_a_row),
            Drawn::NowPlaying => Arc::clone(&self.now_playing),
            Drawn::InAGrid => Arc::clone(&self.in_a_grid),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Portrait {
    in_a_row: Arc<Image>,
    in_a_grid: Arc<Image>,
}

impl Portrait {
    pub(crate) fn of(art: &CoverArt) -> Self {
        let sizing = Sizing::of(art, Drawing::squared);

        Self {
            in_a_row: sizing.at(Portrayed::InARow.drawn()),
            in_a_grid: sizing.at(Portrayed::InAGrid.drawn()),
        }
    }

    pub(crate) fn portrayed(&self, portrayed: Portrayed) -> Arc<Image> {
        match portrayed {
            Portrayed::InARow => Arc::clone(&self.in_a_row),
            Portrayed::InAGrid => Arc::clone(&self.in_a_grid),
        }
    }
}

pub(crate) fn whole_of(art: &CoverArt) -> Arc<Image> {
    Arc::new(Image::from_bytes(painted(art.format), art.bytes.clone()))
}

pub(crate) enum Magnifying<K> {
    Reading(K),
    Read(K, Option<Arc<Image>>),
}

impl<K: PartialEq> Magnifying<K> {
    pub(crate) fn names(&self, wanted: &K) -> bool {
        match self {
            Self::Reading(named) | Self::Read(named, _) => named == wanted,
        }
    }

    pub(crate) fn whole(&self) -> Option<Arc<Image>> {
        match self {
            Self::Reading(_) => None,
            Self::Read(_, whole) => whole.clone(),
        }
    }
}

pub(crate) trait Forget {
    fn forget(self, cx: &mut App);
}

impl Forget for Arc<Image> {
    fn forget(self, cx: &mut App) {
        self.remove_asset(cx);
    }
}

impl<T: Forget> Forget for Option<T> {
    fn forget(self, cx: &mut App) {
        if let Some(held) = self {
            held.forget(cx);
        }
    }
}

impl<T: Forget> Forget for Leaving<T> {
    fn forget(self, cx: &mut App) {
        for held in self {
            held.forget(cx);
        }
    }
}

impl Forget for Art {
    fn forget(self, cx: &mut App) {
        for drawn in [self.in_a_row, self.now_playing, self.in_a_grid] {
            drawn.forget(cx);
        }
    }
}

impl Forget for Portrait {
    fn forget(self, cx: &mut App) {
        for drawn in [self.in_a_row, self.in_a_grid] {
            drawn.forget(cx);
        }
    }
}

impl<K> Forget for Magnifying<K> {
    fn forget(self, cx: &mut App) {
        match self {
            Self::Reading(_) => {}
            Self::Read(_, whole) => whole.forget(cx),
        }
    }
}

pub(crate) const fn painted(format: ImageFormat) -> gpui::ImageFormat {
    match format {
        ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        ImageFormat::Png => gpui::ImageFormat::Png,
        ImageFormat::Webp => gpui::ImageFormat::Webp,
        ImageFormat::Gif => gpui::ImageFormat::Gif,
        ImageFormat::Bmp => gpui::ImageFormat::Bmp,
    }
}

fn landed_since(folder: &Path, tried: SystemTime) -> bool {
    let Ok(listed) = fs::read_dir(folder) else {
        return false;
    };
    listed.flatten().any(|entry| {
        entry
            .metadata()
            .ok()
            .filter(fs::Metadata::is_file)
            .and_then(|held| {
                let changed = u64::try_from(held.ctime()).ok().map(|seconds| {
                    SystemTime::UNIX_EPOCH
                        + Duration::new(seconds, u32::try_from(held.ctime_nsec()).unwrap_or(0))
                });
                held.modified().ok().max(changed)
            })
            .is_some_and(|touched| touched > tried)
    })
}

#[cfg(test)]
mod tests {
    use resonate_core::{AlbumId, ArtistId};
    use resonate_library::{Direction, Library, SortOrder};

    use super::{
        Arranging, Beyond, Favourited, ListedRow, MissingRow, Pass, Planned, Reaching, Shared,
        arranged, beyond_the_listing, headed_by_disc, held_at, held_in, landed_since,
        missing_track_rows, on_the_clipboard, unheld_release_rows,
    };

    const SEARCHED: Reaching = Reaching {
        held: 2,
        whole: true,
        unheld: 1,
        found: 2,
        asking: false,
    };

    #[test]
    fn a_search_lists_what_the_catalog_lacks_and_what_was_found_elsewhere_after_what_it_holds() {
        let rows = beyond_the_listing(SEARCHED);

        assert_eq!(
            rows,
            vec![
                ListedRow::Held(0),
                ListedRow::Held(1),
                ListedRow::Beyond(Beyond::InTheCatalog(1)),
                ListedRow::Unheld(0),
                ListedRow::Beyond(Beyond::Elsewhere(2)),
                ListedRow::Found(0),
                ListedRow::Found(1),
            ]
        );
        assert_eq!(held_at(&rows, 1), Some(1));
        assert_eq!(held_at(&rows, 2), None);
        assert_eq!(held_at(&rows, 5), None);
    }

    #[test]
    fn nothing_is_listed_beyond_a_listing_until_the_whole_of_it_has_been_read() {
        assert!(
            beyond_the_listing(Reaching {
                whole: false,
                ..SEARCHED
            })
            .is_empty()
        );
        assert!(
            beyond_the_listing(Reaching {
                unheld: 0,
                found: 0,
                ..SEARCHED
            })
            .is_empty()
        );
    }

    #[test]
    fn a_search_still_being_asked_elsewhere_says_so_under_what_the_catalog_answered() {
        let rows = beyond_the_listing(Reaching {
            held: 0,
            unheld: 0,
            found: 0,
            asking: true,
            ..SEARCHED
        });

        assert_eq!(rows, vec![ListedRow::Beyond(Beyond::Asking)]);
    }

    #[test]
    fn a_name_held_under_an_id_answers_only_for_the_row_it_was_read_for() {
        use resonate_core::{FrameSpan, Frames, MediaLocation, TrackId};
        use resonate_engine::QueueItem;

        use super::Named;

        let id = TrackId::new(u64::MAX).expect("a non-zero id");
        let held = Named {
            location: MediaLocation::local("/music/first.flac"),
            span: None,
            track: None,
        };
        let row = |path: &str, span| QueueItem {
            id,
            location: MediaLocation::local(path),
            span,
        };

        assert!(held.names(&row("/music/first.flac", None)));
        assert!(!held.names(&row("/music/second.flac", None)));
        assert!(!held.names(&row(
            "/music/first.flac",
            Some(FrameSpan::starting(Frames(44_100)))
        )));
    }

    #[test]
    fn a_row_of_an_album_names_the_track_it_draws_and_a_heading_names_none() {
        let rows = [
            ListedRow::Disc(1),
            ListedRow::Held(0),
            ListedRow::Missing(0),
            ListedRow::Disc(2),
            ListedRow::Held(1),
        ];

        assert_eq!(held_at(&rows, 0), None);
        assert_eq!(held_at(&rows, 1), Some(0));
        assert_eq!(held_at(&rows, 2), None);
        assert_eq!(held_at(&rows, 4), Some(1));
        assert_eq!(held_at(&rows, 5), None);
    }

    fn placed(rows: &[(u32, u32, usize)]) -> Vec<((u32, u32), ListedRow)> {
        rows.iter()
            .map(|(disc, position, held)| ((*disc, *position), ListedRow::Held(*held)))
            .collect()
    }

    const BY_SEAT: Arranging = Arranging {
        sort: SortOrder::AlbumThenTrack,
        reading: Direction::Ascending,
    };

    #[test]
    fn an_album_in_its_own_order_is_drawn_and_played_by_its_seats() {
        let held = placed(&[(1, 3, 0), (1, 1, 1)]);
        let missing = vec![((1, 2), ListedRow::Missing(0))];

        let rows = arranged(held, missing, BY_SEAT);
        assert_eq!(
            rows,
            vec![
                ListedRow::Held(1),
                ListedRow::Missing(0),
                ListedRow::Held(0),
            ]
        );
        assert_eq!(
            held_in(&rows),
            vec![1, 0],
            "an album was played in an order other than the one drawn"
        );
    }

    #[test]
    fn an_album_turned_round_is_drawn_from_its_last_seat() {
        let held = placed(&[(1, 1, 0), (2, 1, 1)]);
        let reversed = Arranging {
            reading: Direction::Descending,
            ..BY_SEAT
        };

        assert_eq!(
            arranged(held, Vec::new(), reversed),
            vec![
                ListedRow::Disc(2),
                ListedRow::Held(1),
                ListedRow::Disc(1),
                ListedRow::Held(0),
            ]
        );
    }

    #[test]
    fn an_album_sorted_another_way_is_drawn_in_that_sort_with_what_it_lacks_after() {
        let held = placed(&[(1, 3, 0), (1, 1, 1)]);
        let missing = vec![
            ((1, 4), ListedRow::Missing(1)),
            ((1, 2), ListedRow::Missing(0)),
        ];
        let by_title = Arranging {
            sort: SortOrder::Title,
            reading: Direction::Ascending,
        };

        assert_eq!(
            arranged(held, missing, by_title),
            vec![
                ListedRow::Held(0),
                ListedRow::Held(1),
                ListedRow::Missing(0),
                ListedRow::Missing(1),
            ]
        );
    }

    #[test]
    fn an_album_on_one_disc_is_listed_without_a_heading_over_it() {
        let rows = placed(&[(1, 1, 0), (1, 2, 1), (1, 3, 2)]);

        assert_eq!(
            headed_by_disc(rows),
            vec![ListedRow::Held(0), ListedRow::Held(1), ListedRow::Held(2)]
        );
    }

    #[test]
    fn a_set_of_two_discs_carries_a_heading_over_each_run_of_rows() {
        let rows = placed(&[(1, 1, 0), (1, 2, 1), (2, 1, 2), (2, 2, 3)]);

        assert_eq!(
            headed_by_disc(rows),
            vec![
                ListedRow::Disc(1),
                ListedRow::Held(0),
                ListedRow::Held(1),
                ListedRow::Disc(2),
                ListedRow::Held(2),
                ListedRow::Held(3),
            ]
        );
    }

    #[test]
    fn a_disc_the_rows_skip_is_headed_by_the_number_the_rows_carry() {
        let rows = placed(&[(1, 1, 0), (3, 1, 1)]);

        assert_eq!(
            headed_by_disc(rows),
            vec![
                ListedRow::Disc(1),
                ListedRow::Held(0),
                ListedRow::Disc(3),
                ListedRow::Held(1),
            ]
        );
    }

    #[test]
    fn a_missing_row_is_headed_the_way_a_held_one_is() {
        let rows = vec![
            ((1, 1), ListedRow::Held(0)),
            ((2, 1), ListedRow::Missing(0)),
        ];

        assert_eq!(
            headed_by_disc(rows),
            vec![
                ListedRow::Disc(1),
                ListedRow::Held(0),
                ListedRow::Disc(2),
                ListedRow::Missing(0),
            ]
        );
    }

    fn album(id: u64) -> AlbumId {
        AlbumId::new(id).expect("a non-zero id")
    }

    fn artist(id: u64) -> ArtistId {
        ArtistId::new(id).expect("a non-zero id")
    }

    #[test]
    fn each_run_of_an_albums_missing_tracks_is_headed_by_the_album_once() {
        let rows = missing_track_rows(
            [
                (album(1), 1),
                (album(1), 1),
                (album(2), 1),
                (album(2), 1),
                (album(2), 1),
            ]
            .into_iter(),
        );

        assert_eq!(
            rows,
            vec![
                MissingRow::Album(0),
                MissingRow::Track(0),
                MissingRow::Track(1),
                MissingRow::Album(2),
                MissingRow::Track(2),
                MissingRow::Track(3),
                MissingRow::Track(4),
            ]
        );
    }

    #[test]
    fn each_run_of_an_artists_unheld_releases_is_headed_by_the_artist_once() {
        let rows = unheld_release_rows([artist(3), artist(3), artist(5)].into_iter());

        assert_eq!(
            rows,
            vec![
                MissingRow::Artist(0),
                MissingRow::Release(0),
                MissingRow::Release(1),
                MissingRow::Artist(2),
                MissingRow::Release(2),
            ]
        );
    }

    #[test]
    fn an_album_missing_rows_from_two_discs_is_headed_by_each_of_them() {
        let rows = missing_track_rows(
            [(album(1), 1), (album(1), 2), (album(1), 2), (album(2), 3)].into_iter(),
        );

        assert_eq!(
            rows,
            vec![
                MissingRow::Album(0),
                MissingRow::Disc(0),
                MissingRow::Track(0),
                MissingRow::Disc(1),
                MissingRow::Track(1),
                MissingRow::Track(2),
                MissingRow::Album(3),
                MissingRow::Track(3),
            ]
        );
    }

    #[test]
    fn a_catalog_short_of_nothing_lists_no_row_and_no_heading() {
        assert!(missing_track_rows(std::iter::empty()).is_empty());
        assert!(unheld_release_rows(std::iter::empty()).is_empty());
    }

    #[test]
    fn an_album_listed_twice_apart_is_headed_twice_because_the_rows_arrive_grouped() {
        let rows = missing_track_rows([(album(1), 1), (album(2), 1), (album(1), 1)].into_iter());

        assert_eq!(
            rows,
            vec![
                MissingRow::Album(0),
                MissingRow::Track(0),
                MissingRow::Album(1),
                MissingRow::Track(1),
                MissingRow::Album(2),
                MissingRow::Track(2),
            ]
        );
    }

    #[test]
    fn the_sidebar_counts_every_favourite_of_all_three_kinds() {
        let held = Favourited {
            artists: 2,
            albums: 1,
            tracks: 9,
        };

        assert_eq!(held.held(), 12);
        assert_eq!(Favourited::default().held(), 0);
    }

    #[test]
    fn a_favourites_pane_with_nothing_kept_in_it_is_empty() {
        assert!(Favourited::default().is_empty());
        assert!(
            !Favourited {
                artists: 0,
                albums: 0,
                tracks: 1,
            }
            .is_empty()
        );
    }

    #[test]
    fn the_favourites_heading_counts_all_three_in_the_order_they_are_stacked() {
        let held = Favourited {
            artists: 2,
            albums: 1,
            tracks: 40,
        };

        assert_eq!(held.counted(), "2 artists · 1 album · 40 tracks");
    }

    #[test]
    fn the_favourites_heading_leaves_out_what_there_is_none_of() {
        let held = Favourited {
            artists: 0,
            albums: 0,
            tracks: 3,
        };

        assert_eq!(held.counted(), "3 tracks");
        assert_eq!(Favourited::default().counted(), "");
    }

    #[test]
    fn what_a_share_put_on_the_clipboard_is_named_by_its_artist_and_title() {
        let shared = Shared {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            ..Shared::default()
        };

        assert_eq!(
            on_the_clipboard(&shared),
            "Pink Floyd — Echoes is on the clipboard"
        );
    }

    #[test]
    fn a_share_of_a_track_with_no_artist_is_named_by_its_title_alone() {
        let shared = Shared {
            title: "Track 07".to_owned(),
            ..Shared::default()
        };

        assert_eq!(on_the_clipboard(&shared), "Track 07 is on the clipboard");
    }

    #[test]
    fn a_file_dropped_in_the_inbox_after_the_last_poll_is_what_the_window_opens_to_ask_about() {
        let folder = std::env::temp_dir().join(format!("resonate-inbox-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("nested")).expect("a writable temporary folder");
        let before = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        let after = std::time::SystemTime::now() + std::time::Duration::from_secs(60);

        assert!(
            !landed_since(&folder, before),
            "a folder holding only a folder"
        );
        std::fs::write(folder.join("an-mbid.flac"), b"audio").expect("a file dropped in");

        assert!(landed_since(&folder, before));
        assert!(!landed_since(&folder, after));

        let kept_its_time = folder.join("an-isrc.flac");
        std::fs::write(&kept_its_time, b"audio").expect("a file copied in");
        std::fs::File::options()
            .write(true)
            .open(&kept_its_time)
            .and_then(|file| file.set_modified(before - std::time::Duration::from_secs(3_600)))
            .expect("the copy keeps the time it had elsewhere");
        std::fs::remove_file(folder.join("an-mbid.flac")).expect("the first file goes");
        assert!(
            landed_since(&folder, before),
            "a file copied in with an old modification time was not seen landing"
        );
        assert!(!landed_since(&folder.join("gone"), before));
        std::fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }

    #[test]
    fn a_preview_stands_until_what_it_read_moves_and_an_apply_leaves_none() {
        let library = Library::open_in_memory().expect("an in-memory catalog");
        let read_at = library.plans_stamp();

        assert_eq!(Planned::after(Pass::Apply, false, read_at), Planned::Not);
        assert_eq!(Planned::after(Pass::Preview, true, read_at), Planned::Not);
        let shown = Planned::after(Pass::Preview, false, read_at);
        assert!(shown.is_shown());
        assert!(!shown.outdated_at(library.plans_stamp()));

        library
            .add_root(&std::env::temp_dir())
            .expect("the catalog takes a root");
        assert!(
            shown.outdated_at(library.plans_stamp()),
            "a root added under a preview left it standing"
        );
        assert!(!Planned::Outdated.outdated_at(library.plans_stamp()));
        assert!(!Planned::Not.outdated_at(library.plans_stamp()));
    }
}
