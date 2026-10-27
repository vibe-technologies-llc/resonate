use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;
use resonate_codec::{
    FileTags, Picturing, TagEdit, TagField, TagSet, TagSink, TagSource, Writing, probe,
};
use resonate_core::MediaLocation;
use rusqlite::params;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::Catalog,
    documents::location,
    error::RewriteOp,
    rewriting::{DocumentRewriting, Stage, crc_of_file, put_back},
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Planned {
    edits: Vec<TagEdit>,
    taken: Vec<TagField>,
    before: SongTags,
}

#[derive(Clone, Debug)]
struct Undone {
    uri: String,
    before: SongTags,
}

pub(crate) struct Tagging {
    folder: PathBuf,
    steps: Mutex<Vec<Vec<Undone>>>,
    rewriting: Mutex<Option<Arc<dyn DocumentRewriting>>>,
}

impl Tagging {
    pub(crate) fn under(folder: PathBuf) -> Self {
        Self {
            folder,
            steps: Mutex::new(Vec::new()),
            rewriting: Mutex::new(None),
        }
    }

    fn keep(&self, step: Vec<Undone>) {
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
    before: SongTags,
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
        *self.tagging.rewriting.lock() = Some(Arc::from(rewriting));
    }

    pub fn tags_writable(&self, name: String) -> bool {
        FileTags::default().writes(&MediaLocation::local(PathBuf::from(name)))
    }

