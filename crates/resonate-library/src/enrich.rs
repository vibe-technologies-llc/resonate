use std::{
    cell::{Cell, RefCell},
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use ahash::AHashSet;
use crossbeam_channel::{SendError, Sender, bounded};
use parking_lot::Mutex;
use resonate_analysis::Verdict;
use resonate_core::{AlbumId, ArtistId, Chromaprint, TrackId};

use crate::{
    AlbumToAsk, ArtistMatch, ArtistProfile, ArtistRelease, ArtistToAsk, CoverArt, Credit, Error,
    Fingerprinters, GroupAsked, GroupMatch, GroupRelease, Library, Link, LookupOp, Mbid, Recording,
    RecordingAsked, RecordingMatch, RecordingRelease, Reference, Release, ReleaseAsked,
    ReleaseGroup, ReleaseMatch, Result, TrackToAsk, Wording, credits,
    enriched::{folded_title, stripped_title},
    model::CoverFrom,
    pass::{Cancelling, EnrichHandle, PassHandle, PassKind},
    reference::credited_as,
    studies::{self, Agreement, Claims, HEARD_AT_LEAST, HeardAs, Studies, ToStudy},
};

pub const RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

pub const REFUSED_AGAIN_AFTER: Duration = Duration::from_secs(60 * 60);

pub const REFRESH_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub const WAITS: Waits = Waits {
    retry_after: RETRY_AFTER,
    refused_again_after: REFUSED_AGAIN_AFTER,
    refresh_after: REFRESH_AFTER,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waits {
    pub retry_after: Duration,
    pub refused_again_after: Duration,
    pub refresh_after: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fruitless {
    Missed,
    Refused,
}

impl Fruitless {
    pub(crate) fn asks(self) -> i64 {
        i64::from(matches!(self, Self::Missed))
    }

    pub(crate) fn refused(self) -> i64 {
        i64::from(matches!(self, Self::Refused))
    }
}

#[derive(Clone, Copy)]
struct Refusals(u32);

const STRICT_SCORE: u8 = 95;

const EXACT_SCORE: u8 = 100;

const RECORDING_MAY_DIFFER_BY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct EnrichOptions {
    pub refresh: bool,
    pub at_most: Option<NonZeroUsize>,
    pub sought: Arc<Sought>,
    pub studies: bool,
}

impl Default for EnrichOptions {
    fn default() -> Self {
        Self {
            refresh: false,
            at_most: None,
            sought: Arc::default(),
            studies: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Seek {
    Album(AlbumId),
    Artist(ArtistId),
}

#[derive(Debug, Default)]
pub struct Sought {
    asked: Mutex<Vec<Seek>>,
}

impl Sought {
    pub fn album(&self, id: AlbumId) {
        self.seek(Seek::Album(id));
    }

    pub fn artist(&self, id: ArtistId) {
        self.seek(Seek::Artist(id));
    }

    fn seek(&self, seek: Seek) {
        let mut asked = self.asked.lock();
        asked.retain(|held| *held != seek);
        asked.push(seek);
    }

    fn taken(&self) -> Vec<Seek> {
        std::mem::take(&mut *self.asked.lock())
    }
}

enum Ask {
    Album(AlbumToAsk),
    Track(TrackId),
    Artist(ArtistId),
}

impl Ask {
    fn is(&self, seek: Seek) -> bool {
        match (self, seek) {
            (Self::Album(album), Seek::Album(id)) => album.id == id,
            (Self::Artist(artist), Seek::Artist(id)) => *artist == id,
            (Self::Album(_), Seek::Artist(_))
            | (Self::Artist(_), Seek::Album(_))
            | (Self::Track(_), _) => false,
        }
    }
}

fn rotated(queue: &mut [Ask], seek: Seek) -> bool {
    match queue.iter().position(|ask| ask.is(seek)) {
        Some(at) => {
            queue[..=at].rotate_right(1);
            true
        }
        None => false,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnrichStats {
    pub albums: u64,
    pub releases: u64,
    pub matched: u64,
    pub covers: u64,
    pub tracks: u64,
    pub named: u64,
    pub artists: u64,
    pub portraits: u64,
    pub releases_found: u64,
    pub refused: u64,
    pub studied: u64,
    pub fakes: u64,
    pub recognised: u64,
    pub misnamed: u64,
}

#[derive(Debug, Default)]
pub struct EnrichProgress {
    albums: AtomicU64,
    releases: AtomicU64,
    matched: AtomicU64,
    covers: AtomicU64,
    tracks: AtomicU64,
    named: AtomicU64,
    artists: AtomicU64,
    portraits: AtomicU64,
    releases_found: AtomicU64,
    refused: AtomicU64,
    studied: AtomicU64,
    fakes: AtomicU64,
    recognised: AtomicU64,
    misnamed: AtomicU64,
    cancelled: AtomicBool,
}

impl EnrichProgress {
    pub fn snapshot(&self) -> EnrichStats {
        EnrichStats {
            albums: self.albums.load(Ordering::Relaxed),
            releases: self.releases.load(Ordering::Relaxed),
            matched: self.matched.load(Ordering::Relaxed),
            covers: self.covers.load(Ordering::Relaxed),
            tracks: self.tracks.load(Ordering::Relaxed),
            named: self.named.load(Ordering::Relaxed),
            artists: self.artists.load(Ordering::Relaxed),
            portraits: self.portraits.load(Ordering::Relaxed),
            releases_found: self.releases_found.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            studied: self.studied.load(Ordering::Relaxed),
            fakes: self.fakes.load(Ordering::Relaxed),
            recognised: self.recognised.load(Ordering::Relaxed),
            misnamed: self.misnamed.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn refuse(&self) {
        self.refused.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_studied(&self, verdict: Verdict) {
        self.studied.fetch_add(1, Ordering::Relaxed);
        if verdict == Verdict::Fake {
            self.fakes.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn note_recognised(&self, agreement: Agreement) {
        self.recognised.fetch_add(1, Ordering::Relaxed);
        if agreement == Agreement::Disagrees {
            self.misnamed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Cancelling for EnrichProgress {
    fn cancel(&self) {
        EnrichProgress::cancel(self);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnrichSummary {
    pub stats: EnrichStats,
    pub cancelled: bool,
    pub stopped_by: Option<LookupOp>,
}

pub(crate) fn start(
    library: Library,
    reference: Arc<dyn Reference>,
    fingerprinters: Arc<Fingerprinters>,
    options: EnrichOptions,
) -> Result<EnrichHandle> {
    let progress = Arc::new(EnrichProgress::default());
    let owned = Arc::clone(&progress);

    let thread = thread::Builder::new()
        .name("resonate-enrich".to_owned())
        .spawn(move || run(&library, &reference, &fingerprinters, &options, &progress))
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Enrich, owned, thread))
}

fn run(
    library: &Library,
    reference: &Arc<dyn Reference>,
    fingerprinters: &Arc<Fingerprinters>,
    options: &EnrichOptions,
    progress: &Arc<EnrichProgress>,
) -> Result<EnrichSummary> {
    note_began(library, options.refresh);
    let pictures = Pictures::start(library, reference, progress);
    let claims = Arc::new(Claims::default());
    let studies = Studies::start(
        library,
        fingerprinters,
        progress,
        &claims,
        to_be_studied(library, options),
    );
    let pass = Pass {
        library,
        reference: reference.as_ref(),
        fingerprinters,
        progress,
        pictures: &pictures,
        claims: &claims,
        refusals: Cell::new(0),
        pictured: RefCell::new(AHashSet::new()),
        covered: RefCell::new(AHashSet::new()),
    };
    let stopped_by = match pass.run(options) {
        Ok(()) => None,
        Err(Error::Unreachable { op, source }) => {
            tracing::warn!(
                error = %source,
                ?op,
                "the reference could not be reached, and the pass ends here"
            );
            Some(op)
        }
        Err(other) => return Err(other),
    };
    pictures.rest();
    studies.rest();
    if stopped_by.is_none() {
        note_finished(library);
    }

    Ok(EnrichSummary {
        stats: progress.snapshot(),
        cancelled: progress.is_cancelled(),
        stopped_by,
    })
}

fn to_be_studied(library: &Library, options: &EnrichOptions) -> Vec<ToStudy> {
    if !options.studies {
        return Vec::new();
    }
    let mut asked = match library.to_study(options.refresh) {
        Ok(asked) => asked,
        Err(error) => {
            tracing::warn!(%error, "the tracks to study could not be read, and none are studied");
            Vec::new()
        }
    };
    if let Some(at_most) = options.at_most {
        asked.truncate(at_most.get());
    }
    asked
}

const PICTURES_ASKED: usize = 64;

const PICTURE_READERS: usize = 2;

enum Picture {
    Cover {
        album: AlbumId,
        release: Mbid,
        group: Option<Mbid>,
    },
    OfTheGroup {
        album: AlbumId,
        group: Mbid,
    },
    Portrait {
        artist: ArtistId,
        links: Vec<Link>,
    },
}

struct Pictures {
    wanted: Option<Sender<Picture>>,
    readers: Vec<JoinHandle<()>>,
}

impl Pictures {
    fn start(
        library: &Library,
        reference: &Arc<dyn Reference>,
        progress: &Arc<EnrichProgress>,
    ) -> Self {
        let (wanted, asked) = bounded(PICTURES_ASKED);
        let readers: Vec<JoinHandle<()>> = (0..PICTURE_READERS)
            .filter_map(|nth| {
                let library = library.shared();
                let reference = Arc::clone(reference);
                let progress = Arc::clone(progress);
                let asked = asked.clone();
                thread::Builder::new()
                    .name(format!("resonate-pictures-{nth}"))
                    .spawn(move || {
                        for picture in asked {
                            fetch(&library, reference.as_ref(), &progress, picture);
                        }
                    })
                    .map_err(|error| {
                        tracing::warn!(%error, "a picture reader did not start");
                    })
                    .ok()
            })
            .collect();

        Self {
            wanted: (!readers.is_empty()).then_some(wanted),
            readers,
        }
    }

    fn hand(&self, picture: Picture) -> std::result::Result<(), SendError<Picture>> {
        match self.wanted.as_ref() {
            Some(wanted) => wanted.send(picture),
            None => Err(SendError(picture)),
        }
    }

    fn rest(mut self) {
        drop(self.wanted.take());
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

fn fetch(
    library: &Library,
    reference: &dyn Reference,
    progress: &EnrichProgress,
    picture: Picture,
) {
    if progress.is_cancelled() {
        return;
    }
    match picture {
        Picture::Cover {
            album,
            release,
            group,
        } => land_cover(
            library,
            progress,
            album,
            reference.cover(&release, group.as_ref()),
        ),
        Picture::OfTheGroup { album, group } => {
            land_cover(library, progress, album, reference.group_cover(&group));
        }
        Picture::Portrait { artist, links } => {
            land_portrait(library, progress, artist, reference.portrait(&links));
        }
    }
}

fn land_cover(
    library: &Library,
    progress: &EnrichProgress,
    album: AlbumId,
    found: Result<Option<CoverArt>>,
) {
    let settled = match found {
        Ok(None) => true,
        Ok(Some(art)) => match library.land_archive_cover(album, &art) {
            Ok(landed) => {
                if landed {
                    progress.covers.fetch_add(1, Ordering::Relaxed);
                }
                true
            }
            Err(error) => {
                tracing::warn!(%error, %album, "a cover the archive answered with was dropped");
                false
            }
        },
        Err(error) => {
            answered(progress, Err(error));
            false
        }
    };
    if settled && let Err(error) = library.note_cover_asked(album) {
        tracing::warn!(%error, %album, "a cover the archive answered for will be asked for again");
    }
}

fn land_portrait(
    library: &Library,
    progress: &EnrichProgress,
    artist: ArtistId,
    found: Result<Option<CoverArt>>,
) {
    let Some(art) = answered(progress, found) else {
        return;
    };
    match library.land_portrait(artist, &art) {
        Ok(true) => {
            progress.portraits.fetch_add(1, Ordering::Relaxed);
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(%error, %artist, "a portrait the reference answered with was dropped")
        }
    }
}

fn answered(progress: &EnrichProgress, found: Result<Option<CoverArt>>) -> Option<CoverArt> {
    match found {
        Ok(found) => found,
        Err(Error::Refused { op, status }) => {
            tracing::warn!(?op, status, "the reference refused a picture");
            progress.refuse();
            None
        }
        Err(Error::Unreadable { op }) => {
            tracing::warn!(
                ?op,
                "a picture answered with something this build cannot read"
            );
            progress.refuse();
            None
        }
        Err(error) => {
            tracing::warn!(%error, "a picture did not arrive");
            None
        }
    }
}

fn note_began(library: &Library, refresh: bool) {
    if let Err(error) = library.note_enrichment_began(refresh) {
        tracing::warn!(%error, "a pass left unfinished will not be picked up by the next run");
    }
}

fn note_finished(library: &Library) {
    if let Err(error) = library.note_enrichment_finished() {
        tracing::warn!(%error, "the next run may pick up a pass that has already finished");
    }
}

enum Heard<T> {
    Answered(T),
    Refused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Identified {
    Found(Mbid),
    Group(Mbid),
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    Exact,
    Wider,
    Narrower,
}

#[derive(Clone, Copy)]
enum Looking<'a> {
    ForAnAlbum { id: AlbumId, title: &'a str },
    ForATrack { id: TrackId, title: &'a str },
}

impl<'a> Looking<'a> {
    fn for_an_album(album: &'a AlbumToAsk) -> Self {
        Self::ForAnAlbum {
            id: album.id,
            title: &album.title,
        }
    }

    fn for_a_track(track: &'a TrackToAsk) -> Self {
        Self::ForATrack {
            id: track.id,
            title: &track.title,
        }
    }

    fn note_the_phrase_landed_nothing(self) {
        match self {
            Self::ForAnAlbum { id, title } => tracing::debug!(
                album = %id,
                %title,
                "the phrase landed nothing, so the words are asked"
            ),
            Self::ForATrack { id, title } => tracing::debug!(
                track = %id,
                %title,
                "the phrase landed nothing, so the words are asked"
            ),
        }
    }
}

trait Landed {
    fn nothing() -> Self;

    fn landed(&self) -> bool;
}

impl Landed for Identified {
    fn nothing() -> Self {
        Self::Nothing
    }

    fn landed(&self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

impl<T> Landed for Option<T> {
    fn nothing() -> Self {
        None
    }

    fn landed(&self) -> bool {
        self.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Certainty {
    Exactly,
    Nearly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Isrc,
    Recording,
    Search,
    Fingerprint,
}

impl Route {
    const ALL: [Self; 4] = [Self::Isrc, Self::Recording, Self::Search, Self::Fingerprint];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Accepted {
    Strict,
    Exact,
}

impl Accepted {
    fn scored(self, score: u8) -> bool {
        match self {
            Self::Strict => score >= STRICT_SCORE,
            Self::Exact => score == EXACT_SCORE,
        }
    }
}

const VERSION_QUALIFIERS: &[&str] = &[
    "albumversion",
    "singleversion",
    "cleanversion",
    "explicitversion",
    "explicit",
    "clean",
    "radioedit",
    "remaster",
    "remastered",
    "bonustrack",
    "deluxeedition",
    "monoversion",
    "stereoversion",
    "originalmix",
];

const DATED_QUALIFIERS: &[&str] = &["remaster", "remastered"];

const BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']')];

const YEAR_DIGITS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Spelling {
    Dequalified,
    AliasStripped,
    AliasMarked,
    Stripped,
    Marked,
    ById,
}

type ReleaseWeight = (Option<Spelling>, bool, bool, u8);

fn a_year(text: &str) -> bool {
    text.len() == YEAR_DIGITS && text.bytes().all(|digit| digit.is_ascii_digit())
}

fn a_dated_qualifier(folded: &str) -> bool {
    DATED_QUALIFIERS.iter().any(|word| {
        folded.strip_suffix(word).is_some_and(a_year)
            || folded.strip_prefix(word).is_some_and(a_year)
    })
}

fn a_version_qualifier(inside: &str) -> bool {
    let folded = folded_title(inside);
    VERSION_QUALIFIERS.contains(&folded.as_str()) || a_dated_qualifier(&folded)
}

fn without_a_qualifier(title: &str) -> Option<&str> {
    for (opening, closing) in BRACKETS {
        let Some(inside) = title.strip_suffix(*closing) else {
            continue;
        };
        let Some(opened) = inside.rfind(*opening) else {
            continue;
        };
        if !a_version_qualifier(&inside[opened + opening.len_utf8()..]) {
            continue;
        }
        let kept = inside[..opened].trim_end();
        if !kept.is_empty() {
            return Some(kept);
        }
    }

    None
}

fn dequalified(title: &str) -> &str {
    let mut kept = title.trim_end();
    while let Some(shorter) = without_a_qualifier(kept) {
        kept = shorter;
    }
    kept
}

fn same_name(found: Option<&str>, named: &str) -> Option<Spelling> {
    let found = found?;
    if folded_title(found) == folded_title(named) {
        return Some(Spelling::Marked);
    }
    if stripped_title(found) == stripped_title(named) {
        return Some(Spelling::Stripped);
    }

    (stripped_title(dequalified(found)) == stripped_title(dequalified(named)))
        .then_some(Spelling::Dequalified)
}

fn as_an_alias(spelling: Spelling) -> Spelling {
    match spelling {
        Spelling::ById => Spelling::ById,
        Spelling::Marked | Spelling::AliasMarked => Spelling::AliasMarked,
        Spelling::Stripped | Spelling::AliasStripped => Spelling::AliasStripped,
        Spelling::Dequalified => Spelling::Dequalified,
    }
}

fn billed(credit: &[Credit]) -> Option<String> {
    let billing = credited_as(credit);
    (!billing.trim().is_empty()).then_some(billing)
}

fn same_credit(credit: &[Credit], named: &str, named_mbid: Option<&Mbid>) -> Option<Spelling> {
    let by_id = credit
        .iter()
        .any(|credit| credit.mbid.is_some() && credit.mbid.as_ref() == named_mbid);
    if by_id {
        return Some(Spelling::ById);
    }
    let as_billed = same_name(billed(credit).as_deref(), named);
    let singly = credit
        .iter()
        .filter_map(|credit| same_name(Some(&credit.name), named))
        .max();

    as_billed.max(singly)
}

fn names_it(found: &ArtistMatch, named: &str) -> Option<Spelling> {
    same_name(Some(&found.name), named).or_else(|| {
        found
            .aliases
            .iter()
            .filter_map(|alias| same_name(Some(alias), named).map(as_an_alias))
            .max()
    })
}

fn owned_by(credit: &[Credit], album: &AlbumToAsk) -> Option<Spelling> {
    match album.owner.as_deref() {
        None => Some(Spelling::Marked),
        Some(owner) => same_credit(credit, owner, album.owner_mbid.as_ref()),
    }
}

fn declared_count(album: &AlbumToAsk) -> u32 {
    album.tagged_tracks.unwrap_or(album.track_count)
}

fn count_fits(found: &ReleaseMatch, album: &AlbumToAsk) -> bool {
    found.track_count == Some(declared_count(album))
}

fn year_of(date: Option<&str>) -> Option<i32> {
    let digits = date?.get(..YEAR_DIGITS)?;
    a_year(digits).then(|| digits.parse().ok()).flatten()
}

fn year_agrees(found: &ReleaseMatch, album: &AlbumToAsk) -> bool {
    album.year.is_some() && year_of(found.date.as_deref()) == album.year
}

fn agreed_owner(found: &ReleaseMatch, album: &AlbumToAsk) -> Option<Spelling> {
    Accepted::Strict
        .scored(found.score)
        .then(|| owned_by(&found.credit, album))
        .flatten()
}

fn matches_strictly(found: &ReleaseMatch, album: &AlbumToAsk) -> Option<Spelling> {
    count_fits(found, album)
        .then(|| agreed_owner(found, album))
        .flatten()
}

fn weighed_release(found: &ReleaseMatch, album: &AlbumToAsk) -> ReleaseWeight {
    (
        agreed_owner(found, album),
        count_fits(found, album),
        year_agrees(found, album),
        found.score,
    )
}

fn matches_as_a_group(found: &GroupMatch, album: &AlbumToAsk) -> Option<Spelling> {
    Accepted::Strict
        .scored(found.score)
        .then(|| owned_by(&found.credit, album))
        .flatten()
}

fn matches_exactly(found: &ArtistMatch, name: &str) -> Option<Spelling> {
    Accepted::Exact
        .scored(found.score)
        .then(|| names_it(found, name))
        .flatten()
}

fn top_of<T, W: Ord>(found: Vec<T>, weighed: impl Fn(&T) -> W) -> Option<T> {
    found.into_iter().reduce(|best, next| {
        if weighed(&next) > weighed(&best) {
            next
        } else {
            best
        }
    })
}

fn top_release(found: Vec<ReleaseMatch>, album: &AlbumToAsk) -> Identified {
    let Some(top) = top_of(found, |found| weighed_release(found, album)) else {
        return Identified::Nothing;
    };
    if matches_strictly(&top, album).is_some() {
        return Identified::Found(top.release);
    }
    if let (Some(_), Some(group)) = (agreed_owner(&top, album), &top.group) {
        tracing::debug!(
            album = %album.id,
            title = %album.title,
            matched = %top.title,
            score = top.score,
            tracks = ?top.track_count,
            %group,
            "the nearest release is another pressing, so its group is asked for"
        );
        return Identified::Group(group.clone());
    }

    tracing::debug!(
        album = %album.id,
        title = %album.title,
        matched = %top.title,
        score = top.score,
        tracks = ?top.track_count,
        artist = ?billed(&top.credit),
        "the nearest release is not taken under the strict rule"
    );
    Identified::Nothing
}

fn top_group(found: Vec<GroupMatch>, album: &AlbumToAsk) -> Option<Mbid> {
    let top = top_of(found, |found| {
        (matches_as_a_group(found, album), found.score)
    })?;
    if matches_as_a_group(&top, album).is_some() {
        return Some(top.group);
    }

    tracing::debug!(
        album = %album.id,
        title = %album.title,
        matched = %top.title,
        score = top.score,
        artist = ?billed(&top.credit),
        "the nearest release group is not taken under the strict rule"
    );
    None
}

fn earliest(date: Option<&str>) -> (bool, &str) {
    match date {
        Some(date) => (false, date),
        None => (true, ""),
    }
}

fn earliest_of<'a>(releases: impl Iterator<Item = &'a GroupRelease>) -> Option<&'a GroupRelease> {
    releases
        .min_by(|one, other| earliest(one.date.as_deref()).cmp(&earliest(other.date.as_deref())))
}

fn counted<'a>(group: &'a ReleaseGroup, count: u32) -> impl Iterator<Item = &'a GroupRelease> + 'a {
    group
        .releases
        .iter()
        .filter(move |release| release.track_count == Some(count))
}

fn widths(group: &ReleaseGroup) -> impl Iterator<Item = u32> + '_ {
    group
        .releases
        .iter()
        .filter_map(|release| release.track_count)
}

fn closest_release<'a>(
    group: &'a ReleaseGroup,
    album: &AlbumToAsk,
) -> Option<(&'a GroupRelease, Fit)> {
    if let Some(exact) = earliest_of(counted(group, declared_count(album))) {
        return Some((exact, Fit::Exact));
    }
    let held = album.track_count;
    if let Some(smallest_wider) = widths(group).filter(|count| *count > held).min() {
        return earliest_of(counted(group, smallest_wider)).map(|wider| (wider, Fit::Wider));
    }
    let widest_narrower = widths(group).filter(|count| *count <= held).max()?;

    earliest_of(counted(group, widest_narrower)).map(|narrower| (narrower, Fit::Narrower))
}

fn apart(one: Duration, other: Duration) -> Duration {
    one.checked_sub(other).unwrap_or_else(|| other - one)
}

fn lengths_agree(found: Option<Duration>, held: Option<Duration>) -> bool {
    match (found, held) {
        (Some(found), Some(held)) => apart(found, held) <= RECORDING_MAY_DIFFER_BY,
        (None, _) | (_, None) => true,
    }
}

fn asked_with(track: &TrackToAsk) -> Option<(&str, Option<&Mbid>)> {
    track
        .artist
        .as_deref()
        .map(|artist| (artist, track.artist_mbid.as_ref()))
        .or_else(|| {
            track
                .owner
                .as_deref()
                .map(|owner| (owner, track.owner_mbid.as_ref()))
        })
}

fn matches_a_recording(found: &RecordingMatch, track: &TrackToAsk) -> Option<Spelling> {
    if !Accepted::Strict.scored(found.score) || !lengths_agree(found.length, track.length) {
        return None;
    }
    let titled = same_name(Some(&found.title), &track.title)?;
    let Some((artist, artist_mbid)) = asked_with(track) else {
        return Some(titled);
    };
    let credited = same_credit(&found.credit, artist, artist_mbid)?;

    Some(titled.min(credited))
}

fn the_only_take(only: &Recording, track: &TrackToAsk) -> Option<Certainty> {
    if lengths_agree(only.length, track.length) {
        return Some(Certainty::Exactly);
    }

    same_name(Some(&only.title), &track.title).map(|_| Certainty::Nearly)
}

fn best_recording(
    recordings: Vec<Recording>,
    track: &TrackToAsk,
) -> Option<(Recording, Certainty)> {
    if let [only] = recordings.as_slice() {
        let certainty = the_only_take(only, track)?;
        return recordings.into_iter().next().map(|only| (only, certainty));
    }
    let Some(length) = track.length else {
        return recordings
            .into_iter()
            .next()
            .map(|first| (first, Certainty::Exactly));
    };

    recordings
        .into_iter()
        .filter_map(|recording| {
            let difference = apart(recording.length?, length);
            (difference <= RECORDING_MAY_DIFFER_BY).then_some((difference, recording))
        })
        .min_by_key(|(difference, _)| *difference)
        .map(|(_, recording)| (recording, Certainty::Exactly))
}

pub(crate) fn top_heard(found: &[RecordingMatch]) -> Option<&RecordingMatch> {
    found
        .iter()
        .filter(|found| found.score >= HEARD_AT_LEAST)
        .max_by_key(|found| found.score)
}

pub(crate) fn agreement(found: &[RecordingMatch], track: &TrackToAsk) -> Agreement {
    let heard: Vec<&RecordingMatch> = found
        .iter()
        .filter(|found| found.score >= HEARD_AT_LEAST)
        .collect();
    if heard.is_empty() {
        return Agreement::Unheard;
    }
    if track.tagged_title.is_none() {
        return Agreement::Unnamed;
    }
    if let Some(mbid) = &track.mbid
        && heard.iter().any(|found| &found.recording == mbid)
    {
        return Agreement::Agrees;
    }

    let named = heard.iter().any(|found| {
        same_name(Some(&found.title), &track.title).is_some()
            && asked_with(track).is_none_or(|(artist, artist_mbid)| {
                same_credit(&found.credit, artist, artist_mbid).is_some()
            })
    });
    if named {
        Agreement::Agrees
    } else {
        Agreement::Disagrees
    }
}

fn recognised(found: Vec<RecordingMatch>) -> Option<RecordingMatch> {
    let top = top_of(found, |found| found.score)?;
    if Accepted::Strict.scored(top.score) {
        return Some(top);
    }

    tracing::debug!(
        matched = %top.title,
        score = top.score,
        "the nearest fingerprint is not taken under the strict rule"
    );
    None
}

fn best_release<'a>(
    recording: &'a Recording,
    album: Option<&AlbumToAsk>,
) -> Option<&'a RecordingRelease> {
    let releases = || recording.releases.iter();
    let the_albums_own = album.and_then(|album| {
        let by_id = album
            .mbid
            .as_ref()
            .and_then(|mbid| releases().find(|release| release.id == *mbid));

        by_id.or_else(|| {
            releases().find(|release| folded_title(&release.title) == folded_title(&album.title))
        })
    });

    the_albums_own.or_else(|| {
        releases().min_by(|one, other| {
            earliest(one.date.as_deref()).cmp(&earliest(other.date.as_deref()))
        })
    })
}

fn top_artist(found: Vec<ArtistMatch>, name: &str) -> Option<Mbid> {
    let top = top_of(found, |found| (matches_exactly(found, name), found.score))?;
    if matches_exactly(&top, name).is_some() {
        return Some(top.mbid);
    }

    tracing::debug!(
        name,
        matched = %top.name,
        score = top.score,
        "the nearest artist is not taken under the exact rule"
    );
    None
}

struct Pass<'a> {
    library: &'a Library,
    reference: &'a dyn Reference,
    fingerprinters: &'a Fingerprinters,
    progress: &'a EnrichProgress,
    pictures: &'a Pictures,
    claims: &'a Claims,
    refusals: Cell<u32>,
    pictured: RefCell<AHashSet<ArtistId>>,
    covered: RefCell<AHashSet<AlbumId>>,
}

impl Pass<'_> {
    fn run(&self, options: &EnrichOptions) -> Result<()> {
        let albums = self.library.albums_to_ask(WAITS, options.refresh)?;
        let (mut due, rematch_only): (Vec<AlbumToAsk>, Vec<AlbumToAsk>) =
            albums.into_iter().partition(|album| !album.rematch_only);
        if let Some(at_most) = options.at_most {
            due.truncate(at_most.get());
        }

        for album in &rematch_only {
            self.rematch(album.id)?;
        }

        let mut tracks = self.library.tracks_to_ask(WAITS, options.refresh)?;
        if let Some(at_most) = options.at_most {
            tracks.truncate(at_most.get());
        }

        let mut artists = self.library.artists_to_ask(WAITS, options.refresh)?;
        if let Some(at_most) = options.at_most {
            artists.truncate(at_most.get());
        }

        let mut queue: Vec<Ask> = due
            .into_iter()
            .map(Ask::Album)
            .chain(tracks.into_iter().map(Ask::Track))
            .chain(artists.into_iter().map(Ask::Artist))
            .collect();

        let mut asked = 0;
        let mut spent = AHashSet::new();
        loop {
            while asked < queue.len() {
                if self.progress.is_cancelled() {
                    return Ok(());
                }
                self.lift(options, &spent, &mut queue, asked)?;
                match &queue[asked] {
                    Ask::Album(album) => {
                        spent.insert(Seek::Album(album.id));
                        self.album(album)?;
                    }
                    Ask::Track(track) => self.track(*track)?,
                    Ask::Artist(artist) => {
                        spent.insert(Seek::Artist(*artist));
                        self.artist(*artist)?;
                    }
                }
                asked += 1;
            }
            let born = self.artists_born_in_the_pass(options, &spent)?;
            if born.is_empty() {
                break;
            }
            queue.extend(born.into_iter().map(Ask::Artist));
        }

        self.library.settle_the_credits()?;
        self.look_again_for_covers()?;
        self.look_again_for_portraits()
    }

    fn artists_born_in_the_pass(
        &self,
        options: &EnrichOptions,
        spent: &AHashSet<Seek>,
    ) -> Result<Vec<ArtistId>> {
        if options.at_most.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .library
            .artists_to_ask(WAITS, options.refresh)?
            .into_iter()
            .filter(|artist| !spent.contains(&Seek::Artist(*artist)))
            .collect())
    }

    fn lift(
        &self,
        options: &EnrichOptions,
        spent: &AHashSet<Seek>,
        queue: &mut Vec<Ask>,
        from: usize,
    ) -> Result<()> {
        for seek in options.sought.taken() {
            if spent.contains(&seek) || rotated(&mut queue[from..], seek) {
                continue;
            }
            if let Some(ask) = self.still_due(seek, options.refresh)? {
                queue.insert(from, ask);
            }
        }
        Ok(())
    }

    fn still_due(&self, seek: Seek, refresh: bool) -> Result<Option<Ask>> {
        Ok(match seek {
            Seek::Album(id) => self
                .library
                .album_if_due(id, WAITS, refresh)?
                .map(Ask::Album),
            Seek::Artist(id) => self
                .library
                .artist_is_due(id, WAITS, refresh)?
                .then_some(Ask::Artist(id)),
        })
    }

    fn heard<T>(&self, answered: Result<T>) -> Result<Heard<T>> {
        match answered {
            Ok(value) => Ok(Heard::Answered(value)),
            Err(Error::Refused { op, status }) => {
                tracing::warn!(?op, status, "the reference refused a lookup");
                self.note_a_refusal();
                Ok(Heard::Refused)
            }
            Err(Error::Unreadable { op }) => {
                tracing::warn!(
                    ?op,
                    "the reference answered with something this build cannot read"
                );
                self.note_a_refusal();
                Ok(Heard::Refused)
            }
            Err(other) => Err(other),
        }
    }

    fn note_a_refusal(&self) {
        self.refusals.set(self.refusals.get().saturating_add(1));
        self.progress.refuse();
    }

    fn refusals(&self) -> Refusals {
        Refusals(self.refusals.get())
    }

    fn fruitlessly(&self, since: Refusals) -> Fruitless {
        if self.refusals.get() > since.0 {
            Fruitless::Refused
        } else {
            Fruitless::Missed
        }
    }

    fn rematch(&self, album: AlbumId) -> Result<()> {
        let matched = self.library.rematch(album)?;
        self.progress
            .matched
            .fetch_add(u64::from(matched), Ordering::Relaxed);
        Ok(())
    }

    fn album(&self, album: &AlbumToAsk) -> Result<()> {
        self.progress.albums.fetch_add(1, Ordering::Relaxed);
        let since = self.refusals();
        if let Some(mbid) = &album.mbid {
            match self.heard(self.reference.release(mbid))? {
                Heard::Answered(Some(release)) => return self.take_release(album, release),
                Heard::Answered(None) => tracing::debug!(
                    album = %album.id,
                    %mbid,
                    "the reference holds no release under the tagged id, so the album is searched for"
                ),
                Heard::Refused => return self.nothing_landed(album, since),
            }
        }
        match self.find_release(album)? {
            Identified::Found(mbid) => return self.land_release(album, &mbid, since),
            Identified::Group(group) => return self.land_group(album, &group, since),
            Identified::Nothing => {}
        }
        if let Some(group) = &album.group {
            match self.heard(self.reference.release_group(group))? {
                Heard::Answered(Some(found)) => return self.settle_group(album, &found, since),
                Heard::Answered(None) => tracing::debug!(
                    album = %album.id,
                    %group,
                    "the reference holds no release group under the tagged id, so one is searched for"
                ),
                Heard::Refused => return self.nothing_landed(album, since),
            }
        }
        match self.find_group(album)? {
            Some(group) => self.land_group(album, &group, since),
            None => self.nothing_landed(album, since),
        }
    }

    fn find_release(&self, album: &AlbumToAsk) -> Result<Identified> {
        self.found_either_way(
            Looking::for_an_album(album),
            |wording| {
                self.reference.find_release(&ReleaseAsked {
                    title: album.title.clone(),
                    artist: album.owner.clone(),
                    artist_mbid: album.owner_mbid.clone(),
                    barcode: album.barcode.clone(),
                    catalog_number: album.catalog_number.clone(),
                    wording,
                })
            },
            |found| Ok(top_release(found, album)),
        )
    }

    fn find_group(&self, album: &AlbumToAsk) -> Result<Option<Mbid>> {
        self.found_either_way(
            Looking::for_an_album(album),
            |wording| {
                self.reference.find_release_group(&GroupAsked {
                    title: album.title.clone(),
                    artist: album.owner.clone(),
                    artist_mbid: album.owner_mbid.clone(),
                    year: album.year,
                    wording,
                })
            },
            |found| Ok(top_group(found, album)),
        )
    }

    fn found_either_way<T, W: Landed>(
        &self,
        looking: Looking<'_>,
        ask: impl Fn(Wording) -> Result<Vec<T>>,
        weigh: impl Fn(Vec<T>) -> Result<W>,
    ) -> Result<W> {
        let found = match self.heard(ask(Wording::Phrase))? {
            Heard::Answered(found) => found,
            Heard::Refused => return Ok(W::nothing()),
        };
        let phrased = weigh(found)?;
        if phrased.landed() {
            return Ok(phrased);
        }

        looking.note_the_phrase_landed_nothing();
        match self.heard(ask(Wording::Words))? {
            Heard::Answered(found) => weigh(found),
            Heard::Refused => Ok(W::nothing()),
        }
    }

    fn nothing_landed(&self, album: &AlbumToAsk, since: Refusals) -> Result<()> {
        self.library
            .stamp_album_asked(album.id, self.fruitlessly(since))?;
        if album.has_release_rows {
            self.rematch(album.id)?;
        }
        Ok(())
    }

    fn land_release(&self, album: &AlbumToAsk, mbid: &Mbid, since: Refusals) -> Result<()> {
        match self.heard(self.reference.release(mbid))? {
            Heard::Answered(Some(release)) => self.take_release(album, release),
            Heard::Answered(None) => {
                tracing::debug!(album = %album.id, %mbid, "the reference holds no release under this id");
                self.nothing_landed(album, since)
            }
            Heard::Refused => self.nothing_landed(album, since),
        }
    }

    fn take_release(&self, album: &AlbumToAsk, release: Release) -> Result<()> {
        self.library.land_release(album.id, &release)?;
        self.progress.releases.fetch_add(1, Ordering::Relaxed);
        self.rematch(album.id)?;
        if !album.has_cover {
            self.cover(album, &release);
        }
        self.credits(&release.credit)
    }

    fn land_group(&self, album: &AlbumToAsk, group: &Mbid, since: Refusals) -> Result<()> {
        match self.heard(self.reference.release_group(group))? {
            Heard::Answered(Some(found)) => self.settle_group(album, &found, since),
            Heard::Answered(None) => {
                tracing::debug!(album = %album.id, %group, "the reference holds no release group under this id");
                self.nothing_landed(album, since)
            }
            Heard::Refused => self.nothing_landed(album, since),
        }
    }

    fn settle_group(
        &self,
        album: &AlbumToAsk,
        found: &ReleaseGroup,
        since: Refusals,
    ) -> Result<()> {
        match closest_release(found, album) {
            Some((release, fit)) => {
                tracing::debug!(
                    album = %album.id,
                    group = %found.id,
                    release = %release.id,
                    ?fit,
                    tracks = ?release.track_count,
                    "the group's closest pressing is landed"
                );
                self.land_release(album, &release.id, since)
            }
            None => self.take_group(album, found),
        }
    }

    fn take_group(&self, album: &AlbumToAsk, group: &ReleaseGroup) -> Result<()> {
        self.library.land_release_group(album.id, group)?;
        if album.has_release_rows {
            self.rematch(album.id)?;
        }
        if !album.has_cover {
            self.group_cover(album, group);
        }
        self.credits(&group.credit)
    }

    fn want(&self, picture: Picture) {
        if let Err(SendError(picture)) = self.pictures.hand(picture) {
            fetch(self.library, self.reference, self.progress, picture);
        }
    }

    fn group_cover(&self, album: &AlbumToAsk, group: &ReleaseGroup) {
        self.covered.borrow_mut().insert(album.id);
        self.want(Picture::OfTheGroup {
            album: album.id,
            group: group.id.clone(),
        });
    }

    fn cover(&self, album: &AlbumToAsk, release: &Release) {
        self.covered.borrow_mut().insert(album.id);
        self.want(Picture::Cover {
            album: album.id,
            release: release.id.clone(),
            group: release.group.clone(),
        });
    }

    fn credits(&self, credits: &[Credit]) -> Result<()> {
        for credit in credits {
            let Some(mbid) = &credit.mbid else {
                continue;
            };
            let Some(artist) = self.library.artist_named(&credit.name)? else {
                continue;
            };
            let unidentified = self
                .library
                .artist_detail(artist)?
                .is_some_and(|detail| detail.mbid.is_none());
            if unidentified {
                self.library.write_artist_mbid(artist, mbid)?;
            }
        }
        Ok(())
    }

    fn track(&self, id: TrackId) -> Result<()> {
        let Some(track) = self.library.track_to_ask(id)? else {
            return Ok(());
        };
        self.progress.tracks.fetch_add(1, Ordering::Relaxed);

        let since = self.refusals();
        for route in Route::ALL {
            if self.progress.is_cancelled() {
                return Ok(());
            }
            if let Some((recording, certainty)) = self.along(route, &track)? {
                return self.take_recording(&track, &recording, certainty, since);
            }
        }
        self.library.stamp_track_asked(id, self.fruitlessly(since))
    }

    fn along(&self, route: Route, track: &TrackToAsk) -> Result<Option<(Recording, Certainty)>> {
        match route {
            Route::Isrc => self.by_isrc(track),
            Route::Recording => self.by_recording(track),
            Route::Search => self.by_search(track),
            Route::Fingerprint => self.by_fingerprint(track),
        }
    }

    fn by_isrc(&self, track: &TrackToAsk) -> Result<Option<(Recording, Certainty)>> {
        let Some(isrc) = &track.isrc else {
            return Ok(None);
        };
        match self.heard(self.reference.recordings_of_isrc(isrc))? {
            Heard::Answered(found) => {
                let Some((named, certainty)) = best_recording(found, track) else {
                    return Ok(None);
                };
                let whole = self.told_where_it_sits(named)?;
                Ok(Some((whole, certainty)))
            }
            Heard::Refused => Ok(None),
        }
    }

    fn told_where_it_sits(&self, named: Recording) -> Result<Recording> {
        if !named.releases.is_empty() {
            return Ok(named);
        }

        Ok(self.recording(&named.id)?.unwrap_or(named))
    }

    fn by_recording(&self, track: &TrackToAsk) -> Result<Option<(Recording, Certainty)>> {
        let Some(mbid) = &track.mbid else {
            return Ok(None);
        };
        Ok(self
            .recording(mbid)?
            .map(|recording| (recording, Certainty::Exactly)))
    }

    fn by_search(&self, track: &TrackToAsk) -> Result<Option<(Recording, Certainty)>> {
        let asked_with = asked_with(track);
        if track.tagged_title.is_none() || (asked_with.is_none() && track.album_title.is_none()) {
            return Ok(None);
        }
        let found = self.found_either_way(
            Looking::for_a_track(track),
            |wording| {
                self.reference.find_recording(&RecordingAsked {
                    title: track.title.clone(),
                    artist: asked_with.map(|(artist, _)| artist.to_owned()),
                    artist_mbid: asked_with.and_then(|(_, mbid)| mbid.cloned()),
                    release: asked_with
                        .is_none()
                        .then(|| track.album_title.clone())
                        .flatten(),
                    length: track.length,
                    wording,
                })
            },
            |found| self.take_match(found, track),
        )?;

        Ok(found.map(|recording| (recording, Certainty::Nearly)))
    }

    fn by_fingerprint(&self, track: &TrackToAsk) -> Result<Option<(Recording, Certainty)>> {
        if !self.fingerprinters.has_a_source() {
            return Ok(None);
        }
        let Some(found) = self.heard_as(track) else {
            return Ok(None);
        };
        let Some(top) = recognised(found) else {
            return Ok(None);
        };

        Ok(self
            .recording(&top.recording)?
            .map(|recording| (recording, Certainty::Nearly)))
    }

    fn heard_as(&self, track: &TrackToAsk) -> Option<Vec<RecordingMatch>> {
        let claim = self.claims.claim(track.id);
        if claim.is_none() {
            self.claims.wait_for(track.id);
        }
        let held = match self.library.study_of(&track.location, track.span) {
            Ok(held) => held.map(|(_, studied)| studied),
            Err(error) => {
                tracing::warn!(%error, track = %track.id, "a track's study could not be read");
                None
            }
        };
        if let Some(studied) = &held
            && studied.recognised.is_some()
        {
            return Some(studied.heard_as.iter().map(HeardAs::as_a_match).collect());
        }
        claim.as_ref()?;

        let print = match held.and_then(|studied| studied.print) {
            Some(print) => print,
            None => self.studied_now(track)?,
        };
        studies::recognised_and_noted(
            self.library,
            self.fingerprinters,
            self.progress,
            track.id,
            print,
        )
    }

    fn studied_now(&self, track: &TrackToAsk) -> Option<Chromaprint> {
        let study = match resonate_analysis::study(
            &self.library.sources(),
            &track.location,
            track.span,
            self.progress,
        ) {
            Ok(study) => study,
            Err(error) => {
                tracing::debug!(%error, track = %track.id, "a track could not be studied to be fingerprinted");
                return None;
            }
        };
        match self.library.note_study(track.id, &study) {
            Ok(()) => self.progress.note_studied(study.judgement.verdict),
            Err(error) => tracing::warn!(%error, track = %track.id, "a study was not kept"),
        }
        study.print
    }

    fn take_match(
        &self,
        found: Vec<RecordingMatch>,
        track: &TrackToAsk,
    ) -> Result<Option<Recording>> {
        let Some(top) = top_of(found, |found| {
            (matches_a_recording(found, track), found.score)
        }) else {
            return Ok(None);
        };
        if matches_a_recording(&top, track).is_none() {
            tracing::debug!(
                track = %track.id,
                title = %track.title,
                matched = %top.title,
                score = top.score,
                artist = ?billed(&top.credit),
                length = ?top.length,
                "the nearest recording is not taken under the strict rule"
            );
            return Ok(None);
        }

        self.told_where_it_sits(top.into_recording()).map(Some)
    }

    fn recording(&self, mbid: &Mbid) -> Result<Option<Recording>> {
        match self.heard(self.reference.recording(mbid))? {
            Heard::Answered(Some(recording)) => Ok(Some(recording)),
            Heard::Answered(None) => {
                tracing::debug!(%mbid, "the reference holds no recording under this id");
                Ok(None)
            }
            Heard::Refused => Ok(None),
        }
    }

    fn take_recording(
        &self,
        track: &TrackToAsk,
        recording: &Recording,
        certainty: Certainty,
        since: Refusals,
    ) -> Result<()> {
        let album = self.album_under(track)?;
        let release = best_release(recording, album.as_ref());
        if self
            .library
            .land_recording(track.id, recording, certainty, release)?
        {
            self.progress.named.fetch_add(1, Ordering::Relaxed);
        }
        if track.album_answered {
            return Ok(());
        }

        let (Some(album), Some(release)) = (album, release) else {
            return Ok(());
        };
        self.land_release(&album, &release.id, since)
    }

    fn album_under(&self, track: &TrackToAsk) -> Result<Option<AlbumToAsk>> {
        match track.album {
            Some(album) => self.library.album_to_ask(album),
            None => Ok(None),
        }
    }

    fn artist(&self, id: ArtistId) -> Result<()> {
        let Some(artist) = self.library.artist_to_ask(id)? else {
            return Ok(());
        };
        self.progress.artists.fetch_add(1, Ordering::Relaxed);
        let since = self.refusals();
        let Some(profile) = self.profile_of(&artist)? else {
            self.bill_the_members(&artist)?;
            return self.library.stamp_artist_asked(id, self.fruitlessly(since));
        };
        self.library.land_artist(artist.id, &profile)?;
        self.discography(artist.id, &profile.mbid)?;
        if !artist.has_portrait && profile.may_be_pictured() {
            self.portrait(artist.id, profile.links.clone());
        }
        Ok(())
    }

    fn bill_the_members(&self, artist: &ArtistToAsk) -> Result<()> {
        let members = credits::members_of(&artist.name);
        if members.len() < 2 {
            return Ok(());
        }
        for member in members {
            if self.progress.is_cancelled() {
                return Ok(());
            }
            if self.library.artist_named(member)?.is_some() {
                continue;
            }
            let found = match self.heard(self.reference.find_artist(member))? {
                Heard::Answered(found) => top_artist(found, member),
                Heard::Refused => return Ok(()),
            };
            if let Some(mbid) = found {
                self.library.bill_an_artist(member, &mbid)?;
            }
        }
        Ok(())
    }

    fn discography(&self, artist: ArtistId, mbid: &Mbid) -> Result<()> {
        let Heard::Answered(groups) = self.heard(self.reference.release_groups_of(mbid))? else {
            return Ok(());
        };
        let kept: Vec<ArtistRelease> = groups.into_iter().filter(worth_keeping).collect();
        let written = self.library.land_artist_releases(artist, &kept)?;
        self.progress
            .releases_found
            .fetch_add(written as u64, Ordering::Relaxed);
        Ok(())
    }

    fn profile_of(&self, artist: &ArtistToAsk) -> Result<Option<ArtistProfile>> {
        if let Some(mbid) = &artist.mbid {
            match self.heard(self.reference.artist(mbid))? {
                Heard::Answered(Some(profile)) => return Ok(Some(profile)),
                Heard::Answered(None) => tracing::debug!(
                    artist = %artist.id,
                    %mbid,
                    "the reference holds no artist under the tagged id, so the artist is searched for"
                ),
                Heard::Refused => return Ok(None),
            }
        }
        let Some(mbid) = self.find_artist(artist)? else {
            return Ok(None);
        };
        match self.heard(self.reference.artist(&mbid))? {
            Heard::Answered(Some(profile)) => Ok(Some(profile)),
            Heard::Answered(None) => {
                tracing::debug!(
                    artist = %artist.id,
                    %mbid,
                    "the reference holds no artist under the id its own search answered"
                );
                Ok(None)
            }
            Heard::Refused => Ok(None),
        }
    }

    fn find_artist(&self, artist: &ArtistToAsk) -> Result<Option<Mbid>> {
        match self.heard(self.reference.find_artist(&artist.name))? {
            Heard::Answered(found) => Ok(top_artist(found, &artist.name)),
            Heard::Refused => Ok(None),
        }
    }

    fn portrait(&self, artist: ArtistId, links: Vec<Link>) {
        if !self.pictured.borrow_mut().insert(artist) {
            return;
        }
        self.want(Picture::Portrait { artist, links });
    }

    fn look_again_for_covers(&self) -> Result<()> {
        for wanted in self.library.albums_wanting_a_cover()? {
            if self.progress.is_cancelled() {
                return Ok(());
            }
            if self.covered.borrow().contains(&wanted.album) {
                continue;
            }
            self.want(match wanted.from {
                CoverFrom::Release { release, group } => Picture::Cover {
                    album: wanted.album,
                    release,
                    group,
                },
                CoverFrom::Group(group) => Picture::OfTheGroup {
                    album: wanted.album,
                    group,
                },
            });
        }
        Ok(())
    }

    fn look_again_for_portraits(&self) -> Result<()> {
        for wanted in self.library.artists_wanting_a_portrait()? {
            if self.progress.is_cancelled() {
                return Ok(());
            }
            self.portrait(wanted.artist, wanted.links);
        }
        Ok(())
    }
}

const KEPT_KINDS: [&str; 2] = ["Album", "EP"];

const SOUNDTRACK: &str = "Soundtrack";

fn worth_keeping(release: &ArtistRelease) -> bool {
    release
        .kind
        .as_deref()
        .is_some_and(|kind| KEPT_KINDS.contains(&kind))
        && release
            .secondary
            .iter()
            .all(|secondary| secondary == SOUNDTRACK)
}

#[cfg(test)]
mod tests {
    use resonate_core::MediaLocation;

    use super::*;

    fn artist_release(kind: Option<&str>, secondary: &[&str]) -> ArtistRelease {
        ArtistRelease {
            mbid: Mbid::new(ORBITS).expect("a well-formed mbid"),
            title: "Orbits".to_owned(),
            kind: kind.map(str::to_owned),
            secondary: secondary.iter().map(|each| (*each).to_owned()).collect(),
            first_released: None,
        }
    }

    #[test]
    fn an_album_and_an_ep_are_worth_keeping_and_a_soundtrack_is_still_one() {
        for kind in ["Album", "EP"] {
            assert!(worth_keeping(&artist_release(Some(kind), &[])), "{kind}");
            assert!(
                worth_keeping(&artist_release(Some(kind), &["Soundtrack"])),
                "{kind} soundtrack"
            );
        }
    }

    #[test]
    fn a_live_album_a_compilation_a_remix_a_single_and_an_unkinded_group_are_left_out() {
        for secondary in [
            ["Live"].as_slice(),
            ["Compilation"].as_slice(),
            ["Remix"].as_slice(),
            ["Soundtrack", "Live"].as_slice(),
            ["Soundtrack", "Compilation"].as_slice(),
        ] {
            assert!(
                !worth_keeping(&artist_release(Some("Album"), secondary)),
                "{secondary:?}"
            );
        }
        for kind in [
            None,
            Some("Single"),
            Some("Other"),
            Some("Broadcast"),
            Some("album"),
        ] {
            assert!(!worth_keeping(&artist_release(kind, &[])), "{kind:?}");
        }
    }

    const ORBITS: &str = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";
    const PORTAL: &str = "9a8b7c6d-5e4f-3a2b-1c0d-9e8f7a6b5c4d";
    const REMASTER: &str = "2b3c4d5e-6f7a-8b9c-0d1e-2f3a4b5c6d7e";

    fn album(owner: Option<&str>) -> AlbumToAsk {
        numbered_album(1, owner)
    }

    fn numbered_album(id: u64, owner: Option<&str>) -> AlbumToAsk {
        AlbumToAsk {
            id: AlbumId::new(id).expect("a non-zero id"),
            title: "Orbits".to_owned(),
            owner: owner.map(str::to_owned),
            track_count: 3,
            year: None,
            mbid: None,
            group: None,
            barcode: None,
            catalog_number: None,
            tagged_tracks: None,
            owner_mbid: None,
            has_cover: false,
            has_release_rows: false,
            rematch_only: false,
        }
    }

    fn release(score: u8, artist: Option<&str>, tracks: Option<u32>) -> ReleaseMatch {
        ReleaseMatch {
            release: Mbid::new(ORBITS).expect("a well-formed mbid"),
            group: None,
            score,
            title: "Orbits".to_owned(),
            credit: credited(artist),
            track_count: tracks,
            date: None,
        }
    }

    fn group_match(score: u8, artist: Option<&str>) -> GroupMatch {
        GroupMatch {
            group: Mbid::new(ORBITS).expect("a well-formed mbid"),
            score,
            title: "Orbits".to_owned(),
            credit: credited(artist),
        }
    }

    fn credit_of(name: &str, joined_by: &str, mbid: Option<&str>) -> Credit {
        Credit {
            name: name.to_owned(),
            joined_by: joined_by.to_owned(),
            mbid: mbid.map(|mbid| Mbid::new(mbid).expect("a well-formed mbid")),
        }
    }

    fn group_release(id: &str, date: Option<&str>, tracks: Option<u32>) -> GroupRelease {
        GroupRelease {
            id: Mbid::new(id).expect("a well-formed mbid"),
            title: "Orbits".to_owned(),
            date: date.map(str::to_owned),
            country: None,
            track_count: tracks,
        }
    }

    fn group_of(releases: Vec<GroupRelease>) -> ReleaseGroup {
        ReleaseGroup {
            id: Mbid::new(ORBITS).expect("a well-formed mbid"),
            title: "Orbits".to_owned(),
            credit: Vec::new(),
            kind: Some("Album".to_owned()),
            first_released: Some("1971-10-30".to_owned()),
            disambiguation: None,
            links: Vec::new(),
            releases,
        }
    }

    fn taken_from(group: &ReleaseGroup, album: &AlbumToAsk) -> Option<String> {
        closest_release(group, album).map(|(release, _)| release.id.as_str().to_owned())
    }

    fn fitted_from(group: &ReleaseGroup, album: &AlbumToAsk) -> Option<(String, Fit)> {
        closest_release(group, album).map(|(release, fit)| (release.id.as_str().to_owned(), fit))
    }

    fn artist(score: u8, name: &str) -> ArtistMatch {
        ArtistMatch {
            mbid: Mbid::new(ORBITS).expect("a well-formed mbid"),
            name: name.to_owned(),
            score,
            kind: None,
            disambiguation: None,
            aliases: Vec::new(),
        }
    }

    fn known_as(name: &str, aliases: &[&str], mbid: &str) -> ArtistMatch {
        ArtistMatch {
            mbid: Mbid::new(mbid).expect("a well-formed mbid"),
            name: name.to_owned(),
            score: EXACT_SCORE,
            kind: None,
            disambiguation: None,
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        }
    }

    #[test]
    fn a_release_is_taken_on_the_score_the_count_and_the_folded_owner_together() {
        let owned = album(Some("The Orbiters"));
        assert!(matches_strictly(&release(100, Some("The Orbiters"), Some(3)), &owned).is_some());
        assert!(matches_strictly(&release(95, Some("the orbiters!"), Some(3)), &owned).is_some());
        assert!(matches_strictly(&release(94, Some("The Orbiters"), Some(3)), &owned).is_none());
        assert!(matches_strictly(&release(100, Some("The Orbiters"), Some(4)), &owned).is_none());
        assert!(matches_strictly(&release(100, Some("The Orbiters"), None), &owned).is_none());
        assert!(matches_strictly(&release(100, Some("The Orbits"), Some(3)), &owned).is_none());
        assert!(matches_strictly(&release(100, None, Some(3)), &owned).is_none());
    }

    #[test]
    fn an_album_with_no_owner_is_taken_on_the_score_and_the_count_alone() {
        let various = album(None);
        assert!(
            matches_strictly(&release(95, Some("Various Artists"), Some(3)), &various).is_some()
        );
        assert!(matches_strictly(&release(100, None, Some(3)), &various).is_some());
        assert!(
            matches_strictly(&release(100, Some("Various Artists"), Some(2)), &various).is_none()
        );
    }

    #[test]
    fn an_artist_is_taken_only_at_a_perfect_score_with_the_same_folded_name() {
        assert!(matches_exactly(&artist(100, "The Orbiters"), "The Orbiters").is_some());
        assert!(matches_exactly(&artist(100, "the orbiters"), "The Orbiters").is_some());
        assert!(matches_exactly(&artist(99, "The Orbiters"), "The Orbiters").is_none());
        assert!(matches_exactly(&artist(100, "Orbiters"), "The Orbiters").is_none());
    }

    #[test]
    fn a_name_stripped_of_its_marks_identifies_the_artist_that_carries_them() {
        assert_eq!(
            matches_exactly(&artist(100, "Marcin Przybyłowicz"), "Marcin Przybyłowicz"),
            Some(Spelling::Marked)
        );
        assert_eq!(
            matches_exactly(&artist(100, "Marcin Przybyłowicz"), "Marcin Przybylowicz"),
            Some(Spelling::Stripped)
        );
        assert_eq!(
            matches_exactly(&artist(100, "Marcin Przybylowicz"), "Marcin Przybyłowicz"),
            Some(Spelling::Stripped)
        );
        assert_eq!(
            matches_exactly(&artist(100, "Marcin Przybylowicz"), "Martin Przybylowicz"),
            None
        );
    }

    #[test]
    fn the_spelling_that_carries_the_marks_is_taken_over_the_one_that_does_not() {
        let spelled = |name: &str, mbid: &str| ArtistMatch {
            mbid: Mbid::new(mbid).expect("a well-formed mbid"),
            name: name.to_owned(),
            score: EXACT_SCORE,
            kind: None,
            disambiguation: None,
            aliases: Vec::new(),
        };
        let listed = vec![
            spelled("Marcin Przybylowicz", ORBITS),
            spelled("Marcin Przybyłowicz", PORTAL),
        ];

        assert_eq!(
            top_artist(listed, "Marcin Przybyłowicz"),
            Some(Mbid::new(PORTAL).expect("a well-formed mbid"))
        );
        assert_eq!(
            top_artist(
                vec![spelled("Marcin Przybyłowicz", PORTAL)],
                "Marcin Przybylowicz"
            ),
            Some(Mbid::new(PORTAL).expect("a well-formed mbid"))
        );
    }

    #[test]
    fn a_name_that_agrees_only_with_an_alias_ranks_below_one_that_agrees_with_the_name() {
        let under_an_alias = known_as("Nothing Alike", &["Marcin Przybyłowicz"], ORBITS);
        let under_its_name = known_as("Marcin Przybylowicz", &[], PORTAL);

        assert_eq!(
            matches_exactly(&under_an_alias, "Marcin Przybyłowicz"),
            Some(Spelling::AliasMarked)
        );
        assert_eq!(
            matches_exactly(&under_its_name, "Marcin Przybyłowicz"),
            Some(Spelling::Stripped)
        );
        assert!(Spelling::Stripped > Spelling::AliasMarked);

        assert_eq!(
            top_artist(vec![under_an_alias, under_its_name], "Marcin Przybyłowicz"),
            Some(Mbid::new(PORTAL).expect("a well-formed mbid"))
        );
    }

    #[test]
    fn an_alias_that_carries_its_marks_is_taken_over_one_stripped_of_them() {
        let both_ways = known_as(
            "Nothing Alike",
            &["Marcin Przybylowicz", "Marcin Przybyłowicz"],
            ORBITS,
        );
        assert_eq!(
            matches_exactly(&both_ways, "Marcin Przybyłowicz"),
            Some(Spelling::AliasMarked)
        );

        let stripped = known_as("Nothing Alike", &["Marcin Przybylowicz"], ORBITS);
        let marked = known_as("Nothing At All Alike", &["Marcin Przybyłowicz"], PORTAL);
        assert_eq!(
            matches_exactly(&stripped, "Marcin Przybyłowicz"),
            Some(Spelling::AliasStripped)
        );
        assert_eq!(
            top_artist(vec![stripped, marked], "Marcin Przybyłowicz"),
            Some(Mbid::new(PORTAL).expect("a well-formed mbid"))
        );
    }

    #[test]
    fn the_top_match_is_the_highest_score_and_the_first_of_a_tie() {
        let found = vec![
            artist(90, "first"),
            artist(100, "second"),
            artist(100, "third"),
        ];
        let top = top_of(found, |found| found.score).expect("a match was listed");
        assert_eq!(top.name, "second");
        assert!(top_of(Vec::<ArtistMatch>::new(), |found| found.score).is_none());
    }

    fn numbered_artist(id: u64) -> ArtistId {
        ArtistId::new(id).expect("a non-zero id")
    }

    fn named(queue: &[Ask]) -> Vec<(char, u64)> {
        queue
            .iter()
            .map(|ask| match ask {
                Ask::Album(album) => ('a', album.id.get()),
                Ask::Track(track) => ('t', track.get()),
                Ask::Artist(artist) => ('r', artist.get()),
            })
            .collect()
    }

    fn lift(sought: &Sought, queue: &mut [Ask]) {
        for seek in sought.taken() {
            rotated(queue, seek);
        }
    }

    fn queued() -> Vec<Ask> {
        vec![
            Ask::Album(numbered_album(1, None)),
            Ask::Album(numbered_album(2, None)),
            Ask::Album(numbered_album(3, None)),
            Ask::Artist(numbered_artist(7)),
            Ask::Artist(numbered_artist(8)),
        ]
    }

    #[test]
    fn what_was_sought_is_lifted_to_the_front_and_the_newest_leads() {
        let sought = Sought::default();
        sought.album(AlbumId::new(3).expect("a non-zero id"));
        sought.artist(ArtistId::new(8).expect("a non-zero id"));

        let mut queue = queued();
        lift(&sought, &mut queue);

        assert_eq!(
            named(&queue),
            vec![('r', 8), ('a', 3), ('a', 1), ('a', 2), ('r', 7)]
        );
    }

    #[test]
    fn a_seek_is_spent_once_and_names_nothing_the_queue_still_holds() {
        let sought = Sought::default();
        sought.album(AlbumId::new(2).expect("a non-zero id"));
        sought.album(AlbumId::new(99).expect("a non-zero id"));

        let mut queue = queued();
        lift(&sought, &mut queue);
        assert_eq!(named(&queue).first().copied(), Some(('a', 2)));

        let mut again = queued();
        lift(&sought, &mut again);
        assert_eq!(named(&again), named(&queued()));
    }

    #[test]
    fn a_seek_asked_for_twice_is_held_once() {
        let sought = Sought::default();
        sought.album(AlbumId::new(3).expect("a non-zero id"));
        sought.album(AlbumId::new(2).expect("a non-zero id"));
        sought.album(AlbumId::new(3).expect("a non-zero id"));

        let mut queue = queued();
        lift(&sought, &mut queue);

        assert_eq!(
            named(&queue),
            vec![('a', 3), ('a', 2), ('a', 1), ('r', 7), ('r', 8)]
        );
    }

    #[test]
    fn what_has_been_asked_already_is_left_where_it_is() {
        let sought = Sought::default();
        sought.album(AlbumId::new(1).expect("a non-zero id"));

        let mut queue = queued();
        lift(&sought, &mut queue[2..]);

        assert_eq!(named(&queue), named(&queued()));
    }

    #[test]
    fn a_near_miss_names_nothing_and_a_strict_match_names_the_release() {
        let owned = album(Some("The Orbiters"));
        assert_eq!(
            top_release(vec![release(100, Some("The Orbiters"), Some(4))], &owned),
            Identified::Nothing
        );
        assert_eq!(top_release(Vec::new(), &owned), Identified::Nothing);
        assert_eq!(
            top_release(
                vec![
                    release(80, Some("The Orbiters"), Some(3)),
                    release(100, Some("The Orbiters"), Some(3))
                ],
                &owned
            ),
            Identified::Found(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
    }

    #[test]
    fn a_release_group_is_taken_on_the_score_and_the_folded_owner_without_any_track_count() {
        let owned = album(Some("The Orbiters"));
        assert!(matches_as_a_group(&group_match(100, Some("The Orbiters")), &owned).is_some());
        assert!(matches_as_a_group(&group_match(95, Some("the orbiters!")), &owned).is_some());
        assert!(matches_as_a_group(&group_match(94, Some("The Orbiters")), &owned).is_none());
        assert!(matches_as_a_group(&group_match(100, Some("The Orbits")), &owned).is_none());
        assert!(matches_as_a_group(&group_match(100, None), &owned).is_none());
        assert!(
            matches_as_a_group(&group_match(95, Some("Various Artists")), &album(None)).is_some()
        );

        assert!(matches_strictly(&release(100, Some("The Orbiters"), Some(4)), &owned).is_none());
        assert!(matches_strictly(&release(100, Some("The Orbiters"), None), &owned).is_none());
        assert_eq!(
            top_group(vec![group_match(100, Some("The Orbiters"))], &owned),
            Some(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
        assert_eq!(
            top_group(vec![group_match(94, Some("The Orbiters"))], &owned),
            None
        );
        assert_eq!(top_group(Vec::new(), &owned), None);
    }

    #[test]
    fn the_release_in_a_group_whose_count_matches_the_album_is_the_one_taken() {
        let owned = album(Some("The Orbiters"));
        let group = group_of(vec![
            group_release(PORTAL, Some("1971-10-30"), Some(4)),
            group_release(REMASTER, Some("2011-06-01"), Some(3)),
            group_release(ORBITS, Some("1971"), Some(3)),
        ]);
        assert_eq!(
            fitted_from(&group, &owned),
            Some((ORBITS.to_owned(), Fit::Exact))
        );

        let declared = AlbumToAsk {
            tagged_tracks: Some(4),
            ..album(Some("The Orbiters"))
        };
        assert_eq!(
            fitted_from(&group, &declared),
            Some((PORTAL.to_owned(), Fit::Exact))
        );

        let counting_nothing = group_of(vec![group_release(REMASTER, Some("2011-06-01"), None)]);
        assert_eq!(taken_from(&counting_nothing, &owned), None);
        assert_eq!(taken_from(&group_of(Vec::new()), &owned), None);
    }

    #[test]
    fn a_group_with_no_exact_pressing_lands_the_smallest_wider_one_or_the_widest_narrower_one() {
        let owned = album(Some("The Orbiters"));
        let wider = group_of(vec![
            group_release(PORTAL, Some("1971-10-30"), Some(6)),
            group_release(REMASTER, Some("2011-06-01"), Some(4)),
            group_release(ORBITS, Some("1971"), Some(4)),
        ]);
        assert_eq!(
            fitted_from(&wider, &owned),
            Some((ORBITS.to_owned(), Fit::Wider))
        );

        let narrower = group_of(vec![
            group_release(PORTAL, Some("1971-10-30"), Some(2)),
            group_release(REMASTER, Some("2011-06-01"), Some(1)),
        ]);
        assert_eq!(
            fitted_from(&narrower, &owned),
            Some((PORTAL.to_owned(), Fit::Narrower))
        );

        let declared_wider = AlbumToAsk {
            tagged_tracks: Some(5),
            ..album(Some("The Orbiters"))
        };
        assert_eq!(
            fitted_from(&wider, &declared_wider),
            Some((ORBITS.to_owned(), Fit::Wider))
        );

        let as_wide_as_the_rip = group_of(vec![group_release(ORBITS, Some("1971"), Some(3))]);
        assert_eq!(
            fitted_from(&as_wide_as_the_rip, &declared_wider),
            Some((ORBITS.to_owned(), Fit::Narrower))
        );
    }

    #[test]
    fn a_credit_agrees_where_any_one_of_its_names_does_and_an_id_beats_every_spelling() {
        let together = vec![
            credit_of("The Weeknd", " with ", Some(PORTAL)),
            credit_of("JENNIE", " & ", None),
            credit_of("Lily-Rose Depp", "", None),
        ];
        assert_eq!(
            same_credit(&together, "The Weeknd", None),
            Some(Spelling::Marked)
        );
        assert_eq!(
            same_credit(&together, "Lily-Rose Depp", None),
            Some(Spelling::Marked)
        );
        assert_eq!(
            same_credit(&together, "The Weeknd with JENNIE & Lily-Rose Depp", None),
            Some(Spelling::Marked)
        );
        assert_eq!(same_credit(&together, "Vince Staples", None), None);

        let portal = Mbid::new(PORTAL).expect("a well-formed mbid");
        assert_eq!(
            same_credit(&together, "Nobody At All", Some(&portal)),
            Some(Spelling::ById)
        );
        let orbits = Mbid::new(ORBITS).expect("a well-formed mbid");
        assert_eq!(
            same_credit(&together, "The Weeknd", Some(&orbits)),
            Some(Spelling::Marked)
        );
        assert!(Spelling::ById > Spelling::Marked);
        assert_eq!(as_an_alias(Spelling::ById), Spelling::ById);

        let unidentified = vec![credit_of("The Weeknd", "", None)];
        assert_eq!(
            same_credit(&unidentified, "The Weeknd", None),
            Some(Spelling::Marked)
        );
        assert_eq!(same_credit(&[], "The Weeknd", None), None);
    }

    #[test]
    fn a_release_owned_by_the_tagged_id_agrees_however_the_credit_spells_it() {
        let identified = AlbumToAsk {
            owner_mbid: Some(Mbid::new(PORTAL).expect("a well-formed mbid")),
            ..album(Some("The Orbiters"))
        };
        let by_id = ReleaseMatch {
            credit: vec![credit_of("Orbiters", "", Some(PORTAL))],
            ..release(100, None, Some(3))
        };
        assert_eq!(matches_strictly(&by_id, &identified), Some(Spelling::ById));
        assert_eq!(
            matches_strictly(&release(100, Some("The Orbiters"), Some(3)), &identified),
            Some(Spelling::Marked)
        );
        assert_eq!(
            matches_as_a_group(
                &GroupMatch {
                    credit: vec![credit_of("Orbiters", "", Some(PORTAL))],
                    ..group_match(100, None)
                },
                &identified
            ),
            Some(Spelling::ById)
        );
    }

    fn dated(id: &str, date: Option<&str>, tracks: Option<u32>) -> ReleaseMatch {
        ReleaseMatch {
            release: Mbid::new(id).expect("a well-formed mbid"),
            date: date.map(str::to_owned),
            ..release(100, Some("The Orbiters"), tracks)
        }
    }

    #[test]
    fn a_hit_is_weighed_on_its_owner_its_count_its_year_and_then_its_score() {
        let owned = AlbumToAsk {
            year: Some(1971),
            ..album(Some("The Orbiters"))
        };
        assert_eq!(
            top_release(
                vec![
                    dated(PORTAL, Some("1971-10-30"), Some(4)),
                    dated(ORBITS, Some("2011-06-01"), Some(3)),
                ],
                &owned
            ),
            Identified::Found(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
        assert_eq!(
            top_release(
                vec![
                    dated(PORTAL, Some("2011-06-01"), Some(3)),
                    dated(ORBITS, Some("1971"), Some(3)),
                ],
                &owned
            ),
            Identified::Found(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
        assert_eq!(
            top_release(
                vec![
                    ReleaseMatch {
                        score: 96,
                        ..dated(PORTAL, Some("1971"), Some(3))
                    },
                    dated(ORBITS, Some("1971"), Some(3)),
                ],
                &owned
            ),
            Identified::Found(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
        assert_eq!(
            top_release(
                vec![
                    dated(PORTAL, Some("1971"), Some(3)),
                    ReleaseMatch {
                        credit: credited(Some("The Orbits")),
                        ..dated(ORBITS, Some("1971"), Some(3))
                    },
                ],
                &owned
            ),
            Identified::Found(Mbid::new(PORTAL).expect("a well-formed mbid"))
        );
        assert_eq!(
            weighed_release(&dated(ORBITS, Some("1971-10-30"), Some(3)), &owned),
            (Some(Spelling::Marked), true, true, 100)
        );
        assert_eq!(
            weighed_release(&dated(ORBITS, Some("nineteen"), Some(4)), &owned),
            (Some(Spelling::Marked), false, false, 100)
        );
        assert_eq!(
            weighed_release(&dated(ORBITS, None, Some(3)), &album(Some("The Orbiters"))),
            (Some(Spelling::Marked), true, false, 100)
        );
        assert_eq!(
            weighed_release(
                &ReleaseMatch {
                    score: 94,
                    ..dated(ORBITS, Some("1971"), Some(3))
                },
                &owned
            ),
            (None, true, true, 94)
        );
    }

    #[test]
    fn a_strict_hit_of_the_wrong_count_names_its_group_and_one_without_a_group_names_nothing() {
        let owned = album(Some("The Orbiters"));
        let another_pressing = ReleaseMatch {
            group: Some(Mbid::new(PORTAL).expect("a well-formed mbid")),
            ..release(100, Some("The Orbiters"), Some(4))
        };
        assert_eq!(
            top_release(vec![another_pressing.clone()], &owned),
            Identified::Group(Mbid::new(PORTAL).expect("a well-formed mbid"))
        );
        assert_eq!(
            top_release(
                vec![ReleaseMatch {
                    score: 94,
                    ..another_pressing.clone()
                }],
                &owned
            ),
            Identified::Nothing
        );
        assert_eq!(
            top_release(
                vec![ReleaseMatch {
                    credit: credited(Some("The Orbits")),
                    ..another_pressing
                }],
                &owned
            ),
            Identified::Nothing
        );
        assert_eq!(
            top_release(vec![release(100, Some("The Orbiters"), Some(4))], &owned),
            Identified::Nothing
        );
        let declared = AlbumToAsk {
            tagged_tracks: Some(4),
            ..album(Some("The Orbiters"))
        };
        assert_eq!(
            top_release(vec![release(100, Some("The Orbiters"), Some(4))], &declared),
            Identified::Found(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
    }

    #[test]
    fn a_release_that_declares_no_date_is_taken_only_where_no_dated_release_matches() {
        let owned = album(Some("The Orbiters"));
        let undated_first = group_of(vec![
            group_release(PORTAL, None, Some(3)),
            group_release(ORBITS, Some("2011-06-01"), Some(3)),
        ]);
        assert_eq!(taken_from(&undated_first, &owned).as_deref(), Some(ORBITS));

        let undated_alone = group_of(vec![
            group_release(PORTAL, None, Some(3)),
            group_release(REMASTER, None, Some(3)),
        ]);
        assert_eq!(taken_from(&undated_alone, &owned).as_deref(), Some(PORTAL));
    }

    const HEARD: &str = "One of These Days";

    fn heard(artist: Option<&str>, length: Option<Duration>) -> TrackToAsk {
        TrackToAsk {
            id: TrackId::new(1).expect("a non-zero id"),
            title: HEARD.to_owned(),
            tagged_title: Some(HEARD.to_owned()),
            artist: artist.map(str::to_owned),
            tagged_artist: artist.map(str::to_owned),
            artist_mbid: None,
            album: None,
            album_title: None,
            album_answered: false,
            owner: None,
            owner_mbid: None,
            location: MediaLocation::local("/music/1.wav"),
            span: None,
            length,
            mbid: None,
            isrc: None,
        }
    }

    fn titled(title: &str, artist: Option<&str>) -> TrackToAsk {
        TrackToAsk {
            title: title.to_owned(),
            tagged_title: Some(title.to_owned()),
            ..heard(artist, None)
        }
    }

    fn credited(name: Option<&str>) -> Vec<Credit> {
        name.into_iter()
            .map(|name| Credit {
                name: name.to_owned(),
                joined_by: String::new(),
                mbid: None,
            })
            .collect()
    }

    fn found_recording(
        score: u8,
        title: &str,
        artist: Option<&str>,
        length: Option<Duration>,
    ) -> RecordingMatch {
        RecordingMatch {
            recording: Mbid::new(ORBITS).expect("a well-formed mbid"),
            score,
            title: title.to_owned(),
            credit: credited(artist),
            length,
            isrcs: Vec::new(),
            releases: Vec::new(),
        }
    }

    fn take(id: &str, length: Option<Duration>, releases: Vec<RecordingRelease>) -> Recording {
        Recording {
            id: Mbid::new(id).expect("a well-formed mbid"),
            title: HEARD.to_owned(),
            credit: Vec::new(),
            length,
            isrcs: Vec::new(),
            releases,
        }
    }

    fn recording_release(id: &str, title: &str, date: Option<&str>) -> RecordingRelease {
        RecordingRelease {
            id: Mbid::new(id).expect("a well-formed mbid"),
            title: title.to_owned(),
            date: date.map(str::to_owned),
            disc: Some(1),
            position: Some(4),
        }
    }

    fn named_by(found: Option<&(Recording, Certainty)>) -> Option<&str> {
        found.map(|(recording, _)| recording.id.as_str())
    }

    fn chosen(found: Option<&RecordingRelease>) -> Option<&str> {
        found.map(|release| release.id.as_str())
    }

    #[test]
    fn a_recording_is_taken_on_the_score_the_folded_title_the_artist_and_the_length_together() {
        let played = heard(Some("The Orbiters"), Some(Duration::from_secs(350)));
        let long = Some(Duration::from_secs(350));

        assert!(
            matches_a_recording(
                &found_recording(100, "One of These Days!", Some("the orbiters"), long),
                &played
            )
            .is_some()
        );
        assert!(
            matches_a_recording(
                &found_recording(95, HEARD, Some("The Orbiters"), None),
                &played
            )
            .is_some()
        );
        assert!(
            matches_a_recording(
                &found_recording(94, HEARD, Some("The Orbiters"), long),
                &played
            )
            .is_none()
        );
        assert!(
            matches_a_recording(
                &found_recording(100, "One of Those Days", Some("The Orbiters"), long),
                &played
            )
            .is_none()
        );
        assert!(
            matches_a_recording(
                &found_recording(100, HEARD, Some("The Orbits"), long),
                &played
            )
            .is_none()
        );
        assert!(matches_a_recording(&found_recording(100, HEARD, None, long), &played).is_none());

        let unnamed = heard(None, None);
        assert_eq!(
            matches_a_recording(&found_recording(100, HEARD, None, long), &unnamed),
            Some(Spelling::Marked)
        );
    }

    #[test]
    fn a_track_naming_no_artist_is_weighed_against_the_owner_of_its_album() {
        let under_an_owner = TrackToAsk {
            owner: Some("The Orbiters".to_owned()),
            ..heard(None, None)
        };
        assert_eq!(
            matches_a_recording(
                &found_recording(100, HEARD, Some("The Orbiters"), None),
                &under_an_owner
            ),
            Some(Spelling::Marked)
        );
        assert_eq!(
            matches_a_recording(
                &found_recording(100, HEARD, Some("The Orbits"), None),
                &under_an_owner
            ),
            None
        );

        let by_id = TrackToAsk {
            owner_mbid: Some(Mbid::new(PORTAL).expect("a well-formed mbid")),
            ..under_an_owner
        };
        let credited_by_id = RecordingMatch {
            credit: vec![credit_of("Orbiters", "", Some(PORTAL))],
            ..found_recording(100, HEARD, None, None)
        };
        assert_eq!(
            matches_a_recording(&credited_by_id, &by_id),
            Some(Spelling::Marked)
        );

        let named_itself = TrackToAsk {
            artist: Some("Ada".to_owned()),
            ..by_id
        };
        assert_eq!(matches_a_recording(&credited_by_id, &named_itself), None);
        assert_eq!(
            asked_with(&named_itself).map(|(artist, mbid)| (artist, mbid.is_some())),
            Some(("Ada", false))
        );
    }

    #[test]
    fn a_collaboration_credit_agrees_where_the_file_names_one_of_its_artists() {
        let played = heard(Some("The Weeknd"), None);
        let together = RecordingMatch {
            credit: vec![
                credit_of("The Weeknd", " with ", None),
                credit_of("JENNIE", " & ", None),
                credit_of("Lily-Rose Depp", "", None),
            ],
            ..found_recording(100, HEARD, None, None)
        };
        assert_eq!(
            matches_a_recording(&together, &played),
            Some(Spelling::Marked)
        );
        assert_eq!(
            matches_a_recording(&together, &heard(Some("Vince Staples"), None)),
            None
        );
    }

    #[test]
    fn a_title_carrying_a_version_qualifier_agrees_with_the_plain_one_and_ranks_below_it() {
        assert_eq!(
            same_name(Some("DDevil"), "DDevil (Album Version)"),
            Some(Spelling::Dequalified)
        );
        assert_eq!(
            same_name(Some("CUBErt"), "CUBErt  (Clean Version)"),
            Some(Spelling::Dequalified)
        );
        assert_eq!(
            same_name(Some("Lost In Hollywood [Explicit]"), "Lost in Hollywood"),
            Some(Spelling::Dequalified)
        );
        assert_eq!(
            same_name(Some("Know"), "Know (Album Version) (Radio Edit)"),
            Some(Spelling::Dequalified)
        );
        assert_eq!(
            same_name(Some("DDevil"), "DDevil (album version)"),
            Some(Spelling::Dequalified)
        );

        assert!(Spelling::Dequalified < Spelling::AliasStripped);
        assert!(Spelling::Dequalified < Spelling::Stripped);
        assert!(Spelling::Dequalified < Spelling::Marked);

        let qualified = titled("DDevil (Album Version)", Some("System of a Down"));
        let plainly = found_recording(100, "DDevil", Some("System of a Down"), None);
        let exactly = found_recording(
            100,
            "DDevil (Album Version)",
            Some("System of a Down"),
            None,
        );
        assert_eq!(
            matches_a_recording(&plainly, &qualified),
            Some(Spelling::Dequalified)
        );
        assert_eq!(
            matches_a_recording(&exactly, &qualified),
            Some(Spelling::Marked)
        );

        let top = top_of(vec![plainly, exactly], |found| {
            (matches_a_recording(found, &qualified), found.score)
        })
        .expect("a match was listed");
        assert_eq!(top.title, "DDevil (Album Version)");
    }

    #[test]
    fn a_bracket_naming_another_recording_is_not_a_qualifier_and_never_agrees() {
        for named in [
            "bad guy (with Justin Bieber)",
            "bad guy (Live)",
            "bad guy (Remix)",
            "bad guy (feat. Someone)",
            "bad guy (Acoustic)",
            "bad guy (Instrumental)",
            "bad guy (Demo)",
            "bad guy (Whatever This Is)",
        ] {
            assert_eq!(
                same_name(Some("bad guy"), named),
                None,
                "{named} was read as a qualified spelling of bad guy"
            );
            assert_eq!(
                same_name(Some(named), "bad guy"),
                None,
                "bad guy was read as a qualified spelling of {named}"
            );
        }

        assert_eq!(same_name(Some("Explicit"), "Bad Guy (Explicit)"), None);
        assert_eq!(same_name(Some("Album Version"), "Bad Guy"), None);
    }

    #[test]
    fn a_remaster_year_reads_as_a_qualifier_whichever_way_round_it_is_written() {
        for spelling in [
            "So What (Remastered 2011)",
            "So What (2011 Remaster)",
            "So What (Remaster)",
            "So What [Remastered]",
            "So What (2011 Remastered)",
            "So What (Remaster 2011)",
        ] {
            assert_eq!(
                same_name(Some("So What"), spelling),
                Some(Spelling::Dequalified),
                "{spelling} was not read as a remaster of So What"
            );
        }

        assert_eq!(same_name(Some("So What"), "So What (Remastered 20)"), None);
        assert_eq!(
            same_name(Some("So What"), "So What (Remastered by Ada)"),
            None
        );
    }

    #[test]
    fn a_de_qualified_title_is_still_refused_where_the_lengths_disagree() {
        let qualified = TrackToAsk {
            length: Some(Duration::from_secs(350)),
            ..titled("DDevil (Album Version)", Some("System of a Down"))
        };
        assert!(
            matches_a_recording(
                &found_recording(
                    100,
                    "DDevil",
                    Some("System of a Down"),
                    Some(Duration::from_secs(350))
                ),
                &qualified
            )
            .is_some()
        );
        assert!(
            matches_a_recording(
                &found_recording(
                    100,
                    "DDevil",
                    Some("System of a Down"),
                    Some(Duration::from_secs(600))
                ),
                &qualified
            )
            .is_none()
        );
    }

    #[test]
    fn a_recording_a_few_seconds_out_is_taken_and_one_a_minute_out_is_not() {
        let played = heard(Some("The Orbiters"), Some(Duration::from_millis(410_000)));
        let running = |millis| {
            found_recording(
                100,
                HEARD,
                Some("The Orbiters"),
                Some(Duration::from_millis(millis)),
            )
        };

        for millis in [405_000, 410_000, 414_999, 415_000] {
            assert!(
                matches_a_recording(&running(millis), &played).is_some(),
                "{millis} ms was not taken for a 410 000 ms file"
            );
        }
        for millis in [392_000, 415_001, 418_440, 478_693] {
            assert!(
                matches_a_recording(&running(millis), &played).is_none(),
                "{millis} ms was taken for a 410 000 ms file"
            );
        }
    }

    fn two_takes() -> Vec<Recording> {
        vec![
            take(ORBITS, Some(Duration::from_millis(349_800)), Vec::new()),
            take(PORTAL, Some(Duration::from_millis(356_000)), Vec::new()),
        ]
    }

    #[test]
    fn an_isrc_that_names_two_takes_answers_the_one_the_file_is_as_long_as() {
        let longer = heard(None, Some(Duration::from_millis(355_500)));
        assert_eq!(
            named_by(best_recording(two_takes(), &longer).as_ref()),
            Some(PORTAL)
        );

        let shorter = heard(None, Some(Duration::from_millis(350_000)));
        assert_eq!(
            named_by(best_recording(two_takes(), &shorter).as_ref()),
            Some(ORBITS)
        );

        let unmeasured = heard(None, None);
        assert_eq!(
            named_by(best_recording(two_takes(), &unmeasured).as_ref()),
            Some(ORBITS)
        );
    }

    #[test]
    fn an_isrc_whose_takes_are_all_the_wrong_length_identifies_nothing() {
        let neither = heard(None, Some(Duration::from_millis(300_000)));
        assert_eq!(best_recording(two_takes(), &neither), None);
        assert_eq!(best_recording(Vec::new(), &neither), None);
    }

    #[test]
    fn an_isrcs_only_take_is_nearly_the_file_where_the_title_agrees_and_the_length_does_not() {
        let neither = heard(None, Some(Duration::from_millis(300_000)));
        let remastered = vec![take(
            ORBITS,
            Some(Duration::from_millis(500_000)),
            Vec::new(),
        )];
        assert_eq!(
            best_recording(remastered.clone(), &neither).map(|(_, certainty)| certainty),
            Some(Certainty::Nearly)
        );
        assert_eq!(
            best_recording(
                remastered,
                &heard(None, Some(Duration::from_millis(500_000)))
            )
            .map(|(_, certainty)| certainty),
            Some(Certainty::Exactly)
        );

        let another_title = vec![Recording {
            title: "A Pillow of Winds".to_owned(),
            ..take(ORBITS, Some(Duration::from_millis(500_000)), Vec::new())
        }];
        assert_eq!(best_recording(another_title, &neither), None);
    }

    #[test]
    fn an_isrc_naming_one_take_identifies_it_even_where_no_length_is_declared() {
        let measured = heard(None, Some(Duration::from_millis(300_000)));
        assert_eq!(
            named_by(best_recording(vec![take(ORBITS, None, Vec::new())], &measured).as_ref()),
            Some(ORBITS)
        );
    }

    #[test]
    fn a_fingerprint_is_weighed_on_its_score_alone_because_the_audio_is_the_evidence() {
        let stem = RecordingMatch {
            recording: Mbid::new(ORBITS).expect("a well-formed mbid"),
            score: 100,
            title: "01 track".to_owned(),
            credit: Vec::new(),
            length: None,
            isrcs: Vec::new(),
            releases: Vec::new(),
        };
        let weak = RecordingMatch {
            score: 80,
            ..stem.clone()
        };

        assert_eq!(
            recognised(vec![stem.clone()]).map(|found| found.recording),
            Some(Mbid::new(ORBITS).expect("a well-formed mbid"))
        );
        assert_eq!(recognised(vec![weak]), None);
        assert_eq!(recognised(Vec::new()), None);
    }

    #[test]
    fn the_release_a_recording_names_is_the_albums_own_before_it_is_the_earliest() {
        let recording = take(
            ORBITS,
            None,
            vec![
                recording_release(REMASTER, "Orbits (2011 Remaster)", Some("2011-06-01")),
                recording_release(PORTAL, "Portals", None),
                recording_release(ORBITS, "Orbits", Some("1971-10-30")),
            ],
        );

        let identified = AlbumToAsk {
            mbid: Some(Mbid::new(REMASTER).expect("a well-formed mbid")),
            ..album(Some("The Orbiters"))
        };
        assert_eq!(
            chosen(best_release(&recording, Some(&identified))),
            Some(REMASTER)
        );

        let titled = album(Some("The Orbiters"));
        assert_eq!(
            chosen(best_release(&recording, Some(&titled))),
            Some(ORBITS)
        );
        assert_eq!(chosen(best_release(&recording, None)), Some(ORBITS));

        let undated = take(
            ORBITS,
            None,
            vec![
                recording_release(PORTAL, "Portals", None),
                recording_release(REMASTER, "Orbits (2011 Remaster)", None),
            ],
        );
        assert_eq!(chosen(best_release(&undated, None)), Some(PORTAL));
        assert_eq!(
            chosen(best_release(&take(ORBITS, None, Vec::new()), None)),
            None
        );
    }
}
