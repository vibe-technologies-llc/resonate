use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use resonate_core::{naming::NAME_BYTES_AT_MOST, text};
use resonate_lyrics::{Beside, KeptSheet, Placing, SheetsBeside as Listed, Standing};

use crate::{
    Error, MobilePlayer, Result,
    documents::{DOCUMENT_REFUSED, location, owned},
    downloads::fresh_destination,
    foreign::{Callback, answered},
    tagging::{Change, SongFile, Undone},
    words,
};

const READ_BACK_AT_MOST: u64 = 16 * 1024 * 1024;

#[uniffi::export(callback_interface)]
pub trait SheetsBeside: Send + Sync {
    fn sheets_beside(&self, uri: String) -> Option<Vec<DocumentBeside>>;
    fn make_beside(&self, uri: String, name: String) -> Option<MadeBeside>;
    fn rewrite_beside(&self, uri: String) -> i32;
    fn remove_beside(&self, uri: String) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DocumentBeside {
    pub name: String,
    pub uri: String,
    pub within: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MadeBeside {
    pub uri: String,
    pub descriptor: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LyricsStanding {
    LeftAlone,
    Replaced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LyricsSaved {
    Written,
    AlreadySaid,
    SheetStands,
    SheetOutranked,
}

#[derive(Clone, Copy, Debug)]
pub enum SheetOp {
    Read,
    Made,
    Opened,
    Written,
}

impl fmt::Display for SheetOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Read => words::errors::sheet_read(),
            Self::Made => words::errors::sheet_made(),
            Self::Opened => words::errors::sheet_opened(),
            Self::Written => words::errors::sheet_written(),
        })
    }
}

impl From<LyricsStanding> for Standing {
    fn from(standing: LyricsStanding) -> Self {
        match standing {
            LyricsStanding::LeftAlone => Self::LeftAlone,
            LyricsStanding::Replaced => Self::Replaced,
        }
    }
}

#[uniffi::export]
pub fn lyric_folders() -> Vec<String> {
    resonate_lyrics::lyric_folders()
        .iter()
        .map(|folder| (*folder).to_owned())
        .collect()
}

#[uniffi::export]
impl MobilePlayer {
    pub fn write_sheets_beside_with(&self, beside: Box<dyn SheetsBeside>) {
        *self.tagging.beside.lock() = Some(beside.into());
    }

    pub fn save_lyrics_into_tags(&self, song: SongFile) -> Result<LyricsSaved> {
        let track = self.catalog.track_uri(&song.uri)?;
        let change = Change {
            lyrics: Some(self.lyrics_to_save(track.id)?),
            ..Change::default()
        };

        let Some(before) = self.retag(&song, &change)? else {
            return Ok(LyricsSaved::AlreadySaid);
        };

        self.tagging.keep(vec![Undone {
            uri: song.uri,
            before,
        }]);
        Ok(LyricsSaved::Written)
    }

    pub fn save_lyrics_beside(
        &self,
        song: SongFile,
        standing: LyricsStanding,
    ) -> Result<LyricsSaved> {
        let track = self.catalog.track_uri(&song.uri)?;
        let text = self.lyrics_to_save(track.id)?;
        let sheet = KeptSheet {
            text: &text,
            document: None,
        };

        match song.path.as_str() {
            "" => self.save_beside_the_document(&song.uri, &track.name, sheet, standing.into()),
            path => saved_beside_the_file(Path::new(path), sheet, standing.into()),
        }
    }
}

impl MobilePlayer {
    fn lyrics_to_save(&self, id: u64) -> Result<String> {
        match self.catalog.lyric_text(id)? {
            Some((Some(text), ..)) if !text.trim().is_empty() => Ok(text),
            _ => Err(Error::NoLyricsKept),
        }
    }