    pub fn edit_tags(&self, songs: Vec<SongFile>, change: SongTags) -> Result<TagsWritten> {
        let mut told = TagsWritten::default();
        let mut step = Vec::new();
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
    fn retag(&self, song: &SongFile, change: &SongTags) -> Result<Option<SongTags>> {
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
        change: &SongTags,
    ) -> Result<Option<Retagged>> {
        let rewriting = self
            .tagging
            .rewriting
            .lock()
            .clone()
            .ok_or(Error::TagsUnwritable)?;
        let at = location(&song.uri)?;
        let tags = FileTags::default();
        let stage = Stage::made(&self.tagging.folder, &track.name)?;
        let working = MediaLocation::local(stage.working());
        if !tags.writes(&working) {
            return Err(Error::TagsUnwritable);
        }

        self.copy_document(&at, &stage.working())?;
        let held = tags.read(&working, Picturing::Whether)?.tags;
        let Some(planned) = planned(&held, change)? else {
            return Ok(None);
        };

        fs::copy(stage.working(), stage.original()).map_err(|source| Error::TagsDocument {
            op: RewriteOp::CopiedAside,
            source,
        })?;
        let original = crc_of_file(&stage.original())?;
        let written = write_planned(&tags, &working, &planned)?;
        let edited = crc_of_file(&stage.working())?;

        self.replace_document(&*rewriting, &song.uri, &stage, edited, original)?;
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
        stage: &Stage,
        edited: u32,
        original: u32,
    ) -> Result<()> {
        let failure = match put_back(rewriting, uri, &stage.working()) {
            Err(
                error @ Error::TagsDocument {
                    op: RewriteOp::Opened,
                    ..
                },
            ) => return Err(error),
            Err(error) => error,
            Ok(()) if self.whole_crc(uri) == Some(edited) => return Ok(()),
            Ok(()) => Error::TagsUnconfirmed,
        };

        let restored = put_back(rewriting, uri, &stage.original()).is_ok()
            && self.whole_crc(uri) == Some(original);
        if restored {
            return Err(failure);
        }

        let kept = stage.keep_original(&self.tagging.folder)?;
        tracing::error!(path = %kept.display(), "a song could not be put back after a failed tag write");
        Err(Error::TagsUnrestored { kept })
    }
}

fn retag_at(local: &MediaLocation, change: &SongTags) -> Result<Option<Retagged>> {
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

fn planned(held: &TagSet, change: &SongTags) -> Result<Option<Planned>> {
    let mut edits = Vec::new();
    let mut taken = Vec::new();
    let mut before = SongTags::default();
    for field in EDITABLE {
        let Some(wanted) = change.wanted(field) else {
            continue;
        };
        let wanted = spelled(field, wanted)?;
        let current = field.read(held);
        match wanted {
            None if current.is_some() => taken.push(field),
            Some(value) if current.as_deref() != Some(value.as_str()) => {
                edits.push(TagEdit { field, value });
            }
            _ => continue,
        }
        *before.slot(field) = Some(current.unwrap_or_default());
    }

    Ok((!edits.is_empty() || !taken.is_empty()).then_some(Planned {
        edits,
        taken,
        before,
    }))
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
        let title = tags.title.clone().unwrap_or_else(|| track.name.clone());
        self.0
            .lock()
            .execute(
                "UPDATE documents SET title = ?2, artist = ?3, album = ?4, album_artist = ?5, genre = ?6, date = ?7, track_number = ?8, disc_number = ?9 WHERE id = ?1",
                params![
                    track.id as i64,
                    title,
                    tags.artist.clone().unwrap_or_default(),
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
        fs::OpenOptions,
        os::fd::IntoRawFd,
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

    fn player() -> MobilePlayer {
        MobilePlayer {
            player: Mutex::new(None),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(Sources::local()),
            failure: Mutex::new(None),
            waiting: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: crate::sync::Syncing::under(std::env::temp_dir()),
            routing: Arc::new(crate::output::Routing::new()),
            enriching: crate::enrichment::Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
            tagging: Tagging::under(std::env::temp_dir()),
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
            title: Some("Echoes".into()),
            artist: Some(" Pink Floyd ".into()),
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
        assert_eq!(retagged.title, "Echoes");
        assert_eq!(retagged.artist, "Pink Floyd");
        assert_eq!(retagged.track_number, Some(7));
        assert_eq!(again.unchanged, 1);
        assert_eq!(player.tags_to_undo(), vec![URI.to_owned()]);

        let undone = player.undo_tags(vec![song]).expect("an undo");
        let restored = player.catalog.track(track.id).expect("the song");

        assert_eq!(undone.written, 1);
        assert_eq!(restored.title, "song.wav");
        assert!(restored.artist.is_empty());
        assert_eq!(restored.track_number, None);
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
            title: Some("Echoes".into()),
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
        assert_eq!(restored.title, "song.wav");
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
            let song = wave(name);
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
                title: Some("Echoes".into()),
                artist: Some("Pink Floyd".into()),
                ..SongTags::default()
            }
        }

        fn staged(&self) -> Vec<PathBuf> {
            fs::read_dir(self.data.join("tag-staging"))
                .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
                .unwrap_or_default()
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
        assert_eq!(edited.tags().title.as_deref(), Some("Echoes"));
        assert_eq!(edited.tags().artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(retagged.title, "Echoes");
        assert_eq!(retagged.artist, "Pink Floyd");
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
        assert_eq!(restored.title, "song.wav");
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
    fn an_original_that_cannot_be_put_back_is_kept_whole() {
        let edited = Edited::under("unrestored", &[Reply::TruncatedThenFull, Reply::Refused]);
        let before = fs::read(&edited.song).expect("the wave");

        let told = edited
            .player
            .edit_tags(vec![edited.file()], Edited::change())
            .expect("an answer");
        let kept: Vec<PathBuf> = fs::read_dir(edited.data.join("tag-unrestored"))
            .expect("the kept folder")
            .flatten()
            .map(|entry| entry.path())
            .collect();

        assert_eq!(told.failed, 1);
        assert!(
            told.failure
                .as_deref()
                .is_some_and(|failure| failure.starts_with("The song's file was left damaged"))
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read(&kept[0]).expect("the original"), before);
        assert_eq!(edited.staged(), Vec::<PathBuf>::new());
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
