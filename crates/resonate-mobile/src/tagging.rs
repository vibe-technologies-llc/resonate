use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;
use resonate_codec::{
    FileTags, Picturing, TagEdit, TagField, TagSet, TagSink, TagSource, Writing, probe,
};
use resonate_core::MediaLocation;
use resonate_library::named_by_the_file;
use rusqlite::params;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::Catalog,
    documents::location,
    error::RewriteOp,
    rewriting::{DocumentRewriting, Stage, crc_of_file, pending_under, put_back},
    saving_lyrics::SheetsBeside,
};

const UNDO_STEPS_KEPT: usize = 16;
const EDITABLE: [TagField; 11] = [
    TagField::Title,
    TagField::Artist,
    TagField::Album,
    TagField::AlbumArtist,
    TagField::Genre,
    TagField::Date,
    TagField::TrackNumber,
    TagField::TrackTotal,
    TagField::DiscNumber,
    TagField::DiscTotal,
    TagField::Composer,
];
const NUMBERED: [TagField; 4] = [
    TagField::TrackNumber,
    TagField::TrackTotal,
    TagField::DiscNumber,
    TagField::DiscTotal,
];

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct SongTags {
    #[uniffi(default)]
    pub title: Option<String>,
    #[uniffi(default)]
    pub artist: Option<String>,
    #[uniffi(default)]
    pub album: Option<String>,
    #[uniffi(default)]
    pub album_artist: Option<String>,
    #[uniffi(default)]
    pub genre: Option<String>,
    #[uniffi(default)]
    pub date: Option<String>,
    #[uniffi(default)]
    pub track_number: Option<String>,
    #[uniffi(default)]
    pub track_total: Option<String>,
    #[uniffi(default)]
    pub disc_number: Option<String>,
    #[uniffi(default)]
    pub disc_total: Option<String>,
    #[uniffi(default)]
    pub composer: Option<String>,
}

impl SongTags {
    fn read(tags: &TagSet) -> Self {
        let mut read = Self::default();

        for field in EDITABLE {
            *read.slot(field) = Some(field.read(tags).unwrap_or_default());
        }
        read
    }

    fn slot(&mut self, field: TagField) -> &mut Option<String> {
        match field {
            TagField::Title => &mut self.title,
            TagField::Artist => &mut self.artist,
            TagField::Album => &mut self.album,
            TagField::AlbumArtist => &mut self.album_artist,
            TagField::Genre => &mut self.genre,
            TagField::Date => &mut self.date,
            TagField::TrackNumber => &mut self.track_number,
            TagField::TrackTotal => &mut self.track_total,
            TagField::DiscNumber => &mut self.disc_number,
            TagField::DiscTotal => &mut self.disc_total,
            _ => &mut self.composer,
        }
    }

