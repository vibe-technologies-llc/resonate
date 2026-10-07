use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    os::unix::{ffi::OsStrExt as _, fs::FileExt as _},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

const MAGIC: &[u8; 8] = b"RSUNDO01";
const JOURNAL_SUFFIX: &str = ".resonate-undo";
const WHOLE_SUFFIX: &str = ".resonate-whole";
const TORN_SUFFIX: &str = ".resonate-torn";
const KEPT_BY_AND_COUNTED: char = '-';
const RUNNING_PROCESSES: &str = "/proc";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

static KEPT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Change<'b> {
    pub(crate) start: u64,
    pub(crate) before: Vec<u8>,
    pub(crate) after: &'b [u8],
}

pub(crate) struct Undo {
    journal: PathBuf,
}

impl Undo {
    pub(crate) fn kept_beside(
        track: &Path,
        stood: u64,
        changes: &[Change<'_>],
    ) -> io::Result<Self> {
        let name = track
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let folder = folder_of(track);
        let journal = folder.join(journal_name(name));

        let written = File::create_new(&journal).and_then(|mut file| {
            file.write_all(&encoded(name, stood, changes))?;
            file.sync_all()?;
            File::open(folder)?.sync_all()
        });
        if let Err(error) = written {
            let _ = fs::remove_file(&journal);
            return Err(error);
        }
        Ok(Self { journal })
    }

    pub(crate) fn roll_back(self, file: &File, changes: &[Change<'_>]) {
        let rolled = changes
            .iter()
            .try_for_each(|change| file.write_all_at(&change.before, change.start))
            .and_then(|()| file.sync_data());
        match rolled {
            Ok(()) => self.done(),
            Err(error) => {
                tracing::warn!(%error, path = %self.journal.display(), "a tag write that failed part way could not be rolled back, so its undo is left for the next run");
            }
        }
    }

    pub(crate) fn done(self) {
        if let Err(error) = fs::remove_file(&self.journal) {
            tracing::debug!(%error, path = %self.journal.display(), "the undo of a landed tag write could not be removed");
        }
    }
}

fn folder_of(track: &Path) -> &Path {
    track
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn journal_name(track: &OsStr) -> PathBuf {
    kept_name(track, JOURNAL_SUFFIX)
}

fn kept_name(track: &OsStr, suffix: &str) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(track);
    name.push(format!(
        ".{}{KEPT_BY_AND_COUNTED}{}{suffix}",
        process::id(),
        KEPT.fetch_add(1, Ordering::Relaxed)
    ));
    PathBuf::from(name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Left {
    Undo,
    WholeCopy,
    TornWholeCopy,
}

impl Left {
    const ALL: [Self; 3] = [Self::Undo, Self::WholeCopy, Self::TornWholeCopy];

    const fn suffix(self) -> &'static str {
        match self {
            Self::Undo => JOURNAL_SUFFIX,
            Self::WholeCopy => WHOLE_SUFFIX,
            Self::TornWholeCopy => TORN_SUFFIX,
        }
    }
}

struct KeptBy<'n> {
    track: &'n str,
    writer: u32,
    left: Left,
}

fn kept_by(kept: &OsStr) -> Option<KeptBy<'_>> {
    let named = kept.to_str()?.strip_prefix('.')?;
    let (left, unsuffixed) = Left::ALL
        .into_iter()
        .find_map(|left| Some((left, named.strip_suffix(left.suffix())?)))?;
    let (track, stamp) = unsuffixed.rsplit_once('.')?;
    let (writer, counted) = stamp.split_once(KEPT_BY_AND_COUNTED)?;
    counted.parse::<u64>().ok()?;
    Some(KeptBy {
        track,
        writer: writer.parse().ok()?,
        left,
    })
}

pub fn names_a_cut_short_write(path: &Path) -> bool {
    path.file_name().and_then(kept_by).is_some_and(|kept| {
        kept.left == Left::TornWholeCopy
            || (kept.writer != process::id() && !is_running(kept.writer))
    })
}

pub(crate) struct WholeCopy {
    copy: PathBuf,
    track: PathBuf,
}

pub(crate) struct Torn {
    pub(crate) whole: PathBuf,
    pub(crate) source: io::Error,
}

pub(crate) enum Unsettled {
    Untouched(io::Error),
    Torn(Torn),
}

impl From<io::Error> for Unsettled {
    fn from(source: io::Error) -> Self {
        Self::Untouched(source)
    }
}

impl WholeCopy {
    pub(crate) fn named_beside(staged: &Path, track: &Path) -> io::Result<Self> {
        let name = track
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let folder = folder_of(track);
        let copy = folder.join(kept_name(name, WHOLE_SUFFIX));

        fs::rename(staged, &copy)?;
        if let Err(error) = File::open(folder).and_then(|folder| folder.sync_all()) {
            let _ = fs::rename(&copy, staged);
            return Err(error);
        }
        Ok(Self {
            copy,
            track: track.to_path_buf(),
        })
    }