    fn save_beside_the_document(
        &self,
        uri: &str,
        name: &str,
        sheet: KeptSheet<'_>,
        standing: Standing,
    ) -> Result<LyricsSaved> {
        let beside = self
            .tagging
            .beside
            .lock()
            .clone()
            .ok_or(Error::LyricsCannotStandBeside)?;
        let stem = Path::new(name)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .ok_or_else(|| {
                unsaved(resonate_lyrics::Error::SidecarUnnamed { track: name.into() })
            })?;
        let named = format!("{stem}{}", sheet.ending());

        if named.len() > NAME_BYTES_AT_MOST {
            return Err(unsaved(resonate_lyrics::Error::SidecarNameTooLong {
                track: name.into(),
            }));
        }

        let standing_beside = answered(Callback::SheetsBeside, || {
            beside.sheets_beside(uri.to_owned())
        })
        .flatten()
        .ok_or(Error::LyricsCannotStandBeside)?;
        let mut listed = Listed::named_after(&stem);

        for document in standing_beside {
            listed.listed(&document.name, !document.within, document.uri);
        }

        let placing = listed.placing(sheet, standing, |uri: &String, body| {
            self.document_says(uri, body)
        })?;

        match placing {
            Placing::Fresh => {
                made_beside(&*beside, uri, &named, sheet.body())?;
                Ok(LyricsSaved::Written)
            }
            Placing::Over(at) => {
                rewritten_beside(&*beside, &at, sheet.body())?;
                Ok(LyricsSaved::Written)
            }
            Placing::AlreadySaid(_) => Ok(LyricsSaved::AlreadySaid),
            Placing::Stands(_) => Ok(LyricsSaved::SheetStands),
            Placing::Outranked(_) => Ok(LyricsSaved::SheetOutranked),
        }
    }

    fn document_says(&self, uri: &str, body: &str) -> Result<bool> {
        let unread = |source| Error::LyricsDocument {
            op: SheetOp::Read,
            source,
        };
        let mut held = Vec::new();

        self.sources
            .open(&location(uri)?)
            .map_err(|source| Error::LyricsDocumentUnread {
                source: Box::new(source),
            })?
            .stream
            .take(READ_BACK_AT_MOST)
            .read_to_end(&mut held)
            .map_err(unread)?;

        Ok(text::decoded(&held).0 == body)
    }
}

fn saved_beside_the_file(
    track: &Path,
    sheet: KeptSheet<'_>,
    standing: Standing,
) -> Result<LyricsSaved> {
    match resonate_lyrics::write_beside(track, sheet, standing) {
        Ok(Beside::Written(_)) => Ok(LyricsSaved::Written),
        Ok(Beside::AlreadySaid(_)) => Ok(LyricsSaved::AlreadySaid),
        Err(resonate_lyrics::Error::SidecarStands { .. }) => Ok(LyricsSaved::SheetStands),
        Err(resonate_lyrics::Error::SidecarOutranked { .. }) => Ok(LyricsSaved::SheetOutranked),
        Err(error) => Err(unsaved(error)),
    }
}

fn made_beside(beside: &dyn SheetsBeside, uri: &str, name: &str, body: &str) -> Result<()> {
    let made = answered(Callback::SheetsBeside, || {
        beside.make_beside(uri.to_owned(), name.to_owned())
    })
    .flatten()
    .filter(|made| made.descriptor >= 0)
    .ok_or_else(|| Error::LyricsDocument {
        op: SheetOp::Made,
        source: io::ErrorKind::PermissionDenied.into(),
    })?;
    let mut document = fresh_destination(made.descriptor)?;

    let written = written_into(&mut document, body);

    drop(document);
    if written.is_err()
        && !answered(Callback::SheetsBeside, || {
            beside.remove_beside(made.uri.clone())
        })
        .unwrap_or(false)
    {
        tracing::warn!("a lyric sheet cut short could not be taken away again");
    }
    written
}

fn rewritten_beside(beside: &dyn SheetsBeside, at: &str, body: &str) -> Result<()> {
    let descriptor = answered(Callback::SheetsBeside, || {
        beside.rewrite_beside(at.to_owned())
    })
    .unwrap_or(DOCUMENT_REFUSED);
    let mut document = owned(descriptor).map_err(|source| Error::LyricsDocument {
        op: SheetOp::Opened,
        source,
    })?;

    written_into(&mut document, body)
}

fn written_into(document: &mut File, body: &str) -> Result<()> {
    let failed = |source| Error::LyricsDocument {
        op: SheetOp::Written,
        source,
    };

    document.write_all(body.as_bytes()).map_err(failed)?;
    if let Err(error) = document.sync_all() {
        tracing::debug!(%error, "a lyric sheet could not be flushed to its provider");
    }
    Ok(())
}

