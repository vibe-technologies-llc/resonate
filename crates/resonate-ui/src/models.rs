mod arranged;
mod asking;
mod deleting;
mod detail;
mod downloading;
mod enriching;
mod favours;
mod filing;
mod found;
mod heard;
mod holding;
mod importing;
mod keeping;
mod linked;
mod listing;
mod passes;
mod pictures;
mod playlists;
mod polling;
mod pressings;
mod reading;
mod scanning;
mod sharing;
mod statistics;
mod suggested;
mod switches;
#[cfg(test)]
mod tests;
mod told;
mod undoing;
mod versions;
mod wanting;
mod wants;
mod watched;
mod watching;

use std::{
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    thread::{self},
    time::{Duration, Instant, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use crossbeam_channel::{Receiver, bounded};
use futures_channel::mpsc::UnboundedSender;
use gpui::{App, Context, Task};
use resonate_core::{
    AlbumId, ArtistId, MediaLocation, PlaylistId, ReleaseTrackId, TrackId, WantId,
};
use resonate_library::{
    Album, AlbumFound, AlbumMatch, AlbumNotHeld, AlbumOrder, Artist, ArtistDetail, ArtistFound,
    ArtistOrder, ArtistTotals, CatalogStamp, Copies, Day, DeliveryFolder, Direction,
    EnrichProgress, EnrichSummary, Favoured, Fingerprinters, Found, GroupRelease, Habits,
    HeldReleaseTrack, HiddenTracks, ImportProgress, ImportSummary, KeepStopped, KeepSummary,
    KeptList, Layout, Learning, Library, Listen, Mbid, Meant, Measured, Missing, MissingTrack,
    MostListened, NamedPlaylist, OrganiseProgress, OrganiseSummary, Playlist, PlaylistEntry,
    PlaylistOrder, PollOptions, PollProgress, PollStats, RecordingMatch, Reference, ReleaseDetail,
    RetagProgress, RetagSummary, SavedQuery, ScanProgress, Search, SongsAsked, SortOrder, Sought,
    Statistics, Suggestion, Sung, Track, UnheldRelease, Window,
};
use resonate_providers::WhyAway;

pub use self::pictures::{Drawn, Portrayed, Scale};
use self::{arranged::Named, watched::load};
pub(crate) use self::{
    arranged::queued_rows,
    pictures::{AtSide, Forget, Magnifying, Picture, drawn_within, magnified_of, picture_of},
};
use crate::{
    ResonateApp,
    downloads::{Downloads, Unfinished, WantStanding},
    format,
    held::Rows,
    keys,
    recent::Recent,
    settings::{Online, Sourcing},
    toast,
    views::statistics::Chart,
    words,
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
const PREVIEWED_AT_MOST: usize = 500;
const FAVOURITES_AT_MOST: usize = 5_000;
const MOST_LISTENED: usize = 10;
const COVERS_HELD: NonZeroUsize = held(512);
const COVERS_WARMED: usize = 256;
const NAMES_HELD: NonZeroUsize = held(4_096);
const ALBUMS_HELD: NonZeroUsize = held(256);
const PORTRAITS_HELD: NonZeroUsize = held(256);
const DECODES_AT_ONCE: usize = 4;
const DECODES_KEPT_FOR_WHAT_IS_SHOWN: usize = 1;
const RELEASED_COVERS_HELD: NonZeroUsize = held(512);
const ANSWERS_HELD: NonZeroUsize = held(128);
const FETCHES_AT_ONCE: usize = 6;

const A_FAILED_COVER_IS_ASKED_AGAIN_AFTER: Duration = Duration::from_secs(60);

const OWED_ASKED_AGAIN_AFTER: Duration = Duration::from_secs(30);

const SCAN_POLL: Duration = Duration::from_millis(100);

const A_QUIET_LOOKUP_SHOWS_AFTER: Duration = Duration::from_secs(3);
const SEARCH_SETTLE: Duration = Duration::from_millis(150);
const ASKED_ELSEWHERE_AFTER: Duration = Duration::from_millis(450);
const POLLS_PER_RELOAD: u32 = 20;
const POLLS_PER_REREAD_WHILE_ASKING: u32 = 5;
const FIRST_ASKED_AFTER: Duration = Duration::from_secs(60);
const ASKED_EVERY: Duration = Duration::from_secs(30 * 60);
const RETRIES_ASKED_AT_LEAST: Duration = Duration::from_secs(30);
const UNHEARD_RETRIES_DOUBLE_AT_MOST: u32 = 5;
const WATCHED_EVERY: Duration = Duration::from_secs(2);
const ROOTS_LOOKED_AT_EVERY: Duration = Duration::from_secs(1);
const ROOTS_LOOKED_AT_EVERY_UNSEEN: Duration = Duration::from_secs(60);
const ROOTS_READ_EVERY: Duration = Duration::from_secs(5);
const ROOTS_QUIET_FOR: Duration = Duration::from_secs(2);
const UNCOVERED_ROOTS_TRIED_AGAIN_AFTER: Duration = Duration::from_secs(5 * 60);
const INBOX_LOOKED_AT_EVERY: Duration = Duration::from_secs(1);

const LINKS_LOOKED_AT_FIRST_AFTER: Duration = Duration::from_secs(60);

const LINKS_LOOKED_AT_EVERY: Duration = Duration::from_secs(10 * 60);
const INBOX_QUIET_FOR: Duration = Duration::from_secs(2);
const GONE_QUIET_FOR: Duration = Duration::from_millis(250);

pub const PICTURED_AT_MOST: usize = 4;

pub const PINNED_IN_THE_SIDEBAR: usize = 5;

fn unnamed_playlist(id: PlaylistId) -> String {
    words::models::unnamed_playlist(id.to_string())
}

fn put_it_back() -> String {
    keys::keyed(words::models::put_it_back(), &[key!(undo)])
}

fn read_further(read: usize) -> String {
    match read {
        0 => words::models::the_rest_held_nothing().to_owned(),
        count => words::models::read_more_of_the_discography(count),
    }
}

const KEEP_THREAD: &str = "resonate-keep";
const KEEPS_LOOKED_AT_EVERY: Duration = Duration::from_secs(1);

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
    paged: Paged,
    favourite_albums: Vec<Album>,
    favourite_artists: Vec<Artist>,
    favourite_tracks: Vec<Track>,
    standing: Option<Standing>,
    instead: Option<String>,
    sung: Option<Sung>,
    kept_songs: Vec<Found>,
    kept_albums: Vec<AlbumFound>,
    kept_for: Option<String>,
    songs_not_held: Vec<Found>,
    albums_not_held: Vec<AlbumNotHeld>,
    albums_counted: u32,
    artists_counted: u32,
    tracks_measured: Measured,
    scoped_measured: Measured,
    release_tracks: Vec<HeldReleaseTrack>,
    release: Option<ReleaseDetail>,
    artist: Option<ArtistDetail>,
    artist_albums: Vec<Album>,
    artist_totals: ArtistTotals,
    album: Option<Album>,
    gone: Option<Selection>,
}

struct Paged {
    albums: Vec<Album>,
    artists: Vec<Artist>,
    tracks: Vec<Track>,
    scoped: Vec<Track>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Held {
    albums: usize,
    artists: usize,
    tracks: Reached,
    scoped: Reached,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Reached {
    rows: usize,
    last: Option<TrackId>,
}

impl Reached {
    fn of(held: &Rows<Track>) -> Self {
        Self {
            rows: held.len(),
            last: held.last().map(|track| track.id),
        }
    }
}

struct Grown {
    from: Held,
    page: Paged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grew {
    Same,
    Renewed,
    Moved,
}

impl Grew {
    const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Moved, _) | (_, Self::Moved) => Self::Moved,
            (Self::Renewed, _) | (_, Self::Renewed) => Self::Renewed,
            (Self::Same, Self::Same) => Self::Same,
        }
    }
}

struct Standing {
    statistics: Statistics,
    heard: Option<Heard>,
    suggestions: Arc<[Suggestion]>,
    roots: Vec<PathBuf>,
    hidden: HiddenTracks,
}

struct Heard {
    most_listened: MostListened,
    by_day: Vec<Day>,
    habits: Habits,
}

pub const HIDDEN_LISTED_AT_MOST: usize = 200;

struct Loaded {
    browsed: Option<Browsed>,
    grown: Option<Grown>,
    shelves: Option<Shelves>,
    meant: Option<Meant>,
}

struct Shelves {
    playlists: Vec<Playlist>,
    lists: Vec<Playlist>,
    pictured: AHashMap<PlaylistId, Arc<[AlbumId]>>,
    pinned: Vec<NamedPlaylist>,
    held: Option<Playlist>,
    entries: Vec<PlaylistEntry>,
    wants: Option<WantsRead>,
    missing_tracks: Vec<MissingTrack>,
    unheld_releases: Vec<UnheldRelease>,
    missing: Missing,
    dismissed: Missing,
}

struct WantsRead {
    wanted: AHashMap<ReleaseTrackId, WantId>,
    standings: AHashMap<WantId, WantStanding>,
    unfinished: Vec<Unfinished>,
    next_try: Option<SystemTime>,
}

impl WantsRead {
    fn of(wants: &[resonate_library::Want]) -> Self {
        Self {
            wanted: wants
                .iter()
                .map(|want| (want.release_track, want.id))
                .collect(),
            standings: wants
                .iter()
                .map(|want| (want.id, WantStanding::of(want)))
                .collect(),
            unfinished: wants.iter().rev().filter_map(Unfinished::of).collect(),
            next_try: wants
                .iter()
                .filter_map(resonate_library::Want::due_at)
                .min(),
        }
    }
}

#[derive(Clone)]
struct Answer {
    songs: Arc<[RecordingMatch]>,
    albums: Arc<[AlbumMatch]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    InPart,
    Whole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListedRow {
    Disc(u32),
    Held(usize),
    Missing(usize),
    NotHeld(usize),
    Found(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beyond {
    Elsewhere(usize),
    Refining(usize),
    Asking,
    Unreached(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pressings {
    Asking,
    Listed(Arc<[GroupRelease]>),
    Unanswered,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Previewed {
    pub query: SavedQuery,
    pub tracks: Arc<[Track]>,
    asked: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingRow {
    Album(usize),
    Disc(usize),
    Track(usize),
    Artist(usize),
    Release(usize),
}

type SaidOfACount = fn(usize) -> String;

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
        let counts: [(usize, SaidOfACount); 3] = [
            (self.artists, words::models::artists_counted),
            (self.albums, words::models::albums_counted),
            (self.tracks, words::models::tracks_counted),
        ];
        counts
            .into_iter()
            .filter(|(held, _)| *held > 0)
            .map(|(held, said)| said(held))
            .collect::<format::Parts<String>>()
            .join(" · ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    Everything,
    TheSearch,
    ThePage,
    ThePlaylists,
}

impl Wanted {
    const fn with(self, other: Self) -> Self {
        match (self, other) {
            (Self::Everything, _) | (_, Self::Everything) => Self::Everything,
            (Self::ThePage, Self::ThePage) => Self::ThePage,
            (Self::ThePlaylists, Self::ThePlaylists) => Self::ThePlaylists,
            (Self::TheSearch | Self::ThePage, Self::TheSearch | Self::ThePage) => Self::TheSearch,
            (Self::ThePlaylists, _) | (_, Self::ThePlaylists) => Self::Everything,
        }
    }

    const fn reads_the_wants(self) -> bool {
        matches!(self, Self::Everything | Self::ThePlaylists)
    }

    const fn reads_the_listing(self) -> bool {
        matches!(self, Self::Everything | Self::TheSearch)
    }

    const fn reads_what_stands(self) -> bool {
        matches!(self, Self::Everything)
    }

    const fn reads_the_shelves(self) -> bool {
        !matches!(self, Self::ThePage)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Playlist,
    Want,
    Unwant,
    Dismiss,
    BringBack,
    Favour { favourite: bool },
    Hide { hidden: bool },
    ForgetDelivered,
    ForgetTheMatch,
    TakePressing,
    PlaceOn,
    TakeRecording,
    GiveIsrc,
    LookUp,
    ReadTheRest,
    ChooseVersion,
    ForgetTheVersionChosen,
    LookForMissingCovers,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edited {
    Landed,
    Failed,
}

impl Change {
    fn doing(self) -> &'static str {
        match self {
            Self::Playlist => words::models::change_the_playlist(),
            Self::Want => words::models::want_that_track(),
            Self::Unwant => words::models::stop_wanting_that_track(),
            Self::Dismiss => words::models::dismiss_that(),
            Self::BringBack => words::models::bring_back_what_was_dismissed(),
            Self::Favour { favourite: true } => words::models::mark_it_a_favourite(),
            Self::Favour { favourite: false } => words::models::take_it_out_of_the_favourites(),
            Self::Hide { hidden: true } => words::models::hide_that_from_the_library(),
            Self::Hide { hidden: false } => words::models::show_that_in_the_library_again(),
            Self::ForgetDelivered => words::models::forget_that_delivery(),
            Self::ForgetTheMatch => words::models::forget_that_match(),
            Self::TakePressing => words::models::take_that_pressing(),
            Self::PlaceOn => words::models::place_the_track_on_that_release(),
            Self::TakeRecording => words::models::treat_the_song_as_that_recording(),
            Self::GiveIsrc => words::models::give_the_track_that_isrc(),
            Self::LookUp => words::models::look_the_track_up(),
            Self::ReadTheRest => words::models::read_the_rest_of_the_discography(),
            Self::ChooseVersion => words::models::keep_that_version(),
            Self::ForgetTheVersionChosen => words::models::forget_the_version_chosen(),
            Self::LookForMissingCovers => words::models::look_for_the_missing_covers_again(),
            Self::Undo => words::models::put_that_back(),
            Self::Redo => words::models::do_that_again(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Asked {
    text: Option<String>,
    meant: Option<String>,
    as_typed: bool,
    album: Option<AlbumId>,
    artist: Option<ArtistId>,
    opened: Option<PlaylistId>,
    order: PlaylistOrder,
    reading: Direction,
    sorting: Sorting,
    reach: usize,
    held: Held,
    window: Window,
    heard: bool,
}

impl Asked {
    fn at_first() -> Self {
        Self {
            text: None,
            meant: None,
            as_typed: false,
            album: None,
            artist: None,
            opened: None,
            order: PlaylistOrder::default(),
            reading: PlaylistOrder::default().reads(),
            sorting: Sorting::default(),
            reach: PAGE,
            held: Held::default(),
            window: Window::default(),
            heard: false,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sorting {
    pub tracks: SortOrder,
    pub tracks_read: Direction,
    pub albums: AlbumOrder,
    pub albums_read: Direction,
    pub artists: ArtistOrder,
    pub artists_read: Direction,
    pub favourites: SortOrder,
    pub favourites_read: Direction,
}

impl Default for Sorting {
    fn default() -> Self {
        Self {
            tracks: SortOrder::DateAdded,
            tracks_read: SortOrder::DateAdded.reads(),
            albums: AlbumOrder::default(),
            albums_read: AlbumOrder::default().reads(),
            artists: ArtistOrder::default(),
            artists_read: ArtistOrder::default().reads(),
            favourites: SortOrder::Favourited,
            favourites_read: SortOrder::Favourited.reads(),
        }
    }
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

#[derive(Clone, Copy, Debug)]
struct LookupAsked {
    prompted: Prompted,
    began: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    WhatChanged,
    Everything,
}

struct ToFile {
    layout: Layout,
    files: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootWaiting {
    ToAdd(PathBuf),
    ToForget(PathBuf),
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
    Identical(Pass),
}

impl Work {
    const fn is_busy(&self) -> bool {
        !matches!(self, Self::Nothing)
    }

    const fn identical(&self) -> Option<Pass> {
        match self {
            Self::Identical(pass) => Some(*pass),
            Self::Nothing
            | Self::Scanning(_)
            | Self::Organising(..)
            | Self::Tagging(..)
            | Self::Importing(..)
            | Self::Polling(_)
            | Self::Forgetting => None,
        }
    }

    const fn scanning(&self) -> Option<&Arc<ScanProgress>> {
        match self {
            Self::Scanning(progress) => Some(progress),
            Self::Nothing
            | Self::Organising(..)
            | Self::Tagging(..)
            | Self::Importing(..)
            | Self::Polling(_)
            | Self::Forgetting
            | Self::Identical(_) => None,
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
            | Self::Forgetting
            | Self::Identical(_) => None,
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
            | Self::Forgetting
            | Self::Identical(_) => None,
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
            | Self::Forgetting
            | Self::Identical(_) => None,
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
            | Self::Forgetting
            | Self::Identical(_) => None,
        }
    }
}

struct KeepPass {
    stop: Arc<AtomicBool>,
    left: u64,
}

#[derive(Clone)]
pub struct Consulted {
    pub reference: Option<Arc<dyn Reference>>,
    pub fingerprinters: Arc<Fingerprinters>,
}

struct PageLearning {
    told: UnboundedSender<usize>,
}

impl Learning for PageLearning {
    fn abandoned(&self) -> bool {
        self.told.is_closed()
    }

    fn landed(&self, songs: usize) {
        let _ = self.told.unbounded_send(songs);
    }
}

pub struct LibraryModel {
    first_read: Option<FirstRead>,
    unreadable: bool,
    listed_for: String,
    library: Arc<Library>,
    reference: Option<Arc<dyn Reference>>,
    for_the_pass: Consulted,
    online: bool,
    after_scan: bool,
    studies: bool,
    lyrics: bool,
    resume: bool,
    albums: Rows<Album>,
    artists: Rows<Artist>,
    tracks: Rows<Track>,
    favourite_albums: Arc<[Album]>,
    favourite_artists: Arc<[Artist]>,
    favourite_tracks: Arc<[Track]>,
    statistics: Statistics,
    most_listened: Arc<MostListened>,
    by_day: Arc<[Day]>,
    habits: Arc<Habits>,
    statistics_shown: bool,
    heard_owed: bool,
    suggestions: Arc<[Suggestion]>,
    opened_suggestion: Option<Previewed>,
    window: Window,
    scoped: Rows<Track>,
    rows: Arc<[ListedRow]>,
    sung: Option<Sung>,
    found: Arc<[Found]>,
    found_for: Option<String>,
    found_in_part: bool,
    narrowed: Arc<[Found]>,
    kept_songs: Arc<[Found]>,
    kept_albums: Arc<[AlbumFound]>,
    kept_for: Option<String>,
    songs_not_held: Arc<[Found]>,
    albums_not_held: Arc<[AlbumNotHeld]>,
    albums_wanted: AHashMap<Mbid, Task<()>>,
    albums_opening: AHashSet<Mbid>,
    artists_found: Arc<[ArtistFound]>,
    albums_found: Arc<[AlbumFound]>,
    albums_shown: Arc<[AlbumFound]>,
    artists_opening: AHashSet<Mbid>,
    albums_missing_tracks_wanted: AHashSet<AlbumId>,
    album_songs: AHashMap<Mbid, Vec<Mbid>>,
    shown: Arc<[Found]>,
    asking: Option<String>,
    answers: Recent<SongsAsked, Answer>,
    meant: Option<Meant>,
    as_typed: bool,
    reaching_out: Option<SongsAsked>,
    owed: Option<String>,
    unreached_for: Option<String>,
    providers_unheard: bool,
    unheard_in_a_row: u32,
    wanting: AHashMap<Mbid, Task<()>>,
    release_tracks: Arc<[HeldReleaseTrack]>,
    release: Option<ReleaseDetail>,
    artist: Option<ArtistDetail>,
    artist_albums: Arc<[Album]>,
    artist_totals: ArtistTotals,
    album: Option<Album>,
    wanted: AHashMap<ReleaseTrackId, WantId>,
    standings: AHashMap<WantId, WantStanding>,
    missing_tracks: Arc<[MissingTrack]>,
    unheld_releases: Arc<[UnheldRelease]>,
    missing: Missing,
    dismissed: Missing,
    missing_track_rows: Arc<[MissingRow]>,
    unheld_release_rows: Arc<[MissingRow]>,
    roots: Vec<PathBuf>,
    hidden: Arc<HiddenTracks>,
    playlists: Arc<[Playlist]>,
    lists: Arc<[Playlist]>,
    pictured: AHashMap<PlaylistId, Arc<[AlbumId]>>,
    pinned: Arc<[NamedPlaylist]>,
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
    polling_every_provider: bool,
    organised: Option<(Pass, OrganiseSummary)>,
    relocated: Vec<(MediaLocation, MediaLocation)>,
    arrived: Option<TrackId>,
    walks_back: bool,
    tags_walk_back: bool,
    previewed: Planned,
    imported: Option<(Pass, ImportSummary)>,
    previewed_import: Planned,
    tagged: Option<(Pass, RetagSummary)>,
    previewed_tags: Planned,
    identical: Option<Arc<Copies>>,
    previewed_identical: Planned,
    sourcing: Sourcing,
    enriching: Option<Arc<EnrichProgress>>,
    lookup_asked: Option<LookupAsked>,
    enrich_owed: bool,
    looked_up: Option<EnrichSummary>,
    sought: Arc<Sought>,
    reach: usize,
    page_moved: bool,
    playing: bool,
    albums_counted: u32,
    artists_counted: u32,
    tracks_measured: Measured,
    scoped_measured: Measured,
    revision: u64,
    album_at: AHashMap<AlbumId, usize>,
    favourite_album_ids: AHashSet<AlbumId>,
    favourite_artist_ids: AHashSet<ArtistId>,
    favoured: AHashMap<Favoured, bool>,
    favours_written: Vec<(Favoured, bool)>,
    charted: Arc<Chart>,
    covers: Recent<AtSide<AlbumId>, Option<Picture>>,
    decoding: AHashSet<AtSide<AlbumId>>,
    released_covers: Recent<AtSide<Mbid>, Option<Picture>>,
    fetching_covers: AHashSet<Mbid>,
    covers_unfetched: AHashMap<Mbid, Instant>,
    magnified: Option<Magnifying<AlbumId>>,
    portraits: Recent<AtSide<ArtistId>, Option<Picture>>,
    decoding_portraits: AHashSet<AtSide<ArtistId>>,
    scale: Scale,
    _warming: Task<()>,
    warmed: bool,
    named: Recent<TrackId, Named>,
    _naming_again: Task<()>,
    read_albums: Recent<AlbumId, Option<Album>>,
    read_albums_at: Option<CatalogStamp>,
    counted: Option<Listen>,
    _load: Task<()>,
    in_flight: Option<Wanted>,
    _scan: Task<()>,
    _organise: Task<()>,
    _import: Task<()>,
    _poll: Task<()>,
    _asking: Task<()>,
    asking_at: Option<Instant>,
    asks_from: Instant,
    next_try: Option<SystemTime>,
    _watching: Task<()>,
    _taking_offers: Task<()>,
    _watching_roots: Task<()>,
    window_seen: bool,
    _watching_inbox: Task<()>,
    _keeping_in_step: Task<()>,
    _refreshing: Task<()>,
    _retag: Task<()>,
    _enrich: Task<()>,
    _edit: Task<()>,
    _counted: Task<()>,
    _settled: Task<()>,
    _passed: Task<()>,
    _shared: Task<()>,
    pressings: Option<(AlbumId, Pressings)>,
    roots_waiting: Vec<RootWaiting>,
    poll_owed: Option<PollOptions>,
    _owed: Task<()>,
    keeping: Option<KeepPass>,
    keep_owed: bool,
    keep_tally: KeepSummary,
    _keeping: Task<()>,
    _keep_again: Task<()>,
    downloads: Downloads,
    downloads_restored: bool,
    to_file: Option<ToFile>,
    _pressings: Task<()>,
    _kept: Task<()>,
    _aged: Task<()>,
    _finding: Task<()>,
    _reaching: Task<()>,
    _showing: Task<()>,
    _learning: Task<()>,
    _previewing: Task<()>,
}

#[derive(Debug, Default)]
struct Followed {
    seen: PollStats,
    since: u32,
}

impl Followed {
    fn reread(&mut self, now: PollStats) -> Option<Wanted> {
        self.since = self.since.saturating_add(1);
        if now == self.seen || self.since < POLLS_PER_REREAD_WHILE_ASKING {
            return None;
        }
        let landed = now.kept > self.seen.kept;
        self.seen = now;
        self.since = 0;
        Some(if landed {
            Wanted::Everything
        } else {
            Wanted::ThePlaylists
        })
    }
}

fn stopped_told(stopped: &KeepStopped) -> String {
    let song = &stopped.song;
    match stopped.why {
        WhyAway::SignInLapsed => words::models::the_cookies_lapsed(song),
        WhyAway::SubscriptionLapsed => words::models::premium_lost(song),
        WhyAway::ToolMissing => words::models::yt_dlp_missing(song),
        WhyAway::NotAnswering => words::models::the_service_stopped_answering(song),
    }
}

fn kept(summary: &KeepSummary) -> Option<Notice> {
    if let Some(stopped) = &summary.stopped {
        return Some(Notice::Trouble(stopped_told(stopped)));
    }
    let missed = summary.gone + summary.unkept;
    match (summary.landed, missed) {
        (0, 0) => None,
        (landed, 0) => Some(Notice::Done(words::models::kept_songs(landed))),
        (0, missed) => Some(Notice::Trouble(words::models::songs_not_kept(missed))),
        (landed, missed) => Some(Notice::Noted(words::models::kept_some_songs_not_others(
            landed, missed,
        ))),
    }
}

fn keep_refused(error: &resonate_library::Error) -> Notice {
    match error {
        resonate_library::Error::NoDeliveryFolder => {
            Notice::Trouble(words::models::no_music_folder().to_owned())
        }
        resonate_library::Error::KeptWithNoTitle => {
            Notice::Trouble(words::models::no_title_to_keep().to_owned())
        }
        error => toast::could_not(words::models::keep_the_song(), error),
    }
}

fn list_kept(name: &str, kept: KeptList, left_out: usize) -> String {
    match left_out {
        0 => words::models::list_kept(kept.held, name, kept.queued),
        unnamed => words::models::list_kept_leaving_out(kept.held, name, kept.queued, unnamed),
    }
}

fn delivery_folder(cx: &App) -> Option<DeliveryFolder> {
    let global = cx.try_global::<ResonateApp>()?;
    Some(DeliveryFolder {
        path: global.music_folder.clone()?,
        layout: Layout::read(&global.organise_as).unwrap_or_default(),
    })
}

impl LibraryModel {
    pub fn new(
        library: Arc<Library>,
        reference: Option<Arc<dyn Reference>>,
        for_the_pass: Consulted,
        online: &Online,
        resume: bool,
        sourcing: Sourcing,
        cx: &mut Context<Self>,
    ) -> Self {
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
        let walks_back = library.walks_back().unwrap_or_default();
        let tags_walk_back = library.retag_walks_back().unwrap_or_default();
        let mut model = Self {
            first_read,
            unreadable: false,
            listed_for: String::new(),
            library,
            reference,
            for_the_pass,
            online: online.enabled,
            after_scan: online.after_scan,
            studies: online.studies,
            lyrics: online.lyrics,
            resume,
            albums: Rows::default(),
            artists: Rows::default(),
            tracks: Rows::default(),
            favourite_albums: Arc::default(),
            favourite_artists: Arc::default(),
            favourite_tracks: Arc::default(),
            statistics: Statistics::default(),
            most_listened: Arc::default(),
            by_day: Arc::default(),
            habits: Arc::default(),
            statistics_shown: false,
            heard_owed: true,
            suggestions: Arc::default(),
            opened_suggestion: None,
            window,
            scoped: Rows::default(),
            rows: Arc::default(),
            sung: None,
            found: Arc::default(),
            found_for: None,
            found_in_part: false,
            narrowed: Arc::default(),
            kept_songs: Arc::default(),
            kept_albums: Arc::default(),
            kept_for: None,
            songs_not_held: Arc::default(),
            albums_not_held: Arc::default(),
            albums_wanted: AHashMap::new(),
            albums_opening: AHashSet::new(),
            artists_found: Arc::default(),
            albums_found: Arc::default(),
            albums_shown: Arc::default(),
            artists_opening: AHashSet::new(),
            albums_missing_tracks_wanted: AHashSet::new(),
            album_songs: AHashMap::new(),
            shown: Arc::default(),
            asking: None,
            answers: Recent::new(ANSWERS_HELD),
            meant: None,
            as_typed: false,
            reaching_out: None,
            owed: None,
            unreached_for: None,
            providers_unheard: false,
            unheard_in_a_row: 0,
            wanting: AHashMap::new(),
            release_tracks: Arc::default(),
            release: None,
            artist: None,
            artist_albums: Arc::default(),
            artist_totals: ArtistTotals::default(),
            album: None,
            wanted: AHashMap::new(),
            standings: AHashMap::new(),
            missing_tracks: Arc::default(),
            unheld_releases: Arc::default(),
            missing: Missing::default(),
            dismissed: Missing::default(),
            missing_track_rows: Arc::default(),
            unheld_release_rows: Arc::default(),
            roots: Vec::new(),
            hidden: Arc::default(),
            playlists: Arc::default(),
            lists: Arc::default(),
            pictured: AHashMap::new(),
            pinned: Arc::default(),
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
            polling_every_provider: false,
            organised: None,
            relocated: Vec::new(),
            arrived: None,
            walks_back,
            tags_walk_back,
            previewed: Planned::Not,
            imported: None,
            previewed_import: Planned::Not,
            tagged: None,
            previewed_tags: Planned::Not,
            identical: None,
            previewed_identical: Planned::Not,
            sourcing,
            enriching: None,
            lookup_asked: None,
            enrich_owed: false,
            looked_up: None,
            sought: Arc::default(),
            reach,
            page_moved: false,
            playing: false,
            albums_counted: 0,
            artists_counted: 0,
            tracks_measured: Measured::default(),
            scoped_measured: Measured::default(),
            revision: 0,
            album_at: AHashMap::new(),
            favourite_album_ids: AHashSet::new(),
            favourite_artist_ids: AHashSet::new(),
            favoured: AHashMap::new(),
            favours_written: Vec::new(),
            charted: Arc::default(),
            covers: Recent::new(COVERS_HELD),
            decoding: AHashSet::new(),
            released_covers: Recent::new(RELEASED_COVERS_HELD),
            fetching_covers: AHashSet::new(),
            covers_unfetched: AHashMap::new(),
            magnified: None,
            portraits: Recent::new(PORTRAITS_HELD),
            decoding_portraits: AHashSet::new(),
            scale: Scale::ONE,
            _warming: Task::ready(()),
            warmed: false,
            named: Recent::new(NAMES_HELD),
            _naming_again: Task::ready(()),
            read_albums: Recent::new(ALBUMS_HELD),
            read_albums_at: None,
            counted: None,
            _load: Task::ready(()),
            in_flight: None,
            _scan: Task::ready(()),
            _organise: Task::ready(()),
            _import: Task::ready(()),
            _poll: Task::ready(()),
            _asking: Task::ready(()),
            asking_at: None,
            asks_from: Instant::now() + FIRST_ASKED_AFTER,
            next_try: None,
            _watching: Task::ready(()),
            _taking_offers: Task::ready(()),
            _watching_roots: Task::ready(()),
            window_seen: true,
            _watching_inbox: Task::ready(()),
            _keeping_in_step: Task::ready(()),
            _refreshing: Task::ready(()),
            _retag: Task::ready(()),
            _enrich: Task::ready(()),
            _edit: Task::ready(()),
            _counted: Task::ready(()),
            _settled: Task::ready(()),
            _passed: Task::ready(()),
            _shared: Task::ready(()),
            pressings: None,
            roots_waiting: Vec::new(),
            poll_owed: None,
            _owed: Task::ready(()),
            keeping: None,
            keep_owed: false,
            keep_tally: KeepSummary::default(),
            _keeping: Task::ready(()),
            _keep_again: Task::ready(()),
            downloads: Downloads::default(),
            downloads_restored: false,
            to_file: None,
            _pressings: Task::ready(()),
            _kept: Task::ready(()),
            _aged: Task::ready(()),
            _finding: Task::ready(()),
            _reaching: Task::ready(()),
            _showing: Task::ready(()),
            _learning: Task::ready(()),
            _previewing: Task::ready(()),
        };
        model.reload(cx);
        model.carry_on_enriching(cx);
        model.ask_when_due(cx);
        model.watch_for_writes_elsewhere(cx);
        model.watch_the_roots(cx);
        model.watch_the_inbox(cx);
        model.keep_the_linked_in_step(cx);
        model.keep_what_was_left_queued(cx);
        model
    }
}
