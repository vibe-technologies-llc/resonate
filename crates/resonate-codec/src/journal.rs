use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    os::unix::{ffi::OsStrExt as _, fs::FileExt as _},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{
    naming,
    writer::{self, Held, Writer},
};

const MAGIC: &[u8; 8] = b"RSUNDO01";
const JOURNAL_SUFFIX: &str = ".resonate-undo";
const WHOLE_SUFFIX: &str = ".resonate-whole";
const TORN_SUFFIX: &str = ".resonate-torn";
const STAGED_ELSEWHERE_SUFFIX: &str = ".resonate-staged";
const KEPT_BY_AND_COUNTED: char = '-';
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
const PAGE_BYTES: usize = 4096;
const PAGES_LAID_AT_ONCE: usize = 256;
const STOOD_MAGIC: &[u8; 8] = b"RSSTOOD1";
const NUMBER_BYTES: u64 = 8;
const STOOD_TRAILER_BYTES: u64 = 5 * NUMBER_BYTES;
const AWAY_MAGIC: &[u8; 8] = b"RSAWAY01";
const AWAY_TRAILER_BYTES: u64 = 3 * NUMBER_BYTES;
const TRACK_PATH_BYTES_AT_MOST: u64 = 4096;

static KEPT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Change<'b> {
    pub(crate) start: u64,
    pub(crate) before: Vec<u8>,
    pub(crate) after: &'b [u8],
}

pub(crate) struct Undo {
    journal: PathBuf,
    held: Held,
}