fn unsaved(source: resonate_lyrics::Error) -> Error {
    Error::LyricsUnsaved {
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        os::fd::IntoRawFd,
        path::PathBuf,
        process,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use parking_lot::Mutex;
    use resonate_codec::{FileTags, Picturing, Sources, TagField, TagSource};
    use resonate_core::{MediaLocation, SourceId};

    use super::*;
    use crate::{
        Documents, PlaybackPreferences, Track, catalog::Catalog, documents::GrantedFiles,
        enrichment::FETCHED_LYRICS, features::Services, tagging::Tagging,
    };

    const SYNCED: &str = "[00:01.000]all that you touch\n[00:05.000]all that you see\n";
    const THEIRS: &str = "[00:02.00]a sheet of the listener's own\n";
    const PREFIX: &str = "content://test/";

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);

            let folder = std::env::temp_dir().join(format!(
                "resonate-saving-lyrics-{name}-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&folder);

            fs::create_dir_all(&folder).expect("a folder");
            Self(folder)
        }

        fn song(&self) -> PathBuf {
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

            let path = self.0.join("Echoes.wav");

            fs::write(&path, bytes).expect("a wave");
            path
        }

        fn path_of(&self, uri: &str) -> PathBuf {
            self.0.join(uri.strip_prefix(PREFIX).expect("a test uri"))
        }

        fn uri_of(&self, path: &Path) -> String {
            let relative = path.strip_prefix(&self.0).expect("within the folder");
            format!("{PREFIX}{}", relative.to_string_lossy())
        }

        fn sheet(&self) -> Option<String> {
            fs::read_to_string(self.0.join("Echoes.lrc")).ok()
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Granted(Arc<Folder>);

    impl Documents for Granted {
        fn open_document(&self, uri: String) -> i32 {
            fs::File::open(self.0.path_of(&uri)).map_or(DOCUMENT_REFUSED, IntoRawFd::into_raw_fd)
        }
    }

    struct Tree {
        folder: Arc<Folder>,
        reaches: bool,
        made: Mutex<Vec<String>>,
    }

    impl SheetsBeside for Tree {
        fn sheets_beside(&self, _uri: String) -> Option<Vec<DocumentBeside>> {
            if !self.reaches {
                return None;
            }

            let listed = |path: &Path, within: bool| {
                fs::read_dir(path)
                    .expect("a folder")
                    .flatten()
                    .map(|entry| DocumentBeside {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        uri: self.folder.uri_of(&entry.path()),
                        within,
                    })
                    .collect::<Vec<_>>()
            };
            let folders = lyric_folders();
            let mut found = listed(&self.folder.0, false);
            let within: Vec<PathBuf> = fs::read_dir(&self.folder.0)
                .expect("a folder")
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_dir()
                        && path.file_name().is_some_and(|name| {
                            folders.contains(&name.to_string_lossy().to_lowercase())
                        })
                })
                .collect();

            for folder in within {
                found.extend(listed(&folder, true));
            }
            Some(found)
        }

        fn make_beside(&self, _uri: String, name: String) -> Option<MadeBeside> {
            let path = self.folder.0.join(&name);
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .ok()?;

            self.made.lock().push(name);
            Some(MadeBeside {
                uri: self.folder.uri_of(&path),
                descriptor: file.into_raw_fd(),
            })
        }

        fn rewrite_beside(&self, uri: String) -> i32 {
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(self.folder.path_of(&uri))
                .map_or(DOCUMENT_REFUSED, IntoRawFd::into_raw_fd)
        }

        fn remove_beside(&self, uri: String) -> bool {
            fs::remove_file(self.folder.path_of(&uri)).is_ok()
        }
    }

    struct Saving {
        player: MobilePlayer,
        folder: Arc<Folder>,
        song: PathBuf,
        track: Track,
    }

    impl Saving {
        fn under(name: &str) -> Self {
            let folder = Arc::new(Folder::new(name));
            let song = folder.song();
            let player = MobilePlayer {
                playing: crate::zoning::Playing::closed(),
                catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
                sources: Arc::new(
                    Sources::local().and(Arc::new(
                        GrantedFiles::new(Arc::new(Granted(Arc::clone(&folder))), "content")
                            .expect("a provider"),
                    )),
                ),
                preferences: Mutex::new(PlaybackPreferences::default()),
                services: Services::default(),
                playlist_undo: Mutex::new(Vec::new()),
                downloads: crate::downloads::Downloads::default(),
                syncing: crate::sync::Syncing::under(std::env::temp_dir()),
                enriching: crate::enrichment::Enriching::default(),
                room: crate::room::Room::under(std::env::temp_dir()),
                tagging: Tagging::under(folder.0.join("data")),
                history_spool: crate::history::Spool::under(folder.0.join("data")),
                served: Arc::new(crate::served::Served::default()),
                together: crate::together::Together::default(),
                rendering: crate::rendering::Rendering::default(),
                offered: crate::offering::Offered::untold(),
                releases: resonate_online::KeptRelease::in_folder(&folder.0.join("data")),
            };

            let track = player
                .catalog
                .upsert(Track {
                    uri: folder.uri_of(&song),
                    name: "Echoes.wav".into(),
                    title: "Echoes".into(),
                    duration_ms: 100,
                    rate: 44_100,
                    codec: "PCM".into(),
                    ..Track::default()
                })
                .expect("a song");
            Self {
                player,
                folder,
                song,
                track,
            }
        }

        fn fetched(self) -> Self {
            self.player
                .catalog
                .keep_lyrics(self.track.id, Some(SYNCED), None, FETCHED_LYRICS)
                .expect("lyrics kept");
            self
        }

        fn by_path(&self) -> SongFile {
            SongFile {
                uri: self.track.uri.clone(),
                path: self.song.to_string_lossy().into_owned(),
            }
        }

        fn by_document(&self) -> SongFile {
            SongFile {
                uri: self.track.uri.clone(),
                path: String::new(),
            }
        }

        fn tree(&self, reaches: bool) -> Arc<Tree> {
            let tree = Arc::new(Tree {
                folder: Arc::clone(&self.folder),
                reaches,
                made: Mutex::new(Vec::new()),
            });

            *self.player.tagging.beside.lock() = Some(Arc::clone(&tree) as Arc<dyn SheetsBeside>);
            tree
        }

        fn lyrics_in_the_tags(&self) -> Option<String> {
            let tags = FileTags::default()
                .read(&MediaLocation::local(self.song.clone()), Picturing::Whether)
                .expect("the song's tags")
                .tags;
            TagField::Lyrics.read(&tags)
        }
    }

    fn read_back(sheet: &str) -> Vec<String> {
        resonate_lyrics::read_lyrics(SourceId::local(), sheet)
            .expect("a readable sheet")
            .expect("its lines")
            .lines()
            .iter()
            .map(|line| line.text.clone())
            .collect()
    }

    #[test]
    fn fetched_lyrics_go_into_the_songs_tags_read_back_and_one_undo_takes_them_out() {
        let saving = Saving::under("tags").fetched();

        let saved = saving
            .player
            .save_lyrics_into_tags(saving.by_path())
            .expect("lyrics written");
        let held = saving.lyrics_in_the_tags();
        let again = saving
            .player
            .save_lyrics_into_tags(saving.by_path())
            .expect("the same lyrics");

        assert_eq!(saved, LyricsSaved::Written);
        assert_eq!(held.as_deref(), Some(SYNCED.trim()));
        assert_eq!(again, LyricsSaved::AlreadySaid);
        assert_eq!(saving.player.tags_to_undo(), vec![saving.track.uri.clone()]);

        let undone = saving
            .player
            .undo_tags(vec![saving.by_path()])
            .expect("an undo");

        assert_eq!(undone.written, 1);
        assert_eq!(saving.lyrics_in_the_tags(), None);
    }

    #[test]
    fn lyrics_saved_beside_a_file_read_back_as_its_sheet_and_leave_one_of_the_listeners_alone() {
        let saving = Saving::under("beside").fetched();

        let saved = saving
            .player
            .save_lyrics_beside(saving.by_path(), LyricsStanding::LeftAlone)
            .expect("a sheet written");
        let written = saving.folder.sheet().expect("the sheet beside the song");
        let again = saving
            .player
            .save_lyrics_beside(saving.by_path(), LyricsStanding::LeftAlone)
            .expect("the same sheet");

        assert_eq!(saved, LyricsSaved::Written);
        assert_eq!(read_back(&written), read_back(SYNCED));
        assert_eq!(again, LyricsSaved::AlreadySaid);

        fs::write(saving.folder.0.join("Echoes.lrc"), THEIRS).expect("a sheet of their own");

        let stands = saving
            .player
            .save_lyrics_beside(saving.by_path(), LyricsStanding::LeftAlone)
            .expect("an answer");

        assert_eq!(stands, LyricsSaved::SheetStands);
        assert_eq!(saving.folder.sheet().as_deref(), Some(THEIRS));
    }

    #[test]
    fn lyrics_saved_beside_a_granted_document_are_made_as_a_sibling_and_replaced_only_when_asked() {
        let saving = Saving::under("document").fetched();
        let tree = saving.tree(true);

        let saved = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::LeftAlone)
            .expect("a sheet written");
        let again = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::LeftAlone)
            .expect("the same sheet");

        assert_eq!(saved, LyricsSaved::Written);
        assert_eq!(saving.folder.sheet().as_deref(), Some(SYNCED));
        assert_eq!(again, LyricsSaved::AlreadySaid);
        assert_eq!(*tree.made.lock(), vec!["Echoes.lrc".to_owned()]);

        fs::write(saving.folder.0.join("Echoes.lrc"), THEIRS).expect("a sheet of their own");

        let stands = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::LeftAlone)
            .expect("an answer");
        let after_refusal = saving.folder.sheet();
        let replaced = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::Replaced)
            .expect("a sheet replaced");

        assert_eq!(stands, LyricsSaved::SheetStands);
        assert_eq!(after_refusal.as_deref(), Some(THEIRS));
        assert_eq!(replaced, LyricsSaved::Written);
        assert_eq!(saving.folder.sheet().as_deref(), Some(SYNCED));
        assert_eq!(tree.made.lock().len(), 1);
    }

    #[test]
    fn a_sheet_in_a_lyrics_folder_or_a_richer_one_beside_a_document_is_never_hidden() {
        let saving = Saving::under("outranked").fetched();

        saving.tree(true);
        fs::create_dir_all(saving.folder.0.join("Lyrics")).expect("a lyrics folder");
        fs::write(saving.folder.0.join("Lyrics").join("echoes.LRC"), THEIRS)
            .expect("a sheet of their own");

        let stands = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::LeftAlone)
            .expect("an answer");

        assert_eq!(stands, LyricsSaved::SheetStands);
        assert_eq!(saving.folder.sheet(), None);

        fs::write(
            saving.folder.0.join("Echoes.lyricsfile.yaml"),
            "lines: []\n",
        )
        .expect("a richer sheet");

        let outranked = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::Replaced)
            .expect("an answer");

        assert_eq!(outranked, LyricsSaved::SheetOutranked);
        assert_eq!(saving.folder.sheet(), None);
    }

    #[test]
    fn a_document_whose_folder_lets_no_sibling_be_made_is_refused_with_its_reason() {
        let saving = Saving::under("refused").fetched();

        let unregistered = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::LeftAlone);

        saving.tree(false);

        let unreached = saving
            .player
            .save_lyrics_beside(saving.by_document(), LyricsStanding::LeftAlone);

        assert!(
            matches!(unregistered, Err(Error::LyricsCannotStandBeside)),
            "{unregistered:?}"
        );
        assert!(
            matches!(unreached, Err(Error::LyricsCannotStandBeside)),
            "{unreached:?}"
        );
        assert_eq!(saving.folder.sheet(), None);
        assert_eq!(
            unreached
                .map_err(|error| error.to_string())
                .err()
                .as_deref(),
            Some(
                "The folder this song was granted from does not let a lyric sheet be written \
                 beside it"
            )
        );
    }

    #[test]
    fn a_song_with_no_lyrics_kept_saves_none_anywhere() {
        let saving = Saving::under("unkept");

        let beside = saving
            .player
            .save_lyrics_beside(saving.by_path(), LyricsStanding::LeftAlone);
        let tags = saving.player.save_lyrics_into_tags(saving.by_path());

        assert!(matches!(beside, Err(Error::NoLyricsKept)), "{beside:?}");
        assert!(matches!(tags, Err(Error::NoLyricsKept)), "{tags:?}");
        assert_eq!(saving.folder.sheet(), None);
        assert_eq!(saving.lyrics_in_the_tags(), None);
    }
}
