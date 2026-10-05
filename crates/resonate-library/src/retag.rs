use std::{
    collections::VecDeque,
    fs, mem,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::SystemTime,
};

use ahash::AHashMap;
use resonate_codec::{
    CoverArt, ImageFormat, Pictured, Picturing, Popularity, Rated, TagEdit, TagField, TagSet,
    TagSink, Writing,
};
use resonate_core::{AlbumId, MediaLocation, TrackId};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{
    Library, StoreOp,
    error::{Error, Result},
    paged::{Paging, ROWS_A_PAGE},
    pass::{Cancelling, PassHandle, PassKind, RetagHandle},
    store,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unwritten {
    Cut,
    Vaulted,
    Unsupported,
    Unreadable,
    Refused,
    Unconfirmed,
}

impl Unwritten {
    pub const fn is_a_failure(self) -> bool {
        matches!(self, Self::Unreadable | Self::Refused | Self::Unconfirmed)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut out of a file it shares",
            Self::Vaulted => "kept in the vault, which nothing writes tags into",
            Self::Unsupported => "a format this build does not write tags to",
            Self::Unreadable => "its tags could not be read",
            Self::Refused => "its tags could not be written",
            Self::Unconfirmed => "what was written did not read back",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassedOver {
    pub path: PathBuf,
    pub why: Unwritten,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub track: TrackId,
    pub path: PathBuf,
    pub edits: Vec<TagEdit>,
    pub taken: Vec<TagField>,
    pub picture: Option<Arc<CoverArt>>,
    pub unpictured: bool,
    pub popularity: Option<Popularity>,
    pub was: Held,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub fields: Vec<(TagField, Option<String>)>,
    pub rated: Option<Rated>,
    pub picture: Option<Arc<CoverArt>>,
}

impl Held {
    fn of(tags: &TagSet, fields: impl IntoIterator<Item = TagField>, rated: Option<Rated>) -> Self {
        Self {
            fields: fields
                .into_iter()
                .map(|field| (field, field.read(tags)))
                .collect(),
            rated,
            picture: None,
        }
    }
}

impl Written {
    fn writing(&self) -> Writing<'_> {
        Writing {
            edits: &self.edits,
            taken: &self.taken,
            picture: self.picture.as_deref(),
            unpictured: self.unpictured,
            popularity: self.popularity,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Retagging {
    pub writes: Vec<Written>,
    pub passed_over: Vec<PassedOver>,
}

impl Retagging {
    fn pass_over(&mut self, path: &Path, why: Unwritten, progress: &RetagProgress) {
        RetagProgress::step(&progress.passed_over);
        self.passed_over.push(PassedOver {
            path: path.to_path_buf(),
            why,
        });
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetagOptions {
    pub roots: Vec<PathBuf>,
    pub apply: bool,
    pub undo: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetagStats {
    pub walked: u64,
    pub written: u64,
    pub fields: u64,
    pub pictures: u64,
    pub ratings: u64,
    pub unchanged: u64,
    pub passed_over: u64,
}

#[derive(Debug, Default)]
pub struct RetagProgress {
    walked: AtomicU64,
    written: AtomicU64,
    fields: AtomicU64,
    pictures: AtomicU64,
    ratings: AtomicU64,
    unchanged: AtomicU64,
    passed_over: AtomicU64,
    cancelled: AtomicBool,
}

impl RetagProgress {
    pub fn snapshot(&self) -> RetagStats {
        RetagStats {
            walked: self.walked.load(Ordering::Relaxed),
            written: self.written.load(Ordering::Relaxed),
            fields: self.fields.load(Ordering::Relaxed),
            pictures: self.pictures.load(Ordering::Relaxed),
            ratings: self.ratings.load(Ordering::Relaxed),
            unchanged: self.unchanged.load(Ordering::Relaxed),
            passed_over: self.passed_over.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn step(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Cancelling for RetagProgress {
    fn cancel(&self) {
        RetagProgress::cancel(self);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetagSummary {
    pub stats: RetagStats,
    pub retagging: Retagging,
    pub cancelled: bool,
}

pub(crate) fn start(
    library: Library,
    tags: Arc<dyn TagSink>,
    options: RetagOptions,
) -> Result<RetagHandle> {
    let walking = library.walk_the_tree()?;
    let progress = Arc::new(RetagProgress::default());
    let owned = Arc::clone(&progress);
    walking.cancelled_by(Arc::clone(&progress) as Arc<dyn Cancelling>);

    let thread = thread::Builder::new()
        .name("resonate-retag".to_owned())
        .spawn(move || {
            let outcome = run(&library, tags.as_ref(), &options, &progress);
            drop(walking);
            outcome
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Retag, owned, thread))
}

fn run(
    library: &Library,
    tags: &dyn TagSink,
    options: &RetagOptions,
    progress: &RetagProgress,
) -> Result<RetagSummary> {
    let known = library.roots()?;
    for root in &options.roots {
        if !known.contains(root) {
            return Err(Error::NotARoot { path: root.clone() });
        }
    }

    let mut noted = Noted::default();
    if options.undo {
        let kept = library.last_retag()?;
        let mut retagging = undone(library, &kept, tags, progress)?;
        noted.walking_back = Some(
            kept.into_iter()
                .map(|kept| (kept.path.clone(), kept))
                .collect(),
        );
        noted.begun = true;
        if options.apply {
            let written = apply(library, tags, &mut retagging, progress, &mut noted)?;
            let finished = retagging.passed_over.is_empty() && !progress.is_cancelled();
            library.retag_walked_back(&written, finished)?;
        }
        return Ok(RetagSummary {
            stats: progress.snapshot(),
            retagging,
            cancelled: progress.is_cancelled(),
        });
    }

    let totals = library.release_totals()?;
    let mut retagging = Retagging::default();
    let mut sleeve = Sleeve::default();
    let mut paging = Paging::by(ROWS_A_PAGE);
    while !progress.is_cancelled() {
        let Some(page) = paging.next(
            |after, at_most| library.tracks_to_tag_after(&options.roots, after, at_most, &totals),
            |row| row.path.as_path(),
        )?
        else {
            break;
        };

        let mut planned = planned(library, &page, tags, progress, &mut sleeve);
        if options.apply {
            apply(library, tags, &mut planned, progress, &mut noted)?;
        }
        for write in &mut planned.writes {
            write.was.picture = None;
        }
        retagging.writes.append(&mut planned.writes);
        retagging.passed_over.append(&mut planned.passed_over);
    }

    Ok(RetagSummary {
        stats: progress.snapshot(),
        retagging,
        cancelled: progress.is_cancelled(),
    })
}

#[derive(Clone, Debug)]
pub(crate) struct TrackToTag {
    pub id: TrackId,
    pub path: PathBuf,
    pub album_id: Option<AlbumId>,
    pub cut: bool,
    pub vaulted: bool,
    pub answered: bool,
    pub title: String,
    pub artist: Option<String>,
    pub artist_sort: Option<String>,
    pub artist_mbid: Option<String>,
    pub mbid: Option<String>,
    pub release_track_mbid: Option<String>,
    pub isrc: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub album_answered: bool,
    pub album: Option<String>,
    pub album_artist_answered: bool,
    pub album_artist: Option<String>,
    pub album_artist_sort: Option<String>,
    pub album_artist_mbid: Option<String>,
    pub album_mbid: Option<String>,
    pub release_group: Option<String>,
    pub date: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub track_total: Option<u32>,
    pub disc_total: Option<u32>,
    pub popularity: Popularity,
}

#[derive(Default)]
struct Sleeve {
    album: Option<AlbumId>,
    picture: Option<Arc<CoverArt>>,
}

impl Sleeve {
    fn of(&mut self, library: &Library, album: AlbumId) -> Option<Arc<CoverArt>> {
        if self.album != Some(album) {
            self.album = Some(album);
            self.picture = match library.cover_art(album) {
                Ok(held) => held.map(Arc::new),
                Err(source) => {
                    tracing::warn!(
                        %source,
                        "an album's cover could not be read, so nothing is written from it"
                    );
                    None
                }
            };
        }

        self.picture.clone()
    }
}

fn planned(
    library: &Library,
    rows: &[TrackToTag],
    tags: &dyn TagSink,
    progress: &RetagProgress,
    sleeve: &mut Sleeve,
) -> Retagging {
    let mut retagging = Retagging::default();
    for group in rows.chunk_by(|one, next| one.path == next.path) {
        if progress.is_cancelled() {
            break;
        }

        RetagProgress::step(&progress.walked);
        let [row] = group else {
            retagging.pass_over(&group[0].path, Unwritten::Cut, progress);
            continue;
        };

        if row.cut {
            retagging.pass_over(&row.path, Unwritten::Cut, progress);
            continue;
        }

        if row.vaulted {
            retagging.pass_over(&row.path, Unwritten::Vaulted, progress);
            continue;
        }

        let location = MediaLocation::local(&row.path);
        if !tags.writes(&location) {
            retagging.pass_over(&row.path, Unwritten::Unsupported, progress);
            continue;
        }

        let sleeved = row.album_id.and_then(|album| sleeve.of(library, album));
        let picturing = match sleeved {
            Some(_) => Picturing::Copied,
            None => Picturing::Whether,
        };
        let held = match tags.read(&location, picturing) {
            Ok(held) => held,
            Err(source) => {
                tracing::debug!(
                    path = %row.path.display(),
                    %source,
                    "a file's own tags could not be read, so nothing is written to it"
                );
                retagging.pass_over(&row.path, Unwritten::Unreadable, progress);
                continue;
            }
        };

        let edits = wanted(row, &held.tags);
        let (picture, replaced) = offered_picture(held.picture, sleeved);
        let (popularity, rated) = rated_otherwise(tags, &location, row.popularity);
        if edits.is_empty() && picture.is_none() && popularity.is_none() {
            RetagProgress::step(&progress.unchanged);
            continue;
        }

        let was = Held {
            picture: replaced,
            ..Held::of(
                &held.tags,
                edits.iter().map(|edit| edit.field),
                popularity.and(rated),
            )
        };
        retagging.writes.push(Written {
            track: row.id,
            path: row.path.clone(),
            edits,
            taken: Vec::new(),
            picture,
            unpictured: false,
            popularity,
            was,
        });
    }

    retagging
}

fn rated_otherwise(
    tags: &dyn TagSink,
    location: &MediaLocation,
    wanted: Popularity,
) -> (Option<Popularity>, Option<Rated>) {
    match tags.rated(location) {
        Ok(held) => (held.differs_from(wanted).then_some(wanted), Some(held)),
        Err(source) => {
            tracing::debug!(
                %location,
                %source,
                "a file's rating could not be read, so none is written to it"
            );
            (None, None)
        }
    }
}

fn offered_picture(
    carried: Pictured,
    sleeved: Option<Arc<CoverArt>>,
) -> (Option<Arc<CoverArt>>, Option<Arc<CoverArt>>) {
    match carried {
        Pictured::Copied(held) if bettered(&held, sleeved.as_deref()) => {
            (sleeved, Some(Arc::new(held)))
        }
        Pictured::Copied(_) | Pictured::Carried => (None, None),
        Pictured::Bare | Pictured::StoodIn => (sleeved, None),
    }
}

fn bettered(held: &CoverArt, sleeved: Option<&CoverArt>) -> bool {
    sleeved.is_some_and(|sleeved| store::betters(sleeved, held))
}

const KEPT_AS_THE_FILE_SPELLS_IT: [TagField; 2] = [TagField::ArtistSort, TagField::AlbumArtistSort];

fn wanted(row: &TrackToTag, held: &TagSet) -> Vec<TagEdit> {
    offered(row)
        .into_iter()
        .filter(|(field, _)| {
            !KEPT_AS_THE_FILE_SPELLS_IT.contains(field) || field.read(held).is_none()
        })
        .filter(|(field, value)| field.read(held).as_deref() != Some(value.as_str()))
        .map(|(field, value)| TagEdit { field, value })
        .collect()
}

fn offered(row: &TrackToTag) -> Vec<(TagField, String)> {
    let mut offered = Vec::new();
    if row.answered {
        offer(&mut offered, TagField::Title, Some(&row.title));
        offer(&mut offered, TagField::Artist, row.artist.as_deref());
    }

    offer(&mut offered, TagField::Isrc, row.isrc.as_deref());
    offer(
        &mut offered,
        TagField::MusicBrainzTrackId,
        row.mbid.as_deref(),
    );
    offer(
        &mut offered,
        TagField::MusicBrainzReleaseTrackId,
        row.release_track_mbid.as_deref(),
    );
    offer(
        &mut offered,
        TagField::MusicBrainzArtistId,
        row.artist_mbid.as_deref(),
    );
    offer(
        &mut offered,
        TagField::ArtistSort,
        row.artist_sort.as_deref(),
    );
    counted(&mut offered, TagField::TrackNumber, row.track_number);
    counted(&mut offered, TagField::DiscNumber, row.disc_number);

    if row.album_answered {
        offer(&mut offered, TagField::Album, row.album.as_deref());
        counted(&mut offered, TagField::TrackTotal, row.track_total);
        counted(&mut offered, TagField::DiscTotal, row.disc_total);
    }

    if row.album_artist_answered {
        offer(
            &mut offered,
            TagField::AlbumArtist,
            row.album_artist.as_deref(),
        );
        offer(
            &mut offered,
            TagField::MusicBrainzAlbumArtistId,
            row.album_artist_mbid.as_deref(),
        );
        offer(
            &mut offered,
            TagField::AlbumArtistSort,
            row.album_artist_sort.as_deref(),
        );
    }

    offer(
        &mut offered,
        TagField::MusicBrainzAlbumId,
        row.album_mbid.as_deref(),
    );
    offer(
        &mut offered,
        TagField::MusicBrainzReleaseGroupId,
        row.release_group.as_deref(),
    );
    offer(&mut offered, TagField::Date, row.date.as_deref());
    offer(&mut offered, TagField::Label, row.label.as_deref());
    offer(
        &mut offered,
        TagField::CatalogNumber,
        row.catalog_number.as_deref(),
    );
    offer(&mut offered, TagField::Barcode, row.barcode.as_deref());
    offered
}

fn offer(into: &mut Vec<(TagField, String)>, field: TagField, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        into.push((field, value.to_owned()));
    }
}

fn counted(into: &mut Vec<(TagField, String)>, field: TagField, value: Option<u32>) {
    if let Some(value) = value.filter(|value| *value > 0) {
        into.push((field, value.to_string()));
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Followed {
    pub track: TrackId,
    pub title: Option<Option<String>>,
    pub artist: Option<Option<String>>,
    pub file_size: u64,
    pub modified: SystemTime,
}

#[derive(Clone, Debug)]
pub(crate) struct Undoing {
    pub path: PathBuf,
    pub pictured: bool,
    pub was: Held,
}

impl Undoing {
    fn of(write: &Written) -> Self {
        Self {
            path: write.path.clone(),
            pictured: write.picture.is_some(),
            was: write.was.clone(),
        }
    }
}

#[derive(Default)]
pub(crate) struct Noted {
    begun: bool,
    pictures: KeptPictures,
    walking_back: Option<AHashMap<PathBuf, KeptRetag>>,
}

impl Noted {
    fn keeps_a_note_of(&self, why: Unwritten) -> bool {
        self.walking_back.is_none() && why == Unwritten::Unconfirmed
    }

    fn noted_before(&self, unwritten: &[PathBuf]) -> Option<Vec<KeptRetag>> {
        let kept = self.walking_back.as_ref()?;

        Some(
            unwritten
                .iter()
                .filter_map(|path| kept.get(path).cloned())
                .collect(),
        )
    }
}

const PICTURES_WEIGHED_AGAINST: usize = 8;

#[derive(Default)]
pub(crate) struct KeptPictures {
    recent: VecDeque<(Arc<CoverArt>, i64)>,
}

impl KeptPictures {
    fn id_of(&mut self, tx: &Transaction<'_>, picture: &Arc<CoverArt>) -> Result<i64> {
        if let Some((_, id)) = self
            .recent
            .iter()
            .find(|(held, _)| Arc::ptr_eq(held, picture) || held.bytes == picture.bytes)
        {
            return Ok(*id);
        }

        tx.prepare_cached("INSERT INTO retagged_pictures (picture) VALUES (?1)")
            .and_then(|mut statement| statement.execute(params![picture.bytes.as_slice()]))
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        let id = tx.last_insert_rowid();
        self.recent.push_front((Arc::clone(picture), id));
        self.recent.truncate(PICTURES_WEIGHED_AGAINST);
        Ok(id)
    }
}

fn apply(
    library: &Library,
    tags: &dyn TagSink,
    retagging: &mut Retagging,
    progress: &RetagProgress,
    noted: &mut Noted,
) -> Result<Vec<PathBuf>> {
    let planned = mem::take(&mut retagging.writes);
    if planned.is_empty() || progress.is_cancelled() {
        retagging.writes = planned;
        return Ok(Vec::new());
    }
    let undoing: Vec<Undoing> = planned.iter().map(Undoing::of).collect();
    let begins = !noted.begun;
    library.retag_to_be_written(&undoing, begins, &mut noted.pictures)?;
    noted.begun = true;

    let mut followed = Vec::with_capacity(planned.len());
    let mut landed = Vec::with_capacity(planned.len());
    let mut unwritten = Vec::new();
    for write in planned {
        if progress.is_cancelled() {
            unwritten.push(write.path.clone());
            retagging.writes.push(write);
            continue;
        }

        match written(tags, &write) {
            Ok(follow) => {
                RetagProgress::step(&progress.written);
                progress.fields.fetch_add(
                    (write.edits.len() + write.taken.len()) as u64,
                    Ordering::Relaxed,
                );
                if write.picture.is_some() {
                    RetagProgress::step(&progress.pictures);
                }
                if write.popularity.is_some() {
                    RetagProgress::step(&progress.ratings);
                }
                followed.push(follow);
                landed.push(write.path.clone());
                retagging.writes.push(write);
            }
            Err(why) => {
                if !noted.keeps_a_note_of(why) {
                    unwritten.push(write.path.clone());
                }
                retagging.pass_over(&write.path, why, progress);
            }
        }
    }

    let noted_again = noted.noted_before(&unwritten);
    library.files_retagged(&followed, &unwritten, noted_again.as_deref())?;
    Ok(landed)
}

fn written(tags: &dyn TagSink, write: &Written) -> std::result::Result<Followed, Unwritten> {
    let location = MediaLocation::local(&write.path);
    if let Err(source) = tags.write(&location, write.writing()) {
        tracing::warn!(
            path = %write.path.display(),
            %source,
            "a file's tags could not be written"
        );
        return Err(Unwritten::Refused);
    }

    let picturing = match write.picture {
        Some(_) => Picturing::Copied,
        None => Picturing::Whether,
    };
    let held = tags.read(&location, picturing).map_err(|source| {
        tracing::warn!(
            path = %write.path.display(),
            %source,
            "a file could not be read back after its tags were written"
        );
        Unwritten::Unconfirmed
    })?;

    for edit in &write.edits {
        if edit.field.read(&held.tags).as_deref() != Some(edit.value.as_str()) {
            tracing::warn!(
                path = %write.path.display(),
                field = %edit.field,
                "a tag this build wrote does not read back, so the catalog does not follow it"
            );
            return Err(Unwritten::Unconfirmed);
        }
    }
    for field in &write.taken {
        if field.read(&held.tags).is_some() {
            tracing::warn!(
                path = %write.path.display(),
                %field,
                "a tag this build took away still reads back, so the catalog does not follow it"
            );
            return Err(Unwritten::Unconfirmed);
        }
    }

    if let Some(popularity) = write.popularity {
        let rated = tags.rated(&location).map_err(|source| {
            tracing::warn!(
                path = %write.path.display(),
                %source,
                "a file's rating could not be read back after it was written"
            );
            Unwritten::Unconfirmed
        })?;
        if rated.differs_from(popularity) {
            tracing::warn!(
                path = %write.path.display(),
                "a rating this build wrote does not read back, so the catalog does not follow it"
            );
            return Err(Unwritten::Unconfirmed);
        }
    }

    if let Some(picture) = write.picture.as_deref() {
        let back = held.picture.into_copied();
        if back.as_ref() != Some(picture) {
            tracing::warn!(
                path = %write.path.display(),
                "a picture this build wrote does not read back, so the catalog does not follow it"
            );
            return Err(Unwritten::Unconfirmed);
        }
    }

    let stamped = fs::metadata(&write.path).map_err(|source| {
        tracing::warn!(
            path = %write.path.display(),
            %source,
            "a file could not be stamped after its tags were written"
        );
        Unwritten::Unconfirmed
    })?;

    Ok(Followed {
        track: write.track,
        title: wrote(write, TagField::Title),
        artist: wrote(write, TagField::Artist),
        file_size: stamped.len(),
        modified: stamped.modified().unwrap_or_else(|_| SystemTime::now()),
    })
}

fn wrote(write: &Written, field: TagField) -> Option<Option<String>> {
    if write.taken.contains(&field) {
        return Some(None);
    }
    write
        .edits
        .iter()
        .find(|edit| edit.field == field)
        .map(|edit| Some(edit.value.clone()))
}

pub(crate) fn files_retagged(
    tx: &Transaction<'_>,
    followed: &[Followed],
    unwritten: &[PathBuf],
    noted_again: Option<&[KeptRetag]>,
) -> Result<()> {
    let mut statement = tx
        .prepare(
            "UPDATE tracks
                SET tagged_title = CASE WHEN ?2 THEN ?3 ELSE tagged_title END,
                    tagged_artist = CASE WHEN ?4 THEN ?5 ELSE tagged_artist END,
                    file_size = ?6,
                    modified = ?7
              WHERE id = ?1",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let kept = StudiesKept::through(tx)?;
    for follow in followed {
        kept.hold(follow.track)?;
        statement
            .execute(params![
                follow.track.get() as i64,
                follow.title.is_some(),
                follow.title.clone().flatten(),
                follow.artist.is_some(),
                follow.artist.clone().flatten(),
                follow.file_size as i64,
                store::to_nanos(follow.modified)
            ])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        kept.put_back(follow.track)?;
    }
    kept.done()?;

    forget_the_notes_of(tx, unwritten)?;
    match noted_again {
        Some(noted_again) => note_again(tx, noted_again),
        None => Ok(()),
    }
}

struct StudiesKept<'t> {
    tx: &'t Transaction<'t>,
}

impl<'t> StudiesKept<'t> {
    const TABLES: [&'static str; 2] = ["track_studies", "unstudied"];

    fn through(tx: &'t Transaction<'t>) -> Result<Self> {
        for table in Self::TABLES {
            tx.execute_batch(&format!(
                "CREATE TEMP TABLE IF NOT EXISTS kept_{table} AS SELECT * FROM main.{table} WHERE 0"
            ))
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
        Ok(Self { tx })
    }

    fn hold(&self, track: TrackId) -> Result<()> {
        for table in Self::TABLES {
            self.tx
                .prepare_cached(&format!(
                    "INSERT INTO temp.kept_{table} SELECT * FROM main.{table} WHERE track_id = ?1"
                ))
                .and_then(|mut statement| statement.execute(params![track.get() as i64]))
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
        Ok(())
    }

    fn put_back(&self, track: TrackId) -> Result<()> {
        for table in Self::TABLES {
            self.tx
                .prepare_cached(&format!(
                    "INSERT OR REPLACE INTO main.{table} SELECT * FROM temp.kept_{table} WHERE track_id = ?1"
                ))
                .and_then(|mut statement| statement.execute(params![track.get() as i64]))
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
        Ok(())
    }

    fn done(self) -> Result<()> {
        for table in Self::TABLES {
            self.tx
                .execute_batch(&format!("DROP TABLE temp.kept_{table}"))
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
        }
        Ok(())
    }
}

pub(crate) fn walked_back(tx: &Transaction<'_>, written: &[PathBuf], finished: bool) -> Result<()> {
    if !finished {
        forget_the_notes_of(tx, written)?;
    }
    sweep_pictures_nothing_names(tx)
}

fn note_again(tx: &Transaction<'_>, kept: &[KeptRetag]) -> Result<()> {
    for held in kept {
        let path = store::path_text(&held.path)?;
        tx.prepare_cached(
            "INSERT OR REPLACE INTO retagged (path, pictured, rated, picture_id)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .and_then(|mut statement| {
            statement.execute(params![path, held.pictured, held.rated, held.picture])
        })
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
        for (field, was) in &held.fields {
            tx.prepare_cached(
                "INSERT OR REPLACE INTO retagged_fields (path, field, was) VALUES (?1, ?2, ?3)",
            )
            .and_then(|mut statement| statement.execute(params![path, field.as_str(), was]))
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
    }
    Ok(())
}

fn sweep_pictures_nothing_names(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM retagged_pictures
          WHERE id NOT IN (SELECT picture_id FROM retagged WHERE picture_id IS NOT NULL)",
        [],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(())
}

fn forget_the_notes_of(tx: &Transaction<'_>, unwritten: &[PathBuf]) -> Result<()> {
    let mut noted = tx
        .prepare_cached("DELETE FROM retagged WHERE path = ?1")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let mut fields = tx
        .prepare_cached("DELETE FROM retagged_fields WHERE path = ?1")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    for path in unwritten {
        let path = store::path_text(path)?;
        for statement in [&mut noted, &mut fields] {
            statement
                .execute(params![path])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
        }
    }
    Ok(())
}

fn rating_kept(rated: Option<Rated>) -> Option<i64> {
    let counted = |plays: Option<u64>| i64::try_from(plays.unwrap_or(0)).unwrap_or(i64::MAX - 1);
    match rated? {
        Rated::Unrateable => None,
        Rated::Unrated { plays } => Some(-counted(plays) - 1),
        Rated::Favourite { plays } => Some(counted(plays)),
    }
}

fn rating_read(kept: Option<i64>) -> Option<Popularity> {
    let kept = kept?;
    Some(if kept < 0 {
        Popularity {
            favourite: false,
            plays: (-(kept + 1)).unsigned_abs(),
        }
    } else {
        Popularity {
            favourite: true,
            plays: kept.unsigned_abs(),
        }
    })
}

pub(crate) fn note_what_was_there(
    tx: &Transaction<'_>,
    undoing: &[Undoing],
    begins: bool,
    pictures: &mut KeptPictures,
) -> Result<()> {
    if begins {
        tx.execute_batch(
            "DELETE FROM retagged; DELETE FROM retagged_fields; DELETE FROM retagged_pictures;",
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
        pictures.recent.clear();
    }
    for held in undoing {
        let path = store::path_text(&held.path)?;
        let picture = held
            .was
            .picture
            .as_ref()
            .map(|picture| pictures.id_of(tx, picture))
            .transpose()?;
        tx.prepare_cached(
            "INSERT OR REPLACE INTO retagged (path, pictured, rated, picture_id)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .and_then(|mut statement| {
            statement.execute(params![
                path,
                held.pictured,
                rating_kept(held.was.rated),
                picture
            ])
        })
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
        for (field, was) in &held.was.fields {
            tx.execute(
                "INSERT OR REPLACE INTO retagged_fields (path, field, was) VALUES (?1, ?2, ?3)",
                params![path, field.as_str(), was],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeptRetag {
    pub path: PathBuf,
    pub pictured: bool,
    pub rated: Option<i64>,
    pub picture: Option<i64>,
    pub fields: Vec<(TagField, Option<String>)>,
}

#[derive(Default)]
struct PicturesPutBack {
    read: AHashMap<i64, Option<Arc<CoverArt>>>,
}

impl PicturesPutBack {
    fn of(&mut self, library: &Library, id: i64) -> Result<Option<Arc<CoverArt>>> {
        if let Some(held) = self.read.get(&id) {
            return Ok(held.clone());
        }
        let read = library.retagged_picture(id)?.map(Arc::new);
        self.read.insert(id, read.clone());
        Ok(read)
    }
}

fn undone(
    library: &Library,
    noted: &[KeptRetag],
    tags: &dyn TagSink,
    progress: &RetagProgress,
) -> Result<Retagging> {
    let mut retagging = Retagging::default();
    let mut pictures = PicturesPutBack::default();
    for kept in noted {
        if progress.is_cancelled() {
            break;
        }
        RetagProgress::step(&progress.walked);
        let Some(track) = library.track_at(&kept.path, None)?.map(|track| track.id) else {
            retagging.pass_over(&kept.path, Unwritten::Unreadable, progress);
            continue;
        };
        let location = MediaLocation::local(&kept.path);
        let held = match tags.read(&location, Picturing::Whether) {
            Ok(held) => held,
            Err(source) => {
                tracing::debug!(
                    path = %kept.path.display(),
                    %source,
                    "a file's own tags could not be read, so nothing is put back into it"
                );
                retagging.pass_over(&kept.path, Unwritten::Unreadable, progress);
                continue;
            }
        };
        let popularity = rating_read(kept.rated);
        let put_back = match kept.picture {
            Some(id) => pictures.of(library, id)?,
            None => None,
        };
        let rated = popularity
            .is_some()
            .then(|| tags.rated(&location).ok())
            .flatten();
        let (edits, taken): (Vec<_>, Vec<_>) =
            kept.fields.iter().partition(|(_, was)| was.is_some());
        retagging.writes.push(Written {
            track,
            path: kept.path.clone(),
            was: Held::of(
                &held.tags,
                kept.fields.iter().map(|(field, _)| *field),
                rated,
            ),
            edits: edits
                .into_iter()
                .filter_map(|(field, was)| {
                    Some(TagEdit {
                        field: *field,
                        value: was.clone()?,
                    })
                })
                .collect(),
            taken: taken.into_iter().map(|(field, _)| *field).collect(),
            unpictured: kept.pictured && put_back.is_none(),
            picture: put_back,
            popularity,
        });
    }
    Ok(retagging)
}

pub(crate) fn retagged_picture(
    connection: &rusqlite::Connection,
    id: i64,
) -> Result<Option<CoverArt>> {
    let bytes: Option<Vec<u8>> = connection
        .query_row(
            "SELECT picture FROM retagged_pictures WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(bytes.and_then(|bytes| {
        Some(CoverArt {
            format: ImageFormat::sniff(&bytes)?,
            bytes,
        })
    }))
}

pub(crate) fn last_retag(connection: &rusqlite::Connection) -> Result<Vec<KeptRetag>> {
    let mut statement = connection
        .prepare(
            "SELECT r.path, r.pictured, r.rated, f.field, f.was, r.picture_id
               FROM retagged r LEFT JOIN retagged_fields f ON f.path = r.path
              ORDER BY r.path, f.field",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut kept: Vec<KeptRetag> = Vec::new();
    for (path, pictured, rated, field, was, picture) in rows {
        let path = PathBuf::from(path);
        if kept.last().is_none_or(|last| last.path != path) {
            kept.push(KeptRetag {
                path,
                pictured,
                rated,
                picture,
                fields: Vec::new(),
            });
        }
        if let (Some(last), Some(field)) =
            (kept.last_mut(), field.as_deref().and_then(TagField::named))
        {
            last.fields.push((field, was));
        }
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> TrackToTag {
        TrackToTag {
            id: TrackId::new(1).expect("a namable track"),
            path: PathBuf::from("/music/echoes.flac"),
            album_id: Some(AlbumId::new(1).expect("a namable album")),
            cut: false,
            vaulted: false,
            answered: true,
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_sort: None,
            artist_mbid: Some("83d91898-7763-47d7-b03b-b92132375c47".to_owned()),
            mbid: Some("b1a9c0de-1111-4222-8333-444455556666".to_owned()),
            release_track_mbid: None,
            isrc: None,
            track_number: Some(6),
            disc_number: None,
            album_answered: true,
            album: Some("Meddle".to_owned()),
            album_artist_answered: true,
            album_artist: Some("Pink Floyd".to_owned()),
            album_artist_sort: None,
            album_artist_mbid: None,
            album_mbid: Some("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f".to_owned()),
            release_group: None,
            date: Some("1971-10-30".to_owned()),
            label: None,
            catalog_number: None,
            barcode: None,
            track_total: Some(6),
            disc_total: Some(1),
            popularity: Popularity::default(),
        }
    }

    fn fields(edits: &[TagEdit]) -> Vec<TagField> {
        edits.iter().map(|edit| edit.field).collect()
    }

    #[test]
    fn a_tag_the_file_already_carries_is_not_written_again() {
        let row = row();
        let held = TagSet {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            album_artist: Some("Pink Floyd".to_owned()),
            track_number: Some(6),
            track_total: Some(6),
            disc_number: None,
            disc_total: Some(1),
            date: Some("1971-10-30".to_owned()),
            musicbrainz_track_id: Some("b1a9c0de-1111-4222-8333-444455556666".to_owned()),
            musicbrainz_artist_id: Some("83d91898-7763-47d7-b03b-b92132375c47".to_owned()),
            musicbrainz_album_id: Some("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f".to_owned()),
            ..TagSet::default()
        };

        assert_eq!(wanted(&row, &held), Vec::new());
    }

    #[test]
    fn a_sort_name_a_lookup_gave_is_written_only_where_the_file_names_none() {
        let row = TrackToTag {
            artist: Some("The Beatles".to_owned()),
            artist_sort: Some("Beatles, The".to_owned()),
            album_artist: Some("The Beatles".to_owned()),
            album_artist_sort: Some("Beatles, The".to_owned()),
            ..row()
        };
        let unsorted = TagSet::default();
        let sorted_by_its_tagger = TagSet {
            artist_sort: Some("Beatles".to_owned()),
            album_artist_sort: Some("Beatles".to_owned()),
            ..TagSet::default()
        };

        let written = fields(&wanted(&row, &unsorted));
        let kept = fields(&wanted(&row, &sorted_by_its_tagger));

        assert!(written.contains(&TagField::ArtistSort), "{written:?}");
        assert!(written.contains(&TagField::AlbumArtistSort), "{written:?}");
        assert!(!kept.contains(&TagField::ArtistSort), "{kept:?}");
        assert!(!kept.contains(&TagField::AlbumArtistSort), "{kept:?}");
    }

    #[test]
    fn an_album_artists_sort_name_waits_for_the_artist_to_be_answered() {
        let row = TrackToTag {
            album_artist_answered: false,
            album_artist_sort: Some("Floyd, Pink".to_owned()),
            ..row()
        };

        assert!(!fields(&wanted(&row, &TagSet::default())).contains(&TagField::AlbumArtistSort));
    }

    #[test]
    fn only_the_fields_that_differ_are_written() {
        let row = row();
        let held = TagSet {
            title: Some("Echos".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            album_artist: Some("Pink Floyd".to_owned()),
            track_number: Some(6),
            track_total: Some(6),
            disc_total: Some(1),
            date: Some("1971-10-30".to_owned()),
            musicbrainz_track_id: Some("b1a9c0de-1111-4222-8333-444455556666".to_owned()),
            musicbrainz_artist_id: Some("83d91898-7763-47d7-b03b-b92132375c47".to_owned()),
            musicbrainz_album_id: Some("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f".to_owned()),
            ..TagSet::default()
        };

        assert_eq!(fields(&wanted(&row, &held)), vec![TagField::Title]);
    }

    #[test]
    fn a_row_no_lookup_answered_offers_neither_of_the_names_it_read_off_the_file() {
        let row = TrackToTag {
            answered: false,
            album_answered: false,
            album_artist_answered: false,
            ..row()
        };

        let named = fields(&wanted(&row, &TagSet::default()));
        assert!(!named.contains(&TagField::Title), "{named:?}");
        assert!(!named.contains(&TagField::Artist), "{named:?}");
        assert!(!named.contains(&TagField::Album), "{named:?}");
        assert!(!named.contains(&TagField::AlbumArtist), "{named:?}");
        assert!(
            named.contains(&TagField::MusicBrainzTrackId),
            "an identifier is never a guess and is offered whatever answered: {named:?}"
        );
    }

    #[test]
    fn a_blank_value_names_nothing_and_is_never_written() {
        let row = TrackToTag {
            title: "   ".to_owned(),
            artist: Some(String::new()),
            ..row()
        };

        let named = fields(&wanted(&row, &TagSet::default()));
        assert!(!named.contains(&TagField::Title), "{named:?}");
        assert!(!named.contains(&TagField::Artist), "{named:?}");
    }

    #[test]
    fn what_was_written_is_what_the_catalog_follows() {
        let write = Written {
            track: TrackId::new(1).expect("a namable track"),
            path: PathBuf::from("/music/echoes.flac"),
            edits: vec![TagEdit {
                field: TagField::Title,
                value: "Echoes".to_owned(),
            }],
            taken: vec![TagField::Album],
            picture: None,
            unpictured: false,
            popularity: None,
            was: Held::default(),
        };

        assert_eq!(
            wrote(&write, TagField::Title),
            Some(Some("Echoes".to_owned()))
        );
        assert_eq!(wrote(&write, TagField::Album), Some(None));
        assert_eq!(wrote(&write, TagField::Artist), None);
    }

    #[test]
    fn a_rating_is_kept_and_read_back_as_the_popularity_that_puts_it_back() {
        assert_eq!(rating_kept(None), None);
        assert_eq!(rating_kept(Some(Rated::Unrateable)), None);
        for (rated, back) in [
            (Rated::Unrated { plays: None }, Popularity::default()),
            (Rated::Unrated { plays: Some(0) }, Popularity::default()),
            (
                Rated::Unrated { plays: Some(4) },
                Popularity {
                    favourite: false,
                    plays: 4,
                },
            ),
            (
                Rated::Favourite { plays: Some(7) },
                Popularity {
                    favourite: true,
                    plays: 7,
                },
            ),
            (
                Rated::Favourite { plays: None },
                Popularity {
                    favourite: true,
                    plays: 0,
                },
            ),
        ] {
            assert_eq!(rating_read(rating_kept(Some(rated))), Some(back));
        }
    }
}