    pub(crate) fn written_back(self) -> Result<(), Unsettled> {
        match copied_over(&self.copy, &self.track) {
            Ok(()) => {}
            Err(Unsettled::Untouched(source)) => {
                let _ = fs::remove_file(&self.copy);
                return Err(Unsettled::Untouched(source));
            }
            Err(Unsettled::Torn(Torn { source, .. })) => {
                return Err(Unsettled::Torn(self.torn(source)));
            }
        }
        if let Err(error) = fs::remove_file(&self.copy) {
            tracing::debug!(%error, path = %self.copy.display(), "the whole copy of a written-back track could not be removed");
        }
        Ok(())
    }

    fn torn(self, source: io::Error) -> Torn {
        let name = self.track.file_name().unwrap_or_default();
        let torn = folder_of(&self.track).join(kept_name(name, TORN_SUFFIX));
        let whole = match fs::rename(&self.copy, &torn) {
            Ok(()) => torn,
            Err(_) => self.copy,
        };
        Torn { whole, source }
    }
}

pub(crate) fn copied_over(whole: &Path, track: &Path) -> Result<(), Unsettled> {
    let mut copy = File::open(whole)?;
    let mut file = OpenOptions::new().write(true).open(track)?;
    let torn = |source| {
        tracing::warn!(%source, track = %track.display(), whole = %whole.display(), "a track failed part way through being written back, so its whole copy is kept for the next write or scan to finish");
        Unsettled::Torn(Torn {
            whole: whole.to_path_buf(),
            source,
        })
    };

    let length = io::copy(&mut copy, &mut file).map_err(torn)?;
    file.set_len(length).map_err(torn)?;
    file.sync_all().map_err(torn)
}

fn is_running(writer: u32) -> bool {
    Path::new(RUNNING_PROCESSES)
        .join(writer.to_string())
        .exists()
}

fn hashed(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn encoded(track: &OsStr, stood: u64, changes: &[Change<'_>]) -> Vec<u8> {
    let name = track.as_bytes();
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&stood.to_le_bytes());
    bytes.extend_from_slice(&(changes.len() as u64).to_le_bytes());
    for change in changes {
        bytes.extend_from_slice(&change.start.to_le_bytes());
        bytes.extend_from_slice(&(change.after.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&change.before);
        bytes.extend_from_slice(change.after);
    }
    let check = hashed(&bytes);
    bytes.extend_from_slice(&check.to_le_bytes());
    bytes
}

struct Kept {
    track: PathBuf,
    stood: u64,
    changes: Vec<(u64, Vec<u8>, Vec<u8>)>,
}

struct Reader<'b> {
    bytes: &'b [u8],
}

impl<'b> Reader<'b> {
    fn taken(&mut self, count: usize) -> Option<&'b [u8]> {
        if count > self.bytes.len() {
            return None;
        }
        let (taken, rest) = self.bytes.split_at(count);
        self.bytes = rest;
        Some(taken)
    }

    fn number(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.taken(8)?.try_into().ok()?))
    }

    fn counted(&mut self) -> Option<usize> {
        usize::try_from(self.number()?).ok()
    }
}