impl Undo {
    pub(crate) fn kept_beside(
        track: &Path,
        stood: u64,
        lands: u64,
        changes: &[Change<'_>],
    ) -> io::Result<Self> {
        Self::kept(track, stood, lands, 0, changes)
    }

    pub(crate) fn kept_shifting(
        track: &Path,
        stood: u64,
        shifted: u64,
        changes: &[Change<'_>],
    ) -> io::Result<Self> {
        Self::kept(track, stood, stood + shifted, shifted, changes)
    }

    fn kept(
        track: &Path,
        stood: u64,
        lands: u64,
        shifted: u64,
        changes: &[Change<'_>],
    ) -> io::Result<Self> {
        let name = track
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let folder = folder_of(track);
        let journal = folder.join(journal_name(name));

        let written = Held::made(&journal).and_then(|held| {
            let mut file = held.file();
            file.write_all(&encoded(name, stood, lands, shifted, changes))?;
            file.sync_all()?;
            File::open(folder)?.sync_all()?;
            Ok(held)
        });
        match written {
            Ok(held) => Ok(Self { journal, held }),
            Err(error) => {
                let _ = fs::remove_file(&journal);
                Err(error)
            }
        }
    }

    pub(crate) fn roll_back(self, file: &File, stood: u64, changes: &[Change<'_>]) {
        let rolled = restored(file, stood, changes);
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
        drop(self.held);
    }
}

fn restored(file: &File, stood: u64, changes: &[Change<'_>]) -> io::Result<()> {
    file.set_len(stood)?;
    changes
        .iter()
        .try_for_each(|change| file.write_all_at(&change.before, change.start))?;
    file.sync_data()
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
    let stamped = format!(
        ".{}{KEPT_BY_AND_COUNTED}{}{suffix}",
        process::id(),
        KEPT.fetch_add(1, Ordering::Relaxed)
    );
    PathBuf::from(naming::named_within(".", track, stamped))
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
    track: &'n [u8],
    cut: bool,
    writer: u32,
    left: Left,
}

impl KeptBy<'_> {
    fn names(&self, track: &OsStr) -> bool {
        let named = track.as_bytes();
        let kept = self.track;
        named == kept || (self.cut && !kept.is_empty() && named.starts_with(kept))
    }
}

const LONGEST_LETTER_BYTES: usize = 4;

fn was_cut(kept: &OsStr) -> bool {
    kept.len() + LONGEST_LETTER_BYTES > naming::NAME_BYTES_AT_MOST
}

fn stamped_with<'n>(kept: &'n OsStr, suffix: &str) -> Option<(&'n [u8], u32)> {
    let unsuffixed = kept
        .as_bytes()
        .strip_prefix(b".")?
        .strip_suffix(suffix.as_bytes())?;
    let dot = unsuffixed.iter().rposition(|byte| *byte == b'.')?;
    let (track, stamp) = (&unsuffixed[..dot], &unsuffixed[dot + 1..]);
    let (writer, counted) = str::from_utf8(stamp)
        .ok()?
        .split_once(KEPT_BY_AND_COUNTED)?;
    counted.parse::<u64>().ok()?;
    Some((track, writer.parse().ok()?))
}

fn kept_by(kept: &OsStr) -> Option<KeptBy<'_>> {
    Left::ALL.into_iter().find_map(|left| {
        let (track, writer) = stamped_with(kept, left.suffix())?;
        Some(KeptBy {
            track,
            cut: was_cut(kept),
            writer,
            left,
        })
    })
}

pub(crate) fn staged_elsewhere_name(track: &Path) -> io::Result<PathBuf> {
    let name = track
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    Ok(kept_name(name, STAGED_ELSEWHERE_SUFFIX))
}

pub fn names_a_cut_short_write(path: &Path) -> bool {
    path.file_name().and_then(kept_by).is_some_and(|kept| {
        kept.left == Left::TornWholeCopy || writer::writer_of(path, kept.writer) == Writer::Gone
    })
}

pub(crate) struct WholeCopy {
    copy: PathBuf,
    track: PathBuf,
    length: u64,
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
        let length = stood_beside(staged, track)?;

        fs::rename(staged, &copy)?;
        if let Err(error) = File::open(folder).and_then(|folder| folder.sync_all()) {
            let _ = fs::rename(&copy, staged);
            return Err(error);
        }
        Ok(Self {
            copy,
            track: track.to_path_buf(),
            length,
        })
    }

    pub(crate) fn kept_elsewhere(staged: &Path, track: &Path) -> io::Result<Self> {
        let track = fs::canonicalize(track)?;
        let name = track
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let folder = folder_of(staged);
        let copy = folder.join(kept_name(name, WHOLE_SUFFIX));
        let length = stood_beside(staged, &track)?;
        bound_beside(staged, &track)?;

        fs::rename(staged, &copy)?;
        if let Err(error) = File::open(folder).and_then(|folder| folder.sync_all()) {
            let _ = fs::rename(&copy, staged);
            return Err(error);
        }
        Ok(Self {
            copy,
            track,
            length,
        })
    }

    pub(crate) fn written_back(self) -> Result<(), Unsettled> {
        let laid = File::open(&self.copy)
            .and_then(|copy| {
                Ok((
                    copy,
                    OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&self.track)?,
                ))
            })
            .map_err(Laying::Unstarted)
            .and_then(|(copy, track)| laid_over(&copy, self.length, &track));
        match laid {
            Ok(()) => {}
            Err(Laying::Unstarted(source)) => {
                let _ = fs::remove_file(&self.copy);
                return Err(Unsettled::Untouched(source));
            }
            Err(Laying::Partway(source)) => {
                warn_torn(&source, &self.track, &self.copy);
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
        let torn = folder_of(&self.copy).join(kept_name(name, TORN_SUFFIX));
        let whole = match fs::rename(&self.copy, &torn) {
            Ok(()) => torn,
            Err(_) => self.copy,
        };
        Torn { whole, source }
    }
}

fn warn_torn(source: &io::Error, track: &Path, whole: &Path) {
    tracing::warn!(%source, track = %track.display(), whole = %whole.display(), "a track failed part way through being written back, so its whole copy is kept for the next write or scan to finish");
}

enum Laying {
    Unstarted(io::Error),
    Partway(io::Error),
}

impl Laying {
    fn into_error(self) -> io::Error {
        match self {
            Self::Unstarted(error) | Self::Partway(error) => error,
        }
    }
}

fn laid_over(copy: &File, length: u64, track: &File) -> Result<(), Laying> {
    let mut chunk = vec![0_u8; PAGE_BYTES * PAGES_LAID_AT_ONCE];
    let mut standing = vec![0_u8; chunk.len()];
    let mut at = 0;
    while at < length {
        let bytes = usize::try_from(length - at).map_or(chunk.len(), |left| left.min(chunk.len()));
        copy.read_exact_at(&mut chunk[..bytes], at)
            .map_err(|error| match at {
                0 => Laying::Unstarted(error),
                _ => Laying::Partway(error),
            })?;
        let held = read_as_far_as_it_goes(track, &mut standing[..bytes], at).unwrap_or(0);
        differing_runs(&chunk[..bytes], &standing[..held]).try_for_each(|run| {
            track
                .write_all_at(&chunk[run.clone()], at + run.start as u64)
                .map_err(Laying::Partway)
        })?;
        at += bytes as u64;
    }
    track.set_len(length).map_err(Laying::Partway)?;
    track.sync_all().map_err(Laying::Partway)
}

fn read_as_far_as_it_goes(file: &File, into: &mut [u8], at: u64) -> io::Result<usize> {
    let mut read = 0;
    while read < into.len() {
        match file.read_at(&mut into[read..], at + read as u64)? {
            0 => break,
            more => read += more,
        }
    }
    Ok(read)
}

fn differing_runs<'b>(
    wanted: &'b [u8],
    standing: &'b [u8],
) -> impl Iterator<Item = std::ops::Range<usize>> + 'b {
    let mut page = 0;
    std::iter::from_fn(move || {
        let pages = wanted.len().div_ceil(PAGE_BYTES);
        let differs = |page: usize| {
            let from = page * PAGE_BYTES;
            let to = (from + PAGE_BYTES).min(wanted.len());
            standing.get(from..to) != Some(&wanted[from..to])
        };
        while page < pages && !differs(page) {
            page += 1;
        }
        if page >= pages {
            return None;
        }
        let first = page;
        while page < pages && differs(page) {
            page += 1;
        }
        Some(first * PAGE_BYTES..(page * PAGE_BYTES).min(wanted.len()))
    })
}

fn page_bytes(length: u64, at: u64) -> usize {
    usize::try_from(length.saturating_sub(at)).map_or(PAGE_BYTES, |left| left.min(PAGE_BYTES))
}

struct Stood {
    length: u64,
    pages: Vec<u64>,
}

