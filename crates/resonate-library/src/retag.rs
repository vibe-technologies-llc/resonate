use std::{
    fs, mem,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::SystemTime,
};

use resonate_codec::{CoverArt, Pictured, Picturing, TagEdit, TagField, TagSet, TagSink, Writing};
use resonate_core::{AlbumId, MediaLocation, TrackId};
use rusqlite::{Transaction, params};

use crate::{
    Library, StoreOp,
    error::{Error, Result},
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
    pub picture: Option<Arc<CoverArt>>,
}

impl Written {
    fn writing(&self) -> Writing<'_> {
        Writing {
            edits: &self.edits,
            picture: self.picture.as_deref(),
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetagStats {
    pub walked: u64,
    pub written: u64,
    pub fields: u64,
    pub pictures: u64,
    pub unchanged: u64,
    pub passed_over: u64,
}

#[derive(Debug, Default)]
pub struct RetagProgress {
    walked: AtomicU64,
    written: AtomicU64,
    fields: AtomicU64,
    pictures: AtomicU64,
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

    let rows = library.tracks_to_tag(&options.roots)?;
    let mut retagging = planned(library, &rows, tags, progress);
    if options.apply {
        apply(library, tags, &mut retagging, progress)?;
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
    pub album_artist_mbid: Option<String>,
    pub album_mbid: Option<String>,
    pub release_group: Option<String>,
    pub date: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub track_total: Option<u32>,
    pub disc_total: Option<u32>,
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
) -> Retagging {
    let mut retagging = Retagging::default();
    let mut sleeve = Sleeve::default();
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

        let held = match tags.read(&location, Picturing::Whether) {
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
        let picture = offered_picture(library, &held.picture, row, &mut sleeve);
        if edits.is_empty() && picture.is_none() {
            RetagProgress::step(&progress.unchanged);
            continue;
        }

        retagging.writes.push(Written {
            track: row.id,
            path: row.path.clone(),
            edits,
            picture,
        });
    }

    retagging
}

fn offered_picture(
    library: &Library,
    carried: &Pictured,
    row: &TrackToTag,
    sleeve: &mut Sleeve,
) -> Option<Arc<CoverArt>> {
    if carried.carries_one() {
        return None;
    }
    sleeve.of(library, row.album_id?)
}

fn wanted(row: &TrackToTag, held: &TagSet) -> Vec<TagEdit> {
    offered(row)
        .into_iter()
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
    pub title: Option<String>,
    pub artist: Option<String>,
    pub file_size: u64,
    pub modified: SystemTime,
}

fn apply(
    library: &Library,
    tags: &dyn TagSink,
    retagging: &mut Retagging,
    progress: &RetagProgress,
) -> Result<()> {
    let planned = mem::take(&mut retagging.writes);
    let mut followed = Vec::with_capacity(planned.len());

    for write in planned {
        if progress.is_cancelled() {
            retagging.writes.push(write);
            continue;
        }

        match written(tags, &write) {
            Ok(follow) => {
                RetagProgress::step(&progress.written);
                progress
                    .fields
                    .fetch_add(write.edits.len() as u64, Ordering::Relaxed);
                if write.picture.is_some() {
                    RetagProgress::step(&progress.pictures);
                }
                followed.push(follow);
                retagging.writes.push(write);
            }
            Err(why) => retagging.pass_over(&write.path, why, progress),
        }
    }

    library.files_retagged(&followed)
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

fn wrote(write: &Written, field: TagField) -> Option<String> {
    write
        .edits
        .iter()
        .find(|edit| edit.field == field)
        .map(|edit| edit.value.clone())
}

pub(crate) fn files_retagged(tx: &Transaction<'_>, followed: &[Followed]) -> Result<()> {
    let mut statement = tx
        .prepare(
            "UPDATE tracks
                SET tagged_title = coalesce(?2, tagged_title),
                    tagged_artist = coalesce(?3, tagged_artist),
                    file_size = ?4,
                    modified = ?5
              WHERE id = ?1",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    for follow in followed {
        statement
            .execute(params![
                follow.track.get() as i64,
                follow.title,
                follow.artist,
                follow.file_size as i64,
                store::to_nanos(follow.modified)
            ])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }

    Ok(())
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
            album_artist_mbid: None,
            album_mbid: Some("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f".to_owned()),
            release_group: None,
            date: Some("1971-10-30".to_owned()),
            label: None,
            catalog_number: None,
            barcode: None,
            track_total: Some(6),
            disc_total: Some(1),
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
            picture: None,
        };

        assert_eq!(wrote(&write, TagField::Title).as_deref(), Some("Echoes"));
        assert_eq!(wrote(&write, TagField::Artist), None);
    }
}