fn decoded(folder: &Path, bytes: &[u8]) -> Option<Kept> {
    let (body, check) = bytes.split_last_chunk::<8>()?;
    if hashed(body) != u64::from_le_bytes(*check) {
        return None;
    }

    let mut reader = Reader { bytes: body };
    if reader.taken(MAGIC.len())? != MAGIC {
        return None;
    }
    let length = reader.counted()?;
    let name = OsStr::from_bytes(reader.taken(length)?);
    if Path::new(name).file_name() != Some(name) {
        return None;
    }
    let stood = reader.number()?;
    let count = reader.counted()?;

    let mut changes = Vec::new();
    for _ in 0..count {
        let start = reader.number()?;
        let length = reader.counted()?;
        let before = reader.taken(length)?.to_vec();
        let after = reader.taken(length)?.to_vec();
        changes.push((start, before, after));
    }
    reader.bytes.is_empty().then(|| Kept {
        track: folder.join(name),
        stood,
        changes,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mended {
    WrittenBack,
    RolledBack,
    Finished,
    Untouched,
    ChangedSince,
    Unreadable,
}

pub fn mend_a_cut_short_write(journal: &Path) -> io::Result<Mended> {
    let kept = journal.file_name().and_then(kept_by);
    if let Some(kept) = kept.filter(|kept| kept.left != Left::Undo) {
        let track = folder_of(journal).join(kept.track);
        return finished_writing_back(journal, &track);
    }

    let bytes = fs::read(journal)?;
    let mended = match decoded(folder_of(journal), &bytes) {
        Some(kept) => rolled_back(&kept)?,
        None => Mended::Unreadable,
    };
    fs::remove_file(journal)?;
    tracing::info!(path = %journal.display(), ?mended, "mended a tag write cut short");
    Ok(mended)
}

fn finished_writing_back(whole: &Path, track: &Path) -> io::Result<Mended> {
    let mended = match copied_over(whole, track) {
        Ok(()) => Mended::WrittenBack,
        Err(Unsettled::Untouched(error))
            if error.kind() == io::ErrorKind::NotFound && !track.exists() =>
        {
            Mended::ChangedSince
        }
        Err(Unsettled::Untouched(error) | Unsettled::Torn(Torn { source: error, .. })) => {
            return Err(error);
        }
    };
    fs::remove_file(whole)?;
    tracing::info!(path = %whole.display(), ?mended, "finished writing back a track a tag write left part rewritten");
    Ok(mended)
}

fn rolled_back(kept: &Kept) -> io::Result<Mended> {
    let file = match OpenOptions::new().read(true).write(true).open(&kept.track) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Mended::ChangedSince),
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() != kept.stood {
        return Ok(Mended::ChangedSince);
    }

    let mut all_before = true;
    let mut all_after = true;
    for (start, before, after) in &kept.changes {
        let mut standing = vec![0; before.len()];
        file.read_exact_at(&mut standing, *start)?;
        let ours = standing
            .iter()
            .zip(before.iter().zip(after))
            .all(|(byte, (was, is))| byte == was || byte == is);
        if !ours {
            return Ok(Mended::ChangedSince);
        }
        all_before &= standing == *before;
        all_after &= standing == *after;
    }
    if all_before {
        return Ok(Mended::Untouched);
    }
    if all_after {
        return Ok(Mended::Finished);
    }

    for (start, before, _) in &kept.changes {
        file.write_all_at(before, *start)?;
    }
    file.sync_data()?;
    Ok(Mended::RolledBack)
}

pub(crate) fn mend_what_a_dead_writer_left(track: &Path) {
    let Some(name) = track.file_name() else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder_of(track)) else {
        return;
    };
    let mut prefix = std::ffi::OsString::from(".");
    prefix.push(name);
    prefix.push(".");

    for entry in entries.flatten() {
        let path = entry.path();
        let named_after_the_track = entry.file_name().as_bytes().starts_with(prefix.as_bytes());
        if !named_after_the_track || !names_a_cut_short_write(&path) {
            continue;
        }
        if let Err(error) = mend_a_cut_short_write(&path) {
            tracing::warn!(%error, path = %path.display(), "a tag write cut short could not be mended");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    const NEVER_A_PROCESS: u32 = 999_999_999;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-journal-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a scratch folder");
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn left_by_a_dead_writer(track: &Path, stood: u64, changes: &[Change<'_>]) -> PathBuf {
        let name = track.file_name().expect("a named track");
        let journal = track.with_file_name(format!(
            ".{}.{NEVER_A_PROCESS}-0{JOURNAL_SUFFIX}",
            name.to_string_lossy()
        ));
        fs::write(&journal, encoded(name, stood, changes)).expect("a journal");
        journal
    }

    #[test]
    fn a_track_a_dead_writer_left_part_written_back_is_finished_from_its_whole_copy() {
        let folder = Folder::new("written-back");
        let track = folder.0.join("Breathe.flac");
        let whole = folder
            .0
            .join(format!(".Breathe.flac.{NEVER_A_PROCESS}-0{WHOLE_SUFFIX}"));

        fs::write(&track, [9_u8; 40]).expect("a torn track");
        fs::write(&whole, [4_u8; 24]).expect("the whole copy");

        assert!(names_a_cut_short_write(&whole));
        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::WrittenBack
        );
        assert_eq!(fs::read(&track).expect("the track"), [4_u8; 24]);
        assert!(!whole.exists(), "the whole copy was left");
    }

    #[test]
    fn a_whole_copy_a_failed_write_back_kept_is_finished_even_by_its_own_process() {
        let folder = Folder::new("torn-copy");
        let track = folder.0.join("Brain Damage.flac");
        fs::write(&track, [1_u8; 8]).expect("a torn track");

        let ours = folder
            .0
            .join(kept_name(OsStr::new("Brain Damage.flac"), WHOLE_SUFFIX));
        fs::write(&ours, [2_u8; 8]).expect("a copy being written back");
        assert!(!names_a_cut_short_write(&ours));

        let torn = WholeCopy {
            copy: ours,
            track: track.clone(),
        }
        .torn(io::Error::from(io::ErrorKind::StorageFull));
        assert!(names_a_cut_short_write(&torn.whole));
        mend_what_a_dead_writer_left(&track);
        assert_eq!(fs::read(&track).expect("the track"), [2_u8; 8]);
        assert!(!torn.whole.exists());
    }

    #[test]
    fn a_write_cut_short_between_its_pages_is_rolled_back_whole() {
        let folder = Folder::new("torn");
        let track = folder.0.join("Echoes.flac");
        let was = vec![1_u8; 64];
        fs::write(&track, &was).expect("a track");

        let changes = [
            Change {
                start: 0,
                before: was[..8].to_vec(),
                after: &[2; 8],
            },
            Change {
                start: 32,
                before: was[32..40].to_vec(),
                after: &[3; 8],
            },
        ];
        let journal = left_by_a_dead_writer(&track, 64, &changes);
        let mut torn = was.clone();
        torn[..8].fill(2);
        torn[32..36].fill(3);
        fs::write(&track, &torn).expect("a torn write");

        assert!(names_a_cut_short_write(&journal));
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::RolledBack
        );

        assert_eq!(fs::read(&track).expect("the track"), was);
        assert!(!journal.exists(), "the journal was left");
    }

    #[test]
    fn a_write_that_finished_or_never_began_is_left_as_it_stands() {
        let folder = Folder::new("whole");
        let track = folder.0.join("Time.flac");
        fs::write(&track, [1_u8; 16]).expect("a track");
        let changes = [Change {
            start: 4,
            before: vec![1; 4],
            after: &[9; 4],
        }];

        let journal = left_by_a_dead_writer(&track, 16, &changes);
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::Untouched
        );

        let mut finished = [1_u8; 16];
        finished[4..8].fill(9);
        fs::write(&track, finished).expect("a finished write");
        let journal = left_by_a_dead_writer(&track, 16, &changes);
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::Finished
        );
        assert_eq!(fs::read(&track).expect("the track"), finished);
    }

    #[test]
    fn a_file_another_program_changed_since_is_not_rolled_back() {
        let folder = Folder::new("changed");
        let track = folder.0.join("Money.flac");
        fs::write(&track, [1_u8; 16]).expect("a track");
        let changes = [Change {
            start: 0,
            before: vec![1; 4],
            after: &[2; 4],
        }];
        let journal = left_by_a_dead_writer(&track, 16, &changes);
        fs::write(&track, [7_u8; 16]).expect("another program's edit");

        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::ChangedSince
        );
        assert_eq!(fs::read(&track).expect("the track"), [7_u8; 16]);
    }

    #[test]
    fn a_journal_cut_short_itself_is_thrown_away_and_the_file_left_alone() {
        let folder = Folder::new("half");
        let track = folder.0.join("Us.flac");
        fs::write(&track, [1_u8; 16]).expect("a track");
        let changes = [Change {
            start: 0,
            before: vec![1; 4],
            after: &[2; 4],
        }];
        let journal = left_by_a_dead_writer(&track, 16, &changes);
        let whole = fs::read(&journal).expect("the journal");
        fs::write(&journal, &whole[..whole.len() - 3]).expect("a journal cut short");

        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::Unreadable
        );
        assert!(!journal.exists());
        assert_eq!(fs::read(&track).expect("the track"), [1_u8; 16]);
    }

    #[test]
    fn only_a_dead_writers_journal_beside_the_track_is_mended() {
        let folder = Folder::new("beside");
        let track = folder.0.join("Brain Damage.flac");
        fs::write(&track, [1_u8; 8]).expect("a track");
        let ours = folder.0.join(format!(
            ".Brain Damage.flac.{}-0{JOURNAL_SUFFIX}",
            process::id()
        ));
        fs::write(&ours, b"ours").expect("a journal of ours");
        let another = folder
            .0
            .join(format!(".Eclipse.flac.{NEVER_A_PROCESS}-0{JOURNAL_SUFFIX}"));
        fs::write(&another, b"another").expect("another track's journal");
        let left = left_by_a_dead_writer(
            &track,
            8,
            &[Change {
                start: 0,
                before: vec![1; 2],
                after: &[2; 2],
            }],
        );

        mend_what_a_dead_writer_left(&track);

        assert!(!left.exists(), "a dead writer's journal was left");
        assert!(ours.exists(), "a live writer's journal was mended");
        assert!(another.exists(), "another track's journal was mended");
        assert!(!names_a_cut_short_write(&ours));
        assert!(names_a_cut_short_write(&another));
    }
}