enum Record {
    Absent,
    Unreadable,
    Kept { copy_length: u64, stood: Stood },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    Unfinished,
    Finished,
    ChangedSince,
}

enum Bound {
    Beside,
    Away(PathBuf),
    Unreadable,
}

fn bound_beside(staged: &Path, track: &Path) -> io::Result<()> {
    let named = track.as_os_str().as_bytes();
    let mut record = named.to_vec();
    record.extend_from_slice(&(named.len() as u64).to_le_bytes());
    record.extend_from_slice(&hashed(named).to_le_bytes());
    record.extend_from_slice(AWAY_MAGIC);

    let mut copy = OpenOptions::new().append(true).open(staged)?;
    copy.write_all(&record)?;
    copy.sync_all()
}

fn bound_of(copy: &File) -> io::Result<(u64, Bound)> {
    let length = copy.metadata()?.len();
    let Some(trailer_at) = length.checked_sub(AWAY_TRAILER_BYTES) else {
        return Ok((length, Bound::Beside));
    };
    let mut trailer = [0_u8; AWAY_TRAILER_BYTES as usize];
    copy.read_exact_at(&mut trailer, trailer_at)?;
    let (numbers, magic) = trailer.split_at(trailer.len() - AWAY_MAGIC.len());
    if magic != AWAY_MAGIC {
        return Ok((length, Bound::Beside));
    }

    let (named, check) = numbers.split_at(NUMBER_BYTES as usize);
    let named = u64::from_le_bytes(named.try_into().unwrap_or_default());
    let check = u64::from_le_bytes(check.try_into().unwrap_or_default());
    let held = trailer_at.checked_sub(named);
    let (Some(held), true) = (held, named <= TRACK_PATH_BYTES_AT_MOST) else {
        return Ok((length, Bound::Unreadable));
    };
    let mut track = vec![0_u8; usize::try_from(named).unwrap_or_default()];
    copy.read_exact_at(&mut track, held)?;
    if hashed(&track) != check {
        return Ok((held, Bound::Unreadable));
    }
    Ok((held, Bound::Away(PathBuf::from(OsStr::from_bytes(&track)))))
}

fn stood_beside(staged: &Path, track: &Path) -> io::Result<u64> {
    let stood = Stood::of(&File::open(track)?)?;
    let mut copy = OpenOptions::new().append(true).open(staged)?;
    let copy_length = copy.metadata()?.len();
    copy.write_all(&stood.encoded(copy_length))?;
    copy.sync_all()?;
    Ok(copy_length)
}

impl Stood {
    fn of(track: &File) -> io::Result<Self> {
        let length = track.metadata()?.len();
        let mut chunk = vec![0_u8; PAGE_BYTES * PAGES_LAID_AT_ONCE];
        let mut pages = Vec::with_capacity(
            usize::try_from(length.div_ceil(PAGE_BYTES as u64)).unwrap_or_default(),
        );
        let mut at = 0;
        while at < length {
            let bytes =
                usize::try_from(length - at).map_or(chunk.len(), |left| left.min(chunk.len()));
            track.read_exact_at(&mut chunk[..bytes], at)?;
            pages.extend(chunk[..bytes].chunks(PAGE_BYTES).map(hashed));
            at += bytes as u64;
        }
        Ok(Self { length, pages })
    }

    fn encoded(&self, copy_length: u64) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            self.pages.len() * NUMBER_BYTES as usize + STOOD_TRAILER_BYTES as usize,
        );
        for page in &self.pages {
            bytes.extend_from_slice(&page.to_le_bytes());
        }
        bytes.extend_from_slice(&self.length.to_le_bytes());
        bytes.extend_from_slice(&copy_length.to_le_bytes());
        bytes.extend_from_slice(&(self.pages.len() as u64).to_le_bytes());
        let check = hashed(&bytes);
        bytes.extend_from_slice(&check.to_le_bytes());
        bytes.extend_from_slice(STOOD_MAGIC);
        bytes
    }

    fn standing(&self, track: &File, copy: &File, copy_length: u64) -> io::Result<Standing> {
        let length = track.metadata()?.len();
        if length < self.length.min(copy_length) || length > self.length.max(copy_length) {
            return Ok(Standing::ChangedSince);
        }

        let mut held = [0_u8; PAGE_BYTES];
        let mut ours = [0_u8; PAGE_BYTES];
        let mut finished = length == copy_length;
        for (index, at) in (0..length).step_by(PAGE_BYTES).enumerate() {
            let bytes = page_bytes(length, at);
            let written = page_bytes(copy_length, at).min(bytes);
            track.read_exact_at(&mut held[..bytes], at)?;
            copy.read_exact_at(&mut ours[..written], at)?;

            let is_ours = written > 0 && held[..written] == ours[..written];
            let is_as_it_stood = self.pages.get(index).is_some_and(|page| {
                page_bytes(self.length, at) == bytes && hashed(&held[..bytes]) == *page
            });
            if !is_ours && !is_as_it_stood {
                return Ok(Standing::ChangedSince);
            }
            finished &= is_ours && written == bytes;
        }
        Ok(match finished {
            true => Standing::Finished,
            false => Standing::Unfinished,
        })
    }
}

