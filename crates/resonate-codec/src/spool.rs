use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::FileExt as _,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use parking_lot::{Condvar, Mutex};

use crate::source::{FormatHint, Media, MediaStream};

pub(crate) const SPOOLED_ON_DISC_AT_MOST: u64 = 8 << 30;
const COPIED_AT_A_TIME: usize = 64 << 10;
const CHOSEN_TEMPORARY_FOLDER: &str = "TMPDIR";
const KEPT_ON_DISC: &str = "/var/tmp";

static NAMED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    Whole,
    Cut,
}

struct Filled {
    written: u64,
    ended: Option<Ending>,
}

pub(crate) struct Spool {
    file: File,
    hint: Option<FormatHint>,
    filled: Mutex<Filled>,
    grown: Condvar,
}

impl Spool {
    pub(crate) fn beginning_with(
        head: &[u8],
        rest: Box<dyn MediaStream>,
        hint: Option<FormatHint>,
    ) -> Result<Arc<Self>, (io::Error, Box<dyn MediaStream>)> {
        Self::beginning_with_at_most(head, rest, hint, SPOOLED_ON_DISC_AT_MOST)
    }

    fn beginning_with_at_most(
        head: &[u8],
        rest: Box<dyn MediaStream>,
        hint: Option<FormatHint>,
        at_most: u64,
    ) -> Result<Arc<Self>, (io::Error, Box<dyn MediaStream>)> {
        let file = match unnamed_file().and_then(|file| file.write_all_at(head, 0).map(|()| file)) {
            Ok(file) => file,
            Err(source) => return Err((source, rest)),
        };
        let spool = Arc::new(Self {
            file,
            hint,
            filled: Mutex::new(Filled {
                written: head.len() as u64,
                ended: None,
            }),
            grown: Condvar::new(),
        });
        let (handing, handed) = mpsc::sync_channel::<Box<dyn MediaStream>>(1);
        let copying = Arc::clone(&spool);
        let started = thread::Builder::new()
            .name("resonate-spool".to_owned())
            .spawn(move || {
                if let Ok(rest) = handed.recv() {
                    copying.copy_from(rest, at_most);
                }
            });
        match started {
            Ok(_) => {
                let _ = handing.send(rest);
                Ok(spool)
            }
            Err(source) => Err((source, rest)),
        }
    }

    fn copy_from(self: Arc<Self>, mut rest: Box<dyn MediaStream>, at_most: u64) {
        let mut reading = vec![0_u8; COPIED_AT_A_TIME];
        let ending = loop {
            if Arc::strong_count(&self) == 1 {
                break Ending::Cut;
            }
            let written = self.filled.lock().written;
            if written >= at_most {
                tracing::warn!(
                    bytes = written,
                    "a source that cannot seek ran past what is spooled of one; it plays to there"
                );
                break Ending::Cut;
            }
            let room = usize::try_from(at_most - written)
                .unwrap_or(usize::MAX)
                .min(reading.len());
            match rest.read(&mut reading[..room]) {
                Ok(0) => break Ending::Whole,
                Ok(read) => {
                    if let Err(source) = self.file.write_all_at(&reading[..read], written) {
                        tracing::warn!(%source, "a spooled source could not be written down");
                        break Ending::Cut;
                    }
                    self.filled.lock().written = written + read as u64;
                    self.grown.notify_all();
                }
                Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
                Err(source) => {
                    tracing::debug!(%source, "a spooled source stopped before its end");
                    break Ending::Cut;
                }
            }
        };
        self.filled.lock().ended = Some(ending);
        self.grown.notify_all();
    }

    pub(crate) fn is_whole(&self) -> bool {
        self.filled.lock().ended == Some(Ending::Whole)
    }

    pub(crate) fn whole(self: &Arc<Self>) -> Option<Media> {
        let len = {
            let filled = self.filled.lock();
            (filled.ended == Some(Ending::Whole)).then_some(filled.written)?
        };
        Some(Media {
            stream: Box::new(Unspooled {
                spool: Arc::clone(self),
                at: 0,
                len,
            }),
            hint: self.hint.clone(),
        })
    }

    pub(crate) fn streamed(self: &Arc<Self>) -> Spooling {
        Spooling {
            spool: Arc::clone(self),
            at: 0,
        }
    }

    fn read_at(&self, into: &mut [u8], at: u64) -> io::Result<usize> {
        let held = {
            let mut filled = self.filled.lock();
            while filled.written <= at && filled.ended.is_none() {
                self.grown.wait(&mut filled);
            }
            filled.written.saturating_sub(at)
        };
        let taking = usize::try_from(held).unwrap_or(usize::MAX).min(into.len());
        if taking == 0 {
            return Ok(0);
        }
        self.file.read_at(&mut into[..taking], at)
    }
}

fn spooled_under() -> Vec<PathBuf> {
    let chosen = env::var_os(CHOSEN_TEMPORARY_FOLDER).filter(|dir| !dir.is_empty());
    let on_disc = chosen.is_none().then(|| PathBuf::from(KEPT_ON_DISC));
    on_disc.into_iter().chain([env::temp_dir()]).collect()
}

fn unnamed_file() -> io::Result<File> {
    unnamed_file_under(&spooled_under())
}