    fn wanted(&self, field: TagField) -> Option<&String> {
        match field {
            TagField::Title => self.title.as_ref(),
            TagField::Artist => self.artist.as_ref(),
            TagField::Album => self.album.as_ref(),
            TagField::AlbumArtist => self.album_artist.as_ref(),
            TagField::Genre => self.genre.as_ref(),
            TagField::Date => self.date.as_ref(),
            TagField::TrackNumber => self.track_number.as_ref(),
            TagField::TrackTotal => self.track_total.as_ref(),
            TagField::DiscNumber => self.disc_number.as_ref(),
            TagField::DiscTotal => self.disc_total.as_ref(),
            _ => self.composer.as_ref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SongFile {
    pub uri: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct TagsWritten {
    pub written: u32,
    pub unchanged: u32,
    pub failed: u32,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) tags: SongTags,
    pub(crate) lyrics: Option<String>,
}

impl From<SongTags> for Change {
    fn from(tags: SongTags) -> Self {
        Self { tags, lyrics: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Planned {
    edits: Vec<TagEdit>,
    taken: Vec<TagField>,
    before: Change,
}

#[derive(Clone, Debug)]
pub(crate) struct Undone {
    pub(crate) uri: String,
    pub(crate) before: Change,
}

pub(crate) struct Tagging {
    folder: PathBuf,
    steps: Mutex<Vec<Vec<Undone>>>,
    pub(crate) rewriting: Mutex<Option<Arc<dyn DocumentRewriting>>>,
    pub(crate) beside: Mutex<Option<Arc<dyn SheetsBeside>>>,
}

impl Tagging {
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }

    pub(crate) fn under(folder: PathBuf) -> Self {
        Self {
            folder,
            steps: Mutex::new(Vec::new()),
            rewriting: Mutex::new(None),
            beside: Mutex::new(None),
        }
    }

    pub(crate) fn keep(&self, step: Vec<Undone>) {
        if step.is_empty() {
            return;
        }

        let mut steps = self.steps.lock();

        steps.push(step);
        if steps.len() > UNDO_STEPS_KEPT {
            steps.remove(0);
        }
    }
}

struct Retagged {
    before: Change,
    written: TagSet,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn song_tags(&self, id: u64) -> Result<SongTags> {
        let track = self.catalog.track(id)?;
        let tags = probe(&self.sources, &location(&track.uri)?)?.tags;
        Ok(SongTags::read(&tags))
    }

    pub fn rewrite_documents_with(&self, rewriting: Box<dyn DocumentRewriting>) {
        let rewriting: Arc<dyn DocumentRewriting> = Arc::from(rewriting);

        *self.tagging.rewriting.lock() = Some(Arc::clone(&rewriting));
        self.put_back_pending(&*rewriting);
    }

    pub fn tags_writable(&self, name: String) -> bool {
        FileTags::default().writes(&MediaLocation::local(PathBuf::from(name)))
    }

    pub fn edit_tags(&self, songs: Vec<SongFile>, change: SongTags) -> Result<TagsWritten> {
        let mut told = TagsWritten::default();
        let mut step = Vec::new();
        let change = Change::from(change);

        for song in &songs {
            match self.retag(song, &change) {
                Ok(Some(before)) => {
                    told.written += 1;
                    step.push(Undone {
                        uri: song.uri.clone(),
                        before,
                    });
                }
                Ok(None) => told.unchanged += 1,
                Err(error) => {
                    told.failed += 1;
                    told.failure.get_or_insert_with(|| error.to_string());
                }
            }
        }
        self.tagging.keep(step);
        Ok(told)
    }

    pub fn tags_to_undo(&self) -> Vec<String> {
        self.tagging
            .steps
            .lock()
            .last()
            .map(|step| step.iter().map(|undone| undone.uri.clone()).collect())
            .unwrap_or_default()
    }

    pub fn undo_tags(&self, songs: Vec<SongFile>) -> Result<TagsWritten> {
        let Some(step) = self.tagging.steps.lock().pop() else {
            return Ok(TagsWritten::default());
        };
        let mut told = TagsWritten::default();
        let mut unfinished = Vec::new();

        for undone in step {
            let Some(song) = songs.iter().find(|song| song.uri == undone.uri) else {
                told.failed += 1;
                unfinished.push(undone);
                continue;
            };
            match self.retag(song, &undone.before) {
                Ok(Some(_)) => told.written += 1,
                Ok(None) => told.unchanged += 1,
                Err(error) => {
                    told.failed += 1;
                    told.failure.get_or_insert_with(|| error.to_string());
                    unfinished.push(undone);
                }
            }
        }
        self.tagging.keep(unfinished);
        Ok(told)
    }
}

impl MobilePlayer {
    pub(crate) fn retag(&self, song: &SongFile, change: &Change) -> Result<Option<Change>> {
        let track = self.catalog.track_uri(&song.uri)?;
        let retagged = match song.path.as_str() {
            "" => self.retag_document(&track, song, change)?,
            path => retag_at(&MediaLocation::local(PathBuf::from(path)), change)?,
        };
        let Some(Retagged { before, written }) = retagged else {
            return Ok(None);
        };

        self.catalog.retagged(&track, &written)?;
        self.services.analyses.lock().remove(&track.id);
        Ok(Some(before))
    }

    fn retag_document(
        &self,
        track: &Track,
        song: &SongFile,
        change: &Change,
    ) -> Result<Option<Retagged>> {
        let rewriting = self
            .tagging
            .rewriting
            .lock()
            .clone()
            .ok_or(Error::TagsUnwritable)?;

        if let Some(unrestored) = self
            .put_back_pending(&*rewriting)
            .into_iter()
            .find(|unrestored| unrestored.uri == song.uri)
        {
            return Err(Error::TagsUnrestored {
                kept: unrestored.kept,
            });
        }

        let at = location(&song.uri)?;
        let tags = FileTags::default();
        let mut stage = Stage::made(&self.tagging.folder, &track.name)?;

        let working = MediaLocation::local(stage.working());

        if !tags.writes(&working) {
            return Err(Error::TagsUnwritable);
        }

        self.copy_document(&at, &stage.working())?;

        let held = tags.read(&working, Picturing::Whether)?.tags;
        let Some(planned) = planned(&held, change)? else {
            return Ok(None);
        };

        let original = stage.set_aside()?;
        let written = write_planned(&tags, &working, &planned)?;
        let edited = crc_of_file(&stage.working())?;

        stage.pend(&song.uri, original)?;
        self.replace_document(&*rewriting, &song.uri, &mut stage, edited, original)?;
        Ok(Some(Retagged {
            before: planned.before,
            written,
        }))
    }

    fn copy_document(&self, at: &MediaLocation, into: &Path) -> Result<()> {
        let copied = |source| Error::TagsDocument {
            op: RewriteOp::CopiedAside,
            source,
        };
        let mut media = self.sources.open(at)?;
        let mut file = File::create_new(into).map_err(copied)?;

        io::copy(&mut media.stream, &mut file).map_err(copied)?;
        Ok(())
    }

    fn replace_document(
        &self,
        rewriting: &dyn DocumentRewriting,
        uri: &str,
        stage: &mut Stage,
        edited: u32,
        original: u32,
    ) -> Result<()> {
        let failure = match put_back(rewriting, uri, &stage.working()) {
            Err(error) => error,
            Ok(()) if self.whole_crc(uri) == Some(edited) => {
                stage.settled();
                return Ok(());
            }
            Ok(()) => Error::TagsUnconfirmed,
        };

        if self.restored(rewriting, uri, &stage.original(), original) {
            stage.settled();
            return Err(failure);
        }

        let kept = stage.kept_aside(&self.tagging.folder);

        tracing::error!(path = %kept.display(), "a song could not be put back after a failed tag write");
        Err(Error::TagsUnrestored { kept })
    }

    fn restored(
        &self,
        rewriting: &dyn DocumentRewriting,
        uri: &str,
        original: &Path,
        crc: u32,
    ) -> bool {
        if self.whole_crc(uri) == Some(crc) {
            return true;
        }
        put_back(rewriting, uri, original).is_ok() && self.whole_crc(uri) == Some(crc)
    }

    fn put_back_pending(&self, rewriting: &dyn DocumentRewriting) -> Vec<Unrestored> {
        let mut unrestored = Vec::new();

        for pending in pending_under(&self.tagging.folder) {
            let original = pending.original();
            let whole = crc_of_file(&original).ok() == Some(pending.original_crc);

            if whole && self.restored(rewriting, &pending.uri, &original, pending.original_crc) {
                pending.settled();
                continue;
            }

            tracing::error!(uri = pending.uri, path = %original.display(), whole, "an interrupted tag rewrite could not be put back");
            unrestored.push(Unrestored {
                uri: pending.uri.clone(),
                kept: original,
            });
        }
        unrestored
    }
}

struct Unrestored {
    uri: String,
    kept: PathBuf,
}

fn retag_at(local: &MediaLocation, change: &Change) -> Result<Option<Retagged>> {
    let tags = FileTags::default();

    if !tags.writes(local) {
        return Err(Error::TagsUnwritable);
    }

    let held = tags.read(local, Picturing::Whether)?.tags;
    let Some(planned) = planned(&held, change)? else {
        return Ok(None);
    };

    let written = write_planned(&tags, local, &planned)?;
    Ok(Some(Retagged {
        before: planned.before,
        written,
    }))
}

fn write_planned(tags: &FileTags, local: &MediaLocation, planned: &Planned) -> Result<TagSet> {
    tags.write(
        local,
        Writing {
            edits: &planned.edits,
            taken: &planned.taken,
            picture: None,
            unpictured: false,
            popularity: None,
        },
    )?;

    let written = tags.read(local, Picturing::Whether)?.tags;

    if !landed(&written, planned) {
        return Err(Error::TagsUnconfirmed);
    }
    Ok(written)
}

fn planned(held: &TagSet, change: &Change) -> Result<Option<Planned>> {
    let mut edits = Vec::new();
    let mut taken = Vec::new();
    let mut before = Change::default();

    for field in EDITABLE {
        let Some(wanted) = change.tags.wanted(field) else {
            continue;
        };
        if let Some(was) =
            planned_field(held, field, spelled(field, wanted)?, &mut edits, &mut taken)
        {
            *before.tags.slot(field) = Some(was);
        }
    }
    if let Some(wanted) = &change.lyrics {
        let wanted = Some(wanted.trim()).filter(|wanted| !wanted.is_empty());

        before.lyrics = planned_field(
            held,
            TagField::Lyrics,
            wanted.map(str::to_owned),
            &mut edits,
            &mut taken,
        );
    }

    Ok((!edits.is_empty() || !taken.is_empty()).then_some(Planned {
        edits,
        taken,
        before,
    }))
}

fn planned_field(
    held: &TagSet,
    field: TagField,
    wanted: Option<String>,
    edits: &mut Vec<TagEdit>,
    taken: &mut Vec<TagField>,
) -> Option<String> {
    let current = field.read(held);

    match wanted {
        None if current.is_some() => taken.push(field),
        Some(value) if current.as_deref() != Some(value.as_str()) => {
            edits.push(TagEdit { field, value });
        }
        _ => return None,
    }
    Some(current.unwrap_or_default())
}

fn spelled(field: TagField, wanted: &str) -> Result<Option<String>> {
    let wanted = wanted.trim();

    if wanted.is_empty() {
        return Ok(None);
    }
    if NUMBERED.contains(&field) {
        let number: u32 = wanted
            .parse()
            .ok()
            .filter(|number| *number > 0)
            .ok_or(Error::TagNumber)?;

        return Ok(Some(number.to_string()));
    }
    Ok(Some(wanted.to_owned()))
}

fn landed(written: &TagSet, planned: &Planned) -> bool {
    planned
        .edits
        .iter()
        .all(|edit| edit.field.read(written).as_deref() == Some(edit.value.as_str()))
        && planned
            .taken
            .iter()
            .all(|field| field.read(written).is_none())
}

impl Catalog {
    fn retagged(&self, track: &Track, tags: &TagSet) -> Result<()> {
        let mut named = tags.clone();

        named_by_the_file(&track.name, &mut named);

        let title = named.title.unwrap_or_else(|| track.name.clone());
        self.0
            .lock()
            .execute(
                "UPDATE documents SET title = ?2, artist = ?3, album = ?4, album_artist = ?5, genre = ?6, date = ?7, track_number = ?8, disc_number = ?9 WHERE id = ?1",
                params![
                    track.id as i64,
                    title,
                    named.artist.unwrap_or_default(),
                    tags.album.clone().unwrap_or_default(),
                    tags.album_artist.clone().unwrap_or_default(),
                    tags.genre.clone().unwrap_or_default(),
                    tags.date.clone().unwrap_or_default(),
                    tags.track_number,
                    tags.disc_number
                ],
            )
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        env,
        fs::{self, OpenOptions},
        os::fd::IntoRawFd,
        process::{self, Command},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use resonate_codec::Sources;

    use super::*;
    use crate::{
        Documents, PlaybackPreferences,
        documents::{DOCUMENT_REFUSED, GrantedFiles},
        features::Services,
    };

    const URI: &str = "content://test/song.wav";
    const ENDING_UNDER: &str = "RESONATE_TAG_REWRITE_ENDING_UNDER";
    const ENDED_MID_WRITE: i32 = 75;

    fn player() -> MobilePlayer {
        MobilePlayer {
            playing: crate::zoning::Playing::closed(),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(Sources::local()),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: crate::sync::Syncing::under(std::env::temp_dir()),
            enriching: crate::enrichment::Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
            tagging: Tagging::under(std::env::temp_dir()),
            history_spool: crate::history::Spool::under(std::env::temp_dir()),
            served: Arc::new(crate::served::Served::default()),
            together: crate::together::Together::default(),
            rendering: crate::rendering::Rendering::default(),
            offered: crate::offering::Offered::untold(),
            releases: resonate_online::KeptRelease::in_folder(&std::env::temp_dir()),
        }
    }

    fn wave(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("resonate-tagging-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);

        fs::create_dir_all(&folder).expect("a folder");

        let rate: u32 = 44_100;
        let samples = vec![0i16; 4_410];
        let data = u32::try_from(samples.len() * 2).expect("a short wave");
        let mut bytes = Vec::new();

        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        let path = folder.join("song.wav");

        fs::write(&path, bytes).expect("a wave");
        path
    }

    fn held(player: &MobilePlayer) -> Track {
        player
            .catalog
            .upsert(Track {
                uri: URI.into(),
                name: "song.wav".into(),
                title: "song.wav".into(),
                duration_ms: 100,
                rate: 44_100,
                codec: "PCM".into(),
                ..Track::default()
            })
            .expect("a song")
    }

    #[test]
    fn tags_written_reach_the_file_and_the_catalog_and_one_undo_puts_them_back() {
        let player = player();
        let path = wave("written");
        let track = held(&player);
        let song = SongFile {
            uri: URI.into(),
            path: path.to_string_lossy().into_owned(),
        };
        let change = SongTags {
            title: Some("Slow Water".into()),
            artist: Some(" Example Band ".into()),
            track_number: Some("07".into()),
            ..SongTags::default()
        };

        let written = player
            .edit_tags(vec![song.clone()], change.clone())
            .expect("an edit");
        let retagged = player.catalog.track(track.id).expect("the song");
        let again = player
            .edit_tags(vec![song.clone()], change)
            .expect("the same edit");

        assert_eq!(written.written, 1);
        assert_eq!(retagged.title, "Slow Water");
        assert_eq!(retagged.artist, "Example Band");
        assert_eq!(retagged.track_number, Some(7));
        assert_eq!(again.unchanged, 1);
        assert_eq!(player.tags_to_undo(), vec![URI.to_owned()]);

        let undone = player.undo_tags(vec![song]).expect("an undo");
        let restored = player.catalog.track(track.id).expect("the song");

        assert_eq!(undone.written, 1);
        assert_eq!(restored.title, "song");
        assert!(restored.artist.is_empty());
        assert_eq!(restored.track_number, None);
    }

    #[test]
    fn a_retag_that_leaves_no_title_or_artist_names_the_song_as_the_scan_does() {
        let player = player();
        let path = wave("untitled");
        let track = player
            .catalog
            .upsert(Track {
                uri: URI.into(),
                name: "Example Band - Slow Water.wav".into(),
                title: "Slow Water".into(),
                artist: "Example Band".into(),
                duration_ms: 100,
                rate: 44_100,
                codec: "PCM".into(),
                ..Track::default()
            })
            .expect("a song");
        let song = SongFile {
            uri: URI.into(),
            path: path.to_string_lossy().into_owned(),
        };

        let written = player
            .edit_tags(
                vec![song],
                SongTags {
                    album: Some("Low Tide".into()),
                    ..SongTags::default()
                },
            )
            .expect("an edit");
        let retagged = player.catalog.track(track.id).expect("the song");

        assert_eq!(written.written, 1);
        assert_eq!(retagged.title, "Slow Water");
        assert_eq!(retagged.artist, "Example Band");
        assert_eq!(retagged.album, "Low Tide");
    }

    #[test]
    fn an_undo_that_leaves_a_song_out_keeps_its_tags_for_the_next_undo() {
        let player = player();
        let path = wave("left-out");
        let track = held(&player);
        let song = SongFile {
            uri: URI.into(),
            path: path.to_string_lossy().into_owned(),
        };
        let change = SongTags {
            title: Some("Slow Water".into()),
            ..SongTags::default()
        };

        player
            .edit_tags(vec![song.clone()], change)
            .expect("an edit");

        let skipped = player.undo_tags(Vec::new()).expect("an undo of nothing");

        assert_eq!(skipped.failed, 1);
        assert_eq!(player.tags_to_undo(), vec![URI.to_owned()]);

        let undone = player.undo_tags(vec![song]).expect("an undo");
        let restored = player.catalog.track(track.id).expect("the song");

        assert_eq!(undone.written, 1);
        assert_eq!(restored.title, "song");
        assert!(player.tags_to_undo().is_empty());
    }

    #[test]
    fn a_number_that_is_not_one_is_refused_and_the_file_is_left_alone() {
        let player = player();
        let path = wave("refused");

        held(&player);

        let before = fs::read(&path).expect("the wave");

        let refused = player
            .edit_tags(
                vec![SongFile {
                    uri: URI.into(),
                    path: path.to_string_lossy().into_owned(),
                }],
                SongTags {
                    disc_number: Some("two".into()),
                    ..SongTags::default()
                },
            )
            .expect("an answer");

        assert_eq!(refused.failed, 1);
        assert_eq!(
            refused.failure.as_deref(),
            Some("A track or disc number must be a whole number above zero")
        );
        assert_eq!(fs::read(&path).expect("the wave"), before);
        assert!(player.tags_to_undo().is_empty());
    }

    #[derive(Clone, Copy)]
    enum Reply {
        Real,
        TruncatedThenFull,
        TruncatedThenNowhere,
        TruncatedThenRefused,
        TruncatedThenEnded,
        Refused,
    }

    struct Directory {
        folder: PathBuf,
        script: Mutex<VecDeque<Reply>>,
        asked: AtomicUsize,
    }

    impl Directory {
        fn answering(folder: &Path, script: &[Reply]) -> Arc<Self> {
            Arc::new(Self {
                folder: folder.to_path_buf(),
                script: Mutex::new(script.iter().copied().collect()),
                asked: AtomicUsize::new(0),
            })
        }

        fn path_of(&self, uri: &str) -> PathBuf {
            self.folder
                .join(uri.strip_prefix("content://test/").expect("a test uri"))
        }

        fn truncated(&self, uri: &str) {
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(self.path_of(uri))
                .expect("a document to truncate");
        }

        fn device(path: &str) -> i32 {
            OpenOptions::new()
                .write(true)
                .open(path)
                .expect("a device")
                .into_raw_fd()
        }
    }

    impl Documents for Directory {
        fn open_document(&self, uri: String) -> i32 {
            fs::File::open(self.path_of(&uri)).map_or(DOCUMENT_REFUSED, IntoRawFd::into_raw_fd)
        }
    }

    struct Rewrites(Arc<Directory>);

    impl DocumentRewriting for Rewrites {
        fn rewrite_document(&self, uri: String) -> i32 {
            self.0.asked.fetch_add(1, Ordering::Relaxed);

            let reply = self.0.script.lock().pop_front().unwrap_or(Reply::Real);
            match reply {
                Reply::Real => OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(self.0.path_of(&uri))
                    .map_or(DOCUMENT_REFUSED, IntoRawFd::into_raw_fd),
                Reply::TruncatedThenFull => {
                    self.0.truncated(&uri);
                    Directory::device("/dev/full")
                }
                Reply::TruncatedThenNowhere => {
                    self.0.truncated(&uri);
                    Directory::device("/dev/null")
                }
                Reply::TruncatedThenRefused => {
                    self.0.truncated(&uri);
                    DOCUMENT_REFUSED
                }
                Reply::TruncatedThenEnded => {
                    self.0.truncated(&uri);
                    process::exit(ENDED_MID_WRITE)
                }
                Reply::Refused => DOCUMENT_REFUSED,
            }
        }
    }

    struct Edited {
        player: MobilePlayer,
        directory: Arc<Directory>,
        song: PathBuf,
        data: PathBuf,
        track: Track,
    }

    impl Edited {
        fn under(name: &str, script: &[Reply]) -> Self {
            Self::over(wave(name), script)
        }

        fn over(song: PathBuf, script: &[Reply]) -> Self {
            let folder = song.parent().expect("a folder").to_path_buf();
            let data = folder.join("data");
            let directory = Directory::answering(&folder, script);
            let mut player = player();

            player.sources = Arc::new(Sources::local().and(Arc::new(
                GrantedFiles::new(directory.clone(), "content").expect("a provider"),
            )));
            player.tagging = Tagging::under(data.clone());
            player.rewrite_documents_with(Box::new(Rewrites(directory.clone())));

            let track = held(&player);
            Self {
                player,
                directory,
                song,
                data,
                track,
            }
        }

        fn file(&self) -> SongFile {
            SongFile {
                uri: URI.into(),
                path: String::new(),
            }
        }

        fn tags(&self) -> TagSet {
            FileTags::default()
                .read(&MediaLocation::local(self.song.clone()), Picturing::Whether)
                .expect("the document's tags")
                .tags
        }

        fn title(&self) -> String {
            self.player
                .catalog
                .track(self.track.id)
                .expect("the song")
                .title
        }

        fn change() -> SongTags {
            SongTags {
                title: Some("Slow Water".into()),
                artist: Some("Example Band".into()),
                ..SongTags::default()
            }
        }

        fn relaunched(self, script: &[Reply]) -> Self {
            let song = self.song.clone();

            drop(self);
            Self::over(song, script)
        }

        fn staged(&self) -> Vec<PathBuf> {
            self.within("tag-staging")
        }

        fn unrestored(&self) -> Vec<PathBuf> {
            self.within("tag-unrestored")
        }

        fn within(&self, area: &str) -> Vec<PathBuf> {
            fs::read_dir(self.data.join(area))
                .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
                .unwrap_or_default()
        }

        fn edit(&self) -> TagsWritten {
            self.player
                .edit_tags(vec![self.file()], Self::change())
                .expect("an answer")
        }

        fn document(&self) -> Vec<u8> {
            fs::read(&self.song).expect("the document")
        }
    }

    #[test]
    fn tags_written_to_a_document_reach_it_through_a_staged_copy_and_one_undo_puts_them_back() {
        let edited = Edited::under("document", &[]);

        let written = edited
            .player
            .edit_tags(vec![edited.file()], Edited::change())
            .expect("an edit");
        let retagged = edited
            .player
            .catalog
            .track(edited.track.id)
            .expect("the song");
        let again = edited
            .player
            .edit_tags(vec![edited.file()], Edited::change())
            .expect("the same edit");

        assert_eq!(written.written, 1);
        assert_eq!(edited.tags().title.as_deref(), Some("Slow Water"));
        assert_eq!(edited.tags().artist.as_deref(), Some("Example Band"));
        assert_eq!(retagged.title, "Slow Water");
        assert_eq!(retagged.artist, "Example Band");
        assert_eq!(again.unchanged, 1);
        assert_eq!(edited.directory.asked.load(Ordering::Relaxed), 1);
        assert_eq!(edited.staged(), Vec::<PathBuf>::new());
        assert_eq!(edited.player.tags_to_undo(), vec![URI.to_owned()]);

        let undone = edited
            .player
            .undo_tags(vec![edited.file()])
            .expect("an undo");
        let restored = edited
            .player
            .catalog
            .track(edited.track.id)
            .expect("the song");

        assert_eq!(undone.written, 1);
        assert_eq!(edited.tags().title, None);
        assert_eq!(restored.title, "song");
        assert!(restored.artist.is_empty());
    }

    #[test]
    fn a_write_back_cut_short_puts_the_original_bytes_back_and_the_row_keeps_its_tags() {
        let edited = Edited::under("cut-short", &[Reply::TruncatedThenFull]);
        let before = fs::read(&edited.song).expect("the wave");

        let told = edited
            .player
            .edit_tags(vec![edited.file()], Edited::change())
            .expect("an answer");

        assert_eq!(told.failed, 1);
        assert_eq!(
            told.failure.as_deref(),
            Some("The song's file could not be written")
        );
        assert_eq!(fs::read(&edited.song).expect("the wave"), before);
        assert_eq!(edited.title(), "song.wav");
        assert!(edited.player.tags_to_undo().is_empty());
        assert_eq!(edited.staged(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_document_that_does_not_hold_what_was_written_is_put_back_as_it_was() {
        let edited = Edited::under("lost", &[Reply::TruncatedThenNowhere]);
        let before = fs::read(&edited.song).expect("the wave");

        let told = edited
            .player
            .edit_tags(vec![edited.file()], Edited::change())
            .expect("an answer");

        assert_eq!(told.failed, 1);
        assert_eq!(
            told.failure.as_deref(),
            Some("The file did not read back with the tags written")
        );
        assert_eq!(fs::read(&edited.song).expect("the wave"), before);
        assert_eq!(edited.title(), "song.wav");
    }

    #[test]
    fn an_original_that_cannot_be_put_back_is_kept_whole_and_the_next_run_puts_it_back() {
        let edited = Edited::under("unrestored", &[Reply::TruncatedThenFull, Reply::Refused]);
        let before = edited.document();

        let told = edited.edit();
        let kept = edited.unrestored();

        assert_eq!(told.failed, 1);
        assert!(
            told.failure
                .as_deref()
                .is_some_and(|failure| failure.starts_with("The song's file was left damaged"))
        );
        assert!(edited.document().is_empty());
        assert_eq!(kept.len(), 1);
        assert_eq!(
            fs::read(kept[0].join("original").join("song.wav")).expect("the original"),
            before
        );
        assert!(kept[0].join("pending").is_file());
        assert!(!kept[0].join("working").exists());
        assert_eq!(edited.staged(), Vec::<PathBuf>::new());

        let relaunched = edited.relaunched(&[]);

        assert_eq!(relaunched.document(), before);
        assert_eq!(relaunched.unrestored(), Vec::<PathBuf>::new());
        assert_eq!(relaunched.directory.asked.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn an_original_whose_move_aside_fails_stays_staged_and_the_next_run_puts_it_back() {
        let edited = Edited::under("unmoved", &[Reply::TruncatedThenFull, Reply::Refused]);
        let before = edited.document();

        fs::create_dir_all(&edited.data).expect("the data folder");
        fs::write(edited.data.join("tag-unrestored"), b"").expect("a file where the folder goes");

        let told = edited.edit();
        let staged = edited.staged();

        assert_eq!(told.failed, 1);
        assert!(
            told.failure
                .as_deref()
                .is_some_and(|failure| failure.starts_with("The song's file was left damaged"))
        );
        assert!(edited.document().is_empty());
        assert_eq!(staged.len(), 1);
        assert_eq!(
            fs::read(staged[0].join("original").join("song.wav")).expect("the original"),
            before
        );
        assert!(staged[0].join("pending").is_file());

        let relaunched = edited.relaunched(&[]);

        assert_eq!(relaunched.document(), before);
        assert_eq!(relaunched.staged(), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_next_edit_of_a_damaged_document_puts_its_original_back_before_writing() {
        let edited = Edited::under(
            "next-edit",
            &[Reply::TruncatedThenFull, Reply::Refused, Reply::Real],
        );

        fs::create_dir_all(&edited.data).expect("the data folder");
        fs::write(edited.data.join("tag-unrestored"), b"").expect("a file where the folder goes");

        let failed = edited.edit();
        let written = edited.edit();

        assert_eq!(failed.failed, 1);
        assert_eq!(written.written, 1);
        assert_eq!(edited.tags().title.as_deref(), Some("Slow Water"));
        assert_eq!(edited.title(), "Slow Water");
        assert_eq!(edited.staged(), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_edit_of_a_document_whose_original_cannot_yet_be_put_back_is_refused() {
        let edited = Edited::under(
            "still-damaged",
            &[Reply::TruncatedThenFull, Reply::Refused, Reply::Refused],
        );
        let before = edited.document();

        edited.edit();

        let refused = edited.edit();
        let kept = edited.unrestored();

        assert_eq!(refused.failed, 1);
        assert!(
            refused
                .failure
                .as_deref()
                .is_some_and(|failure| failure.starts_with("The song's file was left damaged"))
        );
        assert!(edited.document().is_empty());
        assert_eq!(edited.directory.asked.load(Ordering::Relaxed), 3);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            fs::read(kept[0].join("original").join("song.wav")).expect("the original"),
            before
        );
    }

    #[test]
    fn a_descriptor_refused_after_the_provider_emptied_the_document_puts_the_original_back() {
        let edited = Edited::under("emptied-refused", &[Reply::TruncatedThenRefused]);
        let before = edited.document();

        let told = edited.edit();

        assert_eq!(told.failed, 1);
        assert_eq!(
            told.failure.as_deref(),
            Some("The song's file could not be opened for writing")
        );
        assert_eq!(edited.document(), before);
        assert_eq!(edited.directory.asked.load(Ordering::Relaxed), 2);
        assert_eq!(edited.staged(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_rewrite_ended_mid_write_is_put_back_by_the_next_run() {
        if let Some(folder) = env::var_os(ENDING_UNDER) {
            let edited = Edited::over(
                PathBuf::from(folder).join("song.wav"),
                &[Reply::TruncatedThenEnded],
            );

            edited.edit();
            panic!("the rewrite ran to its end");
        }

        let song = wave("ended");
        let before = fs::read(&song).expect("the wave");

        let this_test = std::thread::current()
            .name()
            .expect("a test runs on a thread named for it")
            .to_owned();
        let ended = Command::new(env::current_exe().expect("the test binary"))
            .args([this_test.as_str(), "--exact", "--nocapture"])
            .env(ENDING_UNDER, song.parent().expect("a folder"))
            .status()
            .expect("a run that ends mid-write");
        let staged: Vec<PathBuf> = fs::read_dir(song.with_file_name("data").join("tag-staging"))
            .expect("the staging folder")
            .flatten()
            .map(|entry| entry.path())
            .collect();

        assert_eq!(ended.code(), Some(ENDED_MID_WRITE));
        assert!(fs::read(&song).expect("the document").is_empty());
        assert_eq!(staged.len(), 1);
        assert!(staged[0].join("pending").is_file());
        assert_eq!(
            fs::read(staged[0].join("original").join("song.wav")).expect("the original"),
            before
        );

        let relaunched = Edited::over(song, &[]);

        assert_eq!(relaunched.document(), before);
        assert_eq!(relaunched.staged(), Vec::<PathBuf>::new());
        assert_eq!(relaunched.directory.asked.load(Ordering::Relaxed), 1);

        let written = relaunched.edit();

        assert_eq!(written.written, 1);
        assert_eq!(relaunched.tags().title.as_deref(), Some("Slow Water"));
    }

    #[test]
    fn an_original_an_older_run_kept_is_left_where_it_lies() {
        let edited = Edited::under("older", &[]);
        let older = edited.data.join("tag-unrestored").join("song.wav.4242-0");

        fs::create_dir_all(older.parent().expect("a folder")).expect("the kept folder");
        fs::write(&older, b"an older original").expect("an older original");

        let relaunched = edited.relaunched(&[]);
        let written = relaunched.edit();

        assert_eq!(written.written, 1);
        assert_eq!(
            fs::read(&older).expect("the older original"),
            b"an older original"
        );
        assert_eq!(relaunched.unrestored(), vec![older]);
    }

    #[test]
    fn a_document_refused_for_writing_is_left_untouched() {
        let edited = Edited::under("refused-write", &[Reply::Refused]);
        let before = fs::read(&edited.song).expect("the wave");

        let told = edited
            .player
            .edit_tags(vec![edited.file()], Edited::change())
            .expect("an answer");

        assert_eq!(told.failed, 1);
        assert_eq!(
            told.failure.as_deref(),
            Some("The song's file could not be opened for writing")
        );
        assert_eq!(fs::read(&edited.song).expect("the wave"), before);
        assert_eq!(edited.directory.asked.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_document_is_not_written_where_the_front_end_gave_no_way_to() {
        let player = player();

        held(&player);

        let told = player
            .edit_tags(
                vec![SongFile {
                    uri: URI.into(),
                    path: String::new(),
                }],
                Edited::change(),
            )
            .expect("an answer");

        assert_eq!(told.failed, 1);
        assert_eq!(
            told.failure.as_deref(),
            Some("Tags of this kind of file cannot be written")
        );
    }
}