impl Record {
    fn read(copy: &File, length: u64) -> io::Result<Self> {
        let Some(trailer_at) = length.checked_sub(STOOD_TRAILER_BYTES) else {
            return Ok(Self::Absent);
        };
        let mut trailer = [0_u8; STOOD_TRAILER_BYTES as usize];
        copy.read_exact_at(&mut trailer, trailer_at)?;
        let (numbers, magic) = trailer.split_at(trailer.len() - STOOD_MAGIC.len());
        if magic != STOOD_MAGIC {
            return Ok(Self::Absent);
        }

        let number = |nth: usize| {
            let at = nth * NUMBER_BYTES as usize;
            u64::from_le_bytes(
                numbers[at..at + NUMBER_BYTES as usize]
                    .try_into()
                    .unwrap_or_default(),
            )
        };
        let (stood_length, copy_length, count, check) =
            (number(0), number(1), number(2), number(3));
        let recorded = count
            .checked_mul(NUMBER_BYTES)
            .and_then(|pages| pages.checked_add(copy_length))
            .and_then(|held| held.checked_add(STOOD_TRAILER_BYTES));
        if recorded != Some(length) || count != stood_length.div_ceil(PAGE_BYTES as u64) {
            return Ok(Self::Unreadable);
        }

        let Ok(record_bytes) = usize::try_from(length - copy_length - 2 * NUMBER_BYTES) else {
            return Ok(Self::Unreadable);
        };
        let mut record = vec![0_u8; record_bytes];
        copy.read_exact_at(&mut record, copy_length)?;
        if hashed(&record) != check {
            return Ok(Self::Unreadable);
        }
        let (hashes, _) = record.as_chunks::<{ NUMBER_BYTES as usize }>();
        let pages = hashes
            .iter()
            .take(usize::try_from(count).unwrap_or_default())
            .map(|page| u64::from_le_bytes(*page))
            .collect();
        Ok(Self::Kept {
            copy_length,
            stood: Stood {
                length: stood_length,
                pages,
            },
        })
    }
}