fn unnamed_file_under(folders: &[PathBuf]) -> io::Result<File> {
    let mut refused = io::Error::from(io::ErrorKind::NotFound);
    for folder in folders {
        match unnamed_file_in(folder) {
            Ok(file) => return Ok(file),
            Err(source) => refused = source,
        }
    }
    Err(refused)
}

fn unnamed_file_in(folder: &Path) -> io::Result<File> {
    let path = folder.join(format!(
        "resonate-spool-{}-{}",
        process::id(),
        NAMED.fetch_add(1, Ordering::Relaxed)
    ));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    fs::remove_file(&path)?;
    Ok(file)
}

pub(crate) struct Spooling {
    spool: Arc<Spool>,
    at: u64,
}

impl Read for Spooling {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let read = self.spool.read_at(into, self.at)?;
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Spooling {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this source cannot seek until it has all arrived",
        ))
    }
}

impl MediaStream for Spooling {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

struct Unspooled {
    spool: Arc<Spool>,
    at: u64,
    len: u64,
}

impl Read for Unspooled {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let left = usize::try_from(self.len.saturating_sub(self.at)).unwrap_or(usize::MAX);
        let taking = left.min(into.len());
        if taking == 0 {
            return Ok(0);
        }
        let read = self.spool.file.read_at(&mut into[..taking], self.at)?;
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Unspooled {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let landed = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        let landed = landed.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.at = landed;
        Ok(landed)
    }
}

impl MediaStream for Unspooled {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, time::Duration};

    use super::*;

    struct Piped(Cursor<Vec<u8>>);

    impl Read for Piped {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            thread::sleep(Duration::from_millis(1));
            let taking = into.len().min(7);
            self.0.read(&mut into[..taking])
        }
    }

    impl Seek for Piped {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }

    #[test]
    fn a_spool_lands_in_the_first_folder_that_takes_it_and_leaves_no_name_behind() {
        let missing = env::temp_dir().join(format!("resonate-no-such-folder-{}", process::id()));
        let taking = env::temp_dir().join(format!("resonate-spool-folder-{}", process::id()));
        fs::create_dir_all(&taking).expect("a folder to spool into");

        let spooled = unnamed_file_under(&[missing.clone(), taking.clone()]);
        let refused = unnamed_file_under(std::slice::from_ref(&missing));
        let left = fs::read_dir(&taking)
            .map(Iterator::count)
            .unwrap_or(usize::MAX);
        let _ = fs::remove_dir(&taking);

        assert!(
            spooled.is_ok(),
            "a folder that takes a file was passed over"
        );
        assert!(refused.is_err(), "a folder that is not there took a file");
        assert_eq!(left, 0, "the spool left its name behind");
    }

    #[test]
    fn a_spool_is_kept_on_disc_unless_a_temporary_folder_was_chosen() {
        let folders = spooled_under();

        if env::var_os(CHOSEN_TEMPORARY_FOLDER).is_some_and(|dir| !dir.is_empty()) {
            assert_eq!(folders, [env::temp_dir()]);
        } else {
            assert_eq!(folders, [PathBuf::from(KEPT_ON_DISC), env::temp_dir()]);
        }
    }

    impl MediaStream for Piped {
        fn is_seekable(&self) -> bool {
            false
        }

        fn byte_len(&self) -> Option<u64> {
            None
        }
    }

    fn bytes() -> Vec<u8> {
        (0..=u8::MAX).cycle().take(5_000).collect()
    }

    #[test]
    fn a_spool_streams_what_arrives_and_becomes_seekable_once_it_has_all_arrived() {
        let all = bytes();
        let (head, rest) = all.split_at(100);
        let spool = Spool::beginning_with(head, Box::new(Piped(Cursor::new(rest.to_vec()))), None)
            .map_err(|(source, _)| source)
            .expect("a writable temporary folder");

        let mut streamed = Vec::new();
        spool
            .streamed()
            .read_to_end(&mut streamed)
            .expect("the spool reads to its end");
        assert_eq!(streamed, all);

        assert!(spool.is_whole());
        let mut whole = spool.whole().expect("a spool that has all arrived").stream;
        assert!(whole.is_seekable());
        assert_eq!(whole.byte_len(), Some(all.len() as u64));
        whole.seek(SeekFrom::End(-10)).expect("a seek from its end");
        let mut tail = Vec::new();
        whole.read_to_end(&mut tail).expect("the tail reads");
        assert_eq!(tail, all[all.len() - 10..]);
    }

    #[test]
    fn a_spool_past_its_ceiling_plays_to_there_and_never_claims_to_be_whole() {
        let all = bytes();
        let spool = Spool::beginning_with_at_most(
            &all[..10],
            Box::new(Piped(Cursor::new(all[10..].to_vec()))),
            None,
            1_000,
        )
        .map_err(|(source, _)| source)
        .expect("a writable temporary folder");

        let mut streamed = Vec::new();
        spool
            .streamed()
            .read_to_end(&mut streamed)
            .expect("the spool reads to where it stopped");
        assert_eq!(streamed, all[..1_000]);
        assert!(!spool.is_whole());
        assert!(spool.whole().is_none());
    }
}