pub(crate) fn hashed(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn encoded(track: &OsStr, stood: u64, lands: u64, shifted: u64, changes: &[Change<'_>]) -> Vec<u8> {
    let name = track.as_bytes();
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&stood.to_le_bytes());
    bytes.extend_from_slice(&lands.to_le_bytes());
    bytes.extend_from_slice(&shifted.to_le_bytes());
    bytes.extend_from_slice(&(changes.len() as u64).to_le_bytes());
    for change in changes {
        bytes.extend_from_slice(&change.start.to_le_bytes());
        bytes.extend_from_slice(&(change.before.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&(change.after.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&change.before);
        bytes.extend_from_slice(change.after);
    }
    let check = hashed(&bytes);
    bytes.extend_from_slice(&check.to_le_bytes());
    bytes
}

pub(crate) struct Kept {
    track: PathBuf,
    stood: u64,
    lands: u64,
    shifted: u64,
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

pub(crate) fn decoded(folder: &Path, bytes: &[u8]) -> Option<Kept> {
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
    let lands = reader.number()?;
    let shifted = reader.number()?;
    let count = reader.counted()?;

    let mut changes = Vec::new();
    for _ in 0..count {
        let start = reader.number()?;
        let before_length = reader.counted()?;
        let after_length = reader.counted()?;
        let before = reader.taken(before_length)?.to_vec();
        let after = reader.taken(after_length)?.to_vec();
        changes.push((start, before, after));
    }
    reader.bytes.is_empty().then(|| Kept {
        track: folder.join(name),
        stood,
        lands,
        shifted,
        changes,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mended {
    WrittenBack,
    Unplaced,
    RolledBack,
    CarriedOn,
    Finished,
    Untouched,
    ChangedSince,
    Unreadable,
}

pub fn mend_a_cut_short_write(journal: &Path) -> io::Result<Mended> {
    let kept = journal.file_name().and_then(kept_by);
    if let Some(kept) = kept.filter(|kept| kept.left != Left::Undo) {
        return match track_named_by(folder_of(journal), &kept)? {
            Some(track) => finished_writing_back(journal, &track),
            None => {
                tracing::warn!(path = %journal.display(), "the whole copy a tag write left names no one track, so it is left where it is");
                Ok(Mended::Unplaced)
            }
        };
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

fn track_named_by(folder: &Path, kept: &KeptBy<'_>) -> io::Result<Option<PathBuf>> {
    if !kept.cut {
        return Ok(Some(folder.join(OsStr::from_bytes(kept.track))));
    }
    let mut named = fs::read_dir(folder)?
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| !name.as_bytes().starts_with(b".") && kept.names(name));
    Ok(match (named.next(), named.next()) {
        (Some(only), None) => Some(folder.join(only)),
        _ => None,
    })
}

fn finished_writing_back(whole: &Path, track: &Path) -> io::Result<Mended> {
    let copy = File::open(whole)?;
    let (length, _) = bound_of(&copy)?;
    let mended = match Record::read(&copy, length)? {
        Record::Unreadable => Mended::Unreadable,
        Record::Absent => written_back_over(&copy, length, track, None)?,
        Record::Kept { copy_length, stood } => {
            written_back_over(&copy, copy_length, track, Some(&stood))?
        }
    };
    fs::remove_file(whole)?;
    tracing::info!(path = %whole.display(), ?mended, "finished writing back a track a tag write left part rewritten");
    Ok(mended)
}

fn written_back_over(
    copy: &File,
    length: u64,
    track: &Path,
    stood: Option<&Stood>,
) -> io::Result<Mended> {
    let file = match OpenOptions::new().read(true).write(true).open(track) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Mended::ChangedSince),
        Err(error) => return Err(error),
    };
    let standing = stood
        .map(|stood| stood.standing(&file, copy, length))
        .transpose()?;
    match standing {
        Some(Standing::ChangedSince) => Ok(Mended::ChangedSince),
        Some(Standing::Finished) => Ok(Mended::Finished),
        Some(Standing::Unfinished) | None => {
            laid_over(copy, length, &file).map_err(Laying::into_error)?;
            Ok(Mended::WrittenBack)
        }
    }
}

fn rolled_back(kept: &Kept) -> io::Result<Mended> {
    let file = match OpenOptions::new().read(true).write(true).open(&kept.track) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Mended::ChangedSince),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    if length < kept.stood.min(kept.lands) || length > kept.stood.max(kept.lands) {
        return Ok(Mended::ChangedSince);
    }

    if kept.shifted > 0 {
        return carried_on(&file, kept, length);
    }

    let mut all_before = length == kept.stood;
    let mut all_after = length == kept.lands;
    for (start, before, after) in &kept.changes {
        let reaching = before.len().max(after.len());
        let present = usize::try_from(length.saturating_sub(*start))
            .map_or(reaching, |held| held.min(reaching));
        let mut standing = vec![0; present];
        file.read_exact_at(&mut standing, *start)?;
        let ours = standing
            .iter()
            .enumerate()
            .all(|(at, byte)| before.get(at) == Some(byte) || after.get(at) == Some(byte));
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

    file.set_len(kept.stood)?;
    for (start, before, _) in &kept.changes {
        file.write_all_at(before, *start)?;
    }
    file.sync_data()?;
    Ok(Mended::RolledBack)
}

fn carried_on(file: &File, kept: &Kept, length: u64) -> io::Result<Mended> {
    if length != kept.stood && length != kept.lands {
        return Ok(Mended::ChangedSince);
    }
    let opened = length == kept.lands;

    let mut all_before = true;
    let mut all_after = true;
    for (start, before, after) in &kept.changes {
        let mut opened_before = if opened {
            vec![0; usize::try_from(kept.shifted).unwrap_or_default()]
        } else {
            Vec::new()
        };
        opened_before.extend_from_slice(before);

        let reaching = opened_before.len().max(after.len());
        let present = usize::try_from(length.saturating_sub(*start))
            .map_or(reaching, |held| held.min(reaching));
        let mut standing = vec![0; present];
        file.read_exact_at(&mut standing, *start)?;
        let ours = standing
            .iter()
            .enumerate()
            .all(|(at, byte)| opened_before.get(at) == Some(byte) || after.get(at) == Some(byte));
        if !ours {
            return Ok(Mended::ChangedSince);
        }
        all_before &= standing == opened_before;
        all_after &= standing == *after;
    }

    if !opened {
        return Ok(match all_before {
            true => Mended::Untouched,
            false => Mended::ChangedSince,
        });
    }
    if all_after {
        return Ok(Mended::Finished);
    }
    for (start, _, after) in &kept.changes {
        file.write_all_at(after, *start)?;
    }
    file.sync_data()?;
    Ok(Mended::CarriedOn)
}

fn finished_from_elsewhere(whole: &Path, kept: &KeptBy<'_>) -> io::Result<Mended> {
    let copy = File::open(whole)?;
    let mended = match bound_of(&copy)? {
        (_, Bound::Beside) => return Ok(Mended::Unplaced),
        (length, Bound::Away(track)) if track.file_name().is_some_and(|name| kept.names(name)) => {
            match Record::read(&copy, length)? {
                Record::Kept { copy_length, stood } => {
                    written_back_over(&copy, copy_length, &track, Some(&stood))?
                }
                Record::Absent | Record::Unreadable => Mended::Unreadable,
            }
        }
        (_, Bound::Away(_) | Bound::Unreadable) => Mended::Unreadable,
    };
    fs::remove_file(whole)?;
    tracing::info!(path = %whole.display(), ?mended, "finished writing back a track whose whole copy a tag write kept outside its folder");
    Ok(mended)
}

pub fn mend_what_was_written_back_from_elsewhere() {
    mend_what_was_left_in(&crate::spool::spooled_under());
}

pub(crate) fn mend_what_was_left_in(folders: &[PathBuf]) {
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let path = entry.path();
            if let Some((_, writer)) = stamped_with(&name, STAGED_ELSEWHERE_SUFFIX) {
                swept_unless_living(&path, writer);
                continue;
            }
            let Some(kept) = kept_by(&name).filter(|kept| kept.left != Left::Undo) else {
                continue;
            };
            if !names_a_cut_short_write(&path) {
                continue;
            }
            if let Err(error) = finished_from_elsewhere(&path, &kept) {
                tracing::warn!(%error, path = %path.display(), "a tag write cut short could not be mended from its copy outside the track's folder");
            }
        }
    }
}

fn swept_unless_living(staged: &Path, writer: u32) {
    if writer::writer_of(staged, writer) == Writer::Living {
        return;
    }
    match fs::remove_file(staged) {
        Ok(()) => {
            tracing::info!(path = %staged.display(), "swept a copy a tag write cut short staged outside the track's folder")
        }
        Err(error) => {
            tracing::debug!(%error, path = %staged.display(), "a copy a tag write cut short staged outside the track's folder could not be swept")
        }
    }
}

pub(crate) fn mend_what_a_dead_writer_left(track: &Path) {
    let Some(name) = track.file_name() else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder_of(track)) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let entry_name = entry.file_name();
        let Some(kept) = kept_by(&entry_name).filter(|kept| kept.names(name)) else {
            continue;
        };
        if !names_a_cut_short_write(&path) {
            continue;
        }
        let mended = match kept.left {
            Left::Undo => mend_a_cut_short_write(&path),
            Left::WholeCopy | Left::TornWholeCopy => finished_writing_back(&path, track),
        };
        if let Err(error) = mended {
            tracing::warn!(%error, path = %path.display(), "a tag write cut short could not be mended");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, slice};

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
        left_by_a_dead_writer_landing(track, stood, stood, changes)
    }

    fn left_by_a_dead_writer_landing(
        track: &Path,
        stood: u64,
        lands: u64,
        changes: &[Change<'_>],
    ) -> PathBuf {
        let name = track.file_name().expect("a named track");
        let journal = track.with_file_name(format!(
            ".{}.{NEVER_A_PROCESS}-0{JOURNAL_SUFFIX}",
            name.to_string_lossy()
        ));
        fs::write(&journal, encoded(name, stood, lands, 0, changes)).expect("a journal");
        journal
    }

    #[test]
    fn a_whole_copy_beside_a_track_whose_name_is_not_text_is_named_cut_short_and_written_back() {
        let folder = Folder::new("written-back-bytes");
        let name = OsStr::from_bytes(b"Br\xe9the.flac");
        let track = folder.0.join(name);
        let whole = folder.0.join(OsStr::from_bytes(
            &[
                b".Br\xe9the.flac.".as_slice(),
                format!("{NEVER_A_PROCESS}-0{WHOLE_SUFFIX}").as_bytes(),
            ]
            .concat(),
        ));
        fs::write(&track, [9_u8; 40]).expect("a torn track");
        fs::write(&whole, [4_u8; 24]).expect("the whole copy");

        assert!(names_a_cut_short_write(&whole));
        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::WrittenBack
        );
        assert_eq!(fs::read(&track).expect("the track"), [4_u8; 24]);
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

    fn left_written_back_partway(folder: &Folder, was: &[u8], tagged: &[u8]) -> (PathBuf, PathBuf) {
        let track = folder.0.join("Any Colour You Like.flac");
        let staged = folder.0.join(".Any Colour You Like.staged.flac");
        fs::write(&track, was).expect("a track");
        fs::write(&staged, tagged).expect("a tagged copy");

        let whole = WholeCopy::named_beside(&staged, &track).expect("a whole copy");
        (track, whole.copy)
    }

    fn paged(pages: usize, tail: usize, byte: u8) -> Vec<u8> {
        (0..pages * PAGE_BYTES + tail)
            .map(|at| byte.wrapping_add((at / PAGE_BYTES) as u8))
            .collect()
    }

    #[test]
    fn a_track_a_dead_writer_tore_partway_through_writing_back_is_finished() {
        let folder = Folder::new("torn-pages");
        let was = paged(3, 100, 10);
        let tagged = paged(2, 50, 90);
        let (track, whole) = left_written_back_partway(&folder, &was, &tagged);

        let mut torn = was.clone();
        torn[..PAGE_BYTES].copy_from_slice(&tagged[..PAGE_BYTES]);
        fs::write(&track, &torn).expect("a write back cut short");

        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::WrittenBack
        );
        assert_eq!(fs::read(&track).expect("the track"), tagged);
        assert!(!whole.exists(), "the whole copy was left");
    }

    #[test]
    fn a_track_a_dead_writer_wrote_back_whole_is_left_as_it_stands() {
        let folder = Folder::new("laid-whole");
        let was = paged(2, 7, 30);
        let tagged = paged(3, 0, 60);
        let (track, whole) = left_written_back_partway(&folder, &was, &tagged);

        fs::write(&track, &tagged).expect("a write back that landed");

        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::Finished
        );
        assert_eq!(fs::read(&track).expect("the track"), tagged);
        assert!(!whole.exists(), "the whole copy was left");
    }

    #[test]
    fn a_track_another_program_changed_after_a_dead_writer_is_not_written_over() {
        let folder = Folder::new("rewritten-since");
        let was = paged(3, 0, 1);
        let tagged = paged(3, 0, 120);
        let (track, whole) = left_written_back_partway(&folder, &was, &tagged);

        let mut edited = was.clone();
        edited[..PAGE_BYTES].copy_from_slice(&tagged[..PAGE_BYTES]);
        edited[2 * PAGE_BYTES + 9] ^= 0xff;
        fs::write(&track, &edited).expect("another program's edit");

        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::ChangedSince
        );
        assert_eq!(fs::read(&track).expect("the track"), edited);
        assert!(!whole.exists(), "the whole copy was left");
    }

    #[test]
    fn a_whole_copy_whose_record_was_damaged_writes_nothing_over_the_track() {
        let folder = Folder::new("damaged-record");
        let was = paged(1, 0, 5);
        let tagged = paged(1, 0, 6);
        let (track, whole) = left_written_back_partway(&folder, &was, &tagged);

        let mut damaged = fs::read(&whole).expect("the whole copy");
        damaged[PAGE_BYTES] ^= 0xff;
        fs::write(&whole, damaged).expect("a damaged record");

        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::Unreadable
        );
        assert_eq!(fs::read(&track).expect("the track"), was);
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
            length: 8,
        }
        .torn(io::Error::from(io::ErrorKind::StorageFull));
        assert!(names_a_cut_short_write(&torn.whole));
        mend_what_a_dead_writer_left(&track);
        assert_eq!(fs::read(&track).expect("the track"), [2_u8; 8]);
        assert!(!torn.whole.exists());
    }

    #[test]
    fn a_whole_copy_of_a_track_named_at_the_limit_is_cut_to_fit_and_still_finds_its_track() {
        let folder = Folder::new("longest");
        let name = format!("{}.flac", "ä".repeat(125));
        let track = folder.0.join(&name);
        fs::write(&track, [1_u8; 8]).expect("a torn track");

        let stamped = format!(".{NEVER_A_PROCESS}{KEPT_BY_AND_COUNTED}0{WHOLE_SUFFIX}");
        let whole = folder
            .0
            .join(naming::named_within(".", OsStr::new(&name), &stamped));
        assert!(whole.file_name().expect("a name").len() <= naming::NAME_BYTES_AT_MOST);
        fs::write(&whole, [3_u8; 8]).expect("the whole copy");

        assert_eq!(
            mend_a_cut_short_write(&whole).expect("a mend"),
            Mended::WrittenBack
        );
        assert_eq!(fs::read(&track).expect("the track"), [3_u8; 8]);
    }

    fn kept_elsewhere_by_a_dead_writer(
        folder: &Folder,
        was: &[u8],
        tagged: &[u8],
    ) -> (PathBuf, PathBuf, PathBuf) {
        let spool = folder.0.join("spool");
        let track = folder.0.join("Eclipse.flac");
        fs::create_dir_all(&spool).expect("a spool folder");
        fs::write(&track, was).expect("a track");
        let staged = spool.join(staged_elsewhere_name(&track).expect("a staged name"));
        fs::write(&staged, tagged).expect("a tagged copy");

        let kept = WholeCopy::kept_elsewhere(&staged, &track).expect("a whole copy");
        let dead = spool.join(format!(
            ".Eclipse.flac.{NEVER_A_PROCESS}{KEPT_BY_AND_COUNTED}0{WHOLE_SUFFIX}"
        ));
        fs::rename(&kept.copy, &dead).expect("a copy a dead writer left");
        (spool, track, dead)
    }

    #[test]
    fn a_track_torn_by_a_dead_writer_is_finished_from_the_whole_copy_kept_outside_its_folder() {
        let folder = Folder::new("torn-elsewhere");
        let was = paged(3, 100, 10);
        let tagged = paged(2, 50, 90);
        let (spool, track, whole) = kept_elsewhere_by_a_dead_writer(&folder, &was, &tagged);

        let mut torn = was.clone();
        torn[..PAGE_BYTES].copy_from_slice(&tagged[..PAGE_BYTES]);
        fs::write(&track, &torn).expect("a write back cut short");
        mend_what_was_left_in(slice::from_ref(&spool));

        assert_eq!(fs::read(&track).expect("the track"), tagged);
        assert!(!whole.exists(), "the whole copy was left");
    }

    #[test]
    fn a_track_changed_since_a_dead_writer_kept_it_elsewhere_is_not_written_over() {
        let folder = Folder::new("changed-elsewhere");
        let was = paged(2, 0, 1);
        let tagged = paged(2, 0, 120);
        let (spool, track, whole) = kept_elsewhere_by_a_dead_writer(&folder, &was, &tagged);

        let mut edited = was.clone();
        edited[PAGE_BYTES + 3] ^= 0xff;
        fs::write(&track, &edited).expect("another program's edit");
        mend_what_was_left_in(slice::from_ref(&spool));

        assert_eq!(fs::read(&track).expect("the track"), edited);
        assert!(!whole.exists(), "the whole copy was left");
    }

    #[test]
    fn a_copy_kept_elsewhere_naming_another_track_than_its_name_writes_nothing() {
        let folder = Folder::new("misnamed-elsewhere");
        let was = paged(1, 0, 5);
        let (spool, track, whole) = kept_elsewhere_by_a_dead_writer(&folder, &was, &paged(1, 0, 6));
        let misnamed = spool.join(format!(
            ".Breathe.flac.{NEVER_A_PROCESS}{KEPT_BY_AND_COUNTED}0{WHOLE_SUFFIX}"
        ));
        fs::rename(&whole, &misnamed).expect("a copy under another name");

        mend_what_was_left_in(slice::from_ref(&spool));

        assert_eq!(fs::read(&track).expect("the track"), was);
        assert!(!misnamed.exists(), "a copy that places nothing was left");
    }

    #[test]
    fn only_a_dead_writers_copy_staged_outside_the_tracks_folder_is_swept() {
        let folder = Folder::new("staged-elsewhere");
        let dead = folder.0.join(format!(
            ".Time.flac.{NEVER_A_PROCESS}{KEPT_BY_AND_COUNTED}0{STAGED_ELSEWHERE_SUFFIX}"
        ));
        let living = folder
            .0
            .join(staged_elsewhere_name(Path::new("Time.flac")).expect("a name"));
        let beside = folder.0.join(format!(
            ".Time.flac.{NEVER_A_PROCESS}{KEPT_BY_AND_COUNTED}1{WHOLE_SUFFIX}"
        ));
        let foreign = folder.0.join(".Time.flac.12-0.staged");
        for left in [&dead, &living, &beside, &foreign] {
            fs::write(left, b"left").expect("a file left in the spool");
        }

        mend_what_was_left_in(slice::from_ref(&folder.0));

        assert!(!dead.exists(), "a dead writer's staged copy was left");
        assert!(living.exists(), "a living writer's staged copy was swept");
        assert!(beside.exists(), "a copy naming no track was taken away");
        assert!(foreign.exists(), "a file this build never names was swept");
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
    fn a_write_that_grew_the_file_and_was_cut_short_is_rolled_back_to_its_length() {
        let folder = Folder::new("grown");
        let track = folder.0.join("Wish You Were Here.wav");
        let was = vec![1_u8; 20];
        let changes = [Change {
            start: 16,
            before: was[16..].to_vec(),
            after: &[2; 12],
        }];
        let journal = left_by_a_dead_writer_landing(&track, 20, 28, &changes);

        let mut torn = was.clone();
        torn[16..].fill(2);
        torn.extend_from_slice(&[2; 4]);
        fs::write(&track, &torn).expect("a grown file");
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::RolledBack
        );
        assert_eq!(fs::read(&track).expect("the track"), was);

        let journal = left_by_a_dead_writer_landing(&track, 20, 28, &changes);
        let mut finished = was.clone();
        finished[16..].fill(2);
        finished.extend_from_slice(&[2; 8]);
        fs::write(&track, &finished).expect("a finished write");
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::Finished
        );
        assert_eq!(fs::read(&track).expect("the track"), finished);
    }

    #[test]
    fn a_write_that_shrank_the_file_and_was_cut_short_has_its_tail_put_back() {
        let folder = Folder::new("shrunk");
        let track = folder.0.join("Welcome to the Machine.wav");
        let was: Vec<u8> = (0..32).collect();
        let changes = [
            Change {
                start: 16,
                before: was[16..24].to_vec(),
                after: &[9; 4],
            },
            Change {
                start: 24,
                before: was[24..].to_vec(),
                after: &[],
            },
        ];
        let journal = left_by_a_dead_writer_landing(&track, 32, 20, &changes);

        let mut torn = was.clone();
        torn[16..20].fill(9);
        fs::write(&track, &torn).expect("a file cut before its tail was");
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::RolledBack
        );
        assert_eq!(fs::read(&track).expect("the track"), was);

        let journal = left_by_a_dead_writer_landing(&track, 32, 20, &changes);
        let mut finished = was[..20].to_vec();
        finished[16..].fill(9);
        fs::write(&track, &finished).expect("a finished write");
        assert_eq!(
            mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::Finished
        );
    }

    #[test]
    fn a_copy_laid_over_a_track_writes_only_the_pages_that_differ() {
        let wanted = paged(6, 100, 10);
        let mut standing = wanted.clone();
        standing[PAGE_BYTES * 2 + 5] ^= 1;
        standing[PAGE_BYTES * 3] ^= 1;
        standing[PAGE_BYTES * 6 + 1] ^= 1;

        let runs: Vec<_> = differing_runs(&wanted, &standing).collect();
        assert_eq!(
            runs,
            [PAGE_BYTES * 2..PAGE_BYTES * 4, PAGE_BYTES * 6..wanted.len()]
        );

        let short = &standing[..PAGE_BYTES + 3];
        let runs: Vec<_> = differing_runs(&wanted, short).collect();
        assert_eq!(
            runs,
            [(PAGE_BYTES, wanted.len())]
                .map(|(from, to)| from..to)
                .to_vec()
        );
        assert_eq!(differing_runs(&wanted, &wanted).count(), 0);
    }

    #[test]
    fn a_copy_laid_over_a_longer_track_cuts_it_and_leaves_the_pages_alike_alone() {
        let folder = Folder::new("laid");
        let was = paged(5, 10, 3);
        let wanted = [&was[..PAGE_BYTES * 3], &[9; 100][..]].concat();
        let track = folder.0.join("Brain Damage.wav");
        let copy = folder.0.join("copy");
        fs::write(&track, &was).expect("a track");
        fs::write(&copy, &wanted).expect("a copy");

        let laid = laid_over(
            &File::open(&copy).expect("the copy"),
            wanted.len() as u64,
            &OpenOptions::new()
                .read(true)
                .write(true)
                .open(&track)
                .expect("the track"),
        );

        assert!(laid.is_ok());
        assert_eq!(fs::read(&track).expect("the track"), wanted);
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
